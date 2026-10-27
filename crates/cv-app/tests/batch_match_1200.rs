//! A 1,200-item batch match runs in the background while the application
//! stays responsive.
//!
//! The library is 1,200 unmatched movies. A scripted provider (no network, 2 ms per request, like
//! a fast connection) answers for them: 1,020 have one clear match, 60 have two identical
//! candidates, 60 have only an unrelated result and 60 have none. "Match unmatched" runs as a
//! background job on the provider's lane.
//!
//! # The responsiveness bound
//!
//! "The UI stays responsive" is measured on the runtime that serves the UI's commands, here a
//! two-worker multi-thread runtime (the smallest realistic one), while the job runs. Three probes
//! sample it all the time:
//!
//! - **Command probe.** Sleeps 5 ms in a loop and records how late it wakes. This is the delay
//!   any Tauri command or channel message would see before the runtime picks it up.
//! - **Library probe.** Every 20 ms it reads a page of 60 movies from the library, the query the
//!   grid issues while the user scrolls (a blocking-pool hop, a read connection, the rows). The
//!   job is writing to the same database at the time.
//! - **Progress probe.** Reads the job's progress every 25 ms. The batch must report steady
//!   progress, not one jump at the end.
//!
//! The bound, held for the whole run, for each probe:
//!
//! | Probe | p99 | Stall (a sample over) | Stalls allowed | Ceiling (no sample over) |
//! | --- | --- | --- | --- | --- |
//! | Command | under 50 ms | 250 ms | at most 3 | 2 s |
//! | Library page | under 100 ms | 500 ms | at most 3 | 3 s |
//!
//! Input feels instant under 100 ms, so the p99 figures keep the typical frame well inside that.
//! The worst single sample is not held to the stall figure: a test machine loaded by parallel
//! builds can deschedule the whole test process once or twice, which shows up as an isolated
//! stall that says nothing about the runtime. Work that blocks the runtime stalls it again on
//! every item it touches, so it breaks the stall budget (and the p99) at once; one long block
//! (seconds) breaks the ceiling. The negative control below checks that a provider blocking
//! the runtime on five items fails this verdict. The measured figures are printed with
//! `--nocapture`.
//!
//! # Two tests
//!
//! A wall-clock bound holds only on a quiet machine: parallel builds make the figures above fail
//! for reasons that say nothing about the code. So the functional test (every item handled, the
//! outcome in the library, steady progress, the probes ran) always runs, and the same run held to
//! the latency bound is the `#[ignore]`d budget test, like the other perf probes. Run it by hand on a quiet machine, or with `bun run perf:budgets`:
//!
//! ```text
//! cargo test --release -p cv-app --test batch_match_1200 -- --ignored --nocapture
//! ```

mod support;

use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use cv_app::matching::batch::{BatchMode, submit_batch};
use cv_db::repo::review;
use cv_db::repo::{ItemQuery, insert_item, list_items_at};
use cv_domain::{Item, ItemKind};
use cv_jobs::JobState;
use support::{Entry, Lab, Scripted, arc, dynamic};

/// The batch size. `CV_MATCH_TEST_ITEMS` (a multiple of 20) shrinks it for a quick local run; the
/// task's verification is the default 1,200.
fn items() -> usize {
    std::env::var("CV_MATCH_TEST_ITEMS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1_200)
}

const TICK: Duration = Duration::from_millis(5);
/// The command probe's bound (see the module docs).
const COMMAND: Bound = Bound {
    p99: Duration::from_millis(50),
    stall: Duration::from_millis(250),
    ceiling: Duration::from_secs(2),
};
/// The library probe's bound (see the module docs).
const LIBRARY: Bound = Bound {
    p99: Duration::from_millis(100),
    stall: Duration::from_millis(500),
    ceiling: Duration::from_secs(3),
};
/// How many isolated stalls a probe may see in one run (a loaded machine descheduling the test).
const STALLS_ALLOWED: usize = 3;

fn title_of(n: usize) -> String {
    format!("Synthetic Film {n:04}")
}

/// What the scripted provider does for item `n`, by `n % 20`.
#[derive(Debug, PartialEq, Eq)]
enum Shape {
    Clear,
    Tie,
    Weak,
    Nothing,
}

fn shape(n: usize) -> Shape {
    match n % 20 {
        7 => Shape::Tie,
        13 => Shape::Weak,
        17 => Shape::Nothing,
        _ => Shape::Clear,
    }
}

fn year_of(n: usize) -> u16 {
    1950 + u16::try_from(n % 75).unwrap()
}

fn catalogue(count: usize) -> Vec<Entry> {
    let mut entries = Vec::new();
    for n in 0..count {
        let title = title_of(n);
        match shape(n) {
            Shape::Clear => {
                entries.push(Entry::new(&format!("{}", 100_000 + n), &title, year_of(n)))
            }
            Shape::Tie => {
                for k in 0..2 {
                    entries.push(
                        Entry::new(&format!("{}", 200_000 + 2 * n + k), &title, year_of(n))
                            .found_by(&title),
                    );
                }
            }
            Shape::Weak => entries.push(
                Entry::new(&format!("{}", 400_000 + n), "Unrelated Picture", 1999).found_by(&title),
            ),
            Shape::Nothing => {}
        }
    }
    entries
}

/// The `q` quantile (0..1) of the samples.
fn quantile(sorted: &[Duration], q: f64) -> Duration {
    if sorted.is_empty() {
        return Duration::ZERO;
    }
    let index = ((sorted.len() as f64 * q).ceil() as usize).clamp(1, sorted.len()) - 1;
    sorted[index]
}

/// A probe's responsiveness bound.
struct Bound {
    /// The 99th percentile must be under this.
    p99: Duration,
    /// A sample over this is a stall; at most [`STALLS_ALLOWED`] are accepted.
    stall: Duration,
    /// No sample may reach this.
    ceiling: Duration,
}

struct Latency {
    samples: Vec<Duration>,
}

impl Latency {
    fn new(mut samples: Vec<Duration>) -> Self {
        samples.sort();
        Self { samples }
    }

    fn p99(&self) -> Duration {
        quantile(&self.samples, 0.99)
    }

    fn max(&self) -> Duration {
        self.samples.last().copied().unwrap_or_default()
    }

    /// How many samples are over `threshold`.
    fn over(&self, threshold: Duration) -> usize {
        self.samples.iter().filter(|s| **s > threshold).count()
    }

    /// Whether the samples meet `bound`: the p99 is under it, at most [`STALLS_ALLOWED`] samples
    /// stall and none passes the ceiling. The error says which part failed.
    fn check(&self, bound: &Bound) -> Result<(), String> {
        if self.p99() >= bound.p99 {
            return Err(format!("p99 {:?} over {:?}", self.p99(), bound.p99));
        }
        let stalls = self.over(bound.stall);
        if stalls > STALLS_ALLOWED {
            return Err(format!(
                "{stalls} samples over {:?} (at most {STALLS_ALLOWED} allowed)",
                bound.stall
            ));
        }
        if self.max() >= bound.ceiling {
            return Err(format!("worst {:?} over {:?}", self.max(), bound.ceiling));
        }
        Ok(())
    }

    fn report(&self, what: &str) {
        eprintln!(
            "{what}: {} samples, p50 {:?}, p99 {:?}, max {:?}",
            self.samples.len(),
            quantile(&self.samples, 0.5),
            self.p99(),
            self.max()
        );
    }
}

fn command_probe(stop: &Arc<AtomicBool>) -> tokio::task::JoinHandle<Latency> {
    let stop = Arc::clone(stop);
    tokio::spawn(async move {
        let mut late = Vec::new();
        while !stop.load(Ordering::Relaxed) {
            let asked = Instant::now();
            tokio::time::sleep(TICK).await;
            late.push(asked.elapsed().saturating_sub(TICK));
        }
        Latency::new(late)
    })
}

fn seed(lab: &Lab, count: usize) {
    lab.db
        .write(move |tx| {
            for n in 0..count {
                let mut item = Item::movie(title_of(n));
                item.core.year = year_of(n);
                item.core.path = format!("/library/{}", title_of(n));
                insert_item(tx, &item)?;
            }
            Ok(())
        })
        .expect("seed the library");
}

/// Runs the whole batch with the probes alongside, checks that every item was handled and the
/// library holds the outcome, and returns what the command and library probes measured.
async fn run_batch() -> (Latency, Latency) {
    let count = items();
    assert_eq!(count % 20, 0, "the synthetic mix repeats every 20 items");
    let lab = Lab::new();
    seed(&lab, count);
    let provider = arc(Scripted::new("tmdb", catalogue(count)).latency(Duration::from_millis(2)));
    let ctx = lab.context(vec![dynamic(&provider)]);

    let stop = Arc::new(AtomicBool::new(false));
    let commands = command_probe(&stop);

    let library = {
        let stop = Arc::clone(&stop);
        let db = Arc::clone(&lab.db);
        tokio::spawn(async move {
            let mut round_trips = Vec::new();
            let mut round = 0_usize;
            while !stop.load(Ordering::Relaxed) {
                tokio::time::sleep(Duration::from_millis(20)).await;
                let db = Arc::clone(&db);
                let offset = (round * 61) % (count - 60);
                round += 1;
                let asked = Instant::now();
                let page = tokio::task::spawn_blocking(move || {
                    db.read(|c| list_items_at(c, &ItemQuery::new(ItemKind::Movie), offset, 60))
                })
                .await
                .expect("library query")
                .expect("page");
                round_trips.push(asked.elapsed());
                assert_eq!(page.len(), 60);
            }
            Latency::new(round_trips)
        })
    };

    let started = Instant::now();
    let batch = submit_batch(&lab.manager, &ctx, BatchMode::MatchUnmatched, None, 4)
        .expect("the batch is queued");
    assert_eq!(batch.planned, count);
    assert_eq!(batch.jobs.len(), 1, "one provider, one job");
    let job = batch.jobs[0].job.clone();

    // The progress probe: how many distinct figures the job reported while it ran.
    let progress = {
        let job = job.clone();
        tokio::spawn(async move {
            let mut seen = BTreeSet::new();
            loop {
                let snapshot = job.snapshot();
                seen.insert(snapshot.done);
                if matches!(
                    snapshot.state,
                    JobState::Completed | JobState::Failed | JobState::Cancelled
                ) {
                    return seen;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
    };

    let report = job.wait().await;
    stop.store(true, Ordering::Relaxed);
    let commands = commands.await.unwrap();
    let library = library.await.unwrap();
    let progress = progress.await.unwrap();
    let stats = batch.jobs[0].stats();
    eprintln!(
        "matched {count} items in {:?}: {stats:?}",
        started.elapsed()
    );
    eprintln!(
        "provider requests: {} searches, {} details",
        Scripted::count(&provider.searches),
        Scripted::count(&provider.details)
    );
    commands.report("command wake-up lateness");
    library.report("library page round trip");
    eprintln!("progress figures seen: {}", progress.len());

    // Every item was handled, none failed, and the library holds the outcome.
    let clear = (0..count).filter(|n| shape(*n) == Shape::Clear).count();
    let uncertain = count - clear;
    assert_eq!(report.snapshot.state, JobState::Completed);
    assert_eq!((report.snapshot.total, report.snapshot.failed), (count, 0));
    assert_eq!(stats.accepted, clear as u64);
    assert_eq!(stats.queued, uncertain as u64);
    assert_eq!(
        lab.db.read(review::count_pending).unwrap(),
        uncertain as u64
    );
    let pending = lab.db.read(|c| review::list_pending(c, 5_000, 0)).unwrap();
    let reason = |text: &str| pending.iter().filter(|r| r.reason == text).count();
    assert_eq!(reason("2 strong candidates"), count / 20);
    assert_eq!(reason("No candidates found"), count / 20);
    assert_eq!(
        pending
            .iter()
            .filter(|r| r.reason.starts_with("Best match"))
            .count(),
        count / 20
    );
    // A spot check of what was written: item 0 is a clear match.
    let first = lab
        .db
        .read(|c| cv_db::repo::get_item(c, cv_domain::ItemId(1)))
        .unwrap()
        .unwrap();
    assert_eq!(first.core.last_scraper_id, "tmdb");
    assert_eq!(first.core.ids.get("tmdb"), Some("100000"));

    // The batch reported steady progress, not one jump at the end.
    assert!(
        progress.len() >= 5,
        "only {} progress figures were seen",
        progress.len()
    );

    // The probes ran for the whole job (a machine-independent check).
    assert!(
        commands.samples.len() > 100,
        "the command probe barely ran ({} samples)",
        commands.samples.len()
    );
    assert!(
        library.samples.len() > 20,
        "the library probe barely ran ({} samples)",
        library.samples.len()
    );
    (commands, library)
}

/// The functional half: all items match in the background, with the counts and steady progress.
/// It always runs; the latency figures are only printed here.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn twelve_hundred_items_match_in_the_background() {
    run_batch().await;
}

/// The budget half: the same run, held to the responsiveness bound. `#[ignore]`d because a loaded
/// machine breaks wall-clock bounds; run it on a quiet one (see the module docs).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "perf budget: run by hand on a quiet machine, in the release profile"]
async fn twelve_hundred_items_match_while_the_runtime_stays_responsive() {
    let (commands, library) = run_batch().await;
    if let Err(why) = commands.check(&COMMAND) {
        panic!("the command probe broke its bound: {why}");
    }
    if let Err(why) = library.check(&LIBRARY) {
        panic!("the library probe broke its bound: {why}");
    }
}

/// The probe and its verdict are meaningful: a provider that blocks its thread (as a synchronous
/// HTTP call or an inline database write would) on the only runtime thread stalls the command
/// probe on every item, more often than the stall budget allows, so the bound fails.
#[tokio::test(flavor = "current_thread")]
async fn the_command_probe_catches_blocking_work_on_the_runtime() {
    let lab = Lab::new();
    seed(&lab, 20);
    let provider = arc(Scripted::new("tmdb", catalogue(20)).blocking(Duration::from_millis(300)));
    let ctx = lab.context(vec![dynamic(&provider)]);

    let stop = Arc::new(AtomicBool::new(false));
    let probe = command_probe(&stop);
    tokio::task::yield_now().await;
    let ids: Vec<cv_domain::ItemId> = (1..=5).map(cv_domain::ItemId).collect();
    let batch = submit_batch(&lab.manager, &ctx, BatchMode::MatchUnmatched, Some(&ids), 1).unwrap();
    batch.wait().await;
    tokio::time::sleep(TICK * 2).await;
    stop.store(true, Ordering::Relaxed);
    let latency = probe.await.unwrap();
    latency.report("blocking provider, command wake-up lateness");
    let stalls = latency.over(COMMAND.stall);
    assert!(
        stalls > STALLS_ALLOWED,
        "blocking work on five items should have stalled the runtime past {:?} more than          {STALLS_ALLOWED} times, got {stalls} (worst {:?})",
        COMMAND.stall,
        latency.max()
    );
    assert!(
        latency.check(&COMMAND).is_err(),
        "a blocked runtime passed the command bound"
    );
}
