//! The Organize service: preview, apply, journal and undo over the library and the disk.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

use cv_db::Database;
use cv_db::repo::data_sources::list_sources;
use cv_db::repo::rename_journal::{batch_item_ids, owner_of_file, owner_of_folder};
use cv_domain::{ItemId, MediaFileType};
use cv_rename::conflict::FsDisk;
use cv_rename::conflict::{
    BatchItem, Conflict, ConflictKind, ConflictOptions, ConflictReport, Disk, NameEdit, Resolution,
    ResolutionKind, detect, resolve,
};
use cv_rename::exec::{
    DbJournal, ExecOptions, Executor, Fs, ItemStatus, JournalStore, PathObserver, StdFs, UndoError,
    UndoOptions, UndoStatus, new_batch_id, recover, settle_interrupted, undo_batch,
};
use cv_rename::tv::EpisodeInput;
use cv_rename::{
    Fate, MEDIA_HELD_REASON, MovieInput, MovieRenamer, OpKind, RenameError, TvInput, TvRenamer,
};

use super::check::{FindingSeverity, check_pattern};
use super::examples::examples;
use super::load::{Loaded, LoadedMovie, load, scope_ids};
use super::model::{
    Applied, AppliedItem, AppliedStatus, ApplyRequest, BusyTitle, Choice, CompanionCounts,
    ConflictCode, ConflictRow, Counts, FileChange, JournalBatch, NotRestorable, OrganizeError,
    PatternField, Preview, PreviewRequest, PreviewRow, ResolutionOption, RowKind, RowStatus,
    TemplateProblem, UndoOutcome,
};

/// Why a title that is not scraped yet is not planned, as the organize-state cache keeps it.
const UNSCRAPED_REASON: &str = "It is renamed once it is scraped.";

/// Organize over one library.
#[derive(Clone)]
pub struct OrganizeService {
    db: Arc<Database>,
    observer: Option<Arc<dyn PathObserver>>,
    /// Told which titles a batch or an undo touched, so their cached organize state is
    /// recomputed.
    changed: Option<ChangedHook>,
    /// The disk batches and undos work on: the real one, or one a test makes fail.
    disk: Arc<dyn Fs + Send + Sync>,
    /// Plans titles that are not scraped yet too (the command line's explicit rename steps).
    plan_unscraped: bool,
    /// The Recycle Bin / Trash an undo of a move between drives restores originals from.
    bin: Option<Arc<dyn crate::library_ops::SystemBin>>,
    /// Settings › Deleted files: where a rename's leftovers go.
    choice: Arc<dyn crate::deleted_files::DeletedFilesSource>,
}

/// Called with the ids of the titles an apply or an undo changed.
pub type ChangedHook = Arc<dyn Fn(&[ItemId]) + Send + Sync>;

/// What the preview knows about an item besides its plan.
struct Info {
    kind: RowKind,
    title: String,
    year: u16,
    root: String,
    old_folder: String,
    before_file: String,
}

/// The outcome of planning a request, before it is turned into the answer.
struct Planned {
    infos: BTreeMap<String, Info>,
    /// Display order of the ids that are listed.
    listed: Vec<String>,
    unplanned: BTreeMap<String, String>,
    /// The batch items after the resolutions, by id (every item that was planned and not skipped).
    resolved: BTreeMap<String, BatchItem>,
    /// The batch items as first planned, by id.
    original: BTreeMap<String, BatchItem>,
    skipped: BTreeSet<String>,
    blocked: BTreeSet<String>,
    /// Items that are not scraped yet: listed, but not planned until they are.
    unscraped: BTreeSet<String>,
    /// Items whose names use media-info tokens while their video details are not read: listed,
    /// but not planned until the details are read (see `cv_rename::media_guard`).
    media_unread: BTreeSet<String>,
    /// Items whose names already match the profile.
    tidy: BTreeSet<String>,
    /// Items that are locked and so not planned.
    locked: BTreeSet<String>,
    report: ConflictReport,
    counts: Counts,
    examples: Vec<super::model::Example>,
    problems: Vec<TemplateProblem>,
}

impl OrganizeService {
    pub fn new(db: Arc<Database>) -> Self {
        OrganizeService {
            db,
            observer: None,
            changed: None,
            disk: Arc::new(StdFs),
            plan_unscraped: false,
            bin: None,
            choice: crate::deleted_files::default_source(),
        }
    }

    /// Reads Settings › Deleted files from `choice` at every apply: a custom trash folder takes
    /// the leftovers of files on its volume, and Delete permanently erases them once the batch
    /// has run (the rename itself stays undoable).
    #[must_use]
    pub fn with_choice(
        mut self,
        choice: Arc<dyn crate::deleted_files::DeletedFilesSource>,
    ) -> Self {
        self.choice = choice;
        self
    }

    /// Works on `disk` instead of the real file system: the fault-injection tests
    /// make it fail or stop part way.
    #[must_use]
    pub fn with_disk(mut self, disk: Arc<dyn Fs + Send + Sync>) -> Self {
        self.disk = disk;
        self
    }

    /// Also plans titles that are not scraped yet, by the names their files give. Organize and the
    /// rename after a scrape leave such a title alone until it is scraped; a script that asks the
    /// command line to rename titles it only scanned gets them renamed.
    #[must_use]
    pub fn planning_unscraped(mut self) -> Self {
        self.plan_unscraped = true;
        self
    }

    /// The Recycle Bin / Trash an undo of a move between drives restores originals from and
    /// sends the copies to. Without one, the trash folder only.
    #[must_use]
    pub fn with_bin(mut self, bin: Arc<dyn crate::library_ops::SystemBin>) -> Self {
        self.bin = Some(bin);
        self
    }

    /// Also calls `hook` with the ids of the titles every apply and undo changed, so the cache of
    /// which titles are tidy follows the files.
    #[must_use]
    pub fn with_changed_hook(mut self, hook: ChangedHook) -> Self {
        self.changed = Some(hook);
        self
    }

    fn note_changed(&self, ids: &[ItemId]) {
        if let (Some(hook), false) = (&self.changed, ids.is_empty()) {
            hook(ids);
        }
    }

    /// Also tells `observer` about every move a batch or an undo makes in the library (task
    /// 15.7), so the poster index behind `cv://img` follows the files without a rescan.
    #[must_use]
    pub fn with_path_observer(mut self, observer: Arc<dyn PathObserver>) -> Self {
        self.observer = Some(observer);
        self
    }

    fn journal_store(&self) -> DbJournal {
        let journal = DbJournal::new(Arc::clone(&self.db));
        match &self.observer {
            Some(observer) => journal.with_observer(Arc::clone(observer)),
            None => journal,
        }
    }

    /// The library this service organizes.
    pub fn database(&self) -> &Arc<Database> {
        &self.db
    }

    /// Plans the request and describes it. Nothing touches the disk.
    ///
    /// # Errors
    /// The library cannot be read, or an edited name is empty once cleaned.
    pub fn preview(&self, request: &PreviewRequest) -> Result<Preview, OrganizeError> {
        let planned = self.plan(request)?;
        Ok(describe(planned, &|id| self.labelled_title(id)))
    }

    /// Plans the request as a dry run and says, for every movie and show in its scope, whether
    /// its names already match the profile. Nothing touches the disk. This is what the
    /// organize-state cache stores.
    ///
    /// # Errors
    /// The library cannot be read.
    pub fn item_states(
        &self,
        request: &PreviewRequest,
    ) -> Result<Vec<cv_db::repo::organize_state::OrganizeStateEntry>, OrganizeError> {
        let planned = self.plan(request)?;
        self.states_of(&planned, request)
    }

    /// [`Self::preview`] and [`Self::item_states`] from one plan: what the Organize page shows
    /// and what the organize-state cache would store for the same titles.
    ///
    /// # Errors
    /// The library cannot be read, or an edited name is empty once cleaned.
    pub fn preview_with_states(
        &self,
        request: &PreviewRequest,
    ) -> Result<
        (
            Preview,
            Vec<cv_db::repo::organize_state::OrganizeStateEntry>,
        ),
        OrganizeError,
    > {
        let planned = self.plan(request)?;
        let states = self.states_of(&planned, request)?;
        Ok((describe(planned, &|id| self.labelled_title(id)), states))
    }

    fn states_of(
        &self,
        planned: &Planned,
        request: &PreviewRequest,
    ) -> Result<Vec<cv_db::repo::organize_state::OrganizeStateEntry>, OrganizeError> {
        use cv_db::repo::organize_state::{OrganizeStateEntry, OrganizeStateKind};

        let entry = |id: &str, state, reason: String| {
            id.parse::<i64>().ok().map(|id| OrganizeStateEntry {
                item: ItemId(id),
                state,
                reason,
            })
        };
        let mut out = Vec::new();
        if let Some(problem) = planned.problems.first() {
            // The templates do not work, so nothing was planned. The scope's items are unplanned.
            let ids = scope_ids(&self.db, &request.scope)?;
            let reason = format!("The naming templates have a problem: {}", problem.message);
            out.extend(ids.into_iter().map(|item| OrganizeStateEntry {
                item,
                state: OrganizeStateKind::Unplanned,
                reason: reason.clone(),
            }));
            return Ok(out);
        }
        for id in &planned.tidy {
            out.extend(entry(id, OrganizeStateKind::Tidy, String::new()));
        }
        for id in &planned.locked {
            out.extend(entry(id, OrganizeStateKind::Locked, String::new()));
        }
        for id in &planned.listed {
            let (state, reason) = if planned.unscraped.contains(id) {
                (OrganizeStateKind::Unplanned, UNSCRAPED_REASON.to_owned())
            } else if planned.media_unread.contains(id) {
                (OrganizeStateKind::Unplanned, MEDIA_HELD_REASON.to_owned())
            } else if let Some(reason) = planned.unplanned.get(id) {
                (OrganizeStateKind::Unplanned, reason.clone())
            } else if planned.blocked.contains(id) {
                (OrganizeStateKind::Conflict, conflict_reason(id, planned))
            } else {
                // The reason of a pending rename says how much would change.
                let changes = planned
                    .resolved
                    .get(id)
                    .map_or(1, |item| move_counts(item).0 as usize);
                (
                    OrganizeStateKind::Pending,
                    crate::lifecycle::changes_text(changes),
                )
            };
            out.extend(entry(id, state, reason));
        }
        Ok(out)
    }

    /// Runs the checked changes of the request and records one journal batch.
    ///
    /// The request is planned again, so the disk is judged as it is now. A checked id that is not
    /// a clean change in the new plan (a conflict appeared, it is tidy now, it was skipped) is
    /// reported in [`Applied::not_run`] and left alone.
    ///
    /// # Errors
    /// Only what stops the whole batch: the library cannot be read or the journal cannot be
    /// written. A step that fails on the disk fails its item only.
    pub fn apply(&self, request: &ApplyRequest) -> Result<Applied, OrganizeError> {
        self.apply_as(request, new_batch_id())
    }

    fn apply_as(&self, request: &ApplyRequest, batch_id: String) -> Result<Applied, OrganizeError> {
        // No rescan walks a folder while it moves (see `hold_library`).
        let _held = self.hold_library()?;
        // Nothing else writes into a title while it is renamed (see `title_guard`). The titles
        // are held together, waiting (holding none) for short writes to end. A title whose NFO
        // or artwork is still being written after a scrape is left out at once, one still busy
        // after the wait is left out too; both are reported in `busy` and renamed next time.
        // They are held before the plan is made, so a move, rename or write that ended while
        // this waited is planned from where it left the title.
        let (mut titles, refused) = crate::title_guard::hold_each(
            &self.db,
            request
                .include
                .iter()
                .filter_map(|id| id.parse::<i64>().ok().map(ItemId)),
            crate::title_guard::Work::Rename,
            crate::title_guard::MOVE_WAIT,
        );
        let planned = self.plan(&request.preview)?;
        let wanted: BTreeSet<&str> = request.include.iter().map(String::as_str).collect();
        let mut movies = Vec::new();
        let mut shows = Vec::new();
        let mut run_ids = BTreeSet::new();
        let candidates: Vec<&String> = planned
            .listed
            .iter()
            .filter(|id| wanted.contains(id.as_str()) && !planned.blocked.contains(*id))
            .filter(|id| planned.resolved.contains_key(*id))
            .collect();
        // A held title that has nothing to rename now is let go at once.
        titles.retain(|id| candidates.iter().any(|c| *c == &id.0.to_string()));
        let refused: Vec<_> = refused
            .into_iter()
            .filter(|(id, _)| candidates.iter().any(|c| *c == &id.0.to_string()))
            .collect();
        let busy: Vec<BusyTitle> = refused
            .into_iter()
            .map(|(id, why)| BusyTitle {
                id: id.0.to_string(),
                reason: why.to_string(),
            })
            .collect();
        for id in candidates {
            if busy.iter().any(|b| &b.id == id) {
                continue;
            }
            let Some(item) = planned.resolved.get(id) else {
                continue;
            };
            run_ids.insert(id.clone());
            match planned.infos.get(id).map(|i| i.kind) {
                Some(RowKind::Show) => shows.push(item.clone()),
                _ => movies.push(item.clone()),
            }
        }
        let not_run: Vec<String> = request
            .include
            .iter()
            .filter(|id| !run_ids.contains(*id))
            .cloned()
            .collect();
        if movies.is_empty() && shows.is_empty() {
            return Ok(Applied {
                batch_id: None,
                items: Vec::new(),
                not_run,
                busy,
            });
        }

        let mut journal = self.journal_store();
        let fs = Arc::clone(&self.disk);
        let profile = &request.preview.profile;
        // A data source can lie in a folder of another one; no item takes a root away.
        let roots = self.source_roots()?;
        let mut reports = Vec::new();
        let choice = self.choice.deleted_files();
        let custom = choice
            .custom_folder()
            .map(|folder| folder.to_string_lossy().into_owned());
        let groups: Vec<(Vec<BatchItem>, ExecOptions)> = [
            (movies, profile.movie.exec_options(&batch_id)),
            (shows, profile.tv.exec_options(&batch_id)),
        ]
        .into_iter()
        .filter(|(items, _)| !items.is_empty())
        .map(|(items, mut options)| {
            if let Some(custom) = &custom {
                options.trash_dir.clone_from(custom);
            }
            (items, options.with_protected_dirs(roots.iter().cloned()))
        })
        .collect();
        // Every title of the batch is on record before the first one is renamed, so a stop finds
        // the titles that never started too: Resume finishes the batch the user applied.
        for (items, options) in &groups {
            Executor::new(fs.as_ref(), &mut journal, options.clone())?.mark_items(items)?;
        }
        for (items, options) in groups {
            let report = run_batch(fs.as_ref(), &mut journal, options, &items)?;
            reports.extend(report);
        }
        if choice.erases() {
            crate::trash_folders::erase_rename_leftovers(&self.db, &batch_id);
        }
        // A leftover the batch sent to the trash folder is no file of its title any more.
        let trashed: Vec<ItemId> = reports
            .iter()
            .filter(|r| r.trashed > 0 && r.status == ItemStatus::Done)
            .filter_map(|r| r.id.parse::<i64>().ok().map(ItemId))
            .collect();
        self.forget_missing_files(&trashed);
        let items = reports
            .into_iter()
            .map(|r| AppliedItem {
                title: planned
                    .infos
                    .get(&r.id)
                    .map(|i| i.title.clone())
                    .unwrap_or_default(),
                status: match r.status {
                    ItemStatus::Done => AppliedStatus::Done,
                    ItemStatus::Failed(why) => AppliedStatus::Failed(why),
                    ItemStatus::Canceled => AppliedStatus::Canceled,
                },
                moved: small(r.moved),
                copied: small(r.copied),
                trashed: small(r.trashed),
                notes: r.notes,
                id: r.id,
            })
            .collect::<Vec<AppliedItem>>();
        self.record_history(&batch_id, &planned, &items);
        let touched: Vec<ItemId> = items
            .iter()
            .filter_map(|i| i.id.parse::<i64>().ok().map(ItemId))
            .collect();
        self.note_changed(&touched);
        Ok(Applied {
            batch_id: Some(batch_id),
            items,
            not_run,
            busy,
        })
    }

    /// Writes the History entry of a batch that renamed at least one title (spec
    /// `operation-history`): its titles with their old and new folder or file names, how many
    /// folders and files it changed and the library folders involved. A History entry that
    /// cannot be written does not undo the rename; the journal still holds the batch.
    fn record_history(&self, batch_id: &str, planned: &Planned, items: &[AppliedItem]) {
        let mut renamed = Vec::new();
        for item in items.iter().filter(|i| i.status == AppliedStatus::Done) {
            let (Some(info), Some(batch)) =
                (planned.infos.get(&item.id), planned.resolved.get(&item.id))
            else {
                continue;
            };
            let Ok(id) = item.id.parse::<i64>() else {
                continue;
            };
            renamed.push(RenamedTitle {
                id: ItemId(id),
                title: with_year(&info.title, info.year),
                old_folder: info.old_folder.clone(),
                before_file: info.before_file.clone(),
                root: info.root.clone(),
                item: batch,
            });
        }
        let Some(entry) = rename_entry(batch_id, &renamed) else {
            return;
        };
        if let Err(error) = crate::history::record(&self.db, &entry) {
            tracing::warn!(batch = batch_id, %error, "the rename batch has no History entry");
        }
    }

    /// Drops the files of `ids` that are no longer on disk (the leftovers an apply moved to the
    /// trash folder), so the next plan does not list them again. A video is never
    /// dropped: a missing video is the Missing flag's business. A rescan lists a file again when
    /// an undo brings it back.
    fn forget_missing_files(&self, ids: &[ItemId]) {
        for &id in ids {
            let result = self.db.write(move |tx| {
                let Some(mut item) = cv_db::repo::get_item(tx, id)? else {
                    return Ok(());
                };
                let before = item.core.media_files.len();
                item.core
                    .media_files
                    .retain(|f| f.kind == MediaFileType::Video || f.path().exists());
                if item.core.media_files.len() != before {
                    cv_db::repo::update_item(tx, &item)?;
                }
                Ok(())
            });
            if let Err(error) = result {
                tracing::warn!(item = id.0, %error, "the trashed files stayed listed");
            }
        }
    }

    /// Every organize batch, newest first.
    ///
    /// # Errors
    /// The journal cannot be read.
    pub fn journal(&self) -> Result<Vec<JournalBatch>, OrganizeError> {
        self.recent_journal(usize::MAX)
    }

    /// The newest `limit` batches of [`Self::journal`]: what Activity's history lists.
    ///
    /// # Errors
    /// The journal cannot be read.
    pub fn recent_journal(&self, limit: usize) -> Result<Vec<JournalBatch>, OrganizeError> {
        let journal = DbJournal::new(Arc::clone(&self.db));
        let mut out = Vec::new();
        for b in journal.batches()?.into_iter().take(limit) {
            let ids = self.db.read(|c| batch_item_ids(c, &b.batch_id))?;
            let titles = ids
                .into_iter()
                .filter_map(|id| self.title_of(ItemId(id)))
                .collect();
            let activity = self
                .db
                .read(|c| cv_db::repo::rename_journal::batch_activity(c, &b.batch_id))?;
            // The plans the items ran with say how many files each moved, folder renames
            // included; a move between data sources records its copies instead.
            let planned_moves: u32 = journal
                .item_marks(&b.batch_id)?
                .iter()
                .filter(|mark| mark.state == cv_rename::exec::ItemState::Done)
                .filter_map(|mark| cv_rename::exec::ItemPlan::parse(&mark.plan))
                .map(|plan| move_counts(&plan.item).0)
                .sum();
            let files = if planned_moves > 0 {
                planned_moves
            } else {
                small64(activity.copied)
            };
            out.push(JournalBatch {
                files,
                undone_ms: activity.undone_ms,
                batch_id: b.batch_id,
                created_ms: b.created_ms,
                items: small64(b.items),
                operations: small64(b.operations),
                done: small64(b.done),
                undone: small64(b.undone),
                failed: small64(b.failed),
                planned: small64(b.planned),
                titles,
            });
        }
        Ok(out)
    }

    /// Takes a batch back in reverse. Items whose files were changed since are left as they are
    /// and reported; the others are restored.
    ///
    /// # Errors
    /// The batch is unknown, or the journal cannot be read or written.
    pub fn undo(&self, batch_id: &str) -> Result<UndoOutcome, OrganizeError> {
        self.undo_items(batch_id, None)
    }

    /// [`Self::undo`] for some titles of the batch only (`None`: every title). History's
    /// per-title Undo and its newer-entry check take back only the titles they may.
    ///
    /// # Errors
    /// See [`Self::undo`].
    pub fn undo_items(
        &self,
        batch_id: &str,
        only: Option<&[ItemId]>,
    ) -> Result<UndoOutcome, OrganizeError> {
        let only: Option<Vec<String>> =
            only.map(|ids| ids.iter().map(|id| id.0.to_string()).collect());
        let _held = self.hold_library()?;
        let mut journal = self.journal_store();
        let rows = journal.batch(batch_id)?;
        if rows.is_empty() {
            return Err(OrganizeError::UnknownBatch(batch_id.to_owned()));
        }
        // Nothing else writes into the titles while they go back (see `title_guard`).
        let titles: Vec<ItemId> = self
            .db
            .read(|c| batch_item_ids(c, batch_id))?
            .into_iter()
            .map(ItemId)
            .filter(|id| {
                only.as_ref()
                    .is_none_or(|ids| ids.contains(&id.0.to_string()))
            })
            .collect();
        let work = if crate::library_ops::is_move_batch(batch_id) {
            crate::title_guard::Work::Move
        } else {
            crate::title_guard::Work::Rename
        };
        let _titles =
            crate::title_guard::hold_items(&self.db, &titles, work, crate::title_guard::MOVE_WAIT)
                .map_err(|e| OrganizeError::Move(e.to_string()))?;
        let roots = self.source_roots()?;
        if crate::library_ops::is_move_batch(batch_id) {
            // A move between data sources has rows of its own kind; the move engine takes it
            // back, and an unfinished one is resumed or rolled back first.
            if rows
                .iter()
                .any(|row| row.state == cv_rename::exec::JournalState::Planned)
            {
                return Err(OrganizeError::Move(
                    "This move is unfinished. Resume it or roll it back from Activity first."
                        .to_owned(),
                ));
            }
            return self.undo_move(batch_id, &mut journal, only.as_deref());
        }
        let options = UndoOptions {
            roots,
            only_items: only.clone(),
            ..UndoOptions::default()
        };
        let disk = Arc::clone(&self.disk);
        let fs: &dyn Fs = disk.as_ref();
        let report = match undo_batch(&mut journal, fs, batch_id, &options) {
            Err(UndoError::Unsettled { .. }) => {
                // A crash left rows half done; settle them, then try once more. The rows of a
                // move between data sources are left to the move engine.
                recover(&mut crate::library_ops::ExceptMoves(journal.clone()), fs)?;
                undo_batch(&mut journal, fs, batch_id, &options)?
            }
            other => other?,
        };
        if report.is_complete() && only.is_none() {
            // The whole batch is back: a title a stop had left half renamed is no longer
            // offered to Resume.
            settle_interrupted(&mut journal, batch_id)?;
        }
        let mut outcome = UndoOutcome {
            batch_id: batch_id.to_owned(),
            ..UndoOutcome::default()
        };
        for item in report.items {
            let title = item
                .id
                .as_deref()
                .and_then(|id| id.parse::<i64>().ok())
                .and_then(|id| self.title_of(ItemId(id)))
                .unwrap_or_default();
            match item.status {
                UndoStatus::Restored => outcome.restored += 1,
                UndoStatus::NotRestorable(list) => {
                    for entry in list {
                        outcome.not_restorable.push(NotRestorable {
                            id: item.id.clone(),
                            title: title.clone(),
                            path: entry.path,
                            reason: entry.reason,
                        });
                    }
                }
                UndoStatus::Failed(reason) => outcome.failed.push(NotRestorable {
                    id: item.id.clone(),
                    title,
                    path: String::new(),
                    reason,
                }),
            }
        }
        let touched = self
            .db
            .read(|c| batch_item_ids(c, batch_id))?
            .into_iter()
            .map(ItemId)
            .collect::<Vec<_>>();
        self.note_changed(&touched);
        Ok(outcome)
    }

    /// Takes a finished move between data sources back (see `library_ops::MoveEngine::undo`).
    fn undo_move(
        &self,
        batch_id: &str,
        journal: &mut cv_rename::exec::DbJournal,
        only: Option<&[String]>,
    ) -> Result<UndoOutcome, OrganizeError> {
        use crate::library_ops::{ChangeSourceService, TakeBackStatus};

        // A title whose originals are gone from the trash folder is moved back from its new
        // place as a new verified move; the others come back from the trash folder.
        let mut service = ChangeSourceService::new(Arc::clone(&self.db));
        if let Some(bin) = &self.bin {
            service = service.with_bin(Arc::clone(bin));
        }
        if let Some(observer) = &self.observer {
            service = service.with_path_observer(Arc::clone(observer));
        }
        let taken = service
            .undo_held(batch_id, journal, only)
            .map_err(|e| OrganizeError::Move(e.to_string()))?;
        let mut outcome = UndoOutcome {
            batch_id: batch_id.to_owned(),
            ..UndoOutcome::default()
        };
        for item in taken {
            let title = item
                .item
                .as_deref()
                .and_then(|id| id.parse::<i64>().ok())
                .and_then(|id| self.title_of(ItemId(id)))
                .unwrap_or_default();
            match item.status {
                TakeBackStatus::Restored => outcome.restored += 1,
                TakeBackStatus::NotRestorable(list) => {
                    for (path, reason) in list {
                        outcome.not_restorable.push(NotRestorable {
                            id: item.item.clone(),
                            title: title.clone(),
                            path,
                            reason,
                        });
                    }
                }
                TakeBackStatus::Failed(reason) => outcome.failed.push(NotRestorable {
                    id: item.item.clone(),
                    title,
                    path: String::new(),
                    reason,
                }),
            }
        }
        let touched = self
            .db
            .read(|c| batch_item_ids(c, batch_id))?
            .into_iter()
            .map(ItemId)
            .collect::<Vec<_>>();
        self.note_changed(&touched);
        Ok(outcome)
    }

    /// Holds every data source of the library against rescans until the guard is dropped. A
    /// rescan that walked a show folder before it was renamed and read the library after would
    /// not find the show under the old name and add it a second time, so the files move and the
    /// library follows while no rescan runs.
    fn hold_library(&self) -> Result<cv_scan::lock::SourceGuard, OrganizeError> {
        let ids: Vec<i64> = self
            .db
            .read(|c| list_sources(c, None))?
            .into_iter()
            .map(|s| s.id)
            .collect();
        Ok(cv_scan::lock::hold_sources(ids))
    }

    /// The roots of every data source of the library.
    fn source_roots(&self) -> Result<Vec<PathBuf>, OrganizeError> {
        Ok(self
            .db
            .read(|c| list_sources(c, None))?
            .into_iter()
            .map(|s| PathBuf::from(s.path))
            .collect())
    }

    /// The title of a library item with its year, as a conflict names the title in the way:
    /// `Blade Runner (1982)`. `None` for an id the library does not know.
    fn labelled_title(&self, id: &str) -> Option<String> {
        let id = ItemId(id.parse::<i64>().ok()?);
        let summary = self
            .db
            .read(|c| cv_db::repo::get_summary(c, id))
            .ok()
            .flatten()?;
        Some(with_year(&summary.title, summary.year))
    }

    fn title_of(&self, id: ItemId) -> Option<String> {
        self.db
            .read(|c| cv_db::repo::get_summary(c, id))
            .ok()
            .flatten()
            .map(|s| s.title)
    }

    // ---- planning ---------------------------------------------------------------------------

    fn plan(&self, request: &PreviewRequest) -> Result<Planned, OrganizeError> {
        let profile = &request.profile;
        let ids = scope_ids(&self.db, &request.scope)?;
        let loaded = load(&self.db, &ids)?;
        let has_movies = loaded.iter().any(|l| matches!(l, Loaded::Movie(_)));
        let has_shows = loaded.iter().any(|l| matches!(l, Loaded::Show(_)));

        let mut planned = Planned {
            infos: BTreeMap::new(),
            listed: Vec::new(),
            unplanned: BTreeMap::new(),
            resolved: BTreeMap::new(),
            original: BTreeMap::new(),
            skipped: BTreeSet::new(),
            blocked: BTreeSet::new(),
            unscraped: BTreeSet::new(),
            media_unread: BTreeSet::new(),
            tidy: BTreeSet::new(),
            locked: BTreeSet::new(),
            report: ConflictReport::default(),
            counts: Counts::default(),
            examples: Vec::new(),
            problems: Vec::new(),
        };

        // A template that does not work blocks the preview; the editor shows why.
        let patterns = super::patterns::PatternSet::of(profile);
        for field in PatternField::ALL {
            let relevant = if field.is_tv() { has_shows } else { true };
            if !relevant || (!has_movies && !has_shows && field.is_tv()) {
                continue;
            }
            if let Some(finding) = check_pattern(field, patterns.get(field))
                .into_iter()
                .find(|f| f.severity == FindingSeverity::Error)
            {
                planned.problems.push(TemplateProblem {
                    field,
                    message: finding.message,
                });
            }
        }
        if !planned.problems.is_empty() {
            return Ok(planned);
        }

        let movie_renamer = MovieRenamer::new(profile.movie.clone());
        let tv_renamer = TvRenamer::new(profile.tv.clone());
        // A rename only follows a scrape: a title that is not scraped (new, not matched, waiting
        // for a decision) keeps its names, so made-up names never reach the disk.
        let scraped = if self.plan_unscraped {
            ids.iter().copied().collect()
        } else {
            self.db
                .read(|c| cv_db::repo::lifecycle::scraped_items(c, &ids))?
        };

        let mut items: Vec<BatchItem> = Vec::new();
        for entry in &loaded {
            match entry {
                Loaded::Unplannable {
                    id,
                    kind,
                    title,
                    year,
                    reason,
                } => {
                    let key = id.0.to_string();
                    planned.infos.insert(
                        key.clone(),
                        Info {
                            kind: if *kind == cv_domain::ItemKind::Show {
                                RowKind::Show
                            } else {
                                RowKind::Movie
                            },
                            title: title.clone(),
                            year: *year,
                            root: String::new(),
                            old_folder: String::new(),
                            before_file: String::new(),
                        },
                    );
                    planned.unplanned.insert(key.clone(), reason.clone());
                    planned.listed.push(key);
                    planned.counts.unplanned += 1;
                }
                Loaded::Movie(movie) => {
                    let key = movie.id.0.to_string();
                    if movie.item.core.locked {
                        planned.counts.locked += 1;
                        planned.locked.insert(key);
                        continue;
                    }
                    if !scraped.contains(&movie.id) {
                        planned.infos.insert(
                            key.clone(),
                            Info {
                                kind: RowKind::Movie,
                                title: movie.item.core.title.clone(),
                                year: movie.item.core.year,
                                root: movie.root.clone(),
                                old_folder: movie.folder.clone(),
                                before_file: main_video_name(&movie.files),
                            },
                        );
                        planned.unscraped.insert(key.clone());
                        planned.listed.push(key);
                        continue;
                    }
                    // A data source root that is the movie's own folder (the first-run split adds
                    // `Mixed\Dune (2021)` as a movie folder) cannot be renamed and must never
                    // get the movie's folder inside it: its files are renamed where they are.
                    let own_root =
                        movie.folder.is_empty() && root_is_movie_folder(&movie_renamer, movie);
                    let at_root;
                    let (renamer, item) = if own_root {
                        at_root = (
                            MovieRenamer::new(cv_rename::MovieSettings {
                                rename_folder: false,
                                ..profile.movie.clone()
                            }),
                            in_place_movie(movie),
                        );
                        (&at_root.0, &at_root.1)
                    } else {
                        (&movie_renamer, &movie.item)
                    };
                    // Names with quality tags wait for the video details, so a rename never
                    // strips the tags a name already has.
                    if renamer.waits_for_media_info(&movie.item) {
                        planned.infos.insert(
                            key.clone(),
                            Info {
                                kind: RowKind::Movie,
                                title: movie.item.core.title.clone(),
                                year: movie.item.core.year,
                                root: movie.root.clone(),
                                old_folder: movie.folder.clone(),
                                before_file: main_video_name(&movie.files),
                            },
                        );
                        planned.media_unread.insert(key.clone());
                        planned.listed.push(key);
                        continue;
                    }
                    let input = MovieInput {
                        item,
                        set: movie.set.as_ref(),
                        folder: &movie.folder,
                        files: &movie.files,
                    };
                    match renamer.plan(&input) {
                        Ok(plan) if !plan.needs_rename() => {
                            planned.counts.tidy += 1;
                            planned.tidy.insert(key);
                        }
                        Ok(plan) => {
                            let mut item = BatchItem::from_movie(&key, &movie.root, &plan);
                            if let Some(label) = version_label(movie) {
                                item = item.with_version_label(label);
                            }
                            planned.infos.insert(
                                key.clone(),
                                Info {
                                    kind: RowKind::Movie,
                                    title: movie.item.core.title.clone(),
                                    year: movie.item.core.year,
                                    root: movie.root.clone(),
                                    old_folder: movie.folder.clone(),
                                    before_file: main_video_name(&movie.files),
                                },
                            );
                            planned.listed.push(key);
                            items.push(item);
                        }
                        Err(error) => {
                            planned.infos.insert(
                                key.clone(),
                                Info {
                                    kind: RowKind::Movie,
                                    title: movie.item.core.title.clone(),
                                    year: movie.item.core.year,
                                    root: movie.root.clone(),
                                    old_folder: movie.folder.clone(),
                                    before_file: main_video_name(&movie.files),
                                },
                            );
                            planned
                                .unplanned
                                .insert(key.clone(), unplanned_text(&error));
                            planned.listed.push(key);
                            planned.counts.unplanned += 1;
                        }
                    }
                }
                Loaded::Show(show) => {
                    let key = show.id.0.to_string();
                    if show.show.core.locked {
                        planned.counts.locked += 1;
                        planned.locked.insert(key);
                        continue;
                    }
                    if !scraped.contains(&show.id) {
                        planned.infos.insert(
                            key.clone(),
                            Info {
                                kind: RowKind::Show,
                                title: show.show.core.title.clone(),
                                year: show.show.core.year,
                                root: show.root.clone(),
                                old_folder: show.folder.clone(),
                                before_file: first_episode_name(show),
                            },
                        );
                        planned.unscraped.insert(key.clone());
                        planned.listed.push(key);
                        continue;
                    }
                    let episodes: Vec<EpisodeInput<'_>> = show
                        .episodes
                        .iter()
                        .map(|(item, files)| EpisodeInput { item, files })
                        .collect();
                    let input = TvInput {
                        show: &show.show,
                        folder: &show.folder,
                        files: &show.files,
                        seasons: &show.seasons,
                        episodes: &episodes,
                    };
                    let info = Info {
                        kind: RowKind::Show,
                        title: show.show.core.title.clone(),
                        year: show.show.core.year,
                        root: show.root.clone(),
                        old_folder: show.folder.clone(),
                        before_file: first_episode_name(show),
                    };
                    if tv_renamer.waits_for_media_info(&input) {
                        planned.infos.insert(key.clone(), info);
                        planned.media_unread.insert(key.clone());
                        planned.listed.push(key);
                        continue;
                    }
                    match tv_renamer.plan(&input) {
                        Ok(plan) if !plan.needs_rename() => {
                            planned.counts.tidy += 1;
                            planned.tidy.insert(key);
                        }
                        Ok(plan) => {
                            planned.infos.insert(key.clone(), info);
                            planned.listed.push(key.clone());
                            items.push(BatchItem::from_tv(&key, &show.root, &plan));
                        }
                        Err(error) => {
                            planned.infos.insert(key.clone(), info);
                            planned
                                .unplanned
                                .insert(key.clone(), unplanned_text(&error));
                            planned.listed.push(key);
                            planned.counts.unplanned += 1;
                        }
                    }
                }
            }
        }

        // Conflicts, the user's resolutions, and conflicts again.
        let mut options = ConflictOptions::native();
        options.name_options = profile.movie.name.clone();
        options.space = profile.movie.folder_space.clone();
        let db = &*self.db;
        let owner = |root: &str, path: &str| owner_of(db, root, path);
        let disk = FsDisk::new().with_owner(&owner);

        for item in &items {
            planned.original.insert(item.id.clone(), item.clone());
        }
        let (final_items, skipped) = if request.choices.is_empty() {
            (items, Vec::new())
        } else {
            let choices = to_resolutions(&request.choices);
            let resolved = resolve(&items, &choices, &disk as &dyn Disk, &options)?;
            (resolved.items, resolved.skipped)
        };
        let report = detect(&final_items, &disk, &options);
        planned.blocked = report
            .blocked_items()
            .into_iter()
            .map(str::to_owned)
            .collect();
        planned.skipped = skipped.into_iter().collect();
        planned.report = report;
        for item in final_items {
            planned.resolved.insert(item.id.clone(), item);
        }

        // Counts.
        for id in &planned.listed {
            if planned.unplanned.contains_key(id) {
                continue;
            }
            if planned.skipped.contains(id) {
                planned.counts.skipped += 1;
            } else if planned.blocked.contains(id) {
                planned.counts.conflicts += 1;
            } else if let Some(item) = planned.resolved.get(id) {
                planned.counts.change += 1;
                let (moves, _) = move_counts(item);
                planned.counts.moves += moves;
            }
        }

        // Live examples: library titles with the edge cases, samples for the rest.
        let movies: Vec<&LoadedMovie> = loaded
            .iter()
            .filter_map(|l| match l {
                Loaded::Movie(m) => Some(m),
                _ => None,
            })
            .collect();
        planned.examples = examples(&movies, &movie_renamer);
        Ok(planned)
    }
}

fn run_batch(
    fs: &dyn Fs,
    journal: &mut dyn JournalStore,
    options: ExecOptions,
    items: &[BatchItem],
) -> Result<Vec<cv_rename::exec::ItemReport>, OrganizeError> {
    let report = Executor::new(fs, journal, options)?.run(items)?;
    Ok(report.items)
}

fn unplanned_text(error: &RenameError) -> String {
    match error {
        RenameError::NoTitle => "The title has no name to build a folder from.".to_owned(),
        other => other.to_string(),
    }
}

fn native_join(root: &str, path: &str) -> String {
    let sep = std::path::MAIN_SEPARATOR_STR;
    let root = root.trim_end_matches(['\\', '/']);
    format!("{root}{sep}{}", path.replace('/', sep))
}

/// The library item that owns `path` below `root`: a file's owner, else a folder's.
fn owner_of(db: &Database, root: &str, path: &str) -> Option<String> {
    let absolute = native_join(root, path);
    db.read(|c| match owner_of_file(c, &absolute)? {
        Some(id) => Ok(Some(id)),
        None => owner_of_folder(c, &absolute),
    })
    .ok()
    .flatten()
    .map(|id| id.to_string())
}

fn to_resolutions(choices: &BTreeMap<String, Choice>) -> BTreeMap<String, Resolution> {
    choices
        .iter()
        .map(|(id, choice)| {
            let resolution = match choice {
                Choice::KeepAsVersions => Resolution::KeepAsVersions,
                Choice::Skip => Resolution::Skip,
                Choice::Edit { folder, base } => Resolution::Edit(NameEdit {
                    folder: folder.clone().filter(|f| !f.trim().is_empty()),
                    base: base.clone().filter(|b| !b.trim().is_empty()),
                }),
            };
            (id.clone(), resolution)
        })
        .collect()
}

/// Whether the data source root of a movie that sits directly in it is the movie's own folder:
/// the root's name is the folder name the profile gives, or names the same title and year
/// (`Dune.2021.2160p` for Dune (2021)). Such a root is not a folder of loose movies.
fn root_is_movie_folder(renamer: &MovieRenamer, movie: &LoadedMovie) -> bool {
    let root_name = movie
        .root
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default();
    if root_name.is_empty() {
        return false;
    }
    let same = |a: &str, b: &str| {
        let fold = |s: &str| -> String {
            s.chars()
                .filter(|c| c.is_alphanumeric())
                .flat_map(char::to_lowercase)
                .collect()
        };
        let (a, b) = (fold(a), fold(b));
        !a.is_empty() && a == b
    };
    if let Ok(rendered) = renamer.folder_name(&movie.item, movie.set.as_ref())
        && let Some(last) = rendered.trim_end_matches('/').rsplit('/').next()
        && last.eq_ignore_ascii_case(root_name)
    {
        return true;
    }
    let found =
        cv_scan::parse::title::detect_clean_title_and_year_of_folder::<&str>(root_name, &[]);
    let core = &movie.item.core;
    let year_fits = core.year == 0 || found.year == Some(core.year);
    year_fits
        && found.year.is_some()
        && (same(&found.title, &core.title) || same(&found.title, &core.original_title))
}

/// The movie as planned in its own root folder: not one of several loose movies there.
fn in_place_movie(movie: &LoadedMovie) -> cv_domain::Item {
    let mut item = movie.item.clone();
    if let cv_domain::ItemDetail::Movie(fields) = &mut item.detail {
        fields.multi_movie_dir = false;
    }
    item
}

/// `1080p`, `2160p` and so on from the movie's main video, to tell versions apart.
fn version_label(movie: &LoadedMovie) -> Option<String> {
    let video = movie
        .item
        .core
        .media_files
        .iter()
        .find(|f| f.kind == MediaFileType::Video)?;
    let label = match video.height.max(video.width * 9 / 16) {
        2000.. => "2160p",
        900.. => "1080p",
        600.. => "720p",
        400.. => "480p",
        _ => return None,
    };
    Some(label.to_owned())
}

fn main_video_name(files: &[cv_rename::PlanFile]) -> String {
    files
        .iter()
        .filter(|f| f.kind == MediaFileType::Video)
        .min_by(|a, b| (a.stacking, &a.path).cmp(&(b.stacking, &b.path)))
        .map(|f| f.name().to_owned())
        .unwrap_or_default()
}

fn first_episode_name(show: &super::load::LoadedShow) -> String {
    show.episodes
        .iter()
        .flat_map(|(_, files)| files.iter())
        .filter(|f| f.kind == MediaFileType::Video)
        .map(|f| f.name().to_owned())
        .min()
        .unwrap_or_default()
}

/// Files the item moves or copies, and files it trashes.
/// A title a rename batch renamed, as its History entry names it.
struct RenamedTitle<'a> {
    id: ItemId,
    /// With the year: "Heat (1995)".
    title: String,
    old_folder: String,
    /// The main video's name before the rename.
    before_file: String,
    /// The library folder.
    root: String,
    item: &'a BatchItem,
}

/// The History entry of the titles a rename batch renamed (spec `operation-history`): their old
/// and new folder or file names, how many folders and files it changed and the library folders
/// involved. `None` when it renamed none.
fn rename_entry(batch_id: &str, renamed: &[RenamedTitle<'_>]) -> Option<crate::history::NewEntry> {
    if renamed.is_empty() {
        return None;
    }
    let mut titles = Vec::new();
    let (mut folders, mut files) = (0usize, 0u32);
    let mut roots: BTreeSet<String> = BTreeSet::new();
    for title in renamed {
        let batch = title.item;
        let folder_changed = batch.old_folder != batch.new_folder;
        if folder_changed {
            folders += 1;
        }
        files += move_counts(batch).0;
        roots.insert(title.root.clone());
        let (before, after) = if folder_changed {
            (
                crate::history::last_part(&title.old_folder),
                crate::history::last_part(&batch.new_folder),
            )
        } else {
            (title.before_file.clone(), after_file(batch))
        };
        titles.push(crate::history::NewTitle {
            item_id: title.id,
            title: title.title.clone(),
            before,
            after,
        });
    }
    let summary = match titles.as_slice() {
        [one] => format!("Renamed {}", one.title),
        many => format!("Renamed {}", crate::history::titles_word(many.len())),
    };
    let mut line = format!(
        "{} {}, {} {}",
        folders,
        if folders == 1 { "folder" } else { "folders" },
        files,
        if files == 1 { "file" } else { "files" }
    );
    for root in &roots {
        line.push_str(" · ");
        line.push_str(root);
    }
    Some(crate::history::NewEntry {
        kind: crate::history::HistoryKind::Rename,
        summary,
        line,
        bytes: None,
        extra: serde_json::json!({ "folders": folders, "files": files }),
        batch_ref: Some(batch_id.to_owned()),
        titles,
    })
}

/// Writes (or completes) the History entry of a rename batch from its journal: every title the
/// batch renamed whole, from the plan kept with it. For a batch that Resume finished, or whose
/// entry a crash kept from being written, so History can undo it. A batch journaled without
/// plans (a clean-up, or an older build) gets none.
///
/// # Errors
/// The journal or the library cannot be read or written.
pub(crate) fn record_rename_from_journal(
    db: &Arc<Database>,
    batch_id: &str,
) -> Result<Option<i64>, OrganizeError> {
    let marks = DbJournal::new(Arc::clone(db)).item_marks(batch_id)?;
    let plans: Vec<(ItemId, cv_rename::exec::ItemPlan)> = marks
        .iter()
        .filter(|m| m.state == cv_rename::exec::ItemState::Done)
        .filter_map(|m| {
            let id = ItemId(m.item_id.parse::<i64>().ok()?);
            Some((id, serde_json::from_str(&m.plan).ok()?))
        })
        .collect();
    let mut renamed = Vec::new();
    for (id, plan) in &plans {
        let id = *id;
        let title = db
            .read(move |conn| cv_db::repo::get_summary(conn, id))?
            .map_or_else(String::new, |s| with_year(&s.title, s.year));
        renamed.push(RenamedTitle {
            id,
            title,
            old_folder: plan.item.old_folder.clone(),
            before_file: before_file(&plan.item),
            root: plan.item.root.clone(),
            item: &plan.item,
        });
    }
    let Some(entry) = rename_entry(batch_id, &renamed) else {
        return Ok(None);
    };
    Ok(Some(crate::history::record_batch(db, &entry)?))
}

/// The main video's name before the rename, from a plan.
fn before_file(item: &BatchItem) -> String {
    item.files
        .iter()
        .filter(|f| f.kind == MediaFileType::Video)
        .map(|f| {
            f.from
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(&f.from)
                .to_owned()
        })
        .min()
        .unwrap_or_default()
}

fn move_counts(item: &BatchItem) -> (u32, u32) {
    let (mut moves, mut trashed) = (0u32, 0u32);
    for op in item.ops() {
        match op.kind {
            OpKind::Move | OpKind::Copy => moves += 1,
            OpKind::Trash => trashed += 1,
            OpKind::WriteNfo => {}
        }
    }
    (moves, trashed)
}

fn companions(item: &BatchItem) -> CompanionCounts {
    let mut counts = CompanionCounts::default();
    for file in &item.files {
        if !matches!(file.fate, Fate::Carried { .. }) {
            continue;
        }
        match file.kind {
            MediaFileType::Video => {}
            MediaFileType::Nfo => counts.nfo += 1,
            MediaFileType::Subtitle => counts.subtitles += 1,
            MediaFileType::Trailer => counts.trailers += 1,
            kind if kind.is_graphic() => counts.artwork += 1,
            _ => counts.other += 1,
        }
    }
    counts
}

/// The main video's name after the rename: the first video's first destination.
fn after_file(item: &BatchItem) -> String {
    item.files
        .iter()
        .filter(|f| f.kind == MediaFileType::Video)
        .filter_map(|f| match &f.fate {
            Fate::Carried { to } => to.first().cloned(),
            _ => Some(f.from.clone()),
        })
        .map(|path| path.rsplit('/').next().unwrap_or(&path).to_owned())
        .min()
        .unwrap_or_default()
}

fn small(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

fn small64(n: u64) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

fn root_with_separator(root: &str) -> String {
    format!(
        "{}{}",
        root.trim_end_matches(['\\', '/']),
        std::path::MAIN_SEPARATOR
    )
}

/// Names a library item by its id: its title and year.
type TitleLookup<'a> = &'a dyn Fn(&str) -> Option<String>;

/// `Heat (1995)`, or the title alone without a year.
fn with_year(title: &str, year: u16) -> String {
    if year > 0 {
        format!("{title} ({year})")
    } else {
        title.to_owned()
    }
}

/// A plain sentence for why the item's rename collides, from its first conflict.
fn conflict_reason(id: &str, planned: &Planned) -> String {
    let Some(conflict) = planned.report.for_item(id).next() else {
        return "The rename would collide with another title.".to_owned();
    };
    let others: Vec<String> = conflict
        .items
        .iter()
        .filter(|other| other.as_str() != id)
        .filter_map(|other| planned.infos.get(other))
        .map(|info| with_year(&info.title, info.year))
        .collect();
    match &conflict.kind {
        ConflictKind::DuplicateDestination if !others.is_empty() => {
            format!("It would get the same name as {}.", others.join(", "))
        }
        ConflictKind::DuplicateDestination => {
            "Two of its files would get the same name.".to_owned()
        }
        ConflictKind::ForeignDestination { is_dir: true, .. } => {
            "The folder already exists.".to_owned()
        }
        ConflictKind::ForeignDestination { is_dir: false, .. } => {
            "A file with the new name already exists.".to_owned()
        }
        ConflictKind::PathTooLong { length, limit } => {
            format!("The new path would be {length} characters long; the limit is {limit}.")
        }
        ConflictKind::NameTooLong { length, limit } => {
            format!(
                "A name in the new path would be {length} characters long; the limit is {limit}."
            )
        }
        ConflictKind::CaseOnlyRename => {
            "Only the letter case of the name changes, which this volume treats as the same name."
                .to_owned()
        }
    }
}

fn conflict_row(conflict: &Conflict, planned: &Planned, title: TitleLookup<'_>) -> ConflictRow {
    let code = match &conflict.kind {
        ConflictKind::DuplicateDestination => ConflictCode::Duplicate,
        // The owner is named, never shown by its id: a title the library no longer knows is an
        // unnamed occupant.
        ConflictKind::ForeignDestination { owner, .. } => ConflictCode::Foreign {
            owner: owner.as_ref().and_then(|id| {
                planned
                    .infos
                    .get(id)
                    .map(|i| with_year(&i.title, i.year))
                    .or_else(|| title(id))
            }),
        },
        ConflictKind::PathTooLong { length, limit } => ConflictCode::PathTooLong {
            length: small(*length),
            limit: small(*limit),
        },
        ConflictKind::NameTooLong { length, limit } => ConflictCode::NameTooLong {
            length: small(*length),
            limit: small(*limit),
        },
        ConflictKind::CaseOnlyRename => ConflictCode::CaseOnly,
    };
    let mut titles = Vec::new();
    let mut folders = Vec::new();
    for id in &conflict.items {
        let info = planned.infos.get(id);
        titles.push(info.map(|i| i.title.clone()).unwrap_or_default());
        folders.push(info.map(|i| i.old_folder.clone()).unwrap_or_default());
    }
    ConflictRow {
        code,
        items: conflict.items.clone(),
        titles,
        folders,
        path: conflict.path.clone(),
        options: conflict
            .resolutions()
            .iter()
            .map(|kind| match kind {
                ResolutionKind::KeepAsVersions => ResolutionOption::KeepAsVersions,
                ResolutionKind::Skip => ResolutionOption::Skip,
                ResolutionKind::Edit => ResolutionOption::Edit,
            })
            .collect(),
    }
}

/// Two items that collide usually collide on the folder and on every file in it. The user
/// resolves the pair once, so conflicts of one kind between the same items become one row, named
/// after its shortest (top-most) path.
fn merge_conflicts(rows: Vec<ConflictRow>) -> Vec<ConflictRow> {
    let mut merged: Vec<ConflictRow> = Vec::new();
    for row in rows {
        let same = merged.iter_mut().find(|m| {
            std::mem::discriminant(&m.code) == std::mem::discriminant(&row.code)
                && m.items == row.items
        });
        match same {
            Some(existing) => {
                let depth = |p: &str| (p.matches('/').count(), p.len());
                if depth(&row.path) < depth(&existing.path) {
                    existing.path = row.path;
                    existing.code = row.code;
                }
            }
            None => merged.push(row),
        }
    }
    merged
}

/// The files of an item that change, for the preview.
fn file_changes(item: &BatchItem) -> Vec<FileChange> {
    item.files
        .iter()
        .filter(|f| !f.is_unchanged())
        .map(|f| FileChange {
            kind: f.kind,
            from: f.from.clone(),
            to: match &f.fate {
                Fate::Carried { to } => to.clone(),
                Fate::Trash | Fate::LeftBehind => Vec::new(),
            },
        })
        .collect()
}

/// Turns the plan into the answer.
fn describe(planned: Planned, title: TitleLookup<'_>) -> Preview {
    if !planned.problems.is_empty() {
        return Preview {
            problems: planned.problems,
            ..Preview::default()
        };
    }
    let mut rows = Vec::new();
    for id in &planned.listed {
        let Some(info) = planned.infos.get(id) else {
            continue;
        };
        let mut row = PreviewRow {
            id: id.clone(),
            kind: info.kind,
            title: info.title.clone(),
            year: info.year,
            root: if info.root.is_empty() {
                String::new()
            } else {
                root_with_separator(&info.root)
            },
            before: info.old_folder.clone(),
            after: info.old_folder.clone(),
            before_file: info.before_file.clone(),
            after_file: info.before_file.clone(),
            status: RowStatus::Change,
            moves: 0,
            trashed: 0,
            companions: CompanionCounts::default(),
            files: Vec::new(),
        };
        if planned.unscraped.contains(id) {
            row.status = RowStatus::Unscraped;
            rows.push(row);
            continue;
        }
        if planned.media_unread.contains(id) {
            row.status = RowStatus::MediaUnread;
            rows.push(row);
            continue;
        }
        if let Some(reason) = planned.unplanned.get(id) {
            row.status = RowStatus::Unplanned(reason.clone());
            rows.push(row);
            continue;
        }
        let skipped = planned.skipped.contains(id);
        let item = if skipped {
            planned.original.get(id)
        } else {
            planned.resolved.get(id)
        };
        if let Some(item) = item {
            row.after = item.new_folder.clone();
            row.after_file = after_file(item);
            let (moves, trashed) = move_counts(item);
            row.moves = moves;
            row.trashed = trashed;
            row.companions = companions(item);
            row.files = file_changes(item);
        }
        row.status = if skipped {
            RowStatus::Skipped
        } else if planned.blocked.contains(id) {
            RowStatus::Conflict
        } else {
            RowStatus::Change
        };
        rows.push(row);
    }
    let conflicts = merge_conflicts(
        planned
            .report
            .conflicts
            .iter()
            .map(|c| conflict_row(c, &planned, title))
            .collect(),
    );
    Preview {
        rows,
        conflicts,
        counts: planned.counts,
        examples: planned.examples,
        problems: Vec::new(),
    }
}
