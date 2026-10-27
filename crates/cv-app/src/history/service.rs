//! The History service: the page's list, the bin total and Undo.

use std::sync::Arc;

use cv_db::repo::history::{
    HistoryEntryRow, HistoryTitleRow, count_entries, entries_for_item, entry_by_batch,
    entry_titles, get_entry, list_entries, open_details,
};
use cv_db::{Database, DbError};
use cv_domain::ItemId;
use serde_json::Value;

use super::{
    HistoryEntry, HistoryFilter, HistoryKind, HistoryPage, HistoryTitle, NewEntry, PREVIEW_TITLES,
    TitleState,
};
use crate::library_ops::{BinnedItem, OsTrash, SystemBin, present_all};

/// Why a History call did not run.
#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
    #[error("the library could not be read or written: {0}")]
    Db(#[from] DbError),
    #[error("this History entry no longer exists")]
    UnknownEntry(i64),
    /// The request breaks a rule; the message says which.
    #[error("{0}")]
    Refused(String),
}

impl HistoryError {
    /// A stable code for callers to branch on.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Db(_) => "storage",
            Self::UnknownEntry(_) => "unknown_entry",
            Self::Refused(_) => "refused",
        }
    }
}

/// History over one library.
#[derive(Clone)]
pub struct HistoryService {
    pub(super) db: Arc<Database>,
    pub(super) bin: Arc<dyn SystemBin>,
    pub(super) changed: Option<crate::organize::ChangedHook>,
    /// Takes rename and move batches back; built over the library when not given.
    pub(super) organize: Option<crate::organize::OrganizeService>,
}

impl HistoryService {
    /// A service over the real system bin.
    #[must_use]
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            db,
            bin: Arc::new(OsTrash),
            changed: None,
            organize: None,
        }
    }

    /// Replaces the system bin (tests use a folder).
    #[must_use]
    pub fn with_bin(mut self, bin: Arc<dyn SystemBin>) -> Self {
        self.bin = bin;
        self
    }

    /// Tells `hook` which titles an Undo changed, so the pages that show them follow.
    #[must_use]
    pub fn with_changed_hook(mut self, hook: crate::organize::ChangedHook) -> Self {
        self.changed = Some(hook);
        self
    }

    /// The Organize service whose undo takes rename and move batches back (the app's, with its
    /// disk, path observer and changed hook).
    #[must_use]
    pub fn with_organize(mut self, organize: crate::organize::OrganizeService) -> Self {
        self.organize = Some(organize);
        self
    }

    /// The library this service works on.
    #[must_use]
    pub fn database(&self) -> &Arc<Database> {
        &self.db
    }

    /// Writes an entry and returns its id.
    ///
    /// # Errors
    /// The library cannot be written.
    pub fn record(&self, entry: &NewEntry) -> Result<i64, HistoryError> {
        Ok(super::record(&self.db, entry)?)
    }

    /// One page of entries of `filter`, newest first.
    ///
    /// # Errors
    /// The library cannot be read.
    pub fn list(
        &self,
        filter: HistoryFilter,
        offset: usize,
        limit: usize,
    ) -> Result<HistoryPage, HistoryError> {
        let kinds = filter.kinds();
        Ok(self.db.read(move |conn| {
            let total = count_entries(conn, kinds)?;
            let mut entries = Vec::new();
            for row in list_entries(conn, kinds, offset, limit)? {
                let preview = entry_titles(conn, row.id)?
                    .into_iter()
                    .take(PREVIEW_TITLES)
                    .map(title_of)
                    .collect();
                entries.push(entry_of(row, preview));
            }
            Ok(HistoryPage { entries, total })
        })?)
    }

    /// One entry with its first titles.
    ///
    /// # Errors
    /// The entry is unknown or the library cannot be read.
    pub fn entry(&self, id: i64) -> Result<HistoryEntry, HistoryError> {
        let found = self.db.read(move |conn| {
            let Some(row) = get_entry(conn, id)? else {
                return Ok(None);
            };
            let preview = entry_titles(conn, id)?
                .into_iter()
                .take(PREVIEW_TITLES)
                .map(title_of)
                .collect();
            Ok(Some(entry_of(row, preview)))
        })?;
        found.ok_or(HistoryError::UnknownEntry(id))
    }

    /// Every title of an entry ("Show N more").
    ///
    /// # Errors
    /// The library cannot be read.
    pub fn titles(&self, entry_id: i64) -> Result<Vec<HistoryTitle>, HistoryError> {
        Ok(self
            .db
            .read(move |conn| entry_titles(conn, entry_id))?
            .into_iter()
            .map(title_of)
            .collect())
    }

    /// The entry that records journal batch `batch` (the toast of a rename or move points at it).
    ///
    /// # Errors
    /// The library cannot be read.
    pub fn entry_for_batch(&self, batch: &str) -> Result<Option<i64>, HistoryError> {
        let batch = batch.to_owned();
        Ok(self.db.read(move |conn| entry_by_batch(conn, &batch))?)
    }

    /// The newest entries that touched a title (the "Renamed from" line of its page).
    ///
    /// # Errors
    /// The library cannot be read.
    pub fn for_title(&self, id: ItemId, limit: usize) -> Result<Vec<HistoryEntry>, HistoryError> {
        Ok(self.db.read(move |conn| {
            let mut out = Vec::new();
            for row in entries_for_item(conn, id.0, limit)? {
                let titles = entry_titles(conn, row.id)?
                    .into_iter()
                    .filter(|t| t.item_id == id.0)
                    .map(title_of)
                    .collect();
                out.push(entry_of(row, titles));
            }
            Ok(out)
        })?)
    }

    /// The bytes CineVault sent to the Recycle Bin / Trash that are still there: every binned
    /// item of a delete or move that is not undone, counted when the bin still holds it.
    ///
    /// # Errors
    /// The library cannot be read.
    pub fn bin_total(&self) -> Result<u64, HistoryError> {
        let mut items: Vec<BinnedItem> = Vec::new();
        for kind in [HistoryKind::Delete, HistoryKind::Move] {
            let details = self.db.read(move |conn| {
                let mut out = Vec::new();
                for (id, detail) in open_details(conn, kind)? {
                    let undone: Vec<i64> = entry_titles(conn, id)?
                        .into_iter()
                        .filter(|t| t.state == TitleState::Undone)
                        .map(|t| t.item_id)
                        .collect();
                    out.push((detail, undone));
                }
                Ok(out)
            })?;
            for (detail, undone) in details {
                items.extend(
                    binned_of(&detail)
                        .into_iter()
                        .filter(|(item, _)| !undone.contains(item))
                        .flat_map(|(_, list)| list),
                );
            }
        }
        if items.is_empty() {
            return Ok(0);
        }
        Ok(items
            .iter()
            .zip(present_all(self.bin.as_ref(), &items))
            .filter(|(_, here)| *here)
            .map(|(item, _)| item.size)
            .sum())
    }
}

/// The binned items of an entry's detail, per title: `{"binned": [{"item_id": 1, "items": [..]}]}`.
pub(crate) fn binned_of(detail: &str) -> Vec<(i64, Vec<BinnedItem>)> {
    let Ok(value) = serde_json::from_str::<Value>(detail) else {
        return Vec::new();
    };
    value
        .get("binned")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|entry| {
                    let id = entry.get("item_id")?.as_i64()?;
                    let items = serde_json::from_value(entry.get("items")?.clone()).ok()?;
                    Some((id, items))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn title_of(row: HistoryTitleRow) -> HistoryTitle {
    HistoryTitle {
        item_id: ItemId(row.item_id),
        title: row.title,
        before: row.before,
        after: row.after,
        state: row.state,
    }
}

fn entry_of(row: HistoryEntryRow, preview: Vec<HistoryTitle>) -> HistoryEntry {
    let detail: Value = serde_json::from_str(&row.detail_json).unwrap_or(Value::Null);
    HistoryEntry {
        id: row.id,
        kind: row.kind,
        at: row.at,
        summary: row.summary,
        detail: detail
            .get("line")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        bytes: detail.get("bytes").and_then(Value::as_u64),
        batch_ref: row.batch_ref,
        undone_at: row.undone_at,
        undo_state: row.undo_state,
        undo_note: detail
            .get("undo_off")
            .and_then(Value::as_str)
            .map(str::to_owned),
        titles_total: row.titles,
        titles_undone: row.titles_undone,
        preview,
    }
}
