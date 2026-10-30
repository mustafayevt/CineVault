//! The Tauri surface of the edit commands.
//!
//! The state, the DTOs and the command bodies live in [`cv_ipc::edit`].

use tauri::State;

use crate::nfo_ipc::NfoState;
use crate::settings_ipc::SettingsState;
use crate::sources_ipc::LibraryState;

pub use cv_ipc::edit::*;

/// Every editable field of an item with its lock key, the locked fields and the item lock.
#[tauri::command]
#[specta::specta]
pub async fn get_item_edit(
    library: State<'_, LibraryState>,
    nfo: State<'_, NfoState>,
    edit: State<'_, EditState>,
    id: String,
) -> Result<ItemEditDto, EditCommandError> {
    cv_ipc::edit::get_item_edit(&library, &nfo, &edit, id).await
}

/// Saves the given fields of an item and rewrites its NFO files. Nothing is saved when any value
/// is refused. The reply holds the previous values for Undo.
#[tauri::command]
#[specta::specta]
pub async fn save_item_edit(
    library: State<'_, LibraryState>,
    nfo: State<'_, NfoState>,
    edit: State<'_, EditState>,
    settings: State<'_, SettingsState>,
    id: String,
    changes: Vec<EditFieldDto>,
) -> Result<EditSavedDto, EditCommandError> {
    cv_ipc::edit::save_item_edit(&library, &nfo, &edit, &settings, id, changes).await
}

/// Locks or unlocks one field against scraping and NFO re-reading. `field` is a lock key.
#[tauri::command]
#[specta::specta]
pub async fn set_field_lock(
    library: State<'_, LibraryState>,
    nfo: State<'_, NfoState>,
    edit: State<'_, EditState>,
    id: String,
    field: String,
    locked: bool,
) -> Result<(), EditCommandError> {
    cv_ipc::edit::set_field_lock(&library, &nfo, &edit, id, field, locked).await
}

/// Locks or unlocks the whole item.
#[tauri::command]
#[specta::specta]
pub async fn set_item_lock(
    library: State<'_, LibraryState>,
    nfo: State<'_, NfoState>,
    edit: State<'_, EditState>,
    id: String,
    locked: bool,
) -> Result<(), EditCommandError> {
    cv_ipc::edit::set_item_lock(&library, &nfo, &edit, id, locked).await
}
