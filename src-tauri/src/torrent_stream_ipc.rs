//! The Tauri surface of the Torrent search page.
//!
//! The state, the DTOs and the command bodies live in [`cv_ipc::torrent_stream`]. This module
//! keeps the channel adapters and the thin wrappers.

use tauri::State;
use tauri::ipc::Channel;

use cv_ipc::provider_stream::{ProviderEvent, SearchState};

pub use cv_ipc::torrent_stream::*;

/// Searches TMDB for movies and shows as the person types. The movies and the shows answer as
/// two providers (`movies`, `shows`); the hits come best match first with their posters.
#[tauri::command]
#[specta::specta]
pub async fn torrent_title_search(
    state: State<'_, TorrentState>,
    search: State<'_, SearchState>,
    request: TorrentTitleSearchRequestDto,
    on_event: Channel<ProviderEvent<TorrentTitleHitDto>>,
) -> CommandResult<TorrentTitleStartDto> {
    cv_ipc::torrent_stream::torrent_title_search(
        &state,
        &search,
        request,
        crate::ipc_sink::sink(on_event),
    )
    .await
}

/// The details of a confirmed title: IDs, original title, year, runtime, whether it is anime and
/// the library link.
#[tauri::command]
#[specta::specta]
pub async fn torrent_title_details(
    state: State<'_, TorrentState>,
    kind: String,
    tmdb_id: u32,
) -> CommandResult<TorrentTitleDetailsDto> {
    cv_ipc::torrent_stream::torrent_title_details(&state, kind, tmdb_id).await
}

/// The library strips of the landing: movies below 2160p (upgrade available) and the newest
/// titles, each with its TMDB ID and library poster. Only the library is read.
#[tauri::command]
#[specta::specta]
pub async fn torrent_landing(
    state: State<'_, TorrentState>,
    images: State<'_, crate::img_ipc::ImageState>,
) -> CommandResult<TorrentLandingDto> {
    cv_ipc::torrent_stream::torrent_landing(
        &state,
        std::sync::Arc::clone(images.sources()),
        images.service().map(|service| service.cache().clone()),
    )
    .await
}

/// The seasons of a show.
#[tauri::command]
#[specta::specta]
pub async fn torrent_title_seasons(
    state: State<'_, TorrentState>,
    tmdb_id: u32,
) -> CommandResult<Vec<TorrentSeasonDto>> {
    cv_ipc::torrent_stream::torrent_title_seasons(&state, tmdb_id).await
}

/// The episodes of one season.
#[tauri::command]
#[specta::specta]
pub async fn torrent_title_episodes(
    state: State<'_, TorrentState>,
    tmdb_id: u32,
    season: u16,
) -> CommandResult<Vec<TorrentEpisodeDto>> {
    cv_ipc::torrent_stream::torrent_title_episodes(&state, tmdb_id, season).await
}

/// Searches every source for the releases of a confirmed title and scope. Each source answers as
/// a provider of the search (`Started`, `Partial`, `Results` or `Failed`, then `Done`); the rows
/// are merged across sources, so a release found by three arrives as one row naming all three.
/// `retry_provider` asks one source again and `cancel_search` stops the search.
#[tauri::command]
#[specta::specta]
pub async fn torrent_release_search(
    state: State<'_, TorrentState>,
    search: State<'_, SearchState>,
    request: TorrentSearchRequestDto,
    on_event: Channel<ProviderEvent<TorrentReleaseDto>>,
) -> CommandResult<TorrentSearchStartDto> {
    cv_ipc::torrent_stream::torrent_release_search(
        &state,
        &search,
        request,
        crate::ipc_sink::sink(on_event),
    )
    .await
}

#[cfg(test)]
mod tests;
