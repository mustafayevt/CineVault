//! Media files and their technical streams.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// What a file belonging to an item is.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum MediaFileType {
    Video,
    Trailer,
    Sample,
    VideoExtra,
    Audio,
    Subtitle,
    Nfo,
    Poster,
    Fanart,
    Banner,
    Clearart,
    Disc,
    Clearlogo,
    Thumb,
    Characterart,
    Keyart,
    Squareart,
    SeasonPoster,
    SeasonFanart,
    SeasonBanner,
    SeasonThumb,
    Extrafanart,
    Extrathumb,
    Extra,
    Bif,
    Graphic,
    Mediainfo,
    Vsmeta,
    Theme,
    Text,
    DoubleExt,
    #[default]
    Unknown,
}

impl MediaFileType {
    /// True for image types (artwork and other graphics).
    pub fn is_graphic(self) -> bool {
        use MediaFileType::*;
        matches!(
            self,
            Poster
                | Fanart
                | Banner
                | Clearart
                | Disc
                | Clearlogo
                | Thumb
                | Characterart
                | Keyart
                | Squareart
                | SeasonPoster
                | SeasonFanart
                | SeasonBanner
                | SeasonThumb
                | Extrafanart
                | Extrathumb
                | Graphic
        )
    }
}

/// Stereoscopic 3D layout of a video.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub enum Video3dFormat {
    #[default]
    #[serde(rename = "")]
    None,
    #[serde(rename = "3D")]
    ThreeD,
    #[serde(rename = "3D SBS")]
    SideBySide,
    #[serde(rename = "3D TAB")]
    TopAndBottom,
    #[serde(rename = "3D HSBS")]
    HalfSideBySide,
    #[serde(rename = "3D HTAB")]
    HalfTopAndBottom,
    #[serde(rename = "3D MVC")]
    Mvc,
}

/// Flags on an audio or subtitle stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamFlag {
    Default,
    Dub,
    Original,
    Comment,
    Lyrics,
    Karaoke,
    Forced,
    HearingImpaired,
    VisualImpaired,
}

/// An audio stream.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AudioStream {
    /// Normalised codec such as `AC3`, `DTS-HD MA`, `TrueHD Atmos`, `E-AC-3 Atmos`.
    pub codec: String,
    pub codec_id: String,
    /// ISO 639 language code, empty when unknown.
    pub language: String,
    pub title: String,
    pub flags: BTreeSet<StreamFlag>,
    pub channels: u32,
    pub bitrate_kbps: u32,
    pub bit_depth: u32,
    /// Samples per second, for example `48000`; `0` when unknown (read before this was kept).
    pub sample_rate_hz: u32,
}

/// A subtitle stream (embedded in the container).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SubtitleStream {
    pub codec: String,
    pub codec_id: String,
    pub language: String,
    pub title: String,
    pub flags: BTreeSet<StreamFlag>,
}

/// What a video says about its HDR beyond the format names of [`MediaFile::hdr_formats`]: the
/// Dolby Vision profile, the mastering display and the light levels. Empty strings and `None`
/// mean the file does not say (or media info was read before this was kept).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct HdrDetails {
    /// The Dolby Vision profile with the compatibility id of profile 8: `5`, `7`, `8.1` (HDR10
    /// base), `8.2` (SDR base), `8.4` (HLG base). Empty without a Dolby Vision layer.
    pub dolby_vision_profile: String,
    /// What a player without Dolby Vision plays: `HDR10`, `HLG`, `SDR`, ... Empty when unknown.
    pub dolby_vision_compatibility: String,
    /// The colour primaries of the mastering display, for example `Display P3` or `BT.2020`.
    pub mastering_primaries: String,
    /// The mastering display's darkest and brightest level, in cd/m2 (nits).
    pub mastering_min_nits: Option<f64>,
    pub mastering_max_nits: Option<f64>,
    /// Maximum content light level and maximum frame-average light level, in cd/m2.
    pub max_cll_nits: Option<u32>,
    pub max_fall_nits: Option<u32>,
}

impl HdrDetails {
    /// True when nothing is known.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// One file belonging to an item: a video, subtitle, NFO, artwork file and so on, with the
/// technical data gathered for video files.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MediaFile {
    pub kind: MediaFileType,
    /// The parent directory (absolute, OS-native separators).
    pub directory: String,
    pub filename: String,
    pub size_bytes: u64,
    /// File modification time, epoch milliseconds.
    pub modified_ms: i64,

    pub video_codec: String,
    pub codec_id: String,
    /// Container format; empty means media info has not been gathered yet.
    pub container_format: String,
    pub exact_video_format: String,
    pub video_3d: Video3dFormat,
    pub width: u32,
    pub height: u32,
    /// Display aspect ratio, `0.0` when unknown.
    pub aspect_ratio: f32,
    /// Secondary aspect ratio found by the letterbox detector, `0.0` when none.
    pub aspect_ratio2: f32,
    pub pixel_aspect_ratio: f32,
    pub video_bitrate_kbps: u32,
    pub overall_bitrate_kbps: u32,
    pub bit_depth: u32,
    pub frame_rate: f64,
    pub duration_secs: u32,
    /// Part number within a stack (CD1, CD2, ...), `0` when not stacked.
    pub stacking: u32,
    pub stacking_marker: String,
    /// Container title tag.
    pub title: String,
    pub is_iso: bool,
    pub is_animated_graphic: bool,
    /// HDR formats, for example `Dolby Vision`, `HDR10`, `HDR10+`, `HLG`.
    pub hdr_formats: Vec<String>,
    /// The Dolby Vision profile, mastering display and light levels behind [`Self::hdr_formats`].
    pub hdr_details: HdrDetails,
    /// Checksums by algorithm key (`crc32` as 8 uppercase hex digits, `phash`).
    pub checksums: BTreeMap<String, String>,

    pub audio_streams: Vec<AudioStream>,
    pub subtitles: Vec<SubtitleStream>,
}

impl MediaFile {
    pub fn new(
        kind: MediaFileType,
        directory: impl Into<String>,
        filename: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            directory: directory.into(),
            filename: filename.into(),
            ..Self::default()
        }
    }

    /// The full path: directory plus filename.
    pub fn path(&self) -> PathBuf {
        PathBuf::from(&self.directory).join(&self.filename)
    }

    /// True once media info has been read for this file.
    pub fn has_media_info(&self) -> bool {
        !self.container_format.is_empty()
    }

    /// True for the marker file of a disc structure (`BDMV/index.bdmv`, `VIDEO_TS/VIDEO_TS.IFO`,
    /// `HVDVD_TS/HV000I01.IFO`) when `files` (the title's files) also hold the disc folder itself
    /// as a video. The folder is the title's video then and is read through the disc's main
    /// title; the marker only marks the structure, so it is not read and not listed as a video.
    pub fn is_covered_disc_marker(&self, files: &[MediaFile]) -> bool {
        const MARKERS: [&str; 3] = ["index.bdmv", "video_ts.ifo", "hv000i01.ifo"];
        self.kind == MediaFileType::Video
            && MARKERS.contains(&self.filename.to_ascii_lowercase().as_str())
            && files.iter().any(|folder| {
                folder.kind == MediaFileType::Video
                    && !std::ptr::eq(folder, self)
                    && folder.path() == std::path::Path::new(&self.directory)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_disc_marker_next_to_its_disc_folder_is_covered() {
        let folder = MediaFile::new(MediaFileType::Video, "D:/Media/Spies", "BDMV");
        let marker = MediaFile::new(MediaFileType::Video, "D:/Media/Spies/BDMV", "index.bdmv");
        let files = vec![folder.clone(), marker.clone()];
        assert!(files[1].is_covered_disc_marker(&files));
        assert!(!files[0].is_covered_disc_marker(&files));
        // Alone, the marker is the title's only video.
        assert!(!marker.is_covered_disc_marker(std::slice::from_ref(&marker)));
        let dvd = vec![
            MediaFile::new(MediaFileType::Video, "D:/Heat", "VIDEO_TS"),
            MediaFile::new(MediaFileType::Video, "D:/Heat/VIDEO_TS", "VIDEO_TS.IFO"),
        ];
        assert!(dvd[1].is_covered_disc_marker(&dvd));
    }

    fn populated() -> MediaFile {
        let mut f = MediaFile::new(
            MediaFileType::Video,
            "D:\\Movies\\Alien (1979)",
            "Alien (1979).mkv",
        );
        f.size_bytes = 8_589_934_592;
        f.modified_ms = 1_700_000_000_000;
        f.video_codec = "HEVC".into();
        f.container_format = "Matroska".into();
        f.video_3d = Video3dFormat::HalfSideBySide;
        f.width = 3840;
        f.height = 1608;
        f.aspect_ratio = 2.39;
        f.frame_rate = 23.976;
        f.duration_secs = 7020;
        f.hdr_formats = vec!["Dolby Vision".into(), "HDR10".into()];
        f.hdr_details = HdrDetails {
            dolby_vision_profile: "8.1".into(),
            dolby_vision_compatibility: "HDR10".into(),
            mastering_primaries: "Display P3".into(),
            mastering_min_nits: Some(0.005),
            mastering_max_nits: Some(1000.0),
            max_cll_nits: Some(1000),
            max_fall_nits: Some(400),
        };
        f.checksums.insert("crc32".into(), "DEADBEEF".into());
        f.audio_streams.push(AudioStream {
            codec: "TrueHD Atmos".into(),
            language: "en".into(),
            flags: [StreamFlag::Default, StreamFlag::Original]
                .into_iter()
                .collect(),
            channels: 8,
            ..AudioStream::default()
        });
        f.subtitles.push(SubtitleStream {
            codec: "PGS".into(),
            language: "de".into(),
            flags: [StreamFlag::Forced].into_iter().collect(),
            ..SubtitleStream::default()
        });
        f
    }

    #[test]
    fn fully_populated_file_round_trips() {
        let f = populated();
        let json = serde_json::to_string(&f).unwrap();
        assert_eq!(serde_json::from_str::<MediaFile>(&json).unwrap(), f);
    }

    #[test]
    fn every_file_type_round_trips() {
        use MediaFileType::*;
        for t in [
            Video,
            Trailer,
            Sample,
            VideoExtra,
            Audio,
            Subtitle,
            Nfo,
            Poster,
            Fanart,
            Banner,
            Clearart,
            Disc,
            Clearlogo,
            Thumb,
            Characterart,
            Keyart,
            Squareart,
            SeasonPoster,
            SeasonFanart,
            SeasonBanner,
            SeasonThumb,
            Extrafanart,
            Extrathumb,
            Extra,
            Bif,
            Graphic,
            Mediainfo,
            Vsmeta,
            Theme,
            Text,
            DoubleExt,
            Unknown,
        ] {
            let json = serde_json::to_string(&t).unwrap();
            assert_eq!(serde_json::from_str::<MediaFileType>(&json).unwrap(), t);
        }
    }

    #[test]
    fn video_3d_uses_display_names() {
        assert_eq!(
            serde_json::to_string(&Video3dFormat::HalfSideBySide).unwrap(),
            "\"3D HSBS\""
        );
        assert_eq!(serde_json::to_string(&Video3dFormat::None).unwrap(), "\"\"");
        assert_eq!(
            serde_json::from_str::<Video3dFormat>("\"3D MVC\"").unwrap(),
            Video3dFormat::Mvc
        );
    }

    #[test]
    fn media_info_flag_and_path() {
        let mut f = MediaFile::new(MediaFileType::Video, "movies", "a.mkv");
        assert!(!f.has_media_info());
        f.container_format = "Matroska".into();
        assert!(f.has_media_info());
        assert_eq!(f.path(), PathBuf::from("movies").join("a.mkv"));
        assert!(MediaFileType::Poster.is_graphic());
        assert!(!MediaFileType::Video.is_graphic());
    }
}
