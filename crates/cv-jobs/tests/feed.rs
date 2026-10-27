//! The progress feed: at most 10 Hz per job, collapsed bursts, one final report, and the combined
//! figure mirrored to the taskbar and dock.
//!
//! Every test runs on tokio's paused clock, so timings are virtual and exact.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use cv_jobs::{
    ItemControl, ItemError, ItemWork, JobHandle, JobManager, JobSnapshot, JobSpec, JobState,
    LaneConfig, LaneId, Lanes, ManagerConfig, Overall, OverallKind, OverallTracker,
    PROGRESS_INTERVAL, ProgressTick,
};
use tokio::time::{Instant, sleep};

struct Sleepy(Duration);

impl ItemWork for Sleepy {
    type Item = u32;

    async fn process(&self, _item: &u32, _control: &ItemControl) -> Result<(), ItemError> {
        sleep(self.0).await;
        Ok(())
    }
}

fn manager() -> JobManager {
    JobManager::new(Lanes::new(LaneConfig::default()), ManagerConfig::default())
}

fn job(manager: &JobManager, name: &str, items: u32, item_ms: u64) -> JobHandle {
    manager.submit(
        JobSpec::new(name, LaneId::Disk),
        Sleepy(Duration::from_millis(item_ms)),
        (0..items).collect(),
    )
}

/// A report with the time it was delivered.
type Log = Arc<Mutex<Vec<(Instant, ProgressTick)>>>;

/// Runs the feed in the background and records every report it delivers.
fn record(manager: &JobManager) -> (Log, tokio::task::JoinHandle<()>) {
    let log: Log = Arc::default();
    let sink = Arc::clone(&log);
    let feed = manager.progress_feed(PROGRESS_INTERVAL);
    let task = tokio::spawn(feed.run(move |tick| {
        sink.lock().unwrap().push((Instant::now(), tick));
    }));
    (log, task)
}

fn reports_for(log: &Log, job: &JobHandle) -> Vec<(Instant, JobSnapshot)> {
    let id = job.id();
    log.lock()
        .unwrap()
        .iter()
        .flat_map(|(at, tick)| {
            tick.jobs
                .iter()
                .filter(move |s| s.id == id)
                .map(move |s| (*at, s.clone()))
        })
        .collect()
}

#[test]
fn the_default_interval_is_ten_hertz() {
    assert_eq!(PROGRESS_INTERVAL, Duration::from_millis(100));
}

#[tokio::test(start_paused = true)]
async fn a_job_is_reported_at_most_ten_times_a_second() {
    let manager = manager();
    let (log, task) = record(&manager);
    // 2,000 items finishing every millisecond: 2 virtual seconds, a change on every item.
    let busy = job(&manager, "busy", 2000, 1);
    busy.wait().await;
    sleep(Duration::from_millis(250)).await;
    task.abort();

    let reports = reports_for(&log, &busy);
    assert!(
        reports.len() >= 15,
        "progress should be reported along the way, got {}",
        reports.len()
    );
    assert!(
        reports.len() <= 25,
        "2,000 changes in about 2 s must collapse to about 20 reports, got {}",
        reports.len()
    );
    for pair in reports.windows(2) {
        let gap = pair[1].0 - pair[0].0;
        assert!(
            gap >= PROGRESS_INTERVAL,
            "two reports {gap:?} apart exceed 10 Hz"
        );
    }
    // Any one-second window holds at most ten reports.
    for (i, (start, _)) in reports.iter().enumerate() {
        let in_window = reports[i..]
            .iter()
            .take_while(|(at, _)| *at - *start < Duration::from_secs(1))
            .count();
        assert!(in_window <= 10, "{in_window} reports within one second");
    }
}

#[tokio::test(start_paused = true)]
async fn every_job_is_throttled_on_its_own() {
    let manager = manager();
    let (log, task) = record(&manager);
    let a = job(&manager, "a", 1000, 1);
    let b = job(&manager, "b", 1000, 1);
    a.wait().await;
    b.wait().await;
    sleep(Duration::from_millis(250)).await;
    task.abort();

    for handle in [&a, &b] {
        let reports = reports_for(&log, handle);
        assert!(reports.len() >= 8, "each job is reported");
        for pair in reports.windows(2) {
            assert!(pair[1].0 - pair[0].0 >= PROGRESS_INTERVAL);
        }
    }
    // Both jobs ride in the same report, so a tick is one message however many jobs run.
    let shared = log
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, tick)| tick.jobs.len() == 2)
        .count();
    assert!(shared > 0, "changes of several jobs are batched per tick");
}

#[tokio::test(start_paused = true)]
async fn the_final_state_is_reported_once_and_then_the_feed_goes_quiet() {
    let manager = manager();
    let (log, task) = record(&manager);
    let short = job(&manager, "short", 3, 40);
    short.wait().await;
    sleep(Duration::from_secs(2)).await;
    task.abort();

    let reports = reports_for(&log, &short);
    let last = &reports.last().expect("reported").1;
    assert_eq!(last.state, JobState::Completed);
    assert_eq!(last.done, 3);
    assert_eq!(
        reports
            .iter()
            .filter(|(_, s)| s.state == JobState::Completed)
            .count(),
        1,
        "the final state is reported exactly once"
    );
    let delivered = log.lock().unwrap().len();
    sleep(Duration::from_secs(5)).await;
    assert_eq!(
        log.lock().unwrap().len(),
        delivered,
        "an idle manager produces no reports"
    );
}

#[tokio::test(start_paused = true)]
async fn poll_reports_only_what_changed() {
    let manager = manager();
    let mut feed = manager.progress_feed(PROGRESS_INTERVAL);
    assert!(feed.poll().is_empty(), "no jobs, nothing to report");

    let slow = job(&manager, "slow", 4, 1000);
    let first = feed.poll();
    assert_eq!(first.jobs.len(), 1);
    assert_eq!(first.jobs[0].id, slow.id());
    assert!(
        feed.poll().is_empty(),
        "same progress is not reported again"
    );

    sleep(Duration::from_millis(1100)).await;
    let next = feed.poll();
    assert_eq!(next.jobs.len(), 1);
    assert_eq!(next.jobs[0].done, 1);
}

#[tokio::test(start_paused = true)]
async fn forgotten_jobs_are_dropped_from_tracking() {
    let manager = manager();
    let mut feed = manager.progress_feed(PROGRESS_INTERVAL);
    let quick = job(&manager, "quick", 1, 1);
    quick.wait().await;
    assert_eq!(feed.poll().jobs.len(), 1);
    assert_eq!(manager.forget_finished(), 1);
    let tick = feed.poll();
    assert!(tick.jobs.is_empty());
    assert_eq!(tick.overall.kind, OverallKind::Idle);
}

fn snapshot(id: u64, state: JobState, done: usize, total: usize) -> JobSnapshot {
    JobSnapshot {
        id: cv_jobs::JobId(id),
        name: format!("job {id}"),
        lane: LaneId::Disk,
        state,
        priority: cv_jobs::Priority::Normal,
        total,
        done,
        failed: 0,
        rolled_back: 0,
        current: Vec::new(),
        throughput_per_sec: None,
        eta_secs: None,
        elapsed_secs: 0.0,
        outcome: None,
        result: None,
    }
}

#[test]
fn overall_is_idle_without_active_jobs() {
    let mut tracker = OverallTracker::default();
    assert_eq!(tracker.update(&[]), Overall::IDLE);
    let finished = [snapshot(1, JobState::Completed, 5, 5)];
    assert_eq!(tracker.update(&finished).kind, OverallKind::Idle);
}

#[test]
fn overall_sums_done_over_total_across_jobs() {
    let mut tracker = OverallTracker::default();
    let overall = tracker.update(&[
        snapshot(1, JobState::Running, 10, 20),
        snapshot(2, JobState::Queued, 0, 20),
    ]);
    assert_eq!(overall.kind, OverallKind::Normal);
    assert_eq!(overall.active_jobs, 2);
    assert_eq!(overall.percent(), 25);
}

#[test]
fn overall_never_moves_back_when_a_job_finishes() {
    let mut tracker = OverallTracker::default();
    tracker.update(&[
        snapshot(1, JobState::Running, 10, 20),
        snapshot(2, JobState::Running, 10, 20),
    ]);
    // Job 1 completes; its items still count towards the batch.
    let overall = tracker.update(&[
        snapshot(1, JobState::Completed, 20, 20),
        snapshot(2, JobState::Running, 10, 20),
    ]);
    assert_eq!(overall.percent(), 75);
    assert_eq!(overall.active_jobs, 1);
}

#[test]
fn a_new_batch_starts_from_zero_after_everything_finished() {
    let mut tracker = OverallTracker::default();
    tracker.update(&[snapshot(1, JobState::Running, 5, 10)]);
    tracker.update(&[snapshot(1, JobState::Completed, 10, 10)]);
    let overall = tracker.update(&[
        snapshot(1, JobState::Completed, 10, 10),
        snapshot(2, JobState::Running, 1, 10),
    ]);
    assert_eq!(overall.percent(), 10, "the finished batch no longer counts");
}

#[test]
fn overall_is_paused_only_when_every_active_job_is_paused() {
    let mut tracker = OverallTracker::default();
    let mixed = tracker.update(&[
        snapshot(1, JobState::Paused, 5, 10),
        snapshot(2, JobState::Running, 5, 10),
    ]);
    assert_eq!(mixed.kind, OverallKind::Normal);
    let paused = tracker.update(&[
        snapshot(1, JobState::Paused, 5, 10),
        snapshot(2, JobState::Paused, 5, 10),
    ]);
    assert_eq!(paused.kind, OverallKind::Paused);
    assert_eq!(paused.percent(), 50);
}

#[test]
fn overall_is_indeterminate_until_the_size_is_known() {
    let mut tracker = OverallTracker::default();
    let overall = tracker.update(&[snapshot(1, JobState::Running, 0, 0)]);
    assert_eq!(overall.kind, OverallKind::Indeterminate);
}

#[tokio::test(start_paused = true)]
async fn the_manager_keeps_only_the_newest_finished_jobs() {
    let manager = manager();
    let total = cv_jobs::KEEP_FINISHED_JOBS + 20;
    let mut first = None;
    for n in 0..total {
        let handle = job(&manager, &format!("job {n}"), 1, 1);
        handle.wait().await;
        first.get_or_insert(handle);
    }
    // One more submission prunes the history down to the limit (plus the new job).
    let last = job(&manager, "last", 1, 1);
    let listed = manager.list();
    assert_eq!(listed.len(), cv_jobs::KEEP_FINISHED_JOBS + 1);
    assert!(listed.iter().all(|s| s.name != "job 0"), "the oldest went");
    assert!(listed.iter().any(|s| s.id == last.id()));
    // A handle to a dropped job still works.
    let first = first.unwrap();
    assert_eq!(first.wait().await.snapshot.state, JobState::Completed);
    last.wait().await;
}

#[tokio::test(start_paused = true)]
async fn a_finished_job_is_reported_once_and_then_not_read_again() {
    let manager = manager();
    let mut feed = manager.progress_feed(PROGRESS_INTERVAL);
    let quick = job(&manager, "quick", 1, 1);
    quick.wait().await;
    let tick = feed.poll();
    assert_eq!(tick.jobs.len(), 1);
    assert_eq!(tick.jobs[0].state, JobState::Completed);
    for _ in 0..3 {
        let tick = feed.poll();
        assert!(tick.jobs.is_empty(), "{tick:?}");
        assert_eq!(tick.overall.kind, OverallKind::Idle);
    }
}

#[tokio::test(start_paused = true)]
async fn an_idle_feed_sleeps_until_a_job_is_submitted() {
    let manager = manager();
    let (log, task) = record(&manager);
    sleep(Duration::from_secs(5)).await;
    assert!(
        log.lock().unwrap().is_empty(),
        "nothing to report while idle"
    );

    let started = Instant::now();
    let handle = job(&manager, "late", 3, 50);
    handle.wait().await;
    sleep(Duration::from_millis(300)).await;
    let reports = reports_for(&log, &handle);
    assert!(!reports.is_empty(), "the new job woke the feed");
    assert!(reports[0].0 - started <= PROGRESS_INTERVAL * 2);
    assert_eq!(reports.last().unwrap().1.state, JobState::Completed);
    let count = log.lock().unwrap().len();
    sleep(Duration::from_secs(5)).await;
    assert_eq!(
        log.lock().unwrap().len(),
        count,
        "idle again: no more reports"
    );
    task.abort();
}
