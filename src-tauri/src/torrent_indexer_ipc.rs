//! The Tauri surface of the managed indexer service commands.
//!
//! The DTOs and the command bodies live in [`cv_ipc::torrent_indexer`].

use tauri::State;

use crate::torrent_stream_ipc::{CommandResult, TorrentState};

pub use cv_ipc::torrent_indexer::*;

/// How the service stands now.
#[tauri::command]
#[specta::specta]
pub fn torrent_indexer_service_status(
    state: State<'_, TorrentState>,
) -> CommandResult<IndexerServiceStatusDto> {
    cv_ipc::torrent_indexer::torrent_indexer_service_status(&state)
}

/// Switches the indexer service on and makes sure it is downloaded and running.
#[tauri::command]
#[specta::specta]
pub fn torrent_install_indexer_service(
    state: State<'_, TorrentState>,
) -> CommandResult<IndexerServiceStatusDto> {
    cv_ipc::torrent_indexer::torrent_install_indexer_service(&state)
}

/// Installs the newest release of the indexer service when it publishes a checksum for it.
#[tauri::command]
#[specta::specta]
pub async fn torrent_update_indexer_service(
    state: State<'_, TorrentState>,
) -> CommandResult<IndexerServiceStatusDto> {
    cv_ipc::torrent_indexer::torrent_update_indexer_service(&state).await
}

/// Checks the indexers now.
#[tauri::command]
#[specta::specta]
pub fn torrent_check_sources(state: State<'_, TorrentState>) -> CommandResult<()> {
    cv_ipc::torrent_indexer::torrent_check_sources(&state)
}

/// Opens the service's own page in the browser and returns the admin password for the Copy
/// button.
#[tauri::command]
#[specta::specta]
pub fn torrent_open_indexer_manager(
    app: tauri::AppHandle,
    state: State<'_, TorrentState>,
) -> CommandResult<IndexerManagerAccessDto> {
    cv_ipc::torrent_indexer::torrent_open_indexer_manager(
        &state,
        &crate::torrent_ipc::OpenerHandoff(app),
    )
}

/// The indexers of the service with their health, by name.
#[tauri::command]
#[specta::specta]
pub fn torrent_indexers(state: State<'_, TorrentState>) -> CommandResult<Vec<IndexerDto>> {
    cv_ipc::torrent_indexer::torrent_indexers(&state)
}

/// Switches one indexer on or off for good and returns the list as it is now.
#[tauri::command]
#[specta::specta]
pub fn torrent_set_indexer(
    state: State<'_, TorrentState>,
    id: String,
    on: bool,
) -> CommandResult<Vec<IndexerDto>> {
    cv_ipc::torrent_indexer::torrent_set_indexer(&state, id, on)
}
