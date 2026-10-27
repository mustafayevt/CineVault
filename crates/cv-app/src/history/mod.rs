//! History: one lasting entry per batch that changed the user's files or titles (spec
//! `operation-history`).
//!
//! Every completed rename batch and move batch writes its entry as soon as the batch has run
//! (`organize` and `library_ops::change_source` call [`record`]); deletes and permanent deletes
//! write theirs through `RemovalService`. The journal stays the source of truth for the file
//! steps of a rename or move; the entry names the batch (`batch_ref`), the titles with their old
//! and new names, and a JSON detail with what the page shows and what Undo needs.
//!
//! [`HistoryService`] lists the entries for the History page, sums what is still in the
//! Recycle Bin / Trash, and takes entries back (see its `undo`).

mod service;
pub(crate) mod snapshot;
#[cfg(test)]
mod tests;
mod undo;

use cv_db::repo::history::{
    NewHistoryEntry, NewHistoryTitle, entry_by_batch, extend_entry, insert_entry,
};
use cv_db::{Database, DbError};
use cv_domain::ItemId;
use serde_json::{Value, json};

pub use cv_db::repo::history::{HistoryKind, TitleState, UndoState};
pub(crate) use service::binned_of as binned_items;
pub use service::{HistoryError, HistoryService};
pub use undo::{TitleUndo, UndoPlan, UndoReport};

/// The filter tabs of the History page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HistoryFilter {
    #[default]
    All,
    Renames,
    Moves,
    /// Deletes to the bin and permanent ones.
    Deletes,
    ArtworkAndMatches,
}

impl HistoryFilter {
    /// The kinds the tab lists; empty for every kind.
    #[must_use]
    pub const fn kinds(self) -> &'static [HistoryKind] {
        match self {
            Self::All => &[],
            Self::Renames => &[HistoryKind::Rename],
            Self::Moves => &[HistoryKind::Move],
            Self::Deletes => &[HistoryKind::Delete, HistoryKind::DeletePermanent],
            Self::ArtworkAndMatches => &[HistoryKind::Artwork, HistoryKind::Match],
        }
    }
}

/// One title of an entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryTitle {
    pub item_id: ItemId,
    pub title: String,
    /// The old name or path.
    pub before: String,
    /// The new name or path.
    pub after: String,
    pub state: TitleState,
}

/// One entry as the History page lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    pub id: i64,
    pub kind: HistoryKind,
    /// Epoch milliseconds.
    pub at: i64,
    /// "Renamed 12 titles".
    pub summary: String,
    /// The muted line under it ("12 folders, 41 files · D:\Movies"); sizes are in `bytes`.
    pub detail: String,
    /// The bytes the entry moved or deleted, when it says.
    pub bytes: Option<u64>,
    pub batch_ref: Option<String>,
    pub undone_at: Option<i64>,
    pub undo_state: UndoState,
    /// Why Undo is off for an entry that had one: its files were erased or emptied from the
    /// trash; `None` otherwise.
    pub undo_note: Option<String>,
    pub titles_total: u32,
    pub titles_undone: u32,
    /// The first titles; [`HistoryService::titles`] lists all of them.
    pub preview: Vec<HistoryTitle>,
}

/// A page of entries, newest first.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HistoryPage {
    pub entries: Vec<HistoryEntry>,
    /// Entries of the filter in all.
    pub total: u64,
}

/// How many titles an entry shows before "Show N more".
pub const PREVIEW_TITLES: usize = 5;

/// A title of a new entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTitle {
    pub item_id: ItemId,
    pub title: String,
    pub before: String,
    pub after: String,
}

/// A new entry.
#[derive(Debug, Clone, PartialEq)]
pub struct NewEntry {
    pub kind: HistoryKind,
    pub summary: String,
    /// The muted line of the page.
    pub line: String,
    pub bytes: Option<u64>,
    /// More detail Undo needs (merged into the stored JSON next to `line` and `bytes`).
    pub extra: Value,
    pub batch_ref: Option<String>,
    pub titles: Vec<NewTitle>,
}

impl NewEntry {
    pub(crate) fn detail_json(&self) -> String {
        let mut detail = match &self.extra {
            Value::Object(map) => Value::Object(map.clone()),
            _ => json!({}),
        };
        detail["line"] = json!(self.line);
        if let Some(bytes) = self.bytes {
            detail["bytes"] = json!(bytes);
        }
        detail.to_string()
    }
}

/// Writes an entry in one transaction and returns its id.
///
/// # Errors
/// The library cannot be written.
pub fn record(db: &Database, entry: &NewEntry) -> Result<i64, DbError> {
    let entry = entry.clone();
    db.write(move |tx| record_in(tx, &entry, crate::library_ops::now_millis()))
}

/// Writes the entry of a journal batch (`entry.batch_ref`) that a Resume finished or that has no
/// entry after a crash. When the batch has an entry already, only the titles it does not list
/// yet are added (with the new summary and detail); titles it lists keep their state. Returns
/// the entry's id.
///
/// # Errors
/// The library cannot be written.
pub(crate) fn record_batch(db: &Database, entry: &NewEntry) -> Result<i64, DbError> {
    let Some(batch) = entry.batch_ref.clone() else {
        return record(db, entry);
    };
    let entry = entry.clone();
    db.write(move |tx| match entry_by_batch(tx, &batch)? {
        Some(id) => {
            extend_entry(
                tx,
                id,
                &entry.summary,
                &entry.detail_json(),
                &new_titles(&entry),
            )?;
            Ok(id)
        }
        None => record_in(tx, &entry, crate::library_ops::now_millis()),
    })
}

fn new_titles(entry: &NewEntry) -> Vec<NewHistoryTitle> {
    entry
        .titles
        .iter()
        .map(|t| NewHistoryTitle {
            item_id: t.item_id.0,
            title: t.title.clone(),
            before: t.before.clone(),
            after: t.after.clone(),
            state: TitleState::Done,
        })
        .collect()
}

/// Writes an entry inside a transaction the caller holds.
///
/// # Errors
/// The library cannot be written.
pub fn record_in(conn: &rusqlite::Connection, entry: &NewEntry, at: i64) -> Result<i64, DbError> {
    let undo_state = if entry.kind == HistoryKind::DeletePermanent {
        UndoState::None
    } else {
        UndoState::Available
    };
    let titles = new_titles(entry);
    insert_entry(
        conn,
        &NewHistoryEntry {
            kind: entry.kind,
            at,
            summary: entry.summary.clone(),
            detail_json: entry.detail_json(),
            batch_ref: entry.batch_ref.clone(),
            undo_state,
        },
        &titles,
    )
}

/// "1 title" / "12 titles".
pub(crate) fn titles_word(n: usize) -> String {
    if n == 1 {
        "1 title".to_owned()
    } else {
        format!("{n} titles")
    }
}

/// The short name of a place for a summary: the drive of a Windows path (`D:`), the mount point
/// of a Linux drive (`/media/alex/Films`), else the path.
pub(crate) fn place_name(path: &str) -> String {
    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        return path[..2].to_uppercase();
    }
    if cfg!(target_os = "linux") {
        // A folder on the system drive has no mount point of its own and keeps its path.
        let mount = cv_fs::paths::linux_mount_root(path);
        if mount != "/" {
            return mount;
        }
    }
    path.trim_end_matches(['\\', '/']).to_owned()
}

/// The last part of a path (a folder or file name), or the path when it has none.
pub(crate) fn last_part(path: &str) -> String {
    path.trim_end_matches(['\\', '/'])
        .rsplit(['\\', '/'])
        .next()
        .filter(|part| !part.is_empty())
        .unwrap_or(path)
        .to_owned()
}
