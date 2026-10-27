//! MediaInfo XML sidecar files: a cache for reads, and an optional output.
//!
//! A sidecar sits next to a video and holds what MediaInfo reported about it, in MediaInfo's
//! standard XML format. Reading one instead of the video saves opening the file, which matters
//! on slow network shares ([`read_sidecar`]). Writing one is off by default and only happens
//! when the caller says the setting is on ([`write_sidecar`]).
//!
//! * Names: `<basename>-mediainfo.xml` is preferred, `<filename>.mediainfo.xml` is also read
//!   ([`sidecar_paths`]).
//! * Both directions go through [`MediaSnapshot`], so [`super::apply_snapshot`] maps a sidecar
//!   exactly like a live read.
//! * A sidecar is only trusted while it matches the video: see [`SidecarState`].

mod xml;

use std::path::{Path, PathBuf};

use super::{MediaSnapshot, StreamKind};

/// Why a sidecar could not be read or written. Every message is ready to show.
#[derive(Debug, thiserror::Error)]
pub enum SidecarError {
    #[error("{} could not be accessed: {source}", .path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("The MediaInfo XML is not well-formed (at byte {position}): {message}")]
    Xml { position: u64, message: String },
    #[error("The file is not a MediaInfo XML document.")]
    NotMediaInfo,
}

impl SidecarError {
    fn io(path: &Path, source: std::io::Error) -> Self {
        Self::Io {
            path: path.to_path_buf(),
            source,
        }
    }
}

/// The two names a sidecar can have, in the order they are tried and the first one is also the
/// name a new sidecar gets: `<basename>-mediainfo.xml`, then `<filename>.mediainfo.xml`.
#[must_use]
pub fn sidecar_paths(video: &Path) -> [PathBuf; 2] {
    let name = video
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let stem = video
        .file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    [
        video.with_file_name(format!("{stem}-mediainfo.xml")),
        video.with_file_name(format!("{name}.mediainfo.xml")),
    ]
}

/// Parses MediaInfo XML into a snapshot in the library's conventions (durations in
/// milliseconds, `Channel(s)` and `Duration/String3` field names).
///
/// # Errors
/// [`SidecarError::Xml`] for malformed XML, [`SidecarError::NotMediaInfo`] for another
/// document.
pub fn parse_sidecar(text: &str) -> Result<MediaSnapshot, SidecarError> {
    xml::parse(text)
}

/// Renders a snapshot as standard MediaInfo XML (version 2.0). `media_ref` is the file the
/// snapshot describes, `library_version` the MediaInfo version that read it (may be empty).
#[must_use]
pub fn render_sidecar(snapshot: &MediaSnapshot, media_ref: &str, library_version: &str) -> String {
    xml::render(snapshot, media_ref, library_version)
}

/// True when the snapshot has an audio, video or text stream, which is the condition for
/// writing a sidecar (a read that found only a general section is not worth keeping).
#[must_use]
pub fn worth_writing(snapshot: &MediaSnapshot) -> bool {
    [StreamKind::Audio, StreamKind::Video, StreamKind::Text]
        .into_iter()
        .any(|kind| !snapshot.streams(kind).is_empty())
}

/// What reading the sidecar of a video found.
#[derive(Debug)]
pub enum SidecarState {
    /// No sidecar next to the video.
    Missing,
    /// A sidecar exists but describes a different version of the video (its `FileSize` differs,
    /// or, without one, it is older than the video). Read the video instead.
    Stale { path: PathBuf },
    /// A sidecar exists but cannot be used (malformed, or no streams in it). Read the video
    /// instead; the sidecar is left alone.
    Unusable { path: PathBuf, reason: String },
    /// An up-to-date sidecar: use `snapshot` and do not read the video.
    Fresh {
        path: PathBuf,
        snapshot: MediaSnapshot,
    },
}

impl SidecarState {
    /// The snapshot, only for an up-to-date sidecar.
    #[must_use]
    pub fn into_snapshot(self) -> Option<MediaSnapshot> {
        match self {
            Self::Fresh { snapshot, .. } => Some(snapshot),
            _ => None,
        }
    }
}

/// Looks for the sidecar of `video` and says whether it can stand in for reading the video.
///
/// A sidecar counts as up to date when the `FileSize` in its general section equals the size of
/// the video now; a sidecar without a size counts when it was modified no earlier than the
/// video. The sidecar is never changed here.
///
/// # Errors
/// [`SidecarError::Io`] when the video itself cannot be inspected or a sidecar cannot be read.
pub fn read_sidecar(video: &Path) -> Result<SidecarState, SidecarError> {
    let Some(path) = sidecar_paths(video).into_iter().find(|p| p.is_file()) else {
        return Ok(SidecarState::Missing);
    };
    let video_meta = std::fs::metadata(video).map_err(|e| SidecarError::io(video, e))?;
    let bytes = std::fs::read(&path).map_err(|e| SidecarError::io(&path, e))?;
    let snapshot = match String::from_utf8(bytes)
        .map_err(|_| SidecarError::NotMediaInfo)
        .and_then(|text| parse_sidecar(&text))
    {
        Ok(snapshot) => snapshot,
        Err(error) => {
            return Ok(SidecarState::Unusable {
                path,
                reason: error.to_string(),
            });
        }
    };
    if !worth_writing(&snapshot) {
        return Ok(SidecarState::Unusable {
            path,
            reason: "it lists no video, audio or text stream".to_owned(),
        });
    }
    let recorded_size = snapshot
        .general()
        .and_then(|general| general.get_u64("FileSize"));
    let fresh = match recorded_size {
        Some(size) => size == video_meta.len(),
        None => {
            let sidecar_time = std::fs::metadata(&path).and_then(|m| m.modified());
            match (sidecar_time, video_meta.modified()) {
                (Ok(sidecar), Ok(video)) => sidecar >= video,
                _ => false,
            }
        }
    };
    Ok(if fresh {
        SidecarState::Fresh { path, snapshot }
    } else {
        SidecarState::Stale { path }
    })
}

/// What [`write_sidecar`] did.
#[derive(Debug, PartialEq, Eq)]
pub enum SidecarWrite {
    /// Writing is off in the settings; nothing was touched.
    Disabled,
    /// The snapshot has no audio, video or text stream; nothing was written.
    NothingToWrite,
    /// The sidecar was written (or replaced) at this path.
    Written(PathBuf),
}

/// Writes the sidecar of `video` from `snapshot`, but only when `enabled` (the "store MediaInfo
/// XML next to the media" setting) is true: a user's sidecar is never touched otherwise.
///
/// The file is `<basename>-mediainfo.xml`, or the existing `<filename>.mediainfo.xml` when that
/// is the one already there. The video's current size is recorded as `FileSize` when the
/// snapshot has none, which is what [`read_sidecar`] checks later. The write goes through a
/// flushed temporary file and a rename (`cv_fs::write_atomic`), so a failure never leaves half a
/// sidecar.
///
/// # Errors
/// [`SidecarError::Io`] when the video cannot be inspected or the sidecar cannot be written.
pub fn write_sidecar(
    video: &Path,
    snapshot: &MediaSnapshot,
    library_version: &str,
    enabled: bool,
) -> Result<SidecarWrite, SidecarError> {
    if !enabled {
        return Ok(SidecarWrite::Disabled);
    }
    if !worth_writing(snapshot) {
        return Ok(SidecarWrite::NothingToWrite);
    }
    let size = std::fs::metadata(video)
        .map_err(|e| SidecarError::io(video, e))?
        .len();
    let [preferred, alternative] = sidecar_paths(video);
    let target = if !preferred.is_file() && alternative.is_file() {
        alternative
    } else {
        preferred
    };

    let mut snapshot = snapshot.clone();
    if snapshot.general().is_none() {
        snapshot.push(StreamKind::General, super::StreamInfo::new());
    }
    if let Some(general) = snapshot.general_mut()
        && general.get("FileSize").is_none()
    {
        general.set("FileSize", size.to_string());
    }
    let text = render_sidecar(&snapshot, &video.to_string_lossy(), library_version);

    cv_fs::write_atomic(&target, text.as_bytes()).map_err(|e| SidecarError::io(&target, e))?;
    Ok(SidecarWrite::Written(target))
}

#[cfg(test)]
mod tests;
