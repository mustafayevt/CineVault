//! The drives of the library (spec `drives`): `drives_overview` and the typed
//! `DrivesChanged` event.
//!
//! The rules live in `cv_app::drives`. The event tells the sidebar and the Drives page that the
//! numbers moved (a scan, a move or a delete ended, a drive came or went); they read the overview
//! again. Announcements are joined by the service, so a scan does not make the event a stream.

use std::sync::Arc;

use cv_app::changes::ChangePart;
use cv_app::drives::{DrivesService, EmptyError};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, State};
use tauri_specta::Event;

use crate::boot::Boot;

/// Held in Tauri state. When the app services could not start, the overview is empty.
pub struct DrivesState {
    service: Option<Arc<DrivesService>>,
}

impl DrivesState {
    /// Builds the service over the app services and starts telling the interface when the numbers
    /// move: whenever the files or stages of titles change (a scan, a move, a delete) and whenever
    /// a drive comes or goes.
    pub fn from_boot(boot: &Boot, app: &AppHandle) -> Self {
        let service = boot.services().map(|services| {
            let drives = DrivesService::new(Arc::clone(services.db()), Arc::clone(services.scan()));
            // What CineVault sent to the Recycle Bin, and Settings › Deleted files.
            let choice: Arc<dyn cv_app::deleted_files::DeletedFilesSource> =
                Arc::clone(services.settings()) as _;
            drives.set_trash(
                cv_app::library_ops::PolicyBin::os(Arc::clone(&choice)),
                choice,
            );
            // "Empty trash folders automatically": at start-up and then once a day, only while
            // the setting is on, only CineVault's own trash folders.
            let weak = Arc::downgrade(&drives);
            std::thread::Builder::new()
                .name("trash-auto-empty".to_owned())
                .spawn(move || {
                    loop {
                        let Some(service) = weak.upgrade() else { break };
                        match service.auto_empty() {
                            Ok(Some(report)) if report.batches > 0 => tracing::info!(
                                batches = report.batches,
                                bytes = report.bytes,
                                "emptied old trash folder batches"
                            ),
                            Ok(_) => {}
                            Err(error) => {
                                tracing::warn!(%error, "the trash folders were not emptied")
                            }
                        }
                        drop(service);
                        std::thread::sleep(std::time::Duration::from_secs(24 * 60 * 60));
                    }
                })
                .ok();
            let handle = app.clone();
            drives.on_changed(Arc::new(move || {
                if let Err(error) = (DrivesChanged { at_ms: now_ms() }).emit(&handle) {
                    tracing::warn!(%error, "could not emit the DrivesChanged event");
                }
            }));
            let relay = Arc::clone(&drives);
            services.changes().subscribe(Arc::new(move |change| {
                if change
                    .parts
                    .iter()
                    .any(|part| matches!(part, ChangePart::Files | ChangePart::Stage))
                {
                    relay.notify_changed();
                }
            }));
            drives
        });
        Self { service }
    }
}

#[allow(clippy::cast_precision_loss)]
fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_millis() as f64)
}

/// The numbers of the drives moved: read the overview again.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type, Event)]
pub struct DrivesChanged {
    /// When it was announced, epoch milliseconds.
    pub at_ms: f64,
}

pub use cv_ipc::drives::*;

/// The drives that hold a library folder: name, kind, size and free space, the folders on each
/// with their titles and bytes, the space the library does not use, the resolution mix, whether
/// the drive answers and when it last did. Reads the disks.
#[tauri::command]
#[specta::specta]
pub async fn drives_overview(
    state: State<'_, DrivesState>,
) -> Result<DrivesOverviewDto, DrivesCommandError> {
    cv_ipc::drives::drives_overview(state.service.clone()).await
}

/// What Empty… would erase on the drive `drive_id` (every drive when `None`): the batches of its
/// trash folders and what CineVault sent to the Recycle Bin from it, biggest first.
#[tauri::command]
#[specta::specta]
pub async fn drives_trash_contents(
    state: State<'_, DrivesState>,
    drive_id: Option<String>,
) -> Result<TrashContentsDto, DrivesCommandError> {
    cv_ipc::drives::drives_trash_contents(state.service.clone(), drive_id).await
}

/// What an Empty did.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EmptyReportDto {
    pub bytes: f64,
    pub batches: u32,
    pub bin_items: u32,
    /// Paths that could not be erased, each with the reason.
    pub problems: Vec<String>,
    /// History entries whose Undo was switched off.
    pub entries: u32,
}
/// Empties the trash of the drive `drive_id` (every drive when `None`): erases its trash folders'
/// batches and what CineVault sent to the Recycle Bin from it. `confirm` must be the literal
/// `DELETE` the person typed. History entries whose files were erased lose their Undo.
#[tauri::command]
#[specta::specta]
pub async fn empty_drives_trash(
    state: State<'_, DrivesState>,
    drive_id: Option<String>,
    confirm: String,
) -> Result<EmptyReportDto, DrivesCommandError> {
    let Some(service) = state.service.clone() else {
        return Err(storage("The library is not open.".to_owned()));
    };
    let report = tauri::async_runtime::spawn_blocking(move || {
        service.empty_trash(drive_id.as_deref(), &confirm)
    })
    .await
    .map_err(|error| storage(error.to_string()))?
    .map_err(|error| DrivesCommandError {
        code: match error {
            EmptyError::Confirmation => "confirmation",
            EmptyError::Db(_) => "storage",
        }
        .to_owned(),
        message: error.to_string(),
    })?;
    Ok(EmptyReportDto {
        bytes: exact(report.bytes),
        batches: u32::try_from(report.batches).unwrap_or(u32::MAX),
        bin_items: u32::try_from(report.bin_items).unwrap_or(u32::MAX),
        problems: report
            .problems
            .into_iter()
            .map(|(path, why)| {
                if path.is_empty() {
                    why
                } else {
                    format!("{path}: {why}")
                }
            })
            .collect(),
        entries: u32::try_from(report.entries.len()).unwrap_or(u32::MAX),
    })
}
