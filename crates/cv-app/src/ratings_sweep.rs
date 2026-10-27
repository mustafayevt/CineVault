//! "Fetch missing ratings": the ratings of many titles fetched as one background job.
//!
//! The sweep covers the scraped movies and shows that lack the ratings of a chosen source that
//! can be asked now (a key added after they were scraped), or every scraped title when all
//! ratings are refreshed (see [`SweepScope`]). Each title goes through the same fetch as the
//! title's own Fetch ratings ([`ActionsService::fetch_ratings`]): the stored ratings, the per-source
//! outcomes, the NFO and the "fetching" state on open pages are the same.
//!
//! * One job item is one title, one at a time, with a short pause between titles on top of the
//!   providers' own request lanes, so a large library does not burst through a key's quota.
//! * When a source answers that its request limit is reached (OMDb's free key allows 1,000 a
//!   day; MDBList has its own daily limit), the sweep stops calmly: the remaining titles are left
//!   for later and the summary says how many and why. Nothing fails because of it.
//! * Cancel stops between titles; what was stored stays.

use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use cv_db::repo::get_item;
use cv_db::repo::step_outcome::step_outcome;
use cv_db::{Database, DbError};
use cv_domain::ItemId;
use cv_jobs::{ItemControl, ItemError, ItemWork, JobHandle, JobManager, JobSpec, LaneId, Priority};
use cv_providers::RatingSource;

use crate::actions::{ActionsService, OutcomeKind};
use crate::rating_status::{RatingStatusService, SweepScope};
use crate::step_outcomes::{StepStatus, source_step};
use crate::wording::counted;

/// The name of the job in Activity.
pub const JOB_NAME: &str = "Fetching ratings";

/// The lane of the job: one sweep at a time, apart from the scrapes.
const LANE: &str = "ratings";

/// The pause between two titles, on top of the providers' own request lanes.
const PACE: Duration = Duration::from_millis(250);

/// One title of a sweep.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SweepItem {
    pub id: ItemId,
    /// What Activity shows while the title is worked on.
    pub label: String,
}

impl fmt::Display for SweepItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

/// What a sweep has done so far, and at the end.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SweepSummary {
    /// Titles whose ratings were stored or changed.
    pub updated: usize,
    /// Titles whose ratings were already up to date.
    pub unchanged: usize,
    /// Titles no source had a rating for, or that were skipped (locked, no source chosen).
    pub skipped: usize,
    /// Titles that failed.
    pub failed: usize,
    /// Titles left for later because a source's request limit was reached.
    pub left: usize,
    /// The source whose limit stopped the sweep (`OMDb`, `MDBList`).
    pub limited_by: Option<String>,
    /// The sweep was cancelled.
    pub cancelled: bool,
}

impl SweepSummary {
    /// "Fetched ratings for 41 titles. 3 failed." as the toast says it, with the limit and what
    /// is left when the sweep stopped at one.
    #[must_use]
    pub fn sentence(&self) -> String {
        let mut text = if self.updated == 0 {
            "No new ratings were found.".to_owned()
        } else {
            format!(
                "Fetched ratings for {}.",
                counted(self.updated, "title", "titles")
            )
        };
        if self.failed > 0 {
            text.push_str(&format!(" {} failed.", self.failed));
        }
        if let Some(source) = &self.limited_by {
            text.push_str(&format!(
                " Stopped at the {source} request limit: {} left. Run it again later to fetch the rest.",
                counted(self.left, "title is", "titles are")
            ));
        }
        if self.cancelled {
            text.push_str(" Cancelled.");
        }
        text
    }
}

/// Whether the reason a rating source failed with says its request limit is reached: the
/// provider's "is limiting requests" (HTTP 429 or OMDb's "Request limit reached!") or MDBList's
/// "API limit reached".
#[must_use]
pub fn is_limit_reason(reason: &str) -> bool {
    let lower = reason.to_ascii_lowercase();
    lower.contains("limiting requests")
        || lower.contains("limit reached")
        || lower.contains("too many requests")
}

/// The sources whose request limits can stop a sweep.
const METERED: [RatingSource; 2] = [RatingSource::Omdb, RatingSource::Mdblist];

/// The work of the job: fetch the ratings of one title.
#[derive(Clone)]
pub struct SweepWork {
    actions: Arc<ActionsService>,
    db: Arc<Database>,
    pace: Duration,
    summary: Arc<Mutex<SweepSummary>>,
}

impl SweepWork {
    #[must_use]
    pub fn new(actions: Arc<ActionsService>, db: Arc<Database>) -> Self {
        Self {
            actions,
            db,
            pace: PACE,
            summary: Arc::default(),
        }
    }

    /// The same work without the pause between titles (tests).
    #[must_use]
    pub fn unpaced(mut self) -> Self {
        self.pace = Duration::ZERO;
        self
    }

    /// What the sweep has done so far.
    #[must_use]
    pub fn summary(&self) -> SweepSummary {
        self.summary
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn update(&self, change: impl FnOnce(&mut SweepSummary)) {
        change(&mut self.summary.lock().unwrap_or_else(PoisonError::into_inner));
    }

    /// The metered source that answered the title with its request limit, if one did.
    fn limit_hit(&self, id: ItemId) -> Option<String> {
        METERED.into_iter().find_map(|source| {
            let step = source_step(source.key());
            let outcome = self
                .db
                .read(|conn| step_outcome(conn, id, &step))
                .ok()
                .flatten()?;
            (outcome.status == StepStatus::Failed && is_limit_reason(&outcome.reason))
                .then(|| source.label().to_owned())
        })
    }
}

impl ItemWork for SweepWork {
    type Item = SweepItem;

    async fn process(&self, item: &SweepItem, control: &ItemControl) -> Result<(), ItemError> {
        if control.is_cancelled() {
            self.update(|s| s.cancelled = true);
            return Err(ItemError::Cancelled);
        }
        // After a limit the rest waits for another run: counted, not asked.
        if self.summary().limited_by.is_some() {
            self.update(|s| s.left += 1);
            return Ok(());
        }
        let run = self.actions.fetch_ratings(&[item.id]).await;
        let kind = run.items.first().map(|outcome| outcome.kind);
        let limit = self.limit_hit(item.id);
        self.update(|s| match kind {
            Some(OutcomeKind::Done) => s.updated += 1,
            Some(OutcomeKind::Unchanged) => s.unchanged += 1,
            Some(OutcomeKind::Failed) => s.failed += 1,
            Some(OutcomeKind::Skipped) | None => s.skipped += 1,
        });
        if let Some(source) = limit {
            tracing::info!(%source, "a ratings sweep stopped at a request limit");
            self.update(|s| s.limited_by = Some(source));
        }
        if !self.pace.is_zero() {
            tokio::select! {
                () = tokio::time::sleep(self.pace) => {}
                () = control.cancelled() => {}
            }
        }
        Ok(())
    }

    fn outcome(&self) -> Option<String> {
        let summary = self.summary();
        summary.limited_by.is_some().then(|| summary.sentence())
    }

    fn result(&self) -> Option<String> {
        let summary = self.summary();
        (summary.updated > 0).then(|| format!("ratings for {}", summary.updated))
    }
}

/// The titles of a sweep with the names Activity shows. A title that is gone is left out.
fn items_of(db: &Database, ids: &[ItemId]) -> Vec<SweepItem> {
    ids.iter()
        .filter_map(|&id| {
            let item = db.read(|conn| get_item(conn, id)).ok().flatten()?;
            let title = item.core.title.clone();
            let label = if item.core.year > 0 {
                format!("{title} ({})", item.core.year)
            } else {
                title
            };
            Some(SweepItem { id, label })
        })
        .collect()
}

/// Queues the sweep of `scope` (limited to the titles that lack the ratings of `only`, when
/// given) as one job. `None` when there is nothing to fetch. Must be called inside a tokio
/// runtime or with the jobs runtime set.
///
/// # Errors
/// The library could not be read.
pub fn submit_sweep(
    manager: &JobManager,
    actions: &Arc<ActionsService>,
    status: &RatingStatusService,
    db: &Arc<Database>,
    scope: SweepScope,
    only: Option<RatingSource>,
) -> Result<Option<(JobHandle, SweepWork, usize)>, DbError> {
    let ids = status.sweep_titles(scope, only)?;
    let items = items_of(db, &ids);
    if items.is_empty() {
        return Ok(None);
    }
    let count = items.len();
    let spec = JobSpec::new(
        format!("{JOB_NAME} for {}", counted(count, "title", "titles")),
        LaneId::provider(LANE),
    )
    .priority(Priority::Normal)
    .concurrency(1);
    let work = SweepWork::new(Arc::clone(actions), Arc::clone(db));
    let job = manager.submit(spec, work.clone(), items);
    Ok(Some((job, work, count)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_are_told_apart_from_other_failures() {
        assert!(is_limit_reason(
            "OMDb failed: OMDb is limiting requests; try again in 3600 seconds"
        ));
        assert!(is_limit_reason(
            "MDBList failed: MDBList request failed: API Limit Reached!"
        ));
        assert!(is_limit_reason("OMDb failed: Request limit reached!"));
        assert!(!is_limit_reason(
            "OMDb failed: OMDb request failed: HTTP 503"
        ));
        assert!(!is_limit_reason(
            "MDBList rejected the credentials: invalid key"
        ));
    }

    #[test]
    fn the_summary_says_what_was_fetched_and_what_a_limit_left() {
        let done = SweepSummary {
            updated: 41,
            failed: 2,
            ..SweepSummary::default()
        };
        assert_eq!(done.sentence(), "Fetched ratings for 41 titles. 2 failed.");
        let limited = SweepSummary {
            updated: 1,
            left: 812,
            limited_by: Some("OMDb".to_owned()),
            ..SweepSummary::default()
        };
        assert_eq!(
            limited.sentence(),
            "Fetched ratings for 1 title. Stopped at the OMDb request limit: 812 titles are left. \
             Run it again later to fetch the rest."
        );
        assert_eq!(
            SweepSummary::default().sentence(),
            "No new ratings were found."
        );
    }
}
