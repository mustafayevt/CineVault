//! The managed indexer service in the app: downloaded and started by itself while
//! Torrent search and the service are on, never while either is off, stopped when it is switched
//! off or the app closes, checked for working indexers, updated only with a published checksum.
//! The program is stood in for by a launcher that starts nothing and the checking by a script;
//! the downloads are real, from a local server.

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use cv_app::app_settings::SettingsService;
use cv_app::notices::NoticeHub;
use cv_app::provider_credentials::ProviderCredentialsService;
use cv_app::secrets::SecretStore;
use cv_app::tools::ToolsService;
use cv_app::torrent_settings::{INDEXER_ENABLED_KEY, SOLVER_URL_KEY};
use cv_app::torrents::TorrentService;
use cv_app::torrents::indexer::{
    CHECK_JOB, INSTALL_JOB, IndexerError, IndexerParts, IndexerServiceManager, Provisioner,
    ReleaseSource, RowCallback, ServiceState, UPDATE_JOB, Wanted,
};
use cv_app::torrents::indexer_update::UpdateRefused;
use cv_db::{Database, DbOptions};
use cv_jobs::{JobManager, JobState, LaneConfig, Lanes, ManagerConfig};
use cv_media::tools::{Manifest, Platform, ToolsManager};
use cv_providers::{CredentialResolver, MemoryUserCredentials, ProviderRegistry};
use cv_torrent::cancel::CancelToken;
use cv_torrent::indexer_service::provision::{
    IndexerRow, IndexerState, IndexerStore, OtherLanguageIndexer, PasswordStore, ProvisionError,
    ProvisionReport,
};
use cv_torrent::indexer_service::{Launcher, ServiceError, ServiceHandle, StartParams};
use cv_torrent::model::{MediaKind, Scope, TorrentQuery};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

// ---- a small file server ---------------------------------------------------------------------

#[derive(Clone)]
struct File {
    body: Vec<u8>,
    delay: Duration,
}

struct Server {
    base: String,
    files: Arc<Mutex<HashMap<String, File>>>,
    hits: Arc<Mutex<Vec<String>>>,
}

impl Server {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let files: Arc<Mutex<HashMap<String, File>>> = Arc::default();
        let hits: Arc<Mutex<Vec<String>>> = Arc::default();
        let (served, log) = (Arc::clone(&files), Arc::clone(&hits));
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let (served, log) = (Arc::clone(&served), Arc::clone(&log));
                tokio::spawn(async move {
                    let mut buffer = vec![0_u8; 8192];
                    let read = stream.read(&mut buffer).await.unwrap_or(0);
                    let head = String::from_utf8_lossy(&buffer[..read]).into_owned();
                    let path = head
                        .lines()
                        .next()
                        .and_then(|l| l.split_whitespace().nth(1))
                        .unwrap_or("/")
                        .split('?')
                        .next()
                        .unwrap_or("/")
                        .to_owned();
                    log.lock().unwrap().push(path.clone());
                    let file = served.lock().unwrap().get(&path).cloned();
                    let reply = match file {
                        Some(file) => {
                            tokio::time::sleep(file.delay).await;
                            let mut out = format!(
                                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                file.body.len()
                            )
                            .into_bytes();
                            out.extend(file.body);
                            out
                        }
                        None => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            .to_vec(),
                    };
                    let _ = stream.write_all(&reply).await;
                });
            }
        });
        Self { base, files, hits }
    }

    fn put(&self, path: &str, body: Vec<u8>) {
        self.put_slow(path, body, Duration::ZERO);
    }

    fn put_slow(&self, path: &str, body: Vec<u8>, delay: Duration) {
        self.files
            .lock()
            .unwrap()
            .insert(path.to_owned(), File { body, delay });
    }

    fn hits(&self, path: &str) -> usize {
        self.hits
            .lock()
            .unwrap()
            .iter()
            .filter(|p| *p == path)
            .count()
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }
}

fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (path, contents) in entries {
        writer.start_file(*path, options).unwrap();
        writer.write_all(contents).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

// ---- fakes -----------------------------------------------------------------------------------

#[derive(Default)]
struct FakeLauncher {
    starts: Mutex<Vec<(PathBuf, Option<String>)>>,
    stops: Arc<AtomicUsize>,
    fail: Mutex<Option<String>>,
    handles: Mutex<Vec<Arc<AtomicBool>>>,
}

struct FakeHandle {
    alive: Arc<AtomicBool>,
    stops: Arc<AtomicUsize>,
}

impl ServiceHandle for FakeHandle {
    fn port(&self) -> u16 {
        4242
    }
    fn api_key(&self) -> String {
        "0123456789abcdef0123456789abcdef".to_owned()
    }
    fn is_running(&mut self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }
    fn stop(&mut self) {
        if self.alive.swap(false, Ordering::SeqCst) {
            self.stops.fetch_add(1, Ordering::SeqCst);
        }
    }
}

#[async_trait]
impl Launcher for FakeLauncher {
    async fn start(
        &self,
        params: &StartParams<'_>,
        _http: &cv_providers::HttpClient,
        _cancel: &CancelToken,
    ) -> Result<Box<dyn ServiceHandle>, ServiceError> {
        self.starts.lock().unwrap().push((
            params.program.to_path_buf(),
            params.solver_url.map(str::to_owned),
        ));
        if let Some(reason) = self.fail.lock().unwrap().clone() {
            return Err(ServiceError::Config(reason));
        }
        let alive = Arc::new(AtomicBool::new(true));
        self.handles.lock().unwrap().push(Arc::clone(&alive));
        Ok(Box::new(FakeHandle {
            alive,
            stops: Arc::clone(&self.stops),
        }))
    }
}

impl FakeLauncher {
    fn starts(&self) -> usize {
        self.starts.lock().unwrap().len()
    }
    fn stops(&self) -> usize {
        self.stops.load(Ordering::SeqCst)
    }
    /// The running service ends by itself, as when it crashes.
    fn crash(&self) {
        for alive in self.handles.lock().unwrap().iter() {
            alive.store(false, Ordering::SeqCst);
        }
    }
}

/// What a check finds, and what it was asked.
struct Script {
    rows: Mutex<Vec<(&'static str, IndexerState)>>,
    runs: AtomicUsize,
    user_off_seen: Mutex<Vec<HashSet<String>>>,
    languages_seen: Mutex<Vec<HashSet<String>>>,
    removed: Mutex<Vec<String>>,
    hold: AtomicBool,
    /// Sites the check reports as left out for their language: (id, language).
    other_languages: Mutex<Vec<(&'static str, &'static str)>>,
}

impl Script {
    fn new(rows: &[(&'static str, IndexerState)]) -> Arc<Self> {
        Arc::new(Self {
            rows: Mutex::new(rows.to_vec()),
            runs: AtomicUsize::new(0),
            user_off_seen: Mutex::new(Vec::new()),
            languages_seen: Mutex::new(Vec::new()),
            removed: Mutex::new(Vec::new()),
            hold: AtomicBool::new(false),
            other_languages: Mutex::new(Vec::new()),
        })
    }
}

struct Scripted(Arc<Script>);

#[async_trait]
impl Provisioner for Scripted {
    async fn run(
        &self,
        _base_url: &str,
        store: Arc<dyn IndexerStore>,
        _passwords: Arc<dyn PasswordStore>,
        wanted: &Wanted,
        cancel: &CancelToken,
        on_row: RowCallback,
    ) -> Result<ProvisionReport, ProvisionError> {
        let (user_off, languages) = (&wanted.user_off, &wanted.languages);
        self.0.runs.fetch_add(1, Ordering::SeqCst);
        self.0
            .languages_seen
            .lock()
            .unwrap()
            .push(languages.clone());
        self.0.user_off_seen.lock().unwrap().push(user_off.clone());
        while self.0.hold.load(Ordering::SeqCst) {
            if cancel.is_cancelled() {
                return Err(ProvisionError::Cancelled);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let mut report = ProvisionReport {
            other_languages: self
                .0
                .other_languages
                .lock()
                .unwrap()
                .iter()
                .map(|(id, language)| OtherLanguageIndexer {
                    id: (*id).to_owned(),
                    name: (*id).to_owned(),
                    language: (*language).to_owned(),
                })
                .collect(),
            ..ProvisionReport::default()
        };
        for (id, state) in self.0.rows.lock().unwrap().clone() {
            let off = user_off.contains(id);
            let row = IndexerRow {
                id: id.to_owned(),
                name: id.to_uppercase(),
                state: if off { IndexerState::Off } else { state },
                reason: (state != IndexerState::Working).then(|| "It did not pass".to_owned()),
                checked_at: 1_700_000_000_000,
                user_off: off,
            };
            store.upsert(&row).unwrap();
            on_row(&row);
            report.rows.push(row);
        }
        Ok(report)
    }

    async fn remove(
        &self,
        _base_url: &str,
        _passwords: Arc<dyn PasswordStore>,
        id: &str,
    ) -> Result<(), ProvisionError> {
        self.0.removed.lock().unwrap().push(id.to_owned());
        Ok(())
    }
}

// ---- the lab ---------------------------------------------------------------------------------

fn a_query() -> TorrentQuery {
    TorrentQuery {
        kind: MediaKind::Movie,
        ids: cv_torrent::model::TitleIds::default(),
        title: "Heat".to_owned(),
        original_title: None,
        alternative_titles: Vec::new(),
        year: None,
        scope: Scope::Movie,
        runtime_minutes: None,
        anime: false,
        library_copy: None,
    }
}

struct Lab {
    manager: Arc<IndexerServiceManager>,
    server: Server,
    launcher: Arc<FakeLauncher>,
    script: Arc<Script>,
    settings: Arc<SettingsService>,
    torrents: Arc<TorrentService>,
    tools: Arc<ToolsService>,
    jobs: JobManager,
    notices: Arc<NoticeHub>,
    dir: tempfile::TempDir,
}

const TREE: &[(&str, &[u8])] = &[
    ("Jackett/JackettConsole.exe", b"the program"),
    ("Jackett/Definitions/yts.yml", b"a definition"),
    ("Jackett/Definitions/eztv.yml", b"another"),
];

fn manifest(url: &str, archive: &[u8], version: &str) -> Manifest {
    Manifest::parse(
        &json!({
            "schema": 1,
            "packages": { "indexer-service": {
                "display": "Indexer service (Jackett)", "license": "GPL-2.0",
                "homepage": "https://example.test",
                "platforms": { "windows-x86_64": { "version": version, "artifacts": [{
                    "url": url, "sha256": sha256(archive), "size": archive.len(),
                    "format": "zip", "mode": "tree", "root": "Jackett/",
                    "program": { "provides": "indexer-service", "path": "JackettConsole.exe" }
                }]}}
            }}
        })
        .to_string(),
    )
    .unwrap()
}

impl Lab {
    async fn new(rows: &[(&'static str, IndexerState)]) -> Self {
        Self::build(rows, None).await
    }

    async fn build(
        rows: &[(&'static str, IndexerState)],
        server_setup: Option<&dyn Fn(&Server)>,
    ) -> Self {
        let server = Server::start().await;
        let archive = zip_of(TREE);
        server.put("/jackett.zip", archive.clone());
        if let Some(setup) = server_setup {
            setup(&server);
        }
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(
            Database::open_with(dir.path().join("library.db"), &DbOptions { readers: 2 }).unwrap(),
        );
        let settings = Arc::new(SettingsService::in_memory("test"));
        let secrets = Arc::new(SecretStore::in_memory());
        let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
        let providers = Arc::new(ProviderCredentialsService::with_resolver(
            resolver,
            ProviderRegistry::new(),
        ));
        let torrents = Arc::new(TorrentService::new(
            Arc::clone(&settings),
            Arc::clone(&secrets),
            Arc::clone(&db),
            providers,
        ));
        let tools = Arc::new(ToolsService::with_manager(
            ToolsManager::with_manifest(
                dir.path().join("tools"),
                manifest(&server.url("/jackett.zip"), &archive, "1.0"),
                Some(Platform::WindowsX64),
            ),
            &settings,
        ));
        let jobs = JobManager::new(Lanes::new(LaneConfig::default()), ManagerConfig::default());
        let notices = Arc::new(NoticeHub::new());
        let launcher = Arc::new(FakeLauncher::default());
        let script = Script::new(rows);
        let manager = IndexerServiceManager::with_parts(
            IndexerParts {
                tools: Arc::clone(&tools),
                settings: Arc::clone(&settings),
                secrets,
                db,
                jobs: jobs.clone(),
                torrents: Arc::clone(&torrents),
                notices: Arc::clone(&notices),
                data_dir: dir.path().join("indexer-service"),
            },
            Arc::clone(&launcher) as Arc<dyn Launcher>,
            Arc::new(Scripted(Arc::clone(&script))),
            ReleaseSource {
                latest_url: server.url("/release"),
                download_prefix: server.url("/download/"),
            },
            Duration::from_secs(5),
        );
        Self {
            manager,
            server,
            launcher,
            script,
            settings,
            torrents,
            tools,
            jobs,
            notices,
            dir,
        }
    }

    /// The person accepts the responsibility notice.
    fn on(&self) {
        self.settings
            .set("torrents.notice-accepted", Value::Bool(true))
            .unwrap();
    }

    /// Writes a working indexer row as an earlier check would have left it.
    fn seed_row(&self, id: &str) {
        self.seed_row_in(id, cv_db::repo::torrent_indexers::IndexerState::Working);
    }

    /// Writes an indexer row in `state` as an earlier check would have left it.
    fn seed_row_in(&self, id: &str, state: cv_db::repo::torrent_indexers::IndexerState) {
        let db = Database::open_with(
            self.dir.path().join("library.db"),
            &DbOptions { readers: 1 },
        )
        .unwrap();
        let row = cv_db::repo::torrent_indexers::TorrentIndexer {
            id: id.to_owned(),
            name: id.to_owned(),
            state,
            reason: None,
            checked_at: 1,
            user_off: false,
        };
        db.write(move |tx| cv_db::repo::torrent_indexers::upsert(tx, &row))
            .unwrap();
    }

    fn data_dir(&self) -> PathBuf {
        self.dir.path().join("indexer-service")
    }

    /// A second manager over the same settings, tools and library: the app after a restart.
    fn restarted(&self) -> Arc<IndexerServiceManager> {
        let db = Arc::new(
            Database::open_with(
                self.dir.path().join("library.db"),
                &DbOptions { readers: 1 },
            )
            .unwrap(),
        );
        IndexerServiceManager::with_parts(
            IndexerParts {
                tools: Arc::clone(&self.tools),
                settings: Arc::clone(&self.settings),
                secrets: Arc::new(SecretStore::in_memory()),
                db,
                jobs: self.jobs.clone(),
                torrents: Arc::clone(&self.torrents),
                notices: Arc::clone(&self.notices),
                data_dir: self.data_dir(),
            },
            Arc::new(FakeLauncher::default()) as Arc<dyn Launcher>,
            Arc::new(Scripted(Arc::clone(&self.script))),
            ReleaseSource {
                latest_url: self.server.url("/release"),
                download_prefix: self.server.url("/download/"),
            },
            Duration::from_secs(5),
        )
    }

    /// The names of the jobs seen so far.
    fn job_names(&self) -> Vec<String> {
        self.jobs.list().into_iter().map(|j| j.name).collect()
    }

    /// The ids of the sources a search would ask, besides the built-in ones.
    fn indexer_sources(&self) -> Vec<String> {
        let query = a_query();
        let mut ids: Vec<String> = self
            .torrents
            .sources(&query)
            .unwrap()
            .iter()
            .map(|s| s.id().0.clone())
            .filter(|id| id.starts_with("indexer:"))
            .collect();
        ids.sort();
        ids
    }

    async fn idle_jobs(&self) {
        for _ in 0..400 {
            if self.jobs.list().iter().all(|j| j.state.is_terminal()) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("jobs did not finish: {:?}", self.jobs.list());
    }
}

async fn until(what: &str, mut condition: impl FnMut() -> bool) {
    for _ in 0..500 {
        if condition() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("timed out waiting for {what}");
}

// ---- while it is off -------------------------------------------------------------------------

#[tokio::test]
async fn nothing_is_downloaded_started_or_created_before_the_notice_is_accepted() {
    let lab = Lab::new(&[("yts", IndexerState::Working)]).await;
    lab.manager.apply().await;
    lab.manager.request_apply();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(lab.server.hits("/jackett.zip"), 0);
    assert_eq!(lab.server.hits("/release"), 0);
    assert_eq!(lab.launcher.starts(), 0);
    assert!(!lab.data_dir().exists());
    assert!(!lab.dir.path().join("tools").exists());
    assert!(lab.job_names().is_empty());
    let status = lab.manager.status();
    assert_eq!(status.state, ServiceState::NotInstalled);
    assert_eq!(status.port, None);
    assert!(matches!(
        lab.manager.check_sources(),
        Err(IndexerError::Torrent(_))
    ));
    // The search itself is closed too: no source of any kind is asked.
    assert!(lab.torrents.sources(&a_query()).is_err());
}

#[tokio::test]
async fn a_service_the_person_skipped_is_not_downloaded_either() {
    let lab = Lab::new(&[]).await;
    lab.on();
    lab.settings
        .set(INDEXER_ENABLED_KEY, Value::Bool(false))
        .unwrap();
    lab.manager.apply().await;
    assert_eq!(lab.server.hits("/jackett.zip"), 0);
    assert_eq!(lab.launcher.starts(), 0);
    assert!(!lab.data_dir().exists());
}

// ---- accepting the notice ----------------------------------------------------------------------------

#[tokio::test]
async fn accepting_the_notice_downloads_and_starts_it_by_itself_and_checks_the_indexers() {
    let lab = Lab::new(&[
        ("yts", IndexerState::Working),
        ("eztv", IndexerState::Working),
        ("blocked", IndexerState::NeedsSolver),
        ("gone", IndexerState::Unreachable),
    ])
    .await;
    lab.on();
    // No search, no click: just the accepted notice.
    lab.manager.apply().await;
    assert_eq!(lab.server.hits("/jackett.zip"), 1);
    assert_eq!(lab.launcher.starts(), 1);
    assert!(
        lab.launcher.starts.lock().unwrap()[0]
            .0
            .ends_with("JackettConsole.exe")
    );
    let status = lab.manager.status();
    assert_eq!(status.state, ServiceState::Running);
    assert_eq!(status.version.as_deref(), Some("1.0"));
    assert_eq!(status.port, Some(4242));

    lab.idle_jobs().await;
    let names = lab.job_names();
    assert!(names.contains(&INSTALL_JOB.to_owned()), "{names:?}");
    assert!(names.contains(&CHECK_JOB.to_owned()), "{names:?}");
    assert_eq!(lab.script.runs.load(Ordering::SeqCst), 1);

    // Only the indexers that passed are asked by a search.
    assert_eq!(lab.indexer_sources(), ["indexer:eztv", "indexer:yts"]);
    let status = lab.manager.status();
    assert_eq!((status.working, status.failed), (2, 2));
    let views = lab.manager.indexers();
    assert_eq!(views.len(), 4);
    assert_eq!(
        views.iter().find(|v| v.id == "blocked").unwrap().state,
        IndexerState::NeedsSolver
    );
}

#[tokio::test]
async fn the_settings_alone_drive_it_with_no_call_from_the_page() {
    let lab = Lab::new(&[("yts", IndexerState::Working)]).await;
    lab.on();
    lab.manager.request_apply();
    until("the service to run", || {
        lab.manager.status().state == ServiceState::Running
    })
    .await;
    until("the indexer to be asked", || {
        lab.indexer_sources() == ["indexer:yts"]
    })
    .await;
    assert_eq!(lab.server.hits("/jackett.zip"), 1);
}

#[tokio::test]
async fn applying_again_neither_downloads_nor_restarts() {
    let lab = Lab::new(&[("yts", IndexerState::Working)]).await;
    lab.on();
    lab.manager.apply().await;
    lab.manager.apply().await;
    lab.manager.apply().await;
    lab.idle_jobs().await;
    assert_eq!(lab.server.hits("/jackett.zip"), 1);
    assert_eq!(lab.launcher.starts(), 1);
    assert_eq!(
        lab.script.runs.load(Ordering::SeqCst),
        1,
        "not checked again"
    );
}

#[tokio::test]
async fn an_installed_service_starts_at_app_start_without_downloading() {
    let lab = Lab::new(&[("yts", IndexerState::Working)]).await;
    lab.on();
    lab.manager.apply().await;
    lab.manager.shutdown();
    assert_eq!(lab.launcher.stops(), 1);
    // The next launch: same files, a new manager.
    let again = IndexerServiceManager::with_parts(
        IndexerParts {
            tools: Arc::clone(&lab.tools),
            settings: Arc::clone(&lab.settings),
            secrets: Arc::new(SecretStore::in_memory()),
            db: Arc::new(
                Database::open_with(lab.dir.path().join("library.db"), &DbOptions { readers: 1 })
                    .unwrap(),
            ),
            jobs: lab.jobs.clone(),
            torrents: Arc::clone(&lab.torrents),
            notices: Arc::clone(&lab.notices),
            data_dir: lab.data_dir(),
        },
        Arc::clone(&lab.launcher) as Arc<dyn Launcher>,
        Arc::new(Scripted(Arc::clone(&lab.script))),
        ReleaseSource {
            latest_url: lab.server.url("/release"),
            download_prefix: lab.server.url("/download/"),
        },
        Duration::from_secs(5),
    );
    again.apply().await;
    assert_eq!(lab.server.hits("/jackett.zip"), 1, "no second download");
    assert_eq!(lab.launcher.starts(), 2);
    assert_eq!(again.status().state, ServiceState::Running);
    // The indexers it found last time are searched at once, without waiting for a check.
    assert_eq!(lab.indexer_sources(), ["indexer:yts"]);
    again.shutdown();
}

// ---- taking the notice back ---------------------------------------------------------------------------

#[tokio::test]
async fn taking_the_notice_back_stops_it_and_takes_its_indexers_out_of_the_search() {
    let lab = Lab::new(&[("yts", IndexerState::Working)]).await;
    lab.on();
    lab.manager.apply().await;
    lab.idle_jobs().await;
    assert_eq!(lab.indexer_sources(), ["indexer:yts"]);
    lab.settings
        .set("torrents.notice-accepted", Value::Bool(false))
        .unwrap();
    lab.manager.apply().await;
    assert_eq!(lab.launcher.stops(), 1);
    assert_eq!(lab.manager.status().state, ServiceState::Installed);
    assert_eq!(lab.manager.status().port, None);
    // Accepted again: it starts again from what is on disk.
    lab.on();
    lab.manager.apply().await;
    assert_eq!(lab.launcher.starts(), 2);
    assert_eq!(lab.server.hits("/jackett.zip"), 1);
}

#[tokio::test]
async fn skipping_the_service_stops_it_and_it_stays_off() {
    let lab = Lab::new(&[("yts", IndexerState::Working)]).await;
    lab.on();
    lab.manager.apply().await;
    lab.idle_jobs().await;
    lab.settings
        .set(INDEXER_ENABLED_KEY, Value::Bool(false))
        .unwrap();
    lab.manager.apply().await;
    lab.manager.apply().await;
    assert_eq!(lab.launcher.stops(), 1);
    assert_eq!(lab.launcher.starts(), 1);
    assert_eq!(lab.server.hits("/jackett.zip"), 1);
    assert!(
        lab.torrents
            .sources(&a_query())
            .unwrap()
            .iter()
            .all(|s| !s.id().0.starts_with("indexer:"))
    );
}

#[tokio::test]
async fn skipping_during_the_download_cancels_it_and_nothing_starts() {
    let lab = Lab::build(
        &[],
        Some(&|server: &Server| {
            server.put_slow("/jackett.zip", zip_of(TREE), Duration::from_secs(5));
        }),
    )
    .await;
    // The manifest checksum is of the same bytes, so the slow file is a valid download.
    lab.on();
    lab.manager.request_apply();
    until("the download to begin", || {
        lab.server.hits("/jackett.zip") == 1
            && lab.manager.status().state == ServiceState::Downloading
    })
    .await;
    lab.settings
        .set(INDEXER_ENABLED_KEY, Value::Bool(false))
        .unwrap();
    lab.manager.request_apply();
    until("the download to be cancelled", || {
        lab.manager.status().state == ServiceState::NotInstalled
    })
    .await;
    assert_eq!(lab.launcher.starts(), 0);
    assert!(
        !lab.dir
            .path()
            .join("tools")
            .join("indexer-service")
            .exists(),
        "nothing was installed"
    );
    // And it does not begin again by itself.
    lab.manager.apply().await;
    assert_eq!(lab.server.hits("/jackett.zip"), 1);
    assert_eq!(lab.launcher.starts(), 0);
}

#[tokio::test]
async fn the_app_closing_stops_it() {
    let lab = Lab::new(&[]).await;
    lab.on();
    lab.manager.apply().await;
    lab.manager.shutdown();
    assert_eq!(lab.launcher.stops(), 1);
    assert_eq!(lab.manager.status().port, None);
    lab.manager.shutdown();
    assert_eq!(lab.launcher.stops(), 1, "twice is fine");
}

// ---- keeping it running ----------------------------------------------------------------------

#[tokio::test]
async fn a_service_that_ended_by_itself_is_started_again() {
    let lab = Lab::new(&[]).await;
    lab.on();
    lab.manager.apply().await;
    lab.launcher.crash();
    lab.manager.apply().await;
    assert_eq!(lab.launcher.starts(), 2);
    assert_eq!(lab.manager.status().state, ServiceState::Running);
}

#[tokio::test]
async fn a_new_solver_address_starts_it_again_with_that_address() {
    let lab = Lab::new(&[]).await;
    lab.on();
    lab.manager.apply().await;
    lab.settings
        .set(
            SOLVER_URL_KEY,
            Value::String("http://localhost:8191".into()),
        )
        .unwrap();
    lab.manager.apply().await;
    assert_eq!(lab.launcher.starts(), 2);
    assert_eq!(lab.launcher.stops(), 1);
    assert_eq!(
        lab.launcher.starts.lock().unwrap()[1].1.as_deref(),
        Some("http://localhost:8191")
    );
}

#[tokio::test]
async fn a_service_that_will_not_start_says_why_and_is_tried_again_on_the_next_apply() {
    let lab = Lab::new(&[]).await;
    lab.on();
    *lab.launcher.fail.lock().unwrap() = Some("the port is taken".into());
    lab.manager.apply().await;
    let status = lab.manager.status();
    assert_eq!(status.state, ServiceState::Failed);
    assert!(status.reason.unwrap().contains("the port is taken"));
    *lab.launcher.fail.lock().unwrap() = None;
    lab.manager.apply().await;
    assert_eq!(lab.manager.status().state, ServiceState::Running);
    assert_eq!(lab.server.hits("/jackett.zip"), 1, "the files stayed");
}

#[tokio::test]
async fn a_download_that_fails_says_why_and_starts_nothing() {
    let lab = Lab::new(&[]).await;
    lab.server.files.lock().unwrap().clear();
    lab.on();
    lab.manager.apply().await;
    let status = lab.manager.status();
    assert_eq!(status.state, ServiceState::Failed);
    assert!(status.reason.is_some());
    assert_eq!(lab.launcher.starts(), 0);
    let job = lab
        .jobs
        .list()
        .into_iter()
        .find(|j| j.name == INSTALL_JOB)
        .expect("the install job");
    assert_eq!(job.state, JobState::Failed);
}

// ---- checking the indexers -------------------------------------------------------------------

#[tokio::test]
async fn the_indexers_are_checked_again_after_a_week_or_a_new_version_but_not_before() {
    for (checked, version, expected_runs) in [
        (None, "", 1),
        (Some((0_i64, "1.0")), "", 1),
        (Some((i64::MAX / 2, "1.0")), "", 0),
        (Some((i64::MAX / 2, "0.9")), "", 1),
    ] {
        let _ = version;
        let lab = Lab::new(&[("yts", IndexerState::Working)]).await;
        lab.on();
        // An earlier check left an indexer behind (without one a check is always due).
        lab.seed_row("yts");
        if let Some((at, version)) = checked {
            lab.settings
                .set(
                    "torrents.indexer-service.checked",
                    json!({ "at": at, "version": version }),
                )
                .unwrap();
        }
        lab.manager.apply().await;
        lab.idle_jobs().await;
        assert_eq!(
            lab.script.runs.load(Ordering::SeqCst),
            expected_runs,
            "{checked:?}"
        );
    }
}

#[tokio::test]
async fn a_check_forgets_rows_it_no_longer_lists_and_asks_for_the_tracker_languages() {
    let lab = Lab::new(&[("eztvx", IndexerState::Working)]).await;
    lab.on();
    // Rows an earlier check left: a site now in another language, and one switched off by hand.
    lab.seed_row("rutracker");
    lab.seed_row("kept-off");
    lab.manager.set_indexer_on("kept-off", false).unwrap();
    lab.settings.set("metadata.language", json!("de")).unwrap();
    lab.manager.apply().await;
    lab.idle_jobs().await;
    lab.manager.check_sources().unwrap();
    until("a check", || lab.script.runs.load(Ordering::SeqCst) >= 1).await;
    lab.idle_jobs().await;
    let ids: Vec<String> = lab.manager.indexers().into_iter().map(|v| v.id).collect();
    assert!(ids.contains(&"eztvx".to_owned()), "{ids:?}");
    assert!(
        !ids.contains(&"rutracker".to_owned()),
        "stale row pruned: {ids:?}"
    );
    assert!(
        ids.contains(&"kept-off".to_owned()),
        "a person's off is kept: {ids:?}"
    );
    let languages = lab
        .script
        .languages_seen
        .lock()
        .unwrap()
        .last()
        .cloned()
        .unwrap();
    assert_eq!(languages, HashSet::from(["en".to_owned(), "de".to_owned()]));
}

#[tokio::test]
async fn sites_that_were_down_are_tried_again_after_a_day_not_a_week() {
    use cv_db::repo::torrent_indexers::IndexerState as Db;
    let day = 24 * 60 * 60 * 1000_i64;
    let now = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap();
    for (failed, checked_ago, expected_runs) in [
        (Some(Db::Unreachable), 2 * day, 1),
        (Some(Db::Error), 2 * day, 1),
        (Some(Db::Unreachable), day / 2, 0),
        // A challenge only a solver changes: a week, as before.
        (Some(Db::NeedsSolver), 2 * day, 0),
        (None, 2 * day, 0),
    ] {
        let lab = Lab::new(&[("yts", IndexerState::Working)]).await;
        lab.on();
        lab.seed_row("yts");
        if let Some(state) = failed {
            lab.seed_row_in("downsite", state);
        }
        lab.settings
            .set(
                "torrents.indexer-service.checked",
                json!({ "at": now - checked_ago, "version": "1.0" }),
            )
            .unwrap();
        lab.manager.apply().await;
        lab.idle_jobs().await;
        assert_eq!(
            lab.script.runs.load(Ordering::SeqCst),
            expected_runs,
            "{failed:?} {checked_ago}"
        );
    }
}

#[tokio::test]
async fn the_status_says_what_other_languages_would_add_and_the_default_languages() {
    let lab = Lab::new(&[("eztvx", IndexerState::Working)]).await;
    lab.on();
    *lab.script.other_languages.lock().unwrap() =
        vec![("rutor", "ru"), ("dmhy", "zh"), ("rutracker", "ru")];
    lab.settings
        .set("metadata.fallback-language", json!(["tur", "en"]))
        .unwrap();
    lab.manager.apply().await;
    lab.idle_jobs().await;
    until("a check", || lab.script.runs.load(Ordering::SeqCst) >= 1).await;
    lab.idle_jobs().await;
    let status = lab.manager.status();
    assert_eq!(
        status.other_languages,
        vec![("ru".to_owned(), 2), ("zh".to_owned(), 1)]
    );
    assert_eq!(status.default_languages, ["en", "tr"]);
    // Kept across a restart: the next start shows the counts before any check.
    assert_eq!(lab.restarted().status().other_languages.len(), 2);
}

#[tokio::test]
async fn a_recent_check_that_left_no_indexer_behind_is_run_again() {
    let lab = Lab::new(&[]).await;
    lab.on();
    lab.settings
        .set(
            "torrents.indexer-service.checked",
            json!({ "at": i64::MAX / 2, "version": "" }),
        )
        .unwrap();
    lab.manager.apply().await;
    lab.idle_jobs().await;
    assert_eq!(lab.script.runs.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn check_sources_now_runs_one_check_at_a_time() {
    let lab = Lab::new(&[("yts", IndexerState::Working)]).await;
    assert!(matches!(
        lab.manager.check_sources(),
        Err(IndexerError::Torrent(_))
    ));
    lab.on();
    assert!(matches!(
        lab.manager.check_sources(),
        Err(IndexerError::NotRunning)
    ));
    lab.manager.apply().await;
    lab.idle_jobs().await;
    assert_eq!(lab.script.runs.load(Ordering::SeqCst), 1);

    lab.script.hold.store(true, Ordering::SeqCst);
    lab.manager.check_sources().unwrap();
    until("the check to start", || {
        lab.script.runs.load(Ordering::SeqCst) == 2
    })
    .await;
    assert!(lab.manager.status().checking);
    lab.manager.check_sources().unwrap();
    lab.manager.check_sources().unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(lab.script.runs.load(Ordering::SeqCst), 2, "still one check");
    lab.script.hold.store(false, Ordering::SeqCst);
    lab.idle_jobs().await;
    assert!(!lab.manager.status().checking);
}

#[tokio::test]
async fn a_check_the_app_did_not_live_to_finish_is_due_again_at_the_next_start() {
    let lab = Lab::new(&[("yts", IndexerState::Working)]).await;
    lab.on();
    lab.manager.apply().await;
    lab.idle_jobs().await;
    assert!(
        lab.settings
            .get("torrents.indexer-service.checked")
            .is_some()
    );

    lab.script.hold.store(true, Ordering::SeqCst);
    lab.manager.check_sources().unwrap();
    until("the check to start", || {
        lab.script.runs.load(Ordering::SeqCst) == 2
    })
    .await;
    // The app dies here: nothing says the last check was a good one any more.
    assert_eq!(lab.settings.get("torrents.indexer-service.checked"), None);
    lab.script.hold.store(false, Ordering::SeqCst);
    lab.idle_jobs().await;
    assert!(
        lab.settings
            .get("torrents.indexer-service.checked")
            .is_some()
    );
}

#[tokio::test]
async fn an_indexer_found_broken_leaves_the_search_when_the_next_check_says_so() {
    let lab = Lab::new(&[
        ("yts", IndexerState::Working),
        ("eztv", IndexerState::Working),
    ])
    .await;
    lab.on();
    lab.manager.apply().await;
    lab.idle_jobs().await;
    assert_eq!(lab.indexer_sources(), ["indexer:eztv", "indexer:yts"]);
    *lab.script.rows.lock().unwrap() = vec![
        ("yts", IndexerState::Unreachable),
        ("eztv", IndexerState::Working),
    ];
    lab.manager.check_sources().unwrap();
    until("the second check", || {
        lab.script.runs.load(Ordering::SeqCst) == 2
    })
    .await;
    lab.idle_jobs().await;
    assert_eq!(lab.indexer_sources(), ["indexer:eztv"]);
}

#[tokio::test]
async fn switching_one_indexer_off_removes_it_from_the_search_and_the_service_for_good() {
    let lab = Lab::new(&[
        ("yts", IndexerState::Working),
        ("eztv", IndexerState::Working),
    ])
    .await;
    lab.on();
    lab.manager.apply().await;
    lab.idle_jobs().await;
    lab.manager.set_indexer_on("yts", false).unwrap();
    assert_eq!(lab.indexer_sources(), ["indexer:eztv"]);
    until("the service to be told", || {
        lab.script.removed.lock().unwrap().as_slice() == ["yts"]
    })
    .await;
    let view = lab
        .manager
        .indexers()
        .into_iter()
        .find(|v| v.id == "yts")
        .unwrap();
    assert!(view.user_off);
    assert_eq!(view.state, IndexerState::Off);

    // A later check skips it and keeps it off.
    lab.manager.check_sources().unwrap();
    until("the next check", || {
        lab.script.runs.load(Ordering::SeqCst) == 2
    })
    .await;
    lab.idle_jobs().await;
    assert!(lab.script.user_off_seen.lock().unwrap()[1].contains("yts"));
    assert_eq!(lab.indexer_sources(), ["indexer:eztv"]);

    // Switched back on, it is checked again and returns.
    lab.manager.set_indexer_on("yts", true).unwrap();
    until("the check after switching on", || {
        lab.script.runs.load(Ordering::SeqCst) == 3
    })
    .await;
    lab.idle_jobs().await;
    assert_eq!(lab.indexer_sources(), ["indexer:eztv", "indexer:yts"]);
}

#[tokio::test]
async fn the_page_can_open_the_service_only_while_it_runs() {
    let lab = Lab::new(&[]).await;
    lab.on();
    assert!(matches!(
        lab.manager.manager_access(),
        Err(IndexerError::NotRunning)
    ));
    lab.manager.apply().await;
    let (url, password) = lab.manager.manager_access().unwrap();
    assert_eq!(url, "http://127.0.0.1:4242/UI/Dashboard");
    assert_eq!(password, None, "the fake check saved no password");
}

// ---- updating --------------------------------------------------------------------------------

fn release(base: &str, tag: &str, archive: &[u8], digest: Option<String>) -> Vec<u8> {
    let mut asset = json!({
        "name": "jackett.zip",
        "size": archive.len(),
        "browser_download_url": format!("{base}/download/{tag}/jackett.zip"),
    });
    if let Some(digest) = digest {
        asset["digest"] = json!(digest);
    }
    json!({ "tag_name": tag, "assets": [asset] })
        .to_string()
        .into_bytes()
}

#[tokio::test]
async fn a_newer_release_is_installed_over_the_old_one_and_the_service_comes_back() {
    let lab = Lab::new(&[("yts", IndexerState::Working)]).await;
    lab.on();
    lab.manager.apply().await;
    lab.idle_jobs().await;
    assert_eq!(lab.manager.status().version.as_deref(), Some("1.0"));

    let mut newer: Vec<(&str, &[u8])> = TREE.to_vec();
    newer.push(("Jackett/NEWS.txt", b"what is new"));
    let archive = zip_of(&newer);
    lab.server
        .put("/download/v2.0/jackett.zip", archive.clone());
    lab.server.put(
        "/release",
        release(
            &lab.server.base,
            "v2.0",
            &archive,
            Some(format!("sha256:{}", sha256(&archive))),
        ),
    );
    let version = lab.manager.update().await.expect("update");
    assert_eq!(version, "2.0");
    assert_eq!(lab.manager.status().version.as_deref(), Some("2.0"));
    assert_eq!(lab.manager.status().state, ServiceState::Running);
    assert_eq!(
        lab.launcher.stops(),
        1,
        "stopped while its files were replaced"
    );
    assert_eq!(lab.launcher.starts(), 2);
    assert!(lab.job_names().contains(&UPDATE_JOB.to_owned()));
    assert!(
        lab.dir
            .path()
            .join("tools/indexer-service/2.0/NEWS.txt")
            .exists()
    );
    assert!(!lab.dir.path().join("tools/indexer-service/1.0").exists());
    // The new version is checked again.
    lab.idle_jobs().await;
    assert_eq!(lab.script.runs.load(Ordering::SeqCst), 2);
    // And a second update finds nothing newer.
    assert!(matches!(
        lab.manager.update().await,
        Err(IndexerError::UpToDate)
    ));
}

#[tokio::test]
async fn a_release_without_a_checksum_is_refused_and_nothing_changes() {
    let lab = Lab::new(&[]).await;
    lab.on();
    lab.manager.apply().await;
    let archive = zip_of(TREE);
    lab.server
        .put("/download/v2.0/jackett.zip", archive.clone());
    lab.server.put(
        "/release",
        release(&lab.server.base, "v2.0", &archive, None),
    );
    let error = lab.manager.update().await.expect_err("refused");
    assert!(
        matches!(error, IndexerError::Refused(UpdateRefused::NoDigest)),
        "{error:?}"
    );
    assert_eq!(
        lab.server.hits("/download/v2.0/jackett.zip"),
        0,
        "nothing was downloaded"
    );
    assert_eq!(lab.launcher.stops(), 0, "it kept running");
    assert_eq!(lab.manager.status().version.as_deref(), Some("1.0"));
}

#[tokio::test]
async fn a_download_that_does_not_match_its_published_checksum_is_discarded() {
    let lab = Lab::new(&[]).await;
    lab.on();
    lab.manager.apply().await;
    let archive = zip_of(TREE);
    lab.server
        .put("/download/v2.0/jackett.zip", archive.clone());
    lab.server.put(
        "/release",
        release(
            &lab.server.base,
            "v2.0",
            &archive,
            Some(format!("sha256:{}", "0".repeat(64))),
        ),
    );
    let error = lab.manager.update().await.expect_err("mismatch");
    assert!(error.to_string().contains("checksum"), "{error}");
    // The old version is untouched and the service is running again.
    assert_eq!(lab.manager.status().version.as_deref(), Some("1.0"));
    assert_eq!(lab.manager.status().state, ServiceState::Running);
    assert!(!lab.dir.path().join("tools/indexer-service/2.0").exists());
}

#[tokio::test]
async fn a_download_from_somewhere_else_is_refused() {
    let lab = Lab::new(&[]).await;
    lab.on();
    lab.manager.apply().await;
    let archive = zip_of(TREE);
    let digest = Some(format!("sha256:{}", sha256(&archive)));
    let mut body: Value =
        serde_json::from_slice(&release(&lab.server.base, "v2.0", &archive, digest)).unwrap();
    body["assets"][0]["browser_download_url"] = json!("https://example.com/jackett.zip");
    lab.server.put("/release", body.to_string().into_bytes());
    let error = lab.manager.update().await.expect_err("refused");
    assert!(
        matches!(error, IndexerError::Refused(UpdateRefused::UntrustedUrl)),
        "{error:?}"
    );
}

#[tokio::test]
async fn the_weekly_lookup_only_tells_and_says_it_once() {
    let archive = zip_of(TREE);
    let lab = Lab::build(&[], None).await;
    lab.server.put(
        "/release",
        release(
            &lab.server.base,
            "v3.0",
            &archive,
            Some(format!("sha256:{}", sha256(&archive))),
        ),
    );
    let told = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&told);
    lab.notices.subscribe(move |notice| {
        sink.lock().unwrap().push(notice.title.clone());
        true
    });
    lab.on();
    lab.manager.apply().await;
    let status = lab.manager.status();
    assert_eq!(status.latest_version.as_deref(), Some("3.0"));
    assert_eq!(
        status.version.as_deref(),
        Some("1.0"),
        "nothing was installed"
    );
    assert_eq!(told.lock().unwrap().len(), 1);
    assert_eq!(lab.server.hits("/release"), 1);
    // Looked up once a week, so a second apply does not ask again.
    lab.manager.apply().await;
    assert_eq!(lab.server.hits("/release"), 1);
    assert_eq!(told.lock().unwrap().len(), 1);
    // After a restart the newer version is still announced, without asking again.
    let again = lab.restarted();
    assert_eq!(again.status().latest_version.as_deref(), Some("3.0"));
    assert_eq!(lab.server.hits("/release"), 1);
}

#[tokio::test]
async fn a_release_lookup_that_fails_changes_nothing() {
    let lab = Lab::new(&[]).await;
    lab.server.files.lock().unwrap().remove("/release");
    lab.on();
    lab.manager.apply().await;
    assert_eq!(lab.manager.status().state, ServiceState::Running);
    assert_eq!(lab.manager.status().latest_version, None);
}
