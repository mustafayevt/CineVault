//! The Tauri surface of the show, season and episode pages.
//!
//! The behaviour and the DTOs live in [`cv_ipc::tv`]. Each function here opens a reader on the
//! app's state, reads the one setting the show page needs and calls the shared implementation; the
//! watched toggle stays in this binary with the watched service.

use cv_app::tv_info::season_art_fallback;
use tauri::State;

use crate::curation_ipc::CurationState;
use crate::detail_ipc::parse_id;
use crate::img_ipc::ImageState;
use crate::library_ipc::{LibraryCommandError, storage};
use crate::nfo_ipc::NfoState;
use crate::settings_ipc::SettingsState;
use crate::sources_ipc::LibraryState;

pub use cv_ipc::tv::*;

/// Opens a reader on the shared library, image index, thumbnail cache, lifecycle and ratings.
fn reader(
    library: &State<'_, LibraryState>,
    images: &State<'_, ImageState>,
) -> Result<TvReader, LibraryCommandError> {
    let db = library.database().map_err(storage)?;
    let cache = images.service().map(|service| service.cache().clone());
    Ok(TvReader::new(db, images.sources().clone(), cache)
        .with_lifecycle(library.lifecycle().cloned())
        .with_rating_status(library.rating_status().cloned()))
}

/// Everything the show page and its season pages show.
#[tauri::command]
#[specta::specta]
pub async fn get_show_detail(
    library: State<'_, LibraryState>,
    images: State<'_, ImageState>,
    settings: State<'_, SettingsState>,
    id: String,
) -> Result<ShowDetailDto, LibraryCommandError> {
    let id = parse_id(&id)?;
    let fallback = season_art_fallback(&settings.service().snapshot().values);
    let reader = reader(&library, &images)?.with_season_art_fallback(fallback);
    cv_ipc::tv::get_show_detail(reader, id).await
}

/// Everything the episode page shows.
#[tauri::command]
#[specta::specta]
pub async fn get_episode_detail(
    library: State<'_, LibraryState>,
    images: State<'_, ImageState>,
    id: String,
) -> Result<EpisodeDetailDto, LibraryCommandError> {
    let id = parse_id(&id)?;
    let reader = reader(&library, &images)?;
    cv_ipc::tv::get_episode_detail(reader, id).await
}

/// Marks episodes watched or not watched; a season or a show is its episodes. The report counts
/// the changes and holds the state before, for `restore_watched`. Goes through the watched
/// service, so the NFO files are rewritten.
#[tauri::command]
#[specta::specta]
pub async fn set_episodes_watched(
    library: State<'_, LibraryState>,
    nfo: State<'_, NfoState>,
    curation: State<'_, CurationState>,
    settings: State<'_, SettingsState>,
    ids: Vec<String>,
    watched: bool,
) -> Result<crate::curation_ipc::WatchReportDto, LibraryCommandError> {
    let ids = ids
        .iter()
        .map(|id| parse_id(id))
        .collect::<Result<Vec<_>, _>>()?;
    cv_ipc::curation::mark_watched(&library, &nfo, &curation, &settings, ids, watched).await
}
