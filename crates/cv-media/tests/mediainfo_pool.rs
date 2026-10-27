//! The real MediaInfo library against real files: 50 small sample videos are generated with the
//! FFmpeg from the development tools cache and read in parallel through the worker pool.
//!
//! The tests need the tools (`bun run tools:fetch -- mediainfo ffmpeg`). Without them each test
//! prints why and returns, so a machine that has not fetched the tools still passes.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use cv_media::mediainfo::{
    MediaInfoError, MediaInfoLibrary, MediaInfoPool, MediaSnapshot, PoolOptions, StreamKind,
};
use cv_media::tools::{ToolId, ToolsManager, dev_cache_dir};

const SAMPLES: usize = 50;

/// What a generated sample should look like to MediaInfo.
struct Expected {
    path: PathBuf,
    width: u64,
    height: u64,
    fps: f64,
    channels: u64,
    seconds: u64,
    video_format: &'static str,
    audio_format: &'static str,
}

struct Tools {
    manager: ToolsManager,
    ffmpeg: PathBuf,
}

fn tools() -> Option<Tools> {
    let manager = ToolsManager::new(dev_cache_dir());
    let missing = [ToolId::MediaInfo, ToolId::Ffmpeg]
        .into_iter()
        .find(|tool| manager.resolve(*tool).is_err());
    if let Some(tool) = missing {
        eprintln!("skipped: {tool} is not in the dev tools cache; run `bun run tools:fetch`");
        return None;
    }
    let ffmpeg = manager.resolve(ToolId::Ffmpeg).ok()?.path;
    Some(Tools { manager, ffmpeg })
}

/// Makes sample `i` in `dir`. The settings vary with `i`, so the files differ in container,
/// codecs, size, frame rate, channel count and length.
fn generate(ffmpeg: &Path, dir: &Path, i: usize) -> Expected {
    const SIZES: [(u64, u64); 5] = [(160, 90), (192, 108), (224, 126), (256, 144), (320, 180)];
    const RATES: [u64; 4] = [10, 12, 15, 25];
    let (width, height) = SIZES[i % SIZES.len()];
    let fps = RATES[i % RATES.len()];
    let channels = 1 + (i % 2) as u64;
    let seconds = 1 + (i % 3) as u64;
    let (extension, video_codec, video_format, audio_codec, audio_format) = match i % 3 {
        0 => ("mp4", "mpeg4", "MPEG-4 Visual", "aac", "AAC"),
        1 => ("mkv", "libx264", "AVC", "flac", "FLAC"),
        _ => ("mp4", "libx264", "AVC", "ac3", "AC-3"),
    };
    let path = dir.join(format!("sample-{i:02}.{extension}"));
    let status = Command::new(ffmpeg)
        .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg(format!("testsrc=size={width}x{height}:rate={fps}"))
        .args(["-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000"])
        .args(["-t", &seconds.to_string(), "-ac", &channels.to_string()])
        .args(["-pix_fmt", "yuv420p", "-c:v", video_codec])
        .args(["-preset", "ultrafast"])
        .args(["-c:a", audio_codec])
        .arg(&path)
        .stdin(Stdio::null())
        .status()
        .expect("FFmpeg starts");
    assert!(status.success(), "FFmpeg failed for {}", path.display());
    Expected {
        path,
        width,
        height,
        fps: fps as f64,
        channels,
        seconds,
        video_format,
        audio_format,
    }
}

/// Generates all samples, several FFmpeg processes at a time.
fn generate_samples(ffmpeg: &Path, dir: &Path) -> Vec<Expected> {
    let indexes: Vec<usize> = (0..SAMPLES).collect();
    let mut made = Vec::new();
    for chunk in indexes.chunks(8) {
        std::thread::scope(|scope| {
            let jobs: Vec<_> = chunk
                .iter()
                .map(|&i| scope.spawn(move || generate(ffmpeg, dir, i)))
                .collect();
            made.extend(jobs.into_iter().map(|job| job.join().expect("generator")));
        });
    }
    made
}

fn check(expected: &Expected, snapshot: &MediaSnapshot) {
    let name = expected.path.display();
    let general = snapshot.general().expect("general section");
    let duration_ms = general.get_u64("Duration").expect("duration") as i64;
    let wanted_ms = expected.seconds as i64 * 1000;
    assert!(
        (duration_ms - wanted_ms).abs() <= 200,
        "{name}: duration {duration_ms} ms, wanted about {wanted_ms} ms"
    );

    assert_eq!(snapshot.streams(StreamKind::Video).len(), 1, "{name}");
    let video = snapshot.video().expect("video stream");
    assert_eq!(video.get_u64("Width"), Some(expected.width), "{name}");
    assert_eq!(video.get_u64("Height"), Some(expected.height), "{name}");
    assert_eq!(video.get("Format"), Some(expected.video_format), "{name}");
    let rate = video.get_f64("FrameRate").expect("frame rate");
    assert!((rate - expected.fps).abs() < 0.01, "{name}: {rate} fps");

    assert_eq!(snapshot.streams(StreamKind::Audio).len(), 1, "{name}");
    let audio = &snapshot.streams(StreamKind::Audio)[0];
    assert_eq!(audio.get("Format"), Some(expected.audio_format), "{name}");
    assert_eq!(
        audio.get_u64("Channel(s)"),
        Some(expected.channels),
        "{name}"
    );
}

#[test]
fn fifty_sample_files_are_read_in_parallel_with_the_expected_values() {
    let Some(tools) = tools() else { return };
    let dir = tempfile::tempdir().expect("temp dir");
    let samples = generate_samples(&tools.ffmpeg, dir.path());
    assert_eq!(samples.len(), SAMPLES);
    let paths: Vec<PathBuf> = samples.iter().map(|s| s.path.clone()).collect();

    let library = MediaInfoLibrary::from_tools(&tools.manager).expect("library loads");
    let limit = 4;
    let parallel = MediaInfoPool::new(
        library.clone(),
        PoolOptions {
            limit,
            ..PoolOptions::default()
        },
    );
    let started = Instant::now();
    let items = parallel.extract_all(&paths);
    let parallel_time = started.elapsed();

    assert_eq!(items.len(), SAMPLES);
    let mut snapshots = Vec::new();
    for (expected, item) in samples.iter().zip(&items) {
        assert_eq!(item.path(), expected.path);
        let snapshot = item
            .result
            .as_ref()
            .unwrap_or_else(|e| panic!("{}: {e}", expected.path.display()));
        check(expected, snapshot);
        snapshots.push(snapshot.clone());
    }

    // The same files read one at a time give identical answers, so concurrent instances do not
    // disturb each other.
    let serial = MediaInfoPool::new(
        library,
        PoolOptions {
            limit: 1,
            ..PoolOptions::default()
        },
    );
    let started = Instant::now();
    let again = serial.extract_all(&paths);
    let serial_time = started.elapsed();
    for (item, snapshot) in again.iter().zip(&snapshots) {
        assert_eq!(item.result.as_ref().expect("serial read"), snapshot);
    }
    assert_eq!(serial.peak_concurrency(), 1);
    assert!(parallel.peak_concurrency() <= limit);
    assert_eq!(parallel.abandoned(), 0);
    eprintln!(
        "50 files: {parallel_time:?} with {limit} readers (peak {}), {serial_time:?} with one",
        parallel.peak_concurrency()
    );
}

#[test]
fn a_missing_or_corrupt_file_fails_alone_and_the_good_files_are_read() {
    let Some(tools) = tools() else { return };
    let dir = tempfile::tempdir().expect("temp dir");
    let good: Vec<Expected> = (0..6)
        .map(|i| generate(&tools.ffmpeg, dir.path(), i))
        .collect();

    let garbage = dir.path().join("garbage.mkv");
    let noise: Vec<u8> = (0..64 * 1024_u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
        .collect();
    std::fs::write(&garbage, noise).expect("garbage file");
    let empty = dir.path().join("empty.mp4");
    std::fs::write(&empty, b"").expect("empty file");
    let missing = dir.path().join("missing.mkv");

    let mut paths: Vec<PathBuf> = good.iter().map(|g| g.path.clone()).collect();
    paths.insert(2, garbage.clone());
    paths.insert(4, missing.clone());
    paths.push(empty.clone());

    let library = MediaInfoLibrary::from_tools(&tools.manager).expect("library loads");
    let pool = MediaInfoPool::new(library, PoolOptions::default());
    let items = pool.extract_all(&paths);
    assert_eq!(items.len(), paths.len());

    for item in &items {
        if item.path() == missing {
            assert!(
                matches!(item.result, Err(MediaInfoError::Open { .. })),
                "{:?}",
                item.result
            );
        } else if item.path() == garbage || item.path() == empty {
            // The library opens any file; a corrupt one simply has no media streams.
            let snapshot = item.result.as_ref().expect("opens");
            assert!(!snapshot.has_media_streams(), "{}", item.path().display());
        } else {
            let expected = good.iter().find(|g| g.path == item.path()).expect("sample");
            check(expected, item.result.as_ref().expect("good file"));
        }
    }
}

#[test]
fn file_names_with_accents_and_other_scripts_are_opened() {
    let Some(tools) = tools() else { return };
    let dir = tempfile::tempdir().expect("temp dir");
    let sample = generate(&tools.ffmpeg, dir.path(), 0);
    let odd = dir.path().join("Amélie (2001) 日本語.mp4");
    std::fs::rename(&sample.path, &odd).expect("rename");
    let library = MediaInfoLibrary::from_tools(&tools.manager).expect("library loads");
    let snapshot = library.read(&odd).expect("reads a non-ASCII path");
    check(
        &Expected {
            path: odd,
            ..sample
        },
        &snapshot,
    );
}

#[test]
fn loading_reports_the_version_and_rejects_files_that_are_not_mediainfo() {
    let Some(tools) = tools() else { return };
    let library = MediaInfoLibrary::from_tools(&tools.manager).expect("library loads");
    assert!(
        library
            .version()
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit()),
        "version {:?}",
        library.version()
    );

    // A user-chosen library cannot be run to ask its version; the loader reports it instead.
    let other = ToolsManager::new(tempfile::tempdir().expect("temp dir").path());
    other
        .set_override(ToolId::MediaInfo, Some(library.path()))
        .expect("override accepted");
    let loaded = MediaInfoLibrary::from_tools(&other).expect("loads from the override");
    let resolved = other.resolve(ToolId::MediaInfo).expect("resolves");
    assert_eq!(
        other.version_of(&resolved).as_deref(),
        Some(loaded.version())
    );

    // Not a library at all.
    let dir = tempfile::tempdir().expect("temp dir");
    let text = dir
        .path()
        .join(if cfg!(windows) { "x.dll" } else { "libx.so" });
    std::fs::write(&text, "not a library").expect("write");
    let error = MediaInfoLibrary::load(&text).expect_err("not loadable");
    assert_eq!(error.code(), "load");

    // A real library that is not MediaInfo.
    #[cfg(windows)]
    {
        let error = MediaInfoLibrary::load("kernel32.dll").expect_err("not MediaInfo");
        assert_eq!(error.code(), "unusable", "{error}");
    }

    // Nothing installed and no path: the tools manager's own error comes through.
    let empty = ToolsManager::new(dir.path().join("none"));
    let error = MediaInfoLibrary::from_tools(&empty).expect_err("nothing installed");
    assert_eq!(error.code(), "tool");
}
