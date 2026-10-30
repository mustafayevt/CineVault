//! The Tauri surface of the library folder commands.
//!
//! The rules, the state and the DTOs live in [`cv_ipc::sources`]. Each function here is the thin
//! desktop wrapper.

use tauri::State;

pub use cv_ipc::sources::*;

/// Lists every library folder: movies first, then TV, each in the user's order.
#[tauri::command]
#[specta::specta]
pub fn list_data_sources(
    state: State<'_, LibraryState>,
) -> Result<Vec<DataSourceDto>, SourceCommandError> {
    cv_ipc::sources::list_data_sources(&state)
}

/// Adds a folder to the movie or TV library.
#[tauri::command]
#[specta::specta]
pub async fn add_data_source(
    state: State<'_, LibraryState>,
    kind: SourceKindDto,
    path: String,
) -> Result<DataSourceDto, SourceCommandError> {
    cv_ipc::sources::add_data_source(&state, kind, path).await
}

/// Removes a folder from the library list. Its titles stay in the library.
#[tauri::command]
#[specta::specta]
pub async fn remove_data_source(
    state: State<'_, LibraryState>,
    id: u32,
) -> Result<RemovedSourceDto, SourceCommandError> {
    cv_ipc::sources::remove_data_source(&state, id).await
}

/// Sets the order of one library's folders; `ids` lists every folder of `kind` once.
#[tauri::command]
#[specta::specta]
pub fn reorder_data_sources(
    state: State<'_, LibraryState>,
    kind: SourceKindDto,
    ids: Vec<u32>,
) -> Result<Vec<DataSourceDto>, SourceCommandError> {
    cv_ipc::sources::reorder_data_sources(&state, kind, ids)
}

/// Renames a folder's label ("NAS 4K"). It touches no file and scans nothing; a label another
/// folder of the same library already uses is refused.
#[tauri::command]
#[specta::specta]
pub fn rename_data_source_label(
    state: State<'_, LibraryState>,
    id: u32,
    label: String,
) -> Result<DataSourceDto, SourceCommandError> {
    cv_ipc::sources::rename_data_source_label(&state, id, label)
}

/// Moves a folder to the other library type (Movies or TV shows). No title is deleted or
/// changed; the caller scans the folder again so its titles are read as the new type.
#[tauri::command]
#[specta::specta]
pub async fn change_data_source_kind(
    state: State<'_, LibraryState>,
    id: u32,
    kind: SourceKindDto,
) -> Result<DataSourceDto, SourceCommandError> {
    cv_ipc::sources::change_data_source_kind(&state, id, kind).await
}

/// Moves a folder to a new path and re-points its titles, without scraping again.
#[tauri::command]
#[specta::specta]
pub async fn remap_data_source(
    state: State<'_, LibraryState>,
    id: u32,
    path: String,
) -> Result<RemapSourceDto, SourceCommandError> {
    cv_ipc::sources::remap_data_source(&state, id, path).await
}
