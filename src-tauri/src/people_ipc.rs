//! The Tauri surface of the people of a title: the cast portraits and one person's IMDb page.
//!
//! The behaviour and the DTOs live in [`cv_ipc::people`]. Each function here reads the app's state
//! and the matching service and calls the shared implementation.

use tauri::State;

use crate::detail_ipc::parse_id;
use crate::img_ipc::ImageState;
use crate::library_ipc::{LibraryCommandError, storage};
use crate::match_ipc::MatchState;
use crate::sources_ipc::LibraryState;

pub use cv_ipc::people::*;

/// The portraits of the cast of a title that no file in `.actors` covers, fetched into the
/// thumbnail cache. Empty when the title has no TMDB portraits, or the artwork service is down.
#[tauri::command]
#[specta::specta]
pub async fn get_cast_portraits(
    library: State<'_, LibraryState>,
    images: State<'_, ImageState>,
    matching: State<'_, MatchState>,
    id: String,
) -> Result<Vec<CastPortraitDto>, LibraryCommandError> {
    let id = parse_id(&id)?;
    let db = library.database().map_err(storage)?;
    let cache = images.service().map(|service| service.cache().clone());
    cv_ipc::people::get_cast_portraits(db, cache, matching.artwork.clone(), id).await
}

/// The IMDb ID (`nm0000244`) of a person known by their TMDB ID, read from TMDB and stored with
/// the person. `None` when TMDB has none or cannot be asked now; the cast list then opens the
/// person's TMDB page instead.
#[tauri::command]
#[specta::specta]
pub async fn get_person_imdb_id(
    matching: State<'_, MatchState>,
    tmdb_id: String,
) -> Result<Option<String>, crate::match_ipc::MatchCommandError> {
    let service = matching.get()?;
    cv_ipc::people::get_person_imdb_id(service, tmdb_id)
        .await
        .map_err(|message| crate::match_ipc::MatchCommandError {
            code: "storage".to_owned(),
            message,
        })
}
