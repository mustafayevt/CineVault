//! The "Latest releases" feed of the Torrents page over IPC.
//!
//! * `torrent_latest` reads the next page of every source that can list its newest releases and
//!   streams the rows through the generic provider engine, one provider per source, like the
//!   release search. The feed of each kind (all, movies, TV) is one
//!   [`cv_torrent::latest::FeedSession`] kept here: later pages merge into the same rows, so a
//!   release seen again is never a second row. Opening the feed again within
//!   [`FEED_CACHE_TTL`] answers with the rows already read and asks nobody; Refresh starts over.
//! * `torrent_feed_lookup` looks one feed title up on TMDB (when the person opens its details),
//!   by IMDb ID when a source gave one, else by the title and year read from the release name,
//!   with the confidence of the title search. Answers are kept, so a title is looked up once.
//!
//! The rows carry what the release name says (kind, title, year, season, episode) and the
//! library title they match by title and year, read from the library only.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use async_trait::async_trait;
use cv_app::torrents::{LibraryIndex, TorrentService, TorrentServiceError};
use cv_torrent::CancelToken as TorrentCancel;
use cv_torrent::latest::{
    CursorState, FEED_CACHE_TTL, FeedKind, FeedSession, FeedSkip, FeedSupport, PageOutcome,
    assess_feed,
};
use cv_torrent::merge::MergedRow;
use cv_torrent::model::{MediaKind, SourceError, SourceOutcome};
use cv_torrent::query::match_key;
use cv_torrent::rank::{Assessment, Relevance, upgrade};
use cv_torrent::search::ResultRow;
use cv_torrent::source::TorrentSource;
use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::sync::mpsc;

use crate::matching::{PosterKeys, ScrapeLinkDto};
use crate::provider_stream::{
    CancelToken, Progress, ProviderOutcome, SearchProvider, SearchState, Sink as EventSink,
};
use crate::torrent_stream::{
    CommandResult, TorrentCommandError, TorrentReleaseDto, TorrentState, TorrentTitleHitDto,
    failure, kind_text, link_dto, release_dto, source_kind_text, title_hit_dto,
};

/// Below this confidence the panel says the match may be wrong and offers another.
pub const WEAK_MATCH: f64 = 0.75;
/// The most lookups kept.
const LOOKUPS_KEPT: usize = 500;

/// The feeds and lookups of the page; one per app (or server).
#[derive(Default)]
pub struct FeedState {
    sessions: Mutex<HashMap<&'static str, Arc<FeedSession>>>,
    generations: AtomicU64,
    lookups: Mutex<HashMap<String, TorrentFeedLookupDto>>,
}

impl FeedState {
    /// The feed of `kind` to answer a request with: the one kept, or a new one when there is
    /// none, when asked to refresh, or (for a first page) when the kept one is too old.
    fn session(&self, kind: FeedKind, refresh: bool, more: bool) -> Arc<FeedSession> {
        let mut sessions = self.sessions.lock().unwrap_or_else(PoisonError::into_inner);
        let key = kind_key(kind);
        if let Some(kept) = sessions.get(key)
            && !refresh
            && (more || !kept.is_stale(FEED_CACHE_TTL))
        {
            return Arc::clone(kept);
        }
        let generation = self.generations.fetch_add(1, Ordering::SeqCst) + 1;
        let session = FeedSession::new(kind, generation);
        sessions.insert(key, Arc::clone(&session));
        session
    }

    fn lookup(&self, key: &str) -> Option<TorrentFeedLookupDto> {
        self.lookups
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(key)
            .cloned()
    }

    fn keep_lookup(&self, key: String, answer: TorrentFeedLookupDto) {
        let mut lookups = self.lookups.lock().unwrap_or_else(PoisonError::into_inner);
        if lookups.len() >= LOOKUPS_KEPT {
            lookups.clear();
        }
        lookups.insert(key, answer);
    }
}

fn kind_key(kind: FeedKind) -> &'static str {
    match kind {
        FeedKind::All => "all",
        FeedKind::Movies => "movies",
        FeedKind::Tv => "tv",
    }
}

/// Reads `all`, `movies` or `tv`.
///
/// # Errors
/// `invalid` for anything else.
pub fn parse_feed_kind(text: &str) -> CommandResult<FeedKind> {
    match text {
        "all" => Ok(FeedKind::All),
        "movies" => Ok(FeedKind::Movies),
        "tv" => Ok(FeedKind::Tv),
        other => Err(TorrentCommandError::new(
            "invalid",
            format!("{other:?} is not all, movies or tv."),
        )),
    }
}

// ------------------------------------------------------------------ DTOs

/// A request for the feed.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TorrentFeedRequestDto {
    /// The search id the page cancels and retries by.
    pub search_id: String,
    /// `all`, `movies` or `tv`.
    pub kind: String,
    /// False: the first page (or the rows kept, when they are recent). True: the next page of
    /// every source that has more.
    pub more: bool,
    /// Start over: forget the rows kept and read every source's first page again.
    pub refresh: bool,
    /// For `more`: the generation of the feed the page has. Another one (the feed was
    /// refreshed meanwhile) is refused with `feed_expired`.
    pub generation: Option<String>,
}

/// One source of the feed and where it stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TorrentFeedSourceDto {
    pub id: String,
    pub name: String,
    /// `built-in`, `indexer` or `torznab`.
    pub kind: String,
    /// `asked` (answers on the stream now), `more` (has a next page), `loading` (an earlier
    /// request is still reading it), `done` (no more), `unsupported` (cannot list its newest
    /// releases), `movies-only` or `tv-only` (not of this kind).
    pub state: String,
}

/// What a feed request tells the page at once; the rows of the sources asked follow on the
/// Channel.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TorrentFeedStartDto {
    /// The feed's generation, to send back with `more`.
    pub generation: String,
    pub sources: Vec<TorrentFeedSourceDto>,
    /// For a first page: the rows already read (a feed opened again while it is recent).
    pub rows: Vec<TorrentFeedRowDto>,
}

/// A release of the feed with what its name says and the library title it matches.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TorrentFeedRowDto {
    pub release: TorrentReleaseDto,
    /// `movie` or `show`.
    pub kind: String,
    /// The title as the release name spells it.
    pub title: String,
    pub year: Option<u16>,
    pub season: Option<u16>,
    pub episode: Option<u16>,
    /// The IMDb ID a source gave.
    pub imdb_id: Option<String>,
    /// Rows with the same key are releases of one title (kind, title and, for a movie, year).
    pub title_key: String,
    /// The library title of the same name (and year), when there is one.
    pub in_library: Option<ScrapeLinkDto>,
}

/// What a source sends on the stream: rows, then (in its final answer) where it stands.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum TorrentFeedItemDto {
    Row {
        row: Box<TorrentFeedRowDto>,
    },
    /// `more`, `done` or `unsupported`.
    SourceState {
        state: String,
    },
}

/// A feed title to look up.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TorrentFeedLookupRequestDto {
    /// `movie` or `show`.
    pub kind: String,
    pub title: String,
    pub year: Option<u16>,
    pub imdb_id: Option<String>,
}

/// The TMDB title a feed title is most likely to be.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TorrentFeedLookupDto {
    /// The best match; none when nothing was found.
    pub hit: Option<TorrentTitleHitDto>,
    /// The match may be the wrong title: its confidence is below [`WEAK_MATCH`].
    pub weak: bool,
}

// ------------------------------------------------------------------ rows

fn title_key(kind: MediaKind, title: &str, year: Option<u16>) -> String {
    let key = match_key(title);
    match (kind, year) {
        (MediaKind::Movie, Some(year)) => format!("movie|{key}|{year}"),
        (MediaKind::Movie, None) => format!("movie|{key}"),
        (MediaKind::Show, _) => format!("show|{key}"),
    }
}

/// The DTO of a merged feed release, matched against the library.
#[must_use]
pub fn feed_row_dto(row: MergedRow, library: &LibraryIndex) -> TorrentFeedRowDto {
    let judged = assess_feed(&row.release);
    let found = library.find(judged.kind, &judged.parsed);
    let verdict = found
        .and_then(|found| found.copy.as_ref())
        .map(|copy| upgrade(&judged.parsed, copy));
    let imdb_id = row.release.imdb.clone();
    let season = row.release.season.or(match judged.parsed.span {
        cv_torrent::release::EpisodeSpan::Single { season, .. } => Some(season),
        cv_torrent::release::EpisodeSpan::SeasonPack(season) => Some(season),
        _ => None,
    });
    let episode = row.release.episode.or(match judged.parsed.span {
        cv_torrent::release::EpisodeSpan::Single { episode, .. } => Some(episode),
        _ => None,
    });
    let title = judged.parsed.title.clone();
    let year = judged.parsed.year;
    let kind = judged.kind;
    let release = release_dto(ResultRow {
        id: row.id,
        absorbs: row.absorbs,
        release: row.release,
        assessment: Assessment {
            parsed: judged.parsed,
            relevance: Relevance::Match,
            flags: judged.flags,
            upgrade: verdict,
            pack: judged.pack,
        },
    });
    TorrentFeedRowDto {
        release,
        kind: kind_text(kind).to_owned(),
        title_key: title_key(kind, &title, year),
        title,
        year,
        season,
        episode,
        imdb_id,
        in_library: link_dto(found.map(|found| &found.link)),
    }
}

/// The rows of a feed kind: rows of the other kind (an `All` source can return both) are left
/// out.
fn rows_of(kind: FeedKind, rows: Vec<MergedRow>, library: &LibraryIndex) -> Vec<TorrentFeedRowDto> {
    rows.into_iter()
        .map(|row| feed_row_dto(row, library))
        .filter(|row| {
            kind.admits(if row.kind == "show" {
                MediaKind::Show
            } else {
                MediaKind::Movie
            })
        })
        .collect()
}

// ------------------------------------------------------------------ providers

/// One source of the feed as a provider of the engine: each ask reads its next page.
struct FeedPage {
    session: Arc<FeedSession>,
    source: Arc<dyn TorrentSource>,
    library: Arc<LibraryIndex>,
    health: Option<Arc<TorrentService>>,
}

impl FeedPage {
    async fn read(
        &self,
        partial: Option<mpsc::UnboundedSender<Vec<TorrentFeedItemDto>>>,
    ) -> ProviderOutcome<TorrentFeedItemDto> {
        let kind = self.session.kind();
        let library = Arc::clone(&self.library);
        let early: Option<Arc<dyn Fn(Vec<MergedRow>) + Send + Sync>> = partial.map(|tx| {
            Arc::new(move |rows: Vec<MergedRow>| {
                let items = rows_of(kind, rows, &library)
                    .into_iter()
                    .map(|row| TorrentFeedItemDto::Row { row: Box::new(row) })
                    .collect();
                let _ = tx.send(items);
            }) as Arc<dyn Fn(Vec<MergedRow>) + Send + Sync>
        });
        // The engine drops this future when the feed is cancelled, which drops the requests.
        let outcome = self
            .session
            .read_page(Arc::clone(&self.source), &TorrentCancel::new(), early)
            .await;
        let id = self.source.id().0.clone();
        match outcome {
            PageOutcome::Read { rows, more, .. } => {
                if let Some(service) = &self.health {
                    service.note_outcome(
                        &id,
                        &SourceOutcome::Done {
                            releases: rows.len(),
                        },
                    );
                }
                let mut items: Vec<TorrentFeedItemDto> = rows_of(kind, rows, &self.library)
                    .into_iter()
                    .map(|row| TorrentFeedItemDto::Row { row: Box::new(row) })
                    .collect();
                items.push(TorrentFeedItemDto::SourceState {
                    state: if more { "more" } else { "done" }.to_owned(),
                });
                ProviderOutcome::results(items)
            }
            PageOutcome::Unsupported => {
                ProviderOutcome::results(vec![TorrentFeedItemDto::SourceState {
                    state: "unsupported".to_owned(),
                }])
            }
            PageOutcome::NotAsked | PageOutcome::Cancelled => {
                let state = cursor_text(self.session.cursor(self.source.id()).state);
                ProviderOutcome::results(vec![TorrentFeedItemDto::SourceState {
                    state: state.to_owned(),
                }])
            }
            PageOutcome::Failed(error) => {
                if let Some(service) = &self.health {
                    service.note_outcome(&id, &SourceOutcome::Failed(error.clone()));
                }
                if matches!(error, SourceError::KeyRejected) {
                    return ProviderOutcome::key_rejected();
                }
                let (reason, retryable) = failure(&error);
                ProviderOutcome::failed(reason, retryable)
            }
        }
    }
}

#[async_trait]
impl SearchProvider<TorrentFeedItemDto> for FeedPage {
    fn id(&self) -> &str {
        &self.source.id().0
    }

    async fn search(&self, _cancel: &CancelToken) -> ProviderOutcome<TorrentFeedItemDto> {
        self.read(None).await
    }

    async fn search_progressive(
        &self,
        _cancel: &CancelToken,
        progress: &Progress<TorrentFeedItemDto>,
    ) -> ProviderOutcome<TorrentFeedItemDto> {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let work = self.read(Some(tx));
        tokio::pin!(work);
        loop {
            tokio::select! {
                Some(items) = rx.recv() => progress.partial(items),
                outcome = &mut work => {
                    while let Ok(items) = rx.try_recv() {
                        progress.partial(items);
                    }
                    return outcome;
                }
            }
        }
    }
}

fn cursor_text(state: CursorState) -> &'static str {
    match state {
        CursorState::Open => "more",
        CursorState::Loading => "loading",
        CursorState::Exhausted => "done",
        CursorState::Unsupported => "unsupported",
    }
}

fn skip_text(skip: FeedSkip) -> &'static str {
    match skip {
        FeedSkip::NotOffered => "unsupported",
        FeedSkip::MoviesOnly => "movies-only",
        FeedSkip::TvOnly => "tv-only",
    }
}

/// Which sources a request asks, and every source's state for the page.
fn plan(
    session: &Arc<FeedSession>,
    sources: Vec<Arc<dyn TorrentSource>>,
    more: bool,
    library: &Arc<LibraryIndex>,
    health: Option<&Arc<TorrentService>>,
) -> (
    Vec<TorrentFeedSourceDto>,
    Vec<Arc<dyn SearchProvider<TorrentFeedItemDto>>>,
) {
    let mut listed = Vec::new();
    let mut providers: Vec<Arc<dyn SearchProvider<TorrentFeedItemDto>>> = Vec::new();
    for source in sources {
        let state = match source.latest_support(session.kind()) {
            FeedSupport::Skip(skip) => skip_text(skip),
            FeedSupport::Yes => {
                let cursor = session.cursor(source.id());
                // A first page asks the sources not read yet; `more` asks those with a next page.
                // A source whose first page failed is asked again with `more` too.
                let ask = cursor.state == CursorState::Open && (more || cursor.next_page == 0);
                if ask {
                    "asked"
                } else {
                    cursor_text(cursor.state)
                }
            }
        };
        listed.push(TorrentFeedSourceDto {
            id: source.id().0.clone(),
            name: source.name().to_owned(),
            kind: source_kind_text(source.kind()).to_owned(),
            state: state.to_owned(),
        });
        if state == "asked" {
            providers.push(Arc::new(FeedPage {
                session: Arc::clone(session),
                source,
                library: Arc::clone(library),
                health: health.cloned(),
            }));
        }
    }
    (listed, providers)
}

/// The library index, read off the async runtime. A library that cannot be read only loses the
/// "In your library" marks.
async fn library_index(service: &Arc<TorrentService>) -> Arc<LibraryIndex> {
    let service = Arc::clone(service);
    let index = cv_jobs::client::spawn_blocking(move || service.library_index())
        .await
        .ok()
        .and_then(|result| {
            result
                .map_err(|error| tracing::debug!(%error, "feed library not read"))
                .ok()
        })
        .unwrap_or_default();
    Arc::new(index)
}

/// Reads the feed: the first page of every source that can list its newest releases (or the
/// rows kept, when the feed was read in the last few minutes), or with `more` the next page of
/// every source that has one. Each source answers as a provider of the stream with its rows and,
/// last, a `sourceState` item saying whether it has more.
pub async fn torrent_latest(
    state: &TorrentState,
    search: &SearchState,
    request: TorrentFeedRequestDto,
    on_event: EventSink<TorrentFeedItemDto>,
) -> CommandResult<TorrentFeedStartDto> {
    let service = state.get()?;
    service.ensure_accepted()?;
    let kind = parse_feed_kind(&request.kind)?;
    let feeds = state.feeds();
    let session = feeds.session(kind, request.refresh, request.more);
    let generation = session.generation().to_string();
    if request.more
        && request
            .generation
            .as_deref()
            .is_some_and(|asked| asked != generation)
    {
        return Err(TorrentCommandError::new(
            "feed_expired",
            "The list was read again meanwhile. Refresh to see it.",
        ));
    }
    let sources = service.feed_sources()?;
    let library = library_index(&service).await;
    let (listed, providers) = plan(&session, sources, request.more, &library, Some(&service));
    let rows = if request.more {
        Vec::new()
    } else {
        rows_of(kind, session.rows(), &library)
    };
    search
        .start_on_sink(&request.search_id, providers, on_event)
        .map_err(|error| TorrentCommandError::new("unavailable", error.message))?;
    Ok(TorrentFeedStartDto {
        generation,
        sources: listed,
        rows,
    })
}

// ------------------------------------------------------------------ lookup

fn lookup_key(request: &TorrentFeedLookupRequestDto) -> String {
    format!(
        "{}|{}|{}|{}",
        request.kind,
        request.imdb_id.as_deref().unwrap_or_default(),
        match_key(&request.title),
        request.year.map(|y| y.to_string()).unwrap_or_default()
    )
}

/// What is typed into the title search for a feed title: its IMDb ID when a source gave one;
/// else the title, with the year for a movie (`Title (Year)`).
#[must_use]
pub fn lookup_text(request: &TorrentFeedLookupRequestDto) -> String {
    if let Some(imdb) = request
        .imdb_id
        .as_deref()
        .map(str::trim)
        .filter(|id| id.starts_with("tt") && id.len() > 2)
    {
        return imdb.to_owned();
    }
    match (request.kind.as_str(), request.year) {
        ("movie", Some(year)) => format!("{} ({year})", request.title.trim()),
        _ => request.title.trim().to_owned(),
    }
}

/// Looks a feed title up on TMDB once: the best hit of the title search and whether it is a
/// weak match. Kept for the rest of the run.
pub async fn torrent_feed_lookup(
    state: &TorrentState,
    request: TorrentFeedLookupRequestDto,
) -> CommandResult<TorrentFeedLookupDto> {
    let service = state.get()?;
    let kind = crate::torrent_stream::parse_kind(&request.kind)?;
    let feeds = state.feeds();
    let key = lookup_key(&request);
    if let Some(kept) = feeds.lookup(&key) {
        return Ok(kept);
    }
    let text = lookup_text(&request);
    let hits = match service.title_hits(kind, &text).await {
        Ok(hits) => hits,
        Err(TorrentServiceError::NotFound) => Vec::new(),
        Err(error) => return Err(error.into()),
    };
    let best = hits
        .into_iter()
        .next()
        .and_then(|hit| title_hit_dto(hit, &PosterKeys::new()));
    let answer = TorrentFeedLookupDto {
        weak: best.as_ref().is_none_or(|hit| hit.confidence < WEAK_MATCH),
        hit: best,
    };
    feeds.keep_lookup(key, answer.clone());
    Ok(answer)
}

#[cfg(test)]
mod tests {
    use cv_db::repo::torrent_landing::LibraryTitleRow;
    use cv_domain::{ItemId, ItemKind};
    use cv_torrent::model::{RawRelease, SourceId};

    use super::*;

    fn merged(name: &str, digit: char, category: Option<&str>) -> MergedRow {
        let mut state = cv_torrent::merge::MergeState::new();
        state
            .add(RawRelease {
                source: Some(SourceId("yts".into())),
                name: name.into(),
                info_hash: Some(digit.to_string().repeat(40)),
                size: Some(2_000_000_000),
                seeders: Some(9),
                category: category.map(str::to_owned),
                imdb: Some("tt9900002".into()),
                ..RawRelease::default()
            })
            .remove(0)
    }

    fn library() -> LibraryIndex {
        LibraryIndex::of(vec![LibraryTitleRow {
            id: ItemId(7),
            kind: ItemKind::Movie,
            title: "Paper Lanterns".into(),
            original_title: String::new(),
            year: 2025,
            media_source: "WEB-DL".into(),
            width: 1920,
            height: 1080,
            hdr_formats: Vec::new(),
        }])
    }

    #[test]
    fn a_feed_row_says_what_its_name_says_and_what_the_library_holds() {
        let row = feed_row_dto(
            merged("Paper.Lanterns.2025.2160p.WEB-DL", 'a', None),
            &library(),
        );
        assert_eq!(row.kind, "movie");
        assert_eq!(row.title, "Paper Lanterns");
        assert_eq!(row.year, Some(2025));
        assert_eq!(
            row.title_key,
            format!("movie|{}|2025", match_key("Paper Lanterns"))
        );
        assert_eq!(row.imdb_id.as_deref(), Some("tt9900002"));
        assert_eq!(
            row.in_library.as_ref().map(|l| l.item_id.as_str()),
            Some("7")
        );
        assert_eq!(row.release.upgrade.as_deref(), Some("better"));
        assert!(row.release.matching);
        assert_eq!(row.release.resolution, "2160p");
    }

    #[test]
    fn an_episode_is_a_show_with_its_numbers_and_no_upgrade_mark() {
        let row = feed_row_dto(
            merged("Night.Bakers.S02E05.720p.WEB", 'b', None),
            &library(),
        );
        assert_eq!(row.kind, "show");
        assert_eq!((row.season, row.episode), (Some(2), Some(5)));
        assert_eq!(row.in_library, None);
        assert_eq!(row.release.upgrade, None);
        assert!(row.title_key.starts_with("show|"));
    }

    #[test]
    fn a_kind_keeps_only_its_rows() {
        let rows = vec![
            merged("Paper.Lanterns.2025.1080p", 'a', None),
            merged("Night.Bakers.S02E05.720p", 'b', None),
        ];
        let movies = rows_of(FeedKind::Movies, rows.clone(), &LibraryIndex::default());
        assert_eq!(movies.len(), 1);
        assert_eq!(
            rows_of(FeedKind::Tv, rows.clone(), &LibraryIndex::default()).len(),
            1
        );
        assert_eq!(
            rows_of(FeedKind::All, rows, &LibraryIndex::default()).len(),
            2
        );
    }

    #[test]
    fn a_title_is_looked_up_by_imdb_id_first_then_by_name_and_year() {
        let request = |imdb: Option<&str>, kind: &str| TorrentFeedLookupRequestDto {
            kind: kind.into(),
            title: "Paper Lanterns".into(),
            year: Some(2025),
            imdb_id: imdb.map(str::to_owned),
        };
        assert_eq!(
            lookup_text(&request(Some("tt9900002"), "movie")),
            "tt9900002"
        );
        assert_eq!(
            lookup_text(&request(None, "movie")),
            "Paper Lanterns (2025)"
        );
        assert_eq!(lookup_text(&request(None, "show")), "Paper Lanterns");
        assert_eq!(
            lookup_text(&request(Some("tt"), "movie")),
            "Paper Lanterns (2025)"
        );
    }

    #[test]
    fn the_kept_feed_is_reused_until_it_is_old_or_refreshed() {
        let feeds = FeedState::default();
        let first = feeds.session(FeedKind::All, false, false);
        let again = feeds.session(FeedKind::All, false, false);
        assert_eq!(first.generation(), again.generation());
        let other = feeds.session(FeedKind::Movies, false, false);
        assert_ne!(other.generation(), first.generation());
        let refreshed = feeds.session(FeedKind::All, true, false);
        assert_ne!(refreshed.generation(), first.generation());
        // "More" always continues the feed kept.
        assert_eq!(
            feeds.session(FeedKind::All, false, true).generation(),
            refreshed.generation()
        );
    }

    #[test]
    fn feed_kinds_are_read_from_their_words() {
        assert_eq!(parse_feed_kind("all").unwrap(), FeedKind::All);
        assert_eq!(parse_feed_kind("movies").unwrap(), FeedKind::Movies);
        assert_eq!(parse_feed_kind("tv").unwrap(), FeedKind::Tv);
        assert_eq!(parse_feed_kind("x").unwrap_err().code, "invalid");
    }
}
