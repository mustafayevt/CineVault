//! The review queue: matches that need a human decision.
//!
//! Matching never waits for the user. When the automatic-acceptance rules cannot decide, the
//! matcher writes a `review_queue` row with the reason, the best score, the candidate list and the
//! score breakdown, and marks the item `needs_review`. The inbox reads these rows; resolving one
//! updates the row and the item together.
//!
//! - At most one `pending` row exists per item. Queueing an item that is already pending
//!   replaces that row's contents, so a re-run of matching refreshes the candidates instead of
//!   stacking rows.
//! - `accepted` and `dismissed` rows are kept as history. A `dismissed` row means "not a
//!   catalogued title": [`enqueue`] refuses to queue that item again, so later scans and matching
//!   runs leave it alone until the user [`reopen`]s it.
//! - Nothing here deletes a library row.
//!
//! The candidate list and the breakdown are opaque JSON to this crate; `cv-match` defines their
//! shape (`ReviewCandidate`).

use cv_domain::{ItemId, ReviewState};
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde_json::Value;

use super::flags::{get_flags, set_review_state};
use super::items::item_exists;
use crate::error::{DbError, DbResult};

/// Where a review row stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReviewQueueState {
    /// Waiting for a decision.
    Pending,
    /// A candidate was accepted (by the user, a bulk accept or a later automatic match).
    Accepted,
    /// Left for later; the item stays in `needs_review` but the row is closed.
    Skipped,
    /// Ignored: not a catalogued title. Never queued again.
    Dismissed,
}

impl ReviewQueueState {
    /// The stable text stored in the `state` column.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Accepted => "accepted",
            Self::Skipped => "skipped",
            Self::Dismissed => "dismissed",
        }
    }

    /// Parses the stored text.
    pub fn parse(text: &str) -> Option<Self> {
        [
            Self::Pending,
            Self::Accepted,
            Self::Skipped,
            Self::Dismissed,
        ]
        .into_iter()
        .find(|s| s.as_str() == text)
    }

    /// The review state the item takes when a row ends in this state.
    const fn item_state(self) -> ReviewState {
        match self {
            Self::Pending | Self::Skipped => ReviewState::NeedsReview,
            Self::Accepted | Self::Dismissed => ReviewState::Resolved,
        }
    }
}

/// A match to queue for review.
#[derive(Debug, Clone, PartialEq)]
pub struct NewReview {
    pub item_id: ItemId,
    /// The reason shown in the inbox, for example `2 strong candidates`.
    pub reason: String,
    /// Confidence of the best candidate, `None` when there was none.
    pub best_score: Option<f64>,
    /// JSON array of candidates, best first.
    pub candidates: Value,
    /// JSON breakdown of the best candidate.
    pub score_breakdown: Option<Value>,
}

/// What [`enqueue`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnqueueOutcome {
    /// A new pending row (its id).
    Queued(i64),
    /// The item was already pending; the row was refreshed (its id).
    Refreshed(i64),
    /// The item was dismissed earlier, so nothing was written.
    Dismissed,
}

impl EnqueueOutcome {
    /// The row id, when one was written.
    pub fn review_id(self) -> Option<i64> {
        match self {
            Self::Queued(id) | Self::Refreshed(id) => Some(id),
            Self::Dismissed => None,
        }
    }
}

/// A stored review row.
#[derive(Debug, Clone, PartialEq)]
pub struct ReviewRow {
    pub id: i64,
    pub item_id: ItemId,
    pub reason: String,
    pub state: ReviewQueueState,
    pub best_score: Option<f64>,
    pub candidates: Value,
    pub score_breakdown: Option<Value>,
    pub created_ms: i64,
    pub resolved_ms: Option<i64>,
}

const COLUMNS: &str = "id, item_id, reason, state, best_score, candidates, score_breakdown, \
                       created_ms, resolved_ms";

fn row_of(r: &Row<'_>) -> DbResult<ReviewRow> {
    let state: String = r.get(3)?;
    let candidates: String = r.get(5)?;
    let breakdown: Option<String> = r.get(6)?;
    Ok(ReviewRow {
        id: r.get(0)?,
        item_id: ItemId(r.get(1)?),
        reason: r.get(2)?,
        state: ReviewQueueState::parse(&state)
            .ok_or_else(|| DbError::InvalidData(format!("unknown review queue state `{state}`")))?,
        best_score: r.get(4)?,
        candidates: serde_json::from_str(&candidates)?,
        score_breakdown: breakdown.as_deref().map(serde_json::from_str).transpose()?,
        created_ms: r.get(7)?,
        resolved_ms: r.get(8)?,
    })
}

fn query_rows(
    conn: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> DbResult<Vec<ReviewRow>> {
    let mut stmt = conn.prepare_cached(sql)?;
    let mut rows = stmt.query(params)?;
    let mut out = Vec::new();
    while let Some(r) = rows.next()? {
        out.push(row_of(r)?);
    }
    Ok(out)
}

/// Queues an item for review and marks it `needs_review`.
///
/// An item that is already pending has its row refreshed; an item that was dismissed is not
/// queued again.
///
/// # Errors
/// Fails when the item does not exist or the database fails.
pub fn enqueue(conn: &Connection, review: &NewReview) -> DbResult<EnqueueOutcome> {
    if !item_exists(conn, review.item_id)? {
        return Err(DbError::ItemNotFound(review.item_id.0));
    }
    if is_dismissed(conn, review.item_id)? {
        return Ok(EnqueueOutcome::Dismissed);
    }
    let candidates = serde_json::to_string(&review.candidates)?;
    let breakdown = review
        .score_breakdown
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;

    let pending: Option<i64> = conn
        .prepare_cached("SELECT id FROM review_queue WHERE item_id = ?1 AND state = 'pending'")?
        .query_row([review.item_id.0], |r| r.get(0))
        .optional()?;
    let outcome = if let Some(id) = pending {
        conn.prepare_cached(
            "UPDATE review_queue
                SET reason = ?2, best_score = ?3, candidates = ?4, score_breakdown = ?5
              WHERE id = ?1",
        )?
        .execute(params![
            id,
            review.reason,
            review.best_score,
            candidates,
            breakdown
        ])?;
        EnqueueOutcome::Refreshed(id)
    } else {
        conn.prepare_cached(
            "INSERT INTO review_queue (item_id, reason, best_score, candidates, score_breakdown)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )?
        .execute(params![
            review.item_id.0,
            review.reason,
            review.best_score,
            candidates,
            breakdown
        ])?;
        EnqueueOutcome::Queued(conn.last_insert_rowid())
    };
    set_review_state(conn, review.item_id, ReviewState::NeedsReview)?;
    Ok(outcome)
}

/// One review row by id.
pub fn get_review(conn: &Connection, id: i64) -> DbResult<Option<ReviewRow>> {
    let sql = format!("SELECT {COLUMNS} FROM review_queue WHERE id = ?1");
    Ok(query_rows(conn, &sql, [id])?.into_iter().next())
}

/// The pending row of an item.
pub fn pending_for_item(conn: &Connection, item_id: ItemId) -> DbResult<Option<ReviewRow>> {
    let sql =
        format!("SELECT {COLUMNS} FROM review_queue WHERE item_id = ?1 AND state = 'pending'");
    Ok(query_rows(conn, &sql, [item_id.0])?.into_iter().next())
}

/// Pending rows for the inbox: the most confident first (those are the quick accepts), then by id.
/// Rows without a score come last.
pub fn list_pending(conn: &Connection, limit: u32, offset: u32) -> DbResult<Vec<ReviewRow>> {
    let sql = format!(
        "SELECT {COLUMNS} FROM review_queue WHERE state = 'pending'
         ORDER BY best_score IS NULL, best_score DESC, id LIMIT ?1 OFFSET ?2"
    );
    query_rows(conn, &sql, params![limit, offset])
}

/// Pending rows whose best candidate is at or above `min_score` (the bulk-accept set), best first.
pub fn pending_at_or_above(conn: &Connection, min_score: f64) -> DbResult<Vec<ReviewRow>> {
    let sql = format!(
        "SELECT {COLUMNS} FROM review_queue WHERE state = 'pending' AND best_score >= ?1 - 1e-9
         ORDER BY best_score DESC, id"
    );
    query_rows(conn, &sql, [min_score])
}

/// How many items wait for review (the sidebar count).
pub fn count_pending(conn: &Connection) -> DbResult<u64> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM review_queue WHERE state = 'pending'",
        [],
        |r| r.get(0),
    )?;
    Ok(u64::try_from(n).unwrap_or(0))
}

/// Whether the user dismissed this item. Matching and scans skip such items.
pub fn is_dismissed(conn: &Connection, item_id: ItemId) -> DbResult<bool> {
    Ok(conn
        .prepare_cached(
            "SELECT EXISTS (SELECT 1 FROM review_queue WHERE item_id = ?1 AND state = 'dismissed')",
        )?
        .query_row([item_id.0], |r| r.get(0))?)
}

/// The ids of every dismissed item.
pub fn dismissed_item_ids(conn: &Connection) -> DbResult<Vec<ItemId>> {
    let mut stmt = conn.prepare_cached(
        "SELECT DISTINCT item_id FROM review_queue WHERE state = 'dismissed' ORDER BY item_id",
    )?;
    let ids = stmt
        .query_map([], |r| Ok(ItemId(r.get(0)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ids)
}

/// Closes the pending row of an item with `state` and sets the item's review state to match
/// (`accepted` and `dismissed` resolve it; `skipped` leaves it in `needs_review`). Returns the
/// closed row id, or `None` when the item had no pending row (nothing changes then).
///
/// # Errors
/// Passing [`ReviewQueueState::Pending`] is rejected.
pub fn resolve_pending(
    conn: &Connection,
    item_id: ItemId,
    state: ReviewQueueState,
) -> DbResult<Option<i64>> {
    if state == ReviewQueueState::Pending {
        return Err(DbError::InvalidData(
            "a review row cannot be resolved to `pending`".to_owned(),
        ));
    }
    let id: Option<i64> = conn
        .prepare_cached(
            "UPDATE review_queue
                SET state = ?2, resolved_ms = CAST(unixepoch('subsec') * 1000 AS INTEGER)
              WHERE item_id = ?1 AND state = 'pending'
          RETURNING id",
        )?
        .query_row(params![item_id.0, state.as_str()], |r| r.get(0))
        .optional()?;
    if id.is_some() {
        set_review_state(conn, item_id, state.item_state())?;
    }
    Ok(id)
}

/// Closes the item's pending row as `accepted` and resolves the item. An item flagged
/// `needs_review` without a pending row (a skipped one) goes back to `none`; any other state is
/// left alone, so an item that was never queued is not touched.
pub fn close_accepted(conn: &Connection, item_id: ItemId) -> DbResult<()> {
    if resolve_pending(conn, item_id, ReviewQueueState::Accepted)?.is_none()
        && get_flags(conn, item_id)?.review_state == ReviewState::NeedsReview
    {
        set_review_state(conn, item_id, ReviewState::None)?;
    }
    Ok(())
}

/// Ignores an item ("not a catalogued title"): closes its pending row as `dismissed`, or records a
/// closed row when it had none (ignored straight from the library), and marks the item
/// `resolved`. Returns the row id. An item that is already dismissed is left as it is.
///
/// # Errors
/// Fails when the item does not exist or the database fails.
pub fn dismiss(conn: &Connection, item_id: ItemId) -> DbResult<i64> {
    if !item_exists(conn, item_id)? {
        return Err(DbError::ItemNotFound(item_id.0));
    }
    if let Some(id) = resolve_pending(conn, item_id, ReviewQueueState::Dismissed)? {
        return Ok(id);
    }
    let existing: Option<i64> = conn
        .prepare_cached(
            "SELECT id FROM review_queue WHERE item_id = ?1 AND state = 'dismissed' LIMIT 1",
        )?
        .query_row([item_id.0], |r| r.get(0))
        .optional()?;
    if let Some(id) = existing {
        return Ok(id);
    }
    conn.prepare_cached(
        "INSERT INTO review_queue (item_id, reason, state, resolved_ms)
         VALUES (?1, 'Ignored', 'dismissed', CAST(unixepoch('subsec') * 1000 AS INTEGER))",
    )?
    .execute([item_id.0])?;
    let id = conn.last_insert_rowid();
    set_review_state(conn, item_id, ReviewState::Resolved)?;
    Ok(id)
}

/// Puts a closed row back to `pending` (the Undo of an accept or an ignore) and marks the item
/// `needs_review` again. Fails with [`DbError::InvalidData`] when the row is already pending or
/// the item has another pending row.
pub fn reopen(conn: &Connection, id: i64) -> DbResult<()> {
    let row = get_review(conn, id)?
        .ok_or_else(|| DbError::InvalidData(format!("review row {id} does not exist")))?;
    if row.state == ReviewQueueState::Pending {
        return Err(DbError::InvalidData(format!(
            "review row {id} is already pending"
        )));
    }
    if pending_for_item(conn, row.item_id)?.is_some() {
        return Err(DbError::InvalidData(format!(
            "item {} already has a pending review row",
            row.item_id.0
        )));
    }
    conn.prepare_cached(
        "UPDATE review_queue SET state = 'pending', resolved_ms = NULL WHERE id = ?1",
    )?
    .execute([id])?;
    set_review_state(conn, row.item_id, ReviewState::NeedsReview)?;
    Ok(())
}

#[cfg(test)]
mod tests;
