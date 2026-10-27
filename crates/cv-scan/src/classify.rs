//! File-type classification by path.
//!
//! [`classify`] decides what a file is from its path relative to the data source: video,
//! trailer, sample, extra, audio, subtitle, NFO, one of the artwork types, theme and so on
//! ([`MediaFileType`]). It also reports whether the file belongs to a disc structure (DVD, HD-DVD
//! or Blu-ray) and whether it is the main identifier of one. The path is text; nothing is read
//! from disk, so a file does not need to exist and the rules behave identically on every
//! operating system. Both `/` and `\` separate segments.
//!
//! The rules run in this order; the first that applies wins:
//!
//! 1. any folder between the first segment and the file is an extras folder
//!    ([`EXTRA_FOLDERS`]): [`MediaFileType::Extra`], whatever the extension;
//! 2. `nfo`, `vsmeta`, and `xml` named `*-mediainfo` or `*.mediainfo`;
//! 3. artwork extensions ([`ARTWORK_EXTENSIONS`]) go through the ordered artwork rules below;
//! 4. theme music names (`theme`, `soundtrack`, with an optional number);
//! 5. audio and subtitle extensions;
//! 6. extras by name (`.EXTRAS.`, a trailing `-extras`, `-featurette`, a bracketed `[interview]`);
//! 7. video extensions, which are a trailer, a sample or plain video by name or parent folder;
//! 8. a name ending in `BDMV`, `VIDEO_TS` or `HVDVD_TS` is a disc folder used as a video;
//! 9. `txt` is text, `bif` is a trick-play index, anything else is unknown.
//!
//! [`classify`] never reads a file. [`is_video_content`] is the one check that does: `.ts` is
//! also the extension of TypeScript source, so the walkers ask it before counting a `.ts` file
//! as a video.
//!
//! The artwork rules, in order (all case-insensitive): movie-set artwork (`movieset-poster.jpg`),
//! season poster, fanart, banner and thumb, then poster, numbered fanart, fanart, banner, thumb,
//! clearart, clearlogo, disc art, character art, key art and square art, a file named like its
//! folder (poster), and finally generic graphic.
//!
//! The fixtures in `tests/compat` show that a file in an `extrafanart` or `extrathumbs` folder is
//! retyped only when its name already matches a fanart or thumb rule; any other name there stays a
//! graphic, as the fixtures pin it (there is no catch-all folder rule).

use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

pub use cv_domain::MediaFileType;

/// Folder names (compared without regard to case) whose content is extras.
pub const EXTRA_FOLDERS: &[&str] = &[
    "extra",
    "extras",
    "behind the scenes",
    "behindthescenes",
    "deleted scenes",
    "deletedscenes",
    "deleted",
    "featurette",
    "featurettes",
    "interview",
    "interviews",
    "scene",
    "scenes",
    "short",
    "shorts",
    "other",
    "others",
    "bloopers",
    "trickplay",
];

/// Folder names whose videos are trailers.
pub const TRAILER_FOLDERS: &[&str] = &["trailer", "trailers"];

pub const ARTWORK_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "tbn", "gif", "bmp", "webp"];

pub const VIDEO_EXTENSIONS: &[&str] = &[
    "3gp", "asf", "asx", "avc", "avi", "bdmv", "bin", "bivx", "braw", "dat", "divx", "dv",
    "dvr-ms", "evo", "fli", "flv", "h264", "ifo", "img", "iso", "mts", "mt2s", "m2ts", "m2v",
    "m4v", "mkv", "mk3d", "mov", "mp4", "mpeg", "mpg", "nrg", "nsv", "nuv", "ogm", "pva", "qt",
    "rm", "rmvb", "strm", "svq3", "ts", "ty", "viv", "vob", "vp3", "wmv", "webm", "xvid",
];

pub const AUDIO_EXTENSIONS: &[&str] = &[
    "a52", "aa3", "aac", "ac3", "adt", "adts", "aif", "aiff", "alac", "ape", "at3", "atrac", "au",
    "dts", "dtshd", "flac", "m4a", "m4b", "m4p", "mid", "midi", "mka", "mp3", "mpa", "mlp", "oga",
    "ogg", "pcm", "ra", "ram", "tta", "thd", "wav", "wave", "wma",
];

pub const SUBTITLE_EXTENSIONS: &[&str] = &[
    "aqt", "cvd", "dks", "jss", "sub", "sup", "ttxt", "mpl", "pjs", "psb", "rt", "srt", "smi",
    "ssf", "ssa", "svcd", "usf", "ass", "pgs", "vobsub", "vtt",
];

/// Extensions of disc images and raw data files; media info for them comes only from the XML file.
pub const BINARY_EXTENSIONS: &[&str] = &["bin", "dat", "img", "nrg"];

/// What [`classify`] found out about a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Classification {
    pub file_type: MediaFileType,
    /// The path is part of a DVD, HD-DVD or Blu-ray structure.
    pub disc_file: bool,
    /// The file is the one that identifies a disc structure (`VIDEO_TS.IFO`, `index.bdmv`,
    /// `HV000I01.IFO`).
    pub main_disc_identifier: bool,
}

/// Classifies a path relative to its data source.
pub fn classify(relative_path: &str) -> Classification {
    let segments: Vec<&str> = relative_path
        .split(['/', '\\'])
        .filter(|s| !s.is_empty())
        .collect();
    let Some((file, folders)) = segments.split_last() else {
        return Classification {
            file_type: MediaFileType::Unknown,
            disc_file: false,
            main_disc_identifier: false,
        };
    };
    let parent = folders.last().copied();
    // The first segment is the item's own folder (or the show folder); extras folders count only
    // below it.
    let middle: &[&str] = if folders.len() > 1 {
        &folders[1..]
    } else {
        &[]
    };
    Classification {
        file_type: file_type(file, parent, middle),
        disc_file: is_disc_file(file, &segments),
        main_disc_identifier: is_main_disc_identifier(file),
    }
}

/// The type of a file from its path relative to the data source.
pub fn media_file_type(relative_path: &str) -> MediaFileType {
    classify(relative_path).file_type
}

/// True for a trick-play index name of the form `<name>-<width>-<interval>.bif`, for example
/// `Movie-320-10.bif`.
pub fn is_trickplay_index(file_name: &str) -> bool {
    TRICKPLAY_BIF.is_match(file_name)
}

/// `(base name, extension)`; the extension is lower case and empty when there is no dot.
fn split_extension(file: &str) -> (&str, String) {
    match file.rfind('.') {
        Some(dot) => (&file[..dot], file[dot + 1..].to_lowercase()),
        None => (file, String::new()),
    }
}

fn has(list: &[&str], ext: &str) -> bool {
    list.contains(&ext)
}

fn folder_in(list: &[&str], folder: &str) -> bool {
    let folder = folder.to_lowercase();
    list.contains(&folder.as_str())
}

fn file_type(file: &str, parent: Option<&str>, middle: &[&str]) -> MediaFileType {
    use MediaFileType as T;

    // 1. extras folders
    if middle.iter().any(|f| folder_in(EXTRA_FOLDERS, f)) {
        return T::Extra;
    }

    let (base, ext) = split_extension(file);

    // 2. metadata files
    match ext.as_str() {
        "nfo" => return T::Nfo,
        "vsmeta" => return T::Vsmeta,
        "xml" => {
            let lower = base.to_lowercase();
            if lower.ends_with("-mediainfo") || lower.ends_with(".mediainfo") {
                return T::Mediainfo;
            }
        }
        _ => {}
    }

    // 3. artwork
    if has(ARTWORK_EXTENSIONS, &ext) {
        return image_type(file, base, parent);
    }

    // 4. theme music
    if THEME.is_match(base) {
        return T::Theme;
    }

    // 5. audio and subtitles
    if has(AUDIO_EXTENSIONS, &ext) {
        return T::Audio;
    }
    if has(SUBTITLE_EXTENSIONS, &ext) {
        return T::Subtitle;
    }

    // 6. extras by name
    if base.contains(".EXTRAS.") || EXTRA_SUFFIX.is_match(base) || EXTRA_BRACKET.is_match(base) {
        return T::Extra;
    }

    // 7. video
    if has(VIDEO_EXTENSIONS, &ext) {
        let lower = base.to_lowercase();
        let in_folder = |list: &[&str]| parent.is_some_and(|p| folder_in(list, p));
        if TRAILER.is_match(base)
            || lower == "movie-trailer"
            || lower == "trailer"
            || in_folder(TRAILER_FOLDERS)
        {
            return T::Trailer;
        }
        if SAMPLE.is_match(base) || lower == "sample" || in_folder(&["sample"]) {
            return T::Sample;
        }
        return T::Video;
    }

    // 8. a disc folder used as a media file
    let upper = file.to_uppercase();
    if ["BDMV", "VIDEO_TS", "HVDVD_TS"]
        .iter()
        .any(|disc| upper.ends_with(disc))
    {
        return T::Video;
    }

    // 9. the rest
    match ext.as_str() {
        "txt" => T::Text,
        "bif" => T::Bif,
        _ => T::Unknown,
    }
}

fn image_type(file: &str, base: &str, parent: Option<&str>) -> MediaFileType {
    use MediaFileType as T;
    let rules = &*IMAGE_RULES;
    let parent_lower = parent.map(str::to_lowercase).unwrap_or_default();
    let in_folder = |suffix: &str| parent_lower.ends_with(suffix);

    if rules.movieset.is_match(file) {
        return T::Graphic;
    }
    if rules.season_poster.is_match(file) {
        return T::SeasonPoster;
    }
    if rules.season_fanart.is_match(file) {
        return T::SeasonFanart;
    }
    if rules.season_banner.is_match(file) {
        return T::SeasonBanner;
    }
    if rules.season_thumb.is_match(file) {
        return T::SeasonThumb;
    }
    if rules.poster.is_match(file) {
        return T::Poster;
    }
    if rules.extrafanart.is_match(file) {
        return T::Extrafanart;
    }
    if rules.fanart.is_match(file) {
        return if in_folder("extrafanart") {
            T::Extrafanart
        } else {
            T::Fanart
        };
    }
    if rules.banner.is_match(file) {
        return T::Banner;
    }
    if rules.thumb.is_match(file) {
        return if in_folder("extrathumbs") {
            T::Extrathumb
        } else {
            T::Thumb
        };
    }
    if rules.clearart.is_match(file) {
        return T::Clearart;
    }
    if rules.clearlogo.is_match(file) {
        return T::Clearlogo;
    }
    if rules.disc.is_match(file) {
        return T::Disc;
    }
    if rules.characterart.is_match(file) || in_folder("characterart") {
        return T::Characterart;
    }
    if rules.keyart.is_match(file) {
        return T::Keyart;
    }
    if rules.squareart.is_match(file) {
        return T::Squareart;
    }
    // A file named like its folder is the poster.
    if parent.is_some_and(|p| p.eq_ignore_ascii_case(base)) {
        return T::Poster;
    }
    T::Graphic
}

fn is_disc_file(file: &str, segments: &[&str]) -> bool {
    let lower = file.to_lowercase();
    DISC_DVD.is_match(&lower)
        || DISC_HDDVD.is_match(&lower)
        || DISC_BLURAY.is_match(&lower)
        || segments.iter().any(|segment| {
            ["video_ts", "hvdvd_ts", "bdmv"]
                .iter()
                .any(|disc| segment.eq_ignore_ascii_case(disc))
        })
}

fn is_main_disc_identifier(file: &str) -> bool {
    let lower = file.to_lowercase();
    ["video_ts.ifo", "index.bdmv", "hv000i01.ifo"].contains(&lower.as_str())
}

fn regex(pattern: &str) -> Regex {
    Regex::new(pattern).expect("built-in classification pattern is valid")
}

const EXTRA_WORDS: &str =
    "extras?|behindthescenes?|deleted|featurettes?|interviews?|scenes?|shorts?|others?|bloopers?";
const ART_EXT: &str = "jpg|jpeg|png|tbn|gif|bmp|webp";

static TRICKPLAY_BIF: LazyLock<Regex> = LazyLock::new(|| regex(r"(?i)(-\d+-\d+)\.bif$"));
static THEME: LazyLock<Regex> =
    LazyLock::new(|| regex(r"(?i)^(?:.*[_.-]+)?(?:theme|soundtrack)\d*$"));
static EXTRA_SUFFIX: LazyLock<Regex> =
    LazyLock::new(|| regex(&format!(r"(?i)^.*[_.-](?:{EXTRA_WORDS})\d*$")));
static EXTRA_BRACKET: LazyLock<Regex> =
    LazyLock::new(|| regex(&format!(r"(?i)^.*[\[(](?:{EXTRA_WORDS})[)\]].*$")));
static TRAILER: LazyLock<Regex> =
    LazyLock::new(|| regex(r"(?i)^.*[\[\]()_.-]+trailer[\[\]()_.-]?\d*$"));
static SAMPLE: LazyLock<Regex> = LazyLock::new(|| regex(r"(?i)^.*[\[\]()_.-]+sample[\[\]()]?$"));
static DISC_DVD: LazyLock<Regex> =
    LazyLock::new(|| regex(r"^(?:video_ts|vts_\d\d_\d)\.(?:vob|bup|ifo)$"));
static DISC_HDDVD: LazyLock<Regex> = LazyLock::new(|| {
    regex(r"^(?:hv\d{3}[imt]\d{2}|hv[as]\d{5}|title\d{3})\.(?:evo|bup|ifo|vti|map)$")
});
static DISC_BLURAY: LazyLock<Regex> = LazyLock::new(|| {
    regex(r"^(?:index\.bdmv|movieobject\.bdmv|\d{5}\.m2ts|\d{5}\.clpi|\d{5}\.mpls)$")
});

struct ImageRules {
    movieset: Regex,
    season_poster: Regex,
    season_fanart: Regex,
    season_banner: Regex,
    season_thumb: Regex,
    poster: Regex,
    extrafanart: Regex,
    fanart: Regex,
    banner: Regex,
    thumb: Regex,
    clearart: Regex,
    clearlogo: Regex,
    disc: Regex,
    characterart: Regex,
    keyart: Regex,
    squareart: Regex,
}

static IMAGE_RULES: LazyLock<ImageRules> = LazyLock::new(|| {
    let season = r"season(?:[0-9]{1,6}|-specials|-all)";
    // `(.+[_.-])?` lets a name carry a prefix such as `Movie (1999)-` before the keyword.
    let named = |words: &str| regex(&format!(r"(?i)^(?:.+[_.-])?(?:{words})\.(?:{ART_EXT})$"));
    ImageRules {
        movieset: regex(&format!(
            r"(?i)^movieset-(?:poster|fanart|backdrop|banner|disc|discart|logo|clearlogo|clearart|thumb)\.(?:{ART_EXT})$"
        )),
        season_poster: regex(&format!(r"(?i){season}(?:-poster)?\.(?:{ART_EXT})$")),
        season_fanart: regex(&format!(
            r"(?i){season}-(?:fanart|backdrop|background)\.(?:{ART_EXT})$"
        )),
        season_banner: regex(&format!(r"(?i){season}-banner\.(?:{ART_EXT})$")),
        season_thumb: regex(&format!(
            r"(?i){season}-(?:thumb|landscape)\.(?:{ART_EXT})$"
        )),
        poster: named("poster|folder|movie|cover"),
        extrafanart: regex(&format!(
            r"(?i)^(?:.+[_.-])?(?:fanart|backdrop|background)[0-9]+\.(?:{ART_EXT})$"
        )),
        fanart: named("backdrop|fanart|background"),
        banner: named("banner"),
        thumb: regex(&format!(
            r"(?i)^(?:.+[_.-])?(?:thumb|landscape)[0-9]{{0,2}}\.(?:{ART_EXT})$"
        )),
        clearart: named("clearart"),
        clearlogo: named("clearlogo|logo"),
        disc: named("discart|disc"),
        characterart: regex(&format!(
            r"(?i)^(?:.+[_.-])?characterart[0-9]{{0,2}}\.(?:{ART_EXT})$"
        )),
        keyart: named("keyart"),
        squareart: named("square|squareart|backgroundsquare"),
    }
});

/// MPEG transport stream packets start with this sync byte.
const TS_SYNC: u8 = 0x47;

/// Whether a file that [`classify`] calls a video really holds one, for the extensions other
/// files share: `.ts` is both an MPEG transport stream and TypeScript source code, and a code
/// project (`node_modules` alone) holds thousands of the latter. A `.d.ts` file is never video;
/// any other `.ts` file must start like a transport stream (the sync byte `0x47` every 188
/// bytes, or every 192 bytes behind a 4-byte time code as in `.m2ts`). An empty or unreadable
/// file is given the benefit of the doubt. Every other extension is not read and passes.
pub fn is_video_content(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if !name.ends_with(".ts") {
        return true;
    }
    if name.ends_with(".d.ts") {
        return false;
    }
    let mut head = [0u8; 200];
    let Ok(read) = File::open(path).and_then(|mut file| read_up_to(&mut file, &mut head)) else {
        return true;
    };
    looks_like_transport_stream(&head[..read])
}

/// Whether `head`, the first bytes of a file, looks like an MPEG transport stream.
pub fn looks_like_transport_stream(head: &[u8]) -> bool {
    if head.is_empty() {
        return true;
    }
    let synced = |offset: usize, packet: usize| {
        head.get(offset) == Some(&TS_SYNC)
            && head
                .get(offset + packet)
                .is_none_or(|&byte| byte == TS_SYNC)
    };
    synced(0, 188) || synced(4, 192)
}

fn read_up_to(file: &mut File, buffer: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buffer.len() {
        match file.read(&mut buffer[filled..])? {
            0 => break,
            n => filled += n,
        }
    }
    Ok(filled)
}

#[cfg(test)]
mod tests;
