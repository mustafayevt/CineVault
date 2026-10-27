//! Episode and season parser for TV files.
//!
//! [`parse_episode`] takes the path of a video relative to its show folder (for example
//! `Season 2/Show Name - S02E05-E06 - Title.mkv`) and the show name, and returns:
//!
//! - the season and the episode numbers (several for a multi-episode file),
//! - the aired date for date-based shows (`2020-05-17`, `17.05.2020`),
//! - whether the file name carries a stacking marker (`cd1`, `part2`, `1of2`),
//! - the episode title that is left once markers, release tags and the show name are removed.
//!
//! The patterns, in the order they are tried on a name:
//!
//! 1. `S01E02`, `S01E02E03`, `S01E02-E04`, `S01E05-07` (a range of at most five episodes);
//! 2. `1x02`, `1x02x03`, `01x02 - 01x03`;
//! 3. a season word in many languages (`Season 3`, `Staffel 2`, `Temporada 1`);
//! 4. `Episode 5`, `Ep05`, `1/6` (episode 1 of 6);
//! 5. the show name is taken out, so numbers in it (`24`, `90210`, `The 100`) are not episodes;
//! 6. `Part II`, `Pt. IV` (roman numerals);
//! 7. `OVA 2`, `Special 01`, `SP04`, `Picture Drama 01`, which are season 0;
//! 8. ` S2` for the season and ` E05` for the episode;
//! 9. bare numbers: three digits are season and episode (`102`), two digits or one digit are the
//!    episode, four digits are season and episode only when the season is already known.
//!
//! A name with a `[CRC32]` hash (`[Group] Show - 05 [720p][ABCD1234]`) is anime and is read on its
//! own: the last number is the absolute episode number.
//!
//! The name alone is tried first. When it yields only an episode, the whole relative path is tried
//! too, so `Season 2/Show - 03.mkv` and `S02/03.mkv` give season 2. Without a season, the season
//! is 1 for a file with episodes, and the year for a date-based file. Disc structures
//! (`VIDEO_TS`, `BDMV`) are read from their folders.
//!
//! [`parse_absolute_episode`] reads the same names for a show in absolute order, where
//! `Show - 137.mkv` is episode 137 and not season 1, episode 37.
//!
//! The parser works on text only and behaves the same on every operating system. The byte-exact
//! fixtures are the `episodes` family in `tests/compat`, run through `cv-compat`.

mod absolute;
mod clean;
mod date;
mod title;

use std::fmt;
use std::sync::LazyLock;

use regex::Regex;

use crate::classify::classify;
use crate::parse::remove_stop_and_bad_words_tv;
use crate::stacking::parse_file_name;

pub use absolute::parse_absolute_episode;
pub use clean::clean_episode_title;
pub use date::AiredDate;
pub use title::file_episode_title;

/// What [`parse_episode`] found in a relative path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EpisodeParse {
    /// The season. `None` when the name shows none and there are no episodes and no date.
    /// Season 0 means specials.
    pub season: Option<u32>,
    /// The episode numbers in the order they were found. Several for a multi-episode file.
    pub episodes: Vec<u32>,
    /// The aired date of a date-based file.
    pub date: Option<AiredDate>,
    /// The file name has a stacking marker (`cd1`, `part2`, `-a`, `1of2`).
    pub stacking_marker_found: bool,
    /// The episode title that is left in the name. Empty when nothing is left.
    pub cleaned_title: String,
}

impl fmt::Display for EpisodeParse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.season {
            Some(season) => write!(f, "S{season:02}")?,
            None => write!(f, "S??")?,
        }
        for episode in &self.episodes {
            write!(f, "E{episode:02}")?;
        }
        if let Some(date) = self.date {
            write!(f, " {date}")?;
        }
        Ok(())
    }
}

/// Reads season, episodes and date from a path relative to the show folder.
///
/// `bad_words` are the user's bad words (regular expressions, matched without regard to case);
/// they are removed like release tags before the name is read.
pub fn parse_episode<S: AsRef<str>>(
    relative_path: &str,
    show_name: &str,
    bad_words: &[S],
) -> EpisodeParse {
    let bad: Vec<&str> = bad_words.iter().map(AsRef::as_ref).collect();
    let path = relative_path.replace('\\', "/");
    let components: Vec<&str> = path.split('/').filter(|c| !c.is_empty()).collect();
    let file = components.last().copied().unwrap_or("");
    let disc = classify(&path).disc_file;
    let stacking_marker_found = parse_file_name(file).is_stacked();

    // The date comes out of the path first, so its digits are never read as an episode.
    let (date, undated) = date::extract(&path);
    let undated_components: Vec<&str> = undated.split('/').filter(|c| !c.is_empty()).collect();
    let stem = strip_extension(undated_components.last().copied().unwrap_or(""));

    let ctx = Context::new(show_name, &bad);
    let mut found = if disc {
        // A disc structure is read from its folders; the file name says nothing.
        ctx.detect(&disc_folders(&undated_components), true)
    } else if let Some(anime) = ctx.detect_anime(&stem) {
        anime
    } else {
        let from_name = ctx.detect(&stem, false);
        let full = || {
            let mut parts: Vec<&str> = undated_components
                .iter()
                .take(undated_components.len().saturating_sub(1))
                .copied()
                .collect();
            let stem_text = stem.as_str();
            parts.push(stem_text);
            ctx.detect(&parts.join("/"), false)
        };
        merge(from_name, full)
    };

    if found.season.is_none() {
        if !found.episodes.is_empty() {
            found.season = Some(1);
        } else if let Some(date) = date {
            found.season = Some(date.year);
        }
    }
    let cleaned_title = if disc || found.anime_hash {
        String::new()
    } else {
        clean_episode_title(file, show_name, &bad)
    };
    EpisodeParse {
        season: found.season,
        episodes: found.episodes,
        date,
        stacking_marker_found,
        cleaned_title,
    }
}

/// [`parse_episode`] without user bad words.
pub fn detect_episode_from_filename(relative_path: &str, show_name: &str) -> EpisodeParse {
    parse_episode::<&str>(relative_path, show_name, &[])
}

/// Combines the result for the file name with the result for the whole path.
///
/// When the name gives episodes but no season, the path is read too. Its result replaces the
/// first one if it has a season and the same number of episodes; otherwise only its season is
/// taken, or its episodes when it found more of them (`Show 1/6`). When the name gives nothing,
/// the path result is used.
fn merge(from_name: Found, full: impl FnOnce() -> Found) -> Found {
    if from_name.season.is_some() {
        return from_name;
    }
    if from_name.episodes.is_empty() {
        return full();
    }
    let whole = full();
    let mut found = from_name;
    if whole.season.is_some() && whole.episodes.len() == found.episodes.len() {
        return whole;
    }
    if whole.season.is_some() {
        found.season = whole.season;
    } else if whole.episodes.len() > found.episodes.len() {
        found.episodes = whole.episodes;
    }
    found
}

/// The folders of a disc file: everything before the disc folder, or before the file.
fn disc_folders(components: &[&str]) -> String {
    let disc_folder = components.iter().position(|c| {
        ["VIDEO_TS", "BDMV", "HVDVD_TS"]
            .iter()
            .any(|d| c.eq_ignore_ascii_case(d))
    });
    let end = disc_folder.unwrap_or_else(|| components.len().saturating_sub(1));
    components[..end].join("/")
}

/// The season named by a season word and a number in a file or folder name (`Season 3`,
/// `Staffel 02`, `season01-poster.jpg`). `None` when the name has no such word.
pub(crate) fn season_from_word(text: &str) -> Option<u32> {
    PATTERNS
        .season_long
        .captures(text)
        .and_then(|c| c[2].parse().ok())
}

/// True when a folder name is only a season: `Season 1`, `Staffel 02`, `S01`, `Specials`. A
/// TV library folder that holds such a folder is itself one show, not a folder of shows.
pub(crate) fn is_season_folder_name(name: &str) -> bool {
    static WHOLE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(&format!(
            r"(?i)^(?:(?:{SEASON_WORDS})[{DELIMS}]*\d{{1,4}}|s[{DELIMS}]?\d{{1,4}}|specials?)$"
        ))
        .expect("static season folder pattern")
    });
    WHOLE.is_match(name.trim())
}

fn strip_extension(name: &str) -> String {
    PATTERNS.extension.replace(name, "").into_owned()
}

/// A partial result while the patterns are tried.
#[derive(Debug, Clone, Default)]
struct Found {
    season: Option<u32>,
    episodes: Vec<u32>,
    /// The name was read by the anime rules.
    anime_hash: bool,
}

/// The delimiters between words, for use inside a character class.
const DELIMS: &str = r"\s_.\-\[\](){},";

/// The season words of the pattern list, one per language.
const SEASON_WORDS: &str = "series|season|الموسم|sezóna|sæson|staffel|σεζόν|temporada|فصل|kausi|saison|sezona|évad|þáttaröð|stagione|시즌|seizoen|sesong|sezon|сезон|сезона|säsong|சீசன்";

struct Patterns {
    extension: Regex,
    resolution: Regex,
    year_group: Regex,
    hash_group: Regex,
    multi: Regex,
    episode_number: Regex,
    range_shape: Regex,
    multi_x: Regex,
    season_long: Regex,
    ratio: Regex,
    episode_word: Regex,
    roman: Regex,
    special: Regex,
    season_only: Regex,
    episode_only: Regex,
    bare: [Regex; 4],
    anime_hash: Regex,
    anime_group: Regex,
    anime_number: Regex,
}

static PATTERNS: LazyLock<Patterns> = LazyLock::new(|| {
    let re = |p: &str| Regex::new(&p.replace("<D>", DELIMS)).expect("static episode pattern");
    let bare = |digits: usize| re(&format!(r"(?:^|[<D>/])(\d{{{digits}}})(?:[<D>/]|$)"));
    Patterns {
        extension: re(r"\.[A-Za-z0-9]{2,4}$"),
        resolution: re(r"\d{3,4}x\d{3,4}"),
        year_group: re(r"[(\[]\d{4}[)\]]"),
        hash_group: re(r"\[[0-9A-Fa-f]{8}\]"),
        multi: re(r"(?i)(?:^|[^a-z0-9])s(\d{1,4})[ _]?((?:[epx.\-]+\d{1,4})+)"),
        episode_number: re(r"(?i)[epx_\-]+(\d{1,4})"),
        range_shape: re(r"(?i)^[epx]*\d{1,4}-[epx]*\d{1,4}$"),
        multi_x: re(r"(?i)(?:^|[^0-9a-z])(\d{1,4})(x\d{1,4}(?:[epx]+\d{1,4})*)"),
        season_long: re(&format!(
            r"(?i)(?:^|[^\p{{L}}\p{{N}}])({SEASON_WORDS})[<D>]?(\d{{1,4}})"
        )),
        ratio: re(r"(?:^|[<D>])(\d{1,2})\s*[⧸/⁄]\s*(\d{1,2})(?:[^0-9]|$)"),
        episode_word: re(r"(?i)(?:^|[^\p{L}\p{N}])(?:episode|ep)[. _\-]*(\d{1,4})"),
        roman: re(r"(?i)(?:^|[^a-z0-9])(?:part|pt)[._\s]+([mdclxvi]+)(?:[^a-z0-9]|$)"),
        special: re(
            r"(?i)(?:^|[<D>])(?:special|ova|oav|sp|picture[<D>]*drama)[<D>]*(\d{1,3})(?:[<D>]|$)",
        ),
        season_only: re(r"(?i)[<D>]s[<D>]?(\d{1,4})(?:[^0-9]|$)"),
        episode_only: re(r"(?i)[<D>]ep?[<D>]?(\d{1,4})(?:[^0-9]|$)"),
        bare: [bare(4), bare(3), bare(2), bare(1)],
        anime_hash: re(r"[\[(][0-9A-Fa-f]{8}[\])]"),
        anime_group: re(r"\[[^\]]*\]|\([^)]*\)"),
        anime_number: re(r"(?i)(?:^|[<D>])(\d{1,4})(?:v\d+)?[<D>]*$"),
    }
});

/// The show name as a pattern that finds it with any delimiters between its words.
fn show_pattern(show_name: &str, anchored: bool) -> Option<Regex> {
    let words: Vec<String> = show_name
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|word| word.chars().map(letter_pattern).collect())
        .collect();
    if words.is_empty() {
        return None;
    }
    let body = words.join(r"[^\p{L}\p{N}]+");
    let pattern = if anchored {
        format!(r"(?i)^\s*{body}(\z|[^\p{{L}}\p{{N}}])")
    } else {
        format!(r"(?i)(^|[^\p{{L}}\p{{N}}]){body}(\z|[^\p{{L}}\p{{N}}])")
    };
    Regex::new(&pattern).ok()
}

/// The Latin letters that carry accents, by the letter without them. A file name often spells
/// the show without its accents (`Amelie` for `Amélie`) or the other way round.
const ACCENTED: &[(char, &str)] = &[
    ('a', "àáâãäåāăą"),
    ('c', "çćĉċč"),
    ('d', "ďđ"),
    ('e', "èéêëēĕėęě"),
    ('g', "ĝğġģ"),
    ('h', "ĥħ"),
    ('i', "ìíîïĩīĭįı"),
    ('j', "ĵ"),
    ('k', "ķ"),
    ('l', "ĺļľŀł"),
    ('n', "ñńņňŉ"),
    ('o', "òóôõöøōŏő"),
    ('r', "ŕŗř"),
    ('s', "śŝşšș"),
    ('t', "ţťŧț"),
    ('u', "ùúûüũūŭůűų"),
    ('w', "ŵ"),
    ('y', "ýÿŷ"),
    ('z', "źżž"),
];

/// The letter without its accent, lower case; other characters lower-cased.
pub(crate) fn base_letter(c: char) -> char {
    let lower = c.to_lowercase().next().unwrap_or(c);
    ACCENTED
        .iter()
        .find(|(_, accented)| accented.contains(lower))
        .map_or(lower, |(base, _)| *base)
}

/// `text` lower-cased with the accents of its Latin letters dropped.
pub(crate) fn without_accents(text: &str) -> String {
    text.chars().map(base_letter).collect()
}

/// A pattern for one letter of a show name that also matches it with or without an accent.
fn letter_pattern(c: char) -> String {
    let base = base_letter(c);
    match ACCENTED.iter().find(|(b, _)| *b == base) {
        Some((b, accented)) => format!("[{b}{accented}]"),
        None => regex::escape(&c.to_string()),
    }
}

struct Context<'a> {
    show_anywhere: Option<Regex>,
    bad_words: &'a [&'a str],
}

impl<'a> Context<'a> {
    fn new(show_name: &str, bad_words: &'a [&'a str]) -> Self {
        Context {
            show_anywhere: show_pattern(show_name, false),
            bad_words,
        }
    }

    /// Reads one name or path (without extension). `disc` limits the patterns to the ones that
    /// find a season or an episode in a folder name.
    fn detect(&self, text: &str, disc: bool) -> Found {
        let p = &*PATTERNS;
        let mut s = text.to_owned();
        s = remove_stop_and_bad_words_tv(&s, self.bad_words);
        s = p.resolution.replace_all(&s, " ").into_owned();
        s = p.year_group.replace_all(&s, " ").into_owned();
        s = p.hash_group.replace_all(&s, " ").into_owned();
        s = format!(" {s} ");
        let mut found = Found::default();

        // 1. S01E02, S01E02-E04, S01E05-07
        if let Some(c) = p.multi.captures(&s) {
            found.season = c[1].parse().ok();
            found.episodes = episode_numbers(&c[2]);
            if found.episodes.len() == 2 && p.range_shape.is_match(&c[2]) {
                let (from, to) = (found.episodes[0], found.episodes[1]);
                if from < to && to - from <= 5 {
                    found.episodes = (from..=to).collect();
                }
            }
        }

        // 2. 1x02, 1x02x03, 01x02 - 01x03
        if found.episodes.is_empty() {
            for c in p.multi_x.captures_iter(&s) {
                if found.season.is_none() {
                    found.season = c[1].parse().ok();
                }
                for episode in episode_numbers(&c[2]) {
                    if !found.episodes.contains(&episode) {
                        found.episodes.push(episode);
                    }
                }
            }
        }

        // 3. Season 3, Staffel 2 (the words are taken out of the name)
        if found.season.is_none()
            && let Some(c) = p.season_long.captures(&s)
        {
            found.season = c[2].parse().ok();
            let (start, end) = (
                c.get(1).map_or(0, |m| m.start()),
                c.get(2).map_or(0, |m| m.end()),
            );
            s.replace_range(start..end, " ");
        }
        if disc {
            return found;
        }

        // 4. 1/6, Episode 5
        if found.episodes.is_empty() {
            if let Some(c) = p.ratio.captures(&s) {
                let (episode, total): (u32, u32) =
                    (c[1].parse().unwrap_or(0), c[2].parse().unwrap_or(0));
                if episode <= total {
                    found.episodes = vec![total, episode];
                }
            } else if let Some(c) = p.episode_word.captures(&s) {
                found.episodes = c[1].parse().into_iter().collect();
            }
        }

        // 5. numbers in the show name are not episodes
        if let Some(show) = &self.show_anywhere {
            s = show.replace_all(&s, "$1 $2").into_owned();
        }

        // 6. Part II
        if found.episodes.is_empty()
            && let Some(c) = p.roman.captures(&s)
            && let Some(value) = roman_value(&c[1])
        {
            found.episodes = vec![value];
        }

        // 7. OVA 2, Special 01, SP04
        if found.episodes.is_empty()
            && found.season.is_none()
            && let Some(c) = p.special.captures(&s)
        {
            found.season = Some(0);
            found.episodes = c[1].parse().into_iter().collect();
        }

        // 8. S2, E05
        if found.season.is_none()
            && let Some(c) = p.season_only.captures(&s)
        {
            found.season = c[1].parse().ok();
        }
        if found.episodes.is_empty()
            && let Some(c) = p.episode_only.captures(&s)
        {
            found.episodes = c[1].parse().into_iter().collect();
        }

        // 9. bare numbers
        if found.episodes.is_empty() {
            self.bare_numbers(&s, &mut found);
        }
        found
    }

    /// Bare numbers: the last number of each length counts, four digits first.
    fn bare_numbers(&self, s: &str, found: &mut Found) {
        let p = &*PATTERNS;
        for (index, digits) in [4usize, 3, 2, 1].into_iter().enumerate() {
            let Some(value) = last_run(&p.bare[index], s) else {
                continue;
            };
            match digits {
                4 => {
                    // season and episode, only when the season is known
                    if let Some(season) = found.season
                        && value / 100 == season
                    {
                        found.episodes.push(value % 100);
                    }
                }
                3 => match found.season {
                    None => {
                        found.season = Some(value / 100);
                        found.episodes.push(value % 100);
                    }
                    Some(season) if value / 100 == season => found.episodes.push(value % 100),
                    Some(_) => found.episodes.push(value),
                },
                _ => found.episodes.push(value),
            }
        }
    }

    /// The anime rules for a name with a `[CRC32]` hash. `None` when the name has no hash or no
    /// episode number.
    fn detect_anime(&self, stem: &str) -> Option<Found> {
        let p = &*PATTERNS;
        if !p.anime_hash.is_match(stem) {
            return None;
        }
        let without_hash = p.anime_hash.replace_all(stem, " ");
        let without_groups = p.anime_group.replace_all(&without_hash, " ");
        let text = format!(
            " {} ",
            without_groups
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        );
        if let Some(c) = p.special.captures(&text) {
            return Some(Found {
                season: Some(0),
                episodes: c[1].parse().into_iter().collect(),
                anime_hash: true,
            });
        }
        let number = p.anime_number.captures(&text)?;
        Some(Found {
            season: None,
            episodes: number[1].parse().into_iter().collect(),
            anime_hash: true,
        })
    }
}

/// The episode numbers in text such as `E02E03` or `-04-05`.
fn episode_numbers(text: &str) -> Vec<u32> {
    let mut numbers = Vec::new();
    for c in PATTERNS.episode_number.captures_iter(text) {
        if let Ok(n) = c[1].parse::<u32>()
            && !numbers.contains(&n)
        {
            numbers.push(n);
        }
    }
    numbers
}

/// The number of the last run of digits that the pattern finds. Runs may share a delimiter.
fn last_run(re: &Regex, text: &str) -> Option<u32> {
    let mut last = None;
    let mut at = 0;
    while at <= text.len() {
        let Some(c) = re.captures_at(text, at) else {
            break;
        };
        let digits = c.get(1)?;
        last = digits.as_str().parse().ok();
        at = digits.end();
    }
    last
}

/// The value of a roman numeral such as `IV`. `None` for an invalid or empty one.
fn roman_value(text: &str) -> Option<u32> {
    let digit = |c: char| match c.to_ascii_uppercase() {
        'I' => Some(1),
        'V' => Some(5),
        'X' => Some(10),
        'L' => Some(50),
        'C' => Some(100),
        'D' => Some(500),
        'M' => Some(1000),
        _ => None,
    };
    let values: Vec<u32> = text.chars().map(digit).collect::<Option<_>>()?;
    let mut total: i64 = 0;
    for (i, value) in values.iter().enumerate() {
        if values.get(i + 1).is_some_and(|next| next > value) {
            total -= i64::from(*value);
        } else {
            total += i64::from(*value);
        }
    }
    u32::try_from(total).ok().filter(|v| *v > 0)
}

#[cfg(test)]
mod tests;
