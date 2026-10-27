//! Shared helpers of the disc tests: image and structure builders, generated sample streams and
//! the tools lookup.

#![allow(dead_code)]

pub mod images;
pub mod structure;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use cv_media::mediainfo::MediaInfoLibrary;
use cv_media::tools::{ToolId, ToolsManager, dev_cache_dir};

/// MediaInfo and FFmpeg from the development tools cache.
pub struct Tools {
    pub library: MediaInfoLibrary,
    pub ffmpeg: PathBuf,
}

/// The tools, or `None` (after saying why) so the test can return early.
pub fn tools() -> Option<Tools> {
    let manager = ToolsManager::new(dev_cache_dir());
    let missing = [ToolId::MediaInfo, ToolId::Ffmpeg]
        .into_iter()
        .find(|tool| manager.resolve(*tool).is_err());
    if let Some(tool) = missing {
        eprintln!("skipped: {tool} is not in the dev tools cache; run `bun run tools:fetch`");
        return None;
    }
    let library = MediaInfoLibrary::from_tools(&manager).expect("MediaInfo loads");
    let ffmpeg = manager.resolve(ToolId::Ffmpeg).ok()?.path;
    Some(Tools { library, ffmpeg })
}

fn run(ffmpeg: &Path, args: &[&str], out: &Path) {
    let status = Command::new(ffmpeg)
        .args(["-v", "error", "-y"])
        .args(args)
        .arg(out)
        .stdin(Stdio::null())
        .status()
        .expect("FFmpeg starts");
    assert!(status.success(), "FFmpeg failed for {}", out.display());
}

/// A few seconds of DVD-compliant MPEG-2 video and AC-3 audio in a program stream (a `.VOB`).
pub fn make_vob(ffmpeg: &Path, dir: &Path, seconds: u32) -> Vec<u8> {
    let out = dir.join(format!("sample-{seconds}.vob"));
    let seconds = seconds.to_string();
    run(
        ffmpeg,
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=720x576:rate=25",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000",
            "-t",
            &seconds,
            "-target",
            "pal-dvd",
            "-b:v",
            "1500k",
        ],
        &out,
    );
    std::fs::read(out).expect("VOB")
}

/// A few seconds of H.264 and AC-3 in a Blu-ray style transport stream (`.m2ts`: 188-byte
/// packets, each preceded by a 4-byte arrival time).
pub fn make_m2ts(ffmpeg: &Path, dir: &Path, seconds: u32) -> Vec<u8> {
    let out = dir.join(format!("sample-{seconds}.ts"));
    let length = seconds.to_string();
    run(
        ffmpeg,
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=640x360:rate=24",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000",
            "-t",
            &length,
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "ac3",
            "-f",
            "mpegts",
        ],
        &out,
    );
    let transport = std::fs::read(out).expect("TS");
    let mut bdav = Vec::with_capacity(transport.len() / 188 * 192);
    for (index, packet) in transport.as_chunks::<188>().0.iter().enumerate() {
        bdav.extend_from_slice(&(index as u32 & 0x3FFF_FFFF).to_be_bytes());
        bdav.extend_from_slice(packet);
    }
    bdav
}

/// Writes `files` below `root`, creating folders.
pub fn write_tree(root: &Path, files: &[(&str, Vec<u8>)]) {
    for (path, data) in files {
        let target = root.join(path);
        std::fs::create_dir_all(target.parent().expect("parent")).expect("folder");
        std::fs::write(target, data).expect("file");
    }
}

/// A file's bytes patterned so a misplaced read is noticed: byte `i` is `i * 7 + seed`.
pub fn pattern(len: usize, seed: u8) -> Vec<u8> {
    (0..len)
        .map(|i| (i as u8).wrapping_mul(7).wrapping_add(seed))
        .collect()
}
