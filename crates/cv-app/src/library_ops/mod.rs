//! Library operations that move, remove or delete whole titles.
//!
//! Spec: `metadata-editing` ("Change data source and delete", "Clone and manual entries") and
//! `library-scanning` ("Skip rules").
//!
//! * [`ChangeSourceService`] moves movies and shows to another data source. The folder of a title
//!   keeps its place below the source (`Action/Heat (1995)` stays `Action/Heat (1995)`). A preview
//!   says what would happen, row by row; applying runs the folder moves through the rename
//!   executor, so every move is journaled first, goes through a verified copy on another volume
//!   and can be undone as one batch (`OrganizeService::undo`). The library follows each move in
//!   the same transaction as its journal row, with no rescan.
//! * [`RemovalService`] removes titles from the library and keeps their files; optionally it
//!   writes a `.cinevaultignore` marker into the title's folder so the next scan skips it. The
//!   library is only ever changed by this explicit call, never by a scan.
//! * [`RemovalService::delete_with_files`] sends the files to the system bin (Recycle Bin, macOS
//!   Trash, freedesktop Trash on Linux, sent by one rename) through [`SystemBin`], which remembers where each one went, and hides the titles under
//!   one History entry; History's Undo restores both. Where the bin cannot take a path (a network
//!   share), the files go to `<data source>/.deletedByCineVault/<batch>/` instead. Files are never
//!   deleted for good here, unless Settings › Deleted files is Delete permanently (chosen behind
//!   the typed confirmation): the delete is then a permanent delete ([`PolicyBin`] serves the
//!   other choices: the trash folder of each drive, a custom trash folder).
//! * [`RemovalService::erase`] is the one permanent delete: only with the typed confirmation
//!   `DELETE`, never a library folder root, recorded in History first (see `erase`).
//! * [`ReviewService`] lists the rows that are missing on disk, offline, duplicates of another row
//!   or set aside by a rescan, and removes (with Undo), relocates or keeps the ones the user
//!   chooses. Nothing is removed by it on its own.
//!
//! Every operation looks at the disk as it is now, and refuses a title it cannot handle safely
//! (a folder that other titles share, a data source root, an unreachable drive) with a reason
//! instead of guessing.

pub(crate) mod bin_volumes;
mod change_source;
mod erase;
mod move_engine;
mod move_plan;
pub(crate) mod policy_bin;
mod recovery;
mod removal;
mod review;
#[cfg(test)]
pub(crate) mod tests;
pub(crate) mod trash_bin;
pub(crate) mod uncached;
mod volume;
pub(crate) mod xdg_dirs;
#[cfg(unix)]
pub(crate) mod xdg_trash;

use cv_db::DbError;
use cv_domain::{Item, ItemDetail, ItemId};

pub use bin_volumes::{BinVolumes, NoBin, SystemVolumes, VolumeBin, bin_refusal};
pub use change_source::{
    ChangeSourceService, ChangedHook, MoveApplied, MoveOutcome, MovePlan, MoveResult, MoveRow,
    MoveStatus,
};
pub use erase::{ERASE_CONFIRMATION, EraseFs, EraseReport, ErasedTitle, StdErase};
pub use move_engine::{
    BinLedger, DbBinLedger, JobOutcome, MOVE_BATCH_PREFIX, MoveControl, MoveEngine, MoveFs,
    MovePhase, MoveProgress, STOPPED_PREFIX, StdMoveFs, TakeBackStatus, TakenBack, is_move_batch,
    new_move_batch_id,
};
pub use move_plan::{EntryKind, MoveEntry, SPACE_MARGIN_FLOOR};
pub(crate) use policy_bin::erase_path;
pub use policy_bin::{CHOSEN_FOLDER, ERASED, ERASED_PREFIX, OTHER_VOLUME, PolicyBin, is_erased};
pub use recovery::{
    BatchKind, ExceptMoves, RecoveryError, RecoveryService, ResumeOutcome, RollBackOutcome,
    UnfinishedBatch,
};
pub(crate) use removal::bin_name;
pub use removal::{
    DeleteMethod, DeleteReport, DeletedTitle, FolderUse, IgnoreOutcome, Problem, RemovalPlan,
    RemovalRow, RemovalService, RemoveReport, RemovedTitle,
};
pub use review::{
    DuplicateOf, Relocated, RestoredTitle, ReviewList, ReviewReason, ReviewRemoval, ReviewRow,
    ReviewService,
};
pub use trash_bin::{
    BinError, Binned, BinnedItem, FOLDER_PREFIX, FolderOnly, NOT_IN_BIN, OsTrash, PATH_TAKEN,
    SendOutcome, SystemBin, SystemTrash, present_all, restore_all, send_or_fallback,
};

/// What the system bin is called in words: the Recycle Bin on Windows, the Trash elsewhere.
pub const BIN_NAME: &str = if cfg!(windows) {
    "Recycle Bin"
} else {
    "Trash"
};

/// The name of the marker file that hides a folder from scans.
pub use cv_scan::skip::MARKER_IGNORE;

/// What kind of title a row stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleKind {
    Movie,
    Show,
    Season,
    Episode,
}

impl TitleKind {
    pub(crate) fn of(item: &Item) -> Option<Self> {
        match item.detail {
            ItemDetail::Movie(_) => Some(Self::Movie),
            ItemDetail::Show(_) => Some(Self::Show),
            ItemDetail::Season(_) => Some(Self::Season),
            ItemDetail::Episode(_) => Some(Self::Episode),
            ItemDetail::Set(_) => None,
        }
    }

    /// True for the kinds that own a folder in a data source.
    #[must_use]
    pub const fn owns_folder(self) -> bool {
        matches!(self, Self::Movie | Self::Show)
    }
}

/// Why an operation did not run.
#[derive(Debug, thiserror::Error)]
pub enum LibraryOpsError {
    #[error("the library could not be read or written: {0}")]
    Db(#[from] DbError),
    #[error("this data source is not in the library")]
    UnknownSource(i64),
    /// The request breaks a rule; the message says which.
    #[error("{message}")]
    Rejected { code: &'static str, message: String },
    /// The journal or a disk step stopped the whole batch.
    #[error("{0}")]
    Disk(String),
}

impl LibraryOpsError {
    /// A stable code for callers to branch on.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Db(_) => "storage",
            Self::UnknownSource(_) => "unknown_source",
            Self::Rejected { code, .. } => code,
            Self::Disk(_) => "disk",
        }
    }

    pub(crate) fn rejected(code: &'static str, message: impl Into<String>) -> Self {
        Self::Rejected {
            code,
            message: message.into(),
        }
    }
}

/// The ids once, in order.
pub(crate) fn unique(ids: &[ItemId]) -> Vec<ItemId> {
    let mut seen = std::collections::BTreeSet::new();
    ids.iter().copied().filter(|id| seen.insert(*id)).collect()
}

/// The parts of `path` below `root`, or `None` when it is not below it. Compared without regard
/// to separator style, and to case where the platform's file names ignore it (Windows, macOS;
/// on Linux `Films` and `films` are two folders); the parts keep their spelling. A path with a
/// `.` or `..` part below the root is never below it (it may name the root itself or a place
/// outside).
pub(crate) fn parts_below(root: &str, path: &str) -> Option<Vec<String>> {
    let split = |text: &str| -> Vec<String> {
        text.split(['\\', '/'])
            .filter(|part| !part.is_empty())
            .map(str::to_owned)
            .collect()
    };
    let (root, path) = (split(root), split(path));
    if path.len() < root.len() {
        return None;
    }
    let same = root
        .iter()
        .zip(&path)
        .all(|(a, b)| cv_fs::paths::fold_name(a) == cv_fs::paths::fold_name(b));
    let rest = &path[root.len()..];
    // `.` and `..` name the root itself or a place outside it, whatever the parts say.
    if !same || rest.iter().any(|part| part == "." || part == "..") {
        return None;
    }
    Some(rest.to_vec())
}

/// `root` and `parts` joined with the separator `root` uses.
pub(crate) fn join_like(root: &str, parts: &[String]) -> String {
    let separator = if root.contains('\\') || !root.contains('/') && cfg!(windows) {
        '\\'
    } else {
        '/'
    };
    let mut out = root.trim_end_matches(['\\', '/']).to_owned();
    for part in parts {
        out.push(separator);
        out.push_str(part);
    }
    out
}

pub(crate) fn same_folder(a: &str, b: &str) -> bool {
    parts_below(a, b).is_some_and(|rest| rest.is_empty())
}

pub(crate) fn now_millis() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
