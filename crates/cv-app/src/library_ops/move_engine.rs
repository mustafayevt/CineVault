//! Moving a title folder to another data source, safely (spec `source-moves`).
//!
//! Nothing here is clever; every step is there to keep the original files whole until the copy
//! is proven, and to leave a record that says what to do after any stop.
//!
//! # Journal first
//!
//! Before the disk is touched, the batch and every file's source and destination are written to
//! the journal (`rename_journal`) as `planned`. A move batch has the id prefix [`MOVE_BATCH_PREFIX`],
//! which is how start-up recovery tells it from a rename batch. For each title:
//!
//! | Move | Rows, in order |
//! |---|---|
//! | same volume | one `move` of the folder |
//! | another volume | a `copy` per file (original to its place in the staging folder), one `move` of the folder (the commit), a `trash` per file (original to the trash folder) |
//!
//! # Cross-volume steps
//!
//! 1. Each file is read once, hashed while it is copied to `<name>.cvpart` in a **hidden staging
//!    folder** next to the destination (`.cvmove-<batch>-<id>`, which scans skip, so a half-copied
//!    title is never seen by a scan or a media server), flushed to the disk, read back and
//!    compared by size and hash with the original, renamed to its final name and marked `done`.
//!    The copy is a pipeline: a reader thread reads the original in 8 MiB chunks and hashes them
//!    while the engine writes the chunk before (a bounded channel of a few buffers between them,
//!    so memory stays small); files still go one at a time. The copy is flushed with `sync_all`
//!    before it is read back, and the read-back bypasses the system's file cache where the
//!    system allows ([`super::uncached`]), so the check compares what the disk holds.
//! 2. When every file is verified, the staging folder is renamed to the destination folder (one
//!    atomic step), and the commit row is completed. The journal store re-points the library in
//!    that same transaction: `item.path`, `media_file.directory` and the data source of every
//!    row below the folder. Rows are re-pointed, never deleted and added again.
//! 3. Only then do the originals go, as Settings › Deleted files says through the engine's bin
//!    ([`super::PolicyBin`]: the system bin, the trash folder, a custom trash folder, or erased
//!    when the person chose Delete permanently): to the Recycle Bin / Trash when the engine has a
//!    [`SystemBin`], each `trash` row then recording the bin's id of
//!    its file, else (and for what the bin cannot take, such as a network share) file by file to
//!    `<source root>/.deletedByCineVault/<batch>/...`. They are never deleted. Empty folders the
//!    move leaves behind are removed.
//!
//! Until step 2 the originals are untouched and the library points at them. After step 2 the
//! library points at verified copies and the originals are in the trash folder.
//!
//! # Stops
//!
//! * A copy that does not match its original removes the copy, takes the title back to how it
//!   was (rolls it back) and stops the batch.
//! * Cancel before step 2 is the same roll back; cancel after it lets the title finish (it is
//!   then wholly in the new place) and stops the batch.
//! * Disk full, access denied, a file in use or a missing original stop the batch **without
//!   removing anything**. The row that failed says `stopped: <reason>`; the rest stay `planned`.
//!   The batch is then unfinished and can be resumed or rolled back (see [`super::recovery`]).
//!
//! # Undo and roll back
//!
//! [`MoveEngine::undo`] takes a finished move back: the originals return from the Recycle Bin or
//! the trash folder, the library is re-pointed to them, and the copies go to the Recycle Bin (or
//! `<destination root>/.deletedByCineVault/<batch>-undo/<title folder>/`). When an original is no
//! longer in the bin, the title is reported and the caller copies it back from its new place
//! instead (see `ChangeSourceService::undo_held`).
//! Every file is checked first and a title that cannot be restored whole is left as it is and
//! reported. A roll back ([`MoveEngine::roll_back`]) is the same for an unfinished batch and
//! removes only what the batch itself made: `.cvpart` files, copies and the staging folder.

use std::collections::BTreeSet;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant, UNIX_EPOCH};

use cv_domain::ItemId;
use cv_rename::exec::{
    Completion, Fs, JournalEntry, JournalError, JournalOp, JournalRow, JournalState, JournalStore,
    StdFs, TRASH_DIR, new_batch_id,
};
use sha2::{Digest, Sha256};

use super::move_plan::{EntryKind, MoveEntry, enumerate};
use super::trash_bin::{BinnedItem, SystemBin};

/// Where the engine keeps the bin id of each original it sent to the Recycle Bin / Trash: the
/// `bin_id` of its journal row.
pub trait BinLedger: Send + Sync {
    /// Records the bin id of the `trash` row `seq` of `batch_id`.
    ///
    /// # Errors
    /// The journal cannot be written; the reason in words.
    fn record(&self, batch_id: &str, seq: u32, bin_id: &str) -> Result<(), String>;

    /// The bin ids of a batch's rows, by `seq`.
    fn lookup(&self, batch_id: &str) -> std::collections::BTreeMap<u32, String>;
}

/// The ledger in the library's journal table.
pub struct DbBinLedger(pub Arc<cv_db::Database>);

impl BinLedger for DbBinLedger {
    fn record(&self, batch_id: &str, seq: u32, bin_id: &str) -> Result<(), String> {
        let (batch, bin) = (batch_id.to_owned(), bin_id.to_owned());
        self.0
            .write(move |tx| {
                cv_db::repo::rename_journal::set_bin_id(tx, &batch, i64::from(seq), &bin)
            })
            .map_err(|e| e.to_string())
    }

    fn lookup(&self, batch_id: &str) -> std::collections::BTreeMap<u32, String> {
        let batch = batch_id.to_owned();
        self.0
            .read(move |conn| cv_db::repo::rename_journal::binned_rows(conn, &batch))
            .unwrap_or_default()
            .into_iter()
            .filter_map(|row| Some((u32::try_from(row.seq).ok()?, row.bin_id)))
            .collect()
    }
}

/// The id prefix of a move batch in the journal.
pub const MOVE_BATCH_PREFIX: &str = "move-";

/// What a journal row's `error` starts with when an error stopped the move without removing
/// anything: the row is retried by a resume.
pub const STOPPED_PREFIX: &str = "stopped: ";

/// The suffix of a file that is being copied.
pub const PART_SUFFIX: &str = ".cvpart";

/// The staging folder of a title is a hidden folder with this name start.
const STAGE_PREFIX: &str = ".cvmove-";

pub const CHUNK_BYTES: usize = 8 * 1024 * 1024;

/// Chunks read ahead of the writer: the reader may be this many buffers ahead.
const PIPELINE_DEPTH: usize = 3;

/// The speed is measured over this much recent time.
const SPEED_WINDOW: Duration = Duration::from_secs(6);

/// How often the progress callback is called while a big file is copied.
const PROGRESS_EVERY: Duration = Duration::from_millis(120);

/// A fresh id for a move batch.
#[must_use]
pub fn new_move_batch_id() -> String {
    format!("{MOVE_BATCH_PREFIX}{}", new_batch_id())
}

/// Whether a journal batch is a move between data sources.
#[must_use]
pub fn is_move_batch(batch_id: &str) -> bool {
    batch_id.starts_with(MOVE_BATCH_PREFIX)
}

/// The id prefix of a copy back: a move that is part of an undo and has no History entry of its
/// own. It is a move batch too.
const MOVE_BACK_PREFIX: &str = "move-back-";

/// A fresh id for a copy back (see [`MOVE_BACK_PREFIX`]).
pub(crate) fn new_move_back_batch_id() -> String {
    format!("{MOVE_BACK_PREFIX}{}", new_batch_id())
}

/// Whether a move batch has a History entry: every move but a copy back.
pub(crate) fn wants_history(batch_id: &str) -> bool {
    !batch_id.starts_with(MOVE_BACK_PREFIX)
}

// ---- batches at work --------------------------------------------------------------------------

/// The move batches an engine of this process is working on now. Their journal rows are
/// `planned` while they run, which is what an unfinished batch looks like too; recovery must
/// not offer them.
static RUNNING: LazyLock<Mutex<BTreeSet<String>>> = LazyLock::new(|| Mutex::new(BTreeSet::new()));

fn running() -> std::sync::MutexGuard<'static, BTreeSet<String>> {
    RUNNING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Whether the batch is being moved, resumed, rolled back or undone right now.
#[must_use]
pub fn is_running(batch_id: &str) -> bool {
    running().contains(batch_id)
}

/// Marks a batch as at work until dropped.
struct AtWork(String);

impl AtWork {
    fn new(batch_id: &str) -> Self {
        running().insert(batch_id.to_owned());
        Self(batch_id.to_owned())
    }
}

impl Drop for AtWork {
    fn drop(&mut self) {
        running().remove(&self.0);
    }
}

// ---- transient errors ---------------------------------------------------------------------------

/// An error that is likely gone in a moment: a Windows virus scanner, the indexer or Explorer
/// holding a just-written file or folder (access denied, a sharing or lock violation), a busy
/// file elsewhere. The same rule as every other file write of the app (`cv_fs`).
fn transient(error: &io::Error) -> bool {
    cv_fs::is_transient(error) || error.kind() == io::ErrorKind::ResourceBusy
}

/// The two folders are on different volumes after all (a mount point or a junction on the way).
fn crosses_devices(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::CrossesDevices
        || (cfg!(windows) && error.raw_os_error() == Some(17))
        || (cfg!(unix) && error.raw_os_error() == Some(18))
}

// ---- control ---------------------------------------------------------------------------------

/// Pause and cancel of a running move, shared with the code that shows the Pause and Cancel
/// buttons.
#[derive(Debug, Default)]
pub struct MoveControl {
    cancel: AtomicBool,
    paused: AtomicBool,
}

impl MoveControl {
    #[must_use]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Stops the move at the next chunk and takes the title in hand back.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    /// Holds the copy between two chunks until [`Self::resume`].
    pub fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
    }

    pub fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    #[must_use]
    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }
}

// ---- the disk ----------------------------------------------------------------------------------

/// The file system the engine works on: the executor's [`Fs`] plus the streams a chunked copy
/// needs. A test wraps the real one to fail on purpose.
pub trait MoveFs: Fs + Send + Sync {
    fn open_read(&self, path: &Path) -> io::Result<Box<dyn Read + Send>>;
    /// Creates a new file; fails when one exists.
    fn create_write(&self, path: &Path) -> io::Result<Box<dyn CopyWrite>>;
    /// Gives the finished copy `to` the read-only flag of the original `from`.
    fn keep_read_only(&self, from: &Path, to: &Path) -> io::Result<()> {
        let _ = (from, to);
        Ok(())
    }
    /// Opens a finished copy to read it back for the check: from the disk itself where the
    /// system allows, not from its cache. A plain read by default.
    fn open_verify(&self, path: &Path) -> io::Result<Box<dyn Read + Send>> {
        self.open_read(path)
    }
    /// The bytes read and written at a time.
    fn chunk_bytes(&self) -> usize {
        CHUNK_BYTES
    }
}

/// The destination of a copy.
pub trait CopyWrite: Write + Send {
    /// Flushes the file to the disk and gives it the modification time `modified_ms`.
    fn finish(self: Box<Self>, modified_ms: i64) -> io::Result<()>;
}

/// The real disk, with the long-path handling of [`StdFs`].
#[derive(Debug, Clone, Copy, Default)]
pub struct StdMoveFs;

impl Fs for StdMoveFs {
    fn meta(&self, path: &Path) -> io::Result<Option<cv_rename::exec::Meta>> {
        StdFs.meta(path)
    }
    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        StdFs.create_dir_all(path)
    }
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        StdFs.rename(from, to)
    }
    fn copy_file(&self, from: &Path, to: &Path) -> io::Result<()> {
        StdFs.copy_file(from, to)
    }
    fn same_content(&self, a: &Path, b: &Path) -> io::Result<bool> {
        StdFs.same_content(a, b)
    }
    fn remove_file(&self, path: &Path) -> io::Result<()> {
        StdFs.remove_file(path)
    }
    fn remove_dir(&self, path: &Path) -> io::Result<()> {
        StdFs.remove_dir(path)
    }
    fn list_dir(&self, path: &Path) -> io::Result<Vec<String>> {
        StdFs.list_dir(path)
    }
}

struct StdWrite(std::fs::File);

impl Write for StdWrite {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

impl CopyWrite for StdWrite {
    fn finish(self: Box<Self>, modified_ms: i64) -> io::Result<()> {
        let mut file = self.0;
        file.flush()?;
        file.sync_all()?;
        let time = if modified_ms >= 0 {
            UNIX_EPOCH + Duration::from_millis(u64::try_from(modified_ms).unwrap_or(0))
        } else {
            UNIX_EPOCH
        };
        file.set_modified(time)
    }
}

impl MoveFs for StdMoveFs {
    fn open_read(&self, path: &Path) -> io::Result<Box<dyn Read + Send>> {
        Ok(Box::new(std::fs::File::open(StdFs::native(path))?))
    }

    fn create_write(&self, path: &Path) -> io::Result<Box<dyn CopyWrite>> {
        let file = std::fs::File::options()
            .write(true)
            .create_new(true)
            .open(StdFs::native(path))?;
        Ok(Box::new(StdWrite(file)))
    }

    fn open_verify(&self, path: &Path) -> io::Result<Box<dyn Read + Send>> {
        super::uncached::open_uncached(&StdFs::native(path), CHUNK_BYTES)
    }

    fn keep_read_only(&self, from: &Path, to: &Path) -> io::Result<()> {
        if std::fs::metadata(StdFs::native(from))?
            .permissions()
            .readonly()
        {
            let to = StdFs::native(to);
            let mut perms = std::fs::metadata(&to)?.permissions();
            perms.set_readonly(true);
            std::fs::set_permissions(&to, perms)?;
        }
        Ok(())
    }
}

// ---- jobs and results ---------------------------------------------------------------------------

/// One title to move.
#[derive(Debug, Clone)]
pub struct MoveJob {
    pub item: ItemId,
    pub title: String,
    pub from_root: PathBuf,
    /// The title folder now.
    pub from: PathBuf,
    pub to_root: PathBuf,
    /// The title folder after the move. It does not exist.
    pub to: PathBuf,
    /// Everything in the title folder, as the preview listed it.
    pub entries: Vec<MoveEntry>,
    /// Copy and verify instead of renaming.
    pub across_volumes: bool,
}

/// What the move is doing now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovePhase {
    /// Copying a file.
    Copying,
    /// Reading a copy back to compare it.
    Verifying,
    /// Putting the title in place and moving the originals to the trash folder.
    Finishing,
}

/// Progress of a move: the titles done and, inside the copy, the bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveProgress {
    /// Titles finished.
    pub done: usize,
    pub total: usize,
    /// The title being moved, or the one that was just finished.
    pub title: String,
    /// Bytes copied and to copy in all, across the titles of this run.
    pub bytes_done: u64,
    pub bytes_total: u64,
    /// The file being copied, relative to the title folder; empty between files.
    pub current_file: String,
    pub phase: MovePhase,
    /// Bytes read or written per second lately (copying, then reading the copy back); 0 until
    /// it is known.
    pub bytes_per_second: u64,
    /// The time the rest of the move needs at that speed, copy and check; `None` until known.
    pub seconds_left: Option<u64>,
}

/// How one title ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobOutcome {
    /// The title is in the new place. The notes tell what could not be tidied up.
    Moved { notes: Vec<String> },
    /// A copy failed its check or another rule was broken. The title was taken back whole.
    Failed(String),
    /// The move stopped on an error and removed nothing; it can be resumed or rolled back.
    Stopped(String),
    /// Cancelled; the title is where it was.
    Canceled,
    /// Not started because an earlier title stopped the batch.
    NotStarted,
}

/// The result of one title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobResult {
    pub item: ItemId,
    pub outcome: JobOutcome,
}

/// Why the engine could not go on at all.
#[derive(Debug, thiserror::Error)]
pub enum MoveError {
    #[error(transparent)]
    Journal(#[from] JournalError),
    /// The request is wrong in a way the plan should have caught.
    #[error("{0}")]
    Invalid(String),
}

/// What an undo or a roll back did to one title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TakenBack {
    pub item: Option<String>,
    pub status: TakeBackStatus,
    /// Originals put back where they were.
    pub restored: usize,
    /// Copies removed or sent to the trash folder.
    pub discarded: usize,
    pub notes: Vec<String>,
}

/// How one title went back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TakeBackStatus {
    Restored,
    /// Nothing was touched: a file could not be checked. Each entry is a path and the reason.
    NotRestorable(Vec<(String, String)>),
    /// The disk failed in the middle; the text says why.
    Failed(String),
}

// ---- rows ---------------------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Step {
    entry: JournalEntry,
    state: JournalState,
    size: Option<u64>,
    modified: Option<i64>,
    error: String,
}

impl Step {
    fn fresh(entry: JournalEntry) -> Self {
        Self {
            entry,
            state: JournalState::Planned,
            size: None,
            modified: None,
            error: String::new(),
        }
    }

    fn of(row: JournalRow) -> Self {
        Self {
            entry: row.entry,
            state: row.state,
            size: row.size_bytes,
            modified: row.modified_ms,
            error: row.error,
        }
    }

    fn done(&self) -> bool {
        self.state == JournalState::Done
    }

    /// The error of a stopped row, without the marker.
    fn stopped(&self) -> Option<&str> {
        if self.state == JournalState::Failed {
            self.error.strip_prefix(STOPPED_PREFIX)
        } else {
            None
        }
    }

    /// A row that a resume would run: not yet done, not taken back, and not a finished failure.
    fn open(&self) -> bool {
        match self.state {
            JournalState::Planned => true,
            JournalState::Failed => self.stopped().is_some(),
            JournalState::Done | JournalState::Undone => false,
        }
    }
}

/// The rows of one title.
#[derive(Debug, Clone, Default)]
pub(crate) struct ItemRows {
    pub(crate) item: Option<String>,
    copies: Vec<Step>,
    commit: Option<Step>,
    trashes: Vec<Step>,
}

impl ItemRows {
    fn across(&self) -> bool {
        !self.copies.is_empty()
    }

    fn all(&self) -> impl Iterator<Item = &Step> {
        self.copies
            .iter()
            .chain(self.commit.iter())
            .chain(self.trashes.iter())
    }

    pub(crate) fn committed(&self) -> bool {
        self.commit.as_ref().is_some_and(Step::done)
    }

    /// The title still has work: a row is planned or was stopped.
    pub(crate) fn unfinished(&self) -> bool {
        self.all().any(Step::open)
    }

    /// Files to copy in all, and how many are copied and verified.
    pub(crate) fn files(&self) -> (usize, usize) {
        let done = self.copies.iter().filter(|s| s.done()).count();
        (done, self.copies.len())
    }

    /// The last reason a row stopped with.
    pub(crate) fn stop_reason(&self) -> Option<String> {
        self.all()
            .filter_map(|s| s.stopped().map(str::to_owned))
            .last()
    }

    pub(crate) fn old_folder(&self) -> Option<&str> {
        self.commit.as_ref().map(|s| s.entry.from.as_str())
    }

    pub(crate) fn new_folder(&self) -> Option<&str> {
        self.commit.as_ref().map(|s| s.entry.to.as_str())
    }

    /// The copy row of the original at `from`.
    fn copy_of(&self, from: &str) -> Option<&Step> {
        self.copies.iter().find(|c| c.entry.from == from)
    }

    /// The trash row of the original at `from`.
    fn trash_of(&self, from: &str) -> Option<&Step> {
        self.trashes.iter().find(|t| t.entry.from == from)
    }
}

/// Groups the rows of a batch by title, in the order the batch reaches them.
pub(crate) fn group_rows(rows: Vec<JournalRow>) -> Vec<ItemRows> {
    let mut out: Vec<ItemRows> = Vec::new();
    for row in rows {
        let mut step = Step::of(row);
        let index = match out.iter().position(|i| i.item == step.entry.item_id) {
            Some(index) => index,
            None => {
                out.push(ItemRows {
                    item: step.entry.item_id.clone(),
                    ..ItemRows::default()
                });
                out.len() - 1
            }
        };
        let item = &mut out[index];
        match step.entry.op {
            JournalOp::Copy => item.copies.push(step),
            JournalOp::Move => {
                // Folders only; the table does not remember that.
                step.entry.is_dir = true;
                item.commit = Some(step);
            }
            JournalOp::Trash => item.trashes.push(step),
            JournalOp::WriteNfo => {}
        }
    }
    out
}

/// The titles (item ids as the journal keeps them) that have work left in a move batch: a move
/// that stopped, was cut short or runs now. Such a title is not moved again until that batch is
/// resumed or rolled back.
///
/// # Errors
/// The journal cannot be read.
pub(crate) fn titles_with_unfinished_moves(
    journal: &dyn JournalStore,
) -> Result<BTreeSet<String>, JournalError> {
    let mut out = BTreeSet::new();
    for summary in journal.batches()? {
        if !is_move_batch(&summary.batch_id) || (summary.planned == 0 && summary.failed == 0) {
            continue;
        }
        for item in group_rows(journal.batch(&summary.batch_id)?) {
            if item.unfinished()
                && let Some(id) = item.item
            {
                out.insert(id);
            }
        }
    }
    Ok(out)
}

/// Closes the rows of one title of a finished move that was taken back by moving the title
/// again (see `ChangeSourceService::move_back`): what was done is `undone` now, without a
/// file being touched; the library was pointed back by the new move.
///
/// # Errors
/// The journal cannot be read or written.
pub(crate) fn close_moved_back(
    journal: &mut dyn JournalStore,
    batch_id: &str,
    item: &str,
) -> Result<(), JournalError> {
    let rows = group_rows(journal.batch(batch_id)?);
    let Some(rows) = rows.into_iter().find(|i| i.item.as_deref() == Some(item)) else {
        return Ok(());
    };
    for step in rows.all() {
        if step.done() {
            journal.complete(&step.entry, &Completion::Undone)?;
        } else if step.open() {
            journal.complete(&step.entry, &Completion::Failed("rolled back".to_owned()))?;
        }
    }
    Ok(())
}

/// Whether every problem an undo found is an original missing or changed in the trash folder:
/// the title is whole in its new place and can be moved back from there.
pub(crate) fn only_trash_problems(problems: &[(String, String)]) -> bool {
    !problems.is_empty()
        && problems
            .iter()
            .all(|(_, why)| why == TRASH_GONE || why == TRASH_CHANGED)
}

/// The rows of a title that was just journaled.
fn group_steps(steps: Vec<Step>) -> ItemRows {
    let mut item = ItemRows::default();
    for mut step in steps {
        item.item.clone_from(&step.entry.item_id);
        match step.entry.op {
            JournalOp::Copy => item.copies.push(step),
            JournalOp::Move => {
                step.entry.is_dir = true;
                item.commit = Some(step);
            }
            JournalOp::Trash => item.trashes.push(step),
            JournalOp::WriteNfo => {}
        }
    }
    item
}

fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// The name of a file as a person knows it: a half-written `.cvpart` copy is named after the
/// file it will become.
fn name_of(path: &Path) -> String {
    let name = path
        .file_name()
        .map_or_else(|| text(path), |n| n.to_string_lossy().into_owned());
    name.strip_suffix(PART_SUFFIX)
        .map_or(name.clone(), str::to_owned)
}

/// The outcome of a title that is whole in its new place. While an original is still in the old
/// folder (in use, or not allowed to go to the trash folder) the move is not finished: it is
/// stopped with the reason, its rows stay open and Activity offers Resume and Roll back at once,
/// and the next scan leaves the old folder alone.
fn placed(rows: &ItemRows, notes: Vec<String>) -> JobOutcome {
    match rows.trashes.iter().find(|s| s.open()) {
        None => JobOutcome::Moved { notes },
        Some(step) => JobOutcome::Stopped(format!(
            "{} is still in the old folder: {}. The title is whole in its new place; resume the \
             move in Activity to finish it, or roll it back",
            name_of(Path::new(&step.entry.from)),
            step.stopped()
                .unwrap_or("it could not go to the trash folder")
        )),
    }
}

/// The hidden staging folder of a title, next to its destination.
#[must_use]
pub fn stage_dir(destination: &Path, batch_id: &str, item: Option<&str>) -> PathBuf {
    let batch = batch_id.strip_prefix(MOVE_BATCH_PREFIX).unwrap_or(batch_id);
    let name = format!("{STAGE_PREFIX}{batch}-{}", item.unwrap_or("0"));
    destination
        .parent()
        .map_or_else(|| PathBuf::from(&name), |parent| parent.join(&name))
}

fn part_of(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(PART_SUFFIX);
    path.with_file_name(name)
}

/// What a failed step is: stop and keep everything, or take the title back.
#[derive(Debug)]
enum Fail {
    Journal(JournalError),
    Canceled,
    /// A copy does not match; the title is taken back.
    Mismatch(String),
    /// An error that removes nothing; the text is the plain reason.
    Stop(String),
    /// A folder rename was refused because the two places are on different volumes.
    CrossesDevices,
}

impl From<JournalError> for Fail {
    fn from(error: JournalError) -> Self {
        Fail::Journal(error)
    }
}

/// A plain sentence for an I/O error on `path`.
fn reason_of(error: &io::Error, path: &Path, writing: bool) -> String {
    let name = name_of(path);
    let raw = error.raw_os_error();
    let full = error.kind() == io::ErrorKind::StorageFull
        || matches!(raw, Some(112 | 39 | 28 | 122 | 31 | 1224 | 63));
    if full && writing {
        return "Destination is full".to_owned();
    }
    // ERROR_SHARING_VIOLATION and ERROR_LOCK_VIOLATION on Windows, EBUSY and ETXTBSY elsewhere.
    if matches!(raw, Some(32 | 33 | 16 | 26)) || error.kind() == io::ErrorKind::ResourceBusy {
        return format!("{name} is in use by another program");
    }
    if error.kind() == io::ErrorKind::PermissionDenied {
        return if writing {
            format!("CineVault is not allowed to write {name}")
        } else {
            format!("CineVault is not allowed to read {name}")
        };
    }
    if error.kind() == io::ErrorKind::NotFound {
        return format!("{name} is missing");
    }
    format!("{name}: {error}")
}

// ---- the engine ----------------------------------------------------------------------------------

/// Runs moves, resumes, rolls back and undoes them over a journal and a disk.
pub struct MoveEngine<'a> {
    fs: &'a dyn MoveFs,
    journal: &'a mut dyn JournalStore,
    control: Option<Arc<MoveControl>>,
    /// The data source roots, to find the trash folder of the destination for an undo.
    roots: Vec<PathBuf>,
    bytes_done: u64,
    bytes_total: u64,
    done_titles: usize,
    total_titles: usize,
    title: String,
    last_progress: Option<Instant>,
    /// The Recycle Bin / Trash for the originals, with where their ids are kept; `None` uses
    /// the trash folder only.
    bin: Option<(Arc<dyn SystemBin>, Arc<dyn BinLedger>)>,
    /// Bytes copied plus bytes read back so far: the work done, for the speed and time left.
    work_done: u64,
    /// Recent `(time, work_done)` samples.
    samples: std::collections::VecDeque<(Instant, u64)>,
}

type ProgressFn<'p> = &'p mut dyn FnMut(&MoveProgress);

impl<'a> MoveEngine<'a> {
    pub fn new(fs: &'a dyn MoveFs, journal: &'a mut dyn JournalStore) -> Self {
        Self {
            fs,
            journal,
            control: None,
            roots: Vec::new(),
            bytes_done: 0,
            bytes_total: 0,
            done_titles: 0,
            total_titles: 0,
            title: String::new(),
            last_progress: None,
            bin: None,
            work_done: 0,
            samples: std::collections::VecDeque::new(),
        }
    }

    /// Sends the originals to the Recycle Bin / Trash (and takes them back from there), keeping
    /// each one's bin id in `ledger`.
    #[must_use]
    pub fn with_bin(mut self, bin: Arc<dyn SystemBin>, ledger: Arc<dyn BinLedger>) -> Self {
        self.bin = Some((bin, ledger));
        self
    }

    /// Pause and cancel.
    #[must_use]
    pub fn with_control(mut self, control: Arc<MoveControl>) -> Self {
        self.control = Some(control);
        self
    }

    /// The roots of the data sources (needed by undo, to find the destination's trash folder).
    #[must_use]
    pub fn with_roots(mut self, roots: Vec<PathBuf>) -> Self {
        self.roots = roots;
        self
    }

    // ---- running a batch ------------------------------------------------------------------

    /// Journals the whole batch, then moves the titles one after the other.
    ///
    /// # Errors
    /// Only a journal that cannot be written, which stops everything at once; titles that fail
    /// are in the results.
    pub fn run(
        &mut self,
        batch_id: &str,
        jobs: &[MoveJob],
        progress: ProgressFn<'_>,
    ) -> Result<Vec<JobResult>, MoveError> {
        let _at_work = AtWork::new(batch_id);
        let mut seq = 0_u32;
        let mut all: Vec<Vec<Step>> = Vec::with_capacity(jobs.len());
        for job in jobs {
            let entries = self.rows_for(batch_id, &mut seq, job)?;
            all.push(entries.into_iter().map(Step::fresh).collect());
        }
        // The whole batch is on record before any file changes, in one write: a kill while it
        // is written leaves all of it or none.
        let entries: Vec<JournalEntry> = all.iter().flatten().map(|s| s.entry.clone()).collect();
        self.journal.begin_all(&entries)?;

        self.total_titles = jobs.len();
        self.done_titles = 0;
        self.bytes_done = 0;
        self.bytes_total = jobs
            .iter()
            .filter(|j| j.across_volumes)
            .flat_map(|j| j.entries.iter())
            .filter(|e| e.is_file())
            .map(|e| e.size)
            .sum();

        let mut results = Vec::with_capacity(jobs.len());
        let mut stop: Option<bool> = None; // Some(cancelled?)
        for (job, steps) in jobs.iter().zip(all) {
            let item = job.item;
            if let Some(cancelled) = stop {
                if cancelled {
                    self.abandon(&steps, "canceled before it started")?;
                    results.push(JobResult {
                        item,
                        outcome: JobOutcome::Canceled,
                    });
                } else {
                    results.push(JobResult {
                        item,
                        outcome: JobOutcome::NotStarted,
                    });
                }
                continue;
            }
            self.title.clone_from(&job.title);
            let mut rows = group_steps(steps);
            let outcome = self.run_job(job, &mut rows, progress)?;
            match &outcome {
                JobOutcome::Moved { .. } => self.done_titles += 1,
                // Whole in its new place with an original still in the old folder: the next
                // titles go on; Resume finishes this one.
                JobOutcome::Stopped(_) if rows.committed() => {}
                JobOutcome::Canceled => stop = Some(true),
                JobOutcome::Failed(_) | JobOutcome::Stopped(_) | JobOutcome::NotStarted => {
                    stop = Some(false);
                }
            }
            if matches!(outcome, JobOutcome::Moved { .. }) && self.cancelled() {
                // Cancelled after this title was whole: the next ones do not start.
                stop = Some(true);
            }
            self.report(progress, "", MovePhase::Finishing);
            results.push(JobResult { item, outcome });
        }
        Ok(results)
    }

    /// The rows of one title.
    fn rows_for(
        &self,
        batch_id: &str,
        seq: &mut u32,
        job: &MoveJob,
    ) -> Result<Vec<JournalEntry>, MoveError> {
        let item = Some(job.item.0.to_string());
        let mut next =
            |op: JournalOp, from: &Path, to: &Path, is_dir: bool, parents: Option<u32>| {
                *seq += 1;
                JournalEntry {
                    batch_id: batch_id.to_owned(),
                    seq: *seq,
                    item_id: item.clone(),
                    op,
                    from: text(from),
                    to: text(to),
                    is_dir,
                    created_parents: parents,
                }
            };
        let parents = Some(self.missing_parents(&job.to));
        if !job.across_volumes {
            return Ok(vec![next(
                JournalOp::Move,
                &job.from,
                &job.to,
                true,
                parents,
            )]);
        }
        let stage = stage_dir(&job.to, batch_id, item.as_deref());
        let rel_root = job.from.strip_prefix(&job.from_root).map_err(|_| {
            MoveError::Invalid(format!(
                "{} is not inside {}",
                job.from.display(),
                job.from_root.display()
            ))
        })?;
        let trash = job.from_root.join(TRASH_DIR).join(batch_id).join(rel_root);
        let files: Vec<&MoveEntry> = job.entries.iter().filter(|e| e.is_file()).collect();
        let mut out = Vec::with_capacity(files.len() * 2 + 1);
        for entry in &files {
            out.push(next(
                JournalOp::Copy,
                &job.from.join(&entry.path),
                &stage.join(&entry.path),
                false,
                None,
            ));
        }
        out.push(next(JournalOp::Move, &job.from, &job.to, true, parents));
        for entry in &files {
            out.push(next(
                JournalOp::Trash,
                &job.from.join(&entry.path),
                &trash.join(&entry.path),
                false,
                None,
            ));
        }
        Ok(out)
    }

    /// How many folders above `path` do not exist yet.
    fn missing_parents(&self, path: &Path) -> u32 {
        let mut count = 0;
        let mut dir = path.parent();
        while let Some(current) = dir {
            if current.as_os_str().is_empty() {
                break;
            }
            match self.fs.meta(current) {
                Ok(Some(_)) => break,
                _ => count += 1,
            }
            dir = current.parent();
        }
        count
    }

    fn abandon(&mut self, steps: &[Step], why: &str) -> Result<(), MoveError> {
        for step in steps {
            self.journal
                .complete(&step.entry, &Completion::Failed(why.to_owned()))?;
        }
        Ok(())
    }

    fn run_job(
        &mut self,
        job: &MoveJob,
        rows: &mut ItemRows,
        progress: ProgressFn<'_>,
    ) -> Result<JobOutcome, MoveError> {
        if rows.across() {
            return self.drive(rows, Some(job), false, progress);
        }
        self.rename_item(rows, progress)
    }

    /// A same-volume move: one rename of the folder.
    fn rename_item(
        &mut self,
        rows: &mut ItemRows,
        progress: ProgressFn<'_>,
    ) -> Result<JobOutcome, MoveError> {
        let Some(commit) = rows.commit.clone() else {
            return Ok(JobOutcome::Failed(
                "The move has no folder to rename.".to_owned(),
            ));
        };
        let (from, to) = (
            PathBuf::from(&commit.entry.from),
            PathBuf::from(&commit.entry.to),
        );
        match self.rename_folder(&commit, &from, &to) {
            Ok(()) => Ok(JobOutcome::Moved { notes: Vec::new() }),
            Err(Fail::CrossesDevices) => self.copy_instead(rows, &commit, &from, progress),
            Err(Fail::Journal(error)) => Err(error.into()),
            Err(Fail::Stop(reason)) => {
                self.journal.complete(
                    &commit.entry,
                    &Completion::Failed(format!("{STOPPED_PREFIX}{reason}")),
                )?;
                Ok(JobOutcome::Stopped(reason))
            }
            Err(Fail::Canceled) => Ok(JobOutcome::Canceled),
            Err(Fail::Mismatch(reason)) => {
                self.journal
                    .complete(&commit.entry, &Completion::Failed(reason.clone()))?;
                Ok(JobOutcome::Failed(reason))
            }
        }
    }

    /// The folder rename of a same-volume move. If the system says the folders are on different
    /// volumes after all, the move stops with a reason and nothing is changed.
    fn rename_folder(&mut self, commit: &Step, from: &Path, to: &Path) -> Result<(), Fail> {
        let src = self.meta(from)?;
        let dst = self.meta(to)?;
        if src.is_none() && dst.is_some_and(|m| m.is_dir) {
            // A run that stopped after the rename.
            return self.complete_commit(commit);
        }
        if src.is_none() {
            return Err(Fail::Stop(format!(
                "The folder {} is missing",
                from.display()
            )));
        }
        if dst.is_some() {
            return Err(Fail::Stop(format!(
                "{} exists already; nothing was replaced",
                to.display()
            )));
        }
        if let Some(parent) = to.parent() {
            self.fs
                .create_dir_all(parent)
                .map_err(|e| Fail::Stop(reason_of(&e, parent, true)))?;
        }
        match self.rename(from, to) {
            Ok(()) => self.complete_commit(commit),
            Err(e) if crosses_devices(&e) => Err(Fail::CrossesDevices),
            Err(e) => Err(Fail::Stop(reason_of(&e, from, true))),
        }
    }

    /// A same-volume move whose folder rename the system refused because the two places are on
    /// different volumes after all (a mount point or a junction on the way): the title is
    /// copied and verified file by file instead. The copy and trash rows are journaled before
    /// the first file is copied, as for a move planned as a copy.
    fn copy_instead(
        &mut self,
        rows: &mut ItemRows,
        commit: &Step,
        from: &Path,
        progress: ProgressFn<'_>,
    ) -> Result<JobOutcome, MoveError> {
        let entries = match enumerate(from) {
            Ok(entries) => entries,
            Err(why) => {
                self.journal.complete(
                    &commit.entry,
                    &Completion::Failed(format!("{STOPPED_PREFIX}{why}")),
                )?;
                return Ok(JobOutcome::Stopped(why));
            }
        };
        if let Some(link) = entries
            .iter()
            .find(|e| matches!(e.kind, EntryKind::Link { .. }))
        {
            let why = format!(
                "The folders are on different drives, and the folder contains a link ({}). \
                 Links are not followed or copied to another drive, so move this title by hand",
                link.path
            );
            self.journal
                .complete(&commit.entry, &Completion::Failed(why.clone()))?;
            return Ok(JobOutcome::Failed(why));
        }
        match self.append_files(rows, from, &entries) {
            Ok(()) => {}
            Err(Fail::Journal(error)) => return Err(error.into()),
            Err(other) => {
                let why = match other {
                    Fail::Stop(why) | Fail::Mismatch(why) => why,
                    _ => "The move could not be prepared".to_owned(),
                };
                self.journal.complete(
                    &commit.entry,
                    &Completion::Failed(format!("{STOPPED_PREFIX}{why}")),
                )?;
                return Ok(JobOutcome::Stopped(why));
            }
        }
        self.bytes_total += entries
            .iter()
            .filter(|e| e.is_file())
            .map(|e| e.size)
            .sum::<u64>();
        self.drive(rows, None, false, progress)
    }

    /// Journals a copy and a trash row for each file of `entries` (below the title folder
    /// `from`) that the title's rows do not have yet, all in one write, and adds them to `rows`.
    fn append_files(
        &mut self,
        rows: &mut ItemRows,
        from: &Path,
        entries: &[MoveEntry],
    ) -> Result<(), Fail> {
        let commit = rows
            .commit
            .clone()
            .ok_or_else(|| Fail::Stop("The move has no folder step".to_owned()))?;
        let batch = commit.entry.batch_id.clone();
        let to = PathBuf::from(&commit.entry.to);
        let stage = stage_dir(&to, &batch, rows.item.as_deref());
        let root = self.root_of(from).ok_or_else(|| {
            Fail::Stop(format!(
                "{} is not in a data source CineVault knows",
                from.display()
            ))
        })?;
        let rel_root = from.strip_prefix(&root).map_err(|_| {
            Fail::Stop(format!(
                "{} is not inside {}",
                from.display(),
                root.display()
            ))
        })?;
        let trash = root.join(TRASH_DIR).join(&batch).join(rel_root);
        let new: Vec<&MoveEntry> = entries
            .iter()
            .filter(|e| e.is_file() && rows.copy_of(&text(&from.join(&e.path))).is_none())
            .collect();
        if new.is_empty() {
            return Ok(());
        }
        let mut seq = self
            .journal
            .batch(&batch)?
            .iter()
            .map(|r| r.entry.seq)
            .max()
            .unwrap_or(0);
        let mut next = |op: JournalOp, a: &Path, b: &Path| {
            seq += 1;
            JournalEntry {
                batch_id: batch.clone(),
                seq,
                item_id: rows.item.clone(),
                op,
                from: text(a),
                to: text(b),
                is_dir: false,
                created_parents: None,
            }
        };
        let mut journaled = Vec::with_capacity(new.len() * 2);
        for entry in &new {
            journaled.push(next(
                JournalOp::Copy,
                &from.join(&entry.path),
                &stage.join(&entry.path),
            ));
        }
        for entry in &new {
            journaled.push(next(
                JournalOp::Trash,
                &from.join(&entry.path),
                &trash.join(&entry.path),
            ));
        }
        self.journal.begin_all(&journaled)?;
        for entry in journaled {
            match entry.op {
                JournalOp::Copy => rows.copies.push(Step::fresh(entry)),
                _ => rows.trashes.push(Step::fresh(entry)),
            }
        }
        Ok(())
    }

    /// Renames with a few tries over passing errors (the bounded retry of `cv_fs`).
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        cv_fs::retry("move", from, || self.fs.rename(from, to))
    }

    /// Removes a file with a few tries over passing errors (the bounded retry of `cv_fs`).
    fn remove(&self, path: &Path) -> io::Result<()> {
        cv_fs::retry("remove", path, || self.fs.remove_file(path))
    }

    fn complete_commit(&mut self, commit: &Step) -> Result<(), Fail> {
        let moved = self.fs.meta(Path::new(&commit.entry.to)).ok().flatten();
        self.journal.complete(
            &commit.entry,
            &Completion::Done {
                size_bytes: None,
                modified_ms: moved.map(|m| m.modified_ms),
            },
        )?;
        Ok(())
    }

    /// Carries on an unfinished batch: the copies that are done are checked again against their
    /// originals, the others are copied, and the titles are put in place one after the other.
    /// `titles` names the titles for the progress.
    ///
    /// # Errors
    /// Only a journal that cannot be read or written.
    pub fn resume(
        &mut self,
        batch_id: &str,
        titles: &std::collections::BTreeMap<String, String>,
        progress: ProgressFn<'_>,
    ) -> Result<Vec<(Option<String>, JobOutcome)>, MoveError> {
        let _at_work = AtWork::new(batch_id);
        let mut items: Vec<ItemRows> = Vec::new();
        for item in group_rows(self.journal.batch(batch_id)?) {
            if !item.unfinished() {
                continue;
            }
            if item.commit.is_none() {
                // An incomplete journal: nothing was changed. Close it instead of offering it.
                self.take_back(&mut item.clone(), TakeBack::Rollback)?;
                continue;
            }
            items.push(item);
        }
        self.total_titles = items.len();
        self.done_titles = 0;
        self.bytes_done = 0;
        self.bytes_total = items
            .iter()
            .flat_map(|i| i.copies.iter().filter(|s| !s.done()))
            .filter_map(|s| self.fs.meta(Path::new(&s.entry.from)).ok().flatten())
            .map(|m| m.len)
            .sum();
        let mut out = Vec::with_capacity(items.len());
        let mut stopped = false;
        for item in &mut items {
            let id = item.item.clone();
            if stopped {
                out.push((id, JobOutcome::NotStarted));
                continue;
            }
            self.title = id
                .as_ref()
                .and_then(|id| titles.get(id))
                .cloned()
                .unwrap_or_default();
            let outcome = if item.across() {
                self.drive(item, None, true, progress)?
            } else {
                self.rename_item(item, progress)?
            };
            if matches!(outcome, JobOutcome::Moved { .. }) {
                self.done_titles += 1;
            } else {
                stopped = true;
            }
            self.report(progress, "", MovePhase::Finishing);
            out.push((id, outcome));
        }
        Ok(out)
    }

    // ---- the three phases of a cross-volume title ------------------------------------------------

    /// Copies, commits and trashes the originals of one title. `reverify` re-checks the copies
    /// that are already done (a resume). `job` gives the listing of the folder when it is known.
    pub(crate) fn drive(
        &mut self,
        rows: &mut ItemRows,
        job: Option<&MoveJob>,
        reverify: bool,
        progress: ProgressFn<'_>,
    ) -> Result<JobOutcome, MoveError> {
        match self.copy_commit_trash(rows, job, reverify, progress) {
            Ok(notes) => Ok(placed(rows, notes)),
            Err(Fail::Journal(error)) => Err(error.into()),
            Err(Fail::Canceled) => {
                if rows.committed() {
                    // Whole in the new place: finish the originals and stop.
                    let notes = self
                        .trash_originals(rows)
                        .map_err(|f| self.journal_only(f))?;
                    return Ok(placed(rows, notes));
                }
                let taken = self.take_back(rows, TakeBack::Rollback)?;
                let _ = taken;
                Ok(JobOutcome::Canceled)
            }
            Err(Fail::Mismatch(reason)) => {
                self.take_back(rows, TakeBack::Rollback)?;
                Ok(JobOutcome::Failed(reason))
            }
            Err(Fail::Stop(reason)) => {
                self.mark_stopped(rows, &reason)?;
                Ok(JobOutcome::Stopped(reason))
            }
            Err(Fail::CrossesDevices) => {
                let reason = "The folders are on different drives".to_owned();
                self.mark_stopped(rows, &reason)?;
                Ok(JobOutcome::Stopped(reason))
            }
        }
    }

    /// Writes the reason a move stopped into the first row that still has work to do, so the
    /// reason survives a restart.
    fn mark_stopped(&mut self, rows: &ItemRows, reason: &str) -> Result<(), MoveError> {
        if let Some(step) = rows
            .copies
            .iter()
            .chain(rows.commit.iter())
            .chain(rows.trashes.iter())
            .find(|s| s.open())
        {
            self.journal.complete(
                &step.entry,
                &Completion::Failed(format!("{STOPPED_PREFIX}{reason}")),
            )?;
        }
        Ok(())
    }

    fn journal_only(&self, fail: Fail) -> MoveError {
        match fail {
            Fail::Journal(error) => error.into(),
            Fail::Canceled | Fail::Mismatch(_) | Fail::Stop(_) | Fail::CrossesDevices => {
                MoveError::Invalid("the move stopped".to_owned())
            }
        }
    }

    fn copy_commit_trash(
        &mut self,
        rows: &mut ItemRows,
        job: Option<&MoveJob>,
        reverify: bool,
        progress: ProgressFn<'_>,
    ) -> Result<Vec<String>, Fail> {
        let commit = rows
            .commit
            .clone()
            .ok_or_else(|| Fail::Stop("The move has no folder step".to_owned()))?;
        let (from, to) = (
            PathBuf::from(&commit.entry.from),
            PathBuf::from(&commit.entry.to),
        );
        let stage = stage_dir(&to, &commit.entry.batch_id, rows.item.as_deref());

        if !rows.committed() {
            // A run that stopped after the staging folder became the destination must not make
            // a new, empty staging folder next to it.
            let staging = self.meta(&to)?.is_none();
            if staging {
                self.prepare_stage(&from, &stage, job)?;
            }
            let mut first = 0;
            let mut rounds = 0;
            loop {
                for index in first..rows.copies.len() {
                    self.checkpoint()?;
                    let step = rows.copies[index].clone();
                    let rel = relative(&stage, Path::new(&step.entry.to));
                    if step.done() {
                        if reverify && !self.still_matches(&step, &stage, &to)? {
                            // Ours, and no longer what the original is: copy it again.
                            self.discard_copy(&step, &stage, &to);
                            self.journal.complete(
                                &step.entry,
                                &Completion::Failed(format!("{STOPPED_PREFIX}copy again")),
                            )?;
                        } else {
                            continue;
                        }
                    }
                    self.report(progress, &rel, MovePhase::Copying);
                    rows.copies[index] = self.copy_file(&step, &rel, progress)?;
                }
                // Files that arrived in the old folder while it was copied (a subtitle, an
                // image) come along too, journaled first like the others. Once the staging
                // folder is in place nothing is added to it; the notes name what stays.
                if !staging {
                    break;
                }
                let found = enumerate(&from).map_err(Fail::Stop)?;
                for dir in found.iter().filter(|e| e.kind == EntryKind::Dir) {
                    self.fs
                        .create_dir_all(&stage.join(&dir.path))
                        .map_err(|e| Fail::Stop(reason_of(&e, &stage, true)))?;
                }
                let before = rows.copies.len();
                self.append_files(rows, &from, &found)?;
                if rows.copies.len() == before {
                    break;
                }
                rounds += 1;
                if rounds > 3 {
                    return Err(Fail::Stop(format!(
                        "Files keep arriving in {} while it is moved",
                        from.display()
                    )));
                }
                self.bytes_total += rows.copies[before..]
                    .iter()
                    .filter_map(|s| self.fs.meta(Path::new(&s.entry.from)).ok().flatten())
                    .map(|m| m.len)
                    .sum::<u64>();
                first = before;
            }
            self.commit_title(rows, &commit, &to, &stage)?;
        }
        self.report(progress, "", MovePhase::Finishing);
        self.trash_originals(rows)
    }

    /// Creates the staging folder with the folders of the title in it, so empty folders arrive.
    fn prepare_stage(&self, from: &Path, stage: &Path, job: Option<&MoveJob>) -> Result<(), Fail> {
        self.fs
            .create_dir_all(stage)
            .map_err(|e| Fail::Stop(reason_of(&e, stage, true)))?;
        let entries = match job {
            Some(job) => job.entries.clone(),
            None => enumerate(from).unwrap_or_default(),
        };
        for entry in entries.iter().filter(|e| e.kind == EntryKind::Dir) {
            self.fs
                .create_dir_all(&stage.join(&entry.path))
                .map_err(|e| Fail::Stop(reason_of(&e, stage, true)))?;
        }
        Ok(())
    }

    fn meta(&self, path: &Path) -> Result<Option<cv_rename::exec::Meta>, Fail> {
        self.fs
            .meta(path)
            .map_err(|e| Fail::Stop(reason_of(&e, path, false)))
    }

    /// Copies one file: read once and hashed, written to `.cvpart`, flushed, read back and
    /// compared, renamed into place, journaled `done`.
    fn copy_file(
        &mut self,
        step: &Step,
        rel: &str,
        progress: ProgressFn<'_>,
    ) -> Result<Step, Fail> {
        let (src, dst) = (
            PathBuf::from(&step.entry.from),
            PathBuf::from(&step.entry.to),
        );
        let part = part_of(&dst);
        let Some(src_meta) = self.meta(&src)? else {
            return Err(Fail::Stop(format!(
                "The original {} is missing",
                name_of(&src)
            )));
        };
        if let Some(existing) = self.meta(&dst)? {
            // A crash between the rename into place and the journal leaves a verified file.
            if !existing.is_dir && existing.len == src_meta.len {
                let (a, _) = self.hash_file(&src, rel, progress)?;
                let (b, _) = self.hash_file(&dst, rel, progress)?;
                if a == b {
                    return self.mark_copied(step, &dst);
                }
            }
            // Anything in the hidden staging folder of this batch is a copy this batch made: one
            // that no longer matches its original (it changed) is made again.
            if existing.is_dir {
                return Err(Fail::Stop(format!(
                    "{} exists already and is different; nothing was replaced",
                    name_of(&dst)
                )));
            }
            self.remove(&dst)
                .map_err(|e| Fail::Stop(reason_of(&e, &dst, true)))?;
        }
        if let Some(parent) = dst.parent() {
            self.fs
                .create_dir_all(parent)
                .map_err(|e| Fail::Stop(reason_of(&e, parent, true)))?;
        }
        // A leftover part of ours from an earlier run.
        if self.meta(&part)?.is_some() {
            self.fs
                .remove_file(&part)
                .map_err(|e| Fail::Stop(reason_of(&e, &part, true)))?;
        }

        let reader = self
            .fs
            .open_read(&src)
            .map_err(|e| Fail::Stop(reason_of(&e, &src, false)))?;
        let mut writer = self
            .fs
            .create_write(&part)
            .map_err(|e| Fail::Stop(reason_of(&e, &part, true)))?;
        let (source_hash, copied) =
            match self.pipe(reader, writer.as_mut(), &src, &part, rel, progress) {
                Ok(done) => done,
                Err(Piped::Canceled(fail)) => {
                    drop(writer);
                    let _ = self.fs.remove_file(&part);
                    return Err(fail);
                }
                Err(Piped::Failed(fail)) => {
                    drop(writer);
                    // The partial file is ours; leave it for the roll back to remove.
                    return Err(fail);
                }
            };
        // Flushed to the disk (`sync_all`) before it is read back.
        writer
            .finish(src_meta.modified_ms)
            .map_err(|e| Fail::Stop(reason_of(&e, &part, true)))?;

        // Read the copy back from the disk.
        self.report(progress, rel, MovePhase::Verifying);
        let part_meta = self.meta(&part)?;
        if part_meta.map(|m| m.len) != Some(copied) || copied != src_meta.len {
            let _ = self.fs.remove_file(&part);
            return Err(Fail::Mismatch(format!(
                "The copy of {} is not the same size as the original",
                name_of(&src)
            )));
        }
        let (copy_hash, read_back) = self.hash_copy(&part, rel, progress)?;
        if copy_hash != source_hash || read_back != copied {
            let _ = self.fs.remove_file(&part);
            return Err(Fail::Mismatch(format!(
                "The copy of {} does not match the original",
                name_of(&src)
            )));
        }
        // The original must not have changed while it was copied.
        let after = self.meta(&src)?;
        if after.map(|m| (m.len, m.modified_ms)) != Some((src_meta.len, src_meta.modified_ms)) {
            let _ = self.fs.remove_file(&part);
            return Err(Fail::Stop(format!(
                "{} changed while it was being copied",
                name_of(&src)
            )));
        }
        // A read-only original gives a read-only copy (the flag is what a person set, like the
        // time). Best effort: a destination that keeps no such flag still gets the file.
        if let Err(error) = self.fs.keep_read_only(&src, &part) {
            tracing::debug!(%error, path = %part.display(), "the read-only flag was not copied");
        }
        self.rename(&part, &dst)
            .map_err(|e| Fail::Stop(reason_of(&e, &dst, true)))?;
        self.mark_copied(step, &dst)
    }

    fn mark_copied(&mut self, step: &Step, dst: &Path) -> Result<Step, Fail> {
        let meta = self.meta(dst)?;
        let mut done = step.clone();
        done.state = JournalState::Done;
        done.size = meta.map(|m| m.len);
        done.modified = meta.map(|m| m.modified_ms);
        self.journal.complete(
            &step.entry,
            &Completion::Done {
                size_bytes: done.size,
                modified_ms: done.modified,
            },
        )?;
        Ok(done)
    }

    /// Copies `reader` to `writer` through the pipeline: a reader thread reads and hashes the
    /// chunks in order and hands them over a bounded channel; this thread writes them in that
    /// order, counts the progress and stops at a pause or cancel. Returns the hash of what was
    /// read and the bytes written.
    fn pipe(
        &mut self,
        mut reader: Box<dyn Read + Send>,
        writer: &mut dyn CopyWrite,
        src: &Path,
        part: &Path,
        rel: &str,
        progress: ProgressFn<'_>,
    ) -> Result<(Vec<u8>, u64), Piped> {
        let chunk = self.fs.chunk_bytes().max(1);
        let (full_tx, full_rx) = mpsc::sync_channel::<io::Result<Vec<u8>>>(PIPELINE_DEPTH);
        let (empty_tx, empty_rx) = mpsc::channel::<Vec<u8>>();
        std::thread::scope(|scope| {
            let read_side = scope.spawn(move || -> Option<Vec<u8>> {
                let mut hasher = Sha256::new();
                let mut spare = PIPELINE_DEPTH + 1;
                loop {
                    // A returned buffer, or a new one while fewer than the pipeline holds exist.
                    let mut buffer = match empty_rx.try_recv() {
                        Ok(buffer) => buffer,
                        Err(mpsc::TryRecvError::Empty) if spare > 0 => {
                            spare -= 1;
                            Vec::new()
                        }
                        Err(mpsc::TryRecvError::Empty) => empty_rx.recv().ok()?,
                        Err(mpsc::TryRecvError::Disconnected) => return None,
                    };
                    buffer.resize(chunk, 0);
                    let mut filled = 0;
                    while filled < chunk {
                        match reader.read(&mut buffer[filled..]) {
                            Ok(0) => break,
                            Ok(n) => filled += n,
                            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                            Err(e) => {
                                let _ = full_tx.send(Err(e));
                                return None;
                            }
                        }
                    }
                    if filled == 0 {
                        return Some(hasher.finalize().as_slice().to_vec());
                    }
                    hasher.update(&buffer[..filled]);
                    buffer.truncate(filled);
                    if full_tx.send(Ok(buffer)).is_err() {
                        return None;
                    }
                }
            });
            let mut copied: u64 = 0;
            let outcome = loop {
                if let Err(fail) = self.checkpoint() {
                    break Err(Piped::Canceled(fail));
                }
                match full_rx.recv() {
                    Ok(Ok(buffer)) => {
                        if let Err(e) = writer.write_all(&buffer) {
                            break Err(Piped::Failed(Fail::Stop(reason_of(&e, part, true))));
                        }
                        let n = buffer.len() as u64;
                        copied += n;
                        self.bytes_done += n;
                        self.work_done += n;
                        self.tick(progress, rel, MovePhase::Copying);
                        let _ = empty_tx.send(buffer);
                    }
                    Ok(Err(e)) => break Err(Piped::Failed(Fail::Stop(reason_of(&e, src, false)))),
                    // The read side finished (or stopped; its answer says which).
                    Err(_) => break Ok(()),
                }
            };
            // Unblocks the read side whatever it waits for.
            drop(full_rx);
            drop(empty_tx);
            let hash = read_side.join();
            match (outcome, hash) {
                (Err(piped), _) => Err(piped),
                (Ok(()), Ok(Some(hash))) => Ok((hash, copied)),
                (Ok(()), _) => Err(Piped::Failed(Fail::Stop(format!(
                    "{} could not be read",
                    name_of(src)
                )))),
            }
        })
    }

    /// Hashes a finished copy read back from the disk (past the cache where the system allows).
    fn hash_copy(
        &mut self,
        path: &Path,
        rel: &str,
        progress: ProgressFn<'_>,
    ) -> Result<(Vec<u8>, u64), Fail> {
        let reader = self
            .fs
            .open_verify(path)
            .map_err(|e| Fail::Stop(reason_of(&e, path, false)))?;
        self.hash_reader(reader, path, rel, progress)
    }

    /// Hashes a file in chunks. The second value is the length read.
    fn hash_file(
        &mut self,
        path: &Path,
        rel: &str,
        progress: ProgressFn<'_>,
    ) -> Result<(Vec<u8>, u64), Fail> {
        let reader = self
            .fs
            .open_read(path)
            .map_err(|e| Fail::Stop(reason_of(&e, path, false)))?;
        self.hash_reader(reader, path, rel, progress)
    }

    fn hash_reader(
        &mut self,
        mut reader: Box<dyn Read + Send>,
        path: &Path,
        rel: &str,
        progress: ProgressFn<'_>,
    ) -> Result<(Vec<u8>, u64), Fail> {
        let mut hasher = Sha256::new();
        let mut buffer = vec![0_u8; self.fs.chunk_bytes().max(1)];
        let mut total = 0_u64;
        loop {
            self.checkpoint()?;
            let read = reader
                .read(&mut buffer)
                .map_err(|e| Fail::Stop(reason_of(&e, path, false)))?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
            total += read as u64;
            self.work_done += read as u64;
            self.tick(progress, rel, MovePhase::Verifying);
        }
        Ok((hasher.finalize().as_slice().to_vec(), total))
    }

    /// Whether a copy that was done earlier still matches its original (a resume re-checks).
    fn still_matches(&mut self, step: &Step, stage: &Path, dest: &Path) -> Result<bool, Fail> {
        let src = PathBuf::from(&step.entry.from);
        let Some(copy) = self.copy_location(step, stage, dest) else {
            return Ok(false);
        };
        let (Some(a), Some(b)) = (self.meta(&src)?, self.meta(&copy)?) else {
            return Ok(false);
        };
        if a.len != b.len {
            return Ok(false);
        }
        let mut noop = |_: &MoveProgress| {};
        let (x, _) = self.hash_file(&src, "", &mut noop)?;
        let (y, _) = self.hash_file(&copy, "", &mut noop)?;
        Ok(x == y)
    }

    /// Where a copy is on the disk now: in the staging folder, or in the destination once the
    /// staging folder was renamed.
    fn copy_location(&self, step: &Step, stage: &Path, dest: &Path) -> Option<PathBuf> {
        let in_stage = PathBuf::from(&step.entry.to);
        if self.fs.meta(&in_stage).ok().flatten().is_some() {
            return Some(in_stage);
        }
        // While the staging folder is there, it was not put in place: a file in the
        // destination belongs to someone else (another move of the title, for one).
        if self.fs.meta(stage).ok().flatten().is_some() {
            return None;
        }
        // In the destination: only a file that is what this batch copied counts as its copy
        // (the recorded size and time, or the original's), never a file somebody else put there.
        let rel = relative(stage, &in_stage);
        let moved = dest.join(rel);
        let found = self.fs.meta(&moved).ok().flatten()?;
        let original = self
            .fs
            .meta(Path::new(&step.entry.from))
            .ok()
            .flatten()
            .map(|m| (m.len, m.modified_ms));
        let recorded = step.size.zip(step.modified);
        let same = |known: Option<(u64, i64)>| known == Some((found.len, found.modified_ms));
        (!found.is_dir && (same(recorded) || same(original))).then_some(moved)
    }

    fn discard_copy(&self, step: &Step, stage: &Path, dest: &Path) {
        if let Some(path) = self.copy_location(step, stage, dest) {
            let _ = self.fs.remove_file(&path);
        }
        let _ = self.fs.remove_file(&part_of(Path::new(&step.entry.to)));
    }

    /// The staging folder becomes the destination and the library follows, in one step each.
    fn commit_title(
        &mut self,
        rows: &mut ItemRows,
        commit: &Step,
        to: &Path,
        stage: &Path,
    ) -> Result<(), Fail> {
        // The originals must still be what was copied: the same size, and the time the copy was
        // given (within the 2 s some file systems round times to).
        for step in &rows.copies {
            let src = PathBuf::from(&step.entry.from);
            let now = self.meta(&src)?;
            let retimed = match (now.map(|m| m.modified_ms), step.modified) {
                (Some(now), Some(copied)) => (now - copied).abs() > 2_000,
                _ => false,
            };
            if now.map(|m| m.len) != step.size || retimed {
                // The copy is ours and out of date: it goes, so a resume copies it again.
                self.discard_copy(step, stage, to);
                self.journal.complete(
                    &step.entry,
                    &Completion::Failed(format!("{STOPPED_PREFIX}the original changed")),
                )?;
                return Err(Fail::Stop(format!(
                    "{} changed after it was copied; resume copies it again",
                    name_of(&src)
                )));
            }
        }
        let staged = self.meta(stage)?.is_some();
        let placed = self.meta(to)?.is_some();
        match (staged, placed) {
            (true, false) => {
                if let Some(parent) = to.parent() {
                    self.fs
                        .create_dir_all(parent)
                        .map_err(|e| Fail::Stop(reason_of(&e, parent, true)))?;
                }
                self.rename(stage, to)
                    .map_err(|e| Fail::Stop(reason_of(&e, to, true)))?;
            }
            (false, true) => {}
            (true, true) => {
                return Err(Fail::Stop(format!(
                    "{} exists already; nothing was replaced",
                    to.display()
                )));
            }
            (false, false) => {
                return Err(Fail::Stop(
                    "The copied files are missing; resume copies them again".to_owned(),
                ));
            }
        }
        // The library follows in the same transaction as the row.
        self.journal.complete(
            &commit.entry,
            &Completion::Done {
                size_bytes: None,
                modified_ms: self.fs.meta(to).ok().flatten().map(|m| m.modified_ms),
            },
        )?;
        rows.commit = Some(Step {
            state: JournalState::Done,
            ..commit.clone()
        });
        Ok(())
    }

    /// Moves the originals to the trash folder of their data source, one by one. A file that
    /// cannot be moved stays where it was and is told in the notes.
    fn trash_originals(&mut self, rows: &mut ItemRows) -> Result<Vec<String>, Fail> {
        let mut notes = self.bin_originals(rows)?;
        let mut index = 0;
        while index < rows.trashes.len() {
            let step = rows.trashes[index].clone();
            index += 1;
            if !step.open() {
                continue;
            }
            let (src, dst) = (
                PathBuf::from(&step.entry.from),
                PathBuf::from(&step.entry.to),
            );
            // An original leaves only when its copy is verified, whatever the journal holds.
            if !rows.copy_of(&step.entry.from).is_some_and(Step::done) {
                notes.push(format!(
                    "{} was left in the old folder: it has no verified copy",
                    name_of(&src)
                ));
                self.journal.complete(
                    &step.entry,
                    &Completion::Failed("the original has no verified copy".to_owned()),
                )?;
                rows.trashes[index - 1].state = JournalState::Failed;
                continue;
            }
            let completion = match (self.fs.meta(&src), self.fs.meta(&dst)) {
                (Ok(None), Ok(Some(moved))) => Completion::Done {
                    size_bytes: Some(moved.len),
                    modified_ms: Some(moved.modified_ms),
                },
                (Ok(None), _) => {
                    notes.push(format!(
                        "{} was already gone from the old folder",
                        name_of(&src)
                    ));
                    Completion::Failed("the original was already gone".to_owned())
                }
                (Ok(Some(_)), Ok(Some(_))) => {
                    notes.push(format!(
                        "{} was left in the old folder: its place in the trash folder is taken",
                        name_of(&src)
                    ));
                    Completion::Failed("the trash folder already has this path".to_owned())
                }
                (Ok(Some(_)), Ok(None)) => {
                    let moved = dst
                        .parent()
                        .map_or(Ok(()), |parent| self.fs.create_dir_all(parent))
                        .and_then(|()| self.rename(&src, &dst));
                    match moved {
                        Ok(()) => {
                            let meta = self.fs.meta(&dst).ok().flatten();
                            Completion::Done {
                                size_bytes: meta.map(|m| m.len),
                                modified_ms: meta.map(|m| m.modified_ms),
                            }
                        }
                        Err(e) => {
                            let why = reason_of(&e, &src, true);
                            // In use or not allowed is often over later: the row stays open,
                            // so Resume in Activity tries again.
                            let retry = transient(&e)
                                || e.kind() == io::ErrorKind::PermissionDenied
                                || matches!(e.raw_os_error(), Some(16 | 26 | 32 | 33));
                            notes.push(if retry {
                                format!(
                                    "{} was left in the old folder: {why}. Resume the move in \
                                     Activity to finish it",
                                    name_of(&src)
                                )
                            } else {
                                format!("{} was left in the old folder: {why}", name_of(&src))
                            });
                            if retry {
                                Completion::Failed(format!("{STOPPED_PREFIX}{why}"))
                            } else {
                                Completion::Failed(why)
                            }
                        }
                    }
                }
                (Err(e), _) | (_, Err(e)) => {
                    let why = reason_of(&e, &src, false);
                    notes.push(format!(
                        "{} was left in the old folder: {why}",
                        name_of(&src)
                    ));
                    Completion::Failed(why)
                }
            };
            self.journal.complete(&step.entry, &completion)?;
            let stepped = &mut rows.trashes[index - 1];
            match &completion {
                Completion::Done { .. } => stepped.state = JournalState::Done,
                Completion::Failed(error) => {
                    stepped.state = JournalState::Failed;
                    stepped.error.clone_from(error);
                }
                Completion::Undone => stepped.state = JournalState::Undone,
            }
        }
        notes.extend(self.remove_old_folders(rows));
        Ok(notes)
    }

    /// Sends the originals the bin can take to the Recycle Bin / Trash in one call, each only
    /// when its copy is verified, and records each one's bin id on its row. What the bin does not
    /// take stays open for the trash folder (the caller's loop).
    fn bin_originals(&mut self, rows: &mut ItemRows) -> Result<Vec<String>, Fail> {
        let Some((bin, ledger)) = self.bin.clone() else {
            return Ok(Vec::new());
        };
        let mut picked: Vec<(usize, Option<cv_rename::exec::Meta>)> = Vec::new();
        for (index, step) in rows.trashes.iter().enumerate() {
            let src = Path::new(&step.entry.from);
            if !step.open()
                || !rows.copy_of(&step.entry.from).is_some_and(Step::done)
                || !bin.supports(src)
            {
                continue;
            }
            if let Ok(Some(meta)) = self.fs.meta(src)
                && !meta.is_dir
            {
                picked.push((index, Some(meta)));
            }
        }
        if picked.is_empty() {
            return Ok(Vec::new());
        }
        let paths: Vec<PathBuf> = picked
            .iter()
            .map(|(index, _)| PathBuf::from(&rows.trashes[*index].entry.from))
            .collect();
        let outcome = bin.send_tracked(&paths);
        // An original that left the old folder but is not in the bin was deleted by the system
        // instead; its verified copy is in the new place, and Undo copies it back.
        let mut notes: Vec<String> = outcome
            .sent
            .iter()
            .zip(bin.present(&outcome.sent))
            .filter(|(sent, here)| !here && !super::policy_bin::is_erased(sent))
            .map(|(sent, _)| {
                tracing::warn!(file = %sent.original, "an original sent to the bin is not in it");
                format!(
                    "{} left the old folder but is not in the Recycle Bin; its verified copy is                      in the new place",
                    name_of(Path::new(&sent.original))
                )
            })
            .collect();
        for (index, meta) in picked {
            let step = rows.trashes[index].clone();
            let Some(sent) = outcome.sent.iter().find(|s| s.original == step.entry.from) else {
                continue;
            };
            if !sent.bin_id.is_empty()
                && let Err(error) =
                    ledger.record(&step.entry.batch_id, step.entry.seq, &sent.bin_id)
            {
                tracing::warn!(%error, file = %step.entry.from, "the bin id of an original was not kept");
            }
            let completion = Completion::Done {
                size_bytes: meta.map(|m| m.len),
                modified_ms: meta.map(|m| m.modified_ms),
            };
            self.journal.complete(&step.entry, &completion)?;
            let stepped = &mut rows.trashes[index];
            stepped.state = JournalState::Done;
            stepped.size = meta.map(|m| m.len);
            stepped.modified = meta.map(|m| m.modified_ms);
            stepped.error.clear();
        }
        notes.extend(outcome.error.map(|error| {
            format!(
                "The Recycle Bin refused some originals ({error}); they went to the trash folder"
            )
        }));
        Ok(notes)
    }

    /// The originals of a title that are in the Recycle Bin / Trash, from the ledger.
    fn binned_of(&self, item: &ItemRows) -> Vec<(u32, BinnedItem)> {
        let Some((_, ledger)) = &self.bin else {
            return Vec::new();
        };
        let Some(batch) = item.trashes.first().map(|s| s.entry.batch_id.clone()) else {
            return Vec::new();
        };
        let ids = ledger.lookup(&batch);
        item.trashes
            .iter()
            .filter(|s| s.done())
            .filter_map(|s| {
                let bin_id = ids.get(&s.entry.seq)?;
                Some((
                    s.entry.seq,
                    BinnedItem {
                        original: s.entry.from.clone(),
                        bin_id: bin_id.clone(),
                        size: s.size.unwrap_or(0),
                    },
                ))
            })
            .collect()
    }

    /// Removes the folders of the old title folder that are empty now, deepest first, then the
    /// folder itself and its empty parents up to (never including) the data source root. The
    /// notes name the files that are still in the old folder and were not part of the move.
    fn remove_old_folders(&self, rows: &ItemRows) -> Vec<String> {
        let Some(from) = rows.old_folder().map(Path::new) else {
            return Vec::new();
        };
        remove_empty_tree(self.fs, from);
        let mut notes: Vec<String> = enumerate(from)
            .unwrap_or_default()
            .into_iter()
            .filter(|e| !matches!(e.kind, EntryKind::Dir))
            .filter(|e| rows.trash_of(&text(&from.join(&e.path))).is_none())
            .map(|e| {
                format!(
                    "{} stays in the old folder: it was not part of the move",
                    e.path
                )
            })
            .collect();
        notes.truncate(20);
        let Some(root) = self.root_of(from) else {
            return notes;
        };
        let mut dir = from.parent();
        while let Some(current) = dir {
            if current == root || !current.starts_with(&root) {
                break;
            }
            if self.fs.list_dir(current).is_ok_and(|n| n.is_empty())
                && self.fs.remove_dir(current).is_ok()
            {
                dir = current.parent();
            } else {
                break;
            }
        }
        notes
    }

    fn root_of(&self, path: &Path) -> Option<PathBuf> {
        self.roots
            .iter()
            .filter(|root| path.starts_with(root))
            .max_by_key(|root| root.components().count())
            .cloned()
    }

    // ---- control and progress ----------------------------------------------------------------

    fn cancelled(&self) -> bool {
        self.control.as_ref().is_some_and(|c| c.is_cancelled())
    }

    /// Waits while paused; fails when the move was cancelled.
    fn checkpoint(&self) -> Result<(), Fail> {
        let Some(control) = &self.control else {
            return Ok(());
        };
        loop {
            if control.is_cancelled() {
                return Err(Fail::Canceled);
            }
            if !control.is_paused() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(40));
        }
    }

    fn report(&mut self, progress: ProgressFn<'_>, file: &str, phase: MovePhase) {
        let now = Instant::now();
        self.last_progress = Some(now);
        let (bytes_per_second, seconds_left) = self.speed(now);
        progress(&MoveProgress {
            done: self.done_titles,
            total: self.total_titles,
            title: self.title.clone(),
            bytes_done: self.bytes_done,
            bytes_total: self.bytes_total,
            current_file: file.to_owned(),
            phase,
            bytes_per_second,
            seconds_left,
        });
    }

    /// The speed over the last few seconds and the time the rest needs at it. A gap (a pause, a
    /// title without copies) starts the measure again.
    fn speed(&mut self, now: Instant) -> (u64, Option<u64>) {
        if self
            .samples
            .back()
            .is_some_and(|(at, _)| now.duration_since(*at) > Duration::from_secs(2))
        {
            self.samples.clear();
        }
        self.samples.push_back((now, self.work_done));
        while self
            .samples
            .front()
            .is_some_and(|(at, _)| now.duration_since(*at) > SPEED_WINDOW)
        {
            self.samples.pop_front();
        }
        let Some(&(first_at, first_work)) = self.samples.front() else {
            return (0, None);
        };
        let span = now.duration_since(first_at).as_secs_f64();
        let work = self.work_done.saturating_sub(first_work);
        if span < 0.5 || work == 0 {
            return (0, None);
        }
        #[allow(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss
        )]
        let rate = (work as f64 / span) as u64;
        // Every byte is written once and read back once.
        let left = self
            .bytes_total
            .saturating_mul(2)
            .saturating_sub(self.work_done);
        (rate, (rate > 0).then(|| left.div_ceil(rate)))
    }

    fn tick(&mut self, progress: ProgressFn<'_>, file: &str, phase: MovePhase) {
        if self
            .last_progress
            .is_none_or(|at| at.elapsed() >= PROGRESS_EVERY)
        {
            self.report(progress, file, phase);
        }
    }
}

/// How the copy pipeline stopped.
enum Piped {
    /// Paused-then-cancelled: the part file is removed.
    Canceled(Fail),
    /// The disk failed: the part file stays for the roll back.
    Failed(Fail),
}

/// `path` below `base`, as text; the path itself when it is not below.
fn relative(base: &Path, path: &Path) -> String {
    path.strip_prefix(base).map_or_else(|_| text(path), text)
}

/// Whether `path` is a symbolic link or a junction (not followed).
fn is_link(path: &Path) -> bool {
    std::fs::symlink_metadata(StdFs::native(path)).is_ok_and(|m| m.file_type().is_symlink())
}

/// Removes empty folders below and including `dir`; anything that holds a file stays. A link
/// is never entered or removed: what it points at is not part of the title.
fn remove_empty_tree(fs: &dyn MoveFs, dir: &Path) {
    if is_link(dir) {
        return;
    }
    let Ok(names) = fs.list_dir(dir) else {
        return;
    };
    for name in names {
        let child = dir.join(name);
        if !is_link(&child) && fs.meta(&child).ok().flatten().is_some_and(|m| m.is_dir) {
            remove_empty_tree(fs, &child);
        }
    }
    let _ = fs.remove_dir(dir);
}

// ---- taking a move back ------------------------------------------------------------------------

/// How far a title goes back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TakeBack {
    /// An unfinished or cancelled move: whatever was done is taken back, best effort, and the
    /// copies the batch made are removed.
    Rollback,
    /// A finished move: every file is checked first; the copies go to the trash folder.
    Undo,
}

impl MoveEngine<'_> {
    /// Takes a finished batch back. See the module documentation.
    ///
    /// # Errors
    /// A journal that cannot be read or written.
    pub fn undo(&mut self, batch_id: &str) -> Result<Vec<TakenBack>, MoveError> {
        self.undo_only(batch_id, None)
    }

    /// [`Self::undo`] for the titles `only` names (their journal item ids); `None` takes back
    /// every title.
    ///
    /// # Errors
    /// A journal that cannot be read or written.
    pub fn undo_only(
        &mut self,
        batch_id: &str,
        only: Option<&[String]>,
    ) -> Result<Vec<TakenBack>, MoveError> {
        let _at_work = AtWork::new(batch_id);
        let rows = self.journal.batch(batch_id)?;
        let mut out = Vec::new();
        for mut item in group_rows(rows).into_iter().rev() {
            if !item.all().any(Step::done) {
                continue;
            }
            if let Some(only) = only
                && !item.item.as_ref().is_some_and(|id| only.contains(id))
            {
                continue;
            }
            out.push(self.take_back(&mut item, TakeBack::Undo)?);
        }
        Ok(out)
    }

    /// Takes the unfinished titles of a batch back and removes what the batch made. Titles that
    /// finished stay in their new place (undo takes those back). A title that was already put
    /// in place when the batch stopped goes back like an undo: its copies are the library's
    /// files by then and go to the trash folder, never away for good.
    ///
    /// # Errors
    /// A journal that cannot be read or written.
    pub fn roll_back(&mut self, batch_id: &str) -> Result<Vec<TakenBack>, MoveError> {
        let _at_work = AtWork::new(batch_id);
        let rows = self.journal.batch(batch_id)?;
        let mut out = Vec::new();
        for mut item in group_rows(rows).into_iter().rev() {
            if !item.unfinished() {
                continue;
            }
            out.push(self.take_back(&mut item, TakeBack::Rollback)?);
        }
        Ok(out)
    }

    pub(crate) fn take_back(
        &mut self,
        item: &mut ItemRows,
        mode: TakeBack,
    ) -> Result<TakenBack, MoveError> {
        let mut report = TakenBack {
            item: item.item.clone(),
            status: TakeBackStatus::Restored,
            restored: 0,
            discarded: 0,
            notes: Vec::new(),
        };
        let Some(commit) = item.commit.clone() else {
            // The journal was cut short while it was being written: nothing was touched on the
            // disk yet (the whole batch is journaled first). Close the rows that are left.
            for step in item.copies.iter().chain(item.trashes.iter()) {
                if step.open() {
                    self.journal
                        .complete(&step.entry, &Completion::Failed("rolled back".to_owned()))?;
                }
            }
            return Ok(report);
        };
        let (from, to) = (
            PathBuf::from(&commit.entry.from),
            PathBuf::from(&commit.entry.to),
        );
        let stage = stage_dir(&to, &commit.entry.batch_id, item.item.as_deref());

        // A title that is in its new place goes back like an undo, whatever asked: every file
        // is checked first, and the copies are never removed for good.
        let mode = if commit.done() { TakeBack::Undo } else { mode };
        if commit.done() {
            self.settle_trash_rows(item)?;
        }
        if mode == TakeBack::Undo {
            let problems = self.verify_undo(item, &stage, &to);
            if !problems.is_empty() {
                report.status = TakeBackStatus::NotRestorable(problems);
                return Ok(report);
            }
        }

        // The same-volume move is one folder rename.
        if !item.across() {
            return self.take_back_rename(&commit, &from, &to, report);
        }

        // 1. The originals come back from the Recycle Bin (all at once, every one checked first)
        //    and from the trash folder.
        let binned = self.binned_of(item);
        if !binned.is_empty() {
            let items: Vec<BinnedItem> = binned.iter().map(|(_, b)| b.clone()).collect();
            let restored = match &self.bin {
                Some((bin, _)) => bin.restore(&items),
                None => Err(Vec::new()),
            };
            if let Err(problems) = restored {
                report.status = TakeBackStatus::Failed(problems.first().map_or_else(
                    || "The originals could not be taken back from the Recycle Bin".to_owned(),
                    |(path, why)| format!("{path}: {why}"),
                ));
                return Ok(report);
            }
            for step in item
                .trashes
                .iter()
                .filter(|s| binned.iter().any(|(seq, _)| *seq == s.entry.seq))
            {
                self.journal.complete(&step.entry, &Completion::Undone)?;
                report.restored += 1;
            }
        }
        for step in item.trashes.iter().rev() {
            if binned.iter().any(|(seq, _)| *seq == step.entry.seq) {
                continue;
            }
            if step.done() {
                let (orig, trashed) = (
                    PathBuf::from(&step.entry.from),
                    PathBuf::from(&step.entry.to),
                );
                if self.fs.meta(&orig).ok().flatten().is_some() {
                    // Never replace what is there now; the copies stay too.
                    report.status = TakeBackStatus::Failed(format!(
                        "{} exists in the old place already; nothing was replaced",
                        name_of(&orig)
                    ));
                    return Ok(report);
                }
                if let Some(parent) = orig.parent() {
                    let _ = self.fs.create_dir_all(parent);
                }
                match self.rename(&trashed, &orig) {
                    Ok(()) => {
                        self.journal.complete(&step.entry, &Completion::Undone)?;
                        report.restored += 1;
                    }
                    Err(e) => {
                        report.status = TakeBackStatus::Failed(reason_of(&e, &trashed, true));
                        return Ok(report);
                    }
                }
            } else if !matches!(step.state, JournalState::Undone) {
                self.journal
                    .complete(&step.entry, &Completion::Failed("rolled back".to_owned()))?;
            }
        }
        // 2. The library points at the originals again.
        if commit.done() {
            self.journal.complete(&commit.entry, &Completion::Undone)?;
        } else if commit.state != JournalState::Undone {
            self.journal
                .complete(&commit.entry, &Completion::Failed("rolled back".to_owned()))?;
        }
        // 3. The copies this batch made go: an undo sends them to the trash folder of the
        //    destination. A roll back removes a copy whose original is whole in the old place
        //    and keeps (in that trash folder) one whose original is not: it may be the only one.
        let undo_trash = self.undo_trash(&to, &commit.entry.batch_id, mode);
        // An undo sends the copies to the Recycle Bin when there is one, all in one call after
        // this loop.
        let mut to_bin: Vec<(Step, PathBuf)> = Vec::new();
        for step in item.copies.iter().rev() {
            let place = self.copy_location(step, &stage, &to);
            let _ = self.remove(&part_of(Path::new(&step.entry.to)));
            if let Some(path) = place {
                let keep = mode == TakeBack::Undo || !self.original_is_whole(step, &path);
                if keep
                    && mode == TakeBack::Undo
                    && self
                        .bin
                        .as_ref()
                        .is_some_and(|(bin, _)| bin.supports(&path))
                {
                    to_bin.push((step.clone(), path));
                    continue;
                }
                if keep {
                    let rel = relative(&stage, Path::new(&step.entry.to));
                    // Under the title's own folder, and never onto anything already in the
                    // trash folder: two titles of a batch can hold files of the same name, and
                    // a kept copy may be the only one.
                    let target = self.unused_in_trash(&undo_trash.join(name_of(&to)).join(&rel));
                    let moved = target
                        .parent()
                        .map_or(Ok(()), |parent| self.fs.create_dir_all(parent))
                        .and_then(|()| self.rename(&path, &target));
                    match moved {
                        Ok(()) => {
                            report.discarded += 1;
                            if mode == TakeBack::Rollback {
                                report.notes.push(format!(
                                    "{rel}: its original is not in the old folder, so the copy \
                                     was kept in {}",
                                    target.display()
                                ));
                            }
                        }
                        Err(e) => report.notes.push(format!(
                            "{} stays in the new place: {}",
                            name_of(&path),
                            reason_of(&e, &path, true)
                        )),
                    }
                } else if self.remove(&path).is_ok() {
                    report.discarded += 1;
                }
            }
            if step.done() {
                self.journal.complete(&step.entry, &Completion::Undone)?;
            } else if step.state != JournalState::Undone {
                self.journal
                    .complete(&step.entry, &Completion::Failed("rolled back".to_owned()))?;
            }
        }
        if !to_bin.is_empty()
            && let Some((bin, _)) = self.bin.clone()
        {
            let paths: Vec<PathBuf> = to_bin.iter().map(|(_, p)| p.clone()).collect();
            let outcome = bin.send_tracked(&paths);
            for (step, path) in &to_bin {
                let original = text(path);
                if outcome.sent.iter().any(|s| s.original == original) {
                    report.discarded += 1;
                } else {
                    // The bin refused it: the trash folder of the destination, as without one.
                    let rel = relative(&stage, Path::new(&step.entry.to));
                    let target = self.unused_in_trash(&undo_trash.join(name_of(&to)).join(&rel));
                    let moved = target
                        .parent()
                        .map_or(Ok(()), |parent| self.fs.create_dir_all(parent))
                        .and_then(|()| self.rename(path, &target));
                    match moved {
                        Ok(()) => report.discarded += 1,
                        Err(e) => report.notes.push(format!(
                            "{} stays in the new place: {}",
                            name_of(path),
                            reason_of(&e, path, true)
                        )),
                    }
                }
                if step.done() {
                    self.journal.complete(&step.entry, &Completion::Undone)?;
                } else if step.state != JournalState::Undone {
                    self.journal
                        .complete(&step.entry, &Completion::Failed("rolled back".to_owned()))?;
                }
            }
        }
        // 4. The folders of the title come back too, the empty ones included, and what the
        //    batch made and left empty goes. A file someone put in the new place after the move
        //    stays there and is named.
        if mode == TakeBack::Undo && commit.done() {
            for entry in enumerate(&to).unwrap_or_default() {
                match entry.kind {
                    EntryKind::Dir => {
                        let _ = self.fs.create_dir_all(&from.join(&entry.path));
                    }
                    EntryKind::File | EntryKind::Link { .. } => report.notes.push(format!(
                        "{} stays in {}: it was not part of the move",
                        entry.path,
                        to.display()
                    )),
                }
            }
        }
        remove_empty_tree(self.fs, &stage);
        remove_empty_tree(self.fs, &to);
        self.remove_created_parents(&to, commit.entry.created_parents);
        Ok(report)
    }

    /// Reads from the disk what a stop left of the trash step: an original that is no longer in
    /// the old folder and is in the trash folder went there, though the process was killed before
    /// its row said so. Its row is settled `done`, so the take-back brings it back from the trash
    /// folder instead of taking the copy in the new place for the only one.
    fn settle_trash_rows(&mut self, item: &mut ItemRows) -> Result<(), MoveError> {
        for step in &mut item.trashes {
            if !step.open() {
                continue;
            }
            let (orig, trashed) = (Path::new(&step.entry.from), Path::new(&step.entry.to));
            if let (Ok(None), Ok(Some(moved))) = (self.fs.meta(orig), self.fs.meta(trashed))
                && !moved.is_dir
            {
                self.journal.complete(
                    &step.entry,
                    &Completion::Done {
                        size_bytes: Some(moved.len),
                        modified_ms: Some(moved.modified_ms),
                    },
                )?;
                step.state = JournalState::Done;
                step.size = Some(moved.len);
                step.modified = Some(moved.modified_ms);
                step.error.clear();
            }
        }
        Ok(())
    }

    /// Takes a same-volume move back: the folder is renamed to where it was.
    fn take_back_rename(
        &mut self,
        commit: &Step,
        from: &Path,
        to: &Path,
        mut report: TakenBack,
    ) -> Result<TakenBack, MoveError> {
        let src = self.fs.meta(from).ok().flatten();
        let dst = self.fs.meta(to).ok().flatten();
        // A kill between the folder rename and the journal leaves the folder in the new place
        // while the journal and the library still say the old one: a roll back puts it back.
        let renamed = commit.done() || (src.is_none() && dst.is_some_and(|m| m.is_dir));
        if !renamed {
            if commit.open() {
                self.journal
                    .complete(&commit.entry, &Completion::Failed("rolled back".to_owned()))?;
            }
            return Ok(report);
        }
        if src.is_some() {
            report.status = TakeBackStatus::Failed(format!(
                "{} exists in the old place already; nothing was replaced",
                from.display()
            ));
            return Ok(report);
        }
        if let Some(parent) = from.parent() {
            let _ = self.fs.create_dir_all(parent);
        }
        match self.rename(to, from) {
            Ok(()) => {
                if commit.done() {
                    self.journal.complete(&commit.entry, &Completion::Undone)?;
                } else {
                    self.journal
                        .complete(&commit.entry, &Completion::Failed("rolled back".to_owned()))?;
                }
                report.restored += 1;
                self.remove_created_parents(to, commit.entry.created_parents);
            }
            // The row stays as it is, so the batch is still offered.
            Err(e) => report.status = TakeBackStatus::Failed(reason_of(&e, to, true)),
        }
        Ok(report)
    }

    /// Whether the original of a copy is still in the old place, whole (the copy's size).
    fn original_is_whole(&self, step: &Step, copy: &Path) -> bool {
        let original = self.fs.meta(Path::new(&step.entry.from)).ok().flatten();
        let copy = self.fs.meta(copy).ok().flatten();
        match (original, copy) {
            (Some(o), Some(c)) => !o.is_dir && o.len == c.len,
            _ => false,
        }
    }

    /// The folder copies go to when they are taken back: the trash folder of the destination's
    /// data source.
    fn undo_trash(&self, destination: &Path, batch_id: &str, mode: TakeBack) -> PathBuf {
        let root = self
            .root_of(destination)
            .or_else(|| destination.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| destination.to_path_buf());
        let suffix = match mode {
            TakeBack::Undo => "undo",
            TakeBack::Rollback => "rollback",
        };
        root.join(TRASH_DIR).join(format!("{batch_id}-{suffix}"))
    }

    /// `path`, or `name (2).ext`, `name (3).ext`... when something is there already. A rename
    /// on Windows replaces a file, so a place in the trash folder is always checked first.
    fn unused_in_trash(&self, path: &Path) -> PathBuf {
        if self.fs.meta(path).ok().flatten().is_none() {
            return path.to_path_buf();
        }
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let ext = path
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        (2_u32..)
            .map(|n| path.with_file_name(format!("{stem} ({n}){ext}")))
            .find(|candidate| self.fs.meta(candidate).ok().flatten().is_none())
            .unwrap_or_else(|| path.to_path_buf())
    }

    fn remove_created_parents(&self, path: &Path, count: Option<u32>) {
        let mut left = count.unwrap_or(0);
        let mut dir = path.parent();
        while left > 0 {
            let Some(current) = dir else { break };
            if self.fs.list_dir(current).is_ok_and(|n| n.is_empty())
                && self.fs.remove_dir(current).is_ok()
            {
                left -= 1;
                dir = current.parent();
            } else {
                break;
            }
        }
    }

    /// Checks that a title in its new place can be taken back whole, without touching anything.
    fn verify_undo(&self, item: &ItemRows, stage: &Path, dest: &Path) -> Vec<(String, String)> {
        let mut problems = Vec::new();
        let mut taken: BTreeSet<String> = BTreeSet::new();
        if !item.across() {
            if let Some(commit) = item.commit.as_ref().filter(|c| c.done()) {
                let (from, to) = (Path::new(&commit.entry.from), Path::new(&commit.entry.to));
                if self.fs.meta(to).ok().flatten().is_none() {
                    problems.push((
                        text(to),
                        "The folder is not in its new place any more".to_owned(),
                    ));
                }
                if self.fs.meta(from).ok().flatten().is_some() {
                    problems.push((text(from), "The old place is taken".to_owned()));
                }
            }
            return problems;
        }
        let binned = self.binned_of(item);
        if let Some((bin, _)) = &self.bin
            && !binned.is_empty()
        {
            let items: Vec<BinnedItem> = binned.iter().map(|(_, b)| b.clone()).collect();
            for (item, here) in items.iter().zip(bin.present(&items)) {
                if !here {
                    problems.push((item.original.clone(), TRASH_GONE.to_owned()));
                }
            }
        }
        for step in item.trashes.iter().filter(|s| s.done()) {
            let (orig, trashed) = (Path::new(&step.entry.from), Path::new(&step.entry.to));
            if binned.iter().any(|(seq, _)| *seq == step.entry.seq) {
                if self.fs.meta(orig).ok().flatten().is_some() && taken.insert(text(orig)) {
                    problems.push((text(orig), "The original place is taken".to_owned()));
                }
                continue;
            }
            match self.fs.meta(trashed).ok().flatten() {
                None => problems.push((text(trashed), TRASH_GONE.to_owned())),
                Some(m) if step.size.is_some_and(|s| s != m.len) => {
                    problems.push((text(trashed), TRASH_CHANGED.to_owned()));
                }
                Some(_) => {}
            }
            if self.fs.meta(orig).ok().flatten().is_some() && taken.insert(text(orig)) {
                problems.push((text(orig), "The original place is taken".to_owned()));
            }
        }
        for step in item.copies.iter().filter(|s| s.done()) {
            // An original that never went to the trash folder must still be in the old place:
            // otherwise the copy in the new place is the only one, and it stays there.
            if !item.trash_of(&step.entry.from).is_some_and(Step::done) {
                let original = self.fs.meta(Path::new(&step.entry.from)).ok().flatten();
                if original.map(|m| m.len) != step.size {
                    problems.push((step.entry.from.clone(), ONLY_COPY.to_owned()));
                }
            }
            if self.copy_location(step, stage, dest).is_some() {
                continue;
            }
            // Not found as the copy this batch made: either gone, or changed since.
            let rel = relative(stage, Path::new(&step.entry.to));
            let there = [PathBuf::from(&step.entry.to), dest.join(&rel)]
                .into_iter()
                .find(|p| self.fs.meta(p).ok().flatten().is_some());
            problems.push(match there {
                Some(path) => (
                    text(&path),
                    "The file was changed after the move".to_owned(),
                ),
                None => (
                    step.entry.to.clone(),
                    "The copy is no longer in the new place".to_owned(),
                ),
            });
        }
        problems
    }
}

/// What an undo says about an original that is gone from the trash folder.
pub(crate) const TRASH_GONE: &str = "The original is no longer in the trash folder";

/// What an undo says about an original that changed in the trash folder.
pub(crate) const TRASH_CHANGED: &str = "The original in the trash folder has changed";

/// What an undo says about a copy whose original is nowhere.
const ONLY_COPY: &str = "The original is in neither the old folder nor the trash folder, so the \
                         copy in the new place is the only one";
