//! The torrent Wishlist: titles the user wants a release of, the kind of release they are after,
//! and the checks that look for one.
//!
//! A check is the release search without the page: TMDB details, the scope, the library (an
//! entry is done, and never checked again, once more library copies meet its criteria than did
//! when it was saved), the query, every source that applies, and the matching releases that
//! carry no warning and meet the criteria. What it keeps is stored per entry; a dismissed release never comes back. Nothing is
//! ever handed to the torrent app here: the user chooses every release.

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, PoisonError};

use cv_db::repo::torrent_wishlist::{
    self as rows, MatchRow, MatchState, NewWish, UpdateOutcome, WishRow,
};
use cv_domain::{CalendarDate, MediaSource};
use cv_torrent::CancelToken;
use cv_torrent::model::{Hdr, LibraryCopy, MediaKind, Resolution, Scope, SkipReason, SourceId};
use cv_torrent::rank::Relevance;
use cv_torrent::search::{Orchestrator, Outcome, ResultRow, Sink, Summary};
use serde::{Deserialize, Serialize};

use super::library::{LibraryHolding, movie_copies};
use super::library::{LibraryLink, held_episodes, holding_of, library_copy, linked_title};
use super::title::EpisodeRow;
use super::{TorrentService, TorrentServiceError};
use crate::collections::today;

/// How long an entry rests after a check before the next one that is only due.
pub const CHECK_EVERY_MS: i64 = 6 * 60 * 60 * 1000;

/// The lowest resolution an entry accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MinResolution {
    #[serde(rename = "720p")]
    R720,
    #[serde(rename = "1080p")]
    R1080,
    #[serde(rename = "2160p")]
    R2160,
}

impl MinResolution {
    fn rank(self) -> u8 {
        match self {
            Self::R720 => 2,
            Self::R1080 => 3,
            Self::R2160 => 4,
        }
    }

    /// `720p`, `1080p` or `2160p`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::R720 => "720p",
            Self::R1080 => "1080p",
            Self::R2160 => "2160p",
        }
    }

    /// The resolution named `text`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        [Self::R720, Self::R1080, Self::R2160]
            .into_iter()
            .find(|value| value.as_str() == text)
    }
}

fn resolution_rank(resolution: Resolution) -> u8 {
    match resolution {
        Resolution::R2160 => 4,
        Resolution::R1080 => 3,
        Resolution::R720 => 2,
        Resolution::R480 => 1,
        Resolution::Unknown => 0,
    }
}

/// A kind of source an entry accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WishSource {
    #[serde(rename = "Remux")]
    Remux,
    #[serde(rename = "BluRay")]
    BluRay,
    #[serde(rename = "WEB-DL")]
    WebDl,
    #[serde(rename = "WEBRip")]
    WebRip,
}

impl WishSource {
    /// The word the release search uses: `Remux`, `BluRay`, `WEB-DL` or `WEBRip`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Remux => "Remux",
            Self::BluRay => "BluRay",
            Self::WebDl => "WEB-DL",
            Self::WebRip => "WEBRip",
        }
    }

    /// The source named `text`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        [Self::Remux, Self::BluRay, Self::WebDl, Self::WebRip]
            .into_iter()
            .find(|value| value.as_str() == text)
    }

    /// The kind of a release's source; none for a source the Wishlist does not offer.
    fn of_release(source: &MediaSource, remux: bool) -> Option<Self> {
        if remux {
            return Some(Self::Remux);
        }
        match source {
            MediaSource::UhdBluray | MediaSource::Bluray => Some(Self::BluRay),
            MediaSource::WebDl => Some(Self::WebDl),
            MediaSource::WebRip | MediaSource::Stream => Some(Self::WebRip),
            _ => None,
        }
    }
}

/// What kind of release an entry is after.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WishCriteria {
    /// The lowest resolution; none takes any.
    pub min_resolution: Option<MinResolution>,
    /// The sources accepted; none takes any.
    pub sources: Vec<WishSource>,
    /// Only releases with HDR (any format).
    pub hdr: bool,
    /// The smallest size in bytes.
    pub min_size: Option<u64>,
    /// The largest size in bytes.
    pub max_size: Option<u64>,
    /// The fewest seeders.
    pub min_seeders: Option<u32>,
}

impl WishCriteria {
    /// The criteria stored as `json`; damaged text gives the defaults.
    #[must_use]
    pub fn from_json(json: &str) -> Self {
        serde_json::from_str(json).unwrap_or_default()
    }

    /// The criteria as stored.
    #[must_use]
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_owned())
    }

    fn resolution_ok(&self, resolution: Resolution) -> bool {
        self.min_resolution
            .is_none_or(|min| resolution_rank(resolution) >= min.rank())
    }

    fn source_ok(&self, source: Option<WishSource>) -> bool {
        self.sources.is_empty() || source.is_some_and(|s| self.sources.contains(&s))
    }

    /// Whether a release meets the criteria: the title and scope asked for, no warning, and every
    /// criterion. An unknown size or seeder count fails a set limit.
    #[must_use]
    pub fn met_by_release(&self, row: &ResultRow) -> bool {
        let assessment = &row.assessment;
        let parsed = &assessment.parsed;
        if !matches!(assessment.relevance, Relevance::Match) || !assessment.flags.is_empty() {
            return false;
        }
        self.resolution_ok(parsed.resolution)
            && self.source_ok(WishSource::of_release(&parsed.source, parsed.remux))
            && (!self.hdr || parsed.hdr != Hdr::None)
            && self
                .min_size
                .is_none_or(|min| row.release.size.is_some_and(|size| size >= min))
            && self
                .max_size
                .is_none_or(|max| row.release.size.is_some_and(|size| size <= max))
            && self
                .min_seeders
                .is_none_or(|min| row.release.seeders.is_some_and(|seeders| seeders >= min))
    }

    /// Whether a library copy meets the resolution, source and HDR criteria (size and seeders do
    /// not apply). The library cannot tell a remux from an encode, so a Blu-ray copy counts as a
    /// Remux. Without a copy whose media info was read, only criteria that ask for nothing pass.
    #[must_use]
    pub fn met_by_copy(&self, copy: Option<&LibraryCopy>) -> bool {
        let asks_nothing = self.min_resolution.is_none() && self.sources.is_empty() && !self.hdr;
        let Some(copy) = copy else {
            return asks_nothing;
        };
        let source_ok = self.sources.is_empty()
            || match WishSource::of_release(&copy.source, false) {
                Some(WishSource::BluRay) => self
                    .sources
                    .iter()
                    .any(|s| matches!(s, WishSource::BluRay | WishSource::Remux)),
                other => self.source_ok(other),
            };
        self.resolution_ok(copy.resolution) && source_ok && (!self.hdr || copy.hdr != Hdr::None)
    }
}

/// What an entry looks for within its title.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WishScope {
    Movie,
    Season(u16),
    Episode(u16, u16),
    /// The first aired episode after the newest one the library holds.
    Next,
}

impl WishScope {
    /// `movie`, `season:2`, `episode:2:5` or `next`.
    #[must_use]
    pub fn as_text(self) -> String {
        match self {
            Self::Movie => "movie".to_owned(),
            Self::Season(season) => format!("season:{season}"),
            Self::Episode(season, episode) => format!("episode:{season}:{episode}"),
            Self::Next => "next".to_owned(),
        }
    }

    /// The scope stored as `text`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split(':');
        let scope = match (parts.next()?, parts.next(), parts.next()) {
            ("movie", None, None) => Self::Movie,
            ("next", None, None) => Self::Next,
            ("season", Some(season), None) => Self::Season(season.parse().ok()?),
            ("episode", Some(season), Some(episode)) => {
                Self::Episode(season.parse().ok()?, episode.parse().ok()?)
            }
            _ => return None,
        };
        parts.next().is_none().then_some(scope)
    }

    /// Whether the scope belongs to a title of `kind`.
    #[must_use]
    pub fn fits(self, kind: MediaKind) -> bool {
        matches!(
            (self, kind),
            (Self::Movie, MediaKind::Movie)
                | (
                    Self::Season(_) | Self::Episode(..) | Self::Next,
                    MediaKind::Show
                )
        )
    }
}

/// What the last check of an entry could not do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckNote {
    /// No source answered.
    NoSource,
    /// A "next" entry waits for an episode that airs on this day.
    Airs(CalendarDate),
    /// A "next" entry waits for an episode that is not announced.
    NoNewEpisode,
    /// The check failed; the words say why.
    Failed(String),
}

impl CheckNote {
    fn as_text(&self) -> String {
        match self {
            Self::NoSource => "no-source".to_owned(),
            Self::Airs(date) => format!("airs:{date}"),
            Self::NoNewEpisode => "no-new-episode".to_owned(),
            Self::Failed(words) => words.clone(),
        }
    }

    fn parse(text: &str) -> Self {
        match text {
            "no-source" => Self::NoSource,
            "no-new-episode" => Self::NoNewEpisode,
            _ => text
                .strip_prefix("airs:")
                .and_then(|date| date.parse().ok())
                .map_or_else(|| Self::Failed(text.to_owned()), Self::Airs),
        }
    }
}

/// A release a check kept for an entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WishMatch {
    /// The search's merge key of the release.
    pub key: String,
    pub name: String,
    pub size: Option<u64>,
    pub seeders: Option<u32>,
    pub magnet: Option<String>,
    pub torrent_url: Option<String>,
    /// The ids of the sources that returned it.
    pub sources: Vec<String>,
    /// Epoch milliseconds.
    pub found_ms: i64,
    pub state: MatchState,
}

/// An entry of the Wishlist with its matches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WishEntry {
    pub id: i64,
    pub kind: MediaKind,
    pub tmdb_id: u64,
    pub title: String,
    pub year: Option<u16>,
    pub poster_url: Option<String>,
    pub scope: WishScope,
    pub criteria: WishCriteria,
    /// Epoch milliseconds.
    pub added_ms: i64,
    pub checked_ms: Option<i64>,
    pub note: Option<CheckNote>,
    /// When the library was found to have gained a copy that meets the criteria.
    pub done_ms: Option<i64>,
    /// How many library copies met the criteria when the entry was added, last edited or told to
    /// keep looking.
    pub held_copies: u32,
    /// The copy the library holds of the title and scope, for an entry that is not done.
    pub held: Option<LibraryCopy>,
    /// The episode a "next" entry looked for at its last check.
    pub target: Option<(u16, u16)>,
    /// The library title, when the library has one with this TMDB ID.
    pub library: Option<LibraryLink>,
    /// The matches that are not dismissed, most seeders first.
    pub matches: Vec<WishMatch>,
}

/// What a new entry is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WishRequest {
    pub kind: MediaKind,
    pub tmdb_id: u64,
    pub scope: WishScope,
    pub criteria: WishCriteria,
}

/// What the library holds of a title and scope.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Holding {
    /// The best movie copy, the quality most episodes of a season share when every aired one is
    /// held, or the episode's copy. Its parts are unknown when the media info was not read.
    copy: Option<LibraryCopy>,
    /// How many copies meet the criteria: each movie copy, or one for the season or episode.
    meeting: u32,
}

fn unknown_copy() -> LibraryCopy {
    LibraryCopy {
        resolution: Resolution::Unknown,
        source: MediaSource::Unknown,
        hdr: Hdr::None,
    }
}

/// What a check did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CheckReport {
    /// How many entries were checked.
    pub checked: usize,
    /// How many releases were found that were not known before.
    pub new_matches: usize,
    /// Another check was already running, so this one did nothing.
    pub busy: bool,
}

fn episode_code((season, episode): (u16, u16)) -> String {
    format!("S{season:02}E{episode:02}")
}

fn parse_code(text: &str) -> Option<(u16, u16)> {
    let rest = text.strip_prefix('S')?;
    let (season, episode) = rest.split_once('E')?;
    Some((season.parse().ok()?, episode.parse().ok()?))
}

fn kind_text(kind: MediaKind) -> &'static str {
    match kind {
        MediaKind::Movie => "movie",
        MediaKind::Show => "show",
    }
}

/// What a "next" entry looks for now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NextEpisode {
    /// This episode has aired: search for it.
    Look((u16, u16)),
    /// This episode airs on that day.
    Airs((u16, u16), CalendarDate),
    /// No later episode is announced.
    None,
}

/// The episode a "next" entry looks for: the first aired one after the newest episode the
/// library holds, or the newest aired one when it holds none. Specials are left out.
pub(super) fn next_episode(
    episodes: &[EpisodeRow],
    held: &[(u16, u16)],
    today: CalendarDate,
) -> NextEpisode {
    let regular: Vec<&EpisodeRow> = episodes.iter().filter(|row| row.season > 0).collect();
    let newest_held = held.iter().filter(|(season, _)| *season > 0).max().copied();
    let aired = |row: &EpisodeRow| row.air_date.is_some_and(|date| date <= today);
    let candidate = match newest_held {
        Some(newest) => regular
            .iter()
            .find(|row| (row.season, row.episode) > newest)
            .copied(),
        None => regular
            .iter()
            .rev()
            .find(|row| aired(row))
            .or_else(|| regular.first())
            .copied(),
    };
    let Some(row) = candidate else {
        return NextEpisode::None;
    };
    let number = (row.season, row.episode);
    match row.air_date {
        Some(date) if date <= today => NextEpisode::Look(number),
        Some(date) => NextEpisode::Airs(number, date),
        None => NextEpisode::None,
    }
}

/// Collects the rows of a check: the newest version of each release, without the ones folded
/// into another.
#[derive(Default)]
struct Collect {
    rows: Mutex<HashMap<String, ResultRow>>,
}

impl Collect {
    fn take(&self, rows: Vec<ResultRow>) {
        let mut kept = self.rows.lock().unwrap_or_else(PoisonError::into_inner);
        for row in rows {
            for gone in &row.absorbs {
                kept.remove(gone);
            }
            kept.insert(row.id.clone(), row);
        }
    }
}

impl Sink for Collect {
    fn started(&self, _source: &SourceId) {}

    fn partial(&self, _source: &SourceId, rows: Vec<ResultRow>) {
        self.take(rows);
    }

    fn results(&self, _source: &SourceId, rows: Vec<ResultRow>) {
        self.take(rows);
    }

    fn failed(&self, _source: &SourceId, _error: &cv_torrent::model::SourceError) {}

    fn skipped(&self, _source: &SourceId, _reason: SkipReason) {}

    fn done(&self, _summary: &Summary) {}
}

fn library_error(error: &cv_db::DbError) -> TorrentServiceError {
    TorrentServiceError::Library(error.to_string())
}

fn entry_of(row: WishRow, matches: Vec<WishMatch>, library: Option<LibraryLink>) -> WishEntry {
    let kind = if row.kind == "show" {
        MediaKind::Show
    } else {
        MediaKind::Movie
    };
    WishEntry {
        id: row.id,
        kind,
        tmdb_id: u64::try_from(row.tmdb_id).unwrap_or_default(),
        title: row.title,
        year: row.year.and_then(|year| u16::try_from(year).ok()),
        poster_url: row.poster_url,
        scope: WishScope::parse(&row.scope).unwrap_or(match kind {
            MediaKind::Movie => WishScope::Movie,
            MediaKind::Show => WishScope::Next,
        }),
        criteria: WishCriteria::from_json(&row.criteria_json),
        added_ms: row.added_ms,
        checked_ms: row.checked_ms,
        note: row.check_error.as_deref().map(CheckNote::parse),
        done_ms: row.done_ms,
        held_copies: u32::try_from(row.held_copies).unwrap_or_default(),
        held: None,
        target: row.target.as_deref().and_then(parse_code),
        library,
        matches,
    }
}

fn match_of(row: MatchRow) -> WishMatch {
    WishMatch {
        key: row.key,
        name: row.name,
        size: row.size.and_then(|size| u64::try_from(size).ok()),
        seeders: row.seeders.and_then(|seeders| u32::try_from(seeders).ok()),
        magnet: row.magnet,
        torrent_url: row.torrent_url,
        sources: row
            .sources
            .split(',')
            .filter(|id| !id.is_empty())
            .map(str::to_owned)
            .collect(),
        found_ms: row.found_ms,
        state: row.state,
    }
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Clears the "a check runs" mark when the check ends, however it ends.
struct Running<'a>(&'a std::sync::atomic::AtomicBool);

impl Drop for Running<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl TorrentService {
    /// Every entry of the Wishlist, the newest first, with its matches. Entries that are not done
    /// are judged against the library first: one that more copies meet than when it was saved is
    /// marked done, and when fewer meet it the lower number counts from then on. Only the library
    /// is read.
    ///
    /// # Errors
    /// [`TorrentServiceError::Library`] when the library cannot be read or written.
    pub fn wishlist(&self) -> Result<Vec<WishEntry>, TorrentServiceError> {
        self.wishlist_at(now_ms())
    }

    fn wishlist_at(&self, now: i64) -> Result<Vec<WishEntry>, TorrentServiceError> {
        let entries = self.db.read(rows::list).map_err(|e| library_error(&e))?;
        let mut by_entry: HashMap<i64, Vec<WishMatch>> = HashMap::new();
        for row in self.db.read(rows::matches).map_err(|e| library_error(&e))? {
            by_entry
                .entry(row.entry_id)
                .or_default()
                .push(match_of(row));
        }
        let mut out = Vec::with_capacity(entries.len());
        for row in entries {
            let matches = by_entry.remove(&row.id).unwrap_or_default();
            let mut entry = entry_of(row, matches, None);
            entry.library =
                linked_title(&self.db, entry.kind, entry.tmdb_id).map_err(|e| library_error(&e))?;
            if entry.done_ms.is_none() {
                let holding = self.holding(
                    entry.kind,
                    entry.tmdb_id,
                    entry.library.as_ref(),
                    entry.scope,
                    &entry.criteria,
                )?;
                self.judge(&mut entry, holding.meeting, now)?;
                if entry.done_ms.is_none() {
                    entry.held = holding.copy;
                }
            }
            out.push(entry);
        }
        Ok(out)
    }

    /// Marks the entry done when more copies meet its criteria than when it was saved, or lowers
    /// the number saved when fewer do.
    fn judge(
        &self,
        entry: &mut WishEntry,
        meeting: u32,
        now: i64,
    ) -> Result<(), TorrentServiceError> {
        let id = entry.id;
        if meeting > entry.held_copies {
            self.db
                .write(move |tx| rows::mark_done(tx, id, now))
                .map_err(|e| library_error(&e))?;
            entry.done_ms = Some(now);
        } else if meeting < entry.held_copies {
            self.db
                .write(move |tx| rows::lower_held(tx, id, i64::from(meeting)))
                .map_err(|e| library_error(&e))?;
            entry.held_copies = meeting;
        }
        Ok(())
    }

    /// What the library holds of a title and scope, and how many of those copies meet the
    /// criteria: for a movie each copy with its TMDB ID, for a season every aired episode with the
    /// quality most of them share, for an episode that episode. A "next" entry holds nothing.
    fn holding(
        &self,
        kind: MediaKind,
        tmdb_id: u64,
        link: Option<&LibraryLink>,
        scope: WishScope,
        criteria: &WishCriteria,
    ) -> Result<Holding, TorrentServiceError> {
        let Some(link) = link else {
            return Ok(Holding::default());
        };
        let scope = match scope {
            WishScope::Movie if kind == MediaKind::Movie => {
                let copies = movie_copies(&self.db, tmdb_id).map_err(|e| library_error(&e))?;
                let meeting = copies
                    .iter()
                    .filter(|copy| criteria.met_by_copy(Some(&copy.compared())))
                    .count();
                return Ok(Holding {
                    copy: copies.first().map(super::library::MovieCopy::compared),
                    meeting: u32::try_from(meeting).unwrap_or(u32::MAX),
                });
            }
            WishScope::Movie | WishScope::Next => return Ok(Holding::default()),
            WishScope::Season(season) => {
                let Some(LibraryHolding::Show(show)) =
                    holding_of(&self.db, link, tmdb_id).map_err(|e| library_error(&e))?
                else {
                    return Ok(Holding::default());
                };
                let held = show.held.iter().any(|(s, _)| *s == season);
                let missing = show.missing.iter().any(|(s, _)| *s == season);
                if !held || missing {
                    return Ok(Holding::default());
                }
                Scope::Season(season)
            }
            WishScope::Episode(season, episode) => {
                let held = held_episodes(&self.db, link.item)
                    .map_err(|e| library_error(&e))?
                    .iter()
                    .any(|(s, e, _)| (*s, *e) == (season, episode));
                if !held {
                    return Ok(Holding::default());
                }
                Scope::Episode(season, episode)
            }
        };
        let copy = library_copy(&self.db, Some(link), Some(tmdb_id), scope)
            .map_err(|e| library_error(&e))?;
        Ok(Holding {
            meeting: u32::from(criteria.met_by_copy(copy.as_ref())),
            copy: Some(copy.unwrap_or_else(unknown_copy)),
        })
    }

    /// How many library copies of the title and scope meet the criteria now.
    fn meeting_now(
        &self,
        kind: MediaKind,
        tmdb_id: u64,
        scope: WishScope,
        criteria: &WishCriteria,
    ) -> Result<u32, TorrentServiceError> {
        let link = linked_title(&self.db, kind, tmdb_id).map_err(|e| library_error(&e))?;
        Ok(self
            .holding(kind, tmdb_id, link.as_ref(), scope, criteria)?
            .meeting)
    }

    /// The copy the library holds of a title and scope, for the editor's "You already have
    /// this". Only the library is read.
    ///
    /// # Errors
    /// A library that cannot be read.
    pub fn wishlist_held(
        &self,
        kind: MediaKind,
        tmdb_id: u64,
        scope: WishScope,
    ) -> Result<Option<LibraryCopy>, TorrentServiceError> {
        if !scope.fits(kind) {
            return Ok(None);
        }
        let link = linked_title(&self.db, kind, tmdb_id).map_err(|e| library_error(&e))?;
        Ok(self
            .holding(
                kind,
                tmdb_id,
                link.as_ref(),
                scope,
                &WishCriteria::default(),
            )?
            .copy)
    }

    /// Adds a title to the Wishlist, or replaces the criteria of the entry that has this title
    /// and scope. The title's name, year and poster come from TMDB. The copies the library holds
    /// that already meet the criteria are counted, so only a copy that arrives later makes the
    /// entry done. Returns the entry's id.
    ///
    /// # Errors
    /// `NoticeNotAccepted` before the notice is accepted, TMDB's error, `Invalid` for a scope that
    /// does not belong to the kind of title, or a library that cannot be written.
    pub async fn wishlist_add(&self, request: WishRequest) -> Result<i64, TorrentServiceError> {
        self.ensure_accepted()?;
        if !request.scope.fits(request.kind) {
            return Err(TorrentServiceError::Torrent(
                cv_torrent::TorrentError::Invalid(
                    "That scope does not belong to this kind of title.".to_owned(),
                ),
            ));
        }
        let details = self.title_details(request.kind, request.tmdb_id).await?;
        let held = self
            .holding(
                request.kind,
                request.tmdb_id,
                details.library_item.as_ref(),
                request.scope,
                &request.criteria,
            )?
            .meeting;
        let wish = NewWish {
            kind: kind_text(request.kind).to_owned(),
            tmdb_id: i64::try_from(request.tmdb_id).unwrap_or(i64::MAX),
            title: details.title,
            year: details.year.map(i64::from),
            poster_url: details.poster_url,
            scope: request.scope.as_text(),
            criteria_json: request.criteria.to_json(),
            added_ms: now_ms(),
            held_copies: i64::from(held),
        };
        self.db
            .write(move |tx| rows::add(tx, &wish))
            .map_err(|e| library_error(&e))
    }

    /// Changes an entry's scope and criteria; the entry is judged and checked again, counting the
    /// copies that meet the new criteria from now.
    ///
    /// # Errors
    /// `NotFound` for an unknown entry, `Invalid` when the scope does not belong to the title or
    /// another entry already has it, or a library that cannot be written.
    pub fn wishlist_update(
        &self,
        id: i64,
        scope: WishScope,
        criteria: &WishCriteria,
    ) -> Result<(), TorrentServiceError> {
        let entry = self
            .db
            .read(|c| rows::get(c, id))
            .map_err(|e| library_error(&e))?
            .ok_or(TorrentServiceError::NotFound)?;
        let kind = if entry.kind == "show" {
            MediaKind::Show
        } else {
            MediaKind::Movie
        };
        let invalid = |words: &str| {
            TorrentServiceError::Torrent(cv_torrent::TorrentError::Invalid(words.to_owned()))
        };
        if !scope.fits(kind) {
            return Err(invalid("That scope does not belong to this kind of title."));
        }
        let held = i64::from(self.meeting_now(
            kind,
            u64::try_from(entry.tmdb_id).unwrap_or_default(),
            scope,
            criteria,
        )?);
        let scope_text = scope.as_text();
        let json = criteria.to_json();
        match self
            .db
            .write(move |tx| rows::update(tx, id, &scope_text, &json, held))
            .map_err(|e| library_error(&e))?
        {
            UpdateOutcome::Updated => Ok(()),
            UpdateOutcome::Missing => Err(TorrentServiceError::NotFound),
            UpdateOutcome::Duplicate => Err(invalid("That is already on your Wishlist.")),
        }
    }

    /// Makes a done entry active again: it is checked at once, and only a copy that arrives from
    /// now on makes it done.
    ///
    /// # Errors
    /// `NotFound` for an unknown entry, or a library that cannot be written.
    pub fn wishlist_keep_looking(&self, id: i64) -> Result<(), TorrentServiceError> {
        let entry = self
            .db
            .read(|c| rows::get(c, id))
            .map_err(|e| library_error(&e))?
            .ok_or(TorrentServiceError::NotFound)?;
        let entry = entry_of(entry, Vec::new(), None);
        let held =
            i64::from(self.meeting_now(entry.kind, entry.tmdb_id, entry.scope, &entry.criteria)?);
        let found = self
            .db
            .write(move |tx| rows::keep_looking(tx, id, held))
            .map_err(|e| library_error(&e))?;
        if found {
            Ok(())
        } else {
            Err(TorrentServiceError::NotFound)
        }
    }

    /// Removes an entry and its matches.
    ///
    /// # Errors
    /// A library that cannot be written.
    pub fn wishlist_remove(&self, id: i64) -> Result<bool, TorrentServiceError> {
        self.db
            .write(move |tx| rows::remove(tx, id))
            .map_err(|e| library_error(&e))
    }

    /// Marks a match sent or dismissed (or back to seen).
    ///
    /// # Errors
    /// `NotFound` when the entry has no such match; a library that cannot be written.
    pub fn wishlist_set_match(
        &self,
        id: i64,
        key: &str,
        state: MatchState,
    ) -> Result<(), TorrentServiceError> {
        let key = key.to_owned();
        let found = self
            .db
            .write(move |tx| rows::set_match_state(tx, id, &key, state))
            .map_err(|e| library_error(&e))?;
        if found {
            Ok(())
        } else {
            Err(TorrentServiceError::NotFound)
        }
    }

    /// Marks every new match seen (the Wishlist was opened). Returns how many there were.
    ///
    /// # Errors
    /// A library that cannot be written.
    pub fn wishlist_seen(&self) -> Result<usize, TorrentServiceError> {
        self.db
            .write(|tx| rows::mark_seen(tx))
            .map_err(|e| library_error(&e))
    }

    /// How many matches have not been looked at yet.
    ///
    /// # Errors
    /// A library that cannot be read.
    pub fn wishlist_new_count(&self) -> Result<usize, TorrentServiceError> {
        let count = self
            .db
            .read(rows::count_new)
            .map_err(|e| library_error(&e))?;
        Ok(usize::try_from(count).unwrap_or_default())
    }

    /// Checks the Wishlist: every entry that is not done, or with `due_only` the ones whose last
    /// check is at least [`CHECK_EVERY_MS`] old. One check runs at a time; a second one returns at
    /// once with `busy`. An empty Wishlist contacts nothing.
    ///
    /// # Errors
    /// `NoticeNotAccepted` before the notice is accepted; a library that cannot be read.
    pub async fn wishlist_check(&self, due_only: bool) -> Result<CheckReport, TorrentServiceError> {
        self.wishlist_check_at(due_only, now_ms()).await
    }

    /// [`Self::wishlist_check`] as if it were `now` (epoch milliseconds).
    ///
    /// # Errors
    /// As [`Self::wishlist_check`].
    pub async fn wishlist_check_at(
        &self,
        due_only: bool,
        now: i64,
    ) -> Result<CheckReport, TorrentServiceError> {
        self.ensure_accepted()?;
        if self.wishlist_busy.swap(true, Ordering::AcqRel) {
            return Ok(CheckReport {
                busy: true,
                ..CheckReport::default()
            });
        }
        let _running = Running(&self.wishlist_busy);
        let mut report = CheckReport::default();
        for entry in self.wishlist_at(now)? {
            let due = entry
                .checked_ms
                .is_none_or(|checked| now - checked >= CHECK_EVERY_MS);
            if entry.done_ms.is_some() || (due_only && !due) {
                continue;
            }
            report.checked += 1;
            report.new_matches += self.check_entry(&entry, now).await?;
        }
        Ok(report)
    }

    /// Checks one entry and stores what it found. Returns how many releases were new.
    async fn check_entry(&self, entry: &WishEntry, now: i64) -> Result<usize, TorrentServiceError> {
        let id = entry.id;
        let record = |note: Option<CheckNote>, target: Option<(u16, u16)>| {
            let note = note.map(|note| note.as_text());
            let target = target.map(episode_code);
            self.db
                .write(move |tx| {
                    rows::record_check(tx, id, now, note.as_deref(), target.as_deref())
                })
                .map_err(|e| library_error(&e))
        };
        let details = match self.title_details(entry.kind, entry.tmdb_id).await {
            Ok(details) => details,
            Err(TorrentServiceError::Torrent(error)) => {
                return Err(TorrentServiceError::Torrent(error));
            }
            Err(error) => {
                record(Some(CheckNote::Failed(error.to_string())), entry.target)?;
                return Ok(0);
            }
        };
        let (scope, target) = match entry.scope {
            WishScope::Movie => (Scope::Movie, None),
            WishScope::Season(season) => (Scope::Season(season), None),
            WishScope::Episode(season, episode) => (Scope::Episode(season, episode), None),
            WishScope::Next => {
                let held: Vec<(u16, u16)> = match &details.library_item {
                    Some(link) => held_episodes(&self.db, link.item)
                        .map_err(|e| library_error(&e))?
                        .into_iter()
                        .map(|(season, episode, _)| (season, episode))
                        .collect(),
                    None => Vec::new(),
                };
                let episodes = match self.episode_rows(entry.tmdb_id).await {
                    Ok(episodes) => episodes,
                    Err(error) => {
                        record(Some(CheckNote::Failed(error.to_string())), entry.target)?;
                        return Ok(0);
                    }
                };
                match next_episode(&episodes, &held, today()) {
                    NextEpisode::Look(number) => (Scope::Episode(number.0, number.1), Some(number)),
                    NextEpisode::Airs(number, date) => {
                        record(Some(CheckNote::Airs(date)), Some(number))?;
                        return Ok(0);
                    }
                    NextEpisode::None => {
                        record(Some(CheckNote::NoNewEpisode), entry.target)?;
                        return Ok(0);
                    }
                }
            }
        };
        let mut judged = entry.clone();
        let meeting = self
            .holding(
                entry.kind,
                entry.tmdb_id,
                details.library_item.as_ref(),
                entry.scope,
                &entry.criteria,
            )?
            .meeting;
        self.judge(&mut judged, meeting, now)?;
        if judged.done_ms.is_some() {
            record(None, target)?;
            return Ok(0);
        }
        let query = self.build_query(&details, scope)?;
        let sources = self.sources(&query)?;
        let collect = Arc::new(Collect::default());
        let sink: Arc<dyn Sink> = collect.clone();
        let outcome = Orchestrator::new(sources)
            .run(query, sink, &CancelToken::new())
            .await;
        let note = match &outcome {
            Outcome::Finished(summary) => {
                for (source, result) in &summary.outcomes {
                    self.note_outcome(&source.0, result);
                }
                (summary.answered() == 0 && summary.failed() > 0).then_some(CheckNote::NoSource)
            }
            Outcome::Cancelled => Some(CheckNote::NoSource),
        };
        let kept: Vec<MatchRow> = collect
            .rows
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .filter(|row| entry.criteria.met_by_release(row))
            .map(|row| MatchRow {
                entry_id: id,
                key: row.id.clone(),
                name: row.release.name.clone(),
                size: row.release.size.and_then(|size| i64::try_from(size).ok()),
                seeders: row.release.seeders.map(i64::from),
                magnet: row.release.magnet.clone(),
                torrent_url: row.release.torrent_url.clone(),
                sources: row
                    .release
                    .sources
                    .iter()
                    .map(|source| source.0.as_str())
                    .collect::<Vec<_>>()
                    .join(","),
                found_ms: now,
                state: MatchState::New,
            })
            .collect();
        record(note, target)?;
        self.db
            .write(move |tx| rows::store_matches(tx, &kept))
            .map_err(|e| library_error(&e))
    }
}

#[cfg(test)]
mod tests;
