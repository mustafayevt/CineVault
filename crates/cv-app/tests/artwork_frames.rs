//! Frame-grab candidates in the artwork stream: eight evenly spaced frames of a
//! readable video, offered for thumb and fanart as the provider "Frames" after the online ones.
//! A fake frame source stands in for FFmpeg.

mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use cv_app::app_settings::SettingsService;
use cv_app::artwork::ArtworkService;
use cv_app::artwork::frames::FrameSource;
use cv_app::artwork::stream::SourceAnswer;
use cv_app::provider_credentials::ProviderCredentialsService;
use cv_artwork::download::fetch::StaticFetcher;
use cv_artwork::frame_source::{FrameKinds, PICKER_FRAMES, frame_candidates};
use cv_artwork::thumb::ThumbCache;
use cv_domain::{Item, ItemId, MediaFile, MediaFileType};
use cv_media::aspect::AspectRequest;
use cv_media::framegrab::{FrameCandidate, FrameGrabError, spread_timestamps};
use cv_media::tools::Cancel;
use cv_providers::{CredentialResolver, MemoryUserCredentials, ProviderRegistry, SourcedArtwork};
use support::Lab;

/// Writes one small picture per frame, at the timestamps the real source would use, and
/// remembers what it was asked.
#[derive(Default)]
struct FakeFrames {
    asked: Mutex<Vec<(usize, f64, Vec<PathBuf>)>>,
}

impl FrameSource for FakeFrames {
    fn grab(
        &self,
        request: &AspectRequest,
        dir: &Path,
        _cancel: &Cancel,
    ) -> Result<Vec<SourcedArtwork>, FrameGrabError> {
        let total: f64 = request.parts.iter().map(|p| p.duration_secs).sum();
        if total <= 0.0 {
            return Err(FrameGrabError::UnknownDuration);
        }
        let (from, to) = cv_artwork::frame_source::PICKER_SPAN;
        let at = spread_timestamps(total, PICKER_FRAMES, from, to);
        fs::create_dir_all(dir).unwrap();
        let frames: Vec<FrameCandidate> = at
            .iter()
            .enumerate()
            .map(|(i, secs)| {
                let path = dir.join(format!("frame-{:02}.jpg", i + 1));
                image::RgbImage::from_pixel(64, 36, image::Rgb([u8::try_from(i).unwrap(), 9, 9]))
                    .save(&path)
                    .unwrap();
                FrameCandidate {
                    path,
                    timestamp_secs: *secs,
                    part: 0,
                }
            })
            .collect();
        self.asked.lock().unwrap().push((
            request.parts.len(),
            total,
            frames.iter().map(|f| f.path.clone()).collect(),
        ));
        Ok(frame_candidates(&frames, FrameKinds::default()))
    }
}

struct Rig {
    lab: Lab,
    service: Arc<ArtworkService>,
    frames: Arc<FakeFrames>,
}

fn rig(with_frames: bool) -> Rig {
    let lab = Lab::new();
    let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
    let providers = Arc::new(ProviderCredentialsService::with_resolver(
        resolver,
        ProviderRegistry::new(),
    ));
    let settings = Arc::new(SettingsService::in_memory(""));
    let thumbs = Arc::new(ThumbCache::open(&lab.dir.path().join("cache"), None).unwrap());
    let service = ArtworkService::with_fetcher(
        Arc::clone(&lab.db),
        providers,
        settings,
        thumbs,
        Arc::new(StaticFetcher::new()),
    );
    let frames = Arc::new(FakeFrames::default());
    if with_frames {
        service.set_frame_source(frames.clone(), lab.dir.path().join("frames"));
    }
    Rig {
        lab,
        service,
        frames,
    }
}

impl Rig {
    fn video(&self, folder: &Path, name: &str, on_disk: bool) -> MediaFile {
        fs::create_dir_all(folder).unwrap();
        if on_disk {
            fs::write(folder.join(name), b"video").unwrap();
        }
        let mut file = MediaFile::new(MediaFileType::Video, folder.to_string_lossy(), name);
        file.duration_secs = 7200;
        file
    }

    fn movie(&self, on_disk: bool) -> ItemId {
        let folder = self.lab.dir.path().join("Movies").join("Heat (1995)");
        let mut item = Item::movie("Heat");
        item.core.ids.set("tmdb", "949");
        item.core.path = folder.to_string_lossy().into_owned();
        item.core
            .media_files
            .push(self.video(&folder, "Heat (1995).mkv", on_disk));
        self.lab.add(item)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_movie_with_a_readable_video_offers_eight_frames_for_fanart_and_thumb() {
    let rig = rig(true);
    let movie = rig.movie(true);

    for slot in [MediaFileType::Fanart, MediaFileType::Thumb] {
        let stream = rig.service.candidate_stream(movie, slot).await.unwrap();
        let source = stream.sources.last().expect("a frames source");
        assert_eq!(
            (source.key.as_str(), source.name.as_str()),
            ("frames", "Frames")
        );
        assert!(source.cpu, "frame grabbing is CPU work");

        let SourceAnswer::Candidates(frames) = source.fetch().await else {
            panic!("the frames were not made");
        };
        assert_eq!(frames.len(), 8, "{slot:?}");
        assert!(frames.iter().all(|f| f.provider == "frames"));
        assert!(frames.iter().all(|f| f.url.starts_with("file:///")));
        assert!(frames.iter().all(|f| (f.width, f.height) == (64, 36)));
    }

    // Eight frames, 10% to 90% of the two hours.
    let asked = rig.frames.asked.lock().unwrap().clone();
    assert_eq!(asked[0].1, 7200.0);
    assert_eq!(asked[0].2.len(), 8);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn frames_are_only_offered_for_thumb_and_fanart() {
    let rig = rig(true);
    let movie = rig.movie(true);

    let stream = rig
        .service
        .candidate_stream(movie, MediaFileType::Poster)
        .await
        .unwrap();

    assert!(stream.sources.iter().all(|s| s.key != "frames"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_frames_without_a_readable_video_or_a_frame_source() {
    // The video is not on disk.
    let rig_missing = rig(true);
    let gone = rig_missing.movie(false);
    let stream = rig_missing
        .service
        .candidate_stream(gone, MediaFileType::Fanart)
        .await
        .unwrap();
    assert!(stream.sources.is_empty());

    // Frames were never set up.
    let rig_plain = rig(false);
    let movie = rig_plain.movie(true);
    let stream = rig_plain
        .service
        .candidate_stream(movie, MediaFileType::Fanart)
        .await
        .unwrap();
    assert!(stream.sources.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_episode_offers_frames_for_its_thumb_and_nothing_else() {
    let rig = rig(true);
    let show = rig.lab.show("Dark", 2017);
    let folder = rig.lab.dir.path().join("Shows").join("Dark");
    let mut episode = Item::episode(show, "Secrets");
    episode
        .core
        .media_files
        .push(rig.video(&folder, "Dark S01E01.mkv", true));
    let episode = rig.lab.add(episode);

    let stream = rig
        .service
        .candidate_stream(episode, MediaFileType::Thumb)
        .await
        .unwrap();
    assert_eq!(stream.title, "Secrets");
    assert_eq!(stream.slots, [MediaFileType::Thumb]);
    assert_eq!(
        stream
            .sources
            .iter()
            .map(|s| s.key.as_str())
            .collect::<Vec<_>>(),
        ["frames"]
    );
    let SourceAnswer::Candidates(frames) = stream.sources[0].fetch().await else {
        panic!("the frames were not made");
    };
    assert_eq!(frames.len(), 8);

    let error = rig
        .service
        .candidate_stream(episode, MediaFileType::Poster)
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("cannot have that kind"),
        "{error}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_video_of_unknown_length_fails_with_the_reason() {
    let rig = rig(true);
    let folder = rig.lab.dir.path().join("Movies").join("Ronin (1998)");
    let mut item = Item::movie("Ronin");
    item.core.ids.set("tmdb", "1");
    item.core.path = folder.to_string_lossy().into_owned();
    let mut video = rig.video(&folder, "Ronin (1998).mkv", true);
    video.duration_secs = 0;
    item.core.media_files.push(video);
    let movie = rig.lab.add(item);

    let stream = rig
        .service
        .candidate_stream(movie, MediaFileType::Thumb)
        .await
        .unwrap();
    let SourceAnswer::Failed { reason, retryable } = stream.sources[0].fetch().await else {
        panic!("expected a failure");
    };
    assert!(
        reason.contains("length of the video is not known"),
        "{reason}"
    );
    assert!(!retryable);
}
