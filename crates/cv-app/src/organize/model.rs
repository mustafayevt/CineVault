//! The requests and answers of the Organize service. Plain data: the desktop app turns them into
//! its IPC types, the CLI can do the same.

use std::collections::BTreeMap;

use cv_domain::{ItemId, MediaFileType};
use cv_rename::RenameProfile;

/// What Organize looks at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// Every movie and show of the library.
    Everything,
    /// The given items. A set stands for its movies, a season or episode for its show.
    Items(Vec<ItemId>),
}

/// One of the five naming templates of a profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PatternField {
    MovieFolder,
    MovieFile,
    ShowFolder,
    SeasonFolder,
    EpisodeFile,
}

impl PatternField {
    pub const ALL: [PatternField; 5] = [
        PatternField::MovieFolder,
        PatternField::MovieFile,
        PatternField::ShowFolder,
        PatternField::SeasonFolder,
        PatternField::EpisodeFile,
    ];

    /// True for the three TV templates.
    pub const fn is_tv(self) -> bool {
        matches!(
            self,
            PatternField::ShowFolder | PatternField::SeasonFolder | PatternField::EpisodeFile
        )
    }
}

/// What the user chose for a conflicting item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    /// Put the items that collide side by side in one folder.
    KeepAsVersions,
    /// Leave the item out.
    Skip,
    /// Give the item another folder and/or video base name.
    Edit {
        folder: Option<String>,
        base: Option<String>,
    },
}

/// A preview request. `profile` is the working profile: the active one with the editor's
/// patterns applied (see [`PatternSet::apply_to`]) and the NFO file names of the NFO settings.
#[derive(Debug, Clone)]
pub struct PreviewRequest {
    pub scope: Scope,
    pub profile: RenameProfile,
    /// Resolutions by item id.
    pub choices: BTreeMap<String, Choice>,
}

/// An apply request: the same request as the preview the user looked at, and the ids that are
/// checked. An id that is not a clean change in the new plan is not run.
#[derive(Debug, Clone)]
pub struct ApplyRequest {
    pub preview: PreviewRequest,
    pub include: Vec<String>,
}

/// Movie or show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Movie,
    Show,
}

/// What a row of the preview is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowStatus {
    /// A clean change that can be applied.
    Change,
    /// Blocked by a conflict until the user resolves it.
    Conflict,
    /// The user chose to skip it.
    Skipped,
    /// It cannot be planned; the text says why.
    Unplanned(String),
    /// It is not scraped yet, so it is not planned: a rename only follows a scrape.
    Unscraped,
    /// Held back: the names use media-info tokens (resolution, HDR, audio) and the video details
    /// are not read yet, so a rename would strip the quality tags. It is planned once they are
    /// read; the preview offers to read them now. [`cv_rename::MEDIA_HELD_REASON`] says why.
    MediaUnread,
}

/// What moves along with the item, by kind.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CompanionCounts {
    pub nfo: u32,
    pub artwork: u32,
    pub subtitles: u32,
    pub trailers: u32,
    /// Everything else that is not the video itself: extras, theme music, media-info sidecars.
    pub other: u32,
}

/// One file an item moves, copies or sends to the trash, for the preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    pub kind: MediaFileType,
    /// Where the file is now, relative to the data source root.
    pub from: String,
    /// Where it goes, relative to the root: moved to the first, copied to the others. Empty when
    /// it goes to the trash folder.
    pub to: Vec<String>,
}

/// One item of the diff list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewRow {
    /// The library id.
    pub id: String,
    pub kind: RowKind,
    pub title: String,
    pub year: u16,
    /// The absolute path of the data source root, with a trailing separator.
    pub root: String,
    /// The folder now and after, relative to the root.
    pub before: String,
    pub after: String,
    /// The name of the main video file now and after (a show lists its first episode).
    pub before_file: String,
    pub after_file: String,
    pub status: RowStatus,
    /// Files the item moves or copies.
    pub moves: u32,
    /// Files it sends to the trash folder.
    pub trashed: u32,
    pub companions: CompanionCounts,
    /// Every file the item moves, copies or trashes (the video, its companions, a show's
    /// episodes), in plan order. Files that stay where they are, as they are, are left out.
    pub files: Vec<FileChange>,
}

/// What a conflict is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictCode {
    /// Two items resolve to the same destination.
    Duplicate,
    /// The destination exists and belongs to something else.
    Foreign {
        owner: Option<String>,
    },
    PathTooLong {
        length: u32,
        limit: u32,
    },
    NameTooLong {
        length: u32,
        limit: u32,
    },
    CaseOnly,
}

/// A resolution the user can pick for a conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionOption {
    KeepAsVersions,
    Skip,
    Edit,
}

/// A conflict between items (or between an item and the disk).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictRow {
    pub code: ConflictCode,
    /// The ids of the items involved, with their titles in the same order.
    pub items: Vec<String>,
    pub titles: Vec<String>,
    /// The current folders of the items, in the same order.
    pub folders: Vec<String>,
    /// The destination (relative to the root) the conflict is about.
    pub path: String,
    pub options: Vec<ResolutionOption>,
}

/// The numbers above the diff list.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub change: u32,
    pub tidy: u32,
    pub conflicts: u32,
    pub locked: u32,
    pub skipped: u32,
    pub unplanned: u32,
    /// Files the clean changes move or copy.
    pub moves: u32,
}

/// Which edge case an example shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExampleKind {
    /// A movie with an edition.
    Edition,
    /// A movie without HDR.
    NoHdr,
    /// A colon in the title.
    Colon,
    Plain,
}

/// One live example of the templates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Example {
    pub kind: ExampleKind,
    pub title: String,
    pub year: u16,
    pub folder: String,
    pub file: String,
    /// It stands for a title the library does not have (a built-in sample).
    pub sample: bool,
}

/// A template that does not work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateProblem {
    pub field: PatternField,
    pub message: String,
}

/// The answer to a preview request.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Preview {
    /// Items that change, conflict, were skipped or cannot be planned. Tidy and locked items are
    /// only counted.
    pub rows: Vec<PreviewRow>,
    pub conflicts: Vec<ConflictRow>,
    pub counts: Counts,
    pub examples: Vec<Example>,
    /// Set when a template is broken; then `rows` is empty.
    pub problems: Vec<TemplateProblem>,
}

/// How one item ended in an apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppliedStatus {
    Done,
    /// A step failed and the item was taken back; the text says why.
    Failed(String),
    Canceled,
}

/// One item of an apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedItem {
    pub id: String,
    pub title: String,
    pub status: AppliedStatus,
    pub moved: u32,
    pub copied: u32,
    pub trashed: u32,
    pub notes: Vec<String>,
}

/// The result of an apply.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Applied {
    /// The journal batch; `None` when nothing was run.
    pub batch_id: Option<String>,
    pub items: Vec<AppliedItem>,
    /// Checked ids that were not run because the new plan no longer has them as a clean change,
    /// or because the title was busy with other file work (see [`Applied::busy`]).
    pub not_run: Vec<String>,
    /// Checked ids left out because other file work holds the title (see
    /// `crate::title_guard`): a scrape's NFO or artwork is still being written into its folder
    /// (left out at once), or a write, move or rename of it did not end within the wait. Also in
    /// [`Applied::not_run`]; the next apply renames them.
    pub busy: Vec<BusyTitle>,
}

/// A checked title an apply left out because it was busy, with the reason in plain words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BusyTitle {
    pub id: String,
    pub reason: String,
}

impl Applied {
    /// Why the checked id `id` was left out as busy, when it was.
    #[must_use]
    pub fn busy_reason(&self, id: &str) -> Option<&str> {
        self.busy
            .iter()
            .find(|b| b.id == id)
            .map(|b| b.reason.as_str())
    }

    pub fn done(&self) -> usize {
        self.items
            .iter()
            .filter(|i| i.status == AppliedStatus::Done)
            .count()
    }

    pub fn failed(&self) -> usize {
        self.items
            .iter()
            .filter(|i| matches!(i.status, AppliedStatus::Failed(_)))
            .count()
    }
}

/// One batch of the journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalBatch {
    pub batch_id: String,
    /// When the first operation was written, epoch milliseconds.
    pub created_ms: i64,
    /// Items with a `done` or `undone` operation.
    pub items: u32,
    pub operations: u32,
    pub done: u32,
    pub undone: u32,
    pub failed: u32,
    /// Operations a crash left unsettled.
    pub planned: u32,
    /// The titles of the items, in the order the batch reached them.
    pub titles: Vec<String>,
    /// The files the batch moved or copied, counted as the Organize footer counts them before
    /// the apply (a folder rename moves every file in it); for a move between data sources, the
    /// files it copied to the other volume. `0` when not known (a move on one volume renames
    /// the folder only).
    pub files: u32,
    /// When it was taken back last, epoch milliseconds; `None` when nothing of it was.
    pub undone_ms: Option<i64>,
}

impl JournalBatch {
    /// There is something to take back and nothing is waiting for recovery.
    pub fn undoable(&self) -> bool {
        self.done > 0 && self.planned == 0
    }
}

/// An item an undo could not restore.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotRestorable {
    pub id: Option<String>,
    pub title: String,
    pub path: String,
    pub reason: String,
}

/// The result of an undo.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UndoOutcome {
    pub batch_id: String,
    /// Items fully taken back.
    pub restored: u32,
    pub not_restorable: Vec<NotRestorable>,
    /// Items the disk failed on in the middle of the undo; the text says why.
    pub failed: Vec<NotRestorable>,
}

impl UndoOutcome {
    pub fn is_complete(&self) -> bool {
        self.not_restorable.is_empty() && self.failed.is_empty()
    }
}

/// Why an operation could not run at all. Problems with single items are in the answers.
#[derive(Debug, thiserror::Error)]
pub enum OrganizeError {
    #[error("the library could not be read: {0}")]
    Db(#[from] cv_db::DbError),
    #[error("{0}")]
    Rename(#[from] cv_rename::RenameError),
    #[error("the journal could not be used: {0}")]
    Exec(#[from] cv_rename::exec::ExecError),
    #[error("the batch could not be undone: {0}")]
    Undo(#[from] cv_rename::exec::UndoError),
    #[error("the journal could not be read: {0}")]
    Journal(#[from] cv_rename::exec::JournalError),
    #[error("there is no organize batch {0:?}")]
    UnknownBatch(String),
    /// A move between data sources could not be taken back; the text says why.
    #[error("{0}")]
    Move(String),
}
