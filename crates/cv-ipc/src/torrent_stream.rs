//! The Torrent search page over IPC: the title search and the release
//! search, both streamed through the generic provider engine (`provider_stream`), so the status
//! chips, Retry and `cancel_search` work as they do in the Scrape workspace.
//!
//! * `torrent_title_search` asks TMDB for movies and shows (two providers, `movies` and `shows`)
//!   and sends the hits with their posters (`cv://img` keys), the best match first.
//! * `torrent_title_details`, `torrent_title_seasons` and `torrent_title_episodes` read what the
//!   page needs once a title is confirmed.
//! * `torrent_release_search` builds the query for the confirmed title and scope, and runs every
//!   source that applies as its own provider of one search that shares one merge
//!   (`cv_torrent::search::SearchSession`): a release returned by three sources is one row, sent
//!   again under the same `id` as sources add to it, with `absorbs` naming rows to drop. A source
//!   that does not apply is listed in the start reply with the reason and never contacted.
//!
//! The rules live in `cv_torrent` and `cv_app::torrents`; this module adapts them to DTOs.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use async_trait::async_trait;
use cv_app::artwork::ArtworkService;
use cv_app::torrents::{
    EpisodeInfo, LandingTitle, LibraryHolding, MovieCopy, SeasonInfo, TitleDetails, TitleHit,
    TorrentService, TorrentServiceError,
};
use cv_torrent::CancelToken as TorrentCancel;
use cv_torrent::TorrentError;
use cv_torrent::model::{
    Hdr, MediaKind, Resolution, Scope, SkipReason, SourceError, SourceId, SourceKind, Support,
};
use cv_torrent::rank::{FlagKind, PackKind, Relevance, Upgrade};
use cv_torrent::release::{Audio, Codec};
use cv_torrent::search::{ResultRow, SearchSession, Sink, Summary};
use cv_torrent::source::TorrentSource;
use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::sync::mpsc;

use crate::matching::{
    POSTER_READY, POSTER_WAIT, PosterKeys, ScrapeLinkDto, ScrapeProviderDto, poster_keys_of,
};
use crate::provider_stream::{
    CancelToken, Progress, ProviderOutcome, SearchProvider, SearchState, Sink as EventSink,
};
use cv_app::services::AppServices;
use cv_artwork::library_source::LibrarySources;
use cv_artwork::thumb::ThumbCache;

/// The provider ids of the title search.
pub const MOVIES: &str = "movies";
pub const SHOWS: &str = "shows";

/// Why a Torrent search command did not run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct TorrentCommandError {
    /// Stable code: `notice_not_accepted`, `needs_key`, `key_rejected`, `not_found`, `provider`,
    /// `library`, `invalid`, `unavailable`, `invalid_magnet`, `no_handler` or `failed`.
    pub code: String,
    /// Plain sentence for the user.
    pub message: String,
}

impl TorrentCommandError {
    pub(crate) fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            message: message.into(),
        }
    }

    pub(crate) fn unavailable() -> Self {
        Self::new(
            "unavailable",
            "CineVault could not open the library, so Torrent search is not available.",
        )
    }
}

impl From<TorrentServiceError> for TorrentCommandError {
    fn from(error: TorrentServiceError) -> Self {
        let code = match &error {
            TorrentServiceError::Torrent(TorrentError::NoticeNotAccepted) => "notice_not_accepted",
            TorrentServiceError::Torrent(_) => "invalid",
            TorrentServiceError::NeedsKey => "needs_key",
            TorrentServiceError::KeyRejected => "key_rejected",
            TorrentServiceError::NoMetadataProvider => "unavailable",
            TorrentServiceError::NotFound => "not_found",
            TorrentServiceError::Provider { .. } => "provider",
            TorrentServiceError::Library(_) => "library",
        };
        Self::new(code, error.to_string())
    }
}

impl From<TorrentError> for TorrentCommandError {
    fn from(error: TorrentError) -> Self {
        TorrentServiceError::from(error).into()
    }
}

pub type CommandResult<T> = Result<T, TorrentCommandError>;

/// Torrent search in Tauri state.
pub struct TorrentState {
    pub(crate) service: Option<Arc<TorrentService>>,
    pub(crate) indexer: Option<Arc<cv_app::torrents::indexer::IndexerServiceManager>>,
    artwork: Option<Arc<ArtworkService>>,
    data_dir: Option<std::path::PathBuf>,
}

impl TorrentState {
    pub fn from_services(services: Option<&Arc<AppServices>>) -> Self {
        let state = Self {
            service: services.map(|s| Arc::clone(s.torrents())),
            indexer: services.map(|s| Arc::clone(s.indexer())),
            artwork: services.map(|s| Arc::clone(s.artwork())),
            data_dir: services.map(|s| s.location().root().to_path_buf()),
        };
        // Torrent files downloaded for a hand-over in an earlier run are not kept.
        if let Some(root) = &state.data_dir {
            crate::torrent::clear_tmp(&root.join("torrents").join("tmp"));
        }
        state
    }

    /// The data folder, when the app has one.
    pub(crate) fn data_dir(&self) -> Option<std::path::PathBuf> {
        self.data_dir.clone()
    }

    pub(crate) fn indexer(
        &self,
    ) -> CommandResult<Arc<cv_app::torrents::indexer::IndexerServiceManager>> {
        self.indexer
            .clone()
            .ok_or_else(TorrentCommandError::unavailable)
    }

    pub(crate) fn get(&self) -> CommandResult<Arc<TorrentService>> {
        self.service
            .clone()
            .ok_or_else(TorrentCommandError::unavailable)
    }
}

// ------------------------------------------------------------------ title DTOs

/// A movie or show found on TMDB.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TorrentTitleHitDto {
    /// `movie:123` or `show:456`; stable for the search.
    pub id: String,
    /// `movie` or `show`.
    pub kind: String,
    pub tmdb_id: u32,
    pub title: String,
    pub original_title: Option<String>,
    pub year: Option<u16>,
    pub overview: Option<String>,
    /// Content key of the cached poster (`cv://img`); `None` shows a placeholder.
    pub poster_key: Option<String>,
    pub imdb_id: Option<String>,
    /// How well the title fits what was typed, 0 to 1.
    pub confidence: f64,
    /// Set when the library holds this title.
    pub in_library: Option<ScrapeLinkDto>,
}

pub fn kind_text(kind: MediaKind) -> &'static str {
    match kind {
        MediaKind::Movie => "movie",
        MediaKind::Show => "show",
    }
}

pub fn parse_kind(text: &str) -> CommandResult<MediaKind> {
    match text {
        "movie" => Ok(MediaKind::Movie),
        "show" => Ok(MediaKind::Show),
        other => Err(TorrentCommandError::new(
            "invalid",
            format!("{other:?} is not a movie or a show."),
        )),
    }
}

pub fn tmdb_u32(id: u64) -> Option<u32> {
    u32::try_from(id).ok()
}

pub fn link_dto(link: Option<&cv_app::torrents::LibraryLink>) -> Option<ScrapeLinkDto> {
    link.map(|link| ScrapeLinkDto {
        item_id: link.item.0.to_string(),
        name: link.name.clone(),
    })
}

pub fn title_hit_dto(hit: TitleHit, posters: &PosterKeys) -> Option<TorrentTitleHitDto> {
    let tmdb_id = tmdb_u32(hit.tmdb_id)?;
    let non_empty = |text: String| (!text.trim().is_empty()).then_some(text);
    Some(TorrentTitleHitDto {
        id: format!("{}:{}", kind_text(hit.kind), hit.tmdb_id),
        kind: kind_text(hit.kind).to_owned(),
        tmdb_id,
        original_title: non_empty(hit.original_title).filter(|o| *o != hit.title),
        title: hit.title,
        year: hit.year,
        overview: non_empty(hit.overview),
        poster_key: hit
            .poster_url
            .as_deref()
            .and_then(|url| posters.get(url))
            .cloned(),
        imdb_id: hit.imdb_id,
        confidence: hit.confidence,
        in_library: link_dto(hit.in_library.as_ref()),
    })
}

/// What a confirmed title is.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TorrentTitleDetailsDto {
    pub kind: String,
    pub tmdb_id: u32,
    pub imdb_id: Option<String>,
    pub title: String,
    pub original_title: Option<String>,
    pub year: Option<u16>,
    pub runtime_minutes: Option<u32>,
    pub anime: bool,
    pub poster_key: Option<String>,
    pub in_library: Option<ScrapeLinkDto>,
    /// Content key of the cached backdrop; `None` while it downloads (ask again) or when there is
    /// none, which shows the generated one.
    pub backdrop_key: Option<String>,
    /// `R`, `TV-MA`.
    pub certification: Option<String>,
    /// The TMDB rating on a ten-point scale.
    pub rating: Option<f64>,
    /// The genres as words.
    pub genres: Vec<String>,
    pub overview: Option<String>,
    /// The copy of a movie the library holds.
    pub library_copy: Option<TorrentLibraryCopyDto>,
    /// The episodes of a show the library holds, in aired order.
    pub held_episodes: Vec<TorrentEpisodeRefDto>,
}

/// A movie's copy in the library: "1080p WEB-DL · 5.6 GB · <file name>".
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TorrentLibraryCopyDto {
    /// `2160p`, `1080p`, `720p`, `480p` or `unknown` (media info not read yet).
    pub resolution: String,
    /// `Remux`, `BluRay`, `WEB-DL`, ... or none.
    pub source: Option<String>,
    /// `DV`, `HDR10+`, `HDR10` or `HLG`.
    pub hdr: Option<String>,
    pub size: f64,
    pub file_name: String,
}

/// An episode by its numbers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TorrentEpisodeRefDto {
    pub season: u16,
    pub episode: u16,
}

/// A library title on a landing strip.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TorrentLandingTitleDto {
    /// The library item, as text.
    pub item_id: String,
    /// `movie` or `show`.
    pub kind: String,
    pub tmdb_id: u32,
    pub title: String,
    pub year: Option<u16>,
    /// The library poster (`cv://img` key); `None` shows the generated one.
    pub poster: Option<crate::library::PosterDto>,
    /// The resolution of a movie's copy, `2160p` to `480p`; none for a show.
    pub resolution: Option<String>,
    /// When it was added, epoch milliseconds.
    pub added: Option<f64>,
}

/// The library strips of the Torrent search landing.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TorrentLandingDto {
    /// Movies below 2160p, the lowest first.
    pub upgrades: Vec<TorrentLandingTitleDto>,
    /// How many movies are below 2160p in all.
    pub upgrade_total: u32,
    /// The newest movies and shows.
    pub recent: Vec<TorrentLandingTitleDto>,
}

/// A season of a show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TorrentSeasonDto {
    pub number: u16,
    pub name: String,
    pub episode_count: u32,
    /// `YYYY-MM-DD`.
    pub air_date: Option<String>,
}

/// An episode of a season.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TorrentEpisodeDto {
    pub number: u16,
    pub name: String,
    /// `YYYY-MM-DD`.
    pub air_date: Option<String>,
}

impl From<SeasonInfo> for TorrentSeasonDto {
    fn from(season: SeasonInfo) -> Self {
        Self {
            number: season.number,
            name: season.name,
            episode_count: season.episode_count,
            air_date: season.air_date,
        }
    }
}

impl From<EpisodeInfo> for TorrentEpisodeDto {
    fn from(episode: EpisodeInfo) -> Self {
        Self {
            number: episode.number,
            name: episode.name,
            air_date: episode.air_date,
        }
    }
}

// ------------------------------------------------------------------ title search

/// The request of a title search.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TorrentTitleSearchRequestDto {
    /// The search id the page cancels and retries by.
    pub search_id: String,
    /// What the person typed: a title, `Title (Year)`, `tt0063350`, `tmdb:123` or an address.
    pub query: String,
}

/// The providers a started title search will ask.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TorrentTitleStartDto {
    pub providers: Vec<ScrapeProviderDto>,
}

struct TitleSearch {
    service: Arc<TorrentService>,
    artwork: Option<Arc<ArtworkService>>,
    kind: MediaKind,
    query: String,
}

impl TitleSearch {
    async fn posters(&self, hits: &[TitleHit], wait: Duration) -> PosterKeys {
        poster_keys_of(
            self.artwork.as_ref(),
            hits.iter().filter_map(|h| h.poster_url.as_deref()),
            wait,
        )
        .await
    }

    fn dtos(hits: Vec<TitleHit>, posters: &PosterKeys) -> Vec<TorrentTitleHitDto> {
        hits.into_iter()
            .filter_map(|hit| title_hit_dto(hit, posters))
            .collect()
    }

    fn refusal(error: TorrentServiceError) -> ProviderOutcome<TorrentTitleHitDto> {
        match error {
            TorrentServiceError::NeedsKey => ProviderOutcome::NeedsKey,
            TorrentServiceError::KeyRejected => ProviderOutcome::key_rejected(),
            TorrentServiceError::NotFound => ProviderOutcome::results(Vec::new()),
            TorrentServiceError::Provider { message, retryable } => {
                ProviderOutcome::failed(message, retryable)
            }
            other => ProviderOutcome::failed(other.to_string(), false),
        }
    }
}

#[async_trait]
impl SearchProvider<TorrentTitleHitDto> for TitleSearch {
    fn id(&self) -> &str {
        match self.kind {
            MediaKind::Movie => MOVIES,
            MediaKind::Show => SHOWS,
        }
    }

    async fn search(&self, _cancel: &CancelToken) -> ProviderOutcome<TorrentTitleHitDto> {
        match self.service.title_hits(self.kind, &self.query).await {
            Ok(hits) => {
                let posters = self.posters(&hits, POSTER_WAIT).await;
                ProviderOutcome::results(Self::dtos(hits, &posters))
            }
            Err(error) => Self::refusal(error),
        }
    }

    /// The hits are listed at once with the posters the cache already holds; the posters that
    /// download meanwhile follow as an update of the same entries.
    async fn search_progressive(
        &self,
        _cancel: &CancelToken,
        progress: &Progress<TorrentTitleHitDto>,
    ) -> ProviderOutcome<TorrentTitleHitDto> {
        let hits = match self.service.title_hits(self.kind, &self.query).await {
            Ok(hits) => hits,
            Err(error) => return Self::refusal(error),
        };
        let wanted = hits.iter().any(|hit| hit.poster_url.is_some());
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

pub fn title_providers(
    service: &Arc<TorrentService>,
    artwork: Option<&Arc<ArtworkService>>,
    query: &str,
) -> Vec<Arc<dyn SearchProvider<TorrentTitleHitDto>>> {
    [MediaKind::Movie, MediaKind::Show]
        .into_iter()
        .map(|kind| {
            Arc::new(TitleSearch {
                service: Arc::clone(service),
                artwork: artwork.cloned(),
                kind,
                query: query.to_owned(),
            }) as Arc<dyn SearchProvider<TorrentTitleHitDto>>
        })
        .collect()
}

/// Searches TMDB for movies and shows as the person types. The movies and the shows answer as
/// two providers (`movies`, `shows`); the hits come best match first with their posters.
pub async fn torrent_title_search(
    state: &TorrentState,
    search: &SearchState,
    request: TorrentTitleSearchRequestDto,
    on_event: EventSink<TorrentTitleHitDto>,
) -> CommandResult<TorrentTitleStartDto> {
    let service = state.get()?;
    service.ensure_accepted()?;
    let providers = title_providers(&service, state.artwork.as_ref(), &request.query);
    search
        .start_on_sink(&request.search_id, providers, on_event)
        .map_err(|error| TorrentCommandError::new("unavailable", error.message))?;
    Ok(TorrentTitleStartDto {
        providers: vec![
            ScrapeProviderDto {
                key: MOVIES.to_owned(),
                name: "Movies".to_owned(),
            },
            ScrapeProviderDto {
                key: SHOWS.to_owned(),
                name: "Shows".to_owned(),
            },
        ],
    })
}

/// The details of a confirmed title: IDs, original title, year, runtime, whether it is anime and
/// the library link.
pub async fn torrent_title_details(
    state: &TorrentState,
    kind: String,
    tmdb_id: u32,
) -> CommandResult<TorrentTitleDetailsDto> {
    let service = state.get()?;
    let kind = parse_kind(&kind)?;
    let details: TitleDetails = service.title_details(kind, u64::from(tmdb_id)).await?;
    // The backdrop is a band of the header: TMDB's 1280-wide size is plenty and downloads fast.
    let backdrop_url = details.backdrop_url.as_deref().map(smaller_backdrop);
    let posters = poster_keys_of(
        state.artwork.as_ref(),
        details
            .poster_url
            .as_deref()
            .into_iter()
            .chain(backdrop_url.as_deref()),
        POSTER_WAIT,
    )
    .await;
    let (library_copy, held_episodes) = match &details.holding {
        Some(LibraryHolding::Movie(copy)) => (Some(library_copy_dto(copy)), Vec::new()),
        Some(LibraryHolding::Show(held)) => (
            None,
            held.iter()
                .map(|&(season, episode)| TorrentEpisodeRefDto { season, episode })
                .collect(),
        ),
        None => (None, Vec::new()),
    };
    Ok(TorrentTitleDetailsDto {
        kind: kind_text(details.kind).to_owned(),
        tmdb_id,
        imdb_id: details.ids.imdb,
        title: details.title,
        original_title: details.original_title,
        year: details.year,
        runtime_minutes: details.runtime_minutes,
        anime: details.anime,
        poster_key: details
            .poster_url
            .as_deref()
            .and_then(|url| posters.get(url))
            .cloned(),
        in_library: link_dto(details.library_item.as_ref()),
        backdrop_key: backdrop_url
            .as_deref()
            .and_then(|url| posters.get(url))
            .cloned(),
        certification: details.certification,
        rating: details
            .rating
            .map(|value| (f64::from(value) * 10.0).round() / 10.0),
        genres: details.genres,
        overview: details.overview,
        library_copy,
        held_episodes,
    })
}

/// A TMDB backdrop address at the 1280-wide size instead of the original.
pub fn smaller_backdrop(url: &str) -> String {
    url.replacen("/t/p/original/", "/t/p/w1280/", 1)
}

pub fn library_copy_dto(copy: &MovieCopy) -> TorrentLibraryCopyDto {
    #[allow(clippy::cast_precision_loss)]
    let size = copy.size_bytes as f64;
    TorrentLibraryCopyDto {
        resolution: resolution_text(copy.resolution).to_owned(),
        source: media_source_text(&copy.source, false),
        hdr: hdr_text(copy.hdr).map(str::to_owned),
        size,
        file_name: copy.file_name.clone(),
    }
}

pub fn landing_title_dto(
    title: LandingTitle,
    posters: &std::collections::HashMap<cv_domain::ItemId, cv_artwork::thumb::ThumbKey>,
    cache: Option<&cv_artwork::thumb::ThumbCache>,
) -> Option<TorrentLandingTitleDto> {
    #[allow(clippy::cast_precision_loss)]
    let added = title.added_ms.map(|ms| ms as f64);
    Some(TorrentLandingTitleDto {
        item_id: title.item.0.to_string(),
        kind: kind_text(title.kind).to_owned(),
        tmdb_id: tmdb_u32(title.tmdb_id)?,
        poster: posters
            .get(&title.item)
            .map(|key| crate::library::poster_dto(cache, key)),
        resolution: match title.resolution {
            Resolution::Unknown => None,
            other => Some(resolution_text(other).to_owned()),
        },
        year: title.year,
        added,
        title: title.title,
    })
}

/// The library strips of the landing: movies below 2160p (upgrade available) and the newest
/// titles, each with its TMDB ID and library poster. Only the library is read.
pub async fn torrent_landing(
    state: &TorrentState,
    sources: Arc<LibrarySources>,
    cache: Option<Arc<ThumbCache>>,
) -> CommandResult<TorrentLandingDto> {
    let service = state.get()?;
    tokio::task::spawn_blocking(move || {
        let landing = service.landing()?;
        let ids: Vec<cv_domain::ItemId> = landing
            .upgrades
            .iter()
            .chain(&landing.recent)
            .map(|title| title.item)
            .collect();
        // Posters are a nicety: a failed read shows the generated ones.
        let posters = sources
            .posters_for(service.library(), &ids)
            .unwrap_or_else(|error| {
                tracing::debug!(%error, "landing posters not read");
                std::collections::HashMap::new()
            });
        let dto = |title| landing_title_dto(title, &posters, cache.as_deref());
        Ok(TorrentLandingDto {
            upgrade_total: u32::try_from(landing.upgrade_total).unwrap_or(u32::MAX),
            upgrades: landing.upgrades.into_iter().filter_map(dto).collect(),
            recent: landing.recent.into_iter().filter_map(dto).collect(),
        })
    })
    .await
    .map_err(|_| TorrentCommandError::new("failed", "The landing could not be read."))?
}

/// The seasons of a show.
pub async fn torrent_title_seasons(
    state: &TorrentState,
    tmdb_id: u32,
) -> CommandResult<Vec<TorrentSeasonDto>> {
    let service = state.get()?;
    Ok(service
        .seasons(u64::from(tmdb_id))
        .await?
        .into_iter()
        .map(Into::into)
        .collect())
}

/// The episodes of one season.
pub async fn torrent_title_episodes(
    state: &TorrentState,
    tmdb_id: u32,
    season: u16,
) -> CommandResult<Vec<TorrentEpisodeDto>> {
    let service = state.get()?;
    Ok(service
        .episodes(u64::from(tmdb_id), season)
        .await?
        .into_iter()
        .map(Into::into)
        .collect())
}

// ------------------------------------------------------------------ release search DTOs

/// The request of a release search.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TorrentSearchRequestDto {
    pub search_id: String,
    /// `movie` or `show`.
    pub kind: String,
    pub tmdb_id: u32,
    /// For a show: the season, or none for the whole show.
    pub season: Option<u16>,
    /// For a show: the episode of that season.
    pub episode: Option<u16>,
}

/// A source of a release search and whether it takes part.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TorrentSourceDto {
    pub id: String,
    pub name: String,
    /// `built-in`, `indexer` or `torznab`.
    pub kind: String,
    /// The reason the source is not asked: `no-imdb-id`, `movies-only`, `tv-only`,
    /// `anime-only` or `show-scope`.
    pub skipped: Option<String>,
}

/// What a started release search tells the page at once; the releases follow on the Channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TorrentSearchStartDto {
    /// Every source, in the order they are listed, with the reason when it is skipped. Those
    /// that are not skipped answer on the Channel as providers with the same id.
    pub sources: Vec<TorrentSourceDto>,
}

/// A release's own page on one source's site.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SourcePageDto {
    /// The id of the source (one of `TorrentReleaseDto::sources`).
    pub source: String,
    /// An `http` or `https` address; open it with `torrent_open_page`.
    pub details_url: String,
}

/// A warning about a release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FlagDto {
    /// `no-seeders`, `too-small`, `payload`, `cam`, `telesync`, `telecine` or `screener`.
    pub kind: String,
    /// The warning in plain words.
    pub text: String,
}

/// One release, merged from every source that returned it.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TorrentReleaseDto {
    /// The merge key; a row sent again with the same id replaces the first.
    pub id: String,
    /// Rows that were folded into this one; drop them.
    pub absorbs: Vec<String>,
    pub name: String,
    pub size: Option<f64>,
    pub seeders: Option<u32>,
    pub leechers: Option<u32>,
    /// Epoch milliseconds.
    pub published: Option<f64>,
    /// The ids of the sources that returned it.
    pub sources: Vec<String>,
    /// The release's page on each source that gave one.
    pub pages: Vec<SourcePageDto>,
    pub magnet: Option<String>,
    pub torrent_url: Option<String>,
    pub info_hash: Option<String>,
    /// `2160p`, `1080p`, `720p`, `480p` or `unknown`.
    pub resolution: String,
    /// `Remux`, `BluRay`, `WEB-DL`, `WEBRip`, `HDTV`, `DVD`, ... or none.
    pub source: Option<String>,
    /// `HEVC`, `AVC`, `AV1` or `XviD`.
    pub codec: Option<String>,
    /// `DV`, `HDR10+`, `HDR10` or `HLG`.
    pub hdr: Option<String>,
    /// `Atmos`, `TrueHD`, `DTS-HD`, `DTS`, `DD+`, `AC3` or `AAC`, with the channels when known.
    pub audio: Option<String>,
    /// `episodes`, `season`, `seasons` or `complete` for a pack.
    pub pack: Option<String>,
    pub flags: Vec<FlagDto>,
    /// `better`, `same` or `worse` against the library copy; none when not in the library.
    pub upgrade: Option<String>,
    /// Whether this is the title (and scope) searched for. Others are hidden until asked for.
    pub matching: bool,
}

pub fn resolution_text(resolution: Resolution) -> &'static str {
    match resolution {
        Resolution::R2160 => "2160p",
        Resolution::R1080 => "1080p",
        Resolution::R720 => "720p",
        Resolution::R480 => "480p",
        Resolution::Unknown => "unknown",
    }
}

pub fn source_text(parsed: &cv_torrent::release::ParsedRelease) -> Option<String> {
    media_source_text(&parsed.source, parsed.remux)
}

/// The words of a release's or a copy's source: `Remux`, `BluRay`, `WEB-DL`, ...
pub fn media_source_text(source: &cv_domain::MediaSource, remux: bool) -> Option<String> {
    use cv_domain::MediaSource;
    if remux {
        return Some("Remux".to_owned());
    }
    match source {
        MediaSource::UhdBluray | MediaSource::Bluray => Some("BluRay".to_owned()),
        MediaSource::WebDl => Some("WEB-DL".to_owned()),
        MediaSource::WebRip => Some("WEBRip".to_owned()),
        MediaSource::Tv | MediaSource::HdRip => Some("HDTV".to_owned()),
        MediaSource::Dvd => Some("DVD".to_owned()),
        MediaSource::HdDvd => Some("HD DVD".to_owned()),
        MediaSource::Cam => Some("CAM".to_owned()),
        MediaSource::Ts => Some("TS".to_owned()),
        MediaSource::Tc => Some("TC".to_owned()),
        MediaSource::DvdScr => Some("SCR".to_owned()),
        _ => None,
    }
}

pub fn codec_text(codec: Codec) -> Option<&'static str> {
    match codec {
        Codec::Hevc => Some("HEVC"),
        Codec::Avc => Some("AVC"),
        Codec::Av1 => Some("AV1"),
        Codec::Xvid => Some("XviD"),
        Codec::Unknown => None,
    }
}

pub fn hdr_text(hdr: Hdr) -> Option<&'static str> {
    match hdr {
        Hdr::DolbyVision => Some("DV"),
        Hdr::Hdr10Plus => Some("HDR10+"),
        Hdr::Hdr10 => Some("HDR10"),
        Hdr::Hlg => Some("HLG"),
        Hdr::None => None,
    }
}

pub fn audio_text(audio: Audio, channels: Option<&str>) -> Option<String> {
    let name = match audio {
        Audio::Atmos => "Atmos",
        Audio::TrueHd => "TrueHD",
        Audio::DtsHd => "DTS-HD",
        Audio::Dts => "DTS",
        Audio::DdPlus => "DD+",
        Audio::Ac3 => "AC3",
        Audio::Aac => "AAC",
        Audio::Unknown => return None,
    };
    Some(match channels {
        Some(channels) => format!("{name} {channels}"),
        None => name.to_owned(),
    })
}

pub fn flag_kind_text(kind: FlagKind) -> &'static str {
    use cv_torrent::release::LowQuality;
    match kind {
        FlagKind::NoSeeders => "no-seeders",
        FlagKind::TooSmall => "too-small",
        FlagKind::Payload => "payload",
        FlagKind::LowQuality(LowQuality::Cam) => "cam",
        FlagKind::LowQuality(LowQuality::Telesync) => "telesync",
        FlagKind::LowQuality(LowQuality::Telecine) => "telecine",
        FlagKind::LowQuality(LowQuality::Screener) => "screener",
    }
}

pub fn pack_text(pack: PackKind) -> &'static str {
    match pack {
        PackKind::Episodes => "episodes",
        PackKind::Season => "season",
        PackKind::Seasons => "seasons",
        PackKind::Complete => "complete",
    }
}

/// The DTO of a merged and judged release.
pub fn release_dto(row: ResultRow) -> TorrentReleaseDto {
    let ResultRow {
        id,
        absorbs,
        release,
        assessment,
    } = row;
    let parsed = &assessment.parsed;
    #[allow(clippy::cast_precision_loss)]
    let to_f64 = |value: u64| value as f64;
    TorrentReleaseDto {
        id,
        absorbs,
        size: release.size.map(to_f64),
        seeders: release.seeders,
        leechers: release.leechers,
        published: release.published.map(|date| {
            #[allow(clippy::cast_precision_loss)]
            let millis = date.timestamp_millis() as f64;
            millis
        }),
        sources: release.sources.iter().map(|s| s.0.clone()).collect(),
        pages: release
            .pages
            .iter()
            .map(|page| SourcePageDto {
                source: page.source.0.clone(),
                details_url: page.url.clone(),
            })
            .collect(),
        magnet: release.magnet,
        torrent_url: release.torrent_url,
        info_hash: release.info_hash,
        resolution: resolution_text(parsed.resolution).to_owned(),
        source: source_text(parsed),
        codec: codec_text(parsed.codec).map(str::to_owned),
        hdr: hdr_text(parsed.hdr).map(str::to_owned),
        audio: audio_text(parsed.audio, parsed.channels.as_deref()),
        pack: assessment.pack.map(|p| pack_text(p).to_owned()),
        flags: assessment
            .flags
            .iter()
            .map(|flag| FlagDto {
                kind: flag_kind_text(flag.kind).to_owned(),
                text: flag.text.clone(),
            })
            .collect(),
        upgrade: assessment.upgrade.map(|u| {
            match u {
                Upgrade::Better => "better",
                Upgrade::Same => "same",
                Upgrade::Worse => "worse",
            }
            .to_owned()
        }),
        matching: matches!(assessment.relevance, Relevance::Match),
        name: release.name,
    }
}

pub fn skip_text(reason: SkipReason) -> &'static str {
    match reason {
        SkipReason::NoImdbId => "no-imdb-id",
        SkipReason::MoviesOnly => "movies-only",
        SkipReason::TvOnly => "tv-only",
        SkipReason::AnimeOnly => "anime-only",
        SkipReason::ShowScope => "show-scope",
        SkipReason::Off => "off",
    }
}

pub fn source_kind_text(kind: SourceKind) -> &'static str {
    match kind {
        SourceKind::BuiltIn => "built-in",
        SourceKind::Indexer => "indexer",
        SourceKind::Torznab => "torznab",
    }
}

/// The reason code and whether asking again can help, for a failed source.
#[must_use]
pub fn failure(error: &SourceError) -> (String, bool) {
    match error {
        SourceError::Unreachable(_) => ("unreachable".to_owned(), true),
        SourceError::RateLimited => ("rate-limited".to_owned(), true),
        SourceError::Timeout => ("timeout".to_owned(), true),
        SourceError::Blocked => ("blocked".to_owned(), false),
        // The site's own short message, when it gave one, for the source line and the chip.
        SourceError::BadAnswer(message) => (
            cv_app::torrents::site_message(message).map_or_else(
                || "bad-answer".to_owned(),
                |text| format!("bad-answer: {text}"),
            ),
            false,
        ),
        SourceError::KeyRejected => (crate::provider_stream::KEY_REJECTED.to_owned(), false),
    }
}

// ------------------------------------------------------------------ release search

/// What one source's run came to, kept by a [`ProviderSink`].
#[derive(Default)]
struct RunResult {
    rows: Option<Vec<ResultRow>>,
    failed: Option<SourceError>,
}

/// Collects what a source run reports: partial rows go to a channel (drained into the engine's
/// `Progress` while the source works), the final rows or the failure are kept.
struct ProviderSink {
    partial: mpsc::UnboundedSender<Vec<TorrentReleaseDto>>,
    result: Mutex<RunResult>,
}

impl Sink for ProviderSink {
    fn started(&self, _source: &SourceId) {}

    fn partial(&self, _source: &SourceId, rows: Vec<ResultRow>) {
        let _ = self
            .partial
            .send(rows.into_iter().map(release_dto).collect());
    }

    fn results(&self, _source: &SourceId, rows: Vec<ResultRow>) {
        self.result
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .rows = Some(rows);
    }

    fn failed(&self, _source: &SourceId, error: &SourceError) {
        self.result
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .failed = Some(error.clone());
    }

    fn skipped(&self, _source: &SourceId, _reason: SkipReason) {}

    fn done(&self, _summary: &Summary) {}
}

/// One source as a provider of the generic engine. Asking it again (Retry) runs the same source
/// into the same merge.
pub struct ReleaseSearch {
    session: Arc<SearchSession>,
    source: Arc<dyn TorrentSource>,
    /// Told how the source did, for the health column of Settings.
    health: Option<Arc<TorrentService>>,
}

#[async_trait]
impl SearchProvider<TorrentReleaseDto> for ReleaseSearch {
    fn id(&self) -> &str {
        &self.source.id().0
    }

    async fn search(&self, _cancel: &CancelToken) -> ProviderOutcome<TorrentReleaseDto> {
        // Nobody listens for partial rows here; the final answer holds every row.
        let (tx, _rx) = mpsc::unbounded_channel();
        self.run(tx).await
    }

    async fn search_progressive(
        &self,
        _cancel: &CancelToken,
        progress: &Progress<TorrentReleaseDto>,
    ) -> ProviderOutcome<TorrentReleaseDto> {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let work = self.run(tx);
        tokio::pin!(work);
        loop {
            tokio::select! {
                Some(rows) = rx.recv() => progress.partial(rows),
                outcome = &mut work => {
                    // Rows reported just before the end still go out before the answer.
                    while let Ok(rows) = rx.try_recv() {
                        progress.partial(rows);
                    }
                    return outcome;
                }
            }
        }
    }
}

impl ReleaseSearch {
    /// Runs the source into the session and turns what it reported into the engine's outcome.
    async fn run(
        &self,
        partial: mpsc::UnboundedSender<Vec<TorrentReleaseDto>>,
    ) -> ProviderOutcome<TorrentReleaseDto> {
        let sink = Arc::new(ProviderSink {
            partial,
            result: Mutex::new(RunResult::default()),
        });
        // The engine drops this future when the search is cancelled, which drops the requests;
        // the token only serves the sources' own between-request checks.
        let cancel = TorrentCancel::new();
        let as_sink: Arc<dyn Sink> = sink.clone();
        let outcome = self
            .session
            .run_source(Arc::clone(&self.source), &cancel, &as_sink)
            .await;
        if let (Some(service), Some(outcome)) = (&self.health, &outcome) {
            service.note_outcome(&self.source.id().0, outcome);
        }
        let mut result = sink.result.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(error) = result.failed.take() {
            if matches!(error, SourceError::KeyRejected) {
                return ProviderOutcome::key_rejected();
            }
            let (reason, retryable) = failure(&error);
            return ProviderOutcome::failed(reason, retryable);
        }
        ProviderOutcome::results(
            result
                .rows
                .take()
                .unwrap_or_default()
                .into_iter()
                .map(release_dto)
                .collect(),
        )
    }
}

/// Splits sources into the ones that take part and the page's list of all of them.
pub fn plan_sources(
    session: &Arc<SearchSession>,
    sources: Vec<Arc<dyn TorrentSource>>,
    health: Option<&Arc<TorrentService>>,
) -> (
    Vec<TorrentSourceDto>,
    Vec<Arc<dyn SearchProvider<TorrentReleaseDto>>>,
) {
    let mut listed = Vec::new();
    let mut providers: Vec<Arc<dyn SearchProvider<TorrentReleaseDto>>> = Vec::new();
    for source in sources {
        let skipped = match source.supports(session.query()) {
            Support::Yes => None,
            Support::Skip(reason) => Some(skip_text(reason).to_owned()),
        };
        listed.push(TorrentSourceDto {
            id: source.id().0.clone(),
            name: source.name().to_owned(),
            kind: source_kind_text(source.kind()).to_owned(),
            skipped: skipped.clone(),
        });
        if skipped.is_none() {
            providers.push(Arc::new(ReleaseSearch {
                session: Arc::clone(session),
                source,
                health: health.cloned(),
            }));
        }
    }
    (listed, providers)
}

pub fn scope_of(
    kind: MediaKind,
    season: Option<u16>,
    episode: Option<u16>,
) -> CommandResult<Scope> {
    match (kind, season, episode) {
        (MediaKind::Movie, _, _) => Ok(Scope::Movie),
        (MediaKind::Show, None, None) => Ok(Scope::Show),
        (MediaKind::Show, Some(season), None) => Ok(Scope::Season(season)),
        (MediaKind::Show, Some(season), Some(episode)) => Ok(Scope::Episode(season, episode)),
        (MediaKind::Show, None, Some(_)) => Err(TorrentCommandError::new(
            "invalid",
            "An episode needs its season.",
        )),
    }
}

/// Searches every source for the releases of a confirmed title and scope. Each source answers as
/// a provider of the search (`Started`, `Partial`, `Results` or `Failed`, then `Done`); the rows
/// are merged across sources, so a release found by three arrives as one row naming all three.
/// `retry_provider` asks one source again and `cancel_search` stops the search.
pub async fn torrent_release_search(
    state: &TorrentState,
    search: &SearchState,
    request: TorrentSearchRequestDto,
    on_event: EventSink<TorrentReleaseDto>,
) -> CommandResult<TorrentSearchStartDto> {
    let service = state.get()?;
    let kind = parse_kind(&request.kind)?;
    let scope = scope_of(kind, request.season, request.episode)?;
    let details = service
        .title_details(kind, u64::from(request.tmdb_id))
        .await?;
    let query = service.build_query(&details, scope)?;
    let sources = service.sources(&query)?;
    let session = SearchSession::new(query);
    let (listed, providers) = plan_sources(&session, sources, Some(&service));
    search
        .start_on_sink(&request.search_id, providers, on_event)
        .map_err(|error| TorrentCommandError::new("unavailable", error.message))?;
    Ok(TorrentSearchStartDto { sources: listed })
}
