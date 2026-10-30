//! The Tauri surface of the NFO commands.
//!
//! The state, the DTOs and the command bodies live in [`cv_ipc::nfo`].

use tauri::State;

use crate::settings_ipc::SettingsState;
use crate::sources_ipc::LibraryState;

pub use cv_ipc::nfo::*;

/// Reads the NFO files of the selection (or the whole library) into the library, replacing the
/// values the files hold. Returns a token that `undo_nfo_action` takes.
#[tauri::command]
#[specta::specta]
pub async fn read_nfo(
    library: State<'_, LibraryState>,
    nfo: State<'_, NfoState>,
    settings: State<'_, SettingsState>,
    scope: NfoScopeDto,
) -> Result<NfoActionDto, NfoCommandError> {
    cv_ipc::nfo::read_nfo(&library, &nfo, &settings, scope).await
}

/// Writes the NFO files of the selection (or the whole library) from the library, following the
/// NFO settings. Files that are not CineVault's are never touched.
#[tauri::command]
#[specta::specta]
pub async fn rewrite_nfo(
    library: State<'_, LibraryState>,
    nfo: State<'_, NfoState>,
    settings: State<'_, SettingsState>,
    scope: NfoScopeDto,
) -> Result<NfoActionDto, NfoCommandError> {
    cv_ipc::nfo::rewrite_nfo(&library, &nfo, &settings, scope).await
}

/// Undoes one Read NFO or Rewrite NFO by its token. Files and items that changed since are left
/// alone and listed in the reply.
#[tauri::command]
#[specta::specta]
pub async fn undo_nfo_action(
    library: State<'_, LibraryState>,
    nfo: State<'_, NfoState>,
    token: String,
) -> Result<NfoUndoDto, NfoCommandError> {
    cv_ipc::nfo::undo_nfo_action(&library, &nfo, token).await
}

/// Renders the NFO of a movie as Rewrite NFO would, without writing it, and says whether the NFO
/// on disk already holds the same.
#[tauri::command]
#[specta::specta]
pub async fn preview_movie_nfo(
    library: State<'_, LibraryState>,
    nfo: State<'_, NfoState>,
    settings: State<'_, SettingsState>,
    id: String,
) -> Result<NfoPreviewDto, NfoCommandError> {
    cv_ipc::nfo::preview_movie_nfo(&library, &nfo, &settings, id).await
}

/// Renders the `tvshow.nfo` of a show as Rewrite NFO would, without writing it.
#[tauri::command]
#[specta::specta]
pub async fn preview_show_nfo(
    library: State<'_, LibraryState>,
    nfo: State<'_, NfoState>,
    settings: State<'_, SettingsState>,
    id: String,
) -> Result<NfoPreviewDto, NfoCommandError> {
    cv_ipc::nfo::preview_show_nfo(&library, &nfo, &settings, id).await
}

/// Renders the NFO of the video file an episode sits in as Rewrite NFO would, without writing it.
#[tauri::command]
#[specta::specta]
pub async fn preview_episode_nfo(
    library: State<'_, LibraryState>,
    nfo: State<'_, NfoState>,
    settings: State<'_, SettingsState>,
    id: String,
) -> Result<NfoPreviewDto, NfoCommandError> {
    cv_ipc::nfo::preview_episode_nfo(&library, &nfo, &settings, id).await
}
