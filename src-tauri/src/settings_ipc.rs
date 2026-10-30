//! The Tauri surface of the settings commands.
//!
//! The store, the state and the DTOs live in [`cv_ipc::settings`].

use tauri::State;

pub use cv_ipc::settings::*;

/// Returns the stored settings (those that differ from their defaults).
#[tauri::command]
#[specta::specta]
pub fn get_settings(state: State<'_, SettingsState>) -> SettingsSnapshotDto {
    cv_ipc::settings::get_settings(&state)
}

/// Saves one setting. `value_json` is the JSON text of the new value.
#[tauri::command]
#[specta::specta]
pub async fn set_setting(
    state: State<'_, SettingsState>,
    key: String,
    value_json: String,
) -> Result<(), String> {
    cv_ipc::settings::set_setting(&state, key, value_json).await
}

/// Returns the given settings to their defaults.
#[tauri::command]
#[specta::specta]
pub async fn reset_settings(
    state: State<'_, SettingsState>,
    keys: Vec<String>,
) -> Result<(), String> {
    cv_ipc::settings::reset_settings(&state, keys).await
}
