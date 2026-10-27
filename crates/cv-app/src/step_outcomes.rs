//! Records what the after-scrape steps did for each title (`item_step_outcome`).
//!
//! The steps are artwork, ratings, collections. Each records one outcome
//! per title with a plain reason, so a step that was skipped, found nothing or needed a key is
//! visible to the lifecycle and the ratings strip instead of silent. A failed write is logged and
//! never fails the step.

use std::time::{SystemTime, UNIX_EPOCH};

use cv_db::Database;
use cv_db::repo::step_outcome::record_step_outcome;
use cv_domain::ItemId;

pub use cv_db::repo::step_outcome::{
    STEP_ARTWORK, STEP_COLLECTIONS, STEP_RATINGS, StepOutcome, StepStatus, source_step,
};

/// The current time, epoch milliseconds.
#[must_use]
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// One outcome to store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepRecord {
    pub step: String,
    pub status: StepStatus,
    pub reason: String,
}

impl StepRecord {
    #[must_use]
    pub fn new(step: impl Into<String>, status: StepStatus, reason: impl Into<String>) -> Self {
        Self {
            step: step.into(),
            status,
            reason: reason.into(),
        }
    }
}

/// Stores one outcome for `item`. A problem is logged, not returned.
pub fn record(db: &Database, item: ItemId, step: &str, status: StepStatus, reason: &str) {
    record_all(db, item, &[StepRecord::new(step, status, reason)]);
}

/// Stores several outcomes for `item` in one write.
pub fn record_all(db: &Database, item: ItemId, records: &[StepRecord]) {
    if records.is_empty() {
        return;
    }
    let at = now_ms();
    let records = records.to_vec();
    let written = db.write(move |tx| {
        for r in &records {
            record_step_outcome(tx, item, &r.step, r.status, &r.reason, at)?;
        }
        Ok(())
    });
    if let Err(error) = written {
        tracing::debug!(item = item.0, %error, "a step outcome was not stored");
    }
}
