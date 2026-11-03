//! The release tokens: what the name a video was downloaded under says about the release.
//!
//! [`release_info`] reads the original file name of a title (the name its main video had when it
//! was first scanned, which renames never change) and gives:
//!
//! - **group**: the group after the last `-` (`…x264-GRP`), or of a leading `[GRP]`;
//! - **tags**: the release part of the name as written (`1080p.BluRay.x264`);
//! - **quality**: the source and resolution in the `Source-Resolution` form download managers
//!   use (`Bluray-1080p`, `WEBDL-2160p`), with `Proper` or `Repack` when the name says so.
//!
//! Nothing is guessed: a value the name does not clearly give is empty. A group is only read
//! when the text right before its dash is a release tag, so a dash in a title (`Spider-Man`,
//! `X-Men`) never gives one, and a tag after the dash (`WEB-DL`) is not taken for a group.
//!
//! The release part starts at the first quality tag (resolution, source, codec, HDR, audio)
//! after the episode code or, without one, after the last year, together with the language and
//! `Proper` words right before it (`German.DL.1080p`). Before that point is the title, so a title
//! word that looks like a tag (`Charlotte's.Web.2006`) is not read as one.

use std::sync::LazyLock;

use regex::Regex;

/// What a release name says. Every field is empty when the name does not say it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReleaseInfo {
    pub group: String,
    pub tags: String,
    pub quality: String,
}

/// The characters that separate the words of a release name.
const SEP: &str = r"[ ._\[\]()-]";

/// Resolution, source, codec, HDR and audio tags: what makes a word a release tag.
const QUALITY: &str = concat!(
    r"(?:480|576|720|1080|2160|4320)[pi]|4k|uhd",
    r"|blu-?ray|bdrip|brrip|bd25|bd50|bdremux|remux|web-?dl|web-?rip|web|hdtv|pdtv|sdtv|tvrip|dsr",
    r"|dvd(?:rip|r|5|9|scr)?|hd-?dvd|hdrip|camrip|hdcam|cam|telesync|hdts|telecine|hdtc|r5",
    r"|[xh]\.?26[45]|hevc|avc|xvid|divx|av1|vc-?1|mpeg-?2",
    r"|hdr(?:10\+?|10plus)?|dv|dovi|hlg|sdr|10bit|8bit",
    r"|dts(?:-?hd(?:-?ma)?|-?x|-?ma)?|truehd|atmos|e?ac-?3|aac(?:\d\.\d)?|ddp?(?:\d\.\d)?|dd\+(?:\d\.\d)?",
    r"|flac|opus|lpcm|pcm|mp3",
);

/// Words that belong to the release part when they come right before its first quality tag.
const LEADING: &str = concat!(
    r"proper|repack|rerip|real|internal",
    r"|german|english|french|truefrench|spanish|italian|dutch|russian|japanese|korean|hindi",
    r"|nordic|swedish|danish|norwegian|finnish|polish|portuguese|turkish",
    r"|multi|dual|dl|ml|dubbed|subbed|vostfr",
);

struct Patterns {
    /// A quality tag bounded by separators; group 1 is the tag.
    quality: Regex,
    /// The whole text is a quality tag.
    quality_word: Regex,
    /// The whole text is a leading word.
    leading_word: Regex,
    /// An episode code (`S01E02`, `s1e2e3`, `1x02`).
    episode: Regex,
    /// A year bounded by separators; group 1 is the year.
    year: Regex,
    /// A video file extension.
    extension: Regex,
    /// A `[...]` block at the end, with the separators before it.
    trailing_bracket: Regex,
    /// A leading `[Group]`; group 1 is the group.
    leading_group: Regex,
    /// What a group may hold.
    group_word: Regex,
    resolution: Regex,
    proper: Regex,
    repack: Regex,
    /// The quality sources in the order they are tried, with their labels.
    sources: Vec<(&'static str, Regex)>,
}

static PATTERNS: LazyLock<Patterns> = LazyLock::new(|| {
    let re = |p: &str| Regex::new(p).expect("static release pattern");
    let bounded = |p: &str| re(&format!(r"(?i)(?:^|{SEP})({p})(?:$|{SEP})"));
    Patterns {
        quality: bounded(QUALITY),
        quality_word: re(&format!(r"(?i)^(?:{QUALITY})$")),
        leading_word: re(&format!(r"(?i)^(?:{LEADING})$")),
        episode: re(&format!(
            r"(?i)(?:^|{SEP})(?:s\d{{1,4}}[ ._-]?e\d{{1,4}}(?:[ ._-]?e\d{{1,4}})*|\d{{1,2}}x\d{{2,4}})"
        )),
        year: bounded(r"(?:19|20)\d{2}"),
        extension: re(
            r"(?i)\.(?:mkv|mp4|m4v|avi|mov|wmv|ts|m2ts|mts|iso|img|mpg|mpeg|vob|webm|flv|ogm|ogv|rmvb|3gp|divx|strm)$",
        ),
        trailing_bracket: re(r"[ ._-]*\[[^\[\]]*\]$"),
        leading_group: re(r"^\[([^\[\]]{1,30})\]"),
        group_word: re(r"^[A-Za-z0-9][A-Za-z0-9_@&-]{1,29}$"),
        resolution: bounded(r"(?:480|576|720|1080|2160|4320)[pi]|4k|uhd"),
        proper: bounded(r"proper"),
        repack: bounded(r"repack|rerip"),
        sources: [
            ("Remux", r"remux|bdremux"),
            (
                "Bluray",
                r"uhd[ ._-]?blu-?ray|blu-?ray|bdrip|brrip|bd25|bd50",
            ),
            ("WEBRip", r"web-?rip"),
            ("WEBDL", r"web-?dl|web"),
            ("DVDSCR", r"dvdscr"),
            ("HDTV", r"hdtv|pdtv|sdtv|tvrip|dsr"),
            ("DVD", r"dvd(?:rip|r|5|9)?"),
            ("CAM", r"camrip|hdcam|cam"),
            ("TELESYNC", r"telesync|hdts"),
            ("TELECINE", r"telecine|hdtc"),
            ("REGIONAL", r"r5"),
        ]
        .into_iter()
        .map(|(label, pattern)| (label, bounded(pattern)))
        .collect(),
    }
});

/// What the release name `file_name` says (see the module documentation). A path is read by its
/// last part only.
#[must_use]
pub fn release_info(file_name: &str) -> ReleaseInfo {
    let p = &*PATTERNS;
    let name = file_name.rsplit(['/', '\\']).next().unwrap_or_default();
    let name = p
        .extension
        .find(name)
        .map_or(name, |m| &name[..m.start()])
        .trim();
    if name.is_empty() {
        return ReleaseInfo::default();
    }

    let anchor = title_end(name);
    let Some(start) = tags_start(name, anchor) else {
        return ReleaseInfo {
            group: leading_group(name).unwrap_or_default(),
            ..ReleaseInfo::default()
        };
    };

    // Site tags after the group (`-GRP[site]`) are no part of the release.
    let mut stripped = name;
    while let Some(m) = p.trailing_bracket.find(stripped) {
        stripped = &stripped[..m.start()];
    }
    let dash_group = (stripped.len() > start)
        .then(|| dash_group(stripped, start))
        .flatten();
    let (group, end) = match dash_group {
        Some((group, dash)) => (group, dash),
        None => {
            let end = if stripped.len() > start {
                stripped.len()
            } else {
                name.len()
            };
            (leading_group(name).unwrap_or_default(), end)
        }
    };
    let tags = name[start..end]
        .trim_matches(|c: char| " ._-[]()".contains(c))
        .to_owned();
    let quality = quality_of(&tags);
    ReleaseInfo {
        group,
        tags,
        quality,
    }
}

/// Where the title ends: after the first episode code, else after the last year, else at the
/// start.
fn title_end(name: &str) -> usize {
    let p = &*PATTERNS;
    if let Some(m) = p.episode.find(name) {
        return m.end();
    }
    let mut end = 0;
    let mut at = 0;
    while let Some(caps) = p.year.captures_at(name, at) {
        let year = caps.get(1).expect("year group");
        end = year.end();
        at = year.end();
    }
    end
}

/// Where the release part starts: the first quality tag at or after `anchor`, with the leading
/// words right before it.
fn tags_start(name: &str, anchor: usize) -> Option<usize> {
    let p = &*PATTERNS;
    let tag = p.quality.captures_at(name, anchor)?.get(1)?;
    let mut start = tag.start();
    loop {
        let before = name[anchor..start].trim_end_matches(|c: char| " ._-[]()".contains(c));
        let word_start = before
            .rfind(|c: char| " ._-[]()".contains(c))
            .map_or(0, |i| i + 1);
        let word = &before[word_start..];
        if word.is_empty() || !p.leading_word.is_match(word) {
            return Some(start);
        }
        start = anchor + word_start;
    }
}

/// The group after a dash in the last word of `name`, and where its dash is. The text before the
/// dash in that word must be release tags, and the release part (from `start`) must come before.
fn dash_group(name: &str, start: usize) -> Option<(String, usize)> {
    let p = &*PATTERNS;
    let word_start = name
        .rfind(|c: char| " ._[]()".contains(c))
        .map_or(0, |i| i + 1)
        .max(start);
    let word = &name[word_start..];
    let parts: Vec<&str> = word.split('-').collect();
    if parts.len() < 2 {
        return None;
    }
    // Channels and codecs whose dot split the word (`DD5.1-GRP`, `H.264-GRP`) start with digits.
    let continues_tag = |first: &str| {
        first.bytes().all(|b| b.is_ascii_digit())
            && name[..word_start]
                .strip_suffix('.')
                .and_then(|head| head.chars().last())
                .is_some_and(|c| c.is_ascii_digit() || c.eq_ignore_ascii_case(&'h'))
    };
    // A group right after a closing bracket (`[1080p.x264]-GRP`, as these tokens write it).
    let after_bracket =
        parts[0].is_empty() && word_start > start && name[..word_start].ends_with([']', ')']);
    let mut consumed = None;
    if after_bracket {
        consumed = Some(0);
    } else {
        for (i, part) in parts.iter().enumerate() {
            let with_previous = (i > 0).then(|| parts[i - 1..=i].join("-"));
            let is_tag = p.quality_word.is_match(part)
                || with_previous.is_some_and(|both| p.quality_word.is_match(&both))
                || (i == 0 && continues_tag(part));
            if !is_tag {
                break;
            }
            consumed = Some(parts[..=i].join("-").len());
        }
    }
    let consumed = consumed?;
    if consumed >= word.len() {
        return None;
    }
    let group = &word[consumed + 1..];
    if !is_group(group) {
        return None;
    }
    Some((group.to_owned(), word_start + consumed))
}

/// The group of a name that starts with `[Group]`.
fn leading_group(name: &str) -> Option<String> {
    let found = PATTERNS
        .leading_group
        .captures(name)?
        .get(1)?
        .as_str()
        .trim();
    let crc = found.len() == 8 && found.bytes().all(|b| b.is_ascii_hexdigit());
    (!crc && is_group(found)).then(|| found.to_owned())
}

fn is_group(text: &str) -> bool {
    let p = &*PATTERNS;
    p.group_word.is_match(text)
        && !text.bytes().all(|b| b.is_ascii_digit())
        && !p.quality_word.is_match(text)
        && !p.leading_word.is_match(text)
        && !text.eq_ignore_ascii_case("sample")
}

/// The `Source-Resolution` quality of the release part.
fn quality_of(tags: &str) -> String {
    let p = &*PATTERNS;
    if tags.is_empty() {
        return String::new();
    }
    let resolution = p.resolution.captures(tags).map(|caps| {
        let found = caps[1].to_ascii_lowercase();
        match found.as_str() {
            "4k" | "uhd" | "2160p" | "2160i" | "4320p" | "4320i" => "2160p".to_owned(),
            other => format!("{}p", other.trim_end_matches(['p', 'i'])),
        }
    });
    let source = p
        .sources
        .iter()
        .find(|(_, pattern)| pattern.is_match(tags))
        .map(|(label, _)| *label);
    let mut quality = match (source, resolution) {
        (Some("DVD" | "DVDSCR" | "CAM" | "TELESYNC" | "TELECINE" | "REGIONAL"), _) => {
            source.unwrap_or_default().to_owned()
        }
        (Some("HDTV"), Some(res)) if matches!(res.as_str(), "480p" | "576p") => "SDTV".to_owned(),
        (Some(source), Some(res)) => format!("{source}-{res}"),
        (Some(source), None) => source.to_owned(),
        (None, Some(res)) => res,
        (None, None) => String::new(),
    };
    if !quality.is_empty() {
        if p.proper.is_match(tags) {
            quality.push_str(" Proper");
        } else if p.repack.is_match(tags) {
            quality.push_str(" Repack");
        }
    }
    quality
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(name: &str) -> (String, String, String) {
        let info = release_info(name);
        (info.group, info.tags, info.quality)
    }

    fn owned(group: &str, tags: &str, quality: &str) -> (String, String, String) {
        (group.to_owned(), tags.to_owned(), quality.to_owned())
    }

    #[test]
    fn a_movie_release_is_read() {
        assert_eq!(
            read("Example.Movie.2020.2160p.UHD.BluRay.REMUX.HDR.HEVC.Atmos-GRP.mkv"),
            owned(
                "GRP",
                "2160p.UHD.BluRay.REMUX.HDR.HEVC.Atmos",
                "Remux-2160p"
            )
        );
        assert_eq!(
            read("Example.Movie.2020.1080p.WEB-DL.x264-GRP.mkv"),
            owned("GRP", "1080p.WEB-DL.x264", "WEBDL-1080p")
        );
        assert_eq!(
            read("Example Movie (2020) 720p WEBRip x265-GRP.mp4"),
            owned("GRP", "720p WEBRip x265", "WEBRip-720p")
        );
    }

    #[test]
    fn an_episode_release_keeps_its_languages() {
        assert_eq!(
            read("Example.Show.2017.S01E01.Episode.1.Bluray-1080p.German.DL.x265-GRP.mkv"),
            owned("GRP", "Bluray-1080p.German.DL.x265", "Bluray-1080p")
        );
        assert_eq!(
            read("Example.Movie.2020.German.DL.1080p.BluRay.x264-GRP.mkv"),
            owned("GRP", "German.DL.1080p.BluRay.x264", "Bluray-1080p")
        );
    }

    #[test]
    fn a_dash_in_the_title_gives_nothing() {
        for name in [
            "Spider-Man (2002).mkv",
            "Example.Movie-Part.Two.2021.mkv",
            "X-Men.mkv",
            "Example Movie (2020).mkv",
            "",
        ] {
            assert_eq!(read(name), owned("", "", ""), "{name}");
        }
    }

    #[test]
    fn a_tag_after_the_last_dash_is_no_group() {
        assert_eq!(
            read("Example.Movie.2020.1080p.WEB-DL.mkv"),
            owned("", "1080p.WEB-DL", "WEBDL-1080p")
        );
        assert_eq!(
            read("Example.Movie.2020.1080p.BluRay.DTS-HD.mkv").0,
            "",
            "DTS-HD is a tag"
        );
        assert_eq!(read("Example.Movie.2020.1080p.WEB-DL-GRP.mkv").0, "GRP");
    }

    #[test]
    fn channels_and_dotted_codecs_before_the_group() {
        assert_eq!(
            read("Example.Movie.2020.1080p.AMZN.WEB-DL.DDP5.1.H.264-GRP.mkv"),
            owned("GRP", "1080p.AMZN.WEB-DL.DDP5.1.H.264", "WEBDL-1080p")
        );
        assert_eq!(
            read("Example.Movie.2020.1080p.BluRay.DTS-HD.MA.5.1.x264-GRP.mkv").0,
            "GRP"
        );
        assert_eq!(
            read("Example.Movie.2020.1080p.BluRay.AAC2.0-GRP.mkv").0,
            "GRP"
        );
    }

    #[test]
    fn groups_with_a_dash_and_site_tags() {
        assert_eq!(
            read("Example.Movie.2020.1080p.BluRay.x264-D-Z0N3.mkv").0,
            "D-Z0N3"
        );
        assert_eq!(
            read("Example.Movie.2020.1080p.BluRay.x264-GRP[site].mkv"),
            owned("GRP", "1080p.BluRay.x264", "Bluray-1080p")
        );
    }

    #[test]
    fn a_leading_group() {
        assert_eq!(
            read("[GRP] Example Show - 05 [1080p].mkv"),
            owned("GRP", "1080p", "1080p")
        );
        assert_eq!(read("[ABCD1234] Example Show - 05.mkv").0, "", "a checksum");
        assert_eq!(read("[1080p] Example.mkv").0, "", "a tag");
    }

    #[test]
    fn proper_and_repack() {
        assert_eq!(
            read("Example.Movie.2020.PROPER.1080p.BluRay.x264-GRP.mkv"),
            owned("GRP", "PROPER.1080p.BluRay.x264", "Bluray-1080p Proper")
        );
        assert_eq!(
            read("Example.Show.S02E03.REPACK.720p.HDTV.x264-GRP.mkv").2,
            "HDTV-720p Repack"
        );
    }

    #[test]
    fn a_title_word_that_looks_like_a_tag_is_title() {
        assert_eq!(
            read("Charlottes.Web.2006.1080p.BluRay.x264-GRP.mkv"),
            owned("GRP", "1080p.BluRay.x264", "Bluray-1080p")
        );
        assert_eq!(
            read("Blade.Runner.2049.2017.2160p.WEB-DL.mkv").1,
            "2160p.WEB-DL"
        );
        assert_eq!(
            read("Example.Show.S01E02.The.French.Job.1080p.WEB.h264-GRP.mkv").1,
            "1080p.WEB.h264"
        );
    }

    #[test]
    fn qualities() {
        let quality = |name: &str| read(name).2;
        assert_eq!(quality("Example.2020.DVDRip.XviD-GRP.avi"), "DVD");
        assert_eq!(quality("Example.S01E01.480p.HDTV.x264-GRP.mkv"), "SDTV");
        assert_eq!(quality("Example.S01E01.HDTV.x264-GRP.mkv"), "HDTV");
        assert_eq!(quality("Example.2020.HDCAM.x264-GRP.mkv"), "CAM");
        assert_eq!(
            quality("Example.2020.4K.WEBRip.x265-GRP.mkv"),
            "WEBRip-2160p"
        );
        assert_eq!(quality("Example.2020.x264-GRP.mkv"), "");
    }

    #[test]
    fn a_name_named_by_these_tokens_reads_the_same() {
        assert_eq!(
            read("Example Movie (2020) [1080p.BluRay.x264]-GRP.mkv"),
            owned("GRP", "1080p.BluRay.x264", "Bluray-1080p")
        );
        assert_eq!(
            read(r"D:\Movies\Example.Movie.2020.1080p.BluRay.x264-GRP.mkv").0,
            "GRP"
        );
    }

    #[test]
    fn the_tokens_render_from_the_original_file_name() {
        use crate::tokens::{
            ModelOptions, RenamerOptions, movie_aliases, movie_model, render_name, tv_aliases,
            tv_model,
        };
        let mut movie = cv_domain::Item::movie("Example Movie");
        movie.core.year = 2020;
        movie.core.original_filename = "Example.Movie.2020.1080p.BluRay.x264-GRP.mkv".into();
        let model = movie_model(&movie, None, &ModelOptions::default());
        let name = render_name(
            "${title} (${year}) [${releaseTags}]${-,releaseGroup,} ${releaseQuality}",
            &movie_aliases(),
            &model,
            &RenamerOptions::default(),
        )
        .unwrap();
        assert_eq!(
            name,
            "Example Movie (2020) [1080p.BluRay.x264]-GRP Bluray-1080p"
        );

        let show = cv_domain::Item::show("Example Show");
        let mut episode = cv_domain::Item::episode(cv_domain::ItemId(1), "Pilot");
        episode.core.original_filename =
            "Example.Show.S01E01.Pilot.720p.WEB-DL.DDP5.1.H.264-GRP.mkv".into();
        let model = tv_model(&show, &episode, &ModelOptions::default());
        let name = render_name(
            "${releaseGroup} ${releaseQuality} ${releaseTags}",
            &tv_aliases(),
            &model,
            &RenamerOptions::default(),
        )
        .unwrap();
        assert_eq!(name, "GRP WEBDL-720p 720p.WEB-DL.DDP5.1.H.264");
    }
}
