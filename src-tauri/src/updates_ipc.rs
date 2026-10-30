//! Typed commands for application updates: the status for Settings and the notice, a check, and
//! the confirmed install (download, verify, then run the setup passively and quit, or hand the
//! installer to the operating system).
//!
//! The rules live in `cv_app::updates`. The UI never sends an address: installing names the
//! version of the offer a verified manifest produced, and the service downloads only that.
//!
//! Install on close (Settings, Updates, "Install updates when I close the app", off by default):
//! when it is on, a check that finds an update also downloads and verifies it in the background,
//! and the close request of the main window ([`install_on_close`]) starts the same verified setup
//! the confirm button starts, then lets the window close. The setup waits for the app to exit,
//! installs and starts the new version. A Linux AppImage replaces itself the same way: the
//! verified download is put over the AppImage file and a helper starts it once the app has
//! exited. Nothing is asked, and nothing happens for a portable copy, a package install or a Mac.
//!
//! "Update now" on a copy the Windows setup installed: the verified setup starts with fixed
//! switches (`cv_app::updates::silent_setup_arguments`), and the app quits about a second later
//! through `AppHandle::exit`, the same shutdown as closing the window (jobs paused, services
//! stopped, the library closed). The setup waits for the app to exit, installs over it and
//! starts the new version. A running job first asks the user (`jobs_running`).
//!
//! "Update now" on a Linux AppImage whose folder takes a new file: the verified download is
//! copied beside the AppImage, checked, and renamed over it in one step
//! (`cv_app::updates::replace_appimage_and_relaunch`); the old file is untouched until then, so
//! a failure leaves a working app. A helper starts the new AppImage after this process has
//! exited, and the app quits through the same shutdown as above. A package install (`.deb`,
//! `.rpm`) opens the public release page instead.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use cv_app::data_location::DataLocation;
use cv_app::updates::{
    CheckOutcome, CloseUpdate, InstallMethod, OpenReason, UpdateError, UpdateOffer, UpdateService,
    VerifiedInstaller, install_method_for_current, install_on_close_with, start_silent_setup,
};
use cv_jobs::JobState;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::ipc::Channel;
use tauri::{Manager, State};
use tauri_plugin_opener::OpenerExt;

use crate::boot::Boot;
use crate::settings_ipc::SettingsState;

/// Where a package install (`.deb`, `.rpm`) finds the new version.
const RELEASES_PAGE: &str = "https://github.com/mustafayevt/CineVault/releases/latest";

/// Held in Tauri state.
pub struct UpdatesState {
    service: Arc<UpdateService>,
    cancel: Arc<AtomicBool>,
    installing: AtomicBool,
    /// An update is being downloaded in the background for "install when I close".
    preparing: Arc<AtomicBool>,
}

impl UpdatesState {
    /// Updates read their settings from the app's settings and download into the app's own temp
    /// folder (`tmp/updates` in the data folder). Installers of earlier updates are swept at
    /// start.
    pub fn new(settings: &SettingsState) -> Self {
        let location = DataLocation::resolve_for_process();
        let downloads = location.as_ref().map_or_else(
            || std::env::temp_dir().join("cinevault-updates"),
            |location| location.tmp_dir().join("updates"),
        );
        let service = Arc::new(UpdateService::new(
            settings.shared(),
            env!("CARGO_PKG_VERSION"),
            downloads,
        ));
        // Off the window thread: the data folder may be on a slow drive.
        let sweeping = Arc::clone(&service);
        std::thread::spawn(move || {
            sweeping.sweep_downloads();
            // A half-made copy of an interrupted AppImage update sits beside the AppImage.
            #[cfg(unix)]
            if let Some(folder) = std::env::var_os("APPIMAGE")
                .map(std::path::PathBuf::from)
                .and_then(|path| path.parent().map(std::path::Path::to_path_buf))
            {
                cv_app::updates::remove_stale_parts(&folder);
            }
            // Before the downloads moved to `tmp`, they were kept in the cache folder.
            if let Some(location) = location {
                let old = location.cache_dir().join("updates");
                if old.is_dir()
                    && let Err(error) = std::fs::remove_dir_all(&old)
                {
                    tracing::debug!(%error, "old update downloads stay");
                }
            }
        });
        Self {
            service,
            cancel: Arc::new(AtomicBool::new(false)),
            installing: AtomicBool::new(false),
            preparing: Arc::new(AtomicBool::new(false)),
        }
    }

    /// With "Install updates when I close the app" on and an update on offer, downloads and
    /// verifies it in the background so closing the app can install it. Cheap and safe to call
    /// often: it does nothing when the setting is off, nothing is on offer, the installer is
    /// ready already, or a download is running.
    fn prepare_for_close(&self) {
        let service = &self.service;
        if !service.install_on_close() || self.installing.load(Ordering::SeqCst) {
            return;
        }
        let Some(offer) = service.offer() else {
            return;
        };
        if service.ready_version().as_deref() == Some(offer.version.as_str()) {
            return;
        }
        if self.preparing.swap(true, Ordering::SeqCst) {
            return;
        }
        let service = Arc::clone(service);
        let preparing = Arc::clone(&self.preparing);
        std::thread::spawn(move || {
            let cancel = AtomicBool::new(false);
            match service.download(&offer.version, &cancel, &mut |_| {}) {
                Ok(_) => tracing::info!(
                    version = %offer.version,
                    "the update is downloaded and waits for the app to close"
                ),
                Err(error) => tracing::warn!(%error, "the update could not be prepared for close"),
            }
            preparing.store(false, Ordering::SeqCst);
        });
    }
}

/// The main window was asked to close: with "Install updates when I close the app" on and a
/// verified update downloaded, starts its setup (checked once more first) and lets the close go
/// on. Quick: the setup is only started here, not waited for. A Linux AppImage is replaced here
/// instead (a copy of the download, so it takes a moment), and a helper starts it after exit.
pub fn install_on_close(app: &tauri::AppHandle) {
    let Some(state) = app.try_state::<UpdatesState>() else {
        return;
    };
    // A confirmed install is under way, or the setup was started already.
    if state.installing.load(Ordering::SeqCst) {
        return;
    }
    let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf))
    else {
        return;
    };
    match install_on_close_with(
        &state.service,
        &exe_dir,
        &install_method_for_current,
        &start_silent_setup,
        &replace_and_relaunch,
    ) {
        CloseUpdate::Nothing => {}
        CloseUpdate::Started { version } => {
            state.installing.store(true, Ordering::SeqCst);
            tracing::info!(%version, "the update started on close");
        }
        CloseUpdate::Failed(error) => {
            tracing::warn!(%error, "the update was not installed on close");
        }
    }
}

/// Puts the verified download over the AppImage at `target` and has the new one started after
/// this process exits. Only a Linux copy is ever given an AppImage to replace.
fn replace_and_relaunch(
    installer: &VerifiedInstaller,
    target: &std::path::Path,
) -> Result<(), UpdateError> {
    #[cfg(unix)]
    {
        cv_app::updates::replace_appimage_and_relaunch(installer, target)
    }
    #[cfg(not(unix))]
    {
        let _ = (installer, target);
        Err(UpdateError::Io(
            "this system has no AppImage to replace".to_owned(),
        ))
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
        })
}

fn small(n: u64) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// A newer release, as the notice shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct UpdateOfferDto {
    pub version: String,
    pub current: String,
    /// When the release was published, as the manifest wrote it.
    pub published: Option<String>,
    /// Release notes from the signed manifest, plain text or Markdown.
    pub notes: String,
    pub size_bytes: u32,
}

impl From<UpdateOffer> for UpdateOfferDto {
    fn from(offer: UpdateOffer) -> Self {
        Self {
            version: offer.version,
            current: offer.current,
            published: offer.published,
            notes: offer.notes,
            size_bytes: small(offer.size_bytes),
        }
    }
}

/// What Settings and the shell need to know.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct UpdateStatusDto {
    pub current_version: String,
    /// A channel is configured: the address in Settings, else the built-in channel.
    pub channel_set: bool,
    pub enabled: bool,
    /// The offer found earlier in this session, if any.
    pub offer: Option<UpdateOfferDto>,
}

/// How a check ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum UpdateOutcomeDto {
    /// Updates are off (or this build has no channel); nothing was requested.
    Disabled,
    /// The last check is recent; nothing was requested.
    NotDue,
    UpToDate,
    Available,
}

/// The answer to a check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct UpdateCheckDto {
    pub outcome: UpdateOutcomeDto,
    pub offer: Option<UpdateOfferDto>,
}

impl From<CheckOutcome> for UpdateCheckDto {
    fn from(outcome: CheckOutcome) -> Self {
        let (outcome, offer) = match outcome {
            CheckOutcome::Disabled => (UpdateOutcomeDto::Disabled, None),
            CheckOutcome::NotDue => (UpdateOutcomeDto::NotDue, None),
            CheckOutcome::UpToDate => (UpdateOutcomeDto::UpToDate, None),
            CheckOutcome::Available(offer) => (UpdateOutcomeDto::Available, Some(offer.into())),
        };
        Self { outcome, offer }
    }
}

/// Why a command did not do what was asked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct UpdateCommandError {
    /// Stable code: `no_channel`, `insecure`, `bad_url`, `no_key`, `bad_key`, `network`, `bad_signature`,
    /// `bad_manifest`, `no_build`, `nothing_offered`, `offer_changed`, `cancelled`, `checksum`,
    /// `size`, `download`, `storage`, `settings`, `busy`, `launch` or `jobs_running` (a job is
    /// queued or running and the user has not said to stop it).
    pub code: String,
    /// Plain sentence for the user.
    pub message: String,
}

impl From<UpdateError> for UpdateCommandError {
    fn from(error: UpdateError) -> Self {
        Self {
            code: error.code().to_owned(),
            message: error.to_string(),
        }
    }
}

fn unexpected(error: impl std::fmt::Display) -> UpdateCommandError {
    UpdateCommandError {
        code: "storage".to_owned(),
        message: format!("The update stopped unexpectedly: {error}"),
    }
}

/// How the confirmed update went on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct UpdateInstallDto {
    /// The setup runs by itself and the app quits in about a second; the new version starts
    /// when the setup is done.
    pub restarting: bool,
    /// When the installer was handed to the operating system instead: why, in a plain sentence.
    pub reason: Option<String>,
}

/// How long the app waits after starting the setup before it quits, so the page can say what
/// happens.
const QUIT_DELAY: Duration = Duration::from_millis(1200);

/// Jobs that are queued or running, which quitting would interrupt.
fn busy_jobs(app: &tauri::AppHandle) -> u32 {
    let Some(boot) = app.try_state::<Boot>() else {
        return 0;
    };
    let Some(services) = boot.services() else {
        return 0;
    };
    let busy = services
        .jobs()
        .list()
        .iter()
        .filter(|job| matches!(job.state, JobState::Running | JobState::Queued))
        .count();
    u32::try_from(busy).unwrap_or(u32::MAX)
}

fn jobs_running(count: u32) -> UpdateCommandError {
    UpdateCommandError {
        code: "jobs_running".to_owned(),
        message: if count == 1 {
            "A job is running.".to_owned()
        } else {
            format!("{count} jobs are running.")
        },
    }
}

/// Download progress, sent over a channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct UpdateProgressEvent {
    pub downloaded: u32,
    pub total: u32,
}

/// At most this often during a download.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

/// The update settings and the offer found so far.
#[tauri::command]
#[specta::specta]
pub fn get_update_status(state: State<'_, UpdatesState>) -> UpdateStatusDto {
    // The page reads the status after the user turned "install when I close" on: an update that
    // is on offer starts downloading then.
    state.prepare_for_close();
    let config = state.service.config();
    UpdateStatusDto {
        current_version: env!("CARGO_PKG_VERSION").to_owned(),
        channel_set: config.channel.is_some(),
        enabled: config.enabled,
        offer: state.service.offer().map(Into::into),
    }
}

/// Looks for an update. With `manual` false this is the background check: it does nothing unless
/// updates are on, a channel is set and the interval has passed. With `manual` true it needs a
/// channel and asks it now.
#[tauri::command]
#[specta::specta]
pub async fn check_for_update(
    state: State<'_, UpdatesState>,
    manual: bool,
) -> Result<UpdateCheckDto, UpdateCommandError> {
    let service = Arc::clone(&state.service);
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        if manual {
            service.check_now(now())
        } else {
            service.check_if_due(now())
        }
    })
    .await
    .map_err(unexpected)??;
    if matches!(outcome, CheckOutcome::Available(_)) {
        state.prepare_for_close();
    }
    Ok(outcome.into())
}

/// Jobs that are queued or running now. "Update after it finishes" waits until this is 0.
#[tauri::command]
#[specta::specta]
pub fn update_blocking_jobs(app: tauri::AppHandle) -> u32 {
    busy_jobs(&app)
}

/// Installs the offered `version`. The user has confirmed. The installer is downloaded and its
/// size and SHA-256 are checked against the signed manifest. A copy the Windows setup installed
/// then runs the setup passively and quits (`restarting`); any other copy hands the installer to
/// the operating system and says why (`reason`). Without `stop_jobs`, a queued or running job
/// stops it with `jobs_running` before anything is started.
#[tauri::command]
#[specta::specta]
pub async fn install_update(
    app: tauri::AppHandle,
    state: State<'_, UpdatesState>,
    version: String,
    stop_jobs: bool,
    on_progress: Channel<UpdateProgressEvent>,
) -> Result<UpdateInstallDto, UpdateCommandError> {
    if !stop_jobs {
        let busy = busy_jobs(&app);
        if busy > 0 {
            return Err(jobs_running(busy));
        }
    }
    if state.installing.swap(true, Ordering::SeqCst) {
        return Err(UpdateCommandError {
            code: "busy".to_owned(),
            message: "An update is already being downloaded.".to_owned(),
        });
    }
    let result = download_and_start(
        &app,
        &state,
        version,
        stop_jobs,
        crate::ipc_sink::sink(on_progress),
    )
    .await;
    // Once the setup runs, the app is quitting: a second "Update now" must not start another.
    if !matches!(
        result,
        Ok(UpdateInstallDto {
            restarting: true,
            ..
        })
    ) {
        state.installing.store(false, Ordering::SeqCst);
    }
    if let Err(error) = &result {
        tracing::warn!(code = %error.code, message = %error.message, "the update did not start");
    }
    result
}

async fn download_and_start(
    app: &tauri::AppHandle,
    state: &UpdatesState,
    version: String,
    stop_jobs: bool,
    on_progress: cv_ipc::sink::Sink<UpdateProgressEvent>,
) -> Result<UpdateInstallDto, UpdateCommandError> {
    state.cancel.store(false, Ordering::SeqCst);
    let service = Arc::clone(&state.service);
    let cancel = Arc::clone(&state.cancel);
    let installer = tauri::async_runtime::spawn_blocking(move || {
        let mut last = Instant::now().checked_sub(PROGRESS_INTERVAL);
        let total = service.offer().map_or(0, |offer| small(offer.size_bytes));
        service.download(&version, &cancel, &mut |downloaded| {
            if last.is_none_or(|at| at.elapsed() >= PROGRESS_INTERVAL) {
                last = Some(Instant::now());
                // A closed channel (the page was left) must not stop the download.
                on_progress(UpdateProgressEvent {
                    downloaded: small(downloaded),
                    total,
                });
            }
        })
    })
    .await
    .map_err(unexpected)??;
    // A job may have started during the download.
    if !stop_jobs {
        let busy = busy_jobs(app);
        if busy > 0 {
            return Err(jobs_running(busy));
        }
    }
    let method = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf))
        .map_or(
            InstallMethod::OpenInstaller(OpenReason::NotSetupInstall),
            |dir| install_method_for_current(&dir, installer.path()),
        );
    match method {
        InstallMethod::SilentSetup { install_dir } => {
            tauri::async_runtime::spawn_blocking(move || {
                start_silent_setup(&installer, &install_dir)
            })
            .await
            .map_err(unexpected)??;
            tracing::info!("the update setup started; CineVault quits for it");
            Ok(quit_for_update(app))
        }
        InstallMethod::OpenInstaller(OpenReason::LinuxPackage) => {
            // A package is installed by the system's package tool, so the release page is what
            // helps; the downloaded file is not opened.
            let reason = OpenReason::LinuxPackage;
            tracing::info!(
                reason = reason.code(),
                "the update is a package install; the release page is opened"
            );
            app.opener()
                .open_url(RELEASES_PAGE, None::<&str>)
                .map_err(|error| UpdateCommandError {
                    code: "launch".to_owned(),
                    message: format!(
                        "The release page {RELEASES_PAGE} could not be opened: {error}"
                    ),
                })?;
            Ok(UpdateInstallDto {
                restarting: false,
                reason: Some(reason.message().to_owned()),
            })
        }
        InstallMethod::OpenInstaller(reason) => {
            let path = installer.path().to_path_buf();
            // Checked once more right before the system opens it.
            drop(installer.reverify()?);
            tracing::info!(
                reason = reason.code(),
                installer = %path.display(),
                "the update installer is handed to the operating system"
            );
            app.opener()
                .open_path(path.to_string_lossy().into_owned(), None::<&str>)
                .map_err(|error| UpdateCommandError {
                    code: "launch".to_owned(),
                    message: format!(
                        "The update was downloaded to {} but its installer could not be started: {error}",
                        path.display()
                    ),
                })?;
            Ok(UpdateInstallDto {
                restarting: false,
                reason: Some(reason.message().to_owned()),
            })
        }
        InstallMethod::ReplaceAppImage { target } => {
            tracing::info!(
                version = %installer.version(),
                target = %target.display(),
                "installing the update over the AppImage"
            );
            // A failure here leaves the old AppImage as it was and the app running; the page
            // shows the message and the update stays on offer.
            tauri::async_runtime::spawn_blocking(move || replace_and_relaunch(&installer, &target))
                .await
                .map_err(unexpected)??;
            tracing::info!("the AppImage was replaced; CineVault quits and the new one starts");
            Ok(quit_for_update(app))
        }
    }
}

/// Quits the app a moment from now, through the same shutdown as closing the window, so the
/// reply reaches the page first. Whatever took over (the setup, or the helper that starts the
/// new AppImage) waits for this process to exit.
fn quit_for_update(app: &tauri::AppHandle) -> UpdateInstallDto {
    let quitting = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(QUIT_DELAY).await;
        // The same shutdown as closing the window (`RunEvent::Exit` in main).
        quitting.exit(0);
    });
    UpdateInstallDto {
        restarting: true,
        reason: None,
    }
}

/// Stops the running update download. Returns false when none is running.
#[tauri::command]
#[specta::specta]
pub fn cancel_update_download(state: State<'_, UpdatesState>) -> bool {
    if state.installing.load(Ordering::SeqCst) {
        state.cancel.store(true, Ordering::SeqCst);
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_offer_converts_with_its_notes_and_size() {
        let dto = UpdateOfferDto::from(UpdateOffer {
            version: "1.2.0".to_owned(),
            current: "1.0.0".to_owned(),
            published: None,
            notes: "Faster".to_owned(),
            size_bytes: u64::MAX,
        });
        assert_eq!(dto.version, "1.2.0");
        assert_eq!(dto.notes, "Faster");
        assert_eq!(dto.size_bytes, u32::MAX);
    }

    #[test]
    fn outcomes_convert_and_errors_carry_a_code_and_a_sentence() {
        assert_eq!(
            UpdateCheckDto::from(CheckOutcome::UpToDate).outcome,
            UpdateOutcomeDto::UpToDate
        );
        let error = UpdateCommandError::from(UpdateError::BadSignature);
        assert_eq!(error.code, "bad_signature");
        assert!(error.message.contains("not signed by the expected key"));
    }
}
