//! Frame grabs as an artwork source: stills taken from the video itself, offered next to the
//! online artwork in the picker.
//!
//! Spec: `artwork` (Artwork providers: video frame grabs). The grabbing is `cv_media::framegrab`; this module turns its frames into
//! candidates:
//!
//! * A frame is a local file, so its candidate URL is a `file://` URL ([`file_url`]); [`frame_path`]
//!   turns it back into the path.
//! * A frame can be offered as fanart (a background), as a thumb, or as both, as the settings say.
//! * Frames always rank below online artwork ([`rank_below_online`]): the best-image selection
//!   reaches a frame only when no online image fits, and then uses the middle one
//!   ([`grab_middle_artwork`]).

use std::path::{Path, PathBuf};

use cv_domain::MediaFileType;
use cv_media::aspect::AspectPart;
use cv_media::framegrab::{
    FrameCandidate, FrameGrabError, FrameGrabOptions, grab_candidates, grab_middle_frame,
    grab_spread,
};
use cv_media::tools::{Cancel, ToolsManager};
use cv_providers::SourcedArtwork;
use cv_providers::artwork::FRAME_GRAB_ID;
use cv_providers::model::ArtworkCandidate;

/// The provider key frame candidates are tagged with.
pub const PROVIDER_ID: &str = FRAME_GRAB_ID;

/// File name of the middle frame inside the frame folder.
pub const MIDDLE_FRAME_FILE: &str = "frame-middle.jpg";

/// Which artwork types the frames are offered as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameKinds {
    pub fanart: bool,
    pub thumb: bool,
}

impl Default for FrameKinds {
    fn default() -> Self {
        Self {
            fanart: true,
            thumb: true,
        }
    }
}

impl FrameKinds {
    fn kinds(self) -> impl Iterator<Item = MediaFileType> {
        [
            self.fanart.then_some(MediaFileType::Fanart),
            self.thumb.then_some(MediaFileType::Thumb),
        ]
        .into_iter()
        .flatten()
    }
}

/// Candidates for frames already on disk: all the fanart ones in timeline order, then all the
/// thumb ones. A frame whose size cannot be read is offered with size `0x0`.
pub fn frame_candidates(frames: &[FrameCandidate], kinds: FrameKinds) -> Vec<SourcedArtwork> {
    let sized: Vec<(&FrameCandidate, u32, u32)> = frames
        .iter()
        .map(|frame| {
            let (width, height) = image::image_dimensions(&frame.path).unwrap_or_else(|error| {
                tracing::warn!(%error, path = %frame.path.display(), "frame size unknown");
                (0, 0)
            });
            (frame, width, height)
        })
        .collect();
    let mut out = Vec::new();
    for kind in kinds.kinds() {
        for (frame, width, height) in &sized {
            out.push(SourcedArtwork {
                provider: PROVIDER_ID,
                candidate: ArtworkCandidate {
                    kind,
                    url: file_url(&frame.path),
                    language: None,
                    width: *width,
                    height: *height,
                    score: 0.0,
                    popularity: cv_providers::model::Popularity::Unknown,
                },
            });
        }
    }
    out
}

/// Saves `count` evenly spaced frames of the video into `dir` and offers them.
///
/// # Errors
/// As [`cv_media::framegrab::grab_candidates`]: a missing FFmpeg, a disc image, an unknown length,
/// or cancellation.
pub fn grab_artwork(
    tools: &ToolsManager,
    parts: &[AspectPart],
    count: usize,
    dir: &Path,
    kinds: FrameKinds,
    options: &FrameGrabOptions,
    cancel: &Cancel,
) -> Result<Vec<SourcedArtwork>, FrameGrabError> {
    let frames = grab_candidates(tools, parts, count, dir, options, cancel)?;
    Ok(frame_candidates(&frames, kinds))
}

/// How many frames the artwork picker offers for one video.
pub const PICKER_FRAMES: usize = 8;

/// The part of the video the picker's frames are taken from, as fractions of its length: 10% to
/// 90%, so the opening and closing credits are left out.
pub const PICKER_SPAN: (f64, f64) = (0.10, 0.90);

/// Saves the picker's frames ([`PICKER_FRAMES`] evenly spaced over [`PICKER_SPAN`]) into `dir`
/// and offers them as `kinds` say: all the fanart ones in timeline order, then all the thumb ones.
///
/// # Errors
/// As [`cv_media::framegrab::grab_spread`]: a missing FFmpeg, a disc image, an unknown length,
/// or cancellation.
pub fn grab_picker_artwork(
    tools: &ToolsManager,
    parts: &[AspectPart],
    dir: &Path,
    kinds: FrameKinds,
    options: &FrameGrabOptions,
    cancel: &Cancel,
) -> Result<Vec<SourcedArtwork>, FrameGrabError> {
    let frames = grab_spread(
        tools,
        parts,
        PICKER_FRAMES,
        PICKER_SPAN,
        dir,
        options,
        cancel,
    )?;
    Ok(frame_candidates(&frames, kinds))
}

/// Saves the frame from the middle of the video as `dir/frame-middle.jpg` and offers it. This is
/// the one image an item with no online artwork of a type falls back to.
///
/// # Errors
/// As [`cv_media::framegrab::grab_middle_frame`].
pub fn grab_middle_artwork(
    tools: &ToolsManager,
    parts: &[AspectPart],
    dir: &Path,
    kinds: FrameKinds,
    options: &FrameGrabOptions,
    cancel: &Cancel,
) -> Result<Vec<SourcedArtwork>, FrameGrabError> {
    let frame = grab_middle_frame(tools, parts, &dir.join(MIDDLE_FRAME_FILE), options, cancel)?;
    Ok(frame_candidates(&[frame], kinds))
}

/// The online candidates followed by the frame candidates, so that for every artwork type each
/// online image precedes each frame.
pub fn rank_below_online(
    online: Vec<SourcedArtwork>,
    frames: Vec<SourcedArtwork>,
) -> Vec<SourcedArtwork> {
    let mut out = online;
    out.extend(frames);
    out
}

/// Whether a candidate is a frame grab (its image is a local file).
pub fn is_frame(candidate: &SourcedArtwork) -> bool {
    candidate.provider == PROVIDER_ID
}

// --- file URLs --------------------------------------------------------------------------------

/// A `file:///` URL for an absolute path. Letters, digits and `-._~/:` stay as they are; every
/// other byte is percent-encoded. Back slashes become `/`, and a Windows drive path
/// (`C:\clips\a.jpg`) gets the usual third slash (`file:///C:/clips/a.jpg`).
pub fn file_url(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let mut url = String::from("file://");
    if !text.starts_with('/') {
        url.push('/');
    }
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/:".contains(&byte) {
            url.push(char::from(byte));
        } else {
            url.push_str(&format!("%{byte:02X}"));
        }
    }
    url
}

/// The path a [`file_url`] was made from. `None` for any other URL or invalid escapes.
pub fn path_from_file_url(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("file://")?;
    let mut bytes = Vec::with_capacity(rest.len());
    let mut input = rest.bytes();
    while let Some(byte) = input.next() {
        if byte == b'%' {
            let high = input.next()?;
            let low = input.next()?;
            let hex = [high, low];
            let value = u8::from_str_radix(std::str::from_utf8(&hex).ok()?, 16).ok()?;
            bytes.push(value);
        } else {
            bytes.push(byte);
        }
    }
    let decoded = String::from_utf8(bytes).ok()?;
    // A host (`file://server/x`) or a UNC path (`file:////server/share/x`) would reach out to
    // another machine; frame grabs are always local files.
    let host = !decoded.is_empty() && !decoded.starts_with('/');
    let unc = decoded.starts_with("//") || decoded.contains('\\');
    if host || unc {
        return None;
    }
    // `/C:/clips/a.jpg` is a drive path.
    let drive = decoded.as_bytes();
    let path = if drive.len() >= 3
        && drive[0] == b'/'
        && drive[1].is_ascii_alphabetic()
        && drive[2] == b':'
    {
        &decoded[1..]
    } else {
        &decoded[..]
    };
    Some(PathBuf::from(path))
}

/// The file of a frame candidate.
pub fn frame_path(candidate: &ArtworkCandidate) -> Option<PathBuf> {
    path_from_file_url(&candidate.url)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jpeg(dir: &Path, name: &str, width: u32, height: u32) -> FrameCandidate {
        let path = dir.join(name);
        image::RgbImage::new(width, height).save(&path).unwrap();
        FrameCandidate {
            path,
            timestamp_secs: 1.0,
            part: 0,
        }
    }

    #[test]
    fn frames_are_offered_as_fanart_then_thumbs_with_their_real_size() {
        let dir = tempfile::tempdir().unwrap();
        let frames = [
            jpeg(dir.path(), "frame-01.jpg", 320, 180),
            jpeg(dir.path(), "frame-02.jpg", 320, 180),
        ];
        let list = frame_candidates(&frames, FrameKinds::default());
        let kinds: Vec<MediaFileType> = list.iter().map(|c| c.candidate.kind).collect();
        assert_eq!(
            kinds,
            [
                MediaFileType::Fanart,
                MediaFileType::Fanart,
                MediaFileType::Thumb,
                MediaFileType::Thumb
            ]
        );
        assert!(list.iter().all(is_frame));
        assert!(
            list.iter()
                .all(|c| c.candidate.language.is_none() && c.candidate.score == 0.0)
        );
        assert_eq!(
            (list[0].candidate.width, list[0].candidate.height),
            (320, 180)
        );
        // The files are found again from their URLs, in timeline order.
        assert_eq!(frame_path(&list[0].candidate), Some(frames[0].path.clone()));
        assert_eq!(frame_path(&list[1].candidate), Some(frames[1].path.clone()));
    }

    #[test]
    fn the_settings_choose_which_types_frames_are_offered_as() {
        let dir = tempfile::tempdir().unwrap();
        let frames = [jpeg(dir.path(), "frame-01.jpg", 64, 36)];
        let only = |fanart, thumb| {
            frame_candidates(&frames, FrameKinds { fanart, thumb })
                .iter()
                .map(|c| c.candidate.kind)
                .collect::<Vec<_>>()
        };
        assert_eq!(only(true, false), [MediaFileType::Fanart]);
        assert_eq!(only(false, true), [MediaFileType::Thumb]);
        assert!(only(false, false).is_empty());
    }

    #[test]
    fn a_frame_that_cannot_be_read_is_still_offered_without_a_size() {
        let frame = FrameCandidate {
            path: PathBuf::from("does-not-exist/frame-01.jpg"),
            timestamp_secs: 0.0,
            part: 0,
        };
        let list = frame_candidates(&[frame], FrameKinds::default());
        assert_eq!(list.len(), 2);
        assert_eq!((list[0].candidate.width, list[0].candidate.height), (0, 0));
    }

    #[test]
    fn frames_rank_below_every_online_image() {
        let online = |provider: &'static str, kind, url: &str| SourcedArtwork {
            provider,
            candidate: ArtworkCandidate {
                kind,
                url: url.to_owned(),
                language: None,
                width: 1920,
                height: 1080,
                score: 1.0,
                popularity: cv_providers::model::Popularity::Unknown,
            },
        };
        let frame = online(
            PROVIDER_ID,
            MediaFileType::Fanart,
            "file:///tmp/frame-01.jpg",
        );
        let list = rank_below_online(
            vec![
                online("tmdb", MediaFileType::Thumb, "https://i.test/a.jpg"),
                online("fanart", MediaFileType::Fanart, "https://i.test/b.jpg"),
            ],
            vec![frame],
        );
        let providers: Vec<&str> = list.iter().map(|c| c.provider).collect();
        assert_eq!(providers, ["tmdb", "fanart", "ffmpeg"]);
        assert!(!is_frame(&list[0]) && is_frame(&list[2]));
    }

    #[test]
    fn file_urls_round_trip_for_unix_and_windows_paths() {
        for (path, url) in [
            (
                "/tmp/frames/frame-01.jpg",
                "file:///tmp/frames/frame-01.jpg",
            ),
            (
                "/home/a b/Caf\u{e9} (1999)/frame.jpg",
                "file:///home/a%20b/Caf%C3%A9%20%281999%29/frame.jpg",
            ),
            (
                "C:\\Users\\Alex\\Movies\\Heat #2\\frame-01.jpg",
                "file:///C:/Users/Alex/Movies/Heat%20%232/frame-01.jpg",
            ),
        ] {
            assert_eq!(file_url(Path::new(path)), url, "{path}");
            let back = path_from_file_url(url).unwrap();
            assert_eq!(back, PathBuf::from(path.replace('\\', "/")), "{url}");
        }
    }

    #[test]
    fn other_urls_and_broken_escapes_are_not_frame_paths() {
        assert_eq!(path_from_file_url("https://i.test/a.jpg"), None);
        assert_eq!(path_from_file_url("file:///tmp/a%2"), None);
        assert_eq!(path_from_file_url("file:///tmp/a%zz.jpg"), None);
    }

    #[test]
    fn network_paths_are_not_frame_paths() {
        assert_eq!(path_from_file_url("file:////nas/share/a.jpg"), None);
        assert_eq!(path_from_file_url("file://nas/share/a.jpg"), None);
        assert_eq!(
            path_from_file_url("file:///%5C%5Cnas%5Cshare%5Ca.jpg"),
            None
        );
        assert_eq!(path_from_file_url("file:///C:%5C..%5Cx.jpg"), None);
    }
}
