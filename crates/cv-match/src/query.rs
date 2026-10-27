//! Query normalisation and ID shortcuts.
//!
//! Search input is free text with an optional `(yyyy)` year, or an ID shortcut:
//!
//! | Input | Result |
//! |---|---|
//! | `tt0069293`, `imdb:tt0069293`, `imdb:69293` | IMDb ID `tt0069293` |
//! | `tmdb:693134` | TMDB ID `693134` |
//! | `tvdb:81189` | TVDB ID `81189` |
//! | `https://www.imdb.com/title/tt0069293/` | IMDb ID |
//! | `https://www.themoviedb.org/movie/693134-dune-part-two` (or `/tv/`) | TMDB ID, with a media hint |
//! | `https://thetvdb.com/dereferrer/series/81189`, `...?tab=series&id=81189` | TVDB ID |
//! | `Crash (2004)` | text `Crash`, year 2004 |
//!
//! A search that finds nothing for a query starting with a 4-digit number is retried without
//! that number (see [`NormalizedQuery::retry_without_leading_year`] and [`search_with_retry`]).

use std::sync::LazyLock;

use cv_domain::id_key;
use regex::Regex;

/// Years accepted in `(yyyy)`; anything else stays part of the text.
const MIN_YEAR: u16 = 1880;
const MAX_YEAR: u16 = 2100;

/// Whether an ID shortcut names a movie or a show, when the input says so (URLs do).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaHint {
    Movie,
    Show,
}

/// A provider ID found in the query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdShortcut {
    /// Provider key, one of the [`id_key`] constants (`imdb`, `tmdb`, `tvdb`).
    pub provider: &'static str,
    /// The ID in its canonical stored form (`tt0069293`, `693134`).
    pub id: String,
    /// Movie or show, when the input says so.
    pub hint: Option<MediaHint>,
}

/// The result of normalising a search query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedQuery {
    /// The text to search for: whitespace collapsed, `(yyyy)` removed. Empty for an ID shortcut.
    pub text: String,
    /// The search year from `(yyyy)`.
    pub year: Option<u16>,
    /// Set when the input was an ID shortcut. Callers look the ID up directly, with no text search.
    pub id: Option<IdShortcut>,
}

impl NormalizedQuery {
    /// The query to retry with when this one found nothing: the text without a leading 4-digit
    /// number. `None` when the text does not start with one, or nothing would be left.
    pub fn retry_without_leading_year(&self) -> Option<NormalizedQuery> {
        if self.id.is_some() {
            return None;
        }
        let end = LEADING_YEAR.find(&self.text)?.end();
        let rest = collapse(&self.text[end..]);
        if rest.is_empty() {
            return None;
        }
        Some(NormalizedQuery {
            text: rest,
            year: self.year,
            id: None,
        })
    }
}

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("valid regex")
}

static IMDB_BARE: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^tt(\d{7,})$"));
static IMDB_PREFIXED: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^imdb\s*:\s*(?:tt)?(\d+)$"));
static TMDB_PREFIXED: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^tmdb\s*:\s*(\d+)$"));
static TVDB_PREFIXED: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^tvdb\s*:\s*(\d+)$"));
static IMDB_URL: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?i)^(?:https?://)?(?:[a-z0-9-]+\.)*imdb\.com/(?:[a-z]{2}/)?title/tt(\d{7,})(?:[/?#].*)?$")
});
static TMDB_URL: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?i)^(?:https?://)?(?:[a-z0-9-]+\.)*themoviedb\.org/(movie|tv)/(\d+)(?:[-/?#].*)?$")
});
static TVDB_URL_PATH: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?i)^(?:https?://)?(?:[a-z0-9-]+\.)*thetvdb\.com/(?:dereferrer/)?(series|movies?)/(\d+)(?:[/?#].*)?$",
    )
});
static TVDB_URL_QUERY: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?i)^(?:https?://)?(?:[a-z0-9-]+\.)*thetvdb\.com/[^?#]*\?(?:[^#]*&)?id=(\d+)(?:[&#].*)?$")
});
static TVDB_URL_TAB: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)[?&]tab=(series|movie)\b"));
static YEAR_IN_BRACKETS: LazyLock<Regex> = LazyLock::new(|| re(r"\(\s*(\d{4})\s*\)"));
static LEADING_YEAR: LazyLock<Regex> = LazyLock::new(|| re(r"^\d{4}(?:[\s._:\-]+|$)"));

fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn shortcut(provider: &'static str, id: String, hint: Option<MediaHint>) -> NormalizedQuery {
    NormalizedQuery {
        text: String::new(),
        year: None,
        id: Some(IdShortcut { provider, id, hint }),
    }
}

/// Canonical IMDb ID from digits, zero-padded to 7 digits (`69293` becomes `tt0069293`).
/// `None` for an all-zero ID.
fn imdb_from_digits(digits: &str) -> Option<String> {
    let trimmed = digits.trim_start_matches('0');
    (!trimmed.is_empty()).then(|| format!("tt{trimmed:0>7}"))
}

/// A positive numeric ID without leading zeros; `None` for zero.
fn numeric_id(digits: &str) -> Option<String> {
    let trimmed = digits.trim_start_matches('0');
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

fn hint_of(kind: &str) -> MediaHint {
    if kind.eq_ignore_ascii_case("tv") || kind.eq_ignore_ascii_case("series") {
        MediaHint::Show
    } else {
        MediaHint::Movie
    }
}

fn parse_id_shortcut(input: &str) -> Option<NormalizedQuery> {
    for regex in [&*IMDB_BARE, &*IMDB_PREFIXED, &*IMDB_URL] {
        if let Some(c) = regex.captures(input) {
            return imdb_from_digits(&c[1]).map(|id| shortcut(id_key::IMDB, id, None));
        }
    }
    if let Some(c) = TMDB_PREFIXED.captures(input) {
        return numeric_id(&c[1]).map(|id| shortcut(id_key::TMDB, id, None));
    }
    if let Some(c) = TMDB_URL.captures(input) {
        let hint = Some(hint_of(&c[1]));
        return numeric_id(&c[2]).map(|id| shortcut(id_key::TMDB, id, hint));
    }
    if let Some(c) = TVDB_PREFIXED.captures(input) {
        return numeric_id(&c[1]).map(|id| shortcut(id_key::TVDB, id, None));
    }
    if let Some(c) = TVDB_URL_PATH.captures(input) {
        let hint = Some(hint_of(&c[1]));
        return numeric_id(&c[2]).map(|id| shortcut(id_key::TVDB, id, hint));
    }
    if let Some(c) = TVDB_URL_QUERY.captures(input) {
        let hint = TVDB_URL_TAB.captures(input).map(|t| hint_of(&t[1]));
        return numeric_id(&c[1]).map(|id| shortcut(id_key::TVDB, id, hint));
    }
    None
}

/// Normalises a search input: detects an ID shortcut, otherwise extracts a `(yyyy)` year and
/// collapses whitespace.
///
/// The last `(yyyy)` with a plausible year (1880 to 2100) is removed from the text and becomes
/// the search year. If removing it would leave no text, the input is kept as the text.
pub fn normalize_query(input: &str) -> NormalizedQuery {
    let input = input.trim();
    if let Some(q) = parse_id_shortcut(input) {
        return q;
    }

    let found = YEAR_IN_BRACKETS
        .captures_iter(input)
        .filter_map(|c| {
            let m = c.get(0)?;
            let y: u16 = c[1].parse().ok()?;
            (MIN_YEAR..=MAX_YEAR).contains(&y).then_some((m.range(), y))
        })
        .last();

    if let Some((range, year)) = found {
        let text = collapse(&format!(
            "{} {}",
            &input[..range.start],
            &input[range.end..]
        ));
        if !text.is_empty() {
            return NormalizedQuery {
                text,
                year: Some(year),
                id: None,
            };
        }
    }
    NormalizedQuery {
        text: collapse(input),
        year: None,
        id: None,
    }
}

/// Runs `search` for the query. If it yields nothing and the text starts with a 4-digit number,
/// runs it once more without that number. ID shortcuts are never retried: callers look those up
/// directly and do not call this.
pub fn search_with_retry<T, E>(
    query: &NormalizedQuery,
    mut search: impl FnMut(&NormalizedQuery) -> Result<Vec<T>, E>,
) -> Result<Vec<T>, E> {
    let first = search(query)?;
    if !first.is_empty() {
        return Ok(first);
    }
    match query.retry_without_leading_year() {
        Some(retry) => search(&retry),
        None => Ok(first),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(q: &NormalizedQuery) -> (&'static str, &str, Option<MediaHint>) {
        let s = q.id.as_ref().expect("an ID shortcut");
        (s.provider, s.id.as_str(), s.hint)
    }

    #[test]
    fn bare_imdb_id() {
        let q = normalize_query("tt0069293");
        assert_eq!(id(&q), ("imdb", "tt0069293", None));
        assert!(q.text.is_empty());
        assert_eq!(q.year, None);
        assert_eq!(
            id(&normalize_query("  TT1234567 ")),
            ("imdb", "tt1234567", None)
        );
        assert_eq!(id(&normalize_query("tt12345678")).1, "tt12345678");
    }

    #[test]
    fn imdb_prefix() {
        assert_eq!(id(&normalize_query("imdb:tt0069293")).1, "tt0069293");
        assert_eq!(id(&normalize_query("IMDB: tt0069293")).1, "tt0069293");
        assert_eq!(id(&normalize_query("imdb:69293")).1, "tt0069293");
    }

    #[test]
    fn imdb_urls() {
        for url in [
            "https://www.imdb.com/title/tt0069293/",
            "http://imdb.com/title/tt0069293",
            "https://m.imdb.com/title/tt0069293/?ref_=fn_al_tt_1",
            "www.imdb.com/title/tt0069293/reference",
            "https://www.imdb.com/en/title/tt0069293/",
        ] {
            assert_eq!(
                id(&normalize_query(url)),
                ("imdb", "tt0069293", None),
                "{url}"
            );
        }
    }

    #[test]
    fn tmdb_prefix() {
        assert_eq!(
            id(&normalize_query("tmdb:693134")),
            ("tmdb", "693134", None)
        );
        assert_eq!(id(&normalize_query("TMDB : 693134")).1, "693134");
    }

    #[test]
    fn tmdb_urls_carry_media_hint() {
        assert_eq!(
            id(&normalize_query(
                "https://www.themoviedb.org/movie/693134-dune-part-two"
            )),
            ("tmdb", "693134", Some(MediaHint::Movie))
        );
        assert_eq!(
            id(&normalize_query(
                "https://www.themoviedb.org/tv/1396-breaking-bad?language=en-US"
            )),
            ("tmdb", "1396", Some(MediaHint::Show))
        );
        assert_eq!(
            id(&normalize_query("themoviedb.org/movie/603")),
            ("tmdb", "603", Some(MediaHint::Movie))
        );
    }

    #[test]
    fn tvdb_prefix() {
        assert_eq!(id(&normalize_query("tvdb:81189")), ("tvdb", "81189", None));
        assert_eq!(id(&normalize_query("TVDB:81189")).1, "81189");
    }

    #[test]
    fn tvdb_urls() {
        assert_eq!(
            id(&normalize_query(
                "https://thetvdb.com/dereferrer/series/81189"
            )),
            ("tvdb", "81189", Some(MediaHint::Show))
        );
        assert_eq!(
            id(&normalize_query(
                "https://www.thetvdb.com/?tab=series&id=81189&lid=7"
            )),
            ("tvdb", "81189", Some(MediaHint::Show))
        );
        assert_eq!(
            id(&normalize_query(
                "https://thetvdb.com/dereferrer/movie/4321"
            )),
            ("tvdb", "4321", Some(MediaHint::Movie))
        );
        assert_eq!(id(&normalize_query("https://thetvdb.com/?id=55")).2, None);
    }

    #[test]
    fn zero_ids_are_not_shortcuts() {
        assert!(normalize_query("tmdb:0").id.is_none());
        assert_eq!(normalize_query("tmdb:0").text, "tmdb:0");
        assert!(normalize_query("imdb:0000").id.is_none());
    }

    #[test]
    fn unknown_urls_and_short_tt_are_text() {
        assert!(
            normalize_query("https://example.com/title/tt0069293/")
                .id
                .is_none()
        );
        let q = normalize_query("tt123");
        assert!(q.id.is_none());
        assert_eq!(q.text, "tt123");
    }

    #[test]
    fn year_in_brackets() {
        let q = normalize_query("Crash (2004)");
        assert_eq!(q.text, "Crash");
        assert_eq!(q.year, Some(2004));
        assert!(q.id.is_none());
    }

    #[test]
    fn year_in_the_middle_and_spacing() {
        let q = normalize_query("  The   Thing ( 1982 )  uncut ");
        assert_eq!(q.text, "The Thing uncut");
        assert_eq!(q.year, Some(1982));
    }

    #[test]
    fn last_year_wins() {
        let q = normalize_query("Blade Runner (1982) (2007)");
        assert_eq!(q.text, "Blade Runner (1982)");
        assert_eq!(q.year, Some(2007));
    }

    #[test]
    fn implausible_or_lone_year_is_kept_as_text() {
        let q = normalize_query("Foo (1234)");
        assert_eq!((q.text.as_str(), q.year), ("Foo (1234)", None));
        let q = normalize_query("(2004)");
        assert_eq!((q.text.as_str(), q.year), ("(2004)", None));
        let q = normalize_query("Foo (20045)");
        assert_eq!(q.year, None);
    }

    #[test]
    fn plain_text_is_collapsed() {
        let q = normalize_query("  Solaris \t 1972 ");
        assert_eq!(q.text, "Solaris 1972");
        assert_eq!(q.year, None);
        assert_eq!(normalize_query("").text, "");
    }

    #[test]
    fn leading_year_retry_query() {
        let q = normalize_query("2012 Some Film");
        assert_eq!(q.retry_without_leading_year().unwrap().text, "Some Film");
        let q = normalize_query("1917: The Return (2019)");
        let r = q.retry_without_leading_year().unwrap();
        assert_eq!((r.text.as_str(), r.year), ("The Return", Some(2019)));
        assert_eq!(
            normalize_query("2001 - A Space Odyssey")
                .retry_without_leading_year()
                .unwrap()
                .text,
            "A Space Odyssey"
        );
    }

    #[test]
    fn leading_year_retry_not_applicable() {
        assert!(
            normalize_query("1917")
                .retry_without_leading_year()
                .is_none()
        );
        assert!(
            normalize_query("12345 Films")
                .retry_without_leading_year()
                .is_none()
        );
        assert!(
            normalize_query("300 Spartans")
                .retry_without_leading_year()
                .is_none()
        );
        assert!(
            normalize_query("1917s Revenge")
                .retry_without_leading_year()
                .is_none()
        );
        assert!(
            normalize_query("Solaris")
                .retry_without_leading_year()
                .is_none()
        );
        assert!(
            normalize_query("tt0069293")
                .retry_without_leading_year()
                .is_none()
        );
    }

    #[test]
    fn search_retries_once_without_leading_year() {
        let q = normalize_query("2012 Some Film");
        let mut calls = Vec::new();
        let out: Result<Vec<u8>, ()> = search_with_retry(&q, |n| {
            calls.push(n.text.clone());
            Ok(if n.text == "Some Film" {
                vec![1]
            } else {
                vec![]
            })
        });
        assert_eq!(out, Ok(vec![1]));
        assert_eq!(calls, ["2012 Some Film", "Some Film"]);
    }

    #[test]
    fn search_does_not_retry_when_results_exist_or_no_leading_year() {
        let q = normalize_query("2012 Some Film");
        let mut n = 0;
        let out: Result<Vec<u8>, ()> = search_with_retry(&q, |_| {
            n += 1;
            Ok(vec![1])
        });
        assert_eq!((out, n), (Ok(vec![1]), 1));

        let q = normalize_query("Nothing");
        let mut n = 0;
        let out: Result<Vec<u8>, ()> = search_with_retry(&q, |_| {
            n += 1;
            Ok(vec![])
        });
        assert_eq!((out, n), (Ok(vec![]), 1));
    }

    #[test]
    fn search_error_is_returned_without_retry() {
        let q = normalize_query("2012 Some Film");
        let mut n = 0;
        let out: Result<Vec<u8>, &str> = search_with_retry(&q, |_| {
            n += 1;
            Err("offline")
        });
        assert_eq!((out, n), (Err("offline"), 1));
    }
}
