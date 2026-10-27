//! Job lifecycle: states, progress with ETA, cancel, pause/resume and priority.
//!
//! A [`JobManager`] runs *jobs*: an ordered list of items processed by an [`ItemWork`]
//! implementation on one [`LaneId`] lane. It adds the job control surface (pause, resume, cancel, retry) on top of
//! the lanes:
//!
//! - **States and progress.** Every job publishes a [`JobSnapshot`] (state, done/total, current
//!   items, throughput, ETA) through a `watch` channel on each change.
//! - **Cancel.** Cooperative and bounded: a cancelled job starts no further item, items still
//!   waiting for a lane worker are dropped untouched, and an item that is running gets
//!   [`ManagerConfig::cancel_grace`] to finish by itself (or to notice the [`ItemControl`] and
//!   return [`ItemError::Cancelled`]). If it does not, its work is dropped and
//!   [`ItemWork::rollback`] undoes whatever it left behind. Every started item therefore ends
//!   as done, failed or rolled back, and none is left half-processed. With the default grace of
//!   500 ms the job is over well inside the 1 s promise, provided `rollback` is quick.
//! - **Pause and resume.** A paused job starts no new item; items already running finish.
//!   Time spent paused is excluded from throughput and ETA. [`JobManager::pause_all`] pauses
//!   every job, including ones submitted later.
//! - **Priority.** At most [`ManagerConfig::max_running_jobs`] jobs run at once; waiting jobs
//!   start highest [`Priority`] first and first-come-first-served within a priority, and
//!   [`JobHandle::set_priority`] reorders the waiting jobs. Paused jobs do not occupy a slot.
//!
//! Per-item failures never stop a job; a job only ends as `Failed` when every item failed or its
//! worker panicked.
//!
//! # Failures and retry
//!
//! Failed items are grouped by reason ([`JobHandle::failure_groups`], [`JobReport::failure_groups`]
//! and [`JobStore::failure_groups`] for stored jobs). [`JobHandle::retry_failed`] re-queues
//! exactly the failed items as a new job named "<name> (retry)"; items that succeeded are not
//! touched. A persistent job's retry is persistent too, and
//! [`JobManager::retry_failed_persisted`] retries the failures of a stored job after a restart.
//!
//! # Persistence
//!
//! A manager built with [`JobManager::with_store`] can run *persistent* jobs
//! ([`JobManager::submit_persistent`]): the job and all its items are saved first, every item's
//! outcome is committed before the job counts it, and the job's state and progress follow along.
//! After a crash, a kill or a quit, [`JobManager::resume_persisted`] starts each cut-short job
//! again with only the items that had not finished, so nothing finished is done twice. An item
//! that was running when the process died did not finish and runs again, so persistent
//! workers should be idempotent (renames are protected by the rename journal).

use std::collections::HashSet;
use std::fmt;
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};

use cv_db::repo::jobs::{FINISHED_ITEM_STATES, NewJobItem, REMAINING_ITEM_STATES};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::sync::{Notify, watch};
use tokio::task::JoinSet;
use tokio::time::{Duration, timeout};

use crate::lane::{LaneId, Lanes};
use crate::pipeline::StageError;
use crate::progress::{
    FailureGroup, ItemRecord, ItemStatus, JobId, JobReport, JobSnapshot, JobState, Priority,
    Tracker, group_failures,
};
use crate::store::{
    JobStore, Persist, StoreError, StoredJob, StoredNew, parse_lane, priority_from, priority_value,
    retry_name,
};

/// Job manager settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagerConfig {
    /// Jobs that may run at the same time (at least 1). Waiting jobs start by priority.
    pub max_running_jobs: usize,
    /// How long a running item may keep going after a cancel before it is dropped and rolled
    /// back. Keep it well below one second.
    pub cancel_grace: Duration,
}

impl Default for ManagerConfig {
    fn default() -> Self {
        Self {
            max_running_jobs: 8,
            cancel_grace: Duration::from_millis(500),
        }
    }
}

/// Describes a job to submit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobSpec {
    /// Display name.
    pub name: String,
    /// Lane every item runs on.
    pub lane: LaneId,
    /// Starting priority.
    pub priority: Priority,
    /// Items of this job processed at the same time (at least 1; the lane still bounds it).
    /// Items start in order; use 1 for strictly sequential work such as a rename batch.
    pub concurrency: usize,
}

impl JobSpec {
    /// A normal-priority, sequential job.
    pub fn new(name: impl Into<String>, lane: LaneId) -> Self {
        Self {
            name: name.into(),
            lane,
            priority: Priority::Normal,
            concurrency: 1,
        }
    }

    /// Sets the starting priority.
    pub fn priority(mut self, priority: Priority) -> Self {
        self.priority = priority;
        self
    }

    /// Sets how many items run at the same time.
    pub fn concurrency(mut self, concurrency: usize) -> Self {
        self.concurrency = concurrency.max(1);
        self
    }
}

/// Why processing one item did not succeed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemError {
    /// The item failed; the job goes on with the next item.
    Failed(StageError),
    /// The item noticed the cancel and stopped. [`ItemWork::rollback`] is called for it.
    Cancelled,
}

impl From<StageError> for ItemError {
    fn from(error: StageError) -> Self {
        Self::Failed(error)
    }
}

impl fmt::Display for ItemError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Failed(error) => error.fmt(f),
            Self::Cancelled => f.write_str("cancelled"),
        }
    }
}

impl std::error::Error for ItemError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Control {
    paused: bool,
    cancelled: bool,
}

/// Lets an item see pause and cancel requests while it runs.
#[derive(Debug, Clone)]
pub struct ItemControl {
    rx: watch::Receiver<Control>,
}

impl ItemControl {
    /// True once the job was cancelled. Long items should poll this (or await
    /// [`ItemControl::cancelled`]) at safe points and return [`ItemError::Cancelled`].
    pub fn is_cancelled(&self) -> bool {
        self.rx.borrow().cancelled
    }

    /// Completes when the job is cancelled.
    pub async fn cancelled(&self) {
        let mut rx = self.rx.clone();
        // The sender lives as long as the job; if it is gone the job is over anyway.
        let _ = rx.wait_for(|c| c.cancelled).await;
    }

    fn is_paused(&self) -> bool {
        self.rx.borrow().paused
    }
}

/// The work of a job: how to process one item and how to undo a half-done one.
pub trait ItemWork: Send + Sync + 'static {
    /// One unit of work. Its `Display` form is shown as the current item.
    type Item: fmt::Display + Send + Sync + 'static;

    /// Processes `item`. Called while a worker of the job's lane is held.
    fn process(
        &self,
        item: &Self::Item,
        control: &ItemControl,
    ) -> impl Future<Output = Result<(), ItemError>> + Send;

    /// Undoes the partial effects of `item` after a cancel interrupted it: either
    /// [`ItemWork::process`] returned [`ItemError::Cancelled`], or it overran the cancel grace
    /// and was dropped. It runs after `process` has stopped, never concurrently with it, and
    /// must be quick. The default does nothing, which suits items without side effects.
    fn rollback(&self, item: &Self::Item) -> impl Future<Output = ()> + Send {
        let _ = item;
        async {}
    }

    /// What the job did beyond its counts, read once when it is over and shown with it
    /// ([`JobSnapshot::outcome`]): for example that the items it processed wait for a human
    /// decision rather than being done. The default says nothing.
    fn outcome(&self) -> Option<String> {
        None
    }

    /// What the job produced, read once when it is over: the phrase its step reads as in the
    /// run history ([`JobSnapshot::result`]), such as `212 new`, `198 scraped` or
    /// `artwork for 190`. The default says nothing, and History shows the job's name.
    fn result(&self) -> Option<String> {
        None
    }
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

struct State {
    priority: Priority,
    own_paused: bool,
    global_paused: bool,
    cancelled: bool,
    /// A job slot was granted; the job has started (it may be paused since).
    granted: bool,
    terminal: Option<JobState>,
    /// What the work said about the finished job; see [`ItemWork::outcome`].
    outcome: Option<String>,
    /// What the finished job produced; see [`ItemWork::result`].
    result: Option<String>,
    tracker: Tracker,
}

impl State {
    fn paused(&self) -> bool {
        self.own_paused || self.global_paused
    }

    fn running(&self) -> bool {
        self.granted && self.terminal.is_none() && !self.paused()
    }

    /// Whether the job's run clock ticks: running, or paused with items still finishing.
    fn clock_running(&self) -> bool {
        self.granted
            && self.terminal.is_none()
            && (!self.paused() || !self.tracker.current.is_empty())
    }

    fn waiting_for_slot(&self) -> bool {
        !self.granted && self.terminal.is_none() && !self.paused() && !self.cancelled
    }

    fn job_state(&self) -> JobState {
        if let Some(state) = self.terminal {
            state
        } else if self.paused() {
            JobState::Paused
        } else if self.granted {
            JobState::Running
        } else {
            JobState::Queued
        }
    }
}

/// Re-queues the failed items at the given item positions as a new job with the given priority.
type Retrier = Box<dyn Fn(&[usize], Priority) -> Option<JobHandle> + Send + Sync>;

struct Core {
    id: JobId,
    name: String,
    lane: LaneId,
    seq: u64,
    control: watch::Sender<Control>,
    start: Notify,
    state: Mutex<State>,
    snapshot: watch::Sender<JobSnapshot>,
    /// Where the job saves itself, for a persistent job.
    persist: Option<Persist>,
    /// True once the final state is saved (always true for a job that is not persistent).
    settled: watch::Sender<bool>,
    /// Starts the retry job; see [`JobHandle::retry_failed`].
    retrier: Retrier,
}

impl Core {
    fn view(&self, state: &State) -> JobSnapshot {
        let t = &state.tracker;
        JobSnapshot {
            id: self.id,
            name: self.name.clone(),
            lane: self.lane.clone(),
            state: state.job_state(),
            priority: state.priority,
            total: t.total,
            done: t.done,
            failed: t.failed,
            rolled_back: t.rolled_back,
            current: t.current.values().cloned().collect(),
            throughput_per_sec: t.throughput(),
            eta_secs: if state.terminal.is_some() {
                None
            } else {
                t.eta_secs()
            },
            elapsed_secs: t.elapsed().as_secs_f64(),
            outcome: state.outcome.clone(),
            result: state.result.clone(),
        }
    }

    /// Applies `change` to the state, keeps the running clock and control signal in step, and
    /// publishes a fresh snapshot.
    fn mutate<R>(&self, change: impl FnOnce(&mut State) -> R) -> R {
        let mut state = locked(&self.state);
        let was_running = state.clock_running();
        state.tracker.sync_clock(was_running);
        let result = change(&mut state);
        let running = state.clock_running();
        state.tracker.sync_clock(running);
        let control = Control {
            paused: state.paused(),
            cancelled: state.cancelled,
        };
        self.control.send_if_modified(|current| {
            let changed = *current != control;
            *current = control;
            changed
        });
        let view = self.view(&state);
        drop(state);
        self.snapshot.send_replace(view);
        result
    }

    fn with<R>(&self, read: impl FnOnce(&State) -> R) -> R {
        read(&locked(&self.state))
    }

    fn record(&self, index: usize, label: String, status: ItemStatus) {
        self.mutate(|s| {
            let t = &mut s.tracker;
            t.current.remove(&index);
            match &status {
                ItemStatus::Done => t.note_done(),
                ItemStatus::Failed { .. } => {
                    t.note_done();
                    t.failed += 1;
                }
                ItemStatus::RolledBack => t.rolled_back += 1,
            }
            t.records.push(ItemRecord {
                index,
                label,
                status,
            });
        });
    }

    /// Moves the job to its terminal state, with what the work said about it.
    fn finish(&self, panicked: bool, outcome: Option<String>, result: Option<String>) {
        self.mutate(|s| {
            if s.terminal.is_some() {
                return;
            }
            s.outcome = outcome;
            s.result = result;
            let t = &mut s.tracker;
            t.current.clear();
            t.records.sort_by_key(|r| r.index);
            s.terminal = Some(if panicked {
                JobState::Failed
            } else if s.cancelled && t.done < t.total {
                JobState::Cancelled
            } else if t.total > 0 && t.failed == t.total {
                JobState::Failed
            } else {
                JobState::Completed
            });
        });
    }
}

struct Inner {
    lanes: Lanes,
    config: ManagerConfig,
    store: Option<JobStore>,
    /// Database ids of the persistent jobs this manager already runs.
    attached: Mutex<HashSet<i64>>,
    jobs: Mutex<Vec<Arc<Core>>>,
    next_id: AtomicU64,
    all_paused: AtomicBool,
    /// Counts submitted jobs, so an idle progress feed can sleep until the next one.
    submitted: watch::Sender<u64>,
}

/// Finished jobs the manager keeps for the activity history and "Retry failed"; older ones are
/// dropped when a new job is submitted. Handles to dropped jobs stay valid.
pub const KEEP_FINISHED_JOBS: usize = 50;

impl Inner {
    /// Drops the oldest finished jobs beyond [`KEEP_FINISHED_JOBS`]. Running and waiting jobs
    /// always stay.
    fn prune_finished(jobs: &mut Vec<Arc<Core>>) {
        let finished = jobs
            .iter()
            .filter(|c| c.with(|s| s.terminal.is_some()))
            .count();
        let mut excess = finished.saturating_sub(KEEP_FINISHED_JOBS);
        if excess == 0 {
            return;
        }
        // The Vec is in submission order, so the first finished ones are the oldest.
        jobs.retain(|c| {
            if excess > 0 && c.with(|s| s.terminal.is_some()) {
                excess -= 1;
                false
            } else {
                true
            }
        });
    }

    fn core(&self, id: JobId) -> Option<Arc<Core>> {
        locked(&self.jobs).iter().find(|c| c.id == id).cloned()
    }

    /// Grants free job slots to the waiting jobs, highest priority first, then oldest.
    fn dispatch(&self) {
        let jobs = locked(&self.jobs);
        let running = jobs.iter().filter(|c| c.with(State::running)).count();
        let free = self.config.max_running_jobs.max(1).saturating_sub(running);
        if free == 0 {
            return;
        }
        let mut waiting: Vec<(&Arc<Core>, Priority)> = jobs
            .iter()
            .filter_map(|c| {
                c.with(|s| s.waiting_for_slot().then_some(s.priority))
                    .map(|p| (c, p))
            })
            .collect();
        waiting.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.seq.cmp(&b.0.seq)));
        for (core, _) in waiting.into_iter().take(free) {
            core.mutate(|s| s.granted = true);
            core.start.notify_one();
        }
    }

    fn change(&self, id: JobId, change: impl FnOnce(&mut State)) -> bool {
        let Some(core) = self.core(id) else {
            return false;
        };
        let applied = core.mutate(|s| {
            let live = s.terminal.is_none();
            if live {
                change(s);
            }
            live
        });
        if applied {
            self.dispatch();
        }
        applied
    }
}

/// Runs jobs on the lanes and controls them. Cheap to clone; clones share the same jobs.
#[derive(Clone)]
pub struct JobManager {
    inner: Arc<Inner>,
}

impl JobManager {
    /// Creates a manager that runs job items on `lanes`.
    pub fn new(lanes: Lanes, config: ManagerConfig) -> Self {
        Self::build(lanes, config, None)
    }

    /// Creates a manager that can also run persistent jobs saved in `store`.
    pub fn with_store(lanes: Lanes, config: ManagerConfig, store: JobStore) -> Self {
        Self::build(lanes, config, Some(store))
    }

    fn build(lanes: Lanes, config: ManagerConfig, store: Option<JobStore>) -> Self {
        Self {
            inner: Arc::new(Inner {
                lanes,
                config,
                store,
                attached: Mutex::new(HashSet::new()),
                jobs: Mutex::new(Vec::new()),
                next_id: AtomicU64::new(1),
                all_paused: AtomicBool::new(false),
                submitted: watch::Sender::new(0),
            }),
        }
    }

    /// The lanes jobs run on.
    pub fn lanes(&self) -> &Lanes {
        &self.inner.lanes
    }

    /// Queues a job that processes `items` in order with `work`. Works from any thread, inside
    /// a tokio runtime or not (see [`Lanes::spawn`]). The job starts as soon as a job slot is free. The job lives in memory only; use
    /// [`JobManager::submit_persistent`] for one that survives a restart.
    pub fn submit<W: ItemWork>(&self, spec: JobSpec, work: W, items: Vec<W::Item>) -> JobHandle {
        let launch = Launch::fresh(items.len());
        let pos = (0..items.len()).collect();
        self.launch(spec, Arc::new(work), Arc::new(items), pos, launch)
    }

    /// Like [`JobManager::submit`], but saves the job and its items first, so that
    /// [`JobManager::resume_persisted`] can finish it after a restart. `kind` names the worker:
    /// it is what a later `resume_persisted` call selects the job by.
    ///
    /// The manager must have a store ([`JobManager::with_store`]). The save is a blocking
    /// database write, done before this returns.
    pub fn submit_persistent<W: ItemWork>(
        &self,
        kind: &str,
        spec: JobSpec,
        work: W,
        items: Vec<W::Item>,
    ) -> Result<JobHandle, StoreError>
    where
        W::Item: Serialize,
    {
        let store = self.inner.store.clone().ok_or(StoreError::NoStore)?;
        let stored = items
            .iter()
            .map(|item| {
                Ok(NewJobItem {
                    label: item.to_string(),
                    payload: serde_json::to_string(item)?,
                })
            })
            .collect::<Result<Vec<_>, serde_json::Error>>()?;
        let row = store.insert(
            StoredNew {
                kind: kind.to_owned(),
                name: spec.name.clone(),
                lane: spec.lane.to_string(),
                priority: priority_value(spec.priority),
                payload: serde_json::json!({ "concurrency": spec.concurrency.max(1) }).to_string(),
            },
            stored,
        )?;
        locked(&self.inner.attached).insert(row);
        let mut launch = Launch::fresh(items.len());
        launch.persist = Some(Persist { store, row });
        let pos = (0..items.len()).collect();
        Ok(self.launch(spec, Arc::new(work), Arc::new(items), pos, launch))
    }

    /// Starts again every persistent job of `kind` that was cut short by a crash, a kill or a
    /// quit (state queued, running or paused), with only its unfinished items. `make_work` builds
    /// the worker for each stored job. Jobs that are already running in this manager are skipped,
    /// so calling it twice is harmless.
    ///
    /// A job whose stored data cannot be read back is marked failed (logged) and skipped, so one
    /// broken job never blocks the others. Returns a handle per resumed job, oldest first.
    pub fn resume_persisted<W, F>(
        &self,
        kind: &str,
        mut make_work: F,
    ) -> Result<Vec<JobHandle>, StoreError>
    where
        W: ItemWork,
        W::Item: DeserializeOwned,
        F: FnMut(&StoredJob) -> W,
    {
        let store = self.inner.store.clone().ok_or(StoreError::NoStore)?;
        let mut handles = Vec::new();
        for job in store.resumable(kind)? {
            if locked(&self.inner.attached).contains(&job.id) {
                continue;
            }
            let loaded = match load_resumed::<W::Item>(&store, &job) {
                Ok(loaded) => loaded,
                Err(error) => {
                    tracing::warn!(job = job.id, %error, "stored job cannot be resumed");
                    store.mark_unresumable(job.id, &error.to_string())?;
                    continue;
                }
            };
            store.mark_resumed(job.id)?;
            locked(&self.inner.attached).insert(job.id);
            let spec = JobSpec::new(job.name.clone(), loaded.lane)
                .priority(priority_from(job.priority))
                .concurrency(loaded.concurrency);
            let launch = Launch {
                total: usize::try_from(job.total).unwrap_or(0),
                origin: loaded.origin,
                prior_done: loaded.prior_done,
                prior_failed: loaded.prior_failed,
                prior_records: loaded.prior_records,
                persist: Some(Persist {
                    store: store.clone(),
                    row: job.id,
                }),
            };
            let pos = (0..loaded.items.len()).collect();
            handles.push(self.launch(
                spec,
                Arc::new(make_work(&job)),
                Arc::new(loaded.items),
                pos,
                launch,
            ));
        }
        Ok(handles)
    }

    /// Retries the failed items of a stored job (one from the history, for instance after a
    /// restart) as a new persistent job holding copies of exactly those items. Returns `None`
    /// when the job is unknown or has no failed items. `make_work` builds the worker.
    ///
    /// Unlike [`JobHandle::retry_failed`] this reads every failed item from the database, so it
    /// also covers failures from before a restart.
    pub fn retry_failed_persisted<W, F>(
        &self,
        job_row: i64,
        make_work: F,
    ) -> Result<Option<JobHandle>, StoreError>
    where
        W: ItemWork,
        W::Item: DeserializeOwned,
        F: FnOnce(&StoredJob) -> W,
    {
        let store = self.inner.store.clone().ok_or(StoreError::NoStore)?;
        let Some(job) = store.job(job_row)? else {
            return Ok(None);
        };
        let (lane, concurrency) = job_settings(&job)?;
        let Some((row, failed)) = store.insert_retry(job_row, None)? else {
            return Ok(None);
        };
        let items = match failed
            .iter()
            .map(|item| serde_json::from_str(&item.payload))
            .collect::<Result<Vec<W::Item>, _>>()
        {
            Ok(items) => items,
            Err(error) => {
                store.mark_unresumable(row, &error.to_string())?;
                return Err(error.into());
            }
        };
        locked(&self.inner.attached).insert(row);
        let spec = JobSpec::new(retry_name(&job.name), lane)
            .priority(priority_from(job.priority))
            .concurrency(concurrency);
        let mut launch = Launch::fresh(items.len());
        launch.persist = Some(Persist { store, row });
        let pos = (0..items.len()).collect();
        Ok(Some(self.launch(
            spec,
            Arc::new(make_work(&job)),
            Arc::new(items),
            pos,
            launch,
        )))
    }

    /// Starts a job over the entries of `items` at `pos` (in that order). `launch.origin` gives
    /// each of those entries its position in the job's own item list.
    fn launch<W: ItemWork>(
        &self,
        spec: JobSpec,
        work: Arc<W>,
        items: Arc<Vec<W::Item>>,
        pos: Vec<usize>,
        launch: Launch,
    ) -> JobHandle {
        let inner = &self.inner;
        let id = JobId(inner.next_id.fetch_add(1, Ordering::SeqCst));
        let global_paused = inner.all_paused.load(Ordering::SeqCst);
        let state = State {
            priority: spec.priority,
            own_paused: false,
            global_paused,
            cancelled: false,
            granted: false,
            terminal: None,
            outcome: None,
            result: None,
            tracker: Tracker::resumed(
                launch.total,
                launch.prior_done,
                launch.prior_failed,
                launch.prior_records,
            ),
        };
        let (control, _) = watch::channel(Control {
            paused: global_paused,
            cancelled: false,
        });
        let initial = JobSnapshot {
            id,
            name: spec.name.clone(),
            lane: spec.lane.clone(),
            state: state.job_state(),
            priority: spec.priority,
            total: launch.total,
            done: launch.prior_done,
            failed: launch.prior_failed,
            rolled_back: 0,
            current: Vec::new(),
            throughput_per_sec: None,
            eta_secs: None,
            elapsed_secs: 0.0,
            outcome: None,
            result: None,
        };
        let (snapshot, _) = watch::channel(initial);
        let (settled, _) = watch::channel(launch.persist.is_none());
        let origin = Arc::new(launch.origin);
        let pos = Arc::new(pos);
        let retrier = make_retrier(RetryPlan {
            inner: Arc::downgrade(inner),
            work: Arc::clone(&work),
            items: Arc::clone(&items),
            pos: Arc::clone(&pos),
            origin: Arc::clone(&origin),
            name: spec.name.clone(),
            lane: spec.lane.clone(),
            concurrency: spec.concurrency.max(1),
            persist: launch.persist.clone(),
        });
        let core = Arc::new(Core {
            id,
            name: spec.name.clone(),
            lane: spec.lane.clone(),
            seq: id.0,
            control,
            start: Notify::new(),
            state: Mutex::new(state),
            snapshot,
            persist: launch.persist,
            settled,
            retrier,
        });
        {
            let mut jobs = locked(&inner.jobs);
            Inner::prune_finished(&mut jobs);
            jobs.push(Arc::clone(&core));
        }
        inner.submitted.send_modify(|n| *n = n.wrapping_add(1));

        if core.persist.is_some() {
            inner.lanes.spawn_task(save_job(Arc::clone(&core)));
        }

        let run = Run {
            inner: Arc::clone(inner),
            core: Arc::clone(&core),
            work,
            items,
            pos,
            origin,
            lane: spec.lane,
            concurrency: spec.concurrency.max(1),
        };
        let supervised = Arc::clone(&core);
        let supervised_inner = Arc::clone(inner);
        // Submitting works from any thread, also one outside the runtime (the app's window
        // thread): the lanes pick the runtime.
        inner.lanes.spawn_task(async move {
            // A panic in the driver itself must still end the job, or `wait` would hang.
            if tokio::spawn(drive(run)).await.is_err() {
                supervised.finish(true, None, None);
                supervised_inner.dispatch();
            }
        });
        inner.dispatch();
        JobHandle {
            core,
            inner: Arc::clone(inner),
        }
    }

    /// Handle for a job, if it is still known.
    pub fn get(&self, id: JobId) -> Option<JobHandle> {
        self.inner.core(id).map(|core| JobHandle {
            core,
            inner: Arc::clone(&self.inner),
        })
    }

    /// Snapshots of all known jobs in submission order, with the time-derived fields (elapsed,
    /// throughput, the estimate) as of now.
    pub fn list(&self) -> Vec<JobSnapshot> {
        locked(&self.inner.jobs)
            .iter()
            .map(|c| c.with(|state| c.view(state)))
            .collect()
    }

    /// Snapshots of the jobs that are not over, the jobs submitted after job number `after`, and
    /// the jobs in `also`, in submission order. The progress feed reads this instead of
    /// [`JobManager::list`] so the finished history costs nothing per tick.
    pub fn list_live(&self, after: u64, also: &HashSet<JobId>) -> Vec<JobSnapshot> {
        locked(&self.inner.jobs)
            .iter()
            .filter_map(|c| {
                c.with(|state| {
                    (state.terminal.is_none() || c.id.0 > after || also.contains(&c.id))
                        .then(|| c.view(state))
                })
            })
            .collect()
    }

    /// A receiver that changes whenever a job is submitted.
    pub fn submissions(&self) -> watch::Receiver<u64> {
        self.inner.submitted.subscribe()
    }

    /// Pauses a job. Returns false if the job is unknown or over.
    pub fn pause(&self, id: JobId) -> bool {
        self.inner.change(id, |s| s.own_paused = true)
    }

    /// Resumes a job paused with [`JobManager::pause`]. Returns false if the job is unknown or
    /// over.
    pub fn resume(&self, id: JobId) -> bool {
        self.inner.change(id, |s| s.own_paused = false)
    }

    /// Cancels a job. Returns false if the job is unknown or already over.
    pub fn cancel(&self, id: JobId) -> bool {
        self.inner.change(id, |s| s.cancelled = true)
    }

    /// Changes a job's priority. Returns false if the job is unknown or over.
    pub fn set_priority(&self, id: JobId, priority: Priority) -> bool {
        self.inner.change(id, |s| s.priority = priority)
    }

    /// Pauses every job, including jobs submitted while everything is paused.
    pub fn pause_all(&self) {
        self.set_all_paused(true);
    }

    /// Lifts [`JobManager::pause_all`]. Jobs paused individually stay paused.
    pub fn resume_all(&self) {
        self.set_all_paused(false);
    }

    /// True while [`JobManager::pause_all`] is in effect.
    pub fn is_all_paused(&self) -> bool {
        self.inner.all_paused.load(Ordering::SeqCst)
    }

    fn set_all_paused(&self, paused: bool) {
        let jobs: Vec<Arc<Core>> = {
            let jobs = locked(&self.inner.jobs);
            self.inner.all_paused.store(paused, Ordering::SeqCst);
            jobs.clone()
        };
        for core in jobs {
            core.mutate(|s| {
                if s.terminal.is_none() {
                    s.global_paused = paused;
                }
            });
        }
        self.inner.dispatch();
    }

    /// Forgets jobs that are over, returning how many were dropped. Handles to them stay valid.
    pub fn forget_finished(&self) -> usize {
        let mut jobs = locked(&self.inner.jobs);
        let before = jobs.len();
        jobs.retain(|c| c.with(|s| s.terminal.is_none()));
        before - jobs.len()
    }
}

/// Controls and observes one job. Cheap to clone.
#[derive(Clone)]
pub struct JobHandle {
    core: Arc<Core>,
    inner: Arc<Inner>,
}

impl JobHandle {
    /// The job's id.
    pub fn id(&self) -> JobId {
        self.core.id
    }

    /// The job's row id in the database, for a persistent job.
    pub fn stored_id(&self) -> Option<i64> {
        self.core.persist.as_ref().map(|p| p.row)
    }

    /// The latest snapshot, with the time-derived fields (elapsed, throughput, the estimate) as
    /// of now.
    pub fn snapshot(&self) -> JobSnapshot {
        self.core.with(|state| self.core.view(state))
    }

    /// A receiver that is notified on every change of the job.
    pub fn subscribe(&self) -> watch::Receiver<JobSnapshot> {
        self.core.snapshot.subscribe()
    }

    /// Pauses the job: no new item starts; running items finish.
    pub fn pause(&self) -> bool {
        self.inner.change(self.core.id, |s| s.own_paused = true)
    }

    /// Resumes the job.
    pub fn resume(&self) -> bool {
        self.inner.change(self.core.id, |s| s.own_paused = false)
    }

    /// Cancels the job; see the module documentation for the guarantees.
    pub fn cancel(&self) -> bool {
        self.inner.change(self.core.id, |s| s.cancelled = true)
    }

    /// Changes the priority.
    pub fn set_priority(&self, priority: Priority) -> bool {
        self.inner.change(self.core.id, |s| s.priority = priority)
    }

    /// The failed items so far, grouped by reason (biggest group first).
    pub fn failure_groups(&self) -> Vec<FailureGroup> {
        self.core.with(|s| group_failures(&s.tracker.records))
    }

    /// Re-queues only the failed items of this finished job as a new job, "<name> (retry)",
    /// with this job's lane, concurrency and current priority. Items that succeeded are not
    /// touched. Returns `None` while the job is still going, when nothing failed, or when the
    /// manager is gone. A persistent job's retry is persistent too.
    ///
    /// Only failures of items this run held in memory are retried: for a job that was resumed
    /// after a restart, use [`JobManager::retry_failed_persisted`] to include the earlier ones.
    /// The failures stay on this job's record; retrying twice queues them twice.
    pub fn retry_failed(&self) -> Option<JobHandle> {
        let (slots, priority) = self.core.with(|s| {
            let slots: Vec<usize> = s
                .tracker
                .records
                .iter()
                .filter(|r| matches!(r.status, ItemStatus::Failed { .. }))
                .map(|r| r.index)
                .collect();
            (s.terminal.is_some().then_some(slots), s.priority)
        });
        let slots = slots.filter(|s| !s.is_empty())?;
        (self.core.retrier)(&slots, priority)
    }

    /// Waits until the job is over and returns its report.
    pub async fn wait(&self) -> JobReport {
        let mut rx = self.core.snapshot.subscribe();
        // The core owns the sender and we hold the core, so the channel cannot close here.
        let _ = rx.wait_for(|s| s.state.is_terminal()).await;
        // A persistent job is over for good only once its final state is saved.
        let _ = self.core.settled.subscribe().wait_for(|saved| *saved).await;
        // The snapshot turns terminal in the same step that fixes the records.
        let items = self.core.with(|s| s.tracker.records.clone());
        JobReport {
            snapshot: self.snapshot(),
            items,
        }
    }
}

struct Run<W: ItemWork> {
    inner: Arc<Inner>,
    core: Arc<Core>,
    work: Arc<W>,
    items: Arc<Vec<W::Item>>,
    /// Which entries of `items` this job runs, in order.
    pos: Arc<Vec<usize>>,
    /// Position in the job's own item list of each entry of `pos`.
    origin: Arc<Vec<usize>>,
    lane: LaneId,
    concurrency: usize,
}

async fn drive<W: ItemWork>(run: Run<W>) {
    let mut rx = run.core.control.subscribe();
    tokio::select! {
        () = run.core.start.notified() => {}
        _ = rx.wait_for(|c| c.cancelled) => {}
    }

    let shared = Arc::new(Shared {
        core: Arc::clone(&run.core),
        lanes: run.inner.lanes.clone(),
        work: Arc::clone(&run.work),
        items: Arc::clone(&run.items),
        pos: Arc::clone(&run.pos),
        origin: Arc::clone(&run.origin),
        lane: run.lane.clone(),
        grace: run.inner.config.cancel_grace,
    });

    let mut set: JoinSet<()> = JoinSet::new();
    let mut panicked = false;
    let mut next = 0;
    loop {
        if next < run.pos.len() && set.len() < run.concurrency {
            let cancelled = rx
                .wait_for(|c| c.cancelled || !c.paused)
                .await
                .map_or(true, |c| c.cancelled);
            if cancelled {
                break;
            }
            set.spawn(run_item(Arc::clone(&shared), next, rx.clone()));
            next += 1;
            continue;
        }
        match set.join_next().await {
            Some(Err(_)) => panicked = true,
            Some(Ok(())) => {}
            None => break,
        }
    }
    while let Some(result) = set.join_next().await {
        panicked |= result.is_err();
    }

    run.core
        .finish(panicked, run.work.outcome(), run.work.result());
    run.inner.dispatch();
}

struct Shared<W: ItemWork> {
    core: Arc<Core>,
    lanes: Lanes,
    work: Arc<W>,
    items: Arc<Vec<W::Item>>,
    pos: Arc<Vec<usize>>,
    origin: Arc<Vec<usize>>,
    lane: LaneId,
    grace: Duration,
}

/// Removes an item from the "current" list even if its task panics.
struct CurrentGuard<'a> {
    core: &'a Core,
    index: usize,
}

impl Drop for CurrentGuard<'_> {
    fn drop(&mut self) {
        let index = self.index;
        if self.core.with(|s| s.tracker.current.contains_key(&index)) {
            self.core.mutate(|s| {
                s.tracker.current.remove(&index);
            });
        }
    }
}

enum Attempt {
    /// The job was paused or cancelled by the time a worker was free; nothing ran.
    Skipped,
    Ran(Result<(), ItemError>),
}

async fn run_item<W: ItemWork>(shared: Arc<Shared<W>>, index: usize, rx: watch::Receiver<Control>) {
    let label = shared.items[shared.pos[index]].to_string();
    // The position in the original item list; a resumed job only runs part of it.
    let slot = shared.origin[index];
    let _guard = CurrentGuard {
        core: &shared.core,
        index: slot,
    };
    if let Some(status) = attempt(&shared, index, slot, &label, rx).await {
        // Commit the outcome before the job counts the item, so a kill right here repeats the
        // item instead of losing it.
        if let Some(persist) = &shared.core.persist {
            persist.item(slot, &status).await;
        }
        shared.core.record(slot, label, status);
    }
}

/// Runs one item to a status, or returns `None` if it was never started.
async fn attempt<W: ItemWork>(
    shared: &Shared<W>,
    index: usize,
    slot: usize,
    label: &str,
    mut rx: watch::Receiver<Control>,
) -> Option<ItemStatus> {
    let item = &shared.items[shared.pos[index]];
    let control = ItemControl { rx: rx.clone() };
    loop {
        // Hold off while paused; give up if cancelled.
        let cancelled = rx
            .wait_for(|c| c.cancelled || !c.paused)
            .await
            .map_or(true, |c| c.cancelled);
        if cancelled {
            return None;
        }

        let started = AtomicBool::new(false);
        let mut work = Box::pin(shared.lanes.run(&shared.lane, async {
            if control.is_paused() || control.is_cancelled() {
                return Attempt::Skipped;
            }
            started.store(true, Ordering::SeqCst);
            shared.core.mutate(|s| {
                s.tracker.current.insert(slot, label.to_owned());
            });
            Attempt::Ran(shared.work.process(item, &control).await)
        }));

        let mut cancel_rx = rx.clone();
        let result = tokio::select! {
            biased;
            result = &mut work => Some(result),
            _ = cancel_rx.wait_for(|c| c.cancelled) => None,
        };

        let result = match result {
            Some(result) => result,
            None => {
                if !started.load(Ordering::SeqCst) {
                    // Still waiting for a worker: nothing to undo.
                    return None;
                }
                match timeout(shared.grace, &mut work).await {
                    Ok(result) => result,
                    Err(_) => {
                        // Overran the grace period: stop it and undo what it left behind.
                        drop(work);
                        shared.work.rollback(item).await;
                        return Some(ItemStatus::RolledBack);
                    }
                }
            }
        };

        return match result {
            Attempt::Skipped => continue,
            Attempt::Ran(Ok(())) => Some(ItemStatus::Done),
            Attempt::Ran(Err(ItemError::Failed(error))) => Some(ItemStatus::Failed {
                reason: error.reason,
            }),
            Attempt::Ran(Err(ItemError::Cancelled)) => {
                shared.work.rollback(item).await;
                Some(ItemStatus::RolledBack)
            }
        };
    }
}

/// How a job is launched: fresh, or resumed from stored progress.
struct Launch {
    /// Items of the whole job, finished ones included.
    total: usize,
    /// Position in the original list of each item that still runs.
    origin: Vec<usize>,
    prior_done: usize,
    prior_failed: usize,
    prior_records: Vec<ItemRecord>,
    persist: Option<Persist>,
}

impl Launch {
    fn fresh(total: usize) -> Self {
        Self {
            total,
            origin: (0..total).collect(),
            prior_done: 0,
            prior_failed: 0,
            prior_records: Vec::new(),
            persist: None,
        }
    }
}

/// A stored job read back for resuming.
struct Resumed<I> {
    lane: LaneId,
    concurrency: usize,
    items: Vec<I>,
    origin: Vec<usize>,
    prior_done: usize,
    prior_failed: usize,
    prior_records: Vec<ItemRecord>,
}

fn load_resumed<I: DeserializeOwned>(
    store: &JobStore,
    job: &StoredJob,
) -> Result<Resumed<I>, StoreError> {
    let invalid = |reason: String| StoreError::Invalid {
        job: job.id,
        reason,
    };
    let index = |idx: i64| usize::try_from(idx).map_err(|_| invalid("negative item index".into()));
    let (lane, concurrency) = job_settings(job)?;

    let remaining = store.items(job.id, REMAINING_ITEM_STATES)?;
    let mut items = Vec::with_capacity(remaining.len());
    let mut origin = Vec::with_capacity(remaining.len());
    for stored in &remaining {
        items.push(serde_json::from_str(&stored.payload)?);
        origin.push(index(stored.idx)?);
    }

    let finished = store.items(job.id, FINISHED_ITEM_STATES)?;
    let mut prior_failed = 0;
    let mut prior_records = Vec::with_capacity(finished.len());
    for stored in &finished {
        let status = if stored.state == "failed" {
            prior_failed += 1;
            ItemStatus::Failed {
                reason: stored.reason.clone(),
            }
        } else {
            ItemStatus::Done
        };
        prior_records.push(ItemRecord {
            index: index(stored.idx)?,
            label: stored.label.clone(),
            status,
        });
    }
    Ok(Resumed {
        lane,
        concurrency,
        items,
        origin,
        prior_done: finished.len(),
        prior_failed,
        prior_records,
    })
}

/// Keeps the stored job row in step with the job: saves each published snapshot (several quick
/// changes collapse into the latest), and marks the job settled after saving its final state.
async fn save_job(core: Arc<Core>) {
    let Some(persist) = core.persist.clone() else {
        return;
    };
    let mut rx = core.snapshot.subscribe();
    loop {
        let snapshot = rx.borrow_and_update().clone();
        persist.job(&snapshot).await;
        if snapshot.state.is_terminal() || rx.changed().await.is_err() {
            break;
        }
    }
    core.settled.send_replace(true);
}

/// The lane and concurrency stored with a job.
fn job_settings(job: &StoredJob) -> Result<(LaneId, usize), StoreError> {
    let lane = parse_lane(&job.lane).ok_or_else(|| StoreError::Invalid {
        job: job.id,
        reason: format!("unknown lane `{}`", job.lane),
    })?;
    let settings: serde_json::Value = serde_json::from_str(&job.payload)?;
    let concurrency = settings
        .get("concurrency")
        .and_then(serde_json::Value::as_u64)
        .map_or(1, |n| usize::try_from(n).unwrap_or(1).max(1));
    Ok((lane, concurrency))
}

/// What a job needs to launch a retry of its failed items.
struct RetryPlan<W: ItemWork> {
    inner: Weak<Inner>,
    work: Arc<W>,
    items: Arc<Vec<W::Item>>,
    pos: Arc<Vec<usize>>,
    origin: Arc<Vec<usize>>,
    name: String,
    lane: LaneId,
    concurrency: usize,
    persist: Option<Persist>,
}

/// Builds the closure behind [`JobHandle::retry_failed`]. It holds the manager weakly, so a job
/// does not keep its manager alive.
fn make_retrier<W: ItemWork>(plan: RetryPlan<W>) -> Retrier {
    Box::new(move |slots, priority| {
        let inner = plan.inner.upgrade()?;
        // The retried items, in item order: (slot in the job's list, position in `items`).
        let mut chosen: Vec<(usize, usize)> = slots
            .iter()
            .filter_map(|&slot| {
                let k = plan.origin.iter().position(|&o| o == slot)?;
                Some((slot, plan.pos[k]))
            })
            .collect();
        chosen.sort_unstable();
        chosen.dedup();
        if chosen.is_empty() {
            return None;
        }
        let slots: Vec<usize> = chosen.iter().map(|c| c.0).collect();
        let positions: Vec<usize> = chosen.iter().map(|c| c.1).collect();

        let persist =
            plan.persist
                .as_ref()
                .and_then(|p| match p.store.insert_retry(p.row, Some(&slots)) {
                    Ok(Some((row, copied))) if copied.len() == positions.len() => {
                        locked(&inner.attached).insert(row);
                        Some(Persist {
                            store: p.store.clone(),
                            row,
                        })
                    }
                    Ok(Some((row, _))) => {
                        // Not every failure was saved, so the stored copy would be wrong: drop it
                        // and run the retry without persistence.
                        tracing::warn!(
                            job = p.row,
                            "failed items not fully saved; retry is not persistent"
                        );
                        let _ = p
                            .store
                            .mark_unresumable(row, "retry did not match the saved items");
                        None
                    }
                    Ok(None) => None,
                    Err(error) => {
                        tracing::warn!(job = p.row, %error, "could not save the retry job");
                        None
                    }
                });

        let spec = JobSpec::new(retry_name(&plan.name), plan.lane.clone())
            .priority(priority)
            .concurrency(plan.concurrency);
        let launch = Launch {
            total: positions.len(),
            origin: (0..positions.len()).collect(),
            prior_done: 0,
            prior_failed: 0,
            prior_records: Vec::new(),
            persist,
        };
        Some(JobManager { inner }.launch(
            spec,
            Arc::clone(&plan.work),
            Arc::clone(&plan.items),
            positions,
            launch,
        ))
    })
}
