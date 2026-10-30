//! The Tauri surface of the artwork picker commands.
//!
//! The state, the DTOs and the command bodies live in [`cv_ipc::artwork`].

use tauri::State;

use crate::img_ipc::ImageState;
use crate::library_ipc::PosterDto;

pub use cv_ipc::artwork::*;

/// Sets one image of an item from a candidate's address, a pasted web address or the path of a
/// picture on disk. The old image goes to the trash folder of the data source.
#[tauri::command]
#[specta::specta]
pub async fn place_artwork(
    state: State<'_, ArtworkState>,
    item_id: String,
    slot: String,
    source: String,
) -> CommandResult<ArtworkPlacedDto> {
    cv_ipc::artwork::place_artwork(&state, item_id, slot, source).await
}

/// Downloads the full-size original of a candidate (`candidate_id` is its `id`; a TMDB address
/// is asked at `original`) into the image cache, never into a media folder, and returns its
/// key. The big view of the picker then shows `imageUrl(key, "full")`: the file itself through
/// `cv://img`, nothing over IPC. An original fetched before is not fetched again.
#[tauri::command]
#[specta::specta]
pub async fn artwork_full_image(
    state: State<'_, ArtworkState>,
    images: State<'_, ImageState>,
    candidate_id: String,
) -> CommandResult<PosterDto> {
    cv_ipc::artwork::artwork_full_image(&state, images.sources(), candidate_id).await
}

/// Replaces the extra fanart or the extra thumbs of an item with the chosen images. The old
/// extras go to the trash folder of the data source.
#[tauri::command]
#[specta::specta]
pub async fn place_extra_artwork(
    state: State<'_, ArtworkState>,
    item_id: String,
    slot: String,
    sources: Vec<String>,
) -> CommandResult<ArtworkPlacedDto> {
    cv_ipc::artwork::place_extra_artwork(&state, item_id, slot, sources).await
}

/// Fills only the empty artwork types of the items from the providers (and the extras and
/// portraits the settings ask for).
#[tauri::command]
#[specta::specta]
pub async fn download_missing_artwork(
    state: State<'_, ArtworkState>,
    item_ids: Vec<String>,
) -> CommandResult<ArtworkRunDto> {
    cv_ipc::artwork::download_missing_artwork(&state, item_ids).await
}

/// The step after episodes were scraped: fills the missing season images and episode thumbs of
/// the show of `item_id`, when the after-scrape artwork setting is on. `season` limits it to one
/// season; without it a season or an episode stands for its own season and the show for all of
/// them. Nothing that is there is replaced.
#[tauri::command]
#[specta::specta]
pub async fn fill_season_artwork(
    state: State<'_, ArtworkState>,
    item_id: String,
    season: Option<u32>,
) -> CommandResult<ArtworkRunDto> {
    cv_ipc::artwork::fill_season_artwork(&state, item_id, season).await
}
