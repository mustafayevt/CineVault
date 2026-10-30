//! The Tauri surface of the watched-state and NEW-flag commands.
//!
//! The state, the DTOs and the command bodies live in [`cv_ipc::curation`].

use tauri::State;

use crate::edit_ipc::EditCommandError;

use crate::nfo_ipc::NfoState;
use crate::settings_ipc::SettingsState;
use crate::sources_ipc::LibraryState;

pub use cv_ipc::curation::*;

/// Marks movies, episodes, seasons or shows (a season or show is its episodes) watched or not
/// watched. Marking watched sets the play count to at least 1 and the last-watched date to now;
/// marking not watched resets the play count to 0.
#[tauri::command]
#[specta::specta]
pub async fn set_watched(
    library: State<'_, LibraryState>,
    nfo: State<'_, NfoState>,
    curation: State<'_, CurationState>,
    settings: State<'_, SettingsState>,
    ids: Vec<String>,
    watched: bool,
) -> Result<WatchReportDto, EditCommandError> {
    cv_ipc::curation::set_watched(&library, &nfo, &curation, &settings, ids, watched).await
}

/// Flips the watched state of movies, episodes, seasons or shows: when everything they cover is
/// watched it is marked not watched, otherwise all of it is marked watched.
#[tauri::command]
#[specta::specta]
pub async fn toggle_watched(
    library: State<'_, LibraryState>,
    nfo: State<'_, NfoState>,
    curation: State<'_, CurationState>,
    settings: State<'_, SettingsState>,
    ids: Vec<String>,
) -> Result<ToggleWatchedDto, EditCommandError> {
    cv_ipc::curation::toggle_watched(&library, &nfo, &curation, &settings, ids).await
}

/// Puts back the states a `set_watched` reported, play counts included.
#[tauri::command]
#[specta::specta]
pub async fn restore_watched(
    library: State<'_, LibraryState>,
    nfo: State<'_, NfoState>,
    curation: State<'_, CurationState>,
    settings: State<'_, SettingsState>,
    previous: Vec<WatchStateDto>,
) -> Result<WatchReportDto, EditCommandError> {
    cv_ipc::curation::restore_watched(&library, &nfo, &curation, &settings, previous).await
}

/// Clears the NEW flag of the given titles.
#[tauri::command]
#[specta::specta]
pub async fn clear_new_flags(
    library: State<'_, LibraryState>,
    curation: State<'_, CurationState>,
    ids: Vec<String>,
) -> Result<NewFlagReportDto, EditCommandError> {
    cv_ipc::curation::clear_new_flags(&library, &curation, ids).await
}

/// Clears the NEW flag of a title whose page was opened: only its own flag, so an opened show
/// keeps its NEW episodes (and the marker they give it). Returns the ids that changed.
#[tauri::command]
#[specta::specta]
pub async fn clear_new_on_open(
    library: State<'_, LibraryState>,
    curation: State<'_, CurationState>,
    id: String,
) -> Result<NewFlagReportDto, EditCommandError> {
    cv_ipc::curation::clear_new_on_open(&library, &curation, id).await
}

/// Clears the NEW flag of every title, or of one kind (`movie`, `show` or `episode`).
#[tauri::command]
#[specta::specta]
pub async fn clear_all_new_flags(
    library: State<'_, LibraryState>,
    curation: State<'_, CurationState>,
    kind: Option<String>,
) -> Result<NewFlagReportDto, EditCommandError> {
    cv_ipc::curation::clear_all_new_flags(&library, &curation, kind).await
}

/// Flags the given titles NEW again (Undo of a clear).
#[tauri::command]
#[specta::specta]
pub async fn restore_new_flags(
    library: State<'_, LibraryState>,
    curation: State<'_, CurationState>,
    ids: Vec<String>,
) -> Result<NewFlagReportDto, EditCommandError> {
    cv_ipc::curation::restore_new_flags(&library, &curation, ids).await
}
