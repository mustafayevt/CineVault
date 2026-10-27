//! The app services booted on a temporary data folder: one shared library, the
//! settings notice, daily backups, the thumbnail cache, persisted jobs resumed on launch, jobs
//! paused on quit, and failures routed to the notification centre.

mod support;

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cv_app::data_location::{DataLocation, LocationInputs};
use cv_app::local_library::LOCAL_IMPORT_JOB;
use cv_app::matching::batch::{BatchMode, MATCH_JOB_KIND, plan_batch};
use cv_app::notices::{AppNotice, NoticeLevel};
use cv_app::organize::STATE_JOB_NAME;
use cv_app::provider_credentials::ProviderCredentialsService;
use cv_app::secrets::SecretStore;
use cv_app::services::{AppServices, BootOptions};
use cv_artwork::thumb_job::{ThumbTarget, submit_thumb_job};
use cv_db::repo::ItemQuery;
use cv_db::repo::data_sources::insert_source;
use cv_db::repo::query::count_items;
use cv_domain::{ItemKind, SourceKind};
use cv_jobs::{ItemControl, ItemError, ItemWork, JobSpec, JobState, LaneId, StageError};
use cv_providers::{CredentialResolver, MemoryUserCredentials, ProviderRegistry};
use cv_scan::rescan::RescanOptions;
use cv_scan::rescan::job::{rescan_targets, submit_rescan};
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

fn options(root: &Path) -> BootOptions {
    // Nothing here may reach a provider's website.
    BootOptions::new(location(root), Arc::new(SecretStore::in_memory())).without_providers()
}

fn put(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

async fn until<T>(what: &str, mut check: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(found) = check() {
            return found;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

fn collect_notices(services: &AppServices) -> Arc<Mutex<Vec<AppNotice>>> {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);
    services.notices().subscribe(move |notice| {
        sink.lock().unwrap().push(notice.clone());
        true
    });
    seen
}

/// A job whose items all fail, standing in for a provider that rejects every request.
struct Failing;

impl ItemWork for Failing {
    type Item = String;

    async fn process(&self, _item: &String, _control: &ItemControl) -> Result<(), ItemError> {
        Err(StageError::new("rate limit").into())
    }
}

#[test]
fn a_persisted_rescan_and_thumbnail_build_resume_on_the_next_launch() {
    let temp = TempDir::new().unwrap();
    let media = temp.path().join("movies");
    put(&media.join("Alien (1979)/Alien (1979).mkv"), "a");
    put(&media.join("Heat (1995)/Heat (1995).mkv"), "h");

    // First launch: queue the work, then quit before any of it runs.
    {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let services = AppServices::boot(options(temp.path())).expect("first boot");
        runtime.block_on(async {
            services.jobs().pause_all();
            let path = media.to_string_lossy().into_owned();
            services
                .db()
                .write(move |tx| insert_source(tx, SourceKind::Movie, &path))
                .unwrap();
            let targets = rescan_targets(services.db()).unwrap();
            let rescan = submit_rescan(
                services.jobs(),
                Arc::clone(services.db()),
                RescanOptions::default(),
                targets,
                1,
            )
            .unwrap();
            assert!(rescan.job.stored_id().is_some(), "the job is persisted");
            let thumbs = submit_thumb_job(
                services.jobs(),
                Arc::clone(services.thumbs()),
                vec![ThumbTarget {
                    item: 1,
                    path: media.join("missing.jpg").to_string_lossy().into_owned(),
                }],
                1,
            )
            .unwrap();
            assert!(thumbs.is_some());
            services.shutdown();
        });
        drop(runtime);
    }

    // Second launch on the same data folder.
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let services = AppServices::boot(options(temp.path())).expect("second boot");
        let report = services.start();
        assert_eq!(report.rescans, 1);
        assert_eq!(report.thumbnails, 1);
        assert_eq!(report.total(), 2);
        // No provider is ready in these services, so stored match batches wait.
        assert!(report.matches_deferred);

        until("the resumed jobs to finish", || {
            // The resumed rescan also queues the read of the new titles' NFO files and the check
            // of their folder names; neither is one of the two that were stored.
            let jobs = services.jobs().list();
            let resumed = jobs
                .iter()
                .filter(|j| j.name != LOCAL_IMPORT_JOB && !j.name.starts_with(STATE_JOB_NAME))
                .collect::<Vec<_>>();
            (resumed.len() == 2 && jobs.iter().all(|j| j.state.is_terminal())).then_some(())
        })
        .await;

        // The resumed rescan filled the one shared library.
        let movies = services
            .db()
            .read(|conn| count_items(conn, &ItemQuery::new(ItemKind::Movie)))
            .unwrap();
        assert_eq!(movies, 2);
        services.shutdown();
    });
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn boot_prepares_the_data_folder_and_every_service_uses_it() {
    let temp = TempDir::new().unwrap();
    let services = AppServices::boot(options(temp.path())).expect("boot");
    let root = services.location().root().to_path_buf();
    assert!(root.join("library.db").is_file());
    assert!(root.join("cache").is_dir());
    // The thumbnail cache lives in the cache folder of the data location.
    assert!(root.join("cache").join("thumbs").is_dir());

    // A job submitted through the manager is stored in the library the app reads.
    let job = services.jobs().submit_persistent(
        "test.kind",
        JobSpec::new("Probe", LaneId::Cpu),
        Failing,
        vec!["one".to_owned()],
    );
    let job = job.expect("a store is attached");
    job.wait().await;
    let row = job.stored_id().expect("stored");
    let stored = services
        .db()
        .read(|conn| cv_db::repo::jobs::get_job(conn, row))
        .unwrap();
    assert!(stored.is_some());

    services.shutdown();
}

#[test]
fn a_damaged_settings_file_is_set_aside_and_announced_once() {
    let temp = TempDir::new().unwrap();
    let loc = location(temp.path());
    std::fs::create_dir_all(loc.root()).unwrap();
    std::fs::write(loc.settings_file(), "{ this is not json").unwrap();

    let services = AppServices::boot(options(temp.path())).expect("boot");
    let seen = collect_notices(&services);
    {
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1, "{seen:?}");
        assert_eq!(seen[0].level, NoticeLevel::Warning);
        assert_eq!(seen[0].source.as_deref(), Some("Settings"));
        assert!(seen[0].detail.is_some());
    }
    // The damaged file is kept beside the settings, not deleted.
    let kept = std::fs::read_dir(loc.root())
        .unwrap()
        .filter_map(Result::ok)
        .any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("settings.json.corrupt-")
        });
    assert!(kept, "the damaged settings file is set aside");
    // The settings page does not announce it a second time.
    assert!(services.settings().snapshot().notice.is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn job_failures_reach_the_notification_centre() {
    let temp = TempDir::new().unwrap();
    let services = AppServices::boot(options(temp.path())).expect("boot");
    let seen = collect_notices(&services);
    services.start();

    let job = services.jobs().submit(
        JobSpec::new("Scraping 3 titles", LaneId::provider("tmdb")),
        Failing,
        vec!["a".to_owned(), "b".to_owned()],
    );
    let report = job.wait().await;
    assert_ne!(report.snapshot.state, JobState::Cancelled);

    let notice = until("the failure notice", || {
        seen.lock().unwrap().first().cloned()
    })
    .await;
    assert!(notice.title.starts_with("Scraping 3 titles"), "{notice:?}");
    assert_eq!(notice.source.as_deref(), Some("tmdb"));
    assert!(
        notice
            .detail
            .as_deref()
            .unwrap_or_default()
            .contains("rate limit")
    );
    // One finished job gives one notice.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(seen.lock().unwrap().len(), 1);
    services.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn quitting_pauses_the_jobs() {
    let temp = TempDir::new().unwrap();
    let services = AppServices::boot(options(temp.path())).expect("boot");
    services.start();
    assert!(!services.jobs().is_all_paused());
    services.shutdown();
    assert!(services.jobs().is_all_paused());
    // A second call is harmless.
    services.shutdown();
}

/// Boot options whose only provider is a scripted one that knows "Alien".
fn options_with_scripted_provider(root: &Path) -> BootOptions {
    let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
    let mut registry = ProviderRegistry::new();
    registry.register_metadata(support::arc(support::Scripted::new(
        "scripted",
        vec![support::Entry::new("1", "Alien", 1979)],
    )));
    BootOptions::new(location(root), Arc::new(SecretStore::in_memory())).with_providers(
        ProviderCredentialsService::with_resolver(resolver, registry),
    )
}

#[test]
fn a_match_batch_left_by_the_last_session_resumes_when_a_provider_is_ready() {
    use cv_db::repo::jobs::{NewJob, NewJobItem, insert_job};
    use cv_db::repo::{get_item, insert_item};
    use cv_domain::Item;

    let temp = TempDir::new().unwrap();
    let mut item = Item::movie("Alien");
    item.core.year = 1979;

    // First launch: a batch that never ran is stored, as a quit leaves it.
    let id = {
        let services = AppServices::boot(options_with_scripted_provider(temp.path())).unwrap();
        let id = services
            .db()
            .write(move |tx| insert_item(tx, &item))
            .unwrap();
        let context = services.match_context();
        let plan = plan_batch(
            services.db(),
            &context.providers,
            BatchMode::MatchUnmatched,
            None,
        )
        .unwrap();
        assert_eq!(plan.targets.len(), 1);
        let items: Vec<NewJobItem> = plan
            .targets
            .iter()
            .map(|t| NewJobItem {
                label: t.to_string(),
                payload: serde_json::to_string(t).unwrap(),
            })
            .collect();
        let payload = serde_json::json!({ "concurrency": 1 }).to_string();
        services
            .db()
            .write(move |tx| {
                let job = NewJob {
                    kind: MATCH_JOB_KIND,
                    name: "Scraping 3 titles",
                    lane: "provider:scripted",
                    priority: 0,
                    payload: &payload,
                };
                insert_job(tx, &job, &items)
            })
            .unwrap();
        id
    };

    // Without a provider the batch stays stored.
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let services = AppServices::boot(options(temp.path())).expect("boot without providers");
        let report = services.start();
        assert!(report.matches_deferred);
        assert_eq!(report.matches, 0);
        services.shutdown();
    });
    drop(runtime);

    // With one, it runs and the title is matched.
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let services = AppServices::boot(options_with_scripted_provider(temp.path())).unwrap();
        let report = services.start();
        assert_eq!(report.matches, 1);
        assert!(!report.matches_deferred);
        until("the resumed match", || {
            let item = services.db().read(|c| get_item(c, id)).unwrap()?;
            (item.core.last_scraper_id == "scripted").then_some(())
        })
        .await;
        services.shutdown();
    });
    drop(runtime);
}

/// `advanced.image-cache-size` caps the thumbnail cache from the first start.
#[tokio::test]
async fn the_stored_image_cache_size_caps_the_thumbnail_cache_at_boot() {
    let temp = TempDir::new().unwrap();
    let location = location(temp.path());
    std::fs::create_dir_all(location.settings_file().parent().unwrap()).unwrap();
    cv_app::app_settings::SettingsService::open(cv_app::settings::SettingsStore::new(
        location.settings_file(),
    ))
    .set(cv_app::caches::IMAGE_CACHE_SIZE_KEY, serde_json::json!(256))
    .unwrap();

    let services = AppServices::boot(options(temp.path())).expect("boot");

    assert_eq!(services.thumbs().max_bytes(), Some(256 * 1024 * 1024));
    // A change after boot is applied through the cache service.
    services
        .settings()
        .set(cv_app::caches::IMAGE_CACHE_SIZE_KEY, serde_json::json!(512))
        .unwrap();
    services.caches().apply_settings();
    assert_eq!(services.thumbs().max_bytes(), Some(512 * 1024 * 1024));
}
