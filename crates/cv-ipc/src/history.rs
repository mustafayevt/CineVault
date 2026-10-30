//! The History page read commands and their DTOs: a page of entries, the titles of one entry and
//! the entries that touched a title.
//!
//! The rules live in `cv_app::history`; this module converts to the DTOs the UI sees and reads one
//! page. The History service itself is built by the surface that holds the app's state, because it
//! carries the surface's trash destination and change hooks.

use std::sync::Arc;

use cv_app::history::{
    HistoryEntry, HistoryError, HistoryFilter, HistoryKind, HistoryPage, HistoryService,
    HistoryTitle, TitleState, UndoState,
};
use cv_app::library_ops::LibraryOpsError;
use cv_domain::ItemId;
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
    tokio::task::spawn_blocking(work)
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
