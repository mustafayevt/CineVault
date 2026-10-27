//! Persistence of jobs in the library database.
//!
//! A [`JobStore`] wraps the shared [`Database`]. The [`crate::JobManager`] uses it to
//!
//! - store a job and all its items in one transaction when the job is submitted,
//! - mark each item `done`, `failed` or `rolled_back` in its own committed transaction *before*
//!   the job counts the item as processed, and
//! - keep the job row's state, priority and progress current while the job runs.
//!
//! After a crash, a kill or a quit, the items still `pending` are exactly what remains, so
//! [`crate::JobManager::resume_persisted`] can start the job again from those items only.
//!
//! Persistence failures are logged and never fail a job: a job that cannot be saved still runs,
//! it just cannot be resumed.

use std::sync::Arc;

use cv_db::repo::jobs::{self as rows, JobUpdate, NewJob, NewJobItem};
use cv_db::{Database, DbError};

use crate::lane::LaneId;
use crate::progress::{
    FailureGroup, ItemRecord, ItemStatus, JobSnapshot, JobState, Priority, group_failures,
};

pub use cv_db::repo::jobs::{JobItemCounts, StoredJob, StoredJobItem};

/// What can go wrong while saving or loading jobs.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// The database failed.
    #[error(transparent)]
    Db(#[from] DbError),
    /// An item or the job settings could not be turned into or read from JSON.
    #[error("job data is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    /// A stored job cannot be understood (unknown lane or similar).
    #[error("stored job {job} is invalid: {reason}")]
    Invalid { job: i64, reason: String },
    /// The manager has no job store, so jobs cannot be persisted.
    #[error("this job manager has no job store")]
    NoStore,
}

/// Saves and loads jobs. Cheap to clone.
#[derive(Clone)]
pub struct JobStore {
    db: Arc<Database>,
}

impl std::fmt::Debug for JobStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JobStore").finish_non_exhaustive()
    }
}

impl JobStore {
    /// A store on `db`.
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// The database behind the store.
    pub fn database(&self) -> &Arc<Database> {
        &self.db
    }

    /// The newest jobs first, up to `limit`: the persisted history for the Activity drawer.
    pub fn history(&self, limit: usize) -> Result<Vec<StoredJob>, StoreError> {
        Ok(self.db.read(|c| rows::list_jobs(c, limit))?)
    }

    /// One stored job.
    pub fn job(&self, id: i64) -> Result<Option<StoredJob>, StoreError> {
        Ok(self.db.read(|c| rows::get_job(c, id))?)
    }

    /// How many items of a stored job are in each state.
    pub fn item_counts(&self, id: i64) -> Result<JobItemCounts, StoreError> {
        Ok(self.db.read(|c| rows::job_item_counts(c, id))?)
    }

    /// The stored items of a job that are in one of `states` (`pending`, `done`, `failed`,
    /// `rolled_back`), in item order.
    pub fn items(&self, id: i64, states: &[&str]) -> Result<Vec<StoredJobItem>, StoreError> {
        let states: Vec<String> = states.iter().map(|s| (*s).to_owned()).collect();
        Ok(self.db.read(move |c| {
            let refs: Vec<&str> = states.iter().map(String::as_str).collect();
            rows::job_items_in_states(c, id, &refs)
        })?)
    }

    /// The failed items of a stored job grouped by reason: what the drawer shows for a job from
    /// the history, "3 failed · rate limit".
    pub fn failure_groups(&self, id: i64) -> Result<Vec<FailureGroup>, StoreError> {
        let records: Vec<ItemRecord> = self
            .items(id, &["failed"])?
            .into_iter()
            .map(|item| {
                Ok(ItemRecord {
                    index: usize::try_from(item.idx).map_err(|_| StoreError::Invalid {
                        job: id,
                        reason: "negative item index".into(),
                    })?,
                    label: item.label,
                    status: ItemStatus::Failed {
                        reason: item.reason,
                    },
                })
            })
            .collect::<Result<_, StoreError>>()?;
        Ok(group_failures(&records))
    }

    /// Jobs that were cut short (queued, running or paused), oldest first.
    pub fn resumable(&self, kind: &str) -> Result<Vec<StoredJob>, StoreError> {
        Ok(self.db.read(|c| rows::resumable_jobs(c, Some(kind)))?)
    }

    /// Stores a new job with its items; returns the job row id.
    pub(crate) fn insert(&self, job: StoredNew, items: Vec<NewJobItem>) -> Result<i64, StoreError> {
        Ok(self.db.write(move |tx| {
            rows::insert_job(
                tx,
                &NewJob {
                    kind: &job.kind,
                    name: &job.name,
                    lane: &job.lane,
                    priority: job.priority,
                    payload: &job.payload,
                },
                &items,
            )
        })?)
    }

    /// Stores a retry of job `from`: a new job of the same kind, lane, priority and settings
    /// holding copies of its failed items (only those at `slots`, if given). Returns the new row
    /// id and the copied items in item order, or `None` when there is nothing to retry.
    pub(crate) fn insert_retry(
        &self,
        from: i64,
        slots: Option<&[usize]>,
    ) -> Result<Option<(i64, Vec<StoredJobItem>)>, StoreError> {
        let slots: Option<Vec<i64>> = slots.map(|s| s.iter().map(|&i| i as i64).collect());
        Ok(self.db.write(move |tx| {
            let Some(job) = rows::get_job(tx, from)? else {
                return Ok(None);
            };
            let mut failed = rows::job_items_in_states(tx, from, &["failed"])?;
            if let Some(slots) = &slots {
                failed.retain(|item| slots.contains(&item.idx));
            }
            if failed.is_empty() {
                return Ok(None);
            }
            let copies: Vec<NewJobItem> = failed
                .iter()
                .map(|item| NewJobItem {
                    label: item.label.clone(),
                    payload: item.payload.clone(),
                })
                .collect();
            let name = retry_name(&job.name);
            let row = rows::insert_job(
                tx,
                &NewJob {
                    kind: &job.kind,
                    name: &name,
                    lane: &job.lane,
                    priority: job.priority,
                    payload: &job.payload,
                },
                &copies,
            )?;
            Ok(Some((row, failed)))
        })?)
    }

    /// Counts a new attempt at a resumed job.
    pub(crate) fn mark_resumed(&self, id: i64) -> Result<(), StoreError> {
        Ok(self.db.write(move |tx| rows::bump_attempts(tx, id))?)
    }

    /// Gives up on a stored job that cannot be resumed.
    pub(crate) fn mark_unresumable(&self, id: i64, error: &str) -> Result<(), StoreError> {
        let error = error.to_owned();
        Ok(self.db.write(move |tx| {
            let Some(job) = rows::get_job(tx, id)? else {
                return Ok(());
            };
            rows::update_job(
                tx,
                id,
                &JobUpdate {
                    state: "failed",
                    priority: job.priority,
                    done: job.done,
                    error: &error,
                },
            )
        })?)
    }
}

/// The name of the job that retries the failed items of `name`.
pub(crate) fn retry_name(name: &str) -> String {
    format!("{name} (retry)")
}

/// A job to insert, with owned fields (the write closure must be `'static`).
pub(crate) struct StoredNew {
    pub kind: String,
    pub name: String,
    pub lane: String,
    pub priority: i64,
    pub payload: String,
}

/// Where one running job saves itself.
#[derive(Clone)]
pub(crate) struct Persist {
    pub store: JobStore,
    pub row: i64,
}

impl Persist {
    /// Saves how one item ended and waits until it is committed.
    pub async fn item(&self, idx: usize, status: &ItemStatus) {
        let (state, reason) = match status {
            ItemStatus::Done => ("done", String::new()),
            ItemStatus::Failed { reason } => ("failed", reason.clone()),
            ItemStatus::RolledBack => ("rolled_back", String::new()),
        };
        let db = Arc::clone(&self.store.db);
        let row = self.row;
        let result = tokio::task::spawn_blocking(move || {
            db.write(move |tx| rows::set_job_item_state(tx, row, idx as i64, state, &reason))
        })
        .await;
        log_failure(row, "item", flatten(result));
    }

    /// Saves the job's state, priority and progress.
    pub async fn job(&self, snapshot: &JobSnapshot) {
        let db = Arc::clone(&self.store.db);
        let row = self.row;
        let state = state_text(snapshot.state);
        let priority = priority_value(snapshot.priority);
        let done = snapshot.done as i64;
        let error = match snapshot.state {
            JobState::Failed if snapshot.total > 0 && snapshot.failed == snapshot.total => {
                "every item failed"
            }
            JobState::Failed => "the job worker stopped unexpectedly",
            _ => "",
        };
        let result = tokio::task::spawn_blocking(move || {
            db.write(move |tx| {
                rows::update_job(
                    tx,
                    row,
                    &JobUpdate {
                        state,
                        priority,
                        done,
                        error,
                    },
                )
            })
        })
        .await;
        log_failure(row, "job", flatten(result));
    }
}

fn flatten<T>(result: Result<Result<T, DbError>, tokio::task::JoinError>) -> Result<T, String> {
    match result {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(error.to_string()),
        Err(error) => Err(error.to_string()),
    }
}

fn log_failure(row: i64, what: &str, result: Result<(), String>) {
    if let Err(error) = result {
        tracing::warn!(job = row, %error, "could not save {what} progress; the job cannot be fully resumed");
    }
}

/// The database text of a job state.
pub(crate) fn state_text(state: JobState) -> &'static str {
    match state {
        JobState::Queued => "queued",
        JobState::Running => "running",
        JobState::Paused => "paused",
        JobState::Completed => "succeeded",
        JobState::Failed => "failed",
        JobState::Cancelled => "canceled",
    }
}

/// The database value of a priority.
pub(crate) fn priority_value(priority: Priority) -> i64 {
    match priority {
        Priority::Low => -1,
        Priority::Normal => 0,
        Priority::High => 1,
    }
}

/// The priority of a database value.
pub(crate) fn priority_from(value: i64) -> Priority {
    match value {
        v if v < 0 => Priority::Low,
        0 => Priority::Normal,
        _ => Priority::High,
    }
}

/// Parses the text of a lane (`disk`, `cpu` or `provider:<id>`).
pub(crate) fn parse_lane(text: &str) -> Option<LaneId> {
    match text {
        "disk" => Some(LaneId::Disk),
        "cpu" => Some(LaneId::Cpu),
        other => other
            .strip_prefix("provider:")
            .filter(|id| !id.is_empty())
            .map(LaneId::provider),
    }
}
