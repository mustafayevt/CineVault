//! Tolerant NFO readers: what the movie, set, show, season and episode readers share.
//!
//! NFOs come from Kodi, Jellyfin, Emby, MediaPortal, NextPVR and many other tools, and people
//! edit them by hand. The readers therefore never fail on a file:
//!
//! * The XML is read with the forgiving tree reader ([`crate::tree`]): unclosed tags, stray
//!   closing tags and truncated files keep everything read so far. A tag that was never closed
//!   swallows the tags after it; the readers lift those back out, so one bad
//!   element costs only its own value.
//! * Element and attribute names are compared without regard to case, and a few alternative
//!   spellings are folded together (`original_title`, `imdb_id`, `plotoutline`, ...).
//! * Every value is read on its own. A value that does not parse (a rating of `n/a`, a date of
//!   `unknown`) is skipped and the rest of the file is still read.
//! * Provider IDs come from `<uniqueid>`, the older `<id>`, `<imdbid>`, `<tmdbid>`, `<tvdbid>`
//!   elements and the JAXB `<ids><entry>` map. IDs still missing afterwards are searched in the
//!   raw text (URLs and `tt` numbers), so an NFO that is only a URL or an ID still yields its
//!   ID, see [`detect_movie_ids`] and [`detect_show_ids`].
//!
//! The readers fill the same plain models the writers use ([`MovieNfo`](crate::MovieNfo) and
//! friends). What the readers map is documented on each reader. A model read from an NFO is
//! "valid" (CineVault manages the file) when it has a title; episodes are valid with a title or a
//! number. Elements the readers do not know are not part of the models; a rewrite keeps them
//! through [`crate::preserve`].

use std::sync::LazyLock;

use cv_domain::{
    CalendarDate, Credit, CreditKind, Person, ProviderIds, Rating, Ratings, Trailer, id_key,
    rating_key,
};
use regex::Regex;

use crate::kodi_movie::youtube_id;
use crate::locale::parse_genre;
use crate::xml::{Element, Node};

// ---- bytes to text ------------------------------------------------------------------------

/// Decodes the bytes of an NFO file. A UTF-8 or UTF-16 byte-order mark is honoured and removed;
/// UTF-16 without a mark is not detected. Bytes that are not valid UTF-8 are read as Windows-1252
/// (the encoding old NFO files without a declaration usually have).
pub fn decode_nfo_bytes(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    for (mark, little_endian) in [([0xFF, 0xFE], true), ([0xFE, 0xFF], false)] {
        if let Some(rest) = bytes.strip_prefix(&mark) {
            let units = rest.as_chunks::<2>().0.iter().map(|pair| {
                if little_endian {
                    u16::from_le_bytes([pair[0], pair[1]])
                } else {
                    u16::from_be_bytes([pair[0], pair[1]])
                }
            });
            return char::decode_utf16(units)
                .map(|c| c.unwrap_or('\u{fffd}'))
                .collect();
        }
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => bytes.iter().map(|b| windows_1252(*b)).collect(),
    }
}

fn windows_1252(byte: u8) -> char {
    const HIGH: [char; 32] = [
        '\u{20ac}', '\u{81}', '\u{201a}', '\u{192}', '\u{201e}', '\u{2026}', '\u{2020}',
        '\u{2021}', '\u{2c6}', '\u{2030}', '\u{160}', '\u{2039}', '\u{152}', '\u{8d}', '\u{17d}',
        '\u{8f}', '\u{90}', '\u{2018}', '\u{2019}', '\u{201c}', '\u{201d}', '\u{2022}', '\u{2013}',
        '\u{2014}', '\u{2dc}', '\u{2122}', '\u{161}', '\u{203a}', '\u{153}', '\u{9d}', '\u{17e}',
        '\u{178}',
    ];
    match byte {
        0x80..=0x9f => HIGH[usize::from(byte - 0x80)],
        other => char::from(other),
    }
}

// ---- raw-text ID fallback -----------------------------------------------------------------

/// IDs found in the raw text of an NFO.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DetectedIds {
    /// IMDb ID with the `tt` prefix.
    pub imdb: Option<String>,
    pub tmdb: Option<u64>,
    /// TVDB series ID (shows only).
    pub tvdb: Option<u64>,
}

struct Patterns {
    imdb: Regex,
    imdb_legacy: Regex,
    tmdb_movie: Regex,
    tmdb_collection: Regex,
    tmdb_tv: Vec<Regex>,
    tvdb: Vec<Regex>,
}

static PATTERNS: LazyLock<Patterns> = LazyLock::new(|| {
    let re = |p: &str| Regex::new(p).expect("static NFO pattern");
    Patterns {
        imdb: re(r"tt\d{6,}"),
        imdb_legacy: re(r"(?i)imdb\.com/title\?(\d{6,})"),
        tmdb_movie: re(r"(?i)themoviedb\.org/movie/(\d+)"),
        tmdb_collection: re(r"(?i)themoviedb\.org/collection/(\d+)"),
        tmdb_tv: vec![
            re(r"(?i)themoviedb\.org/tv/(\d+)"),
            re(r"(?i)api\.themoviedb\.org/3/tv/(\d+)"),
            re(r#"(?i)"tmdb"\s*:\s*"?(\d+)"#),
        ],
        tvdb: vec![
            re(r"(?i)thetvdb\.com/series/(\d+)"),
            re(r"(?i)thetvdb\.com/\?tab=series&(?:amp;)?id=(\d+)"),
            re(r"(?i)thetvdb\.com/api/[^/\s]+/series/(\d+)"),
            re(r#"(?i)"tvdb"\s*:\s*"?(\d+)"#),
        ],
    }
});

fn first_number(patterns: &[&Regex], text: &str) -> Option<u64> {
    patterns.iter().find_map(|p| {
        p.captures(text)
            .and_then(|c| c[1].parse::<u64>().ok())
            .filter(|n| *n > 0)
    })
}

fn detect_imdb(text: &str) -> Option<String> {
    let p = &*PATTERNS;
    p.imdb
        .find(text)
        .map(|m| m.as_str().to_owned())
        .or_else(|| p.imdb_legacy.captures(text).map(|c| format!("tt{}", &c[1])))
}

/// Finds a movie's IDs in raw text: an IMDb `tt` number and a `themoviedb.org/movie/<id>` URL.
pub fn detect_movie_ids(text: &str) -> DetectedIds {
    let p = &*PATTERNS;
    DetectedIds {
        imdb: detect_imdb(text),
        tmdb: first_number(&[&p.tmdb_movie], text),
        tvdb: None,
    }
}

/// Finds the TMDB collection ID of a `themoviedb.org/collection/<id>` URL in raw text.
pub fn detect_collection_id(text: &str) -> Option<u64> {
    first_number(&[&PATTERNS.tmdb_collection], text)
}

/// Finds a show's IDs in raw text: an IMDb `tt` number, a `themoviedb.org/tv/<id>` URL and a
/// `thetvdb.com/series/<id>` URL (also the older query-string form and the JSON `episodeguide`).
pub fn detect_show_ids(text: &str) -> DetectedIds {
    let p = &*PATTERNS;
    let tmdb: Vec<&Regex> = p.tmdb_tv.iter().collect();
    let tvdb: Vec<&Regex> = p.tvdb.iter().collect();
    DetectedIds {
        imdb: detect_imdb(text),
        tmdb: first_number(&tmdb, text),
        tvdb: first_number(&tvdb, text),
    }
}

/// Adds the IDs of `found` that `ids` lacks.
pub(crate) fn merge_detected(ids: &mut ProviderIds, found: &DetectedIds) {
    if !ids.contains(id_key::IMDB)
        && let Some(imdb) = &found.imdb
    {
        ids.set(id_key::IMDB, imdb);
    }
    if !ids.contains(id_key::TMDB)
        && let Some(tmdb) = found.tmdb
    {
        ids.set(id_key::TMDB, tmdb.to_string());
    }
    if !ids.contains(id_key::TVDB)
        && let Some(tvdb) = found.tvdb
    {
        ids.set(id_key::TVDB, tvdb.to_string());
    }
}

// ---- the element tree ---------------------------------------------------------------------

/// Elements that hold a single value. One of them with child elements was left open in the file.
const LEAF: &[&str] = &[
    "title",
    "originaltitle",
    "sorttitle",
    "showtitle",
    "year",
    "plot",
    "outline",
    "tagline",
    "runtime",
    "mpaa",
    "certification",
    "premiered",
    "releasedate",
    "aired",
    "genre",
    "country",
    "studio",
    "tag",
    "director",
    "credits",
    "trailer",
    "imdbid",
    "tmdbid",
    "tvdbid",
    "id",
    "playcount",
    "lastplayed",
    "watched",
    "top250",
    "votes",
    "userrating",
    "criticrating",
    "status",
    "dateadded",
    "lockdata",
    "season",
    "seasonnumber",
    "episode",
    "displayseason",
    "displayepisode",
    "epbookmark",
    "code",
    "source",
    "edition",
    "original_filename",
    "user_note",
    "english_title",
    "crc32",
    "collectionnumber",
    "languages",
    "language",
    "showlink",
    "description",
    "subtitle",
    "overview",
    "videoassettitle",
];

const ALIASES: &[(&str, &str)] = &[
    ("original_title", "originaltitle"),
    ("sort_title", "sorttitle"),
    ("imdb_id", "imdbid"),
    ("tmdb_id", "tmdbid"),
    ("tvdb_id", "tvdbid"),
    ("plotoutline", "outline"),
    ("release_date", "releasedate"),
    ("last_played", "lastplayed"),
    ("date_added", "dateadded"),
    ("play_count", "playcount"),
    ("contentrating", "mpaa"),
    ("collection_number", "collectionnumber"),
    ("desc", "description"),
];

fn canonical_name(name: &str) -> String {
    let lower = name.trim().to_ascii_lowercase();
    ALIASES
        .iter()
        .find(|(from, _)| *from == lower)
        .map_or(lower, |(_, to)| (*to).to_owned())
}

fn has_elements(element: &Element) -> bool {
    element
        .children
        .iter()
        .any(|n| matches!(n, Node::Element(_)))
}

/// Moves the elements an open value tag swallowed back next to it.
fn push_lifted(out: &mut Element, mut element: Element) {
    if LEAF.contains(&element.name.as_str()) && has_elements(&element) {
        let children = std::mem::take(&mut element.children);
        out.push(element);
        for node in children {
            if let Node::Element(child) = node {
                push_lifted(out, child);
            }
        }
    } else {
        out.push(element);
    }
}

fn lowered(element: &Element) -> Element {
    Element {
        name: canonical_name(&element.name),
        attrs: element
            .attrs
            .iter()
            .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.clone()))
            .collect(),
        text: element.text.clone(),
        children: element
            .children
            .iter()
            .filter_map(|node| match node {
                Node::Element(child) => Some(Node::Element(lowered(child))),
                Node::Comment(_) => None,
            })
            .collect(),
    }
}

/// A copy of a root element that is easy to read: names and attribute names in lower case,
/// alternative spellings folded, comments dropped, and the children of open value tags lifted to
/// the root's own level.
pub(crate) fn prepare(root: &Element) -> Element {
    let source = lowered(root);
    let mut out = Element {
        children: Vec::new(),
        ..source.clone()
    };
    for node in source.children {
        if let Node::Element(child) = node {
            push_lifted(&mut out, child);
        }
    }
    out
}

/// The first root called one of `names` (compared without case), prepared for reading.
pub(crate) fn find_root(roots: &[Element], names: &[&str]) -> Option<Element> {
    roots
        .iter()
        .find(|r| names.iter().any(|n| r.name.trim().eq_ignore_ascii_case(n)))
        .map(prepare)
}

/// The direct child elements called `name`.
pub(crate) fn kids<'a>(element: &'a Element, name: &str) -> impl Iterator<Item = &'a Element> {
    element.children.iter().filter_map(move |n| match n {
        Node::Element(e) if e.name == name => Some(e),
        _ => None,
    })
}

/// The children called `name`, also those inside a plural container (`<genres><genre>`).
pub(crate) fn each<'a>(element: &'a Element, name: &str) -> Vec<&'a Element> {
    let plural = format!("{name}s");
    let mut found = Vec::new();
    for node in &element.children {
        let Node::Element(child) = node else { continue };
        if child.name == name {
            found.push(child);
        } else if child.name == plural {
            found.extend(kids(child, name));
        }
    }
    found
}

/// The trimmed text of the first non-empty child called `name`; empty when there is none.
pub(crate) fn text<'a>(element: &'a Element, name: &str) -> &'a str {
    kids(element, name)
        .map(|e| e.text.trim())
        .find(|t| !t.is_empty())
        .unwrap_or_default()
}

/// The first non-empty text among several names.
pub(crate) fn text_of_any<'a>(element: &'a Element, names: &[&str]) -> &'a str {
    names
        .iter()
        .map(|n| text(element, n))
        .find(|t| !t.is_empty())
        .unwrap_or_default()
}

/// The trimmed, non-empty texts of all children called `name` (also inside a plural container).
pub(crate) fn texts(element: &Element, name: &str) -> Vec<String> {
    each(element, name)
        .into_iter()
        .map(|e| e.text.trim().to_owned())
        .filter(|t| !t.is_empty())
        .collect()
}

// ---- values -------------------------------------------------------------------------------

static NUMBER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"-?\d+(?:[.,]\d+)?").unwrap());
static DIGITS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d+").unwrap());
static YEAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d{4}").unwrap());
static ISO_DATE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\d{4})\D(\d{1,2})\D(\d{1,2})").unwrap());
static DOTTED_DATE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\d{1,2})\.(\d{1,2})\.(\d{4})").unwrap());
static TIMESTAMP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(\d{4})-(\d{2})-(\d{2})(?:[T ](\d{2}):(\d{2})(?::(\d{2}))?)?").unwrap()
});
static HOURS_MINUTES: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(\d+)\s*h\w*(?:\s*(\d+)\s*m\w*)?$").unwrap());
static IMDB_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"tt\d{6,}").unwrap());

/// The first four-digit run, as a year. `0` when there is none.
pub(crate) fn parse_year(text: &str) -> u32 {
    YEAR.find(text)
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(0)
}

/// A whole number (`12`, `1,234`); `0` when there is none.
pub(crate) fn parse_count(text: &str) -> u32 {
    let digits: String = text
        .trim()
        .split('.')
        .next()
        .unwrap_or_default()
        .chars()
        .filter(char::is_ascii_digit)
        .collect();
    digits.parse().unwrap_or(0)
}

/// A vote count written with any thousands separator (`1,234`, `1.234`, `1 234`).
pub(crate) fn parse_votes(text: &str) -> u32 {
    let digits: String = text.chars().filter(char::is_ascii_digit).collect();
    digits.parse().unwrap_or(0)
}

/// A decimal number with a point or comma, ignoring text around it (`8.5/10`).
pub(crate) fn parse_float(text: &str) -> Option<f32> {
    let found = NUMBER.find(text)?.as_str().replace(',', ".");
    found.parse().ok()
}

/// The runtime in minutes from `120`, `120 min`, `1h 58m` or `1:58:00`. `0` when unknown.
pub(crate) fn parse_minutes(text: &str) -> u32 {
    let text = text.trim();
    if text.contains(':') {
        let parts: Vec<u32> = text
            .split(':')
            .map(|p| p.trim().parse().unwrap_or(0))
            .collect();
        return match parts.as_slice() {
            [h, m] | [h, m, _] => h * 60 + m,
            _ => 0,
        };
    }
    if let Some(c) = HOURS_MINUTES.captures(text) {
        let part = |i| {
            c.get(i)
                .and_then(|m| m.as_str().parse::<u32>().ok())
                .unwrap_or(0)
        };
        return part(1) * 60 + part(2);
    }
    DIGITS
        .find(text)
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(0)
}

pub(crate) fn parse_bool(text: &str) -> bool {
    matches!(
        text.trim().to_ascii_lowercase().as_str(),
        "true" | "1" | "yes" | "y"
    )
}

/// A date from `2024-03-01`, `2024/3/1`, `2024-03-01T10:00:00Z` or `01.03.2024`.
pub(crate) fn parse_date(text: &str) -> Option<CalendarDate> {
    let text = text.trim();
    if let Some(c) = ISO_DATE.captures(text) {
        return CalendarDate::new(c[1].parse().ok()?, c[2].parse().ok()?, c[3].parse().ok()?);
    }
    let c = DOTTED_DATE.captures(text)?;
    CalendarDate::new(c[3].parse().ok()?, c[2].parse().ok()?, c[1].parse().ok()?)
}

/// A time stamp as `yyyy-MM-dd HH:mm:ss` (a date alone gets midnight); `None` for other text.
pub(crate) fn parse_timestamp(text: &str) -> Option<String> {
    let c = TIMESTAMP.captures(text.trim())?;
    let part = |i: usize| c.get(i).map_or("00", |m| m.as_str());
    Some(format!(
        "{}-{}-{} {}:{}:{}",
        &c[1],
        &c[2],
        &c[3],
        part(4),
        part(5),
        part(6)
    ))
}

pub(crate) fn is_http(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

/// Splits a genre list written as `Action, Drama`, `Action/Drama`, `Action;Drama` or
/// `Action|Drama`.
pub(crate) fn split_list(text: &str) -> Vec<String> {
    text.split([';', ',', '/', '|'])
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect()
}

pub(crate) fn read_genres(root: &Element) -> Vec<cv_domain::Genre> {
    let mut genres = Vec::new();
    for value in texts(root, "genre") {
        for part in split_list(&value) {
            if let Some(genre) = parse_genre(&part)
                && !genres.contains(&genre)
            {
                genres.push(genre);
            }
        }
    }
    genres
}

/// Tag-per-value lists (studio, country): one entry per tag; a single tag holding several
/// values joined by `,` or `/` (the MediaPortal way) is split. Also returns the text as written.
pub(crate) fn read_list(root: &Element, name: &str) -> (Vec<String>, String) {
    let values = texts(root, name);
    let raw = values.join(", ");
    if let [single] = values.as_slice() {
        let parts = crate::tv::split_show_list(single);
        if !parts.is_empty() {
            return (parts, raw);
        }
    }
    (values, raw)
}

// ---- IDs ----------------------------------------------------------------------------------

/// The IMDb ID in a text (`tt0103639`, also inside a URL).
pub(crate) fn imdb_in(text: &str) -> Option<&str> {
    IMDB_ID.find(text).map(|m| m.as_str())
}

fn digits_in(text: &str) -> Option<&str> {
    DIGITS.find(text).map(|m| m.as_str())
}

/// The ID key for a `type` / `key` / rating name.
pub(crate) fn id_name(name: &str) -> String {
    let lower = name.trim().to_ascii_lowercase();
    match lower.as_str() {
        "themoviedb" | "tmdb" | "moviedb" => id_key::TMDB.to_owned(),
        "thetvdb" | "tvdb" => id_key::TVDB.to_owned(),
        "tmdbset" | "tmdbcolid" | "collection" => id_key::TMDB_SET.to_owned(),
        _ => lower,
    }
}

fn put_id(ids: &mut ProviderIds, key: &str, value: &str) {
    if ids.contains(key) {
        return;
    }
    let value = value.trim();
    match key {
        id_key::IMDB => {
            if let Some(imdb) = imdb_in(value) {
                ids.set(key, imdb);
            }
        }
        id_key::TMDB | id_key::TVDB | id_key::TMDB_SET => {
            if let Some(digits) = digits_in(value) {
                ids.set(key, digits);
            }
        }
        _ => {
            ids.set(key, value);
        }
    }
}

/// Reads the provider IDs of a movie, show or episode element. `tv` makes a numeric `<id>` a TVDB
/// ID; in a movie `<id>` counts only when it looks like an IMDb ID.
pub(crate) fn read_ids(root: &Element, tv: bool) -> ProviderIds {
    let mut ids = ProviderIds::new();
    for unique in each(root, "uniqueid") {
        let value = unique.text.trim();
        match unique.attribute("type").map(id_name) {
            Some(key) if !key.is_empty() => put_id(&mut ids, &key, value),
            _ => put_id(&mut ids, id_key::IMDB, value),
        }
    }
    for map in kids(root, "ids") {
        for entry in kids(map, "entry") {
            let key = id_name(text(entry, "key"));
            if !key.is_empty() {
                put_id(&mut ids, &key, text(entry, "value"));
            }
        }
    }
    for name in ["imdbid", "imdb"] {
        put_id(&mut ids, id_key::IMDB, text(root, name));
    }
    put_id(&mut ids, id_key::TMDB, text(root, "tmdbid"));
    put_id(&mut ids, id_key::TMDB, text(root, "tmdb"));
    put_id(&mut ids, id_key::TVDB, text(root, "tvdbid"));
    put_id(&mut ids, id_key::TMDB_SET, text(root, "collectionnumber"));
    put_id(&mut ids, id_key::TMDB_SET, text(root, "tmdbcolid"));
    let plain = text(root, "id");
    if imdb_in(plain).is_some() {
        put_id(&mut ids, id_key::IMDB, plain);
    } else if tv && plain.bytes().all(|b| b.is_ascii_digit()) {
        put_id(&mut ids, id_key::TVDB, plain);
    }
    ids
}

// ---- ratings ------------------------------------------------------------------------------

/// The rating source key for a rating `name` attribute.
fn rating_source(name: &str) -> String {
    let lower = name.trim().to_ascii_lowercase();
    match lower.as_str() {
        "themoviedb" | "tmdb" => id_key::TMDB.to_owned(),
        "rottentomatoes" => "tomatometerallcritics".to_owned(),
        "metascore" => "metacritic".to_owned(),
        "nfo" => rating_key::NFO.to_owned(),
        "" => rating_key::NFO.to_owned(),
        _ => lower,
    }
}

fn rating_of(id: &str, value: &str, votes: &str, max: u32) -> Option<Rating> {
    let value = parse_float(value)?;
    Some(Rating::new(id, value, parse_votes(votes), max))
}

/// Reads `<ratings><rating name max><value/><votes/></rating></ratings>`, or else the old
/// `<rating>` and `<votes>` pair (stored under the `NFO` key), plus `<userrating>` (user, out of
/// 10; `0` means none) and `<criticrating>` (`critic`, out of 100).
pub(crate) fn read_ratings(root: &Element) -> Ratings {
    let mut ratings = Ratings::new();
    for container in kids(root, "ratings") {
        for entry in kids(container, "rating") {
            let max = entry
                .attribute("max")
                .and_then(parse_float)
                .map(|m| m as u32)
                .filter(|m| *m > 0)
                .unwrap_or(10);
            let value = match text(entry, "value") {
                "" => entry.text.trim(),
                value => value,
            };
            let id = rating_source(entry.attribute("name").unwrap_or_default());
            if let Some(rating) = rating_of(&id, value, text(entry, "votes"), max) {
                ratings.insert(rating);
            }
        }
    }
    if ratings.is_empty()
        && let Some(old) = kids(root, "rating").find(|r| !has_elements(r))
        && let Some(rating) = rating_of(rating_key::NFO, &old.text, text(root, "votes"), 10)
    {
        ratings.insert(rating);
    }
    if let Some(user) = parse_float(text(root, "userrating"))
        && user > 0.0
    {
        ratings.insert(Rating::new(rating_key::USER, user, 0, 10));
    }
    if let Some(critic) = parse_float(text(root, "criticrating"))
        && critic > 0.0
    {
        ratings.insert(Rating::new("critic", critic, 0, 100));
    }
    ratings
}

// ---- people -------------------------------------------------------------------------------

fn person_of(element: &Element) -> Person {
    let name = match text(element, "name") {
        "" => element.text.trim(),
        name => name,
    };
    let mut person = Person::new(name);
    for (attr, key) in [
        ("tmdbid", id_key::TMDB),
        ("imdbid", id_key::IMDB),
        ("tvdbid", id_key::TVDB),
    ] {
        let value = element
            .attribute(attr)
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| text(element, attr));
        put_id(&mut person.ids, key, value);
    }
    let thumb = text(element, "thumb");
    if is_http(thumb) {
        person.thumb_url = thumb.to_owned();
    }
    let profile = text(element, "profile");
    if is_http(profile) {
        person.profile_url = profile.to_owned();
    }
    person
}

fn credit_of(element: &Element, kind: CreditKind) -> Option<Credit> {
    let person = person_of(element);
    (!person.name.trim().is_empty()).then(|| Credit {
        kind,
        person,
        role: text(element, "role").to_owned(),
    })
}

/// Cast and crew: actors (guest stars when their `<type>` says so), directors, writers
/// (`<credits>`), producers, and the other crew of CineVault's own `<crew>` block.
pub(crate) fn read_credits(root: &Element) -> Vec<Credit> {
    let mut credits = Vec::new();
    for actor in each(root, "actor") {
        let guest = text(actor, "type").to_ascii_lowercase().contains("guest");
        let kind = if guest {
            CreditKind::Guest
        } else {
            CreditKind::Actor
        };
        credits.extend(credit_of(actor, kind));
    }
    for (name, kind) in [
        ("director", CreditKind::Director),
        ("credits", CreditKind::Writer),
        ("producer", CreditKind::Producer),
    ] {
        for element in each(root, name) {
            credits.extend(credit_of(element, kind));
        }
    }
    for crew in kids(root, "crew") {
        let Some(role) = kids(crew, "role").next() else {
            continue;
        };
        let kind = match role.text.trim().to_ascii_uppercase().as_str() {
            "COMPOSER" => CreditKind::Composer,
            "EDITOR" => CreditKind::Editor,
            "CAMERA" => CreditKind::Camera,
            "OTHER" => CreditKind::Other,
            "DIRECTOR" => CreditKind::Director,
            "WRITER" => CreditKind::Writer,
            "PRODUCER" => CreditKind::Producer,
            // Actors are listed in their own elements.
            _ => continue,
        };
        let Some(mut credit) = credit_of(crew, kind) else {
            continue;
        };
        credit.role = role.attribute("subrole").unwrap_or_default().to_owned();
        // Directors, writers and producers have their own elements too, which carry no job
        // title (`<director>`, `<credits>`) or no picture: the crew entry completes them.
        let listed = credits.iter_mut().find(|c| {
            c.kind == kind
                && c.person
                    .name
                    .trim()
                    .eq_ignore_ascii_case(credit.person.name.trim())
        });
        match listed {
            Some(listed) => complete_credit(listed, credit),
            None => credits.push(credit),
        }
    }
    // A person listed twice (as a writer for the story and again for the screenplay) is one.
    cv_domain::merge_crew(&credits)
}

/// Fills what `listed` lacks (the job title, the pictures and the IDs) from `other`, the same
/// person's crew entry.
fn complete_credit(listed: &mut Credit, other: Credit) {
    if listed.role.trim().is_empty() {
        listed.role = other.role;
    }
    let (have, more) = (&mut listed.person, other.person);
    if have.thumb_url.trim().is_empty() {
        have.thumb_url = more.thumb_url;
    }
    if have.profile_url.trim().is_empty() {
        have.profile_url = more.profile_url;
    }
    for (key, value) in more.ids.iter() {
        if !have.ids.contains(key) {
            have.ids.set(key, value);
        }
    }
}

// ---- trailer ------------------------------------------------------------------------------

fn query_value<'a>(query: &'a str, keys: &[&str]) -> Option<&'a str> {
    query.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        keys.iter()
            .any(|k| key.eq_ignore_ascii_case(k))
            .then_some(value)
            .filter(|v| !v.is_empty())
    })
}

/// A trailer from the text of `<trailer>`. The YouTube plug-in URLs Kodi uses become YouTube
/// watch links; the HD-Trailers plug-in URL (`.../video/<site>/<url-encoded link>`) becomes the
/// link it wraps; any other text is kept.
pub(crate) fn read_trailer(raw: &str) -> Option<Trailer> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let lower = raw.to_ascii_lowercase();
    let (url, provider) = if lower.starts_with("plugin://plugin.video.youtube") {
        let query = raw.split_once('?').map_or("", |(_, q)| q);
        match query_value(query, &["video_id", "videoid"]) {
            Some(id) => (format!("https://www.youtube.com/watch?v={id}"), "youtube"),
            None => (raw.to_owned(), ""),
        }
    } else if lower.starts_with(HDTRAILERS_PLUGIN) {
        match hdtrailers_link(&raw[HDTRAILERS_PLUGIN.len()..]) {
            Some(link) => {
                let provider = if youtube_id(&link).is_some() {
                    "youtube"
                } else if link.contains("hd-trailers.net") {
                    "hdtrailers"
                } else {
                    ""
                };
                (link, provider)
            }
            None => (raw.to_owned(), ""),
        }
    } else {
        (raw.to_owned(), "")
    };
    Some(Trailer {
        url,
        provider: provider.to_owned(),
        in_nfo: true,
        ..Trailer::default()
    })
}

const HDTRAILERS_PLUGIN: &str = "plugin://plugin.video.hdtrailers_net/video/";

/// The web link inside an HD-Trailers plug-in URL, given the text after `.../video/`: either
/// `<site>/<encoded link>` or `<anything>?/<encoded link>`. `None` when no http(s) link decodes.
fn hdtrailers_link(rest: &str) -> Option<String> {
    let after_site = rest.split_once('/').map(|(_, tail)| tail);
    let after_query = rest.split_once("?/").map(|(_, tail)| tail);
    [after_site, after_query]
        .into_iter()
        .flatten()
        .map(form_decode)
        .find(|link| {
            let lower = link.to_ascii_lowercase();
            lower.starts_with("http://") || lower.starts_with("https://")
        })
}

/// Form-style URL decoding: `+` is a space, `%XX` is a byte; bad escapes stay as they are.
fn form_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' => {
                let hex = bytes
                    .get(i + 1..i + 3)
                    .and_then(|pair| std::str::from_utf8(pair).ok())
                    .and_then(|pair| u8::from_str_radix(pair, 16).ok());
                if let Some(byte) = hex {
                    out.push(byte);
                    i += 2;
                } else {
                    out.push(b'%');
                }
            }
            other => out.push(other),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ---- artwork ------------------------------------------------------------------------------

/// The URLs of a `<fanart>` element: nested `<thumb>` elements, else its own text.
pub(crate) fn fanart_urls(root: &Element) -> Vec<String> {
    let mut urls = Vec::new();
    for fanart in kids(root, "fanart") {
        let nested: Vec<&str> = kids(fanart, "thumb")
            .map(|t| t.text.trim())
            .filter(|u| is_http(u))
            .collect();
        if nested.is_empty() {
            if is_http(&fanart.text) {
                urls.push(fanart.text.trim().to_owned());
            }
        } else {
            urls.extend(nested.into_iter().map(str::to_owned));
        }
    }
    urls
}

/// `(aspect, url)` of every top-level `<thumb>` that holds an http(s) URL, with the element for
/// its other attributes. A thumb without an aspect is a poster.
pub(crate) fn thumbs(root: &Element) -> Vec<(String, &Element)> {
    kids(root, "thumb")
        .filter(|t| is_http(&t.text))
        .map(|t| {
            let aspect = t.attribute("aspect").unwrap_or_default().trim();
            let aspect = if aspect.is_empty() { "poster" } else { aspect };
            (aspect.to_ascii_lowercase(), t)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::parse_roots;

    fn root(text: &str) -> Element {
        prepare(&parse_roots(text)[0])
    }

    #[test]
    fn bytes_are_decoded_by_mark_and_fall_back_to_windows_1252() {
        assert_eq!(decode_nfo_bytes(b"\xEF\xBB\xBF<a/>"), "<a/>");
        assert_eq!(decode_nfo_bytes(b"\xFF\xFE<\0a\0/\0>\0"), "<a/>");
        assert_eq!(decode_nfo_bytes(b"\xFE\xFF\0<\0a\0/\0>"), "<a/>");
        assert_eq!(decode_nfo_bytes("caf\u{e9}".as_bytes()), "caf\u{e9}");
        assert_eq!(decode_nfo_bytes(b"caf\xE9 \x80"), "caf\u{e9} \u{20ac}");
    }

    #[test]
    fn raw_text_ids() {
        let movie = detect_movie_ids("see https://www.themoviedb.org/movie/949-heat and tt0113277");
        assert_eq!(movie.imdb.as_deref(), Some("tt0113277"));
        assert_eq!(movie.tmdb, Some(949));
        assert_eq!(movie.tvdb, None);
        assert_eq!(
            detect_movie_ids("http://www.imdb.com/Title?0113277")
                .imdb
                .as_deref(),
            Some("tt0113277")
        );
        let show = detect_show_ids(
            "https://www.themoviedb.org/tv/1396 https://thetvdb.com/series/81189 tt0903747",
        );
        assert_eq!(
            (show.imdb.as_deref(), show.tmdb, show.tvdb),
            (Some("tt0903747"), Some(1396), Some(81189))
        );
        let guide = detect_show_ids(r#"{"tmdb":"1396","tvdb":"81189"}"#);
        assert_eq!((guide.tmdb, guide.tvdb), (Some(1396), Some(81189)));
        assert_eq!(
            detect_show_ids("http://thetvdb.com/?tab=series&id=81189").tvdb,
            Some(81189)
        );
        assert_eq!(detect_movie_ids("nothing here"), DetectedIds::default());
        assert_eq!(detect_movie_ids("themoviedb.org/movie/0").tmdb, None);
    }

    #[test]
    fn values_parse_tolerantly() {
        assert_eq!(parse_year("1995-03-02"), 1995);
        assert_eq!(parse_year("n/a"), 0);
        assert_eq!(parse_count("1,234"), 1234);
        assert_eq!(parse_count("3.0"), 3);
        assert_eq!(parse_float("7,5"), Some(7.5));
        assert_eq!(parse_float("8.5/10"), Some(8.5));
        assert_eq!(parse_float("n/a"), None);
        assert_eq!(parse_minutes("120"), 120);
        assert_eq!(parse_minutes("120 min"), 120);
        assert_eq!(parse_minutes("1h 58m"), 118);
        assert_eq!(parse_minutes("1:58:00"), 118);
        assert_eq!(parse_minutes("soon"), 0);
        assert_eq!(parse_date("2024/3/1").unwrap().to_string(), "2024-03-01");
        assert_eq!(
            parse_date("2024-03-01T10:00:00Z").unwrap().to_string(),
            "2024-03-01"
        );
        assert_eq!(parse_date("01.03.2024").unwrap().to_string(), "2024-03-01");
        assert_eq!(parse_date("2024-13-45"), None);
        assert_eq!(parse_date("unknown"), None);
        assert_eq!(
            parse_timestamp("2024-03-01T10:20").as_deref(),
            Some("2024-03-01 10:20:00")
        );
        assert_eq!(
            parse_timestamp("2024-03-01").as_deref(),
            Some("2024-03-01 00:00:00")
        );
        assert_eq!(parse_timestamp("yesterday"), None);
    }

    #[test]
    fn names_are_folded_and_open_tags_are_lifted() {
        let r = root(
            "<Movie><Original_Title>Heat</Original_Title><year>1995<plot>Two men.</plot><runtime>170</runtime></Movie>",
        );
        assert_eq!(text(&r, "originaltitle"), "Heat");
        assert_eq!(text(&r, "year"), "1995");
        assert_eq!(text(&r, "plot"), "Two men.");
        assert_eq!(text(&r, "runtime"), "170");
    }

    #[test]
    fn ids_come_from_every_spelling() {
        let r = root(
            "<movie><uniqueid type=\"themoviedb\" default=\"true\">949</uniqueid><uniqueid type=\"IMDb\">tt0113277</uniqueid><id>12345</id><tvdbid>7</tvdbid><ids><entry><key>trakt</key><value>55</value></entry></ids></movie>",
        );
        let ids = read_ids(&r, false);
        assert_eq!(ids.get("tmdb"), Some("949"));
        assert_eq!(ids.get("imdb"), Some("tt0113277"));
        assert_eq!(ids.get("tvdb"), Some("7"));
        assert_eq!(ids.get("trakt"), Some("55"));
        assert_eq!(ids.len(), 4, "a numeric <id> of a movie is not an ID");
        let tv = read_ids(&root("<tvshow><id>81189</id></tvshow>"), true);
        assert_eq!(tv.get("tvdb"), Some("81189"));
        let imdb_id = read_ids(&root("<movie><id>tt0113277</id></movie>"), false);
        assert_eq!(imdb_id.get("imdb"), Some("tt0113277"));
        let url = read_ids(
            &root("<movie><imdb>http://www.imdb.com/title/tt0113277/</imdb></movie>"),
            false,
        );
        assert_eq!(url.get("imdb"), Some("tt0113277"));
    }

    #[test]
    fn ratings_in_both_layouts() {
        let new = read_ratings(&root(
            "<movie><ratings><rating name=\"themoviedb\" max=\"10\" default=\"true\"><value>7.5</value><votes>1,200</votes></rating><rating name=\"rottenTomatoes\" max=\"100\"><value>91</value><votes>0</votes></rating><rating name=\"metascore\" max=\"100\"><value>n/a</value></rating></ratings><userrating>8</userrating></movie>",
        ));
        let tmdb = new.get("tmdb").unwrap();
        assert_eq!((tmdb.value, tmdb.votes, tmdb.max_value), (7.5, 1200, 10));
        assert_eq!(new.get("tomatometerallcritics").unwrap().max_value, 100);
        assert!(new.get("metacritic").is_none(), "a bad value is skipped");
        assert_eq!(new.get("user").unwrap().value, 8.0);
        let old = read_ratings(&root(
            "<movie><rating>7,2</rating><votes>345</votes><userrating>0</userrating></movie>",
        ));
        assert_eq!(old.get("NFO").unwrap().votes, 345);
        assert!(old.get("user").is_none());
    }

    #[test]
    fn credits_cover_actors_crew_and_guests() {
        let r = root(
            "<episodedetails><director tmdbid=\"5\">Ann Lee</director><credits>Bo Wu</credits><actor><name>Cy Day</name><role>Hero</role><thumb>https://example.test/c.jpg</thumb><tmdbid>9</tmdbid></actor><actor><name>Di Egan</name><type>GuestStar</type></actor><actor><role>nobody</role></actor></episodedetails>",
        );
        let credits = read_credits(&r);
        let kinds: Vec<_> = credits
            .iter()
            .map(|c| (c.person.name.as_str(), c.kind))
            .collect();
        assert_eq!(
            kinds,
            [
                ("Cy Day", CreditKind::Actor),
                ("Di Egan", CreditKind::Guest),
                ("Ann Lee", CreditKind::Director),
                ("Bo Wu", CreditKind::Writer),
            ]
        );
        assert_eq!(credits[0].role, "Hero");
        assert_eq!(credits[0].person.ids.get("tmdb"), Some("9"));
        assert_eq!(credits[0].person.thumb_url, "https://example.test/c.jpg");
        assert_eq!(credits[2].person.ids.get("tmdb"), Some("5"));
    }

    #[test]
    fn a_writer_listed_twice_is_read_once() {
        let r = root(
            "<movie><credits>Dan O'Bannon</credits><credits>Ronald Shusett</credits><credits>Dan O'Bannon</credits></movie>",
        );
        let names: Vec<_> = read_credits(&r)
            .iter()
            .map(|c| c.person.name.clone())
            .collect();
        assert_eq!(names, ["Dan O'Bannon", "Ronald Shusett"]);
    }

    #[test]
    fn plug_in_trailers_become_links() {
        let t = |raw| read_trailer(raw).unwrap();
        assert_eq!(
            t("plugin://plugin.video.youtube/play/?video_id=abc123").url,
            "https://www.youtube.com/watch?v=abc123"
        );
        assert_eq!(
            t("plugin://plugin.video.youtube/?action=play_video&videoid=abc123").provider,
            "youtube"
        );
        let wrapped = t(
            "plugin://plugin.video.hdtrailers_net/video/hd-trailers.net/https%3A%2F%2Fwww.hd-trailers.net%2Fmovie%2Fheat%2F",
        );
        assert_eq!(wrapped.url, "https://www.hd-trailers.net/movie/heat/");
        assert_eq!(wrapped.provider, "hdtrailers");
        assert_eq!(
            t("plugin://plugin.video.hdtrailers_net/video/apple.com/http%3A%2F%2Ftrailers.apple.com%2Fa+b%2Fx.mov%3Fq%3D1").url,
            "http://trailers.apple.com/a b/x.mov?q=1"
        );
        assert_eq!(
            t("plugin://plugin.video.hdtrailers_net/video/x?/https%3A%2F%2Fexample.test%2Ft").url,
            "https://example.test/t"
        );
        // not a link behind the plug-in: the text is kept
        assert_eq!(
            t("plugin://plugin.video.hdtrailers_net/video/heat/1").url,
            "plugin://plugin.video.hdtrailers_net/video/heat/1"
        );
        assert_eq!(
            t("https://example.test/t.mp4").url,
            "https://example.test/t.mp4"
        );
        assert_eq!(
            t("plugin://plugin.video.other/x").url,
            "plugin://plugin.video.other/x"
        );
        assert!(read_trailer("  ").is_none());
    }

    #[test]
    fn single_tag_lists_are_split_and_multi_tag_lists_are_not() {
        let r = root(
            "<movie><studio>A Co, B Co</studio><country>US</country><country>FR</country></movie>",
        );
        assert_eq!(read_list(&r, "studio").0, ["A Co", "B Co"]);
        assert_eq!(read_list(&r, "country").0, ["US", "FR"]);
        assert_eq!(read_list(&r, "country").1, "US, FR");
    }
}
