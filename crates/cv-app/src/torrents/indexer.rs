//! The managed indexer service in the app.
//!
//! [`IndexerServiceManager`] keeps the service in the state the settings ask for:
//!
//! * While Torrent search is off, or the person switched the indexer service off (Skip, or its
//!   switch), nothing is downloaded, started or contacted, and a service that runs is stopped.
//! * While both are on, a missing install starts downloading by itself as the Activity job
//!   "Downloading the indexer service" (nothing to click), the service is started in the background
//!   as soon as it is installed (it never waits for the first search), and its indexers are checked
//!   as the job "Checking torrent sources" on the first start, after a version change, every seven
//!   days and when the person asks. Indexers that pass become sources as they pass.
//! * The service runs while the app runs: there is no idle timer. It is stopped when the notice
//!   or the service is switched off and on [`IndexerServiceManager::shutdown`]; the program also
//!   ends by itself if the app dies (see `cv_torrent::indexer_service::process`).
//!
//! The process ([`Launcher`]) and the provisioning ([`Provisioner`]) are behind traits so the
//! tests drive the whole life cycle without starting a program.

use std::collections::HashSet;
use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use async_trait::async_trait;
use cv_db::Database;
use cv_db::repo::torrent_indexers as db_rows;
use cv_jobs::{
    ItemControl, ItemError, ItemWork, JobHandle, JobManager, JobSpec, LaneId, Priority, StageError,
};
use cv_media::tools::{Package, PlatformSpec, ToolId, ToolState};
use cv_providers::http::HttpRequest;
use cv_torrent::TorrentError;
use cv_torrent::cancel::CancelToken;
use cv_torrent::indexer_service::provision::{
    IndexerRow, IndexerState, IndexerStore, PasswordStore, ProvisionError, ProvisionOptions,
    ProvisionReport, open_session, provision,
};
use cv_torrent::indexer_service::{
    AdminClient, Launcher, ProcessLauncher, READY_TIMEOUT, ServiceHandle, StartParams,
};
use cv_torrent::sources::indexer::{ID_PREFIX, IndexerSource};
use serde_json::Value;

use super::TorrentService;
use super::indexer_update::{
    DOWNLOAD_PREFIX, LATEST_RELEASE_URL, UpdateRefused, is_newer, plan_update, release_version,
};
use crate::app_settings::SettingsService;
use crate::notices::{AppNotice, NoticeHub, NoticeLevel};
use crate::secrets::SecretStore;
use crate::tools::ToolsService;
use crate::torrent_settings::{
    ADMIN_PASSWORD_ACCOUNT, TorrentSettings, default_tracker_languages, tracker_languages,
};

/// The Activity job that downloads the service.
pub const INSTALL_JOB: &str = "Downloading the indexer service";
/// The Activity job that installs a newer version.
pub const UPDATE_JOB: &str = "Updating the indexer service";
/// The Activity job that checks the indexers.
pub const CHECK_JOB: &str = "Checking torrent sources";

/// Setting: when the indexers were last checked and with which version (`{ "at": ms, "version" }`).
const LAST_CHECK_KEY: &str = "torrents.indexer-service.checked";
/// Setting: when the newest release was last looked up (milliseconds).
const UPDATE_CHECKED_KEY: &str = "torrents.indexer-service.update-checked";
/// Setting: the newest version that lookup found, so the update stays announced after a restart
/// until it is installed.
const LATEST_KEY: &str = "torrents.indexer-service.latest";
/// Setting: how many sites each language the person has not chosen would add, as the last check
/// found them (`{ "ru": 7, "zh": 4 }`).
const OTHER_LANGUAGES_KEY: &str = "torrents.indexer-service.other-languages";
/// The indexers are checked again after this long.
pub const RECHECK_AFTER_MS: i64 = 7 * 24 * 60 * 60 * 1000;
/// Sites that did not answer or failed for another reason (a challenge aside, which only a solver
/// changes) are tried again after this long: a site that is down today is often back tomorrow.
pub const RETRY_FAILED_AFTER_MS: i64 = 24 * 60 * 60 * 1000;
/// The newest release is looked up again after this long.
pub const UPDATE_LOOKUP_AFTER_MS: i64 = 7 * 24 * 60 * 60 * 1000;

/// How the service stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceState {
    /// Not on this computer; nothing asks for it (or the download has not begun).
    NotInstalled,
    /// The download is running.
    Downloading,
    /// On this computer and not running.
    Installed,
    /// Starting, until it answers.
    Starting,
    /// Running and answering.
    Running,
    /// It ended while it should be running.
    Stopped,
    /// It could not be installed or started; the reason says why.
    Failed,
}

/// What Settings shows about the service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceStatus {
    pub state: ServiceState,
    /// The installed version.
    pub version: Option<String>,
    /// The newest version found by the last look-up, when it is newer than the installed one.
    pub latest_version: Option<String>,
    /// The loopback port while it runs.
    pub port: Option<u16>,
    /// Indexers that passed the last check and are used by searches.
    pub working: usize,
    /// Indexers that failed the last check.
    pub failed: usize,
    /// A check of the indexers is running.
    pub checking: bool,
    /// Plain words for `Failed`.
    pub reason: Option<String>,
    /// The tracker languages when the person has chosen none (English and their other languages).
    pub default_languages: Vec<String>,
    /// How many sites each language that is not a tracker language would add, as the last check
    /// found them, most first.
    pub other_languages: Vec<(String, usize)>,
}

/// The counts saved under [`OTHER_LANGUAGES_KEY`], most first then by code; nothing for an
/// unreadable value.
fn read_other_languages(value: Option<&Value>) -> Vec<(String, usize)> {
    let Some(map) = value.and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut out: Vec<(String, usize)> = map
        .iter()
        .filter_map(|(code, count)| {
            let count = usize::try_from(count.as_u64()?).ok()?;
            (count > 0).then(|| (code.clone(), count))
        })
        .collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    out
}

/// Whether a site's last check failed in a way that may pass tomorrow.
fn may_pass_later(state: db_rows::IndexerState) -> bool {
    matches!(
        state,
        db_rows::IndexerState::Unreachable | db_rows::IndexerState::Error
    )
}

/// An indexer for the list under the service's card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexerView {
    pub id: String,
    pub name: String,
    pub state: IndexerState,
    pub reason: Option<String>,
    pub checked_at_ms: i64,
    pub user_off: bool,
}

/// Why a request about the service failed.
#[derive(Debug, thiserror::Error)]
pub enum IndexerError {
    #[error("{0}")]
    Torrent(#[from] TorrentError),
    #[error("The indexer service is not running.")]
    NotRunning,
    #[error("{0}")]
    Refused(#[from] UpdateRefused),
    #[error("The indexer service is already up to date.")]
    UpToDate,
    #[error("{0}")]
    Failed(String),
}

/// Called with each indexer as soon as its check is done.
pub type RowCallback = Arc<dyn Fn(&IndexerRow) + Send + Sync>;

/// What a check is asked to set up.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Wanted {
    /// The indexers a person switched off.
    pub user_off: HashSet<String>,
    /// The tracker languages (`en`, `ru`); only indexers in one of them are set up.
    pub languages: HashSet<String>,
}

/// The checking of the indexers behind a trait, so the tests need no service.
#[async_trait]
pub trait Provisioner: Send + Sync {
    /// Signs in to the service at `base_url`, checks its indexers and reports each as it is done.
    ///
    /// # Errors
    /// As [`provision`].
    async fn run(
        &self,
        base_url: &str,
        store: Arc<dyn IndexerStore>,
        passwords: Arc<dyn PasswordStore>,
        wanted: &Wanted,
        cancel: &CancelToken,
        on_row: RowCallback,
    ) -> Result<ProvisionReport, ProvisionError>;

    /// Removes one indexer from the service.
    ///
    /// # Errors
    /// As [`open_session`].
    async fn remove(
        &self,
        base_url: &str,
        passwords: Arc<dyn PasswordStore>,
        id: &str,
    ) -> Result<(), ProvisionError>;
}

/// Checks the indexers of a real service through its admin API.
#[derive(Debug, Clone, Copy, Default)]
pub struct AdminProvisioner;

#[async_trait]
impl Provisioner for AdminProvisioner {
    async fn run(
        &self,
        base_url: &str,
        store: Arc<dyn IndexerStore>,
        passwords: Arc<dyn PasswordStore>,
        wanted: &Wanted,
        cancel: &CancelToken,
        on_row: RowCallback,
    ) -> Result<ProvisionReport, ProvisionError> {
        let admin = AdminClient::new(base_url)?;
        open_session(&admin, passwords.as_ref()).await?;
        provision(
            &admin,
            store,
            &wanted.user_off,
            &wanted.languages,
            ProvisionOptions::default(),
            cancel,
            on_row.as_ref(),
        )
        .await
    }

    async fn remove(
        &self,
        base_url: &str,
        passwords: Arc<dyn PasswordStore>,
        id: &str,
    ) -> Result<(), ProvisionError> {
        let admin = AdminClient::new(base_url)?;
        open_session(&admin, passwords.as_ref()).await?;
        admin.delete(id).await?;
        Ok(())
    }
}

/// The admin password in the credential store.
struct SecretPassword(Arc<SecretStore>);

impl PasswordStore for SecretPassword {
    fn get(&self) -> Option<String> {
        self.0
            .get(ADMIN_PASSWORD_ACCOUNT)
            .ok()
            .flatten()
            .map(|secret| secret.expose().to_owned())
    }

    fn set(&self, password: &str) -> Result<(), String> {
        self.0
            .set(ADMIN_PASSWORD_ACCOUNT, password)
            .map_err(|error| error.to_string())
    }
}

/// The indexer rows in the library database.
struct DbIndexers(Arc<Database>);

fn to_db_state(state: IndexerState) -> db_rows::IndexerState {
    db_rows::IndexerState::parse(state.as_str()).unwrap_or(db_rows::IndexerState::Error)
}

fn from_db_state(state: db_rows::IndexerState) -> IndexerState {
    IndexerState::parse(state.as_str()).unwrap_or(IndexerState::Error)
}

impl IndexerStore for DbIndexers {
    fn upsert(&self, row: &IndexerRow) -> Result<(), String> {
        let row = db_rows::TorrentIndexer {
            id: row.id.clone(),
            name: row.name.clone(),
            state: to_db_state(row.state),
            reason: row.reason.clone(),
            checked_at: row.checked_at,
            user_off: row.user_off,
        };
        self.0
            .write(move |tx| db_rows::upsert(tx, &row))
            .map_err(|error| error.to_string())
    }
}

#[derive(Default)]
struct State {
    state: Option<ServiceState>,
    reason: Option<String>,
    running: Option<Box<dyn ServiceHandle>>,
    port: Option<u16>,
    api_key: Option<String>,
    /// The challenge solver address the running service was configured with.
    solver: String,
    install_job: Option<JobHandle>,
    check_job: Option<JobHandle>,
    start_cancel: Option<CancelToken>,
    /// Ids of the source objects handed to the Torrent search service.
    registered: HashSet<String>,
    latest: Option<String>,
    notified: Option<String>,
}

/// Keeps the indexer service in the state the settings ask for.
pub struct IndexerServiceManager {
    tools: Arc<ToolsService>,
    settings: Arc<SettingsService>,
    secrets: Arc<SecretStore>,
    db: Arc<Database>,
    jobs: JobManager,
    torrents: Arc<TorrentService>,
    notices: Arc<NoticeHub>,
    launcher: Arc<dyn Launcher>,
    provisioner: Arc<dyn Provisioner>,
    data_dir: PathBuf,
    releases_url: String,
    download_prefix: String,
    ready_timeout: Duration,
    apply_lock: tokio::sync::Mutex<()>,
    state: Mutex<State>,
}

impl fmt::Debug for IndexerServiceManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IndexerServiceManager")
            .finish_non_exhaustive()
    }
}

/// Where the newest release is described and which downloads are trusted (tests point both at a
/// local server).
#[derive(Debug, Clone)]
pub struct ReleaseSource {
    pub latest_url: String,
    pub download_prefix: String,
}

/// What the manager is made of.
pub struct IndexerParts {
    pub tools: Arc<ToolsService>,
    pub settings: Arc<SettingsService>,
    pub secrets: Arc<SecretStore>,
    pub db: Arc<Database>,
    pub jobs: JobManager,
    pub torrents: Arc<TorrentService>,
    pub notices: Arc<NoticeHub>,
    /// The service's data folder (`<app data>/indexer-service`).
    pub data_dir: PathBuf,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// One step of a job with a single step.
struct OneStep(&'static str);

impl fmt::Display for OneStep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl IndexerServiceManager {
    /// A manager that starts the real program and checks the real service.
    #[must_use]
    pub fn new(parts: IndexerParts) -> Arc<Self> {
        Self::with_parts(
            parts,
            Arc::new(ProcessLauncher),
            Arc::new(AdminProvisioner),
            ReleaseSource {
                latest_url: LATEST_RELEASE_URL.to_owned(),
                download_prefix: DOWNLOAD_PREFIX.to_owned(),
            },
            READY_TIMEOUT,
        )
    }

    /// A manager with its own launcher, provisioner, release address and readiness limit (tests).
    #[must_use]
    pub fn with_parts(
        parts: IndexerParts,
        launcher: Arc<dyn Launcher>,
        provisioner: Arc<dyn Provisioner>,
        releases: ReleaseSource,
        ready_timeout: Duration,
    ) -> Arc<Self> {
        Arc::new(Self {
            tools: parts.tools,
            settings: parts.settings,
            secrets: parts.secrets,
            db: parts.db,
            jobs: parts.jobs,
            torrents: parts.torrents,
            notices: parts.notices,
            launcher,
            provisioner,
            data_dir: parts.data_dir,
            releases_url: releases.latest_url,
            download_prefix: releases.download_prefix,
            ready_timeout,
            apply_lock: tokio::sync::Mutex::new(()),
            state: Mutex::new(State::default()),
        })
    }

    // ---- what is wanted ---------------------------------------------------------------------

    /// Whether the service should exist now: the Torrent search notice was accepted and the person did not switch
    /// the service off.
    fn wanted(&self) -> bool {
        self.torrents.notice_accepted()
            && TorrentSettings::read(&self.settings).indexer_service_enabled
    }

    fn solver_url(&self) -> String {
        TorrentSettings::read(&self.settings).solver_url
    }

    /// Reacts to a change of the switches: a service that is no longer wanted is stopped and a
    /// download that is no longer wanted is cancelled at once, then everything is brought to the
    /// state the settings ask for.
    pub fn request_apply(self: &Arc<Self>) {
        if !self.wanted() {
            self.cancel_work();
        }
        let manager = Arc::clone(self);
        cv_jobs::spawn(async move {
            manager.apply().await;
        });
    }

    /// Cancels a download or a start in progress and stops a running service.
    fn cancel_work(&self) {
        let (install, check, start) = {
            let mut state = lock(&self.state);
            (
                state.install_job.take(),
                state.check_job.take(),
                state.start_cancel.take(),
            )
        };
        if let Some(job) = install {
            job.cancel();
            self.tools.cancel_install();
        }
        if let Some(job) = check {
            job.cancel();
        }
        if let Some(cancel) = start {
            cancel.cancel();
        }
        self.stop_service();
    }

    /// Brings the service to the state the settings ask for. One run at a time.
    pub async fn apply(self: &Arc<Self>) {
        let _turn = self.apply_lock.lock().await;
        if !self.wanted() {
            self.cancel_work();
            self.settle_idle();
            return;
        }

        // The right version is on disk, or the download starts by itself.
        let mut status = self.tools.manager().status_of(ToolId::IndexerService);
        match status.state {
            ToolState::Unsupported => {
                self.set_failed("The indexer service is not available for this computer.");
                return;
            }
            ToolState::Ready => {}
            ToolState::NotInstalled | ToolState::Damaged | ToolState::OverrideMissing => {
                if let Err(reason) = self.install(None, INSTALL_JOB).await {
                    if self.wanted() {
                        self.set_failed(&reason);
                    }
                    return;
                }
                if !self.wanted() {
                    self.settle_idle();
                    return;
                }
                status = self.tools.manager().status_of(ToolId::IndexerService);
            }
        }
        let (Some(program), Some(version)) = (status.path.clone(), status.version.clone()) else {
            self.set_failed("The indexer service could not be found after it was installed.");
            return;
        };

        // A service that runs with another solver address is started again with the new one.
        let solver = self.solver_url();
        let restart = {
            let state = lock(&self.state);
            state.running.is_some() && state.solver != solver
        };
        if restart {
            self.stop_service();
        }
        let alive = {
            let mut state = lock(&self.state);
            let alive = state
                .running
                .as_mut()
                .is_some_and(|handle| handle.is_running());
            if !alive && state.running.is_some() {
                state.running = None;
                state.port = None;
            }
            alive
        };
        if !alive && !self.start(&program, &version, &solver).await {
            return;
        }
        if !self.wanted() {
            self.cancel_work();
            self.settle_idle();
            return;
        }
        self.sync_sources();
        self.check_if_due(&version);
        self.look_up_latest_if_due(&version).await;
    }

    async fn start(
        self: &Arc<Self>,
        program: &std::path::Path,
        version: &str,
        solver: &str,
    ) -> bool {
        let Some(http) = self.torrents.http_client() else {
            self.set_failed("The torrent sources could not start.");
            return false;
        };
        let cancel = CancelToken::new();
        {
            let mut state = lock(&self.state);
            state.state = Some(ServiceState::Starting);
            state.reason = None;
            state.start_cancel = Some(cancel.clone());
        }
        let install_dir = self
            .tools
            .manager()
            .root()
            .join(Package::IndexerService.as_str())
            .join(version);
        let solver_option = (!solver.is_empty()).then_some(solver);
        let params = StartParams {
            program,
            install_dir: &install_dir,
            data_dir: &self.data_dir,
            solver_url: solver_option,
            ready_timeout: self.ready_timeout,
        };
        let started = self.launcher.start(&params, &http, &cancel).await;
        let mut state = lock(&self.state);
        state.start_cancel = None;
        match started {
            Ok(handle) if self.wanted() => {
                state.port = Some(handle.port());
                state.api_key = Some(handle.api_key());
                state.solver = solver.to_owned();
                state.running = Some(handle);
                state.state = Some(ServiceState::Running);
                true
            }
            Ok(mut handle) => {
                // Switched off while it was starting.
                drop(state);
                handle.stop();
                self.settle_idle();
                false
            }
            Err(cv_torrent::indexer_service::ServiceError::Cancelled) => {
                drop(state);
                self.settle_idle();
                false
            }
            Err(error) => {
                tracing::warn!(%error, "the indexer service did not start");
                state.state = Some(ServiceState::Failed);
                state.reason = Some(error.to_string());
                false
            }
        }
    }

    fn set_failed(&self, reason: &str) {
        let mut state = lock(&self.state);
        state.state = Some(ServiceState::Failed);
        state.reason = Some(reason.to_owned());
    }

    /// The state when nothing runs and nothing is being installed: on disk or not.
    fn settle_idle(&self) {
        let installed =
            self.tools.manager().status_of(ToolId::IndexerService).state == ToolState::Ready;
        let mut state = lock(&self.state);
        state.state = Some(if installed {
            ServiceState::Installed
        } else {
            ServiceState::NotInstalled
        });
        state.reason = None;
    }

    // ---- installing -------------------------------------------------------------------------

    /// Installs the service (`spec` `None`: the pinned version) as an Activity job and waits for
    /// it. Returns the reason in plain words when it fails or is cancelled.
    async fn install(
        self: &Arc<Self>,
        spec: Option<PlatformSpec>,
        job_name: &str,
    ) -> Result<(), String> {
        {
            let mut state = lock(&self.state);
            state.state = Some(ServiceState::Downloading);
            state.reason = None;
        }
        let failure: Arc<Mutex<Option<String>>> = Arc::default();
        let work = InstallWork {
            tools: Arc::clone(&self.tools),
            spec,
            failure: Arc::clone(&failure),
        };
        let job = self.jobs.submit(
            JobSpec::new(job_name, LaneId::provider("indexer-service")).priority(Priority::Normal),
            work,
            vec![OneStep("Indexer service")],
        );
        lock(&self.state).install_job = Some(job.clone());
        let report = job.wait().await;
        lock(&self.state).install_job = None;
        let failed = report.snapshot.failed > 0 || report.snapshot.done == 0;
        if failed {
            let reason = lock(&failure)
                .take()
                .unwrap_or_else(|| "The download was cancelled.".to_owned());
            return Err(reason);
        }
        Ok(())
    }

    /// Looks up the newest release and installs it in place of the current version.
    ///
    /// # Errors
    /// [`IndexerError`]: the feature is off, the release cannot be trusted (no checksum, an
    /// address that is not the project's), nothing newer exists, or the install fails.
    pub async fn update(self: &Arc<Self>) -> Result<String, IndexerError> {
        self.torrents.ensure_accepted()?;
        let release = self.fetch_release().await?;
        let status = self.tools.manager().status_of(ToolId::IndexerService);
        let installed = status.version.clone().unwrap_or_default();
        let pinned = self
            .tools
            .manager()
            .pinned_spec(Package::IndexerService)
            .ok_or_else(|| {
                IndexerError::Failed("There is no download for this computer.".into())
            })?;
        let plan = plan_update(&release, &pinned, &installed, &self.download_prefix)?
            .ok_or(IndexerError::UpToDate)?;
        let installed_new = {
            let _turn = self.apply_lock.lock().await;
            // The program's files cannot be replaced while it runs.
            self.stop_service();
            let result = self.install(Some(plan.spec), UPDATE_JOB).await;
            if result.is_ok() {
                // A new version is checked again, and nothing newer is known until the next lookup.
                let _ = self.settings.reset(&[LAST_CHECK_KEY, LATEST_KEY]);
                lock(&self.state).latest = None;
            }
            result
        };
        // Either way the service comes back: the new version, or the old one that a failed
        // download leaves untouched.
        self.apply().await;
        installed_new.map_err(IndexerError::Failed)?;
        Ok(plan.version)
    }

    async fn fetch_release(&self) -> Result<Value, IndexerError> {
        let client = self
            .torrents
            .http_client()
            .ok_or_else(|| IndexerError::Failed("The torrent sources could not start.".into()))?;
        let request = HttpRequest::get("torrent:update", &self.releases_url)
            .header("Accept", "application/vnd.github+json")
            .uncached();
        let response = client.send(request).await.map_err(|error| {
            IndexerError::Failed(format!("The release could not be read: {error}"))
        })?;
        response
            .json::<Value>()
            .map_err(|_| IndexerError::Refused(UpdateRefused::Unreadable))
    }

    /// Looks up the newest release once a week and, when it is newer, says so once. Never
    /// installs anything.
    async fn look_up_latest_if_due(&self, installed: &str) {
        let last = self
            .settings
            .get(UPDATE_CHECKED_KEY)
            .and_then(|value| value.as_i64())
            .unwrap_or(0);
        if now_ms() - last < UPDATE_LOOKUP_AFTER_MS {
            return;
        }
        let Ok(release) = self.fetch_release().await else {
            return;
        };
        let _ = self.settings.set(UPDATE_CHECKED_KEY, Value::from(now_ms()));
        let Ok(version) = release_version(&release) else {
            return;
        };
        if !is_newer(&version, installed) {
            return;
        }
        let _ = self.settings.set(LATEST_KEY, Value::from(version.clone()));
        let announce = {
            let mut state = lock(&self.state);
            state.latest = Some(version.clone());
            let first = state.notified.as_deref() != Some(version.as_str());
            state.notified = Some(version.clone());
            first
        };
        if announce {
            self.notices.publish(
                AppNotice::new(NoticeLevel::Info, "A newer indexer service is available")
                    .detail(format!(
                        "Version {version} can be installed from Settings, Torrent sources."
                    ))
                    .source("Torrent search"),
            );
        }
    }

    // ---- running ----------------------------------------------------------------------------

    /// Stops the service and takes its indexers out of the search.
    fn stop_service(&self) {
        let running = {
            let mut state = lock(&self.state);
            state.port = None;
            state.api_key = None;
            state.running.take()
        };
        if let Some(mut handle) = running {
            handle.stop();
        }
        self.remove_all_sources();
    }

    /// Ends everything for good: the app is closing.
    pub fn shutdown(&self) {
        self.cancel_work();
    }

    fn running_info(&self) -> Option<(String, String)> {
        let state = lock(&self.state);
        let port = state.port?;
        let key = state.api_key.clone()?;
        Some((format!("http://127.0.0.1:{port}"), key))
    }

    // ---- the indexers -----------------------------------------------------------------------

    fn rows(&self) -> Vec<db_rows::TorrentIndexer> {
        self.db.read(db_rows::list).unwrap_or_else(|error| {
            tracing::warn!(%error, "the indexer list could not be read");
            Vec::new()
        })
    }

    /// The indexers as the last check left them, by name.
    #[must_use]
    pub fn indexers(&self) -> Vec<IndexerView> {
        self.rows()
            .into_iter()
            .map(|row| IndexerView {
                id: row.id,
                name: row.name,
                state: from_db_state(row.state),
                reason: row.reason,
                checked_at_ms: row.checked_at,
                user_off: row.user_off,
            })
            .collect()
    }

    /// Makes the sources of the search match the rows: one source per indexer that works and is
    /// not switched off, none for the others.
    fn sync_sources(&self) {
        for row in self.rows() {
            self.register(&IndexerRow {
                id: row.id,
                name: row.name,
                state: from_db_state(row.state),
                reason: row.reason,
                checked_at: row.checked_at,
                user_off: row.user_off,
            });
        }
    }

    /// Adds the indexer to the search when it works, takes it out otherwise.
    fn register(&self, row: &IndexerRow) {
        let source_id = format!("{ID_PREFIX}{}", row.id);
        let working = row.state == IndexerState::Working && !row.user_off;
        let info = self.running_info();
        let client = self.torrents.http_client();
        match (working, info, client) {
            (true, Some((base, key)), Some(client)) => {
                self.torrents.set_extra_source(Arc::new(IndexerSource::new(
                    client, &base, &key, &row.id, &row.name,
                )));
                lock(&self.state).registered.insert(source_id);
            }
            _ => {
                self.torrents.remove_extra_source(&source_id);
                lock(&self.state).registered.remove(&source_id);
            }
        }
    }

    /// Forgets the rows a finished check no longer lists (a site that left the service, went
    /// adult, became a built-in source or is not in a tracker language), and takes them out of the
    /// search. A person's own "off" is kept.
    fn prune_rows(&self, report: &ProvisionReport) {
        let listed: HashSet<&str> = report.rows.iter().map(|row| row.id.as_str()).collect();
        let stale: Vec<String> = self
            .rows()
            .into_iter()
            .filter(|row| !row.user_off && !listed.contains(row.id.as_str()))
            .map(|row| row.id)
            .collect();
        for id in stale {
            let source_id = format!("{ID_PREFIX}{id}");
            self.torrents.remove_extra_source(&source_id);
            lock(&self.state).registered.remove(&source_id);
            let row_id = id.clone();
            if let Err(error) = self.db.write(move |tx| db_rows::remove(tx, &row_id)) {
                tracing::warn!(%id, %error, "a stale indexer row was not removed");
            }
        }
    }

    fn remove_all_sources(&self) {
        let ids: Vec<String> = lock(&self.state).registered.drain().collect();
        for id in ids {
            self.torrents.remove_extra_source(&id);
        }
    }

    /// Switches one indexer on or off for good. Off takes it out of the search at once and out of
    /// the service; on checks it again.
    ///
    /// # Errors
    /// [`IndexerError`] while the feature is off or when the row cannot be saved.
    pub fn set_indexer_on(self: &Arc<Self>, id: &str, on: bool) -> Result<(), IndexerError> {
        self.torrents.ensure_accepted()?;
        let row_id = id.to_owned();
        self.db
            .write(move |tx| db_rows::set_user_off(tx, &row_id, !on))
            .map_err(|error| IndexerError::Failed(error.to_string()))?;
        if on {
            if self.running_info().is_some() {
                self.check_sources()?;
            }
            return Ok(());
        }
        self.torrents
            .remove_extra_source(&format!("{ID_PREFIX}{id}"));
        lock(&self.state)
            .registered
            .remove(&format!("{ID_PREFIX}{id}"));
        if let Some((base, _)) = self.running_info() {
            let (provisioner, passwords) = (
                Arc::clone(&self.provisioner),
                Arc::new(SecretPassword(Arc::clone(&self.secrets))),
            );
            let id = id.to_owned();
            cv_jobs::spawn(async move {
                if let Err(error) = provisioner.remove(&base, passwords, &id).await {
                    tracing::debug!(%error, "an indexer could not be removed from the service");
                }
            });
        }
        Ok(())
    }

    // ---- checking the indexers --------------------------------------------------------------

    fn check_if_due(self: &Arc<Self>, version: &str) {
        let last = self.settings.get(LAST_CHECK_KEY);
        let rows = self.rows();
        // Sites that were down or failed are tried again a day later, not a week.
        let wait = if rows
            .iter()
            .any(|row| !row.user_off && may_pass_later(row.state))
        {
            RETRY_FAILED_AFTER_MS
        } else {
            RECHECK_AFTER_MS
        };
        // A check that left no indexer behind (its results were lost, or the service's own files
        // were) is not a check worth waiting a week on.
        let due = rows.is_empty()
            || match last {
                Some(value) => {
                    let at = value.get("at").and_then(Value::as_i64).unwrap_or(0);
                    let was = value
                        .get("version")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    was != version || now_ms() - at >= wait
                }
                None => true,
            };
        if due {
            let _ = self.check_sources();
        }
    }

    /// Checks the indexers now, as the Activity job "Checking torrent sources". Does nothing when
    /// a check is already running.
    ///
    /// # Errors
    /// [`IndexerError::NotRunning`] when the service is not running.
    pub fn check_sources(self: &Arc<Self>) -> Result<(), IndexerError> {
        self.torrents.ensure_accepted()?;
        if self.running_info().is_none() {
            return Err(IndexerError::NotRunning);
        }
        let mut state = lock(&self.state);
        if let Some(job) = &state.check_job
            && !job.snapshot().state.is_terminal()
        {
            return Ok(());
        }
        let job = self.jobs.submit(
            JobSpec::new(CHECK_JOB, LaneId::provider("indexer-service")).priority(Priority::Low),
            CheckWork {
                manager: Arc::clone(self),
            },
            vec![OneStep("Torrent sources")],
        );
        state.check_job = Some(job);
        Ok(())
    }

    // ---- what Settings shows ----------------------------------------------------------------

    /// How the service stands.
    #[must_use]
    pub fn status(&self) -> ServiceStatus {
        let installed = self.tools.manager().status_of(ToolId::IndexerService);
        let rows = self.rows();
        let working = rows
            .iter()
            .filter(|row| row.state == db_rows::IndexerState::Working && !row.user_off)
            .count();
        let failed = rows
            .iter()
            .filter(|row| {
                matches!(
                    row.state,
                    db_rows::IndexerState::NeedsSolver
                        | db_rows::IndexerState::Unreachable
                        | db_rows::IndexerState::Error
                )
            })
            .count();
        let state = lock(&self.state);
        let version = (installed.state == ToolState::Ready)
            .then(|| installed.version.clone())
            .flatten();
        let current = state.state.unwrap_or(if version.is_some() {
            ServiceState::Installed
        } else {
            ServiceState::NotInstalled
        });
        // The version found by the last lookup, remembered across restarts; one that is not newer
        // than the installed one (installed since, by hand) is not announced.
        let latest = state
            .latest
            .clone()
            .or_else(|| {
                self.settings
                    .get(LATEST_KEY)
                    .and_then(|value| value.as_str().map(str::to_owned))
            })
            .filter(|latest| version.as_deref().is_some_and(|v| is_newer(latest, v)));
        ServiceStatus {
            state: current,
            version,
            latest_version: latest,
            port: state.port,
            working,
            failed,
            checking: state
                .check_job
                .as_ref()
                .is_some_and(|job| !job.snapshot().state.is_terminal()),
            reason: state.reason.clone(),
            default_languages: default_tracker_languages(&self.settings.values()),
            other_languages: read_other_languages(self.settings.get(OTHER_LANGUAGES_KEY).as_ref()),
        }
    }

    /// The address of the service's own page and the admin password to sign in with, for the
    /// person who wants to look at it.
    ///
    /// # Errors
    /// [`IndexerError::NotRunning`] when the service is not running.
    pub fn manager_access(&self) -> Result<(String, Option<String>), IndexerError> {
        let (base, _) = self.running_info().ok_or(IndexerError::NotRunning)?;
        let password = SecretPassword(Arc::clone(&self.secrets)).get();
        Ok((format!("{base}/UI/Dashboard"), password))
    }
}

/// The download of the service as a job of one step.
struct InstallWork {
    tools: Arc<ToolsService>,
    spec: Option<PlatformSpec>,
    failure: Arc<Mutex<Option<String>>>,
}

impl ItemWork for InstallWork {
    type Item = OneStep;

    async fn process(&self, _item: &OneStep, control: &ItemControl) -> Result<(), ItemError> {
        if control.is_cancelled() {
            return Err(ItemError::Cancelled);
        }
        let tools = Arc::clone(&self.tools);
        let spec = self.spec.clone();
        let work = tokio::task::spawn_blocking(move || match spec {
            Some(spec) => tools.install_spec(ToolId::IndexerService, &spec, &mut |_| {}),
            None => tools.install(ToolId::IndexerService, &mut |_| {}),
        });
        let result = tokio::select! {
            joined = work => joined,
            () = control.cancelled() => {
                self.tools.cancel_install();
                return Err(ItemError::Cancelled);
            }
        };
        match result {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => {
                if matches!(error.code(), "cancelled") {
                    return Err(ItemError::Cancelled);
                }
                let text = error.to_string();
                *lock(&self.failure) = Some(text.clone());
                Err(StageError::new(text).into())
            }
            Err(error) => {
                let text = format!("The download stopped unexpectedly: {error}");
                *lock(&self.failure) = Some(text.clone());
                Err(StageError::new(text).into())
            }
        }
    }
}

/// The check of the indexers as a job of one step.
struct CheckWork {
    manager: Arc<IndexerServiceManager>,
}

impl ItemWork for CheckWork {
    type Item = OneStep;

    async fn process(&self, _item: &OneStep, control: &ItemControl) -> Result<(), ItemError> {
        let manager = &self.manager;
        let Some((base, _)) = manager.running_info() else {
            return Err(StageError::new("The indexer service is not running").into());
        };
        let cancel = CancelToken::new();
        let watcher = {
            let (cancel, control) = (cancel.clone(), control.clone());
            tokio::spawn(async move {
                control.cancelled().await;
                cancel.cancel();
            })
        };
        // Forget the last check while this one runs: if the app ends before it is done (a crash,
        // the power), the next start finds the check due instead of waiting a week with half of
        // the indexers never looked at.
        let _ = manager.settings.reset(&[LAST_CHECK_KEY]);
        let wanted = Wanted {
            user_off: manager
                .rows()
                .into_iter()
                .filter(|row| row.user_off)
                .map(|row| row.id)
                .collect(),
            languages: tracker_languages(&manager.settings.values()),
        };
        let store: Arc<dyn IndexerStore> = Arc::new(DbIndexers(Arc::clone(&manager.db)));
        let passwords: Arc<dyn PasswordStore> =
            Arc::new(SecretPassword(Arc::clone(&manager.secrets)));
        let outcome = {
            let on_row: RowCallback = {
                let manager = Arc::clone(manager);
                Arc::new(move |row: &IndexerRow| manager.register(row))
            };
            let manager = Arc::clone(manager);
            manager
                .provisioner
                .run(&base, store, passwords, &wanted, &cancel, on_row)
                .await
        };
        watcher.abort();
        match outcome {
            Ok(report) => {
                manager.prune_rows(&report);
                let counts: serde_json::Map<String, Value> = report
                    .other_language_counts()
                    .into_iter()
                    .map(|(code, count)| (code, Value::from(count)))
                    .collect();
                let _ = manager
                    .settings
                    .set(OTHER_LANGUAGES_KEY, Value::Object(counts));
                let version = manager
                    .tools
                    .manager()
                    .status_of(ToolId::IndexerService)
                    .version
                    .unwrap_or_default();
                let _ = manager.settings.set(
                    LAST_CHECK_KEY,
                    serde_json::json!({ "at": now_ms(), "version": version }),
                );
                tracing::info!(
                    working = report.count(IndexerState::Working),
                    "the torrent sources were checked"
                );
                Ok(())
            }
            Err(ProvisionError::Cancelled) => Err(ItemError::Cancelled),
            Err(error) => {
                tracing::warn!(%error, "checking the torrent sources failed");
                Err(StageError::new(error.to_string()).into())
            }
        }
    }
}
