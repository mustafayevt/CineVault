//! The executor: applies the plans of a batch to the disk, safely.
//!
//! [`Executor::run`] takes the [`BatchItem`](crate::conflict::BatchItem)s that survived conflict
//! detection and performs their operations one item at a time. The rules:
//!
//! - **Write-ahead.** Every step gets a row in the [`JournalStore`] (`rename_journal`, as
//!   `planned`) *before* it touches the disk, and is settled (`done` or `failed`) after. No step
//!   runs without its row; a journal that cannot be written stops the batch.
//! - **Same volume: rename.** A move is a single atomic `rename`, so it is instant. A whole
//!   folder is renamed once when the plan says [`FolderChange::Rename`](crate::FolderChange);
//!   otherwise (a shared folder, files left behind, a destination folder with content, another
//!   volume) the files are moved one by one.
//! - **Cross volume: copy, verify, delete.** The copy is written next to the destination as
//!   `<name>.cvpart`, compared byte for byte with the original, renamed into place, and only then
//!   is the original removed. The original is whole until the copy is proven.
//! - **Trash, never delete.** A file the plan no longer needs, a file a move or copy would
//!   replace, and an unwanted file go to `<data source>/.deletedByCineVault/<batch>/<path>`.
//!   Same volume, so it is instant. An NFO is trashed or replaced only when CineVault wrote it
//!   (`cv_nfo::ownership`); a foreign NFO is left alone and noted.
//! - **Case.** A rename that only changes case goes through a temporary name (two journal rows,
//!   both written first). A rename onto a different file replaces it by trashing it.
//! - **Long paths.** On Windows the real [`StdFs`] hands the system extended-length paths
//!   (`\\?\C:\...`, see [`extended_length_path`]) once a path is long.
//! - **Clean-up.** After an item, folders it left empty are removed up to (never including) the
//!   data source root and never the trash; when unwanted patterns are set
//!   ([`ExecOptions::unwanted_patterns`], [`DEFAULT_UNWANTED`]), matching files in the renamed
//!   folder, or in an old folder that holds nothing else, go to the trash first.
//! - **Item by item.** A step that fails takes back the item's earlier steps (newest first) and
//!   the item is reported `Failed`; other items go on. Operations are ordered so a file that is
//!   another operation's destination is moved away first, within an item and between items (a
//!   swap).
//! - **Library paths** follow in the same transaction as the journal row when the store is the
//!   [`DbJournal`]: `media_file` rows of a moved file, `item.path` and `media_file.directory`
//!   below a moved folder. No rescan. [`DbJournal::with_observer`] also reports each committed move
//!   (undo and recovery included) so in-memory path caches, such as the poster index behind
//!   `cv://img`, follow it.
//!
//! - **Between data sources**. [`Executor::move_folders`] moves whole folders into another
//!   data source with one journal row each: a rename on one volume, a verified tree copy on another
//!   (`tree.rs`), with the data source of the items handed over in the same transaction.
//!   [`Executor::trash_paths`] is the journaled trash of "delete with files" for paths the system
//!   trash cannot take.
//!
//! [`recover`] settles the rows a crash left `planned`, looking at the disk, so a crash in the
//! middle of a batch leaves every file at its old or its new place (see its documentation for
//! the table). A batch can then be run again: steps whose result is already on the disk are
//! counted as `already_applied`.
//!
//! The journal rows are what [`undo_batch`] replays: a batch is taken back newest
//! operation first, every destination is verified (size and modification time) and an item that
//! cannot be restored is reported with the reason while the others are restored.

#[cfg(test)]
mod edge_tests;
#[cfg(any(test, feature = "fault-injection"))]
pub mod fault;
mod fs;
mod journal;
mod recover;
mod run;
#[cfg(test)]
mod safety_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod transfer_tests;
mod tree;
mod undo;
#[cfg(test)]
mod undo_tests;

pub use fs::{Fs, Meta, OWN_READ_WAIT, Patient, ReadByCineVault, StdFs, extended_length_path};
pub use journal::{
    BatchSummary, Completion, DbJournal, ItemMark, ItemState, JournalEntry, JournalError,
    JournalOp, JournalRow, JournalState, JournalStore, MemoryJournal, PathObserver,
};
pub use recover::{
    ABANDONED, INTERRUPTED, RESUMED, Recovered, Recovery, RecoveryReport, ResumeReport,
    UnfinishedResume, recover, recover_with, resume_interrupted, resume_unfinished,
    settle_interrupted,
};
pub use run::{
    BatchReport, ExecOptions, Executor, FolderMove, ItemPlan, ItemReport, ItemStatus, TrashRequest,
    new_batch_id, trash_target, unwanted_files_in,
};
pub use undo::{
    NotRestored, UndoError, UndoItemReport, UndoOptions, UndoReport, UndoStatus, undo_batch,
};

/// The trash folder in the root of a data source.
pub const TRASH_DIR: &str = ".deletedByCineVault";

/// The unwanted-file patterns the app offers by default.
pub const DEFAULT_UNWANTED: [&str; 4] = [".txt", ".url", ".html", ".sfv"];

/// The prefix of the temporary name a case-only rename goes through.
pub(crate) const CASE_TMP_PREFIX: &str = ".cvcase-";

/// The suffix of a copy that is not verified yet.
pub(crate) const PART_SUFFIX: &str = ".cvpart";

/// Why a batch could not run.
#[derive(Debug, thiserror::Error)]
pub enum ExecError {
    #[error(transparent)]
    Journal(#[from] JournalError),
    /// An unwanted-file pattern is neither an extension nor a regular expression.
    #[error("the unwanted-file pattern `{pattern}` is not valid: {reason}")]
    BadPattern { pattern: String, reason: String },
}
