//! Job identity, states, priorities and the progress model (done/total, throughput, ETA).

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use tokio::time::{Duration, Instant};

use crate::lane::LaneId;

/// Identifies one job within a [`crate::JobManager`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct JobId(pub u64);

impl fmt::Display for JobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "job-{}", self.0)
    }
}

/// Lifecycle state of a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    /// Waiting for a job slot.
    Queued,
    /// Processing items.
    Running,
    /// No new item starts until resumed (by the user, or because everything is paused).
    Paused,
    /// Every item was processed. Individual item failures do not stop a job.
    Completed,
    /// The job could not do its work: every item failed, or the worker panicked.
    Failed,
    /// Cancelled before every item was processed.
    Cancelled,
}

impl JobState {
    /// True once the job is over and will not change again.
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

/// Scheduling priority of a job. Higher priorities start first when job slots are scarce.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    /// Background housekeeping.
    Low,
    /// The default.
    Normal,
    /// Raised by the user; starts before everything else that is waiting.
    High,
}

/// How one item ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ItemStatus {
    /// The item was fully processed.
    Done,
    /// The item failed; the job went on.
    Failed {
        /// Groupable reason, such as `rate limit`.
        reason: String,
    },
    /// The item was interrupted by a cancel and its partial work was rolled back.
    RolledBack,
}

/// What happened to one item that was started. Items never started are not recorded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemRecord {
    /// Position of the item in the job's item list.
    pub index: usize,
    /// Display label of the item.
    pub label: String,
    /// Outcome.
    pub status: ItemStatus,
}

/// Point-in-time view of a job for the Activity drawer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobSnapshot {
    /// The job.
    pub id: JobId,
    /// Display name.
    pub name: String,
    /// Lane its items run on.
    pub lane: LaneId,
    /// The browser session that started it, empty for the desktop and background work.
    #[serde(default)]
    pub started_by: String,
    /// Current state.
    pub state: JobState,
    /// Scheduling priority.
    pub priority: Priority,
    /// Number of items.
    pub total: usize,
    /// Items processed so far, successfully or not.
    pub done: usize,
    /// Items that failed (a subset of `done`).
    pub failed: usize,
    /// Items rolled back by a cancel.
    pub rolled_back: usize,
    /// Labels of the items being processed right now.
    pub current: Vec<String>,
    /// Items per second over the time the job actually ran (pauses excluded).
    pub throughput_per_sec: Option<f64>,
    /// Estimated seconds remaining, once at least one item is done.
    pub eta_secs: Option<f64>,
    /// Seconds the job has actually run (pauses excluded).
    pub elapsed_secs: f64,
    /// Once the job is over, what it did beyond its counts, in a short phrase from the work
    /// itself ([`crate::ItemWork::outcome`]), such as `6 titles wait in Needs review`.
    #[serde(default)]
    pub outcome: Option<String>,
    /// Once the job is over, what it produced, as its step in the run history
    /// ([`crate::ItemWork::result`]), such as `212 new` or `198 scraped`.
    #[serde(default)]
    pub result: Option<String>,
}

impl JobSnapshot {
    /// Items finished without failure.
    pub fn succeeded(&self) -> usize {
        self.done - self.failed
    }

    /// Fraction done in `0.0..=1.0` (1.0 for an empty job).
    pub fn fraction(&self) -> f64 {
        if self.total == 0 {
            1.0
        } else {
            self.done as f64 / self.total as f64
        }
    }
}

/// Final account of a job: its last snapshot and the record of every started item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobReport {
    /// The final snapshot.
    pub snapshot: JobSnapshot,
    /// Started items in item order. Items that were never started are absent, so after a
    /// cancel this is exactly the set the rename journal holds.
    pub items: Vec<ItemRecord>,
}

impl JobReport {
    /// The failed items grouped by reason.
    pub fn failure_groups(&self) -> Vec<FailureGroup> {
        group_failures(&self.items)
    }
}

/// One failed item inside a [`FailureGroup`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailedItem {
    /// Position of the item in the job's item list.
    pub index: usize,
    /// Display label of the item.
    pub label: String,
}

/// The failed items of a job that share one reason, as the Activity drawer and the notification
/// centre show them: "3 failed · rate limit".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailureGroup {
    /// The shared reason, such as `rate limit`.
    pub reason: String,
    /// The failed items in item order.
    pub items: Vec<FailedItem>,
}

impl FailureGroup {
    /// Number of failed items in the group.
    pub fn count(&self) -> usize {
        self.items.len()
    }

    /// The drawer line without the action, such as `3 failed · rate limit`.
    pub fn summary(&self) -> String {
        format!("{} failed · {}", self.count(), self.reason)
    }
}

/// Groups failed items by reason. Biggest groups come first, ties by reason; items inside a
/// group are in item order. Items that did not fail are ignored.
pub fn group_failures<'a>(records: impl IntoIterator<Item = &'a ItemRecord>) -> Vec<FailureGroup> {
    let mut groups: BTreeMap<&str, Vec<FailedItem>> = BTreeMap::new();
    for record in records {
        if let ItemStatus::Failed { reason } = &record.status {
            groups.entry(reason).or_default().push(FailedItem {
                index: record.index,
                label: record.label.clone(),
            });
        }
    }
    let mut groups: Vec<FailureGroup> = groups
        .into_iter()
        .map(|(reason, mut items)| {
            items.sort_by_key(|i| i.index);
            FailureGroup {
                reason: reason.to_owned(),
                items,
            }
        })
        .collect();
    groups.sort_by(|a, b| {
        b.count()
            .cmp(&a.count())
            .then_with(|| a.reason.cmp(&b.reason))
    });
    groups
}

/// Mutable progress bookkeeping of one job. Not synchronised; the owner locks it.
#[derive(Debug)]
pub(crate) struct Tracker {
    pub total: usize,
    pub done: usize,
    pub failed: usize,
    pub rolled_back: usize,
    pub current: BTreeMap<usize, String>,
    pub records: Vec<ItemRecord>,
    /// Time spent running, pauses excluded, up to `since`.
    active: Duration,
    /// Set while the clock ticks: the job is running, or paused with items still finishing.
    since: Option<Instant>,
    /// Items that finished in this run (a resumed job's earlier items not counted), and the
    /// running time when the last of them did: what the estimate is made from.
    finished_here: usize,
    finished_at: Duration,
}

impl Tracker {
    /// Counts an item that finished (done or failed).
    pub fn note_done(&mut self) {
        self.done += 1;
        self.finished_here += 1;
        self.finished_at = self.elapsed();
    }
    /// A tracker for a job that starts with `done` of `total` items already finished (`failed`
    /// of them failed), with their records. Throughput and ETA only count the time since.
    pub fn resumed(total: usize, done: usize, failed: usize, records: Vec<ItemRecord>) -> Self {
        Self {
            total,
            done,
            failed,
            rolled_back: 0,
            current: BTreeMap::new(),
            records,
            active: Duration::ZERO,
            since: None,
            finished_here: 0,
            finished_at: Duration::ZERO,
        }
    }

    /// Folds elapsed running time into `active` and restarts or stops the clock according to
    /// whether the job `running` now. Call before and after every state change.
    pub fn sync_clock(&mut self, running: bool) {
        let now = Instant::now();
        if let Some(since) = self.since.take() {
            self.active += now.saturating_duration_since(since);
        }
        if running {
            self.since = Some(now);
        }
    }

    pub fn elapsed(&self) -> Duration {
        match self.since {
            Some(since) => self.active + Instant::now().saturating_duration_since(since),
            None => self.active,
        }
    }

    pub fn throughput(&self) -> Option<f64> {
        let secs = self.elapsed().as_secs_f64();
        (self.done > 0 && secs > 0.0).then(|| self.done as f64 / secs)
    }

    /// The time left: the items still to do at the average time an item took in this run, less
    /// the time spent since the last one finished, so the estimate counts down while a long item
    /// runs instead of standing still. Never below zero.
    pub fn eta_secs(&self) -> Option<f64> {
        if self.finished_here == 0 {
            return None;
        }
        let per_item = self.finished_at.as_secs_f64() / self.finished_here as f64;
        let left = self.total.saturating_sub(self.done) as f64 * per_item;
        let since_last = self
            .elapsed()
            .saturating_sub(self.finished_at)
            .as_secs_f64();
        Some((left - since_last).max(0.0))
    }
}
