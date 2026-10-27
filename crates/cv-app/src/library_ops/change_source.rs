//! Change data source: move titles, files included, to another data source.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use cv_db::Database;
use cv_db::repo::data_sources::list_sources;
use cv_db::repo::get_item;
use cv_db::repo::library_ops::delete_fingerprints_under;
use cv_db::repo::library_ops::{files_of_items, lies_under, other_items_in_folder, tree_ids};
use cv_db::repo::rename_journal::PathObserver;
use cv_domain::{DataSource, Item, ItemId, SourceKind};
use cv_rename::exec::DbJournal;

use super::move_engine::{
    DbBinLedger, JobOutcome, MoveControl, MoveEngine, MoveFs, MoveJob, MoveProgress, StdMoveFs,
    TakeBackStatus, TakenBack, close_moved_back, group_rows, new_move_batch_id,
    only_trash_problems, titles_with_unfinished_moves,
};
use super::move_plan::{
    EntryKind, MoveEntry, enumerate, existing_destinations, free_space, margin_for,
};
use super::trash_bin::{BinnedItem, SystemBin};
use super::volume::same_volume;
use super::{LibraryOpsError, TitleKind, join_like, parts_below, same_folder, unique};
use crate::history::{HistoryKind, NewEntry, NewTitle, place_name, titles_word};
use crate::title_guard::{self, Work};
use cv_rename::exec::JournalStore;

/// What the preview says about one title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MoveStatus {
    /// The folder will be moved.
    Ready,
    /// The title is in the chosen data source already.
    SameSource,
    /// A movie needs a movie data source and a show a TV one.
    WrongKind,
    /// Other titles keep files in the same folder. Moving it would take them along.
    SharedFolder,
    /// The title's files sit directly in the root of its data source.
    InSourceRoot,
    /// The folder or a data source cannot be reached right now; the text says which.
    Unavailable(String),
    /// The destination folder, or a file in it, exists already. Nothing is ever replaced or
    /// merged; the row's `conflicts` list the paths.
    DestinationTaken,
    /// The copy needs more room than the destination volume has free.
    InsufficientSpace { needed: u64, free: u64 },
    /// The title cannot be moved; the text says why.
    NotMovable(String),
}

/// One title of the preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveRow {
    pub id: ItemId,
    pub kind: TitleKind,
    pub title: String,
    pub year: u16,
    /// The data source the title is in now.
    pub source: String,
    /// The folder now.
    pub from: String,
    /// The folder after the move, in the chosen data source.
    pub to: String,
    pub status: MoveStatus,
    /// Files in the title folder on disk (links and folders not counted).
    pub files: u32,
    /// Their size in bytes.
    pub bytes: u64,
    /// The folder will be copied and verified, not renamed.
    pub across_volumes: bool,
    /// Every file and folder of the title folder that will move, in order. Empty when the title
    /// cannot be moved before the disk was read.
    pub entries: Vec<MoveEntry>,
    /// What a copy asks of the destination volume: the files plus the safety margin (1% and at
    /// least 100 MB). 0 for a rename.
    pub needed_bytes: u64,
    /// Free space on the destination volume, when the system says.
    pub free_bytes: Option<u64>,
    /// Destination paths that exist already; any of them blocks the move.
    pub conflicts: Vec<String>,
}

/// What the preview of a change of data source says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovePlan {
    pub target: DataSource,
    pub rows: Vec<MoveRow>,
}

impl MovePlan {
    /// How many titles will be moved.
    #[must_use]
    pub fn ready(&self) -> usize {
        self.rows
            .iter()
            .filter(|row| row.status == MoveStatus::Ready)
            .count()
    }
}

/// How one title ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MoveResult {
    Moved,
    /// The disk refused; the folder is where it was.
    Failed(String),
    /// Not moved because the preview did not allow it.
    Skipped(MoveStatus),
    Canceled,
    /// An error (the destination is full, access was denied, a file is in use) stopped the move
    /// without removing anything. The text is the plain reason. The batch can be resumed or
    /// rolled back from Activity. This is also the result of a title that is whole in its new
    /// place while an original could not leave the old folder (the text says so): the move is
    /// not finished until Resume takes that original to the trash folder.
    Stopped(String),
}

/// One title of the answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveOutcome {
    pub id: ItemId,
    pub title: String,
    pub result: MoveResult,
    /// Things worth telling: files left behind in the old place, for example.
    pub notes: Vec<String>,
}

/// The answer of an apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveApplied {
    /// The journal batch, for Undo. `None` when nothing was moved.
    pub batch_id: Option<String>,
    pub items: Vec<MoveOutcome>,
}

impl MoveApplied {
    #[must_use]
    pub fn moved(&self) -> usize {
        self.items
            .iter()
            .filter(|item| item.result == MoveResult::Moved)
            .count()
    }
}

/// Moves titles between data sources.
#[derive(Clone)]
pub struct ChangeSourceService {
    db: Arc<Database>,
    observer: Option<Arc<dyn PathObserver>>,
    probes: Probes,
    changed: Option<ChangedHook>,
    disk: Arc<dyn MoveFs>,
    /// Where originals go once their copies are verified; `None` keeps them in the trash folder.
    bin: Option<Arc<dyn SystemBin>>,
}

/// Told which titles a move changed.
pub type ChangedHook = Arc<dyn Fn(&[ItemId]) + Send + Sync>;

/// What the plan asks the machine: the free space of a volume and whether two paths share one.
/// The defaults ask the system; a test, or a drive the system describes badly, can replace them.
#[derive(Clone)]
struct Probes {
    free_space: FreeSpaceProbe,
    same_volume: VolumeProbe,
}

type FreeSpaceProbe = Arc<dyn Fn(&Path) -> Option<u64> + Send + Sync>;
type VolumeProbe = Arc<dyn Fn(&Path, &Path) -> bool + Send + Sync>;

impl Default for Probes {
    fn default() -> Self {
        Self {
            free_space: Arc::new(free_space),
            same_volume: Arc::new(same_volume),
        }
    }
}

/// Everything the preview reads from the library for one title.
struct Loaded {
    item: Item,
    kind: TitleKind,
    files: Vec<cv_db::repo::library_ops::OwnedFile>,
    shared: bool,
    /// A move of the title did not finish (or is running now).
    unfinished_move: bool,
}

impl ChangeSourceService {
    #[must_use]
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            db,
            observer: None,
            probes: Probes::default(),
            changed: None,
            disk: Arc::new(StdMoveFs),
            bin: None,
        }
    }

    /// Sends the originals of a move between drives to the Recycle Bin / Trash (and the copies
    /// of an undo too) instead of the trash folder. The app passes the real bin;
    /// without one everything goes to the trash folder.
    #[must_use]
    pub fn with_bin(mut self, bin: Arc<dyn SystemBin>) -> Self {
        self.bin = Some(bin);
        self
    }

    /// The move engine over `journal`, with the bin when there is one.
    fn engine<'a>(&self, fs: &'a dyn MoveFs, journal: &'a mut DbJournal) -> MoveEngine<'a> {
        let engine = MoveEngine::new(fs, journal);
        match &self.bin {
            Some(bin) => {
                engine.with_bin(Arc::clone(bin), Arc::new(DbBinLedger(Arc::clone(&self.db))))
            }
            None => engine,
        }
    }

    /// Replaces the disk the move works on. For tests that fail on purpose.
    #[must_use]
    pub fn with_disk(mut self, disk: Arc<dyn MoveFs>) -> Self {
        self.disk = disk;
        self
    }

    /// Tells `hook` which titles every finished move changed, so the pages that show them and
    /// the cache of tidy names follow.
    #[must_use]
    pub fn with_changed_hook(mut self, hook: ChangedHook) -> Self {
        self.changed = Some(hook);
        self
    }

    /// Replaces the free-space question of the preview.
    #[must_use]
    pub fn with_free_space_probe(
        mut self,
        probe: impl Fn(&Path) -> Option<u64> + Send + Sync + 'static,
    ) -> Self {
        self.probes.free_space = Arc::new(probe);
        self
    }

    /// Replaces the "same volume" question of the preview: a rename when it says yes, a copy
    /// verified file by file when it says no.
    #[must_use]
    pub fn with_volume_probe(
        mut self,
        probe: impl Fn(&Path, &Path) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.probes.same_volume = Arc::new(probe);
        self
    }

    /// Also tells `observer` about every move and every undo, so the poster index behind
    /// `cv://img` follows the files without a rescan.
    #[must_use]
    pub fn with_path_observer(mut self, observer: Arc<dyn PathObserver>) -> Self {
        self.observer = Some(observer);
        self
    }

    /// The library this service works on.
    #[must_use]
    pub fn database(&self) -> &Arc<Database> {
        &self.db
    }

    /// Looks at the titles and the disk and says what moving them to `target` would do. Nothing
    /// is changed. A season or an episode stands for its show.
    ///
    /// # Errors
    /// The data source is unknown or the library cannot be read.
    pub fn plan(&self, ids: &[ItemId], target: i64) -> Result<MovePlan, LibraryOpsError> {
        let target = self.target(target)?;
        let ids = unique(ids);
        let unfinished = titles_with_unfinished_moves(&DbJournal::new(Arc::clone(&self.db)))
            .map_err(|e| LibraryOpsError::Disk(e.to_string()))?;
        let loaded = self.db.read(move |conn| {
            let mut out: Vec<Result<Loaded, (ItemId, String)>> = Vec::new();
            let mut seen = Vec::new();
            for id in ids {
                let Some(mut item) = get_item(conn, id)? else {
                    out.push(Err((id, String::new())));
                    continue;
                };
                // A season or an episode is moved with its show.
                if matches!(
                    TitleKind::of(&item),
                    Some(TitleKind::Season | TitleKind::Episode)
                ) && let Some(parent) = item.parent_id
                    && let Some(show) = get_item(conn, parent)?
                {
                    item = show;
                }
                let (Some(kind), Some(id)) = (TitleKind::of(&item), item.id) else {
                    out.push(Err((id, item.core.title.clone())));
                    continue;
                };
                if seen.contains(&id) {
                    continue;
                }
                seen.push(id);
                if !kind.owns_folder() {
                    out.push(Err((id, item.core.title.clone())));
                    continue;
                }
                let tree = tree_ids(conn, id)?;
                let files = files_of_items(conn, &tree)?;
                let shared = !item.core.path.is_empty()
                    && !other_items_in_folder(conn, id, &item.core.path)?.is_empty();
                out.push(Ok(Loaded {
                    item,
                    kind,
                    files,
                    shared,
                    unfinished_move: unfinished.contains(&id.0.to_string()),
                }));
            }
            Ok(out)
        })?;

        let target_available = Path::new(&target.path).is_dir();
        let rows = loaded
            .into_iter()
            .map(|entry| match entry {
                Ok(loaded) => judge(&loaded, &target, target_available, &self.probes),
                Err((id, title)) => MoveRow {
                    id,
                    kind: TitleKind::Movie,
                    title,
                    year: 0,
                    source: String::new(),
                    from: String::new(),
                    to: String::new(),
                    status: MoveStatus::NotMovable(
                        "This title is no longer in the library or has no folder.".to_owned(),
                    ),
                    files: 0,
                    bytes: 0,
                    across_volumes: false,
                    entries: Vec::new(),
                    needed_bytes: 0,
                    free_bytes: None,
                    conflicts: Vec::new(),
                },
            })
            .collect();
        let mut rows: Vec<MoveRow> = rows;
        check_space(&mut rows, &target.path, &self.probes);
        Ok(MovePlan { target, rows })
    }

    /// Plans again, so the disk is judged as it is now, and moves every title that is ready as
    /// one journal batch. `progress` is called while files are copied and after each title.
    ///
    /// # Errors
    /// The data source is unknown, or the journal or the library cannot be written (which stops
    /// the batch). A folder that cannot be moved fails alone and stays where it was.
    pub fn apply(
        &self,
        ids: &[ItemId],
        target: i64,
        progress: &mut dyn FnMut(&MoveProgress),
    ) -> Result<MoveApplied, LibraryOpsError> {
        self.apply_with(ids, target, None, progress)
    }

    /// [`Self::apply`] with a handle that pauses and cancels the move.
    ///
    /// # Errors
    /// See [`Self::apply`].
    pub fn apply_with(
        &self,
        ids: &[ItemId],
        target: i64,
        control: Option<Arc<MoveControl>>,
        progress: &mut dyn FnMut(&MoveProgress),
    ) -> Result<MoveApplied, LibraryOpsError> {
        // No rescan walks a folder while it moves to the other data source: it would find the
        // show under neither place the library knows and add it again.
        let sources = self.db.read(|conn| list_sources(conn, None))?;
        let _held = cv_scan::lock::hold_sources(sources.iter().map(|s| s.id));
        // Nothing else writes into a title while it moves (see `title_guard`). The titles are
        // held together, waiting (holding none) for short writes to end; a title that stays
        // busy, or whose NFO or artwork is still being written after a scrape, is left where it
        // is, with the reason.
        let mut titles = Vec::new();
        for id in unique(ids) {
            titles.push(title_guard::title_of(&self.db, id)?);
        }
        let (guards, refused) =
            title_guard::hold_each(&self.db, titles, Work::Move, title_guard::MOVE_WAIT);
        let busy: Vec<(ItemId, String)> = refused
            .into_iter()
            .map(|(title, why)| (title, why.to_string()))
            .collect();
        let mut plan = self.plan(ids, target)?;
        for row in &mut plan.rows {
            if let Some((_, why)) = busy.iter().find(|(id, _)| *id == row.id) {
                row.status = MoveStatus::NotMovable(why.clone());
            }
        }
        let applied = self.run_plan(&plan, control, progress, true);
        drop(guards);
        let applied = applied?;
        let moved: Vec<ItemId> = applied
            .items
            .iter()
            .filter(|i| i.result == MoveResult::Moved)
            .map(|i| i.id)
            .collect();
        if let Some(hook) = &self.changed
            && !moved.is_empty()
        {
            hook(&moved);
        }
        Ok(applied)
    }

    /// Takes a title of a finished move back by moving it again, from its new place to the
    /// old data source, as a new verified move: the undo of a move whose originals are no
    /// longer in the trash folder (emptied by hand, say). The caller holds the data sources
    /// and the title. The title must still be where the move put it (`moved_to`), and nothing
    /// may be in the old place; a new batch is journaled and can be undone in its turn.
    ///
    /// # Errors
    /// See [`Self::apply`].
    pub(crate) fn move_back(
        &self,
        id: ItemId,
        old_folder: &Path,
        moved_to: &Path,
    ) -> Result<Option<MoveApplied>, LibraryOpsError> {
        let item = self.db.read(move |conn| get_item(conn, id))?;
        let Some(item) = item else {
            return Ok(None);
        };
        if !same_folder(&item.core.path, &text_of(moved_to)) {
            return Ok(None);
        }
        let sources = self.db.read(|conn| list_sources(conn, None))?;
        let Some(old_source) = sources
            .iter()
            .filter(|s| lies_under(&text_of(old_folder), &s.path))
            .max_by_key(|s| s.path.len())
        else {
            return Ok(None);
        };
        let plan = self.plan(&[id], old_source.id)?;
        if plan
            .rows
            .first()
            .is_none_or(|row| !same_folder(&row.to, &text_of(old_folder)))
        {
            return Ok(None);
        }
        // The copy back is part of an undo, not a move of its own: no History entry.
        let applied = self.run_plan(&plan, None, &mut |_| {}, false)?;
        if let Some(hook) = &self.changed
            && applied.moved() > 0
        {
            hook(&[id]);
        }
        Ok(Some(applied))
    }

    /// Undoes a finished move batch. A title whose originals are no longer in the trash folder
    /// (emptied by hand, say) but is whole in its new place is moved back from there as a new
    /// verified move (see [`Self::move_back`]); every other title goes back as
    /// [`MoveEngine::undo`] says. The caller holds the data sources and the titles.
    ///
    /// # Errors
    /// The journal or the library cannot be read or written.
    pub(crate) fn undo_held(
        &self,
        batch_id: &str,
        journal: &mut DbJournal,
        only: Option<&[String]>,
    ) -> Result<Vec<TakenBack>, LibraryOpsError> {
        let sources = self.db.read(|conn| list_sources(conn, None))?;
        let roots = sources.iter().map(|s| PathBuf::from(&s.path)).collect();
        let fs = Arc::clone(&self.disk);
        let taken = self
            .engine(fs.as_ref(), journal)
            .with_roots(roots)
            .undo_only(batch_id, only)
            .map_err(|e| LibraryOpsError::Disk(e.to_string()))?;
        let grouped = group_rows(
            journal
                .batch(batch_id)
                .map_err(|e| LibraryOpsError::Disk(e.to_string()))?,
        );
        let mut out = Vec::with_capacity(taken.len());
        for mut item in taken {
            let TakeBackStatus::NotRestorable(problems) = &item.status else {
                out.push(item);
                continue;
            };
            let folders = item.item.as_deref().and_then(|id| {
                let rows = grouped.iter().find(|r| r.item.as_deref() == Some(id))?;
                Some((
                    id.parse::<i64>().ok()?,
                    PathBuf::from(rows.old_folder()?),
                    PathBuf::from(rows.new_folder()?),
                ))
            });
            let Some((id, old, new)) = folders.filter(|_| only_trash_problems(problems)) else {
                out.push(item);
                continue;
            };
            match self.move_back(ItemId(id), &old, &new)? {
                Some(applied) if applied.moved() == 1 => {
                    close_moved_back(journal, batch_id, &id.to_string())
                        .map_err(|e| LibraryOpsError::Disk(e.to_string()))?;
                    item.status = TakeBackStatus::Restored;
                    item.restored = 1;
                    item.notes.push(
                        "The originals were no longer in the trash folder, so the title was \
                         copied back from its new place and checked."
                            .to_owned(),
                    );
                    item.notes
                        .extend(applied.items.into_iter().flat_map(|i| i.notes));
                }
                Some(applied) => {
                    let why = applied
                        .items
                        .first()
                        .map(|i| match &i.result {
                            MoveResult::Failed(why) | MoveResult::Stopped(why) => why.clone(),
                            MoveResult::Skipped(status) => format!("{status:?}"),
                            MoveResult::Canceled => "cancelled".to_owned(),
                            MoveResult::Moved => String::new(),
                        })
                        .unwrap_or_default();
                    item.notes.push(format!(
                        "The originals were no longer in the trash folder, and moving the title \
                         back from its new place did not work: {why}"
                    ));
                    out.push(item);
                    continue;
                }
                None => {}
            }
            out.push(item);
        }
        Ok(out)
    }

    /// Moves the titles of `plan` that are ready, as one journal batch. With `record`, a batch
    /// that moved a title writes its History entry.
    fn run_plan(
        &self,
        plan: &MovePlan,
        control: Option<Arc<MoveControl>>,
        progress: &mut dyn FnMut(&MoveProgress),
        record: bool,
    ) -> Result<MoveApplied, LibraryOpsError> {
        let sources = self.db.read(|conn| list_sources(conn, None))?;
        let mut jobs = Vec::new();
        let mut items: Vec<MoveOutcome> = Vec::new();
        for row in &plan.rows {
            if row.status == MoveStatus::Ready {
                jobs.push(MoveJob {
                    item: row.id,
                    title: row.title.clone(),
                    from_root: PathBuf::from(&row.source),
                    from: PathBuf::from(&row.from),
                    to_root: PathBuf::from(&plan.target.path),
                    to: PathBuf::from(&row.to),
                    entries: row.entries.clone(),
                    across_volumes: row.across_volumes,
                });
            } else {
                items.push(MoveOutcome {
                    id: row.id,
                    title: row.title.clone(),
                    result: MoveResult::Skipped(row.status.clone()),
                    notes: Vec::new(),
                });
            }
        }
        if jobs.is_empty() {
            return Ok(MoveApplied {
                batch_id: None,
                items,
            });
        }

        // A copy back (no History entry) is told apart in the journal by its id, so a Resume or
        // the start-up check never gives it an entry of its own.
        let batch_id = if record {
            new_move_batch_id()
        } else {
            super::move_engine::new_move_back_batch_id()
        };
        let mut journal = DbJournal::new(Arc::clone(&self.db));
        if let Some(observer) = &self.observer {
            journal = journal.with_observer(Arc::clone(observer));
        }
        let fs = Arc::clone(&self.disk);
        let mut engine = self
            .engine(fs.as_ref(), &mut journal)
            .with_roots(sources.iter().map(|s| PathBuf::from(&s.path)).collect());
        if let Some(control) = control {
            engine = engine.with_control(control);
        }
        let results = engine
            .run(&batch_id, &jobs, progress)
            .map_err(|e| LibraryOpsError::Disk(e.to_string()))?;

        for (job, result) in jobs.iter().zip(results) {
            let (result, notes) = match result.outcome {
                JobOutcome::Moved { notes } => {
                    self.forget_fingerprints(&text_of(&job.from))?;
                    (MoveResult::Moved, notes)
                }
                JobOutcome::Failed(why) => (MoveResult::Failed(why), Vec::new()),
                JobOutcome::Stopped(why) => (MoveResult::Stopped(why), Vec::new()),
                JobOutcome::Canceled => (MoveResult::Canceled, Vec::new()),
                JobOutcome::NotStarted => (
                    MoveResult::Stopped(
                        "Not started: an earlier title stopped the move.".to_owned(),
                    ),
                    Vec::new(),
                ),
            };
            items.push(MoveOutcome {
                id: job.item,
                title: job.title.clone(),
                result,
                notes,
            });
        }
        if record {
            record_move(&self.db, &batch_id, plan, &items);
        }
        Ok(MoveApplied {
            batch_id: Some(batch_id),
            items,
        })
    }

    fn target(&self, id: i64) -> Result<DataSource, LibraryOpsError> {
        self.db
            .read(|conn| list_sources(conn, None))?
            .into_iter()
            .find(|source| source.id == id)
            .ok_or(LibraryOpsError::UnknownSource(id))
    }

    /// The scanner's memory of the old files would make the next scan of the old source look for
    /// them; drop it. The files are known under their new paths by the next scan of the target.
    fn forget_fingerprints(&self, folder: &str) -> Result<(), LibraryOpsError> {
        let folder = folder.to_owned();
        self.db
            .write(move |tx| delete_fingerprints_under(tx, &folder))?;
        Ok(())
    }
}

/// A title a move batch put in its new place, as its History entry names it.
struct MovedTitle {
    id: ItemId,
    /// With the year: "Heat (1995)".
    title: String,
    /// The data source it came from.
    source: String,
    from: String,
    to: String,
    files: u32,
    bytes: u64,
}

/// Writes the History entry of a move batch that moved at least one title: "Moved 3 titles
/// from D: to E:", the files and bytes, and each title's old and new folder. An entry that
/// cannot be written does not undo the move; the journal still holds the batch.
fn record_move(db: &Database, batch_id: &str, plan: &MovePlan, items: &[MoveOutcome]) {
    let moved: Vec<MovedTitle> = items
        .iter()
        .filter(|i| i.result == MoveResult::Moved)
        .filter_map(|i| plan.rows.iter().find(|r| r.id == i.id))
        .map(|r| MovedTitle {
            id: r.id,
            title: with_year(&r.title, r.year),
            source: r.source.clone(),
            from: r.from.clone(),
            to: r.to.clone(),
            files: r.files,
            bytes: r.bytes,
        })
        .collect();
    let Some(entry) = move_entry(db, batch_id, &plan.target.path, &moved) else {
        return;
    };
    if let Err(error) = crate::history::record(db, &entry) {
        tracing::warn!(batch = batch_id, %error, "the move batch has no History entry");
    }
}

/// Writes (or completes) the History entry of a move batch from its journal: every title the
/// batch put in its new place with nothing left to do. For a batch that Resume finished, or
/// whose entry a crash kept from being written, so History can undo it. A copy back (part of an
/// undo) gets none.
///
/// # Errors
/// The journal or the library cannot be read or written.
pub(crate) fn record_move_from_journal(
    db: &Arc<Database>,
    batch_id: &str,
) -> Result<Option<i64>, LibraryOpsError> {
    if !super::move_engine::wants_history(batch_id) {
        return Ok(None);
    }
    let rows = DbJournal::new(Arc::clone(db))
        .batch(batch_id)
        .map_err(|e| LibraryOpsError::Disk(e.to_string()))?;
    let sources: Vec<String> = db
        .read(|conn| list_sources(conn, None))?
        .into_iter()
        .map(|s| s.path)
        .collect();
    let source_of = |path: &str| -> String {
        sources
            .iter()
            .filter(|root| lies_under(path, root))
            .max_by_key(|root| root.len())
            .cloned()
            .unwrap_or_else(|| {
                Path::new(path)
                    .parent()
                    .map_or_else(|| path.to_owned(), text_of)
            })
    };
    let mut moved = Vec::new();
    for item in group_rows(rows) {
        if !item.committed() || item.unfinished() {
            continue;
        }
        let (Some(id), Some(from), Some(to)) = (
            item.item.as_deref().and_then(|id| id.parse::<i64>().ok()),
            item.old_folder(),
            item.new_folder(),
        ) else {
            continue;
        };
        let id = ItemId(id);
        let title = db
            .read(move |conn| cv_db::repo::get_summary(conn, id))?
            .map_or_else(String::new, |s| with_year(&s.title, s.year));
        let (files, bytes) = files_below(Path::new(to));
        moved.push(MovedTitle {
            id,
            title,
            source: source_of(from),
            from: from.to_owned(),
            to: to.to_owned(),
            files,
            bytes,
        });
    }
    let Some(target) = moved.first().map(|m| source_of(&m.to)) else {
        return Ok(None);
    };
    let Some(entry) = move_entry(db, batch_id, &target, &moved) else {
        return Ok(None);
    };
    Ok(Some(crate::history::record_batch(db, &entry)?))
}

/// The files below a folder and their bytes (links are not followed).
fn files_below(path: &Path) -> (u32, u64) {
    let Ok(meta) = path.symlink_metadata() else {
        return (0, 0);
    };
    if meta.is_file() {
        return (1, meta.len());
    }
    if !meta.is_dir() {
        return (0, 0);
    }
    std::fs::read_dir(path)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| files_below(&entry.path()))
                .fold((0, 0), |(f, b), (x, y)| (f + x, b + y))
        })
        .unwrap_or((0, 0))
}

/// The History entry of the titles a move batch moved; `None` when it moved none.
fn move_entry(
    db: &Database,
    batch_id: &str,
    target: &str,
    moved: &[MovedTitle],
) -> Option<NewEntry> {
    if moved.is_empty() {
        return None;
    }
    let to = place_name(target);
    let from: BTreeSet<String> = moved.iter().map(|r| place_name(&r.source)).collect();
    let what = match moved {
        [one] => one.title.clone(),
        many => titles_word(many.len()),
    };
    let summary = match from.iter().next() {
        Some(only) if from.len() == 1 && *only != to => format!("Moved {what} from {only} to {to}"),
        _ => format!("Moved {what} to {to}"),
    };
    let files: u32 = moved.iter().map(|r| r.files).sum();
    let bytes: u64 = moved.iter().map(|r| r.bytes).sum();
    Some(NewEntry {
        kind: HistoryKind::Move,
        summary,
        line: format!(
            "{files} {} · {target}",
            if files == 1 { "file" } else { "files" },
        ),
        bytes: Some(bytes),
        extra: serde_json::json!({
            "files": files,
            "target": target,
            "binned": binned_of_batch(db, batch_id),
        }),
        batch_ref: Some(batch_id.to_owned()),
        titles: moved
            .iter()
            .map(|r| NewTitle {
                item_id: r.id,
                title: r.title.clone(),
                before: r.from.clone(),
                after: r.to.clone(),
            })
            .collect(),
    })
}

/// The originals a move batch sent to the Recycle Bin / Trash, per title, as History keeps them
/// for its bin total.
fn binned_of_batch(db: &Database, batch_id: &str) -> Vec<serde_json::Value> {
    let batch = batch_id.to_owned();
    let rows = db
        .read(move |conn| cv_db::repo::rename_journal::binned_rows(conn, &batch))
        .unwrap_or_default();
    let mut by_item: std::collections::BTreeMap<i64, Vec<BinnedItem>> =
        std::collections::BTreeMap::new();
    for row in rows {
        by_item
            .entry(row.item_id.unwrap_or(0))
            .or_default()
            .push(BinnedItem {
                original: row.from_path,
                bin_id: row.bin_id,
                size: row
                    .size_bytes
                    .and_then(|s| u64::try_from(s).ok())
                    .unwrap_or(0),
            });
    }
    by_item
        .into_iter()
        .map(|(item_id, items)| serde_json::json!({ "item_id": item_id, "items": items }))
        .collect()
}

fn with_year(title: &str, year: u16) -> String {
    if year > 0 {
        format!("{title} ({year})")
    } else {
        title.to_owned()
    }
}

/// Decides what happens to one title.
fn judge(loaded: &Loaded, target: &DataSource, target_available: bool, probes: &Probes) -> MoveRow {
    let core = &loaded.item.core;
    let mut row = MoveRow {
        id: loaded.item.id.unwrap_or(ItemId(0)),
        kind: loaded.kind,
        title: core.title.clone(),
        year: core.year,
        source: core.data_source.clone(),
        from: core.path.clone(),
        to: String::new(),
        status: MoveStatus::Ready,
        files: 0,
        bytes: 0,
        across_volumes: false,
        entries: Vec::new(),
        needed_bytes: 0,
        free_bytes: None,
        conflicts: Vec::new(),
    };

    let wants = match loaded.kind {
        TitleKind::Movie => SourceKind::Movie,
        _ => SourceKind::Tv,
    };
    if target.kind != wants {
        row.status = MoveStatus::WrongKind;
        return row;
    }
    if same_folder(&core.data_source, &target.path) {
        row.status = MoveStatus::SameSource;
        return row;
    }
    if core.path.is_empty() || core.data_source.is_empty() {
        row.status = MoveStatus::NotMovable("The title has no folder in a data source.".to_owned());
        return row;
    }
    let Some(parts) = parts_below(&core.data_source, &core.path) else {
        row.status = MoveStatus::NotMovable(
            "The title's folder is outside its data source, so it cannot be moved safely."
                .to_owned(),
        );
        return row;
    };
    if parts.is_empty() {
        row.status = MoveStatus::InSourceRoot;
        return row;
    }
    row.to = join_like(&target.path, &parts);
    if loaded.shared {
        row.status = MoveStatus::SharedFolder;
        return row;
    }
    if loaded
        .files
        .iter()
        .any(|file| !lies_under(&file.directory, &core.path))
    {
        row.status = MoveStatus::NotMovable(
            "Some of the title's files are outside its folder, so it cannot be moved as one."
                .to_owned(),
        );
        return row;
    }
    if !target_available {
        row.status = MoveStatus::Unavailable(format!(
            "The data source {} cannot be reached.",
            target.path
        ));
        return row;
    }
    if !Path::new(&core.data_source).is_dir() {
        row.status = MoveStatus::Unavailable(format!(
            "The data source {} cannot be reached.",
            core.data_source
        ));
        return row;
    }
    if loaded.unfinished_move {
        row.status = MoveStatus::NotMovable(
            "A move of this title did not finish. Resume it or roll it back in Activity first."
                .to_owned(),
        );
        return row;
    }
    if !Path::new(&core.path).is_dir() {
        row.status = MoveStatus::Unavailable(format!("The folder {} was not found.", core.path));
        return row;
    }
    if std::fs::symlink_metadata(&core.path).is_ok_and(|m| m.file_type().is_symlink()) {
        // Moving the link would leave its files behind; following it would move files that are
        // somewhere else.
        row.status = MoveStatus::NotMovable(
            "The title folder is a link to another place, so move this title by hand.".to_owned(),
        );
        return row;
    }
    match enumerate(Path::new(&core.path)) {
        Ok(entries) => {
            row.files =
                u32::try_from(entries.iter().filter(|e| e.is_file()).count()).unwrap_or(u32::MAX);
            row.bytes = entries.iter().map(|e| e.size).sum();
            row.entries = entries;
        }
        Err(why) => {
            row.status = MoveStatus::NotMovable(why);
            return row;
        }
    }
    row.conflicts = existing_destinations(Path::new(&row.to), &row.entries);
    if !row.conflicts.is_empty() {
        row.status = MoveStatus::DestinationTaken;
        return row;
    }
    row.across_volumes = !(probes.same_volume)(Path::new(&core.path), Path::new(&row.to));
    if row.across_volumes
        && let Some(link) = row
            .entries
            .iter()
            .find(|e| matches!(e.kind, EntryKind::Link { .. }))
    {
        // A link cannot be copied and checked, and following it would copy files that are not
        // part of the title. It is listed; the move to another volume does not take it.
        row.status = MoveStatus::NotMovable(format!(
            "The folder contains a link ({}). Links are not followed or copied to another \
             drive, so move this title by hand.",
            link.path
        ));
    }
    row
}

/// Marks the rows that need more room on the destination volume than it has. The rows share the
/// space in order, so two titles that each fit but not together do not both pass.
fn check_space(rows: &mut [MoveRow], target_root: &str, probes: &Probes) {
    let free = (probes.free_space)(Path::new(target_root));
    let mut copied: u64 = 0;
    for row in rows
        .iter_mut()
        .filter(|row| row.status == MoveStatus::Ready && row.across_volumes)
    {
        let total = copied.saturating_add(row.bytes);
        row.needed_bytes = row.bytes.saturating_add(margin_for(total));
        row.free_bytes = free;
        if let Some(free) = free
            && total.saturating_add(margin_for(total)) > free
        {
            row.status = MoveStatus::InsufficientSpace {
                needed: row.needed_bytes,
                free,
            };
            continue;
        }
        copied = total;
    }
}

fn text_of(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
