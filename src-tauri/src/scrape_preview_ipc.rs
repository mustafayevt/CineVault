//! The Tauri surface of the streaming scrape preview commands.
//!
//! The engine, the DTOs and the command bodies live in [`cv_ipc::scrape_preview`]; this module keeps the
//! channel adapters and the thin wrappers.

use cv_ipc::matching::{CommandResult, MatchKindDto, MatchState};
use cv_ipc::provider_stream::{ProviderEvent, SearchState};
use tauri::State;
use tauri::ipc::Channel;

pub use cv_ipc::scrape_preview::*;

/// Shows what scraping a chosen result would do to a title. The diff of what the search result
/// carries arrives at once as the source `hit`; the diff of every field follows as the source
/// `details` when the provider's details have arrived, and replaces the first rows by `field`.
/// `retry_provider` asks `details` again; `cancel_search` stops the preview.
#[tauri::command]
#[specta::specta]
pub async fn stream_scrape_preview(
    state: State<'_, MatchState>,
    search: State<'_, SearchState>,
    item_id: String,
    candidate: PreviewCandidateDto,
    language: String,
    search_id: String,
    on_event: Channel<ProviderEvent<FieldDiffDto>>,
) -> CommandResult<ScrapePreviewStartDto> {
    cv_ipc::scrape_preview::stream_scrape_preview(
        &state,
        &search,
        item_id,
        candidate,
        language,
        search_id,
        crate::ipc_sink::sink(on_event),
    )
    .await
}

/// The field keys and the do-not-overwrite mode a preset means for a kind of title. `custom`
/// holds the keys the user ticked and is read only for the Custom preset. Pass the result to
/// `apply_scrape`, which leaves locked fields and locked titles alone.
#[tauri::command]
#[specta::specta]
pub fn scrape_field_selection(
    kind: MatchKindDto,
    preset: ScrapePresetDto,
    custom: Vec<String>,
) -> ScrapeSelectionDto {
    cv_ipc::scrape_preview::scrape_field_selection(kind, preset, custom)
}
