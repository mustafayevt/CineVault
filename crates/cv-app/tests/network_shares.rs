//! Network shares at start, through the booted app services: a remembered share is connected
//! before its folders are probed and scanned, so its titles never read offline on the way, while
//! the folders on other drives are scanned at once; a share that does not connect is offline
//! with its reason, and its titles stay, offline, never missing.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use cv_app::data_location::{DataLocation, LocationInputs};
use cv_app::network_shares::{OfflineReason, secret_account};
use cv_app::scanning::SourceState;
use cv_app::secrets::SecretStore;
use cv_app::services::{AppServices, BootOptions};
use cv_db::repo::data_sources::insert_source;
use cv_db::repo::network_shares::{NewNetworkShare, insert_share};
use cv_domain::SourceKind;
use cv_jobs::LaneId;
use cv_shares::fake::FakeConnector;
use cv_shares::{
    Cancel, Connected, Login, ShareAddress, ShareConnector, ShareError, ShareErrorKind, Support,
};
use tempfile::TempDir;

const PASSWORD: &str = "Example-Pass-42";

fn location(root: &Path) -> DataLocation {
    let inputs = LocationInputs {
        exe_dir: root.join("app"),
        user_data_base: root.join("userdata"),
        user_config_base: root.join("userconfig"),
    };
    std::fs::create_dir_all(&inputs.exe_dir).unwrap();
    DataLocation::resolve(&inputs)
}

fn put(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, b"video").unwrap();
}

/// A share whose files are not there until it connects: before a restart the test moves the
/// share's folder aside, and the connect moves it back after a pause, as a NAS that takes a moment
/// to answer.
struct Appearing {
    fake: FakeConnector,
    aside: Option<PathBuf>,
    pause: Duration,
    connected: AtomicBool,
}

impl ShareConnector for Appearing {
    fn support(&self) -> Support {
        self.fake.support()
    }

    fn connect(
        &self,
        share: &ShareAddress,
        login: &Login,
        cancel: &Cancel,
    ) -> Result<Connected, ShareError> {
        std::thread::sleep(self.pause);
        let root = self.fake.root_of(share);
        if let Some(aside) = self.aside.as_ref().filter(|aside| aside.exists()) {
            std::fs::rename(aside, &root)
                .map_err(|e| ShareError::new(ShareErrorKind::Other, e.to_string()))?;
        }
        let connected = self.fake.connect(share, login, cancel)?;
        self.connected.store(true, Ordering::SeqCst);
        Ok(connected)
    }

    fn is_connected(&self, share: &ShareAddress, cancel: &Cancel) -> Result<bool, ShareError> {
        self.fake.is_connected(share, cancel)
    }

    fn disconnect(&self, share: &ShareAddress, cancel: &Cancel) -> Result<(), ShareError> {
        self.fake.disconnect(share, cancel)
    }

    fn local_root(
        &self,
        share: &ShareAddress,
        cancel: &Cancel,
    ) -> Result<Option<PathBuf>, ShareError> {
        self.fake.local_root(share, cancel)
    }
}

fn boot(root: &Path, secrets: &Arc<SecretStore>, connector: Arc<Appearing>) -> Arc<AppServices> {
    let options = BootOptions::new(location(root), Arc::clone(secrets))
        .without_providers()
        .with_share_connector(connector);
    AppServices::boot(options).expect("boot")
}

fn availability(services: &AppServices, folder: &str) -> Option<String> {
    let pattern = format!("%{folder}%");
    services
        .db()
        .read(move |c| {
            Ok(c.query_row(
                "SELECT availability FROM item WHERE kind = 'movie' AND path LIKE ?1",
                [pattern],
                |row| row.get::<_, String>(0),
            )
            .ok())
        })
        .unwrap()
}

/// Whether every job has ended and at least `rescans` scans ran.
fn settled(services: &AppServices, rescans: usize) -> bool {
    let jobs = services.jobs().list();
    jobs.iter().all(|job| job.state.is_terminal())
        && jobs
            .iter()
            .filter(|job| is_scan(&job.name, job.lane.clone()))
            .count()
            >= rescans
}

fn is_scan(name: &str, lane: LaneId) -> bool {
    lane == LaneId::Disk && name.starts_with("Scanning")
}

fn until(what: &str, mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !check() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A library with a local folder and a folder on the remembered share `\\nas\movies`, both scanned
/// in a first session, and the share's files moved aside as after a restart of the computer.
struct Library {
    temp: TempDir,
    shares: PathBuf,
    aside: PathBuf,
    secrets: Arc<SecretStore>,
}

fn connector(shares: &Path, aside: Option<PathBuf>, pause: Duration) -> Arc<Appearing> {
    Arc::new(Appearing {
        fake: FakeConnector::new(shares),
        aside,
        pause,
        connected: AtomicBool::new(false),
    })
}

fn scanned_then_restarted() -> Library {
    let temp = TempDir::new().unwrap();
    let local = temp.path().join("local");
    let shares = temp.path().join("shares");
    let share_root = shares.join("nas").join("movies");
    put(&local.join("Alien (1979)").join("Alien (1979).mkv"));
    put(&share_root.join("Films/Heat (1995)/Heat (1995).mkv"));
    let secrets = Arc::new(SecretStore::in_memory());

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let services = boot(
        temp.path(),
        &secrets,
        connector(&shares, None, Duration::ZERO),
    );
    let (local_path, films) = (
        local.to_string_lossy().into_owned(),
        share_root.join("Films").to_string_lossy().into_owned(),
    );
    let root = share_root.to_string_lossy().into_owned();
    let id = services
        .db()
        .write(move |tx| {
            insert_source(tx, SourceKind::Movie, &local_path)?;
            insert_source(tx, SourceKind::Movie, &films)?;
            insert_share(
                tx,
                &NewNetworkShare {
                    server: "nas",
                    share: "movies",
                    user: "alex",
                    local_root: &root,
                    remember: true,
                },
            )
            .map(|share| share.id)
        })
        .unwrap();
    secrets.set(&secret_account(id), PASSWORD).unwrap();
    runtime.block_on(async { services.start() });
    until("the first scans", || {
        settled(&services, 1)
            && availability(&services, "Heat").as_deref() == Some("online")
            && availability(&services, "Alien").as_deref() == Some("online")
    });
    services.shutdown();
    drop(services);

    // The computer restarted: the share is not connected, its files are not there.
    let aside = temp.path().join("aside");
    std::fs::rename(&share_root, &aside).unwrap();
    Library {
        temp,
        shares,
        aside,
        secrets,
    }
}

#[test]
fn a_remembered_share_connects_before_its_titles_are_checked_and_other_folders_do_not_wait() {
    let library = scanned_then_restarted();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let connector = connector(
        &library.shares,
        Some(library.aside.clone()),
        Duration::from_millis(800),
    );
    let services = boot(
        library.temp.path(),
        &library.secrets,
        Arc::clone(&connector),
    );
    runtime.block_on(async { services.start() });

    // The local folder's scan starts at once, while the share is still connecting.
    assert!(!connector.connected.load(Ordering::SeqCst));
    assert!(
        services
            .jobs()
            .list()
            .iter()
            .any(|job| is_scan(&job.name, job.lane.clone())),
        "the local folder is scanned without waiting for the share"
    );
    // The health check does not call the share's folder offline while it connects.
    let health = services.scan().source_health().unwrap();
    assert!(!connector.connected.load(Ordering::SeqCst));
    assert!(
        health
            .iter()
            .all(|folder| folder.state == SourceState::Online && folder.reason.is_none()),
        "{health:?}"
    );

    let mut seen = Vec::new();
    until("the share's scan", || {
        seen.push(availability(&services, "Heat"));
        connector.connected.load(Ordering::SeqCst) && settled(&services, 2)
    });
    assert!(
        seen.iter().all(|state| state.as_deref() == Some("online")),
        "{seen:?}"
    );
    assert_eq!(availability(&services, "Heat").as_deref(), Some("online"));
    services.shutdown();
}

#[test]
fn a_share_that_does_not_connect_is_offline_with_its_reason_and_its_titles_are_never_missing() {
    let library = scanned_then_restarted();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let connector = connector(&library.shares, None, Duration::ZERO);
    let movies = ShareAddress::new("nas", "movies").unwrap();
    connector.fake.fail_connect(
        &movies,
        Some(ShareError::new(ShareErrorKind::WrongLogin, "logon failure")),
    );
    let services = boot(
        library.temp.path(),
        &library.secrets,
        Arc::clone(&connector),
    );
    runtime.block_on(async { services.start() });

    until("both scans", || settled(&services, 2));
    let share = &services.shares().list().unwrap()[0];
    assert_eq!(share.last_error.as_deref(), Some("wrong_login"));
    let health = services.scan().source_health().unwrap();
    let films = health
        .iter()
        .find(|folder| folder.source.path.contains("Films"))
        .unwrap();
    assert_eq!(films.state, SourceState::Offline);
    assert_eq!(films.reason, Some(OfflineReason::WrongPassword));
    assert_eq!(
        films.reason.map(OfflineReason::label),
        Some("Wrong password")
    );
    let local = health
        .iter()
        .find(|folder| folder.source.path.contains("local"))
        .unwrap();
    assert_eq!((local.state, local.reason), (SourceState::Online, None));

    // The scan of the folder that cannot be read keeps the title, offline.
    assert_eq!(availability(&services, "Heat").as_deref(), Some("offline"));
    assert_eq!(availability(&services, "Alien").as_deref(), Some("online"));
    services.shutdown();
}
