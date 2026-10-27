//! The actions the app runs on a selection of titles: one named action over a list of library ids.
//!
//! Each action is a single step of the batch engine, so the app and the command line do the same
//! work with the same rules:
//!
//! | Action id | Step | Does |
//! |---|---|---|
//! | `fetch-ratings` | ratings | fetches and stores the ratings of the chosen sources |
//! | `detect-aspect-ratio` | aspect ratio | measures the picture without its black bars |
//! | `calculate-checksums` | checksum | CRC-32 and the perceptual hash |
//! | `redownload-artwork` | artwork | fetches artwork that is gone from disk again |
//! | `reload-media-info` | media info (`force`) | forgets and reads the media info of the videos again |
//! | `cleanup-unwanted` | cleanup | moves the profile's unwanted files to the trash folder |
//! | `cleanup-unwanted-preview` | cleanup (dry run) | lists what the clean-up would move |
//!
//! [`ItemAction::plan`] turns an action and the ids into a [`Plan`]; the desktop app queues that
//! plan as a job of the batch backend ([`CommandBackend::submit_plan`]), which the Activity drawer
//! shows like any other job. An action is addressed by its id string, so the set can grow without
//! a new command.
//!
//! [`CommandBackend::submit_plan`]: super::CommandBackend::submit_plan

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::wording::counted;
use cv_db::Database;
use cv_db::repo::get_summary;
use cv_domain::{ItemId, ItemKind};

use super::backend::CommandBackend;
use super::model::{
    BatchReport, EntityKind, ItemResult, Plan, PlannedStep, SelectionSpec, Step, StepReport,
    StepStatus,
};

/// One thing the app can do to a selection.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ItemAction {
    FetchRatings,
    DetectAspectRatio,
    CalculateChecksums,
    RedownloadArtwork,
    ReloadMediaInfo,
    /// Move the unwanted files to the trash folder; with `dry_run`, only list them.
    CleanupUnwanted {
        dry_run: bool,
    },
}

impl ItemAction {
    /// The built-in actions, in the order the app lists them.
    #[must_use]
    pub fn built_in() -> Vec<ItemAction> {
        vec![
            Self::FetchRatings,
            Self::DetectAspectRatio,
            Self::CalculateChecksums,
            Self::RedownloadArtwork,
            Self::ReloadMediaInfo,
            Self::CleanupUnwanted { dry_run: true },
            Self::CleanupUnwanted { dry_run: false },
        ]
    }

    /// The id the app sends to run this action.
    #[must_use]
    pub fn id(&self) -> String {
        match self {
            Self::FetchRatings => "fetch-ratings",
            Self::DetectAspectRatio => "detect-aspect-ratio",
            Self::CalculateChecksums => "calculate-checksums",
            Self::RedownloadArtwork => "redownload-artwork",
            Self::ReloadMediaInfo => "reload-media-info",
            Self::CleanupUnwanted { dry_run: true } => "cleanup-unwanted-preview",
            Self::CleanupUnwanted { dry_run: false } => "cleanup-unwanted",
        }
        .to_owned()
    }

    /// The action an id stands for.
    #[must_use]
    pub fn parse(id: &str) -> Option<ItemAction> {
        let id = id.trim();
        Self::built_in()
            .into_iter()
            .find(|action| action.id() == id)
    }

    /// The name of the job in the Activity drawer.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::FetchRatings => "Fetch ratings",
            Self::DetectAspectRatio => "Detect aspect ratio",
            Self::CalculateChecksums => "Calculate checksums",
            Self::RedownloadArtwork => "Re-download artwork",
            Self::ReloadMediaInfo => "Reload media info",
            Self::CleanupUnwanted { dry_run: true } => "Preview unwanted-file clean-up",
            Self::CleanupUnwanted { dry_run: false } => "Clean up unwanted files",
        }
        .to_owned()
    }

    /// The step of the batch engine that does the work.
    #[must_use]
    pub fn step(&self) -> Step {
        match self {
            Self::FetchRatings => Step::Ratings,
            Self::DetectAspectRatio => Step::AspectRatio,
            Self::CalculateChecksums => Step::Checksum,
            Self::RedownloadArtwork => Step::Artwork,
            Self::ReloadMediaInfo => Step::MediaInfo,
            Self::CleanupUnwanted { .. } => Step::Cleanup,
        }
    }

    /// Whether the action works on this kind of title. Everything is for movies and TV shows.
    #[must_use]
    pub fn supports(&self, kind: EntityKind) -> bool {
        match kind {
            EntityKind::Movie | EntityKind::Tvshow => true,
            EntityKind::Movieset => false,
        }
    }

    /// The plan that runs the action on these titles (library item ids of `kind`).
    #[must_use]
    pub fn plan(&self, kind: EntityKind, ids: &[i64]) -> Plan {
        let mut planned = PlannedStep::new(self.step());
        planned.selection = SelectionSpec {
            ids: ids.to_vec(),
            ..SelectionSpec::default()
        };
        match self {
            Self::ReloadMediaInfo => {
                planned.options.args.insert("force".into(), "true".into());
            }
            Self::RedownloadArtwork => {
                planned
                    .options
                    .args
                    .insert("redownload".into(), "true".into());
            }
            _ => {}
        }
        Plan {
            kind,
            dry_run: matches!(self, Self::CleanupUnwanted { dry_run: true }),
            steps: vec![planned],
        }
    }
}

/// How many titles with something to say an outcome lists.
const LINES_KEPT: usize = 8;

/// What a selection action did, for the page that asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemActionOutcome {
    /// The id of the action.
    pub action: String,
    /// The jobs the Activity drawer shows for it (one per kind of title in the selection).
    pub jobs: Vec<i64>,
    /// How it ended overall: the most serious status of its steps.
    pub status: StepStatus,
    /// One plain sentence: the step's own, or a joined one when the selection had several kinds.
    pub message: String,
    /// Titles in the selection that the action ran on.
    pub titles: u32,
    pub done: u32,
    pub unchanged: u32,
    pub skipped: u32,
    pub failed: u32,
    /// Selected titles the action did not run on (a movie set for a movie-only action, or a
    /// season), with the reason.
    pub left_out: Vec<String>,
    /// Titles that failed, were skipped or (a preview) would change, with why; at most a few.
    pub lines: Vec<ItemResult>,
}

/// Why a selection action did not start.
#[derive(Debug, thiserror::Error)]
pub enum ItemActionError {
    #[error("there is no action called `{0}`")]
    UnknownAction(String),
    #[error("no title is selected")]
    NothingSelected,
    #[error("{0}")]
    Unsupported(String),
    #[error("the library could not be read: {0}")]
    Library(String),
    #[error("the batch runner is not running")]
    NotRunning,
}

impl ItemActionError {
    /// A short stable name for the page to branch on.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::UnknownAction(_) => "unknown_action",
            Self::NothingSelected => "nothing_selected",
            Self::Unsupported(_) => "unsupported",
            Self::Library(_) => "storage",
            Self::NotRunning => "not_running",
        }
    }
}

/// Seriousness of a status for the overall outcome: higher wins.
fn weight(status: StepStatus) -> u8 {
    match status {
        StepStatus::Failed => 4,
        StepStatus::Unavailable => 3,
        StepStatus::Done => 2,
        StepStatus::Planned => 1,
        StepStatus::Skipped => 0,
    }
}

fn count(report: &StepReport, name: &str) -> u32 {
    report
        .counts
        .get(name)
        .map_or(0, |n| u32::try_from(*n).unwrap_or(u32::MAX))
}

/// The selected titles sorted into plans: one group per kind of title, with what was left out.
struct Grouped {
    groups: BTreeMap<u8, (EntityKind, Vec<i64>)>,
    left_out: Vec<String>,
}

fn group_by_kind(action: &ItemAction, kinds: Vec<(ItemId, Option<ItemKind>)>) -> Grouped {
    let mut groups: BTreeMap<u8, (EntityKind, Vec<i64>)> = BTreeMap::new();
    let (mut gone, mut unsupported, mut parts) = (0_usize, 0_usize, 0_usize);
    for (id, kind) in kinds {
        let (slot, entity) = match kind {
            None => {
                gone += 1;
                continue;
            }
            Some(ItemKind::Movie) => (0, EntityKind::Movie),
            Some(ItemKind::Show) => (1, EntityKind::Tvshow),
            Some(ItemKind::Set) => (2, EntityKind::Movieset),
            Some(_) => {
                parts += 1;
                continue;
            }
        };
        if !action.supports(entity) {
            unsupported += 1;
            continue;
        }
        groups
            .entry(slot)
            .or_insert((entity, Vec::new()))
            .1
            .push(id.0);
    }
    let mut left_out = Vec::new();
    if gone > 0 {
        left_out.push(if gone == 1 {
            "1 title is no longer in the library".to_owned()
        } else {
            format!("{gone} titles are no longer in the library")
        });
    }
    if parts > 0 {
        left_out.push(format!(
            "{}: choose their show instead",
            counted(parts, "season or episode", "seasons or episodes")
        ));
    }
    if unsupported > 0 {
        left_out.push(format!(
            "{}: this action works on movies and shows",
            counted(unsupported, "movie set", "movie sets")
        ));
    }
    Grouped { groups, left_out }
}

/// Runs `action` on the titles `ids` through the batch backend, one job per kind of title, and
/// waits for the reports.
///
/// # Errors
/// [`ItemActionError`] when nothing can be run: no title, titles of a kind the action does not
/// take, or a library that cannot be read.
pub async fn run_item_action(
    backend: &CommandBackend,
    db: &Arc<Database>,
    action: &ItemAction,
    ids: &[ItemId],
) -> Result<ItemActionOutcome, ItemActionError> {
    if ids.is_empty() {
        return Err(ItemActionError::NothingSelected);
    }
    // The kind of every selected title decides which plan it goes into.
    let wanted = ids.to_vec();
    let reader = Arc::clone(db);
    let kinds = tokio::task::spawn_blocking(move || {
        reader.read(|conn| {
            wanted
                .iter()
                .map(|id| Ok((*id, get_summary(conn, *id)?.map(|s| s.kind))))
                .collect::<Result<Vec<_>, cv_db::DbError>>()
        })
    })
    .await
    .map_err(|error| ItemActionError::Library(error.to_string()))?
    .map_err(|error| ItemActionError::Library(error.to_string()))?;

    let Grouped { groups, left_out } = group_by_kind(action, kinds);
    if groups.is_empty() {
        return Err(if left_out.is_empty() {
            ItemActionError::NothingSelected
        } else {
            ItemActionError::Unsupported(format!(
                "{} cannot run on this selection: {}.",
                action.label(),
                left_out.join("; ")
            ))
        });
    }

    let mut outcome = ItemActionOutcome {
        action: action.id(),
        jobs: Vec::new(),
        status: StepStatus::Skipped,
        message: String::new(),
        titles: 0,
        done: 0,
        unchanged: 0,
        skipped: 0,
        failed: 0,
        left_out,
        lines: Vec::new(),
    };
    let mut messages: Vec<String> = Vec::new();
    let mut first = true;
    for (kind, ids) in groups.into_values() {
        let submitted = backend
            .submit_plan(&action.label(), action.plan(kind, &ids))
            .map_err(|_| ItemActionError::NotRunning)?;
        outcome.jobs.push(submitted.job);
        let report: BatchReport = submitted.report(kind).await;
        outcome.titles += u32::try_from(report.selection.titles).unwrap_or(u32::MAX);
        let (status, message) = match report.steps.first() {
            Some(step) => {
                outcome.done += count(step, "done").max(count(step, "read"));
                outcome.unchanged += count(step, "unchanged");
                outcome.skipped += count(step, "skipped");
                outcome.failed += count(step, "failed");
                outcome.lines.extend(
                    step.items
                        .iter()
                        .filter(|line| {
                            matches!(line.status.as_str(), "failed" | "skipped" | "planned")
                        })
                        .cloned(),
                );
                (step.status, step.message.clone())
            }
            None => (
                StepStatus::Failed,
                report
                    .error
                    .unwrap_or_else(|| "The action did not run.".to_owned()),
            ),
        };
        if first || weight(status) > weight(outcome.status) {
            outcome.status = status;
        }
        first = false;
        messages.push(message);
    }
    outcome.lines.truncate(LINES_KEPT);
    messages.dedup();
    outcome.message = messages.join(" ");
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_built_in_action_has_its_own_id_that_parses_back() {
        let actions = ItemAction::built_in();
        let mut ids: Vec<String> = actions.iter().map(ItemAction::id).collect();
        for (action, id) in actions.iter().zip(&ids) {
            assert_eq!(ItemAction::parse(id).as_ref(), Some(action), "{id}");
        }
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), actions.len(), "ids are unique");
        assert_eq!(ItemAction::parse("not-an-action"), None);
    }

    #[test]
    fn a_plan_is_one_step_on_exactly_the_given_titles() {
        let plan = ItemAction::FetchRatings.plan(EntityKind::Movie, &[4, 9]);
        assert_eq!(plan.kind, EntityKind::Movie);
        assert!(!plan.dry_run);
        let [step] = plan.steps.as_slice() else {
            panic!("one step");
        };
        assert_eq!(step.step, Step::Ratings);
        assert_eq!(step.selection.ids, [4, 9]);
        assert!(step.options.args.is_empty());
    }

    #[test]
    fn reload_forces_and_the_cleanup_preview_is_a_dry_run() {
        let reload = ItemAction::ReloadMediaInfo.plan(EntityKind::Tvshow, &[1]);
        assert!(reload.steps[0].options.flag("force", false));
        let redownload = ItemAction::RedownloadArtwork.plan(EntityKind::Movie, &[1]);
        assert!(redownload.steps[0].options.flag("redownload", false));
        assert!(
            ItemAction::CleanupUnwanted { dry_run: true }
                .plan(EntityKind::Movie, &[1])
                .dry_run
        );
        assert!(
            !ItemAction::CleanupUnwanted { dry_run: false }
                .plan(EntityKind::Movie, &[1])
                .dry_run
        );
    }

    #[test]
    fn no_selection_action_takes_a_movie_set() {
        for action in ItemAction::built_in() {
            assert!(!action.supports(EntityKind::Movieset), "{}", action.id());
            assert!(action.supports(EntityKind::Tvshow));
            assert!(action.supports(EntityKind::Movie), "{}", action.id());
        }
    }

    #[test]
    fn a_movie_set_is_left_out_with_the_reason() {
        let grouped = group_by_kind(
            &ItemAction::FetchRatings,
            vec![
                (ItemId(31), Some(ItemKind::Show)),
                (ItemId(4), Some(ItemKind::Set)),
            ],
        );
        assert_eq!(grouped.groups.len(), 1, "only the show runs");
        assert_eq!(
            grouped.left_out,
            ["1 movie set: this action works on movies and shows"]
        );
    }
}
