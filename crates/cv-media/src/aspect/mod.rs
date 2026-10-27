//! Aspect-ratio detection: the shape of the visible picture without its black bars, found with
//! FFmpeg's `cropdetect`.
//!
//! Spec: `media-info` (Aspect ratio detection).
//!
//! How it works:
//!
//! 1. [`ArdMode`] says how many samples of how many seconds to take; [`plan_samples`] spreads
//!    them evenly over the video, skipping the first 2 % and last 8 % of the runtime. Stacked
//!    parts count as one timeline.
//! 2. One FFmpeg run reads the darkest luma of the first frame, which sets the black threshold.
//! 3. Each sample is one FFmpeg run with `cropdetect`; the last crop rectangle it reports (the
//!    largest picture area seen in the sample) gives the sample's ratio `width x pixel aspect /
//!    height`.
//! 4. Every sample's ratio is rounded to the user's [`AspectRatioList`] (nearest by geometric
//!    mean, or the next wider entry) and the rounded ratios vote. The most common is the primary
//!    ratio. In accurate mode a second format holding at least 6 % of the votes is the secondary
//!    ratio, and [`MultiFormat`] can make the taller or wider format the primary.
//! 5. [`AspectDetection::apply_to`] stores the result on the file as measured. A picture is often
//!    wider than its frame: a 2.39:1 film in a 16:9 frame has black bars above and below, and the
//!    ratio of the visible picture is the one to show, not the frame's.
//!
//! ISO images are skipped ([`AspectError::IsoNotSupported`]). A missing FFmpeg is a typed
//! error ([`AspectError::Tool`], see [`AspectError::settings_link`]) so the UI can offer the
//! download or a path instead of failing silently.

pub(crate) mod ffmpeg;
mod plan;
mod ratios;

use std::path::{Path, PathBuf};

use cv_domain::MediaFile;
use serde::{Deserialize, Serialize};

pub use plan::{ArdMode, Sample, plan_samples};
pub use ratios::{AspectRatioList, MultiFormat, Votes};

use crate::disc::is_image_path;
use crate::tools::{Cancel, ToolError, ToolId, ToolsManager};

/// Where the app sends the user to download FFmpeg or set its path.
pub const TOOLS_SETTINGS_LINK: &str = "/settings/tools";

/// The user's aspect-ratio detection settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ArdSettings {
    pub mode: ArdMode,
    /// Round to the next wider ratio of the list instead of the nearest.
    pub round_up: bool,
    pub multi_format: MultiFormat,
    pub ratios: AspectRatioList,
    /// Percent of the runtime left out at the start (default 2).
    pub ignore_beginning_pct: f32,
    /// Percent of the runtime left out at the end (default 8).
    pub ignore_end_pct: f32,
}

impl Default for ArdSettings {
    fn default() -> Self {
        Self {
            mode: ArdMode::Standard,
            round_up: false,
            multi_format: MultiFormat::MostCommon,
            ratios: AspectRatioList::default(),
            ignore_beginning_pct: 2.0,
            ignore_end_pct: 8.0,
        }
    }
}

/// Why detection did not produce a ratio. Every message is ready to show.
#[derive(Debug, thiserror::Error)]
pub enum AspectError {
    /// Disc images are not sampled; the item keeps the ratio MediaInfo reported.
    #[error("ISO images are not supported for aspect ratio detection, so {} was skipped.", .0.display())]
    IsoNotSupported(PathBuf),
    /// FFmpeg is not installed or its path is broken; see [`AspectError::settings_link`].
    #[error(transparent)]
    Tool(#[from] ToolError),
    #[error("There is no video file to check.")]
    NoParts,
    #[error("The length of the video is not known, so there is nothing to sample yet.")]
    UnknownDuration,
    #[error("FFmpeg could not be started ({}): {source}", .program.display())]
    Start {
        program: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("FFmpeg found no picture to measure in {samples} samples of the video.")]
    NoUsableSamples { samples: usize },
    #[error("The detection was cancelled.")]
    Cancelled,
}

impl AspectError {
    /// A short stable name for the kind of failure, for the UI to branch on.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::IsoNotSupported(_) => "iso_not_supported",
            Self::Tool(_) => "tool_missing",
            Self::NoParts => "no_files",
            Self::UnknownDuration => "unknown_duration",
            Self::Start { .. } => "ffmpeg_start",
            Self::NoUsableSamples { .. } => "no_samples",
            Self::Cancelled => "cancelled",
        }
    }

    /// The Settings page to open when the fix is to download FFmpeg or set its path.
    #[must_use]
    pub const fn settings_link(&self) -> Option<&'static str> {
        match self {
            Self::Tool(
                ToolError::NotInstalled(_)
                | ToolError::OverrideMissing { .. }
                | ToolError::OverrideInvalid { .. },
            ) => Some(TOOLS_SETTINGS_LINK),
            _ => None,
        }
    }
}

/// One video file of the item being checked.
#[derive(Debug, Clone, PartialEq)]
pub struct AspectPart {
    pub path: PathBuf,
    pub duration_secs: f64,
    /// A disc image, which is skipped.
    pub disc_image: bool,
}

impl AspectPart {
    #[must_use]
    pub fn new(path: impl Into<PathBuf>, duration_secs: f64) -> Self {
        let path = path.into();
        let disc_image = is_image_path(&path);
        Self {
            path,
            duration_secs,
            disc_image,
        }
    }
}

/// What to measure: the parts of one video in playing order (one file, or the stacked parts).
#[derive(Debug, Clone, PartialEq)]
pub struct AspectRequest {
    pub parts: Vec<AspectPart>,
    /// Pixel aspect ratio of the stored frame (`0.0` or `1.0` for square pixels); anamorphic
    /// video is stored squeezed, so the measured shape is widened by it.
    pub pixel_aspect_ratio: f32,
}

impl AspectRequest {
    /// The request for the stacked video files of one item, with the first file's pixel aspect
    /// ratio.
    #[must_use]
    pub fn from_media_files(files: &[&MediaFile]) -> Self {
        let parts = files
            .iter()
            .map(|file| {
                let mut part = AspectPart::new(
                    Path::new(&file.directory).join(&file.filename),
                    f64::from(file.duration_secs),
                );
                part.disc_image |= file.is_iso;
                part
            })
            .collect();
        Self {
            parts,
            pixel_aspect_ratio: files.first().map_or(1.0, |f| f.pixel_aspect_ratio),
        }
    }
}

/// The outcome of a detection.
#[derive(Debug, Clone, PartialEq)]
pub struct AspectDetection {
    pub mode: ArdMode,
    /// The main ratio, rounded onto the user's list.
    pub primary: f32,
    /// The second format of a multi-format film, `0.0` when none (or when the mode does not
    /// look for one).
    pub secondary: f32,
    /// Samples that gave a picture area.
    pub samples_used: usize,
    /// Samples taken.
    pub samples_planned: usize,
    /// Each rounded ratio seen and how many samples voted for it, narrowest first.
    pub votes: Vec<(f32, u32)>,
}

impl AspectDetection {
    /// Stores the result on `file` and returns whether it was stored.
    ///
    /// The measured shape of the picture replaces the ratio MediaInfo reported for the frame,
    /// wider or narrower: black bars above and below make a picture wider than its frame (a
    /// 2.39:1 film in a 1.78:1 frame is the ordinary case), and bars at the sides make it
    /// narrower. Only a failed detection (no ratio) leaves the file as it was. The secondary
    /// ratio is only touched by accurate runs, which are the only ones that look for it.
    pub fn apply_to(&self, file: &mut MediaFile) -> bool {
        if self.primary <= 0.0 {
            return false;
        }
        file.aspect_ratio = self.primary;
        if self.mode == ArdMode::Accurate {
            file.aspect_ratio2 = self.secondary.max(0.0);
        }
        true
    }
}

/// Detects the visible aspect ratio of a video.
///
/// The check order is deliberate: disc images are refused before FFmpeg is looked for, so the
/// "ISO not supported" notice appears even where FFmpeg is not installed.
///
/// # Errors
/// [`AspectError::IsoNotSupported`] for a disc image, [`AspectError::Tool`] when FFmpeg is not
/// installed (the UI links to [`TOOLS_SETTINGS_LINK`]), [`AspectError::UnknownDuration`] when no
/// part has a length, [`AspectError::NoUsableSamples`] when FFmpeg measured nothing,
/// [`AspectError::Cancelled`] when `cancel` is set, [`AspectError::Start`] when FFmpeg cannot
/// start.
pub fn detect_aspect_ratio(
    tools: &ToolsManager,
    request: &AspectRequest,
    settings: &ArdSettings,
    cancel: &Cancel,
) -> Result<AspectDetection, AspectError> {
    if request.parts.is_empty() {
        return Err(AspectError::NoParts);
    }
    if let Some(image) = request.parts.iter().find(|p| p.disc_image) {
        return Err(AspectError::IsoNotSupported(image.path.clone()));
    }
    let durations: Vec<f64> = request.parts.iter().map(|p| p.duration_secs).collect();
    let samples = plan_samples(
        &durations,
        settings.mode,
        f64::from(settings.ignore_beginning_pct),
        f64::from(settings.ignore_end_pct),
    );
    if samples.is_empty() {
        return Err(AspectError::UnknownDuration);
    }
    let ffmpeg = tools.resolve(ToolId::Ffmpeg)?;
    // FFmpeg has the parts open while the samples run: a rename of their folder waits for them.
    let _reading: Vec<cv_fs::ReadGuard> = request
        .parts
        .iter()
        .map(|part| cv_fs::reading(&part.path))
        .collect();

    let pixel_aspect = if request.pixel_aspect_ratio > 0.0 {
        request.pixel_aspect_ratio
    } else {
        1.0
    };
    // The threshold comes from the first part that has a length.
    let first = request
        .parts
        .iter()
        .find(|p| p.duration_secs > 0.0)
        .unwrap_or(&request.parts[0]);
    let dark_level = ffmpeg::dark_level_from_output(&ffmpeg::run(
        &ffmpeg.path,
        &ffmpeg::dark_level_args(&first.path),
        cancel,
    )?);

    let mut votes = Votes::default();
    for sample in &samples {
        if cancel.is_cancelled() {
            return Err(AspectError::Cancelled);
        }
        let part = &request.parts[sample.part];
        let output = ffmpeg::run(
            &ffmpeg.path,
            &ffmpeg::crop_args(&part.path, sample.start, sample.length, dark_level),
            cancel,
        )?;
        let Some(crop) = ffmpeg::last_crop(&output) else {
            tracing::debug!(file = %part.path.display(), at = sample.start, "no picture in sample");
            continue;
        };
        #[allow(clippy::cast_precision_loss)]
        let measured = crop.width as f32 * pixel_aspect / crop.height as f32;
        votes.add(settings.ratios.round(measured, settings.round_up));
    }
    if votes.total() == 0 {
        return Err(AspectError::NoUsableSamples {
            samples: samples.len(),
        });
    }
    let accurate = settings.mode == ArdMode::Accurate;
    let (primary, secondary) = votes.choose(accurate, settings.multi_format);
    Ok(AspectDetection {
        mode: settings.mode,
        primary,
        secondary,
        samples_used: votes.total() as usize,
        samples_planned: samples.len(),
        votes: votes.entries(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cv_domain::MediaFileType;

    fn detection(mode: ArdMode, primary: f32, secondary: f32) -> AspectDetection {
        AspectDetection {
            mode,
            primary,
            secondary,
            samples_used: 6,
            samples_planned: 6,
            votes: Vec::new(),
        }
    }

    fn file(aspect: f32, aspect2: f32) -> MediaFile {
        MediaFile {
            aspect_ratio: aspect,
            aspect_ratio2: aspect2,
            ..MediaFile::new(MediaFileType::Video, "C:\\Movies", "film.mkv")
        }
    }

    #[test]
    fn a_ratio_narrower_than_the_frame_is_applied() {
        let mut f = file(2.39, 0.0);
        assert!(detection(ArdMode::Standard, 1.78, 0.0).apply_to(&mut f));
        assert!((f.aspect_ratio - 1.78).abs() < f32::EPSILON);
    }

    #[test]
    fn a_ratio_wider_than_the_frame_is_applied_as_measured() {
        // A scope film in a 16:9 frame: black bars above and below, so the picture is wider.
        let mut f = file(1.78, 0.0);
        assert!(detection(ArdMode::Standard, 2.39, 0.0).apply_to(&mut f));
        assert!((f.aspect_ratio - 2.39).abs() < f32::EPSILON);
    }

    #[test]
    fn a_letterboxed_crop_of_a_full_hd_frame_is_stored_as_2_39() {
        // What cropdetect prints for a 1920x1080 frame with bars above and below, as FFmpeg
        // writes it: read, measured and rounded onto the list the way a detection does.
        let output = "[Parsed_cropdetect_0 @ 0x1] x1:0 x2:1919 y1:138 y2:941 w:1920 h:804 x:0 y:138 pts:0 t:0.000000 limit:0.1 crop=1920:804:0:138";
        let crop = ffmpeg::last_crop(output).expect("a picture");
        let settings = ArdSettings::default();
        #[allow(clippy::cast_precision_loss)]
        let measured = crop.width as f32 / crop.height as f32;
        let primary = settings.ratios.round(measured, settings.round_up);
        let mut f = file(1.78, 0.0);
        assert!(detection(ArdMode::Standard, primary, 0.0).apply_to(&mut f));
        assert!((f.aspect_ratio - 2.39).abs() < 0.005, "{}", f.aspect_ratio);
    }

    #[test]
    fn only_accurate_runs_touch_the_secondary_ratio() {
        let mut f = file(2.40, 1.85);
        assert!(detection(ArdMode::Standard, 2.39, 0.0).apply_to(&mut f));
        assert!((f.aspect_ratio2 - 1.85).abs() < f32::EPSILON, "kept");

        assert!(detection(ArdMode::Accurate, 2.39, 1.90).apply_to(&mut f));
        assert!((f.aspect_ratio2 - 1.90).abs() < f32::EPSILON);

        assert!(detection(ArdMode::Accurate, 2.39, 0.0).apply_to(&mut f));
        assert!(f.aspect_ratio2.abs() < f32::EPSILON, "cleared");
    }

    #[test]
    fn a_secondary_wider_than_the_frame_is_kept() {
        let mut f = file(2.0, 0.0);
        assert!(detection(ArdMode::Accurate, 1.78, 2.39).apply_to(&mut f));
        assert!((f.aspect_ratio2 - 2.39).abs() < f32::EPSILON);
    }

    #[test]
    fn a_failed_detection_has_nothing_to_apply() {
        let mut f = file(1.78, 0.0);
        assert!(!detection(ArdMode::Standard, 0.0, 0.0).apply_to(&mut f));
    }

    #[test]
    fn iso_images_are_skipped_before_ffmpeg_is_looked_for() {
        // An empty tools folder: FFmpeg is absent, yet the answer is the ISO notice.
        let empty = tempfile::tempdir().expect("temp dir");
        let tools = ToolsManager::new(empty.path());
        let request = AspectRequest {
            parts: vec![AspectPart::new("D:\\Films\\Disc.ISO", 7200.0)],
            pixel_aspect_ratio: 1.0,
        };
        let err = detect_aspect_ratio(&tools, &request, &ArdSettings::default(), &Cancel::new())
            .expect_err("skipped");
        assert!(matches!(err, AspectError::IsoNotSupported(_)), "{err}");
        assert_eq!(err.code(), "iso_not_supported");
        assert!(err.to_string().contains("ISO images are not supported"));
        assert_eq!(err.settings_link(), None);
    }

    #[test]
    fn a_media_file_flagged_as_iso_is_skipped_whatever_its_name() {
        let mut f = file(1.78, 0.0);
        f.is_iso = true;
        f.duration_secs = 7000;
        let request = AspectRequest::from_media_files(&[&f]);
        assert!(request.parts[0].disc_image);
        assert!((request.parts[0].duration_secs - 7000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_missing_ffmpeg_is_a_typed_error_that_links_to_the_tools_page() {
        let empty = tempfile::tempdir().expect("temp dir");
        let tools = ToolsManager::new(empty.path());
        let request = AspectRequest {
            parts: vec![AspectPart::new("D:\\Films\\Film.mkv", 7200.0)],
            pixel_aspect_ratio: 1.0,
        };
        let err = detect_aspect_ratio(&tools, &request, &ArdSettings::default(), &Cancel::new())
            .expect_err("no FFmpeg");
        assert!(
            matches!(
                err,
                AspectError::Tool(ToolError::NotInstalled(ToolId::Ffmpeg))
            ),
            "{err}"
        );
        assert_eq!(err.code(), "tool_missing");
        assert_eq!(err.settings_link(), Some("/settings/tools"));
    }

    #[test]
    fn requests_without_files_or_length_are_refused() {
        let empty = tempfile::tempdir().expect("temp dir");
        let tools = ToolsManager::new(empty.path());
        let run = |parts| {
            detect_aspect_ratio(
                &tools,
                &AspectRequest {
                    parts,
                    pixel_aspect_ratio: 1.0,
                },
                &ArdSettings::default(),
                &Cancel::new(),
            )
        };
        assert!(matches!(run(Vec::new()), Err(AspectError::NoParts)));
        assert!(matches!(
            run(vec![AspectPart::new("film.mkv", 0.0)]),
            Err(AspectError::UnknownDuration)
        ));
    }

    #[test]
    fn settings_serialise_with_the_documented_names() {
        let json = serde_json::to_value(ArdSettings::default()).expect("serialises");
        assert_eq!(json["mode"], "default");
        assert_eq!(json["multiFormat"], "mostCommon");
        assert_eq!(json["roundUp"], false);
        assert_eq!(json["ignoreBeginningPct"], 2.0);
        assert_eq!(json["ignoreEndPct"], 8.0);
        let back: ArdSettings =
            serde_json::from_str(r#"{"mode":"accurate","ratios":[2.39,1.78]}"#).expect("parses");
        assert_eq!(back.mode, ArdMode::Accurate);
        assert_eq!(back.ratios.values(), &[1.78, 2.39]);
        assert!((back.ignore_end_pct - 8.0).abs() < f32::EPSILON);
    }
}
