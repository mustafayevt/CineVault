//! The `assignMovieSet` step: puts each movie of its scope into its TMDB collection, creating the
//! collections the library lacks. It is "Find collections" of the collection pages
//! ([`CollectionService::find`]), so movies with an item lock or a locked collection field stay
//! as they are, and the collection NFOs are written again.
//!
//! Without a provider that knows collections (TMDB without a key) the step is `skipped` with a
//! sentence that names the Providers page, like the other steps that need a key.

use std::sync::Arc;

use cv_db::repo::get_summary;
use cv_domain::ItemId;

use super::{Run, blocking};
use crate::automation::model::{EntityKind, ItemResult, PlannedStep, Step, StepReport, StepStatus};
use crate::collections::{CollectionError, CollectionService, FindReport};
use crate::nfo::NfoSettings;
use crate::wording::counted;

/// The title and year of each movie, for the report lines.
async fn labels(run: &Run<'_>, ids: Vec<ItemId>) -> Vec<(ItemId, String, u16)> {
    let db = Arc::clone(run.services().db());
    blocking(move || {
        ids.into_iter()
            .map(|id| {
                let summary = db.read(|c| get_summary(c, id)).ok().flatten();
                let (title, year) = summary.map_or((String::new(), 0), |s| (s.title, s.year));
                (id, title, year)
            })
            .collect()
    })
    .await
    .unwrap_or_default()
}

/// One report line per movie of the run.
fn lines(report: &FindReport, labels: &[(ItemId, String, u16)]) -> Vec<ItemResult> {
    labels
        .iter()
        .map(|(id, title, year)| {
            let line = |status: &str| ItemResult::new(Some(id.0), title, *year, status);
            if report.assigned.iter().any(|a| a.movie == *id) {
                line("done")
            } else if let Some(failure) = report.failed.iter().find(|f| f.movie == *id) {
                line("failed").message(failure.message.clone())
            } else {
                line("unchanged")
            }
        })
        .collect()
}

impl Run<'_> {
    /// Puts the movies of the step's scope into their collections.
    pub(super) async fn movie_set(&mut self, planned: &PlannedStep) -> StepReport {
        let step = Step::MovieSet;
        if self.kind != EntityKind::Movie {
            return StepReport::new(
                step,
                StepStatus::Skipped,
                "Collections are assigned to movies only.",
            );
        }
        let ids = match self.step_titles(planned).await {
            Ok(ids) => ids,
            Err(message) => return StepReport::new(step, StepStatus::Failed, message),
        };
        if ids.is_empty() {
            let why = match planned.scope {
                Some(scope) => format!("There is no movie in the {} scope.", scope.name()),
                None => "No movie is selected.".to_owned(),
            };
            return StepReport::new(step, StepStatus::Skipped, why);
        }
        let matching = Arc::clone(self.services().matching());
        let context = matching.context();
        let providers = context.providers.refs();
        if !providers.iter().any(|p| p.offers_collections()) {
            return StepReport::new(
                step,
                StepStatus::Skipped,
                CollectionError::NoProvider.to_string(),
            );
        }
        if self.dry() {
            return StepReport::new(
                step,
                StepStatus::Planned,
                format!(
                    "Would look up the collections of {}.",
                    counted(ids.len(), "movie", "movies")
                ),
            )
            .count("titles", ids.len());
        }

        let values = self.services().settings().snapshot().values;
        let nfo = NfoSettings::from_values(&values);
        let service = CollectionService::new(
            Arc::clone(self.services().db()),
            Arc::clone(&self.engine.nfo),
        );
        let outcome = match service
            .find(&providers, &ids, &context.settings.request, &nfo)
            .await
        {
            Ok(outcome) => outcome,
            Err(error @ CollectionError::NoProvider) => {
                return StepReport::new(step, StepStatus::Skipped, error.to_string());
            }
            Err(error) => return StepReport::new(step, StepStatus::Failed, error.to_string()),
        };
        let report = &outcome.report;
        let labels = labels(self, ids).await;
        let mut message = format!(
            "Collections: {} assigned, {} already in theirs, {} in no collection, {} \
             locked, {} failed.",
            counted(report.assigned.len(), "movie", "movies"),
            report.unchanged,
            report.no_collection,
            report.locked,
            report.failed.len()
        );
        if !report.created.is_empty() {
            message.push_str(&format!(
                " {} created.",
                counted(report.created.len(), "collection", "collections")
            ));
        }
        if let Some(problem) = &outcome.nfo_problem {
            message.push_str(&format!(" The collection NFOs: {problem}"));
        }
        let mut step_report = StepReport::new(step, StepStatus::Done, message)
            .count("assigned", report.assigned.len())
            .count("created", report.created.len())
            .count("unchanged", report.unchanged as usize)
            .count("no_collection", report.no_collection as usize)
            .count("locked", report.locked as usize)
            .count("failed", report.failed.len())
            .count("nfo_files_written", outcome.nfo_files_written as usize);
        step_report.items = lines(report, &labels);
        step_report
    }
}
