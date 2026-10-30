//! The "Latest releases" feed: the newest releases of every source that can list them, a page
//! at a time, merged into one list.
//!
//! A source says through [`TorrentSource::latest_support`] whether it has a newest-first list
//! for a [`FeedKind`], and [`TorrentSource::latest`] reads one page of it. A [`FeedSession`] keeps
//! the merge of every page read so far (so a release that comes back on a later page, or from
//! another source, stays one row) and a cursor per source: the next page to read and whether the
//! source has more. A source stops contributing when it says it has no more, when a page adds
//! nothing new (a site that ignores the page number sends the same rows again), after
//! [`MAX_FEED_PAGES`] pages, or when it cannot list its newest releases at all (an indexer that
//! refuses a search without words).
//!
//! The judgement of a feed row ([`assess_feed`]) is the parser's reading of the name and the same
//! warnings as a search, without the relevance check: there is no title to compare with.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use crate::cancel::CancelToken;
use crate::merge::{MergeState, MergedRow};
use crate::model::{
    MediaKind, RawRelease, Release, Scope, SourceError, SourceId, SourceKind, TitleIds,
    TorrentQuery,
};
use crate::rank::{Flag, PackKind, flags};
use crate::release::{EpisodeSpan, ParsedRelease, parse_release};
use crate::search::{AGGREGATE_DEADLINE_FACTOR, SOURCE_DEADLINE};
use crate::source::{PartialSink, SearchContext, TorrentSource};

/// The most pages read from one source in one feed.
pub const MAX_FEED_PAGES: u32 = 10;
/// How long the first page of a feed is kept before opening the page reads it again.
pub const FEED_CACHE_TTL: Duration = Duration::from_secs(5 * 60);

/// Which releases the feed lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FeedKind {
    All,
    Movies,
    Tv,
}

impl FeedKind {
    /// Whether movie releases are asked for.
    #[must_use]
    pub fn wants_movies(self) -> bool {
        matches!(self, Self::All | Self::Movies)
    }

    /// Whether TV releases are asked for.
    #[must_use]
    pub fn wants_tv(self) -> bool {
        matches!(self, Self::All | Self::Tv)
    }

    /// Whether a row of this kind belongs in the feed.
    #[must_use]
    pub fn admits(self, kind: MediaKind) -> bool {
        match kind {
            MediaKind::Movie => self.wants_movies(),
            MediaKind::Show => self.wants_tv(),
        }
    }
}

/// Why a source takes no part in a feed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedSkip {
    /// The source has no list of its newest releases (it searches one title at a time).
    NotOffered,
    /// The source lists movies only and the feed is TV.
    MoviesOnly,
    /// The source lists TV only and the feed is movies.
    TvOnly,
}

/// Whether a source takes part in a feed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedSupport {
    Yes,
    Skip(FeedSkip),
}

/// One page of a source's newest releases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LatestAnswer {
    /// The releases of the page, and whether the source says there is a next page.
    Page { rows: Vec<RawRelease>, more: bool },
    /// The source cannot list its newest releases (an indexer that needs search words). Not an
    /// error: it simply takes no part.
    Unsupported,
}

/// Movie or show, read from the release itself: an episode, season or pack is a show; otherwise
/// the source's category decides (Torznab `5xxx`, The Pirate Bay `205`/`208`/`212`, Knaben
/// `TV / ...`); anything else is a movie.
#[must_use]
pub fn feed_kind_of(release: &Release, parsed: &ParsedRelease) -> MediaKind {
    if parsed.span.is_tv() || release.season.is_some() || release.episode.is_some() {
        return MediaKind::Show;
    }
    if release.category.as_deref().is_some_and(category_is_tv) {
        return MediaKind::Show;
    }
    MediaKind::Movie
}

fn category_is_tv(category: &str) -> bool {
    let category = category.trim();
    if let Ok(number) = category.parse::<u32>() {
        return (5000..6000).contains(&number) || matches!(number, 205 | 208 | 212);
    }
    let lower = category.to_ascii_lowercase();
    lower.starts_with("tv") || lower.contains("episode") || lower.starts_with("series")
}

/// What is judged about a feed row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedAssessment {
    pub parsed: ParsedRelease,
    pub kind: MediaKind,
    pub flags: Vec<Flag>,
    pub pack: Option<PackKind>,
}

/// Parses a feed row and gives it the warnings a search gives (payload, cam or screener, no
/// seeders). There is no runtime to check the size against and no title to check relevance
/// with.
#[must_use]
pub fn assess_feed(release: &Release) -> FeedAssessment {
    let parsed = parse_release(&release.name, &release.files);
    let kind = feed_kind_of(release, &parsed);
    let query = TorrentQuery {
        kind,
        ids: TitleIds::default(),
        title: String::new(),
        original_title: None,
        alternative_titles: Vec::new(),
        year: None,
        scope: if kind == MediaKind::Movie {
            Scope::Movie
        } else {
            Scope::Show
        },
        runtime_minutes: None,
        anime: false,
        library_copy: None,
    };
    let flags = flags(release, &parsed, &query);
    let pack = match parsed.span {
        EpisodeSpan::Multi { .. } => Some(PackKind::Episodes),
        EpisodeSpan::SeasonPack(_) => Some(PackKind::Season),
        EpisodeSpan::SeasonRange { .. } => Some(PackKind::Seasons),
        EpisodeSpan::Complete => Some(PackKind::Complete),
        EpisodeSpan::None | EpisodeSpan::Single { .. } => None,
    };
    FeedAssessment {
        parsed,
        kind,
        flags,
        pack,
    }
}

/// Where a source stands in a feed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorState {
    /// It has a next page (or has not been read yet).
    Open,
    /// A page is being read.
    Loading,
    /// It has no more.
    Exhausted,
    /// It cannot list its newest releases.
    Unsupported,
}

/// A source's place in a feed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    /// The next page to read, from 0.
    pub next_page: u32,
    pub state: CursorState,
}

impl Default for Cursor {
    fn default() -> Self {
        Self {
            next_page: 0,
            state: CursorState::Open,
        }
    }
}

/// What reading one page of a source came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageOutcome {
    /// The page was read. `rows` are the merged rows it touched; `added` how many releases were
    /// new to the feed; `more` whether the source will be asked again.
    Read {
        rows: Vec<MergedRow>,
        added: usize,
        more: bool,
    },
    /// The source cannot list its newest releases; it is left out from now on.
    Unsupported,
    /// The source has no more, or a page of it is already being read; nothing was asked.
    NotAsked,
    /// The page failed; the source is asked for the same page next time.
    Failed(SourceError),
    /// The feed was cancelled.
    Cancelled,
}

/// The feed of one kind: its merge and every source's cursor.
pub struct FeedSession {
    kind: FeedKind,
    generation: u64,
    created: Instant,
    deadline: Duration,
    merge: Mutex<MergeState>,
    cursors: Mutex<HashMap<SourceId, Cursor>>,
}

impl std::fmt::Debug for FeedSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FeedSession")
            .field("kind", &self.kind)
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}

/// Puts a cursor that was loading back to open when the read did not finish (a cancel drops the
/// future), so the page is asked again next time.
struct LoadingGuard<'a> {
    session: &'a FeedSession,
    id: SourceId,
    done: bool,
}

impl Drop for LoadingGuard<'_> {
    fn drop(&mut self) {
        if !self.done {
            let mut cursors = self
                .session
                .cursors
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if let Some(cursor) = cursors.get_mut(&self.id)
                && cursor.state == CursorState::Loading
            {
                cursor.state = CursorState::Open;
            }
        }
    }
}

impl FeedSession {
    /// A new feed of `kind`; `generation` tells it apart from the feeds before it.
    #[must_use]
    pub fn new(kind: FeedKind, generation: u64) -> Arc<Self> {
        Self::with_deadline(kind, generation, SOURCE_DEADLINE)
    }

    /// Like [`Self::new`] with another per-source deadline (tests use a short one).
    #[must_use]
    pub fn with_deadline(kind: FeedKind, generation: u64, deadline: Duration) -> Arc<Self> {
        Arc::new(Self {
            kind,
            generation,
            created: Instant::now(),
            deadline,
            merge: Mutex::new(MergeState::new()),
            cursors: Mutex::new(HashMap::new()),
        })
    }

    #[must_use]
    pub fn kind(&self) -> FeedKind {
        self.kind
    }

    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Whether the first page is older than `ttl`, so opening the feed reads it again.
    #[must_use]
    pub fn is_stale(&self, ttl: Duration) -> bool {
        self.created.elapsed() >= ttl
    }

    /// Every row so far, in the order the releases were first seen.
    #[must_use]
    pub fn rows(&self) -> Vec<MergedRow> {
        self.merge
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .rows()
    }

    /// How many distinct releases the feed holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.merge
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    /// Whether the feed holds nothing yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Where `source` stands.
    #[must_use]
    pub fn cursor(&self, source: &SourceId) -> Cursor {
        self.cursors
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(source)
            .copied()
            .unwrap_or_default()
    }

    /// Merges `raws` of `source` and returns the touched rows.
    fn merge(&self, source: &SourceId, mut raws: Vec<RawRelease>) -> Vec<MergedRow> {
        for raw in &mut raws {
            raw.source.get_or_insert_with(|| source.clone());
        }
        self.merge
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .add_all(raws)
    }

    /// Reads the next page of `source` into the feed. Rows the source hands over early go to
    /// `partial` as merged rows. A source that is loading, exhausted or unsupported is not asked.
    pub async fn read_page(
        self: &Arc<Self>,
        source: Arc<dyn TorrentSource>,
        cancel: &CancelToken,
        partial: Option<Arc<dyn Fn(Vec<MergedRow>) + Send + Sync>>,
    ) -> PageOutcome {
        let id = source.id().clone();
        let page = {
            let mut cursors = self.cursors.lock().unwrap_or_else(PoisonError::into_inner);
            let cursor = cursors.entry(id.clone()).or_default();
            if cursor.state != CursorState::Open {
                return PageOutcome::NotAsked;
            }
            cursor.state = CursorState::Loading;
            cursor.next_page
        };
        let mut guard = LoadingGuard {
            session: self,
            id: id.clone(),
            done: false,
        };
        let before = self.len();

        let session = Arc::clone(self);
        let partial_id = id.clone();
        let sink: PartialSink = Arc::new(move |rows| {
            let merged = session.merge(&partial_id, rows);
            if let Some(partial) = &partial {
                partial(merged);
            }
        });
        let context = SearchContext::with_partial(cancel.clone(), sink);
        let deadline = if source.kind() == SourceKind::Torznab {
            self.deadline * AGGREGATE_DEADLINE_FACTOR
        } else {
            self.deadline
        };
        let read = tokio::time::timeout(deadline, source.latest(self.kind, page, &context));
        let result = tokio::select! {
            () = cancel.cancelled() => return PageOutcome::Cancelled,
            result = read => result,
        };
        if cancel.is_cancelled() {
            return PageOutcome::Cancelled;
        }
        let answer = match result {
            Ok(Ok(answer)) => answer,
            Ok(Err(error)) => return PageOutcome::Failed(error),
            Err(_elapsed) => return PageOutcome::Failed(SourceError::Timeout),
        };
        let outcome = match answer {
            LatestAnswer::Unsupported => {
                self.set_cursor(&id, page, CursorState::Unsupported);
                PageOutcome::Unsupported
            }
            LatestAnswer::Page { rows, more } => {
                let rows = self.merge(&id, rows);
                let added = self.len().saturating_sub(before);
                // A page that adds nothing new ends the source: a site that ignores the page
                // number would otherwise be asked for the same rows again and again.
                let next = page + 1;
                let more = more && added > 0 && next < MAX_FEED_PAGES;
                self.set_cursor(
                    &id,
                    next,
                    if more {
                        CursorState::Open
                    } else {
                        CursorState::Exhausted
                    },
                );
                PageOutcome::Read { rows, added, more }
            }
        };
        guard.done = true;
        outcome
    }

    fn set_cursor(&self, id: &SourceId, next_page: u32, state: CursorState) {
        self.cursors
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id.clone(), Cursor { next_page, state });
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use async_trait::async_trait;

    use super::*;
    use crate::model::Support;

    fn raw(name: &str, digit: char) -> RawRelease {
        RawRelease {
            name: name.into(),
            info_hash: Some(digit.to_string().repeat(40)),
            size: Some(1_000_000_000),
            seeders: Some(5),
            ..RawRelease::default()
        }
    }

    /// A source whose pages are scripted; the page number asked is recorded.
    struct Paged {
        id: SourceId,
        pages: Vec<LatestAnswer>,
        asked: AtomicU32,
        last_page: AtomicU32,
        fail_first: bool,
    }

    fn paged(id: &str, pages: Vec<LatestAnswer>) -> Arc<Paged> {
        Arc::new(Paged {
            id: SourceId(id.into()),
            pages,
            asked: AtomicU32::new(0),
            last_page: AtomicU32::new(u32::MAX),
            fail_first: false,
        })
    }

    #[async_trait]
    impl TorrentSource for Paged {
        fn id(&self) -> &SourceId {
            &self.id
        }
        fn name(&self) -> &str {
            &self.id.0
        }
        fn kind(&self) -> SourceKind {
            SourceKind::BuiltIn
        }
        fn supports(&self, _query: &TorrentQuery) -> Support {
            Support::Yes
        }
        async fn search(
            &self,
            _query: &TorrentQuery,
            _context: &SearchContext,
        ) -> Result<Vec<RawRelease>, SourceError> {
            Ok(Vec::new())
        }
        fn latest_support(&self, _kind: FeedKind) -> FeedSupport {
            FeedSupport::Yes
        }
        async fn latest(
            &self,
            _kind: FeedKind,
            page: u32,
            _context: &SearchContext,
        ) -> Result<LatestAnswer, SourceError> {
            let asked = self.asked.fetch_add(1, Ordering::SeqCst);
            self.last_page.store(page, Ordering::SeqCst);
            if self.fail_first && asked == 0 {
                return Err(SourceError::RateLimited);
            }
            Ok(self
                .pages
                .get(page as usize)
                .cloned()
                .unwrap_or(LatestAnswer::Page {
                    rows: Vec::new(),
                    more: false,
                }))
        }
    }

    async fn read(session: &Arc<FeedSession>, source: &Arc<Paged>) -> PageOutcome {
        session
            .read_page(
                Arc::clone(source) as Arc<dyn TorrentSource>,
                &CancelToken::new(),
                None,
            )
            .await
    }

    #[tokio::test]
    async fn pages_are_read_in_turn_and_the_source_stops_when_it_says_so() {
        let session = FeedSession::new(FeedKind::All, 1);
        let source = paged(
            "a",
            vec![
                LatestAnswer::Page {
                    rows: vec![raw("One.2026.1080p", 'a'), raw("Two.2026.720p", 'b')],
                    more: true,
                },
                LatestAnswer::Page {
                    rows: vec![raw("Three.2026.1080p", 'c')],
                    more: false,
                },
            ],
        );
        let first = read(&session, &source).await;
        assert!(matches!(
            first,
            PageOutcome::Read {
                added: 2,
                more: true,
                ..
            }
        ));
        let second = read(&session, &source).await;
        assert!(matches!(
            second,
            PageOutcome::Read {
                added: 1,
                more: false,
                ..
            }
        ));
        assert_eq!(source.last_page.load(Ordering::SeqCst), 1);
        assert_eq!(session.cursor(&source.id).state, CursorState::Exhausted);
        assert_eq!(read(&session, &source).await, PageOutcome::NotAsked);
        assert_eq!(source.asked.load(Ordering::SeqCst), 2);
        assert_eq!(session.len(), 3);
    }

    #[tokio::test]
    async fn rows_seen_on_an_earlier_page_or_another_source_stay_one_row() {
        let session = FeedSession::new(FeedKind::All, 1);
        let a = paged(
            "a",
            vec![
                LatestAnswer::Page {
                    rows: vec![raw("One.2026.1080p", 'a')],
                    more: true,
                },
                LatestAnswer::Page {
                    rows: vec![raw("One.2026.1080p", 'a'), raw("Two.2026.1080p", 'b')],
                    more: true,
                },
            ],
        );
        let b = paged(
            "b",
            vec![LatestAnswer::Page {
                rows: vec![raw("Two.2026.1080p", 'b')],
                more: false,
            }],
        );
        read(&session, &a).await;
        read(&session, &a).await;
        let PageOutcome::Read { rows, added, .. } = read(&session, &b).await else {
            panic!("read");
        };
        assert_eq!(added, 0);
        assert_eq!(session.len(), 2);
        assert_eq!(rows[0].release.sources.len(), 2, "both sources named");
    }

    #[tokio::test]
    async fn a_page_that_adds_nothing_ends_a_source_that_ignores_the_page_number() {
        let session = FeedSession::new(FeedKind::All, 1);
        let same = LatestAnswer::Page {
            rows: vec![raw("One.2026.1080p", 'a')],
            more: true,
        };
        let source = paged("a", vec![same.clone(), same.clone(), same]);
        read(&session, &source).await;
        let second = read(&session, &source).await;
        assert!(matches!(
            second,
            PageOutcome::Read {
                added: 0,
                more: false,
                ..
            }
        ));
        assert_eq!(session.cursor(&source.id).state, CursorState::Exhausted);
    }

    #[tokio::test]
    async fn a_source_is_read_at_most_the_page_limit() {
        let session = FeedSession::new(FeedKind::All, 1);
        let pages: Vec<LatestAnswer> = (0..MAX_FEED_PAGES + 3)
            .map(|n| LatestAnswer::Page {
                rows: vec![RawRelease {
                    name: format!("Title.{n}.2026.1080p"),
                    info_hash: Some(format!("{n:040}")),
                    ..RawRelease::default()
                }],
                more: true,
            })
            .collect();
        let source = paged("a", pages);
        for _ in 0..MAX_FEED_PAGES + 3 {
            read(&session, &source).await;
        }
        assert_eq!(source.asked.load(Ordering::SeqCst), MAX_FEED_PAGES);
    }

    #[tokio::test]
    async fn an_unsupported_source_is_left_out_quietly() {
        let session = FeedSession::new(FeedKind::Movies, 1);
        let source = paged("a", vec![LatestAnswer::Unsupported]);
        assert_eq!(read(&session, &source).await, PageOutcome::Unsupported);
        assert_eq!(read(&session, &source).await, PageOutcome::NotAsked);
        assert_eq!(session.cursor(&source.id).state, CursorState::Unsupported);
    }

    #[tokio::test]
    async fn a_failed_page_is_asked_again() {
        let session = FeedSession::new(FeedKind::All, 1);
        let source = Arc::new(Paged {
            id: SourceId("a".into()),
            pages: vec![LatestAnswer::Page {
                rows: vec![raw("One.2026.1080p", 'a')],
                more: false,
            }],
            asked: AtomicU32::new(0),
            last_page: AtomicU32::new(u32::MAX),
            fail_first: true,
        });
        assert_eq!(
            read(&session, &source).await,
            PageOutcome::Failed(SourceError::RateLimited)
        );
        assert_eq!(session.cursor(&source.id).state, CursorState::Open);
        assert!(matches!(
            read(&session, &source).await,
            PageOutcome::Read { added: 1, .. }
        ));
        assert_eq!(source.last_page.load(Ordering::SeqCst), 0, "the same page");
    }

    #[test]
    fn a_session_goes_stale_after_its_time() {
        let session = FeedSession::new(FeedKind::All, 1);
        assert!(!session.is_stale(FEED_CACHE_TTL));
        assert!(session.is_stale(Duration::ZERO));
    }

    fn release(name: &str, category: Option<&str>) -> Release {
        Release {
            id: name.into(),
            name: name.into(),
            category: category.map(str::to_owned),
            seeders: Some(0),
            ..Release::default()
        }
    }

    #[test]
    fn the_kind_comes_from_the_name_then_the_category() {
        let kind = |name: &str, category: Option<&str>| {
            let release = release(name, category);
            assess_feed(&release).kind
        };
        assert_eq!(kind("Night.Bakers.S02E01.720p.WEB", None), MediaKind::Show);
        assert_eq!(kind("Night Bakers Season 2 1080p", None), MediaKind::Show);
        assert_eq!(kind("Paper.Lanterns.2025.1080p", None), MediaKind::Movie);
        assert_eq!(
            kind("Talk Show 2026 10 30 720p", Some("5030")),
            MediaKind::Show
        );
        assert_eq!(
            kind("Talk Show 2026 10 30 720p", Some("208")),
            MediaKind::Show
        );
        assert_eq!(
            kind("Talk Show 2026 10 30 720p", Some("TV / HD")),
            MediaKind::Show
        );
        assert_eq!(kind("Paper Lanterns 2025", Some("2040")), MediaKind::Movie);
        assert_eq!(kind("Paper Lanterns 2025", Some("207")), MediaKind::Movie);
        assert_eq!(
            kind("Paper Lanterns 2025", Some("Movies / HD")),
            MediaKind::Movie
        );
    }

    #[test]
    fn feed_rows_carry_the_low_quality_and_seeder_warnings() {
        let assessed = assess_feed(&release("Paper.Lanterns.2025.HDTS.x264", None));
        let kinds: Vec<_> = assessed.flags.iter().map(|f| f.kind).collect();
        assert!(kinds.contains(&crate::rank::FlagKind::LowQuality(
            crate::release::LowQuality::Telesync
        )));
        assert!(kinds.contains(&crate::rank::FlagKind::NoSeeders));
        assert_eq!(assessed.parsed.title, "Paper Lanterns");
        assert_eq!(assessed.parsed.year, Some(2025));
    }

    #[test]
    fn the_feed_kind_admits_its_rows() {
        assert!(FeedKind::All.admits(MediaKind::Movie) && FeedKind::All.admits(MediaKind::Show));
        assert!(
            FeedKind::Movies.admits(MediaKind::Movie) && !FeedKind::Movies.admits(MediaKind::Show)
        );
        assert!(FeedKind::Tv.admits(MediaKind::Show) && !FeedKind::Tv.admits(MediaKind::Movie));
    }
}
