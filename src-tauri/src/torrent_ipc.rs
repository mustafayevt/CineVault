//! The Tauri surface of the torrent hand-off and source settings.
//!
//! The rules, the DTOs and the command bodies live in [`cv_ipc::torrent`]. This module keeps the
//! Tauri opener adapter and the thin wrappers.

use std::path::Path;

use tauri::State;
use tauri_plugin_opener::OpenerExt;

use crate::torrent_stream_ipc::{CommandResult, TorrentState};

pub use cv_ipc::torrent::*;

/// The Tauri opener as a [`Handoff`].
pub struct OpenerHandoff<R: tauri::Runtime>(pub tauri::AppHandle<R>);

impl<R: tauri::Runtime> Handoff for OpenerHandoff<R> {
    fn open_url(&self, url: &str) -> Result<(), String> {
        self.0
            .opener()
            .open_url(url, None::<&str>)
            .map_err(|error| error.to_string())
    }

    fn open_path(&self, path: &Path) -> Result<(), String> {
        self.0
            .opener()
            .open_path(path.to_string_lossy().into_owned(), None::<&str>)
            .map_err(|error| error.to_string())
    }
}

/// Gives a magnet link to the system's magnet handler (the person's torrent app).
#[tauri::command]
#[specta::specta]
pub async fn torrent_open_magnet(
    app: tauri::AppHandle,
    state: State<'_, TorrentState>,
    magnet: String,
) -> CommandResult<()> {
    cv_ipc::torrent::torrent_open_magnet(&state, &OpenerHandoff(app), magnet).await
}

/// Opens a release's page on its source's site in the browser (`http` and `https` only).
#[tauri::command]
#[specta::specta]
pub async fn torrent_open_page(
    app: tauri::AppHandle,
    state: State<'_, TorrentState>,
    address: String,
) -> CommandResult<()> {
    cv_ipc::torrent::torrent_open_page(&state, &OpenerHandoff(app), address).await
}

/// Downloads a release's `.torrent` file and opens it with the registered program.
#[tauri::command]
#[specta::specta]
pub async fn torrent_open_torrent_file(
    app: tauri::AppHandle,
    state: State<'_, TorrentState>,
    url: String,
    name: String,
) -> CommandResult<()> {
    cv_ipc::torrent::torrent_open_torrent_file(&state, &OpenerHandoff(app), url, name).await
}

/// Every source with its switch, health and when it was last asked.
#[tauri::command]
#[specta::specta]
pub fn torrent_sources_status(
    state: State<'_, TorrentState>,
) -> CommandResult<Vec<TorrentSourceStatusDto>> {
    cv_ipc::torrent::torrent_sources_status(&state)
}

/// Switches a source on or off and returns the list as it is now. `indexer-service` switches the
/// managed indexer service.
#[tauri::command]
#[specta::specta]
pub fn torrent_set_source(
    state: State<'_, TorrentState>,
    id: String,
    on: bool,
) -> CommandResult<Vec<TorrentSourceStatusDto>> {
    cv_ipc::torrent::torrent_set_source(&state, id, on)
}

/// Tests the user's own Torznab instance. `api_key` `None` uses the saved key. A failed test is
/// an answer (`ok: false` with the message), not an error.
#[tauri::command]
#[specta::specta]
pub async fn torrent_test_torznab(
    state: State<'_, TorrentState>,
    url: String,
    api_key: Option<String>,
) -> CommandResult<TorznabTestDto> {
    cv_ipc::torrent::torrent_test_torznab(&state, url, api_key).await
}

/// Saves the user's own Torznab instance. `api_key`: `None` keeps the saved key, an empty text
/// removes it, other text replaces it. An empty address removes the instance and its key.
#[tauri::command]
#[specta::specta]
pub fn torrent_set_torznab(
    state: State<'_, TorrentState>,
    url: String,
    api_key: Option<String>,
) -> CommandResult<Vec<TorrentSourceStatusDto>> {
    cv_ipc::torrent::torrent_set_torznab(&state, url, api_key)
}

/// Looks for a Jackett the person runs themselves; `None` when none answers.
#[tauri::command]
#[specta::specta]
pub async fn torrent_detect_jackett(
    state: State<'_, TorrentState>,
) -> CommandResult<Option<OwnJackettDto>> {
    cv_ipc::torrent::torrent_detect_jackett(&state).await
}
