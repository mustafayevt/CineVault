//! The streaming Scrape workspace: `stream_scrape_candidates` asks
//! every enabled metadata provider for a title at once and sends each provider's hits over a
//! Channel as it answers.
//!
//! The rules live in `cv_app::match_service` (the merge by shared IDs, the library marker and the
//! nothing-found reason). This module adapts its sources to the generic provider engine
//! (`provider_stream`), turns hits into DTOs with `cv://img` poster keys, and says why a search
//! found nothing as one last `Failed` event from the pseudo-provider `search`, just before
//! `Done`.
//!
//! An entry that several providers know has one `id` for the whole search. When a later provider
//! adds to an entry that was sent already, the entry is sent again under the same `id` and the
//! page replaces it; `absorbs` names entries that were joined into this one and are to be dropped.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use cv_app::artwork::ArtworkService;
use cv_app::match_service::{
    HitProvider, MergedHit, ScrapeAnswer, ScrapeCandidateStream, ScrapeSource,
};
use serde::Serialize;
use specta::Type;
use tauri::State;
use tauri::ipc::Channel;

use crate::match_ipc::{
    CommandResult, MatchCommandError, MatchKindDto, MatchState, POSTER_READY, POSTER_WAIT,
    PosterKeys, ReviewCandidateDto, ScrapeLinkDto, ScrapeProviderDto, parse_item, poster_keys_of,
};
use crate::provider_stream::{
    CancelToken, Finish, Progress, ProviderEvent, ProviderOutcome, SearchProvider, SearchState,
};

/// The pseudo-provider that carries the reason a search found nothing.
pub const SEARCH_PROVIDER: &str = "search";

/// A provider that has an entry, with its own ID for it.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HitProviderDto {
    pub provider: String,
    pub provider_id: String,
    /// Confidence of this provider's hit, 0 to 1.
    pub confidence: f64,
}

impl From<HitProvider> for HitProviderDto {
    fn from(hit: HitProvider) -> Self {
        Self {
            provider: hit.provider,
            provider_id: hit.provider_id,
            confidence: hit.confidence,
        }
    }
}

/// One title the search found, possibly known by several providers.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ScrapeStreamHitDto {
    /// Stable for the whole search; an entry sent again with the same `id` replaces the first.
    pub id: String,
    /// The most confident provider's hit. Its `confidence` is the entry's.
    pub candidate: ReviewCandidateDto,
    /// Every provider that has the title, the most confident first.
    pub providers: Vec<HitProviderDto>,
    /// The IDs that joined the providers' hits, such as `imdb:tt0069293`.
    pub ids: Vec<String>,
    /// Set when another library item is already linked to this title.
    pub in_library: Option<ScrapeLinkDto>,
    /// Entries sent earlier that this one now includes; drop them.
    pub absorbs: Vec<String>,
}

impl ScrapeStreamHitDto {
    fn of(hit: MergedHit, posters: &PosterKeys) -> Self {
        Self {
            id: hit.id,
            candidate: ReviewCandidateDto::of(hit.candidate, posters),
            providers: hit.providers.into_iter().map(Into::into).collect(),
            ids: hit.ids,
            in_library: hit.in_library.map(|link| ScrapeLinkDto {
                item_id: link.item.0.to_string(),
                name: link.name,
            }),
            absorbs: hit.absorbs,
        }
    }
}

/// What a started scrape search tells the page at once; the hits follow on the Channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ScrapeStreamStartDto {
    /// The text that is searched for (the item's own title when the query was blank).
    pub query: String,
    /// The providers that will answer, the default first. Providers that wait for a key are
    /// listed last and answer `NeedsKey`.
    pub providers: Vec<ScrapeProviderDto>,
}

/// One source of a search as a provider of the generic engine.
struct ScrapeSearch {
    source: ScrapeSource,
    artwork: Option<Arc<ArtworkService>>,
}

#[async_trait]
impl SearchProvider<ScrapeStreamHitDto> for ScrapeSearch {
    fn id(&self) -> &str {
        &self.source.key
    }

    async fn search(&self, _cancel: &CancelToken) -> ProviderOutcome<ScrapeStreamHitDto> {
        match self.source.fetch().await {
            ScrapeAnswer::Hits(hits) => {
                let posters = self.posters(&hits, POSTER_WAIT).await;
                ProviderOutcome::results(Self::dtos(hits, &posters))
            }
            other => Self::refusal(other),
        }
    }

    /// The hits are listed at once with the posters the cache already holds; the posters that
    /// download meanwhile follow as an update of the same entries, so the list does not wait for
    /// the pictures.
    async fn search_progressive(
        &self,
        _cancel: &CancelToken,
        progress: &Progress<ScrapeStreamHitDto>,
    ) -> ProviderOutcome<ScrapeStreamHitDto> {
        let hits = match self.source.fetch().await {
            ScrapeAnswer::Hits(hits) => hits,
            other => return Self::refusal(other),
        };
        let wanted = hits.iter().any(|hit| hit.candidate.poster_url.is_some());
        if self.artwork.is_none() || !wanted {
            return ProviderOutcome::results(Self::dtos(hits, &PosterKeys::new()));
        }
        let ready = self.posters(&hits, POSTER_READY).await;
        progress.partial(Self::dtos(hits.clone(), &ready));
        let later = self.posters(&hits, POSTER_WAIT).await;
        if later.len() > ready.len() {
            progress.update(Self::dtos(hits, &later));
        }
        ProviderOutcome::results(Vec::new())
    }
}

impl ScrapeSearch {
    async fn posters(&self, hits: &[MergedHit], wait: Duration) -> PosterKeys {
        poster_keys_of(
            self.artwork.as_ref(),
            hits.iter()
                .filter_map(|hit| hit.candidate.poster_url.as_deref()),
            wait,
        )
        .await
    }

    fn dtos(hits: Vec<MergedHit>, posters: &PosterKeys) -> Vec<ScrapeStreamHitDto> {
        hits.into_iter()
            .map(|hit| ScrapeStreamHitDto::of(hit, posters))
            .collect()
    }

    fn refusal(answer: ScrapeAnswer) -> ProviderOutcome<ScrapeStreamHitDto> {
        match answer {
            ScrapeAnswer::Hits(_) => ProviderOutcome::results(Vec::new()),
            ScrapeAnswer::NeedsKey => ProviderOutcome::NeedsKey,
            ScrapeAnswer::KeyRejected => ProviderOutcome::key_rejected(),
            ScrapeAnswer::Failed { reason, retryable } => {
                ProviderOutcome::failed(reason, retryable)
            }
        }
    }
}

/// The providers of a planned search, for the engine.
fn searches(
    stream: &ScrapeCandidateStream,
    artwork: Option<&Arc<ArtworkService>>,
) -> Vec<Arc<dyn SearchProvider<ScrapeStreamHitDto>>> {
    stream
        .sources
        .iter()
        .map(|source| {
            Arc::new(ScrapeSearch {
                source: source.clone(),
                artwork: artwork.cloned(),
            }) as Arc<dyn SearchProvider<ScrapeStreamHitDto>>
        })
        .collect()
}

/// What the search adds before `Done`: the reason, when it found nothing.
fn nothing_found(stream: &ScrapeCandidateStream) -> Finish<ScrapeStreamHitDto> {
    let summary = stream.summary();
    Arc::new(move || {
        summary
            .nothing_found()
            .map(|(_, reason)| ProviderEvent::Failed {
                provider: SEARCH_PROVIDER.to_owned(),
                reason,
                retryable: false,
            })
    })
}

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
    let service = state.get()?;
    let item = parse_item(&item_id)?;
    let stream = service.scrape_candidate_stream(item, &query, kind.item_kind(), &language)?;
    let start = ScrapeStreamStartDto {
        query: stream.query.clone(),
        providers: stream
            .sources
            .iter()
            .map(|source| ScrapeProviderDto {
                key: source.key.clone(),
                name: source.name.clone(),
            })
            .collect(),
    };
    crate::provider_stream::start_on_channel_finishing(
        &search,
        &search_id,
        searches(&stream, state.artwork.as_ref()),
        on_event,
        Some(nothing_found(&stream)),
    )
    .map_err(|error| MatchCommandError {
        code: "storage".to_owned(),
        message: error.message,
    })?;
    Ok(start)
}

#[cfg(test)]
mod tests;
