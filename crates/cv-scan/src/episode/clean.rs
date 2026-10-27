//! The episode title that is left in a file name.
//!
//! The steps, in order, on the file name without its extension:
//!
//! 1. resolutions (`1920x1080`), `(2015)`-style years, `[ABCD1234]` hashes, IMDb ids and dates
//!    are removed;
//! 2. release tags and bad words are removed where they stand between delimiters;
//! 3. stacking words with a number (`cd1`, `part2`) are removed;
//! 4. a name that is only the show name leaves nothing; a show name at the start is removed
//!    unless that would leave nothing. The show name matches with or without its accents;
//! 5. season and episode markers go (`S01E02`, `1x02`). If nothing is left, the name before this
//!    step is the title (an episode named only `S01E02` keeps `S01E02`);
//! 6. a roman part (`Part II`), a lone episode (` E05`) and leftover `-10` or `x02` pieces go,
//!    but only when something else remains;
//! 7. delimiters become spaces.

use std::sync::LazyLock;

use regex::Regex;

use super::{PATTERNS, date, roman_value, show_pattern, strip_extension, without_accents};
use crate::parse::remove_stop_and_bad_words_tv;

struct Patterns {
    imdb: Regex,
    stacking_word: Regex,
    season_episode: Regex,
    number_x_number: Regex,
    roman: Regex,
    lone_episode: Regex,
    leftover: Regex,
}

static CLEAN: LazyLock<Patterns> = LazyLock::new(|| {
    let re = |p: &str| Regex::new(p).expect("static title pattern");
    Patterns {
        imdb: re(r"(?i)tt\d{6,}"),
        stacking_word: re(
            r"(?i)(^|[\s_.\-\[\](){},])(?:cd|dvd|p(?:ar)?t|dis[ck])[1-9][0-9]?($|[\s_.\-\[\](){},])",
        ),
        // S01E02, S01E02E03, S01E02-E03, S01E05-07
        season_episode: re(
            r"(?i)(^|[^a-z0-9])s\d{1,4}[ _]?[ex]\d{1,4}(?:[\-.]?[ex]\d{1,4})?(?:[\-.]\d{1,4})*",
        ),
        // 1x02, 1x02x03, 1x02-03
        number_x_number: re(
            r"(?i)([\s_.\-])\d{1,4}x\d{1,4}(?:[\-.]?[ex]\d{1,4})?(?:[\-.]\d{1,4})*",
        ),
        roman: re(r"(?i)(^|[^a-z0-9])(?:part|pt)[._\s]+([mdclxvi]+)($|[^a-z0-9])"),
        lone_episode: re(r"(?i)\se[\s_.\-]?\d{1,4}\b"),
        leftover: re(r"(?i)[x_\-]+\d{1,4}"),
    }
});

/// Cleans the title of an episode file. `file_name` may carry folders and an extension.
pub fn clean_episode_title<S: AsRef<str>>(
    file_name: &str,
    show_name: &str,
    bad_words: &[S],
) -> String {
    let p = &*CLEAN;
    let base = file_name.rsplit(['/', '\\']).next().unwrap_or(file_name);
    let mut s = strip_extension(base);
    s = PATTERNS.resolution.replace_all(&s, " ").into_owned();
    s = PATTERNS.year_group.replace_all(&s, " ").into_owned();
    s = PATTERNS.hash_group.replace_all(&s, " ").into_owned();
    s = p.imdb.replace_all(&s, " ").into_owned();
    s = date::remove_for_title(&s);
    s = remove_stop_and_bad_words_tv(&s, bad_words);
    s = p.stacking_word.replace_all(&s, "$1").into_owned();

    let show = show_name.trim();
    if !show.is_empty() && without_accents(trim_delimiters(&s)) == without_accents(show) {
        return String::new();
    }
    if let Some(prefix) = show_pattern(show, true) {
        let without_show = prefix.replace(&s, "$1").into_owned();
        if has_content(&without_show) {
            s = without_show;
        }
    }

    let snapshot = normalize(&s);
    let after_season = p.season_episode.replace_all(&s, "$1").into_owned();
    let marker_free = p
        .number_x_number
        .replace_all(&after_season, "$1")
        .into_owned();
    if marker_free != s {
        if !has_content(&marker_free) {
            return snapshot;
        }
        s = marker_free;
    }

    let without_roman = p
        .roman
        .replace_all(&s, |c: &regex::Captures<'_>| {
            if roman_value(&c[2]).is_some() {
                format!("{} {}", &c[1], &c[3])
            } else {
                c[0].to_owned()
            }
        })
        .into_owned();
    s = keep_content(s, without_roman);
    let without_episode = p.lone_episode.replace_all(&s, " ").into_owned();
    s = keep_content(s, without_episode);
    let without_leftover = p.leftover.replace_all(&s, " ").into_owned();
    s = keep_content(s, without_leftover);
    normalize(&s)
}

/// `after` when it still has a word or number in it, otherwise `before`.
fn keep_content(before: String, after: String) -> String {
    if has_content(&after) { after } else { before }
}

fn has_content(text: &str) -> bool {
    text.chars().any(char::is_alphanumeric)
}

fn is_delimiter(c: char) -> bool {
    c.is_whitespace() || "[](){}_,.-".contains(c)
}

fn trim_delimiters(text: &str) -> &str {
    text.trim_matches(is_delimiter)
}

/// Delimiters become single spaces.
fn normalize(text: &str) -> String {
    text.split(is_delimiter)
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}
