//! The Tauri surface of the selection actions.
//!
//! The DTOs and the command body live in [`cv_ipc::actions`].

use tauri::State;

use crate::automation_ipc::AutomationState;
use crate::boot::Boot;

pub use cv_ipc::actions::*;

/// Runs the action on the titles and resolves when it has ended. The job shows in the Activity
/// drawer while it runs. `action` is an id such as `download-trailer` or `reload-media-info`.
#[tauri::command]
#[specta::specta]
pub async fn run_item_action(
    boot: State<'_, Boot>,
    automation: State<'_, AutomationState>,
    ids: Vec<String>,
    action: String,
) -> Result<ItemActionResultDto, ItemActionCommandError> {
    cv_ipc::actions::run_item_action(boot.services(), automation.backend(), ids, action).await
}
