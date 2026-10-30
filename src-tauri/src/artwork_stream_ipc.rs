//! The Tauri surface of the streaming artwork stream commands.
//!
//! The engine, the DTOs and the command bodies live in [`cv_ipc::artwork_stream`]; this module keeps the
//! channel adapters and the thin wrappers.

use cv_ipc::artwork::{ArtworkCandidateDto, ArtworkState, CommandResult};
use cv_ipc::provider_stream::{ProviderEvent, SearchState};
use tauri::State;
use tauri::ipc::Channel;

pub use cv_ipc::artwork_stream::*;

/// Searches every provider that offers one artwork type of an item. Each provider answers on
/// `on_event` as soon as it can (`Started`, then `Results`, `Failed` or `NeedsKey`, then `Done`
/// once all have answered); `retry_provider` asks one again and `cancel_search` stops the search.
/// The returned start names the providers and pins the image the item has now.
#[tauri::command]
#[specta::specta]
pub async fn stream_artwork_candidates(
    artwork: State<'_, ArtworkState>,
    search: State<'_, SearchState>,
    item_id: String,
    slot: String,
    search_id: String,
    on_event: Channel<ProviderEvent<ArtworkCandidateDto>>,
) -> CommandResult<ArtworkStreamStartDto> {
    cv_ipc::artwork_stream::stream_artwork_candidates(
        &artwork,
        &search,
        item_id,
        slot,
        search_id,
        crate::ipc_sink::sink(on_event),
    )
    .await
}

/// How many images each artwork type of an item has at the providers, and the image it has now
/// for the type, without downloading a candidate thumbnail. The picker shows these on its type
/// strip while it searches only the type on screen.
/// A season or an episode has no such count (`unsupported`): its picker searches each type.
#[tauri::command]
#[specta::specta]
pub async fn artwork_candidate_counts(
    artwork: State<'_, ArtworkState>,
    item_id: String,
) -> CommandResult<Vec<ArtworkSlotCountDto>> {
    cv_ipc::artwork_stream::artwork_candidate_counts(&artwork, item_id).await
}
