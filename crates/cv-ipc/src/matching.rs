//! Typed commands for matching: the match batches, the Needs review inbox and the
//! search-and-scrape panel.
//!
//! The rules live in `cv_app::match_service` (and `cv_app::matching` below it). This module opens
//! the service from the app services, runs the database work off the UI thread and converts to
//! the DTOs the UI sees. Every command is an `async fn`, so it runs inside the async runtime the
//! job manager and the providers need.
//!
//! Posters of candidates are remote images. They never cross IPC: the artwork service downloads
//! them into the thumbnail cache (the same cache and downloads as the artwork picker) and the DTOs
//! carry the `cv://img` key. A poster that is not ready within [`POSTER_WAIT`] stays `null` (the
//! UI draws a placeholder) while its download goes on, so the next list has it.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use cv_app::artwork::ArtworkService;
use cv_app::match_service::{
    CandidateView, Choice, EvidenceTick, MatchService, MatchServiceError, REVIEW_LIST_LIMIT,
    ReviewEntry, ScrapeApply, TickKind, TickVerdict,
};
use cv_app::matching::batch::BatchMode;
use cv_domain::{ItemId, ItemKind};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::sources::LibraryState;
use cv_app::services::AppServices;

/// Held in Tauri state. When the app services could not start, every command answers with a
/// `storage` error that says so.
pub struct MatchState {
    pub service: Option<Arc<MatchService>>,
    pub artwork: Option<Arc<ArtworkService>>,
}

/// How long a list of candidates waits for their posters to download.
pub const POSTER_WAIT: Duration = Duration::from_millis(2500);

/// How long a list that goes out early waits for posters: long enough to pick up the ones the
/// cache holds already, short enough not to hold the list back.
pub const POSTER_READY: Duration = Duration::from_millis(80);

/// Thumbnail keys of candidate posters, by remote address.
pub type PosterKeys = HashMap<String, String>;

impl MatchState {
    pub fn from_services(services: Option<&Arc<AppServices>>) -> Self {
        Self {
            service: services.map(|services| Arc::clone(services.matching())),
            artwork: services.map(|services| Arc::clone(services.artwork())),
        }
    }

    /// Downloads the posters of the candidates into the thumbnail cache and returns their keys by
    /// address. Empty when the artwork service is not running.
    async fn poster_keys<'a>(&self, urls: impl Iterator<Item = &'a str>) -> PosterKeys {
        poster_keys_of(self.artwork.as_ref(), urls, POSTER_WAIT).await
    }

    pub fn get(&self) -> Result<Arc<MatchService>, MatchCommandError> {
        self.service.clone().ok_or_else(|| MatchCommandError {
            code: "storage".to_owned(),
            message: "CineVault could not open the library, so titles cannot be matched."
                .to_owned(),
        })
    }
}

/// The thumbnail keys of remote posters, by address; empty without an artwork service. Posters
/// that are not ready within `wait` keep downloading and are found next time.
pub async fn poster_keys_of<'a>(
    artwork: Option<&Arc<ArtworkService>>,
    urls: impl Iterator<Item = &'a str>,
    wait: Duration,
) -> PosterKeys {
    let Some(artwork) = artwork else {
        return PosterKeys::new();
    };
    let urls: Vec<String> = urls.map(str::to_owned).collect();
    artwork
        .remote_thumbs(urls, wait)
        .await
        .into_iter()
        .map(|(url, key)| (url, key.to_string()))
        .collect()
}

/// Why a command did not do what was asked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct MatchCommandError {
    /// Stable code: `storage`, `not_found`, `not_pending`, `no_provider`, `provider`, `locked`,
    /// `unsupported`, `invalid` or `queue`.
    pub code: String,
    /// Plain sentence for the user.
    pub message: String,
}

impl From<MatchServiceError> for MatchCommandError {
    fn from(error: MatchServiceError) -> Self {
        Self {
            code: error.code().to_owned(),
            message: error.to_string(),
        }
    }
}

pub type CommandResult<T> = Result<T, MatchCommandError>;

pub fn parse_item(text: &str) -> CommandResult<ItemId> {
    text.trim()
        .parse::<i64>()
        .map(ItemId)
        .map_err(|_| MatchCommandError {
            code: "invalid".to_owned(),
            message: format!("{text:?} is not a library item id."),
        })
}

fn parse_items(texts: &[String]) -> CommandResult<Vec<ItemId>> {
    texts.iter().map(|text| parse_item(text)).collect()
}

fn row_id(id: u32) -> i64 {
    i64::from(id)
}

fn small(id: i64) -> u32 {
    u32::try_from(id).unwrap_or(u32::MAX)
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, MatchServiceError> + Send + 'static,
) -> CommandResult<T> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| MatchCommandError {
            code: "storage".to_owned(),
            message: error.to_string(),
        })?
        .map_err(Into::into)
}

// ---- DTOs ------------------------------------------------------------------------------------

/// Movie or show: the kinds that are matched online.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum MatchKindDto {
    Movie,
    Show,
}

impl MatchKindDto {
    pub fn item_kind(self) -> ItemKind {
        match self {
            Self::Movie => ItemKind::Movie,
            Self::Show => ItemKind::Show,
        }
    }

    pub fn of(kind: ItemKind) -> Self {
        if kind == ItemKind::Show {
            Self::Show
        } else {
            Self::Movie
        }
    }
}

/// What an evidence check is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum EvidenceKindDto {
    Title,
    Runtime,
    Language,
    Year,
    Id,
}

/// Whether a candidate agrees with what is known locally. `unknown` means nothing to compare.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum EvidenceVerdictDto {
    Agree,
    Conflict,
    Unknown,
}

/// One evidence check on a candidate card: a tick, a cross or a neutral dot, with its text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ReviewEvidenceTickDto {
    pub kind: EvidenceKindDto,
    pub verdict: EvidenceVerdictDto,
    /// Short text such as `runtime 167 = 167 min`.
    pub text: String,
}

impl From<EvidenceTick> for ReviewEvidenceTickDto {
    fn from(tick: EvidenceTick) -> Self {
        Self {
            kind: match tick.kind {
                TickKind::Title => EvidenceKindDto::Title,
                TickKind::Runtime => EvidenceKindDto::Runtime,
                TickKind::Language => EvidenceKindDto::Language,
                TickKind::Year => EvidenceKindDto::Year,
                TickKind::Id => EvidenceKindDto::Id,
            },
            verdict: match tick.verdict {
                TickVerdict::Agree => EvidenceVerdictDto::Agree,
                TickVerdict::Conflict => EvidenceVerdictDto::Conflict,
                TickVerdict::Unknown => EvidenceVerdictDto::Unknown,
            },
            text: tick.text,
        }
    }
}

/// How a confidence was reached. All values are fractions.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ReviewScoreBreakdownDto {
    /// Best Dice or Jaro-Winkler similarity of the title.
    pub similarity: f64,
    /// Subtracted for a year that is off or unknown.
    pub year_penalty: f64,
    /// Subtracted when the candidate has no poster.
    pub poster_penalty: f64,
    /// Net adjustment from runtime, language and ID evidence; negative when they disagree.
    pub evidence: f64,
}

/// A title the matcher found for a queued item.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ReviewCandidateDto {
    /// Key of the provider that found it; accepting asks this provider for the details.
    pub provider: String,
    pub provider_id: String,
    pub title: String,
    /// The title in its original language, when it differs from `title` ("Solyaris").
    pub original_title: Option<String>,
    pub year: Option<u16>,
    pub director: Option<String>,
    pub runtime_minutes: Option<u32>,
    /// Original language as an ISO 639 code.
    pub language: Option<String>,
    pub overview: Option<String>,
    /// Content key of the cached poster (`cv://img`); `None` shows a generated placeholder.
    pub poster_key: Option<String>,
    /// Confidence, 0 to 1.
    pub confidence: f64,
    /// The value lists are ordered by, best first: the confidence plus how well known the title
    /// is. Only for ordering, never shown.
    pub rank: f64,
    pub evidence: Vec<ReviewEvidenceTickDto>,
    pub breakdown: ReviewScoreBreakdownDto,
}

impl ReviewCandidateDto {
    pub fn of(view: CandidateView, posters: &PosterKeys) -> Self {
        let poster_key = view
            .poster_url
            .as_deref()
            .and_then(|url| posters.get(url))
            .cloned();
        Self {
            provider: view.provider,
            provider_id: view.provider_id,
            title: view.title,
            original_title: view.original_title,
            year: view.year,
            director: view.director,
            runtime_minutes: view.runtime_minutes,
            language: view.language,
            overview: view.overview,
            poster_key,
            confidence: view.confidence,
            rank: view.rank,
            evidence: view.evidence.into_iter().map(Into::into).collect(),
            breakdown: ReviewScoreBreakdownDto {
                similarity: view.breakdown.similarity,
                year_penalty: view.breakdown.year_penalty,
                poster_penalty: view.breakdown.poster_penalty,
                evidence: view.breakdown.evidence,
            },
        }
    }
}

/// What the scan found about the item on disk.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ReviewLocalEvidenceDto {
    /// The title parsed from the folder or file name.
    pub title: String,
    pub year: Option<u16>,
    pub runtime_seconds: Option<u32>,
    /// `1080p · AVC · FLAC 1.0`.
    pub tech_summary: Option<String>,
    /// Primary audio language as an ISO 639 code.
    pub audio_language: Option<String>,
    /// IDs the item already has, such as `tmdb:693134`.
    pub ids: Vec<String>,
    pub has_nfo: bool,
    /// Content key of a video frame (`cv://img`) and where in the film it was taken.
    pub frame_key: Option<String>,
    pub frame_at_seconds: Option<u32>,
}

/// One entry of the Needs review inbox.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ReviewItemDto {
    /// The review row; stable while the item is queued.
    pub id: u32,
    /// The library item.
    pub item_id: String,
    pub kind: MatchKindDto,
    /// The label in the queue, for example `Solaris\Solaris.mkv`.
    pub name: String,
    /// The full path of the folder.
    pub path: String,
    /// Why it was not matched automatically, as plain text.
    pub reason: String,
    /// The reason is something to look at (an ambiguity), not just a note.
    pub attention: bool,
    /// An optional sentence that helps the decision.
    pub note: Option<String>,
    /// The best candidates score the same: Accept all leaves the title for a person.
    pub tied: bool,
    /// The folder as the user names it: `Movies › Arrival (2016)`; `path` is the full one.
    pub short_path: String,
    pub local: ReviewLocalEvidenceDto,
    /// Best first.
    pub candidates: Vec<ReviewCandidateDto>,
}

impl ReviewItemDto {
    fn of(entry: ReviewEntry, posters: &PosterKeys) -> Self {
        let local = entry.local;
        Self {
            tied: entry.tied,
            short_path: entry.short_path,
            id: small(entry.review_id),
            item_id: entry.item.0.to_string(),
            kind: MatchKindDto::of(entry.kind),
            name: entry.name,
            path: entry.path,
            reason: entry.reason,
            attention: entry.attention,
            note: entry.note,
            local: ReviewLocalEvidenceDto {
                title: local.title,
                year: local.year,
                runtime_seconds: local.runtime_seconds,
                tech_summary: local.tech_summary,
                audio_language: local.audio_language,
                ids: local.ids,
                has_nfo: local.has_nfo,
                frame_key: None,
                frame_at_seconds: None,
            },
            candidates: entry
                .candidates
                .into_iter()
                .map(|candidate| ReviewCandidateDto::of(candidate, posters))
                .collect(),
        }
    }
}

/// A library item that is already linked to a title.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ScrapeLinkDto {
    pub item_id: String,
    /// How the library lists it, for example `Solaris (2002)`.
    pub name: String,
}

/// A provider that can search and scrape the kind of item. The first one is the default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ScrapeProviderDto {
    pub key: String,
    pub name: String,
}

/// A metadata provider as the Providers page lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MetadataProviderDto {
    pub id: String,
    pub name: String,
    pub movies: bool,
    pub shows: bool,
    /// Its credentials are in place, so it can be asked.
    pub ready: bool,
    /// A Kodi XML scraper add-on.
    pub kodi: bool,
    /// It may join the fallback chain.
    pub can_fallback: bool,
}

/// A Kodi add-on scraper that is listed but cannot run, with the reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UnsupportedAddonDto {
    pub addon_id: String,
    pub addon_name: String,
    /// `movies` or `tvshows`.
    pub content: String,
    pub reason: String,
}

/// The providers the Providers page offers for the choices (default, fallback, universal).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCatalogueDto {
    pub providers: Vec<MetadataProviderDto>,
    pub unsupported_addons: Vec<UnsupportedAddonDto>,
    /// The fields the universal provider can take from a chosen source.
    pub universal_fields: Vec<String>,
}

/// What to scrape and how.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ScrapeRequestDto {
    pub provider: String,
    pub provider_id: String,
    /// ISO 639 code of the language to scrape in.
    pub language: String,
    /// The field keys to write (`cv_domain::MetaField::key`).
    pub fields: Vec<String>,
    /// Write only the fields that are empty.
    pub do_not_overwrite: bool,
    /// The title is already in the library under another item; this item is a second version.
    pub second_version: bool,
}

/// The batch modes the UI can start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum MatchModeDto {
    /// The titles flagged NEW by a scan.
    MatchNew,
    /// Every title no provider has scraped.
    MatchUnmatched,
    /// The selection, matched or not.
    MatchSelected,
    /// Refresh the selection by its stored IDs.
    RescrapeByIds,
    /// Accept the top candidate for the selection, with no review.
    ForceBestMatch,
}

impl From<MatchModeDto> for BatchMode {
    fn from(mode: MatchModeDto) -> Self {
        match mode {
            MatchModeDto::MatchNew => Self::MatchNew,
            MatchModeDto::MatchUnmatched => Self::MatchUnmatched,
            MatchModeDto::MatchSelected => Self::MatchSelected,
            MatchModeDto::RescrapeByIds => Self::RescrapeByIds,
            MatchModeDto::ForceBestMatch => Self::ForceBestMatch,
        }
    }
}

/// A match batch that was queued.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MatchStartedDto {
    /// Titles that were queued.
    pub planned: u32,
    /// Titles no ready provider can serve.
    pub without_provider: u32,
    /// The jobs in the Activity drawer, one per provider.
    pub job_ids: Vec<u32>,
}

// ---- state-level operations (testable without a window) --------------------------------------

impl MatchState {
    /// Queues a batch. Must run inside the async runtime.
    pub async fn start_batch(
        &self,
        mode: MatchModeDto,
        items: Option<Vec<String>>,
    ) -> CommandResult<MatchStartedDto> {
        let service = self.get()?;
        let selection = items.as_deref().map(parse_items).transpose()?;
        let mode = BatchMode::from(mode);
        let batch = blocking_in_runtime(move || service.submit(mode, selection.as_deref())).await?;
        Ok(MatchStartedDto {
            planned: u32::try_from(batch.planned).unwrap_or(u32::MAX),
            without_provider: u32::try_from(batch.no_provider.len()).unwrap_or(u32::MAX),
            job_ids: batch
                .jobs
                .iter()
                .map(|job| u32::try_from(job.job.id().0).unwrap_or(u32::MAX))
                .collect(),
        })
    }
}

/// Runs `work` on the blocking pool of the runtime it was called in. The job manager spawns its
/// workers on the current runtime, so the batch must be submitted from a thread that belongs to
/// it.
async fn blocking_in_runtime<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, MatchServiceError> + Send + 'static,
) -> CommandResult<T> {
    let handle = tokio::runtime::Handle::current();
    blocking(move || {
        let _entered = handle.enter();
        work()
    })
    .await
}

// ---- the batch commands ----------------------------------------------------------------------

/// Queues a match batch in the background. `item_ids` is the selection for the modes that work on
/// one; for `match_new` and `match_unmatched` it narrows the library-wide set when given.
pub async fn start_match_batch(
    state: &MatchState,
    mode: MatchModeDto,
    item_ids: Option<Vec<String>>,
) -> CommandResult<MatchStartedDto> {
    state.start_batch(mode, item_ids).await
}

/// The Match selection action: identifies the selected titles in the background. The uncertain
/// ones arrive in Needs review. Resolves to how many were queued.
pub async fn match_library_items(state: &MatchState, item_ids: Vec<String>) -> CommandResult<u32> {
    let started = state
        .start_batch(MatchModeDto::MatchSelected, Some(item_ids))
        .await?;
    Ok(started.planned)
}

// ---- the review inbox ------------------------------------------------------------------------

/// The pending items of the Needs review inbox, the most confident first.
pub async fn list_review_items(state: &MatchState) -> CommandResult<Vec<ReviewItemDto>> {
    let service = state.get()?;
    let entries = blocking(move || service.pending(REVIEW_LIST_LIMIT)).await?;
    let posters = state
        .poster_keys(
            entries
                .iter()
                .flat_map(|entry| &entry.candidates)
                .filter_map(|candidate| candidate.poster_url.as_deref()),
        )
        .await;
    Ok(entries
        .into_iter()
        .map(|entry| ReviewItemDto::of(entry, &posters))
        .collect())
}

/// How many items need review; feeds the sidebar. It is the lifecycle count: uncertain matches
/// and titles that could not be matched, so the badge, Home and the
/// Needs review page agree.
pub async fn count_review_items(state: &MatchState, library: &LibraryState) -> CommandResult<u32> {
    if let Some(lifecycle) = library.lifecycle().cloned() {
        return blocking(move || {
            let of = |kind| lifecycle.counts(kind).map(|c| c.needs_review);
            let movies = of(cv_domain::ItemKind::Movie)?;
            Ok(movies.saturating_add(of(cv_domain::ItemKind::Show)?))
        })
        .await;
    }
    let service = state.get()?;
    blocking(move || service.pending_count()).await
}

/// Matches the item of a review row to the candidate and scrapes it. The item leaves the queue.
pub async fn accept_review_candidate(
    state: &MatchState,
    review_id: u32,
    provider: String,
    provider_id: String,
) -> CommandResult<()> {
    let service = state.get()?;
    let choice = Choice {
        provider,
        provider_id,
    };
    service
        .accept(row_id(review_id), &choice)
        .await
        .map(|_| ())
        .map_err(Into::into)
}

/// Accepts the best candidate of every given review row. Resolves to the rows that were accepted.
pub async fn accept_top_review_candidates(
    state: &MatchState,
    review_ids: Vec<u32>,
) -> CommandResult<Vec<u32>> {
    let service = state.get()?;
    let ids: Vec<i64> = review_ids.into_iter().map(row_id).collect();
    let accepted = service.accept_top(&ids).await?;
    Ok(accepted.into_iter().map(small).collect())
}

/// Marks the item "not a catalogued title". Scans never queue it again.
pub async fn ignore_review_item(state: &MatchState, review_id: u32) -> CommandResult<()> {
    let service = state.get()?;
    blocking(move || service.ignore(row_id(review_id))).await
}

/// Puts accepted or ignored rows back in the queue (the Undo of both). An item accepted in this
/// session is put back as it was before the scrape; its files on disk stay.
pub async fn reopen_review_items(state: &MatchState, review_ids: Vec<u32>) -> CommandResult<()> {
    let service = state.get()?;
    let ids: Vec<i64> = review_ids.into_iter().map(row_id).collect();
    blocking(move || service.reopen(&ids)).await
}

/// Searches by title or by an ID shortcut for the item of a review row, with the usual scoring.
pub async fn search_review_candidates(
    state: &MatchState,
    review_id: u32,
    query: String,
) -> CommandResult<Vec<ReviewCandidateDto>> {
    let service = state.get()?;
    let found = service.search_review(row_id(review_id), &query).await?;
    let posters = state
        .poster_keys(found.iter().filter_map(|c| c.poster_url.as_deref()))
        .await;
    Ok(found
        .into_iter()
        .map(|candidate| ReviewCandidateDto::of(candidate, &posters))
        .collect())
}

// ---- the search-and-scrape panel -------------------------------------------------------------

/// Every metadata provider with whether it is ready, the Kodi add-ons that cannot be used and
/// the fields the universal provider combines. The choices themselves are the `providers.*`
/// settings.
pub async fn list_metadata_providers(state: &MatchState) -> CommandResult<ProviderCatalogueDto> {
    let service = state.get()?;
    let catalogue = service.provider_catalogue();
    Ok(ProviderCatalogueDto {
        providers: catalogue
            .providers
            .into_iter()
            .map(|p| MetadataProviderDto {
                id: p.id,
                name: p.name,
                movies: p.movies,
                shows: p.shows,
                ready: p.ready,
                kodi: p.kodi,
                can_fallback: p.can_fallback,
            })
            .collect(),
        unsupported_addons: catalogue
            .unsupported_addons
            .into_iter()
            .map(|a| UnsupportedAddonDto {
                addon_id: a.addon_id,
                addon_name: a.addon_name,
                content: a.content,
                reason: a.reason,
            })
            .collect(),
        universal_fields: catalogue.universal_fields,
    })
}

/// What a scrape from the Scrape workspace did, for the Undo toast.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ScrapeAppliedDto {
    /// Pass to `undo_scrape` to put the title back as it was; `None` when it cannot be undone
    /// or when a History entry records the change.
    pub undo: Option<String>,
    /// The History entry of a change of match: pass it to `history_undo`. A first scrape or a
    /// scrape with the same match has none.
    pub history_entry: Option<String>,
}

/// Links the item to the result and writes the chosen fields. Locks and do-not-overwrite are
/// honoured. The answer carries the token that undoes the scrape.
pub async fn apply_scrape(
    state: &MatchState,
    item_id: String,
    request: ScrapeRequestDto,
) -> CommandResult<ScrapeAppliedDto> {
    let service = state.get()?;
    let item = parse_item(&item_id)?;
    let apply = ScrapeApply {
        provider: request.provider,
        provider_id: request.provider_id,
        language: request.language,
        fields: request.fields,
        do_not_overwrite: request.do_not_overwrite,
        second_version: request.second_version,
    };
    let token = service.keep_scrape_snapshot(item);
    match service.scrape_apply_recorded(item, &apply).await {
        Ok((_, entry)) => {
            if entry.is_some()
                && let Some(token) = token
            {
                // The History entry is the Undo; the copy kept in memory is not needed.
                service.forget_scrape(token);
            }
            Ok(ScrapeAppliedDto {
                undo: token
                    .filter(|_| entry.is_none())
                    .map(|token| token.to_string()),
                history_entry: entry.map(|id| id.to_string()),
            })
        }
        Err(error) => {
            if let Some(token) = token {
                service.forget_scrape(token);
            }
            Err(error.into())
        }
    }
}

/// What the search-only pass found for a title.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SearchOnlyDto {
    /// Best first.
    pub candidates: Vec<ReviewCandidateDto>,
    /// The top candidate scores 90 percent or more and is not tied.
    pub confident: bool,
}

/// Searches for a New title and scores the results. Nothing is written: no title, NFO, artwork or
/// stage changes. An empty `query` uses the title's own name and year.
pub async fn search_new_title(
    state: &MatchState,
    item_id: String,
    query: String,
    language: String,
) -> CommandResult<SearchOnlyDto> {
    let service = state.get()?;
    let found = service
        .search_only(parse_item(&item_id)?, &query, &language)
        .await?;
    let posters = state
        .poster_keys(
            found
                .candidates
                .iter()
                .filter_map(|c| c.poster_url.as_deref()),
        )
        .await;
    Ok(SearchOnlyDto {
        candidates: found
            .candidates
            .into_iter()
            .map(|candidate| ReviewCandidateDto::of(candidate, &posters))
            .collect(),
        confident: found.confident,
    })
}

/// The title the item is linked to, scored like the search results, for the match picker to pin
/// first. `None` when the title has no ID the provider can look up.
pub async fn current_match_candidate(
    state: &MatchState,
    item_id: String,
    language: String,
) -> CommandResult<Option<ReviewCandidateDto>> {
    let service = state.get()?;
    let Some(current) = service
        .current_match(parse_item(&item_id)?, &language)
        .await?
    else {
        return Ok(None);
    };
    let posters = state
        .poster_keys(current.poster_url.as_deref().into_iter())
        .await;
    Ok(Some(ReviewCandidateDto::of(current, &posters)))
}

/// A title that Accept all confident scraped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConfidentAcceptedDto {
    pub item_id: String,
    /// Pass to `undo_scrape` to put the title back as it was; `None` when it cannot be undone.
    pub undo: Option<String>,
    /// The review row the title was accepted from: `reopen_review_items` is its Undo.
    pub review_id: Option<u32>,
}

/// What Accept all confident did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AcceptConfidentDto {
    pub accepted: Vec<ConfidentAcceptedDto>,
    /// The one History entry of the run: pass it to `history_undo` to take every title back.
    pub history_entry: Option<String>,
    /// Titles with no confident top candidate: left as they were.
    pub skipped: Vec<String>,
    /// Titles whose scrape failed or was refused: left as they were.
    pub failed: Vec<String>,
}

/// Scrapes every given title that has a confident top candidate (90 percent or more, not tied)
/// with that candidate. Runs only when the user asks.
pub async fn accept_confident_titles(
    state: &MatchState,
    item_ids: Vec<String>,
) -> CommandResult<AcceptConfidentDto> {
    let service = state.get()?;
    let ids = parse_items(&item_ids)?;
    let done = service.accept_confident(&ids).await?;
    let text = |ids: Vec<ItemId>| ids.into_iter().map(|id| id.0.to_string()).collect();
    Ok(AcceptConfidentDto {
        history_entry: done.history_entry.map(|id| id.to_string()),
        accepted: done
            .accepted
            .into_iter()
            .map(|a| ConfidentAcceptedDto {
                item_id: a.item.0.to_string(),
                undo: a.undo.map(|token| token.to_string()),
                review_id: a.review_id.map(small),
            })
            .collect(),
        skipped: text(done.skipped),
        failed: text(done.failed),
    })
}

/// Puts a title back as it was before a scrape from the workspace. Files already written stay.
pub async fn undo_scrape(state: &MatchState, token: String) -> CommandResult<()> {
    let service = state.get()?;
    let token: i64 = token.parse().map_err(|_| MatchCommandError {
        code: "invalid".to_owned(),
        message: "That scrape cannot be undone.".to_owned(),
    })?;
    blocking(move || service.undo_scrape(token)).await
}
