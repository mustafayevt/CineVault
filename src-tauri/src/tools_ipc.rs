//! The Tauri surface of the external tool commands.
//!
//! The state, the DTOs and the command bodies live in [`cv_ipc::tools`].

use tauri::State;
use tauri::ipc::Channel;

use crate::scan_ipc::ScanState;
use crate::settings_ipc::SettingsState;

pub use cv_ipc::tools::*;

/// Lists every tool with its state and version.
#[tauri::command]
#[specta::specta]
pub async fn list_tools(
    state: State<'_, ToolsState>,
) -> Result<Vec<ToolStatusDto>, ToolCommandError> {
    cv_ipc::tools::list_tools(&state).await
}

/// Sets the path to use for a tool (a file, or the folder that holds it), or clears it with
/// `None` so the downloaded copy is used again. Returns the updated list.
#[tauri::command]
#[specta::specta]
pub async fn set_tool_path(
    state: State<'_, ToolsState>,
    settings: State<'_, SettingsState>,
    tool: String,
    path: Option<String>,
) -> Result<Vec<ToolStatusDto>, ToolCommandError> {
    cv_ipc::tools::set_tool_path(&state, &settings, tool, path).await
}

/// Downloads the program that provides `tool`, checks its SHA-256 and installs it. Progress goes
/// to `on_progress`. Returns the updated list.
#[tauri::command]
#[specta::specta]
pub async fn install_tool(
    state: State<'_, ToolsState>,
    scan: State<'_, ScanState>,
    tool: String,
    on_progress: Channel<ToolInstallEvent>,
) -> Result<Vec<ToolStatusDto>, ToolCommandError> {
    cv_ipc::tools::install_tool(&state, &scan, tool, crate::ipc_sink::sink(on_progress)).await
}

/// Announces the tools the first-run setup is about to download, one after the other, so the
/// work that needs them (the media info of the first scan) waits for them instead of
/// reporting them missing. Unknown ids and ready tools are ignored.
#[tauri::command]
#[specta::specta]
pub fn expect_tools(state: State<'_, ToolsState>, tools: Vec<String>) {
    cv_ipc::tools::expect_tools(&state, tools)
}

/// Stops the running download. Returns false when none is running.
#[tauri::command]
#[specta::specta]
pub fn cancel_tool_install(state: State<'_, ToolsState>) -> bool {
    cv_ipc::tools::cancel_tool_install(&state)
}
