//! Typed commands for History, Delete to the Recycle Bin and permanent delete (specs
//! `operation-history` and `title-deletion`).
//!
//! The rules live in `cv_app::history` and `cv_app::library_ops`. Every command that changes the
//! disk runs on a blocking thread and, when it wrote or changed a History entry, emits
//! [`HistoryChanged`] so an open History page refreshes. Ids travel as text.

use std::sync::{Arc, OnceLock};

use cv_app::history::{HistoryKind, HistoryService, UndoReport};
use cv_app::library_ops::{
    DeleteMethod, DeleteReport, EraseReport, ErasedTitle, Problem, RemovalService,
};
use cv_app::organize::OrganizeService;
use cv_artwork::library_source::LibrarySources;
use cv_db::repo::rename_journal::PathObserver;
use cv_domain::ItemId;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, State};
use tauri_specta::Event;

use crate::artwork_ipc::ArtworkState;
use crate::match_ipc::MatchState;
use crate::sources_ipc::LibraryState;

pub use cv_ipc::history::*;

/// Held in Tauri state. The services are built by the first command that needs them.
pub struct HistoryState {
    history: OnceLock<Result<Arc<HistoryService>, String>>,
    removal: OnceLock<Result<Arc<RemovalService>, String>>,
    /// The poster index behind `cv://img`; an Undo that moves folders back tells it.
    sources: Arc<LibrarySources>,
}

impl HistoryState {
    pub fn new(sources: Arc<LibrarySources>) -> Self {
        Self {
            history: OnceLock::new(),
            removal: OnceLock::new(),
            sources,
        }
    }

    fn history(&self, library: &LibraryState) -> Result<Arc<HistoryService>, HistoryCommandError> {
        self.history
            .get_or_init(|| {
                let db = library.database()?;
                let observer: Arc<dyn PathObserver> = self.sources.clone();
                let mut organize = OrganizeService::new(Arc::clone(&db))
                    .with_path_observer(observer)
                    .with_bin(library.bin())
                    .with_choice(library.deleted_files());
                let mut history = HistoryService::new(db).with_bin(library.bin());
                if let Some(state) = library.organize_state() {
                    organize = organize.with_changed_hook(state.hook());
                    history = history.with_changed_hook(state.hook());
                }
                Ok(Arc::new(history.with_organize(organize)))
            })
            .as_ref()
            .map(Arc::clone)
            .map_err(|message| storage(message))
    }

    fn removal(&self, library: &LibraryState) -> Result<Arc<RemovalService>, HistoryCommandError> {
        self.removal
            .get_or_init(|| {
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

/// History changed: an entry was written or undone. `entry_id` names it when one did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, Event)]
pub struct HistoryChanged {
    pub entry_id: Option<String>,
}

fn announce(app: &AppHandle, entry: Option<i64>) {
    let event = HistoryChanged {
        entry_id: entry.map(|id| id.to_string()),
    };
    // A page that misses the event shows the old list until it is opened again: log, move on.
    if let Err(error) = event.emit(app) {
        tracing::warn!(%error, "could not emit the HistoryChanged event");
    }
}

/// Tells the open pages that these titles' files and stage changed.
fn titles_changed(library: &LibraryState, ids: &[ItemId]) {
    if let Some(hub) = library.changes()
        && !ids.is_empty()
    {
        use cv_app::changes::ChangePart;
        hub.publish(
            ids,
            &[
                ChangePart::Files,
                ChangePart::Stage,
                ChangePart::Details,
                ChangePart::Ratings,
                ChangePart::Artwork,
            ],
        );
    }
}

// ---- History ---------------------------------------------------------------------------------

/// One page of History entries of a tab, newest first (`limit` at most 200).
#[tauri::command]
#[specta::specta]
pub async fn history_list(
    library: State<'_, LibraryState>,
    history: State<'_, HistoryState>,
    filter: HistoryFilterDto,
    offset: u32,
    limit: u32,
) -> Result<HistoryPageDto, HistoryCommandError> {
    let service = history.history(&library)?;
    cv_ipc::history::history_list(service, filter, offset, limit).await
}

/// Every title of an entry ("Show N more").
#[tauri::command]
#[specta::specta]
pub async fn history_titles(
    library: State<'_, LibraryState>,
    history: State<'_, HistoryState>,
    entry_id: String,
) -> Result<Vec<HistoryTitleDto>, HistoryCommandError> {
    let service = history.history(&library)?;
    cv_ipc::history::history_titles(service, entry_id).await
}

/// The newest entries that touched a title, with that title's line only (the "Renamed from …"
/// line of a title page reads the newest rename).
#[tauri::command]
#[specta::specta]
pub async fn history_for_title(
    library: State<'_, LibraryState>,
    history: State<'_, HistoryState>,
    id: String,
) -> Result<Vec<HistoryEntryDto>, HistoryCommandError> {
    let service = history.history(&library)?;
    cv_ipc::history::history_for_title(service, id).await
}

/// The History entry of a rename or move batch, so its toast's Undo points at the same record.
#[tauri::command]
#[specta::specta]
pub async fn history_entry_for_batch(
    library: State<'_, LibraryState>,
    history: State<'_, HistoryState>,
    batch_id: String,
) -> Result<Option<String>, HistoryCommandError> {
    let service = history.history(&library)?;
    cv_ipc::history::history_entry_for_batch(service, batch_id).await
}

/// How much of what CineVault sent to the Recycle Bin / Trash is still there, in bytes.
#[tauri::command]
#[specta::specta]
pub async fn history_bin_total(
    library: State<'_, LibraryState>,
    history: State<'_, HistoryState>,
) -> Result<f64, HistoryCommandError> {
    let service = history.history(&library)?;
    cv_ipc::history::history_bin_total(service).await
}

/// A title an Undo left as it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct TitleUndoDto {
    pub id: String,
    pub title: String,
    pub reason: String,
}

/// What an Undo did.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct UndoReportDto {
    pub entry_id: String,
    /// The titles taken back.
    pub undone: Vec<String>,
    /// The titles left as they are, with the reason ("Undo \"Renamed 12 titles\" first.").
    pub left: Vec<TitleUndoDto>,
    /// The entry's state afterwards.
    pub undo_state: String,
    pub undone_at: Option<f64>,
}

impl From<UndoReport> for UndoReportDto {
    fn from(report: UndoReport) -> Self {
        Self {
            entry_id: report.entry_id.to_string(),
            undone: report.undone.iter().map(|id| id.0.to_string()).collect(),
            left: report
                .left
                .into_iter()
                .map(|t| TitleUndoDto {
                    id: t.item_id.0.to_string(),
                    title: t.title,
                    reason: t.reason,
                })
                .collect(),
            undo_state: state_text(report.state).to_owned(),
            undone_at: report.undone_at.map(number),
        }
    }
}

/// Takes an entry back, whichever kind it is: artwork and match entries are undone by their own
/// services (they hold the title and write the NFO again); the others by the History service.
async fn undo_entry(
    history: &HistoryState,
    library: &LibraryState,
    artwork: &ArtworkState,
    matching: &MatchState,
    entry: i64,
    only: Option<ItemId>,
) -> Result<UndoReport, HistoryCommandError> {
    let service = history.history(library)?;
    let kind = {
        let service = Arc::clone(&service);
        blocking(move || Ok(service.entry(entry)?.kind)).await?
    };
    match kind {
        HistoryKind::Artwork => {
            let artwork = artwork.get().map_err(|e| storage(&e.message))?;
            Ok(artwork.undo_entry(entry, only).await?)
        }
        HistoryKind::Match => {
            let matching = matching
                .service
                .clone()
                .ok_or_else(|| storage("CineVault could not open the library."))?;
            Ok(matching.undo_entry(entry, only).await?)
        }
        _ => {
            blocking(move || {
                Ok(match only {
                    Some(item) => service.undo_title(entry, item)?,
                    None => service.undo(entry)?,
                })
            })
            .await
        }
    }
}

/// Takes a whole entry back (History's Undo, Undo all and Restore, and every toast's Undo).
#[tauri::command]
#[specta::specta]
pub async fn history_undo(
    app: AppHandle,
    library: State<'_, LibraryState>,
    history: State<'_, HistoryState>,
    artwork: State<'_, ArtworkState>,
    matching: State<'_, MatchState>,
    entry_id: String,
) -> Result<UndoReportDto, HistoryCommandError> {
    let entry = id_of(&entry_id)?;
    let report = undo_entry(&history, &library, &artwork, &matching, entry, None).await?;
    titles_changed(&library, &report.undone);
    announce(&app, Some(entry));
    Ok(report.into())
}

/// Takes back one title's part of an entry (the Undo of "Renamed from …" on a title page).
#[tauri::command]
#[specta::specta]
pub async fn history_undo_title(
    app: AppHandle,
    library: State<'_, LibraryState>,
    history: State<'_, HistoryState>,
    artwork: State<'_, ArtworkState>,
    matching: State<'_, MatchState>,
    entry_id: String,
    id: String,
) -> Result<UndoReportDto, HistoryCommandError> {
    let entry = id_of(&entry_id)?;
    let item = ItemId(id_of(&id)?);
    let report = undo_entry(&history, &library, &artwork, &matching, entry, Some(item)).await?;
    titles_changed(&library, &report.undone);
    announce(&app, Some(entry));
    Ok(report.into())
}

// ---- Delete and permanent delete -------------------------------------------------------------

/// A title an operation did not handle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct RefusedTitleDto {
    pub id: String,
    pub title: String,
    pub reason: String,
}

fn refused(problems: Vec<Problem>) -> Vec<RefusedTitleDto> {
    problems
        .into_iter()
        .map(|p| RefusedTitleDto {
            id: p.id.0.to_string(),
            title: p.title,
            reason: p.reason,
        })
        .collect()
}

/// A title sent to the Recycle Bin / Trash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct BinnedTitleDto {
    pub id: String,
    pub title: String,
    /// `recycle_bin` (the system bin), `trash_folder` (the library folder's trash folder, for a
    /// network share) or `nothing_on_disk`.
    pub went_to: String,
    pub notes: Vec<String>,
}

/// The answer of Delete.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct DeleteTitlesDto {
    pub deleted: Vec<BinnedTitleDto>,
    /// Titles left as they are (an offline drive, files that could not be sent), with the reason.
    pub refused: Vec<RefusedTitleDto>,
    /// The History entry, which the toast's Undo passes to `history_undo`.
    pub entry_id: Option<String>,
}

impl From<DeleteReport> for DeleteTitlesDto {
    fn from(report: DeleteReport) -> Self {
        Self {
            deleted: report
                .deleted
                .into_iter()
                .map(|t| BinnedTitleDto {
                    id: t.id.0.to_string(),
                    title: t.title,
                    went_to: match t.method {
                        DeleteMethod::SystemTrash => "recycle_bin",
                        DeleteMethod::DataSourceTrash => "trash_folder",
                        DeleteMethod::NothingOnDisk => "nothing_on_disk",
                        DeleteMethod::Erased => "erased",
                    }
                    .to_owned(),
                    notes: t.notes,
                })
                .collect(),
            refused: refused(report.problems),
            entry_id: report.history_id.map(|id| id.to_string()),
        }
    }
}

/// Delete: sends the titles' folders (or their own files) to the Recycle Bin / Trash and takes
/// the titles out of the library, with one History entry for Undo. Nothing is erased.
#[tauri::command]
#[specta::specta]
pub async fn delete_titles(
    app: AppHandle,
    library: State<'_, LibraryState>,
    history: State<'_, HistoryState>,
    ids: Vec<String>,
) -> Result<DeleteTitlesDto, HistoryCommandError> {
    let service = history.removal(&library)?;
    let ids = ids_of(&ids)?;
    let report = blocking(move || Ok(service.delete_with_files(&ids)?)).await?;
    let gone: Vec<ItemId> = report.deleted.iter().map(|t| t.id).collect();
    titles_changed(&library, &gone);
    if report.history_id.is_some() {
        announce(&app, report.history_id);
    }
    Ok(report.into())
}

/// A title of a permanent delete.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct ErasedTitleDto {
    pub id: String,
    pub title: String,
    pub erased: Vec<String>,
    /// What was kept because the title stopped at `error`.
    pub left: Vec<String>,
    /// The file it stopped at, with the reason (a file in use).
    pub error: Option<String>,
}

impl From<ErasedTitle> for ErasedTitleDto {
    fn from(title: ErasedTitle) -> Self {
        Self {
            id: title.id.0.to_string(),
            title: title.title,
            erased: title.erased,
            left: title.left,
            error: title.error,
        }
    }
}

/// The answer of a permanent delete.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct EraseTitlesDto {
    /// Titles erased whole; they left the library.
    pub erased: Vec<ErasedTitleDto>,
    /// Titles that stopped at a file and stay in the library with what is left.
    pub stopped: Vec<ErasedTitleDto>,
    /// Titles refused before anything was erased.
    pub refused: Vec<RefusedTitleDto>,
    /// The History entry (without Undo).
    pub entry_id: Option<String>,
}

impl From<EraseReport> for EraseTitlesDto {
    fn from(report: EraseReport) -> Self {
        Self {
            erased: report.erased.into_iter().map(Into::into).collect(),
            stopped: report.stopped.into_iter().map(Into::into).collect(),
            refused: refused(report.problems),
            entry_id: report.history_id.map(|id| id.to_string()),
        }
    }
}

/// Delete permanently: erases the titles' files for good. `confirm` is what the person typed in
/// the confirmation dialog; anything but exactly `DELETE` erases nothing.
#[tauri::command]
#[specta::specta]
pub async fn erase_titles(
    app: AppHandle,
    library: State<'_, LibraryState>,
    history: State<'_, HistoryState>,
    ids: Vec<String>,
    confirm: String,
) -> Result<EraseTitlesDto, HistoryCommandError> {
    let service = history.removal(&library)?;
    let ids = ids_of(&ids)?;
    let report = blocking(move || Ok(service.erase(&ids, &confirm)?)).await?;
    let touched: Vec<ItemId> = report
        .erased
        .iter()
        .chain(&report.stopped)
        .map(|t| t.id)
        .collect();
    titles_changed(&library, &touched);
    if report.history_id.is_some() {
        announce(&app, report.history_id);
    }
    Ok(report.into())
}

#[cfg(test)]
mod tests {
    use cv_app::history::UndoState;

    use super::*;

    #[test]
    fn an_undo_report_carries_the_reason_of_each_title_left() {
        let report = UndoReport {
            entry_id: 4,
            undone: vec![ItemId(1)],
            left: vec![cv_app::history::TitleUndo {
                item_id: ItemId(2),
                title: "Heat (1995)".to_owned(),
                reason: "Undo \"Renamed 2 titles\" first.".to_owned(),
            }],
            state: UndoState::Partial,
            undone_at: None,
        };
        let dto = UndoReportDto::from(report);
        assert_eq!(dto.entry_id, "4");
        assert_eq!(dto.undone, ["1"]);
        assert_eq!(dto.left[0].id, "2");
        assert_eq!(dto.undo_state, "partial");
    }

    #[test]
    fn a_delete_report_names_where_each_title_went_and_its_entry() {
        let report = DeleteReport {
            deleted: vec![cv_app::library_ops::DeletedTitle {
                id: ItemId(7),
                title: "Godzilla".to_owned(),
                method: DeleteMethod::DataSourceTrash,
                paths: 1,
                notes: Vec::new(),
            }],
            problems: Vec::new(),
            batch_id: None,
            history_id: Some(3),
        };
        let dto = DeleteTitlesDto::from(report);
        assert_eq!(dto.deleted[0].went_to, "trash_folder");
        assert_eq!(dto.entry_id.as_deref(), Some("3"));
    }
}
