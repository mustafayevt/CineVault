//! The executor: runs the plans of a batch against the disk, journaling every step first.

use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use cv_domain::MediaFileType;
use cv_nfo::ownership::classify_nfo_file;
use regex::Regex;

use super::fs::{Fs, Patient, is_within, same_path};
use super::journal::{Completion, ItemState, JournalEntry, JournalError, JournalOp, JournalStore};
use super::{CASE_TMP_PREFIX, ExecError, PART_SUFFIX, TRASH_DIR};
use crate::conflict::BatchItem;
use crate::plan::{FolderChange, Op, OpKind};

/// How a batch is executed.
#[derive(Debug, Clone)]
pub struct ExecOptions {
    /// Names the batch in the journal and the trash folder. Unique per run; see [`new_batch_id`].
    pub batch_id: String,
    /// The trash folder in the root of each data source.
    pub trash_dir: String,
    /// Files in a renamed item's folder that are moved to the trash: an extension such as `.txt`
    /// or a regular expression on the file name, ignoring case. Empty switches the clean-up
    /// off; [`crate::exec::DEFAULT_UNWANTED`] is the default list when it is on.
    pub unwanted_patterns: Vec<String>,
    /// Remove the folders a rename leaves empty (never a data source root).
    pub remove_empty_folders: bool,
    /// Folders that are never removed and never renamed as a whole: the roots of every data
    /// source of the library. A data source can lie inside the folder of another one (a TV source
    /// in a folder of a movie source), and an item of the outer one must not take it away.
    pub protected_dirs: Vec<PathBuf>,
    /// How long a rename waits for CineVault's own reads inside what it moves to end
    /// ([`super::OWN_READ_WAIT`] by default).
    pub own_read_wait: std::time::Duration,
}

impl ExecOptions {
    pub fn new(batch_id: impl Into<String>) -> Self {
        ExecOptions {
            batch_id: batch_id.into(),
            trash_dir: TRASH_DIR.to_owned(),
            unwanted_patterns: Vec::new(),
            remove_empty_folders: true,
            protected_dirs: Vec::new(),
            own_read_wait: super::OWN_READ_WAIT,
        }
    }

    /// Never removes or renames these folders as a whole; see [`ExecOptions::protected_dirs`].
    #[must_use]
    pub fn with_protected_dirs(mut self, dirs: impl IntoIterator<Item = PathBuf>) -> Self {
        self.protected_dirs.extend(dirs);
        self
    }
}

/// What the executor was given for one item, kept with the item's mark in the journal so a
/// resume runs exactly that again (steps already on the disk then count as done).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ItemPlan {
    pub item: BatchItem,
    pub trash_dir: String,
    pub unwanted_patterns: Vec<String>,
    pub remove_empty_folders: bool,
    pub protected_dirs: Vec<PathBuf>,
}

impl ItemPlan {
    fn of(item: &BatchItem, options: &ExecOptions) -> Self {
        ItemPlan {
            item: item.clone(),
            trash_dir: options.trash_dir.clone(),
            unwanted_patterns: options.unwanted_patterns.clone(),
            remove_empty_folders: options.remove_empty_folders,
            protected_dirs: options.protected_dirs.clone(),
        }
    }

    /// The options to run the plan again with, in `batch_id`.
    pub fn options(&self, batch_id: &str) -> ExecOptions {
        ExecOptions {
            batch_id: batch_id.to_owned(),
            trash_dir: self.trash_dir.clone(),
            unwanted_patterns: self.unwanted_patterns.clone(),
            remove_empty_folders: self.remove_empty_folders,
            protected_dirs: self.protected_dirs.clone(),
            own_read_wait: super::OWN_READ_WAIT,
        }
    }

    /// The plan stored with an item mark; `None` when there is none or it cannot be read.
    pub fn parse(text: &str) -> Option<Self> {
        serde_json::from_str(text).ok()
    }
}

/// Where a file of `root` goes in the trash of batch `batch`: `<root>/<trash_dir>/<batch>/<rel>`
/// when `trash_dir` is a folder name, the custom trash folder's
/// `<trash_dir>/.deletedByCineVault/<batch>/<drive>/<path>` when `trash_dir` is a full path on
/// the file's volume (Settings › Deleted files), and the data source's own trash folder for a file on another
/// volume (a move there would be a copy).
#[must_use]
pub fn trash_target(root: &Path, trash_dir: &str, batch: &str, path: &Path, rel: &Path) -> PathBuf {
    let dir = Path::new(trash_dir);
    if dir.is_absolute() {
        return cv_fs::volume::custom_trash_path(dir, batch, path)
            .unwrap_or_else(|| root.join(TRASH_DIR).join(batch).join(rel));
    }
    root.join(trash_dir).join(batch).join(rel)
}

/// A fresh batch id: the time in milliseconds and a counter.
pub fn new_batch_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    format!("{ms}-{}", COUNTER.fetch_add(1, Ordering::Relaxed))
}

/// How one item ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemStatus {
    Done,
    /// A step failed. Every step of the item that had succeeded was taken back (see `notes` for
    /// any that could not be), so the item is where it was before.
    Failed(String),
    /// The batch was canceled before this item started.
    Canceled,
}

/// What happened to one item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemReport {
    pub id: String,
    pub status: ItemStatus,
    /// Files and folders moved (a case-only rename counts once).
    pub moved: usize,
    pub copied: usize,
    /// Files moved to the trash folder, replaced files and clean-up included.
    pub trashed: usize,
    /// Steps that needed nothing because the disk already showed their result.
    pub already_applied: usize,
    /// Anything the item did not do or could not take back, in words.
    pub notes: Vec<String>,
    /// Folders removed because the rename left them empty.
    pub removed_folders: Vec<String>,
}

impl ItemReport {
    fn new(id: &str) -> Self {
        ItemReport {
            id: id.to_owned(),
            status: ItemStatus::Done,
            moved: 0,
            copied: 0,
            trashed: 0,
            already_applied: 0,
            notes: Vec::new(),
            removed_folders: Vec::new(),
        }
    }
}

/// What happened to a batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchReport {
    pub batch_id: String,
    pub items: Vec<ItemReport>,
}

impl BatchReport {
    pub fn done(&self) -> usize {
        self.count(|s| matches!(s, ItemStatus::Done))
    }

    pub fn failed(&self) -> usize {
        self.count(|s| matches!(s, ItemStatus::Failed(_)))
    }

    fn count(&self, f: impl Fn(&ItemStatus) -> bool) -> usize {
        self.items.iter().filter(|i| f(&i.status)).count()
    }
}

/// A step that did not work: the disk (the item fails and is rolled back) or the journal (the
/// whole batch stops, because nothing may happen unjournaled).
enum Fail {
    Op(String),
    Journal(JournalError),
}

impl From<JournalError> for Fail {
    fn from(e: JournalError) -> Self {
        Fail::Journal(e)
    }
}

type Step<T> = Result<T, Fail>;

type ProgressFn<'a> = Box<dyn FnMut(&ItemReport) + 'a>;

mod transfer;
pub use transfer::{FolderMove, TrashRequest};

enum Applied {
    Done,
    NothingToDo,
    Skipped(String),
}

/// The state of the item being executed.
struct Ctx {
    root: PathBuf,
    item_id: Option<String>,
    /// The journaled steps that were done, in order, for the roll-back.
    steps: Vec<JournalEntry>,
    report: ItemReport,
    /// A step left the item half done in a way the roll-back cannot see (a file left under a
    /// temporary name).
    left_half: bool,
}

fn io_fail(what: &str, path: &Path, e: &io::Error) -> Fail {
    Fail::Op(format!("{what} {}: {}", path.display(), plain_reason(e)))
}

/// [`io_fail`] for a folder: "access denied" on a folder means something has a file below it
/// open, which the sentence says, with what to do about it.
fn io_fail_folder(what: &str, path: &Path, e: &io::Error) -> Fail {
    Fail::Op(format!(
        "{what} {}: {}",
        path.display(),
        plain_folder_reason(e)
    ))
}

/// The sentence for a folder that cannot be renamed because a file below it is open.
pub(crate) const FOLDER_IN_USE: &str = "the folder is in use by another program (for example a \
     media player reading it over the network, or File Explorer). Close it and try again. If \
     nothing has it open, check that you may change this folder.";

/// [`plain_reason`] for a folder operation.
pub(crate) fn plain_folder_reason(e: &io::Error) -> String {
    if own_read(e).is_none()
        && (e.kind() == io::ErrorKind::PermissionDenied
            || matches!(e.raw_os_error(), Some(32 | 33)))
    {
        FOLDER_IN_USE.to_owned()
    } else {
        plain_reason(e)
    }
}

fn own_read(e: &io::Error) -> Option<&super::ReadByCineVault> {
    e.get_ref()
        .and_then(|inner| inner.downcast_ref::<super::ReadByCineVault>())
}

/// A sentence for the errors a person can fix (no room, no access, a file in use, CineVault's
/// own read still running); any other error keeps the system's own text.
pub(crate) fn plain_reason(e: &io::Error) -> String {
    let raw = e.raw_os_error();
    if let Some(read) = own_read(e) {
        let name = read.path.file_name().map_or_else(
            || read.path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        format!(
            "CineVault is still reading {name} (media information, a checksum or the aspect \
             ratio). Try again when that has finished."
        )
    } else if e.kind() == io::ErrorKind::StorageFull || matches!(raw, Some(112 | 28)) {
        "the destination is full".to_owned()
    } else if e.kind() == io::ErrorKind::PermissionDenied {
        "access was denied".to_owned()
    } else if e.kind() == io::ErrorKind::ResourceBusy || matches!(raw, Some(32 | 33)) {
        "the file is in use by another program".to_owned()
    } else {
        e.to_string()
    }
}

/// The executor. See the [module documentation](super).
pub struct Executor<'a> {
    /// The file system, every operation tried again while it fails for a moment.
    fs: Patient<'a>,
    journal: &'a mut dyn JournalStore,
    options: ExecOptions,
    unwanted: Vec<Regex>,
    cancel: Option<&'a dyn Fn() -> bool>,
    progress: Option<ProgressFn<'a>>,
    next_seq: u32,
}

impl<'a> Executor<'a> {
    /// # Errors
    /// [`ExecError::BadPattern`] when an unwanted-file pattern is not a valid expression, and a
    /// journal error when the batch already has rows that cannot be read.
    pub fn new(
        fs: &'a dyn Fs,
        journal: &'a mut dyn JournalStore,
        options: ExecOptions,
    ) -> Result<Self, ExecError> {
        let unwanted = options
            .unwanted_patterns
            .iter()
            .map(|p| compile_unwanted(p))
            .collect::<Result<Vec<_>, _>>()?;
        // A batch id used before (a resumed run) continues its numbering.
        let next_seq = journal
            .batch(&options.batch_id)?
            .iter()
            .map(|r| r.entry.seq + 1)
            .max()
            .unwrap_or(0);
        Ok(Executor {
            fs: Patient::new(fs).with_read_wait(options.own_read_wait),
            journal,
            options,
            unwanted,
            cancel: None,
            progress: None,
            next_seq,
        })
    }

    /// Stops the batch between items when `cancel` returns true. The item in progress finishes.
    #[must_use]
    pub fn with_cancel(mut self, cancel: &'a dyn Fn() -> bool) -> Self {
        self.cancel = Some(cancel);
        self
    }

    /// Calls `progress` after each item.
    #[must_use]
    pub fn with_progress(mut self, progress: impl FnMut(&ItemReport) + 'a) -> Self {
        self.progress = Some(Box::new(progress));
        self
    }

    /// Runs the batch. Items run one after the other, ordered so an item that moves into a path
    /// another item vacates runs after it. A failing item is rolled back and does not stop the
    /// others; the report holds the result of each, in the order of `items`.
    ///
    /// # Errors
    /// Only a journal that cannot be written, which stops the batch at once: no step runs
    /// without its row.
    pub fn run(&mut self, items: &[BatchItem]) -> Result<BatchReport, ExecError> {
        self.mark_items(items)?;
        let mut reports: Vec<Option<ItemReport>> = vec![None; items.len()];
        for index in order_items(items) {
            let item = &items[index];
            let canceled = self.cancel.is_some_and(|c| c());
            let report = if canceled {
                // Never started: it is as it was.
                self.journal
                    .close_item(&self.options.batch_id, &item.id, ItemState::RolledBack)?;
                let mut r = ItemReport::new(&item.id);
                r.status = ItemStatus::Canceled;
                r
            } else {
                self.run_item(item)?
            };
            if let Some(progress) = self.progress.as_mut() {
                progress(&report);
            }
            reports[index] = Some(report);
        }
        Ok(BatchReport {
            batch_id: self.options.batch_id.clone(),
            items: reports.into_iter().flatten().collect(),
        })
    }

    /// Marks every item `running` with its plan, in one write, before any of them runs: a stop
    /// anywhere in the batch then finds the items that never started too, so Resume finishes
    /// the whole batch and Activity counts it truthfully. [`run`](Self::run) does this itself;
    /// a caller that runs one batch in several executors (movies, then shows) marks them all
    /// first.
    ///
    /// # Errors
    /// The journal cannot be written.
    pub fn mark_items(&mut self, items: &[BatchItem]) -> Result<(), ExecError> {
        let marks: Vec<(String, String)> = items
            .iter()
            .map(|item| {
                let plan = ItemPlan::of(item, &self.options);
                (
                    item.id.clone(),
                    serde_json::to_string(&plan).unwrap_or_default(),
                )
            })
            .collect();
        self.journal.open_items(&self.options.batch_id, &marks)?;
        Ok(())
    }

    fn run_item(&mut self, item: &BatchItem) -> Result<ItemReport, ExecError> {
        let mut ctx = Ctx {
            root: PathBuf::from(&item.root),
            item_id: Some(item.id.clone()),
            steps: Vec::new(),
            report: ItemReport::new(&item.id),
            left_half: false,
        };
        // The item's mark goes first and is settled last: a stop between two of its steps, when
        // every row is settled, still shows the item as unfinished at the next start.
        let plan = serde_json::to_string(&ItemPlan::of(item, &self.options)).unwrap_or_default();
        let batch_id = self.options.batch_id.clone();
        self.journal.open_item(&batch_id, &item.id, &plan)?;
        let state = match self.execute_item(&mut ctx, item) {
            Ok(()) => ItemState::Done,
            // The mark stays `running`: recovery finds the item unfinished.
            Err(Fail::Journal(e)) => return Err(e.into()),
            Err(Fail::Op(message)) => {
                let notes = self.roll_back(&mut ctx)?;
                let whole = notes.is_empty() && !ctx.left_half;
                ctx.report.notes.extend(notes);
                ctx.report.status = ItemStatus::Failed(message);
                if whole {
                    ItemState::RolledBack
                } else {
                    ItemState::Unfinished
                }
            }
        };
        self.journal.close_item(&batch_id, &item.id, state)?;
        Ok(ctx.report)
    }

    fn execute_item(&mut self, ctx: &mut Ctx, item: &BatchItem) -> Step<()> {
        let ops = item.ops();
        validate_paths(item, &ops)?;
        if self
            .fs
            .meta(&ctx.root)
            .ok()
            .flatten()
            .is_none_or(|m| !m.is_dir)
        {
            return Err(Fail::Op(format!(
                "the data source {} is not available",
                ctx.root.display()
            )));
        }

        let mut ops = order_ops(ops);
        let old_abs = join(&ctx.root, &item.old_folder);
        let new_abs = join(&ctx.root, &item.new_folder);

        // The folder first: one rename moves every file in it. A folder that is (or holds) the
        // root of a data source is never renamed as a whole: its files move one by one.
        let whole_folder = item.folder_change == FolderChange::Rename
            && !item.old_folder.is_empty()
            && !item.new_folder.is_empty()
            && item.old_folder != item.new_folder
            && !self.holds_protected(&old_abs);
        let mut folder_renamed = false;
        if whole_folder && self.rename_folder(ctx, &old_abs, &new_abs)? {
            folder_renamed = true;
            ops = rewrite_into(ops, &item.old_folder, &item.new_folder);
        }

        // The folders the item may leave empty: those its files left, and the parents of a renamed
        // folder.
        let mut touched: Vec<PathBuf> = if folder_renamed {
            vec![old_abs.clone()]
        } else {
            Vec::new()
        };
        for op in &ops {
            if let Some(parent) = op
                .from
                .as_deref()
                .and_then(|f| join(&ctx.root, f).parent().map(Path::to_path_buf))
            {
                touched.push(parent);
            }
            self.apply_op(ctx, op)?;
        }

        self.clean_up(ctx, item, &old_abs, &new_abs, folder_renamed, touched);
        Ok(())
    }

    // ---- journaling -------------------------------------------------------------------

    fn entry(
        &mut self,
        ctx: &Ctx,
        op: JournalOp,
        from: &Path,
        to: &Path,
        is_dir: bool,
    ) -> JournalEntry {
        let seq = self.next_seq;
        self.next_seq += 1;
        JournalEntry {
            batch_id: self.options.batch_id.clone(),
            seq,
            item_id: ctx.item_id.clone(),
            op,
            from: from.to_string_lossy().into_owned(),
            to: to.to_string_lossy().into_owned(),
            is_dir,
            created_parents: self.missing_parents(to),
        }
    }

    /// How many folders above `to` do not exist yet, from its parent upwards: the folders the
    /// operation will create. `None` when the disk cannot tell.
    fn missing_parents(&self, to: &Path) -> Option<u32> {
        let mut count = 0;
        let mut dir = to.parent();
        while let Some(current) = dir {
            if current.as_os_str().is_empty() {
                break;
            }
            match self.fs.meta(current) {
                Ok(None) => count += 1,
                Ok(Some(_)) => break,
                Err(_) => return None,
            }
            dir = current.parent();
        }
        Some(count)
    }

    /// Settles an entry as done, reading the destination for the size and time undo checks.
    fn done(&mut self, ctx: &mut Ctx, entry: JournalEntry, dest: &Path) -> Step<()> {
        let meta = self.fs.meta(dest).ok().flatten();
        let completion = Completion::Done {
            size_bytes: meta.filter(|m| !m.is_dir).map(|m| m.len),
            modified_ms: meta.map(|m| m.modified_ms),
        };
        self.journal.complete(&entry, &completion)?;
        ctx.steps.push(entry);
        Ok(())
    }

    /// Settles an entry as failed and gives the failure back.
    fn failed(&mut self, entry: &JournalEntry, fail: Fail) -> Fail {
        let Fail::Op(message) = &fail else {
            return fail;
        };
        match self
            .journal
            .complete(entry, &Completion::Failed(message.clone()))
        {
            Ok(()) => fail,
            Err(e) => Fail::Journal(e),
        }
    }

    // ---- moving -----------------------------------------------------------------------

    /// Renames or copy-verify-deletes a path. No journal.
    fn raw_move(&self, from: &Path, to: &Path, is_dir: bool) -> io::Result<()> {
        move_path(&self.fs, from, to, is_dir)
    }

    fn copy_verified(&self, from: &Path, to: &Path) -> io::Result<()> {
        copy_verified(&self.fs, from, to)
    }

    /// Moves a file, journaled, replacing nothing: whatever is at the destination goes to the
    /// trash first.
    fn move_file(
        &mut self,
        ctx: &mut Ctx,
        from: &Path,
        to: &Path,
        file_type: MediaFileType,
    ) -> Step<Applied> {
        let src = self
            .fs
            .meta(from)
            .map_err(|e| io_fail("cannot read", from, &e))?;
        let dst = self
            .fs
            .meta(to)
            .map_err(|e| io_fail("cannot read", to, &e))?;
        let Some(src) = src else {
            return if dst.is_some() {
                Ok(Applied::NothingToDo)
            } else {
                Err(Fail::Op(format!("{} is missing", from.display())))
            };
        };
        if let Some(dst) = dst {
            if is_case_only(from, to)
                && !self.two_entries(from, to)
                && dst.len == src.len
                && dst.modified_ms == src.modified_ms
            {
                self.case_only_move(ctx, from, to, false)?;
                return Ok(Applied::Done);
            }
            if dst.is_dir {
                return Err(Fail::Op(format!("{} is a folder", to.display())));
            }
            if let Some(why) = self.protected_nfo(file_type, to)? {
                return Ok(Applied::Skipped(why));
            }
            self.trash(ctx, to)?;
        }
        let entry = self.entry(ctx, JournalOp::Move, from, to, false);
        self.journal.begin(&entry)?;
        match self.raw_move(from, to, false) {
            Ok(()) => self.done(ctx, entry, to)?,
            Err(e) => return Err(self.failed(&entry, io_fail("cannot move", from, &e))),
        }
        ctx.report.moved += 1;
        Ok(Applied::Done)
    }

    /// A rename that changes only the case of a name goes through a temporary name, because on a
    /// case-insensitive volume the destination is the source. Both journal rows are written
    /// before the first rename, so recovery can finish a rename that stopped in the middle.
    fn case_only_move(&mut self, ctx: &mut Ctx, from: &Path, to: &Path, is_dir: bool) -> Step<()> {
        let parent = from.parent().unwrap_or(Path::new(""));
        let first = self.entry(ctx, JournalOp::Move, from, Path::new(""), is_dir);
        let tmp = parent.join(format!("{CASE_TMP_PREFIX}{}", first.seq));
        let first = JournalEntry {
            to: tmp.to_string_lossy().into_owned(),
            ..first
        };
        let second = self.entry(ctx, JournalOp::Move, &tmp, to, is_dir);
        self.journal.begin(&first)?;
        self.journal.begin(&second)?;

        let fail_of = if is_dir { io_fail_folder } else { io_fail };
        if let Err(e) = self.fs.rename(from, &tmp) {
            let fail = self.failed(&first, fail_of("cannot rename", from, &e));
            let _ = self.failed(&second, Fail::Op("the first rename failed".into()));
            return Err(fail);
        }
        self.done(ctx, first, &tmp)?;
        if let Err(e) = self.fs.rename(&tmp, to) {
            let fail = self.failed(&second, fail_of("cannot rename", &tmp, &e));
            // Put the original name back. The first row is then taken back already, so the
            // roll-back of the item must not try it again (and the journal must not say the file
            // is under the temporary name).
            match self.fs.rename(&tmp, from) {
                Ok(()) => {
                    if let Some(first) = ctx.steps.pop() {
                        self.journal.complete(&first, &Completion::Undone)?;
                    }
                }
                Err(e) => {
                    ctx.left_half = true;
                    ctx.report.notes.push(format!(
                        "{} is left under a temporary name: {e}",
                        tmp.display()
                    ));
                }
            }
            return Err(fail);
        }
        self.done(ctx, second, to)?;
        ctx.report.moved += 1;
        Ok(())
    }

    /// True when `from` and `to`, which differ in case (or normalisation) only, are two entries of
    /// the disk: their folders list both spellings. On a case-insensitive volume they are one
    /// entry (the folder lists one spelling only, the old one or, once renamed, the new one), and
    /// a rename through a temporary name changes its case. On a case-sensitive volume they can be
    /// two files, and that rename would overwrite the second one.
    fn two_entries(&self, from: &Path, to: &Path) -> bool {
        self.lists_exactly(from) && self.lists_exactly(to)
    }

    /// True when the folder of `path` lists an entry with exactly its name.
    fn lists_exactly(&self, path: &Path) -> bool {
        lists_exactly(&self.fs, path)
    }

    /// Renames the item's folder as a whole. `Ok(false)` means it has to be done file by file:
    /// the destination is a folder with content, or the two are on different volumes.
    fn rename_folder(&mut self, ctx: &mut Ctx, from: &Path, to: &Path) -> Step<bool> {
        let src = self
            .fs
            .meta(from)
            .map_err(|e| io_fail("cannot read", from, &e))?;
        let dst = self
            .fs
            .meta(to)
            .map_err(|e| io_fail("cannot read", to, &e))?;
        let Some(src) = src else {
            return if dst.is_some_and(|d| d.is_dir) {
                ctx.report.already_applied += 1;
                Ok(true)
            } else {
                Err(Fail::Op(format!(
                    "the folder {} is missing",
                    from.display()
                )))
            };
        };
        if !src.is_dir {
            return Err(Fail::Op(format!("{} is not a folder", from.display())));
        }
        if let Some(dst) = dst {
            if is_case_only(from, to)
                && !self.two_entries(from, to)
                && dst.modified_ms == src.modified_ms
            {
                self.case_only_move(ctx, from, to, true)?;
                return Ok(true);
            }
            if !dst.is_dir {
                return Err(Fail::Op(format!("{} is a file", to.display())));
            }
            let names = self
                .fs
                .list_dir(to)
                .map_err(|e| io_fail("cannot list", to, &e))?;
            if !names.is_empty() {
                return Ok(false);
            }
            // An empty folder is in the way of nothing.
            self.fs
                .remove_dir(to)
                .map_err(|e| io_fail("cannot replace", to, &e))?;
        }
        let entry = self.entry(ctx, JournalOp::Move, from, to, true);
        self.journal.begin(&entry)?;
        let result = match to.parent() {
            Some(parent) => self
                .fs
                .create_dir_all(parent)
                .and_then(|()| self.fs.rename(from, to)),
            None => self.fs.rename(from, to),
        };
        match result {
            Ok(()) => {
                self.done(ctx, entry, to)?;
                ctx.report.moved += 1;
                Ok(true)
            }
            Err(e) if e.kind() == io::ErrorKind::CrossesDevices => {
                let fail = Fail::Op("the folder is on another volume; moved file by file".into());
                match self.failed(&entry, fail) {
                    Fail::Journal(e) => Err(Fail::Journal(e)),
                    Fail::Op(_) => Ok(false),
                }
            }
            Err(e) => Err(self.failed(&entry, io_fail_folder("cannot rename", from, &e))),
        }
    }

    fn copy_file(
        &mut self,
        ctx: &mut Ctx,
        from: &Path,
        to: &Path,
        file_type: MediaFileType,
    ) -> Step<Applied> {
        let src = self
            .fs
            .meta(from)
            .map_err(|e| io_fail("cannot read", from, &e))?;
        if src.is_none() {
            return Err(Fail::Op(format!("{} is missing", from.display())));
        }
        let dst = self
            .fs
            .meta(to)
            .map_err(|e| io_fail("cannot read", to, &e))?;
        if let Some(dst) = dst {
            if dst.is_dir {
                return Err(Fail::Op(format!("{} is a folder", to.display())));
            }
            if self.fs.same_content(from, to).unwrap_or(false) {
                return Ok(Applied::NothingToDo);
            }
            if let Some(why) = self.protected_nfo(file_type, to)? {
                return Ok(Applied::Skipped(why));
            }
            self.trash(ctx, to)?;
        }
        let entry = self.entry(ctx, JournalOp::Copy, from, to, false);
        self.journal.begin(&entry)?;
        match self.copy_verified(from, to) {
            Ok(()) => self.done(ctx, entry, to)?,
            Err(e) => return Err(self.failed(&entry, io_fail("cannot copy", from, &e))),
        }
        ctx.report.copied += 1;
        Ok(Applied::Done)
    }

    /// Why an NFO at `path` must not be touched: it is not one CineVault wrote.
    fn protected_nfo(&self, file_type: MediaFileType, path: &Path) -> Step<Option<String>> {
        if file_type != MediaFileType::Nfo {
            return Ok(None);
        }
        let ownership = classify_nfo_file(path).map_err(|e| io_fail("cannot read", path, &e))?;
        Ok((!ownership.may_overwrite()).then(|| {
            format!(
                "{} is an NFO CineVault did not write; left alone",
                path.display()
            )
        }))
    }

    /// Moves a file into `<root>/<trash>/<batch>/<its path below the root>`, journaled.
    fn trash(&mut self, ctx: &mut Ctx, path: &Path) -> Step<()> {
        let rel = path
            .strip_prefix(&ctx.root)
            .map_err(|_| Fail::Op(format!("{} is outside the data source", path.display())))?;
        let base = trash_target(
            &ctx.root,
            &self.options.trash_dir,
            &self.options.batch_id,
            path,
            rel,
        );
        let dest = self.unused_name(&base)?;
        let entry = self.entry(ctx, JournalOp::Trash, path, &dest, false);
        self.journal.begin(&entry)?;
        match self.raw_move(path, &dest, false) {
            Ok(()) => self.done(ctx, entry, &dest)?,
            Err(e) => return Err(self.failed(&entry, io_fail("cannot trash", path, &e))),
        }
        ctx.report.trashed += 1;
        Ok(())
    }

    /// `path`, or a numbered name when it is taken.
    fn unused_name(&self, path: &Path) -> Step<PathBuf> {
        unused_path(&self.fs, path).map_err(|e| io_fail("cannot read", path, &e))
    }

    fn apply_op(&mut self, ctx: &mut Ctx, op: &Op) -> Step<()> {
        let from = op.from.as_deref().map(|p| join(&ctx.root, p));
        let to = op.to.as_deref().map(|p| join(&ctx.root, p));
        let applied = match (op.kind, from, to) {
            (OpKind::Move, Some(from), Some(to)) => {
                self.move_file(ctx, &from, &to, op.file_type)?
            }
            (OpKind::Copy, Some(from), Some(to)) => {
                self.copy_file(ctx, &from, &to, op.file_type)?
            }
            (OpKind::Trash, Some(from), _) => self.trash_planned(ctx, &from, op.file_type)?,
            (OpKind::WriteNfo, _, Some(to)) => Applied::Skipped(format!(
                "{} is written by the NFO service, not by the renamer",
                to.display()
            )),
            _ => Applied::Skipped("the operation has no path".into()),
        };
        match applied {
            Applied::Done => {}
            Applied::NothingToDo => ctx.report.already_applied += 1,
            Applied::Skipped(why) => ctx.report.notes.push(why),
        }
        Ok(())
    }

    /// A file the plan no longer needs. An NFO goes only when CineVault wrote it.
    fn trash_planned(
        &mut self,
        ctx: &mut Ctx,
        path: &Path,
        file_type: MediaFileType,
    ) -> Step<Applied> {
        if self
            .fs
            .meta(path)
            .map_err(|e| io_fail("cannot read", path, &e))?
            .is_none()
        {
            return Ok(Applied::NothingToDo);
        }
        if file_type == MediaFileType::Nfo {
            let ownership =
                classify_nfo_file(path).map_err(|e| io_fail("cannot read", path, &e))?;
            if !ownership.is_managed() {
                return Ok(Applied::Skipped(format!(
                    "{} is not an NFO CineVault wrote; left in place",
                    path.display()
                )));
            }
        }
        self.trash(ctx, path)?;
        Ok(Applied::Done)
    }

    // ---- taking back --------------------------------------------------------------------

    /// Takes back the steps of a failed item, newest first. Returns what could not be taken
    /// back.
    fn roll_back(&mut self, ctx: &mut Ctx) -> Result<Vec<String>, ExecError> {
        let mut notes = Vec::new();
        let steps: Vec<JournalEntry> = ctx.steps.drain(..).rev().collect();
        for step in steps {
            let (from, to) = (PathBuf::from(&step.from), PathBuf::from(&step.to));
            let result = match step.op {
                JournalOp::Move | JournalOp::Trash => self.raw_move(&to, &from, step.is_dir),
                JournalOp::Copy => match self.trash(ctx, &to) {
                    Ok(()) => {
                        // The clean-up row is part of the roll-back, not of the batch: it must
                        // not be offered to an undo.
                        if let Some(cleanup) = ctx.steps.pop() {
                            self.journal.complete(&cleanup, &Completion::Undone)?;
                        }
                        Ok(())
                    }
                    Err(Fail::Journal(e)) => return Err(e.into()),
                    Err(Fail::Op(m)) => Err(io::Error::other(m)),
                },
                JournalOp::WriteNfo => Ok(()),
            };
            match result {
                Ok(()) => self.journal.complete(&step, &Completion::Undone)?,
                Err(e) => notes.push(format!("could not take back {}: {e}", step.from)),
            }
        }
        Ok(notes)
    }

    // ---- clean-up ----------------------------------------------------------------------

    /// Unwanted files and empty folders. Nothing here fails the item: what cannot be cleaned
    /// is noted and left.
    fn clean_up(
        &mut self,
        ctx: &mut Ctx,
        item: &BatchItem,
        old_abs: &Path,
        new_abs: &Path,
        folder_renamed: bool,
        mut touched: Vec<PathBuf>,
    ) {
        if let Err(Fail::Op(message)) =
            self.trash_unwanted(ctx, item, old_abs, new_abs, folder_renamed)
        {
            ctx.report.notes.push(message);
        }
        if !self.options.remove_empty_folders {
            return;
        }
        touched.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
        touched.dedup();
        for dir in touched {
            self.remove_empty_chain(ctx, &dir, folder_renamed.then_some(new_abs));
        }
    }

    fn trash_unwanted(
        &mut self,
        ctx: &mut Ctx,
        item: &BatchItem,
        old_abs: &Path,
        new_abs: &Path,
        folder_renamed: bool,
    ) -> Step<()> {
        if self.unwanted.is_empty() || item.old_folder.is_empty() || item.new_folder.is_empty() {
            return Ok(());
        }
        let protected: Vec<PathBuf> = item
            .ops()
            .iter()
            .filter_map(|op| op.to.as_deref().map(|p| join(&ctx.root, p)))
            .collect();
        // The folder the item now owns: its top-level files only.
        if folder_renamed {
            for name in self.unwanted_in(new_abs)? {
                let path = new_abs.join(name);
                if !protected.iter().any(|p| same_path(p, &path)) {
                    self.trash(ctx, &path)?;
                }
            }
        }
        // The folder the item left: only when nothing but unwanted files is in it.
        if !same_path(old_abs, new_abs) && !folder_renamed {
            let all = match self.fs.list_dir(old_abs) {
                Ok(names) => names,
                Err(_) => return Ok(()),
            };
            let unwanted = self.unwanted_in(old_abs)?;
            if !all.is_empty() && unwanted.len() == all.len() {
                for name in unwanted {
                    self.trash(ctx, &old_abs.join(name))?;
                }
            }
        }
        Ok(())
    }

    /// The names of the files in `dir` (not folders) that match an unwanted pattern.
    fn unwanted_in(&self, dir: &Path) -> Step<Vec<String>> {
        let Ok(names) = self.fs.list_dir(dir) else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for name in names {
            let is_file = self
                .fs
                .meta(&dir.join(&name))
                .map_err(|e| io_fail("cannot read", dir, &e))?
                .is_some_and(|m| !m.is_dir);
            if is_file && self.unwanted.iter().any(|re| re.is_match(&name)) {
                out.push(name);
            }
        }
        Ok(out)
    }

    /// Whether `dir` is a protected folder or lies above one.
    fn holds_protected(&self, dir: &Path) -> bool {
        self.options
            .protected_dirs
            .iter()
            .any(|protected| is_within(dir, protected))
    }

    /// Removes `dir` when it is empty, then its parent, and so on up to (not including) the data
    /// source root or another protected folder. Stops at the first folder with anything in it.
    fn remove_empty_chain(&mut self, ctx: &mut Ctx, dir: &Path, keep: Option<&Path>) {
        let trash_root = ctx.root.join(&self.options.trash_dir);
        let own_trash = ctx.root.join(TRASH_DIR);
        let mut dir = dir.to_path_buf();
        loop {
            if !is_within(&ctx.root, &dir)
                || same_path(&ctx.root, &dir)
                || is_within(&trash_root, &dir)
                || is_within(&own_trash, &dir)
                || keep.is_some_and(|k| same_path(k, &dir))
                || self.holds_protected(&dir)
            {
                return;
            }
            match self.fs.meta(&dir) {
                Ok(None) => {}
                Ok(Some(m)) if m.is_dir => match self.fs.list_dir(&dir) {
                    Ok(names) if names.is_empty() => {
                        if self.fs.remove_dir(&dir).is_err() {
                            return;
                        }
                        ctx.report
                            .removed_folders
                            .push(dir.to_string_lossy().into_owned());
                    }
                    _ => return,
                },
                _ => return,
            }
            let Some(parent) = dir.parent() else { return };
            dir = parent.to_path_buf();
        }
    }
}

/// Renames a path, creating the parent of the destination. A file on another volume is copied
/// next to the destination under a part name, compared byte for byte with the original, renamed
/// into place, and only then is the original deleted: a crash at any point leaves the original
/// whole. A folder on another volume is copied as a tree the same way (see [`super::tree`]); the
/// files of the original that cannot be removed afterwards stay where they are. No journal.
pub(crate) fn move_path(fs: &dyn Fs, from: &Path, to: &Path, is_dir: bool) -> io::Result<()> {
    if let Some(parent) = to.parent() {
        fs.create_dir_all(parent)?;
    }
    match fs.rename(from, to) {
        Err(e) if e.kind() == io::ErrorKind::CrossesDevices && is_dir => {
            super::tree::move_tree(fs, from, to).map(|_| ())
        }
        Err(e) if e.kind() == io::ErrorKind::CrossesDevices && !is_dir => {
            copy_verified(fs, from, to)?;
            if let Err(e) = fs.remove_file(from) {
                // Keep one copy only, and keep the original.
                let _ = fs.remove_file(to);
                return Err(e);
            }
            Ok(())
        }
        other => other,
    }
}

fn copy_verified(fs: &dyn Fs, from: &Path, to: &Path) -> io::Result<()> {
    if let Some(parent) = to.parent() {
        fs.create_dir_all(parent)?;
    }
    let part = part_path(to);
    if fs.meta(&part)?.is_some() {
        fs.remove_file(&part)?;
    }
    fs.copy_file(from, &part)?;
    if !fs.same_content(from, &part)? {
        let _ = fs.remove_file(&part);
        return Err(io::Error::other("the copy does not match the original"));
    }
    fs.rename(&part, to)
}

/// `path`, or `name (2).ext`, `name (3).ext` ... when it is taken.
pub(crate) fn unused_path(fs: &dyn Fs, path: &Path) -> io::Result<PathBuf> {
    if fs.meta(path)?.is_none() {
        return Ok(path.to_path_buf());
    }
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    for n in 2.. {
        let candidate = path.with_file_name(format!("{stem} ({n}){ext}"));
        if fs.meta(&candidate)?.is_none() {
            return Ok(candidate);
        }
    }
    unreachable!("the counter does not end")
}

/// The names of the files (not folders) in `dir` that match one of the unwanted-file patterns:
/// an extension such as `.txt` or a regular expression on the name, ignoring case. A folder that
/// cannot be listed has none. This is the matching the clean-up after a rename uses, for the
/// standalone clean-up of a folder.
///
/// # Errors
/// [`ExecError::BadPattern`] when a pattern is neither an extension nor a valid expression.
pub fn unwanted_files_in(
    fs: &dyn Fs,
    dir: &Path,
    patterns: &[String],
) -> Result<Vec<String>, ExecError> {
    let patterns = patterns
        .iter()
        .map(|p| compile_unwanted(p))
        .collect::<Result<Vec<_>, _>>()?;
    if patterns.is_empty() {
        return Ok(Vec::new());
    }
    let Ok(mut names) = fs.list_dir(dir) else {
        return Ok(Vec::new());
    };
    names.sort();
    Ok(names
        .into_iter()
        .filter(|name| patterns.iter().any(|re| re.is_match(name)))
        .filter(|name| {
            fs.meta(&dir.join(name))
                .ok()
                .flatten()
                .is_some_and(|meta| !meta.is_dir)
        })
        .collect())
}

fn compile_unwanted(pattern: &str) -> Result<Regex, ExecError> {
    let is_extension = pattern.len() > 1
        && pattern.starts_with('.')
        && pattern[1..].chars().all(|c| c.is_ascii_alphanumeric());
    let source = if is_extension {
        format!("(?i){}$", regex::escape(pattern))
    } else {
        format!("(?i){pattern}")
    };
    Regex::new(&source).map_err(|e| ExecError::BadPattern {
        pattern: pattern.to_owned(),
        reason: e.to_string(),
    })
}

/// `a/b/c` below `root`, with the platform separator.
fn join(root: &Path, rel: &str) -> PathBuf {
    let mut path = root.to_path_buf();
    for part in rel.split('/').filter(|p| !p.is_empty()) {
        path.push(part);
    }
    path
}

/// The two paths spell one name two ways: they differ only in case, or only in Unicode
/// normalisation (the decomposed `e` + U+0301 a Mac writes and the composed `é` a template
/// gives). A volume that ignores case (NTFS, APFS) or normalisation (APFS, HFS+) takes them for
/// one entry, so the rename goes through a temporary name. Whether they are one entry on this
/// volume is judged on the disk (`two_entries`, sizes and times) before that.
pub(crate) fn is_case_only(a: &Path, b: &Path) -> bool {
    a != b && folded_name(a) == folded_name(b)
}

/// True when the folder of `path` lists an entry with exactly its name (not one the volume only
/// finds through folding case or normalisation).
pub(crate) fn lists_exactly(fs: &dyn Fs, path: &Path) -> bool {
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return false;
    };
    let name = name.to_string_lossy();
    fs.list_dir(parent)
        .is_ok_and(|names| names.iter().any(|n| *n == name))
}

/// The path composed (NFC) and in lower case.
fn folded_name(path: &Path) -> String {
    icu_normalizer::ComposingNormalizerBorrowed::new_nfc()
        .normalize(&path.to_string_lossy())
        .to_lowercase()
}

/// `<name>.cvpart`: where a copy is written until it is verified.
pub(crate) fn part_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(PART_SUFFIX);
    path.with_file_name(name)
}

/// A plan path must stay below the data source root.
fn validate_paths(item: &BatchItem, ops: &[Op]) -> Step<()> {
    let mut paths: Vec<&str> = vec![&item.old_folder, &item.new_folder];
    for op in ops {
        paths.extend(op.from.as_deref());
        paths.extend(op.to.as_deref());
    }
    for path in paths {
        let bad = path.starts_with('/')
            || path.starts_with('\\')
            || path.contains('\\')
            || Path::new(path).components().any(|c| match c {
                Component::Normal(name) => name.to_string_lossy().contains(':'),
                Component::CurDir | Component::Prefix(_) | Component::RootDir => true,
                Component::ParentDir => true,
            });
        if bad {
            return Err(Fail::Op(format!(
                "the path `{path}` is not inside the data source"
            )));
        }
    }
    Ok(())
}

/// `from` paths below `old` now live below `new`; an op that then moves a file onto itself is
/// dropped.
fn rewrite_into(ops: Vec<Op>, old: &str, new: &str) -> Vec<Op> {
    let prefix = format!("{old}/");
    ops.into_iter()
        .map(|mut op| {
            if let Some(from) = &op.from
                && let Some(rest) = from.strip_prefix(&prefix)
            {
                op.from = Some(format!("{new}/{rest}"));
            }
            op
        })
        .filter(|op| !(op.kind == OpKind::Move && op.from == op.to))
        .collect()
}

/// Orders the operations of one item: a file that is another operation's destination is moved
/// away first, and copies are made before their source moves or goes to the trash. Operations
/// otherwise keep their order. A cycle (a swap) keeps the planned order for what is left; the
/// replaced file then goes to the trash and the journal shows it.
pub(crate) fn order_ops(ops: Vec<Op>) -> Vec<Op> {
    let n = ops.len();
    let vacates = |op: &Op| matches!(op.kind, OpKind::Move | OpKind::Trash);
    let writes = |op: &Op| matches!(op.kind, OpKind::Move | OpKind::Copy);
    // before[j] lists the operations that must come before j.
    let mut before: Vec<Vec<usize>> = vec![Vec::new(); n];
    for i in 0..n {
        for j in 0..n {
            if i == j {
                continue;
            }
            let (a, b) = (&ops[i], &ops[j]);
            let vacated_for_b = vacates(a) && writes(b) && a.from.is_some() && a.from == b.to;
            let copy_first = a.kind == OpKind::Copy
                && matches!(b.kind, OpKind::Move | OpKind::Trash)
                && a.from.is_some()
                && a.from == b.from;
            if vacated_for_b || copy_first {
                before[j].push(i);
            }
        }
    }
    let mut emitted = vec![false; n];
    let mut order = Vec::with_capacity(n);
    while order.len() < n {
        let next = (0..n)
            .find(|&j| !emitted[j] && before[j].iter().all(|&i| emitted[i]))
            .or_else(|| (0..n).find(|&j| !emitted[j]))
            .expect("an operation is left");
        emitted[next] = true;
        order.push(next);
    }
    let mut slots: Vec<Option<Op>> = ops.into_iter().map(Some).collect();
    order.into_iter().filter_map(|i| slots[i].take()).collect()
}

/// Orders the items of a batch: an item that moves into a path another item leaves runs after
/// that item. Returns indices into `items`. Items otherwise keep their order; a cycle keeps the
/// given order.
pub(crate) fn order_items(items: &[BatchItem]) -> Vec<usize> {
    struct Vacated {
        key: String,
        /// A renamed folder: everything below it is vacated too.
        folder: bool,
    }
    let key = |item: &BatchItem, path: &str| format!("{}\u{0}{path}", item.root).to_lowercase();
    let vacated: Vec<Vec<Vacated>> = items
        .iter()
        .map(|item| {
            let mut v: Vec<Vacated> = item
                .ops()
                .iter()
                .filter(|op| matches!(op.kind, OpKind::Move | OpKind::Trash))
                .filter_map(|op| op.from.as_deref())
                .map(|p| Vacated {
                    key: key(item, p),
                    folder: false,
                })
                .collect();
            if item.folder_change == FolderChange::Rename
                && !item.old_folder.is_empty()
                && item.old_folder != item.new_folder
            {
                v.push(Vacated {
                    key: key(item, &item.old_folder),
                    folder: true,
                });
            }
            v
        })
        .collect();
    let claimed: Vec<Vec<String>> = items
        .iter()
        .map(|item| {
            let mut c: Vec<String> = item
                .ops()
                .iter()
                .filter(|op| matches!(op.kind, OpKind::Move | OpKind::Copy))
                .filter_map(|op| op.to.as_deref())
                .map(|p| key(item, p))
                .collect();
            if !item.new_folder.is_empty() && item.old_folder != item.new_folder {
                c.push(key(item, &item.new_folder));
            }
            c
        })
        .collect();

    let n = items.len();
    let depends = |a: usize, b: usize| {
        claimed[a].iter().any(|c| {
            vacated[b]
                .iter()
                .any(|v| *c == v.key || (v.folder && c.starts_with(&format!("{}/", v.key))))
        })
    };
    let mut emitted = vec![false; n];
    let mut order = Vec::with_capacity(n);
    while order.len() < n {
        let next = (0..n)
            .find(|&a| !emitted[a] && (0..n).all(|b| b == a || emitted[b] || !depends(a, b)))
            .or_else(|| (0..n).find(|&a| !emitted[a]))
            .expect("an item is left");
        emitted[next] = true;
        order.push(next);
    }
    order
}

#[cfg(test)]
mod plain_reason_tests {
    use super::*;

    #[test]
    fn the_errors_a_person_can_fix_get_plain_sentences() {
        let full = io::Error::from(io::ErrorKind::StorageFull);
        let denied = io::Error::from(io::ErrorKind::PermissionDenied);
        let busy = io::Error::from_raw_os_error(if cfg!(windows) { 32 } else { 16 });
        assert_eq!(plain_reason(&full), "the destination is full");
        assert_eq!(plain_reason(&denied), "access was denied");
        assert_eq!(plain_reason(&busy), "the file is in use by another program");
        assert_eq!(plain_reason(&io::Error::other("odd")), "odd");
    }

    #[test]
    fn a_folder_that_is_held_says_what_holds_it_and_what_to_do() {
        let denied = io::Error::from(io::ErrorKind::PermissionDenied);
        assert_eq!(plain_folder_reason(&denied), FOLDER_IN_USE);
        assert!(FOLDER_IN_USE.contains("media player reading it over the network"));
        let full = io::Error::from(io::ErrorKind::StorageFull);
        assert_eq!(plain_folder_reason(&full), "the destination is full");
        // CineVault's own read is named as such, not blamed on another program.
        let own = io::Error::other(crate::exec::ReadByCineVault {
            path: PathBuf::from("Heat")
                .join("BDMV")
                .join("STREAM")
                .join("00800.m2ts"),
        });
        for reason in [plain_reason(&own), plain_folder_reason(&own)] {
            assert!(
                reason.starts_with("CineVault is still reading 00800.m2ts"),
                "{reason}"
            );
        }
    }
}
