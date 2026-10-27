//! Title spellings and the text queries sent to the text-search sources.
//!
//! Everything here is pure. [`normalise`] is the one way titles are compared; [`match_key`] is
//! the looser key the relevance check uses (it also ignores spaces, so "Widow s Bay", "Widows
//! Bay" and "Widow's Bay" are one title); [`variants`] lists the spellings worth searching and
//! [`text_queries`] adds the scope suffix and applies the cap of [`MAX_TEXT_QUERIES`].

use std::collections::HashSet;

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

use crate::model::{Scope, TorrentQuery};

/// The most text queries sent to one source for one search.
pub const MAX_TEXT_QUERIES: usize = 3;

fn is_apostrophe(c: char) -> bool {
    matches!(
        c,
        '\'' | '\u{2019}' | '\u{2018}' | '\u{02BC}' | '`' | '\u{00B4}'
    )
}

fn collapse_spaces(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// One pass of [`normalise`].
fn normalise_once(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.to_lowercase().nfkd() {
        if is_combining_mark(c) || is_apostrophe(c) {
            continue;
        }
        if c == '&' {
            out.push_str(" and ");
        } else if c.is_alphanumeric() {
            out.push(c);
        } else {
            out.push(' ');
        }
    }
    collapse_spaces(&out)
}

/// The comparison form of a title: lower case, no accents, apostrophes dropped, `&` read as
/// "and", every other punctuation mark or separator a single space.
///
/// Idempotent: normalising the result changes nothing.
#[must_use]
pub fn normalise(text: &str) -> String {
    // One pass is enough for nearly every input; looping until nothing changes makes the
    // idempotence a guarantee rather than a property of the Unicode tables.
    let mut current = normalise_once(text);
    for _ in 0..4 {
        let next = normalise_once(&current);
        if next == current {
            break;
        }
        current = next;
    }
    current
}

/// [`normalise`] without the spaces: the key titles are matched by.
///
/// "Widow's Bay", "Widows Bay" and the release spelling "Widow s Bay" share one key, as do
/// "Spider-Man" and "Spiderman".
#[must_use]
pub fn match_key(text: &str) -> String {
    normalise(text).chars().filter(|c| *c != ' ').collect()
}

/// The match keys of every title a release may carry: the title, the original title and the
/// alternative titles.
#[must_use]
pub fn accepted_keys(query: &TorrentQuery) -> HashSet<String> {
    std::iter::once(query.title.as_str())
        .chain(query.original_title.as_deref())
        .chain(query.alternative_titles.iter().map(String::as_str))
        .map(match_key)
        .filter(|key| !key.is_empty())
        .collect()
}

fn strip_accents(text: &str) -> String {
    text.nfkd().filter(|c| !is_combining_mark(*c)).collect()
}

/// Punctuation read as a word break: `Mission: Impossible - Fallout` becomes
/// `Mission Impossible Fallout`.
fn strip_punctuation(text: &str) -> String {
    let spaced: String = text
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c.is_whitespace() || is_apostrophe(c) || c == '&' {
                c
            } else {
                ' '
            }
        })
        .collect();
    let without_apostrophes: String = spaced.chars().filter(|c| !is_apostrophe(*c)).collect();
    collapse_spaces(&without_apostrophes)
}

/// `&` read as "and" and the other way round.
fn swap_and(text: &str) -> Option<String> {
    if text.contains('&') {
        return Some(collapse_spaces(&text.replace('&', " and ")));
    }
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.iter().any(|word| word.eq_ignore_ascii_case("and")) {
        let swapped: Vec<&str> = words
            .iter()
            .map(|word| {
                if word.eq_ignore_ascii_case("and") {
                    "&"
                } else {
                    word
                }
            })
            .collect();
        return Some(swapped.join(" "));
    }
    None
}

/// The spellings of a title worth searching, best first and without duplicates:
/// the title as written; with apostrophes removed; with apostrophes replaced by a space; with
/// punctuation removed; with `&` and "and" swapped; without accents; then the original title and
/// the alternative titles as written.
///
/// A spelling is a duplicate when it is the same words in the same case-folded form, so "Amélie"
/// and "Amelie" are both kept (a source may match only one) but nothing is listed twice.
#[must_use]
pub fn variants(title: &str, original: Option<&str>, alternatives: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut add = |candidate: String| {
        let candidate = collapse_spaces(&candidate);
        if candidate.is_empty() {
            return;
        }
        if seen.insert(candidate.to_lowercase()) {
            out.push(candidate);
        }
    };

    let written = collapse_spaces(title);
    add(written.clone());
    add(written.chars().filter(|c| !is_apostrophe(*c)).collect());
    add(written
        .chars()
        .map(|c| if is_apostrophe(c) { ' ' } else { c })
        .collect());
    add(strip_punctuation(&written));
    if let Some(swapped) = swap_and(&written) {
        add(swapped);
    }
    add(strip_accents(&written));
    for other in original
        .into_iter()
        .chain(alternatives.iter().map(String::as_str))
    {
        add(other.to_owned());
    }
    out
}

/// A season or episode code: `S01`, `S01E02`.
fn season_code(season: u16) -> String {
    format!("S{season:02}")
}

fn episode_code(season: u16, episode: u16) -> String {
    format!("S{season:02}E{episode:02}")
}

/// The text queries for a title and scope, in the order they are sent (the first matters most):
///
/// * movie: each spelling with the year (just the spelling without a year);
/// * show: each spelling;
/// * season: each spelling with `S01`;
/// * episode: the first spelling with `S01E02`, the first with `S01` (season packs), then the
///   further spellings with `S01E02`.
///
/// At most [`MAX_TEXT_QUERIES`], no duplicates.
#[must_use]
pub fn text_queries(query: &TorrentQuery) -> Vec<String> {
    let spellings = variants(
        &query.title,
        query.original_title.as_deref(),
        &query.alternative_titles,
    );
    let mut out: Vec<String> = Vec::new();
    match query.scope {
        Scope::Movie => {
            for spelling in &spellings {
                out.push(match query.year {
                    Some(year) => format!("{spelling} {year}"),
                    None => spelling.clone(),
                });
            }
        }
        Scope::Show => out.extend(spellings.iter().cloned()),
        Scope::Season(season) => {
            for spelling in &spellings {
                out.push(format!("{spelling} {}", season_code(season)));
            }
        }
        Scope::Episode(season, episode) => {
            if let Some(first) = spellings.first() {
                out.push(format!("{first} {}", episode_code(season, episode)));
                out.push(format!("{first} {}", season_code(season)));
            }
            for spelling in spellings.iter().skip(1) {
                out.push(format!("{spelling} {}", episode_code(season, episode)));
            }
        }
    }
    out.truncate(MAX_TEXT_QUERIES);
    out
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::model::{MediaKind, TitleIds};

    fn query(title: &str, year: Option<u16>, scope: Scope) -> TorrentQuery {
        TorrentQuery {
            kind: if matches!(scope, Scope::Movie) {
                MediaKind::Movie
            } else {
                MediaKind::Show
            },
            ids: TitleIds::default(),
            title: title.to_owned(),
            original_title: None,
            alternative_titles: Vec::new(),
            year,
            scope,
            runtime_minutes: None,
            anime: false,
            library_copy: None,
        }
    }

    #[test]
    fn normalise_folds_case_accents_apostrophes_and_punctuation() {
        assert_eq!(normalise("Widow's Bay"), "widows bay");
        assert_eq!(normalise("Widow\u{2019}s  Bay"), "widows bay");
        assert_eq!(normalise("Amélie"), "amelie");
        assert_eq!(normalise("Léon: The Professional"), "leon the professional");
        assert_eq!(normalise("Fast & Furious"), "fast and furious");
        assert_eq!(
            normalise("Mission: Impossible \u{2013} Fallout"),
            "mission impossible fallout"
        );
        assert_eq!(normalise("Spider-Man"), "spider man");
        assert_eq!(normalise("  "), "");
        assert_eq!(
            normalise("\u{FF21}\u{FF22}"),
            "ab",
            "full-width letters fold (NFKD)"
        );
    }

    #[test]
    fn match_key_ignores_spaces_so_the_release_spelling_matches() {
        assert_eq!(match_key("Widow's Bay"), match_key("Widow s Bay"));
        assert_eq!(match_key("Widows Bay"), match_key("widow.s.bay"));
        assert_eq!(match_key("Spider-Man"), match_key("Spiderman"));
        assert_ne!(match_key("Dune"), match_key("Dune Part Two"));
    }

    #[test]
    fn apostrophe_title_gets_the_written_and_the_stripped_spelling() {
        let queries = text_queries(&query("Widow's Bay", Some(2019), Scope::Movie));
        assert_eq!(queries[0], "Widow's Bay 2019");
        assert_eq!(queries[1], "Widows Bay 2019");
        assert_eq!(queries[2], "Widow s Bay 2019");
        assert_eq!(queries.len(), 3);
    }

    #[test]
    fn accents_give_both_spellings_once() {
        let queries = text_queries(&query("Amélie", None, Scope::Movie));
        assert_eq!(queries, vec!["Amélie", "Amelie"]);
        let queries = text_queries(&query("Léon", Some(1994), Scope::Movie));
        assert_eq!(queries, vec!["Léon 1994", "Leon 1994"]);
    }

    #[test]
    fn punctuation_and_the_ampersand_each_add_a_spelling() {
        assert_eq!(
            variants("Mission: Impossible \u{2013} Fallout", None, &[]),
            vec![
                "Mission: Impossible \u{2013} Fallout".to_owned(),
                "Mission Impossible Fallout".to_owned()
            ]
        );
        assert_eq!(
            variants("Fast & Furious", None, &[]),
            vec!["Fast & Furious", "Fast and Furious"]
        );
        assert_eq!(
            variants("Fast and Furious", None, &[]),
            vec!["Fast and Furious", "Fast & Furious"]
        );
    }

    #[test]
    fn a_plain_title_has_one_spelling() {
        assert_eq!(variants("Dune", None, &[]), vec!["Dune"]);
        assert!(variants("   ", None, &[]).is_empty());
    }

    #[test]
    fn original_and_alternative_titles_follow_and_the_cap_is_three() {
        let mut q = query("The Handmaiden", Some(2016), Scope::Movie);
        q.original_title = Some("\u{C544}\u{AC00}\u{C528}".to_owned());
        q.alternative_titles = vec!["Mademoiselle".to_owned(), "La Servante".to_owned()];
        let all = variants(&q.title, q.original_title.as_deref(), &q.alternative_titles);
        assert_eq!(all.len(), 4);
        let queries = text_queries(&q);
        assert_eq!(
            queries,
            vec![
                "The Handmaiden 2016".to_owned(),
                "\u{C544}\u{AC00}\u{C528} 2016".to_owned(),
                "Mademoiselle 2016".to_owned()
            ]
        );
    }

    #[test]
    fn an_original_title_equal_to_the_title_is_not_searched_twice() {
        let mut q = query("Dune", Some(2021), Scope::Movie);
        q.original_title = Some("dune".to_owned());
        q.alternative_titles = vec!["DUNE".to_owned()];
        assert_eq!(text_queries(&q), vec!["Dune 2021"]);
    }

    #[test]
    fn a_movie_without_a_year_has_no_year_suffix() {
        assert_eq!(
            text_queries(&query("Dune", None, Scope::Movie)),
            vec!["Dune"]
        );
    }

    #[test]
    fn an_episode_searches_the_code_and_the_season_pack() {
        let queries = text_queries(&query("Severance", None, Scope::Episode(1, 2)));
        assert_eq!(queries, vec!["Severance S01E02", "Severance S01"]);
    }

    #[test]
    fn an_episode_with_more_spellings_keeps_the_pack_query_inside_the_cap() {
        let queries = text_queries(&query("Marvel's Agents", None, Scope::Episode(2, 5)));
        assert_eq!(
            queries,
            vec![
                "Marvel's Agents S02E05",
                "Marvel's Agents S02",
                "Marvels Agents S02E05"
            ]
        );
    }

    #[test]
    fn a_season_adds_the_season_code_and_a_show_nothing() {
        assert_eq!(
            text_queries(&query("Amélie", None, Scope::Season(3))),
            vec!["Amélie S03", "Amelie S03"]
        );
        assert_eq!(
            text_queries(&query("Amélie", None, Scope::Show)),
            vec!["Amélie", "Amelie"]
        );
    }

    #[test]
    fn season_and_episode_numbers_are_two_digits_or_more() {
        let queries = text_queries(&query("Show", None, Scope::Episode(12, 103)));
        assert_eq!(queries, vec!["Show S12E103", "Show S12"]);
    }

    #[test]
    fn an_empty_title_sends_nothing() {
        assert!(text_queries(&query("", Some(2000), Scope::Movie)).is_empty());
        assert!(text_queries(&query(" ", None, Scope::Episode(1, 1))).is_empty());
    }

    #[test]
    fn accepted_keys_cover_the_title_original_and_alternatives() {
        let mut q = query("Amélie", Some(2001), Scope::Movie);
        q.original_title = Some("Le Fabuleux Destin d'Am\u{E9}lie Poulain".to_owned());
        q.alternative_titles = vec!["Amelie from Montmartre".to_owned(), String::new()];
        let keys = accepted_keys(&q);
        assert_eq!(keys.len(), 3);
        assert!(keys.contains("amelie"));
        assert!(keys.contains("lefabuleuxdestindameliepoulain"));
    }

    proptest! {
        #[test]
        fn normalise_is_idempotent(text in "\\PC{0,40}") {
            let once = normalise(&text);
            prop_assert_eq!(normalise(&once), once);
        }

        #[test]
        fn normalise_output_has_no_double_or_edge_spaces(text in "\\PC{0,40}") {
            let out = normalise(&text);
            prop_assert!(!out.contains("  "));
            prop_assert_eq!(out.trim(), out.as_str());
        }

        #[test]
        fn every_variant_is_the_same_title_as_one_of_the_accepted_titles(
            title in "[A-Za-z0-9 '&:\u{E9}\u{E8}\u{2019}-]{1,24}",
            original in proptest::option::of("[A-Za-z0-9 ']{1,16}"),
            alt in proptest::collection::vec("[A-Za-z0-9 ']{1,16}", 0..3),
        ) {
            let mut q = query(&title, None, Scope::Show);
            q.original_title = original;
            q.alternative_titles = alt;
            let accepted = accepted_keys(&q);
            for variant in variants(&q.title, q.original_title.as_deref(), &q.alternative_titles) {
                let key = match_key(&variant);
                prop_assert!(
                    key.is_empty() || accepted.contains(&key),
                    "variant {variant:?} (key {key:?}) not in {accepted:?}"
                );
            }
        }

        #[test]
        fn text_queries_are_at_most_three_and_distinct(
            title in "[A-Za-z0-9 '&]{1,20}",
            season in 1u16..30,
            episode in 1u16..60,
        ) {
            for scope in [Scope::Movie, Scope::Show, Scope::Season(season), Scope::Episode(season, episode)] {
                let queries = text_queries(&query(&title, Some(2000), scope));
                prop_assert!(queries.len() <= MAX_TEXT_QUERIES);
                let unique: HashSet<_> = queries.iter().map(|q| q.to_lowercase()).collect();
                prop_assert_eq!(unique.len(), queries.len());
            }
        }
    }
}
