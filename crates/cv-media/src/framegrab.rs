//! Frame-grab artwork: still JPEG frames taken from a video with FFmpeg, offered in the artwork
//! picker as a provider.
//!
//! Spec: `media-info` (Frame-grab artwork).
//!
//! * [`grab_frame`] writes one frame at a chosen timestamp (a thumb or fanart).
//! * [`grab_candidates`] writes several evenly spaced frames; [`grab_middle_frame`] writes the
//!   one from the middle of the video, which is what an episode without a thumb is offered.
//! * Stacked parts count as one timeline: a timestamp is mapped to the part that plays it.
//! * Anamorphic video is stored squeezed, so frames are widened to their display shape.
//!
//! Disc images are refused (the same notice as aspect-ratio detection). A missing FFmpeg is a
//! typed error ([`FrameGrabError::Tool`], see [`FrameGrabError::settings_link`]) so the UI can
//! offer the download or a path.

use std::ffi::OsString;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::aspect::{AspectError, AspectPart, TOOLS_SETTINGS_LINK, ffmpeg};
use crate::tools::{Cancel, ToolError, ToolId, ToolsManager};

/// First bytes of every JPEG file.
const JPEG_MAGIC: [u8; 3] = [0xFF, 0xD8, 0xFF];
/// A frame is never taken closer than this to the end, where a seek can land past the last
/// frame and produce nothing.
const END_MARGIN_SECS: f64 = 0.5;

/// How the frames are written.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameGrabOptions {
    /// JPEG quality from 1 (worst) to 100 (best); the default 90 suits artwork.
    pub quality: u8,
    /// Frames wider than this are scaled down (never up); `None` keeps the stored width.
    pub max_width: Option<u32>,
    /// Pixel aspect ratio of the stored frame (`0.0` or `1.0` for square pixels); anamorphic
    /// video is widened by it so the picture has its display shape.
    pub pixel_aspect_ratio: f32,
}

impl Default for FrameGrabOptions {
    fn default() -> Self {
        Self {
            quality: 90,
            max_width: None,
            pixel_aspect_ratio: 1.0,
        }
    }
}

/// One frame written to disk.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameCandidate {
    pub path: PathBuf,
    /// Position on the whole (stacked) timeline, in seconds.
    pub timestamp_secs: f64,
    /// Index of the part the frame came from.
    pub part: usize,
}

/// Why a frame was not produced. Every message is ready to show.
#[derive(Debug, thiserror::Error)]
pub enum FrameGrabError {
    /// FFmpeg is not installed or its path is broken; see [`FrameGrabError::settings_link`].
    #[error(transparent)]
    Tool(#[from] ToolError),
    #[error("Frames cannot be taken from disc images, so {} was skipped.", .0.display())]
    IsoNotSupported(PathBuf),
    #[error("There is no video file to take a frame from.")]
    NoParts,
    #[error("The length of the video is not known, so there is no frame to choose yet.")]
    UnknownDuration,
    #[error("FFmpeg could not be started ({}): {source}", .program.display())]
    Start {
        program: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("FFmpeg did not produce a frame at {timestamp_secs:.1} s of {}.", .file.display())]
    NoFrame { file: PathBuf, timestamp_secs: f64 },
    #[error("The frame could not be saved to {}: {source}", .path.display())]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("The frame grab was cancelled.")]
    Cancelled,
}

impl FrameGrabError {
    /// A short stable name for the kind of failure, for the UI to branch on.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Tool(_) => "tool_missing",
            Self::IsoNotSupported(_) => "iso_not_supported",
            Self::NoParts => "no_files",
            Self::UnknownDuration => "unknown_duration",
            Self::Start { .. } => "ffmpeg_start",
            Self::NoFrame { .. } => "no_frame",
            Self::Write { .. } => "write_failed",
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

/// The timestamps of `count` evenly spaced candidates over a timeline of `total` seconds: the
/// centres of `count` equal slices, so a single candidate is the exact middle and the first and
/// last never sit on the opening or closing seconds.
#[must_use]
pub fn candidate_timestamps(total: f64, count: usize) -> Vec<f64> {
    if count == 0 || total <= 0.0 {
        return Vec::new();
    }
    let latest = (total - END_MARGIN_SECS).max(0.0);
    (0..count)
        .map(|i| {
            #[allow(clippy::cast_precision_loss)]
            let (slice, n) = (i as f64 + 0.5, count as f64);
            (total * slice / n).min(latest)
        })
        .collect()
}

/// The timestamps of `count` frames spread evenly over the part of a `total`-second timeline
/// between the fractions `from` and `to` (both ends included): `count = 8` over `0.1..0.9` gives
/// 10%, 21.4%, ... 90% of the length. A single frame is the middle of the span. The fractions are
/// clamped to `0..=1`, and no frame is taken in the closing half second.
#[must_use]
pub fn spread_timestamps(total: f64, count: usize, from: f64, to: f64) -> Vec<f64> {
    if count == 0 || total <= 0.0 {
        return Vec::new();
    }
    let (from, to) = (from.clamp(0.0, 1.0), to.clamp(0.0, 1.0));
    let (from, to) = if from <= to { (from, to) } else { (to, from) };
    let latest = (total - END_MARGIN_SECS).max(0.0);
    (0..count)
        .map(|i| {
            #[allow(clippy::cast_precision_loss)]
            let fraction = if count == 1 {
                f64::midpoint(from, to)
            } else {
                from + (to - from) * (i as f64) / ((count - 1) as f64)
            };
            (total * fraction).min(latest)
        })
        .collect()
}

/// Maps a position on the stacked timeline to `(part index, seconds into that part)`.
fn locate(parts: &[AspectPart], at: f64) -> (usize, f64) {
    let mut offset = 0.0;
    let mut last = 0;
    for (index, part) in parts.iter().enumerate() {
        if part.duration_secs <= 0.0 {
            continue;
        }
        last = index;
        if at < offset + part.duration_secs {
            return (index, (at - offset).max(0.0));
        }
        offset += part.duration_secs;
    }
    // Past the end (rounding): near the end of the last part with a length.
    let length = parts[last].duration_secs.max(0.0);
    (last, (length - END_MARGIN_SECS).max(0.0))
}

/// The video filter: widen to display shape, then optionally limit the width.
fn filter(options: &FrameGrabOptions) -> String {
    let mut chain = Vec::new();
    if options.pixel_aspect_ratio > 0.0 && (options.pixel_aspect_ratio - 1.0).abs() > 0.005 {
        chain.push(format!(
            "scale=trunc(iw*{:.4}/2)*2:ih,setsar=1",
            options.pixel_aspect_ratio
        ));
    }
    if let Some(width) = options.max_width.filter(|w| *w >= 16) {
        chain.push(format!("scale='min({width},iw)':-2"));
    }
    if chain.is_empty() {
        "null".to_owned()
    } else {
        chain.join(",")
    }
}

/// FFmpeg's `-q:v` (2 best, 31 worst) for a quality of 1 to 100.
fn qscale(quality: u8) -> u32 {
    let quality = u32::from(quality.clamp(1, 100));
    31 - (quality * 29 + 50) / 100
}

fn grab_args(file: &Path, at: f64, out: &Path, options: &FrameGrabOptions) -> Vec<OsString> {
    let mut args: Vec<OsString> = [
        "-hide_banner",
        "-nostats",
        "-nostdin",
        "-an",
        "-dn",
        "-sn",
        "-y",
        "-ss",
    ]
    .map(OsString::from)
    .to_vec();
    args.push(format!("{at:.3}").into());
    args.push("-i".into());
    args.push(file.as_os_str().to_owned());
    args.extend(
        [
            "-frames:v".to_owned(),
            "1".to_owned(),
            "-vf".to_owned(),
            filter(options),
            "-q:v".to_owned(),
            qscale(options.quality).to_string(),
            "-pix_fmt".to_owned(),
            "yuvj420p".to_owned(),
            "-f".to_owned(),
            "image2".to_owned(),
        ]
        .map(OsString::from),
    );
    args.push(out.as_os_str().to_owned());
    args
}

fn is_jpeg(path: &Path) -> bool {
    let mut head = [0_u8; 3];
    fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut head))
        .is_ok()
        && head == JPEG_MAGIC
}

/// Takes one frame of `file` at `at` seconds and saves it as a JPEG at `out`. The picture is
/// written beside `out` first and moved into place, so `out` is never half written.
fn grab_one(
    ffmpeg_path: &Path,
    file: &Path,
    at: f64,
    out: &Path,
    options: &FrameGrabOptions,
    cancel: &Cancel,
) -> Result<(), FrameGrabError> {
    let mut temp_name = out.file_stem().map(OsString::from).unwrap_or_default();
    temp_name.push(".partial.jpg");
    let temp = out.with_file_name(temp_name);
    let _ = fs::remove_file(&temp);
    // FFmpeg has the video open while it runs: a rename of its folder waits for it.
    let _reading = cv_fs::reading(file);

    let ran = ffmpeg::run(ffmpeg_path, &grab_args(file, at, &temp, options), cancel);
    if let Err(err) = ran {
        let _ = fs::remove_file(&temp);
        return Err(match err {
            AspectError::Start { program, source } => FrameGrabError::Start { program, source },
            _ => FrameGrabError::Cancelled,
        });
    }
    if !is_jpeg(&temp) {
        let _ = fs::remove_file(&temp);
        return Err(FrameGrabError::NoFrame {
            file: file.to_path_buf(),
            timestamp_secs: at,
        });
    }
    // Replacing an existing frame: rename does not overwrite everywhere.
    let _ = fs::remove_file(out);
    fs::rename(&temp, out).map_err(|source| {
        let _ = fs::remove_file(&temp);
        FrameGrabError::Write {
            path: out.to_path_buf(),
            source,
        }
    })
}

fn total_duration(parts: &[AspectPart]) -> Result<f64, FrameGrabError> {
    if parts.is_empty() {
        return Err(FrameGrabError::NoParts);
    }
    if let Some(image) = parts.iter().find(|p| p.disc_image) {
        return Err(FrameGrabError::IsoNotSupported(image.path.clone()));
    }
    let total: f64 = parts.iter().map(|p| p.duration_secs.max(0.0)).sum();
    if total > 0.0 {
        Ok(total)
    } else {
        Err(FrameGrabError::UnknownDuration)
    }
}

fn create_dir(dir: &Path) -> Result<(), FrameGrabError> {
    fs::create_dir_all(dir).map_err(|source| FrameGrabError::Write {
        path: dir.to_path_buf(),
        source,
    })
}

/// Saves the frame at `timestamp_secs` of the stacked `parts` as a JPEG at `out`.
///
/// # Errors
/// [`FrameGrabError::IsoNotSupported`] for a disc image, [`FrameGrabError::Tool`] when FFmpeg is
/// not installed (the UI links to [`TOOLS_SETTINGS_LINK`]), [`FrameGrabError::UnknownDuration`]
/// when no part has a length, [`FrameGrabError::NoFrame`] when FFmpeg produced nothing,
/// [`FrameGrabError::Write`] when `out` cannot be written, [`FrameGrabError::Cancelled`] when
/// `cancel` is set and [`FrameGrabError::Start`] when FFmpeg cannot start.
pub fn grab_frame(
    tools: &ToolsManager,
    parts: &[AspectPart],
    timestamp_secs: f64,
    out: &Path,
    options: &FrameGrabOptions,
    cancel: &Cancel,
) -> Result<FrameCandidate, FrameGrabError> {
    let total = total_duration(parts)?;
    let ffmpeg = tools.resolve(ToolId::Ffmpeg)?;
    let at = timestamp_secs.clamp(0.0, (total - END_MARGIN_SECS).max(0.0));
    let (part, offset) = locate(parts, at);
    if let Some(dir) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
        create_dir(dir)?;
    }
    grab_one(
        &ffmpeg.path,
        &parts[part].path,
        offset,
        out,
        options,
        cancel,
    )?;
    Ok(FrameCandidate {
        path: out.to_path_buf(),
        timestamp_secs: at,
        part,
    })
}

/// Saves the frame from the middle of the video, the one an item without a thumb is offered.
///
/// # Errors
/// As [`grab_frame`].
pub fn grab_middle_frame(
    tools: &ToolsManager,
    parts: &[AspectPart],
    out: &Path,
    options: &FrameGrabOptions,
    cancel: &Cancel,
) -> Result<FrameCandidate, FrameGrabError> {
    let total = total_duration(parts)?;
    grab_frame(tools, parts, total / 2.0, out, options, cancel)
}

/// Saves `count` evenly spaced frames into `dir` as `frame-01.jpg`, `frame-02.jpg` and so on,
/// in timeline order, and returns them. A frame that FFmpeg cannot produce is left out as long
/// as at least one is made; if none is, the first failure is returned.
///
/// # Errors
/// As [`grab_frame`]; [`FrameGrabError::Cancelled`] stops the whole run.
pub fn grab_candidates(
    tools: &ToolsManager,
    parts: &[AspectPart],
    count: usize,
    dir: &Path,
    options: &FrameGrabOptions,
    cancel: &Cancel,
) -> Result<Vec<FrameCandidate>, FrameGrabError> {
    let total = total_duration(parts)?;
    grab_at(
        tools,
        parts,
        &candidate_timestamps(total, count),
        dir,
        options,
        cancel,
    )
}

/// As [`grab_candidates`], with the frames spread evenly between the fractions `from` and `to`
/// of the video's length (see [`spread_timestamps`]): the way the artwork picker offers frames,
/// from 10% to 90% so the opening and closing credits are left out.
///
/// # Errors
/// As [`grab_candidates`].
pub fn grab_spread(
    tools: &ToolsManager,
    parts: &[AspectPart],
    count: usize,
    (from, to): (f64, f64),
    dir: &Path,
    options: &FrameGrabOptions,
    cancel: &Cancel,
) -> Result<Vec<FrameCandidate>, FrameGrabError> {
    let total = total_duration(parts)?;
    grab_at(
        tools,
        parts,
        &spread_timestamps(total, count, from, to),
        dir,
        options,
        cancel,
    )
}

fn grab_at(
    tools: &ToolsManager,
    parts: &[AspectPart],
    timestamps: &[f64],
    dir: &Path,
    options: &FrameGrabOptions,
    cancel: &Cancel,
) -> Result<Vec<FrameCandidate>, FrameGrabError> {
    let ffmpeg = tools.resolve(ToolId::Ffmpeg)?;
    create_dir(dir)?;
    let mut made = Vec::new();
    let mut first_failure = None;
    for (index, &at) in timestamps.iter().enumerate() {
        if cancel.is_cancelled() {
            return Err(FrameGrabError::Cancelled);
        }
        let (part, offset) = locate(parts, at);
        let out = dir.join(format!("frame-{:02}.jpg", index + 1));
        match grab_one(
            &ffmpeg.path,
            &parts[part].path,
            offset,
            &out,
            options,
            cancel,
        ) {
            Ok(()) => made.push(FrameCandidate {
                path: out,
                timestamp_secs: at,
                part,
            }),
            Err(FrameGrabError::Cancelled) => return Err(FrameGrabError::Cancelled),
            Err(err) => {
                tracing::warn!(error = %err, "a frame candidate could not be made");
                first_failure.get_or_insert(err);
            }
        }
    }
    match (made.is_empty(), first_failure) {
        (true, Some(err)) => Err(err),
        _ => Ok(made),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(name: &str, secs: f64) -> AspectPart {
        AspectPart::new(name, secs)
    }

    #[test]
    fn eight_frames_spread_from_ten_to_ninety_percent() {
        let at = spread_timestamps(1000.0, 8, 0.1, 0.9);
        assert_eq!(at.len(), 8);
        assert!((at[0] - 100.0).abs() < 1e-9);
        assert!((at[7] - 900.0).abs() < 1e-9);
        assert!(at.windows(2).all(|w| w[1] > w[0]), "timeline order: {at:?}");
        let gaps: Vec<f64> = at.windows(2).map(|w| w[1] - w[0]).collect();
        assert!(
            gaps.iter().all(|g| (g - gaps[0]).abs() < 1e-9),
            "even: {gaps:?}"
        );
        assert_eq!(spread_timestamps(100.0, 1, 0.1, 0.9), vec![50.0]);
        assert!(spread_timestamps(0.0, 8, 0.1, 0.9).is_empty());
        for at in spread_timestamps(0.4, 3, 0.1, 0.9) {
            assert!((0.0..=0.4).contains(&at), "{at}");
        }
    }

    #[test]
    fn one_candidate_is_the_middle() {
        assert_eq!(candidate_timestamps(100.0, 1), vec![50.0]);
    }

    #[test]
    fn candidates_are_the_centres_of_equal_slices() {
        assert_eq!(candidate_timestamps(100.0, 4), vec![12.5, 37.5, 62.5, 87.5]);
        assert!(candidate_timestamps(100.0, 0).is_empty());
        assert!(candidate_timestamps(0.0, 3).is_empty());
    }

    #[test]
    fn a_tiny_video_keeps_candidates_inside_it() {
        for at in candidate_timestamps(0.4, 3) {
            assert!((0.0..=0.4).contains(&at), "{at}");
        }
    }

    #[test]
    fn a_timestamp_maps_to_the_part_that_plays_it() {
        let parts = [part("a.mkv", 100.0), part("b.mkv", 50.0)];
        assert_eq!(locate(&parts, 40.0), (0, 40.0));
        assert_eq!(locate(&parts, 100.0), (1, 0.0));
        assert_eq!(locate(&parts, 120.0), (1, 20.0));
        let (idx, secs) = locate(&parts, 500.0);
        assert_eq!(idx, 1);
        assert!((secs - 49.5).abs() < 1e-9);
    }

    #[test]
    fn parts_without_a_length_are_passed_over() {
        let parts = [part("a.mkv", 0.0), part("b.mkv", 60.0)];
        assert_eq!(locate(&parts, 10.0), (1, 10.0));
    }

    #[test]
    fn the_filter_widens_anamorphic_video_and_limits_the_width() {
        assert_eq!(filter(&FrameGrabOptions::default()), "null");
        let options = FrameGrabOptions {
            pixel_aspect_ratio: 1.4568,
            max_width: Some(1280),
            ..FrameGrabOptions::default()
        };
        let chain = filter(&options);
        assert!(
            chain.starts_with("scale=trunc(iw*1.4568/2)*2:ih,setsar=1,"),
            "{chain}"
        );
        assert!(chain.ends_with("scale='min(1280,iw)':-2"), "{chain}");
    }

    #[test]
    fn quality_maps_onto_ffmpeg_qscale() {
        assert_eq!(qscale(100), 2);
        assert_eq!(qscale(1), 31);
        assert!((2..=5).contains(&qscale(90)));
        assert_eq!(qscale(0), 31);
    }

    #[test]
    fn the_arguments_seek_before_the_input_and_write_one_jpeg_frame() {
        let args = grab_args(
            Path::new("in.mkv"),
            12.5,
            Path::new("out.jpg"),
            &FrameGrabOptions::default(),
        );
        let text: Vec<String> = args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let ss = text.iter().position(|a| a == "-ss").expect("-ss");
        let input = text.iter().position(|a| a == "-i").expect("-i");
        assert!(ss < input);
        assert_eq!(text[ss + 1], "12.500");
        assert!(text.windows(2).any(|w| w == ["-frames:v", "1"]));
        assert_eq!(text.last().map(String::as_str), Some("out.jpg"));
    }

    #[test]
    fn requests_without_files_or_length_or_with_a_disc_image_are_refused() {
        let empty = tempfile::tempdir().expect("temp dir");
        let tools = ToolsManager::new(empty.path());
        let cancel = Cancel::new();
        let out = Path::new("unused.jpg");
        let options = FrameGrabOptions::default();

        let err = grab_frame(&tools, &[], 1.0, out, &options, &cancel).expect_err("no parts");
        assert!(matches!(err, FrameGrabError::NoParts));
        let err = grab_frame(&tools, &[part("a.mkv", 0.0)], 1.0, out, &options, &cancel)
            .expect_err("no length");
        assert!(matches!(err, FrameGrabError::UnknownDuration));
        let iso = [part("D:\\Disc.ISO", 7200.0)];
        let err = grab_frame(&tools, &iso, 1.0, out, &options, &cancel).expect_err("iso");
        assert!(matches!(err, FrameGrabError::IsoNotSupported(_)), "{err}");
        assert_eq!(err.code(), "iso_not_supported");
    }

    #[test]
    fn a_missing_ffmpeg_is_a_typed_error_that_links_to_the_tools_page() {
        let empty = tempfile::tempdir().expect("temp dir");
        let tools = ToolsManager::new(empty.path());
        let parts = [part("D:\\Films\\Film.mkv", 7200.0)];
        let err = grab_middle_frame(
            &tools,
            &parts,
            &empty.path().join("out.jpg"),
            &FrameGrabOptions::default(),
            &Cancel::new(),
        )
        .expect_err("no FFmpeg");
        assert!(
            matches!(
                err,
                FrameGrabError::Tool(ToolError::NotInstalled(ToolId::Ffmpeg))
            ),
            "{err}"
        );
        assert_eq!(err.code(), "tool_missing");
        assert_eq!(err.settings_link(), Some("/settings/tools"));
    }
}
