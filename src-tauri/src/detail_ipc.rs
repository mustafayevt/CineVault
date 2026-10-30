//! The Tauri surface of the movie detail page.
//!
//! The behaviour and the DTOs live in [`cv_ipc::detail`]. Each function here opens a reader on the
//! app's state and calls the shared implementation, so a fix is written once; the watched toggle
//! stays in this binary with the watched service.

use tauri::State;

use crate::curation_ipc::CurationState;
use crate::img_ipc::ImageState;
use crate::library_ipc::{LibraryCommandError, storage};
use crate::nfo_ipc::NfoState;
use crate::settings_ipc::SettingsState;
use crate::sources_ipc::LibraryState;

pub use cv_ipc::detail::*;

/// Opens a reader on the shared library, image index, thumbnail cache, lifecycle and ratings.
fn reader(
    library: &State<'_, LibraryState>,
    images: &State<'_, ImageState>,
) -> Result<DetailReader, LibraryCommandError> {
    let db = library.database().map_err(storage)?;
    let cache = images.service().map(|service| service.cache().clone());
    Ok(DetailReader::new(db, images.sources().clone(), cache)
        .with_lifecycle(library.lifecycle().cloned())
        .with_rating_status(library.rating_status().cloned()))
}

/// Everything the movie detail page shows.
#[tauri::command]
#[specta::specta]
pub async fn get_movie_detail(
    library: State<'_, LibraryState>,
    images: State<'_, ImageState>,
    id: String,
) -> Result<MovieDetailDto, LibraryCommandError> {
    let id = parse_id(&id)?;
    let reader = reader(&library, &images)?;
    cv_ipc::detail::get_movie_detail(reader, id).await
}

/// Marks a movie watched or not watched. The report names what changed and the state before, for
/// `restore_watched`. Goes through the watched service, so the NFO files are rewritten.
#[tauri::command]
#[specta::specta]
pub async fn set_movie_watched(
    library: State<'_, LibraryState>,
    nfo: State<'_, NfoState>,
    curation: State<'_, CurationState>,
    settings: State<'_, SettingsState>,
    id: String,
    watched: bool,
) -> Result<crate::curation_ipc::WatchReportDto, LibraryCommandError> {
    let id = parse_id(&id)?;
    cv_ipc::curation::mark_watched(&library, &nfo, &curation, &settings, vec![id], watched).await
}
