//! Title and year extraction from file and folder names.
//!
//! [`detect_clean_title_and_year`] works in these steps:
//!
//! 1. strip the file extension (`\.\w{2,4}$`), resolution tokens such as `1280x720` and the
//!    release tags that contain a delimiter themselves (`WEB-DL`, `Blu-ray`, `H.264`, `DDP5.1`,
//!    `23.976`, ...; see [`DELIMITED_TAGS`]);
//! 2. apply the user's bad words, each a regular expression matched without regard to case, to
//!    the whole name; when that would leave nothing, the name is kept as it was;
//! 3. take the `[...]` groups out as "optionals" (searched for a year later), drop `{...}`
//!    groups, and cut a TV-recording date marker (`_16.09.15_21-00_`) that starts after
//!    character 10;
//! 4. split on the delimiter set and join `VIDEO` + `TS` back to `VIDEO_TS`;
//! 5. remove edition names of one to three words, from the second token on; the first removal is
//!    a stop position;
//! 6. the first hard stop word (or IMDb ID) from the third token on is a stop position;
//! 7. the year is the last token, never the first, that is four digits with
//!    `1800 < year < current year + 5`; without one, the first valid year in the optionals;
//! 8. the title is every token before the earlier of the stop position and the year, with the
//!    Roman numerals I to X upper-cased and a trailing ` aka ...` removed.
//!
//! Soft stop words (`extended`, `proper`, `multi`, ...) are removed from the year position on.
//! Because the title ends at the year, they never change the result; the list is kept for the TV
//! helper [`remove_stop_and_bad_words_tv`].
//!
//! Bad words are applied once, to the whole name (step 2); they are not checked again per token.

use std::sync::LazyLock;
use std::time::{SystemTime, UNIX_EPOCH};

use cv_domain::Edition;
use regex::Regex;

/// The title and year found in a name. An empty title means the name held nothing usable.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CleanTitle {
    pub title: String,
    pub year: Option<u16>,
}

/// Characters that separate the words of a release name.
const DELIMITERS: &str = "[]() {}_,.-";

/// Release tags that end the title when they appear from the third token on.
pub const HARD_STOPWORDS: &[&str] = &[
    "1080p",
    "1080i",
    "2160p",
    "720p",
    "576p",
    "480p",
    "1440p",
    "4k",
    "8k",
    "3d",
    "10bit",
    "ac3",
    "amzn",
    "atmos",
    "av1",
    "bdrip",
    "bluray",
    "brrip",
    "cam",
    "cd1",
    "cd2",
    "cd3",
    "cd4",
    "cd5",
    "cd6",
    "cd7",
    "cd8",
    "cd9",
    "disc1",
    "disc2",
    "disc3",
    "disc4",
    "disc5",
    "disc6",
    "disc7",
    "disc8",
    "disc9",
    "dl",
    "dts",
    "dubbed",
    "dvd",
    "dvd1",
    "dvd2",
    "dvd3",
    "dvd4",
    "dvd5",
    "dvd6",
    "dvd7",
    "dvd8",
    "dvd9",
    "dvdrip",
    "dvdscr",
    "h264",
    "h265",
    "hd",
    "hdr",
    "hdr10",
    "hdr10+",
    "hdrip",
    "hdtv",
    "hevc",
    "ld",
    "nfo",
    "ntsc",
    "pal",
    "remastered",
    "repack",
    "rerip",
    "remux",
    "sd",
    "subbed",
    "subs",
    "svcd",
    "tc",
    "telecine",
    "telesync",
    "ts",
    "truehd",
    "uhd",
    "uncut",
    "unrated",
    "vhs",
    "webdl",
    "webrip",
    "ws",
    "x264",
    "x265",
    "xvid",
];

/// Tags that are removed from the year position on.
pub const SOFT_STOPWORDS: &[&str] = &[
    "complete", "custom", "dc", "docu", "doku", "extended", "fragment", "internal", "limited",
    "local", "ma", "multi", "pal", "proper", "read", "retail", "se", "www", "xxx",
];

/// Release tags that contain a delimiter themselves, as regular-expression alternatives.
///
/// Splitting a name on [`DELIMITERS`] would break these into pieces that mean nothing on their
/// own (`WEB-DL` into `WEB` and `DL`, `23.976` into `23` and `976`), and those pieces would end up
/// in the title of a name without a year. So they are removed whole before the split, when a
/// delimiter comes before them and no letter or digit follows. Tags without an inner delimiter
/// (`1080p`, `x264`, `WEBRip`) are handled as stop words after the split instead.
const DELIMITED_TAGS: &[&str] = &[
    // Sources written with a hyphen.
    r"web-(?:dl|rip|dlrip)",
    r"blu-ray",
    r"(?:bd|br|dvd|hd|tv|vhs)-rip",
    r"hd-(?:dvd|tv)",
    // Video codecs written with a dot.
    r"[hx]\.26[45]",
    // Audio formats with a channel layout (`DDP5.1`, `AAC2.0`, `DTS-HD.MA.7.1` keeps `MA 7 1`
    // after the year, where it does no harm).
    r"(?:dd\+?|ddp|e?ac-?3|aac|dts|flac|lpcm|opus|truehd)[1-9]\.[0-2]",
    r"dts-(?:hd|x|es)",
    // Frame rates.
    r"23\.9(?:76|8)",
    r"(?:24|25|30|50|60)\.0{2,3}",
    r"(?:29|59)\.94",
];

use cv_domain::EDITION_SPELLINGS as EDITION_NAMES;

struct Patterns {
    extension: Regex,
    resolution: Regex,
    clean_words: Regex,
    optionals: Regex,
    braces: Regex,
    otr_marker: Regex,
    imdb_id: Regex,
    aka: Regex,
    edition_tag: Regex,
}

static PATTERNS: LazyLock<Patterns> = LazyLock::new(|| {
    let re = |p: &str| Regex::new(p).expect("static title pattern");
    Patterns {
        extension: re(r"\.\w{2,4}$"),
        resolution: re(r"\d{3,4}x\d{3,4}"),
        clean_words: re(&format!(
            r"(?i){}(?:{})\b",
            delimiter_class(),
            DELIMITED_TAGS.join("|")
        )),
        optionals: re(r"\[([^\]]*)\]"),
        braces: re(r"\{[^}]*\}"),
        otr_marker: re(r"_\d{2}\.\d{2}\.\d{2}[_ ]+\d{2}\-\d{2}\_"),
        imdb_id: re(r"^(?i:tt)\d{6,}$"),
        aka: re(r"(?i)\s+aka\s+.*$"),
        edition_tag: re(r"(?i)\{edition-([^}]+)\}"),
    }
});

/// Finds the title and year of a file name, using the current year as the upper limit.
///
/// `bad_words` are the user's bad words: lower-case regular expressions (one that is not a valid
/// expression is matched literally). See the module documentation for the algorithm.
pub fn detect_clean_title_and_year<S: AsRef<str>>(name: &str, bad_words: &[S]) -> CleanTitle {
    detect_clean_title_and_year_at(name, bad_words, current_year())
}

/// Like [`detect_clean_title_and_year`] with the current year given, so tests do not depend on
/// the clock. A year is plausible when `1800 < year < current_year + 5`.
pub fn detect_clean_title_and_year_at<S: AsRef<str>>(
    name: &str,
    bad_words: &[S],
    current_year: u16,
) -> CleanTitle {
    detect(name, bad_words, current_year, true)
}

/// For a folder name. A folder has no file extension, so `Heat.1995` keeps its year instead of
/// losing `.1995` as an "extension". Everything else is as for a file name.
pub fn detect_clean_title_and_year_of_folder<S: AsRef<str>>(
    name: &str,
    bad_words: &[S],
) -> CleanTitle {
    detect(name, bad_words, current_year(), false)
}

fn detect<S: AsRef<str>>(
    name: &str,
    bad_words: &[S],
    current_year: u16,
    strip_extension: bool,
) -> CleanTitle {
    let p = &*PATTERNS;
    if name.trim().is_empty() {
        return CleanTitle::default();
    }

    // 1. extension, resolution, clean words
    let mut work = name.to_owned();
    if strip_extension {
        work = p.extension.replace(&work, "").into_owned();
    }
    work = p.resolution.replace_all(&work, "").into_owned();
    work = p.clean_words.replace_all(&work, "").into_owned();

    // 2. bad words on the whole name
    let cleaned = remove_bad_words(&work, bad_words);
    if !cleaned.trim().is_empty() {
        work = cleaned;
    }

    // 3. optionals, braces, TV-recording marker
    let optionals: Vec<String> = p
        .optionals
        .captures_iter(&work)
        .map(|c| c[1].to_owned())
        .collect();
    work = p.optionals.replace_all(&work, " ").into_owned();
    work = p.braces.replace_all(&work, " ").into_owned();
    if let Some(m) = p.otr_marker.find(&work)
        && work[..m.start()].chars().count() > 10
    {
        work.truncate(m.start());
    }

    // 4. tokens
    let mut tokens = join_video_ts(split_tokens(&work));
    if tokens.is_empty() {
        return CleanTitle::default();
    }

    // 5. editions, from the second token on
    let mut stop = usize::MAX;
    let mut i = 1;
    while i < tokens.len() {
        if let Some((_, len)) = edition_match_at(&tokens, i) {
            tokens.drain(i..i + len);
            stop = stop.min(i);
        } else {
            i += 1;
        }
    }

    // 6. hard stop words and IMDb IDs, from the third token on
    let hard_stop =
        (2..tokens.len()).find(|&i| is_hard_stopword(&tokens[i]) || p.imdb_id.is_match(&tokens[i]));
    if let Some(at) = hard_stop {
        stop = stop.min(at);
    }

    // 7. year
    let year_at = (1..tokens.len())
        .rev()
        .find(|&i| plausible_year(&tokens[i], current_year).is_some());
    let year = year_at
        .and_then(|i| plausible_year(&tokens[i], current_year))
        .or_else(|| {
            optionals
                .iter()
                .flat_map(|o| split_tokens(o))
                .find_map(|t| plausible_year(&t, current_year))
        });

    // 8. title
    let end = stop.min(year_at.unwrap_or(usize::MAX)).min(tokens.len());
    let title = tokens[..end]
        .iter()
        .map(|t| roman_numeral(t).unwrap_or_else(|| t.clone()))
        .collect::<Vec<_>>()
        .join(" ");
    let title = p.aka.replace(&title, "").trim().to_owned();
    CleanTitle { title, year }
}

/// The edition named in a name: an explicit `{edition-XYZ}` tag, otherwise the first edition
/// name (`Director's Cut`, `Extended Edition`, `IMAX`, ...) found among the words. Names that
/// are not built in are kept as free text; no edition gives [`Edition::None`].
pub fn detect_edition(name: &str) -> Edition {
    if let Some(c) = PATTERNS.edition_tag.captures(name)
        && let Some(edition) = Edition::parse(&c[1])
    {
        return edition;
    }
    let stripped = PATTERNS.braces.replace_all(name, " ");
    let tokens = split_tokens(&stripped);
    (0..tokens.len())
        .find_map(|i| edition_match_at(&tokens, i).map(|(edition, _)| edition))
        .unwrap_or(Edition::None)
}

/// TV names: hard stop words, soft stop words and bad words are removed only when they stand
/// between delimiters, so a word inside another word is never cut. `Show.Name.hdtv.x264`
/// loses `hdtv` and `x264`; `Hdtvshow` is left alone.
pub fn remove_stop_and_bad_words_tv<S: AsRef<str>>(name: &str, bad_words: &[S]) -> String {
    let mut out = name.to_owned();
    let words = HARD_STOPWORDS
        .iter()
        .chain(SOFT_STOPWORDS)
        .map(|w| regex::escape(w))
        .chain(
            bad_words
                .iter()
                .map(AsRef::as_ref)
                .filter(|w| !w.is_empty())
                .map(bad_word_pattern),
        );
    for word in words {
        let Ok(re) = Regex::new(&format!(
            r"(?i)([\[\](){{}} _,.-])(?:{word})([\[\](){{}} _,.-]|$)"
        )) else {
            continue;
        };
        // Two stop words in a row share one delimiter, so repeat until nothing changes.
        for _ in 0..8 {
            let next = re.replace_all(&out, "$1").into_owned();
            if next == out {
                break;
            }
            out = next;
        }
    }
    out
}

/// A bad word is a regular expression; one that does not compile is matched literally.
fn bad_word_pattern(word: &str) -> String {
    if Regex::new(&format!("(?i){word}")).is_ok() {
        word.to_owned()
    } else {
        regex::escape(word)
    }
}

fn remove_bad_words<S: AsRef<str>>(text: &str, bad_words: &[S]) -> String {
    let mut out = text.to_owned();
    for word in bad_words
        .iter()
        .map(AsRef::as_ref)
        .filter(|w| !w.is_empty())
    {
        if let Ok(re) = Regex::new(&format!("(?i){}", bad_word_pattern(word))) {
            out = re.replace_all(&out, "").into_owned();
        }
    }
    out
}

/// A regular-expression character class matching any one of [`DELIMITERS`].
fn delimiter_class() -> String {
    let chars: String = DELIMITERS
        .chars()
        .map(|c| regex::escape(&c.to_string()))
        .collect();
    format!("[{chars}]")
}

fn split_tokens(text: &str) -> Vec<String> {
    text.split(|c: char| DELIMITERS.contains(c))
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
        .collect()
}

/// `VIDEO_TS` is a folder name, not two words: join the pieces the `_` delimiter separated.
fn join_video_ts(tokens: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(tokens.len());
    for token in tokens {
        match out.last_mut() {
            Some(last)
                if last.eq_ignore_ascii_case("video") && token.eq_ignore_ascii_case("ts") =>
            {
                last.push('_');
                last.push_str(&token);
            }
            _ => out.push(token),
        }
    }
    out
}

fn is_hard_stopword(token: &str) -> bool {
    let lower = token.to_lowercase();
    HARD_STOPWORDS.contains(&lower.as_str())
}

fn plausible_year(token: &str, current_year: u16) -> Option<u16> {
    if token.len() != 4 || !token.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let year: u16 = token.parse().ok()?;
    (year > 1800 && year < current_year.saturating_add(5)).then_some(year)
}

fn roman_numeral(token: &str) -> Option<String> {
    const NUMERALS: [&str; 10] = ["i", "ii", "iii", "iv", "v", "vi", "vii", "viii", "ix", "x"];
    let lower = token.to_lowercase();
    NUMERALS
        .contains(&lower.as_str())
        .then(|| token.to_uppercase())
}

fn edition_token(token: &str) -> String {
    token
        .chars()
        .filter(|c| !matches!(c, '\'' | '\u{2019}' | '\u{b4}' | '`'))
        .flat_map(char::to_lowercase)
        .collect()
}

/// The edition whose name starts at `at`, with the number of tokens it spans. The longest name
/// wins.
fn edition_match_at(tokens: &[String], at: usize) -> Option<(Edition, usize)> {
    let window: Vec<String> = tokens[at..]
        .iter()
        .take(3)
        .map(|t| edition_token(t))
        .collect();
    (1..=window.len()).rev().find_map(|len| {
        let words = window[..len].join(" ");
        EDITION_NAMES
            .iter()
            .find(|(_, name)| *name == words)
            .and_then(|(key, _)| Edition::parse(key))
            .map(|edition| (edition, len))
    })
}

/// The calendar year of the system clock (UTC), 1970 when the clock is unreadable.
fn current_year() -> u16 {
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() / 86_400);
    // Civil-from-days (proleptic Gregorian); only the year is needed.
    let z = i64::try_from(days).unwrap_or(0) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let year = yoe + era * 400 + i64::from(mp >= 10);
    u16::try_from(year).unwrap_or(1970)
}
