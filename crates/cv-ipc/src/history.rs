//! The History page read commands and their DTOs: a page of entries, the titles of one entry and
//! the entries that touched a title.
//!
//! The rules live in `cv_app::history`; this module converts to the DTOs the UI sees and reads one
//! page. The History service itself is built by the surface that holds the app's state, because it
//! carries the surface's trash destination and change hooks.

use std::sync::{Arc, OnceLock};

use cv_app::history::{
    HistoryEntry, HistoryError, HistoryFilter, HistoryKind, HistoryPage, HistoryService,
    HistoryTitle, TitleState, UndoReport, UndoState,
};
use cv_app::library_ops::{
    DeleteMethod, DeleteReport, EraseReport, ErasedTitle, LibraryOpsError, Problem, RemovalService,
};
use cv_app::organize::OrganizeService;
use cv_artwork::library_source::LibrarySources;
use cv_db::repo::rename_journal::PathObserver;
use cv_domain::ItemId;

use crate::artwork::ArtworkState;
use crate::matching::MatchState;
use crate::sources::LibraryState;
use serde::{Deserialize, Serialize};
use specta::Type;

/// Why a command did not run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct HistoryCommandError {
    /// `storage`, `bad_id`, `unknown_entry`, `refused`, `confirmation`, or another refused rule.
    pub code: String,
    /// Plain sentence for the user.
    pub message: String,
}

/// A History failure in the command shape.
pub fn storage(message: &str) -> HistoryCommandError {
    HistoryCommandError {
        code: "storage".to_owned(),
        message: message.to_owned(),
    }
}

impl From<HistoryError> for HistoryCommandError {
    fn from(error: HistoryError) -> Self {
        Self {
            code: error.code().to_owned(),
            message: error.to_string(),
        }
    }
}

impl From<LibraryOpsError> for HistoryCommandError {
    fn from(error: LibraryOpsError) -> Self {
        Self {
            code: error.code().to_owned(),
            message: error.to_string(),
        }
    }
}

/// Runs blocking History work off the async workers and turns a join failure into a storage error.
pub async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, HistoryCommandError> + Send + 'static,
) -> Result<T, HistoryCommandError> {
    cv_jobs::client::spawn_blocking(work)
        .await
        .map_err(|error| HistoryCommandError {
            code: "storage".to_owned(),
            message: format!("The operation stopped unexpectedly: {error}"),
        })?
}

/// The numeric id behind a History entry's text id.
pub fn id_of(text: &str) -> Result<i64, HistoryCommandError> {
    text.trim().parse::<i64>().map_err(|_| HistoryCommandError {
        code: "bad_id".to_owned(),
        message: format!("\"{text}\" is not a known id."),
    })
}

/// The library items behind a list of text ids.
pub fn ids_of(ids: &[String]) -> Result<Vec<ItemId>, HistoryCommandError> {
    ids.iter().map(|id| id_of(id).map(ItemId)).collect()
}

#[allow(clippy::cast_precision_loss)]
/// An id or a timestamp as the number the UI reads.
pub fn number(value: i64) -> f64 {
    value as f64
}

#[allow(clippy::cast_precision_loss)]
/// A byte count as the number the UI reads.
pub fn bytes_of(bytes: u64) -> f64 {
    bytes as f64
}

/// One title of an entry.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct HistoryTitleDto {
    pub id: String,
    pub title: String,
    /// The old name or path.
    pub before: String,
    /// The new name or path.
    pub after: String,
    /// `done`, `undone`, or `left` (a permanent delete stopped before it).
    pub state: String,
}

impl From<HistoryTitle> for HistoryTitleDto {
    fn from(title: HistoryTitle) -> Self {
        Self {
            id: title.item_id.0.to_string(),
            title: title.title,
            before: title.before,
            after: title.after,
            state: match title.state {
                TitleState::Done => "done",
                TitleState::Undone => "undone",
                TitleState::Left => "left",
            }
            .to_owned(),
        }
    }
}

/// One History entry.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct HistoryEntryDto {
    pub id: String,
    /// `rename`, `move`, `delete`, `delete_permanent`, `artwork` or `match`.
    pub kind: String,
    /// Epoch milliseconds.
    pub at: f64,
    /// "Renamed 12 titles".
    pub summary: String,
    /// The muted line under it; sizes are in `bytes`.
    pub detail: String,
    pub bytes: Option<f64>,
    /// The journal batch of a rename or move.
    pub batch_id: Option<String>,
    /// When the entry was wholly undone (epoch milliseconds).
    pub undone_at: Option<f64>,
    /// `available`, `partial`, `undone` or `none` (no Undo: a permanent delete).
    pub undo_state: String,
    /// Why Undo is off for an entry that had one (its files were erased, or emptied from the
    /// trash); `null` otherwise.
    pub undo_note: Option<String>,
    pub titles_total: u32,
    pub titles_undone: u32,
    /// The first titles; `history_titles` lists all.
    pub preview: Vec<HistoryTitleDto>,
}

/// The wire word of a History kind.
pub fn kind_text(kind: HistoryKind) -> &'static str {
    kind.as_str()
}

/// The wire word of an entry's Undo state.
pub fn state_text(state: UndoState) -> &'static str {
    match state {
        UndoState::Available => "available",
        UndoState::Partial => "partial",
        UndoState::Undone => "undone",
        UndoState::None => "none",
    }
}

impl From<HistoryEntry> for HistoryEntryDto {
    fn from(entry: HistoryEntry) -> Self {
        Self {
            id: entry.id.to_string(),
            kind: kind_text(entry.kind).to_owned(),
            at: number(entry.at),
            summary: entry.summary,
            detail: entry.detail,
            bytes: entry.bytes.map(bytes_of),
            batch_id: entry.batch_ref,
            undone_at: entry.undone_at.map(number),
            undo_state: state_text(entry.undo_state).to_owned(),
            undo_note: entry.undo_note,
            titles_total: entry.titles_total,
            titles_undone: entry.titles_undone,
            preview: entry.preview.into_iter().map(Into::into).collect(),
        }
    }
}

/// A page of History, newest first.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct HistoryPageDto {
    pub entries: Vec<HistoryEntryDto>,
    /// Entries of the filter in all.
    pub total: f64,
}

impl From<HistoryPage> for HistoryPageDto {
    fn from(page: HistoryPage) -> Self {
        Self {
            entries: page.entries.into_iter().map(Into::into).collect(),
            total: bytes_of(page.total),
        }
    }
}

/// The filter tabs of the History page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum HistoryFilterDto {
    All,
    Renames,
    Moves,
    Deletes,
    ArtworkAndMatches,
}

impl From<HistoryFilterDto> for HistoryFilter {
    fn from(filter: HistoryFilterDto) -> Self {
        match filter {
            HistoryFilterDto::All => Self::All,
            HistoryFilterDto::Renames => Self::Renames,
            HistoryFilterDto::Moves => Self::Moves,
            HistoryFilterDto::Deletes => Self::Deletes,
            HistoryFilterDto::ArtworkAndMatches => Self::ArtworkAndMatches,
        }
    }
}

/// One page of History entries of a tab, newest first (`limit` at most 200).
pub async fn history_list(
    service: Arc<HistoryService>,
    filter: HistoryFilterDto,
    offset: u32,
    limit: u32,
) -> Result<HistoryPageDto, HistoryCommandError> {
    let (offset, limit) = (offset as usize, limit.clamp(1, 200) as usize);
    blocking(move || Ok(service.list(filter.into(), offset, limit)?.into())).await
}

/// Every title of an entry ("Show N more").
pub async fn history_titles(
    service: Arc<HistoryService>,
    entry_id: String,
) -> Result<Vec<HistoryTitleDto>, HistoryCommandError> {
    let entry = id_of(&entry_id)?;
    blocking(move || Ok(service.titles(entry)?.into_iter().map(Into::into).collect())).await
}

/// The newest entries that touched a title, with that title's line only (the "Renamed from …"
/// line of a title page reads the newest rename).
pub async fn history_for_title(
    service: Arc<HistoryService>,
    id: String,
) -> Result<Vec<HistoryEntryDto>, HistoryCommandError> {
    let item = ItemId(id_of(&id)?);
    blocking(move || {
        Ok(service
            .for_title(item, 20)?
            .into_iter()
            .map(Into::into)
            .collect())
    })
    .await
}

/// The History entry of a rename or move batch, so its toast's Undo points at the same record.
pub async fn history_entry_for_batch(
    service: Arc<HistoryService>,
    batch_id: String,
) -> Result<Option<String>, HistoryCommandError> {
    blocking(move || Ok(service.entry_for_batch(&batch_id)?.map(|id| id.to_string()))).await
}

/// How much of what CineVault sent to the Recycle Bin / Trash is still there, in bytes.
pub async fn history_bin_total(service: Arc<HistoryService>) -> Result<f64, HistoryCommandError> {
    blocking(move || Ok(bytes_of(service.bin_total()?))).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_must_be_numbers() {
        assert_eq!(id_of(" 12 ").unwrap(), 12);
        assert_eq!(id_of("x").unwrap_err().code, "bad_id");
    }
}

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

    pub fn history(
        &self,
        library: &LibraryState,
    ) -> Result<Arc<HistoryService>, HistoryCommandError> {
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

    pub fn removal(
        &self,
        library: &LibraryState,
    ) -> Result<Arc<RemovalService>, HistoryCommandError> {
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
pub async fn history_undo(
    library: &LibraryState,
    history: &HistoryState,
    artwork: &ArtworkState,
    matching: &MatchState,
    entry_id: String,
) -> Result<UndoReportDto, HistoryCommandError> {
    let entry = id_of(&entry_id)?;
    let report = undo_entry(history, library, artwork, matching, entry, None).await?;
    titles_changed(library, &report.undone);
    Ok(report.into())
}

/// Takes back one title's part of an entry (the Undo of "Renamed from …" on a title page).
pub async fn history_undo_title(
    library: &LibraryState,
    history: &HistoryState,
    artwork: &ArtworkState,
    matching: &MatchState,
    entry_id: String,
    id: String,
) -> Result<UndoReportDto, HistoryCommandError> {
    let entry = id_of(&entry_id)?;
    let item = ItemId(id_of(&id)?);
    let report = undo_entry(history, library, artwork, matching, entry, Some(item)).await?;
    titles_changed(library, &report.undone);
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
pub async fn delete_titles(
    library: &LibraryState,
    history: &HistoryState,
    ids: Vec<String>,
) -> Result<DeleteTitlesDto, HistoryCommandError> {
    let service = history.removal(library)?;
    let ids = ids_of(&ids)?;
    let report = blocking(move || Ok(service.delete_with_files(&ids)?)).await?;
    let gone: Vec<ItemId> = report.deleted.iter().map(|t| t.id).collect();
    titles_changed(library, &gone);
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
pub async fn erase_titles(
    library: &LibraryState,
    history: &HistoryState,
    ids: Vec<String>,
    confirm: String,
) -> Result<EraseTitlesDto, HistoryCommandError> {
    let service = history.removal(library)?;
    let ids = ids_of(&ids)?;
    let report = blocking(move || Ok(service.erase(&ids, &confirm)?)).await?;
    let touched: Vec<ItemId> = report
        .erased
        .iter()
        .chain(&report.stopped)
        .map(|t| t.id)
        .collect();
    titles_changed(library, &touched);
    Ok(report.into())
}

#[cfg(test)]
mod write_tests {
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
