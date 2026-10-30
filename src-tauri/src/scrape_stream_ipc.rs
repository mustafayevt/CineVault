//! The Tauri surface of the streaming scrape search.
//!
//! The engine, the DTOs and the command body live in [`cv_ipc::scrape_stream`]; this module keeps
//! the channel adapter and the thin wrapper.

use cv_ipc::matching::{CommandResult, MatchKindDto, MatchState};
use cv_ipc::provider_stream::{ProviderEvent, SearchState};
use tauri::State;
use tauri::ipc::Channel;

pub use cv_ipc::scrape_stream::*;

/// Searches every enabled metadata provider for a title as a movie or a show. Each provider
/// answers on `on_event` as soon as it can (`Started`, then `Results`, `Failed` or `NeedsKey`,
/// then `Done` once all have answered); hits that share an IMDb, TMDB or TVDB ID arrive as one
/// entry. When nothing is found, a `Failed` event from the provider `search` says why just before
/// `Done`. `retry_provider` asks one provider again and `cancel_search` stops the search.
#[tauri::command]
#[specta::specta]
#[allow(
    clippy::too_many_arguments,
    reason = "a Tauri command takes its state, its parameters and its channel as separate arguments"
)]
pub async fn stream_scrape_candidates(
    state: State<'_, MatchState>,
    search: State<'_, SearchState>,
    item_id: String,
    query: String,
    kind: MatchKindDto,
    language: String,
    search_id: String,
    on_event: Channel<ProviderEvent<ScrapeStreamHitDto>>,
) -> CommandResult<ScrapeStreamStartDto> {
    cv_ipc::scrape_stream::stream_scrape_candidates(
        &state,
        &search,
        item_id,
        query,
        kind,
        language,
        search_id,
        crate::ipc_sink::sink(on_event),
    )
    .await
}
