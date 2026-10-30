//! The Tauri surface of the update commands.
//!
//! The state, the DTOs and the command bodies live in [`cv_ipc::updates`]. This module supplies
//! the desktop platform (opener, quit, busy jobs) and the close hook.

use std::sync::Arc;

use cv_jobs::JobState;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;

use crate::boot::Boot;

pub use cv_ipc::updates::*;

use cv_app::updates::CloseUpdate;

struct TauriPlatform(AppHandle);

impl UpdatePlatform for TauriPlatform {
    fn open_url(&self, url: &str) -> Result<(), String> {
        self.0
            .opener()
            .open_url(url, None::<&str>)
            .map_err(|error| error.to_string())
    }

    fn open_path(&self, path: &std::path::Path) -> Result<(), String> {
        self.0
            .opener()
            .open_path(path.to_string_lossy().into_owned(), None::<&str>)
            .map_err(|error| error.to_string())
    }

    fn quit(&self) {
        self.0.exit(0);
    }

    fn busy_jobs(&self) -> u32 {
        let Some(boot) = self.0.try_state::<Boot>() else {
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
}

/// The main window was asked to close: with "Install updates when I close the app" on and a
/// verified update downloaded, starts its setup (checked once more first) and lets the close go
/// on.
pub fn install_on_close(app: &AppHandle) {
    let Some(state) = app.try_state::<UpdatesState>() else {
        return;
    };
    let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf))
    else {
        return;
    };
    match cv_ipc::updates::install_on_close(&state, &exe_dir) {
        CloseUpdate::Nothing => {}
        CloseUpdate::Started { version } => {
            tracing::info!(%version, "the update started on close");
        }
        CloseUpdate::Failed(error) => {
            tracing::warn!(%error, "the update was not installed on close");
        }
    }
}

/// The update settings and the offer found so far.
#[tauri::command]
#[specta::specta]
pub fn get_update_status(state: State<'_, UpdatesState>) -> UpdateStatusDto {
    cv_ipc::updates::get_update_status(&state)
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
    cv_ipc::updates::check_for_update(&state, manual).await
}

/// Jobs that are queued or running now. "Update after it finishes" waits until this is 0.
#[tauri::command]
#[specta::specta]
pub fn update_blocking_jobs(app: AppHandle) -> u32 {
    cv_ipc::updates::update_blocking_jobs(&TauriPlatform(app))
}

/// Installs the offered `version`. The user has confirmed. The installer is downloaded and its
/// size and SHA-256 are checked against the signed manifest. A copy the Windows setup installed
/// then runs the setup passively and quits (`restarting`); any other copy hands the installer to
/// the operating system and says why (`reason`). Without `stop_jobs`, a queued or running job
/// stops it with `jobs_running` before anything is started.
#[tauri::command]
#[specta::specta]
pub async fn install_update(
    app: AppHandle,
    state: State<'_, UpdatesState>,
    version: String,
    stop_jobs: bool,
    on_progress: tauri::ipc::Channel<UpdateProgressEvent>,
) -> Result<UpdateInstallDto, UpdateCommandError> {
    let platform: Arc<dyn UpdatePlatform> = Arc::new(TauriPlatform(app));
    cv_ipc::updates::install_update(
        &state,
        &platform,
        version,
        stop_jobs,
        crate::ipc_sink::sink(on_progress),
    )
    .await
}

/// Stops the running update download. Returns false when none is running.
#[tauri::command]
#[specta::specta]
pub fn cancel_update_download(state: State<'_, UpdatesState>) -> bool {
    cv_ipc::updates::cancel_update_download(&state)
}
