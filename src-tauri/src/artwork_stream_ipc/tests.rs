//! The streaming artwork picker with mock providers: a slow one, a failing one, one without a
//! key and one that does not offer the type. No network.

use std::io::Cursor;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use cv_app::app_settings::SettingsService;
use cv_app::artwork::ArtworkService;
use cv_app::provider_credentials::ProviderCredentialsService;
use cv_artwork::download::fetch::StaticFetcher;
use cv_artwork::thumb::ThumbCache;
use cv_db::repo::insert_item;
use cv_db::{Database, DbOptions};
use cv_domain::{Item, ItemId, MediaFile, MediaFileType, ProviderIds};
use cv_jobs::{LaneConfig, Lanes};
use cv_providers::model::{ArtworkCandidate, MediaKind, RequestOptions};
use cv_providers::{
    ArtworkProvider, CredentialResolver, MemoryUserCredentials, Provider, ProviderError,
    ProviderRegistry,
};
use image::{ImageFormat, Rgb, RgbImage};

use super::*;
use crate::provider_stream::{RetryOutcome, SearchRegistry, Sink};

fn jpeg(seed: u8) -> Vec<u8> {
    let image = RgbImage::from_pixel(40, 60, Rgb([seed, 200, 10]));
    let mut out = Cursor::new(Vec::new());
    image.write_to(&mut out, ImageFormat::Jpeg).unwrap();
    out.into_inner()
}

/// How a mock provider answers.
#[derive(Clone)]
enum Script {
    /// Offers these images (type, address) after a delay.
    Images(Vec<(MediaFileType, &'static str)>, Duration),
    /// Fails as a provider does when its server cannot be reached.
    Unreachable,
}

struct Mock {
    id: &'static str,
    name: &'static str,
    offers: &'static [MediaFileType],
    script: Script,
}

#[async_trait]
impl Provider for Mock {
    fn id(&self) -> &'static str {
        self.id
    }
    fn name(&self) -> &'static str {
        self.name
    }
}

#[async_trait]
impl ArtworkProvider for Mock {
    fn offers(&self, _kind: MediaKind, slot: MediaFileType) -> bool {
        self.offers.contains(&slot)
    }

    async fn artwork(
        &self,
        _kind: MediaKind,
        _ids: &ProviderIds,
        _options: &RequestOptions,
    ) -> Result<Vec<ArtworkCandidate>, ProviderError> {
        match &self.script {
            Script::Images(images, delay) => {
                tokio::time::sleep(*delay).await;
                Ok(images
                    .iter()
                    .map(|(kind, url)| ArtworkCandidate {
                        kind: *kind,
                        url: (*url).to_owned(),
                        language: Some("en".to_owned()),
                        width: 2000,
                        height: 3000,
                        score: 5.0,
                        popularity: cv_providers::model::Popularity::Unknown,
                    })
                    .collect())
            }
            Script::Unreachable => Err(ProviderError::Request {
                provider: self.name.to_owned(),
                message: "timed out".to_owned(),
            }),
        }
    }
}

const POSTER_ART: &[MediaFileType] = &[MediaFileType::Poster, MediaFileType::Fanart];
const FANART_TV_ART: &[MediaFileType] = &[
    MediaFileType::Poster,
    MediaFileType::Fanart,
    MediaFileType::Clearart,
];

struct Rig {
    dir: tempfile::TempDir,
    service: Arc<ArtworkService>,
    thumbs: Arc<ThumbCache>,
    db: Arc<Database>,
}

fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(
        Database::open_with(dir.path().join("library.db"), &DbOptions { readers: 2 }).unwrap(),
    );
    let mut registry = ProviderRegistry::new();
    // Slow, with a key.
    registry.register_artwork(Arc::new(Mock {
        id: "tmdb",
        name: "TMDB",
        offers: POSTER_ART,
        script: Script::Images(
            vec![
                (MediaFileType::Poster, "https://img.test/tmdb-1.jpg"),
                (MediaFileType::Poster, "https://img.test/tmdb-2.jpg"),
            ],
            Duration::from_millis(250),
        ),
    }));
    // Quick, with a key, and the only one with clear art.
    registry.register_artwork(Arc::new(Mock {
        id: "fanart",
        name: "Fanart.tv",
        offers: FANART_TV_ART,
        script: Script::Images(
            vec![
                (MediaFileType::Poster, "https://img.test/fanart-1.jpg"),
                (MediaFileType::Clearart, "https://img.test/fanart-art.jpg"),
            ],
            Duration::ZERO,
        ),
    }));
    // Needs no key, cannot be reached.
    registry.register_artwork(Arc::new(Mock {
        id: "imdb",
        name: "IMDb",
        offers: POSTER_ART,
        script: Script::Unreachable,
    }));
    // Offers posters but the user has not set its key.
    registry.register_artwork(Arc::new(Mock {
        id: "tvdb",
        name: "TheTVDB",
        offers: POSTER_ART,
        script: Script::Images(Vec::new(), Duration::ZERO),
    }));
    let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
    let providers = Arc::new(ProviderCredentialsService::with_resolver(
        resolver, registry,
    ));
    providers.set("tmdb", "api_key", "key-one").unwrap();
    providers.set("fanart", "api_key", "key-two").unwrap();
    let settings = Arc::new(SettingsService::in_memory(""));
    let fetcher = Arc::new(StaticFetcher::new());
    for (n, url) in [
        "https://img.test/tmdb-1.jpg",
        "https://img.test/tmdb-2.jpg",
        "https://img.test/fanart-1.jpg",
        "https://img.test/fanart-art.jpg",
    ]
    .into_iter()
    .enumerate()
    {
        fetcher.insert(url, jpeg(u8::try_from(n).unwrap() * 40));
    }
    let thumbs = Arc::new(ThumbCache::open(&dir.path().join("cache"), None).unwrap());
    let service = ArtworkService::with_fetcher(
        Arc::clone(&db),
        providers,
        settings,
        Arc::clone(&thumbs),
        fetcher,
    );
    Rig {
        dir,
        service,
        thumbs,
        db,
    }
}

impl Rig {
    /// A movie with a folder, a video and IDs, and the poster it already has when asked.
    fn movie(&self, title: &str, with_poster: bool) -> (ItemId, PathBuf) {
        let folder = self.dir.path().join("Movies").join(title);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join(format!("{title}.mkv")), b"video").unwrap();
        let mut item = Item::movie(title);
        item.core.ids.set("tmdb", "949");
        item.core.path = folder.to_string_lossy().into_owned();
        item.core.data_source = self
            .dir
            .path()
            .join("Movies")
            .to_string_lossy()
            .into_owned();
        item.core.media_files.push(MediaFile::new(
            MediaFileType::Video,
            folder.to_string_lossy(),
            format!("{title}.mkv"),
        ));
        if with_poster {
            std::fs::write(folder.join(format!("{title}-poster.jpg")), jpeg(9)).unwrap();
            item.core.media_files.push(MediaFile::new(
                MediaFileType::Poster,
                folder.to_string_lossy(),
                format!("{title}-poster.jpg"),
            ));
        }
        let id = self
            .db
            .write(move |tx| insert_item(tx, &item))
            .expect("insert item");
        (id, folder)
    }
}

type Events = Arc<Mutex<Vec<ProviderEvent<ArtworkCandidateDto>>>>;

fn collector() -> (Events, Sink<ArtworkCandidateDto>) {
    let events: Events = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&events);
    (
        events,
        Arc::new(move |event| sink.lock().unwrap().push(event)),
    )
}

async fn until(what: &str, mut check: impl FnMut() -> bool) {
    for _ in 0..500 {
        if check() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("timed out waiting for {what}");
}

fn kinds(events: &[ProviderEvent<ArtworkCandidateDto>]) -> Vec<String> {
    events
        .iter()
        .map(|event| match event {
            ProviderEvent::Started { provider } => format!("started {provider}"),
            ProviderEvent::Results { provider, items } => {
                format!("results {provider} {}", items.len())
            }
            ProviderEvent::Partial { provider, items } => {
                format!("partial {provider} {}", items.len())
            }
            ProviderEvent::Update { provider, items } => {
                format!("update {provider} {}", items.len())
            }
            ProviderEvent::Failed {
                provider,
                retryable,
                ..
            } => format!("failed {provider} retryable={retryable}"),
            ProviderEvent::NeedsKey { provider } => format!("needs-key {provider}"),
            ProviderEvent::Done => "done".to_owned(),
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn quick_providers_answer_before_the_slow_one_and_a_failure_can_be_retried() {
    let rig = rig();
    let (item, _) = rig.movie("Heat", true);
    let stream = rig
        .service
        .candidate_stream(item, MediaFileType::Poster)
        .await
        .unwrap();
    let start = start_dto(&stream, Some(&rig.thumbs));
    let registry = SearchRegistry::default();
    let (events, sink) = collector();
    let lanes = Lanes::new(LaneConfig::default());

    registry.start(
        &lanes,
        "s1",
        searches(&stream.sources, Some(&rig.thumbs)),
        sink,
    );
    until("done", || {
        events.lock().unwrap().last() == Some(&ProviderEvent::Done)
    })
    .await;

    // The providers are listed in the order of the default ranking, and all are asked before
    // any answers.
    assert_eq!(
        start
            .providers
            .iter()
            .map(|p| p.key.as_str())
            .collect::<Vec<_>>(),
        ["tmdb", "fanart", "imdb", "tvdb"]
    );
    let seen = kinds(&events.lock().unwrap());
    // The providers run side by side, so a quick one can answer (here imdb failing at once)
    // before the last one has announced itself: every provider announces itself, and before it
    // answers, but the order among the announcements is not fixed.
    for provider in ["tmdb", "fanart", "imdb", "tvdb"] {
        let started = seen
            .iter()
            .position(|kind| *kind == format!("started {provider}"))
            .unwrap_or_else(|| panic!("{provider} never started: {seen:?}"));
        let answered = seen
            .iter()
            .position(|kind| {
                kind.split(' ').nth(1) == Some(provider) && !kind.starts_with("started")
            })
            .unwrap_or(seen.len());
        assert!(started < answered, "{provider} answered first: {seen:?}");
    }
    // The slow provider answers last, after the others and just before Done.
    assert_eq!(seen[seen.len() - 2], "results tmdb 2", "{seen:?}");
    assert_eq!(seen[seen.len() - 1], "done");
    assert!(seen.contains(&"results fanart 1".to_owned()), "{seen:?}");
    assert!(seen.contains(&"failed imdb retryable=true".to_owned()));
    assert!(seen.contains(&"needs-key tvdb".to_owned()));

    // Its candidates carry a thumbnail key in the image cache.
    let all = events.lock().unwrap().clone();
    let tmdb = all
        .iter()
        .find_map(|event| match event {
            ProviderEvent::Results { provider, items } if provider == "tmdb" => Some(items),
            _ => None,
        })
        .unwrap();
    assert_eq!(tmdb[0].provider, "tmdb");
    assert!(!tmdb[0].image.key.is_empty());
    assert!(tmdb[0].image.thumbhash.is_some(), "the thumbnail was built");

    // Retrying the failed provider asks that one only.
    events.lock().unwrap().clear();
    assert_eq!(registry.retry("s1", "imdb"), RetryOutcome::Started);
    until("second done", || {
        events.lock().unwrap().last() == Some(&ProviderEvent::Done)
    })
    .await;
    assert_eq!(
        kinds(&events.lock().unwrap()),
        ["started imdb", "failed imdb retryable=true", "done"]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_image_the_item_has_now_is_pinned_first() {
    let rig = rig();
    let (item, _) = rig.movie("Heat", true);
    let stream = rig
        .service
        .candidate_stream(item, MediaFileType::Poster)
        .await
        .unwrap();
    let start = start_dto(&stream, Some(&rig.thumbs));

    let current = start.current.expect("the item has a poster");
    assert_eq!(current.provider, "current");
    assert_eq!((current.width, current.height), (40, 60));
    assert!(current.id.starts_with("file:///"), "{}", current.id);

    // An item without a poster pins nothing.
    let (bare, _) = rig.movie("Ronin", false);
    let stream = rig
        .service
        .candidate_stream(bare, MediaFileType::Poster)
        .await
        .unwrap();
    assert!(start_dto(&stream, None).current.is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_clear_art_search_lists_only_the_provider_that_offers_it() {
    let rig = rig();
    let (item, _) = rig.movie("Heat", false);
    let stream = rig
        .service
        .candidate_stream(item, MediaFileType::Clearart)
        .await
        .unwrap();
    assert_eq!(
        stream
            .sources
            .iter()
            .map(|s| s.key.as_str())
            .collect::<Vec<_>>(),
        ["fanart"]
    );

    let registry = SearchRegistry::default();
    let (events, sink) = collector();
    registry.start(
        &Lanes::new(LaneConfig::default()),
        "s2",
        searches(&stream.sources, Some(&rig.thumbs)),
        sink,
    );
    until("done", || {
        events.lock().unwrap().last() == Some(&ProviderEvent::Done)
    })
    .await;
    assert_eq!(
        kinds(&events.lock().unwrap()),
        ["started fanart", "results fanart 1", "done"]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_type_the_item_cannot_have_is_refused() {
    let rig = rig();
    let (item, _) = rig.movie("Heat", false);
    let error = rig
        .service
        .candidate_stream(item, MediaFileType::Video)
        .await
        .unwrap_err();
    assert_eq!(ArtworkCommandError::from(error).code, "unsupported");
}

/// Stands in for FFmpeg: writes one small picture per frame it is asked for.
struct FakeFrames;

impl cv_app::artwork::frames::FrameSource for FakeFrames {
    fn grab(
        &self,
        request: &cv_media::aspect::AspectRequest,
        dir: &std::path::Path,
        _cancel: &cv_media::tools::Cancel,
    ) -> Result<Vec<cv_providers::SourcedArtwork>, cv_media::framegrab::FrameGrabError> {
        let total: f64 = request.parts.iter().map(|p| p.duration_secs).sum();
        let (from, to) = cv_artwork::frame_source::PICKER_SPAN;
        std::fs::create_dir_all(dir).unwrap();
        let frames: Vec<_> = cv_media::framegrab::spread_timestamps(
            total,
            cv_artwork::frame_source::PICKER_FRAMES,
            from,
            to,
        )
        .into_iter()
        .enumerate()
        .map(|(i, at)| {
            let path = dir.join(format!("frame-{:02}.jpg", i + 1));
            RgbImage::from_pixel(64, 36, Rgb([u8::try_from(i).unwrap(), 5, 5]))
                .save(&path)
                .unwrap();
            cv_media::framegrab::FrameCandidate {
                path,
                timestamp_secs: at,
                part: 0,
            }
        })
        .collect();
        Ok(cv_artwork::frame_source::frame_candidates(
            &frames,
            cv_artwork::frame_source::FrameKinds::default(),
        ))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn frames_stream_after_the_online_providers_on_the_cpu_lane() {
    let rig = rig();
    rig.service
        .set_frame_source(Arc::new(FakeFrames), rig.dir.path().join("frames"));
    let (item, folder) = rig.movie("Heat", false);
    // Give the video a length, as the media info read does.
    rig.db
        .write(move |tx| {
            let mut stored = cv_db::repo::get_item(tx, item)?.unwrap();
            for file in &mut stored.core.media_files {
                file.duration_secs = 7200;
            }
            cv_db::repo::update_item(tx, &stored)
        })
        .unwrap();
    assert!(folder.join("Heat.mkv").is_file());

    let stream = rig
        .service
        .candidate_stream(item, MediaFileType::Fanart)
        .await
        .unwrap();
    let start = start_dto(&stream, Some(&rig.thumbs));
    // Frames are listed after every online provider.
    assert_eq!(start.providers.last().unwrap().key, "frames");
    assert_eq!(start.providers.last().unwrap().name, "Frames");
    let searches = searches(&stream.sources, Some(&rig.thumbs));
    let lanes: Vec<_> = searches.iter().map(|s| s.lane()).collect();
    assert_eq!(*lanes.last().unwrap(), cv_jobs::LaneId::Cpu);

    let registry = SearchRegistry::default();
    let (events, sink) = collector();
    registry.start(&Lanes::new(LaneConfig::default()), "s3", searches, sink);
    until("done", || {
        events.lock().unwrap().last() == Some(&ProviderEvent::Done)
    })
    .await;

    let all = events.lock().unwrap().clone();
    let frames = all
        .iter()
        .find_map(|event| match event {
            ProviderEvent::Results { provider, items } if provider == "frames" => Some(items),
            _ => None,
        })
        .expect("frames answered");
    assert_eq!(frames.len(), 8);
    assert!(frames.iter().all(|f| f.provider == "frames"));
    assert!(frames[0].id.starts_with("file:///"));
    assert!(frames[0].image.thumbhash.is_some());
}

/// A source that hands over its images in batches, as an online provider does while its
/// thumbnails download.
struct Batches;

fn picker(url: &str) -> PickerCandidate {
    PickerCandidate {
        provider: "wide".to_owned(),
        url: url.to_owned(),
        language: None,
        width: 2000,
        height: 3000,
        score: 50,
        best: 0,
        votes: 0,
        rating: 0,
        likes: 0,
        low_quality: false,
        key: cv_artwork::thumb::ThumbKey::of_bytes(url.as_bytes()),
    }
}

#[async_trait]
impl cv_app::artwork::stream::CandidateFetch for Batches {
    async fn fetch(&self) -> SourceAnswer {
        SourceAnswer::Candidates(vec![picker("a"), picker("b"), picker("c")])
    }

    async fn fetch_progressive(
        &self,
        early: &cv_app::artwork::stream::EarlyBatch<'_>,
    ) -> SourceAnswer {
        early(vec![picker("a")]);
        tokio::time::sleep(Duration::from_millis(20)).await;
        early(vec![picker("b")]);
        tokio::time::sleep(Duration::from_millis(20)).await;
        SourceAnswer::Candidates(vec![picker("c")])
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_provider_fills_the_picker_batch_by_batch_in_order() {
    let source = CandidateSource::new("wide", "Wide", false, Arc::new(Batches));
    let registry = SearchRegistry::default();
    let (events, sink) = collector();
    registry.start(
        &Lanes::new(LaneConfig::default()),
        "s1",
        searches(&[source], None),
        sink,
    );
    until("done", || {
        events.lock().unwrap().last() == Some(&ProviderEvent::Done)
    })
    .await;

    let events = events.lock().unwrap().clone();
    assert_eq!(
        kinds(&events),
        [
            "started wide",
            "partial wide 1",
            "partial wide 1",
            "results wide 1",
            "done"
        ]
    );
    let ids: Vec<&str> = events
        .iter()
        .filter_map(|event| match event {
            ProviderEvent::Partial { items, .. } | ProviderEvent::Results { items, .. } => {
                Some(items[0].id.as_str())
            }
            _ => None,
        })
        .collect();
    assert_eq!(ids, ["a", "b", "c"]);
}
