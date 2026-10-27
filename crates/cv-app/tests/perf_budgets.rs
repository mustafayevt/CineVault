//! Performance probes for the app-level budgets: cold start on 5,000 titles, and
//! input latency while every job lane is busy.
//!
//! Both are `#[ignore]`d: a wall-clock bound holds only on a quiet machine, and a test that fails
//! now and then teaches people to ignore red. Run them by hand, in the
//! release profile:
//!
//! ```text
//! cargo test --release -p cv-app --test perf_budgets -- --ignored --nocapture --test-threads=1
//! ```
//!
//! # Cold start
//!
//! `cold_start_5k` fills a data folder with a 5,000-movie library (a scan of a generated tree),
//! quits, and boots a new `AppServices` on the same folder, which is what the desktop shell does
//! before it shows a window. The clock runs from the call to `AppServices::boot` until the first
//! screen's data is in hand: `start()` returned (resume, startup scan queued), and the library's
//! first page of 120 rows, its total and the sidebar counts were read while the startup scan was
//! running. The budget is 1.5 s for that part; the webview's own start-up and the page load are
//! measured separately (`bun run perf:coldstart`).
//!
//! # Input latency under load
//!
//! `input_latency_under_load` runs the services on a four-worker runtime with the disk lane (2
//! workers) and the CPU lane (3 workers) both busy: a first-fill rescan of a second 5,000-movie
//! source writes the database while CPU-bound jobs keep every CPU worker spinning. Two probes
//! sample the runtime all the time, as in the thumbnail probe of `cv-artwork`:
//!
//! - the command probe sleeps 5 ms in a loop and records how late it wakes (the delay a Tauri
//!   command would see before the runtime picks it up);
//! - the query probe reads the first library page the way a command does (blocking-pool hop, count
//!   plus 60 rows) every 20 ms and records the round trip.
//!
//! The p99 of each must stay under 50 ms. Run it on four logical cores to match the budget's
//! machine: `start /affinity F /wait <test exe> --ignored --nocapture` (Windows).

use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use cv_app::data_location::{DataLocation, LocationInputs, PORTABLE_MARKER};
use cv_app::secrets::SecretStore;
use cv_app::services::{AppServices, BootOptions};
use cv_db::Database;
use cv_db::repo::data_sources::insert_source;
use cv_db::repo::query::{count_items, list_items_at};
use cv_db::repo::{Filter, ItemFlag, ItemQuery};
use cv_domain::{ItemKind, SourceKind};
use cv_jobs::{ItemControl, ItemError, ItemWork, JobSpec, LaneConfig, LaneId};
use tempfile::TempDir;

const LIBRARY: usize = 5_000;
const COLD_START_BUDGET: Duration = Duration::from_millis(1_500);
const INPUT_P99: Duration = Duration::from_millis(50);
/// Rows the grid asks for on its first screen (viewport plus overscan).
const FIRST_SCREEN_ROWS: usize = 120;
const TICK: Duration = Duration::from_millis(5);
/// The measured window lasts at least this long, with the CPU burn running to its end, so a p99
/// is taken over hundreds of samples. A fast first fill ends in about 3 s and leaves under a
/// hundred query samples, where the p99 is the single worst sample.
const MIN_WINDOW: Duration = Duration::from_secs(10);
/// Fewer query samples than this make the p99 a single outlier: the probe refuses the verdict.
const MIN_QUERY_SAMPLES: usize = 200;

/// The data folder of a test: under `root`, or, when `root` holds the portable marker, the `data`
/// folder beside it (the layout of a portable install).
fn location(root: &Path) -> DataLocation {
    let exe_dir = if root.join(PORTABLE_MARKER).is_file() {
        root.to_path_buf()
    } else {
        root.join("app")
    };
    let inputs = LocationInputs {
        exe_dir,
        user_data_base: root.join("userdata"),
        user_config_base: root.join("userconfig"),
    };
    fs::create_dir_all(&inputs.exe_dir).unwrap();
    DataLocation::resolve(&inputs)
}

fn options(root: &Path, lanes: LaneConfig) -> BootOptions {
    let mut options = BootOptions::new(location(root), Arc::new(SecretStore::in_memory()));
    options.lanes = lanes;
    // Nothing here may reach a provider's website.
    options.without_providers()
}

/// `count` movie folders, each with a video and an NFO, spread over eight writer threads.
fn build_tree(root: &Path, prefix: &str, count: usize) {
    let threads = 8;
    std::thread::scope(|scope| {
        for t in 0..threads {
            scope.spawn(move || {
                for n in (t..count).step_by(threads) {
                    let year = 1950 + (n % 75);
                    let name = format!("{prefix} {n:05} ({year})");
                    let dir = root.join(&name);
                    fs::create_dir_all(&dir).expect("movie folder");
                    fs::write(dir.join(format!("{name}.mkv")), format!("video {n}"))
                        .expect("video");
                    fs::write(
                        dir.join(format!("{name}.nfo")),
                        format!(
                            "<movie><title>{prefix} {n:05}</title><year>{year}</year>\
                             <plot>Synthetic plot number {n}.</plot></movie>"
                        ),
                    )
                    .expect("nfo");
                }
            });
        }
    });
}

fn add_source(services: &AppServices, path: &Path) {
    let path = path.to_string_lossy().into_owned();
    services
        .db()
        .write(move |tx| insert_source(tx, SourceKind::Movie, &path))
        .expect("source");
}

/// Waits until at least one job exists and every job has stayed ended for half a second (a job
/// that ends can queue the next one, like the NFO import after a rescan).
async fn all_jobs_done(services: &AppServices, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(300);
    let mut quiet_since: Option<Instant> = None;
    loop {
        let jobs = services.jobs().list();
        if !jobs.is_empty() && jobs.iter().all(|j| j.state.is_terminal()) {
            let since = *quiet_since.get_or_insert_with(Instant::now);
            if since.elapsed() >= Duration::from_millis(500) {
                return;
            }
        } else {
            quiet_since = None;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// What the library window asks for first: the rows, their total, and the sidebar counts.
fn first_screen(db: &Database) -> (u64, usize) {
    let movies = ItemQuery::new(ItemKind::Movie);
    let total = db.read(|c| count_items(c, &movies)).expect("count");
    let rows = db
        .read(|c| list_items_at(c, &movies, 0, FIRST_SCREEN_ROWS))
        .expect("rows");
    for kind in [ItemKind::Show, ItemKind::Set] {
        db.read(|c| count_items(c, &ItemQuery::new(kind)))
            .expect("sidebar count");
    }
    let review = ItemQuery::new(ItemKind::Movie).filter(Filter::Flag {
        flag: ItemFlag::NeedsReview,
        value: true,
    });
    db.read(|c| count_items(c, &review)).expect("review count");
    (total, rows.len())
}

/// `CV_PERF_PORTABLE_DIR`: build the 5,000-movie data folder in this directory as a portable
/// install (marker file plus `data` and `movies` folders) and keep it, so `perf-coldstart.mjs`
/// can launch the real app on it. Without it the probe works in a temporary folder.
enum Workspace {
    Temp(TempDir),
    Kept(std::path::PathBuf),
}

impl Workspace {
    fn open() -> Self {
        match std::env::var_os("CV_PERF_PORTABLE_DIR") {
            Some(dir) => {
                let dir = std::path::PathBuf::from(dir);
                fs::create_dir_all(&dir).unwrap();
                fs::write(dir.join(PORTABLE_MARKER), "").unwrap();
                Self::Kept(dir)
            }
            None => Self::Temp(TempDir::new().unwrap()),
        }
    }

    fn path(&self) -> &Path {
        match self {
            Self::Temp(dir) => dir.path(),
            Self::Kept(dir) => dir,
        }
    }
}

#[test]
#[ignore = "perf probe: run by hand on a quiet machine, in the release profile"]
fn cold_start_5k() {
    let temp = Workspace::open();
    let media = temp.path().join("movies");
    fs::create_dir_all(&media).unwrap();
    build_tree(&media, "Synthetic Film", LIBRARY);

    // First launch: fill the library, then quit.
    {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let services =
            AppServices::boot(options(temp.path(), LaneConfig::default())).expect("first boot");
        add_source(&services, &media);
        let filled = Instant::now();
        runtime.block_on(async {
            services.start();
            all_jobs_done(&services, "the first fill").await;
        });
        eprintln!("first fill of {LIBRARY} movies: {:?}", filled.elapsed());
        let total = services
            .db()
            .read(|c| count_items(c, &ItemQuery::new(ItemKind::Movie)))
            .unwrap();
        assert_eq!(total, LIBRARY as u64);
        services.shutdown();
        drop(services);
        drop(runtime);
    }

    // Second launch: the clock starts where the desktop shell builds its services.
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let started = Instant::now();
    let services =
        AppServices::boot(options(temp.path(), LaneConfig::default())).expect("second boot");
    let booted = started.elapsed();
    let report = runtime.block_on(async { services.start() });
    let running = started.elapsed();
    let (total, rows) = first_screen(services.db());
    let ready = started.elapsed();
    eprintln!(
        "cold start, {LIBRARY} movies: boot {booted:?}, start {:?} after, first screen data {:?} \
         after, total {ready:?} (budget {COLD_START_BUDGET:?}); resumed {report:?}",
        running - booted,
        ready - running,
    );
    assert_eq!(total, LIBRARY as u64);
    assert_eq!(rows, FIRST_SCREEN_ROWS);

    // The startup scan runs behind the first screen and finds nothing to do.
    let scan_started = Instant::now();
    runtime.block_on(all_jobs_done(&services, "the startup scan"));
    eprintln!(
        "startup scan of the unchanged library, from the first screen: {:?}",
        scan_started.elapsed()
    );
    services.shutdown();
    assert!(
        ready < COLD_START_BUDGET,
        "cold start took {ready:?}, the budget is {COLD_START_BUDGET:?}"
    );
}

/// Keeps a CPU worker busy: each item spins for a few milliseconds on the blocking pool.
struct Burn {
    stop: Arc<AtomicBool>,
    done: Arc<AtomicUsize>,
}

impl ItemWork for Burn {
    type Item = u32;

    async fn process(&self, _item: &u32, _control: &ItemControl) -> Result<(), ItemError> {
        if self.stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        let done = Arc::clone(&self.done);
        let _ = tokio::task::spawn_blocking(move || {
            let until = Instant::now() + Duration::from_millis(20);
            let mut x = 1u64;
            while Instant::now() < until {
                x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                std::hint::black_box(x);
            }
            done.fetch_add(1, Ordering::Relaxed);
        })
        .await;
        Ok(())
    }
}

fn quantile(sorted: &[Duration], q: f64) -> Duration {
    if sorted.is_empty() {
        return Duration::ZERO;
    }
    let index = ((sorted.len() as f64 * q).ceil() as usize).clamp(1, sorted.len()) - 1;
    sorted[index]
}

fn report(name: &str, mut samples: Vec<Duration>) -> Duration {
    samples.sort();
    let p99 = quantile(&samples, 0.99);
    eprintln!(
        "{name}: {} samples, p50 {:?}, p95 {:?}, p99 {:?}, max {:?}, over 50 ms: {}, over 250 ms: {}",
        samples.len(),
        quantile(&samples, 0.50),
        quantile(&samples, 0.95),
        p99,
        samples.last().copied().unwrap_or_default(),
        samples
            .iter()
            .filter(|s| **s > Duration::from_millis(50))
            .count(),
        samples
            .iter()
            .filter(|s| **s > Duration::from_millis(250))
            .count(),
    );
    p99
}

#[test]
#[ignore = "perf probe: run by hand on a quiet machine, in the release profile"]
fn input_latency_under_load() {
    let temp = TempDir::new().unwrap();
    let first = temp.path().join("movies-a");
    let second = temp.path().join("movies-b");
    fs::create_dir_all(&first).unwrap();
    fs::create_dir_all(&second).unwrap();
    build_tree(&first, "Synthetic Film", LIBRARY);
    build_tree(&second, "Loaded Film", LIBRARY);

    // The shape of a four-core machine: two disk workers, three CPU workers, four runtime threads.
    let lanes = LaneConfig {
        disk: 2,
        cpu: 3,
        ..LaneConfig::default()
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .unwrap();
    let services = AppServices::boot(options(temp.path(), lanes)).expect("boot");
    add_source(&services, &first);
    runtime.block_on(async {
        services.start();
        all_jobs_done(&services, "the first fill").await;
    });
    let before = services
        .db()
        .read(|c| count_items(c, &ItemQuery::new(ItemKind::Movie)))
        .unwrap();
    assert_eq!(before, LIBRARY as u64);

    // Load: a first-fill rescan of a second source (disk lane, database writes, NFO import) and
    // CPU-bound jobs on every CPU worker, until the rescan has finished.
    let stop = Arc::new(AtomicBool::new(false));
    let burned = Arc::new(AtomicUsize::new(0));
    let (command, query, loaded) = runtime.block_on(async {
        add_source(&services, &second);
        let rescan = services.scan().rescan_all().expect("rescan");
        let burn_jobs: Vec<_> = (0..3)
            .map(|_| {
                services.jobs().submit(
                    JobSpec::new("Burn", LaneId::Cpu).concurrency(1),
                    Burn {
                        stop: Arc::clone(&stop),
                        done: Arc::clone(&burned),
                    },
                    (0..10_000).collect(),
                )
            })
            .collect();

        let measuring = Arc::new(AtomicBool::new(true));
        let command = {
            let measuring = Arc::clone(&measuring);
            tokio::spawn(async move {
                let mut samples = Vec::new();
                while measuring.load(Ordering::Relaxed) {
                    let asked = Instant::now();
                    tokio::time::sleep(TICK).await;
                    samples.push(asked.elapsed().saturating_sub(TICK));
                }
                samples
            })
        };
        let query = {
            let measuring = Arc::clone(&measuring);
            let db = Arc::clone(services.db());
            tokio::spawn(async move {
                let mut samples = Vec::new();
                while measuring.load(Ordering::Relaxed) {
                    let asked = Instant::now();
                    let db = Arc::clone(&db);
                    let (total, rows) = tokio::task::spawn_blocking(move || {
                        let q = ItemQuery::new(ItemKind::Movie);
                        let total = db.read(|c| count_items(c, &q)).expect("count");
                        let rows = db.read(|c| list_items_at(c, &q, 0, 60)).expect("rows");
                        (total, rows.len())
                    })
                    .await
                    .expect("query task");
                    assert!(total >= LIBRARY as u64 && rows == 60);
                    samples.push(asked.elapsed());
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                samples
            })
        };

        let loaded = Instant::now();
        let window_end = loaded + MIN_WINDOW;
        // The runner (scripts/perf-budgets.mjs) starts the page-side measurement on this line.
        eprintln!("load started");
        // Wait for the rescan (the disk lane's work), not for the burn jobs.
        let deadline = Instant::now() + Duration::from_secs(300);
        loop {
            let jobs = services.jobs().list();
            let busy = jobs
                .iter()
                .filter(|j| j.name != "Burn")
                .any(|j| !j.state.is_terminal());
            if !busy && jobs.iter().any(|j| j.name != "Burn") {
                break;
            }
            assert!(Instant::now() < deadline, "the loading rescan never ended");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let loaded = loaded.elapsed();
        // The rescan is over; the CPU burn keeps every CPU worker busy until the window is long
        // enough.
        while Instant::now() < window_end {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        measuring.store(false, Ordering::Relaxed);
        stop.store(true, Ordering::Relaxed);
        for job in &burn_jobs {
            job.cancel();
        }
        drop(rescan);
        (command.await.unwrap(), query.await.unwrap(), loaded)
    });

    let total = services
        .db()
        .read(|c| count_items(c, &ItemQuery::new(ItemKind::Movie)))
        .unwrap();
    eprintln!(
        "load: first fill of {LIBRARY} more movies in {loaded:?} with {} CPU burns done; \
         library now {total} movies",
        burned.load(Ordering::Relaxed)
    );
    assert_eq!(total, (LIBRARY * 2) as u64);
    let command_p99 = report("command wake-up lateness", command);
    let query_samples = query.len();
    let query_p99 = report("first-page query round trip", query);
    services.shutdown();
    assert!(
        query_samples >= MIN_QUERY_SAMPLES,
        "only {query_samples} query samples, a p99 over fewer than {MIN_QUERY_SAMPLES} is not a          measurement (is the machine overloaded?)"
    );
    assert!(
        command_p99 < INPUT_P99,
        "command p99 {command_p99:?}, the budget is {INPUT_P99:?}"
    );
    assert!(
        query_p99 < INPUT_P99,
        "query p99 {query_p99:?}, the budget is {INPUT_P99:?}"
    );
}
