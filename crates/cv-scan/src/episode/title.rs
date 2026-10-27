//! The title an episode gets from its file name.
//!
//! [`clean_episode_title`](super::clean_episode_title) gives what is left of a name once the
//! markers, release tags and a leading show name are removed. That text is what the Kodi formats'
//! parser gives, and the compatibility fixtures pin it byte for byte. It is often no title at all:
//! `[SubGroup] One Piece - 001 [1080p].mkv` leaves `SubGroup One Piece 001`, `Amelie S01E01.mkv`
//! in the show `Amélie Show` leaves `Amelie`, and `Dark - S01E01.mkv` leaves `S01E01`.
//!
//! [`file_episode_title`] turns that text into the episode's title:
//!
//! 1. the release group goes: a leading `[Group]` or `(Group)`, and a trailing `-GROUP` that
//!    follows a release tag (`x264-GRP`, not `Spider-Man`);
//! 2. the show name and the episode number at the start or the end go (`001`, `101` for S01E01,
//!    `S01E01`, `1x01`, `E05`), and so do words of the show name before the season and episode
//!    marker (`Amelie S01E02` in the show `Amélie Show`), with or without accents;
//! 3. what is left is the title. When nothing is left, or only words of the show name and the
//!    number, the episode has no title.

use std::sync::LazyLock;

use regex::Regex;

use super::{strip_extension, without_accents};

struct Patterns {
    leading_group: Regex,
    trailing_group: Regex,
    marker: Regex,
    marker_in_name: Regex,
}

static TITLE: LazyLock<Patterns> = LazyLock::new(|| {
    let re = |p: &str| Regex::new(p).expect("static title pattern");
    Patterns {
        leading_group: re(r"^\s*[\[(]([^\[\]()]+)[\])]"),
        // `x264-GRP` at the end of the name: the token before the dash and the group.
        trailing_group: re(r"(?:^|[\s._\[\]()])([^\s._\-\[\]()]+)-([\p{L}\p{N}]+)\s*$"),
        // S01E01, S01E01E02, 1x01, E05, Ep05
        marker: re(r"(?i)^(?:s\d{1,4}(?:[ex]\d{1,4})*|\d{1,4}x\d{1,4}(?:x\d{1,4})*|ep?\d{1,4})$"),
        // The first season and episode marker of a name (`S01E02`, `1x02`).
        marker_in_name: re(r"(?i)(?:^|[^a-z0-9])(?:s\d{1,4}[\s._-]?[ex]\d{1,4}|\d{1,4}x\d{1,4})"),
    }
});

/// The title of an episode from its file name: `cleaned` (what
/// [`clean_episode_title`](super::clean_episode_title) gave for `file_name`) without the release
/// group, the show name and the episode number. Empty when nothing else is in the name.
///
/// `season` and `numbers` are the episode's season and episode numbers as the scan settled them,
/// so `001` of an absolute-numbered show and `101` of S01E01 are known as the number.
pub fn file_episode_title(
    cleaned: &str,
    file_name: &str,
    show_name: &str,
    season: Option<u32>,
    numbers: &[u32],
) -> String {
    let p = &*TITLE;
    // The words of the text with where they stand, so the title keeps its own punctuation.
    let mut words: Vec<(usize, &str)> = word_spans(cleaned).collect();
    let base = file_name.rsplit(['/', '\\']).next().unwrap_or(file_name);
    let stem = strip_extension(base);

    if let Some(group) = p.leading_group.captures(&stem) {
        let group_words: Vec<String> = word_spans(&group[1]).map(|(_, w)| key(w)).collect();
        if starts_with(&words, &group_words) {
            words.drain(..group_words.len());
        }
    }
    if let Some(found) = p.trailing_group.captures(&stem) {
        let before = key(&found[1]);
        let group = key(&found[2]);
        let follows_title = words.len() >= 2 && key(words[words.len() - 2].1) == before;
        if words.last().is_some_and(|(_, w)| key(w) == group) && !follows_title {
            words.pop();
        }
    }

    let show: Vec<String> = word_spans(show_name).map(|(_, w)| key(w)).collect();
    // Words of the show name before the marker are the show, even a part of it (`Amelie` for
    // `Amélie Show`): `Amelie S01E02 - Le Café` is titled `Le Café`.
    let named = p
        .leading_group
        .find(&stem)
        .map_or(stem.as_str(), |group| &stem[group.end()..]);
    if let Some(marker) = p.marker_in_name.find(named) {
        let lead: Vec<String> = word_spans(&named[..marker.start()])
            .map(|(_, w)| key(w))
            .collect();
        if lead.iter().all(|w| show.contains(w)) && starts_with(&words, &lead) {
            words.drain(..lead.len());
        }
    }
    let is_number = |word: &str| is_number_word(word, season, numbers);

    // The show name and the number at either end; inside, a word stays (`The 5 Doctors`).
    loop {
        let before = words.len();
        if starts_with(&words, &show) {
            words.drain(..show.len());
        }
        while words.first().is_some_and(|(_, w)| is_number(w)) {
            words.remove(0);
        }
        if ends_with(&words, &show) {
            words.truncate(words.len() - show.len());
        }
        while words.last().is_some_and(|(_, w)| is_number(w)) {
            words.pop();
        }
        if words.len() == before {
            break;
        }
    }
    // Only words of the show name and numbers: a part of the show name, no title.
    if words
        .iter()
        .all(|(_, w)| is_number(w) || show.contains(&key(w)))
    {
        return String::new();
    }
    let (Some(&(first, _)), Some(&(last, word))) = (words.first(), words.last()) else {
        return String::new();
    };
    cleaned[first..last + word.len()].to_owned()
}

/// A word that only restates the episode: a marker, or a number that is the episode number
/// (`001`), the season and episode together (`101`, `0101`) or the season.
fn is_number_word(word: &str, season: Option<u32>, numbers: &[u32]) -> bool {
    if TITLE.marker.is_match(word) {
        return true;
    }
    if word.is_empty() || !word.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let Ok(value) = word.parse::<u64>() else {
        return false;
    };
    let season = season.map(u64::from);
    numbers.iter().map(|&n| u64::from(n)).any(|n| {
        value == n
            || season.is_some_and(|s| value == s * 100 + n || value == s * 1000 + n)
            || season == Some(value)
    })
}

/// The words of a text (runs of letters and digits) with their byte offsets.
fn word_spans(text: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut start = None;
    let mut spans = Vec::new();
    for (i, c) in text.char_indices() {
        match (c.is_alphanumeric(), start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                spans.push((s, &text[s..i]));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        spans.push((s, &text[s..]));
    }
    spans.into_iter()
}

/// A word compared without regard to case or accents.
fn key(word: &str) -> String {
    without_accents(word)
}

fn starts_with(words: &[(usize, &str)], part: &[String]) -> bool {
    !part.is_empty()
        && words.len() >= part.len()
        && words.iter().zip(part).all(|((_, w), p)| key(w) == *p)
}

fn ends_with(words: &[(usize, &str)], part: &[String]) -> bool {
    !part.is_empty()
        && words.len() >= part.len()
        && words[words.len() - part.len()..]
            .iter()
            .zip(part)
            .all(|((_, w), p)| key(w) == *p)
}
