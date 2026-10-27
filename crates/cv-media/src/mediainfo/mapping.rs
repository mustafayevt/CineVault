//! Maps the raw MediaInfo fields of a [`MediaSnapshot`] to CineVault's stream model.
//!
//! The fields extracted:
//!
//! * **video:** codec, resolution (halved for side-by-side and top-and-bottom multi-view),
//!   scan type, frame rate, bit depth, bit rate, aspect ratio from the pixel aspect ratio;
//! * **HDR:** Dolby Vision, HDR10+, HDR10 and HLG from the `HDR_Format*` fields, with the
//!   colour description as the fallback;
//! * **3D:** multi-view layout, then the filename, then the frame-size heuristics;
//! * **audio:** codec variants (DTS-X, DTS-HD MA, TrueHD Atmos, E-AC-3 Atmos ...), channels,
//!   bit rate, bit depth, language, title and flags;
//! * **subtitles:** codec, language, title and flags (default, forced, SDH).
//!
//! Nothing here reads a file: the input is the snapshot, so the same code serves the live
//! library, the XML sidecar cache and recorded fixtures.

use std::collections::BTreeSet;
use std::sync::LazyLock;

use cv_domain::{AudioStream, HdrDetails, MediaFile, StreamFlag, SubtitleStream, Video3dFormat};
use regex::Regex;

use super::snapshot::{MediaSnapshot, StreamInfo, StreamKind};

/// A duration given as a bare number above this is milliseconds, below it seconds. MediaInfo's
/// library reports milliseconds, its XML export reports seconds with decimals.
const MILLISECONDS_ABOVE: f64 = 25_000.0;

static SDH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(sdh|cc|hearing[-\s]?impaired)\b").expect("SDH regex"));
static COMMENTARY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bcomment(ary|aries)?\b").expect("commentary regex"));
static DUB: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(dub|dubbed)\b").expect("dub regex"));
static ORIGINAL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\boriginal\b").expect("original regex"));
static DESCRIPTION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(audio[-\s]?description|descriptive|visually[-\s]?impaired)\b")
        .expect("description regex")
});
static KARAOKE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bkaraoke\b").expect("karaoke regex"));

// ---------------------------------------------------------------------------------------------
// Whole file
// ---------------------------------------------------------------------------------------------

/// Fills the technical fields of `file` from `snapshot`, replacing what an earlier read left.
///
/// Set: container format, duration, overall bit rate, title tag, the video fields, HDR formats,
/// 3D layout, audio streams and embedded subtitles. Not touched: the name, size, stacking,
/// checksums, ISO flag and the aspect-ratio detector's secondary ratio.
///
/// `file.filename` is used as a hint for the 3D layout when MediaInfo reports none.
pub fn apply_snapshot(file: &mut MediaFile, snapshot: &MediaSnapshot) {
    file.video_codec.clear();
    file.codec_id.clear();
    file.exact_video_format.clear();
    file.video_3d = Video3dFormat::None;
    file.width = 0;
    file.height = 0;
    file.aspect_ratio = 0.0;
    file.pixel_aspect_ratio = 0.0;
    file.video_bitrate_kbps = 0;
    file.bit_depth = 0;
    file.frame_rate = 0.0;
    file.hdr_formats.clear();
    file.hdr_details = HdrDetails::default();
    file.audio_streams.clear();
    file.subtitles.clear();

    let general = snapshot.general();
    file.container_format = general
        .map(|g| container_format(g, &file.filename))
        .unwrap_or_default();
    file.overall_bitrate_kbps = general.map_or(0, overall_bitrate_kbps);
    file.title = general
        .and_then(|g| first_of(g, &["Title", "Movie"]))
        .unwrap_or_default()
        .to_owned();

    let video = main_video(snapshot);
    if let Some(video) = video {
        apply_video(file, video);
    }
    file.duration_secs = duration_secs(general, video);

    file.audio_streams = snapshot
        .streams(StreamKind::Audio)
        .iter()
        .map(map_audio_stream)
        .collect();
    file.subtitles = snapshot
        .streams(StreamKind::Text)
        .iter()
        .map(map_subtitle_stream)
        .collect();

    // A file MediaInfo could open and find streams in counts as read, even without a format name.
    if file.container_format.is_empty() && snapshot.has_media_streams() {
        file.container_format = "unknown".to_owned();
    }
}

/// The first video stream that is a moving picture, not an attached cover (some containers list
/// a JPEG or PNG cover as a video stream).
fn main_video(snapshot: &MediaSnapshot) -> Option<&StreamInfo> {
    let streams = snapshot.streams(StreamKind::Video);
    streams
        .iter()
        .find(|s| !matches!(s.get("Format"), Some("JPEG" | "PNG" | "BMP" | "GIF")))
        .or_else(|| streams.first())
}

fn apply_video(file: &mut MediaFile, video: &StreamInfo) {
    file.video_codec = video_codec(video);
    file.codec_id = video.get("CodecID").unwrap_or_default().to_owned();

    let mut width = video.get_u64("Width").unwrap_or(0) as u32;
    let mut height = video.get_u64("Height").unwrap_or(0) as u32;
    let (video_3d, halve) = detect_3d(video, width, height, &file.filename);
    match halve {
        Halve::Width => width /= 2,
        Halve::Height => height /= 2,
        Halve::Neither => {}
    }
    file.video_3d = video_3d;
    file.width = width;
    file.height = height;

    let interlaced = video.get("ScanType").is_some_and(|scan| {
        scan.to_ascii_lowercase().contains("interlaced") || matches!(scan, "MBAFF" | "PAFF")
    });
    file.exact_video_format = if height == 0 {
        String::new()
    } else {
        format!("{height}{}", if interlaced { 'i' } else { 'p' })
    };

    file.pixel_aspect_ratio = video.get_f64("PixelAspectRatio").unwrap_or(0.0) as f32;
    file.aspect_ratio = aspect_ratio(video, width, height, halve != Halve::Neither);
    file.video_bitrate_kbps = bitrate_kbps(video, false);
    file.bit_depth = video.get_u64("BitDepth").unwrap_or(0) as u32;
    file.frame_rate = ["FrameRate", "FrameRate_Original"]
        .into_iter()
        .filter_map(|name| video.get_f64(name))
        .find(|rate| *rate > 0.0)
        .unwrap_or(0.0);
    file.hdr_formats = hdr_formats(video);
    file.hdr_details = hdr_details(video);
}

/// Display aspect ratio: width times pixel aspect ratio over height. Without a pixel aspect
/// ratio MediaInfo's own display aspect ratio is used (unless the size was halved for 3D), then
/// the plain frame shape.
fn aspect_ratio(video: &StreamInfo, width: u32, height: u32, halved: bool) -> f32 {
    let shape = if height == 0 {
        0.0
    } else {
        f64::from(width) / f64::from(height)
    };
    if let Some(par) = video.get_f64("PixelAspectRatio").filter(|p| *p > 0.0) {
        return (shape * par) as f32;
    }
    if !halved && let Some(dar) = video.get_f64("DisplayAspectRatio").filter(|d| *d > 0.0) {
        return dar as f32;
    }
    shape as f32
}

/// The container name: `Menu_Format_List` (disc structures) or `Format`, with the disc names
/// spelled out; replaced by a file extension from the list MediaInfo gives for the container
/// (`mkv mk3d mka mks`) when there is one: the file's own extension if it is in the list, else
/// the first. (The first alone would call every MPEG-4 file `braw`.)
fn container_format(general: &StreamInfo, filename: &str) -> String {
    let named = first_of(general, &["Menu_Format_List", "Format"])
        .map(|name| match name.trim() {
            "BDAV" => "Blu-Ray Video".to_owned(),
            "DVD-Video" => "DVD Video".to_owned(),
            other => other.to_owned(),
        })
        .unwrap_or_default();
    let own = filename
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase());
    first_of(general, &["Format/Extensions", "Codec/Extensions"])
        .and_then(|list| {
            let mut extensions = list.split_whitespace().map(str::to_ascii_lowercase);
            let first = extensions.next()?;
            Some(match own {
                Some(own) if first == own || extensions.any(|e| e == own) => own,
                _ => first,
            })
        })
        .unwrap_or(named)
}

fn overall_bitrate_kbps(general: &StreamInfo) -> u32 {
    first_of(general, &["OverallBitRate", "OverallBitRate_Maximum"])
        .and_then(first_number)
        .map_or(0, kbps)
}

/// Runtime in whole seconds: `Duration/String3` (h:m:s) first, else `Duration`, from the general
/// section first and the video stream second.
fn duration_secs(general: Option<&StreamInfo>, video: Option<&StreamInfo>) -> u32 {
    for stream in general.into_iter().chain(video) {
        if let Some(text) = stream.get("Duration/String3")
            && let Some(seconds) = parse_clock(text)
        {
            return seconds.round() as u32;
        }
        if let Some(value) = stream.get("Duration").and_then(first_number)
            && value > 0.0
        {
            let seconds = if value > MILLISECONDS_ABOVE {
                value / 1000.0
            } else {
                value
            };
            return seconds.round() as u32;
        }
    }
    0
}

/// `hh:mm:ss` or `hh:mm:ss.mmm` to seconds.
fn parse_clock(text: &str) -> Option<f64> {
    let mut parts = text.trim().split(':');
    let hours: f64 = parts.next()?.parse().ok()?;
    let minutes: f64 = parts.next()?.parse().ok()?;
    let seconds: f64 = parts.next()?.parse().ok()?;
    parts
        .next()
        .is_none()
        .then_some(hours * 3600.0 + minutes * 60.0 + seconds)
}

// ---------------------------------------------------------------------------------------------
// Video codec, 3D, HDR
// ---------------------------------------------------------------------------------------------

/// The video codec name: the container's codec hint for MPEG-4 Visual (`DivX 5`, `XviD`),
/// `MPEG-1`/`MPEG-2` from the format version, `VC-1`, else MediaInfo's format name (`AVC`,
/// `HEVC`, `AV1`, `VP9` ...).
#[must_use]
pub fn video_codec(video: &StreamInfo) -> String {
    let codec_id = video.get("CodecID").unwrap_or_default();
    let hint = video.get("CodecID/Hint");
    if codec_id.eq_ignore_ascii_case("xvid") || hint.is_some_and(|h| h.eq_ignore_ascii_case("xvid"))
    {
        return "XviD".to_owned();
    }
    let Some(format) = video.get("Format") else {
        return hint.unwrap_or(codec_id).to_owned();
    };
    if format.starts_with("VC-1") || matches!(codec_id, "WVC1" | "WMVA" | "WMV3") {
        return "VC-1".to_owned();
    }
    match format {
        "MPEG Video" => match video.get("Format_Version").map(str::trim) {
            Some("Version 1") => "MPEG-1".to_owned(),
            Some("Version 2") => "MPEG-2".to_owned(),
            _ => format.to_owned(),
        },
        "MPEG-4 Visual" => hint.unwrap_or(format).to_owned(),
        other => other.to_owned(),
    }
}

/// Which side of the frame holds one eye and must be halved to get the real picture size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Halve {
    Neither,
    Width,
    Height,
}

/// Finds the 3D layout of a video, and whether its reported size covers both eyes.
///
/// In order:
/// 1. the multi-view fields (`MultiView_Count` 2 with a layout): a side-by-side or
///    top-and-bottom layout halves the size, any other layout is MVC;
/// 2. a layout token in `filename` (`3D HSBS`, `3D.TAB`, ...), which is the only way to tell the
///    half variants from the full ones;
/// 3. frame-size heuristics: 3840 wide and at most 1080 high is side by side, 1920x2160 is top
///    and bottom;
/// 4. a bare `3D` in the filename.
#[must_use]
pub fn detect_3d(
    video: &StreamInfo,
    width: u32,
    height: u32,
    filename: &str,
) -> (Video3dFormat, Halve) {
    if video.get_u64("MultiView_Count") == Some(2) {
        let layout = video
            .get("MultiView_Layout")
            .map(|l| l.to_ascii_lowercase().replace(['-', '_'], " "))
            .unwrap_or_default();
        return if layout.contains("side by side") {
            (Video3dFormat::SideBySide, Halve::Width)
        } else if layout.contains("top") && layout.contains("bottom") {
            (Video3dFormat::TopAndBottom, Halve::Height)
        } else {
            (Video3dFormat::Mvc, Halve::Neither)
        };
    }
    let named = video_3d_from_name(filename);
    if !matches!(named, Video3dFormat::None | Video3dFormat::ThreeD) {
        return (named, Halve::Neither);
    }
    if width == 3840 && (1..=1080).contains(&height) {
        return (Video3dFormat::SideBySide, Halve::Neither);
    }
    if width == 1920 && height == 2160 {
        return (Video3dFormat::TopAndBottom, Halve::Neither);
    }
    (named, Halve::Neither)
}

/// The 3D layout a filename announces: `HSBS`, `HTAB`/`HOU`, `SBS`, `TAB`/`OU`, `MVC`, or a bare
/// `3D`. The short names (`SBS`, `TAB`, `OU`, `MVC`) count only next to a `3D` token or after
/// `half`; `HSBS`, `HTAB` and `HOU` are unambiguous on their own.
#[must_use]
pub fn video_3d_from_name(filename: &str) -> Video3dFormat {
    let lower = filename.to_ascii_lowercase();
    let tokens: Vec<&str> = lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .collect();
    let has_3d = tokens.contains(&"3d");
    for (index, token) in tokens.iter().enumerate() {
        let half = index > 0 && tokens[index - 1] == "half";
        let layout = match *token {
            "hsbs" | "halfsbs" => Video3dFormat::HalfSideBySide,
            "htab" | "hou" | "halftab" | "halfou" => Video3dFormat::HalfTopAndBottom,
            "sbs" if half => Video3dFormat::HalfSideBySide,
            "tab" | "ou" if half => Video3dFormat::HalfTopAndBottom,
            "sbs" if has_3d => Video3dFormat::SideBySide,
            "tab" | "ou" if has_3d => Video3dFormat::TopAndBottom,
            "mvc" if has_3d => Video3dFormat::Mvc,
            _ => continue,
        };
        return layout;
    }
    if has_3d {
        Video3dFormat::ThreeD
    } else {
        Video3dFormat::None
    }
}

/// The HDR formats of a video, in the order `Dolby Vision`, `HDR10+` or `HDR10`, `HLG`.
///
/// Read from `HDR_Format`, `HDR_Format_Commercial` and `HDR_Format_Compatibility` (each may list
/// several layers separated by ` / `). HDR10+ replaces HDR10, since it carries the HDR10 base.
/// Without any of those fields the colour description decides: a hybrid log-gamma transfer is
/// `HLG`; a PQ transfer or BT.2100 primaries is a plain `HDR`.
#[must_use]
pub fn hdr_formats(video: &StreamInfo) -> Vec<String> {
    let mut seen = HdrSeen::default();
    let fields = [
        video.get("HDR_Format"),
        video.get("HDR_Format_Commercial"),
        video.get("HDR_Format_Compatibility"),
    ];
    let mut any_field = fields.iter().any(Option::is_some);
    for field in fields {
        seen.scan(field);
    }
    if !any_field {
        // Older libraries only had the combined string.
        let combined = first_of(video, &["HDR_Format/String", "HDR_Format_String"]);
        any_field = combined.is_some();
        seen.scan(combined);
    }

    let transfer = video
        .get("transfer_characteristics")
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if transfer.contains("hlg") || transfer.contains("b67") {
        seen.hlg = true;
    }

    let mut formats = Vec::new();
    if seen.dolby_vision {
        formats.push("Dolby Vision".to_owned());
    }
    if seen.hdr10_plus {
        formats.push("HDR10+".to_owned());
    } else if seen.hdr10 {
        formats.push("HDR10".to_owned());
    }
    if seen.hlg {
        formats.push("HLG".to_owned());
    }
    if formats.is_empty() && !any_field {
        let pq = transfer.contains("pq") || transfer.contains("2084");
        let bt2100 = video
            .get("colour_primaries")
            .is_some_and(|p| p.contains("2100"));
        if pq || bt2100 {
            formats.push("HDR".to_owned());
        }
    }
    formats
}

#[derive(Default)]
struct HdrSeen {
    dolby_vision: bool,
    hdr10_plus: bool,
    hdr10: bool,
    hlg: bool,
}

impl HdrSeen {
    fn scan(&mut self, text: Option<&str>) {
        let Some(text) = text else { return };
        for token in text.split(['/', ',']) {
            let token = token.trim().to_ascii_lowercase();
            if token.contains("dolby vision") {
                self.dolby_vision = true;
            } else if token.contains("2094") || token.starts_with("hdr10+") {
                self.hdr10_plus = true;
            } else if token.contains("2086") || token.starts_with("hdr10") {
                self.hdr10 = true;
            } else if token.contains("hlg") {
                self.hlg = true;
            }
        }
    }
}

/// The Dolby Vision profile number (`8` for `dvhe.08`), when the video has a Dolby Vision layer.
#[must_use]
pub fn dolby_vision_profile(video: &StreamInfo) -> Option<u8> {
    // Layers are listed in the same order in every `HDR_Format_*` field; Dolby Vision is the
    // layer whose format says so.
    let position = video
        .get("HDR_Format")?
        .split('/')
        .position(|layer| layer.to_ascii_lowercase().contains("dolby vision"))?;
    let profile = video
        .get("HDR_Format_Profile")?
        .split('/')
        .nth(position)?
        .trim();
    profile.rsplit('.').next()?.parse().ok()
}

/// The Dolby Vision profile and what is under it, the mastering display and the light levels of a
/// video, from `HDR_Format_Profile`, `HDR_Format_Compatibility`, `MasteringDisplay_ColorPrimaries`,
/// `MasteringDisplay_Luminance`, `MaxCLL` and `MaxFALL`. Whatever the file does not say stays
/// empty.
///
/// The profile is written the way people name it: `8.1` is profile 8 with an HDR10 base layer,
/// `8.2` with an SDR one and `8.4` with an HLG one (likewise profile 10); the other profiles are
/// just their number.
#[must_use]
pub fn hdr_details(video: &StreamInfo) -> HdrDetails {
    let mut details = HdrDetails::default();
    if let Some(position) = dolby_vision_position(video) {
        let layer = |name: &str| -> Option<String> {
            let parts: Vec<&str> = video.get(name)?.split('/').map(str::trim).collect();
            parts
                .get(position)
                .or_else(|| parts.last())
                .map(|part| (*part).to_owned())
        };
        let compatibility = layer("HDR_Format_Compatibility")
            .map(|text| compatibility_name(&text))
            .unwrap_or_default();
        if let Some(profile) = layer("HDR_Format_Profile").and_then(|text| profile_number(&text)) {
            let id = match (profile, compatibility.as_str()) {
                (8 | 10, "HDR10" | "HDR10+") => ".1",
                (8 | 10, "SDR") => ".2",
                (8 | 10, "HLG") => ".4",
                _ => "",
            };
            details.dolby_vision_profile = format!("{profile}{id}");
        }
        details.dolby_vision_compatibility = compatibility;
    }
    details.mastering_primaries = video
        .get("MasteringDisplay_ColorPrimaries")
        .map(|p| p.trim().to_owned())
        .unwrap_or_default();
    if let Some(luminance) = video.get("MasteringDisplay_Luminance") {
        details.mastering_min_nits = labelled_number(luminance, "min");
        details.mastering_max_nits = labelled_number(luminance, "max");
    }
    details.max_cll_nits = leading_number(video.get("MaxCLL")).map(|n| n.round() as u32);
    details.max_fall_nits = leading_number(video.get("MaxFALL")).map(|n| n.round() as u32);
    details
}

/// Which `/`-separated layer of the `HDR_Format*` fields is Dolby Vision.
fn dolby_vision_position(video: &StreamInfo) -> Option<usize> {
    video
        .get("HDR_Format")?
        .split('/')
        .position(|layer| layer.to_ascii_lowercase().contains("dolby vision"))
}

/// The profile number in `dvhe.08`, `dvhe.08.06` or `Profile 8`.
fn profile_number(text: &str) -> Option<u8> {
    let lower = text.trim().to_ascii_lowercase();
    let digits = match lower.split_once('.') {
        Some((_, rest)) if lower.starts_with("dv") => rest,
        _ => lower.trim_start_matches(|c: char| !c.is_ascii_digit()),
    };
    let number: String = digits.chars().take_while(char::is_ascii_digit).collect();
    number.parse().ok()
}

/// The base layer's format, as the names the library uses (`HDR10`, `HDR10+`, `HLG`, `SDR`).
fn compatibility_name(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    if lower.contains("hdr10+") || lower.contains("2094") {
        "HDR10+".to_owned()
    } else if lower.contains("hdr10") || lower.contains("2086") {
        "HDR10".to_owned()
    } else if lower.contains("hlg") {
        "HLG".to_owned()
    } else if lower.contains("sdr") {
        "SDR".to_owned()
    } else {
        text.trim().to_owned()
    }
}

/// The number after `label:` in `min: 0.0050 cd/m2, max: 1000 cd/m2`.
fn labelled_number(text: &str, label: &str) -> Option<f64> {
    let lower = text.to_ascii_lowercase();
    let start = lower.find(&format!("{label}:"))? + label.len() + 1;
    leading_number(Some(lower[start..].trim_start()))
}

/// The number a text starts with (`1000 cd/m2`).
fn leading_number(text: Option<&str>) -> Option<f64> {
    let text = text?.trim();
    let end = text
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(text.len());
    text[..end].parse().ok().filter(|n: &f64| *n >= 0.0)
}

// ---------------------------------------------------------------------------------------------
// Audio
// ---------------------------------------------------------------------------------------------

/// Maps one MediaInfo audio stream.
#[must_use]
pub fn map_audio_stream(audio: &StreamInfo) -> AudioStream {
    let mut flags = BTreeSet::new();
    set_if_yes(&mut flags, audio, "Default", StreamFlag::Default);
    set_if_yes(&mut flags, audio, "Forced", StreamFlag::Forced);
    add_service_kind_flags(&mut flags, audio);
    let title = audio.get("Title").unwrap_or_default().to_owned();
    for (pattern, flag) in [
        (&*COMMENTARY, StreamFlag::Comment),
        (&*DUB, StreamFlag::Dub),
        (&*ORIGINAL, StreamFlag::Original),
        (&*DESCRIPTION, StreamFlag::VisualImpaired),
        (&*KARAOKE, StreamFlag::Karaoke),
    ] {
        if pattern.is_match(&title) {
            flags.insert(flag);
        }
    }
    AudioStream {
        codec: audio_codec(audio),
        codec_id: audio.get("CodecID").unwrap_or_default().to_owned(),
        language: language(audio),
        title,
        flags,
        channels: audio_channels(audio),
        bitrate_kbps: bitrate_kbps(audio, true),
        bit_depth: audio.get_u64("BitDepth").unwrap_or(0) as u32,
        sample_rate_hz: audio
            .get_u64("SamplingRate")
            .and_then(|rate| u32::try_from(rate).ok())
            .unwrap_or(0),
    }
}

/// The audio codec name, with the variants told apart:
///
/// | MediaInfo | Name |
/// |---|---|
/// | DTS with the `XLL` and `X` features | `DTS-X` (`DTS-X IMAX` with the `IMAX` feature) |
/// | DTS with `XLL` or profile `MA` | `DTS-HD MA` |
/// | DTS with `XBR` or profile `HRA` | `DTS-HD HRA` |
/// | DTS with `X96` / `ES` or `XCh` / profile `Express` | `DTS 96/24` / `DTS-ES` / `DTS Express` |
/// | `MLP FBA` | `TrueHD`, or `TrueHD Atmos` (16-channel form or an Atmos commercial name) |
/// | `E-AC-3` | `E-AC-3`, or `E-AC-3 Atmos` (joint object coding or an Atmos commercial name) |
/// | `AC-3` | `AC3`, or `AC3 EX` |
/// | `MPEG Audio` | `MP3`, `MP2` or `MP1` by layer |
///
/// Other formats keep MediaInfo's name (`AAC`, `FLAC`, `Opus`, `PCM`, `Vorbis` ...).
#[must_use]
pub fn audio_codec(audio: &StreamInfo) -> String {
    let Some(format) = audio.get("Format") else {
        return audio.get("CodecID").unwrap_or_default().to_owned();
    };
    let commercial = first_of(audio, &["Format_Commercial_IfAny", "Format_Commercial"])
        .unwrap_or_default()
        .to_ascii_lowercase();
    let features_text = audio
        .get("Format_AdditionalFeatures")
        .unwrap_or_default()
        .to_ascii_uppercase();
    let features: Vec<&str> = features_text.split_whitespace().collect();
    let profile = audio
        .get("Format_Profile")
        .unwrap_or_default()
        .to_ascii_uppercase();
    let atmos = commercial.contains("atmos");

    match format {
        "DTS" | "DTS-HD" | "DTS XLL" => dts_variant(&commercial, &features, &profile),
        f if f.starts_with("MLP FBA") || f == "TrueHD" => {
            if atmos || f.contains("16-ch") || features.contains(&"16-CH") {
                "TrueHD Atmos".to_owned()
            } else {
                "TrueHD".to_owned()
            }
        }
        "E-AC-3" => {
            if atmos || features.contains(&"JOC") {
                "E-AC-3 Atmos".to_owned()
            } else {
                "E-AC-3".to_owned()
            }
        }
        "AC-3" => {
            if commercial.contains("digital ex") || features.contains(&"EX") {
                "AC3 EX".to_owned()
            } else {
                "AC3".to_owned()
            }
        }
        "MPEG Audio" => match profile.as_str() {
            "LAYER 3" => "MP3".to_owned(),
            "LAYER 2" => "MP2".to_owned(),
            "LAYER 1" => "MP1".to_owned(),
            _ => format.to_owned(),
        },
        other => other.to_owned(),
    }
}

fn dts_variant(commercial: &str, features: &[&str], profile: &str) -> String {
    let has = |name: &str| features.contains(&name);
    let profile_has = |name: &str| profile.split('/').any(|p| p.trim() == name);
    let object_audio =
        commercial.contains("dts:x") || commercial.contains("dts-x") || (has("XLL") && has("X"));
    if object_audio {
        return if has("IMAX") { "DTS-X IMAX" } else { "DTS-X" }.to_owned();
    }
    let name = if has("XLL") || profile_has("MA") || commercial.contains("master audio") {
        "DTS-HD MA"
    } else if has("XBR") || profile_has("HRA") || commercial.contains("high resolution") {
        "DTS-HD HRA"
    } else if has("X96") || profile_has("96/24") {
        "DTS 96/24"
    } else if has("ES") || has("XCH") || has("XXCH") || profile_has("ES") {
        "DTS-ES"
    } else if profile_has("EXPRESS") || profile_has("LBR") {
        "DTS Express"
    } else {
        "DTS"
    };
    name.to_owned()
}

/// Channels: the larger of `Channel(s)` and `Channel(s)_Original`. A value such as `16 objects`
/// counts audio objects, not channels, and is ignored.
fn audio_channels(audio: &StreamInfo) -> u32 {
    ["Channel(s)", "Channel(s)_Original"]
        .into_iter()
        .filter_map(|name| audio.get(name))
        .flat_map(|text| text.split('/'))
        .filter(|part| !part.to_ascii_lowercase().contains("object"))
        .filter_map(|part| first_number(part.trim()))
        .map(|n| n.round() as u32)
        .max()
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------------------------
// Subtitles
// ---------------------------------------------------------------------------------------------

/// Maps one MediaInfo text stream (an embedded subtitle).
#[must_use]
pub fn map_subtitle_stream(text: &StreamInfo) -> SubtitleStream {
    let mut flags = BTreeSet::new();
    set_if_yes(&mut flags, text, "Default", StreamFlag::Default);
    set_if_yes(&mut flags, text, "Forced", StreamFlag::Forced);
    add_service_kind_flags(&mut flags, text);
    let title = text.get("Title").unwrap_or_default().to_owned();
    if SDH.is_match(&title) {
        flags.insert(StreamFlag::HearingImpaired);
    }
    SubtitleStream {
        codec: subtitle_codec(text),
        codec_id: text.get("CodecID").unwrap_or_default().to_owned(),
        language: language(text),
        title,
        flags,
    }
}

/// The subtitle format: `SubRip` for plain UTF-8 text (how containers store `.srt`), else the
/// codec hint or MediaInfo's format name (`PGS`, `VobSub`, `ASS`, `SSA`, `Timed Text` ...).
fn subtitle_codec(text: &StreamInfo) -> String {
    let format = text.get("CodecID/Hint").or_else(|| text.get("Format"));
    match format {
        Some("UTF-8" | "SRT") => "SubRip".to_owned(),
        Some(other) => other.to_owned(),
        None => text.get("CodecID").unwrap_or_default().to_owned(),
    }
}

// ---------------------------------------------------------------------------------------------
// Shared
// ---------------------------------------------------------------------------------------------

/// A stream's language as a lower-case ISO 639 code, with an upper-case region when MediaInfo
/// reports one (`pt-BR`). `Language` is tried first, then the three- and two-letter forms, then
/// `Language/String` when it looks like a code. Undetermined (`und`) is empty.
#[must_use]
pub fn language(stream: &StreamInfo) -> String {
    [
        "Language",
        "Language/String3",
        "Language/String2",
        "Language/String",
    ]
    .into_iter()
    .filter_map(|name| stream.get(name))
    .find_map(clean_language)
    .unwrap_or_default()
}

fn clean_language(value: &str) -> Option<String> {
    let value = value.trim().split('/').next()?.trim();
    let is_letters = |s: &str| s.chars().all(|c| c.is_ascii_alphabetic());
    let mut parts = value.split(['-', '_']);
    let primary = parts.next()?;
    if !(2..=3).contains(&primary.len()) || !is_letters(primary) {
        return None;
    }
    let mut code = primary.to_ascii_lowercase();
    if matches!(code.as_str(), "und" | "zxx" | "mis" | "mul") {
        return None;
    }
    for part in parts {
        if part.len() == 2 && is_letters(part) {
            code.push('-');
            code.push_str(&part.to_ascii_uppercase());
        } else if part.len() == 4 && is_letters(part) {
            code.push('-');
            code.push_str(&part[..1].to_ascii_uppercase());
            code.push_str(&part[1..].to_ascii_lowercase());
        } else {
            break;
        }
    }
    Some(code)
}

fn set_if_yes(flags: &mut BTreeSet<StreamFlag>, stream: &StreamInfo, name: &str, flag: StreamFlag) {
    if stream
        .get(name)
        .is_some_and(|v| v.eq_ignore_ascii_case("yes"))
    {
        flags.insert(flag);
    }
}

/// `ServiceKind` flags: `HI` hearing impaired, `VI` visually impaired, `C` commentary,
/// `K` karaoke, `O` original (the code or the long name). MediaInfo fills `ServiceKind` from the
/// Matroska track flags too: `FlagOriginal` gives `O` / `Original` and `FlagCommentary` gives
/// `C` / `Commentary`, so a track the container marks is flagged whatever its title says.
fn add_service_kind_flags(flags: &mut BTreeSet<StreamFlag>, stream: &StreamInfo) {
    for name in ["ServiceKind", "ServiceKind/String"] {
        let Some(kind) = stream.get(name) else {
            continue;
        };
        let lower = kind.trim().to_ascii_lowercase();
        let flag = match lower.as_str() {
            "hi" => Some(StreamFlag::HearingImpaired),
            "vi" => Some(StreamFlag::VisualImpaired),
            "c" => Some(StreamFlag::Comment),
            "k" => Some(StreamFlag::Karaoke),
            "o" => Some(StreamFlag::Original),
            _ if lower.contains("hearing") => Some(StreamFlag::HearingImpaired),
            _ if lower.contains("visually") => Some(StreamFlag::VisualImpaired),
            _ if lower.contains("commentary") => Some(StreamFlag::Comment),
            _ if lower.contains("original") => Some(StreamFlag::Original),
            _ if lower.contains("karaoke") => Some(StreamFlag::Karaoke),
            _ => None,
        };
        flags.extend(flag);
    }
}

/// Bit rate in kb/s. Streams that report two rates for a variable and a constant part (`VBR /
/// CBR`, as DTS-HD does for the extension and the core) are summed when `sum_mixed` is set.
fn bitrate_kbps(stream: &StreamInfo, sum_mixed: bool) -> u32 {
    let Some(text) = first_of(stream, &["BitRate", "BitRate_Nominal"]) else {
        return 0;
    };
    let mixed = stream.get("BitRate_Mode").is_some_and(|mode| {
        let mode = mode.to_ascii_uppercase();
        mode.contains("VBR") && mode.contains("CBR")
    });
    let values = text.split('/').filter_map(|p| first_number(p.trim()));
    let total: f64 = if sum_mixed && mixed {
        values.sum()
    } else {
        values.take(1).sum()
    };
    kbps(total)
}

fn kbps(bits_per_second: f64) -> u32 {
    (bits_per_second / 1000.0).round().max(0.0) as u32
}

fn first_of<'a>(stream: &'a StreamInfo, names: &[&str]) -> Option<&'a str> {
    names.iter().find_map(|name| stream.get(name))
}

/// The leading number of a text such as `1920`, `23.976` or `8 channels`.
fn first_number(text: &str) -> Option<f64> {
    let text = text.trim();
    let end = text
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(text.len());
    text[..end].parse().ok()
}

#[cfg(test)]
mod tests;
