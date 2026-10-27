//! The absolute episode number of an anime file.
//!
//! A show in absolute order counts its episodes from the first one to the last, whatever the
//! seasons: `Show - 137.mkv` is episode 137 of the show. The regular parser reads that name as
//! season 1, episode 37, because three digits are a season and an episode in the numbering of
//! most TV releases. [`parse_absolute_episode`] reads the same names the other way: the one
//! number that is left once the release tags, the groups in brackets and the show name are taken
//! out is the absolute number.

use std::sync::LazyLock;

use regex::Regex;

use super::{Context, DELIMS, PATTERNS, date, strip_extension};
use crate::classify::classify;
use crate::parse::remove_stop_and_bad_words_tv;

/// A number of one to four digits between delimiters, with an optional version (`05v2`).
static NUMBER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"(?:^|[{DELIMS}])(\d{{1,4}})(?:v\d+)?(?:[{DELIMS}]|$)"
    ))
    .expect("static absolute number pattern")
});

/// Reads the absolute episode number from a path relative to the show folder.
///
/// Returns `None` when the name does not carry one: it has a season and episode marker
/// (`S02E05`, `2x05`), it is a special (`OVA 2`), a date-based file or a disc structure, or it has
/// no number at all. The accepted shapes are:
///
/// * `[Group] Show - 137 [1080p][ABCD1234].mkv` (a name with a `[CRC32]` hash);
/// * `Show - 137.mkv`, `Show 137.mkv`, `Show.137.1080p.mkv`, `Show - 137v2 - Title.mkv`;
/// * `Show - Episode 137.mkv`, `Show E137.mkv`.
///
/// When a name holds several numbers, the one that follows a dash wins (`Show 2 - 05`), then the
/// last one. `bad_words` are the user's bad words, as for [`super::parse_episode`].
pub fn parse_absolute_episode<S: AsRef<str>>(
    relative_path: &str,
    show_name: &str,
    bad_words: &[S],
) -> Option<u32> {
    let path = relative_path.replace('\\', "/");
    if classify(&path).disc_file {
        return None;
    }
    let (date, undated) = date::extract(&path);
    if date.is_some() {
        return None;
    }
    let file = undated.split('/').rfind(|c| !c.is_empty())?;
    let stem = strip_extension(file);
    let bad: Vec<&str> = bad_words.iter().map(AsRef::as_ref).collect();
    Context::new(show_name, &bad).detect_absolute(&stem)
}

impl Context<'_> {
    fn detect_absolute(&self, stem: &str) -> Option<u32> {
        let p = &*PATTERNS;
        if p.anime_hash.is_match(stem)
            && let Some(found) = self.detect_anime(stem)
        {
            return match found.season {
                Some(0) => None,
                _ => found.episodes.first().copied(),
            };
        }

        let mut s = remove_stop_and_bad_words_tv(stem, self.bad_words);
        for noise in [&p.resolution, &p.year_group, &p.hash_group, &p.anime_group] {
            s = noise.replace_all(&s, " ").into_owned();
        }
        s = format!(" {s} ");
        if p.multi.is_match(&s) || p.multi_x.is_match(&s) || p.special.is_match(&s) {
            return None;
        }
        if let Some(show) = &self.show_anywhere {
            s = show.replace_all(&s, "$1 $2").into_owned();
        }
        if let Some(c) = p
            .episode_word
            .captures(&s)
            .or_else(|| p.episode_only.captures(&s))
        {
            return c[1].parse().ok();
        }
        pick_number(&s)
    }
}

/// The number among the standalone numbers of `text`: the one after a dash, else the last.
fn pick_number(text: &str) -> Option<u32> {
    let mut found: Vec<(u32, bool)> = Vec::new();
    let mut at = 0;
    while at <= text.len() {
        let Some(c) = NUMBER.captures_at(text, at) else {
            break;
        };
        let digits = c.get(1)?;
        if let Ok(value) = digits.as_str().parse::<u32>() {
            let before = text[..digits.start()].trim_end_matches([' ', '_', '.']);
            found.push((value, before.ends_with('-')));
        }
        at = digits.end();
    }
    found
        .iter()
        .rev()
        .find(|(_, after_dash)| *after_dash)
        .or(found.last())
        .map(|(value, _)| *value)
}
