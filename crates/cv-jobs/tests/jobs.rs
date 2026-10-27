//! Job control: states, progress with ETA, cancellation (bounded, per-item completion or
//! rollback), pause/resume and priority.
//!
//! Every test runs on tokio's paused clock, so timings are virtual and exact: nothing here
//! depends on how loaded the machine is.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use cv_jobs::{
    ItemControl, ItemError, ItemStatus, ItemWork, JobHandle, JobManager, JobSpec, JobState,
    LaneConfig, LaneId, Lanes, ManagerConfig, Priority, StageError,
};
use tokio::time::{Instant, sleep};

const HOUR: Duration = Duration::from_secs(3600);

/// A configurable worker that records everything it does.
struct Probe {
    delay: Duration,
    /// Item that ignores cancellation and never finishes on its own.
    stubborn: Option<u32>,
    /// Item that watches the cancel signal and returns `Cancelled`.
    cooperative: Option<u32>,
    fail: fn(u32) -> Option<&'static str>,
    started: Mutex<Vec<u32>>,
    finished: Mutex<Vec<u32>>,
    /// Items that began but neither finished nor were rolled back.
    partial: Mutex<Vec<u32>>,
    rolled_back: Mutex<Vec<u32>>,
}

impl Probe {
    fn new(delay_ms: u64) -> Self {
        Self {
            delay: Duration::from_millis(delay_ms),
            stubborn: None,
            cooperative: None,
            fail: |_| None,
            started: Mutex::default(),
            finished: Mutex::default(),
            partial: Mutex::default(),
            rolled_back: Mutex::default(),
        }
    }

    fn started(&self) -> Vec<u32> {
        self.started.lock().unwrap().clone()
    }
    fn finished(&self) -> Vec<u32> {
        self.finished.lock().unwrap().clone()
    }
    fn partial(&self) -> Vec<u32> {
        self.partial.lock().unwrap().clone()
    }
    fn rolled_back(&self) -> Vec<u32> {
        self.rolled_back.lock().unwrap().clone()
    }
}

/// Newtype so the test can keep a reference to the probe it hands to the manager.
struct Work(Arc<Probe>);

impl ItemWork for Work {
    type Item = u32;

    async fn process(&self, item: &u32, control: &ItemControl) -> Result<(), ItemError> {
        let p = &self.0;
        p.started.lock().unwrap().push(*item);
        p.partial.lock().unwrap().push(*item);
        if p.stubborn == Some(*item) {
            sleep(HOUR).await;
        } else if p.cooperative == Some(*item) {
            tokio::select! {
                () = sleep(HOUR) => {}
                () = control.cancelled() => return Err(ItemError::Cancelled),
            }
        } else {
            sleep(p.delay).await;
        }
        p.partial.lock().unwrap().retain(|i| i != item);
        if let Some(reason) = (p.fail)(*item) {
            return Err(StageError::new(reason).into());
        }
        p.finished.lock().unwrap().push(*item);
        Ok(())
    }

    async fn rollback(&self, item: &u32) {
        self.0.partial.lock().unwrap().retain(|i| i != item);
        self.0.rolled_back.lock().unwrap().push(*item);
    }
}

fn manager(max_running_jobs: usize) -> JobManager {
    JobManager::new(
        Lanes::new(LaneConfig::default()),
        ManagerConfig {
            max_running_jobs,
            ..ManagerConfig::default()
        },
    )
}

fn disk_job(name: &str) -> JobSpec {
    JobSpec::new(name, LaneId::Disk)
}

/// Waits until `check` holds for the job's snapshot.
async fn until(job: &JobHandle, check: impl Fn(&cv_jobs::JobSnapshot) -> bool) {
    let mut rx = job.subscribe();
    rx.wait_for(|s| check(s)).await.expect("job alive");
}

fn near(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "expected about {expected}, got {actual}"
    );
}

#[tokio::test(start_paused = true)]
async fn reports_state_progress_current_item_throughput_and_eta() {
    let probe = Arc::new(Probe::new(100));
    let job = manager(4).submit(disk_job("media info"), Work(probe), (0..10).collect());

    let first = job.snapshot();
    assert_eq!(first.total, 10);
    assert!(matches!(first.state, JobState::Queued | JobState::Running));

    until(&job, |s| s.done == 4).await;
    let s = job.snapshot();
    assert_eq!(s.state, JobState::Running);
    assert_eq!((s.done, s.total, s.failed), (4, 10, 0));
    near(s.fraction(), 0.4, 1e-9);
    near(s.throughput_per_sec.expect("throughput"), 10.0, 0.5);
    near(s.eta_secs.expect("eta"), 0.6, 0.05);

    let report = job.wait().await;
    let s = report.snapshot;
    assert_eq!(s.state, JobState::Completed);
    assert_eq!((s.done, s.total), (10, 10));
    assert!(s.current.is_empty());
    assert_eq!(s.eta_secs, None);
    assert_eq!(report.items.len(), 10);
    assert!(report.items.iter().all(|r| r.status == ItemStatus::Done));
}

/// While the last item takes long, the estimate counts down to zero instead of
/// standing at "~1 s".
#[tokio::test(start_paused = true)]
async fn the_estimate_counts_down_while_an_item_takes_long() {
    let mut probe = Probe::new(100);
    probe.stubborn = Some(3);
    let probe = Arc::new(probe);
    let job = manager(1).submit(disk_job("scan"), Work(Arc::clone(&probe)), (0..4).collect());
    until(&job, |s| s.done == 3).await;
    near(job.snapshot().eta_secs.expect("eta"), 0.1, 0.02);
    sleep(Duration::from_millis(60)).await;
    near(job.snapshot().eta_secs.expect("eta"), 0.04, 0.02);
    sleep(Duration::from_secs(1)).await;
    assert_eq!(job.snapshot().eta_secs, Some(0.0));
}

#[tokio::test(start_paused = true)]
async fn current_items_are_those_being_processed() {
    let probe = Arc::new(Probe::new(100));
    let job = manager(4).submit(
        disk_job("scan").concurrency(2),
        Work(probe),
        (0..6).collect(),
    );
    until(&job, |s| s.current.len() == 2).await;
    let mut current = job.snapshot().current;
    current.sort();
    assert_eq!(current, ["0", "1"]);
    job.wait().await;
}

#[tokio::test(start_paused = true)]
async fn item_failures_do_not_stop_the_job_and_are_counted() {
    let mut probe = Probe::new(10);
    probe.fail = |i| (i % 4 == 0).then_some("rate limit");
    let probe = Arc::new(probe);
    let job = manager(4).submit(disk_job("thumbnails"), Work(probe), (0..8).collect());
    let report = job.wait().await;

    assert_eq!(report.snapshot.state, JobState::Completed);
    assert_eq!((report.snapshot.done, report.snapshot.failed), (8, 2));
    assert_eq!(report.snapshot.succeeded(), 6);
    let failed: Vec<_> = report
        .items
        .iter()
        .filter(|r| matches!(&r.status, ItemStatus::Failed { reason } if reason == "rate limit"))
        .map(|r| r.index)
        .collect();
    assert_eq!(failed, [0, 4]);
}

#[tokio::test(start_paused = true)]
async fn job_fails_only_when_every_item_fails() {
    let mut probe = Probe::new(10);
    probe.fail = |_| Some("no match");
    let job = manager(4).submit(disk_job("match"), Work(Arc::new(probe)), (0..3).collect());
    let report = job.wait().await;
    assert_eq!(report.snapshot.state, JobState::Failed);
    assert_eq!(report.snapshot.failed, 3);
}

#[tokio::test(start_paused = true)]
async fn empty_job_completes() {
    let job = manager(4).submit(disk_job("empty"), Work(Arc::new(Probe::new(1))), vec![]);
    let report = job.wait().await;
    assert_eq!(report.snapshot.state, JobState::Completed);
    near(report.snapshot.fraction(), 1.0, 1e-9);
}

// ---- cancellation ---------------------------------------------------------------------------

/// The spec scenario: cancel an organize batch while item 30 of 100 is being moved. Item 30
/// ignores the cancel signal, so it is dropped after the grace period and rolled back.
#[tokio::test(start_paused = true)]
async fn cancel_mid_batch_rolls_back_the_current_item_within_one_second() {
    let mut probe = Probe::new(10);
    probe.stubborn = Some(29); // item 30 of 100
    let probe = Arc::new(probe);
    let job = manager(4).submit(
        disk_job("organize"),
        Work(Arc::clone(&probe)),
        (0..100).collect(),
    );

    until(&job, |s| s.current == ["29"]).await;
    let before = Instant::now();
    assert!(job.cancel());
    let report = job.wait().await;
    let took = before.elapsed();

    assert!(took <= Duration::from_secs(1), "cancel took {took:?}");
    assert_eq!(report.snapshot.state, JobState::Cancelled);

    // The journal holds items 1-30: 29 completed and item 30 rolled back.
    let indices: Vec<usize> = report.items.iter().map(|r| r.index).collect();
    assert_eq!(indices, (0..30).collect::<Vec<_>>());
    assert!(
        report.items[..29]
            .iter()
            .all(|r| r.status == ItemStatus::Done)
    );
    assert_eq!(report.items[29].status, ItemStatus::RolledBack);
    assert_eq!(report.snapshot.rolled_back, 1);
    assert_eq!(report.snapshot.done, 29);

    // Items 31-100 were never touched, and nothing is left half-processed.
    assert_eq!(probe.started(), (0..30).collect::<Vec<_>>());
    assert_eq!(probe.rolled_back(), [29]);
    assert!(probe.partial().is_empty());
}

#[tokio::test(start_paused = true)]
async fn cooperative_item_rolls_back_at_once() {
    let mut probe = Probe::new(10);
    probe.cooperative = Some(5);
    let probe = Arc::new(probe);
    let job = manager(4).submit(
        disk_job("organize"),
        Work(Arc::clone(&probe)),
        (0..20).collect(),
    );
    until(&job, |s| s.current == ["5"]).await;
    let before = Instant::now();
    job.cancel();
    let report = job.wait().await;

    assert!(before.elapsed() < Duration::from_millis(100));
    assert_eq!(report.snapshot.state, JobState::Cancelled);
    assert_eq!(report.items.len(), 6);
    assert_eq!(report.items[5].status, ItemStatus::RolledBack);
    assert_eq!(probe.rolled_back(), [5]);
    assert!(probe.partial().is_empty());
}

#[tokio::test(start_paused = true)]
async fn item_that_finishes_within_the_grace_period_completes() {
    let probe = Arc::new(Probe::new(200)); // shorter than the 500 ms grace
    let job = manager(4).submit(
        disk_job("organize"),
        Work(Arc::clone(&probe)),
        (0..10).collect(),
    );
    until(&job, |s| s.current == ["3"]).await;
    job.cancel();
    let report = job.wait().await;

    assert_eq!(report.snapshot.state, JobState::Cancelled);
    assert_eq!(report.items.len(), 4);
    assert!(report.items.iter().all(|r| r.status == ItemStatus::Done));
    assert!(probe.rolled_back().is_empty());
    assert_eq!(probe.started(), [0, 1, 2, 3]);
}

#[tokio::test(start_paused = true)]
async fn cancel_does_not_start_items_waiting_for_a_lane_worker() {
    // One disk worker, four items in flight at once: only item 0 holds the worker.
    let lanes = Lanes::new(LaneConfig {
        disk: 1,
        ..LaneConfig::default()
    });
    let manager = JobManager::new(lanes, ManagerConfig::default());
    let probe = Arc::new(Probe::new(50));
    let job = manager.submit(
        disk_job("copy").concurrency(4),
        Work(Arc::clone(&probe)),
        (0..10).collect(),
    );
    until(&job, |s| s.current == ["0"]).await;
    job.cancel();
    let report = job.wait().await;

    assert_eq!(report.snapshot.state, JobState::Cancelled);
    assert_eq!(probe.started(), [0]);
    assert_eq!(report.items.len(), 1);
}

#[tokio::test(start_paused = true)]
async fn cancelling_a_queued_job_never_runs_it() {
    let mgr = manager(1);
    let blocker = mgr.submit(
        disk_job("blocker"),
        Work(Arc::new(Probe::new(100))),
        (0..5).collect(),
    );
    let probe = Arc::new(Probe::new(10));
    let queued = mgr.submit(
        disk_job("queued"),
        Work(Arc::clone(&probe)),
        (0..5).collect(),
    );
    assert_eq!(queued.snapshot().state, JobState::Queued);

    assert!(queued.cancel());
    let report = queued.wait().await;
    assert_eq!(report.snapshot.state, JobState::Cancelled);
    assert!(report.items.is_empty());
    assert!(probe.started().is_empty());
    // The cancelled job gave nothing up: the blocker still finishes normally.
    assert_eq!(blocker.wait().await.snapshot.state, JobState::Completed);
}

#[tokio::test(start_paused = true)]
async fn control_calls_on_a_finished_job_are_refused() {
    let job = manager(4).submit(disk_job("done"), Work(Arc::new(Probe::new(1))), vec![1]);
    job.wait().await;
    assert!(!job.cancel());
    assert!(!job.pause());
    assert!(!job.set_priority(Priority::High));
}

// ---- pause and resume -----------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn pause_stops_new_items_and_resume_continues() {
    let probe = Arc::new(Probe::new(100));
    let job = manager(4).submit(
        disk_job("artwork"),
        Work(Arc::clone(&probe)),
        (0..10).collect(),
    );

    until(&job, |s| s.done == 2).await;
    assert!(job.pause());
    assert_eq!(job.snapshot().state, JobState::Paused);

    // Let the item that was already running finish, then stay paused for an hour.
    sleep(HOUR).await;
    let paused = job.snapshot();
    assert_eq!(paused.state, JobState::Paused);
    assert_eq!(
        paused.done, 3,
        "only the in-flight item may finish while paused"
    );
    assert_eq!(probe.started().len(), 3);
    assert!(paused.current.is_empty());
    // The hour spent paused is not counted as running time.
    assert!(paused.elapsed_secs < 1.0, "elapsed {}", paused.elapsed_secs);
    near(paused.throughput_per_sec.expect("throughput"), 10.0, 1.0);

    assert!(job.resume());
    let report = job.wait().await;
    assert_eq!(report.snapshot.state, JobState::Completed);
    assert_eq!(report.snapshot.done, 10);
    assert_eq!(probe.finished(), (0..10).collect::<Vec<_>>());
    assert!(report.snapshot.elapsed_secs < 1.5);
}

#[tokio::test(start_paused = true)]
async fn pause_all_pauses_running_queued_and_new_jobs() {
    let mgr = manager(4);
    let a = Arc::new(Probe::new(100));
    let b = Arc::new(Probe::new(100));
    let job_a = mgr.submit(disk_job("a"), Work(Arc::clone(&a)), (0..10).collect());
    until(&job_a, |s| s.done == 1).await;

    mgr.pause_all();
    assert!(mgr.is_all_paused());
    let job_b = mgr.submit(disk_job("b"), Work(Arc::clone(&b)), (0..10).collect());
    assert_eq!(job_b.snapshot().state, JobState::Paused);

    sleep(HOUR).await;
    assert_eq!(job_a.snapshot().state, JobState::Paused);
    assert_eq!(job_a.snapshot().done, 2);
    assert!(b.started().is_empty());

    // An individual pause survives resume_all.
    job_b.pause();
    mgr.resume_all();
    assert!(!mgr.is_all_paused());
    assert_eq!(job_a.wait().await.snapshot.state, JobState::Completed);
    sleep(HOUR).await;
    assert_eq!(job_b.snapshot().state, JobState::Paused);
    assert!(b.started().is_empty());

    job_b.resume();
    assert_eq!(job_b.wait().await.snapshot.state, JobState::Completed);
}

#[tokio::test(start_paused = true)]
async fn paused_job_does_not_hold_a_job_slot() {
    let mgr = manager(1);
    let a = mgr.submit(
        disk_job("a"),
        Work(Arc::new(Probe::new(100))),
        (0..10).collect(),
    );
    let probe_b = Arc::new(Probe::new(10));
    let b = mgr.submit(disk_job("b"), Work(Arc::clone(&probe_b)), (0..3).collect());
    assert_eq!(b.snapshot().state, JobState::Queued);

    until(&a, |s| s.done == 1).await;
    a.pause();
    // The freed slot lets job b run to completion while a is paused.
    assert_eq!(b.wait().await.snapshot.state, JobState::Completed);
    assert_eq!(a.snapshot().state, JobState::Paused);
    a.resume();
    assert_eq!(a.wait().await.snapshot.state, JobState::Completed);
}

#[tokio::test(start_paused = true)]
async fn pause_applies_to_items_waiting_for_a_lane_worker() {
    let lanes = Lanes::new(LaneConfig {
        disk: 1,
        ..LaneConfig::default()
    });
    let mgr = JobManager::new(lanes, ManagerConfig::default());
    let probe = Arc::new(Probe::new(100));
    let job = mgr.submit(
        disk_job("copy").concurrency(4),
        Work(Arc::clone(&probe)),
        (0..8).collect(),
    );
    until(&job, |s| s.current == ["0"]).await;
    job.pause();
    sleep(HOUR).await;
    // Items 1-3 were queued behind the busy worker; none of them started after the pause.
    assert_eq!(probe.started(), [0]);
    job.resume();
    assert_eq!(job.wait().await.snapshot.state, JobState::Completed);
    assert_eq!(probe.finished().len(), 8);
}

// ---- priority -------------------------------------------------------------------------------

/// Records the order in which jobs start.
struct Mark {
    name: &'static str,
    order: Arc<Mutex<Vec<&'static str>>>,
}

impl ItemWork for Mark {
    type Item = u32;

    async fn process(&self, _item: &u32, _control: &ItemControl) -> Result<(), ItemError> {
        self.order.lock().unwrap().push(self.name);
        sleep(Duration::from_millis(10)).await;
        Ok(())
    }
}

#[tokio::test(start_paused = true)]
async fn waiting_jobs_start_by_priority_then_submission_order() {
    let mgr = manager(1);
    let order = Arc::new(Mutex::new(Vec::new()));
    let submit = |name: &'static str, priority: Priority| {
        mgr.submit(
            disk_job(name).priority(priority),
            Mark {
                name,
                order: Arc::clone(&order),
            },
            vec![0],
        )
    };

    // "first" takes the only slot; the rest wait.
    let first = submit("first", Priority::Normal);
    let low = submit("low", Priority::Low);
    let normal_a = submit("normal-a", Priority::Normal);
    let late = submit("late", Priority::Low);
    let normal_b = submit("normal-b", Priority::Normal);
    // Raising a waiting job moves it to the front.
    assert!(late.set_priority(Priority::High));
    assert_eq!(late.snapshot().priority, Priority::High);

    for job in [&first, &low, &normal_a, &late, &normal_b] {
        job.wait().await;
    }
    assert_eq!(
        *order.lock().unwrap(),
        ["first", "late", "normal-a", "normal-b", "low"]
    );
}

#[tokio::test(start_paused = true)]
async fn job_snapshots_list_in_submission_order_and_can_be_forgotten() {
    let mgr = manager(4);
    let a = mgr.submit(disk_job("a"), Work(Arc::new(Probe::new(10))), vec![1]);
    let b = mgr.submit(disk_job("b"), Work(Arc::new(Probe::new(10))), vec![1]);
    let names: Vec<_> = mgr.list().into_iter().map(|s| s.name).collect();
    assert_eq!(names, ["a", "b"]);
    assert!(mgr.get(a.id()).is_some());

    a.wait().await;
    b.wait().await;
    assert_eq!(mgr.forget_finished(), 2);
    assert!(mgr.list().is_empty());
    assert!(mgr.get(a.id()).is_none());
    assert!(!mgr.cancel(a.id()));
}

/// A worker that says what its items came to once the job is over.
struct Reviewed;

impl ItemWork for Reviewed {
    type Item = u32;

    async fn process(&self, _item: &u32, _control: &ItemControl) -> Result<(), ItemError> {
        Ok(())
    }

    fn outcome(&self) -> Option<String> {
        Some("3 titles wait in Needs review".to_owned())
    }
}

#[tokio::test(start_paused = true)]
async fn a_finished_job_carries_the_outcome_its_work_reports() {
    let job = manager(1).submit(disk_job("match"), Reviewed, (0..3).collect());
    assert_eq!(job.snapshot().outcome, None);
    let report = job.wait().await;
    assert_eq!(report.snapshot.state, JobState::Completed);
    assert_eq!(
        report.snapshot.outcome.as_deref(),
        Some("3 titles wait in Needs review")
    );
    // Work that says nothing leaves it empty.
    let plain = manager(1).submit(disk_job("scan"), Work(Arc::new(Probe::new(1))), vec![1]);
    assert_eq!(plain.wait().await.snapshot.outcome, None);
}
