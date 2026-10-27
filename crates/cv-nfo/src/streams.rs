//! The `<fileinfo>` stream details: codec tokens, resolution, HDR type, stereo mode and the
//! common aspect ratio, in the spelling Kodi reads.
//!
//! `cv-media` stores display names (`AVC`, `HEVC`, `DTS-HD MA`, `TrueHD Atmos`). The mappers
//! below normalise their input (case, spaces, dashes, slashes), so they also accept the older
//! spellings found in imported data (`h264`, `DTSHD-MA`, `TrueHD/Atmos`).

use cv_domain::{AudioStream, MediaFile, SubtitleStream, Video3dFormat};

/// What `<fileinfo>` needs to know about the main video file and the files next to it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VideoInfo {
    /// Video codec such as `AVC`, `HEVC`, `h264`, `MPEG-2`, `AV1`.
    pub codec: String,
    pub width: u32,
    pub height: u32,
    /// Display aspect ratio, `0.0` to derive it from the frame size.
    pub aspect_ratio: f32,
    pub duration_secs: u32,
    /// HDR formats such as `Dolby Vision`, `HDR10`, `HDR10+`, `HLG`.
    pub hdr_formats: Vec<String>,
    /// Stereoscopic layout text such as `3D SBS`; empty for 2D.
    pub stereo_format: String,
    /// Container format such as `Matroska`, `MPEG-4`; empty when unknown.
    pub container: String,
    /// Video bitrate in kbit/s, `0` when unknown.
    pub video_bitrate_kbps: u32,
    /// Bitrate of the whole file in kbit/s, `0` when unknown.
    pub overall_bitrate_kbps: u32,
    /// Frames per second, `0.0` when unknown.
    pub frame_rate: f64,
    /// Bits per colour sample, `0` when unknown.
    pub bit_depth: u32,
    /// Size of the video in bytes (all parts of a stacked movie), `0` when unknown.
    pub size_bytes: u64,
    pub audio: Vec<AudioStream>,
    pub subtitles: Vec<SubtitleStream>,
    /// File creation time, local `yyyy-MM-dd HH:mm:ss`.
    pub created: Option<String>,
    /// File modification time, local `yyyy-MM-dd HH:mm:ss`.
    pub modified: Option<String>,
}

impl VideoInfo {
    /// The stream facts of a media file. Times are not known to the media file record and stay
    /// empty.
    pub fn from_media_file(file: &MediaFile) -> Self {
        Self {
            codec: file.video_codec.clone(),
            width: file.width,
            height: file.height,
            aspect_ratio: file.aspect_ratio,
            duration_secs: file.duration_secs,
            hdr_formats: file.hdr_formats.clone(),
            stereo_format: stereo_format_text(file.video_3d).to_owned(),
            container: file.container_format.clone(),
            video_bitrate_kbps: file.video_bitrate_kbps,
            overall_bitrate_kbps: file.overall_bitrate_kbps,
            frame_rate: file.frame_rate,
            bit_depth: file.bit_depth,
            size_bytes: file.size_bytes,
            audio: file.audio_streams.clone(),
            subtitles: file.subtitles.clone(),
            created: None,
            modified: None,
        }
    }
}

/// The text of a 3D layout, as the media-info mapping and file names spell it.
pub fn stereo_format_text(format: Video3dFormat) -> &'static str {
    match format {
        Video3dFormat::None => "",
        Video3dFormat::ThreeD => "3D",
        Video3dFormat::SideBySide => "3D SBS",
        Video3dFormat::TopAndBottom => "3D TAB",
        Video3dFormat::HalfSideBySide => "3D HSBS",
        Video3dFormat::HalfTopAndBottom => "3D HTAB",
        Video3dFormat::Mvc => "3D MVC",
    }
}

/// Lower-case letters and digits only: `DTS-HD MA` gives `dtshdma`, `TrueHD/Atmos` `truehdatmos`.
fn squash(text: &str) -> String {
    text.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// The `<codec>` of a video stream: `AVC` and `h264` give `h264`, `h265` and `HEVC` give `hevc`,
/// `MPEG-2` gives `mpeg2`; anything else is written in lower case.
pub fn video_codec_token(codec: &str) -> String {
    let squashed = squash(codec);
    match squashed.as_str() {
        "avc" | "avc1" | "h264" | "x264" => "h264".to_owned(),
        "hevc" | "h265" | "x265" => "hevc".to_owned(),
        s if s.starts_with("mpeg") && s[4..].chars().all(|c| c.is_ascii_digit()) => s.to_owned(),
        _ => codec.trim().to_lowercase(),
    }
}

/// The `<codec>` of an audio stream in Kodi's spelling.
///
/// | Stream | Token |
/// |---|---|
/// | `DTS`, `DTS-ES`, `DTS 96/24`, `DTS Express` | `dca` |
/// | `DTS-HD MA` | `dtshd_ma` |
/// | `DTS-HD HRA` | `dtshd_hra` |
/// | `DTS-X` | `dtshd_ma_x` |
/// | `DTS-X IMAX` | `dtshd_ma_x_imax` |
/// | `TrueHD` / `TrueHD Atmos` | `truehd` / `truehd_atmos` |
/// | `E-AC-3` / `E-AC-3 Atmos` | `eac3` / `eac3_ddp_atmos` |
/// | `AC3`, `AC3 EX` | `ac3` |
///
/// Other codecs (`AAC`, `FLAC`, `MP3`, `Opus`, `Vorbis`) are written in lower case.
pub fn audio_codec_token(codec: &str) -> String {
    let squashed = squash(codec);
    match squashed.as_str() {
        "dts" | "dtses" | "dts9624" | "dtsexpress" | "dtshd" => "dca",
        "dtshdma" | "dtsma" => "dtshd_ma",
        "dtshdhra" | "dtshra" => "dtshd_hra",
        "dtsx" => "dtshd_ma_x",
        "dtsximax" => "dtshd_ma_x_imax",
        "truehd" => "truehd",
        "truehdatmos" => "truehd_atmos",
        "eac3" => "eac3",
        "eac3atmos" => "eac3_ddp_atmos",
        "ac3" | "ac3ex" => "ac3",
        _ => return codec.trim().to_lowercase(),
    }
    .to_owned()
}

/// The `<hdrtype>` of a video: `dolbyvision` wins over `hdr10plus`; anything else is the first
/// format in lower case without spaces. `None` for an SDR video.
pub fn hdr_type_token(formats: &[String]) -> Option<String> {
    let squashed: Vec<String> = formats
        .iter()
        .map(|f| squash(&f.replace('+', "plus")))
        .filter(|f| !f.is_empty())
        .collect();
    if squashed.iter().any(|f| f.contains("dolbyvision")) {
        Some("dolbyvision".to_owned())
    } else if squashed.iter().any(|f| f.contains("hdr10plus")) {
        Some("hdr10plus".to_owned())
    } else {
        squashed.into_iter().next()
    }
}

/// The `<stereomode>` for a 3D layout, or `None` for 2D.
///
/// Side-by-side layouts (`3D SBS`, `3D HSBS`) are `split_vertical`, top-and-bottom layouts
/// (`3D TAB`, `3D HTAB`) `split_horizontal`; checkerboard, row-interleaved and block layouts map
/// to `checkerboard`, `row_interleaved` and `off`. Anything else is written in lower case.
pub fn stereo_mode_token(format: &str) -> Option<String> {
    let text = format.trim().to_lowercase();
    if text.is_empty() {
        return None;
    }
    let squashed = squash(&text);
    Some(match squashed.as_str() {
        "3dsbs" | "3dhsbs" => "split_vertical".to_owned(),
        "3dtab" | "3dhtab" => "split_horizontal".to_owned(),
        s if s.starts_with("checkerboard") => "checkerboard".to_owned(),
        s if s.starts_with("rowinterleaved") => "row_interleaved".to_owned(),
        s if s.starts_with("block") => "off".to_owned(),
        _ => text,
    })
}

/// The common aspect ratios Kodi snaps a measured ratio to.
const COMMON_RATIOS: [f32; 14] = [
    1.33, 1.37, 1.43, 1.56, 1.66, 1.78, 1.85, 1.90, 2.00, 2.20, 2.35, 2.40, 2.55, 2.76,
];

/// The aspect ratio written in `<aspect>`: the nearest common ratio, where the boundary between
/// two ratios is their geometric mean. `0.0` stays `0.0`.
pub fn common_aspect_ratio(ratio: f32) -> f32 {
    if ratio.is_nan() || ratio <= 0.0 || !ratio.is_finite() {
        return 0.0;
    }
    COMMON_RATIOS
        .windows(2)
        .find(|pair| ratio * ratio < pair[0] * pair[1])
        .map_or(COMMON_RATIOS[COMMON_RATIOS.len() - 1], |pair| pair[0])
}

/// The `<resolution>` of a frame size: `4K` for the 2160 bucket, `8K` for 4320, otherwise the
/// number of lines (`1080`); empty when the size is below every bucket.
pub fn resolution_token(width: u32, height: u32) -> String {
    use cv_media::resolution::{Resolution, resolution_bucket};
    match resolution_bucket(width, height) {
        None => String::new(),
        Some(Resolution::P2160) => "4K".to_owned(),
        Some(Resolution::P4320) => "8K".to_owned(),
        Some(bucket) => bucket.height().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_codecs() {
        assert_eq!(video_codec_token("AVC"), "h264");
        assert_eq!(video_codec_token("h264"), "h264");
        assert_eq!(video_codec_token("HEVC"), "hevc");
        assert_eq!(video_codec_token("h265"), "hevc");
        assert_eq!(video_codec_token("MPEG-2"), "mpeg2");
        assert_eq!(video_codec_token("mpeg-4"), "mpeg4");
        assert_eq!(video_codec_token("AV1"), "av1");
        assert_eq!(video_codec_token(""), "");
    }

    #[test]
    fn audio_codecs_accept_display_names_and_old_spellings() {
        let pairs = [
            ("AC3", "ac3"),
            ("AC3 EX", "ac3"),
            ("AC3EX", "ac3"),
            ("DTS", "dca"),
            ("DTS-ES", "dca"),
            ("DTS-HD MA", "dtshd_ma"),
            ("DTSHD-MA", "dtshd_ma"),
            ("DTS-HD HRA", "dtshd_hra"),
            ("DTSHD-HRA", "dtshd_hra"),
            ("DTS-X", "dtshd_ma_x"),
            ("DTS-X IMAX", "dtshd_ma_x_imax"),
            ("DTS-X-IMAX", "dtshd_ma_x_imax"),
            ("TrueHD", "truehd"),
            ("TrueHD Atmos", "truehd_atmos"),
            ("TrueHD/Atmos", "truehd_atmos"),
            ("E-AC-3", "eac3"),
            ("EAC3", "eac3"),
            ("E-AC-3 Atmos", "eac3_ddp_atmos"),
            ("EAC3/Atmos", "eac3_ddp_atmos"),
            ("AAC", "aac"),
            ("FLAC", "flac"),
            ("MP3", "mp3"),
            ("Opus", "opus"),
        ];
        for (input, want) in pairs {
            assert_eq!(audio_codec_token(input), want, "{input}");
        }
    }

    #[test]
    fn hdr_types() {
        let f = |list: &[&str]| {
            hdr_type_token(&list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>())
        };
        assert_eq!(
            f(&["Dolby Vision", "HDR10"]).as_deref(),
            Some("dolbyvision")
        );
        assert_eq!(
            f(&["HDR10", "Dolby Vision"]).as_deref(),
            Some("dolbyvision")
        );
        assert_eq!(f(&["HDR10+"]).as_deref(), Some("hdr10plus"));
        assert_eq!(f(&["HLG"]).as_deref(), Some("hlg"));
        assert_eq!(f(&["HDR10"]).as_deref(), Some("hdr10"));
        assert_eq!(f(&[]), None);
    }

    #[test]
    fn stereo_modes() {
        assert_eq!(
            stereo_mode_token("3D SBS").as_deref(),
            Some("split_vertical")
        );
        assert_eq!(
            stereo_mode_token("3D HSBS").as_deref(),
            Some("split_vertical")
        );
        assert_eq!(
            stereo_mode_token("3D TAB").as_deref(),
            Some("split_horizontal")
        );
        assert_eq!(stereo_mode_token("3D MVC").as_deref(), Some("3d mvc"));
        assert_eq!(
            stereo_mode_token("checkerboard_rl").as_deref(),
            Some("checkerboard")
        );
        assert_eq!(
            stereo_mode_token("row_interleaved_lr").as_deref(),
            Some("row_interleaved")
        );
        assert_eq!(stereo_mode_token("block_lr").as_deref(), Some("off"));
        assert_eq!(stereo_mode_token(""), None);
    }

    #[test]
    fn aspect_ratios_snap_to_the_common_list() {
        assert_eq!(common_aspect_ratio(1.7777), 1.78);
        assert_eq!(common_aspect_ratio(1.78), 1.78);
        assert_eq!(common_aspect_ratio(2.39), 2.40);
        assert_eq!(common_aspect_ratio(2.4), 2.40);
        assert_eq!(common_aspect_ratio(1.0), 1.33);
        assert_eq!(common_aspect_ratio(4.0), 2.76);
        assert_eq!(common_aspect_ratio(0.0), 0.0);
        assert_eq!(common_aspect_ratio(1.85), 1.85);
    }

    #[test]
    fn resolutions() {
        assert_eq!(resolution_token(1920, 1080), "1080");
        assert_eq!(resolution_token(1920, 800), "1080");
        assert_eq!(resolution_token(3840, 2160), "4K");
        assert_eq!(resolution_token(7680, 4320), "8K");
        assert_eq!(resolution_token(720, 576), "576");
        assert_eq!(resolution_token(0, 0), "");
    }
}
