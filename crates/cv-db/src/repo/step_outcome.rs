//! What each after-scrape step did for each item: the `item_step_outcome` table.
//!
//! The after-scrape steps (artwork, ratings, collections) record one row per
//! item and step each time they run, replacing the row of the last run. That makes results that
//! used to be silent (a step skipped, a title the source does not know, a missing key) visible to
//! the lifecycle, the stage explanation and the ratings strip. A rating source has its own row
//! next to the step's, named `ratings:<source key>` (see [`source_step`]).
//!
//! The scrape itself has a row too, [`STEP_SCRAPE`]: whether a scrape was tried for the title and
//! how it ended. A title without one has never had a scrape tried (it is just New); a title whose
//! last scrape found no confident match ([`StepStatus::is_scrape_miss`]) is Not matched with that
//! row's reason, until a scrape or a Change match succeeds.
//!
//! Rows go with their item (`ON DELETE CASCADE`), and nothing here touches any other table.

use std::collections::BTreeMap;

use cv_domain::ItemId;
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::error::DbResult;

/// The artwork download of a scrape.
pub const STEP_ARTWORK: &str = "artwork";
/// The ratings fetch of a scrape.
pub const STEP_RATINGS: &str = "ratings";
/// Putting a movie into its collection.
pub const STEP_COLLECTIONS: &str = "collections";
/// The scrape of the title itself: `done` when a match was scraped into it, `not_found` when the
/// search found no confident match (nothing, or only candidates that wait for a decision),
/// `needs_key` when the provider has no key and `failed` when the provider failed.
pub const STEP_SCRAPE: &str = "scrape";

/// The step name of one rating source, for example `ratings:omdb`.
#[must_use]
pub fn source_step(source_key: &str) -> String {
    format!("{STEP_RATINGS}:{source_key}")
}

/// How a step ended for an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum StepStatus {
    /// The step changed something for the item.
    Done,
    /// There was nothing to do: the item has it already.
    Unchanged,
    /// The step did not apply, and the reason says why (a setting, a missing tool or account).
    Skipped,
    /// The step needs an API key that is not set.
    NeedsKey,
    /// The source does not know the title, or has nothing for it.
    NotFound,
    /// The step tried and failed.
    Failed,
}

impl StepStatus {
    /// The text stored in the table.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Done => "done",
            Self::Unchanged => "unchanged",
            Self::Skipped => "skipped",
            Self::NeedsKey => "needs_key",
            Self::NotFound => "not_found",
            Self::Failed => "failed",
        }
    }

    /// Reads the stored text; `None` for anything else.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "done" => Self::Done,
            "unchanged" => Self::Unchanged,
            "skipped" => Self::Skipped,
            "needs_key" => Self::NeedsKey,
            "not_found" => Self::NotFound,
            "failed" => Self::Failed,
            _ => return None,
        })
    }

    /// Whether the step did its job: it changed something or had nothing left to do.
    #[must_use]
    pub const fn is_ok(self) -> bool {
        matches!(self, Self::Done | Self::Unchanged)
    }

    /// For [`STEP_SCRAPE`]: the scrape was tried and found no confident match (the search found
    /// nothing or could not decide, the provider has no key, or it failed).
    #[must_use]
    pub const fn is_scrape_miss(self) -> bool {
        matches!(self, Self::NotFound | Self::NeedsKey | Self::Failed)
    }
}

/// One stored outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepOutcome {
    pub item: ItemId,
    pub step: String,
    pub status: StepStatus,
    /// One plain sentence; empty when there is nothing to add.
    pub reason: String,
    /// When the step ended, epoch milliseconds.
    pub at_ms: i64,
}

fn outcome_of(row: &Row<'_>) -> rusqlite::Result<StepOutcome> {
    let status: String = row.get(2)?;
    Ok(StepOutcome {
        item: ItemId(row.get(0)?),
        step: row.get(1)?,
        // The CHECK constraint allows only the known values; a newer build's value reads as a
        // skip instead of failing the whole read.
        status: StepStatus::parse(&status).unwrap_or(StepStatus::Skipped),
        reason: row.get(3)?,
        at_ms: row.get(4)?,
    })
}

const COLUMNS: &str = "item_id, step, status, reason, at_ms";

/// Stores the outcome of `step` for `item`, replacing the one of the last run. An item that no
/// longer exists is ignored (nothing is stored).
pub fn record_step_outcome(
    conn: &Connection,
    item: ItemId,
    step: &str,
    status: StepStatus,
    reason: &str,
    at_ms: i64,
) -> DbResult<()> {
    conn.prepare_cached(
        "INSERT INTO item_step_outcome (item_id, step, status, reason, at_ms)
         SELECT ?1, ?2, ?3, ?4, ?5 WHERE EXISTS (SELECT 1 FROM item WHERE id = ?1)
         ON CONFLICT (item_id, step) DO UPDATE SET
             status = excluded.status, reason = excluded.reason, at_ms = excluded.at_ms",
    )?
    .execute(params![item.0, step, status.as_str(), reason, at_ms])?;
    Ok(())
}

/// The outcome of one step for one item.
pub fn step_outcome(conn: &Connection, item: ItemId, step: &str) -> DbResult<Option<StepOutcome>> {
    Ok(conn
        .prepare_cached(&format!(
            "SELECT {COLUMNS} FROM item_step_outcome WHERE item_id = ?1 AND step = ?2"
        ))?
        .query_row(params![item.0, step], outcome_of)
        .optional()?)
}

/// Every outcome of one item, ordered by step name.
pub fn step_outcomes(conn: &Connection, item: ItemId) -> DbResult<Vec<StepOutcome>> {
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT {COLUMNS} FROM item_step_outcome WHERE item_id = ?1 ORDER BY step"
    ))?;
    let rows = stmt.query_map(params![item.0], outcome_of)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// The outcomes of many items, grouped by item (items without any are absent).
pub fn step_outcomes_for(
    conn: &Connection,
    items: &[ItemId],
) -> DbResult<BTreeMap<ItemId, Vec<StepOutcome>>> {
    let mut grouped: BTreeMap<ItemId, Vec<StepOutcome>> = BTreeMap::new();
    for chunk in items.chunks(500) {
        let marks = vec!["?"; chunk.len()].join(",");
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM item_step_outcome WHERE item_id IN ({marks}) ORDER BY item_id, step"
        ))?;
        let rows = stmt.query_map(
            rusqlite::params_from_iter(chunk.iter().map(|id| id.0)),
            outcome_of,
        )?;
        for row in rows {
            let row = row?;
            grouped.entry(row.item).or_default().push(row);
        }
    }
    Ok(grouped)
}

/// The items whose `step` ended with `status`, newest first.
pub fn items_with_status(
    conn: &Connection,
    step: &str,
    status: StepStatus,
) -> DbResult<Vec<StepOutcome>> {
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT {COLUMNS} FROM item_step_outcome WHERE step = ?1 AND status = ?2
         ORDER BY at_ms DESC, item_id"
    ))?;
    let rows = stmt.query_map(params![step, status.as_str()], outcome_of)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Forgets the outcome of `step` for `item` (a step that was switched off, or a rescrape that
/// starts over). Returns whether a row was removed.
pub fn clear_step_outcome(conn: &Connection, item: ItemId, step: &str) -> DbResult<bool> {
    let removed = conn
        .prepare_cached("DELETE FROM item_step_outcome WHERE item_id = ?1 AND step = ?2")?
        .execute(params![item.0, step])?;
    Ok(removed > 0)
}

#[cfg(test)]
mod tests {
    use cv_domain::Item;

    use super::*;
    use crate::Database;
    use crate::repo::insert_item;

    fn library() -> (tempfile::TempDir, Database, ItemId) {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(dir.path().join("library.db")).unwrap();
        let id = db
            .write(|tx| insert_item(tx, &Item::movie("Heat")))
            .unwrap();
        (dir, db, id)
    }

    #[test]
    fn an_outcome_reads_back_and_the_next_run_replaces_it() {
        let (_dir, db, id) = library();
        db.write(move |tx| {
            record_step_outcome(
                tx,
                id,
                STEP_RATINGS,
                StepStatus::NeedsKey,
                "no OMDb key",
                10,
            )
        })
        .unwrap();
        let first = db.read(move |c| step_outcome(c, id, STEP_RATINGS)).unwrap();
        assert_eq!(
            first,
            Some(StepOutcome {
                item: id,
                step: "ratings".into(),
                status: StepStatus::NeedsKey,
                reason: "no OMDb key".into(),
                at_ms: 10,
            })
        );

        db.write(move |tx| record_step_outcome(tx, id, STEP_RATINGS, StepStatus::Done, "", 20))
            .unwrap();
        let all = db.read(move |c| step_outcomes(c, id)).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!((all[0].status, all[0].at_ms), (StepStatus::Done, 20));
    }

    #[test]
    fn steps_are_independent_and_listed_by_status() {
        let (_dir, db, id) = library();
        db.write(move |tx| {
            record_step_outcome(tx, id, STEP_ARTWORK, StepStatus::Done, "", 1)?;
            record_step_outcome(tx, id, &source_step("omdb"), StepStatus::NeedsKey, "key", 2)?;
            record_step_outcome(tx, id, STEP_COLLECTIONS, StepStatus::NeedsKey, "tool", 3)
        })
        .unwrap();
        let steps: Vec<String> = db
            .read(|c| step_outcomes(c, id))
            .unwrap()
            .into_iter()
            .map(|o| o.step)
            .collect();
        assert_eq!(steps, ["artwork", "collections", "ratings:omdb"]);
        let needing = db
            .read(|c| items_with_status(c, STEP_COLLECTIONS, StepStatus::NeedsKey))
            .unwrap();
        assert_eq!(needing.len(), 1);
        let grouped = db.read(move |c| step_outcomes_for(c, &[id])).unwrap();
        assert_eq!(grouped[&id].len(), 3);
    }

    #[test]
    fn a_missing_item_stores_nothing_and_a_deleted_item_takes_its_rows_along() {
        let (_dir, db, id) = library();
        db.write(move |tx| {
            record_step_outcome(tx, ItemId(9_999), STEP_ARTWORK, StepStatus::Done, "", 1)?;
            record_step_outcome(tx, id, STEP_ARTWORK, StepStatus::Done, "", 1)
        })
        .unwrap();
        assert!(
            db.read(move |c| step_outcome(c, ItemId(9_999), STEP_ARTWORK))
                .unwrap()
                .is_none()
        );
        db.write(move |tx| {
            tx.execute("DELETE FROM item WHERE id = ?1", [id.0])?;
            Ok(())
        })
        .unwrap();
        assert!(db.read(move |c| step_outcomes(c, id)).unwrap().is_empty());
    }

    #[test]
    fn clearing_removes_one_step() {
        let (_dir, db, id) = library();
        db.write(move |tx| {
            record_step_outcome(tx, id, STEP_ARTWORK, StepStatus::Done, "", 1)?;
            record_step_outcome(tx, id, STEP_RATINGS, StepStatus::Done, "", 1)
        })
        .unwrap();
        assert!(
            db.write(move |tx| clear_step_outcome(tx, id, STEP_ARTWORK))
                .unwrap()
        );
        assert!(
            !db.write(move |tx| clear_step_outcome(tx, id, STEP_ARTWORK))
                .unwrap()
        );
        assert_eq!(db.read(move |c| step_outcomes(c, id)).unwrap().len(), 1);
    }

    #[test]
    fn the_status_text_round_trips() {
        for status in [
            StepStatus::Done,
            StepStatus::Unchanged,
            StepStatus::Skipped,
            StepStatus::NeedsKey,
            StepStatus::NotFound,
            StepStatus::Failed,
        ] {
            assert_eq!(StepStatus::parse(status.as_str()), Some(status));
        }
        assert_eq!(StepStatus::parse("other"), None);
    }
}
