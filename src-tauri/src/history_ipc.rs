//! The Tauri surface of History, Delete to the Recycle Bin and permanent delete.
//!
//! The state, the DTOs and the command bodies live in [`cv_ipc::history`]. This module keeps the
//! typed `HistoryChanged` event and the thin wrappers.

use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, State};
use tauri_specta::Event;

use crate::artwork_ipc::ArtworkState;
use crate::match_ipc::MatchState;
use crate::sources_ipc::LibraryState;

pub use cv_ipc::history::*;

/// History changed: an entry was written or undone. `entry_id` names it when one did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type, Event)]
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
    cv_ipc::history::history_list(history.history(&library)?, filter, offset, limit).await
}

/// Every title of an entry ("Show N more").
#[tauri::command]
#[specta::specta]
pub async fn history_titles(
    library: State<'_, LibraryState>,
    history: State<'_, HistoryState>,
    entry_id: String,
) -> Result<Vec<HistoryTitleDto>, HistoryCommandError> {
    cv_ipc::history::history_titles(history.history(&library)?, entry_id).await
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
    cv_ipc::history::history_for_title(history.history(&library)?, id).await
}

/// The History entry of a rename or move batch, so its toast's Undo points at the same record.
#[tauri::command]
#[specta::specta]
pub async fn history_entry_for_batch(
    library: State<'_, LibraryState>,
    history: State<'_, HistoryState>,
    batch_id: String,
) -> Result<Option<String>, HistoryCommandError> {
    cv_ipc::history::history_entry_for_batch(history.history(&library)?, batch_id).await
}

/// How much of what CineVault sent to the Recycle Bin / Trash is still there, in bytes.
#[tauri::command]
#[specta::specta]
pub async fn history_bin_total(
    library: State<'_, LibraryState>,
    history: State<'_, HistoryState>,
) -> Result<f64, HistoryCommandError> {
    cv_ipc::history::history_bin_total(history.history(&library)?).await
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
    let report =
        cv_ipc::history::history_undo(&library, &history, &artwork, &matching, entry_id).await?;
    announce(&app, report.entry_id.parse().ok());
    Ok(report)
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
    let report =
        cv_ipc::history::history_undo_title(&library, &history, &artwork, &matching, entry_id, id)
            .await?;
    announce(&app, report.entry_id.parse().ok());
    Ok(report)
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
    let report = cv_ipc::history::delete_titles(&library, &history, ids).await?;
    if let Some(id) = report
        .entry_id
        .as_deref()
        .and_then(|text| text.parse().ok())
    {
        announce(&app, Some(id));
    }
    Ok(report)
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
    let report = cv_ipc::history::erase_titles(&library, &history, ids, confirm).await?;
    if let Some(id) = report
        .entry_id
        .as_deref()
        .and_then(|text| text.parse().ok())
    {
        announce(&app, Some(id));
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_event_names_the_entry() {
        let event = super::HistoryChanged {
            entry_id: Some("4".to_owned()),
        };
        assert_eq!(event.entry_id.as_deref(), Some("4"));
    }
}
