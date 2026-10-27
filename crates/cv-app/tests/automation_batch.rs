//! The batch engine behind the `cinevault` command line: the steps run in their fixed
//! order, selection narrows them, a dry run changes nothing, and the report carries per-title
//! results. Real folders, a real library, a scripted metadata provider and no network.

mod support;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use cv_app::automation::{
    BatchEngine, BatchReport, BatchRequest, EngineOptions, EntityKind, Scope, Step, StepReport,
    StepStatus,
};
use cv_app::data_location::{DataLocation, LocationInputs};
use cv_app::media_info::{MediaReader, ReaderSource};
use cv_app::provider_credentials::ProviderCredentialsService;
use cv_app::secrets::SecretStore;
use cv_app::services::{AppServices, BootOptions};
use cv_db::repo::data_sources::insert_source;
use cv_db::repo::{ItemQuery, list_items};
use cv_domain::{ItemKind, SourceKind};
use cv_media::mediainfo::{MediaInfoError, MediaSnapshot, StreamInfo, StreamKind};
use cv_providers::{
    CredentialResolver, HttpClient, MemoryUserCredentials, ProviderRegistry, TmdbProvider,
};
use support::{Entry, Scripted, arc};
use tempfile::TempDir;

fn location(root: &Path) -> DataLocation {
    let inputs = LocationInputs {
        exe_dir: root.join("app"),
        user_data_base: root.join("userdata"),
        user_config_base: root.join("userconfig"),
    };
    std::fs::create_dir_all(&inputs.exe_dir).unwrap();
    DataLocation::resolve(&inputs)
}

fn catalogue() -> Scripted {
    Scripted::new(
        "scripted",
        vec![
            Entry::new("1", "Alien", 1979),
            Entry::new("2", "Heat", 1995),
            Entry::new("3", "Arrival", 2016),
        ],
    )
}

fn providers() -> ProviderCredentialsService {
    let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
    let mut registry = ProviderRegistry::new();
    registry.register_metadata(arc(catalogue()));
    ProviderCredentialsService::with_resolver(resolver, registry)
}

struct FakeReader(Arc<AtomicUsize>);

impl MediaReader for FakeReader {
    fn read(&self, _path: &Path) -> Result<MediaSnapshot, MediaInfoError> {
        self.0.fetch_add(1, Ordering::SeqCst);
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

fn fake_source(reads: &Arc<AtomicUsize>) -> ReaderSource {
    let reads = Arc::clone(reads);
    Arc::new(move || Ok(Arc::new(FakeReader(Arc::clone(&reads))) as Arc<dyn MediaReader>))
}

/// A library folder tree, the app services over it, and the engine.
struct Rig {
    temp: TempDir,
    services: Arc<AppServices>,
    engine: Arc<BatchEngine>,
    reads: Arc<AtomicUsize>,
}

impl Rig {
    fn new() -> Self {
        Self::with(providers())
    }

    fn with(providers: ProviderCredentialsService) -> Self {
        let temp = TempDir::new().unwrap();
        // The steps are checked on files nothing else rewrites: the NFO follower and the write
        // after media info would race the NFO step.
        let options = BootOptions::new(location(temp.path()), Arc::new(SecretStore::in_memory()))
            .without_background_nfo()
            .with_providers(providers);
        let services = AppServices::boot(options).expect("boot");
        let reads = Arc::new(AtomicUsize::new(0));
        services.scan().set_media_reader(fake_source(&reads));
        // The command line matches explicitly; no titles are matched behind its back.
        services.scan().set_after_scan(Arc::new(|_| {}));
        services
            .settings()
            .set("media.runtime-from-media", true.into())
            .unwrap();
        let engine = BatchEngine::new(Arc::clone(&services), EngineOptions::default());
        Self {
            temp,
            services,
            engine,
            reads,
        }
    }

    fn root(&self, name: &str) -> PathBuf {
        self.temp.path().join(name)
    }

    /// A movie folder with one video file, as a download would leave it.
    fn movie(&self, source: &str, folder: &str) {
        let dir = self.root(source).join(folder);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{folder}.mkv")), folder).unwrap();
    }

    fn add_source(&self, name: &str) -> i64 {
        let path = self.root(name);
        std::fs::create_dir_all(&path).unwrap();
        let text = path.to_string_lossy().into_owned();
        self.services
            .db()
            .write(move |tx| insert_source(tx, SourceKind::Movie, &text))
            .unwrap()
            .id
    }

    async fn run(&self, request: BatchRequest) -> BatchReport {
        self.engine.run(&request, &|_| {}).await
    }

    fn folders(&self, source: &str) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(self.root(source))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// Every file under `source` with its size, sorted.
    fn files(&self, source: &str) -> Vec<(PathBuf, u64)> {
        fn walk(dir: &Path, into: &mut Vec<(PathBuf, u64)>) {
            for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, into);
                } else {
                    let size = entry.metadata().map_or(0, |m| m.len());
                    into.push((path, size));
                }
            }
        }
        let mut files = Vec::new();
        walk(&self.root(source), &mut files);
        files.sort();
        files
    }

    fn titles(&self) -> Vec<String> {
        let mut titles: Vec<String> = self
            .services
            .db()
            .read(|c| list_items(c, &ItemQuery::new(ItemKind::Movie), None, 100))
            .unwrap()
            .items
            .into_iter()
            .map(|i| i.title)
            .collect();
        titles.sort();
        titles
    }
}

fn request(steps: impl FnOnce(&mut cv_app::automation::Steps)) -> BatchRequest {
    let mut request = BatchRequest::new(EntityKind::Movie);
    steps(&mut request.steps);
    request
}

fn step(report: &BatchReport, which: Step) -> &StepReport {
    report
        .steps
        .iter()
        .find(|s| s.step == which)
        .unwrap_or_else(|| panic!("no {which} step in {:?}", report.steps))
}

fn assert_ok(report: &BatchReport) {
    assert!(report.error.is_none(), "{:?}", report.error);
    for step in &report.steps {
        assert!(
            !matches!(step.status, StepStatus::Failed | StepStatus::Unavailable),
            "{step:?}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_nightly_script_finds_scrapes_and_renames_new_movies() {
    let rig = Rig::new();
    rig.add_source("movies");
    rig.movie("movies", "alien.1979.1080p.bluray");
    rig.movie("movies", "heat.1995.720p");

    // `cinevault movie --update-all --scrape-new --rename-new`
    let report = rig
        .run(request(|s| {
            s.update = true;
            s.scrape = Some(Scope::New);
            s.rename = Some(Scope::New);
        }))
        .await;
    assert_ok(&report);
    assert_eq!(report.exit_code(), 0);
    let names: Vec<_> = report.steps.iter().map(|s| s.step.name()).collect();
    assert_eq!(names, ["update", "scrape", "rename"]);

    assert_eq!(step(&report, Step::Update).counts["new_or_changed"], 2);
    let scrape = step(&report, Step::Scrape);
    assert_eq!(scrape.counts["matched"], 2, "{scrape:?}");
    assert!(scrape.items.iter().all(|i| i.status == "matched"));
    let rename = step(&report, Step::Rename);
    assert_eq!(rename.counts["renamed"], 2, "{rename:?}");
    assert!(rename.undo.is_some());
    assert!(rename.items.iter().all(|i| i.status == "renamed"));

    // The folders on disk now carry the scraped names.
    let folders = rig.folders("movies");
    assert_eq!(folders.len(), 2, "{folders:?}");
    assert!(
        folders.iter().any(|f| f.starts_with("Alien")),
        "{folders:?}"
    );
    assert!(folders.iter().any(|f| f.starts_with("Heat")), "{folders:?}");
    assert_eq!(rig.titles(), ["Alien", "Heat"]);
}

/// Review 14.1: a rename step never moves a folder while a scrape's NFO or artwork is still being
/// written into it. After its bounded wait it renames nothing and says why; once the writes are
/// over, the same step renames.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_rename_step_waits_for_the_scrape_writes_and_renames_nothing_while_they_last() {
    let rig = Rig::new();
    rig.add_source("movies");
    rig.movie("movies", "heat.1995.720p");
    let scraped = rig
        .run(request(|s| {
            s.update = true;
            s.scrape = Some(Scope::New);
        }))
        .await;
    assert_ok(&scraped);
    let before = rig.folders("movies");

    let engine = BatchEngine::new(
        Arc::clone(&rig.services),
        EngineOptions {
            follow_up_wait: Some(std::time::Duration::from_millis(300)),
            ..EngineOptions::default()
        },
    );
    let rename = || request(|s| s.rename = Some(Scope::All));
    let writing = rig.services.scrape_follow_up().hold();
    let report = engine.run(&rename(), &|_| {}).await;
    let step_report = step(&report, Step::Rename);
    assert_eq!(step_report.status, StepStatus::Failed, "{step_report:?}");
    assert!(
        step_report.message.contains("still being written"),
        "{step_report:?}"
    );
    assert_eq!(rig.folders("movies"), before, "nothing moved");

    drop(writing);
    let report = engine.run(&rename(), &|_| {}).await;
    assert_ok(&report);
    assert_eq!(step(&report, Step::Rename).counts["renamed"], 1);
    assert!(
        rig.folders("movies").iter().any(|f| f.starts_with("Heat")),
        "{:?}",
        rig.folders("movies")
    );
}

/// reg3-4: without the TMDB key, identification waits for it. The scrape step is skipped with
/// where to add the key, not failed: running it again cannot help, and it is not a failure.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_scrape_without_the_tmdb_key_is_skipped_with_where_to_add_it() {
    let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
    let mut registry = ProviderRegistry::new();
    let http = HttpClient::builder().build().unwrap();
    registry.register_metadata(Arc::new(TmdbProvider::new(http, resolver.clone())));
    registry.register_metadata(arc(catalogue()));
    let rig = Rig::with(ProviderCredentialsService::with_resolver(
        resolver, registry,
    ));
    rig.add_source("movies");
    rig.movie("movies", "alien.1979.1080p");

    for dry_run in [true, false] {
        let mut asked = request(|s| {
            s.update = true;
            s.scrape = Some(Scope::All);
        });
        asked.dry_run = dry_run;
        let report = rig.run(asked).await;
        let scrape = step(&report, Step::Scrape);
        assert_eq!(scrape.status, StepStatus::Skipped, "{scrape:?}");
        assert!(
            scrape
                .message
                .contains("TMDB key in Settings, Scraping and providers"),
            "{}",
            scrape.message
        );
        assert_ok(&report);
        assert_eq!(report.exit_code(), 0);
    }
    assert_eq!(rig.titles(), ["alien"], "the title waits, unmatched");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn steps_run_in_the_fixed_order_even_when_asked_in_another_order() {
    let rig = Rig::new();
    rig.add_source("movies");
    rig.movie("movies", "alien.1979.1080p");

    let asked = request(|s| {
        // Every step, the last first.
        s.nfo = true;
        s.rename = Some(Scope::New);
        s.aspect_ratio = Some(Scope::New);
        s.media_info = true;
        s.scrape = Some(Scope::New);
        s.update = true;
    });
    let report = rig.run(asked).await;
    let names: Vec<_> = report.steps.iter().map(|s| s.step.name()).collect();
    assert_eq!(
        names,
        [
            "update",
            "scrape",
            "media-info",
            "aspect-ratio",
            "rename",
            "nfo"
        ]
    );
    // The steps that used to be placeholders now call their services. Here there is no FFmpeg,
    // and the step says so.
    assert_eq!(
        step(&report, Step::AspectRatio).status,
        StepStatus::Unavailable
    );
    assert_eq!(report.exit_code(), 1);

    // The order shows in the files: the NFO is written next to the video where the rename put it.
    let renamed = rig.folders("movies");
    assert_eq!(renamed.len(), 1);
    let nfo_dir = rig.root("movies").join(&renamed[0]);
    let nfos: Vec<_> = std::fs::read_dir(&nfo_dir)
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "nfo"))
        .collect();
    assert!(!nfos.is_empty(), "no NFO in {nfo_dir:?}");
    // And the media info of the new title was read after the update found it.
    assert_eq!(rig.reads.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_planned_rename_writes_each_path_with_one_separator() {
    let rig = Rig::new();
    rig.add_source("movies");
    // A movie two folders down: the library keeps `Collection/Heat 1995` relative to the root.
    let nested = rig.root("movies").join("Collection").join("heat.1995.720p");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(nested.join("heat.1995.720p.mkv"), "heat").unwrap();
    assert_ok(&rig.run(request(|s| s.update = true)).await);

    let mut dry = request(|s| s.rename = Some(Scope::All));
    dry.dry_run = true;
    let report = rig.run(dry).await;
    assert_ok(&report);
    let rename = step(&report, Step::Rename);
    let item = rename.items.first().expect("the movie is planned");
    let from = item.from.as_deref().unwrap();
    let to = item.to.as_deref().unwrap();
    let other = if cfg!(windows) { '/' } else { '\\' };
    assert!(!from.contains(other), "{from}");
    assert!(!to.contains(other), "{to}");
    assert_eq!(std::path::Path::new(from), nested.as_path());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_dry_run_reports_the_plan_and_changes_nothing() {
    let rig = Rig::new();
    rig.add_source("movies");
    rig.movie("movies", "alien.1979.1080p");
    rig.movie("movies", "heat.1995.720p");
    // Find and scrape for real; the rename is what is planned.
    assert_ok(
        &rig.run(request(|s| {
            s.update = true;
            s.scrape = Some(Scope::New);
        }))
        .await,
    );
    let before = rig.folders("movies");
    assert_eq!(before, ["alien.1979.1080p", "heat.1995.720p"]);
    // The scrape wrote each title's NFO next to its video (the follow-up of any scrape); the dry
    // run must leave exactly these files as they are.
    let files_before = rig.files("movies");
    assert_eq!(files_before.len(), 4, "{files_before:?}");
    assert_eq!(walk_nfo_count(&rig.root("movies")), 2);

    let mut dry = request(|s| {
        s.rename = Some(Scope::All);
        s.nfo = true;
        s.scrape = Some(Scope::All);
        s.update = true;
    });
    dry.dry_run = true;
    let report = rig.run(dry).await;
    assert_ok(&report);
    assert!(report.dry_run);
    for planned in &report.steps {
        assert_eq!(planned.status, StepStatus::Planned, "{planned:?}");
    }

    let rename = step(&report, Step::Rename);
    assert_eq!(rename.counts["to_rename"], 2);
    assert_eq!(rename.items.len(), 2);
    for item in &rename.items {
        assert_eq!(item.status, "rename");
        let from = item.from.as_deref().unwrap();
        let to = item.to.as_deref().unwrap();
        assert_ne!(from, to);
        assert!(from.contains("1979") || from.contains("1995"), "{from}");
        assert!(
            to.contains(&format!(
                "({})",
                if from.contains("1979") { 1979 } else { 1995 }
            )),
            "{to}"
        );
    }
    assert!(rename.undo.is_none());

    // Nothing moved, nothing was written, no job was started.
    assert_eq!(rig.folders("movies"), before);
    assert_eq!(rig.files("movies"), files_before);
    assert!(
        rig.services
            .jobs()
            .list()
            .iter()
            .all(|j| !j.name.starts_with("Rescan") || j.state.is_terminal())
    );
    assert_eq!(rig.titles(), ["Alien", "Heat"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn selection_by_source_path_and_query_narrows_every_step() {
    let rig = Rig::new();
    rig.add_source("first");
    rig.add_source("second");
    rig.movie("first", "alien.1979.1080p");
    rig.movie("first", "heat.1995.720p");
    rig.movie("second", "arrival.2016.2160p");
    assert_ok(
        &rig.run(request(|s| {
            s.update = true;
            s.scrape = Some(Scope::New);
        }))
        .await,
    );
    assert_eq!(rig.titles(), ["Alien", "Arrival", "Heat"]);

    let planned = |select: &dyn Fn(&mut BatchRequest)| {
        let mut asked = request(|s| s.rename = Some(Scope::All));
        asked.dry_run = true;
        select(&mut asked);
        let rig = &rig;
        async move {
            let report = rig.run(asked).await;
            assert!(report.error.is_none(), "{:?}", report.error);
            let mut titles: Vec<String> = step(&report, Step::Rename)
                .items
                .iter()
                .map(|i| i.title.clone())
                .collect();
            titles.sort();
            (titles, report.selection)
        }
    };

    // By data source: its position, or its folder.
    let (titles, summary) = planned(&|r| r.selection.sources = vec!["2".to_owned()]).await;
    assert_eq!(titles, ["Arrival"]);
    assert!(summary.narrowed && summary.titles == 1);
    let second = rig.root("second").to_string_lossy().into_owned();
    let (titles, _) = planned(&|r| r.selection.sources = vec![second.clone()]).await;
    assert_eq!(titles, ["Arrival"]);

    // By folder.
    let heat_dir = rig.root("first").join("heat.1995.720p");
    let (titles, _) =
        planned(&|r| r.selection.paths = vec![heat_dir.to_string_lossy().into_owned()]).await;
    assert_eq!(titles, ["Heat"]);
    let first = rig.root("first").to_string_lossy().into_owned();
    let (titles, _) = planned(&|r| r.selection.paths = vec![first.clone()]).await;
    assert_eq!(titles, ["Alien", "Heat"]);

    // By quick-search query, with the grammar of the search box.
    let (titles, _) = planned(&|r| r.selection.query = Some("year:1990..2010".to_owned())).await;
    assert_eq!(titles, ["Heat"]);
    let (titles, _) = planned(&|r| r.selection.query = Some("-year:1990..2010".to_owned())).await;
    assert_eq!(titles, ["Alien", "Arrival"]);

    // The selectors narrow each other.
    let (titles, _) = planned(&|r| {
        r.selection.sources = vec!["1".to_owned()];
        r.selection.query = Some("year:1990..".to_owned());
    })
    .await;
    assert_eq!(titles, ["Heat"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_selection_that_cannot_be_made_stops_the_batch_before_it_starts() {
    let rig = Rig::new();
    rig.add_source("first");
    rig.movie("first", "alien.1979.1080p");
    for (what, select) in [
        ("year:abc", "query"),
        ("7", "source"),
        ("relative/dir", "path"),
        ("C:\\not\\a\\source", "path"),
    ] {
        let mut asked = request(|s| s.rename = Some(Scope::All));
        asked.dry_run = true;
        match select {
            "query" => asked.selection.query = Some(what.to_owned()),
            "source" => asked.selection.sources = vec![what.to_owned()],
            _ => asked.selection.paths = vec![what.to_owned()],
        }
        let report = rig.run(asked).await;
        assert!(report.error.is_some(), "{select} {what}: {report:?}");
        assert!(report.steps.is_empty());
        assert_eq!(report.exit_code(), 2);
    }
    let none = rig.run(request(|_| {})).await;
    assert!(none.error.as_deref().unwrap().contains("No step"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn media_info_and_nfo_steps_report_per_title_results() {
    let rig = Rig::new();
    rig.add_source("movies");
    rig.movie("movies", "alien.1979.1080p");
    assert_ok(
        &rig.run(request(|s| {
            s.update = true;
            s.scrape = Some(Scope::New);
        }))
        .await,
    );
    // The scrape itself wrote the NFO (the follow-up of any scrape, by the NFO settings). Take it
    // away so the NFO step has a file to write.
    let written = nfo_files(&rig.root("movies"));
    assert_eq!(written.len(), 1, "{written:?}");
    std::fs::remove_file(&written[0]).unwrap();
    let report = rig
        .run(request(|s| {
            s.media_info = true;
            s.nfo = true;
        }))
        .await;
    assert_ok(&report);
    let nfo = step(&report, Step::Nfo);
    assert_eq!(nfo.counts["changed"], 1, "{nfo:?}");
    assert_eq!(nfo.items[0].status, "written");
    assert!(nfo.undo.is_some());
    // Run again: nothing to write, nothing touched.
    let again = rig.run(request(|s| s.nfo = true)).await;
    assert_eq!(step(&again, Step::Nfo).counts["unchanged"], 1);
}

fn nfo_files(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(nfo_files(&path));
        } else if path.extension().is_some_and(|x| x == "nfo") {
            found.push(path);
        }
    }
    found
}

fn walk_nfo_count(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| {
            let path = entry.path();
            if path.is_dir() {
                walk_nfo_count(&path)
            } else {
                usize::from(path.extension().is_some_and(|x| x == "nfo"))
            }
        })
        .sum()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failed_step_stops_the_steps_after_it() {
    let rig = Rig::new();
    rig.add_source("movies");
    rig.movie("movies", "heat.1995.720p");
    assert_ok(
        &rig.run(request(|s| {
            s.update = true;
            s.scrape = Some(Scope::New);
        }))
        .await,
    );
    let before = rig.folders("movies");
    // The rename waits for the NFO and artwork of a scrape, and fails when they do not finish.
    let engine = BatchEngine::new(
        Arc::clone(&rig.services),
        EngineOptions {
            follow_up_wait: Some(std::time::Duration::from_millis(300)),
            ..EngineOptions::default()
        },
    );
    let writing = rig.services.scrape_follow_up().hold();
    let report = engine
        .run(
            &request(|s| {
                s.rename = Some(Scope::All);
                s.nfo = true;
            }),
            &|_| {},
        )
        .await;
    drop(writing);
    assert_eq!(step(&report, Step::Rename).status, StepStatus::Failed);
    let nfo = step(&report, Step::Nfo);
    assert_eq!(nfo.status, StepStatus::Skipped);
    assert!(nfo.message.contains("rename"), "{}", nfo.message);
    // The rename moved nothing.
    assert_eq!(rig.folders("movies"), before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_tv_batch_works_on_shows_and_a_new_episode_counts_for_its_show() {
    let rig = Rig::new();
    let shows = rig.root("shows");
    let episode = |name: &str, file: &str| {
        let dir = shows.join(name).join("Season 1");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(file), file).unwrap();
    };
    episode("Dark Matter", "Dark.Matter.S01E01.mkv");
    episode("Dark Matter", "Dark.Matter.S01E02.mkv");
    episode("Severance", "Severance.S01E01.mkv");
    let text = shows.to_string_lossy().into_owned();
    rig.services
        .db()
        .write(move |tx| insert_source(tx, SourceKind::Tv, &text))
        .unwrap();
    // A movie source must not be rescanned by a TV batch.
    rig.add_source("movies");
    rig.movie("movies", "heat.1995.720p");

    let mut asked = BatchRequest::new(EntityKind::Tvshow);
    asked.steps.update = true;
    asked.steps.rename = Some(Scope::New);
    asked.steps.nfo = true;
    asked.dry_run = false;
    let report = rig.run(asked).await;
    assert_ok(&report);
    // Three episodes were found, in two shows: the batch counts shows.
    assert_eq!(step(&report, Step::Update).counts["new_or_changed"], 2);
    assert_eq!(report.selection.titles, 2);
    assert_eq!(report.selection.sources.len(), 1);
    let nfo = step(&report, Step::Nfo);
    // Each show brings its seasons and episodes: 2 shows, 2 seasons, 3 episodes.
    assert_eq!(nfo.items.len(), 7, "{nfo:?}");
    for show in ["Dark Matter", "Severance"] {
        assert!(nfo.items.iter().any(|i| i.title == show), "{show} missing");
    }
    // The movie source was left alone.
    assert!(rig.titles().is_empty());
}
