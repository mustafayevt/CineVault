//! Typed commands for change data source, remove from library and delete with files.
//!
//! The rules live in `cv_app::library_ops`. Every command that changes the disk is two steps:
//! a preview (`plan_*`) that reads the library and the disk and says what would happen, and the
//! action itself, which looks at the disk again. Moving a title streams a progress event per
//! title over a channel and returns the journal batch, which `organize_undo` takes back. Work
//! runs on a blocking thread.

use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use cv_app::library_ops::{
    BatchKind, ChangeSourceService, DeleteMethod, DeleteReport, EntryKind, FolderUse,
    IgnoreOutcome, MoveApplied, MoveControl, MoveEntry, MovePhase, MovePlan, MoveProgress,
    MoveResult, MoveRow, MoveStatus, Problem, RecoveryService, RemovalPlan, RemovalRow,
    RemovalService, RemoveReport, ReviewList, ReviewRow, ReviewService, TitleKind, UnfinishedBatch,
};
use cv_artwork::library_source::LibrarySources;
use cv_db::repo::rename_journal::PathObserver;
use cv_domain::ItemId;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tauri::ipc::Channel;

use crate::sources_ipc::LibraryState;

/// Held in Tauri state. The services are built by the first command that needs them.
pub struct LibraryOpsState {
    change: OnceLock<Result<Arc<ChangeSourceService>, String>>,
    removal: OnceLock<Result<Arc<RemovalService>, String>>,
    review: OnceLock<Result<Arc<ReviewService>, String>>,
    recovery: OnceLock<Result<Arc<RecoveryService>, String>>,
    /// The poster index behind `cv://img`; told about every move so thumbnails follow files.
    sources: Arc<LibrarySources>,
    /// Pause and cancel of the move that is running now.
    control: Mutex<Option<Arc<MoveControl>>>,
}

impl LibraryOpsState {
    pub fn new(sources: Arc<LibrarySources>) -> Self {
        Self {
            change: OnceLock::new(),
            removal: OnceLock::new(),
            review: OnceLock::new(),
            recovery: OnceLock::new(),
            sources,
            control: Mutex::new(None),
        }
    }

    fn change(
        &self,
        library: &LibraryState,
    ) -> Result<Arc<ChangeSourceService>, LibraryOpsCommandError> {
        self.change
            .get_or_init(|| {
                let db = library.database()?;
                let observer: Arc<dyn PathObserver> = self.sources.clone();
                // Originals of a move between drives go to the Recycle Bin / Trash.
                let mut service = ChangeSourceService::new(db)
                    .with_path_observer(observer)
                    .with_bin(library.bin());
                // Files and stage changed for every moved title, and the name check starts again.
                if let Some(state) = library.organize_state() {
                    service = service.with_changed_hook(state.hook());
                }
                Ok(Arc::new(service))
            })
            .as_ref()
            .map(Arc::clone)
            .map_err(|message| storage(message))
    }

    fn recovery(
        &self,
        library: &LibraryState,
    ) -> Result<Arc<RecoveryService>, LibraryOpsCommandError> {
        self.recovery
            .get_or_init(|| {
                let db = library.database()?;
                let observer: Arc<dyn PathObserver> = self.sources.clone();
                let mut service = RecoveryService::new(db).with_path_observer(observer);
                if let Some(state) = library.organize_state() {
                    service = service.with_changed_hook(state.hook());
                }
                Ok(Arc::new(service))
            })
            .as_ref()
            .map(Arc::clone)
            .map_err(|message| storage(message))
    }

    fn review(&self, library: &LibraryState) -> Result<Arc<ReviewService>, LibraryOpsCommandError> {
        self.review
            .get_or_init(|| Ok(Arc::new(ReviewService::new(library.database()?))))
            .as_ref()
            .map(Arc::clone)
            .map_err(|message| storage(message))
    }

    fn removal(
        &self,
        library: &LibraryState,
    ) -> Result<Arc<RemovalService>, LibraryOpsCommandError> {
        self.removal
            .get_or_init(|| {
                // Deleted titles are hidden, not moved: the poster index needs no word of it.
                Ok(Arc::new(
                    RemovalService::new(library.database()?)
                        .with_bin(library.bin())
                        .with_choice(library.deleted_files()),
                ))
            })
            .as_ref()
            .map(Arc::clone)
            .map_err(|message| storage(message))
    }
}

fn storage(message: &str) -> LibraryOpsCommandError {
    LibraryOpsCommandError {
        code: "storage".to_owned(),
        message: message.to_owned(),
    }
}

/// Why a command did not run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct LibraryOpsCommandError {
    /// `storage`, `bad_id`, `unknown_source`, `disk`, or the code of the refused rule: `title`,
    /// `year`, `data_source`, `unavailable`, `exists`.
    pub code: String,
    /// Plain sentence for the user.
    pub message: String,
}

impl From<cv_app::library_ops::LibraryOpsError> for LibraryOpsCommandError {
    fn from(error: cv_app::library_ops::LibraryOpsError) -> Self {
        Self {
            code: error.code().to_owned(),
            message: error.to_string(),
        }
    }
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, LibraryOpsCommandError> + Send + 'static,
) -> Result<T, LibraryOpsCommandError> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| LibraryOpsCommandError {
            code: "storage".to_owned(),
            message: format!("The operation stopped unexpectedly: {error}"),
        })?
}

fn ids_of(ids: &[String]) -> Result<Vec<ItemId>, LibraryOpsCommandError> {
    ids.iter()
        .map(|id| {
            id.trim()
                .parse::<i64>()
                .map(ItemId)
                .map_err(|_| LibraryOpsCommandError {
                    code: "bad_id".to_owned(),
                    message: format!("\"{id}\" is not a title."),
                })
        })
        .collect()
}

#[allow(clippy::cast_precision_loss)]
fn bytes_of(bytes: u64) -> f64 {
    bytes as f64
}

fn kind_text(kind: TitleKind) -> &'static str {
    match kind {
        TitleKind::Movie => "movie",
        TitleKind::Show => "show",
        TitleKind::Season => "season",
        TitleKind::Episode => "episode",
    }
}

// ---- change data source ----------------------------------------------------------------------

/// One title of the change-data-source preview.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct MoveRowDto {
    pub id: String,
    /// `movie` or `show`.
    pub kind: String,
    pub title: String,
    pub year: u32,
    /// The data source the title is in now.
    pub source: String,
    pub from: String,
    pub to: String,
    /// `ready`, `same_source`, `wrong_kind`, `shared_folder`, `in_source_root`, `unavailable`,
    /// `destination_taken`, `insufficient_space` or `not_movable`.
    pub status: String,
    /// The reason for `unavailable` and `not_movable`; empty otherwise.
    pub reason: String,
    pub files: u32,
    pub bytes: f64,
    /// The folder is copied and verified instead of renamed.
    pub across_volumes: bool,
    /// What a copy asks of the destination volume: the files plus the safety margin. 0 for a
    /// rename.
    pub needed_bytes: f64,
    /// Free space on the destination volume, when the system says.
    pub free_bytes: Option<f64>,
    /// Destination paths that exist already; any of them blocks the move.
    pub conflicts: Vec<String>,
}

/// One file or folder of a title folder in the move preview.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct MoveEntryDto {
    /// The path below the title folder.
    pub path: String,
    /// `file`, `folder` or `link`.
    pub kind: String,
    pub size: f64,
    /// Where a link points; empty otherwise.
    pub link_target: String,
}

impl From<MoveEntry> for MoveEntryDto {
    fn from(entry: MoveEntry) -> Self {
        let (kind, link_target) = match entry.kind {
            EntryKind::File => ("file", String::new()),
            EntryKind::Dir => ("folder", String::new()),
            EntryKind::Link { target } => ("link", target),
        };
        Self {
            path: entry.path,
            kind: kind.to_owned(),
            size: bytes_of(entry.size),
            link_target,
        }
    }
}

fn status_parts(status: &MoveStatus) -> (&'static str, String) {
    match status {
        MoveStatus::Ready => ("ready", String::new()),
        MoveStatus::SameSource => ("same_source", String::new()),
        MoveStatus::WrongKind => ("wrong_kind", String::new()),
        MoveStatus::SharedFolder => ("shared_folder", String::new()),
        MoveStatus::InSourceRoot => ("in_source_root", String::new()),
        MoveStatus::Unavailable(why) => ("unavailable", why.clone()),
        MoveStatus::DestinationTaken => ("destination_taken", String::new()),
        MoveStatus::InsufficientSpace { .. } => ("insufficient_space", String::new()),
        MoveStatus::NotMovable(why) => ("not_movable", why.clone()),
    }
}

impl From<MoveRow> for MoveRowDto {
    fn from(row: MoveRow) -> Self {
        let (status, reason) = status_parts(&row.status);
        Self {
            id: row.id.0.to_string(),
            kind: kind_text(row.kind).to_owned(),
            title: row.title,
            year: u32::from(row.year),
            source: row.source,
            from: row.from,
            to: row.to,
            status: status.to_owned(),
            reason,
            files: row.files,
            bytes: bytes_of(row.bytes),
            across_volumes: row.across_volumes,
            needed_bytes: bytes_of(row.needed_bytes),
            free_bytes: row.free_bytes.map(bytes_of),
            conflicts: row.conflicts,
        }
    }
}

/// The preview of a change of data source.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct MovePlanDto {
    pub target_id: u32,
    pub target_path: String,
    pub rows: Vec<MoveRowDto>,
    /// How many titles will be moved.
    pub ready: u32,
}

impl From<MovePlan> for MovePlanDto {
    fn from(plan: MovePlan) -> Self {
        let ready = u32::try_from(plan.ready()).unwrap_or(u32::MAX);
        Self {
            target_id: u32::try_from(plan.target.id).unwrap_or(0),
            target_path: plan.target.path,
            rows: plan.rows.into_iter().map(MoveRowDto::from).collect(),
            ready,
        }
    }
}

/// Says what moving the titles to the data source would do. Changes nothing.
#[tauri::command]
#[specta::specta]
pub async fn plan_change_source(
    library: State<'_, LibraryState>,
    ops: State<'_, LibraryOpsState>,
    ids: Vec<String>,
    target: u32,
) -> Result<MovePlanDto, LibraryOpsCommandError> {
    let service = ops.change(&library)?;
    let ids = ids_of(&ids)?;
    blocking(move || Ok(service.plan(&ids, i64::from(target))?.into())).await
}

/// Everything that moves with one title: every file and subfolder with its size, and the row's
/// verdict. The list can be long (a show), so the plan of many titles leaves it out.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct MoveTitlePreviewDto {
    pub row: MoveRowDto,
    pub entries: Vec<MoveEntryDto>,
}

/// Says exactly what moving one title to the data source would copy or rename: every file and
/// folder with its destination below `row.to`, the total size, whether it is a rename or a
/// copy, the free-space check and the destination conflicts. Changes nothing.
#[tauri::command]
#[specta::specta]
pub async fn preview_change_source_title(
    library: State<'_, LibraryState>,
    ops: State<'_, LibraryOpsState>,
    id: String,
    target: u32,
) -> Result<MoveTitlePreviewDto, LibraryOpsCommandError> {
    let service = ops.change(&library)?;
    let ids = ids_of(&[id])?;
    blocking(move || {
        let plan = service.plan(&ids, i64::from(target))?;
        let row = plan
            .rows
            .into_iter()
            .next()
            .ok_or_else(|| LibraryOpsCommandError {
                code: "bad_id".to_owned(),
                message: "This title is not in the library.".to_owned(),
            })?;
        let entries = row
            .entries
            .iter()
            .cloned()
            .map(MoveEntryDto::from)
            .collect();
        Ok(MoveTitlePreviewDto {
            row: row.into(),
            entries,
        })
    })
    .await
}

/// How one title ended.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct MoveOutcomeDto {
    pub id: String,
    pub title: String,
    /// `moved`, `failed`, `skipped`, `canceled` or `stopped`. A stopped move removed nothing and
    /// can be resumed or rolled back from Activity.
    pub result: String,
    /// Why a title failed or was skipped (the preview status for a skipped one).
    pub reason: String,
    pub notes: Vec<String>,
}

/// The answer of a change of data source.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct MoveAppliedDto {
    /// The journal batch; `organize_undo` takes it back. None when nothing moved.
    pub batch_id: Option<String>,
    pub items: Vec<MoveOutcomeDto>,
    pub moved: u32,
}

impl From<MoveApplied> for MoveAppliedDto {
    fn from(applied: MoveApplied) -> Self {
        let moved = u32::try_from(applied.moved()).unwrap_or(u32::MAX);
        Self {
            batch_id: applied.batch_id,
            moved,
            items: applied
                .items
                .into_iter()
                .map(|item| {
                    let (result, reason) = match item.result {
                        MoveResult::Moved => ("moved", String::new()),
                        MoveResult::Failed(why) => ("failed", why),
                        MoveResult::Skipped(status) => {
                            ("skipped", status_parts(&status).0.to_owned())
                        }
                        MoveResult::Canceled => ("canceled", String::new()),
                        MoveResult::Stopped(why) => ("stopped", why),
                    };
                    MoveOutcomeDto {
                        id: item.id.0.to_string(),
                        title: item.title,
                        result: result.to_owned(),
                        reason,
                        notes: item.notes,
                    }
                })
                .collect(),
        }
    }
}

/// Progress of a move: after each title, and while a file is copied.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct MoveProgressDto {
    /// Titles finished.
    pub done: u32,
    pub total: u32,
    /// The title being moved, or the one just finished.
    pub title: String,
    /// Bytes copied and to copy in all (0 for a rename).
    pub bytes_done: f64,
    pub bytes_total: f64,
    /// The file being copied, relative to the title folder; empty between files.
    pub current_file: String,
    /// `copying`, `verifying` or `finishing`.
    pub phase: String,
    /// Bytes read or written per second lately; 0 until known.
    pub bytes_per_second: f64,
    /// Seconds the rest of the move needs at that speed (copy and check); `null` until known.
    pub seconds_left: Option<f64>,
}

impl From<&MoveProgress> for MoveProgressDto {
    fn from(progress: &MoveProgress) -> Self {
        Self {
            done: u32::try_from(progress.done).unwrap_or(u32::MAX),
            total: u32::try_from(progress.total).unwrap_or(u32::MAX),
            title: progress.title.clone(),
            bytes_done: bytes_of(progress.bytes_done),
            bytes_total: bytes_of(progress.bytes_total),
            current_file: progress.current_file.clone(),
            phase: match progress.phase {
                MovePhase::Copying => "copying",
                MovePhase::Verifying => "verifying",
                MovePhase::Finishing => "finishing",
            }
            .to_owned(),
            bytes_per_second: bytes_of(progress.bytes_per_second),
            seconds_left: progress.seconds_left.map(bytes_of),
        }
    }
}

/// Moves the titles to the data source as one undoable batch. Progress goes to `on_progress`.
#[tauri::command]
#[specta::specta]
pub async fn apply_change_source(
    library: State<'_, LibraryState>,
    ops: State<'_, LibraryOpsState>,
    ids: Vec<String>,
    target: u32,
    on_progress: Channel<MoveProgressDto>,
) -> Result<MoveAppliedDto, LibraryOpsCommandError> {
    let service = ops.change(&library)?;
    let ids = ids_of(&ids)?;
    let control = MoveControl::new();
    *ops.control.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::clone(&control));
    let result = blocking(move || {
        let applied =
            service.apply_with(&ids, i64::from(target), Some(control), &mut |progress| {
                // A closed channel (the page was left) must not stop the move.
                let _ = on_progress.send(MoveProgressDto::from(progress));
            })?;
        Ok(applied.into())
    })
    .await;
    *ops.control.lock().unwrap_or_else(PoisonError::into_inner) = None;
    result
}

/// What to do with the move that is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum MoveControlDto {
    /// Hold the copy between two chunks.
    Pause,
    /// Carry on after a pause.
    Resume,
    /// Stop. Before the copy is verified the title is taken back to how it was; after that it is
    /// already whole in the new place and finishes.
    Cancel,
}

/// Pauses, resumes or cancels the move that is running. Does nothing when none is.
#[tauri::command]
#[specta::specta]
pub fn control_move(ops: State<'_, LibraryOpsState>, action: MoveControlDto) {
    let control = ops
        .control
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    if let Some(control) = control {
        match action {
            MoveControlDto::Pause => control.pause(),
            MoveControlDto::Resume => control.resume(),
            MoveControlDto::Cancel => {
                control.cancel();
                // A cancel must not wait behind a pause.
                control.resume();
            }
        }
    }
}

// ---- unfinished moves and renames ------------------------------------------------------------

/// A move or a rename that did not finish: Activity's Needs attention offers Resume and Roll
/// back for it.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct UnfinishedBatchDto {
    pub batch_id: String,
    /// `move` or `rename`.
    pub kind: String,
    /// When the batch was written to the journal, epoch milliseconds.
    pub created_ms: f64,
    /// The first few titles that still have work.
    pub titles: Vec<String>,
    /// How many titles still have work.
    pub items: u32,
    /// Files copied and verified, and files to copy (a move); titles renamed, and titles (a rename).
    pub files_done: u32,
    pub files_total: u32,
    /// A plain sentence: why it stopped.
    pub reason: String,
}

impl From<UnfinishedBatch> for UnfinishedBatchDto {
    fn from(batch: UnfinishedBatch) -> Self {
        let small = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        Self {
            batch_id: batch.batch_id,
            kind: match batch.kind {
                BatchKind::Move => "move",
                BatchKind::Rename => "rename",
            }
            .to_owned(),
            created_ms: bytes_of(u64::try_from(batch.created_ms).unwrap_or(0)),
            titles: batch.titles,
            items: small(batch.items),
            files_done: small(batch.files_done),
            files_total: small(batch.files_total),
            reason: batch.reason,
        }
    }
}

/// Lists the moves and renames that did not finish, newest first.
#[tauri::command]
#[specta::specta]
pub async fn list_unfinished_batches(
    library: State<'_, LibraryState>,
    ops: State<'_, LibraryOpsState>,
) -> Result<Vec<UnfinishedBatchDto>, LibraryOpsCommandError> {
    let service = ops.recovery(&library)?;
    blocking(move || {
        let found = service.find().map_err(recovery_error)?;
        Ok(found.into_iter().map(UnfinishedBatchDto::from).collect())
    })
    .await
}

fn recovery_error(error: cv_app::library_ops::RecoveryError) -> LibraryOpsCommandError {
    use cv_app::library_ops::RecoveryError as E;
    LibraryOpsCommandError {
        code: match &error {
            E::Db(_) => "storage",
            E::Unknown(_) => "unknown_batch",
            E::Busy(_) => "busy",
            E::Journal(_) | E::Exec(_) | E::Undo(_) | E::Move(_) => "journal",
        }
        .to_owned(),
        message: error.to_string(),
    }
}

/// What a resume did.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct ResumeOutcomeDto {
    pub batch_id: String,
    /// Titles now in their new place (steps run, for a rename).
    pub completed: u32,
    /// Why the batch stopped again; it is still unfinished then.
    pub stopped: Option<String>,
    pub notes: Vec<String>,
}

/// Carries on an unfinished move or rename: the files already copied are checked again against
/// their originals, then the rest is copied and the titles are put in place. Progress goes to
/// `on_progress`; `control_move` pauses and cancels it.
#[tauri::command]
#[specta::specta]
pub async fn resume_unfinished_batch(
    library: State<'_, LibraryState>,
    ops: State<'_, LibraryOpsState>,
    batch_id: String,
    on_progress: Channel<MoveProgressDto>,
) -> Result<ResumeOutcomeDto, LibraryOpsCommandError> {
    let service = ops.recovery(&library)?;
    let control = MoveControl::new();
    *ops.control.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::clone(&control));
    let result = blocking(move || {
        let outcome = service
            .resume(&batch_id, Some(control), &mut |progress| {
                let _ = on_progress.send(MoveProgressDto::from(progress));
            })
            .map_err(recovery_error)?;
        Ok(ResumeOutcomeDto {
            batch_id: outcome.batch_id,
            completed: u32::try_from(outcome.completed).unwrap_or(u32::MAX),
            stopped: outcome.stopped,
            notes: outcome.notes,
        })
    })
    .await;
    *ops.control.lock().unwrap_or_else(PoisonError::into_inner) = None;
    result
}

/// What a roll back did.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct RollBackOutcomeDto {
    pub batch_id: String,
    /// Originals or old names put back.
    pub restored: u32,
    /// Copies and part files removed.
    pub removed: u32,
    /// What could not be taken back, with the reason.
    pub problems: Vec<String>,
}

/// Takes an unfinished move or rename back, removing only what the batch itself made (the
/// `.cvpart` files, the copies and the staging folder); the original files were never touched.
#[tauri::command]
#[specta::specta]
pub async fn roll_back_unfinished_batch(
    library: State<'_, LibraryState>,
    ops: State<'_, LibraryOpsState>,
    batch_id: String,
) -> Result<RollBackOutcomeDto, LibraryOpsCommandError> {
    let service = ops.recovery(&library)?;
    blocking(move || {
        let outcome = service.roll_back(&batch_id).map_err(recovery_error)?;
        Ok(RollBackOutcomeDto {
            batch_id: outcome.batch_id,
            restored: u32::try_from(outcome.restored).unwrap_or(u32::MAX),
            removed: u32::try_from(outcome.removed).unwrap_or(u32::MAX),
            problems: outcome.problems,
        })
    })
    .await
}

// ---- remove and delete -----------------------------------------------------------------------

/// One title of the removal preview.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct RemovalRowDto {
    pub id: String,
    /// `movie`, `show`, `season` or `episode`.
    pub kind: String,
    pub title: String,
    pub year: u32,
    pub folder: String,
    /// `own`, `shared`, `source_root` or `none`: what the title has in its folder.
    pub folder_use: String,
    pub files: u32,
    pub bytes: f64,
    /// Seasons and episodes below the title.
    pub children: u32,
    pub on_disk: bool,
}

/// A title an operation did not handle.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct ProblemDto {
    pub id: String,
    pub title: String,
    pub reason: String,
}

impl From<Problem> for ProblemDto {
    fn from(problem: Problem) -> Self {
        Self {
            id: problem.id.0.to_string(),
            title: problem.title,
            reason: problem.reason,
        }
    }
}

/// The preview of a removal.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct RemovalPlanDto {
    pub rows: Vec<RemovalRowDto>,
    pub skipped: Vec<ProblemDto>,
}

impl From<RemovalRow> for RemovalRowDto {
    fn from(row: RemovalRow) -> Self {
        Self {
            id: row.id.0.to_string(),
            kind: kind_text(row.kind).to_owned(),
            title: row.title,
            year: u32::from(row.year),
            folder: row.folder,
            folder_use: match row.folder_use {
                FolderUse::Own => "own",
                FolderUse::Shared => "shared",
                FolderUse::SourceRoot => "source_root",
                FolderUse::NoFolder => "none",
            }
            .to_owned(),
            files: row.files,
            bytes: bytes_of(row.bytes),
            children: row.children,
            on_disk: row.on_disk,
        }
    }
}

impl From<RemovalPlan> for RemovalPlanDto {
    fn from(plan: RemovalPlan) -> Self {
        Self {
            rows: plan.rows.into_iter().map(RemovalRowDto::from).collect(),
            skipped: plan.skipped.into_iter().map(ProblemDto::from).collect(),
        }
    }
}

/// Says what removing or deleting the titles would touch. Changes nothing.
#[tauri::command]
#[specta::specta]
pub async fn plan_removal(
    library: State<'_, LibraryState>,
    ops: State<'_, LibraryOpsState>,
    ids: Vec<String>,
) -> Result<RemovalPlanDto, LibraryOpsCommandError> {
    let service = ops.removal(&library)?;
    let ids = ids_of(&ids)?;
    blocking(move || Ok(service.plan(&ids)?.into())).await
}

/// A title taken out of the library.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct RemovedTitleDto {
    pub id: String,
    pub title: String,
    /// `not_requested`, `written` or `skipped`.
    pub ignore: String,
    /// Why the marker was not written, for `skipped`.
    pub ignore_reason: String,
}

/// The answer of "remove from library".
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct RemoveReportDto {
    pub removed: Vec<RemovedTitleDto>,
    pub problems: Vec<ProblemDto>,
}

impl From<RemoveReport> for RemoveReportDto {
    fn from(report: RemoveReport) -> Self {
        Self {
            removed: report
                .removed
                .into_iter()
                .map(|title| {
                    let (ignore, ignore_reason) = match title.ignore {
                        IgnoreOutcome::NotRequested => ("not_requested", String::new()),
                        IgnoreOutcome::Written => ("written", String::new()),
                        IgnoreOutcome::Skipped(why) => ("skipped", why),
                    };
                    RemovedTitleDto {
                        id: title.id.0.to_string(),
                        title: title.title,
                        ignore: ignore.to_owned(),
                        ignore_reason,
                    }
                })
                .collect(),
            problems: report.problems.into_iter().map(ProblemDto::from).collect(),
        }
    }
}

/// Takes the titles out of the library and leaves their files alone. With `ignore_folders`, each
/// title that has its folder to itself also gets a `.cinevaultignore` marker.
#[tauri::command]
#[specta::specta]
pub async fn remove_from_library(
    library: State<'_, LibraryState>,
    ops: State<'_, LibraryOpsState>,
    ids: Vec<String>,
    ignore_folders: bool,
) -> Result<RemoveReportDto, LibraryOpsCommandError> {
    let service = ops.removal(&library)?;
    let ids = ids_of(&ids)?;
    blocking(move || Ok(service.remove_from_library(&ids, ignore_folders)?.into())).await
}

/// A title deleted with its files.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct DeletedTitleDto {
    pub id: String,
    pub title: String,
    /// `system_trash`, `data_source_trash` or `nothing_on_disk`.
    pub method: String,
    pub paths: u32,
    pub notes: Vec<String>,
}

/// The answer of "delete with files".
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct DeleteReportDto {
    pub deleted: Vec<DeletedTitleDto>,
    pub problems: Vec<ProblemDto>,
    /// The journal batch when the data source trash was used; `organize_undo` takes it back.
    pub batch_id: Option<String>,
}

impl From<DeleteReport> for DeleteReportDto {
    fn from(report: DeleteReport) -> Self {
        Self {
            batch_id: report.batch_id,
            problems: report.problems.into_iter().map(ProblemDto::from).collect(),
            deleted: report
                .deleted
                .into_iter()
                .map(|title| DeletedTitleDto {
                    id: title.id.0.to_string(),
                    title: title.title,
                    method: match title.method {
                        DeleteMethod::SystemTrash => "system_trash",
                        DeleteMethod::DataSourceTrash => "data_source_trash",
                        DeleteMethod::NothingOnDisk => "nothing_on_disk",
                        DeleteMethod::Erased => "erased",
                    }
                    .to_owned(),
                    paths: title.paths,
                    notes: title.notes,
                })
                .collect(),
        }
    }
}

/// Sends the titles' files to the system trash and removes the titles from the library. Files
/// the system trash cannot take go to the data source's trash folder; nothing is deleted for good.
#[tauri::command]
#[specta::specta]
pub async fn delete_with_files(
    library: State<'_, LibraryState>,
    ops: State<'_, LibraryOpsState>,
    ids: Vec<String>,
) -> Result<DeleteReportDto, LibraryOpsCommandError> {
    let service = ops.removal(&library)?;
    let ids = ids_of(&ids)?;
    blocking(move || Ok(service.delete_with_files(&ids)?.into())).await
}

// ---- missing and duplicate review ------------------------------------------------------------

/// The row a duplicate duplicates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct DuplicateOfDto {
    pub id: String,
    pub title: String,
    pub year: u32,
}

/// One row of the missing/duplicate review.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct ReviewRowDto {
    pub id: String,
    /// `movie`, `show` or `episode`.
    pub kind: String,
    pub title: String,
    pub year: u32,
    /// The data source the title is in.
    pub source: String,
    pub folder: String,
    /// `missing`, `offline`, `duplicate` or `superseded`.
    pub reason: String,
    /// One plain sentence saying why the row is listed.
    pub detail: String,
    pub duplicate_of: Option<DuplicateOfDto>,
    pub files: u32,
    pub bytes: f64,
    /// When a scan last saw the title's files, else when the row last changed (epoch ms).
    pub last_seen_ms: f64,
    /// The title can be pointed at the folder its files moved to.
    pub can_relocate: bool,
}

/// The review list.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct ReviewListDto {
    pub rows: Vec<ReviewRowDto>,
    /// Rows the user chose to keep, which are left out of `rows`.
    pub kept: u32,
}

impl From<ReviewRow> for ReviewRowDto {
    fn from(row: ReviewRow) -> Self {
        Self {
            id: row.id.0.to_string(),
            kind: kind_text(row.kind).to_owned(),
            title: row.title,
            year: u32::from(row.year),
            source: row.source,
            folder: row.folder,
            reason: row.reason.as_str().to_owned(),
            detail: row.detail,
            duplicate_of: row.duplicate_of.map(|of| DuplicateOfDto {
                id: of.id.0.to_string(),
                title: of.title,
                year: u32::from(of.year),
            }),
            files: row.files,
            bytes: bytes_of(row.bytes),
            last_seen_ms: millis_of(row.last_seen_ms),
            can_relocate: row.can_relocate,
        }
    }
}

#[allow(clippy::cast_precision_loss)]
fn millis_of(ms: i64) -> f64 {
    ms as f64
}

impl From<ReviewList> for ReviewListDto {
    fn from(list: ReviewList) -> Self {
        Self {
            rows: list.rows.into_iter().map(ReviewRowDto::from).collect(),
            kept: list.kept,
        }
    }
}

fn review(
    ops: &State<'_, LibraryOpsState>,
    library: &State<'_, LibraryState>,
) -> Result<Arc<ReviewService>, LibraryOpsCommandError> {
    ops.review(library)
}

/// The titles that are missing on disk, offline, duplicates of another row or set aside by a
/// rescan, each with its reason, size and last-seen date. Reads only: nothing is removed.
#[tauri::command]
#[specta::specta]
pub async fn list_library_review(
    library: State<'_, LibraryState>,
    ops: State<'_, LibraryOpsState>,
) -> Result<ReviewListDto, LibraryOpsCommandError> {
    let service = review(&ops, &library)?;
    blocking(move || Ok(service.list()?.into())).await
}

/// The answer of removing review rows.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct ReviewRemovalDto {
    pub removed: Vec<RemovedTitleDto>,
    pub problems: Vec<ProblemDto>,
    /// Pass to `undo_review_removal` to put the removed rows back. None when nothing was removed.
    pub undo_token: Option<String>,
}

/// Takes the chosen review rows out of the library. Their files stay on disk. Only rows that the
/// review lists can be removed.
#[tauri::command]
#[specta::specta]
pub async fn remove_review_rows(
    library: State<'_, LibraryState>,
    ops: State<'_, LibraryOpsState>,
    ids: Vec<String>,
) -> Result<ReviewRemovalDto, LibraryOpsCommandError> {
    let service = review(&ops, &library)?;
    let ids = ids_of(&ids)?;
    blocking(move || {
        let done = service.remove(&ids)?;
        let report = RemoveReportDto::from(done.report);
        Ok(ReviewRemovalDto {
            removed: report.removed,
            problems: report.problems,
            undo_token: done.undo_token,
        })
    })
    .await
}

/// A removed row put back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct RestoredTitleDto {
    /// The id the title had before it was removed.
    pub was: String,
    /// The id it has now.
    pub id: String,
    pub title: String,
}

/// Puts back the rows of a removal (the Undo of `remove_review_rows`). The titles get new ids.
#[tauri::command]
#[specta::specta]
pub async fn undo_review_removal(
    library: State<'_, LibraryState>,
    ops: State<'_, LibraryOpsState>,
    token: String,
) -> Result<Vec<RestoredTitleDto>, LibraryOpsCommandError> {
    let service = review(&ops, &library)?;
    blocking(move || {
        Ok(service
            .undo_removal(&token)?
            .into_iter()
            .map(|title| RestoredTitleDto {
                was: title.was.0.to_string(),
                id: title.id.0.to_string(),
                title: title.title,
            })
            .collect())
    })
    .await
}

/// Leaves the chosen rows in the library and out of the review until they have a different
/// problem. Returns how many were kept.
#[tauri::command]
#[specta::specta]
pub async fn keep_review_rows(
    library: State<'_, LibraryState>,
    ops: State<'_, LibraryOpsState>,
    ids: Vec<String>,
) -> Result<u32, LibraryOpsCommandError> {
    let service = review(&ops, &library)?;
    let ids = ids_of(&ids)?;
    blocking(move || Ok(u32::try_from(service.keep(&ids)?).unwrap_or(u32::MAX))).await
}

/// Takes back "keep": the rows are listed again while they still have a problem.
#[tauri::command]
#[specta::specta]
pub async fn unkeep_review_rows(
    library: State<'_, LibraryState>,
    ops: State<'_, LibraryOpsState>,
    ids: Vec<String>,
) -> Result<(), LibraryOpsCommandError> {
    let service = review(&ops, &library)?;
    let ids = ids_of(&ids)?;
    blocking(move || Ok(service.unkeep(&ids)?)).await
}

/// A title pointed at its new folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct RelocatedDto {
    pub id: String,
    pub from: String,
    pub to: String,
    /// `online`, `offline`, `missing` or `invalid` after the move.
    pub availability: String,
}

/// Points a missing or offline movie or show at the folder its files moved to. No file is
/// touched; `from` lets the page offer to point it back.
#[tauri::command]
#[specta::specta]
pub async fn relocate_review_row(
    library: State<'_, LibraryState>,
    ops: State<'_, LibraryOpsState>,
    id: String,
    folder: String,
) -> Result<RelocatedDto, LibraryOpsCommandError> {
    let service = review(&ops, &library)?;
    let id = ids_of(std::slice::from_ref(&id))?[0];
    blocking(move || {
        let done = service.relocate(id, &folder)?;
        Ok(RelocatedDto {
            id: done.id.0.to_string(),
            from: done.from,
            to: done.to,
            availability: done.availability.as_str().to_owned(),
        })
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_numbers_and_anything_else_is_refused() {
        assert_eq!(
            ids_of(&["12".to_owned(), " 7 ".to_owned()]).unwrap(),
            vec![ItemId(12), ItemId(7)]
        );
        assert_eq!(ids_of(&["x".to_owned()]).unwrap_err().code, "bad_id");
    }

    #[test]
    fn a_status_becomes_a_code_and_a_reason() {
        assert_eq!(status_parts(&MoveStatus::Ready), ("ready", String::new()));
        assert_eq!(
            status_parts(&MoveStatus::Unavailable("gone".into())),
            ("unavailable", "gone".to_owned())
        );
    }

    #[test]
    fn a_move_row_carries_the_space_check_and_the_conflicts() {
        let row = MoveRow {
            id: ItemId(4),
            kind: TitleKind::Movie,
            title: "Heat".into(),
            year: 1995,
            source: "D:/Movies".into(),
            from: "D:/Movies/Heat (1995)".into(),
            to: "Z:/NAS 4K/Heat (1995)".into(),
            status: MoveStatus::InsufficientSpace {
                needed: 62,
                free: 40,
            },
            files: 3,
            bytes: 60,
            across_volumes: true,
            entries: vec![
                MoveEntry {
                    path: "Heat.mkv".into(),
                    kind: EntryKind::File,
                    size: 60,
                },
                MoveEntry {
                    path: "link".into(),
                    kind: EntryKind::Link {
                        target: "/elsewhere".into(),
                    },
                    size: 0,
                },
            ],
            needed_bytes: 62,
            free_bytes: Some(40),
            conflicts: vec!["Z:/NAS 4K/Heat (1995)".into()],
        };
        let entries: Vec<MoveEntryDto> = row
            .entries
            .iter()
            .cloned()
            .map(MoveEntryDto::from)
            .collect();
        let dto = MoveRowDto::from(row);
        assert_eq!(dto.status, "insufficient_space");
        assert_eq!((dto.needed_bytes, dto.free_bytes), (62.0, Some(40.0)));
        assert_eq!(dto.conflicts, ["Z:/NAS 4K/Heat (1995)"]);
        assert_eq!(entries[0].kind, "file");
        assert_eq!(
            (entries[1].kind.as_str(), entries[1].link_target.as_str()),
            ("link", "/elsewhere")
        );
    }

    #[test]
    fn an_unfinished_batch_names_its_titles_progress_and_reason() {
        let dto = UnfinishedBatchDto::from(UnfinishedBatch {
            batch_id: "move-1700000000000-1".into(),
            kind: BatchKind::Move,
            created_ms: 1_700_000_000_000,
            titles: vec!["Heat".into()],
            items: 1,
            files_done: 3,
            files_total: 9,
            reason: "Destination is full.".into(),
            in_new_place: false,
        });
        assert_eq!(dto.kind, "move");
        assert_eq!((dto.files_done, dto.files_total, dto.items), (3, 9, 1));
        assert_eq!(dto.reason, "Destination is full.");
        let stopped = MoveResult::Stopped("Destination is full".into());
        let applied = MoveApplied {
            batch_id: Some("move-1".into()),
            items: vec![cv_app::library_ops::MoveOutcome {
                id: ItemId(1),
                title: "Heat".into(),
                result: stopped,
                notes: Vec::new(),
            }],
        };
        let dto = MoveAppliedDto::from(applied);
        assert_eq!(dto.items[0].result, "stopped");
        assert_eq!(dto.items[0].reason, "Destination is full");
    }

    /// The missing/duplicate review through its commands, the way the page calls them: list,
    /// keep and unkeep, remove with Undo, and relocate.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_review_commands_list_keep_remove_undo_and_relocate() {
        use std::path::Path;

        use cv_app::data_location::{DataLocation, LocationInputs};
        use cv_app::secrets::SecretStore;
        use cv_app::services::{AppServices, BootOptions};
        use cv_db::repo::data_sources::insert_source;
        use cv_db::repo::{StdFileProbe, get_item, insert_item};
        use cv_domain::{Item, MediaFile, MediaFileType, SourceKind};
        use cv_scan::sources::{PathStyle, SourceGuard, SystemFolders};
        use tauri::Manager;
        use tauri::test::{mock_builder, mock_context, noop_assets};

        use crate::boot::Boot;

        let temp = tempfile::tempdir().unwrap();
        let inputs = LocationInputs {
            exe_dir: temp.path().join("app"),
            user_data_base: temp.path().join("userdata"),
            user_config_base: temp.path().join("userconfig"),
        };
        std::fs::create_dir_all(&inputs.exe_dir).unwrap();
        let services = AppServices::boot(BootOptions::new(
            DataLocation::resolve(&inputs),
            Arc::new(SecretStore::in_memory()),
        ))
        .expect("boot");
        let db = Arc::clone(services.db());
        let movies = temp.path().join("media").join("Movies");
        let archive = temp.path().join("media").join("Archive");
        let (movies_root, archive_root) = (
            movies.to_string_lossy().into_owned(),
            archive.to_string_lossy().into_owned(),
        );
        {
            let (a, b) = (movies_root.clone(), archive_root.clone());
            db.write(move |tx| {
                insert_source(tx, SourceKind::Movie, &a)?;
                insert_source(tx, SourceKind::Movie, &b)?;
                Ok(())
            })
            .unwrap();
        }
        let add_movie = |root: &Path, name: &str| -> ItemId {
            let folder = root.join(name);
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(folder.join(format!("{name}.mkv")), b"video").unwrap();
            let mut item = Item::movie(name.split(" (").next().unwrap());
            item.core.year = 1995;
            item.core.data_source = root.to_string_lossy().into_owned();
            item.core.path = folder.to_string_lossy().into_owned();
            let mut video = MediaFile::new(
                MediaFileType::Video,
                folder.to_string_lossy(),
                format!("{name}.mkv"),
            );
            video.size_bytes = 5;
            item.core.media_files.push(video);
            db.write(move |tx| insert_item(tx, &item)).unwrap()
        };
        let gone = add_movie(&movies, "Gone (1995)");
        let moved = add_movie(&movies, "Moved (1995)");
        add_movie(&movies, "Fine (1995)");
        std::fs::remove_dir_all(movies.join("Gone (1995)")).unwrap();
        std::fs::create_dir_all(&archive).unwrap();
        std::fs::rename(movies.join("Moved (1995)"), archive.join("Moved (1995)")).unwrap();
        db.refresh_availability(&StdFileProbe, cv_db::repo::PathStyle::current())
            .unwrap();

        let boot = Boot::for_services(Arc::clone(&services));
        let app = mock_builder()
            .build(mock_context(noop_assets()))
            .expect("mock app builds");
        let guard = SourceGuard::new(
            PathStyle::current(),
            SystemFolders::new(PathStyle::current()),
        );
        app.manage(LibraryState::with_guard(&boot, guard));
        app.manage(LibraryOpsState::new(Arc::new(LibrarySources::new())));
        let library = || app.state::<LibraryState>();
        let ops = || app.state::<LibraryOpsState>();
        let listed = |list: &ReviewListDto| -> Vec<(String, String)> {
            let mut rows: Vec<(String, String)> = list
                .rows
                .iter()
                .map(|row| (row.title.clone(), row.reason.clone()))
                .collect();
            rows.sort();
            rows
        };

        let list = list_library_review(library(), ops()).await.expect("list");
        assert_eq!(
            listed(&list),
            [
                ("Gone".to_owned(), "missing".to_owned()),
                ("Moved".to_owned(), "missing".to_owned())
            ]
        );
        assert_eq!(list.kept, 0);

        // Keep leaves the row in the library and out of the list; unkeep lists it again.
        let kept = keep_review_rows(library(), ops(), vec![gone.0.to_string()])
            .await
            .expect("keep");
        assert_eq!(kept, 1);
        let list = list_library_review(library(), ops()).await.expect("list");
        assert_eq!(list.kept, 1);
        assert!(list.rows.iter().all(|row| row.id != gone.0.to_string()));
        unkeep_review_rows(library(), ops(), vec![gone.0.to_string()])
            .await
            .expect("unkeep");
        assert_eq!(
            list_library_review(library(), ops())
                .await
                .expect("list")
                .rows
                .len(),
            2
        );

        // Remove takes the row out; its Undo puts it back under a new id.
        let removed = remove_review_rows(library(), ops(), vec![gone.0.to_string()])
            .await
            .expect("remove");
        assert_eq!(removed.removed.len(), 1);
        assert!(db.read(move |c| get_item(c, gone)).unwrap().is_none());
        let token = removed.undo_token.expect("an undo token");
        let restored = undo_review_removal(library(), ops(), token)
            .await
            .expect("undo");
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].was, gone.0.to_string());
        assert_eq!(restored[0].title, "Gone");
        // An id that is not a number is refused before anything runs.
        let refused = keep_review_rows(library(), ops(), vec!["x".to_owned()])
            .await
            .unwrap_err();
        assert_eq!(refused.code, "bad_id");

        // Relocate points the moved title at its new folder; no file is touched.
        let to = archive.join("Moved (1995)").to_string_lossy().into_owned();
        let relocated = relocate_review_row(library(), ops(), moved.0.to_string(), to.clone())
            .await
            .expect("relocate");
        assert_eq!(relocated.availability, "online");
        assert_eq!(relocated.to, to);
        assert!(
            archive
                .join("Moved (1995)")
                .join("Moved (1995).mkv")
                .is_file()
        );
        let list = list_library_review(library(), ops()).await.expect("list");
        assert_eq!(listed(&list), [("Gone".to_owned(), "missing".to_owned())]);
    }

    #[test]
    fn a_review_row_keeps_its_reason_size_and_original() {
        use cv_app::library_ops::{DuplicateOf, ReviewReason};
        let row = ReviewRow {
            id: ItemId(9),
            kind: TitleKind::Show,
            title: "Breaking Bad".into(),
            year: 2008,
            source: "D:/TV".into(),
            folder: "D:/TV/Breaking Bad".into(),
            reason: ReviewReason::Duplicate,
            detail: "Duplicate of Breaking Bad (2008)".into(),
            duplicate_of: Some(DuplicateOf {
                id: ItemId(3),
                title: "Breaking Bad".into(),
                year: 2008,
            }),
            files: 62,
            bytes: 4_000,
            last_seen_ms: 1_700_000_000_000,
            can_relocate: false,
        };
        let dto = ReviewRowDto::from(row);
        assert_eq!((dto.id.as_str(), dto.kind.as_str()), ("9", "show"));
        assert_eq!(dto.reason, "duplicate");
        assert_eq!(dto.duplicate_of.map(|of| of.id), Some("3".to_owned()));
        assert_eq!((dto.files, dto.bytes), (62, 4_000.0));
        assert!(!dto.can_relocate);
    }

    /// The missing and duplicate review through its commands: list, keep and take it
    /// back, remove with Undo, and relocate. Nothing is ever removed without a command.
    mod commands {
        use std::path::Path;
        use std::sync::Arc;

        use cv_app::data_location::{DataLocation, LocationInputs};
        use cv_app::secrets::SecretStore;
        use cv_app::services::{AppServices, BootOptions};
        use cv_domain::{Availability, Item, ItemId, SourceKind};
        use cv_scan::sources::{PathStyle, SourceGuard, SystemFolders};
        use tauri::Manager;
        use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};

        use super::super::*;
        use crate::boot::Boot;

        fn app(root: &Path) -> (tauri::App<MockRuntime>, Arc<AppServices>) {
            let inputs = LocationInputs {
                exe_dir: root.join("app"),
                user_data_base: root.join("userdata"),
                user_config_base: root.join("userconfig"),
            };
            std::fs::create_dir_all(&inputs.exe_dir).unwrap();
            let options = BootOptions::new(
                DataLocation::resolve(&inputs),
                Arc::new(SecretStore::in_memory()),
            )
            .without_providers();
            let services = AppServices::boot(options).expect("boot");
            let boot = Boot::for_services(Arc::clone(&services));
            let app = mock_builder()
                .build(mock_context(noop_assets()))
                .expect("mock app builds");
            let guard = SourceGuard::new(
                PathStyle::current(),
                SystemFolders::new(PathStyle::current()),
            );
            app.manage(LibraryState::with_guard(&boot, guard));
            app.manage(LibraryOpsState::new(Arc::new(LibrarySources::new())));
            app.manage(boot);
            (app, services)
        }

        /// A movie whose folder is gone from the movie library folder `library`.
        fn missing_movie(services: &AppServices, library: &Path, title: &str) -> ItemId {
            let library_path = library.to_string_lossy().into_owned();
            let mut item = Item::movie(title);
            item.core.year = 1995;
            item.core.path = library.join(title).to_string_lossy().into_owned();
            item.core.data_source.clone_from(&library_path);
            services
                .db()
                .write(move |conn| {
                    let id = cv_db::repo::insert_item(conn, &item)?;
                    cv_db::repo::set_availability(conn, id, Availability::Missing, "gone")?;
                    Ok(id)
                })
                .unwrap()
        }

        async fn rows(app: &tauri::App<MockRuntime>) -> ReviewListDto {
            list_library_review(app.state(), app.state())
                .await
                .expect("list")
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn keep_remove_undo_and_relocate_go_through_the_commands() {
            let root = tempfile::tempdir().unwrap();
            let library = root.path().join("Movies");
            std::fs::create_dir_all(&library).unwrap();
            let (app, services) = app(root.path());
            let library_text = library.to_string_lossy().into_owned();
            services
                .db()
                .write(move |conn| {
                    cv_db::repo::data_sources::insert_source(conn, SourceKind::Movie, &library_text)
                })
                .unwrap();
            let heat = missing_movie(&services, &library, "Heat (1995)");
            let ronin = missing_movie(&services, &library, "Ronin (1995)");

            let listed = rows(&app).await;
            assert_eq!(listed.rows.len(), 2);
            assert!(listed.rows.iter().all(|row| row.reason == "missing"));

            // Keep leaves the row in the library and out of the list; unkeep lists it again.
            let kept = keep_review_rows(app.state(), app.state(), vec![heat.0.to_string()])
                .await
                .expect("keep");
            assert_eq!(kept, 1);
            let listed = rows(&app).await;
            assert_eq!((listed.rows.len(), listed.kept), (1, 1));
            let still = services
                .db()
                .read(move |conn| cv_db::repo::get_item(conn, heat))
                .unwrap();
            assert!(still.is_some(), "a kept row stays in the library");
            unkeep_review_rows(app.state(), app.state(), vec![heat.0.to_string()])
                .await
                .expect("unkeep");
            assert_eq!(rows(&app).await.rows.len(), 2);

            // Remove takes the row out (its files, had it any, stay); Undo puts it back.
            let removed = remove_review_rows(app.state(), app.state(), vec![ronin.0.to_string()])
                .await
                .expect("remove");
            assert_eq!(removed.removed.len(), 1);
            let token = removed.undo_token.expect("an undo token");
            assert_eq!(rows(&app).await.rows.len(), 1);
            let gone = services
                .db()
                .read(move |conn| cv_db::repo::get_item(conn, ronin))
                .unwrap();
            assert!(gone.is_none());
            let restored = undo_review_removal(app.state(), app.state(), token)
                .await
                .expect("undo");
            assert_eq!(restored.len(), 1);
            assert_eq!(restored[0].title, "Ronin (1995)");
            assert_eq!(rows(&app).await.rows.len(), 2);

            // Relocate points the title at the folder its files moved to: it leaves the list.
            let moved = library.join("Heat 1995 moved");
            std::fs::create_dir_all(&moved).unwrap();
            std::fs::write(moved.join("heat.mkv"), b"video").unwrap();
            let relocated = relocate_review_row(
                app.state(),
                app.state(),
                heat.0.to_string(),
                moved.to_string_lossy().into_owned(),
            )
            .await
            .expect("relocate");
            assert_eq!(relocated.id, heat.0.to_string());
            assert_eq!(relocated.to, moved.to_string_lossy());
            let listed = rows(&app).await;
            assert!(listed.rows.iter().all(|row| row.id != heat.0.to_string()));
        }
    }
}
