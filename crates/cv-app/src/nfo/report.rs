//! What an NFO action reports: one unit per item (or per file of episodes), and totals.

use std::path::PathBuf;

use cv_domain::ItemId;
use serde::{Deserialize, Serialize};

/// What happened to one NFO path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FileOutcome {
    /// A new file was written.
    Created,
    /// An existing managed (or empty or pointer) file was replaced.
    Updated,
    /// The file already holds this content.
    Unchanged,
    /// A foreign file sits at the path; it was left alone.
    KeptForeign,
    /// A managed NFO the naming set no longer includes was moved to the journal's trash.
    MovedToTrash,
    Failed {
        message: String,
    },
}

/// One NFO path and what happened to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileReport {
    pub path: PathBuf,
    pub outcome: FileOutcome,
}

/// Why a unit was not processed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    /// The id is not in the library.
    NotFound,
    /// The kind of item has no NFO of its own (a season without a show, say).
    Unsupported,
    /// The item has no video file or folder to put an NFO next to.
    NoFiles,
    /// The folder is not reachable (an unplugged drive).
    FolderMissing,
    /// Sets are written under the collection data folder, which is not set.
    NoCollectionFolder,
    /// No NFO of the item exists.
    NoNfo,
    /// The NFO exists but has no title, so it is not CineVault's to read.
    NotManaged,
}

/// How a unit ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UnitStatus {
    /// Something changed: a file was written or moved, or the library values changed.
    Changed,
    /// Nothing needed to change.
    Unchanged,
    /// Every target held a foreign file, so nothing was written.
    KeptForeign,
    Skipped {
        reason: SkipReason,
    },
    Failed {
        message: String,
    },
}

/// One item (a movie, a set, a show's `tvshow.nfo`, a season, a video file with its episodes).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnitReport {
    pub item: ItemId,
    /// A name to show: the title, with the season and episode for TV.
    pub label: String,
    pub status: UnitStatus,
    pub files: Vec<FileReport>,
}

impl UnitReport {
    pub(crate) fn skipped(item: ItemId, label: impl Into<String>, reason: SkipReason) -> Self {
        Self {
            item,
            label: label.into(),
            status: UnitStatus::Skipped { reason },
            files: Vec::new(),
        }
    }

    pub(crate) fn failed(item: ItemId, label: impl Into<String>, message: String) -> Self {
        Self {
            item,
            label: label.into(),
            status: UnitStatus::Failed { message },
            files: Vec::new(),
        }
    }
}

/// Which action ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    Read,
    Rewrite,
}

/// Totals over the units, ready for a toast.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Totals {
    pub changed: u32,
    pub unchanged: u32,
    pub kept_foreign: u32,
    pub skipped: u32,
    pub failed: u32,
    /// Files created or replaced.
    pub files_written: u32,
    /// Files moved to the trash because the naming set dropped them.
    pub files_trashed: u32,
}

/// The result of Read NFO or Rewrite NFO.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionReport {
    pub kind: ActionKind,
    /// Names the undo record; `None` when nothing changed, so there is nothing to undo.
    pub token: Option<String>,
    pub units: Vec<UnitReport>,
    pub totals: Totals,
}

impl ActionReport {
    pub(crate) fn new(kind: ActionKind, token: Option<String>, units: Vec<UnitReport>) -> Self {
        let mut totals = Totals::default();
        for unit in &units {
            match unit.status {
                UnitStatus::Changed => totals.changed += 1,
                UnitStatus::Unchanged => totals.unchanged += 1,
                UnitStatus::KeptForeign => totals.kept_foreign += 1,
                UnitStatus::Skipped { .. } => totals.skipped += 1,
                UnitStatus::Failed { .. } => totals.failed += 1,
            }
            for file in &unit.files {
                match file.outcome {
                    FileOutcome::Created | FileOutcome::Updated => totals.files_written += 1,
                    FileOutcome::MovedToTrash => totals.files_trashed += 1,
                    _ => {}
                }
            }
        }
        Self {
            kind,
            token,
            units,
            totals,
        }
    }
}

/// What an undo did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UndoSummary {
    /// Files and items put back.
    pub restored: u32,
    /// What could not be put back, each with its reason (the file or item changed since).
    pub skipped: Vec<String>,
}
