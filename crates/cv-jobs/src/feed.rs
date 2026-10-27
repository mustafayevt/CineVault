//! Throttled progress feed for the UI, the taskbar and the dock.
//!
//! A [`ProgressFeed`] watches a [`JobManager`] and reports, at most once per interval (100 ms by
//! default, so at most 10 Hz), the jobs whose progress changed since the last report, plus one
//! [`Overall`] figure that mirrors every running job to the Windows taskbar and the macOS dock.
//! Jobs that did not change cost nothing, and a burst of fast items collapses into one report.
//! Only the jobs that are not over (plus new and just-finished ones) are read on each tick, and
//! while no job is active the feed sleeps until the next one is submitted.
//!
//! The feed is a plain future ([`ProgressFeed::run`]) so the caller picks the runtime, and
//! [`ProgressFeed::poll`] is its synchronous core for tests.

use std::collections::{HashMap, HashSet};

use tokio::sync::watch;
use tokio::time::{Duration, MissedTickBehavior, interval};

use crate::manager::JobManager;
use crate::progress::{JobId, JobSnapshot, JobState};

/// The shortest time between two reports: 100 ms, so at most 10 Hz per job.
pub const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

/// What the taskbar or dock should show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverallKind {
    /// No job is active: show nothing.
    Idle,
    /// Jobs are active but their size is not known yet.
    Indeterminate,
    /// Jobs are running; show the fraction.
    Normal,
    /// Every active job is paused; show the fraction in the paused style.
    Paused,
}

/// Combined progress of all active jobs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Overall {
    /// What to show.
    pub kind: OverallKind,
    /// Items done over items in total across the current batch, in `0.0..=1.0`.
    pub fraction: f64,
    /// Jobs that are queued, running or paused.
    pub active_jobs: usize,
}

impl Overall {
    /// Nothing is happening.
    pub const IDLE: Self = Self {
        kind: OverallKind::Idle,
        fraction: 0.0,
        active_jobs: 0,
    };

    /// The fraction as a whole percentage in `0..=100`.
    pub fn percent(&self) -> u8 {
        (self.fraction.clamp(0.0, 1.0) * 100.0).round() as u8
    }
}

/// Folds job snapshots into one [`Overall`]. A *batch* starts when the first job becomes active
/// and ends when none is, so jobs that finish early keep counting and the bar never moves back.
#[derive(Debug, Default)]
pub struct OverallTracker {
    batch: HashSet<JobId>,
}

impl OverallTracker {
    /// Computes the combined progress for the current `snapshots`.
    pub fn update(&mut self, snapshots: &[JobSnapshot]) -> Overall {
        let active: Vec<&JobSnapshot> = snapshots
            .iter()
            .filter(|s| !s.state.is_terminal())
            .collect();
        if active.is_empty() {
            self.batch.clear();
            return Overall::IDLE;
        }
        self.batch.extend(active.iter().map(|s| s.id));
        let (done, total) = snapshots
            .iter()
            .filter(|s| self.batch.contains(&s.id))
            .fold((0usize, 0usize), |(d, t), s| (d + s.done, t + s.total));
        let all_paused = active.iter().all(|s| s.state == JobState::Paused);
        let kind = if total == 0 {
            OverallKind::Indeterminate
        } else if all_paused {
            OverallKind::Paused
        } else {
            OverallKind::Normal
        };
        Overall {
            kind,
            fraction: if total == 0 {
                0.0
            } else {
                done as f64 / total as f64
            },
            active_jobs: active.len(),
        }
    }
}

/// One report of a [`ProgressFeed`].
#[derive(Debug, Clone, PartialEq)]
pub struct ProgressTick {
    /// Snapshots of the jobs that changed since the previous report, in submission order.
    pub jobs: Vec<JobSnapshot>,
    /// The combined progress as of now.
    pub overall: Overall,
    /// True when `overall` differs from the previous report.
    pub overall_changed: bool,
}

impl ProgressTick {
    /// True when there is nothing new to report.
    pub fn is_empty(&self) -> bool {
        self.jobs.is_empty() && !self.overall_changed
    }
}

/// True when two snapshots show the same progress. The time-derived fields (elapsed time,
/// throughput) are ignored: they drift constantly and must not defeat the throttle. The ETA
/// counts in whole seconds, so a job whose last item takes long is reported at most once a
/// second with the time it has left, instead of keeping a stale "~1 s".
fn same_progress(a: &JobSnapshot, b: &JobSnapshot) -> bool {
    let whole = |eta: Option<f64>| eta.map(|secs| secs.ceil() as u64);
    whole(a.eta_secs) == whole(b.eta_secs)
        && a.id == b.id
        && a.state == b.state
        && a.priority == b.priority
        && a.total == b.total
        && a.done == b.done
        && a.failed == b.failed
        && a.rolled_back == b.rolled_back
        && a.current == b.current
        && a.name == b.name
}

/// Reports job progress changes at a bounded rate. Create it with
/// [`JobManager::progress_feed`].
pub struct ProgressFeed {
    manager: JobManager,
    interval: Duration,
    last: HashMap<JobId, JobSnapshot>,
    overall: Option<Overall>,
    tracker: OverallTracker,
    /// The highest job number seen so far: newer jobs are always read once.
    seen_up_to: u64,
    submissions: watch::Receiver<u64>,
}

impl ProgressFeed {
    fn new(manager: JobManager, interval: Duration) -> Self {
        let submissions = manager.submissions();
        Self {
            manager,
            interval: interval.max(Duration::from_millis(1)),
            last: HashMap::new(),
            overall: Some(Overall::IDLE),
            tracker: OverallTracker::default(),
            seen_up_to: 0,
            submissions,
        }
    }

    /// Compares the jobs with what was reported last and returns the difference. A job's final
    /// state is reported once; jobs the manager forgot are dropped silently.
    pub fn poll(&mut self) -> ProgressTick {
        // Jobs reported last time while not over, and the jobs of the current batch, are read
        // again so their last state is reported and the overall bar never moves back.
        let mut also: HashSet<JobId> = self
            .last
            .values()
            .filter(|s| !s.state.is_terminal())
            .map(|s| s.id)
            .collect();
        also.extend(self.tracker.batch.iter().copied());
        let snapshots = self.manager.list_live(self.seen_up_to, &also);
        if let Some(newest) = snapshots.iter().map(|s| s.id.0).max() {
            self.seen_up_to = self.seen_up_to.max(newest);
        }
        let mut changed = Vec::new();
        for snapshot in &snapshots {
            let unchanged = self
                .last
                .get(&snapshot.id)
                .is_some_and(|previous| same_progress(previous, snapshot));
            if !unchanged {
                self.last.insert(snapshot.id, snapshot.clone());
                changed.push(snapshot.clone());
            }
        }
        let overall = self.tracker.update(&snapshots);
        // A finished job's last state was just reported. It stays known only while it still
        // counts in the overall batch; after that it is not read again.
        let live: HashSet<JobId> = snapshots
            .iter()
            .filter(|s| !s.state.is_terminal() || self.tracker.batch.contains(&s.id))
            .map(|s| s.id)
            .collect();
        self.last.retain(|id, _| live.contains(id));
        let overall_changed = self.overall.is_none_or(|previous| {
            previous.kind != overall.kind
                || previous.active_jobs != overall.active_jobs
                || previous.percent() != overall.percent()
        });
        if overall_changed {
            self.overall = Some(overall);
        }
        ProgressTick {
            jobs: changed,
            overall,
            overall_changed,
        }
    }

    /// Polls once per interval and hands every non-empty report to `sink`, forever. Dropping
    /// the future (or aborting its task) stops the feed. Two reports are never closer together
    /// than the interval, however fast the jobs progress.
    pub async fn run(mut self, mut sink: impl FnMut(ProgressTick) + Send) {
        let mut ticker = interval(self.interval);
        // After a stall, resume the rhythm instead of reporting in a burst.
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            // Mark the submissions so far as seen before reading, so a job submitted after this
            // read wakes an idle feed below.
            self.submissions.borrow_and_update();
            let tick = self.poll();
            let idle = tick.overall.active_jobs == 0;
            if !tick.is_empty() {
                sink(tick);
            }
            if idle {
                // Nothing to watch: sleep until the next job instead of ticking 10 times a second.
                if self.submissions.changed().await.is_err() {
                    return;
                }
                ticker.reset();
            }
        }
    }
}

impl JobManager {
    /// A feed that reports this manager's job progress at most once per `interval`
    /// ([`PROGRESS_INTERVAL`] for the 10 Hz limit).
    pub fn progress_feed(&self, interval: Duration) -> ProgressFeed {
        ProgressFeed::new(self.clone(), interval)
    }
}
