//! Every automation action of the batch engine runs against mock providers, mock tools
//! and none reports `unavailable`.
//!
//! The mocks:
//!
//! * the metadata and artwork providers are in-process fakes (no network);
//! * FFmpeg is a tiny program compiled with rustc
//!   from `tests/support/mock_*.rs`;
//!
//! The library is real: a temp data folder, real files, a real scan.

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
use cv_app::actions::{CHECKSUMS_AFTER_SCRAPE_KEY, OutcomeKind, RatingSources};
use cv_app::automation::{
    BatchEngine, BatchReport, CommandBackend, EngineOptions, EntityKind, ItemAction, Plan,
    PlannedStep, Scope, Step, StepReport, StepStatus, run_item_action,
};
use cv_app::data_location::{DataLocation, LocationInputs};
use cv_app::media_info::{MediaReader, ReaderSource};
use cv_app::organize::OrganizeService;
use cv_app::provider_credentials::ProviderCredentialsService;
use cv_app::secrets::SecretStore;
use cv_app::services::{AppServices, BootOptions};
use cv_artwork::frame_source::file_url;
use cv_db::repo::data_sources::insert_source;
use cv_db::repo::media_info::{list_unreadable, pending_media_info};
use cv_db::repo::{ItemQuery, get_item, list_items, set_item_locked, update_item};
use cv_domain::{
    ItemId, ItemKind, MediaFile, MediaFileType, ProviderIds, Rating, Ratings, SourceKind,
};
use cv_media::mediainfo::{MediaInfoError, MediaSnapshot, StreamInfo, StreamKind};
use cv_media::tools::ToolId;
use cv_providers::model::{ArtworkCandidate, MediaKind, RequestOptions};
use cv_providers::{
    ArtworkProvider, CredentialResolver, MemoryUserCredentials, Provider, ProviderError,
    ProviderRegistry, RatingFetcher, RatingProvider,
};
use image::{ImageFormat, Rgb, RgbImage};
use serde_json::json;
use support::{Entry, Scripted, arc};
use tempfile::TempDir;

// ---- the mock programs ----------------------------------------------------------------------

fn build_mock(name: &'static str) -> PathBuf {
    let dir = tempfile::tempdir().expect("temp dir").keep();
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/support")
        .join(format!("mock_{name}.rs"));
    let out = dir.join(if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    });
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let status = Command::new(rustc)
        .args(["--edition", "2021", "-o"])
        .arg(&out)
        .arg(&source)
        .status()
        .expect("run rustc");
    assert!(status.success(), "mock {name} did not compile");
    out
}

fn mock_ffmpeg() -> &'static Path {
    static BUILT: OnceLock<PathBuf> = OnceLock::new();
    BUILT.get_or_init(|| build_mock("ffmpeg"))
}

/// The copy of a mock a test owns, so its call log is its own.
fn own_copy(mock: &Path, dir: &Path, name: &str) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let target = dir.join(if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    });
    std::fs::copy(mock, &target).unwrap();
    #[cfg(unix)]
    wait_until_runnable(&target);
    target
}

/// Linux refuses to run a file ("text file busy") while any process still holds it open for
/// writing, and a process another test is starting at that moment briefly holds a copy of the
/// descriptor we just closed. Runs the file until it is accepted, so the test's first real call
/// does not hit that window.
#[cfg(unix)]
fn wait_until_runnable(program: &Path) {
    const TEXT_FILE_BUSY: i32 = 26;
    for _ in 0..200 {
        match Command::new(program).arg("-version").output() {
            Err(error) if error.raw_os_error() == Some(TEXT_FILE_BUSY) => {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            _ => return,
        }
    }
}

// ---- the mock providers ---------------------------------------------------------------------

struct Fixed {
    id: &'static str,
    ratings: Vec<Rating>,
}

#[async_trait]
impl Provider for Fixed {
    fn id(&self) -> &'static str {
        self.id
    }
    fn name(&self) -> &'static str {
        self.id
    }
}

#[async_trait]
impl RatingProvider for Fixed {
    async fn ratings(
        &self,
        _kind: MediaKind,
        _ids: &ProviderIds,
        _options: &RequestOptions,
    ) -> Result<Ratings, ProviderError> {
        Ok(self.ratings.iter().cloned().collect())
    }
}

fn rating_sources(resolver: &CredentialResolver) -> RatingSources {
    let dataset = Arc::new(Fixed {
        id: "imdbdataset",
        ratings: vec![Rating::new("imdb", 8.3, 700_123, 10)],
    });
    RatingSources {
        fetcher: RatingFetcher::new(resolver.clone()).with_dataset(dataset),
        dataset: None,
        scrapers: Vec::new(),
    }
}

/// An artwork provider whose images are files on disk (`file:` addresses).
struct Gallery {
    poster: String,
}

#[async_trait]
impl Provider for Gallery {
    fn id(&self) -> &'static str {
        "imdb"
    }
    fn name(&self) -> &'static str {
        "Gallery"
    }
}

#[async_trait]
impl ArtworkProvider for Gallery {
    async fn artwork(
        &self,
        _kind: MediaKind,
        _ids: &ProviderIds,
        _options: &RequestOptions,
    ) -> Result<Vec<ArtworkCandidate>, ProviderError> {
        Ok(vec![ArtworkCandidate {
            kind: MediaFileType::Poster,
            url: self.poster.clone(),
            language: Some("en".to_owned()),
            width: 2000,
            height: 3000,
            score: 8.0,
            popularity: cv_providers::model::Popularity::Unknown,
        }])
    }
}

// ---- the rig --------------------------------------------------------------------------------

fn location(root: &Path) -> DataLocation {
    let inputs = LocationInputs {
        exe_dir: root.join("app"),
        user_data_base: root.join("userdata"),
        user_config_base: root.join("userconfig"),
    };
    std::fs::create_dir_all(&inputs.exe_dir).unwrap();
    DataLocation::resolve(&inputs)
}

struct FakeReader;

impl MediaReader for FakeReader {
    fn read(&self, _path: &Path) -> Result<MediaSnapshot, MediaInfoError> {
        let mut snapshot = MediaSnapshot::new();
        let mut general = StreamInfo::new();
        general.push("Format", "Matroska");
        general.push("Duration", "3600000");
        snapshot.push(StreamKind::General, general);
        let mut video = StreamInfo::new();
        video.push("Format", "HEVC");
        video.push("Width", "1920");
        video.push("Height", "1080");
        snapshot.push(StreamKind::Video, video);
        Ok(snapshot)
    }

    fn version(&self) -> String {
        "test".to_owned()
    }
}

fn jpeg() -> Vec<u8> {
    let image = RgbImage::from_pixel(40, 60, Rgb([9, 200, 10]));
    let mut out = std::io::Cursor::new(Vec::new());
    image.write_to(&mut out, ImageFormat::Jpeg).unwrap();
    out.into_inner()
}

struct Rig {
    temp: TempDir,
    services: Arc<AppServices>,
    engine: Arc<BatchEngine>,
}

impl Rig {
    fn new() -> Self {
        Self::build(|scripted| scripted)
    }

    /// A rig whose metadata provider also knows one collection, "Heat Collection" (`tmdbSet` 10).
    fn with_collection() -> Self {
        Self::build(|scripted| scripted.with_collection("10", "Heat Collection"))
    }

    fn build(provider: impl FnOnce(Scripted) -> Scripted) -> Self {
        Self::build_with(provider, false)
    }

    /// `scan_hooks` keeps what the app does after a scan (match); the other
    /// tests replace it with nothing and run each step themselves.
    fn build_with(provider: impl FnOnce(Scripted) -> Scripted, scan_hooks: bool) -> Self {
        let temp = TempDir::new().unwrap();
        let poster = temp.path().join("gallery-poster.jpg");
        std::fs::write(&poster, jpeg()).unwrap();
        let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
        let mut registry = ProviderRegistry::new();
        registry.register_metadata(arc(provider(Scripted::new(
            "scripted",
            vec![
                Entry::new("1", "Alien", 1979),
                Entry::new("2", "Heat", 1995),
            ],
        ))));
        registry.register_artwork(Arc::new(Gallery {
            poster: file_url(&poster),
        }));
        let providers = ProviderCredentialsService::with_resolver(resolver.clone(), registry);
        let mut options =
            BootOptions::new(location(temp.path()), Arc::new(SecretStore::in_memory()))
                .with_providers(providers);
        // Changes are announced at once, so a test can read them right after the write.
        options.change_window = std::time::Duration::ZERO;
        let services = AppServices::boot(options).expect("boot");
        services.scan().set_media_reader({
            let source: ReaderSource =
                Arc::new(|| Ok(Arc::new(FakeReader) as Arc<dyn MediaReader>));
            source
        });
        // The batch matches explicitly; no title is matched behind its back.
        if !scan_hooks {
            services.scan().set_after_scan(Arc::new(|_| {}));
        }
        services
            .settings()
            .set("media.runtime-from-media", true.into())
            .unwrap();
        services
            .actions()
            .set_rating_sources(rating_sources(&resolver));

        let tools_dir = temp.path().join("mock-tools");
        let ffmpeg = own_copy(mock_ffmpeg(), &tools_dir.join("ffmpeg"), "ffmpeg");
        services
            .tools()
            .set_path(services.settings(), ToolId::Ffmpeg, Some(&ffmpeg))
            .unwrap();
        let engine = BatchEngine::new(Arc::clone(&services), EngineOptions::default());
        Self {
            temp,
            services,
            engine,
        }
    }

    fn root(&self, name: &str) -> PathBuf {
        self.temp.path().join(name)
    }

    fn add_source(&self, kind: SourceKind, name: &str) {
        let path = self.root(name);
        std::fs::create_dir_all(&path).unwrap();
        let text = path.to_string_lossy().into_owned();
        self.services
            .db()
            .write(move |tx| insert_source(tx, kind, &text))
            .unwrap();
    }

    /// A movie folder with a video big enough to hash.
    fn movie(&self, source: &str, folder: &str) -> PathBuf {
        let dir = self.root(source).join(folder);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{folder}.mkv")), vec![7_u8; 300 * 1024]).unwrap();
        dir
    }

    fn plan(kind: EntityKind, dry_run: bool, steps: &[(Step, Option<Scope>)]) -> Plan {
        Plan {
            kind,
            dry_run,
            steps: steps
                .iter()
                .map(|(step, scope)| {
                    let mut planned = PlannedStep::new(*step);
                    planned.scope = *scope;
                    planned
                })
                .collect(),
        }
    }

    async fn run(&self, plan: Plan) -> BatchReport {
        self.engine.run_plan(&plan, &|_| {}).await
    }

    fn movie_id(&self) -> ItemId {
        self.services
            .db()
            .read(|c| list_items(c, &ItemQuery::new(ItemKind::Movie), None, 10))
            .unwrap()
            .items[0]
            .id
    }

    fn item(&self, id: ItemId) -> cv_domain::Item {
        self.services
            .db()
            .read(|c| get_item(c, id))
            .unwrap()
            .unwrap()
    }
}

fn step(report: &BatchReport, which: Step) -> &StepReport {
    report
        .steps
        .iter()
        .find(|s| s.step == which)
        .unwrap_or_else(|| panic!("no {which} step in {:?}", report.steps))
}

fn files_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .filter(|e| e.path().is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Scans the movie source, scrapes the movie and reads its media info.
async fn library_with_one_movie(rig: &Rig) -> (ItemId, PathBuf) {
    rig.add_source(SourceKind::Movie, "movies");
    let dir = rig.movie("movies", "heat.1995.1080p");
    let report = rig
        .run(Rig::plan(
            EntityKind::Movie,
            false,
            &[
                (Step::Update, None),
                (Step::Scrape, Some(Scope::New)),
                (Step::MediaInfo, None),
            ],
        ))
        .await;
    assert!(report.succeeded(), "{report:?}");
    // The steps after the scrape (the ratings fetch, which writes the NFO too) run in the
    // background; a test starts from the title they leave behind.
    let id = rig.movie_id();
    let working = Arc::clone(rig.services.lifecycle().working());
    assert!(
        wait_until(|| !working.is_working(id)).await,
        "the steps after the scrape did not finish"
    );
    (id, dir)
}

// ---- the tests ------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_action_runs_against_the_mocks_and_none_reports_unavailable() {
    let rig = Rig::new();
    let (id, dir) = library_with_one_movie(&rig).await;

    let all = Some(Scope::All);
    let mut plan = Rig::plan(
        EntityKind::Movie,
        false,
        &[
            (Step::Checksum, all),
            (Step::AspectRatio, all),
            (Step::Ratings, all),
            (Step::Artwork, all),
            (Step::ArtworkCleanup, all),
        ],
    );
    // `calculateChecksum` with `type: all` asks for the CRC-32 and the perceptual hash.
    plan.steps[0]
        .options
        .args
        .insert("type".to_owned(), "all".to_owned());
    let report = rig.run(plan).await;
    assert!(report.error.is_none(), "{:?}", report.error);
    for done in &report.steps {
        assert_ne!(done.status, StepStatus::Unavailable, "{done:?}");
        assert_ne!(done.status, StepStatus::Failed, "{done:?}");
        assert!(!done.has_failed_items(), "{done:?}");
    }
    assert_eq!(report.exit_code(), 0, "{report:?}");

    // Checksums and the aspect ratio are on the video file.
    let video = rig
        .item(id)
        .core
        .media_files
        .into_iter()
        .find(|f| f.kind == MediaFileType::Video)
        .unwrap();
    assert_eq!(video.checksums["crc32"].len(), 8, "{:?}", video.checksums);
    assert_eq!(video.checksums["phash"].len(), 16, "{:?}", video.checksums);
    assert!(
        (video.aspect_ratio - 1.33).abs() < 0.02,
        "aspect ratio {}",
        video.aspect_ratio
    );
    assert_eq!(step(&report, Step::Checksum).status, StepStatus::Done);
    assert_eq!(step(&report, Step::AspectRatio).status, StepStatus::Done);

    // The rating came from the (mock) source and is stored.
    assert_eq!(step(&report, Step::Ratings).status, StepStatus::Done);
    assert!(
        rig.item(id).core.ratings.iter().any(|r| r.id == "imdb"),
        "no imdb rating"
    );

    // The poster is next to the video.
    let names = files_in(&dir);
    assert!(names.iter().any(|n| n.contains("poster")), "{names:?}");
    assert_eq!(step(&report, Step::Artwork).status, StepStatus::Done);

    // Nothing to tidy yet; the clean-up says so.
    let cleanup = step(&report, Step::ArtworkCleanup);
    assert_eq!(cleanup.counts["unchanged"], 1, "{cleanup:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_second_run_changes_nothing_and_says_so() {
    let rig = Rig::new();
    // The artwork step is driven explicitly below. The after-scrape download, and the ratings
    // fetch that writes the NFO, would race it (and the folder snapshot) from a background task
    // nothing can wait on.
    rig.services.artwork().set_after_scrape_runs(false);
    rig.services
        .settings()
        .set("ratings.fetch-after-scrape-movie", json!(false))
        .unwrap();
    let (_id, dir) = library_with_one_movie(&rig).await;
    let all = Some(Scope::All);
    let steps = [
        (Step::Checksum, all),
        (Step::AspectRatio, all),
        (Step::Ratings, all),
        (Step::Artwork, all),
    ];
    let first = rig.run(Rig::plan(EntityKind::Movie, false, &steps)).await;
    assert!(first.succeeded(), "{first:?}");
    let before = files_in(&dir);

    let second = rig.run(Rig::plan(EntityKind::Movie, false, &steps)).await;
    assert!(second.succeeded(), "{second:?}");
    assert_eq!(files_in(&dir), before, "nothing new is written");
    // The artwork is never fetched again.
    for fixed in [Step::Checksum, Step::Artwork] {
        let again = step(&second, fixed);
        assert_eq!(again.counts["unchanged"], 1, "{again:?}");
        assert_eq!(again.counts["done"], 0, "{again:?}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_dry_run_plans_every_action_and_changes_nothing() {
    let rig = Rig::new();
    // The artwork step is driven explicitly below. The after-scrape download, and the ratings
    // fetch that writes the NFO, would race it (and the folder snapshot) from a background task
    // nothing can wait on.
    rig.services.artwork().set_after_scrape_runs(false);
    rig.services
        .settings()
        .set("ratings.fetch-after-scrape-movie", json!(false))
        .unwrap();
    let (id, dir) = library_with_one_movie(&rig).await;
    let before = files_in(&dir);
    let item_before = rig.item(id);

    let all = Some(Scope::All);
    let report = rig
        .run(Rig::plan(
            EntityKind::Movie,
            true,
            &[
                (Step::Checksum, all),
                (Step::AspectRatio, all),
                (Step::Ratings, all),
                (Step::Artwork, all),
                (Step::ArtworkCleanup, all),
            ],
        ))
        .await;
    assert!(report.error.is_none());
    for done in &report.steps {
        assert!(
            matches!(done.status, StepStatus::Planned | StepStatus::Skipped),
            "{done:?}"
        );
    }
    assert_eq!(files_in(&dir), before);
    assert_eq!(rig.item(id), item_before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_missing_tool_makes_its_step_unavailable_and_the_others_still_run() {
    let rig = Rig::new();
    let (_id, _dir) = library_with_one_movie(&rig).await;
    // The user's paths are cleared: nothing is installed.
    rig.services
        .tools()
        .set_path(rig.services.settings(), ToolId::Ffmpeg, None)
        .unwrap();
    let all = Some(Scope::All);
    let report = rig
        .run(Rig::plan(
            EntityKind::Movie,
            false,
            &[
                (Step::AspectRatio, all),
                (Step::Checksum, all),
                (Step::Ratings, all),
            ],
        ))
        .await;
    let aspect = step(&report, Step::AspectRatio);
    assert_eq!(aspect.status, StepStatus::Unavailable);
    assert!(
        aspect.message.contains("Settings, External tools"),
        "{}",
        aspect.message
    );
    // The CRC-32 needs no tool; the ratings step ran after the one that could not.
    assert_eq!(step(&report, Step::Checksum).status, StepStatus::Done);
    assert_eq!(step(&report, Step::Ratings).status, StepStatus::Done);
    assert_eq!(report.exit_code(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_source_that_wants_a_key_skips_with_a_plain_sentence() {
    let rig = Rig::new();
    let (id, _dir) = library_with_one_movie(&rig).await;
    // OMDb wants the user's key and none is set; the real provider never sends a request.
    let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
    let http = cv_providers::HttpClient::builder().build().unwrap();
    rig.services.actions().set_rating_sources(RatingSources {
        fetcher: RatingFetcher::new(resolver.clone())
            .with_omdb(Arc::new(cv_providers::OmdbProvider::new(http, resolver))),
        dataset: None,
        scrapers: Vec::new(),
    });
    rig.services
        .settings()
        .set("ratings.sources-movie", json!("omdb"))
        .unwrap();
    let report = rig
        .run(Rig::plan(
            EntityKind::Movie,
            false,
            &[(Step::Ratings, Some(Scope::All))],
        ))
        .await;
    let ratings = step(&report, Step::Ratings);
    assert_eq!(ratings.status, StepStatus::Skipped, "{ratings:?}");
    assert!(
        ratings.message.contains("no API key")
            && ratings.message.contains("Scraping and providers"),
        "{}",
        ratings.message
    );
    // A skip is not a failure of the batch.
    assert_eq!(report.exit_code(), 0);
    let _ = id;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn artwork_the_naming_no_longer_uses_goes_to_the_trash_folder() {
    let rig = Rig::new();
    let (id, dir) = library_with_one_movie(&rig).await;
    let all = Some(Scope::All);
    let first = rig
        .run(Rig::plan(EntityKind::Movie, false, &[(Step::Artwork, all)]))
        .await;
    assert_eq!(step(&first, Step::Artwork).status, StepStatus::Done);
    let old = dir.join("heat.1995.1080p-poster.jpg");
    assert!(old.is_file(), "{:?}", files_in(&dir));

    // The user switches to short names and a poster under the new name appears.
    rig.services
        .settings()
        .set("metadata.artwork-names", json!("short"))
        .unwrap();
    std::fs::copy(&old, dir.join("poster.jpg")).unwrap();
    let dir_text = dir.to_string_lossy().into_owned();
    rig.services
        .db()
        .write(move |tx| {
            let mut item = get_item(tx, id)?.unwrap();
            item.core
                .media_files
                .retain(|f| f.kind != MediaFileType::Poster);
            for name in ["heat.1995.1080p-poster.jpg", "poster.jpg"] {
                item.core.media_files.push(MediaFile::new(
                    MediaFileType::Poster,
                    dir_text.clone(),
                    name,
                ));
            }
            update_item(tx, &item)
        })
        .unwrap();

    // A dry run lists the file and moves nothing.
    let planned = rig
        .run(Rig::plan(
            EntityKind::Movie,
            true,
            &[(Step::ArtworkCleanup, all)],
        ))
        .await;
    let step_report = step(&planned, Step::ArtworkCleanup);
    assert_eq!(step_report.status, StepStatus::Planned, "{step_report:?}");
    assert!(old.is_file());

    let done = rig
        .run(Rig::plan(
            EntityKind::Movie,
            false,
            &[(Step::ArtworkCleanup, all)],
        ))
        .await;
    assert_eq!(step(&done, Step::ArtworkCleanup).status, StepStatus::Done);
    assert!(!old.exists(), "the old poster is still there");
    assert!(dir.join("poster.jpg").is_file(), "the poster in use stays");
    // Nothing was deleted: the file is in the trash folder of the data source.
    let trash = rig.root("movies").join(".deletedByCineVault");
    let mut moved = Vec::new();
    let mut stack = vec![trash];
    while let Some(next) = stack.pop() {
        for entry in std::fs::read_dir(&next).into_iter().flatten().flatten() {
            if entry.path().is_dir() {
                stack.push(entry.path());
            } else {
                moved.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
    }
    assert_eq!(moved, ["heat.1995.1080p-poster.jpg"]);
    // The library no longer lists it.
    assert!(
        rig.item(id)
            .core
            .media_files
            .iter()
            .all(|f| f.filename != "heat.1995.1080p-poster.jpg")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_after_scrape_steps_run_for_a_title_the_settings_switch_them_on_for() {
    let rig = Rig::new();
    rig.add_source(SourceKind::Movie, "movies");
    let dir = rig.movie("movies", "heat.1995.1080p");
    let report = rig
        .run(Rig::plan(
            EntityKind::Movie,
            false,
            &[(Step::Update, None), (Step::MediaInfo, None)],
        ))
        .await;
    assert!(report.succeeded(), "{report:?}");
    // Match it through the scrape pipeline with the after-scrape steps switched on.
    let settings = rig.services.settings();
    settings
        .set(CHECKSUMS_AFTER_SCRAPE_KEY, json!("crc32"))
        .unwrap();
    let scrape = rig
        .run(Rig::plan(
            EntityKind::Movie,
            false,
            &[(Step::Scrape, Some(Scope::New))],
        ))
        .await;
    assert!(scrape.succeeded(), "{scrape:?}");

    let id = rig.movie_id();
    let waited = wait_until(|| {
        let item = rig.item(id);
        item.core
            .media_files
            .iter()
            .any(|f| f.kind == MediaFileType::Video && f.checksums.contains_key("crc32"))
    })
    .await;
    assert!(
        waited,
        "after-scrape steps did not finish: {:?}",
        files_in(&dir)
    );
    // The rating (on by default for movies) was fetched too.
    assert!(wait_until(|| rig.item(id).core.ratings.iter().any(|r| r.id == "imdb")).await);
}

/// A scrape renames nothing (spec renamer, "Renaming is never automatic"): a folder named like a
/// release keeps its name through a scrape and its after-scrape steps, even when an old settings
/// file still holds the removed "automatic" rename mode, and no rename batch is journaled.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_scrape_renames_nothing_even_with_a_leftover_automatic_setting() {
    let rig = Rig::new();
    rig.services
        .settings()
        .set("renamer.mode", json!("automatic"))
        .unwrap();
    let (id, dir) = library_with_one_movie(&rig).await;
    let before = files_in(&dir);
    assert!(wait_until(|| rig.item(id).core.ratings.iter().any(|r| r.id == "imdb")).await);
    // The after-scrape steps (ratings, artwork, NFO) have had their time.
    rig.services.scrape_follow_up().idle().await;

    assert!(dir.is_dir(), "the folder keeps its name");
    assert!(
        dir.join("heat.1995.1080p.mkv").is_file(),
        "{:?}",
        files_in(&dir)
    );
    let journal = OrganizeService::new(Arc::clone(rig.services.db()))
        .journal()
        .unwrap();
    assert!(journal.is_empty(), "no rename was journaled: {journal:?}");
    assert!(files_in(&dir).len() >= before.len());
}

/// The live snapshot of the persistent job with the stored row id `row`.
fn job_snapshot(rig: &Rig, row: i64) -> Option<cv_jobs::JobSnapshot> {
    let jobs = rig.services.jobs();
    jobs.list()
        .into_iter()
        .find(|snapshot| jobs.get(snapshot.id).and_then(|h| h.stored_id()) == Some(row))
}

async fn wait_until(mut done: impl FnMut() -> bool) -> bool {
    for _ in 0..300 {
        if done() {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    }
    false
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn no_action_is_unavailable_any_more_and_the_clean_up_says_where_to_switch_it_on() {
    let rig = Rig::new();
    let (_id, _dir) = library_with_one_movie(&rig).await;
    let report = rig
        .run(Rig::plan(
            EntityKind::Movie,
            false,
            &[
                (Step::MovieSet, Some(Scope::All)),
                (Step::Cleanup, Some(Scope::All)),
            ],
        ))
        .await;
    let unavailable: Vec<Step> = report
        .steps
        .iter()
        .filter(|s| s.status == StepStatus::Unavailable)
        .map(|s| s.step)
        .collect();
    assert!(unavailable.is_empty(), "{unavailable:?}");
    // Without a provider that knows collections, assigning them is a skip that names the page.
    let sets = step(&report, Step::MovieSet);
    assert_eq!(sets.status, StepStatus::Skipped, "{sets:?}");
    assert!(
        sets.message.contains("Scraping and providers"),
        "{}",
        sets.message
    );
    // The clean-up is off in the built-in profiles; the skip names the page that turns it on.
    let cleanup = step(&report, Step::Cleanup);
    assert_eq!(cleanup.status, StepStatus::Skipped, "{cleanup:?}");
    assert!(cleanup.message.contains("Organize"), "{}", cleanup.message);
}

// assignMovieSet runs "Find collections": the movie goes into its TMDB collection, which is
// created when the library has none, and a second run leaves it there.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn assign_movie_set_puts_movies_into_their_collections() {
    let rig = Rig::with_collection();
    let (id, _dir) = library_with_one_movie(&rig).await;
    // Start from a movie in no collection that knows its collection ID.
    rig.services
        .db()
        .write(move |tx| {
            let mut item = get_item(tx, id)?.unwrap();
            item.parent_id = None;
            item.core.ids.set(cv_domain::id_key::TMDB_SET, "10");
            update_item(tx, &item)
        })
        .unwrap();

    let dry = rig
        .run(Rig::plan(
            EntityKind::Movie,
            true,
            &[(Step::MovieSet, Some(Scope::All))],
        ))
        .await;
    assert_eq!(step(&dry, Step::MovieSet).status, StepStatus::Planned);
    assert_eq!(rig.item(id).parent_id, None, "a dry run changes nothing");

    let report = rig
        .run(Rig::plan(
            EntityKind::Movie,
            false,
            &[(Step::MovieSet, Some(Scope::All))],
        ))
        .await;
    let sets = step(&report, Step::MovieSet);
    assert_eq!(sets.status, StepStatus::Done, "{sets:?}");
    assert_eq!(sets.counts.get("assigned"), Some(&1), "{sets:?}");
    assert_eq!(sets.counts.get("created"), Some(&1), "{sets:?}");
    assert_eq!(sets.items.len(), 1);
    assert_eq!(sets.items[0].status, "done");
    let set = rig
        .item(id)
        .parent_id
        .expect("the movie is in a collection");
    assert_eq!(rig.item(set).core.title, "Heat Collection");

    let again = rig
        .run(Rig::plan(
            EntityKind::Movie,
            false,
            &[(Step::MovieSet, Some(Scope::All))],
        ))
        .await;
    let sets = step(&again, Step::MovieSet);
    assert_eq!(sets.counts.get("assigned"), Some(&0), "{sets:?}");
    assert_eq!(sets.counts.get("unchanged"), Some(&1), "{sets:?}");
    assert_eq!(rig.item(id).parent_id, Some(set));
}

// The movie set endpoint takes the actions that make sense for sets and says so for the rest.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn movie_sets_take_the_set_level_actions() {
    let rig = Rig::new();
    let report = rig
        .run(Rig::plan(
            EntityKind::Movieset,
            false,
            &[
                (Step::ArtworkCleanup, Some(Scope::All)),
                (Step::Ratings, Some(Scope::All)),
            ],
        ))
        .await;
    for step_report in &report.steps {
        let expected = if step_report.step == Step::Ratings {
            StepStatus::Unavailable
        } else {
            // No set exists yet, so there is nothing to do.
            StepStatus::Skipped
        };
        assert_eq!(step_report.status, expected, "{step_report:?}");
    }
}

// ---- selection actions -----------------------------------------------------------

impl Rig {
    /// A custom naming profile that switches the unwanted-file clean-up on, and makes it active.
    fn clean_up_unwanted(&self, patterns: &[&str]) {
        let section = json!({ "cleanupUnwanted": true, "unwantedPatterns": patterns });
        let settings = self.services.settings();
        settings
            .set(
                "renamer.profiles",
                json!([{ "id": "custom-tidy", "name": "Tidy", "movie": section, "tv": section }]),
            )
            .unwrap();
        settings
            .set("renamer.active-profile", json!("custom-tidy"))
            .unwrap();
    }

    fn movie_ids(&self) -> Vec<i64> {
        self.services
            .db()
            .read(|c| list_items(c, &ItemQuery::new(ItemKind::Movie), None, 50))
            .unwrap()
            .items
            .iter()
            .map(|summary| summary.id.0)
            .collect()
    }
}

/// Every file below `dir`, relative, with forward slashes.
fn tree_of(dir: &Path) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                out.push(
                    path.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unwanted_files_are_previewed_then_moved_to_the_trash_folder_and_the_batch_can_be_undone() {
    let rig = Rig::new();
    rig.services.artwork().set_after_scrape_runs(false);
    let (id, dir) = library_with_one_movie(&rig).await;
    rig.clean_up_unwanted(&[".txt", ".url"]);
    std::fs::write(dir.join("notes.txt"), "n").unwrap();
    std::fs::write(dir.join("RELEASE.URL"), "[InternetShortcut]").unwrap();
    std::fs::create_dir_all(dir.join("Extras")).unwrap();
    std::fs::write(dir.join("Extras").join("readme.txt"), "deeper").unwrap();
    let before = tree_of(&rig.root("movies"));

    // The preview names the files and changes nothing.
    let preview = rig
        .run(ItemAction::CleanupUnwanted { dry_run: true }.plan(EntityKind::Movie, &[id.0]))
        .await;
    let cleanup = step(&preview, Step::Cleanup);
    assert_eq!(cleanup.status, StepStatus::Planned, "{cleanup:?}");
    let said = cleanup.items[0].message.clone().unwrap_or_default();
    assert!(
        said.contains("notes.txt") && said.contains("RELEASE.URL"),
        "{said}"
    );
    assert_eq!(tree_of(&rig.root("movies")), before);

    // The real run moves them below the trash folder of the data source; the deeper folder stays.
    let done = rig
        .run(ItemAction::CleanupUnwanted { dry_run: false }.plan(EntityKind::Movie, &[id.0]))
        .await;
    let cleanup = step(&done, Step::Cleanup);
    assert_eq!(cleanup.status, StepStatus::Done, "{cleanup:?}");
    assert_eq!(cleanup.counts["done"], 1, "{cleanup:?}");
    let now = tree_of(&rig.root("movies"));
    assert!(
        !now.contains(&"heat.1995.1080p/notes.txt".to_owned()),
        "{now:?}"
    );
    assert!(
        now.contains(&"heat.1995.1080p/heat.1995.1080p.mkv".to_owned()),
        "{now:?}"
    );
    assert!(
        now.contains(&"heat.1995.1080p/Extras/readme.txt".to_owned()),
        "{now:?}"
    );
    let trashed: Vec<&String> = now
        .iter()
        .filter(|path| path.starts_with(".deletedByCineVault/"))
        .collect();
    assert_eq!(trashed.len(), 2, "{now:?}");
    assert!(
        trashed.iter().all(|p| p.contains("/heat.1995.1080p/")),
        "{trashed:?}"
    );

    // Nothing is left to do, and the move is one batch of the Organize journal.
    let again = rig
        .run(ItemAction::CleanupUnwanted { dry_run: false }.plan(EntityKind::Movie, &[id.0]))
        .await;
    assert_eq!(step(&again, Step::Cleanup).counts["unchanged"], 1);
    let organize = OrganizeService::new(Arc::clone(rig.services.db()));
    let batches = organize.journal().unwrap();
    assert_eq!(batches.len(), 1, "{batches:?}");
    organize.undo(&batches[0].batch_id).unwrap();
    assert_eq!(tree_of(&rig.root("movies")), before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_clean_up_switch_of_organize_lists_the_default_file_types_on_the_active_preset() {
    let rig = Rig::new();
    rig.services.artwork().set_after_scrape_runs(false);
    let (id, dir) = library_with_one_movie(&rig).await;
    std::fs::write(dir.join("RARBG.txt"), "n").unwrap();
    std::fs::write(dir.join("Visit us.url"), "[InternetShortcut]").unwrap();
    let preview = ItemAction::CleanupUnwanted { dry_run: true }.plan(EntityKind::Movie, &[id.0]);

    // Off on a fresh install: the preview says where to turn it on.
    let off = rig.run(preview.clone()).await;
    let cleanup = step(&off, Step::Cleanup);
    assert_ne!(cleanup.status, StepStatus::Planned, "{cleanup:?}");
    assert!(cleanup.message.contains("Organize"), "{}", cleanup.message);

    // The switch works with the built-in profile and the default file types.
    rig.services
        .settings()
        .set("renamer.cleanup-unwanted", json!(true))
        .unwrap();
    let on = rig.run(preview).await;
    let cleanup = step(&on, Step::Cleanup);
    assert_eq!(cleanup.status, StepStatus::Planned, "{cleanup:?}");
    let said = cleanup.items[0].message.clone().unwrap_or_default();
    assert!(
        said.contains("RARBG.txt") && said.contains("Visit us.url"),
        "{said}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_clean_up_never_takes_a_video_and_leaves_locked_titles_alone() {
    let rig = Rig::new();
    rig.services.artwork().set_after_scrape_runs(false);
    let (id, dir) = library_with_one_movie(&rig).await;
    // A pattern that also matches the video of the movie.
    rig.clean_up_unwanted(&[".txt", r"\.mkv$"]);
    std::fs::write(dir.join("notes.txt"), "n").unwrap();
    let run = ItemAction::CleanupUnwanted { dry_run: false }.plan(EntityKind::Movie, &[id.0]);

    rig.services
        .db()
        .write(move |tx| set_item_locked(tx, &[id], true))
        .unwrap();
    let locked = rig.run(run.clone()).await;
    let cleanup = step(&locked, Step::Cleanup);
    assert_eq!(cleanup.status, StepStatus::Skipped, "{cleanup:?}");
    assert!(cleanup.message.contains("locked"), "{}", cleanup.message);
    assert!(dir.join("notes.txt").is_file());

    rig.services
        .db()
        .write(move |tx| set_item_locked(tx, &[id], false))
        .unwrap();
    let done = rig.run(run).await;
    assert_eq!(step(&done, Step::Cleanup).status, StepStatus::Done);
    assert!(!dir.join("notes.txt").exists());
    assert!(dir.join("heat.1995.1080p.mkv").is_file(), "the video stays");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_selection_action_runs_on_exactly_the_chosen_titles_as_a_job_with_a_report() {
    let rig = Rig::new();
    let (first, _dir) = library_with_one_movie(&rig).await;
    rig.movie("movies", "alien.1979.1080p");
    let update = rig
        .run(Rig::plan(EntityKind::Movie, false, &[(Step::Update, None)]))
        .await;
    assert!(update.succeeded(), "{update:?}");
    let ids = rig.movie_ids();
    assert_eq!(ids.len(), 2);
    let other = ItemId(*ids.iter().find(|n| **n != first.0).unwrap());

    let backend = CommandBackend::start(Arc::clone(&rig.engine));
    let action = ItemAction::CalculateChecksums;
    let submitted = backend
        .submit_plan(&action.label(), action.plan(EntityKind::Movie, &[first.0]))
        .expect("queued");
    let job = submitted.job;
    let report = submitted.report(EntityKind::Movie).await;
    assert!(report.succeeded(), "{report:?}");
    assert_eq!(report.selection.titles, 1);
    let checksums = |id: ItemId| {
        rig.item(id)
            .core
            .media_files
            .iter()
            .filter(|f| f.kind == MediaFileType::Video)
            .all(|f| f.checksums.contains_key("crc32"))
    };
    assert!(checksums(first), "the chosen title has its checksum");
    assert!(!checksums(other), "the other title was not touched");

    // The job is in the job list under the name of the action, ready for the Activity drawer.
    assert!(
        wait_until(|| job_snapshot(&rig, job).is_some_and(|view| view.state.is_terminal())).await,
        "the job did not finish"
    );
    let view = job_snapshot(&rig, job).unwrap();
    assert_eq!(view.name, "Calculate checksums");
    assert_eq!(view.failed, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_fetch_ratings_and_redownload_artwork_actions_work_on_a_selection() {
    let rig = Rig::new();
    // Nothing downloads in the background while the test deletes and fetches the poster.
    rig.services.artwork().set_after_scrape_runs(false);
    let (id, dir) = library_with_one_movie(&rig).await;
    let backend = CommandBackend::start(Arc::clone(&rig.engine));
    let run = |action: ItemAction| {
        backend
            .submit_plan(&action.label(), action.plan(EntityKind::Movie, &[id.0]))
            .expect("queued")
            .report(EntityKind::Movie)
    };

    let ratings = run(ItemAction::FetchRatings).await;
    assert_eq!(
        step(&ratings, Step::Ratings).status,
        StepStatus::Done,
        "{ratings:?}"
    );
    assert!(rig.item(id).core.ratings.iter().any(|r| r.id == "imdb"));

    // Artwork is fetched once; deleting the poster and re-downloading brings it back.
    let mut planned = PlannedStep::new(Step::Artwork);
    planned.selection.ids = vec![id.0];
    let first = rig
        .run(Plan {
            kind: EntityKind::Movie,
            dry_run: false,
            steps: vec![planned],
        })
        .await;
    assert_eq!(step(&first, Step::Artwork).status, StepStatus::Done);
    let poster = dir.join("heat.1995.1080p-poster.jpg");
    assert!(poster.is_file(), "{:?}", files_in(&dir));
    std::fs::remove_file(&poster).unwrap();
    let again = run(ItemAction::RedownloadArtwork).await;
    let artwork = step(&again, Step::Artwork);
    assert_eq!(artwork.status, StepStatus::Done, "{artwork:?}");
    assert!(poster.is_file(), "{:?}", files_in(&dir));
}

/// Reads media info through a switch: it counts the reads and fails while `fail` is set.
struct Switch {
    reads: Arc<AtomicUsize>,
    fail: Arc<std::sync::atomic::AtomicBool>,
}

impl MediaReader for Switch {
    fn read(&self, path: &Path) -> Result<MediaSnapshot, MediaInfoError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        if self.fail.load(Ordering::SeqCst) {
            return Err(MediaInfoError::Open {
                path: path.to_path_buf(),
            });
        }
        FakeReader.read(path)
    }

    fn version(&self) -> String {
        "test".to_owned()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_unreadable_video_is_marked_and_only_reload_media_info_reads_it_again() {
    let rig = Rig::new();
    let reads = Arc::new(AtomicUsize::new(0));
    let fail = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let source: ReaderSource = {
        let (reads, fail) = (Arc::clone(&reads), Arc::clone(&fail));
        Arc::new(move || {
            Ok(Arc::new(Switch {
                reads: Arc::clone(&reads),
                fail: Arc::clone(&fail),
            }) as Arc<dyn MediaReader>)
        })
    };
    rig.services.scan().set_media_reader(source);
    rig.add_source(SourceKind::Movie, "movies");
    rig.movie("movies", "heat.1995.1080p");
    let scanned = rig
        .run(Rig::plan(EntityKind::Movie, false, &[(Step::Update, None)]))
        .await;
    assert!(scanned.succeeded(), "{scanned:?}");
    let id = rig.movie_id();

    // The failed read left a mark at the size and time of the file: the title is no longer
    // pending, which is what a later launch sees, so the file is not tried again.
    let db = rig.services.db();
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    assert!(db.read(|c| pending_media_info(c, 10)).unwrap().is_empty());
    let marks = db.read(list_unreadable).unwrap();
    assert_eq!(marks.len(), 1, "{marks:?}");
    assert_eq!(marks[0].filename, "heat.1995.1080p.mkv");
    rig.services.scan().request_media_info();
    rig.services.scan().media_info_idle().await;
    assert_eq!(reads.load(Ordering::SeqCst), 1, "not tried again");
    assert!(!rig.item(id).core.media_files[0].has_media_info());

    // Reload clears the mark and reads the file again, here successfully.
    fail.store(false, Ordering::SeqCst);
    let reload = ItemAction::ReloadMediaInfo.plan(EntityKind::Movie, &[id.0]);
    let report = rig.run(reload.clone()).await;
    let reloaded = step(&report, Step::MediaInfo);
    assert_eq!(reloaded.status, StepStatus::Done, "{reloaded:?}");
    assert_eq!(reloaded.counts["read"], 1, "{reloaded:?}");
    assert_eq!(reads.load(Ordering::SeqCst), 2);
    assert!(db.read(list_unreadable).unwrap().is_empty());
    let video = &rig.item(id).core.media_files[0];
    assert!(video.has_media_info() && video.width == 1920, "{video:?}");

    // A reload reads the file even when it has its details: they are cleared and read again.
    let report = rig.run(reload).await;
    assert_eq!(step(&report, Step::MediaInfo).counts["read"], 1);
    assert_eq!(reads.load(Ordering::SeqCst), 3);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reload_media_info_reads_only_the_titles_it_is_given_and_clears_their_stale_details() {
    let rig = Rig::new();
    let reads = Arc::new(AtomicUsize::new(0));
    let fail = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let source: ReaderSource = {
        let (reads, fail) = (Arc::clone(&reads), Arc::clone(&fail));
        Arc::new(move || {
            Ok(Arc::new(Switch {
                reads: Arc::clone(&reads),
                fail: Arc::clone(&fail),
            }) as Arc<dyn MediaReader>)
        })
    };
    rig.services.scan().set_media_reader(source);
    rig.add_source(SourceKind::Movie, "movies");
    rig.movie("movies", "heat.1995.1080p");
    rig.movie("movies", "alien.1979.1080p");
    let scanned = rig
        .run(Rig::plan(EntityKind::Movie, false, &[(Step::Update, None)]))
        .await;
    assert!(scanned.succeeded(), "{scanned:?}");
    assert_eq!(
        reads.load(Ordering::SeqCst),
        2,
        "both were read by the scan"
    );

    // Both titles now hold details that are out of date.
    let ids = rig.movie_ids();
    assert_eq!(ids.len(), 2);
    for id in &ids {
        let id = ItemId(*id);
        rig.services
            .db()
            .write(move |tx| {
                let mut item = get_item(tx, id)?.expect("item");
                item.core.media_files[0].width = 1;
                update_item(tx, &item)
            })
            .unwrap();
    }

    // Reloading the first one reads that title again and no other.
    let report = rig
        .run(ItemAction::ReloadMediaInfo.plan(EntityKind::Movie, &ids[..1]))
        .await;
    let reloaded = step(&report, Step::MediaInfo);
    assert_eq!(reloaded.status, StepStatus::Done, "{reloaded:?}");
    assert_eq!(reloaded.counts["titles"], 1, "{reloaded:?}");
    assert_eq!(reads.load(Ordering::SeqCst), 3, "one more read, not two");
    assert_eq!(
        rig.item(ItemId(ids[0])).core.media_files[0].width,
        1920,
        "the stale value was replaced by what the file says"
    );
    assert_eq!(
        rig.item(ItemId(ids[1])).core.media_files[0].width,
        1,
        "the other title was left as it was"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reload_media_info_keeps_the_stored_details_when_mediainfo_is_missing() {
    let rig = Rig::new();
    let (id, _dir) = library_with_one_movie(&rig).await;
    assert!(rig.item(id).core.media_files[0].has_media_info());
    let gone: ReaderSource = Arc::new(|| Err(MediaInfoError::OutOfMemory));
    rig.services.scan().set_media_reader(gone);

    let report = rig
        .run(ItemAction::ReloadMediaInfo.plan(EntityKind::Movie, &[id.0]))
        .await;
    let reloaded = step(&report, Step::MediaInfo);
    assert_eq!(reloaded.status, StepStatus::Unavailable, "{reloaded:?}");
    assert!(
        rig.item(id).core.media_files[0].has_media_info(),
        "nothing was cleared"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_selection_runner_reports_one_outcome_and_names_what_it_left_out() {
    let rig = Rig::new();
    let (id, _dir) = library_with_one_movie(&rig).await;
    let backend = CommandBackend::start(Arc::clone(&rig.engine));
    let db = rig.services.db();

    let outcome = run_item_action(&backend, db, &ItemAction::FetchRatings, &[id, ItemId(9999)])
        .await
        .expect("runs");
    assert_eq!(outcome.action, "fetch-ratings");
    assert_eq!(outcome.status, StepStatus::Done, "{outcome:?}");
    assert_eq!((outcome.titles, outcome.done, outcome.failed), (1, 1, 0));
    assert_eq!(outcome.jobs.len(), 1);
    assert!(
        outcome
            .left_out
            .iter()
            .any(|line| line.contains("no longer")),
        "{:?}",
        outcome.left_out
    );

    // Nothing selected, or only titles the library does not have, is an error with a code.
    let none = run_item_action(&backend, db, &ItemAction::FetchRatings, &[]).await;
    assert_eq!(none.unwrap_err().code(), "nothing_selected");
    let gone = run_item_action(&backend, db, &ItemAction::FetchRatings, &[ItemId(9999)]).await;
    assert_eq!(gone.unwrap_err().code(), "unsupported");
}

// ---- step outcomes ------------------------------------------

fn outcomes_of(rig: &Rig, id: ItemId) -> Vec<cv_db::repo::step_outcome::StepOutcome> {
    rig.services
        .db()
        .read(move |c| cv_db::repo::step_outcome::step_outcomes(c, id))
        .unwrap()
}

fn outcome_for(
    rig: &Rig,
    id: ItemId,
    step: &str,
) -> Option<cv_db::repo::step_outcome::StepOutcome> {
    outcomes_of(rig, id).into_iter().find(|o| o.step == step)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_ratings_step_records_needs_key_and_not_found_instead_of_staying_silent() {
    use cv_db::repo::step_outcome::StepStatus as Recorded;

    let rig = Rig::new();
    // Nothing runs behind the test's back: it fetches the ratings itself.
    let settings = rig.services.settings();
    settings
        .set("ratings.fetch-after-scrape-movie", json!(false))
        .unwrap();
    rig.services.artwork().set_after_scrape_runs(false);
    let (id, _dir) = library_with_one_movie(&rig).await;
    assert!(outcome_for(&rig, id, "ratings").is_none());

    // OMDb wants the user's key and none is set: the step needs a key, and so does the source.
    let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
    let http = cv_providers::HttpClient::builder().build().unwrap();
    rig.services.actions().set_rating_sources(RatingSources {
        fetcher: RatingFetcher::new(resolver.clone()).with_omdb(Arc::new(
            cv_providers::OmdbProvider::new(http, resolver.clone()),
        )),
        dataset: None,
        scrapers: Vec::new(),
    });
    settings
        .set("ratings.sources-movie", json!("omdb"))
        .unwrap();
    rig.services.actions().fetch_ratings(&[id]).await;
    let step = outcome_for(&rig, id, "ratings").expect("the step is recorded");
    assert_eq!(step.status, Recorded::NeedsKey, "{step:?}");
    assert!(step.reason.contains("API key"), "{}", step.reason);
    let source = outcome_for(&rig, id, "ratings:omdb").expect("the source is recorded");
    assert_eq!(source.status, Recorded::NeedsKey, "{source:?}");

    // A source that does not know the title is "not found", not a silent skip.
    rig.services.actions().set_rating_sources(RatingSources {
        fetcher: RatingFetcher::new(resolver.clone()).with_dataset(Arc::new(Fixed {
            id: "imdbdataset",
            ratings: Vec::new(),
        })),
        dataset: None,
        scrapers: Vec::new(),
    });
    settings
        .set("ratings.sources-movie", json!("imdb-dataset"))
        .unwrap();
    rig.services.actions().fetch_ratings(&[id]).await;
    let step = outcome_for(&rig, id, "ratings").unwrap();
    assert_eq!(step.status, Recorded::NotFound, "{step:?}");
    let source = outcome_for(&rig, id, "ratings:imdb-dataset").unwrap();
    assert_eq!(source.status, Recorded::NotFound, "{source:?}");
    assert!(
        outcome_for(&rig, id, "ratings:omdb").is_some(),
        "older rows stay"
    );

    // Once a source answers, the step is done and the rating is stored.
    rig.services
        .actions()
        .set_rating_sources(rating_sources(&resolver));
    rig.services.actions().fetch_ratings(&[id]).await;
    let step = outcome_for(&rig, id, "ratings").unwrap();
    assert_eq!(step.status, Recorded::Done, "{step:?}");
    assert!(step.at_ms > 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_after_scrape_extras_leave_an_outcome_each() {
    use cv_db::repo::step_outcome::StepStatus as Recorded;

    let rig = Rig::with_collection();
    let (id, _dir) = library_with_one_movie(&rig).await;

    let done = wait_until(|| {
        ["artwork", "ratings", "collections"]
            .iter()
            .all(|step| outcome_for(&rig, id, step).is_some())
    })
    .await;
    assert!(done, "{:?}", outcomes_of(&rig, id));
    let status = |step: &str| outcome_for(&rig, id, step).unwrap().status;
    assert_eq!(status("artwork"), Recorded::Done);
    assert_eq!(status("ratings"), Recorded::Done);
    // The scripted provider knows no collection for the movie.
    assert_eq!(status("collections"), Recorded::NotFound);
}

// ---- what changed, announced to open pages -------------------

use cv_app::changes::{ChangePart, ItemsChanged};

/// Every change the services announce from now on.
fn record_changes(rig: &Rig) -> Arc<std::sync::Mutex<Vec<ItemsChanged>>> {
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);
    rig.services.changes().subscribe(Arc::new(move |change| {
        sink.lock().unwrap().push(change.clone());
    }));
    seen
}

/// The part sets announced for `id`, in the order they came.
fn parts_for(seen: &std::sync::Mutex<Vec<ItemsChanged>>, id: ItemId) -> Vec<Vec<ChangePart>> {
    seen.lock()
        .unwrap()
        .iter()
        .filter(|change| change.items.contains(&id))
        .map(|change| change.parts.clone())
        .collect()
}

fn announced(seen: &std::sync::Mutex<Vec<ItemsChanged>>, id: ItemId, parts: &[ChangePart]) -> bool {
    let mut wanted = parts.to_vec();
    wanted.sort_unstable();
    parts_for(seen, id).contains(&wanted)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_scrape_the_media_info_and_the_steps_after_it_announce_the_parts_they_changed() {
    let rig = Rig::new();
    let seen = record_changes(&rig);
    let (id, _dir) = library_with_one_movie(&rig).await;

    // The scrape wrote the fields, the provider's ratings and the stage.
    assert!(announced(
        &seen,
        id,
        &[ChangePart::Details, ChangePart::Ratings, ChangePart::Stage]
    ));
    // Media info stored the video's facts.
    assert!(announced(
        &seen,
        id,
        &[ChangePart::Files, ChangePart::Details]
    ));
    // The steps after the scrape: the poster, and the ratings from their sources.
    let landed = wait_until(|| {
        announced(&seen, id, &[ChangePart::Artwork, ChangePart::Files])
            && announced(&seen, id, &[ChangePart::Ratings, ChangePart::Stage])
    })
    .await;
    assert!(landed, "{:?}", parts_for(&seen, id));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_ratings_fetch_announces_its_start_as_a_stage_change_and_its_end_as_ratings() {
    let rig = Rig::new();
    let settings = rig.services.settings();
    settings
        .set("ratings.fetch-after-scrape-movie", json!(false))
        .unwrap();
    rig.services.artwork().set_after_scrape_runs(false);
    let (id, _dir) = library_with_one_movie(&rig).await;
    let seen = record_changes(&rig);

    rig.services.actions().fetch_ratings(&[id]).await;
    // The NFO is written again with the new ratings (announced as Files, unless a background
    // write got there first), and the title's names are checked again in the background (one
    // more Stage). When those finish is up to the machine, and announcements of one burst leave
    // in an order of their own, so check what was announced, not their order, and that the start
    // came first.
    let arrived =
        wait_until(|| parts_for(&seen, id).contains(&vec![ChangePart::Ratings, ChangePart::Stage]))
            .await;
    let parts = parts_for(&seen, id);
    assert!(arrived, "{parts:?}");
    assert_eq!(parts[0], vec![ChangePart::Stage], "the start comes first");
    for part in &parts {
        assert!(
            [
                vec![ChangePart::Stage],
                vec![ChangePart::Files],
                vec![ChangePart::Ratings, ChangePart::Stage],
            ]
            .contains(part),
            "{parts:?}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_rename_and_its_undo_announce_files_and_stage() {
    let rig = Rig::new();
    rig.services
        .settings()
        .set("ratings.fetch-after-scrape-movie", json!(false))
        .unwrap();
    let (id, _dir) = library_with_one_movie(&rig).await;
    let seen = record_changes(&rig);

    let report = rig
        .run(Rig::plan(
            EntityKind::Movie,
            false,
            &[(Step::Rename, Some(Scope::All))],
        ))
        .await;
    assert!(report.succeeded(), "{report:?}");
    assert!(announced(
        &seen,
        id,
        &[ChangePart::Files, ChangePart::Stage]
    ));
    seen.lock().unwrap().clear();

    // The undo goes through an Organize service with the same hook the app gives every one.
    let organize = OrganizeService::new(Arc::clone(rig.services.db()))
        .with_changed_hook(rig.services.organize_state().hook());
    let batches = organize.journal().unwrap();
    let outcome = organize.undo(&batches[0].batch_id).unwrap();
    assert_eq!(outcome.restored, 1, "{outcome:?}");
    assert!(announced(
        &seen,
        id,
        &[ChangePart::Files, ChangePart::Stage]
    ));
}

// ---- ratings: chips, the dataset notice and the show default -----------------------

/// A rating source that always fails, as an IMDb dataset that cannot be downloaded does.
struct Broken;

#[async_trait]
impl Provider for Broken {
    fn id(&self) -> &'static str {
        "imdbdataset"
    }
    fn name(&self) -> &'static str {
        "IMDb ratings dataset"
    }
}

#[async_trait]
impl RatingProvider for Broken {
    async fn ratings(
        &self,
        _kind: MediaKind,
        _ids: &ProviderIds,
        _options: &RequestOptions,
    ) -> Result<Ratings, ProviderError> {
        Err(ProviderError::Request {
            provider: "IMDb ratings dataset".to_owned(),
            message: "the download failed".to_owned(),
        })
    }
}

/// Takes every fetched rating and the outcomes of the ratings step off the title, as if none had
/// been fetched yet.
fn forget_ratings(rig: &Rig, id: ItemId) {
    rig.services
        .db()
        .write(move |tx| {
            let mut item = get_item(tx, id)?.unwrap();
            item.core.ratings = Ratings::default();
            update_item(tx, &item)?;
            for outcome in cv_db::repo::step_outcome::step_outcomes(tx, id)? {
                if outcome.step.starts_with("ratings") {
                    cv_db::repo::step_outcome::clear_step_outcome(tx, id, &outcome.step)?;
                }
            }
            Ok(())
        })
        .unwrap();
}

fn chips_of(rig: &Rig, id: ItemId) -> Vec<cv_app::rating_status::RatingChip> {
    let item = rig
        .services
        .db()
        .read(move |c| get_item(c, id))
        .unwrap()
        .unwrap();
    rig.services
        .rating_status()
        .chips(id, item.kind(), &item.core.ratings)
        .unwrap()
}

fn chip_state(chips: &[cv_app::rating_status::RatingChip], key: &str) -> &'static str {
    chips
        .iter()
        .find(|c| c.key == key)
        .unwrap_or_else(|| panic!("no chip for {key}"))
        .state
        .as_str()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn omdb_without_a_key_shows_needs_key_on_the_titles_chips() {
    let rig = Rig::new();
    let (id, _dir) = library_with_one_movie(&rig).await;
    let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
    let http = cv_providers::HttpClient::builder().build().unwrap();
    rig.services.actions().set_rating_sources(RatingSources {
        fetcher: RatingFetcher::new(resolver.clone())
            .with_omdb(Arc::new(cv_providers::OmdbProvider::new(http, resolver))),
        dataset: None,
        scrapers: Vec::new(),
    });
    rig.services
        .settings()
        .set("ratings.sources-movie", json!("omdb"))
        .unwrap();
    forget_ratings(&rig, id);
    rig.services.actions().fetch_ratings(&[id]).await;

    let chips = chips_of(&rig, id);

    // The step outcome says OMDb needs a key; IMDb, Rotten Tomatoes and Metacritic come from it.
    for key in ["imdb", "tomatometerallcritics", "metacritic"] {
        assert_eq!(chip_state(&chips, key), "needs_key", "{key}");
    }
    let imdb = chips.iter().find(|c| c.key == "imdb").unwrap();
    assert_eq!(imdb.provider.as_deref(), Some("omdb"));
    // Trakt is a service the user has not connected.
    assert_eq!(chip_state(&chips, "trakt"), "connect_service");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_dataset_download_raises_one_notice_with_retry_and_the_titles_show_failed() {
    let rig = Rig::new();
    let (id, _dir) = library_with_one_movie(&rig).await;
    // Give the title an IMDb ID so the dataset is asked for it.
    rig.services
        .db()
        .write(move |tx| {
            let mut item = get_item(tx, id)?.unwrap();
            item.core.ids.set("imdb", "tt0113277");
            update_item(tx, &item)
        })
        .unwrap();
    forget_ratings(&rig, id);
    let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
    rig.services.actions().set_rating_sources(RatingSources {
        fetcher: RatingFetcher::new(resolver).with_dataset(Arc::new(Broken)),
        dataset: None,
        scrapers: Vec::new(),
    });
    rig.services
        .settings()
        .set("ratings.sources-movie", json!("imdb-dataset"))
        .unwrap();
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    {
        let seen = Arc::clone(&seen);
        rig.services.notices().subscribe(move |notice| {
            seen.lock().unwrap().push(notice.clone());
            true
        });
    }

    let first = rig.services.actions().fetch_ratings(&[id]).await;
    // Asking again does not repeat the notice.
    rig.services.actions().fetch_ratings(&[id]).await;

    assert_eq!(first.items[0].kind, OutcomeKind::Skipped, "{first:?}");
    let notices: Vec<_> = seen
        .lock()
        .unwrap()
        .iter()
        .filter(|n| n.title.contains("dataset download failed"))
        .cloned()
        .collect();
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert_eq!(
        notices[0].action,
        Some(cv_app::notices::NoticeAction::RetryImdbDataset)
    );
    assert!(
        notices[0]
            .detail
            .as_deref()
            .is_some_and(|d| d.contains("the download failed"))
    );
    // The title shows IMDb as failed, and the retry says plainly it has no dataset to retry here.
    let chips = chips_of(&rig, id);
    assert_eq!(chip_state(&chips, "imdb"), "failed");
    let retry = rig.services.actions().retry_imdb_dataset().await;
    assert!(retry.unwrap_err().contains("not available"));
}

/// A rating source that fails like a provider whose server cannot be reached.
struct Unreachable(&'static str);

#[async_trait]
impl Provider for Unreachable {
    fn id(&self) -> &'static str {
        self.0
    }
    fn name(&self) -> &'static str {
        self.0
    }
}

#[async_trait]
impl RatingProvider for Unreachable {
    async fn ratings(
        &self,
        _kind: MediaKind,
        _ids: &ProviderIds,
        _options: &RequestOptions,
    ) -> Result<Ratings, ProviderError> {
        Err(ProviderError::Request {
            provider: self.0.to_owned(),
            message: "the server did not answer".to_owned(),
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn with_the_default_settings_omdb_and_mdblist_are_asked_and_their_ratings_shown() {
    // A reported case: OMDb and MDBList keys set, the rating sources never touched. The fakes
    // stand in for the two keyed providers.
    let rig = Rig::new();
    let (id, dir) = library_with_one_movie(&rig).await;
    forget_ratings(&rig, id);
    let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
    rig.services.actions().set_rating_sources(RatingSources {
        fetcher: RatingFetcher::new(resolver)
            .with_dataset(Arc::new(Fixed {
                id: "imdbdataset",
                ratings: vec![Rating::new("imdb", 8.3, 700_123, 10)],
            }))
            .with_omdb(Arc::new(Fixed {
                id: "omdb",
                ratings: vec![
                    Rating::new("tomatometerallcritics", 88.0, 0, 100),
                    Rating::new("metacritic", 76.0, 0, 100),
                ],
            }))
            .with_mdblist(Arc::new(Fixed {
                id: "mdblist",
                ratings: vec![Rating::new("letterboxd", 4.1, 640_000, 5)],
            })),
        dataset: None,
        scrapers: Vec::new(),
    });
    assert_eq!(
        rig.services.settings().get("ratings.sources-movie"),
        None,
        "nothing chosen by hand"
    );

    let run = rig.services.actions().fetch_ratings(&[id]).await;
    assert_eq!(run.items[0].kind, OutcomeKind::Done, "{run:?}");

    let chips = chips_of(&rig, id);
    for key in ["imdb", "tomatometerallcritics", "metacritic", "letterboxd"] {
        assert_eq!(chip_state(&chips, key), "value", "{key}: {chips:?}");
    }
    // Every source asked has its own truthful outcome.
    let outcomes = rig
        .services
        .db()
        .read(move |c| cv_db::repo::step_outcome::step_outcomes(c, id))
        .unwrap();
    for step in ["ratings:imdb-dataset", "ratings:omdb", "ratings:mdblist"] {
        let found = outcomes.iter().find(|o| o.step == step);
        assert!(
            found.is_some_and(|o| o.status == cv_app::step_outcomes::StepStatus::Done),
            "{step}: {outcomes:?}"
        );
    }
    // The NFO next to the video carries them too.
    let nfo = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|x| x == "nfo"))
        .expect("an NFO");
    let text = std::fs::read_to_string(nfo).unwrap();
    assert!(text.contains("letterboxd"), "{text}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failing_rating_source_raises_one_notice_even_when_others_stored_ratings() {
    let rig = Rig::new();
    let (id, _dir) = library_with_one_movie(&rig).await;
    forget_ratings(&rig, id);
    let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
    rig.services.actions().set_rating_sources(RatingSources {
        fetcher: RatingFetcher::new(resolver)
            .with_dataset(Arc::new(Fixed {
                id: "imdbdataset",
                ratings: vec![Rating::new("imdb", 8.3, 700_123, 10)],
            }))
            .with_omdb(Arc::new(Unreachable("omdb"))),
        dataset: None,
        scrapers: Vec::new(),
    });
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    {
        let seen = Arc::clone(&seen);
        rig.services.notices().subscribe(move |notice| {
            seen.lock().unwrap().push(notice.clone());
            true
        });
    }

    let first = rig.services.actions().fetch_ratings(&[id]).await;
    rig.services.actions().fetch_ratings(&[id]).await;

    // IMDb was stored, so the step is done; the OMDb failure still reaches Activity, once.
    assert_eq!(first.items[0].kind, OutcomeKind::Done, "{first:?}");
    let notices: Vec<_> = seen
        .lock()
        .unwrap()
        .iter()
        .filter(|n| n.title.starts_with("OMDb ratings"))
        .cloned()
        .collect();
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert_eq!(notices[0].provider_key.as_deref(), Some("omdb"));
    assert!(
        notices[0]
            .detail
            .as_deref()
            .is_some_and(|d| d.contains("did not answer"))
    );
    // Rotten Tomatoes only comes from OMDb here: it shows failed, and its Retry names OMDb.
    let chips = chips_of(&rig, id);
    let rt = chips
        .iter()
        .find(|c| c.key == "tomatometerallcritics")
        .unwrap();
    assert_eq!(rt.state.as_str(), "failed");
    assert_eq!(rt.provider.as_deref(), Some("omdb"));
    assert_eq!(chip_state(&chips, "imdb"), "value");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_ratings_chips_say_fetching_while_the_fetch_runs() {
    let rig = Rig::new();
    let (id, _dir) = library_with_one_movie(&rig).await;
    forget_ratings(&rig, id);
    let working = Arc::clone(rig.services.lifecycle().working());

    assert_ne!(chip_state(&chips_of(&rig, id), "imdb"), "fetching");
    let hold = working.hold_ratings(id);
    assert_eq!(chip_state(&chips_of(&rig, id), "imdb"), "fetching");
    drop(hold);
    assert_ne!(chip_state(&chips_of(&rig, id), "imdb"), "fetching");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_new_show_gets_its_ratings_after_the_scrape_with_the_default_settings() {
    let rig = Rig::new();
    // Nothing is set: the default fetches ratings after a show scrape, like for a movie.
    let after = cv_app::actions::AfterScrape::from_values(&Default::default());
    assert!(after.ratings_show && after.ratings_movie);
    let values = rig.services.settings().snapshot().values;
    assert_eq!(
        values.get("ratings.fetch-after-scrape-show"),
        None,
        "the default is not a stored override"
    );
    assert!(
        cv_app::ratings::rating_settings_from_values(&values)
            .show
            .after_scrape
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn storing_ratings_announces_the_ratings_of_that_title() {
    let rig = Rig::new();
    let (id, _dir) = library_with_one_movie(&rig).await;
    forget_ratings(&rig, id);
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    {
        let seen = Arc::clone(&seen);
        rig.services.changes().subscribe(Arc::new(
            move |change: &cv_app::changes::ItemsChanged| {
                seen.lock().unwrap().push(change.clone());
            },
        ));
    }

    rig.services.actions().fetch_ratings(&[id]).await;

    let seen = seen.lock().unwrap();
    assert!(
        seen.iter().any(|change| {
            change.items.contains(&id)
                && change.parts.contains(&cv_app::changes::ChangePart::Ratings)
        }),
        "{seen:?}"
    );
}
