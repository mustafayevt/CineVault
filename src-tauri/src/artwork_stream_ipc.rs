//! The streaming artwork picker: `stream_artwork_candidates` asks every
//! provider that offers one artwork type, each in its own task, and sends what each one found
//! over a Channel as soon as it has it.
//!
//! The rules live in `cv_app::artwork::stream`. This module adapts its sources to the generic
//! provider engine (`provider_stream`), turns candidates into DTOs whose images are `cv://img`
//! keys (the thumbnails are already in the cache), and returns the start of the search: the title,
//! the providers that will answer and the image the item has now, pinned first.

use std::sync::Arc;

use async_trait::async_trait;
use cv_app::artwork::PickerCandidate;
use cv_app::artwork::stream::{CandidateSource, CandidateStream, SourceAnswer};
use cv_artwork::thumb::ThumbCache;
use cv_jobs::LaneId;
use serde::Serialize;
use specta::Type;
use tauri::State;
use tauri::ipc::Channel;

use crate::artwork_ipc::{
    ArtworkCandidateDto, ArtworkCommandError, ArtworkProviderDto, ArtworkState, CommandResult,
    candidate_dto, parse_item, parse_slot, slot_name,
};
use crate::provider_stream::{
    CancelToken, Progress, ProviderEvent, ProviderOutcome, SearchProvider, SearchState,
};

/// What a started artwork search tells the page at once; the candidates follow on the Channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct ArtworkStreamStartDto {
    pub title: String,
    pub year: Option<u32>,
    /// The artwork types the item can have, in listing order.
    pub slots: Vec<String>,
    /// The providers that will answer, in the order of their default ranking.
    pub providers: Vec<ArtworkProviderDto>,
    /// The image the item has now for the type, pinned first. Its `provider` is `current`.
    pub current: Option<ArtworkCandidateDto>,
}

/// One source of a search as a provider of the generic engine.
struct ArtworkSearch {
    source: CandidateSource,
    thumbs: Option<Arc<ThumbCache>>,
}

impl ArtworkSearch {
    fn dtos(&self, found: Vec<PickerCandidate>) -> Vec<ArtworkCandidateDto> {
        found
            .into_iter()
            .map(|candidate| candidate_dto(candidate, self.thumbs.as_deref()))
            .collect()
    }

    fn outcome(&self, answer: SourceAnswer) -> ProviderOutcome<ArtworkCandidateDto> {
        match answer {
            SourceAnswer::Candidates(found) => ProviderOutcome::results(self.dtos(found)),
            SourceAnswer::NeedsKey => ProviderOutcome::NeedsKey,
            SourceAnswer::KeyRejected => ProviderOutcome::key_rejected(),
            SourceAnswer::Failed { reason, retryable } => {
                ProviderOutcome::failed(reason, retryable)
            }
        }
    }
}

#[async_trait]
impl SearchProvider<ArtworkCandidateDto> for ArtworkSearch {
    fn id(&self) -> &str {
        &self.source.key
    }

    fn lane(&self) -> LaneId {
        if self.source.cpu {
            LaneId::Cpu
        } else {
            LaneId::provider(self.source.key.clone())
        }
    }

    async fn search(&self, _cancel: &CancelToken) -> ProviderOutcome<ArtworkCandidateDto> {
        self.outcome(self.source.fetch().await)
    }

    /// The best images go out as soon as their thumbnails are in the cache, a round at a time,
    /// and the rest follows, so the picker fills while the provider is still downloading.
    async fn search_progressive(
        &self,
        _cancel: &CancelToken,
        progress: &Progress<ArtworkCandidateDto>,
    ) -> ProviderOutcome<ArtworkCandidateDto> {
        let early = |batch: Vec<PickerCandidate>| {
            progress.partial(self.dtos(batch));
        };
        self.outcome(self.source.fetch_progressive(&early).await)
    }
}

/// The providers of a planned search, for the engine.
fn searches(
    sources: &[CandidateSource],
    thumbs: Option<&Arc<ThumbCache>>,
) -> Vec<Arc<dyn SearchProvider<ArtworkCandidateDto>>> {
    sources
        .iter()
        .map(|source| {
            Arc::new(ArtworkSearch {
                source: source.clone(),
                thumbs: thumbs.cloned(),
            }) as Arc<dyn SearchProvider<ArtworkCandidateDto>>
        })
        .collect()
}

fn start_dto(stream: &CandidateStream, thumbs: Option<&ThumbCache>) -> ArtworkStreamStartDto {
    ArtworkStreamStartDto {
        title: stream.title.clone(),
        year: Some(u32::from(stream.year)).filter(|year| *year > 0),
        slots: stream
            .slots
            .iter()
            .copied()
            .filter_map(slot_name)
            .map(str::to_owned)
            .collect(),
        providers: stream
            .sources
            .iter()
            .map(|source| ArtworkProviderDto {
                key: source.key.clone(),
                name: source.name.clone(),
            })
            .collect(),
        current: stream
            .current
            .clone()
            .map(|candidate: PickerCandidate| candidate_dto(candidate, thumbs)),
    }
}

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
    let service = artwork.get()?;
    let item = parse_item(&item_id)?;
    let kind = parse_slot(&slot)?;
    let stream = service.candidate_stream(item, kind).await?;
    let start = start_dto(&stream, artwork.thumbs.as_deref());
    search
        .start_on_channel(
            &search_id,
            searches(&stream.sources, artwork.thumbs.as_ref()),
            on_event,
        )
        .map_err(|error| ArtworkCommandError::new("storage", error.message))?;
    Ok(start)
}

/// How many images one artwork type of an item has.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct ArtworkSlotCountDto {
    pub slot: String,
    pub count: u32,
    /// The image the item has now for the type; its `provider` is `current`.
    pub current: Option<ArtworkCandidateDto>,
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
    let service = artwork.get()?;
    let counts = service.candidate_counts(parse_item(&item_id)?).await?;
    Ok(counts
        .into_iter()
        .filter_map(|entry| {
            slot_name(entry.slot).map(|name| ArtworkSlotCountDto {
                slot: name.to_owned(),
                count: entry.count,
                current: entry
                    .current
                    .map(|candidate| candidate_dto(candidate, artwork.thumbs.as_deref())),
            })
        })
        .collect())
}

#[cfg(test)]
mod tests;
