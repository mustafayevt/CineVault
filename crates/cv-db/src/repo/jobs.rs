//! Persistent jobs: the `job` row of a background job and the `job_item` rows it works through.
//!
//! The job system (`cv-jobs`) writes a job and all its items in one transaction when the job is
//! submitted, moves each item out of `pending` in its own committed transaction as soon as the
//! item ends, and keeps the job's state and counters current. After a crash or a quit, the rows
//! still `pending` (or `rolled_back`) are exactly the items that remain to be run.
//!
//! States are plain text here so this crate does not depend on the job system; the allowed values
//! are enforced by `CHECK` constraints in the schema.

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::error::DbResult;

/// Item states that still have to run.
pub const REMAINING_ITEM_STATES: &[&str] = &["pending", "rolled_back"];

/// Item states that are over.
pub const FINISHED_ITEM_STATES: &[&str] = &["done", "failed"];

/// A job to store.
#[derive(Debug, Clone)]
pub struct NewJob<'a> {
    /// Which worker handles the job (selects the code that resumes it).
    pub kind: &'a str,
    /// Display name.
    pub name: &'a str,
    /// Lane text, for example `disk` or `provider:tmdb`.
    pub lane: &'a str,
    /// Priority: negative low, 0 normal, positive high.
    pub priority: i64,
    /// JSON settings of the job.
    pub payload: &'a str,
}

/// One item to store with a job.
#[derive(Debug, Clone)]
pub struct NewJobItem {
    /// Display label.
    pub label: String,
    /// The item as JSON.
    pub payload: String,
}

/// The mutable part of a job row.
#[derive(Debug, Clone)]
pub struct JobUpdate<'a> {
    /// One of `queued`, `running`, `paused`, `succeeded`, `failed`, `canceled`.
    pub state: &'a str,
    /// Priority: negative low, 0 normal, positive high.
    pub priority: i64,
    /// Items processed so far, failed ones included.
    pub done: i64,
    /// Error text of a failed job.
    pub error: &'a str,
}

/// A stored job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredJob {
    pub id: i64,
    pub kind: String,
    pub name: String,
    pub lane: String,
    pub state: String,
    pub priority: i64,
    pub attempts: i64,
    pub done: i64,
    pub total: i64,
    pub payload: String,
    pub error: String,
    pub created_ms: i64,
    pub started_ms: Option<i64>,
    pub finished_ms: Option<i64>,
}

/// A stored job item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredJobItem {
    /// Position in the job's original item list.
    pub idx: i64,
    pub label: String,
    /// The item as JSON.
    pub payload: String,
    /// `pending`, `done`, `failed` or `rolled_back`.
    pub state: String,
    /// Failure reason of a `failed` item.
    pub reason: String,
}

/// How many items of a job are in each state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct JobItemCounts {
    pub pending: i64,
    pub done: i64,
    pub failed: i64,
    pub rolled_back: i64,
}

const JOB_COLUMNS: &str = "id, kind, name, lane, state, priority, attempts, progress_done, \
     progress_total, payload, error, created_ms, started_ms, finished_ms";

fn job_from_row(r: &Row<'_>) -> rusqlite::Result<StoredJob> {
    Ok(StoredJob {
        id: r.get(0)?,
        kind: r.get(1)?,
        name: r.get(2)?,
        lane: r.get(3)?,
        state: r.get(4)?,
        priority: r.get(5)?,
        attempts: r.get(6)?,
        done: r.get(7)?,
        total: r.get(8)?,
        payload: r.get(9)?,
        error: r.get(10)?,
        created_ms: r.get(11)?,
        started_ms: r.get(12)?,
        finished_ms: r.get(13)?,
    })
}

/// Stores a queued job together with all its items (state `pending`, in order). Returns the new
/// job id. Run it inside one transaction so a job is never stored without its items.
pub fn insert_job(conn: &Connection, job: &NewJob<'_>, items: &[NewJobItem]) -> DbResult<i64> {
    conn.prepare_cached(
        "INSERT INTO job (kind, name, lane, state, priority, progress_total, payload)
         VALUES (?1, ?2, ?3, 'queued', ?4, ?5, ?6)",
    )?
    .execute(params![
        job.kind,
        job.name,
        job.lane,
        job.priority,
        items.len() as i64,
        job.payload
    ])?;
    let id = conn.last_insert_rowid();
    let mut stmt = conn.prepare_cached(
        "INSERT INTO job_item (job_id, idx, label, payload) VALUES (?1, ?2, ?3, ?4)",
    )?;
    for (idx, item) in items.iter().enumerate() {
        stmt.execute(params![id, idx as i64, item.label, item.payload])?;
    }
    Ok(id)
}

/// Updates a job's state, priority and progress. `started_ms` is set the first time the job
/// runs and `finished_ms` when it reaches a final state. Unknown ids are ignored.
pub fn update_job(conn: &Connection, id: i64, update: &JobUpdate<'_>) -> DbResult<()> {
    conn.prepare_cached(
        "UPDATE job SET
             state = ?2,
             priority = ?3,
             progress_done = ?4,
             error = ?5,
             started_ms = CASE WHEN ?2 = 'running' AND started_ms IS NULL
                               THEN CAST(unixepoch('subsec') * 1000 AS INTEGER)
                               ELSE started_ms END,
             finished_ms = CASE WHEN ?2 IN ('succeeded', 'failed', 'canceled')
                                THEN CAST(unixepoch('subsec') * 1000 AS INTEGER)
                                ELSE NULL END
         WHERE id = ?1",
    )?
    .execute(params![
        id,
        update.state,
        update.priority,
        update.done,
        update.error
    ])?;
    Ok(())
}

/// Counts one more attempt at running the job (called when it is resumed).
pub fn bump_attempts(conn: &Connection, id: i64) -> DbResult<()> {
    conn.prepare_cached("UPDATE job SET attempts = attempts + 1 WHERE id = ?1")?
        .execute([id])?;
    Ok(())
}

/// Records how one item ended (`pending`, `done`, `failed` or `rolled_back`). `reason` is kept
/// for failures and cleared otherwise.
pub fn set_job_item_state(
    conn: &Connection,
    job_id: i64,
    idx: i64,
    state: &str,
    reason: &str,
) -> DbResult<()> {
    conn.prepare_cached(
        "UPDATE job_item SET
             state = ?3,
             reason = ?4,
             finished_ms = CASE WHEN ?3 = 'pending' THEN NULL
                                ELSE CAST(unixepoch('subsec') * 1000 AS INTEGER) END
         WHERE job_id = ?1 AND idx = ?2",
    )?
    .execute(params![job_id, idx, state, reason])?;
    Ok(())
}

/// One job by id.
pub fn get_job(conn: &Connection, id: i64) -> DbResult<Option<StoredJob>> {
    Ok(conn
        .prepare_cached(&format!("SELECT {JOB_COLUMNS} FROM job WHERE id = ?1"))?
        .query_row([id], job_from_row)
        .optional()?)
}

/// Jobs that were queued, running or paused (so were cut short), oldest first. With `kind`, only
/// jobs of that kind.
pub fn resumable_jobs(conn: &Connection, kind: Option<&str>) -> DbResult<Vec<StoredJob>> {
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT {JOB_COLUMNS} FROM job
         WHERE state IN ('queued', 'running', 'paused') AND (?1 IS NULL OR kind = ?1)
         ORDER BY id"
    ))?;
    let rows = stmt.query_map([kind], job_from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// The newest jobs first, up to `limit`: the persisted history of the Activity drawer.
pub fn list_jobs(conn: &Connection, limit: usize) -> DbResult<Vec<StoredJob>> {
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT {JOB_COLUMNS} FROM job ORDER BY id DESC LIMIT ?1"
    ))?;
    let rows = stmt.query_map([limit as i64], job_from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// The items of a job that are in one of `states`, in item order.
pub fn job_items_in_states(
    conn: &Connection,
    job_id: i64,
    states: &[&str],
) -> DbResult<Vec<StoredJobItem>> {
    let mut stmt = conn.prepare_cached(
        "SELECT idx, label, payload, state, reason FROM job_item WHERE job_id = ?1 ORDER BY idx",
    )?;
    let rows = stmt.query_map([job_id], |r| {
        Ok(StoredJobItem {
            idx: r.get(0)?,
            label: r.get(1)?,
            payload: r.get(2)?,
            state: r.get(3)?,
            reason: r.get(4)?,
        })
    })?;
    let mut items = Vec::new();
    for row in rows {
        let item = row?;
        if states.contains(&item.state.as_str()) {
            items.push(item);
        }
    }
    Ok(items)
}

/// How many items of a job are in each state.
pub fn job_item_counts(conn: &Connection, job_id: i64) -> DbResult<JobItemCounts> {
    let mut stmt = conn
        .prepare_cached("SELECT state, COUNT(*) FROM job_item WHERE job_id = ?1 GROUP BY state")?;
    let rows = stmt.query_map([job_id], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
    })?;
    let mut counts = JobItemCounts::default();
    for row in rows {
        let (state, n) = row?;
        match state.as_str() {
            "pending" => counts.pending = n,
            "done" => counts.done = n,
            "failed" => counts.failed = n,
            "rolled_back" => counts.rolled_back = n,
            _ => {}
        }
    }
    Ok(counts)
}

/// Deletes a finished job and its items (the history clean-up). Jobs that can still be resumed
/// are never deleted. Returns whether a row was removed.
pub fn delete_finished_job(conn: &Connection, id: i64) -> DbResult<bool> {
    let n = conn
        .prepare_cached(
            "DELETE FROM job WHERE id = ?1 AND state IN ('succeeded', 'failed', 'canceled')",
        )?
        .execute([id])?;
    Ok(n > 0)
}
