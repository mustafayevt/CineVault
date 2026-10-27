//! Reading a release name: title, year, season and episodes, resolution, source,
//! codec, HDR, audio, and the warnings a name or file list can carry.
//!
//! Title and year come from the library scanner's parser ([`cv_scan::parse`]), the source from
//! its source detector and single or multi-episode numbers from its episode parser, so a release
//! is read exactly like a file in the library. What the scanner does not read (packs and season
//! ranges, resolution, codec, HDR, audio, cam words) is a small set of expressions here.
//!
//! The parser is total: any text gives a [`ParsedRelease`], with unknown parts left unknown.

use std::sync::LazyLock;

use cv_domain::MediaSource;
use cv_scan::parse::{detect_clean_title_and_year, detect_media_source};
use regex::Regex;

use crate::model::{Hdr, Resolution};

/// Which episodes a release holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EpisodeSpan {
    /// Not a TV release as far as the name shows (a movie, or a whole-show name without a
    /// season).
    None,
    /// One episode: `S01E02`.
    Single { season: u16, episode: u16 },
    /// Consecutive episodes of one season: `S01E01-E02`, `S01E01E02`.
    Multi { season: u16, first: u16, last: u16 },
    /// A whole season: `S01`, `Season 1`.
    SeasonPack(u16),
    /// Several seasons: `S01-S03`, `Seasons 1-3`.
    SeasonRange { first: u16, last: u16 },
    /// The whole show: `Complete Series`.
    Complete,
}

impl EpisodeSpan {
    /// Whether the name carried any season or episode information.
    #[must_use]
    pub fn is_tv(self) -> bool {
        self != Self::None
    }

    /// Whether the release is more than one episode (a pack of any kind).
    #[must_use]
    pub fn is_pack(self) -> bool {
        matches!(
            self,
            Self::SeasonPack(_) | Self::SeasonRange { .. } | Self::Complete | Self::Multi { .. }
        )
    }
}

/// Video codec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Codec {
    /// x265, H.265, HEVC.
    Hevc,
    /// x264, H.264, AVC.
    Avc,
    Av1,
    Xvid,
    Unknown,
}

/// Audio format, best first when a name lists several.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Audio {
    Atmos,
    TrueHd,
    DtsHd,
    Dts,
    /// Dolby Digital Plus (E-AC-3).
    DdPlus,
    /// Dolby Digital (AC-3).
    Ac3,
    Aac,
    Unknown,
}

/// A recording that is not a retail source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LowQuality {
    Cam,
    Telesync,
    Telecine,
    Screener,
}

/// What a release name says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedRelease {
    /// The title as the name spells it, words separated by single spaces.
    pub title: String,
    pub year: Option<u16>,
    pub span: EpisodeSpan,
    pub resolution: Resolution,
    pub source: MediaSource,
    /// A disc remux (no re-encode): ranks above the plain disc source.
    pub remux: bool,
    pub codec: Codec,
    pub hdr: Hdr,
    pub audio: Audio,
    /// `5.1`, `7.1`, `2.0` when the name gives the channels next to the audio format.
    pub channels: Option<String>,
    pub low_quality: Option<LowQuality>,
    /// The name or file list shows an executable or archive instead of video.
    pub payload_warning: bool,
}

struct Patterns {
    www_prefix: Regex,
    video_extension: Regex,
    resolution_2160: Regex,
    resolution_1080: Regex,
    resolution_720: Regex,
    resolution_480: Regex,
    hevc: Regex,
    avc: Regex,
    av1: Regex,
    xvid: Regex,
    dolby_vision: Regex,
    hdr10_plus: Regex,
    hdr10: Regex,
    hlg: Regex,
    atmos: Regex,
    truehd: Regex,
    dts_hd: Regex,
    dts: Regex,
    dd_plus: Regex,
    ac3: Regex,
    aac: Regex,
    channels: Regex,
    remux: Regex,
    cam: Regex,
    telesync: Regex,
    telecine: Regex,
    screener: Regex,
    marker: Regex,
    tag_cut: Regex,
    episode: Regex,
    episode_tail: Regex,
    cross: Regex,
    season_range: Regex,
    season_word_range: Regex,
    season_pack: Regex,
    season_word: Regex,
    complete: Regex,
    anime_dash: Regex,
}

/// A token boundary: the start or end of the text, or any character that is not a letter or
/// digit (release names separate words with dots, dashes, spaces, brackets and underscores).
const B: &str = r"(?:^|[^a-z0-9])";
const E: &str = r"(?:$|[^a-z0-9])";

static PATTERNS: LazyLock<Patterns> = LazyLock::new(|| {
    let re = |pattern: &str| Regex::new(pattern).expect("static release pattern");
    let token = |inner: &str| re(&format!("(?i){B}(?:{inner}){E}"));
    Patterns {
        www_prefix: re(
            r"(?i)^\s*(?:www\.)?[a-z0-9-]+\.(?:com|org|net|to|io|cc|me|info|tv)\s*[-\u{2013}]\s*",
        ),
        video_extension: re(
            r"(?i)\.(?:mkv|mp4|avi|m4v|mov|wmv|ts|m2ts|mpg|mpeg|webm|flv|iso|vob)$",
        ),
        resolution_2160: token(r"2160[pi]?|4k|uhd"),
        resolution_1080: token(r"1080[pi]|fhd"),
        resolution_720: token(r"720[pi]"),
        resolution_480: token(r"480[pi]|576[pi]|360p|sd"),
        hevc: token(r"x\.?265|h[ ._-]?265|hevc"),
        avc: token(r"x\.?264|h[ ._-]?264|avc"),
        av1: token(r"av1"),
        xvid: token(r"xvid|divx"),
        dolby_vision: token(r"dolby[ ._-]?vision|dv|dovi"),
        hdr10_plus: token(r"hdr10\+|hdr10plus|hdr10p"),
        hdr10: token(r"hdr10|hdr"),
        hlg: token(r"hlg"),
        atmos: token(r"atmos"),
        truehd: token(r"true[ ._-]?hd"),
        dts_hd: token(r"dts[ ._-]?hd(?:[ ._-]?(?:ma|hra))?|dts[ ._-]?x"),
        dts: token(r"dts"),
        dd_plus: re(&format!(
            r"(?i){B}(?:ddp|dd\+|e[ ._-]?ac[ ._-]?3|dd[ ._]plus)(?:[0-9]|{E})"
        )),
        ac3: re(&format!(r"(?i){B}(?:ac[ ._-]?3|dd)(?:[0-9]|{E})")),
        aac: re(&format!(r"(?i){B}aac(?:[0-9]|{E})")),
        channels: re(
            r"(?i)(?:atmos|aac|ac[ ._-]?3|ddp?\+?|e[ ._-]?ac[ ._-]?3|dts(?:[ ._-]?hd)?(?:[ ._-]?ma)?|true[ ._-]?hd|flac|opus)[ ._-]*([1-9][ .]?[01])(?:[^0-9]|$)",
        ),
        remux: token(r"remux|bd[ ._-]?remux"),
        cam: token(r"cam|camrip|hdcam"),
        telesync: token(r"telesync|hdts|ts|pdvd"),
        telecine: token(r"telecine|hdtc|tc"),
        screener: token(r"scr|screener|dvdscr|dvdscreener"),
        // Tags that cannot be part of a title: where the title ends when the name has no year.
        tag_cut: token(
            r"2160[pi]|1080[pi]|720[pi]|576[pi]|480[pi]|blu[ ._-]?ray|bdrip|brrip|web[ ._-]?dl|webrip|hdtv|dvdrip|x\.?26[45]|h[ ._-]?26[45]|hevc|remux",
        ),
        // The first place a name says "this is TV": a season/episode code, a season word or a
        // "complete" word. Everything before it is the title.
        marker: re(&format!(
            r"(?i){B}(?:s\d{{1,3}}(?:[ ._]?e\d|{E})|\d{{1,2}}x\d{{2,3}}{E}|seasons?[ ._-]?\d|complete[ ._-]+(?:series|collection|show|season|boxset|box[ ._-]?set|seasons))"
        )),
        episode: re(r"(?i)(?:^|[^a-z0-9])s(\d{1,3})[ ._]?e(\d{1,4})"),
        episode_tail: re(r"(?i)^(?:[ ._]?-[ ._]?e?|[ ._]?e)(\d{1,3})(?:[^a-z0-9]|$)"),
        cross: re(r"(?i)(?:^|[^a-z0-9])(\d{1,2})x(\d{2,3})(?:[^a-z0-9]|$)"),
        season_range: re(&format!(
            r"(?i){B}s(\d{{1,2}})[ ._]?(?:-|\u{{2013}}|to|~|\+|&)[ ._]?s?(\d{{1,2}}){E}"
        )),
        season_word_range: re(&format!(
            r"(?i){B}seasons?[ ._-]?(\d{{1,2}})[ ._]?(?:-|\u{{2013}}|to|~|\+|&)[ ._]?(?:seasons?[ ._-]?)?s?(\d{{1,2}}){E}"
        )),
        season_pack: re(&format!(r"(?i){B}s(\d{{1,3}}){E}")),
        season_word: re(&format!(r"(?i){B}seasons?[ ._-]?(\d{{1,3}}){E}")),
        complete: re(&format!(
            r"(?i){B}complete[ ._-]+(?:series|collection|show|boxset|box[ ._-]?set|seasons){E}"
        )),
        anime_dash: re(r"(?i)\s-\s(\d{1,4})(?:v\d)?(?:[\s\[(]|$)"),
    }
});

fn matches(pattern: &Regex, text: &str) -> bool {
    pattern.is_match(text)
}

fn resolution_of(name: &str) -> Resolution {
    let p = &*PATTERNS;
    if matches(&p.resolution_2160, name) {
        Resolution::R2160
    } else if matches(&p.resolution_1080, name) {
        Resolution::R1080
    } else if matches(&p.resolution_720, name) {
        Resolution::R720
    } else if matches(&p.resolution_480, name) {
        Resolution::R480
    } else {
        Resolution::Unknown
    }
}

fn codec_of(name: &str) -> Codec {
    let p = &*PATTERNS;
    if matches(&p.hevc, name) {
        Codec::Hevc
    } else if matches(&p.av1, name) {
        Codec::Av1
    } else if matches(&p.avc, name) {
        Codec::Avc
    } else if matches(&p.xvid, name) {
        Codec::Xvid
    } else {
        Codec::Unknown
    }
}

fn hdr_of(name: &str) -> Hdr {
    let p = &*PATTERNS;
    if matches(&p.dolby_vision, name) {
        Hdr::DolbyVision
    } else if matches(&p.hdr10_plus, name) {
        Hdr::Hdr10Plus
    } else if matches(&p.hdr10, name) {
        Hdr::Hdr10
    } else if matches(&p.hlg, name) {
        Hdr::Hlg
    } else {
        Hdr::None
    }
}

fn audio_of(name: &str) -> Audio {
    let p = &*PATTERNS;
    if matches(&p.atmos, name) {
        Audio::Atmos
    } else if matches(&p.truehd, name) {
        Audio::TrueHd
    } else if matches(&p.dts_hd, name) {
        Audio::DtsHd
    } else if matches(&p.dts, name) {
        Audio::Dts
    } else if matches(&p.dd_plus, name) {
        Audio::DdPlus
    } else if matches(&p.ac3, name) {
        Audio::Ac3
    } else if matches(&p.aac, name) {
        Audio::Aac
    } else {
        Audio::Unknown
    }
}

fn channels_of(name: &str) -> Option<String> {
    PATTERNS
        .channels
        .captures(name)
        .map(|c| c[1].replace([' ', '_'], "."))
}

/// A recording quality warning from the name's words or the source detector.
fn low_quality_of(name: &str, source: &MediaSource) -> Option<LowQuality> {
    let p = &*PATTERNS;
    match source {
        MediaSource::Cam => return Some(LowQuality::Cam),
        MediaSource::Ts => return Some(LowQuality::Telesync),
        MediaSource::Tc => return Some(LowQuality::Telecine),
        MediaSource::DvdScr => return Some(LowQuality::Screener),
        _ => {}
    }
    if matches(&p.cam, name) {
        Some(LowQuality::Cam)
    } else if matches(&p.telesync, name) {
        Some(LowQuality::Telesync)
    } else if matches(&p.telecine, name) {
        Some(LowQuality::Telecine)
    } else if matches(&p.screener, name) {
        Some(LowQuality::Screener)
    } else {
        None
    }
}

/// File extensions that are not video and are not disc images.
const PAYLOAD_EXTENSIONS: [&str; 9] =
    ["exe", "bat", "cmd", "lnk", "msi", "vbs", "zip", "rar", "7z"];

/// Whether a file name ends in an executable or archive extension. `.iso` is a disc image and is
/// not a warning. `.scr` counts only in a file list: at the end of a release *name* it is
/// usually the "screener" tag.
fn is_payload_file(file: &str) -> bool {
    let lower = file.trim().to_lowercase();
    let Some((_, extension)) = lower.rsplit_once('.') else {
        return false;
    };
    PAYLOAD_EXTENSIONS.contains(&extension) || extension == "scr"
}

fn is_payload_name(name: &str) -> bool {
    let lower = name.trim().to_lowercase();
    lower
        .rsplit_once('.')
        .is_some_and(|(_, extension)| PAYLOAD_EXTENSIONS.contains(&extension))
}

fn number(text: &str) -> Option<u16> {
    text.parse().ok()
}

/// The season and episodes named by a code such as `S01E02`, `S01E01E02`, `S01E01-E03`,
/// `S01E01-03`.
fn episode_span(name: &str) -> EpisodeSpan {
    let p = &*PATTERNS;
    if let Some(caps) = p.episode.captures(name)
        && let (Some(season), Some(first)) = (number(&caps[1]), number(&caps[2]))
    {
        let whole = caps.get(0).expect("group 0");
        let mut tail = &name[whole.end()..];
        let mut last = first;
        while let Some(next) = p.episode_tail.captures(tail) {
            let Some(value) = number(&next[1]) else { break };
            let connector = next.get(0).expect("group 0");
            last = value;
            tail = &tail[connector.end()..];
        }
        // `S01E01-E03` is episodes one to three.
        return if last > first && last - first < 100 {
            EpisodeSpan::Multi {
                season,
                first,
                last,
            }
        } else {
            EpisodeSpan::Single {
                season,
                episode: first,
            }
        };
    }
    if let Some(caps) = p.cross.captures(name)
        && let (Some(season), Some(episode)) = (number(&caps[1]), number(&caps[2]))
    {
        return EpisodeSpan::Single { season, episode };
    }
    for range in [&p.season_range, &p.season_word_range] {
        if let Some(caps) = range.captures(name)
            && let (Some(first), Some(last)) = (number(&caps[1]), number(&caps[2]))
            && first <= last
        {
            return EpisodeSpan::SeasonRange { first, last };
        }
    }
    for pack in [&p.season_pack, &p.season_word] {
        if let Some(caps) = pack.captures(name)
            && let Some(season) = number(&caps[1])
        {
            return EpisodeSpan::SeasonPack(season);
        }
    }
    if matches(&p.complete, name) {
        return EpisodeSpan::Complete;
    }
    EpisodeSpan::None
}

/// The name without a video extension and a leading site name (`www.site.com - `).
fn clean_name(name: &str) -> String {
    let p = &*PATTERNS;
    let trimmed = name.trim();
    let without_site = p.www_prefix.replace(trimmed, "");
    p.video_extension
        .replace(&without_site, "")
        .trim()
        .to_owned()
}

/// The title and year of a name that the library parser reads as a movie name.
fn title_and_year(text: &str) -> (String, Option<u16>) {
    // The scanner's parser stops at a tag only from the third word on, so `Heat.720p` would
    // keep the tag; cut the name at the first tag that cannot be part of a title.
    let text = match PATTERNS.tag_cut.find(text) {
        Some(found) if found.start() > 0 && !text[..found.start()].trim().is_empty() => {
            &text[..found.start()]
        }
        _ => text,
    };
    // A fake extension keeps the parser from taking `.1968` or `.WEB` for one.
    let parsed = detect_clean_title_and_year(&format!("{text}.mkv"), &[] as &[&str]);
    (
        parsed
            .title
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" "),
        parsed.year,
    )
}

/// Anime group releases (`[Group] Show - 05 [1080p].mkv`) name the episode by a bare number
/// after a dash. Returns the title text before the number and the episode.
fn anime_episode(name: &str) -> Option<(String, u16)> {
    if !name.trim_start().starts_with('[') {
        return None;
    }
    let last = PATTERNS.anime_dash.captures_iter(name).last()?;
    let episode = last[1].parse().ok()?;
    let head = &name[..last.get(0)?.start()];
    Some((head.to_owned(), episode))
}

/// Reads a release name. `files` are the names of the files inside the torrent when the source
/// lists them (they only feed the payload warning).
#[must_use]
pub fn parse_release(name: &str, files: &[String]) -> ParsedRelease {
    let p = &*PATTERNS;
    let cleaned = clean_name(name);

    let mut span = episode_span(&cleaned);
    let (title, year) = if span.is_tv() {
        // The title is what comes before the first season or episode code.
        let head = p
            .marker
            .find(&cleaned)
            .map_or(cleaned.as_str(), |m| &cleaned[..m.start()]);
        let (title, year) = title_and_year(head);
        if title.is_empty() {
            // `S01E02.Title...` with nothing before the code: read the whole name.
            title_and_year(&cleaned)
        } else {
            (title, year)
        }
    } else {
        if let Some((head, episode)) = anime_episode(&cleaned) {
            span = EpisodeSpan::Single { season: 1, episode };
            title_and_year(&head)
        } else {
            title_and_year(&cleaned)
        }
    };

    let source = detect_media_source(&cleaned);
    let low_quality = low_quality_of(&cleaned, &source);
    let payload_warning = is_payload_name(&cleaned) || files.iter().any(|f| is_payload_file(f));
    ParsedRelease {
        title,
        year,
        span,
        resolution: resolution_of(&cleaned),
        remux: matches(&p.remux, &cleaned),
        source,
        codec: codec_of(&cleaned),
        hdr: hdr_of(&cleaned),
        audio: audio_of(&cleaned),
        channels: channels_of(&cleaned),
        low_quality,
        payload_warning,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_movie_name_is_read() {
        let r = parse_release("Night.of.the.Living.Dead.1968.1080p.BluRay.x264-GRP", &[]);
        assert_eq!(r.title, "Night of the Living Dead");
        assert_eq!(r.year, Some(1968));
        assert_eq!(r.resolution, Resolution::R1080);
        assert_eq!(r.source, MediaSource::Bluray);
        assert_eq!(r.codec, Codec::Avc);
        assert_eq!(r.span, EpisodeSpan::None);
    }

    #[test]
    fn episode_codes_are_read() {
        let r = parse_release(
            "Severance.S01E02.Half.Loop.1080p.ATVP.WEB-DL.DDP5.1.H.264-GRP",
            &[],
        );
        assert_eq!(r.title, "Severance");
        assert_eq!(
            r.span,
            EpisodeSpan::Single {
                season: 1,
                episode: 2
            }
        );
        assert_eq!(r.audio, Audio::DdPlus);
        assert_eq!(r.channels.as_deref(), Some("5.1"));
    }

    #[test]
    fn a_name_that_is_not_text_gives_an_unknown_release() {
        let r = parse_release("", &[]);
        assert_eq!(r.title, "");
        assert_eq!(r.resolution, Resolution::Unknown);
        assert_eq!(r.span, EpisodeSpan::None);
        assert!(!r.payload_warning);
    }

    #[test]
    fn payload_files_warn_but_disc_images_do_not() {
        assert!(is_payload_file("Movie.2020.exe"));
        assert!(is_payload_file("README.ZIP"));
        assert!(is_payload_file("player.scr"));
        assert!(!is_payload_file("Movie.2020.iso"));
        assert!(!is_payload_file("Movie.2020.mkv"));
        assert!(!is_payload_file("noextension"));
        assert!(!is_payload_name("Movie.2020.DVDSCR"));
        assert!(is_payload_name("Movie.2020.1080p.exe"));
    }
}
