//! Reads real videos with the app's MediaInfo reader (the pool with its per-drive limit and
//! size-scaled time limit, disc folders through their main title) and prints the time of each.
//! It only reads: nothing is written next to the videos and no library is touched.
//!
//! Opt-in: set `CINEVAULT_LIVE_LIBRARY=1` and `CINEVAULT_LIVE_PATHS` to the videos to read,
//! separated by `;` (a disc may be given as its folder or its `BDMV\index.bdmv`). MediaInfo comes
//! from `.dev-tools` (`bun run tools:fetch -- mediainfo`). Without them the test is skipped.

use std::path::PathBuf;
use std::time::Instant;

use cv_app::media_info::{LibraryReader, MediaReader};
use cv_domain::{MediaFile, MediaFileType};
use cv_media::mediainfo::apply_snapshot;
use cv_media::tools::{ToolsManager, dev_cache_dir};

#[test]
fn real_videos_give_codec_and_resolution() {
    if std::env::var_os("CINEVAULT_LIVE_LIBRARY").is_none() {
        eprintln!("skipped: set CINEVAULT_LIVE_LIBRARY=1 and CINEVAULT_LIVE_PATHS");
        return;
    }
    let paths: Vec<PathBuf> = std::env::var("CINEVAULT_LIVE_PATHS")
        .unwrap_or_default()
        .split(';')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .collect();
    assert!(!paths.is_empty(), "CINEVAULT_LIVE_PATHS names no video");
    let reader = LibraryReader::from_tools(&ToolsManager::new(dev_cache_dir()))
        .expect("MediaInfo in .dev-tools");
    let mut failures = Vec::new();
    for path in &paths {
        let started = Instant::now();
        let result = reader.read(path);
        let seconds = started.elapsed().as_secs_f64();
        match result {
            Ok(snapshot) => {
                let mut file = MediaFile::new(MediaFileType::Video, "", "");
                apply_snapshot(&mut file, &snapshot);
                let audio: Vec<String> = file
                    .audio_streams
                    .iter()
                    .map(|a| format!("{} {}ch", a.codec, a.channels))
                    .collect();
                eprintln!(
                    "{seconds:7.1}s  {} {}x{} {}s hdr=[{}] audio=[{}] subs={}  {}",
                    file.video_codec,
                    file.width,
                    file.height,
                    file.duration_secs,
                    file.hdr_formats.join(", "),
                    audio.join(", "),
                    file.subtitles.len(),
                    path.display()
                );
                if file.video_codec.is_empty() || file.width == 0 || file.height == 0 {
                    failures.push(format!("{}: no codec or resolution", path.display()));
                }
            }
            Err(error) => {
                eprintln!("{seconds:7.1}s  FAILED {error}  {}", path.display());
                failures.push(format!("{}: {error}", path.display()));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
