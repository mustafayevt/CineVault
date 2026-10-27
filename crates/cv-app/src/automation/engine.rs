//! The batch engine: runs a [`Plan`] over the app's services. One engine serves both front ends.
//!
//! * The command line turns a [`BatchRequest`] into a plan with [`BatchRequest::plan`]: update,
//!   scrape, media info, aspect ratio, rename, NFO, whatever order
//!   the flags were given in.
//! * The app builds a plan for a selection action with `ItemAction::plan`.
//!
//! Every step reads the library as the steps before it left it:
//!
//! * **update** rescans the selected data sources and remembers the titles it found;
//! * **scrape**, **rename** and the other steps with a `new` scope then work on those titles
//!   (without an update step in the run: on the titles the library flags as NEW);
//! * **dry run** plans every step and reports what it would do; no file, setting or library row
//!   changes, and no job is started.
//!
//! The engine borrows the shared services (`AppServices`), so the same code runs in the
//! command-line binary (which boots them itself) and inside the desktop app (which receives the
//! request over local IPC). Batches run one at a time.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use cv_db::Database;
use cv_db::repo::filter::{Filter, ItemFlag};
use cv_db::repo::media_info::pending_media_info;
use cv_db::repo::rename_journal::PathObserver;
use cv_db::repo::{ItemQuery, get_item, get_summary, list_items, review};
use cv_domain::ItemId;
use cv_scan::rescan::Skipped;

mod movie_sets;
mod steps;

use super::model::{
    BatchReport, BatchRequest, CredentialSummary, EntityKind, ItemResult, Plan, PlannedStep, Scope,
    SelectionSpec, Step, StepReport, StepStatus,
};
use super::selection::{self, Selection};
use crate::matching::batch::{BatchMode, MatchTarget, plan_batch};
use crate::nfo::report::{FileOutcome, UnitStatus};
use crate::nfo::{NfoScope, NfoService, NfoSettings};
use crate::organize::{
    AppliedStatus, ApplyRequest, ConflictCode, ConflictRow, OrganizeService, PreviewRequest,
    RowStatus, Scope as OrganizeScope,
};
use crate::rename_settings::{RenameSettings, apply_nfo_choices};
use crate::scanning::{MediaInfoStatus, ReloadError};
use crate::services::AppServices;
use crate::wording::counted;

/// The longest a batch waits for the NFO and artwork steps that follow its scrape.
const FOLLOW_UP_WAIT: std::time::Duration = std::time::Duration::from_secs(600);

/// Why a rename step did not run.
const FOLLOW_UPS_BUSY: &str = "The NFO or artwork of a scraped title is still being written, so \
                               nothing was renamed. Run the rename again once the scrape's steps \
                               are over.";

/// A message about what the engine is doing, for a progress display.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Progress {
    /// The step the message belongs to; `None` for the run as a whole.
    pub step: Option<Step>,
    pub message: String,
    /// How the step ended; set on the message that closes a step, so a listener (the job behind a
    /// selection action) can follow the steps one by one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<StepStatus>,
}

/// Receives [`Progress`] while a batch runs.
pub type ProgressSink<'a> = &'a (dyn Fn(Progress) + Send + Sync);

/// What the engine adds to the app's services.
#[derive(Default)]
pub struct EngineOptions {
    /// Told about every move a rename makes, so the poster index follows the files. The
    /// desktop app passes its image sources here.
    pub path_observer: Option<Arc<dyn PathObserver>>,
    /// The longest a rename step waits for the NFO and artwork steps of a scrape; ten minutes
    /// when not set. Tests shorten it.
    pub follow_up_wait: Option<std::time::Duration>,
}

/// Runs batches over the app's services.
pub struct BatchEngine {
    services: Arc<AppServices>,
    organize: Arc<OrganizeService>,
    nfo: Arc<NfoService>,
    /// Batches run one at a time: two renames over one library would race.
    gate: tokio::sync::Mutex<()>,
    /// See [`EngineOptions::follow_up_wait`].
    follow_up_wait: std::time::Duration,
}

impl BatchEngine {
    #[must_use]
    pub fn new(services: Arc<AppServices>, options: EngineOptions) -> Arc<Self> {
        // The command line renames what a script asks it to, scraped or not (its own contract).
        let mut organize = OrganizeService::new(Arc::clone(services.db()))
            .planning_unscraped()
            .with_changed_hook(services.organize_state().hook())
            .with_bin(crate::library_ops::PolicyBin::os(
                Arc::clone(services.settings())
                    as Arc<dyn crate::deleted_files::DeletedFilesSource>,
            ))
            .with_choice(Arc::clone(services.settings())
                as Arc<dyn crate::deleted_files::DeletedFilesSource>);
        if let Some(observer) = options.path_observer {
            organize = organize.with_path_observer(observer);
        }
        let location = services.location();
        let nfo = NfoService::new(Arc::clone(services.db()), location.nfo_undo_dir());
        Arc::new(Self {
            services,
            organize: Arc::new(organize),
            nfo: Arc::new(nfo),
            gate: tokio::sync::Mutex::new(()),
            follow_up_wait: options.follow_up_wait.unwrap_or(FOLLOW_UP_WAIT),
        })
    }

    /// The services the engine runs over.
    #[must_use]
    pub fn services(&self) -> &Arc<AppServices> {
        &self.services
    }

    /// Runs the batch. Never fails as a whole: a batch that cannot start is a report with
    /// [`BatchReport::error`] set, and a step that fails is a failed [`StepReport`].
    pub async fn run(&self, request: &BatchRequest, sink: ProgressSink<'_>) -> BatchReport {
        self.run_plan(&request.plan(), sink).await
    }

    /// Runs the steps of the plan in the order they are listed. Failures are reported as in
    /// [`BatchEngine::run`].
    pub async fn run_plan(&self, plan: &Plan, sink: ProgressSink<'_>) -> BatchReport {
        let _turn = self.gate.lock().await;
        let Some(first) = plan.steps.first() else {
            return BatchReport::failed_for(
                plan.kind,
                plan.dry_run,
                "No step was asked for. Choose at least one, such as --update-all, --scrape-new \
                 or --rename-all.",
            );
        };
        let selection = match self
            .select(plan.kind, &first.selection, &first.titles)
            .await
        {
            Ok(selection) => selection,
            Err(message) => return BatchReport::failed_for(plan.kind, plan.dry_run, message),
        };
        sink(Progress {
            step: None,
            message: format!(
                "{} selected{}",
                plural(selection.items.len(), plan.kind),
                if selection.narrowed {
                    " by the given selection"
                } else {
                    ""
                }
            ),
            status: None,
        });
        let mut report = BatchReport {
            kind: plan.kind,
            dry_run: plan.dry_run,
            forwarded: false,
            selection: selection.summary(),
            steps: Vec::new(),
            credentials: Vec::new(),
            error: None,
        };
        if plan
            .steps
            .iter()
            .any(|planned| planned.step == Step::Scrape)
        {
            report.credentials = self.credentials();
        }
        let mut run = Run {
            engine: self,
            kind: plan.kind,
            dry_run: plan.dry_run,
            sink,
            selection,
            chosen: (first.selection.clone(), first.titles.clone()),
            update_ran: false,
            found: Vec::new(),
            new_cache: None,
        };
        let mut stopped_by: Option<Step> = None;
        for planned in &plan.steps {
            let step = planned.step;
            if let Some(earlier) = stopped_by {
                report.steps.push(StepReport::new(
                    step,
                    StepStatus::Skipped,
                    format!("Not run because the {earlier} step failed."),
                ));
                continue;
            }
            run.say(step, "starting");
            let done = run.step(planned).await;
            run.say_done(step, &done.message, done.status);
            if done.status == StepStatus::Failed {
                stopped_by = Some(step);
            }
            report.steps.push(done);
        }
        report.selection = run.selection.summary();
        report
    }

    async fn select(
        &self,
        kind: EntityKind,
        spec: &SelectionSpec,
        titles: &[String],
    ) -> Result<Selection, String> {
        let db = Arc::clone(self.services.db());
        let settings = Arc::clone(self.services.settings());
        let (spec, titles) = (spec.clone(), titles.to_vec());
        let mut picked = blocking(move || selection::resolve(&db, &settings, kind, &spec))
            .await?
            .map_err(|error| error.to_string())?;
        if !titles.is_empty() {
            let wanted: Vec<String> = titles.iter().map(|t| t.trim().to_lowercase()).collect();
            picked
                .items
                .retain(|item| wanted.contains(&item.title.trim().to_lowercase()));
            picked.narrowed = true;
        }
        Ok(picked)
    }

    fn credentials(&self) -> Vec<CredentialSummary> {
        credential_summaries(self.services.providers())
    }
}

/// The providers' credential status: how important each is, its state (`active`, `missing` or
/// `rejected`) and whether it is ready. It is built from [`cv_providers::CredentialStatus`]
/// only, so no key can reach a report.
#[must_use]
pub fn credential_summaries(
    service: &crate::provider_credentials::ProviderCredentialsService,
) -> Vec<CredentialSummary> {
    let Ok(statuses) = service.list() else {
        return Vec::new();
    };
    statuses
        .into_iter()
        .map(|status| CredentialSummary {
            provider: status.provider.to_owned(),
            name: status.display_name.to_owned(),
            importance: snake_case_of(status.importance),
            state: snake_case_of(status.state),
            active: status.active,
        })
        .collect()
}

fn snake_case_of(value: impl serde::Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn plural(count: usize, kind: EntityKind) -> String {
    let noun = match (kind, count) {
        (EntityKind::Movie, 1) => "movie",
        (EntityKind::Movie, _) => "movies",
        (EntityKind::Movieset, 1) => "movie set",
        (EntityKind::Movieset, _) => "movie sets",
        (EntityKind::Tvshow, 1) => "TV show",
        (EntityKind::Tvshow, _) => "TV shows",
    };
    format!("{count} {noun}")
}

/// Runs blocking work off the async threads.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| format!("The work stopped unexpectedly: {error}"))
}

/// The state of one running batch.
struct Run<'a> {
    engine: &'a BatchEngine,
    kind: EntityKind,
    dry_run: bool,
    sink: ProgressSink<'a>,
    /// The titles of the step that runs now.
    selection: Selection,
    /// The selection `selection` was made from.
    chosen: (SelectionSpec, Vec<String>),
    /// The update step ran for real in this batch.
    update_ran: bool,
    /// The titles that update found or changed.
    found: Vec<ItemId>,
    /// The `new` titles, once known.
    new_cache: Option<Vec<ItemId>>,
}

impl Run<'_> {
    fn say(&self, step: Step, message: &str) {
        (self.sink)(Progress {
            step: Some(step),
            message: message.to_owned(),
            status: None,
        });
    }

    /// The closing message of a step.
    fn say_done(&self, step: Step, message: &str, status: StepStatus) {
        (self.sink)(Progress {
            step: Some(step),
            message: message.to_owned(),
            status: Some(status),
        });
    }

    fn services(&self) -> &Arc<AppServices> {
        &self.engine.services
    }

    fn dry(&self) -> bool {
        self.dry_run
    }

    /// Makes the selection of the step that runs now (only when it differs from the last one).
    async fn select_for(&mut self, planned: &PlannedStep) -> Result<(), String> {
        let wanted = (planned.selection.clone(), planned.titles.clone());
        if self.chosen != wanted {
            self.selection = self.engine.select(self.kind, &wanted.0, &wanted.1).await?;
            self.chosen = wanted;
            self.new_cache = None;
        }
        Ok(())
    }

    async fn step(&mut self, planned: &PlannedStep) -> StepReport {
        let step = planned.step;
        if let Err(message) = self.select_for(planned).await {
            return StepReport::new(step, StepStatus::Failed, message);
        }
        if self.kind == EntityKind::Movieset
            && !matches!(step, Step::Nfo | Step::ReadNfo | Step::ArtworkCleanup)
        {
            return unavailable(
                step,
                "Movie sets support the NFO and artwork clean-up \
                 actions in batch runs in this version.",
            );
        }
        match step {
            Step::Update => self.update().await,
            Step::Scrape => self.scrape(planned).await,
            Step::MediaInfo if planned.options.flag("force", false) => {
                self.reload_media_info(planned).await
            }
            Step::MediaInfo => self.media_info().await,
            Step::Rename => self.rename(planned).await,
            Step::Nfo => self.nfo(planned, false).await,
            Step::ReadNfo => self.nfo(planned, true).await,
            Step::AspectRatio
            | Step::Checksum
            | Step::Ratings
            | Step::Artwork
            | Step::ArtworkCleanup
            | Step::Cleanup => self.action(planned).await,
            Step::MovieSet => self.movie_set(planned).await,
        }
    }

    // ---- which titles ------------------------------------------------------------------

    /// The titles of the `new` scope.
    async fn new_titles(&mut self) -> Result<Vec<ItemId>, String> {
        if let Some(cached) = &self.new_cache {
            return Ok(cached.clone());
        }
        let wanted: HashSet<ItemId> = self.selection.ids().into_iter().collect();
        let candidates = if self.update_ran {
            self.found.clone()
        } else {
            let db = Arc::clone(self.services().db());
            let kind = self.kind;
            blocking(move || flagged_new(&db, kind))
                .await?
                .map_err(|error| format!("The library could not be read: {error}"))?
        };
        let mut seen = HashSet::new();
        let ids: Vec<ItemId> = candidates
            .into_iter()
            .filter(|id| wanted.contains(id) && seen.insert(*id))
            .collect();
        self.new_cache = Some(ids.clone());
        Ok(ids)
    }

    /// The titles a step with this scope works on. The scrape step picks the unscraped titles
    /// itself (`scrape_filters`); every other step gets them filtered here.
    async fn scoped(&mut self, scope: Scope, scrape_filters: bool) -> Result<Vec<ItemId>, String> {
        match scope {
            Scope::New => self.new_titles().await,
            Scope::All => Ok(self.selection.ids()),
            Scope::Unscraped if scrape_filters => Ok(self.selection.ids()),
            Scope::Unscraped => {
                let ids = self.selection.ids();
                let db = Arc::clone(self.services().db());
                blocking(move || {
                    ids.into_iter()
                        .filter(|id| {
                            db.read(|conn| get_item(conn, *id))
                                .ok()
                                .flatten()
                                .is_some_and(|item| item.core.last_scraper_id.is_empty())
                        })
                        .collect()
                })
                .await
            }
        }
    }

    /// The titles of a planned step: its scope, or the whole selection without one.
    async fn step_titles(&mut self, planned: &PlannedStep) -> Result<Vec<ItemId>, String> {
        match planned.scope {
            Some(scope) => self.scoped(scope, false).await,
            None => Ok(self.selection.ids()),
        }
    }

    async fn reselect(&mut self) -> Result<(), String> {
        let (spec, titles) = self.chosen.clone();
        self.selection = self.engine.select(self.kind, &spec, &titles).await?;
        Ok(())
    }

    // ---- update ------------------------------------------------------------------------

    async fn update(&mut self) -> StepReport {
        let step = Step::Update;
        let sources = self.selection.sources.clone();
        if sources.is_empty() {
            return StepReport::new(
                step,
                StepStatus::Skipped,
                "There is no data source of this kind. Add a folder in the app first.",
            );
        }
        if self.dry() {
            return StepReport::new(
                step,
                StepStatus::Planned,
                format!(
                    "Would rescan {}.",
                    counted(sources.len(), "data source", "data sources")
                ),
            )
            .count("sources", sources.len());
        }
        let scan = Arc::clone(self.services().scan());
        let mut runs = Vec::new();
        for source in &sources {
            match scan.rescan_source(source.id) {
                Ok(Some(run)) => runs.push(run),
                Ok(None) => {}
                Err(error) => {
                    return StepReport::new(
                        step,
                        StepStatus::Failed,
                        format!("The rescan of {} could not start: {error}", source.path),
                    );
                }
            }
        }
        let mut new_ids: Vec<ItemId> = Vec::new();
        let mut changed_ids: Vec<ItemId> = Vec::new();
        let mut unreadable: Vec<String> = Vec::new();
        for run in &runs {
            run.handle.job.wait().await;
            for report in run.handle.reports() {
                new_ids.extend(report.new_items.iter().copied());
                changed_ids.extend(report.updated.iter().copied());
                if let Some(skipped) = report.skipped {
                    let why = match skipped {
                        Skipped::SourceUnavailable => "could not be read",
                        Skipped::SourceEmpty => "is empty (a disconnected drive?)",
                    };
                    unreadable.push(format!("{} {why}", report.source.path));
                }
            }
        }
        // The thumbnails and the media info of what the scan found follow; wait so a command
        // line run does not end with that work half done.
        scan.media_info_idle().await;

        let mut found = Vec::new();
        for id in new_ids.iter().chain(&changed_ids) {
            found.push(*id);
        }
        let kind = self.kind;
        let db = Arc::clone(self.services().db());
        let owners = blocking(move || {
            let mut owners = Vec::new();
            for id in found {
                if let Some(owner) = owning_title(&db, id, kind)
                    && !owners.contains(&owner)
                {
                    owners.push(owner);
                }
            }
            owners
        })
        .await;
        let owners = match owners {
            Ok(owners) => owners,
            Err(message) => return StepReport::new(step, StepStatus::Failed, message),
        };
        self.update_ran = true;
        self.found = owners;
        self.new_cache = None;
        // The selection is made again so the titles the scan just found are part of it.
        if let Err(message) = self.reselect().await {
            return StepReport::new(step, StepStatus::Failed, message);
        }
        let wanted: HashSet<ItemId> = self.selection.ids().into_iter().collect();
        let fresh: Vec<_> = self
            .selection
            .items
            .iter()
            .filter(|item| self.found.contains(&item.id))
            .map(|item| ItemResult::new(Some(item.id.0), &item.title, item.year, "new"))
            .collect();
        let found_selected = self.found.iter().filter(|id| wanted.contains(id)).count();

        let mut report = StepReport::new(
            step,
            StepStatus::Done,
            format!(
                "Rescanned {}: {} new or changed.",
                counted(runs.len(), "data source", "data sources"),
                counted(found_selected, "title", "titles")
            ),
        )
        .count("sources", runs.len())
        .count("new_or_changed", found_selected)
        .count("unreadable_sources", unreadable.len());
        report.items = fresh;
        if !unreadable.is_empty() {
            report.message = format!("{} {}", report.message, unreadable.join("; "));
        }
        report
    }

    // ---- scrape ------------------------------------------------------------------------

    async fn scrape(&mut self, planned: &PlannedStep) -> StepReport {
        let step = Step::Scrape;
        let scope = planned.scope.unwrap_or(Scope::New);
        let matching = Arc::clone(self.services().matching());
        if !matching.has_providers() {
            return StepReport::new(
                step,
                StepStatus::Unavailable,
                "No metadata provider is ready. Add a key in Settings, Scraping and providers.",
            );
        }
        if matching.identification_paused() {
            // A step that needs a key the user has not added is skipped with where to add it.
            return StepReport::new(
                step,
                StepStatus::Skipped,
                crate::match_service::MatchServiceError::IdentificationPaused.to_string(),
            );
        }
        if let Some(name) = planned.options.arg("scraper") {
            let providers = matching.match_providers();
            if providers.get(name).is_none() {
                let ready: Vec<&str> = providers.all().iter().map(|p| p.id()).collect();
                return StepReport::new(
                    step,
                    StepStatus::Failed,
                    format!(
                        "There is no ready scraper called `{name}`. Ready: {}.",
                        ready.join(", ")
                    ),
                );
            }
        }
        let mode = match scope {
            Scope::New => BatchMode::MatchNew,
            Scope::Unscraped => BatchMode::MatchUnmatched,
            Scope::All => BatchMode::ForceBestMatch,
        };
        let ids = match self.scoped(scope, true).await {
            Ok(ids) => ids,
            Err(message) => return StepReport::new(step, StepStatus::Failed, message),
        };
        if ids.is_empty() {
            return StepReport::new(
                step,
                StepStatus::Skipped,
                format!("There is nothing to scrape in the {} scope.", scope.name()),
            );
        }

        let db = Arc::clone(self.services().db());
        let providers = matching.match_providers();
        let planned_ids = ids.clone();
        let plan = blocking(move || plan_batch(&db, &providers, mode, Some(&planned_ids))).await;
        let plan = match plan {
            Ok(Ok(plan)) => plan,
            Ok(Err(error)) => return StepReport::new(step, StepStatus::Failed, error.to_string()),
            Err(message) => return StepReport::new(step, StepStatus::Failed, message),
        };
        if plan.targets.is_empty() {
            return StepReport::new(
                step,
                StepStatus::Skipped,
                "No provider can serve the selected titles.",
            )
            .count("no_provider", plan.no_provider.len());
        }

        if self.dry() {
            let mut report = StepReport::new(
                step,
                StepStatus::Planned,
                format!(
                    "Would scrape {}.",
                    counted(plan.targets.len(), "title", "titles")
                ),
            )
            .count("titles", plan.targets.len())
            .count("no_provider", plan.no_provider.len());
            report.items = plan
                .targets
                .iter()
                .map(|t| ItemResult::new(Some(t.item), &t.label, 0, "scrape"))
                .collect();
            return report;
        }

        let submit_ids = ids.clone();
        let service = Arc::clone(&matching);
        let batch = blocking(move || service.submit(mode, Some(&submit_ids))).await;
        let batch = match batch {
            Ok(Ok(batch)) => batch,
            // The TMDB key is missing: the step waits for the user to add it, and its message
            // says where. Running it again would not help, so it is no failure.
            Ok(Err(error @ crate::match_service::MatchServiceError::IdentificationPaused)) => {
                return StepReport::new(step, StepStatus::Skipped, error.to_string());
            }
            Ok(Err(error)) => return StepReport::new(step, StepStatus::Failed, error.to_string()),
            Err(message) => return StepReport::new(step, StepStatus::Failed, message),
        };

        let mut failures: BTreeMap<i64, String> = BTreeMap::new();
        let mut stats = crate::matching::batch::BatchStats::default();
        for job in &batch.jobs {
            let done = job.job.wait().await;
            let here: Vec<&MatchTarget> = plan
                .targets
                .iter()
                .filter(|t| t.provider == job.provider)
                .collect();
            for record in &done.items {
                if let cv_jobs::ItemStatus::Failed { reason } = &record.status
                    && let Some(target) = here.get(record.index)
                {
                    failures.insert(target.item, reason.clone());
                }
            }
            let s = job.stats();
            stats.accepted += s.accepted;
            stats.rescraped += s.rescraped;
            stats.queued += s.queued;
            stats.ignored += s.ignored;
            stats.locked += s.locked;
        }

        // The NFO and the artwork follow each scrape in the background. The next
        // steps, and the command line's exit, wait for them: a rename moves the folder they
        // write into (and the rename step refuses to run while they still write).
        self.wait_for_follow_ups().await;

        // What became of each title is read back from the library.
        let db = Arc::clone(self.services().db());
        let targets = plan.targets.clone();
        let outcomes = blocking(move || {
            targets
                .iter()
                .map(|target| {
                    let id = ItemId(target.item);
                    let matched = db
                        .read(|conn| get_item(conn, id))
                        .ok()
                        .flatten()
                        .is_some_and(|item| !item.core.last_scraper_id.is_empty());
                    let review = db
                        .read(|conn| review::pending_for_item(conn, id))
                        .ok()
                        .flatten()
                        .is_some();
                    (target.item, target.label.clone(), matched, review)
                })
                .collect::<Vec<_>>()
        })
        .await
        .unwrap_or_default();
        let mut items = Vec::new();
        for (id, label, matched, review) in outcomes {
            let item = if let Some(reason) = failures.get(&id) {
                ItemResult::new(Some(id), label, 0, "failed").message(reason.clone())
            } else if matched {
                ItemResult::new(Some(id), label, 0, "matched")
            } else if review {
                ItemResult::new(Some(id), label, 0, "needs-review")
                    .message("Waiting in the Needs review inbox.")
            } else {
                ItemResult::new(Some(id), label, 0, "unmatched")
            };
            items.push(item);
        }
        let count = |status: &str| items.iter().filter(|i| i.status == status).count();
        let (matched, review, failed) = (count("matched"), count("needs-review"), count("failed"));
        let mut report = StepReport::new(
            step,
            StepStatus::Done,
            format!(
                "Scraped {}: {matched} matched, {review} {} review, {failed} failed.",
                counted(items.len(), "title", "titles"),
                if review == 1 { "needs" } else { "need" }
            ),
        )
        .count("titles", items.len())
        .count("matched", matched)
        .count("needs_review", review)
        .count("failed", failed)
        .count(
            "locked",
            usize::try_from(stats.locked).unwrap_or(usize::MAX),
        )
        .count(
            "ignored",
            usize::try_from(stats.ignored).unwrap_or(usize::MAX),
        );
        report.items = items;
        report
    }

    /// Waits for the NFO and artwork steps that follow the scrape, with an upper bound. Returns
    /// whether they are over.
    async fn wait_for_follow_ups(&self) -> bool {
        let follow_up = Arc::clone(self.services().scrape_follow_up());
        let over = tokio::time::timeout(self.engine.follow_up_wait, follow_up.idle())
            .await
            .is_ok();
        if !over {
            tracing::warn!("a scrape's NFO or artwork steps did not end in time");
        }
        over
    }

    // ---- media info --------------------------------------------------------------------

    async fn media_info(&mut self) -> StepReport {
        let step = Step::MediaInfo;
        let scan = Arc::clone(self.services().scan());
        if scan.media_info_status() == MediaInfoStatus::Missing {
            return StepReport::new(
                step,
                StepStatus::Unavailable,
                "MediaInfo is not installed. Install it from Settings, External tools.",
            );
        }
        let db = Arc::clone(self.services().db());
        let before = pending(&db).await;
        if self.dry() {
            return StepReport::new(
                step,
                StepStatus::Planned,
                format!(
                    "Would read the media info of {}.",
                    counted(before, "title", "titles")
                ),
            )
            .count("pending", before);
        }
        if before == 0 {
            return StepReport::new(
                step,
                StepStatus::Skipped,
                "Every title already has its media info.",
            );
        }
        scan.request_media_info();
        scan.media_info_idle().await;
        let after = pending(&db).await;
        StepReport::new(
            step,
            StepStatus::Done,
            format!(
                "Read the media info of {}; {after} could not be read.",
                counted(before.saturating_sub(after), "title", "titles")
            ),
        )
        .count("read", before.saturating_sub(after))
        .count("still_pending", after)
    }

    /// Reload media info (`force`): the details of the chosen titles are forgotten and read from
    /// the files again, and the "unreadable" marks of their videos are cleared.
    async fn reload_media_info(&mut self, planned: &PlannedStep) -> StepReport {
        let step = Step::MediaInfo;
        let ids = match self.step_titles(planned).await {
            Ok(ids) => ids,
            Err(message) => return StepReport::new(step, StepStatus::Failed, message),
        };
        if ids.is_empty() {
            return StepReport::new(step, StepStatus::Skipped, "No title is selected.");
        }
        let scan = Arc::clone(self.services().scan());
        if scan.media_info_status() == MediaInfoStatus::Missing {
            return StepReport::new(
                step,
                StepStatus::Unavailable,
                "MediaInfo is not installed. Install it from Settings, External tools.",
            );
        }
        if self.dry() {
            return StepReport::new(
                step,
                StepStatus::Planned,
                format!(
                    "Would read the media info of {} again.",
                    counted(ids.len(), "title", "titles")
                ),
            )
            .count("titles", ids.len());
        }
        match scan.reload_media_info(&ids).await {
            Ok(reloaded) if reloaded.titles == 0 => StepReport::new(
                step,
                StepStatus::Skipped,
                "None of the selected titles has a video file to read.",
            ),
            Ok(reloaded) => {
                let failed = reloaded.failed.len();
                let mut report = StepReport::new(
                    step,
                    StepStatus::Done,
                    format!(
                        "Read the media info of {} again; {failed} could not be read.",
                        counted(reloaded.titles, "title", "titles")
                    ),
                )
                .count("titles", reloaded.titles)
                .count("read", usize::try_from(reloaded.read).unwrap_or(usize::MAX))
                .count("failed", failed);
                report.items = reloaded
                    .failed
                    .into_iter()
                    .map(|(title, why)| ItemResult::new(None, title, 0, "failed").message(why))
                    .collect();
                report
            }
            Err(ReloadError::NoMediaInfo(message)) => {
                StepReport::new(step, StepStatus::Unavailable, message)
            }
            Err(error) => StepReport::new(step, StepStatus::Failed, error.to_string()),
        }
    }

    // ---- rename ------------------------------------------------------------------------

    async fn rename(&mut self, planned: &PlannedStep) -> StepReport {
        let step = Step::Rename;
        let scope = planned.scope.unwrap_or(Scope::New);
        let ids = match self.scoped(scope, false).await {
            Ok(ids) => ids,
            Err(message) => return StepReport::new(step, StepStatus::Failed, message),
        };
        if ids.is_empty() {
            return StepReport::new(
                step,
                StepStatus::Skipped,
                format!("There is nothing to rename in the {} scope.", scope.name()),
            );
        }
        // A folder is never moved while a scrape's NFO or artwork is still being written into
        // it: the write would land in the old place or recreate the old folder.
        if !self.services().scrape_follow_up().is_idle() && !self.wait_for_follow_ups().await {
            return StepReport::new(step, StepStatus::Failed, FOLLOW_UPS_BUSY.to_owned());
        }
        let values = self.services().settings().snapshot().values;
        let book = RenameSettings::from_values(&values);
        let nfo_settings = NfoSettings::from_values(&values);
        let mut profile = match planned.options.arg("profile") {
            None => book.active_profile(&nfo_settings),
            Some(name) => {
                if let Some(profile) = named_profile(&book, name, &nfo_settings) {
                    profile
                } else {
                    let known: Vec<String> = book
                        .book
                        .list()
                        .into_iter()
                        .map(|(_, name, _)| name)
                        .collect();
                    return StepReport::new(
                        step,
                        StepStatus::Failed,
                        format!(
                            "There is no naming profile called `{name}`. Available: {}.",
                            known.join(", ")
                        ),
                    );
                }
            }
        };
        crate::rename_settings::apply_subfolder_choice(
            &mut profile,
            crate::rename_settings::keep_subfolders(&values),
        );
        crate::rename_settings::apply_colon_choice(&mut profile, &values);
        let preview_request = PreviewRequest {
            scope: OrganizeScope::Items(ids),
            profile,
            choices: BTreeMap::new(),
        };
        let organize = Arc::clone(&self.engine.organize);
        let request = preview_request.clone();
        let preview = match blocking(move || organize.preview(&request)).await {
            Ok(Ok(preview)) => preview,
            Ok(Err(error)) => return StepReport::new(step, StepStatus::Failed, error.to_string()),
            Err(message) => return StepReport::new(step, StepStatus::Failed, message),
        };
        if !preview.problems.is_empty() {
            let problems: Vec<String> =
                preview.problems.iter().map(|p| p.message.clone()).collect();
            return StepReport::new(
                step,
                StepStatus::Failed,
                format!(
                    "The naming templates have a problem: {}. Fix it in Organize.",
                    problems.join("; ")
                ),
            );
        }

        let conflicts: BTreeMap<&str, String> = preview
            .conflicts
            .iter()
            .flat_map(|c| {
                c.items
                    .iter()
                    .map(move |id| (id.as_str(), conflict_text(c)))
            })
            .collect();
        let mut items: Vec<ItemResult> = Vec::new();
        let mut include: Vec<String> = Vec::new();
        for row in &preview.rows {
            let id = row.id.parse::<i64>().ok();
            let (status, message) = match &row.status {
                RowStatus::Change => {
                    include.push(row.id.clone());
                    (if self.dry() { "rename" } else { "planned" }, None)
                }
                RowStatus::Conflict => (
                    "conflict",
                    Some(
                        conflicts
                            .get(row.id.as_str())
                            .cloned()
                            .unwrap_or_else(|| "It collides with another title.".to_owned()),
                    ),
                ),
                RowStatus::Skipped => ("skipped", None),
                RowStatus::Unplanned(why) => ("unplanned", Some(why.clone())),
                RowStatus::MediaUnread => {
                    ("held", Some(format!("{}.", cv_rename::MEDIA_HELD_REASON)))
                }
                RowStatus::Unscraped => (
                    "unscraped",
                    Some("It is not scraped yet, so it keeps its names until it is.".to_owned()),
                ),
            };
            let mut item = ItemResult::new(id, &row.title, row.year, status);
            item.from = Some(absolute(&row.root, &row.before));
            item.to = Some(absolute(&row.root, &row.after));
            item.from_file = Some(row.before_file.clone());
            item.to_file = Some(row.after_file.clone());
            item.message = message;
            items.push(item);
        }
        let counts = preview.counts;
        let summary = format!(
            "{} to rename, {} already tidy, {} in conflict, {} locked.",
            counts.change, counts.tidy, counts.conflicts, counts.locked
        );
        let base = |status, text: String| {
            StepReport::new(step, status, text)
                .count("to_rename", counts.change as usize)
                .count("tidy", counts.tidy as usize)
                .count("conflicts", counts.conflicts as usize)
                .count("locked", counts.locked as usize)
                .count("unplanned", counts.unplanned as usize)
                .count("files", counts.moves as usize)
        };
        if self.dry() {
            let mut report = base(StepStatus::Planned, format!("Would rename: {summary}"));
            report.items = items;
            return report;
        }
        if include.is_empty() {
            let mut report = base(StepStatus::Skipped, format!("Nothing to rename: {summary}"));
            report.items = items;
            return report;
        }

        let organize = Arc::clone(&self.engine.organize);
        let apply = ApplyRequest {
            preview: preview_request,
            include,
        };
        let applied = match blocking(move || organize.apply(&apply)).await {
            Ok(Ok(applied)) => applied,
            Ok(Err(error)) => return StepReport::new(step, StepStatus::Failed, error.to_string()),
            Err(message) => return StepReport::new(step, StepStatus::Failed, message),
        };
        let mut renamed = 0_usize;
        let mut failed = 0_usize;
        for done in &applied.items {
            let Some(item) = items
                .iter_mut()
                .find(|i| i.id.map(|n| n.to_string()) == Some(done.id.clone()))
            else {
                continue;
            };
            match &done.status {
                AppliedStatus::Done => {
                    item.status = "renamed".to_owned();
                    renamed += 1;
                }
                AppliedStatus::Failed(why) => {
                    item.status = "failed".to_owned();
                    item.message = Some(why.clone());
                    failed += 1;
                }
                AppliedStatus::Canceled => item.status = "canceled".to_owned(),
            }
            if item.message.is_none() && !done.notes.is_empty() {
                item.message = Some(done.notes.join(" "));
            }
        }
        for id in &applied.not_run {
            if let Some(item) = items
                .iter_mut()
                .find(|i| i.id.map(|n| n.to_string()).as_deref() == Some(id.as_str()))
            {
                item.status = "not-run".to_owned();
                item.message = Some(match applied.busy_reason(id) {
                    Some(why) => format!("{why} It was not renamed."),
                    None => "The plan changed before it could run.".to_owned(),
                });
            }
        }
        let mut report = base(
            StepStatus::Done,
            format!(
                "Renamed {}, {failed} failed; {summary}",
                counted(renamed, "title", "titles")
            ),
        )
        .count("renamed", renamed)
        .count("failed", failed);
        report.items = items;
        report.undo = applied.batch_id;
        report
    }

    // ---- NFO ---------------------------------------------------------------------------

    /// Writes the NFO files from the library, or (`read`) reads them into the library.
    async fn nfo(&mut self, planned: &PlannedStep, read: bool) -> StepReport {
        let step = if read { Step::ReadNfo } else { Step::Nfo };
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
        let values = self.services().settings().snapshot().values;
        let settings = NfoSettings::from_values(&values);
        if self.dry() {
            return StepReport::new(
                step,
                StepStatus::Planned,
                format!(
                    "Would {} the NFO files of {}.",
                    if read { "read" } else { "rewrite" },
                    counted(ids.len(), "title", "titles")
                ),
            )
            .count("titles", ids.len());
        }
        let service = Arc::clone(&self.engine.nfo);
        let scope = NfoScope::Items(ids);
        let report = match blocking(move || {
            if read {
                service.read(&scope, &settings)
            } else {
                service.rewrite(&scope, &settings)
            }
        })
        .await
        {
            Ok(Ok(report)) => report,
            Ok(Err(error)) => return StepReport::new(step, StepStatus::Failed, error.to_string()),
            Err(message) => return StepReport::new(step, StepStatus::Failed, message),
        };
        let items: Vec<ItemResult> = report
            .units
            .iter()
            .map(|unit| {
                let (status, message) = match &unit.status {
                    UnitStatus::Changed => ("written", None),
                    UnitStatus::Unchanged => ("unchanged", None),
                    UnitStatus::KeptForeign => (
                        "kept-foreign",
                        Some(
                            "A file that is not CineVault's is in the way; it was left alone."
                                .to_owned(),
                        ),
                    ),
                    UnitStatus::Skipped { reason } => ("skipped", Some(format!("{reason:?}"))),
                    UnitStatus::Failed { message } => ("failed", Some(message.clone())),
                };
                let mut item = ItemResult::new(Some(unit.item.0), &unit.label, 0, status);
                item.message = message;
                item
            })
            .collect();
        let totals = report.totals;
        let files_failed = report
            .units
            .iter()
            .flat_map(|u| &u.files)
            .filter(|f| matches!(f.outcome, FileOutcome::Failed { .. }))
            .count();
        let mut step_report = StepReport::new(
            step,
            StepStatus::Done,
            format!(
                "{} the NFO files: {} {}, {} unchanged, {} failed.",
                if read { "Read" } else { "Rewrote" },
                totals.changed,
                if read { "updated" } else { "written" },
                totals.unchanged,
                totals.failed
            ),
        )
        .count("changed", totals.changed as usize)
        .count("unchanged", totals.unchanged as usize)
        .count("kept_foreign", totals.kept_foreign as usize)
        .count("skipped", totals.skipped as usize)
        .count("failed", (totals.failed as usize).max(files_failed))
        .count("files_written", totals.files_written as usize);
        step_report.items = items;
        step_report.undo = report.token;
        step_report
    }
}

fn unavailable(step: Step, message: &str) -> StepReport {
    StepReport::new(step, StepStatus::Unavailable, message)
}

/// The naming profile called `name` (its id or its name, any case; `default` is the built-in
/// default) with the NFO file names applied.
fn named_profile(
    settings: &RenameSettings,
    name: &str,
    nfo: &NfoSettings,
) -> Option<cv_rename::RenameProfile> {
    let wanted = name.trim();
    let listed = settings.book.list();
    let found = listed
        .iter()
        .find(|(id, label, _)| {
            id.eq_ignore_ascii_case(wanted) || label.eq_ignore_ascii_case(wanted)
        })
        .map(|(id, _, _)| id.clone());
    // Scripts often ask for the profile called "Default": that is the
    // built-in default profile, unless the user made a profile with that name.
    let id = found.or_else(|| {
        wanted
            .eq_ignore_ascii_case("default")
            .then(|| cv_rename::profile::DEFAULT_PROFILE_ID.to_owned())
    })?;
    let mut profile = settings.book.get(&id)?;
    apply_nfo_choices(&mut profile, nfo);
    Some(profile)
}

/// Why a title is in conflict, in the words Organize uses.
fn conflict_text(conflict: &ConflictRow) -> String {
    match &conflict.code {
        // One title twice: two of its files (an episode and its repack) would get one name.
        ConflictCode::Duplicate if conflict.items.len() <= 1 => format!(
            "Two files of “{}” would become “{}”.",
            conflict.titles.first().map_or("", String::as_str),
            conflict.path
        ),
        ConflictCode::Duplicate if conflict.items.len() == 2 => {
            format!("Another title would also become “{}”.", conflict.path)
        }
        ConflictCode::Duplicate => format!(
            "{} other titles would also become “{}”.",
            conflict.items.len() - 1,
            conflict.path
        ),
        ConflictCode::Foreign { .. } => {
            "The destination already exists and belongs to something else.".to_owned()
        }
        ConflictCode::PathTooLong { length, limit } => {
            format!("The new path is {length} characters long; the limit is {limit}.")
        }
        ConflictCode::NameTooLong { length, limit } => {
            format!("The new name is {length} characters long; the limit is {limit}.")
        }
        ConflictCode::CaseOnly => "Only the letter case changes.".to_owned(),
    }
}

/// The titles of `kind` the library flags as NEW.
fn flagged_new(db: &Database, kind: EntityKind) -> cv_db::DbResult<Vec<ItemId>> {
    let query = ItemQuery::new(kind.item_kind()).filter(Filter::Flag {
        flag: ItemFlag::New,
        value: true,
    });
    let mut ids = Vec::new();
    let mut cursor = None;
    loop {
        let page = db.read(|conn| list_items(conn, &query, cursor.as_ref(), 500))?;
        ids.extend(page.items.iter().map(|item| item.id));
        match page.next {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    Ok(ids)
}

/// The title of `kind` that `id` is, or belongs to (an episode's show).
fn owning_title(db: &Database, id: ItemId, kind: EntityKind) -> Option<ItemId> {
    let mut current = id;
    for _ in 0..4 {
        let summary = db.read(|conn| get_summary(conn, current)).ok().flatten()?;
        if summary.kind == kind.item_kind() {
            return Some(summary.id);
        }
        current = summary.parent_id?;
    }
    None
}

async fn pending(db: &Arc<Database>) -> usize {
    let db = Arc::clone(db);
    blocking(move || {
        db.read(|conn| pending_media_info(conn, usize::MAX))
            .map(|list| list.len())
            .unwrap_or(0)
    })
    .await
    .unwrap_or(0)
}

/// A folder of the rename plan as one absolute path: the data source root (which ends with the
/// platform's separator) and the folder relative to it, which the library stores with `/`. The
/// relative part is written with the platform's separator too, so a path never mixes the two.
fn absolute(root: &str, relative: &str) -> String {
    let relative = relative.trim_start_matches('/');
    if std::path::MAIN_SEPARATOR == '/' {
        format!("{root}{relative}")
    } else {
        format!(
            "{root}{}",
            relative.replace('/', std::path::MAIN_SEPARATOR_STR)
        )
    }
}

#[cfg(test)]
mod conflict_text_tests {
    use super::*;

    fn duplicate(items: &[&str], titles: &[&str]) -> ConflictRow {
        ConflictRow {
            code: ConflictCode::Duplicate,
            items: items.iter().map(|s| (*s).to_owned()).collect(),
            titles: titles.iter().map(|s| (*s).to_owned()).collect(),
            folders: Vec::new(),
            path: "Breaking Bad (2008)/Season 01/Breaking Bad - S01E03.mkv".to_owned(),
            options: Vec::new(),
        }
    }

    /// reg2-8: two files of one show (an episode and its repack) are not "another title".
    #[test]
    fn two_files_of_one_title_are_named_as_such() {
        assert_eq!(
            conflict_text(&duplicate(&["7"], &["Breaking Bad"])),
            "Two files of “Breaking Bad” would become “Breaking Bad (2008)/Season 01/Breaking Bad - S01E03.mkv”."
        );
        assert_eq!(
            conflict_text(&duplicate(&["7", "8"], &["Heat", "Heat"])),
            "Another title would also become “Breaking Bad (2008)/Season 01/Breaking Bad - S01E03.mkv”."
        );
        assert!(
            conflict_text(&duplicate(&["7", "8", "9"], &["A", "B", "C"]))
                .starts_with("2 other titles would also become")
        );
    }
}
