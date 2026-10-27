//! Frame grabs as artwork candidates: eight evenly spaced stills from the video,
//! offered next to the online images as the provider `frames` for thumbs and fanart.
//!
//! The frames are taken with FFmpeg on the CPU lane (`cv_artwork::frame_source`, which uses
//! `cv_media::framegrab`), from 10% to 90% of the video's length, and are saved as JPEG files in
//! a cache folder under the title's id. A frame is a local file, so its candidate address is a
//! `file:` URL that `place_artwork` takes like any other. The source is listed after the online
//! providers, and answers when the stills are ready, which is why it comes last.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use cv_artwork::download::config::ArtworkConfig;
use cv_artwork::frame_source::{FrameKinds, grab_picker_artwork, path_from_file_url};
use cv_artwork::thumb::{ThumbCache, ThumbSize};
use cv_domain::{Item, MediaFile, MediaFileType};
use cv_media::aspect::AspectRequest;
use cv_media::framegrab::{FrameGrabError, FrameGrabOptions};
use cv_media::tools::{Cancel, ToolsManager};
use cv_providers::SourcedArtwork;

use super::PickerCandidate;
use super::stream::{CandidateFetch, CandidateSource, SourceAnswer};

/// The provider key of the frame grabs in a candidate search.
pub const FRAMES_PROVIDER: &str = "frames";

/// The name the interface shows for it.
pub const FRAMES_NAME: &str = "Frames";

/// Takes the stills of a video. The real one runs FFmpeg; tests bring their own.
pub trait FrameSource: Send + Sync {
    /// Saves the picker's frames of the video into `dir` and offers them as fanart and thumb
    /// candidates, in timeline order. Stops early when `cancel` is set.
    ///
    /// # Errors
    /// A missing FFmpeg, a disc image, an unknown length, or cancellation.
    fn grab(
        &self,
        request: &AspectRequest,
        dir: &Path,
        cancel: &Cancel,
    ) -> Result<Vec<SourcedArtwork>, FrameGrabError>;
}

/// The frame source of the app: FFmpeg from the tools manager.
pub struct FfmpegFrames {
    tools: Arc<ToolsManager>,
}

impl FfmpegFrames {
    #[must_use]
    pub fn new(tools: Arc<ToolsManager>) -> Self {
        Self { tools }
    }
}

impl FrameSource for FfmpegFrames {
    fn grab(
        &self,
        request: &AspectRequest,
        dir: &Path,
        cancel: &Cancel,
    ) -> Result<Vec<SourcedArtwork>, FrameGrabError> {
        let options = FrameGrabOptions {
            pixel_aspect_ratio: request.pixel_aspect_ratio,
            ..FrameGrabOptions::default()
        };
        grab_picker_artwork(
            &self.tools,
            &request.parts,
            dir,
            FrameKinds::default(),
            &options,
            cancel,
        )
    }
}

/// The frame source of an `ArtworkService` and the folder it saves frames in.
#[derive(Clone)]
pub(super) struct FrameSetup {
    pub source: Arc<dyn FrameSource>,
    pub dir: PathBuf,
}

/// The video files frames can be taken from: plain files that are on disk. `None` for a title
/// without one, a disc folder or a video that is not there.
pub(super) fn readable_videos(item: &Item) -> Option<Vec<&MediaFile>> {
    let videos: Vec<&MediaFile> = item
        .core
        .media_files
        .iter()
        .filter(|f| f.kind == MediaFileType::Video)
        .collect();
    let readable = !videos.is_empty()
        && videos
            .iter()
            .all(|f| Path::new(&f.directory).join(&f.filename).is_file());
    readable.then_some(videos)
}

/// Stops the frame grabbing when the search that asked for it is dropped.
struct StopOnDrop(Cancel);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

/// One title's frames, as a source of a candidate search.
struct Frames {
    source: Arc<dyn FrameSource>,
    request: AspectRequest,
    dir: PathBuf,
    kind: MediaFileType,
    thumbs: Arc<ThumbCache>,
    config: ArtworkConfig,
}

#[async_trait]
impl CandidateFetch for Frames {
    async fn fetch(&self) -> SourceAnswer {
        let cancel = Cancel::new();
        let _stop = StopOnDrop(cancel.clone());
        let (source, request, dir) = (
            Arc::clone(&self.source),
            self.request.clone(),
            self.dir.clone(),
        );
        let grabbed =
            tokio::task::spawn_blocking(move || source.grab(&request, &dir, &cancel)).await;
        let frames = match grabbed {
            Ok(Ok(frames)) => frames,
            Ok(Err(error)) => {
                let help = error.settings_link().map_or(
                    "",
                    |_| " Download FFmpeg or set its path under Settings, External tools.",
                );
                return SourceAnswer::Failed {
                    reason: format!("{error}{help}"),
                    retryable: matches!(
                        error,
                        FrameGrabError::NoFrame { .. } | FrameGrabError::Write { .. }
                    ),
                };
            }
            Err(_) => {
                return SourceAnswer::Failed {
                    reason: "The frames could not be taken.".to_owned(),
                    retryable: true,
                };
            }
        };
        let kind = self.kind;
        let selection = self.config.selection.clone();
        let thumbs = Arc::clone(&self.thumbs);
        let built = tokio::task::spawn_blocking(move || {
            frames
                .iter()
                .filter(|frame| frame.candidate.kind == kind)
                .filter_map(|frame| {
                    let path = path_from_file_url(&frame.candidate.url)?;
                    let ensured = thumbs.ensure(&path, &[ThumbSize::Grid]).ok()?;
                    let mut candidate = PickerCandidate::of(
                        frame,
                        kind,
                        &selection,
                        ensured.key,
                        (ensured.meta.source_width, ensured.meta.source_height),
                    );
                    candidate.provider = FRAMES_PROVIDER.to_owned();
                    candidate.language = None;
                    candidate.width = ensured.meta.source_width;
                    candidate.height = ensured.meta.source_height;
                    Some(candidate)
                })
                .collect::<Vec<_>>()
        })
        .await;
        match built {
            Ok(candidates) => SourceAnswer::Candidates(candidates),
            Err(_) => SourceAnswer::Failed {
                reason: "The frames could not be shown.".to_owned(),
                retryable: true,
            },
        }
    }
}

impl super::ArtworkService {
    /// Lets candidate searches offer frame grabs: `source` takes the stills and `dir` is where
    /// they are saved (a folder per title is made under it).
    pub fn set_frame_source(&self, source: Arc<dyn FrameSource>, dir: impl Into<PathBuf>) {
        *self
            .frames
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(FrameSetup {
            source,
            dir: dir.into(),
        });
    }

    /// The frame-grab source for a title whose video can be read, when frames are set up and the
    /// searched type is thumb or fanart. `None` otherwise.
    pub(super) fn frames_source(
        &self,
        item: &Item,
        kind: MediaFileType,
    ) -> Option<CandidateSource> {
        if !matches!(kind, MediaFileType::Thumb | MediaFileType::Fanart) {
            return None;
        }
        let setup = self
            .frames
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()?;
        let videos = readable_videos(item)?;
        let id = item.id?;
        Some(CandidateSource::new(
            FRAMES_PROVIDER,
            FRAMES_NAME,
            true,
            Arc::new(Frames {
                source: setup.source,
                request: AspectRequest::from_media_files(&videos),
                dir: setup.dir.join(id.0.to_string()),
                kind,
                thumbs: Arc::clone(&self.thumbs),
                config: self.config(),
            }),
        ))
    }
}
