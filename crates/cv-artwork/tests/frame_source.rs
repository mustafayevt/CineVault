//! Frame-grab artwork against real FFmpeg: a short test clip is made in the test, frames are
//! grabbed from it and offered as artwork candidates whose files exist and have the clip's size.
//!
//! The test needs FFmpeg (`bun run tools:fetch -- ffmpeg`). Without it the test prints why and
//! returns.

use std::path::Path;
use std::process::{Command, Stdio};

use cv_artwork::frame_source::{
    FrameKinds, MIDDLE_FRAME_FILE, PROVIDER_ID, frame_path, grab_artwork, grab_middle_artwork,
};
use cv_domain::MediaFileType;
use cv_media::aspect::AspectPart;
use cv_media::framegrab::FrameGrabOptions;
use cv_media::tools::{Cancel, ToolId, ToolsManager, dev_cache_dir};

fn clip(ffmpeg: &Path, dir: &Path) -> std::path::PathBuf {
    let out = dir.join("clip.mp4");
    let status = Command::new(ffmpeg)
        .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg("testsrc=size=320x180:rate=5:duration=6")
        .args(["-c:v", "libx264", "-preset", "ultrafast", "-g", "5"])
        .args(["-pix_fmt", "yuv420p"])
        .arg(&out)
        .stdin(Stdio::null())
        .status()
        .expect("FFmpeg starts");
    assert!(status.success());
    out
}

#[test]
fn grabbed_frames_become_fanart_and_thumb_candidates() {
    let tools = ToolsManager::new(dev_cache_dir());
    let Ok(found) = tools.resolve(ToolId::Ffmpeg) else {
        eprintln!("skipped: FFmpeg is not in the dev tools cache; run `bun run tools:fetch`");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let video = clip(&found.path, dir.path());
    let parts = [AspectPart::new(video, 6.0)];
    let frames_dir = dir.path().join("frames");
    let cancel = Cancel::new();
    let options = FrameGrabOptions::default();

    let list = grab_artwork(
        &tools,
        &parts,
        3,
        &frames_dir,
        FrameKinds::default(),
        &options,
        &cancel,
    )
    .unwrap();
    // Three frames, each as fanart and as thumb.
    assert_eq!(list.len(), 6);
    assert!(list.iter().all(|c| c.provider == PROVIDER_ID));
    let fanart = list
        .iter()
        .filter(|c| c.candidate.kind == MediaFileType::Fanart)
        .count();
    assert_eq!(fanart, 3);
    for item in &list {
        let path = frame_path(&item.candidate).expect("a file URL");
        assert!(path.is_file(), "{}", path.display());
        assert_eq!((item.candidate.width, item.candidate.height), (320, 180));
    }

    let middle = grab_middle_artwork(
        &tools,
        &parts,
        &frames_dir,
        FrameKinds {
            fanart: false,
            thumb: true,
        },
        &options,
        &cancel,
    )
    .unwrap();
    assert_eq!(middle.len(), 1);
    assert_eq!(middle[0].candidate.kind, MediaFileType::Thumb);
    assert!(
        frame_path(&middle[0].candidate)
            .unwrap()
            .ends_with(MIDDLE_FRAME_FILE)
    );
}
