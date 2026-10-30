//! The selection actions of the after-scrape services: one command, `run_item_action`, runs a
//! named action on a list of titles.
//!
//! The rules live in `cv_app::automation::item_action`: each action is one step of the batch
//! engine, queued as a job of the batch backend, so it shows in the sidebar progress card under its
//! name and runs one at a time with the command line. The command waits for the report and
//! answers with how it went; the UI says so with a toast. Which actions exist is the
//! `ItemAction` list; the page knows the ids (`fetch-ratings`, `reload-media-info` ...).

use cv_app::automation::{
    CommandBackend, ItemAction, ItemActionError, ItemActionOutcome, StepStatus,
};
use cv_app::services::AppServices;
use cv_domain::ItemId;
use serde::Serialize;
use specta::Type;
use std::sync::Arc;

/// A refusal: `message` is ready to show, `code` says why (`unknown_action`, `bad_id`,
/// `nothing_selected`, `unsupported`, `storage`, `not_running`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct ItemActionCommandError {
    pub code: String,
    pub message: String,
}

impl From<ItemActionError> for ItemActionCommandError {
    fn from(error: ItemActionError) -> Self {
        Self {
            code: error.code().to_owned(),
            message: match error {
                ItemActionError::NotRunning => {
                    "CineVault is still starting. Try again in a moment.".to_owned()
                }
                other => capitalised(&other.to_string()),
            },
        }
    }
}

fn capitalised(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// How an action ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ItemActionStatusDto {
    /// It ran.
    Done,
    /// A preview: what it would do.
    Planned,
    /// Nothing to do, or a key, account or setting is missing; the message says which.
    Skipped,
    /// A tool is not installed.
    Unavailable,
    Failed,
}

impl From<StepStatus> for ItemActionStatusDto {
    fn from(status: StepStatus) -> Self {
        match status {
            StepStatus::Done => Self::Done,
            StepStatus::Planned => Self::Planned,
            StepStatus::Skipped => Self::Skipped,
            StepStatus::Unavailable => Self::Unavailable,
            StepStatus::Failed => Self::Failed,
        }
    }
}

/// A title that failed, was skipped or (in a preview) would change, with why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct ItemActionLineDto {
    pub title: String,
    /// `failed`, `skipped` or `planned`.
    pub status: String,
    pub message: Option<String>,
}

/// What an action did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct ItemActionResultDto {
    pub action: String,
    /// The jobs the Activity drawer shows for it.
    pub jobs: Vec<u32>,
    pub status: ItemActionStatusDto,
    /// One plain sentence.
    pub message: String,
    /// Titles the action ran on.
    pub titles: u32,
    pub done: u32,
    pub unchanged: u32,
    pub skipped: u32,
    pub failed: u32,
    /// Selected titles it did not run on, with why.
    pub left_out: Vec<String>,
    pub lines: Vec<ItemActionLineDto>,
}

impl From<ItemActionOutcome> for ItemActionResultDto {
    fn from(outcome: ItemActionOutcome) -> Self {
        Self {
            action: outcome.action,
            jobs: outcome
                .jobs
                .into_iter()
                .filter_map(|job| u32::try_from(job).ok())
                .collect(),
            status: outcome.status.into(),
            message: outcome.message,
            titles: outcome.titles,
            done: outcome.done,
            unchanged: outcome.unchanged,
            skipped: outcome.skipped,
            failed: outcome.failed,
            left_out: outcome.left_out,
            lines: outcome
                .lines
                .into_iter()
                .map(|line| ItemActionLineDto {
                    title: line.title,
                    status: line.status,
                    message: line.message,
                })
                .collect(),
        }
    }
}

fn refused(code: &str, message: impl Into<String>) -> ItemActionCommandError {
    ItemActionCommandError {
        code: code.to_owned(),
        message: message.into(),
    }
}

/// Runs the action on the titles and resolves when it has ended. The job shows in the Activity
/// drawer while it runs. `action` is an id such as `download-trailer` or `reload-media-info`.
pub async fn run_item_action(
    services: Option<&Arc<AppServices>>,
    backend: Option<Arc<CommandBackend>>,
    ids: Vec<String>,
    action: String,
) -> Result<ItemActionResultDto, ItemActionCommandError> {
    let action = ItemAction::parse(&action)
        .ok_or_else(|| ItemActionCommandError::from(ItemActionError::UnknownAction(action)))?;
    let ids: Vec<ItemId> = ids
        .iter()
        .map(|id| {
            id.trim()
                .parse::<i64>()
                .map(ItemId)
                .map_err(|_| refused("bad_id", format!("{id:?} is not a library item id.")))
        })
        .collect::<Result<_, _>>()?;
    let (Some(services), Some(backend)) = (services, backend) else {
        return Err(ItemActionError::NotRunning.into());
    };
    cv_app::automation::run_item_action(&backend, services.db(), &action, &ids)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refusals_carry_a_code_and_a_sentence_that_starts_with_a_capital() {
        let error = ItemActionCommandError::from(ItemActionError::UnknownAction("nope".into()));
        assert_eq!(error.code, "unknown_action");
        assert_eq!(error.message, "There is no action called `nope`");
        let starting = ItemActionCommandError::from(ItemActionError::NotRunning);
        assert_eq!(starting.code, "not_running");
        assert!(starting.message.contains("starting"));
    }

    #[test]
    fn every_status_maps_to_its_own_word() {
        let words: Vec<ItemActionStatusDto> = [
            StepStatus::Done,
            StepStatus::Planned,
            StepStatus::Skipped,
            StepStatus::Unavailable,
            StepStatus::Failed,
        ]
        .into_iter()
        .map(Into::into)
        .collect();
        assert_eq!(
            words,
            [
                ItemActionStatusDto::Done,
                ItemActionStatusDto::Planned,
                ItemActionStatusDto::Skipped,
                ItemActionStatusDto::Unavailable,
                ItemActionStatusDto::Failed
            ]
        );
    }
}
