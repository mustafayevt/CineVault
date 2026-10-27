//! History: the lasting record of every batch that changed the user's files or titles
//! (spec `operation-history`).
//!
//! An entry ([`HistoryEntryRow`]) has a kind, a time, a one-line summary, a JSON detail with
//! what the page shows and what Undo needs, and the journal batch of a rename or move. The titles
//! it touched are [`HistoryTitleRow`]s with their old and new names. Nothing here removes an
//! entry: History is never trimmed automatically.

use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter};

use crate::error::{DbError, DbResult};

/// What an entry records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum HistoryKind {
    Rename,
    Move,
    /// Sent to the Recycle Bin / Trash (or a library folder's trash folder).
    Delete,
    /// Erased for good after the typed confirmation; never undoable.
    DeletePermanent,
    Artwork,
    Match,
}

impl HistoryKind {
    /// Every kind, in the order the page's filters name them.
    pub const ALL: [Self; 6] = [
        Self::Rename,
        Self::Move,
        Self::Delete,
        Self::DeletePermanent,
        Self::Artwork,
        Self::Match,
    ];

    /// The text stored in the table.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Rename => "rename",
            Self::Move => "move",
            Self::Delete => "delete",
            Self::DeletePermanent => "delete_permanent",
            Self::Artwork => "artwork",
            Self::Match => "match",
        }
    }

    /// The kind stored as `text`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == text)
    }
}

/// Whether an entry can still be undone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UndoState {
    /// Nothing of it has been undone.
    Available,
    /// Some of its titles were undone, one by one.
    Partial,
    /// Undone; `undone_at` says when.
    Undone,
    /// It cannot be undone (a permanent delete).
    None,
}

impl UndoState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Partial => "partial",
            Self::Undone => "undone",
            Self::None => "none",
        }
    }

    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        [Self::Available, Self::Partial, Self::Undone, Self::None]
            .into_iter()
            .find(|state| state.as_str() == text)
    }
}

/// What became of one title of an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TitleState {
    /// The change was made.
    Done,
    /// The change was taken back.
    Undone,
    /// The change was not made for this title (a permanent delete stopped at a file in use).
    Left,
}

impl TitleState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Done => "done",
            Self::Undone => "undone",
            Self::Left => "left",
        }
    }

    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        [Self::Done, Self::Undone, Self::Left]
            .into_iter()
            .find(|state| state.as_str() == text)
    }
}

/// A new entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewHistoryEntry {
    pub kind: HistoryKind,
    /// Epoch milliseconds.
    pub at: i64,
    pub summary: String,
    /// Valid JSON; `{}` when there is nothing more to say.
    pub detail_json: String,
    pub batch_ref: Option<String>,
    pub undo_state: UndoState,
}

/// A title of a new entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewHistoryTitle {
    pub item_id: i64,
    pub title: String,
    pub before: String,
    pub after: String,
    pub state: TitleState,
}

/// A stored entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntryRow {
    pub id: i64,
    pub kind: HistoryKind,
    pub at: i64,
    pub summary: String,
    pub detail_json: String,
    pub batch_ref: Option<String>,
    pub undone_at: Option<i64>,
    pub undo_state: UndoState,
    /// How many titles the entry touched.
    pub titles: u32,
    /// How many of them were undone.
    pub titles_undone: u32,
}

/// A stored title of an entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryTitleRow {
    pub entry_id: i64,
    pub item_id: i64,
    pub title: String,
    pub before: String,
    pub after: String,
    pub state: TitleState,
}

/// A later entry that changed a title again: an older entry's Undo leaves that title alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewerEntry {
    pub item_id: i64,
    pub entry_id: i64,
    pub summary: String,
}

fn bad(what: &str, text: &str) -> DbError {
    DbError::InvalidData(format!("unknown history {what} `{text}`"))
}

const ENTRY_COLUMNS: &str = "e.id, e.kind, e.at, e.summary, e.detail_json, e.batch_ref, \
     e.undone_at, e.undo_state, \
     (SELECT COUNT(*) FROM history_title t WHERE t.entry_id = e.id), \
     (SELECT COUNT(*) FROM history_title t WHERE t.entry_id = e.id AND t.state = 'undone')";

fn entry_row(row: &Row<'_>) -> rusqlite::Result<(HistoryEntryRow, String, String)> {
    let kind: String = row.get(1)?;
    let state: String = row.get(7)?;
    Ok((
        HistoryEntryRow {
            id: row.get(0)?,
            kind: HistoryKind::Rename,
            at: row.get(2)?,
            summary: row.get(3)?,
            detail_json: row.get(4)?,
            batch_ref: row.get(5)?,
            undone_at: row.get(6)?,
            undo_state: UndoState::Available,
            titles: u32::try_from(row.get::<_, i64>(8)?).unwrap_or(0),
            titles_undone: u32::try_from(row.get::<_, i64>(9)?).unwrap_or(0),
        },
        kind,
        state,
    ))
}

fn finish(raw: (HistoryEntryRow, String, String)) -> DbResult<HistoryEntryRow> {
    let (mut entry, kind, state) = raw;
    entry.kind = HistoryKind::parse(&kind).ok_or_else(|| bad("kind", &kind))?;
    entry.undo_state = UndoState::parse(&state).ok_or_else(|| bad("undo state", &state))?;
    Ok(entry)
}

/// Writes an entry and its titles; returns the entry's id. Run it in the transaction of the
/// change it records.
///
/// # Errors
/// The library cannot be written, or `detail_json` is not valid JSON.
pub fn insert_entry(
    conn: &Connection,
    entry: &NewHistoryEntry,
    titles: &[NewHistoryTitle],
) -> DbResult<i64> {
    conn.prepare_cached(
        "INSERT INTO history_entry (kind, at, summary, detail_json, batch_ref, undo_state)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?
    .execute(params![
        entry.kind.as_str(),
        entry.at,
        entry.summary,
        entry.detail_json,
        entry.batch_ref,
        entry.undo_state.as_str(),
    ])?;
    let id = conn.last_insert_rowid();
    let mut stmt = conn.prepare_cached(
        "INSERT OR REPLACE INTO history_title (entry_id, item_id, title, before, after, state)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?;
    for title in titles {
        stmt.execute(params![
            id,
            title.item_id,
            title.title,
            title.before,
            title.after,
            title.state.as_str(),
        ])?;
    }
    Ok(id)
}

/// Entries of the given kinds (every kind when empty), newest first, `limit` of them from
/// `offset` on.
///
/// # Errors
/// The library cannot be read.
pub fn list_entries(
    conn: &Connection,
    kinds: &[HistoryKind],
    offset: usize,
    limit: usize,
) -> DbResult<Vec<HistoryEntryRow>> {
    let (filter, values) = kind_filter(kinds);
    let sql = format!(
        "SELECT {ENTRY_COLUMNS} FROM history_entry e {filter} \
         ORDER BY e.at DESC, e.id DESC LIMIT {} OFFSET {}",
        i64::try_from(limit.max(1)).unwrap_or(i64::MAX),
        i64::try_from(offset).unwrap_or(i64::MAX),
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map(params_from_iter(values.iter()), entry_row)?;
    rows.map(|row| finish(row?)).collect()
}

/// How many entries of the given kinds there are (every kind when empty).
///
/// # Errors
/// The library cannot be read.
pub fn count_entries(conn: &Connection, kinds: &[HistoryKind]) -> DbResult<u64> {
    let (filter, values) = kind_filter(kinds);
    let n: i64 = conn
        .prepare_cached(&format!("SELECT COUNT(*) FROM history_entry e {filter}"))?
        .query_row(params_from_iter(values.iter()), |r| r.get(0))?;
    Ok(u64::try_from(n).unwrap_or(0))
}

fn kind_filter(kinds: &[HistoryKind]) -> (String, Vec<&'static str>) {
    if kinds.is_empty() {
        return (String::new(), Vec::new());
    }
    let marks = vec!["?"; kinds.len()].join(", ");
    (
        format!("WHERE e.kind IN ({marks})"),
        kinds.iter().map(|k| k.as_str()).collect(),
    )
}

/// One entry.
///
/// # Errors
/// The library cannot be read.
pub fn get_entry(conn: &Connection, id: i64) -> DbResult<Option<HistoryEntryRow>> {
    conn.prepare_cached(&format!(
        "SELECT {ENTRY_COLUMNS} FROM history_entry e WHERE e.id = ?1"
    ))?
    .query_row([id], entry_row)
    .optional()?
    .map(finish)
    .transpose()
}

/// The entry that records journal batch `batch`.
///
/// # Errors
/// The library cannot be read.
pub fn entry_by_batch(conn: &Connection, batch: &str) -> DbResult<Option<i64>> {
    Ok(conn
        .prepare_cached("SELECT id FROM history_entry WHERE batch_ref = ?1 ORDER BY id DESC")?
        .query_row([batch], |r| r.get(0))
        .optional()?)
}

/// The titles of an entry, in the order they were written.
///
/// # Errors
/// The library cannot be read.
pub fn entry_titles(conn: &Connection, entry_id: i64) -> DbResult<Vec<HistoryTitleRow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT entry_id, item_id, title, before, after, state FROM history_title
         WHERE entry_id = ?1 ORDER BY rowid",
    )?;
    let rows = stmt.query_map([entry_id], |row| {
        Ok((
            HistoryTitleRow {
                entry_id: row.get(0)?,
                item_id: row.get(1)?,
                title: row.get(2)?,
                before: row.get(3)?,
                after: row.get(4)?,
                state: TitleState::Done,
            },
            row.get::<_, String>(5)?,
        ))
    })?;
    rows.map(|row| {
        let (mut title, state) = row?;
        title.state = TitleState::parse(&state).ok_or_else(|| bad("title state", &state))?;
        Ok(title)
    })
    .collect()
}

/// The entries that touched `item_id`, newest first.
///
/// # Errors
/// The library cannot be read.
pub fn entries_for_item(
    conn: &Connection,
    item_id: i64,
    limit: usize,
) -> DbResult<Vec<HistoryEntryRow>> {
    let sql = format!(
        "SELECT {ENTRY_COLUMNS} FROM history_entry e
         JOIN history_title h ON h.entry_id = e.id AND h.item_id = ?1
         ORDER BY e.at DESC, e.id DESC LIMIT {}",
        i64::try_from(limit.max(1)).unwrap_or(i64::MAX)
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map([item_id], entry_row)?;
    rows.map(|row| finish(row?)).collect()
}

/// The later entries (written after `entry_id`) that changed any of `item_ids` and still stand
/// for that title: not undone as a whole, and the title's part not undone either.
///
/// # Errors
/// The library cannot be read.
pub fn newer_entries_for(
    conn: &Connection,
    entry_id: i64,
    item_ids: &[i64],
) -> DbResult<Vec<NewerEntry>> {
    let mut out = Vec::new();
    let mut stmt = conn.prepare_cached(
        "SELECT h.item_id, e.id, e.summary FROM history_title h
         JOIN history_entry e ON e.id = h.entry_id
         WHERE h.item_id = ?1 AND e.id > ?2 AND e.undo_state <> 'undone' AND h.state = 'done'
         ORDER BY e.id DESC LIMIT 1",
    )?;
    for &item in item_ids {
        if let Some(found) = stmt
            .query_row(params![item, entry_id], |row| {
                Ok(NewerEntry {
                    item_id: row.get(0)?,
                    entry_id: row.get(1)?,
                    summary: row.get(2)?,
                })
            })
            .optional()?
        {
            out.push(found);
        }
    }
    Ok(out)
}

/// Sets the undo state of an entry, with the time it was (wholly) undone.
///
/// # Errors
/// The library cannot be written.
pub fn set_undo_state(
    conn: &Connection,
    entry_id: i64,
    state: UndoState,
    undone_at: Option<i64>,
) -> DbResult<()> {
    conn.prepare_cached("UPDATE history_entry SET undo_state = ?2, undone_at = ?3 WHERE id = ?1")?
        .execute(params![entry_id, state.as_str(), undone_at])?;
    Ok(())
}

/// Sets what became of one title of an entry.
///
/// # Errors
/// The library cannot be written.
pub fn set_title_state(
    conn: &Connection,
    entry_id: i64,
    item_id: i64,
    state: TitleState,
) -> DbResult<()> {
    conn.prepare_cached(
        "UPDATE history_title SET state = ?3 WHERE entry_id = ?1 AND item_id = ?2",
    )?
    .execute(params![entry_id, item_id, state.as_str()])?;
    Ok(())
}

/// Adds to an entry the titles it does not list yet (a batch that Resume finished) and, when
/// any is added, replaces its summary and detail. Titles it lists already keep their state. An
/// entry whose titles were all undone becomes partly undone when titles are added. Returns how
/// many were added.
///
/// # Errors
/// The library cannot be written, or `detail_json` is not valid JSON.
pub fn extend_entry(
    conn: &Connection,
    entry_id: i64,
    summary: &str,
    detail_json: &str,
    titles: &[NewHistoryTitle],
) -> DbResult<usize> {
    let mut added = 0;
    let mut stmt = conn.prepare_cached(
        "INSERT OR IGNORE INTO history_title (entry_id, item_id, title, before, after, state)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?;
    for title in titles {
        added += stmt.execute(params![
            entry_id,
            title.item_id,
            title.title,
            title.before,
            title.after,
            title.state.as_str(),
        ])?;
    }
    if added > 0 {
        conn.prepare_cached(
            "UPDATE history_entry SET summary = ?2, detail_json = ?3 WHERE id = ?1",
        )?
        .execute(params![entry_id, summary, detail_json])?;
        conn.prepare_cached(
            "UPDATE history_entry SET undo_state = 'partial', undone_at = NULL
             WHERE id = ?1 AND undo_state = 'undone'",
        )?
        .execute([entry_id])?;
    }
    Ok(added)
}

/// Replaces the detail of an entry (a permanent delete adds what it erased and what it left).
///
/// # Errors
/// The library cannot be written, or `detail_json` is not valid JSON.
pub fn set_detail(conn: &Connection, entry_id: i64, detail_json: &str) -> DbResult<()> {
    conn.prepare_cached("UPDATE history_entry SET detail_json = ?2 WHERE id = ?1")?
        .execute(params![entry_id, detail_json])?;
    Ok(())
}

/// The detail of every entry of `kind` that is not wholly undone, newest first.
///
/// # Errors
/// The library cannot be read.
pub fn open_details(conn: &Connection, kind: HistoryKind) -> DbResult<Vec<(i64, String)>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, detail_json FROM history_entry
         WHERE kind = ?1 AND undo_state <> 'undone' ORDER BY id DESC",
    )?;
    let rows = stmt.query_map([kind.as_str()], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Database;

    fn db() -> (tempfile::TempDir, Database) {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(dir.path().join("library.db")).unwrap();
        (dir, db)
    }

    fn entry(kind: HistoryKind, at: i64, summary: &str) -> NewHistoryEntry {
        NewHistoryEntry {
            kind,
            at,
            summary: summary.to_owned(),
            detail_json: "{}".to_owned(),
            batch_ref: None,
            undo_state: UndoState::Available,
        }
    }

    fn title(item_id: i64) -> NewHistoryTitle {
        NewHistoryTitle {
            item_id,
            title: format!("Title {item_id}"),
            before: "old".to_owned(),
            after: "new".to_owned(),
            state: TitleState::Done,
        }
    }

    #[test]
    fn entries_survive_reopening_the_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        {
            let db = Database::open(&path).unwrap();
            db.write(|tx| {
                insert_entry(
                    tx,
                    &entry(HistoryKind::Artwork, 5, "Changed poster"),
                    &[title(7)],
                )
            })
            .unwrap();
        }
        let db = Database::open(&path).unwrap();
        let listed = db.read(|c| list_entries(c, &[], 0, 10)).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].summary, "Changed poster");
        assert_eq!(listed[0].titles, 1);
        let titles = db.read(|c| entry_titles(c, listed[0].id)).unwrap();
        assert_eq!(titles[0].item_id, 7);
    }

    #[test]
    fn the_list_is_newest_first_filtered_and_paged() {
        let (_dir, db) = db();
        db.write(|tx| {
            insert_entry(tx, &entry(HistoryKind::Rename, 10, "r1"), &[])?;
            insert_entry(tx, &entry(HistoryKind::Move, 20, "m1"), &[])?;
            insert_entry(tx, &entry(HistoryKind::Delete, 30, "d1"), &[])?;
            insert_entry(tx, &entry(HistoryKind::Rename, 40, "r2"), &[])?;
            Ok(())
        })
        .unwrap();
        let all: Vec<String> = db
            .read(|c| list_entries(c, &[], 0, 10))
            .unwrap()
            .into_iter()
            .map(|e| e.summary)
            .collect();
        assert_eq!(all, ["r2", "d1", "m1", "r1"]);
        let renames = db
            .read(|c| list_entries(c, &[HistoryKind::Rename], 0, 10))
            .unwrap();
        assert_eq!(renames.len(), 2);
        let page = db.read(|c| list_entries(c, &[], 1, 2)).unwrap();
        assert_eq!(page[0].summary, "d1");
        assert_eq!(page.len(), 2);
        assert_eq!(
            db.read(|c| count_entries(c, &[HistoryKind::Delete]))
                .unwrap(),
            1
        );
    }

    #[test]
    fn newer_entries_name_the_later_change_of_a_title_until_it_is_undone() {
        let (_dir, db) = db();
        let (first, second) = db
            .write(|tx| {
                let a = insert_entry(
                    tx,
                    &entry(HistoryKind::Move, 10, "Moved"),
                    &[title(1), title(2)],
                )?;
                let b = insert_entry(tx, &entry(HistoryKind::Rename, 20, "Renamed"), &[title(1)])?;
                Ok((a, b))
            })
            .unwrap();
        let newer = db.read(|c| newer_entries_for(c, first, &[1, 2])).unwrap();
        assert_eq!(newer.len(), 1);
        assert_eq!((newer[0].item_id, newer[0].entry_id), (1, second));
        assert_eq!(newer[0].summary, "Renamed");
        db.write(move |tx| set_title_state(tx, second, 1, TitleState::Undone))
            .unwrap();
        assert!(
            db.read(|c| newer_entries_for(c, first, &[1, 2]))
                .unwrap()
                .is_empty()
        );
        let row = db.read(|c| get_entry(c, second)).unwrap().unwrap();
        assert_eq!(row.titles_undone, 1);
        db.write(move |tx| set_undo_state(tx, second, UndoState::Undone, Some(99)))
            .unwrap();
        let row = db.read(|c| get_entry(c, second)).unwrap().unwrap();
        assert_eq!(
            (row.undo_state, row.undone_at),
            (UndoState::Undone, Some(99))
        );
    }

    #[test]
    fn extending_an_entry_adds_only_new_titles_and_reopens_an_undone_one() {
        let (_dir, db) = db();
        let id = db
            .write(|tx| insert_entry(tx, &entry(HistoryKind::Move, 10, "Moved A"), &[title(1)]))
            .unwrap();
        db.write(move |tx| {
            set_title_state(tx, id, 1, TitleState::Undone)?;
            set_undo_state(tx, id, UndoState::Undone, Some(50))
        })
        .unwrap();
        let added = db
            .write(move |tx| {
                extend_entry(
                    tx,
                    id,
                    "Moved 2 titles",
                    r#"{"files":3}"#,
                    &[title(1), title(2)],
                )
            })
            .unwrap();
        assert_eq!(added, 1);
        let row = db.read(move |c| get_entry(c, id)).unwrap().unwrap();
        assert_eq!(
            (row.summary.as_str(), row.undo_state, row.undone_at),
            ("Moved 2 titles", UndoState::Partial, None)
        );
        let titles = db.read(move |c| entry_titles(c, id)).unwrap();
        assert_eq!(
            titles
                .iter()
                .map(|t| (t.item_id, t.state))
                .collect::<Vec<_>>(),
            [(1, TitleState::Undone), (2, TitleState::Done)],
            "the undone title keeps its state"
        );
        // Nothing new: the state stays.
        assert_eq!(
            db.write(move |tx| extend_entry(tx, id, "Moved 2 titles", "{}", &[title(2)]))
                .unwrap(),
            0
        );
    }

    #[test]
    fn invalid_detail_is_refused() {
        let (_dir, db) = db();
        let mut bad = entry(HistoryKind::Rename, 1, "x");
        bad.detail_json = "not json".to_owned();
        assert!(db.write(move |tx| insert_entry(tx, &bad, &[])).is_err());
    }
}
