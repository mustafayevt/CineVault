//! Provider ID detection in names, URLs and NFO text.

use std::sync::LazyLock;

use regex::Regex;

/// IDs found in a text. A missing ID is `None`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedIds {
    /// IMDb ID including the `tt` prefix, for example `tt0103639`.
    pub imdb: Option<String>,
    /// TMDB ID. Zero, a number that does not fit a 32-bit signed integer and text without
    /// digits count as missing.
    pub tmdb: Option<u32>,
    /// TVDB ID as the digits found.
    pub tvdb: Option<String>,
}

struct Patterns {
    imdb: Regex,
    imdb_url: Regex,
    tmdb: Regex,
    tmdb_url: Regex,
    tvdb: Regex,
    tvdb_url: Regex,
}

static PATTERNS: LazyLock<Patterns> = LazyLock::new(|| {
    let re = |p: &str| Regex::new(p).expect("static ID pattern");
    Patterns {
        imdb: re(r"(tt\d{6,})"),
        imdb_url: re(r"(?i)imdb\.com/Title\?(\d{6,})"),
        tmdb: re(r"(?i)(?:tmdbid|tmdb)[ ._=-]?(\d+)"),
        tmdb_url: re(r"(?i)themoviedb\.org/(?:movie|tv)/(\d+)"),
        tvdb: re(r"(?i)(?:tvdbid|tvdb)[ ._=-]?(\d+)"),
        tvdb_url: re(r"(?i)thetvdb\.com/(?:movies|series)/(\d+)"),
    }
});

/// Finds the IMDb, TMDB and TVDB IDs in `text`.
///
/// - IMDb: `tt` followed by at least six digits, otherwise the number of an
///   `imdb.com/Title?<digits>` URL (returned with the `tt` prefix).
/// - TMDB: `tmdb` or `tmdbid`, an optional separator (space, `.`, `_`, `=` or `-`) and digits,
///   otherwise a `themoviedb.org/movie/<digits>` or `/tv/<digits>` URL.
/// - TVDB: the same with `tvdb` and `tvdbid`, otherwise a `thetvdb.com/series/<digits>` or
///   `/movies/<digits>` URL.
///
/// The first match of each kind wins. Keywords are matched without regard to case.
pub fn detect_ids(text: &str) -> ParsedIds {
    let p = &*PATTERNS;
    let imdb = p
        .imdb
        .captures(text)
        .map(|c| c[1].to_owned())
        .or_else(|| p.imdb_url.captures(text).map(|c| format!("tt{}", &c[1])));
    let tmdb = p
        .tmdb
        .captures(text)
        .and_then(|c| parse_id(&c[1]))
        .or_else(|| p.tmdb_url.captures(text).and_then(|c| parse_id(&c[1])));
    let tvdb = p
        .tvdb
        .captures(text)
        .map(|c| c[1].to_owned())
        .or_else(|| p.tvdb_url.captures(text).map(|c| c[1].to_owned()));
    ParsedIds { imdb, tmdb, tvdb }
}

/// Parses an ID the way a signed 32-bit integer is parsed; zero and overflow mean "no ID".
fn parse_id(digits: &str) -> Option<u32> {
    digits
        .parse::<i32>()
        .ok()
        .filter(|id| *id > 0)
        .and_then(|id| u32::try_from(id).ok())
}
