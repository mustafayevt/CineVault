//! The streaming scrape preview and the field presets.
//!
//! `stream_scrape_preview` shows what scraping a chosen result would do, in two phases that
//! arrive over one Channel as two sources of the generic provider engine: `hit` answers at once
//! with the diff of the fields the search result carries, and `details` answers when the
//! provider's details have arrived with the diff of every field. A field row is identified by
//! its `field` key; a row of the second phase replaces the row of the first with the same key.
//!
//! `scrape_field_selection` turns a preset (Everything, Only empty, Artwork only, Custom) into
//! the field keys and the do-not-overwrite mode that `apply_scrape` takes.

use std::sync::Arc;

use async_trait::async_trait;
use cv_app::artwork::ArtworkService;
use cv_app::match_service::{
    DetailsAnswer, FieldDiff, PreviewCandidate, ScrapePreset, ScrapePreviewStream, scrape_selection,
};
use cv_domain::FieldChange;
use cv_jobs::LaneId;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tauri::ipc::Channel;

use crate::match_ipc::{
    CommandResult, MatchCommandError, MatchKindDto, MatchState, POSTER_WAIT, parse_item,
    poster_keys_of,
};
use crate::provider_stream::{
    CancelToken, ProviderEvent, ProviderOutcome, SearchProvider, SearchState,
};

/// The source that answers at once with the fields of the search result.
pub const HIT_SOURCE: &str = "hit";
/// The source that answers when the provider's details have arrived.
pub const DETAILS_SOURCE: &str = "details";

/// What applying would do to one field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum FieldChangeDto {
    /// Nothing: the values are equal, or the provider has none.
    Same,
    /// The stored field is empty and the provider has a value.
    Added,
    /// The stored field has a value that would be replaced.
    Changed,
}

impl From<FieldChange> for FieldChangeDto {
    fn from(change: FieldChange) -> Self {
        match change {
            FieldChange::Same => Self::Same,
            FieldChange::Added => Self::Added,
            FieldChange::Changed => Self::Changed,
        }
    }
}

/// One field of the stored title next to the same field of the chosen result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FieldDiffDto {
    /// The field key; also the key `apply_scrape` takes.
    pub field: String,
    /// `text`, `people`, `ratings-and-ids` or `artwork`.
    pub group: String,
    /// What the stored title has; `None` when the field is empty.
    pub current: Option<String>,
    /// What the provider has; `None` when it has no value (which never blanks a stored one).
    pub incoming: Option<String>,
    pub change: FieldChangeDto,
    /// The field, or the whole title, is locked: applying leaves it alone.
    pub locked: bool,
    /// For an artwork field: the key of the incoming image in the image cache (`cv://img`), when
    /// it was ready in time.
    pub incoming_image_key: Option<String>,
}

impl FieldDiffDto {
    fn of(diff: FieldDiff, posters: &std::collections::HashMap<String, String>) -> Self {
        let incoming_image_key = diff
            .incoming
            .as_deref()
            .and_then(|url| posters.get(url))
            .cloned();
        Self {
            field: diff.field,
            group: diff.group.code().to_owned(),
            current: diff.current,
            incoming: diff.incoming,
            change: diff.change.into(),
            locked: diff.locked,
            incoming_image_key,
        }
    }
}

/// The chosen result, as the search showed it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PreviewCandidateDto {
    pub provider: String,
    pub provider_id: String,
    pub title: String,
    pub year: Option<u16>,
    pub runtime_minutes: Option<u32>,
    /// The plot the search result carried, if it had one.
    pub overview: Option<String>,
}

impl From<PreviewCandidateDto> for PreviewCandidate {
    fn from(dto: PreviewCandidateDto) -> Self {
        Self {
            provider: dto.provider,
            provider_id: dto.provider_id,
            title: dto.title,
            year: dto.year,
            runtime_minutes: dto.runtime_minutes,
            overview: dto.overview,
        }
    }
}

/// What a started preview tells the page at once; the diff follows on the Channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ScrapePreviewStartDto {
    /// The whole title is locked, so applying changes nothing.
    pub item_locked: bool,
}

/// The presets of the Scrape workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ScrapePresetDto {
    Everything,
    OnlyEmpty,
    ArtworkOnly,
    Custom,
}

impl From<ScrapePresetDto> for ScrapePreset {
    fn from(dto: ScrapePresetDto) -> Self {
        match dto {
            ScrapePresetDto::Everything => Self::Everything,
            ScrapePresetDto::OnlyEmpty => Self::OnlyEmpty,
            ScrapePresetDto::ArtworkOnly => Self::ArtworkOnly,
            ScrapePresetDto::Custom => Self::Custom,
        }
    }
}

/// What `apply_scrape` takes for a preset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ScrapeSelectionDto {
    /// The field keys to write.
    pub fields: Vec<String>,
    /// Write only the fields that are empty.
    pub do_not_overwrite: bool,
}

/// Phase one: the diff of what the search result carries. Answers at once.
struct HitPhase {
    stream: Arc<ScrapePreviewStream>,
}

#[async_trait]
impl SearchProvider<FieldDiffDto> for HitPhase {
    fn id(&self) -> &str {
        HIT_SOURCE
    }

    fn lane(&self) -> LaneId {
        LaneId::Cpu
    }

    async fn search(&self, _cancel: &CancelToken) -> ProviderOutcome<FieldDiffDto> {
        let none = std::collections::HashMap::new();
        ProviderOutcome::results(
            self.stream
                .hit_fields()
                .iter()
                .cloned()
                .map(|diff| FieldDiffDto::of(diff, &none))
                .collect(),
        )
    }
}

/// Phase two: the provider's details, diffed field by field.
struct DetailsPhase {
    stream: Arc<ScrapePreviewStream>,
    artwork: Option<Arc<ArtworkService>>,
}

#[async_trait]
impl SearchProvider<FieldDiffDto> for DetailsPhase {
    fn id(&self) -> &str {
        DETAILS_SOURCE
    }

    fn lane(&self) -> LaneId {
        LaneId::provider(self.stream.provider())
    }

    async fn search(&self, _cancel: &CancelToken) -> ProviderOutcome<FieldDiffDto> {
        match self.stream.details_fields().await {
            DetailsAnswer::Fields(fields) => {
                // The incoming single images, ready for cv://img when they arrive in time.
                let urls: Vec<&str> = fields
                    .iter()
                    .filter(|d| d.group == cv_app::match_service::DiffGroup::Artwork)
                    .filter(|d| !d.field.starts_with("extra"))
                    .filter_map(|d| d.incoming.as_deref())
                    .filter(|url| url.starts_with("http"))
                    .collect();
                let posters =
                    poster_keys_of(self.artwork.as_ref(), urls.into_iter(), POSTER_WAIT).await;
                ProviderOutcome::results(
                    fields
                        .into_iter()
                        .map(|diff| FieldDiffDto::of(diff, &posters))
                        .collect(),
                )
            }
            DetailsAnswer::NeedsKey => ProviderOutcome::NeedsKey,
            DetailsAnswer::KeyRejected => ProviderOutcome::key_rejected(),
            DetailsAnswer::Failed { reason, retryable } => {
                ProviderOutcome::failed(reason, retryable)
            }
        }
    }
}

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
    let service = state.get()?;
    let item = parse_item(&item_id)?;
    let stream = Arc::new(service.scrape_preview_stream(item, &candidate.into(), &language)?);
    let start = ScrapePreviewStartDto {
        item_locked: stream.item_locked,
    };
    let phases: Vec<Arc<dyn SearchProvider<FieldDiffDto>>> = vec![
        Arc::new(HitPhase {
            stream: Arc::clone(&stream),
        }),
        Arc::new(DetailsPhase {
            stream,
            artwork: state.artwork.clone(),
        }),
    ];
    search
        .start_on_channel(&search_id, phases, on_event)
        .map_err(|error| MatchCommandError {
            code: "storage".to_owned(),
            message: error.message,
        })?;
    Ok(start)
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
    let selection = scrape_selection(kind.item_kind(), preset.into(), &custom);
    ScrapeSelectionDto {
        fields: selection.fields,
        do_not_overwrite: selection.do_not_overwrite,
    }
}

#[cfg(test)]
mod tests;
