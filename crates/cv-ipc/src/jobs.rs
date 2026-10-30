//! Job progress as the UI reads it, and the hub that fans it out to every open page.
//!
//! [`ProgressHub`] is transport-neutral: a page subscribes with a sink that returns `false` when
//! its transport is gone. The desktop feed polls the [`JobManager`] and publishes each report; the
//! server does the same from its own feed.

use std::sync::{Mutex, MutexGuard};

use cv_jobs::{
    JobManager, JobSnapshot, JobState, Overall, OverallKind, OverallTracker, Priority, ProgressTick,
};
use serde::Serialize;
use specta::Type;

/// Job lifecycle state as the UI sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum JobStateDto {
    Queued,
    Running,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

/// Scheduling priority as the UI sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PriorityDto {
    Low,
    Normal,
    High,
}

/// Progress of one job.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct JobProgress {
    /// Session-local job id.
    pub id: u32,
    /// Display name.
    pub name: String,
    /// Lane the job runs on, such as `disk`, `cpu` or `provider:tmdb`.
    pub lane: String,
    pub state: JobStateDto,
    pub priority: PriorityDto,
    /// Number of items.
    pub total: u32,
    /// Items processed so far, successfully or not.
    pub done: u32,
    /// Items that failed (a subset of `done`).
    pub failed: u32,
    /// Items rolled back by a cancel.
    pub rolled_back: u32,
    /// Labels of the items being processed right now.
    pub current: Vec<String>,
    /// Items per second while the job ran.
    pub throughput_per_sec: Option<f64>,
    /// Estimated seconds remaining.
    pub eta_secs: Option<f64>,
    /// Seconds the job has run, pauses excluded.
    pub elapsed_secs: f64,
    /// Once the job is over, what it did beyond its counts (`6 titles wait in Needs review`).
    pub outcome: Option<String>,
    /// Once the job is over, what it produced, as its step in the run history (`212 new`,
    /// `198 scraped`, `artwork for 190`); `None` shows the job's name.
    pub result: Option<String>,
}

/// What the combined taskbar and dock progress shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum OverallKindDto {
    Idle,
    Indeterminate,
    Normal,
    Paused,
}

/// Combined progress of all active jobs (the value mirrored to the taskbar and dock).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
pub struct OverallProgress {
    pub kind: OverallKindDto,
    /// Whole percentage in 0..=100.
    pub percent: u8,
    /// Jobs that are queued, running or paused.
    pub active_jobs: u32,
}

/// One message on the progress channel.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct JobProgressUpdate {
    /// The jobs that changed since the previous message (every job in the first message).
    pub jobs: Vec<JobProgress>,
    /// The combined progress.
    pub overall: OverallProgress,
}

fn small(n: impl TryInto<u32>) -> u32 {
    n.try_into().unwrap_or(u32::MAX)
}

impl From<&JobSnapshot> for JobProgress {
    fn from(s: &JobSnapshot) -> Self {
        Self {
            id: small(s.id.0),
            name: s.name.clone(),
            lane: s.lane.to_string(),
            state: match s.state {
                JobState::Queued => JobStateDto::Queued,
                JobState::Running => JobStateDto::Running,
                JobState::Paused => JobStateDto::Paused,
                JobState::Completed => JobStateDto::Completed,
                JobState::Failed => JobStateDto::Failed,
                JobState::Cancelled => JobStateDto::Cancelled,
            },
            priority: match s.priority {
                Priority::Low => PriorityDto::Low,
                Priority::Normal => PriorityDto::Normal,
                Priority::High => PriorityDto::High,
            },
            total: small(s.total),
            done: small(s.done),
            failed: small(s.failed),
            rolled_back: small(s.rolled_back),
            current: s.current.clone(),
            throughput_per_sec: s.throughput_per_sec,
            eta_secs: s.eta_secs,
            elapsed_secs: s.elapsed_secs,
            outcome: s.outcome.clone(),
            result: s.result.clone(),
        }
    }
}

impl From<&Overall> for OverallProgress {
    fn from(o: &Overall) -> Self {
        Self {
            kind: match o.kind {
                OverallKind::Idle => OverallKindDto::Idle,
                OverallKind::Indeterminate => OverallKindDto::Indeterminate,
                OverallKind::Normal => OverallKindDto::Normal,
                OverallKind::Paused => OverallKindDto::Paused,
            },
            percent: o.percent(),
            active_jobs: small(o.active_jobs),
        }
    }
}

/// The message for a set of jobs and their combined progress.
pub fn update_of(jobs: &[JobSnapshot], overall: &Overall) -> JobProgressUpdate {
    JobProgressUpdate {
        jobs: jobs.iter().map(JobProgress::from).collect(),
        overall: overall.into(),
    }
}

struct Subscriber {
    sink: Box<dyn FnMut(JobProgressUpdate) -> bool + Send>,
    /// The next report is sent in full, so a report that was prepared before this subscriber
    /// joined cannot leave it with stale jobs.
    needs_full: bool,
}

/// The subscribers of the progress stream. A sink that returns `false` (a closed page) is dropped
/// on the next send.
#[derive(Default)]
pub struct ProgressHub {
    subscribers: Mutex<Vec<Subscriber>>,
}

impl ProgressHub {
    fn lock(&self) -> MutexGuard<'_, Vec<Subscriber>> {
        self.subscribers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.lock().len()
    }

    /// Adds a sink and immediately sends it `initial`, the state of every job.
    pub fn subscribe(
        &self,
        sink: impl FnMut(JobProgressUpdate) -> bool + Send + 'static,
        initial: JobProgressUpdate,
    ) {
        let mut sink = Box::new(sink);
        if !sink(initial) {
            return;
        }
        self.lock().push(Subscriber {
            sink,
            needs_full: true,
        });
    }

    /// Sends one report to every subscriber: the changed jobs, or for a new subscriber every job
    /// (`all_jobs` is only called when someone needs it).
    pub fn publish(&self, tick: &ProgressTick, all_jobs: impl FnOnce() -> Vec<JobSnapshot>) {
        let mut subscribers = self.lock();
        if subscribers.is_empty() {
            return;
        }
        let delta = update_of(&tick.jobs, &tick.overall);
        let mut full: Option<JobProgressUpdate> = None;
        let mut all_jobs = Some(all_jobs);
        subscribers.retain_mut(|subscriber| {
            let update = if subscriber.needs_full {
                subscriber.needs_full = false;
                if full.is_none() {
                    let jobs = all_jobs.take().map(|f| f()).unwrap_or_default();
                    full = Some(update_of(&jobs, &tick.overall));
                }
                full.clone().unwrap_or_else(|| delta.clone())
            } else {
                delta.clone()
            };
            (subscriber.sink)(update)
        });
    }
}

/// Opens the progress stream: the sink first gets every job, later reports only what changed.
pub fn subscribe_job_progress(
    hub: &ProgressHub,
    manager: &JobManager,
    sink: impl FnMut(JobProgressUpdate) -> bool + Send + 'static,
) {
    let jobs = manager.list();
    let overall = OverallTracker::default().update(&jobs);
    hub.subscribe(sink, update_of(&jobs, &overall));
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    fn empty() -> JobProgressUpdate {
        update_of(&[], &Overall::IDLE)
    }

    #[test]
    fn progress_messages_use_snake_case_field_and_state_names() {
        let hub = ProgressHub::default();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        hub.subscribe(
            move |update| {
                sink.lock()
                    .unwrap()
                    .push(serde_json::to_value(&update).unwrap());
                true
            },
            empty(),
        );
        let messages = seen.lock().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["overall"]["kind"], "idle");
        assert_eq!(messages[0]["overall"]["active_jobs"], 0);
    }

    #[test]
    fn a_closed_sink_is_dropped_and_does_not_stop_the_others() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let hub = ProgressHub::default();
        let closed = Arc::new(AtomicBool::new(false));
        let dead = Arc::new(Mutex::new(0_usize));
        let dead_count = Arc::clone(&dead);
        let flag = Arc::clone(&closed);
        hub.subscribe(
            move |_| {
                *dead_count.lock().unwrap() += 1;
                !flag.load(Ordering::SeqCst)
            },
            empty(),
        );
        let alive = Arc::new(Mutex::new(0_usize));
        let alive_count = Arc::clone(&alive);
        hub.subscribe(
            move |_| {
                *alive_count.lock().unwrap() += 1;
                true
            },
            empty(),
        );
        assert_eq!(hub.len(), 2);
        let tick = ProgressTick {
            jobs: Vec::new(),
            overall: Overall::IDLE,
            overall_changed: true,
        };
        closed.store(true, Ordering::SeqCst);
        hub.publish(&tick, Vec::new);
        assert_eq!(hub.len(), 1);
        assert_eq!(*dead.lock().unwrap(), 2, "the initial and the report");
        assert_eq!(*alive.lock().unwrap(), 2, "the initial and the report");
    }

    #[test]
    fn a_new_subscriber_gets_every_job_first() {
        let hub = ProgressHub::default();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        hub.subscribe(
            move |update| {
                sink.lock().unwrap().push(update);
                true
            },
            empty(),
        );
        assert_eq!(seen.lock().unwrap().len(), 1);
        assert!(seen.lock().unwrap()[0].jobs.is_empty());
    }
}
