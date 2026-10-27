//! The image cache of a 5,000-poster library is built in the background
//! while the application stays responsive.
//!
//! The library is 5,000 movie folders, each with a video and a 500 x 750 poster. A rescan
//! creates the items, the thumbnail job follows it ([`build_after`]) and caches the grid and
//! list sizes of every poster.
//!
//! # The responsiveness bound
//!
//! "The UI stays responsive" is measured on the runtime that serves the UI's commands and
//! `cv://img` requests, here a two-worker multi-thread runtime (the smallest realistic one),
//! while the job runs. Two probes sample it all the time:
//!
//! - **Command probe.** Sleeps 5 ms in a loop and records how late it wakes. This is the delay
//!   any Tauri command or channel message would see before the runtime picks it up.
//! - **Image probe.** Every 20 ms it asks an [`ImageService`] for a grid thumbnail the job has
//!   already built (a `cv://img` request: blocking-pool hop, cache read, headers) and records the
//!   round trip.
//!
//! The bound, held for the whole build, for each probe:
//!
//! | Probe | p99 | Stall (a sample over) | Stalls allowed | Ceiling (no sample over) |
//! | --- | --- | --- | --- | --- |
//! | Command | under 50 ms | 250 ms | at most 3 | 2 s |
//! | Image request | under 100 ms | 500 ms | at most 3 | 3 s |
//!
//! Input feels instant under 100 ms, so the p99 figures keep the typical frame well inside that.
//! The worst single sample is not held to the stall figure: a test machine loaded by parallel
//! builds can deschedule the whole test process once or twice, which shows up as an isolated
//! stall that says nothing about the runtime. Image work done on the runtime stalls it again on
//! every image, so it breaks the stall budget (and the p99) at once; one long block (seconds)
//! breaks the ceiling. The negative control below checks that five bursts of inline image work
//! fail this verdict. The measured figures are printed with `--nocapture`.
//!
//! # Two tests, one of them a perf probe
//!
//! The wall-clock bound only holds on a quiet machine, so it is an `#[ignore]`d perf probe
//! (`perf_probe_…`, with its negative control); `bun run perf:budgets` runs it. The
//! default suite runs the same library build at 400 posters and asserts only what is exact:
//! every poster ends up cached at both sizes, with its placeholder and palette.

mod support;

use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use cv_artwork::img::{ImageRequest, ImageService, SourceMap};
use cv_artwork::thumb::{ThumbKey, ThumbSize};
use cv_artwork::thumb_job::{BUILD_SIZES, build_after, default_parallelism};
use cv_jobs::JobState;
use support::{Lib, poster_jpeg, variant};

/// The library size. `CV_THUMB_TEST_POSTERS` shrinks it for a quick local run; the task's
/// verification is the default 5,000.
fn posters() -> usize {
    std::env::var("CV_THUMB_TEST_POSTERS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5_000)
}

const TICK: Duration = Duration::from_millis(5);
/// The command probe's bound (see the module docs).
const COMMAND: Bound = Bound {
    p99: Duration::from_millis(50),
    stall: Duration::from_millis(250),
    ceiling: Duration::from_secs(2),
};
/// The image probe's bound (see the module docs).
const IMAGE: Bound = Bound {
    p99: Duration::from_millis(100),
    stall: Duration::from_millis(500),
    ceiling: Duration::from_secs(3),
};
/// How many isolated stalls a probe may see in one run (a loaded machine descheduling the test).
const STALLS_ALLOWED: usize = 3;

fn poster_name(n: usize) -> String {
    let year = 1950 + (n % 75);
    format!("Synthetic Film {n:04} ({year})")
}

fn build_tree(root: &Path, base: &[u8], count: usize) {
    let threads = 8;
    std::thread::scope(|scope| {
        for t in 0..threads {
            scope.spawn(move || {
                for n in (t..count).step_by(threads) {
                    let name = poster_name(n);
                    let dir = root.join(&name);
                    fs::create_dir_all(&dir).expect("movie folder");
                    fs::write(dir.join(format!("{name}.mkv")), format!("video {n}"))
                        .expect("video");
                    fs::write(dir.join(format!("{name}-poster.jpg")), variant(base, n))
                        .expect("poster");
                }
            });
        }
    });
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

/// Builds the cache of `posters` posters with the two probes running, checks that every poster
/// is cached at both sizes, and hands back the probes' samples. No timing is asserted here.
async fn build_with_probes(posters: usize) -> (Latency, Latency) {
    let lib = Lib::movies();
    let base = poster_jpeg(500, 750, 7);
    let built = Instant::now();
    build_tree(&lib.root, &base, posters);
    eprintln!("built {posters} posters in {:?}", built.elapsed());

    let started = Instant::now();
    let rescan = lib.rescan();
    let thumbs_task = {
        let changed = rescan.clone();
        build_after(
            &lib.manager,
            &lib.db,
            &lib.cache,
            rescan.job.clone(),
            move || changed.changed_items(),
            default_parallelism(),
        )
    };
    eprintln!(
        "building with {} images at a time on {} cores",
        default_parallelism(),
        std::thread::available_parallelism().map_or(0, usize::from)
    );

    // Probe 1: how late does the runtime run a timer?
    let stop = Arc::new(AtomicBool::new(false));
    let command_probe = {
        let stop = Arc::clone(&stop);
        tokio::spawn(async move {
            let mut late = Vec::new();
            while !stop.load(Ordering::Relaxed) {
                let asked = Instant::now();
                tokio::time::sleep(TICK).await;
                late.push(asked.elapsed().saturating_sub(TICK));
            }
            Latency::new(late)
        })
    };

    // Probe 2: a cv://img request for a thumbnail that is already built.
    let sources = Arc::new(SourceMap::new());
    let service = Arc::new(ImageService::new(Arc::clone(&lib.cache), sources));
    let keys: Arc<Vec<ThumbKey>> = Arc::new(
        (0..posters)
            .map(|n| ThumbKey::of_bytes(&variant(&base, n)))
            .collect(),
    );
    let image_probe = {
        let stop = Arc::clone(&stop);
        let service = Arc::clone(&service);
        let cache = Arc::clone(&lib.cache);
        let keys = Arc::clone(&keys);
        tokio::spawn(async move {
            let mut round_trips = Vec::new();
            let mut round = 0_usize;
            while !stop.load(Ordering::Relaxed) {
                tokio::time::sleep(Duration::from_millis(20)).await;
                // Pick a thumbnail that is already built (the search is not part of the timing).
                let found = {
                    let (cache, keys) = (Arc::clone(&cache), Arc::clone(&keys));
                    let start = round * 37;
                    tokio::task::spawn_blocking(move || {
                        (0..256)
                            .map(|i| &keys[(start + i) % posters])
                            .find(|k| cache.has(k, ThumbSize::Grid))
                            .cloned()
                    })
                    .await
                    .expect("search")
                };
                round += 1;
                let Some(key) = found else { continue };
                let service = Arc::clone(&service);
                let asked = Instant::now();
                let response = tokio::task::spawn_blocking(move || {
                    let path = format!("/img/{key}/grid");
                    service.handle(&ImageRequest {
                        method: "GET",
                        path: &path,
                        if_none_match: None,
                    })
                })
                .await
                .expect("image request");
                round_trips.push(asked.elapsed());
                assert_eq!(response.status, 200);
            }
            Latency::new(round_trips)
        })
    };

    let thumbs = thumbs_task
        .await
        .unwrap()
        .expect("a thumbnail job was queued");
    let report = thumbs.job.wait().await;
    stop.store(true, Ordering::Relaxed);
    let commands = command_probe.await.unwrap();
    let images = image_probe.await.unwrap();
    eprintln!(
        "rescan and cache build of {posters} posters: {:?}",
        started.elapsed()
    );
    commands.report("command wake-up lateness");
    images.report("cv://img request round trip");

    // Every poster is cached at both sizes, with its placeholder and palette.
    assert_eq!(report.snapshot.state, JobState::Completed);
    assert_eq!(
        (report.snapshot.total, report.snapshot.failed),
        (posters, 0)
    );
    assert_eq!(thumbs.stats().generated, posters as u64);
    assert_eq!(lib.cache.generations(), posters as u64);
    let missing = keys
        .iter()
        .filter(|k| {
            !BUILD_SIZES.iter().all(|s| lib.cache.has(k, *s)) || lib.cache.meta(k).is_none()
        })
        .count();
    assert_eq!(missing, 0, "posters without a cached grid, list or meta");
    assert_eq!(lib.cache.usage().unwrap().entries, posters as u64);

    (commands, images)
}

/// The correctness half of the thumbnail-cache check, with no wall clock in it: a rescan of a library of
/// posters caches every one of them at both sizes, with its placeholder and palette, while the
/// probes run (the image probe asserts every request it makes is answered). The responsiveness
/// bound is the ignored perf probe below.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_large_library_is_fully_cached_by_the_background_job() {
    build_with_probes(400).await;
}

/// The responsiveness half of the thumbnail-cache check, a perf probe: timing ceilings are only meaningful on a
/// quiet machine, so it does not run in the default suite (a loaded machine fails it for reasons
/// unrelated to the code). Run it with
/// `cargo test -p cv-artwork --test thumb_job_5k -- --ignored --nocapture`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "perf probe: wall-clock ceilings, run on a quiet machine"]
async fn perf_probe_five_thousand_posters_keep_the_runtime_responsive() {
    let (commands, images) = build_with_probes(posters()).await;
    assert!(
        commands.samples.len() > 100,
        "the command probe barely ran ({} samples)",
        commands.samples.len()
    );
    if let Err(why) = commands.check(&COMMAND) {
        panic!("the command probe broke its bound: {why}");
    }
    assert!(
        images.samples.len() > 20,
        "the image probe barely ran ({} samples)",
        images.samples.len()
    );
    if let Err(why) = images.check(&IMAGE) {
        panic!("the image probe broke its bound: {why}");
    }
}

/// The probe and its verdict are meaningful: the same image work done inline on the async
/// runtime, instead of on the job's blocking threads, stalls the command probe every time it
/// runs, more often than the stall budget allows, so the bound fails.
#[tokio::test(flavor = "current_thread")]
#[ignore = "perf probe: wall-clock figures, run on a quiet machine"]
async fn the_command_probe_catches_image_work_done_on_the_runtime() {
    let lib = Lib::movies();
    let base = poster_jpeg(500, 750, 9);
    let stop = Arc::new(AtomicBool::new(false));
    let probe = {
        let stop = Arc::clone(&stop);
        tokio::spawn(async move {
            let mut late = Vec::new();
            while !stop.load(Ordering::Relaxed) {
                let asked = Instant::now();
                tokio::time::sleep(TICK).await;
                late.push(asked.elapsed().saturating_sub(TICK));
            }
            Latency::new(late)
        })
    };
    tokio::task::yield_now().await;
    let mut n = 0;
    // Five commands that each do 400 ms of image work inline, with the runtime free in between.
    for _ in 0..5 {
        let started = Instant::now();
        while started.elapsed() < Duration::from_millis(400) {
            // Blocking the only runtime thread, as an inline `ensure` in a command would.
            lib.cache
                .ensure_bytes(&variant(&base, n), &BUILD_SIZES)
                .unwrap();
            n += 1;
        }
        tokio::time::sleep(TICK * 4).await;
    }
    stop.store(true, Ordering::Relaxed);
    let latency = probe.await.unwrap();
    latency.report("inline image work, command wake-up lateness");
    let stalls = latency.over(COMMAND.stall);
    assert!(
        stalls > STALLS_ALLOWED,
        "five bursts of inline work should have stalled the runtime past {:?} more than          {STALLS_ALLOWED} times, got {stalls} (worst {:?})",
        COMMAND.stall,
        latency.max()
    );
    assert!(
        latency.check(&COMMAND).is_err(),
        "a blocked runtime passed the command bound"
    );
}
