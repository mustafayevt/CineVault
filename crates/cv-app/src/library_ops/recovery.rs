//! Unfinished moves and renames: found at start-up, resumed or rolled back (spec
//! `source-moves` "Interrupted moves are safe and recoverable").
//!
//! A batch is **unfinished** when the journal says work is left: a row is still `planned` (the
//! app was closed or crashed), or a move stopped on an error that removes nothing (the row says
//! `stopped: <reason>`), or recovery found a rename step that never started. Start-up recovery
//! ([`crate::organize::recover_interrupted_batches`]) settles the rows of renames from what the
//! disk shows and leaves the rows of moves alone; [`RecoveryService::find`] then lists every
//! unfinished batch before any new file work, and Activity offers two things for each:
//!
//! * **Resume** carries on. For a move, the files already copied are checked again against their
//!   originals (size and hash), the rest are copied, and each title is put in place as when the
//!   move first ran. For a rename, each title the stop left half renamed is run again from the
//!   plan the journal kept for it, in the same batch: what is on the disk already counts as done,
//!   the rest runs (a batch journaled before plans were kept runs its never-started steps).
//! * **Roll back** takes back the titles the stop left unfinished, and only those. For a move,
//!   only what the batch made is removed: the `.cvpart` files, the copies and the staging folder;
//!   the originals were never touched. For a rename, the done steps of the unfinished titles are
//!   undone like any undo. In both, a title that had finished before the stop stays in its new
//!   place; History still undoes the whole batch.
//!
//! When a Resume finishes titles, the batch's History entry is written, or completed with the
//! titles it did not list yet, from the journal, so History can undo them. At start-up
//! [`RecoveryService::record_unrecorded`] does the same for a finished batch that has no entry
//! (the app stopped between the batch's end and its entry).
//!
//! Nothing is deleted that this batch did not create, and nothing is replaced.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use cv_db::Database;
use cv_db::repo::data_sources::list_sources;
use cv_db::repo::library_ops::delete_fingerprints_under;
use cv_db::repo::rename_journal::PathObserver;
use cv_domain::ItemId;
use cv_rename::exec::{
    DbJournal, ExecError, INTERRUPTED, ItemMark, ItemState, ItemStatus, JournalError, JournalRow,
    JournalState, JournalStore, StdFs, UndoError, UndoOptions, recover, resume_interrupted,
    resume_unfinished, settle_interrupted, undo_batch,
};

use super::change_source::{ChangedHook, record_move_from_journal};
use super::move_engine::{
    ItemRows, JobOutcome, MoveControl, MoveEngine, MoveError, MoveFs, MoveProgress, StdMoveFs,
    group_rows, is_move_batch, is_running,
};
use crate::notices::{AppNotice, NoticeAction, NoticeLevel};

/// What kind of work was cut short.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchKind {
    /// A move of titles to another data source.
    Move,
    /// An Organize or an automatic rename.
    Rename,
}

/// One unfinished batch, for Activity's Needs attention.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnfinishedBatch {
    pub batch_id: String,
    pub kind: BatchKind,
    /// When the batch was written to the journal, epoch milliseconds.
    pub created_ms: i64,
    /// The titles that still have work (the first few are enough to name the batch).
    pub titles: Vec<String>,
    /// How many titles still have work.
    pub items: usize,
    /// Files copied and verified so far, and files to copy in all, for a move; titles renamed
    /// and titles in the batch, for a rename.
    pub files_done: usize,
    pub files_total: usize,
    /// A plain sentence: why it stopped.
    pub reason: String,
    /// A title of the batch is in its new place already and only the originals are left to
    /// tidy (moves only).
    pub in_new_place: bool,
}

impl UnfinishedBatch {
    /// The notice Activity shows for the batch, with the Resume and Roll back actions.
    #[must_use]
    pub fn notice(&self) -> AppNotice {
        let what = match self.kind {
            BatchKind::Move => "A move to another source did not finish",
            BatchKind::Rename => "A rename did not finish",
        };
        let names = match self.titles.as_slice() {
            [] => String::new(),
            [only] => format!("{only}. "),
            [first, rest @ ..] => format!("{first} and {} more. ", rest.len()),
        };
        let detail = match self.kind {
            BatchKind::Move if self.in_new_place => format!(
                "{names}{} The title is in its new place; some originals are still in the old \
                 folder. Resume the move to finish it, or roll it back.",
                self.reason
            ),
            BatchKind::Move => format!(
                "{names}{} The original files are untouched. Resume the move or roll it back.",
                self.reason
            ),
            BatchKind::Rename => {
                format!("{names}{} Resume the rename or roll it back.", self.reason)
            }
        };
        AppNotice::new(NoticeLevel::Warning, what)
            .detail(detail)
            .source("Recovery")
            .action(NoticeAction::ResolveUnfinished)
            .batch_id(self.batch_id.clone())
    }
}

/// What a resume did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResumeOutcome {
    pub batch_id: String,
    /// Titles that are in their new place now (or steps that were run, for a rename).
    pub completed: usize,
    /// Why the batch stopped again, when it did; it is still unfinished then.
    pub stopped: Option<String>,
    pub notes: Vec<String>,
}

/// What a roll back did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RollBackOutcome {
    pub batch_id: String,
    /// Originals or old names put back.
    pub restored: usize,
    /// Copies and part files removed.
    pub removed: usize,
    /// Things that could not be taken back, with the reason.
    pub problems: Vec<String>,
}

/// Why recovery could not run.
#[derive(Debug, thiserror::Error)]
pub enum RecoveryError {
    #[error("the library could not be read: {0}")]
    Db(#[from] cv_db::DbError),
    #[error(transparent)]
    Journal(#[from] JournalError),
    #[error(transparent)]
    Exec(#[from] ExecError),
    #[error(transparent)]
    Undo(#[from] UndoError),
    #[error(transparent)]
    Move(#[from] MoveError),
    /// The batch is not in the journal, or has nothing left to do.
    #[error("there is no unfinished batch {0:?}")]
    Unknown(String),
    /// A title of the batch is busy with other work; the text says with what.
    #[error("{0}")]
    Busy(String),
}

/// A journal that leaves the rows of moves between data sources to the move engine: start-up
/// recovery settles renames by looking at the disk, which would wrongly call a half-copied
/// move "never started".
pub struct ExceptMoves<S>(pub S);

impl<S: JournalStore> JournalStore for ExceptMoves<S> {
    fn begin(&mut self, entry: &cv_rename::exec::JournalEntry) -> Result<(), JournalError> {
        self.0.begin(entry)
    }
    fn begin_all(&mut self, entries: &[cv_rename::exec::JournalEntry]) -> Result<(), JournalError> {
        self.0.begin_all(entries)
    }
    fn complete(
        &mut self,
        entry: &cv_rename::exec::JournalEntry,
        completion: &cv_rename::exec::Completion,
    ) -> Result<(), JournalError> {
        self.0.complete(entry, completion)
    }
    fn batch(&self, batch_id: &str) -> Result<Vec<JournalRow>, JournalError> {
        self.0.batch(batch_id)
    }
    fn pending(&self) -> Result<Vec<JournalRow>, JournalError> {
        let mut rows = self.0.pending()?;
        rows.retain(|row| !is_move_batch(&row.entry.batch_id));
        Ok(rows)
    }
    fn batches(&self) -> Result<Vec<cv_rename::exec::BatchSummary>, JournalError> {
        self.0.batches()
    }
    fn open_item(&mut self, batch_id: &str, item_id: &str, plan: &str) -> Result<(), JournalError> {
        self.0.open_item(batch_id, item_id, plan)
    }
    fn close_item(
        &mut self,
        batch_id: &str,
        item_id: &str,
        state: ItemState,
    ) -> Result<(), JournalError> {
        self.0.close_item(batch_id, item_id, state)
    }
    fn item_marks(&self, batch_id: &str) -> Result<Vec<ItemMark>, JournalError> {
        self.0.item_marks(batch_id)
    }
    fn item_marks_in(&self, state: ItemState) -> Result<Vec<ItemMark>, JournalError> {
        let mut marks = self.0.item_marks_in(state)?;
        marks.retain(|mark| !is_move_batch(&mark.batch_id));
        Ok(marks)
    }
}

/// The items of a rename batch that are unfinished: their mark says so, or (a batch journaled
/// before items were marked) a step of theirs is still planned or never started. The flag says
/// whether a step that belongs to no item is open too.
fn unfinished_rename_items(rows: &[JournalRow], marks: &[ItemMark]) -> (Vec<String>, bool) {
    let mut loose = false;
    let mut ids: Vec<String> = marks
        .iter()
        .filter(|m| m.state == ItemState::Unfinished)
        .map(|m| m.item_id.clone())
        .collect();
    for row in rows {
        let open = row.state == JournalState::Planned
            || (row.state == JournalState::Failed && row.error == INTERRUPTED);
        if !open {
            continue;
        }
        let Some(id) = row.entry.item_id.as_deref() else {
            loose = true;
            continue;
        };
        // An item with a mark is unfinished only when its mark says so.
        if marks.iter().any(|m| m.item_id == id) {
            continue;
        }
        if !ids.iter().any(|known| known == id) {
            ids.push(id.to_owned());
        }
    }
    (ids, loose)
}

/// Writes (or completes) the History entry of a batch from its journal. A failure is logged:
/// the journal still holds the batch.
fn record_from_journal(db: &Arc<Database>, batch_id: &str) -> Option<i64> {
    let written = if is_move_batch(batch_id) {
        record_move_from_journal(db, batch_id).map_err(|e| e.to_string())
    } else {
        crate::organize::record_rename_from_journal(db, batch_id).map_err(|e| e.to_string())
    };
    written.unwrap_or_else(|error| {
        tracing::warn!(batch = batch_id, %error, "the batch has no History entry");
        None
    })
}

/// Finds, resumes and rolls back unfinished batches.
#[derive(Clone)]
pub struct RecoveryService {
    db: Arc<Database>,
    observer: Option<Arc<dyn PathObserver>>,
    disk: Arc<dyn MoveFs>,
    changed: Option<ChangedHook>,
}

impl RecoveryService {
    #[must_use]
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            db,
            observer: None,
            disk: Arc::new(StdMoveFs),
            changed: None,
        }
    }

    /// Also tells `observer` about every move, so the poster index follows the files.
    #[must_use]
    pub fn with_path_observer(mut self, observer: Arc<dyn PathObserver>) -> Self {
        self.observer = Some(observer);
        self
    }

    /// Replaces the disk. For tests that fail on purpose.
    #[must_use]
    pub fn with_disk(mut self, disk: Arc<dyn MoveFs>) -> Self {
        self.disk = disk;
        self
    }

    /// Tells `hook` which titles a resume or a roll back changed.
    #[must_use]
    pub fn with_changed_hook(mut self, hook: ChangedHook) -> Self {
        self.changed = Some(hook);
        self
    }

    fn journal(&self) -> DbJournal {
        let journal = DbJournal::new(Arc::clone(&self.db));
        match &self.observer {
            Some(observer) => journal.with_observer(Arc::clone(observer)),
            None => journal,
        }
    }

    fn title_of(&self, id: &str) -> Option<String> {
        let id = ItemId(id.parse::<i64>().ok()?);
        self.db
            .read(|conn| cv_db::repo::get_summary(conn, id))
            .ok()
            .flatten()
            .map(|summary| summary.title)
    }

    fn titles(&self, ids: impl IntoIterator<Item = String>) -> BTreeMap<String, String> {
        ids.into_iter()
            .filter_map(|id| self.title_of(&id).map(|title| (id, title)))
            .collect()
    }

    /// Every unfinished batch, newest first. Call it after start-up recovery and before any
    /// scan or other file work.
    ///
    /// # Errors
    /// The journal cannot be read.
    pub fn find(&self) -> Result<Vec<UnfinishedBatch>, RecoveryError> {
        let journal = self.journal();
        let mut out = Vec::new();
        for summary in journal.batches()? {
            // A batch at work now looks unfinished too; it is not offered while it runs.
            if is_running(&summary.batch_id) {
                continue;
            }
            if is_move_batch(&summary.batch_id) {
                if summary.planned == 0 && summary.failed == 0 {
                    continue;
                }
                let rows = journal.batch(&summary.batch_id)?;
                let items = group_rows(rows);
                let open: Vec<&ItemRows> = items.iter().filter(|i| i.unfinished()).collect();
                if open.is_empty() {
                    continue;
                }
                let (done, total) = open
                    .iter()
                    .map(|i| i.files())
                    .fold((0, 0), |(d, t), (x, y)| (d + x, t + y));
                let reason = open.iter().find_map(|i| i.stop_reason()).map_or_else(
                    || "CineVault closed before the move finished.".to_owned(),
                    |why| format!("{why}."),
                );
                let titles = open
                    .iter()
                    .filter_map(|i| i.item.as_deref())
                    .filter_map(|id| self.title_of(id))
                    .take(3)
                    .collect();
                out.push(UnfinishedBatch {
                    batch_id: summary.batch_id,
                    kind: BatchKind::Move,
                    created_ms: summary.created_ms,
                    titles,
                    items: open.len(),
                    files_done: done,
                    files_total: total,
                    reason,
                    in_new_place: open.iter().any(|i| i.committed()),
                });
            } else {
                let rows = journal.batch(&summary.batch_id)?;
                let marks = journal.item_marks(&summary.batch_id)?;
                let (ids, loose) = unfinished_rename_items(&rows, &marks);
                if ids.is_empty() && !loose {
                    continue;
                }
                let titles: Vec<String> = ids
                    .iter()
                    .filter_map(|id| self.title_of(id))
                    .take(3)
                    .collect();
                out.push(UnfinishedBatch {
                    batch_id: summary.batch_id,
                    kind: BatchKind::Rename,
                    created_ms: summary.created_ms,
                    titles,
                    items: ids.len(),
                    // A rename is counted by title (every title of the batch has a mark from
                    // before the first was renamed); a batch journaled before marks, by step.
                    files_done: if marks.is_empty() {
                        rows.iter()
                            .filter(|r| r.state == JournalState::Done)
                            .count()
                    } else {
                        marks.iter().filter(|m| m.state == ItemState::Done).count()
                    },
                    files_total: if marks.is_empty() {
                        rows.len()
                    } else {
                        marks.len()
                    },
                    reason: "CineVault closed before the rename finished.".to_owned(),
                    in_new_place: false,
                });
            }
        }
        Ok(out)
    }

    fn roots(&self) -> Result<Vec<PathBuf>, RecoveryError> {
        Ok(self
            .db
            .read(|conn| list_sources(conn, None))?
            .into_iter()
            .map(|s| PathBuf::from(s.path))
            .collect())
    }

    /// Carries on an unfinished batch. See the [module](self) for what that means.
    ///
    /// # Errors
    /// The batch is not unfinished, or the journal cannot be read or written. A stop on the disk
    /// is no error: the outcome says why the batch stopped again.
    pub fn resume(
        &self,
        batch_id: &str,
        control: Option<Arc<MoveControl>>,
        progress: &mut dyn FnMut(&MoveProgress),
    ) -> Result<ResumeOutcome, RecoveryError> {
        self.require_unfinished(batch_id)?;
        let sources = self.db.read(|conn| list_sources(conn, None))?;
        let _held = cv_scan::lock::hold_sources(sources.iter().map(|s| s.id));
        let _titles = self.hold_titles(batch_id)?;
        // The batch may have changed while this waited for the sources.
        self.require_unfinished(batch_id)?;
        let mut outcome = ResumeOutcome {
            batch_id: batch_id.to_owned(),
            ..ResumeOutcome::default()
        };
        if is_move_batch(batch_id) {
            let mut journal = self.journal();
            let rows = journal.batch(batch_id)?;
            let grouped = group_rows(rows);
            let titles = self.titles(grouped.iter().filter_map(|i| i.item.clone()));
            let folders: BTreeMap<String, String> = grouped
                .iter()
                .filter_map(|i| Some((i.item.clone()?, i.old_folder()?.to_owned())))
                .collect();
            let disk = Arc::clone(&self.disk);
            let mut engine = MoveEngine::new(disk.as_ref(), &mut journal).with_roots(self.roots()?);
            if let Some(control) = control {
                engine = engine.with_control(control);
            }
            let results = engine.resume(batch_id, &titles, progress)?;
            let mut moved = Vec::new();
            for (item, result) in results {
                match result {
                    JobOutcome::Moved { notes } => {
                        outcome.completed += 1;
                        outcome.notes.extend(notes);
                        if let Some(id) = item.as_deref().and_then(|id| id.parse::<i64>().ok()) {
                            moved.push(ItemId(id));
                        }
                        if let Some(folder) = item.as_ref().and_then(|id| folders.get(id)) {
                            let folder = folder.clone();
                            self.db
                                .write(move |tx| delete_fingerprints_under(tx, &folder))?;
                        }
                    }
                    JobOutcome::Failed(why) | JobOutcome::Stopped(why) => {
                        outcome.stopped.get_or_insert(why);
                    }
                    JobOutcome::Canceled => {
                        outcome
                            .stopped
                            .get_or_insert_with(|| "The move was cancelled.".to_owned());
                    }
                    JobOutcome::NotStarted => {}
                }
            }
            self.announce(&moved);
        } else {
            let mut journal = ExceptMoves(self.journal());
            recover(&mut journal, &StdFs)?;
            // Each half-renamed title is run again whole, from the plan kept with it.
            let resumed = resume_unfinished(&mut journal, &StdFs, batch_id, &self.roots()?)?;
            for item in &resumed.items {
                match &item.status {
                    ItemStatus::Done => outcome.completed += 1,
                    ItemStatus::Failed(why) => {
                        let title = self.title_of(&item.id).unwrap_or_else(|| item.id.clone());
                        outcome.notes.push(format!("{title}: {why}"));
                    }
                    ItemStatus::Canceled => {}
                }
                outcome.notes.extend(item.notes.iter().cloned());
            }
            for (id, why) in resumed.skipped {
                let title = self.title_of(&id).unwrap_or(id);
                outcome.notes.push(format!("{title}: {why}"));
            }
            // A batch journaled before items were marked: its never-started steps.
            let report = resume_interrupted(&mut journal, &StdFs, batch_id)?;
            outcome.completed += report.resumed;
            for (path, why) in report.skipped {
                outcome.notes.push(format!("{path}: {why}"));
            }
            if !outcome.notes.is_empty() {
                outcome.stopped = Some("Some steps could not be run.".to_owned());
            }
            let ids = self.batch_items(batch_id)?;
            self.announce(&ids);
        }
        // The titles this finished go into the batch's History entry, so History can undo them.
        record_from_journal(&self.db, batch_id);
        Ok(outcome)
    }

    /// Writes the History entry of every finished batch that has none: the app stopped between
    /// the end of a move or rename and its entry. Call it at start-up, after start-up recovery
    /// and before any new file work. A batch that is unfinished gets its entry when Resume
    /// finishes it; a clean-up, a copy back and a batch rolled back get none. Returns how many
    /// entries were written.
    ///
    /// # Errors
    /// The journal or the library cannot be read.
    pub fn record_unrecorded(&self) -> Result<usize, RecoveryError> {
        let unfinished: std::collections::BTreeSet<String> =
            self.find()?.into_iter().map(|b| b.batch_id).collect();
        let mut written = 0;
        for summary in self.journal().batches()? {
            let batch = summary.batch_id;
            if is_running(&batch) || unfinished.contains(&batch) {
                continue;
            }
            let id = batch.clone();
            if self
                .db
                .read(move |conn| cv_db::repo::history::entry_by_batch(conn, &id))?
                .is_some()
            {
                continue;
            }
            if record_from_journal(&self.db, &batch).is_some() {
                tracing::info!(batch, "a finished batch without a History entry got one");
                written += 1;
            }
        }
        Ok(written)
    }

    /// Takes an unfinished batch back. See the [module](self) for what that means.
    ///
    /// # Errors
    /// The batch is not unfinished, or the journal cannot be read or written.
    pub fn roll_back(&self, batch_id: &str) -> Result<RollBackOutcome, RecoveryError> {
        self.require_unfinished(batch_id)?;
        let sources = self.db.read(|conn| list_sources(conn, None))?;
        let _held = cv_scan::lock::hold_sources(sources.iter().map(|s| s.id));
        let _titles = self.hold_titles(batch_id)?;
        // The batch may have changed while this waited for the sources.
        self.require_unfinished(batch_id)?;
        let mut outcome = RollBackOutcome {
            batch_id: batch_id.to_owned(),
            ..RollBackOutcome::default()
        };
        let roots = self.roots()?;
        if is_move_batch(batch_id) {
            let mut journal = self.journal();
            let disk = Arc::clone(&self.disk);
            let taken = MoveEngine::new(disk.as_ref(), &mut journal)
                .with_roots(roots)
                .roll_back(batch_id)?;
            for item in taken {
                outcome.restored += item.restored;
                outcome.removed += item.discarded;
                outcome.problems.extend(item.notes);
                match item.status {
                    super::move_engine::TakeBackStatus::Restored => {}
                    super::move_engine::TakeBackStatus::NotRestorable(list) => {
                        outcome
                            .problems
                            .extend(list.into_iter().map(|(path, why)| format!("{path}: {why}")));
                    }
                    super::move_engine::TakeBackStatus::Failed(why) => outcome.problems.push(why),
                }
            }
        } else {
            let mut journal = ExceptMoves(self.journal());
            recover(&mut journal, &StdFs)?;
            // Only the titles the stop left unfinished go back, as for a move; a title that had
            // finished stays renamed (History undoes the whole batch).
            let rows = journal.batch(batch_id)?;
            let marks = journal.item_marks(batch_id)?;
            // A batch journaled before items were marked cannot tell its titles apart (its rows
            // may have no title): it is taken back whole, as before.
            let options = UndoOptions {
                roots,
                only_items: (!marks.is_empty()).then(|| unfinished_rename_items(&rows, &marks).0),
                ..UndoOptions::default()
            };
            let report = undo_batch(&mut journal, &StdFs, batch_id, &options)?;
            for item in &report.items {
                outcome.restored += item.restored;
                outcome.removed += item.discarded;
                match &item.status {
                    cv_rename::exec::UndoStatus::Restored => {}
                    cv_rename::exec::UndoStatus::NotRestorable(list) => outcome
                        .problems
                        .extend(list.iter().map(|p| format!("{}: {}", p.path, p.reason))),
                    cv_rename::exec::UndoStatus::Failed(why) => outcome.problems.push(why.clone()),
                }
            }
            settle_interrupted(&mut journal, batch_id)?;
        }
        let ids = self.batch_items(batch_id)?;
        self.announce(&ids);
        Ok(outcome)
    }

    fn require_unfinished(&self, batch_id: &str) -> Result<(), RecoveryError> {
        if self.find()?.iter().any(|b| b.batch_id == batch_id) {
            Ok(())
        } else {
            Err(RecoveryError::Unknown(batch_id.to_owned()))
        }
    }

    /// Holds the titles of the batch against other writes (see [`crate::title_guard`]).
    fn hold_titles(&self, batch_id: &str) -> Result<crate::title_guard::TitleGuard, RecoveryError> {
        let ids = self.batch_items(batch_id)?;
        let work = if is_move_batch(batch_id) {
            crate::title_guard::Work::Move
        } else {
            crate::title_guard::Work::Rename
        };
        crate::title_guard::hold_items(&self.db, &ids, work, crate::title_guard::MOVE_WAIT).map_err(
            |e| match e {
                crate::title_guard::GuardError::Busy(busy) => RecoveryError::Busy(busy.to_string()),
                crate::title_guard::GuardError::Db(db) => RecoveryError::Db(db),
            },
        )
    }

    fn batch_items(&self, batch_id: &str) -> Result<Vec<ItemId>, RecoveryError> {
        let id = batch_id.to_owned();
        Ok(self
            .db
            .read(move |conn| {
                let rows = cv_db::repo::rename_journal::batch_rows(conn, &id)?;
                let mut ids: Vec<i64> = rows.iter().filter_map(|r| r.item_id).collect();
                ids.sort_unstable();
                ids.dedup();
                Ok(ids)
            })?
            .into_iter()
            .map(ItemId)
            .collect())
    }

    fn announce(&self, ids: &[ItemId]) {
        if let Some(hook) = &self.changed
            && !ids.is_empty()
        {
            hook(ids);
        }
    }
}
