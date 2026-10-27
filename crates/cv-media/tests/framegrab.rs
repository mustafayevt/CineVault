//! Frame-grab artwork against real FFmpeg: a clip of three colour bands (red, green, blue, two
//! seconds each) is made in the test, so the colour of a grabbed frame says which part of the
//! video it came from. The middle frame must be a valid JPEG of the clip's size and green.
//!
//! The tests need FFmpeg (`bun run tools:fetch -- ffmpeg`). Without it each one prints why and
//! returns. The clips are tiny (320x180, six seconds, written to a temp folder).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use cv_media::aspect::AspectPart;
use cv_media::framegrab::{
    FrameGrabError, FrameGrabOptions, grab_candidates, grab_frame, grab_middle_frame,
};
use cv_media::tools::{Cancel, ToolId, ToolsManager, dev_cache_dir};

const WIDTH: u32 = 320;
const HEIGHT: u32 = 180;

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

/// A six-second clip: red for 2 s, green for 2 s, blue for 2 s, a keyframe every second.
fn banded_clip(ffmpeg: &Ffmpeg, dir: &Path, name: &str) -> PathBuf {
    let out = dir.join(name);
    let mut command = Command::new(&ffmpeg.path);
    command.args(["-v", "error", "-y"]);
    for colour in ["red", "green", "blue"] {
        command.args([
            "-f",
            "lavfi",
            "-i",
            &format!("color=c={colour}:size={WIDTH}x{HEIGHT}:rate=5:duration=2"),
        ]);
    }
    let status = command
        .args(["-filter_complex", "[0:v][1:v][2:v]concat=n=3:v=1:a=0[v]"])
        .args(["-map", "[v]", "-c:v", "libx264", "-preset", "ultrafast"])
        .args(["-g", "5", "-pix_fmt", "yuv420p"])
        .arg(&out)
        .stdin(Stdio::null())
        .status()
        .expect("FFmpeg starts");
    assert!(status.success(), "FFmpeg could not make {}", out.display());
    out
}

/// The average colour of an image, read back with FFmpeg (scaled down to one pixel).
fn average_rgb(ffmpeg: &Ffmpeg, image: &Path) -> [u8; 3] {
    let output = Command::new(&ffmpeg.path)
        .args(["-v", "error", "-i"])
        .arg(image)
        .args(["-vf", "scale=1:1", "-frames:v", "1"])
        .args(["-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
        .stdin(Stdio::null())
        .output()
        .expect("FFmpeg starts");
    assert_eq!(output.stdout.len(), 3, "one RGB pixel");
    [output.stdout[0], output.stdout[1], output.stdout[2]]
}

/// Width and height from the first start-of-frame marker of a JPEG.
fn jpeg_size(bytes: &[u8]) -> (u32, u32) {
    assert_eq!(&bytes[..3], &[0xFF, 0xD8, 0xFF], "JPEG signature");
    let mut at = 2;
    while at + 9 < bytes.len() {
        assert_eq!(bytes[at], 0xFF, "marker at {at}");
        let marker = bytes[at + 1];
        let length = usize::from(u16::from_be_bytes([bytes[at + 2], bytes[at + 3]]));
        if matches!(marker, 0xC0..=0xC2) {
            let height = u32::from(u16::from_be_bytes([bytes[at + 5], bytes[at + 6]]));
            let width = u32::from(u16::from_be_bytes([bytes[at + 7], bytes[at + 8]]));
            return (width, height);
        }
        at += 2 + length;
    }
    panic!("no frame header in the JPEG");
}

fn is_dominant(rgb: [u8; 3], channel: usize) -> bool {
    let main = i32::from(rgb[channel]);
    rgb.iter()
        .enumerate()
        .filter(|(i, _)| *i != channel)
        .all(|(_, other)| main - i32::from(*other) > 100)
}

#[test]
fn the_middle_frame_is_a_jpeg_of_the_middle_of_the_video() {
    let Some(ffmpeg) = ffmpeg() else { return };
    let dir = tempfile::tempdir().expect("temp dir");
    let clip = banded_clip(&ffmpeg, dir.path(), "bands.mp4");
    let parts = [AspectPart::new(&clip, 6.0)];
    let out = dir.path().join("art").join("thumb.jpg");

    let frame = grab_middle_frame(
        &ffmpeg.tools,
        &parts,
        &out,
        &FrameGrabOptions::default(),
        &Cancel::new(),
    )
    .expect("middle frame");

    assert_eq!(frame.path, out);
    assert!((frame.timestamp_secs - 3.0).abs() < 1e-9);
    let bytes = std::fs::read(&out).expect("frame file");
    assert_eq!(jpeg_size(&bytes), (WIDTH, HEIGHT));
    let rgb = average_rgb(&ffmpeg, &out);
    assert!(is_dominant(rgb, 1), "the middle band is green, got {rgb:?}");
    assert!(
        !out.with_file_name("thumb.partial.jpg").exists(),
        "no partial file is left behind"
    );
}

#[test]
fn evenly_spaced_candidates_walk_through_the_video() {
    let Some(ffmpeg) = ffmpeg() else { return };
    let dir = tempfile::tempdir().expect("temp dir");
    let clip = banded_clip(&ffmpeg, dir.path(), "bands.mp4");
    let parts = [AspectPart::new(&clip, 6.0)];
    let art = dir.path().join("candidates");

    let frames = grab_candidates(
        &ffmpeg.tools,
        &parts,
        3,
        &art,
        &FrameGrabOptions::default(),
        &Cancel::new(),
    )
    .expect("candidates");

    assert_eq!(frames.len(), 3);
    let names: Vec<_> = frames
        .iter()
        .map(|f| f.path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["frame-01.jpg", "frame-02.jpg", "frame-03.jpg"]);
    for (frame, (at, channel)) in frames.iter().zip([(1.0, 0), (3.0, 1), (5.0, 2)]) {
        assert!((frame.timestamp_secs - at).abs() < 1e-9);
        let rgb = average_rgb(&ffmpeg, &frame.path);
        assert!(is_dominant(rgb, channel), "frame at {at} s, got {rgb:?}");
    }
}

#[test]
fn stacked_parts_form_one_timeline_and_the_width_can_be_limited() {
    let Some(ffmpeg) = ffmpeg() else { return };
    let dir = tempfile::tempdir().expect("temp dir");
    let first = banded_clip(&ffmpeg, dir.path(), "cd1.mp4");
    let second = banded_clip(&ffmpeg, dir.path(), "cd2.mp4");
    let parts = [AspectPart::new(&first, 6.0), AspectPart::new(&second, 6.0)];
    let out = dir.path().join("late.jpg");
    let options = FrameGrabOptions {
        max_width: Some(160),
        ..FrameGrabOptions::default()
    };

    // 11 s of the stack is 5 s into the second file: its blue band.
    let frame = grab_frame(&ffmpeg.tools, &parts, 11.0, &out, &options, &Cancel::new())
        .expect("frame from the second part");

    assert_eq!(frame.part, 1);
    let bytes = std::fs::read(&out).expect("frame file");
    assert_eq!(jpeg_size(&bytes), (160, 90));
    assert!(is_dominant(average_rgb(&ffmpeg, &out), 2));
}

#[test]
fn a_file_that_is_not_a_video_gives_no_frame_and_no_file() {
    let Some(ffmpeg) = ffmpeg() else { return };
    let dir = tempfile::tempdir().expect("temp dir");
    let bogus = dir.path().join("broken.mkv");
    std::fs::write(&bogus, b"not a video").expect("write");
    let out = dir.path().join("none.jpg");

    let err = grab_middle_frame(
        &ffmpeg.tools,
        &[AspectPart::new(&bogus, 60.0)],
        &out,
        &FrameGrabOptions::default(),
        &Cancel::new(),
    )
    .expect_err("nothing to grab");

    assert!(matches!(err, FrameGrabError::NoFrame { .. }), "{err}");
    assert_eq!(err.code(), "no_frame");
    assert!(!out.exists());
    assert!(!dir.path().join("none.partial.jpg").exists());
}
