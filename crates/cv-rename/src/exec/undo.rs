//! Undo: takes a journaled batch back, newest operation first, and says what it could
//! not restore and why.
//!
//! [`undo_batch`] reads the `done` rows of one batch and replays them in reverse.
//! It works **item by item**, and an item is restored whole or not at all:
//!
//! 1. **Verify.** Every `done` row of the item is checked against the disk without touching it.
//!    The destination of a row (where the operation left the file) must still be there and
//!    unchanged: same size and modification time as the journal recorded. A folder is checked for
//!    existence only (adding a file to it changes its time, and its files have rows of their own).
//!    The original location must be free. Rows are checked in the order the undo will run them,
//!    so a file that a later row of the batch moves on is trusted, and a path a later row frees
//!    counts as free.
//! 2. **Restore.** Only when every row passed, the item is taken back: a move or a trash goes
//!    back to its original path (the parent folders are created again), and a copy or a written
//!    NFO goes to `<data source>/.deletedByCineVault/<batch>-undo/` (nothing is deleted for good).
//!    Each row becomes `undone` as it is taken back, so the library paths follow in the same
//!    transaction with the `DbJournal`.
//! 3. **Clean up.** Folders the batch created and the undo emptied are removed as soon as a row is
//!    taken back (before a folder rename of the item is taken back and carries them along), never
//!    above the common parent of a row's two paths, never a data source root, never the trash and
//!    never a folder a later row moves back as a whole. The journal records how many folders
//!    each operation created, so a folder that was there before the batch (an empty `Season 01`
//!    the files were moved into) stays. A batch journaled before that was recorded removes the
//!    empty folders below the common parent, as before.
//!
//! An item with a row that fails verification is left exactly as the organize left it and is
//! reported [`UndoStatus::NotRestorable`] with one [`NotRestored`] per problem. The other items are
//! restored (a *partial undo*). If the disk fails in the middle of an item, the rows already taken
//! back are put forward again and the item is reported [`UndoStatus::Failed`].
//!
//! The undo is safe to run again: a row whose destination is gone while its original is back,
//! unchanged, counts as restored (a crash between the move and the journal), and a batch whose
//! rows are all `undone` gives an empty report. Rows an interrupted organize left `planned` must be
//! settled by [`recover`](super::recover()) first.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use super::TRASH_DIR;
use super::fs::{Fs, Meta, Patient, comparable, is_within};
use super::journal::{
    Completion, JournalEntry, JournalError, JournalOp, JournalRow, JournalState, JournalStore,
};
use super::run::{is_case_only, lists_exactly, move_path, unused_path};

/// How an undo behaves.
#[derive(Debug, Clone)]
pub struct UndoOptions {
    /// The trash folder in the root of each data source (where undone copies go).
    pub trash_dir: String,
    /// The data source roots. Used to find the trash of an undone copy and to stop the removal of
    /// folders the undo emptied; without them the trash is found through the batch's own trash
    /// rows, and a copy of a batch with none cannot be undone.
    pub roots: Vec<PathBuf>,
    /// Remove the folders the batch created and the undo left empty.
    pub remove_empty_folders: bool,
    /// Take back only these items of the batch (by the id the executor was given); `None` takes
    /// back every item. A roll-back of an unfinished batch takes back only the items the stop
    /// left half done, as a roll-back of a move does.
    pub only_items: Option<Vec<String>>,
}

impl Default for UndoOptions {
    fn default() -> Self {
        UndoOptions {
            trash_dir: TRASH_DIR.to_owned(),
            roots: Vec::new(),
            remove_empty_folders: true,
            only_items: None,
        }
    }
}

/// Why an undo did not run.
#[derive(Debug, thiserror::Error)]
pub enum UndoError {
    #[error(transparent)]
    Journal(#[from] JournalError),
    /// The batch has rows a crash left `planned`; [`recover`](super::recover()) settles them.
    #[error("batch {batch_id} has {rows} unsettled journal rows; run the start-up recovery first")]
    Unsettled { batch_id: String, rows: usize },
}

/// A path an undo could not restore, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotRestored {
    /// Where the operation had left the file, or the original location when that is the problem.
    pub path: String,
    pub reason: String,
}

/// How one item ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UndoStatus {
    /// Every operation of the item was taken back.
    Restored,
    /// Nothing of the item was touched: at least one operation could not be verified.
    NotRestorable(Vec<NotRestored>),
    /// The disk failed in the middle; the item is back as the organize left it. The text says why.
    Failed(String),
}

/// What happened to one item of the batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoItemReport {
    /// The library item id; none for rows whose item is gone from the library.
    pub id: Option<String>,
    pub status: UndoStatus,
    /// Files and folders moved back to their original path.
    pub restored: usize,
    /// Copies and written files moved to the trash folder.
    pub discarded: usize,
    /// Rows that needed nothing because the disk already showed the original state.
    pub already_restored: usize,
    /// Folders removed because the undo left them empty.
    pub removed_folders: Vec<String>,
    /// Anything else worth telling.
    pub notes: Vec<String>,
}

/// What an undo did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoReport {
    pub batch_id: String,
    /// One per item with rows to take back, in the order the undo ran (newest first). Empty when
    /// the batch has nothing left to undo.
    pub items: Vec<UndoItemReport>,
}

impl UndoReport {
    pub fn restored(&self) -> usize {
        self.count(|s| matches!(s, UndoStatus::Restored))
    }

    pub fn not_restorable(&self) -> usize {
        self.count(|s| matches!(s, UndoStatus::NotRestorable(_)))
    }

    pub fn failed(&self) -> usize {
        self.count(|s| matches!(s, UndoStatus::Failed(_)))
    }

    /// Every item was restored (also true when there was nothing to undo).
    pub fn is_complete(&self) -> bool {
        self.restored() == self.items.len()
    }

    /// Every problem of every item that could not be restored, with the item id.
    pub fn problems(&self) -> impl Iterator<Item = (&Option<String>, &NotRestored)> {
        self.items.iter().flat_map(|item| match &item.status {
            UndoStatus::NotRestorable(list) => list.iter().map(|p| (&item.id, p)).collect(),
            _ => Vec::new(),
        })
    }

    fn count(&self, f: impl Fn(&UndoStatus) -> bool) -> usize {
        self.items.iter().filter(|i| f(&i.status)).count()
    }
}

/// What the undo does with one row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    /// Move the destination back to the original path.
    Restore { is_dir: bool },
    /// The original is back already; only the journal needs to follow.
    AlreadyRestored { is_dir: bool },
    /// A copy or a written file: move it to the trash.
    Discard,
    /// A copy or a written file that is gone already.
    Gone,
}

struct Planned {
    row: JournalRow,
    action: Action,
}

/// Takes the `done` rows of `batch_id` back, newest first. See the [module](self) for the rules.
///
/// # Errors
/// A journal that cannot be read or written, or a batch with `planned` rows. A path that cannot be
/// restored is no error: it is in the report.
pub fn undo_batch(
    journal: &mut dyn JournalStore,
    fs: &dyn Fs,
    batch_id: &str,
    options: &UndoOptions,
) -> Result<UndoReport, UndoError> {
    // Every operation is tried again while another program holds a handle for a moment.
    let patient = Patient::new(fs);
    let fs: &dyn Fs = &patient;
    let rows = journal.batch(batch_id)?;
    let unsettled = rows
        .iter()
        .filter(|r| r.state == JournalState::Planned)
        .count();
    if unsettled > 0 {
        return Err(UndoError::Unsettled {
            batch_id: batch_id.to_owned(),
            rows: unsettled,
        });
    }
    let mut report = UndoReport {
        batch_id: batch_id.to_owned(),
        items: Vec::new(),
    };
    let undoer = Undoer {
        fs,
        options,
        batch_id,
        trash_hint: trash_hint(&rows, &options.trash_dir),
        created: created_folders(&rows),
    };
    for group in groups(rows) {
        if let Some(only) = &options.only_items {
            let id = group[0].entry.item_id.as_deref();
            if !id.is_some_and(|id| only.iter().any(|o| o == id)) {
                continue;
            }
        }
        report.items.push(undoer.item(journal, group)?);
    }
    Ok(report)
}

/// The `done` rows of each item, newest row first; the item with the newest row first. Rows
/// without an item are one group each.
fn groups(rows: Vec<JournalRow>) -> Vec<Vec<JournalRow>> {
    let mut by_item: BTreeMap<String, Vec<JournalRow>> = BTreeMap::new();
    let mut loose: Vec<Vec<JournalRow>> = Vec::new();
    for row in rows.into_iter().filter(|r| r.state == JournalState::Done) {
        match row.entry.item_id.clone() {
            Some(id) => by_item.entry(id).or_default().push(row),
            None => loose.push(vec![row]),
        }
    }
    let mut all: Vec<Vec<JournalRow>> = by_item.into_values().chain(loose).collect();
    for group in &mut all {
        group.sort_by_key(|r| std::cmp::Reverse(r.entry.seq));
    }
    all.sort_by_key(|g| std::cmp::Reverse(g[0].entry.seq));
    all
}

/// The folders the batch created, from what each row recorded, or `None` when a row that ran
/// does not say (a batch journaled before this was recorded).
fn created_folders(rows: &[JournalRow]) -> Option<HashSet<Vec<String>>> {
    let mut created = HashSet::new();
    for row in rows
        .iter()
        .filter(|r| matches!(r.state, JournalState::Done | JournalState::Undone))
    {
        let levels = row.entry.created_parents?;
        let mut dir = Path::new(&row.entry.to).parent();
        for _ in 0..levels {
            let Some(current) = dir else { break };
            created.insert(comparable(current));
            dir = current.parent();
        }
    }
    Some(created)
}

/// The data source root some trash row of the batch reveals: the path before the trash folder.
fn trash_hint(rows: &[JournalRow], trash_dir: &str) -> Option<PathBuf> {
    rows.iter()
        .filter(|r| r.entry.op == JournalOp::Trash)
        .find_map(|r| {
            let path = Path::new(&r.entry.to);
            let at = path.components().position(|c| {
                c.as_os_str()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(trash_dir)
            })?;
            Some(path.components().take(at).collect::<PathBuf>())
        })
}

/// What the disk would show during the verification: the real disk, with the effect of the rows
/// already checked laid over it.
struct Overlay<'a> {
    fs: &'a dyn Fs,
    seen: HashMap<Vec<String>, Option<Meta>>,
    /// The paths the rows checked so far take something away from.
    vacated: Vec<PathBuf>,
}

impl Overlay<'_> {
    fn meta(&self, path: &Path) -> Result<Option<Meta>, String> {
        if let Some(meta) = self.seen.get(&comparable(path)) {
            return Ok(*meta);
        }
        let real = self
            .fs
            .meta(path)
            .map_err(|e| format!("{} cannot be read: {e}", path.display()))?;
        // A volume that ignores Unicode normalisation (APFS) finds a vacated `Amélie` when asked
        // for its decomposed spelling: that is the same entry, and it is about to be gone.
        if real.is_some()
            && self.vacated.iter().any(|v| is_case_only(v, path))
            && !lists_exactly(self.fs, path)
        {
            return Ok(None);
        }
        Ok(real)
    }

    fn set(&mut self, path: &Path, meta: Option<Meta>) {
        self.seen.insert(comparable(path), meta);
        if meta.is_none() {
            self.vacated.push(path.to_path_buf());
        }
    }
}

/// True when `meta` is what the journal recorded for the destination of `row`.
fn unchanged(meta: &Meta, row: &JournalRow) -> bool {
    match row.size_bytes {
        Some(len) => {
            !meta.is_dir
                && meta.len == len
                && row.modified_ms.is_none_or(|ms| ms == meta.modified_ms)
        }
        // A folder (or a row with no record): nothing but its kind to compare.
        None => !row.entry.is_dir || meta.is_dir,
    }
}

struct Undoer<'a> {
    fs: &'a dyn Fs,
    options: &'a UndoOptions,
    batch_id: &'a str,
    trash_hint: Option<PathBuf>,
    /// The folders the batch created; `None` when the journal does not say.
    created: Option<HashSet<Vec<String>>>,
}

impl Undoer<'_> {
    fn item(
        &self,
        journal: &mut dyn JournalStore,
        group: Vec<JournalRow>,
    ) -> Result<UndoItemReport, UndoError> {
        let mut report = UndoItemReport {
            id: group[0].entry.item_id.clone(),
            status: UndoStatus::Restored,
            restored: 0,
            discarded: 0,
            already_restored: 0,
            removed_folders: Vec::new(),
            notes: Vec::new(),
        };
        let steps = match self.verify(group) {
            Ok(steps) => steps,
            Err(problems) => {
                report.status = UndoStatus::NotRestorable(problems);
                return Ok(report);
            }
        };
        self.restore(journal, steps, &mut report)?;
        Ok(report)
    }

    // ---- step 1: verify ------------------------------------------------------------------

    /// Checks every row, newest first, without touching the disk. All problems are collected.
    fn verify(&self, group: Vec<JournalRow>) -> Result<Vec<Planned>, Vec<NotRestored>> {
        let mut overlay = Overlay {
            fs: self.fs,
            seen: HashMap::new(),
            vacated: Vec::new(),
        };
        let mut steps = Vec::new();
        let mut problems = Vec::new();
        for row in group {
            match self.verify_row(&row, &mut overlay) {
                Ok(action) => steps.push(Planned { row, action }),
                Err(problem) => problems.push(problem),
            }
        }
        if problems.is_empty() {
            Ok(steps)
        } else {
            Err(problems)
        }
    }

    fn verify_row(&self, row: &JournalRow, overlay: &mut Overlay) -> Result<Action, NotRestored> {
        let (from, to) = (Path::new(&row.entry.from), Path::new(&row.entry.to));
        let problem = |path: &str, reason: String| NotRestored {
            path: path.to_owned(),
            reason,
        };
        let here = overlay
            .meta(to)
            .map_err(|reason| problem(&row.entry.to, reason))?;
        match row.entry.op {
            JournalOp::Move | JournalOp::Trash => {
                let Some(meta) = here else {
                    // The destination is gone. If the original is back and unchanged, only the
                    // journal missed it.
                    let back = overlay
                        .meta(from)
                        .map_err(|reason| problem(&row.entry.from, reason))?;
                    return match back {
                        Some(m) if unchanged(&m, row) => {
                            Ok(Action::AlreadyRestored { is_dir: m.is_dir })
                        }
                        _ => Err(problem(
                            &row.entry.to,
                            format!(
                                "{} no longer exists; it was moved, renamed or deleted after the \
                                 organize",
                                row.entry.to
                            ),
                        )),
                    };
                };
                if !unchanged(&meta, row) {
                    return Err(problem(
                        &row.entry.to,
                        changed_reason(&row.entry.to, &meta, row),
                    ));
                }
                let occupied = overlay
                    .meta(from)
                    .map_err(|reason| problem(&row.entry.from, reason))?;
                if occupied.is_some() {
                    return Err(problem(
                        &row.entry.from,
                        format!(
                            "{} cannot go back to {} because something else is there now",
                            row.entry.to, row.entry.from
                        ),
                    ));
                }
                overlay.set(to, None);
                overlay.set(from, Some(meta));
                Ok(Action::Restore {
                    is_dir: meta.is_dir,
                })
            }
            JournalOp::Copy | JournalOp::WriteNfo => {
                let Some(meta) = here else {
                    return Ok(Action::Gone);
                };
                if !unchanged(&meta, row) {
                    return Err(problem(
                        &row.entry.to,
                        changed_reason(&row.entry.to, &meta, row),
                    ));
                }
                if self.trash_base(to).is_none() {
                    return Err(problem(
                        &row.entry.to,
                        format!(
                            "{} cannot be put in the trash: its data source is not known",
                            row.entry.to
                        ),
                    ));
                }
                overlay.set(to, None);
                Ok(Action::Discard)
            }
        }
    }

    /// `<data source>/<trash>/<batch>-undo` for a file below a data source.
    fn trash_base(&self, path: &Path) -> Option<PathBuf> {
        let root = self
            .options
            .roots
            .iter()
            .filter(|r| is_within(r, path))
            .max_by_key(|r| r.components().count())
            .or(self.trash_hint.as_ref().filter(|r| is_within(r, path)))?;
        Some(
            root.join(&self.options.trash_dir)
                .join(format!("{}-undo", self.batch_id)),
        )
    }

    // ---- step 2: restore -----------------------------------------------------------------

    fn restore(
        &self,
        journal: &mut dyn JournalStore,
        steps: Vec<Planned>,
        report: &mut UndoItemReport,
    ) -> Result<(), UndoError> {
        // The steps taken so far, for the roll-forward.
        let mut taken: Vec<(JournalEntry, Taken)> = Vec::new();
        // Folders a step still moves back as a whole: never cleaned away before it ran.
        let mut pending_dirs: Vec<Vec<String>> = steps
            .iter()
            .filter(|s| matches!(s.action, Action::Restore { is_dir: true }))
            .map(|s| comparable(Path::new(&s.row.entry.to)))
            .collect();
        for step in steps {
            let mut entry = step.row.entry.clone();
            let (from, to) = (PathBuf::from(&entry.from), PathBuf::from(&entry.to));
            let result: Result<Taken, String> = match step.action {
                Action::Restore { is_dir } => {
                    entry.is_dir = is_dir;
                    if is_dir
                        && let Some(at) = pending_dirs.iter().position(|d| *d == comparable(&to))
                    {
                        pending_dirs.swap_remove(at);
                    }
                    move_path(self.fs, &to, &from, is_dir)
                        .map(|()| Taken::Moved)
                        .map_err(|e| {
                            let reason = if is_dir {
                                super::run::plain_folder_reason(&e)
                            } else {
                                super::run::plain_reason(&e)
                            };
                            format!("cannot move {} back: {reason}", to.display())
                        })
                }
                Action::AlreadyRestored { is_dir } => {
                    entry.is_dir = is_dir;
                    Ok(Taken::Nothing)
                }
                Action::Gone => Ok(Taken::Nothing),
                Action::Discard => self.discard(&to),
            };
            match result {
                Ok(taken_as) => {
                    journal.complete(&entry, &Completion::Undone)?;
                    match &taken_as {
                        Taken::Moved => {
                            report.restored += 1;
                            // Right away: a folder rename taken back later moves the folder these
                            // empty folders are in, and they would be left behind in it.
                            if self.options.remove_empty_folders {
                                self.remove_empty_below_common(
                                    &from,
                                    &to,
                                    &pending_dirs,
                                    &mut report.removed_folders,
                                );
                            }
                        }
                        Taken::Trashed(_) => report.discarded += 1,
                        Taken::Nothing => report.already_restored += 1,
                    }
                    taken.push((entry, taken_as));
                }
                Err(message) => {
                    report.notes.extend(self.roll_forward(journal, taken)?);
                    report.status = UndoStatus::Failed(message);
                    report.restored = 0;
                    report.discarded = 0;
                    report.already_restored = 0;
                    return Ok(());
                }
            }
        }
        Ok(())
    }

    /// Moves a copy or a written file into the undo trash.
    fn discard(&self, path: &Path) -> Result<Taken, String> {
        let base = self
            .trash_base(path)
            .ok_or_else(|| format!("{} cannot be put in the trash", path.display()))?;
        // The path below the data source, kept under the trash batch folder.
        let root = base
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| format!("{} cannot be put in the trash", path.display()))?;
        let rel = path.strip_prefix(root).unwrap_or(path);
        let dest = unused_path(self.fs, &base.join(rel))
            .map_err(|e| format!("cannot read {}: {e}", base.display()))?;
        move_path(self.fs, path, &dest, false)
            .map_err(|e| format!("cannot trash {}: {e}", path.display()))?;
        Ok(Taken::Trashed(dest))
    }

    /// Puts the steps taken so far forward again, newest first, and marks their rows `done`.
    /// Returns what could not be put forward.
    fn roll_forward(
        &self,
        journal: &mut dyn JournalStore,
        taken: Vec<(JournalEntry, Taken)>,
    ) -> Result<Vec<String>, UndoError> {
        let mut notes = Vec::new();
        for (entry, how) in taken.into_iter().rev() {
            let (from, to) = (PathBuf::from(&entry.from), PathBuf::from(&entry.to));
            let result = match &how {
                Taken::Moved => move_path(self.fs, &from, &to, entry.is_dir),
                Taken::Trashed(dest) => move_path(self.fs, dest, &to, false),
                // The disk was never changed by these.
                Taken::Nothing => continue,
            };
            match result {
                Ok(()) => {
                    let meta = self.fs.meta(&to).ok().flatten();
                    journal.complete(
                        &entry,
                        &Completion::Done {
                            size_bytes: meta.filter(|m| !m.is_dir).map(|m| m.len),
                            modified_ms: meta.map(|m| m.modified_ms),
                        },
                    )?;
                }
                Err(e) => notes.push(format!(
                    "could not put {} back to {}: {e}",
                    from.display(),
                    to.display()
                )),
            }
        }
        Ok(notes)
    }

    // ---- step 3: clean up ----------------------------------------------------------------

    /// Removes the empty folders above `to` (where the file was) that lie below the common parent
    /// of the two paths. Stops at the first folder with content, at a data source root, at the
    /// trash folder, at a folder in `keep` and at a folder the batch did not create.
    fn remove_empty_below_common(
        &self,
        from: &Path,
        to: &Path,
        keep: &[Vec<String>],
        removed: &mut Vec<String>,
    ) {
        let (a, b) = (comparable(from), comparable(to));
        let common = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
        if common == 0 {
            return;
        }
        let mut dir = to.parent().map(Path::to_path_buf);
        while let Some(current) = dir {
            if comparable(&current).len() <= common
                || keep.contains(&comparable(&current))
                || self
                    .created
                    .as_ref()
                    .is_some_and(|made| !made.contains(&comparable(&current)))
                || self
                    .options
                    .roots
                    .iter()
                    .any(|r| comparable(r) == comparable(&current))
                || current.file_name().is_some_and(|n| {
                    n.to_string_lossy()
                        .eq_ignore_ascii_case(&self.options.trash_dir)
                })
            {
                return;
            }
            match self.fs.list_dir(&current) {
                Ok(names) if names.is_empty() => {
                    if self.fs.remove_dir(&current).is_err() {
                        return;
                    }
                    removed.push(current.to_string_lossy().into_owned());
                }
                _ => return,
            }
            dir = current.parent().map(Path::to_path_buf);
        }
    }
}

/// How a step was taken back.
enum Taken {
    /// The destination was moved to the original path.
    Moved,
    /// The destination was moved to this path in the trash.
    Trashed(PathBuf),
    /// The disk was not changed: the original was back already, or a copy was gone.
    Nothing,
}

fn changed_reason(path: &str, meta: &Meta, row: &JournalRow) -> String {
    match row.size_bytes {
        Some(len) if meta.is_dir => {
            format!("{path} is a folder now; it was a file of {len} bytes after the organize")
        }
        Some(len) if meta.len != len => format!(
            "{path} was modified after the organize (it had {len} bytes and has {} now)",
            meta.len
        ),
        Some(_) => {
            format!("{path} was modified after the organize (its modification time changed)")
        }
        None => format!("{path} is a file now; it was a folder after the organize"),
    }
}
