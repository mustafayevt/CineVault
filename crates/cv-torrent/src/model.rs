//! The types shared by every part of torrent search.

use chrono::{DateTime, Utc};
use cv_domain::MediaSource;

/// Movie or show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MediaKind {
    Movie,
    Show,
}

/// What is searched for within a title.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scope {
    Movie,
    Show,
    Season(u16),
    Episode(u16, u16),
}

/// The IDs a title is known by.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TitleIds {
    pub tmdb: Option<u64>,
    /// `tt0063350`.
    pub imdb: Option<String>,
    pub tvdb: Option<u64>,
}

/// Video resolution of a release.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Resolution {
    R2160,
    R1080,
    R720,
    R480,
    Unknown,
}

/// HDR format of a release.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Hdr {
    DolbyVision,
    Hdr10Plus,
    Hdr10,
    Hlg,
    None,
}

/// The copy of a title the library already holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryCopy {
    pub resolution: Resolution,
    pub source: MediaSource,
    pub hdr: Hdr,
}

/// Everything a source needs to search for one title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TorrentQuery {
    pub kind: MediaKind,
    pub ids: TitleIds,
    pub title: String,
    pub original_title: Option<String>,
    pub alternative_titles: Vec<String>,
    pub year: Option<u16>,
    pub scope: Scope,
    pub runtime_minutes: Option<u32>,
    pub anime: bool,
    pub library_copy: Option<LibraryCopy>,
}

/// A source's id: `torrentio`, `yts`, `eztv`, `apibay`, `knaben`, `nyaa`, `indexer:<id>`, `torznab`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SourceId(pub String);

/// Where a source comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    BuiltIn,
    Indexer,
    Torznab,
}

/// Why a source does not apply to a query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    NoImdbId,
    MoviesOnly,
    TvOnly,
    AnimeOnly,
    ShowScope,
    Off,
}

/// Whether a source applies to a query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Support {
    Yes,
    Skip(SkipReason),
}

/// One release as a source reported it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawRelease {
    pub source: Option<SourceId>,
    pub name: String,
    pub info_hash: Option<String>,
    pub magnet: Option<String>,
    pub torrent_url: Option<String>,
    pub size: Option<u64>,
    pub seeders: Option<u32>,
    pub leechers: Option<u32>,
    pub published: Option<DateTime<Utc>>,
    pub imdb: Option<String>,
    pub season: Option<u16>,
    pub episode: Option<u16>,
    pub files: Vec<String>,
    pub category: Option<String>,
    /// The release's own page on the source's site (`http` or `https`), where the source gives
    /// one or it can be built from what it gave.
    pub details_url: Option<String>,
}

/// The page of a release on one source's site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourcePage {
    pub source: SourceId,
    pub url: String,
}

/// An address that is safe to hand to the browser: `http` or `https`, with a host, no spaces.
#[must_use]
pub fn web_page(url: &str) -> Option<String> {
    let url = url.trim();
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    (!rest.is_empty() && !rest.starts_with('/') && !url.chars().any(char::is_whitespace))
        .then(|| url.to_owned())
}

/// One release after the merge: every source that returned it folded into one record.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Release {
    /// The merge key: the lower-case hex info-hash, or a name-and-size key when no source gave
    /// a hash.
    pub id: String,
    pub name: String,
    /// Lower-case hex, when any source gave one.
    pub info_hash: Option<String>,
    /// A magnet link: the first one a source gave, else one built from the info-hash.
    pub magnet: Option<String>,
    pub torrent_url: Option<String>,
    pub size: Option<u64>,
    /// The highest count any source reported.
    pub seeders: Option<u32>,
    pub leechers: Option<u32>,
    /// The earliest date any source reported.
    pub published: Option<DateTime<Utc>>,
    pub imdb: Option<String>,
    pub season: Option<u16>,
    pub episode: Option<u16>,
    pub files: Vec<String>,
    pub category: Option<String>,
    /// Every source that returned this release, in the order they first did.
    pub sources: Vec<SourceId>,
    /// The release's page on each source that gave one, in the order the sources first did.
    pub pages: Vec<SourcePage>,
}

/// Why a source failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SourceError {
    #[error("unreachable: {0}")]
    Unreachable(String),
    #[error("rate limited")]
    RateLimited,
    /// An HTML challenge page came back where data was expected.
    #[error("blocked by a challenge page")]
    Blocked,
    #[error("bad answer: {0}")]
    BadAnswer(String),
    #[error("timed out")]
    Timeout,
    /// The source rejected the API key (the user's own Torznab instance, the indexer service).
    #[error("the key was rejected")]
    KeyRejected,
}

/// What a source did in one search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceOutcome {
    Skipped(SkipReason),
    Done { releases: usize },
    Failed(SourceError),
}
