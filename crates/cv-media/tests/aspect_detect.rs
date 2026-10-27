//! Aspect-ratio detection against real FFmpeg on sample videos made in the test: letterboxed,
//! pillarboxed, full-frame and multi-format clips built with the `pad` filter. Results must be
//! within 0.01 of the true shape.
//!
//! The tests need FFmpeg (`bun run tools:fetch -- ffmpeg`). Without it each one prints why and
//! returns. The clips are small (640x360, a few seconds, written to a temp folder).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use cv_domain::{MediaFile, MediaFileType};
use cv_media::aspect::{
    ArdMode, ArdSettings, AspectDetection, AspectError, AspectPart, AspectRatioList, AspectRequest,
    MultiFormat, detect_aspect_ratio,
};
use cv_media::tools::{Cancel, ToolId, ToolsManager, dev_cache_dir};

struct Ffmpeg {
    tools: ToolsManager,
    path: PathBuf,
}

fn ffmpeg() -> Option<Ffmpeg> {
    let tools = ToolsManager::new(dev_cache_dir());
    match tools.resolve(ToolId::Ffmpeg) {
        Ok(found) => Some(Ffmpeg {
            path: found.path,
            tools,
        }),
        Err(_) => {
            eprintln!("skipped: FFmpeg is not in the dev tools cache; run `bun run tools:fetch`");
            None
        }
    }
}

/// Encodes `args` (inputs and filters) into `out` with a keyframe every second, so sample seeks
/// land close to where they are aimed.
fn encode(ffmpeg: &Ffmpeg, args: &[&str], out: &Path) {
    let status = Command::new(&ffmpeg.path)
        .args(["-v", "error", "-y"])
        .args(args)
        .args(["-c:v", "libx264", "-preset", "ultrafast", "-g", "5"])
        .args(["-pix_fmt", "yuv420p", "-an"])
        .arg(out)
        .stdin(Stdio::null())
        .status()
        .expect("FFmpeg starts");
    assert!(status.success(), "FFmpeg could not make {}", out.display());
}

/// A clip of `seconds` whose picture is `content_w` x `content_h`, centred in a 640x360 frame.
fn bars_clip(
    ffmpeg: &Ffmpeg,
    dir: &Path,
    name: &str,
    content: (u32, u32),
    seconds: u32,
) -> PathBuf {
    let out = dir.join(name);
    let (w, h) = content;
    let source = format!("testsrc2=size={w}x{h}:rate=5:duration={seconds}");
    let pad = "pad=640:360:(ow-iw)/2:(oh-ih)/2:black";
    encode(ffmpeg, &["-f", "lavfi", "-i", &source, "-vf", pad], &out);
    out
}

/// A clip that is 2.39:1 for the first `wide` seconds and 1.90:1 for the next `tall`.
fn multi_format_clip(ffmpeg: &Ffmpeg, dir: &Path, wide: u32, tall: u32) -> PathBuf {
    let out = dir.join("multi.mp4");
    let a = format!("testsrc2=size=640x268:rate=5:duration={wide}");
    let b = format!("testsrc2=size=640x336:rate=5:duration={tall}");
    encode(
        ffmpeg,
        &[
            "-f",
            "lavfi",
            "-i",
            &a,
            "-f",
            "lavfi",
            "-i",
            &b,
            "-filter_complex",
            "[0:v]pad=640:360:0:46:black[a];[1:v]pad=640:360:0:12:black[b];[a][b]concat=n=2:v=1:a=0[v]",
            "-map",
            "[v]",
        ],
        &out,
    );
    out
}

fn settings(mode: ArdMode) -> ArdSettings {
    ArdSettings {
        mode,
        ..ArdSettings::default()
    }
}

fn detect(
    ffmpeg: &Ffmpeg,
    parts: &[(&Path, f64)],
    settings: &ArdSettings,
) -> Result<AspectDetection, AspectError> {
    let request = AspectRequest {
        parts: parts
            .iter()
            .map(|(path, secs)| AspectPart::new(*path, *secs))
            .collect(),
        pixel_aspect_ratio: 1.0,
    };
    detect_aspect_ratio(&ffmpeg.tools, &request, settings, &Cancel::new())
}

fn assert_ratio(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() <= 0.01,
        "expected {expected} within 0.01, got {actual}"
    );
}

#[test]
fn letterboxed_clips_report_their_picture_shape_in_every_mode() {
    let Some(ffmpeg) = ffmpeg() else { return };
    let dir = tempfile::tempdir().expect("temp dir");
    // 640 wide pictures in a 640x360 frame; the heights are even, as cropdetect reports them.
    let cases = [
        ((640, 268), 2.39),
        ((640, 320), 2.00),
        ((640, 346), 1.85),
        ((640, 360), 1.78),
    ];
    for ((w, h), expected) in cases {
        let clip = bars_clip(&ffmpeg, dir.path(), &format!("lb-{h}.mp4"), (w, h), 20);
        for mode in [ArdMode::Fast, ArdMode::Standard, ArdMode::Accurate] {
            let found = detect(&ffmpeg, &[(&clip, 20.0)], &settings(mode)).expect("detects");
            assert_ratio(found.primary, expected);
            assert!(found.samples_used > 0 && found.samples_used <= found.samples_planned);
            assert_eq!(found.secondary, 0.0, "{h}p clip in {mode:?} mode");
        }
    }
}

#[test]
fn a_pillarboxed_clip_reports_a_narrow_ratio() {
    let Some(ffmpeg) = ffmpeg() else { return };
    let dir = tempfile::tempdir().expect("temp dir");
    // 4:3 picture, bars left and right.
    let clip = bars_clip(&ffmpeg, dir.path(), "pillar.mp4", (480, 360), 20);
    let found = detect(&ffmpeg, &[(&clip, 20.0)], &settings(ArdMode::Standard)).expect("detects");
    assert_ratio(found.primary, 1.33);

    // Applied to a file whose MediaInfo ratio is the 16:9 frame.
    let mut file = MediaFile {
        aspect_ratio: 640.0 / 360.0,
        ..MediaFile::new(MediaFileType::Video, "C:\\Movies", "pillar.mp4")
    };
    assert!(found.apply_to(&mut file));
    assert_ratio(file.aspect_ratio, 1.33);
}

#[test]
fn the_nearest_list_entry_wins_unless_rounding_up() {
    let Some(ffmpeg) = ffmpeg() else { return };
    let dir = tempfile::tempdir().expect("temp dir");
    // 640x272 is 2.353.
    let clip = bars_clip(&ffmpeg, dir.path(), "r.mp4", (640, 272), 20);
    let nearest = detect(&ffmpeg, &[(&clip, 20.0)], &settings(ArdMode::Standard)).expect("ok");
    assert_ratio(nearest.primary, 2.35);

    let up = ArdSettings {
        round_up: true,
        ratios: AspectRatioList::new([1.78, 2.40]),
        ..ArdSettings::default()
    };
    assert_ratio(
        detect(&ffmpeg, &[(&clip, 20.0)], &up).expect("ok").primary,
        2.40,
    );

    let no_list = ArdSettings {
        ratios: AspectRatioList::empty(),
        ..ArdSettings::default()
    };
    assert_ratio(
        detect(&ffmpeg, &[(&clip, 20.0)], &no_list)
            .expect("ok")
            .primary,
        2.353,
    );
}

#[test]
fn accurate_mode_finds_the_second_format_of_a_multi_format_film() {
    let Some(ffmpeg) = ffmpeg() else { return };
    let dir = tempfile::tempdir().expect("temp dir");
    // 40 s of 2.39:1, then 20 s of 1.90:1.
    let clip = multi_format_clip(&ffmpeg, dir.path(), 40, 20);

    let accurate = detect(&ffmpeg, &[(&clip, 60.0)], &settings(ArdMode::Accurate)).expect("ok");
    assert_ratio(accurate.primary, 2.39);
    assert_ratio(accurate.secondary, 1.90);
    assert!(accurate.samples_used >= 20, "{accurate:?}");

    // The taller format can be chosen as the main one.
    let higher = ArdSettings {
        multi_format: MultiFormat::Higher,
        ..settings(ArdMode::Accurate)
    };
    let found = detect(&ffmpeg, &[(&clip, 60.0)], &higher).expect("ok");
    assert_ratio(found.primary, 1.90);
    assert_ratio(found.secondary, 2.39);

    // Other modes never record a second format.
    let standard = detect(&ffmpeg, &[(&clip, 60.0)], &settings(ArdMode::Standard)).expect("ok");
    assert_eq!(standard.secondary, 0.0);
    assert_ratio(standard.primary, 2.39);

    // Applying an accurate result stores both ratios.
    let mut file = MediaFile {
        aspect_ratio: 1.78,
        ..MediaFile::new(MediaFileType::Video, "C:\\Movies", "multi.mp4")
    };
    // 2.39 is wider than the 16:9 frame, so it cannot be the picture: nothing is stored.
    assert!(!accurate.apply_to(&mut file));
    file.aspect_ratio = 2.40;
    assert!(accurate.apply_to(&mut file));
    assert_ratio(file.aspect_ratio, 2.39);
    assert_ratio(file.aspect_ratio2, 1.90);
}

#[test]
fn stacked_parts_are_sampled_as_one_timeline() {
    let Some(ffmpeg) = ffmpeg() else { return };
    let dir = tempfile::tempdir().expect("temp dir");
    let first = bars_clip(&ffmpeg, dir.path(), "cd1.mp4", (640, 268), 15);
    let second = bars_clip(&ffmpeg, dir.path(), "cd2.mp4", (640, 268), 15);
    let found = detect(
        &ffmpeg,
        &[(&first, 15.0), (&second, 15.0)],
        &settings(ArdMode::Standard),
    )
    .expect("detects");
    assert_ratio(found.primary, 2.39);
    assert_eq!(found.samples_planned, 6);
}

#[test]
fn an_unreadable_video_reports_no_usable_samples() {
    let Some(ffmpeg) = ffmpeg() else { return };
    let dir = tempfile::tempdir().expect("temp dir");
    let missing = dir.path().join("not-there.mkv");
    let err = detect(&ffmpeg, &[(&missing, 600.0)], &settings(ArdMode::Fast)).expect_err("fails");
    assert!(
        matches!(err, AspectError::NoUsableSamples { samples: 4 }),
        "{err}"
    );
}

#[test]
fn cancelling_stops_the_detection() {
    let Some(ffmpeg) = ffmpeg() else { return };
    let dir = tempfile::tempdir().expect("temp dir");
    let clip = bars_clip(&ffmpeg, dir.path(), "c.mp4", (640, 268), 10);
    let cancel = Cancel::new();
    cancel.cancel();
    let request = AspectRequest {
        parts: vec![AspectPart::new(&clip, 10.0)],
        pixel_aspect_ratio: 1.0,
    };
    let err = detect_aspect_ratio(&ffmpeg.tools, &request, &ArdSettings::default(), &cancel)
        .expect_err("cancelled");
    assert!(matches!(err, AspectError::Cancelled), "{err}");
}

#[test]
fn anamorphic_video_is_widened_by_its_pixel_aspect_ratio() {
    let Some(ffmpeg) = ffmpeg() else { return };
    let dir = tempfile::tempdir().expect("temp dir");
    // A 4:3 pixel frame (400x300) whose pixels are 4:3 wide is a 16:9 picture.
    let clip = bars_clip(&ffmpeg, dir.path(), "anamorphic.mp4", (400, 300), 20);
    let request = AspectRequest {
        parts: vec![AspectPart::new(&clip, 20.0)],
        pixel_aspect_ratio: 4.0 / 3.0,
    };
    let found = detect_aspect_ratio(
        &ffmpeg.tools,
        &request,
        &settings(ArdMode::Standard),
        &Cancel::new(),
    )
    .expect("detects");
    assert_ratio(found.primary, 1.78);
}
