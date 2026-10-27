//! The steps that call the automation services: ratings, aspect ratio, checksums,
//! artwork and clean-up of artwork.
//!
//! Each step takes the titles of its scope, calls the service, and reports one line per title
//! (`done`, `unchanged`, `skipped` or `failed`). A step that cannot run says why:
//!
//! * a tool is missing (FFmpeg): the step is `unavailable`, like the media-info step
//!   without MediaInfo, and says where to install it;
//! * a key, an account, a provider or the program is not set up: the step is `skipped` with a
//!   plain sentence that names the page to set it up on (Providers or Settings).
//!   That is not a failure, and the steps after it still run;
//! * a title fails: the title is `failed`, the step still ran, and the batch's exit code is 1.

use std::sync::Arc;

use cv_media::checksum::ChecksumKind;

use super::Run;
use crate::actions::{ActionRun, Blocker, OutcomeKind};
use crate::automation::model::{ItemResult, PlannedStep, Step, StepReport, StepStatus};
use crate::wording::counted;

/// What a step does, for the dry run: "Would {verb} 3 titles."
fn verb(step: Step) -> &'static str {
    match step {
        Step::AspectRatio => "measure the aspect ratio of",
        Step::Checksum => "calculate the checksums of",
        Step::Ratings => "fetch the ratings of",
        Step::Artwork => "download missing artwork for",
        Step::ArtworkCleanup => "tidy the artwork of",
        Step::Cleanup => "tidy the unwanted files of",
        _ => "work on",
    }
}

/// What a step did, in words: "Fetched ratings".
fn past(step: Step) -> &'static str {
    match step {
        Step::AspectRatio => "Aspect ratio",
        Step::Checksum => "Checksums",
        Step::Ratings => "Ratings",
        Step::Artwork => "Artwork",
        Step::ArtworkCleanup => "Artwork clean-up",
        Step::Cleanup => "Unwanted-file clean-up",
        _ => "The step",
    }
}

/// Turns what an action did into the step's report.
pub(super) fn report_of(step: Step, run: ActionRun, dry_run: bool) -> StepReport {
    let count = |kind| run.count(kind);
    let (done, unchanged, skipped, failed) = (
        count(OutcomeKind::Done),
        count(OutcomeKind::Unchanged),
        count(OutcomeKind::Skipped),
        count(OutcomeKind::Failed),
    );
    let items: Vec<ItemResult> = run
        .items
        .iter()
        .map(|item| {
            let word = match item.kind {
                OutcomeKind::Done if dry_run => "planned",
                OutcomeKind::Done => "done",
                OutcomeKind::Unchanged => "unchanged",
                OutcomeKind::Skipped => "skipped",
                OutcomeKind::Failed => "failed",
            };
            ItemResult::new(Some(item.id.0), &item.title, item.year, word)
                .message(item.message.clone())
        })
        .collect();
    let finish = |status, message: String| {
        let mut report = StepReport::new(step, status, message)
            .count("done", done)
            .count("unchanged", unchanged)
            .count("skipped", skipped)
            .count("failed", failed);
        report.items = items.clone();
        report
    };
    match &run.blocker {
        Some(Blocker::Tool(message)) => return finish(StepStatus::Unavailable, message.clone()),
        Some(Blocker::Setup(message)) => return finish(StepStatus::Skipped, message.clone()),
        None => {}
    }
    if run.items.is_empty() {
        return finish(StepStatus::Skipped, "There is nothing to do.".to_owned());
    }
    if done + unchanged + failed == 0 {
        // Every title was skipped. One reason says it all; several are counted.
        let mut reasons: Vec<&str> = run.items.iter().map(|i| i.message.as_str()).collect();
        reasons.dedup();
        let message = match reasons.as_slice() {
            [only] => (*only).to_owned(),
            [first, ..] => format!(
                "{} skipped. For example: {first}",
                counted(skipped, "title", "titles")
            ),
            [] => "There is nothing to do.".to_owned(),
        };
        return finish(StepStatus::Skipped, message);
    }
    let status = if dry_run {
        StepStatus::Planned
    } else {
        StepStatus::Done
    };
    let message = format!(
        "{}: {done} {}, {unchanged} already fine, {skipped} skipped, {failed} failed.",
        past(step),
        if dry_run { "planned" } else { "done" }
    );
    finish(status, message)
}

impl Run<'_> {
    /// The steps that work title by title: aspect ratio, checksums, ratings,
    /// artwork and artwork clean-up.
    pub(super) async fn action(&mut self, planned: &PlannedStep) -> StepReport {
        let step = planned.step;
        let ids = match self.step_titles(planned).await {
            Ok(ids) => ids,
            Err(message) => return StepReport::new(step, StepStatus::Failed, message),
        };
        if ids.is_empty() {
            let why = match planned.scope {
                Some(scope) => format!("There is no title in the {} scope.", scope.name()),
                None => "No title is selected.".to_owned(),
            };
            return StepReport::new(step, StepStatus::Skipped, why);
        }
        let actions = Arc::clone(self.services().actions());
        if step == Step::Cleanup {
            let dry = self.dry();
            let run = actions.cleanup_unwanted(&ids, dry).await;
            return report_of(step, run, dry);
        }
        if self.dry() {
            if step == Step::ArtworkCleanup {
                let run = actions.cleanup_artwork(&ids, true).await;
                return report_of(step, run, true);
            }
            return StepReport::new(
                step,
                StepStatus::Planned,
                format!(
                    "Would {} {}.",
                    verb(step),
                    counted(ids.len(), "title", "titles")
                ),
            )
            .count("titles", ids.len());
        }
        let run = match step {
            Step::AspectRatio => actions.detect_aspect(&ids).await,
            Step::Checksum => {
                let kind = planned
                    .options
                    .arg("type")
                    .and_then(ChecksumKind::parse)
                    .unwrap_or(ChecksumKind::Crc32);
                actions.checksums(&ids, kind).await
            }
            Step::Ratings => actions.fetch_ratings(&ids).await,
            Step::Artwork => {
                let only: Vec<String> = planned
                    .options
                    .arg("scraper")
                    .map(|list| {
                        list.split(',')
                            .map(|name| name.trim().to_lowercase())
                            .filter(|name| !name.is_empty())
                            .collect()
                    })
                    .unwrap_or_default();
                if planned.options.flag("redownload", false) {
                    actions.redownload_artwork(&ids).await
                } else {
                    actions.download_artwork(&ids, &only).await
                }
            }
            Step::ArtworkCleanup => actions.cleanup_artwork(&ids, false).await,
            other => {
                return StepReport::new(
                    other,
                    StepStatus::Unavailable,
                    "This step is not part of batch runs in this version.",
                );
            }
        };
        report_of(step, run, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::ItemOutcome;
    use cv_domain::{Item, ItemId};

    fn outcome(kind: OutcomeKind, message: &str) -> ItemOutcome {
        let mut item = Item::movie("Heat");
        item.id = Some(ItemId(7));
        ItemOutcome::of(&item, kind, message)
    }

    fn run_of(items: Vec<ItemOutcome>, blocker: Option<Blocker>) -> ActionRun {
        ActionRun { items, blocker }
    }

    #[test]
    fn a_missing_tool_is_unavailable_and_a_missing_setup_is_a_skip() {
        let tool = report_of(
            Step::AspectRatio,
            run_of(
                vec![],
                Some(Blocker::Tool("FFmpeg is not installed.".into())),
            ),
            false,
        );
        assert_eq!(tool.status, StepStatus::Unavailable);
        let setup = report_of(
            Step::Cleanup,
            run_of(vec![], Some(Blocker::Setup("Choose a program.".into()))),
            false,
        );
        assert_eq!(setup.status, StepStatus::Skipped);
        assert_eq!(setup.message, "Choose a program.");
    }

    #[test]
    fn titles_that_were_all_skipped_for_one_reason_say_that_reason() {
        let report = report_of(
            Step::Ratings,
            run_of(
                vec![
                    outcome(
                        OutcomeKind::Skipped,
                        "Add a key in Settings, Scraping and providers.",
                    ),
                    outcome(
                        OutcomeKind::Skipped,
                        "Add a key in Settings, Scraping and providers.",
                    ),
                ],
                None,
            ),
            false,
        );
        assert_eq!(report.status, StepStatus::Skipped);
        assert_eq!(
            report.message,
            "Add a key in Settings, Scraping and providers."
        );
        assert_eq!(report.counts["skipped"], 2);
    }

    #[test]
    fn a_failed_title_is_reported_and_the_step_still_counts_as_run() {
        let report = report_of(
            Step::Checksum,
            run_of(
                vec![
                    outcome(OutcomeKind::Done, "Calculated crc32."),
                    outcome(OutcomeKind::Failed, "The file could not be read."),
                ],
                None,
            ),
            false,
        );
        assert_eq!(report.status, StepStatus::Done);
        assert_eq!(report.counts["done"], 1);
        assert_eq!(report.counts["failed"], 1);
        assert!(report.has_failed_items());
        assert_eq!(report.items[0].status, "done");
        assert_eq!(report.items[1].status, "failed");
    }

    #[test]
    fn a_dry_run_marks_what_would_happen() {
        let report = report_of(
            Step::ArtworkCleanup,
            run_of(vec![outcome(OutcomeKind::Done, "1 file would move.")], None),
            true,
        );
        assert_eq!(report.status, StepStatus::Planned);
        assert_eq!(report.items[0].status, "planned");
    }
}
