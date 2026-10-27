//! NFO and artwork after any scraper: a scrape from a provider that is not TMDB
//! (OFDb-like, knowing only an IMDb ID) writes the NFO next to the video and fills the missing
//! artwork through an artwork provider by that IMDb ID. When no artwork provider knows the title,
//! the NFO is still written and the artwork step says so. Mock providers, no network.

mod support;

use std::fs;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use cv_app::app_settings::SettingsService;
use cv_app::artwork::ArtworkService;
use cv_app::match_service::{MatchService, ScrapeApply};
use cv_app::nfo::NfoService;
use cv_app::notices::NoticeHub;
use cv_app::provider_credentials::ProviderCredentialsService;
use cv_app::scrape_follow_up::ScrapeFollowUp;
use cv_app::step_outcomes::{STEP_ARTWORK, StepStatus};
use cv_artwork::download::fetch::StaticFetcher;
use cv_artwork::thumb::ThumbCache;
use cv_db::repo::step_outcome::step_outcome;
use cv_domain::{Item, ItemDetail, ItemId, MediaFile, MediaFileType, ProviderIds, Trailer};
use cv_providers::model::{
    ArtworkCandidate, Capabilities, MediaKind, RequestOptions, SearchHit, SearchQuery,
};
use cv_providers::{
    ArtworkProvider, CredentialResolver, MemoryUserCredentials, MetadataProvider, Provider,
    ProviderError, ProviderRegistry,
};
use image::{ImageFormat, Rgb, RgbImage};
use support::Lab;

fn jpeg(seed: u8) -> Vec<u8> {
    let image = RgbImage::from_pixel(40, 60, Rgb([seed, 200, 10]));
    let mut out = Cursor::new(Vec::new());
    image.write_to(&mut out, ImageFormat::Jpeg).unwrap();
    out.into_inner()
}

/// A German-database-like scraper: it knows its own ID and gives the IMDb ID, nothing else.
struct OfdbLike;

#[async_trait]
impl Provider for OfdbLike {
    fn id(&self) -> &'static str {
        "ofdb"
    }
    fn name(&self) -> &'static str {
        "OFDb"
    }
}

#[async_trait]
impl MetadataProvider for OfdbLike {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            movies: true,
            shows: false,
            orderings: Vec::new(),
            lookup_ids: vec!["ofdb"],
            best_effort: false,
        }
    }

    async fn search(
        &self,
        _query: &SearchQuery,
        _options: &RequestOptions,
    ) -> Result<Vec<SearchHit>, ProviderError> {
        Ok(Vec::new())
    }

    async fn lookup(
        &self,
        _kind: MediaKind,
        _ids: &ProviderIds,
        _options: &RequestOptions,
    ) -> Result<Option<SearchHit>, ProviderError> {
        Ok(None)
    }

    async fn details(
        &self,
        _kind: MediaKind,
        ids: &ProviderIds,
        _options: &RequestOptions,
    ) -> Result<Item, ProviderError> {
        let mut item = Item::movie("Das Boot");
        item.core.year = 1981;
        item.core.plot = "Ein U-Boot im Atlantik.".to_owned();
        item.core
            .ids
            .set("ofdb", ids.get("ofdb").unwrap_or_default());
        if ids.get("ofdb") == Some("1") {
            item.core.ids.set("imdb", "tt0082096");
        }
        if ids.get("ofdb") == Some("kw") {
            // A provider that returns keywords as tags, as TMDB does.
            item.core.ids.set("imdb", "tt0082096");
            for keyword in ["u-boat", "submarine", "world war ii", "claustrophobia"] {
                item.core.tags.add(keyword);
            }
        }
        if ids.get("ofdb") == Some("yt") {
            // A scrape that found a YouTube video, as the TMDB and TheTVDB scrapes do.
            item.core.ids.set("imdb", "tt0082096");
            if let ItemDetail::Movie(movie) = &mut item.detail {
                movie.trailers.push(Trailer {
                    id: "v1".to_owned(),
                    name: "Official Trailer".to_owned(),
                    url: "https://www.youtube.com/watch?v=abc123XYZ_-".to_owned(),
                    quality: "1080p".to_owned(),
                    language: "de".to_owned(),
                    provider: "ofdb".to_owned(),
                    in_nfo: true,
                    ..Trailer::default()
                });
            }
        }
        Ok(item)
    }
}

/// An artwork provider that knows titles by their IMDb ID only.
struct ByImdb;

#[async_trait]
impl Provider for ByImdb {
    fn id(&self) -> &'static str {
        "imdb"
    }
    fn name(&self) -> &'static str {
        "IMDb"
    }
}

#[async_trait]
impl ArtworkProvider for ByImdb {
    async fn artwork(
        &self,
        _kind: MediaKind,
        ids: &ProviderIds,
        _options: &RequestOptions,
    ) -> Result<Vec<ArtworkCandidate>, ProviderError> {
        if ids.get("imdb") != Some("tt0082096") {
            return Err(ProviderError::NotFound("IMDb".to_owned()));
        }
        Ok(vec![ArtworkCandidate {
            kind: MediaFileType::Poster,
            url: "https://img.test/boot-poster.jpg".to_owned(),
            language: Some("de".to_owned()),
            width: 2000,
            height: 3000,
            score: 8.0,
            popularity: cv_providers::model::Popularity::Unknown,
        }])
    }
}

/// An artwork provider with a poster, a fanart and a banner for the same title.
struct Gallery;

#[async_trait]
impl Provider for Gallery {
    fn id(&self) -> &'static str {
        "imdb"
    }
    fn name(&self) -> &'static str {
        "IMDb"
    }
}

#[async_trait]
impl ArtworkProvider for Gallery {
    async fn artwork(
        &self,
        _kind: MediaKind,
        ids: &ProviderIds,
        _options: &RequestOptions,
    ) -> Result<Vec<ArtworkCandidate>, ProviderError> {
        if ids.get("imdb") != Some("tt0082096") {
            return Err(ProviderError::NotFound("IMDb".to_owned()));
        }
        let candidate = |kind, url: &str, width, height| ArtworkCandidate {
            kind,
            url: url.to_owned(),
            language: Some("de".to_owned()),
            width,
            height,
            score: 8.0,
            popularity: cv_providers::model::Popularity::Unknown,
        };
        Ok(vec![
            candidate(
                MediaFileType::Poster,
                "https://img.test/boot-poster.jpg",
                2000,
                3000,
            ),
            candidate(
                MediaFileType::Fanart,
                "https://img.test/boot-fanart.jpg",
                3840,
                2160,
            ),
            candidate(
                MediaFileType::Banner,
                "https://img.test/boot-banner.jpg",
                1000,
                185,
            ),
        ])
    }
}

/// [`ByImdb`] that takes its time to answer, so a test can cancel the artwork job mid-run.
struct SlowByImdb(std::time::Duration);

#[async_trait]
impl Provider for SlowByImdb {
    fn id(&self) -> &'static str {
        "imdb"
    }
    fn name(&self) -> &'static str {
        "IMDb"
    }
}

#[async_trait]
impl ArtworkProvider for SlowByImdb {
    async fn artwork(
        &self,
        kind: MediaKind,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Vec<ArtworkCandidate>, ProviderError> {
        tokio::time::sleep(self.0).await;
        ByImdb.artwork(kind, ids, options).await
    }
}

struct Rig {
    lab: Lab,
    matching: Arc<MatchService>,
    handles: Arc<Mutex<Vec<tokio::task::JoinHandle<()>>>>,
    library: PathBuf,
    /// The notices the follow-up raised.
    notices: Arc<Mutex<Vec<cv_app::notices::AppNotice>>>,
}

fn rig() -> Rig {
    rig_with(Arc::new(ByImdb))
}

fn rig_with(artwork_provider: Arc<dyn ArtworkProvider>) -> Rig {
    let lab = Lab::new();
    let mut registry = ProviderRegistry::new();
    registry.register_metadata(Arc::new(OfdbLike));
    registry.register_artwork(artwork_provider);
    let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
    let providers = Arc::new(ProviderCredentialsService::with_resolver(
        resolver, registry,
    ));
    let settings = Arc::new(SettingsService::in_memory(""));
    let matching = MatchService::new(
        Arc::clone(&lab.db),
        lab.manager.clone(),
        Arc::clone(&providers),
        Arc::clone(&settings),
        Arc::new(NoticeHub::new()),
    );
    let fetcher = Arc::new(StaticFetcher::new());
    fetcher.insert("https://img.test/boot-poster.jpg", jpeg(1));
    fetcher.insert("https://img.test/boot-fanart.jpg", jpeg(2));
    fetcher.insert("https://img.test/boot-banner.jpg", jpeg(3));
    let thumbs = Arc::new(ThumbCache::open(&lab.dir.path().join("cache"), None).unwrap());
    let artwork = ArtworkService::with_fetcher(
        Arc::clone(&lab.db),
        providers,
        Arc::clone(&settings),
        thumbs,
        fetcher,
    );
    let nfo = Arc::new(NfoService::new(
        Arc::clone(&lab.db),
        lab.dir.path().join("nfo-undo"),
    ));
    let follow_up = ScrapeFollowUp::new(nfo, artwork, Arc::clone(&settings));
    let hub = Arc::new(NoticeHub::new());
    let notices: Arc<Mutex<Vec<cv_app::notices::AppNotice>>> = Arc::default();
    {
        let notices = Arc::clone(&notices);
        hub.subscribe(move |notice| {
            notices.lock().unwrap().push(notice.clone());
            true
        });
    }
    follow_up.set_notices(hub);
    {
        let follow_up = Arc::clone(&follow_up);
        matching.set_after_undo(Arc::new(move |item| follow_up.after_undo(item)));
    }
    // As in the app: the artwork step runs as a named job.
    follow_up.set_jobs(lab.manager.clone());
    let handles: Arc<Mutex<Vec<tokio::task::JoinHandle<()>>>> = Arc::default();
    {
        let handles = Arc::clone(&handles);
        matching.set_after_match(Arc::new(move |item| {
            handles
                .lock()
                .unwrap()
                .push(follow_up.after_match_task(item));
        }));
    }
    let library = lab.dir.path().join("Filme");
    Rig {
        lab,
        matching,
        handles,
        library,
        notices,
    }
}

impl Rig {
    fn movie(&self) -> (ItemId, PathBuf) {
        let folder = self.library.join("Das Boot (1981)");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("Das Boot (1981).mkv"), b"video").unwrap();
        let mut item = Item::movie("Das Boot");
        item.core.path = folder.to_string_lossy().into_owned();
        item.core.data_source = self.library.to_string_lossy().into_owned();
        item.core.media_files.push(MediaFile::new(
            MediaFileType::Video,
            folder.to_string_lossy(),
            "Das Boot (1981).mkv",
        ));
        (self.lab.add(item), folder)
    }

    async fn scrape(&self, item: ItemId, provider_id: &str) {
        self.start_scrape(item, provider_id).await;
        self.follow_ups_done().await;
    }

    /// The scrape alone: its follow-up runs on in the background.
    async fn start_scrape(&self, item: ItemId, provider_id: &str) {
        self.matching
            .scrape_apply(
                item,
                &ScrapeApply {
                    provider: "ofdb".to_owned(),
                    provider_id: provider_id.to_owned(),
                    language: "de".to_owned(),
                    fields: cv_domain::all_fields()
                        .iter()
                        .map(|f| f.key().to_owned())
                        .collect(),
                    do_not_overwrite: false,
                    second_version: false,
                },
            )
            .await
            .expect("scrape");
    }

    async fn follow_ups_done(&self) {
        let handles: Vec<_> = self.handles.lock().unwrap().drain(..).collect();
        for handle in handles {
            handle.await.unwrap();
        }
    }

    fn artwork_outcome(&self, item: ItemId) -> (StepStatus, String) {
        let outcome = self
            .lab
            .db
            .read(|c| step_outcome(c, item, STEP_ARTWORK))
            .unwrap()
            .expect("the artwork step left an outcome");
        (outcome.status, outcome.reason)
    }
}

fn names(folder: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(folder)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_scrape_from_a_non_tmdb_provider_writes_the_nfo_and_fetches_artwork_by_imdb_id() {
    let rig = rig();
    let (item, folder) = rig.movie();

    rig.scrape(item, "1").await;

    assert_eq!(
        names(&folder),
        [
            "Das Boot (1981)-poster.jpg",
            "Das Boot (1981).mkv",
            "Das Boot (1981).nfo"
        ]
    );
    let nfo = fs::read_to_string(folder.join("Das Boot (1981).nfo")).unwrap();
    assert!(
        nfo.contains("Das Boot") && nfo.contains("tt0082096"),
        "{nfo}"
    );
    assert_eq!(rig.lab.item(item).core.ids.get("tmdb"), None, "no TMDB ID");
    assert_eq!(rig.artwork_outcome(item).0, StepStatus::Done);
}

/// Which files to write: with default settings a scrape always writes the NFO and the
/// artwork of every type the providers have, under the names of the preset.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_scrape_with_default_settings_writes_the_nfo_and_every_artwork_type() {
    let rig = rig_with(Arc::new(Gallery));
    let (item, folder) = rig.movie();

    rig.scrape(item, "1").await;

    assert_eq!(
        names(&folder),
        [
            "Das Boot (1981)-banner.jpg",
            "Das Boot (1981)-fanart.jpg",
            "Das Boot (1981)-poster.jpg",
            "Das Boot (1981).mkv",
            "Das Boot (1981).nfo"
        ]
    );
}

/// Provider keywords are never tags (spec metadata-providers): a title whose NFO has the tag
/// "Criterion" keeps exactly that tag through a scrape, even when the scrape is asked for every
/// field, and the rewritten NFO has no keyword.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_scrape_keeps_the_tags_of_the_nfo_and_never_imports_keywords() {
    let rig = rig();
    let (item, folder) = rig.movie();
    let mut stored = rig.lab.item(item);
    stored.core.tags.add("Criterion");
    rig.lab
        .db
        .write(move |tx| cv_db::repo::update_item(tx, &stored))
        .unwrap();

    // `start_scrape` asks for every field, "tags" included.
    rig.scrape(item, "kw").await;

    let nfo = fs::read_to_string(folder.join("Das Boot (1981).nfo")).unwrap();
    assert_eq!(nfo.matches("<tag>").count(), 1, "{nfo}");
    assert!(nfo.contains("<tag>Criterion</tag>"), "{nfo}");
    assert!(!nfo.contains("submarine"), "{nfo}");
    let tags: Vec<String> = rig
        .lab
        .item(item)
        .core
        .tags
        .iter()
        .map(str::to_owned)
        .collect();
    assert_eq!(tags, ["Criterion"]);
}

/// A trailer a scrape found stays in the library and is written to the NFO as the plug-in address
/// Kodi, Jellyfin and Emby play, although CineVault itself no longer downloads trailers.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_scraped_youtube_trailer_is_written_to_the_nfo_as_a_plugin_address() {
    let rig = rig();
    let (item, folder) = rig.movie();

    rig.scrape(item, "yt").await;

    let nfo = fs::read_to_string(folder.join("Das Boot (1981).nfo")).unwrap();
    assert!(
        nfo.contains("<trailer>plugin://plugin.video.youtube/play/?video_id=abc123XYZ_-</trailer>"),
        "{nfo}"
    );
    let stored = rig.lab.item(item);
    let ItemDetail::Movie(movie) = &stored.detail else {
        panic!("not a movie");
    };
    assert_eq!(
        movie.trailers.len(),
        1,
        "the trailer is kept in the library"
    );
}

/// Spec `job-system`, "Job named in plain words": the after-scrape artwork step is a job that
/// Activity names "Downloading artwork for N titles", one for the titles of a scrape.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_artwork_step_is_one_job_named_in_plain_words() {
    let rig = rig();
    let (first, folder) = rig.movie();
    let second_folder = rig.library.join("Das Boot 2 (1981)");
    fs::create_dir_all(&second_folder).unwrap();
    fs::write(second_folder.join("Das Boot 2 (1981).mkv"), b"video").unwrap();
    let mut item = Item::movie("Das Boot 2");
    item.core.path = second_folder.to_string_lossy().into_owned();
    item.core.data_source = rig.library.to_string_lossy().into_owned();
    item.core.media_files.push(MediaFile::new(
        MediaFileType::Video,
        second_folder.to_string_lossy(),
        "Das Boot 2 (1981).mkv",
    ));
    let second = rig.lab.add(item);

    rig.start_scrape(first, "1").await;
    rig.start_scrape(second, "1").await;
    rig.follow_ups_done().await;

    let jobs: Vec<_> = rig
        .lab
        .manager
        .list()
        .into_iter()
        .filter(|job| job.name.starts_with("Downloading artwork"))
        .collect();
    assert_eq!(
        jobs.len(),
        1,
        "one job for the titles of the scrape: {jobs:?}"
    );
    assert_eq!(jobs[0].name, "Downloading artwork for 2 titles");
    assert!(jobs[0].state.is_terminal());
    // Its step in the run history says what it produced.
    assert_eq!(jobs[0].result.as_deref(), Some("artwork for 2"));
    assert!(folder.join("Das Boot (1981)-poster.jpg").exists());
    assert_eq!(rig.artwork_outcome(first).0, StepStatus::Done);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_follow_up_keeps_renames_out_until_its_writes_are_done_and_never_waits_on_itself() {
    use cv_app::title_guard::{self, Work};
    use std::time::{Duration, Instant};

    let rig = rig();
    let (item, folder) = rig.movie();
    // Another short write (a subtitle, say) holds the title, so the follow-up's NFO step waits.
    let subtitle = title_guard::hold(&rig.lab.db, [item], Work::Write, Duration::ZERO).unwrap();

    rig.start_scrape(item, "1").await;
    assert!(
        title_guard::is_pinned(&rig.lab.db, item),
        "pinned from the moment the follow-up is queued"
    );
    // A rename asking now waits for the short write (holding nothing); once that is over it
    // meets the follow-up's pin and is told at once, with the reason, instead of waiting for
    // the NFO and the artwork.
    let db = Arc::clone(&rig.lab.db);
    let rename = std::thread::spawn(move || {
        let asked = Instant::now();
        let refused = title_guard::hold(&db, [item], Work::Rename, title_guard::MOVE_WAIT)
            .map(drop)
            .expect_err("the title is pinned while its NFO and artwork are written");
        (refused, asked.elapsed())
    });
    std::thread::sleep(Duration::from_millis(200));
    drop(subtitle);
    let released = Instant::now();
    let (refused, waited) = rename.join().unwrap();
    assert_eq!(refused.work, Work::Scrape, "{refused}");
    assert!(
        waited < title_guard::MOVE_WAIT,
        "told at once, waited {waited:?}"
    );
    rig.follow_ups_done().await;

    // Its own NFO and artwork writes went ahead under the pin (a write waits up to 10 s for
    // another holder, so a follow-up waiting on itself would take that long).
    assert!(
        released.elapsed() < title_guard::WRITE_WAIT,
        "took {:?}",
        released.elapsed()
    );
    assert!(folder.join("Das Boot (1981).nfo").exists());
    assert!(folder.join("Das Boot (1981)-poster.jpg").exists());
    assert!(!title_guard::is_pinned(&rig.lab.db, item), "the pin ends");
    assert!(title_guard::hold(&rig.lab.db, [item], Work::Rename, Duration::ZERO).is_ok());
}

/// Review 14.7: cancelling the artwork job never lets a rename or move of a title start while
/// that title's artwork run is still at work. The job drops an item that overruns its grace
/// period, and with it the run's hold on the title, while an image write it started on a
/// blocking thread goes on (and makes missing folders); the title's steps then ended and its pin
/// with them, so a rename could move the folder under that write.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cancelled_artwork_job_keeps_the_title_until_its_artwork_run_is_over() {
    use cv_app::title_guard;
    use std::time::{Duration, Instant};

    let rig = rig_with(Arc::new(SlowByImdb(Duration::from_secs(3))));
    let (item, folder) = rig.movie();
    rig.start_scrape(item, "1").await;

    // The artwork job is at work on the title: cancel it.
    let deadline = Instant::now() + Duration::from_secs(10);
    let job =
        loop {
            let running =
                rig.lab.manager.list().into_iter().find(|job| {
                    job.name.starts_with("Downloading artwork") && !job.current.is_empty()
                });
            if let Some(job) = running {
                break job;
            }
            assert!(Instant::now() < deadline, "the artwork job did not start");
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(rig.lab.manager.cancel(job.id));
    loop {
        let state = rig
            .lab
            .manager
            .list()
            .into_iter()
            .find(|j| j.id == job.id)
            .map(|j| j.state);
        if state.is_some_and(|s| s.is_terminal()) {
            break;
        }
        assert!(Instant::now() < deadline, "the job did not end");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    // The job is over, the title's artwork run is not: the title stays out of reach of a
    // rename or move until it is.
    assert!(
        title_guard::is_pinned(&rig.lab.db, item),
        "the title's steps are not over while its artwork run still goes on"
    );
    assert!(title_guard::holder(&rig.lab.db, item).is_some());
    rig.follow_ups_done().await;
    assert!(!title_guard::is_pinned(&rig.lab.db, item), "the pin ends");
    assert_eq!(title_guard::holder(&rig.lab.db, item), None);
    // What was under way when the job was cancelled is written whole, in the title's folder.
    assert!(folder.join("Das Boot (1981)-poster.jpg").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn when_no_artwork_provider_knows_the_title_the_nfo_is_still_written_and_the_reason_is_recorded()
 {
    let rig = rig();
    let (item, folder) = rig.movie();

    // This title has no IMDb ID, so the artwork provider has nothing to look up.
    rig.scrape(item, "2").await;

    assert_eq!(
        names(&folder),
        ["Das Boot (1981).mkv", "Das Boot (1981).nfo"]
    );
    assert_eq!(
        rig.artwork_outcome(item),
        (
            StepStatus::NotFound,
            "No artwork provider knows this title.".to_owned()
        )
    );
    // Activity says so too, naming the title.
    let notices = rig.notices.lock().unwrap().clone();
    let artwork: Vec<_> = notices
        .iter()
        .filter(|n| n.source.as_deref() == Some("Artwork"))
        .collect();
    assert_eq!(artwork.len(), 1, "{notices:?}");
    assert!(artwork[0].title.contains("Das Boot"), "{:?}", artwork[0]);
    assert_eq!(
        artwork[0].detail.as_deref(),
        Some("No artwork provider knows this title.")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn undoing_a_scrape_writes_the_nfo_of_the_restored_title_again() {
    let rig = rig();
    let (item, folder) = rig.movie();
    let nfo = folder.join("Das Boot (1981).nfo");

    let token = rig.matching.keep_scrape_snapshot(item).expect("a snapshot");
    rig.scrape(item, "1").await;
    let scraped = fs::read_to_string(&nfo).unwrap();
    assert!(scraped.contains("Ein U-Boot im Atlantik."), "{scraped}");

    rig.matching.undo_scrape(token).unwrap();
    // The NFO is written again in the background, from the restored library row.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        let text = fs::read_to_string(&nfo).unwrap_or_default();
        if !text.contains("Ein U-Boot im Atlantik.") && !text.contains("tt0082096") {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the NFO still describes the undone scrape: {text}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(rig.lab.item(item).core.plot, "");
}
