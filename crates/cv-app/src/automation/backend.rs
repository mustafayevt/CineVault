//! The queue that runs batch plans for the app.
//!
//! [`CommandBackend`] queues a plan (the selection actions of the app build them) for a single
//! worker, which runs the plans one after another on the shared [`BatchEngine`]: the plans of
//! separate calls queue up in the order they arrived, and none of them overlaps another or a
//! command-line batch.
//!
//! The call returns as soon as the plan is queued; the outcome of every run is kept in
//! [`CommandBackend::runs`] (the most recent [`KEPT_RUNS`]).
//!
//! # Jobs
//!
//! Every queued plan is a persistent job of the job system (kind [`JOB_KIND`]) with one item per
//! step of the plan, so it appears in the job store, the Activity history and the progress feed
//! like any other job. The plan itself runs on the worker below; the job's items follow it: the
//! item of a step is done when the engine reports the step done, and failed (with the step's
//! message as the reason) when the step failed or was unavailable. A plan that waits behind
//! another one shows as running with its first step pending.
//!
//! A plan the app was closed in the middle of is not resumed (steps are not safe to repeat
//! blindly): at the next start its unfinished steps are marked failed with the reason "the app
//! closed before this step ran".

use std::collections::VecDeque;
use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use cv_jobs::{ItemControl, ItemError, ItemWork, JobSpec, LaneId, StageError};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot, watch};

use super::engine::{BatchEngine, Progress};
use super::model::{BatchReport, Plan, Step, StepStatus};

/// The kind of the persistent jobs that run queued plans.
pub const JOB_KIND: &str = "automation.batch";

/// The lane the batch jobs are accounted to. A lane of their own: a step holds a worker only
/// while it waits for the engine, and the engine's real work runs on the other lanes.
const LANE: &str = "batch";

/// How many finished runs [`CommandBackend::runs`] remembers.
pub const KEPT_RUNS: usize = 64;

/// A plan that has run.
#[derive(Debug, Clone)]
pub struct BatchRun {
    /// 1 for the first batch since the app started.
    pub number: u64,
    /// The job that ran it (the ID the Activity drawer shows).
    pub job: i64,
    /// The steps in the order they ran (the phase order, not the order they were sent in).
    pub steps: Vec<Step>,
    pub report: BatchReport,
}

struct Queued {
    number: u64,
    job: i64,
    plan: Plan,
    board: Arc<Board>,
    /// Told the report when the batch ends (the desktop app's selection actions wait for it).
    reply: Option<oneshot::Sender<BatchReport>>,
}

/// A plan the app queued with [`CommandBackend::submit_plan`].
#[derive(Debug)]
pub struct Submitted {
    /// 1 for the first batch since the app started.
    pub number: u64,
    /// The job that shows the batch in the Activity drawer.
    pub job: i64,
    report: oneshot::Receiver<BatchReport>,
}

impl Submitted {
    /// Waits for the batch to end and returns its report. A batch that was dropped without
    /// running (the app is closing) is a report that says so.
    pub async fn report(self, kind: super::model::EntityKind) -> BatchReport {
        self.report
            .await
            .unwrap_or_else(|_| BatchReport::failed_for(kind, false, "The app is closing."))
    }
}

/// One step of a batch, as the item of its job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct StepItem {
    index: usize,
    step: String,
}

impl fmt::Display for StepItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.step)
    }
}

/// How one step ended.
#[derive(Debug, Clone, PartialEq, Eq)]
enum StepEnd {
    Done,
    /// The step did nothing, for the reason given (a key to add, nothing in its scope). That is
    /// no failure: the job's item is done and the reason is part of what the job came to.
    Skipped(String),
    Failed(String),
}

#[derive(Debug, Clone, Default)]
struct BoardState {
    ends: Vec<Option<StepEnd>>,
    /// The batch is over; steps without an end did not run.
    closed: bool,
}

/// Where the worker tells the job's items how the steps went.
struct Board {
    state: watch::Sender<BoardState>,
    /// The step names, in plan order.
    names: Vec<String>,
}

/// A short, groupable reason from a step's message.
fn reason_of(message: &str) -> String {
    let first = message.lines().next().unwrap_or("").trim();
    if first.is_empty() {
        "the step failed".to_owned()
    } else {
        first.chars().take(120).collect()
    }
}

fn end_of(status: StepStatus, message: &str) -> StepEnd {
    match status {
        StepStatus::Done | StepStatus::Planned => StepEnd::Done,
        StepStatus::Skipped => {
            StepEnd::Skipped(message.lines().next().unwrap_or("").trim().to_owned())
        }
        StepStatus::Failed | StepStatus::Unavailable => StepEnd::Failed(reason_of(message)),
    }
}

impl Board {
    fn new(names: Vec<String>) -> Arc<Self> {
        let (state, _) = watch::channel(BoardState {
            ends: vec![None; names.len()],
            closed: false,
        });
        Arc::new(Self { state, names })
    }

    /// The steps that were skipped, each with why: `scrape skipped: Online scraping is
    /// paused until you add a TMDB key in Settings, Scraping and providers.`
    fn skipped(&self) -> Option<String> {
        let state = self.state.borrow();
        let parts: Vec<String> = state
            .ends
            .iter()
            .zip(&self.names)
            .filter_map(|(end, name)| match end {
                Some(StepEnd::Skipped(reason)) if !reason.is_empty() => {
                    Some(format!("{name} skipped: {reason}"))
                }
                Some(StepEnd::Skipped(_)) => Some(format!("{name} skipped")),
                _ => None,
            })
            .collect();
        (!parts.is_empty()).then(|| parts.join("; "))
    }

    fn end_step(&self, index: usize, end: StepEnd) {
        self.state.send_modify(|state| {
            if let Some(slot @ None) = state.ends.get_mut(index) {
                *slot = Some(end);
            }
        });
    }

    /// The batch is over: the report is the last word on every step, and a step that has no
    /// entry in it did not run.
    fn settle(&self, report: &BatchReport) {
        let unfinished = report
            .error
            .clone()
            .map_or_else(|| "the step did not run".to_owned(), |e| reason_of(&e));
        self.state.send_modify(|state| {
            for (index, slot) in state.ends.iter_mut().enumerate() {
                if slot.is_none() {
                    *slot = Some(match report.steps.get(index) {
                        Some(step) => end_of(step.status, &step.message),
                        None => StepEnd::Failed(unfinished.clone()),
                    });
                }
            }
            state.closed = true;
        });
    }
}

/// The work of a batch job: each item waits for the worker to finish its step.
struct BatchWork {
    board: Arc<Board>,
}

impl ItemWork for BatchWork {
    type Item = StepItem;

    async fn process(&self, item: &StepItem, control: &ItemControl) -> Result<(), ItemError> {
        let mut state = self.board.state.subscribe();
        let index = item.index;
        let waiting = async {
            state
                .wait_for(|s| s.closed || s.ends.get(index).is_some_and(Option::is_some))
                .await
                .map(|s| s.ends.get(index).cloned().flatten())
        };
        tokio::select! {
            ended = waiting => match ended {
                Ok(Some(StepEnd::Done | StepEnd::Skipped(_))) => Ok(()),
                Ok(Some(StepEnd::Failed(reason))) => Err(StageError::new(reason).into()),
                Ok(None) => Err(StageError::new("the step did not run").into()),
                Err(_) => Err(StageError::new("the app stopped before this step ended").into()),
            },
            () = control.cancelled() => Err(ItemError::Cancelled),
        }
    }

    fn outcome(&self) -> Option<String> {
        self.board.skipped()
    }
}

/// The work of a batch job that an earlier run of the app left unfinished.
struct InterruptedWork;

impl ItemWork for InterruptedWork {
    type Item = StepItem;

    async fn process(&self, _item: &StepItem, _control: &ItemControl) -> Result<(), ItemError> {
        Err(StageError::new("the app closed before this step ran").into())
    }
}

struct Shared {
    engine: Arc<BatchEngine>,
    runs: Mutex<VecDeque<BatchRun>>,
    /// Batches accepted and not finished.
    pending: watch::Sender<usize>,
}

/// Runs the plans the app queues, one after another.
pub struct CommandBackend {
    shared: Arc<Shared>,
    queue: mpsc::UnboundedSender<Queued>,
    counter: Mutex<u64>,
    worker: tokio::task::JoinHandle<()>,
}

impl std::fmt::Debug for CommandBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandBackend")
            .field("pending", &*self.shared.pending.borrow())
            .finish_non_exhaustive()
    }
}

impl CommandBackend {
    /// Starts the worker on the current tokio runtime.
    #[must_use]
    pub fn start(engine: Arc<BatchEngine>) -> Arc<Self> {
        let (queue, mut inbox) = mpsc::unbounded_channel::<Queued>();
        let (pending, _) = watch::channel(0_usize);
        let shared = Arc::new(Shared {
            engine,
            runs: Mutex::new(VecDeque::new()),
            pending,
        });
        // Batches an earlier run left half done are closed before new ones arrive.
        let manager = shared.engine.services().jobs().clone();
        match manager.resume_persisted(JOB_KIND, |_| InterruptedWork) {
            Ok(closed) if !closed.is_empty() => {
                tracing::info!(
                    count = closed.len(),
                    "closed batches left over from a previous run"
                );
            }
            Ok(_) => {}
            Err(error) => tracing::warn!(%error, "left-over batches could not be closed"),
        }
        let worker_shared = Arc::clone(&shared);
        let worker = cv_jobs::spawn(async move {
            while let Some(job) = inbox.recv().await {
                run_one(&worker_shared, job).await;
            }
        });
        Arc::new(Self {
            shared,
            queue,
            counter: Mutex::new(0),
            worker,
        })
    }

    /// The finished runs, oldest first.
    #[must_use]
    pub fn runs(&self) -> Vec<BatchRun> {
        self.shared
            .runs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .cloned()
            .collect()
    }

    /// Completes when every accepted batch has finished.
    pub async fn idle(&self) {
        let mut pending = self.shared.pending.subscribe();
        // The sender lives as long as `self`, so waiting cannot fail.
        let _ = pending.wait_for(|count| *count == 0).await;
    }

    /// Queues a plan the app built itself (a selection action) as a job named `name`, behind the
    /// batches already waiting. The job shows in the Activity drawer; the returned handle waits
    /// for the report.
    ///
    /// # Errors
    /// The batch could not be saved as a job, or the runner has stopped.
    pub fn submit_plan(&self, name: &str, plan: Plan) -> Result<Submitted, SubmitError> {
        let (reply, report) = oneshot::channel();
        let (number, job) = self.enqueue(plan, name.to_owned(), Some(reply))?;
        tracing::info!(number, job, name, "selection action queued");
        Ok(Submitted {
            number,
            job,
            report,
        })
    }

    /// Queues a checked plan as a job; returns the run number and the job's ID.
    fn enqueue(
        &self,
        plan: Plan,
        name: String,
        reply: Option<oneshot::Sender<BatchReport>>,
    ) -> Result<(u64, i64), SubmitError> {
        let steps: Vec<StepItem> = plan
            .steps
            .iter()
            .enumerate()
            .map(|(index, planned)| StepItem {
                index,
                step: planned.step.name().to_owned(),
            })
            .collect();
        let board = Board::new(steps.iter().map(|s| s.step.clone()).collect());
        let spec = JobSpec::new(name, LaneId::provider(LANE));
        let handle = self
            .shared
            .engine
            .services()
            .jobs()
            .submit_persistent(
                JOB_KIND,
                spec,
                BatchWork {
                    board: Arc::clone(&board),
                },
                steps,
            )
            .map_err(|error| {
                tracing::error!(%error, "a batch could not be saved as a job");
                SubmitError::NotSaved
            })?;
        let job = handle.stored_id().ok_or(SubmitError::NotSaved)?;
        let number = {
            let mut counter = self.counter.lock().unwrap_or_else(PoisonError::into_inner);
            *counter += 1;
            *counter
        };
        self.shared.pending.send_modify(|count| *count += 1);
        let kind = plan.kind;
        if self
            .queue
            .send(Queued {
                number,
                job,
                plan,
                board: Arc::clone(&board),
                reply,
            })
            .is_err()
        {
            self.shared.pending.send_modify(|count| *count -= 1);
            let report = BatchReport::failed_for(kind, false, "The command runner has stopped");
            board.settle(&report);
            return Err(SubmitError::Stopped);
        }
        Ok((number, job))
    }
}

impl Drop for CommandBackend {
    fn drop(&mut self) {
        self.worker.abort();
    }
}

async fn run_one(shared: &Arc<Shared>, job: Queued) {
    let Queued {
        number,
        job,
        plan,
        board,
        reply,
    } = job;
    let steps: Vec<Step> = plan.steps.iter().map(|p| p.step).collect();
    let engine = Arc::clone(&shared.engine);
    let kind = plan.kind;
    // A task of its own, so a panic inside a step cannot stop the queue.
    let step_board = Arc::clone(&board);
    let outcome = tokio::spawn(async move {
        let finished = AtomicUsize::new(0);
        let sink = |progress: Progress| {
            tracing::debug!(
                number,
                step = progress.step.map(Step::name),
                message = %progress.message,
                "batch progress"
            );
            // The message that closes a step ends the next item of the job; steps close in
            // plan order.
            if let Some(status) = progress.status {
                let index = finished.fetch_add(1, Ordering::SeqCst);
                step_board.end_step(index, end_of(status, &progress.message));
            }
        };
        engine.run_plan(&plan, &sink).await
    })
    .await;
    let report = outcome.unwrap_or_else(|error| {
        tracing::error!(number, %error, "a command batch stopped unexpectedly");
        BatchReport::failed_for(kind, false, "The batch stopped unexpectedly.")
    });
    if let Some(error) = &report.error {
        tracing::warn!(number, %error, "a command batch could not start");
    }
    board.settle(&report);
    if let Some(reply) = reply {
        // Nobody may be waiting any more (the page was closed); that is not an error.
        let _ = reply.send(report.clone());
    }
    {
        let mut runs = shared.runs.lock().unwrap_or_else(PoisonError::into_inner);
        if runs.len() >= KEPT_RUNS {
            runs.pop_front();
        }
        runs.push_back(BatchRun {
            number,
            job,
            steps,
            report,
        });
    }
    shared.pending.send_modify(|count| *count -= 1);
}

/// Why a plan could not be queued.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SubmitError {
    #[error("The batch could not be saved as a job")]
    NotSaved,
    #[error("The command runner has stopped")]
    Stopped,
}
