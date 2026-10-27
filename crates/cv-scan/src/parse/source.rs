//! Media source detection from a name.

use std::sync::LazyLock;

use cv_domain::MediaSource;
use regex::Regex;

/// Characters that bound a source token: path separators, space, `_`, `,`, `.`, brackets, `-`.
const BOUND: &str = r"[/\ _,.()\[\]-]";

/// Source patterns in priority order; the first that matches a whole token wins. Hyphenated
/// spellings are part of the pattern; `-` is a bound only around the token.
const SOURCES: &[(MediaSource, &str)] = &[
    (
        MediaSource::UhdBluray,
        r"uhd[ ._-]?blu-?ray|uhdbd|uhd[ ._-]?remux|ultra[ ._-]?hd[ ._-]?blu-?ray",
    ),
    (
        MediaSource::Bluray,
        r"blu-?ray|blueray|bdrip|brrip|bdremux|bd[ ._-]?remux|fullbluray|fullblu-ray|bd25|bd50",
    ),
    (MediaSource::HdDvd, r"hd-?dvd"),
    (MediaSource::DvdScr, r"dvdscr|dvdscreener"),
    (MediaSource::Dvd, r"dvd|dvdrip|dvdr|dvd5|dvd9|dvdremux"),
    (MediaSource::WebDl, r"web-dl|webdl|web dl|web|web-dlrip"),
    (MediaSource::WebRip, r"webrip|web-rip"),
    (MediaSource::HdRip, r"hdrip"),
    (MediaSource::Tv, r"hdtv|pdtv|dsr|tvrip"),
    (MediaSource::Vhs, r"vhs|vhsrip"),
    (MediaSource::Laserdisc, r"laserdisc"),
    (MediaSource::Cam, r"cam|camrip|hdcam"),
    (MediaSource::Tc, r"telecine|tc|hdtc"),
    (MediaSource::Ts, r"telesync|ts|hdts"),
    (MediaSource::R5, r"r5"),
];

static COMPILED: LazyLock<Vec<(MediaSource, Regex)>> = LazyLock::new(|| {
    SOURCES
        .iter()
        .map(|(source, pattern)| {
            let re = Regex::new(&format!(r"(?i)(?:^|{BOUND})(?:{pattern})(?:$|{BOUND})"))
                .expect("static media source pattern");
            (source.clone(), re)
        })
        .collect()
});

/// The media source named in `name`, or [`MediaSource::Unknown`].
///
/// A source counts when it is a whole token, bounded by path separators, spaces, `_`, `,`, `.`,
/// brackets or `-` (or by the ends of the text). `Dune.Part.Two.2024.2160p.WEB-DL.DV.mkv` is
/// [`MediaSource::WebDl`]; `Tiger.Movie.mkv` is not a TV source.
pub fn detect_media_source(name: &str) -> MediaSource {
    COMPILED
        .iter()
        .find(|(_, re)| re.is_match(name))
        .map_or(MediaSource::Unknown, |(source, _)| source.clone())
}
