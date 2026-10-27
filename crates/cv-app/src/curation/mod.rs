//! Day-to-day curation of the library: watched state and the NEW flag.
//!
//! Spec: `metadata-editing` ("Watched state and play counts", "New flag management").
//!
//! * [`WatchService`] marks movies, episodes, seasons and shows watched or not watched. A season
//!   or a show is its episodes. The reply carries the previous state of every title that changed,
//!   so an Undo puts back exactly those titles with their old play counts. The NFO files of the
//!   changed titles are rewritten.
//! * [`NewFlagService`] clears (or sets again) the NEW flag of a selection, of everything or of
//!   one kind, and returns the titles it changed for Undo.
//!
//! The NFO is a copy of the library values, so a failed NFO write never undoes the change: it is
//! reported next to the result.

mod new_flags;
#[cfg(test)]
mod tests;
mod watched;

use cv_db::DbError;
use cv_domain::ItemId;

pub use new_flags::{NewFlagReport, NewFlagService};
pub use watched::{WatchReport, WatchService};

use crate::edit::EditError;
use crate::nfo::{NfoScope, NfoService, NfoSettings, UnitStatus};

/// Rewrites the NFO files of `ids`. Returns the files written and the first problem, if any.
pub(crate) fn rewrite_nfo(
    nfo: &NfoService,
    ids: &[ItemId],
    settings: &NfoSettings,
) -> (u32, Option<String>) {
    if ids.is_empty() {
        return (0, None);
    }
    match nfo.rewrite(&NfoScope::Items(ids.to_vec()), settings) {
        Ok(report) => {
            let problem = report.units.iter().find_map(|unit| match &unit.status {
                UnitStatus::Failed { message } => Some(message.clone()),
                _ => None,
            });
            (report.totals.files_written, problem)
        }
        Err(error) => (0, Some(error.to_string())),
    }
}

pub(crate) fn not_found(error: DbError) -> EditError {
    match error {
        DbError::ItemNotFound(_) => EditError::NotFound,
        other => EditError::Db(other),
    }
}

pub(crate) fn now_millis() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
