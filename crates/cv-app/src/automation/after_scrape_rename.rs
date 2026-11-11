//! The runner of Rename after Scrape: the batch engine's rename step over the titles a scrape was
//! written into.
//!
//! The after-scrape rename queue ([`crate::actions::ActionsService::set_rename_runner`]) hands
//! over its titles; they are renamed as one batch job per kind of title, so the Activity drawer
//! shows the work and one Undo takes it back. The rename step renames only the titles whose plan
//! is a plain change. What it did is told through the notices:
//!
//! * "Renamed 12 titles after Scrape", an information notice with the journal batch, which the
//!   window shows as a toast with Undo;
//! * "2 titles were not renamed after Scrape", a warning for Activity's Needs attention that lists
//!   each title held back (a conflict, video details not read yet, a failure) and why;
//! * "Rename after Scrape did not run", a warning with the reason, when the step could not run
//!   (a naming template with a problem, "Metadata only" turned on in the meantime).

use std::collections::BTreeMap;
use std::sync::{Arc, Weak};

use cv_db::repo::get_summary;
use cv_domain::{ItemId, ItemKind};

use super::backend::CommandBackend;
use super::model::{
    EntityKind, ItemResult, Plan, PlannedStep, Scope, SelectionSpec, Step, StepReport, StepStatus,
};
use crate::notices::{AppNotice, NoticeAction, NoticeLevel};
use crate::services::AppServices;
use crate::wording::counted;

/// The name of the job in the Activity drawer, and the source of its notices.
const JOB_NAME: &str = "Rename after Scrape";

/// How many held-back titles a notice lists.
const LINES_KEPT: usize = 8;

/// The item statuses of the rename step that mean "this title was not renamed, and the user should
/// know why".
const HELD: [&str; 5] = ["conflict", "held", "unplanned", "failed", "not-run"];

/// Makes `backend` the runner of the after-scrape rename queue of `services`.
pub(crate) fn install(backend: &Arc<CommandBackend>, services: &Arc<AppServices>) {
    let backend: Weak<CommandBackend> = Arc::downgrade(backend);
    let weak_services = Arc::downgrade(services);
    services.actions().set_rename_runner(Arc::new(move |ids| {
        let (Some(backend), Some(services)) = (backend.upgrade(), weak_services.upgrade()) else {
            return;
        };
        cv_jobs::spawn(async move { run(&backend, &services, ids).await });
    }));
}

async fn run(backend: &CommandBackend, services: &Arc<AppServices>, ids: Vec<ItemId>) {
    let db = Arc::clone(services.db());
    let kinds = tokio::task::spawn_blocking(move || {
        db.read(|conn| {
            ids.iter()
                .map(|id| Ok((*id, get_summary(conn, *id)?.map(|s| s.kind))))
                .collect::<Result<Vec<_>, cv_db::DbError>>()
        })
    })
    .await;
    let kinds = match kinds {
        Ok(Ok(kinds)) => kinds,
        Ok(Err(error)) => {
            tracing::warn!(%error, "rename after scrape: the titles could not be read");
            return;
        }
        Err(error) => {
            tracing::warn!(%error, "rename after scrape: the titles could not be read");
            return;
        }
    };
    let mut steps = Vec::new();
    for (kind, ids) in group(kinds) {
        let Ok(submitted) = backend.submit_plan(JOB_NAME, plan(kind, &ids)) else {
            tracing::warn!("rename after scrape: the batch runner has stopped");
            return;
        };
        let report = submitted.report(kind).await;
        match report.error {
            Some(error) if report.steps.is_empty() => {
                steps.push(StepReport::new(Step::Rename, StepStatus::Failed, error));
            }
            _ => steps.extend(report.steps),
        }
    }
    for notice in notices_for(&steps) {
        services.notices().publish(notice);
    }
}

/// The movies and the shows among `kinds`, in that order. Titles that are gone or are no movie or
/// show are left out.
fn group(kinds: Vec<(ItemId, Option<ItemKind>)>) -> Vec<(EntityKind, Vec<i64>)> {
    let mut groups: BTreeMap<u8, (EntityKind, Vec<i64>)> = BTreeMap::new();
    for (id, kind) in kinds {
        let (slot, entity) = match kind {
            Some(ItemKind::Movie) => (0, EntityKind::Movie),
            Some(ItemKind::Show) => (1, EntityKind::Tvshow),
            _ => continue,
        };
        groups
            .entry(slot)
            .or_insert((entity, Vec::new()))
            .1
            .push(id.0);
    }
    groups.into_values().collect()
}

/// The plan that renames exactly `ids`.
fn plan(kind: EntityKind, ids: &[i64]) -> Plan {
    let mut planned = PlannedStep::new(Step::Rename);
    planned.scope = Some(Scope::All);
    planned.selection = SelectionSpec {
        ids: ids.to_vec(),
        ..SelectionSpec::default()
    };
    Plan {
        kind,
        dry_run: false,
        steps: vec![planned],
    }
}

/// What the rename steps of one hand-over tell the user.
pub(crate) fn notices_for(steps: &[StepReport]) -> Vec<AppNotice> {
    let mut notices = Vec::new();
    let mut held: Vec<&ItemResult> = Vec::new();
    let mut not_run: Vec<&str> = Vec::new();
    for step in steps.iter().filter(|s| s.step == Step::Rename) {
        let renamed = step.counts.get("renamed").copied().unwrap_or(0);
        if renamed > 0
            && let Some(batch) = &step.undo
        {
            let renamed = usize::try_from(renamed).unwrap_or(usize::MAX);
            notices.push(
                AppNotice::new(
                    NoticeLevel::Info,
                    format!(
                        "Renamed {} after Scrape",
                        counted(renamed, "title", "titles")
                    ),
                )
                .source(JOB_NAME)
                .batch_id(batch.clone())
                .action(NoticeAction::UndoRename),
            );
        }
        held.extend(
            step.items
                .iter()
                .filter(|item| HELD.contains(&item.status.as_str())),
        );
        if step.status == StepStatus::Failed && step.items.is_empty() {
            not_run.push(&step.message);
        }
    }
    if !held.is_empty() {
        let mut lines: Vec<String> = held
            .iter()
            .take(LINES_KEPT)
            .map(|item| {
                let name = if item.year > 0 {
                    format!("{} ({})", item.title, item.year)
                } else {
                    item.title.clone()
                };
                match &item.message {
                    Some(why) => format!("{name}: {why}"),
                    None => name,
                }
            })
            .collect();
        if held.len() > LINES_KEPT {
            lines.push(format!("…and {} more.", held.len() - LINES_KEPT));
        }
        lines.push("Open them in Rename to see the plan and settle it.".to_owned());
        let verb = if held.len() == 1 { "was" } else { "were" };
        notices.push(
            AppNotice::new(
                NoticeLevel::Warning,
                format!(
                    "{} {verb} not renamed after Scrape",
                    counted(held.len(), "title", "titles")
                ),
            )
            .source(JOB_NAME)
            .detail(lines.join("\n")),
        );
    }
    if !not_run.is_empty() {
        not_run.dedup();
        notices.push(
            AppNotice::new(NoticeLevel::Warning, "Rename after Scrape did not run")
                .source(JOB_NAME)
                .detail(not_run.join(" ")),
        );
    }
    notices
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(title: &str, year: u16, status: &str, message: Option<&str>) -> ItemResult {
        let mut item = ItemResult::new(Some(1), title, year, status);
        item.message = message.map(str::to_owned);
        item
    }

    fn rename_step(renamed: u64, undo: Option<&str>, items: Vec<ItemResult>) -> StepReport {
        let mut step = StepReport::new(Step::Rename, StepStatus::Done, "Renamed")
            .count("renamed", usize::try_from(renamed).unwrap());
        step.undo = undo.map(str::to_owned);
        step.items = items;
        step
    }

    #[test]
    fn a_clean_rename_says_how_many_with_its_undo() {
        let notices = notices_for(&[rename_step(
            12,
            Some("batch-7"),
            vec![item("Heat", 1995, "renamed", None)],
        )]);
        assert_eq!(notices.len(), 1);
        let done = &notices[0];
        assert_eq!(done.level, NoticeLevel::Info);
        assert_eq!(done.title, "Renamed 12 titles after Scrape");
        assert_eq!(done.batch_id.as_deref(), Some("batch-7"));
        assert_eq!(done.action, Some(NoticeAction::UndoRename));
    }

    #[test]
    fn held_back_titles_are_listed_with_their_reason() {
        let notices = notices_for(&[rename_step(
            1,
            Some("b"),
            vec![
                item("Heat", 1995, "renamed", None),
                item(
                    "Alien",
                    1979,
                    "conflict",
                    Some("It would take the name of Aliens."),
                ),
                item("Tidy", 2001, "skipped", None),
            ],
        )]);
        assert_eq!(notices.len(), 2);
        assert_eq!(notices[0].title, "Renamed 1 title after Scrape");
        let held = &notices[1];
        assert_eq!(held.level, NoticeLevel::Warning);
        assert_eq!(held.title, "1 title was not renamed after Scrape");
        let detail = held.detail.as_deref().unwrap();
        assert!(detail.starts_with("Alien (1979): It would take the name of Aliens."));
        assert!(!detail.contains("Tidy"), "a tidy title is not held back");
    }

    #[test]
    fn nothing_renamed_and_nothing_held_says_nothing() {
        let notices = notices_for(&[rename_step(
            0,
            None,
            vec![item("Tidy", 2001, "skipped", None)],
        )]);
        assert!(notices.is_empty());
    }

    #[test]
    fn a_step_that_could_not_run_says_why() {
        let step = StepReport::new(
            Step::Rename,
            StepStatus::Failed,
            "Metadata only is on in Settings › Naming.",
        );
        let notices = notices_for(&[step]);
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].title, "Rename after Scrape did not run");
        assert_eq!(
            notices[0].detail.as_deref(),
            Some("Metadata only is on in Settings › Naming.")
        );
    }

    #[test]
    fn a_long_list_is_cut_short() {
        let items = (0..11)
            .map(|n| item(&format!("Title {n}"), 2000, "held", None))
            .collect();
        let notices = notices_for(&[rename_step(0, None, items)]);
        let detail = notices[0].detail.as_deref().unwrap();
        assert!(detail.contains("…and 3 more."));
        assert_eq!(notices[0].title, "11 titles were not renamed after Scrape");
    }

    #[test]
    fn the_plan_renames_exactly_the_titles_given() {
        let plan = plan(EntityKind::Movie, &[3, 5]);
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].step, Step::Rename);
        assert_eq!(plan.steps[0].scope, Some(Scope::All));
        assert_eq!(plan.steps[0].selection.ids, vec![3, 5]);
        assert!(!plan.dry_run);
    }

    #[test]
    fn movies_and_shows_go_in_separate_groups_and_the_rest_is_left_out() {
        let groups = group(vec![
            (ItemId(1), Some(ItemKind::Show)),
            (ItemId(2), Some(ItemKind::Movie)),
            (ItemId(3), None),
            (ItemId(4), Some(ItemKind::Movie)),
        ]);
        assert_eq!(
            groups,
            vec![
                (EntityKind::Movie, vec![2, 4]),
                (EntityKind::Tvshow, vec![1])
            ]
        );
    }
}
