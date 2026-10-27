//! The write-ahead rename journal: one row per file operation of an organize batch.
//!
//! The executor (`cv-rename`) inserts a row as `planned` **before** it touches the disk and moves
//! it to `done` or `failed` afterwards. A row still `planned` after a restart is an operation the
//! process was interrupted in; crash recovery looks at the disk and settles it. Undo replays the
//! `done` rows of a batch in reverse and marks them `undone`.
//!
//! Paths in the journal are absolute with the native separators, like the paths of the library
//! rows. [`repoint_file`] and [`repoint_folder`] move the library rows (`media_file`, `item.path`)
//! to a new location, so a caller can finish an operation and update the library in one
//! transaction, with no rescan. No row is ever deleted.

use rusqlite::{Connection, Row, params};

use super::library_ops::lies_under;
use super::path_case::fold;
use crate::error::DbResult;

/// Told about every path change the library followed, after it is committed.
///
/// The journal updates the library rows in its own transaction; a cache that keeps paths in
/// memory (the poster index behind `cv://img`) implements this to follow the same moves, so it
/// needs no rescan either. `from` and `to` are the paths as the journal stores them; a folder
/// move covers everything below it. An undo reports the reverse move.
pub trait PathObserver: Send + Sync {
    fn path_moved(&self, from: &str, to: &str, is_dir: bool);
}

/// A row to insert as `planned`.
#[derive(Debug, Clone, Copy)]
pub struct NewJournalRow<'a> {
    pub batch_id: &'a str,
    /// The position in the batch; unique per batch.
    pub seq: i64,
    pub item_id: Option<i64>,
    /// One of `move`, `copy`, `write_nfo`, `trash`.
    pub op: &'a str,
    pub from_path: &'a str,
    pub to_path: &'a str,
    /// How many folders above `to_path` the operation creates (see the `created_parents`
    /// column); `None` when not known.
    pub created_parents: Option<i64>,
}

/// A stored journal row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalRow {
    pub id: i64,
    pub batch_id: String,
    pub seq: i64,
    pub item_id: Option<i64>,
    pub op: String,
    pub from_path: String,
    pub to_path: String,
    /// `planned`, `done`, `failed` or `undone`.
    pub state: String,
    pub size_bytes: Option<i64>,
    pub modified_ms: Option<i64>,
    pub error: String,
    pub created_ms: i64,
    pub finished_ms: Option<i64>,
    /// How many folders above `to_path` the operation created; `None` for rows written before
    /// this was recorded.
    pub created_parents: Option<i64>,
}

const NOW_MS: &str = "CAST(unixepoch('subsec') * 1000 AS INTEGER)";

const COLUMNS: &str = "id, batch_id, seq, item_id, op, from_path, to_path, state, size_bytes, \
                       modified_ms, error, created_ms, finished_ms, created_parents";

fn row_of(row: &Row<'_>) -> rusqlite::Result<JournalRow> {
    Ok(JournalRow {
        id: row.get(0)?,
        batch_id: row.get(1)?,
        seq: row.get(2)?,
        item_id: row.get(3)?,
        op: row.get(4)?,
        from_path: row.get(5)?,
        to_path: row.get(6)?,
        state: row.get(7)?,
        size_bytes: row.get(8)?,
        modified_ms: row.get(9)?,
        error: row.get(10)?,
        created_ms: row.get(11)?,
        finished_ms: row.get(12)?,
        created_parents: row.get(13)?,
    })
}

/// Inserts a row as `planned`. An item id that does not exist is stored as none.
pub fn insert_planned(conn: &Connection, row: &NewJournalRow<'_>) -> DbResult<()> {
    conn.execute(
        "INSERT INTO rename_journal \
             (batch_id, seq, item_id, op, from_path, to_path, state, created_parents) \
         VALUES (?1, ?2, (SELECT id FROM item WHERE id = ?3), ?4, ?5, ?6, 'planned', ?7)",
        params![
            row.batch_id,
            row.seq,
            row.item_id,
            row.op,
            row.from_path,
            row.to_path,
            row.created_parents
        ],
    )?;
    Ok(())
}

/// Marks a row `done`, with the size and modification time of the destination.
pub fn mark_done(
    conn: &Connection,
    batch_id: &str,
    seq: i64,
    size_bytes: Option<i64>,
    modified_ms: Option<i64>,
) -> DbResult<()> {
    conn.execute(
        &format!(
            "UPDATE rename_journal SET state = 'done', size_bytes = ?3, modified_ms = ?4, \
             error = '', finished_ms = {NOW_MS} WHERE batch_id = ?1 AND seq = ?2"
        ),
        params![batch_id, seq, size_bytes, modified_ms],
    )?;
    Ok(())
}

/// Marks a row `failed`.
pub fn mark_failed(conn: &Connection, batch_id: &str, seq: i64, error: &str) -> DbResult<()> {
    conn.execute(
        &format!(
            "UPDATE rename_journal SET state = 'failed', error = ?3, finished_ms = {NOW_MS} \
             WHERE batch_id = ?1 AND seq = ?2"
        ),
        params![batch_id, seq, error],
    )?;
    Ok(())
}

/// Marks a row `undone`.
pub fn mark_undone(conn: &Connection, batch_id: &str, seq: i64) -> DbResult<()> {
    conn.execute(
        &format!(
            "UPDATE rename_journal SET state = 'undone', finished_ms = {NOW_MS} \
             WHERE batch_id = ?1 AND seq = ?2"
        ),
        params![batch_id, seq],
    )?;
    Ok(())
}

/// Records the bin id of a `trash` row whose original went to the Recycle Bin / Trash.
pub fn set_bin_id(conn: &Connection, batch_id: &str, seq: i64, bin_id: &str) -> DbResult<()> {
    conn.execute(
        "UPDATE rename_journal SET bin_id = ?3 WHERE batch_id = ?1 AND seq = ?2",
        params![batch_id, seq, bin_id],
    )?;
    Ok(())
}

/// A `trash` row of a batch whose original went to the Recycle Bin / Trash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinnedRow {
    pub seq: i64,
    pub item_id: Option<i64>,
    /// Where the original was.
    pub from_path: String,
    pub bin_id: String,
    pub size_bytes: Option<i64>,
    /// `done`, or `undone` once restored.
    pub state: String,
}

/// The rows of a batch that sent originals to the Recycle Bin / Trash, in order.
pub fn binned_rows(conn: &Connection, batch_id: &str) -> DbResult<Vec<BinnedRow>> {
    let mut stmt = conn.prepare(
        "SELECT seq, item_id, from_path, bin_id, size_bytes, state FROM rename_journal
         WHERE batch_id = ?1 AND bin_id IS NOT NULL ORDER BY seq",
    )?;
    let rows = stmt.query_map([batch_id], |r| {
        Ok(BinnedRow {
            seq: r.get(0)?,
            item_id: r.get(1)?,
            from_path: r.get(2)?,
            bin_id: r.get(3)?,
            size_bytes: r.get(4)?,
            state: r.get(5)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// The rows of one batch, in order.
pub fn batch_rows(conn: &Connection, batch_id: &str) -> DbResult<Vec<JournalRow>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM rename_journal WHERE batch_id = ?1 ORDER BY seq"
    ))?;
    let rows = stmt.query_map([batch_id], row_of)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// One batch of the journal, for the list a person undoes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchSummary {
    pub batch_id: String,
    /// When the first operation was written, epoch milliseconds.
    pub created_ms: i64,
    /// Every row of the batch.
    pub operations: u64,
    /// Rows that are `done`: what an undo would take back.
    pub done: u64,
    pub undone: u64,
    pub failed: u64,
    /// Rows still `planned`: a crash left them; recovery settles them before an undo.
    pub planned: u64,
    /// Distinct library items with a `done` or `undone` row.
    pub items: u64,
}

/// Every batch with its counts, newest first.
pub fn batch_summaries(conn: &Connection) -> DbResult<Vec<BatchSummary>> {
    let mut stmt = conn.prepare(
        "SELECT batch_id, MIN(created_ms), COUNT(*),                 COALESCE(SUM(state = 'done'), 0), COALESCE(SUM(state = 'undone'), 0),                 COALESCE(SUM(state = 'failed'), 0), COALESCE(SUM(state = 'planned'), 0),                 COUNT(DISTINCT CASE WHEN state IN ('done', 'undone') THEN item_id END)          FROM rename_journal GROUP BY batch_id ORDER BY MAX(id) DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        let count = |i: usize| -> rusqlite::Result<u64> {
            Ok(u64::try_from(row.get::<_, i64>(i)?).unwrap_or(0))
        };
        Ok(BatchSummary {
            batch_id: row.get(0)?,
            created_ms: row.get(1)?,
            operations: count(2)?,
            done: count(3)?,
            undone: count(4)?,
            failed: count(5)?,
            planned: count(6)?,
            items: count(7)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// When a batch's work happened and how many files it copied, for Activity's history.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BatchActivity {
    /// When its last operation was done (or, for an undone one, first done), epoch milliseconds.
    pub done_ms: Option<i64>,
    /// When its last operation was taken back, epoch milliseconds; `None` when nothing was.
    pub undone_ms: Option<i64>,
    /// `copy` rows that were done or later undone: the files a move between volumes copied.
    pub copied: u64,
}

/// The times and copy count of one batch (see [`BatchActivity`]).
pub fn batch_activity(conn: &Connection, batch_id: &str) -> DbResult<BatchActivity> {
    let mut stmt = conn.prepare_cached(
        "SELECT MAX(CASE WHEN state = 'done' THEN finished_ms END),
                MAX(CASE WHEN state = 'undone' THEN finished_ms END),
                COALESCE(SUM(op = 'copy' AND state IN ('done', 'undone')), 0)
         FROM rename_journal WHERE batch_id = ?1",
    )?;
    Ok(stmt.query_row([batch_id], |row| {
        Ok(BatchActivity {
            done_ms: row.get(0)?,
            undone_ms: row.get(1)?,
            copied: u64::try_from(row.get::<_, i64>(2)?).unwrap_or(0),
        })
    })?)
}

/// The library items a batch took (or gave back) files of, in the order the batch reached them.
pub fn batch_item_ids(conn: &Connection, batch_id: &str) -> DbResult<Vec<i64>> {
    let mut stmt = conn.prepare(
        "SELECT item_id FROM rename_journal \
         WHERE batch_id = ?1 AND item_id IS NOT NULL AND state IN ('done', 'undone') \
         GROUP BY item_id ORDER BY MIN(seq)",
    )?;
    let rows = stmt.query_map([batch_id], |row| row.get(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// The mark of one item of a batch (`rename_item_mark`): written before the item's first
/// operation and settled after its last, so a stop between two operations is found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemMarkRow {
    pub batch_id: String,
    pub item_key: String,
    /// `running`, `done`, `rolled_back`, `unfinished` or `abandoned`.
    pub state: String,
    /// What the executor was given for the item, as JSON; empty when not recorded.
    pub plan: String,
    pub started_ms: i64,
    pub finished_ms: Option<i64>,
}

fn mark_of(row: &Row<'_>) -> rusqlite::Result<ItemMarkRow> {
    Ok(ItemMarkRow {
        batch_id: row.get(0)?,
        item_key: row.get(1)?,
        state: row.get(2)?,
        plan: row.get(3)?,
        started_ms: row.get(4)?,
        finished_ms: row.get(5)?,
    })
}

const MARK_COLUMNS: &str = "batch_id, item_key, state, plan, started_ms, finished_ms";

/// Marks an item of a batch `running`, with its plan. An item run again in the same batch (a
/// resume) is marked `running` again.
pub fn open_item(conn: &Connection, batch_id: &str, item_key: &str, plan: &str) -> DbResult<()> {
    conn.execute(
        &format!(
            "INSERT INTO rename_item_mark (batch_id, item_key, state, plan) \
             VALUES (?1, ?2, 'running', ?3) \
             ON CONFLICT (batch_id, item_key) DO UPDATE SET state = 'running', \
                 plan = excluded.plan, started_ms = {NOW_MS}, finished_ms = NULL"
        ),
        params![batch_id, item_key, plan],
    )?;
    Ok(())
}

/// Settles the mark of an item: `done`, `rolled_back`, `unfinished` or `abandoned`.
pub fn close_item(conn: &Connection, batch_id: &str, item_key: &str, state: &str) -> DbResult<()> {
    conn.execute(
        &format!(
            "UPDATE rename_item_mark SET state = ?3, finished_ms = {NOW_MS} \
             WHERE batch_id = ?1 AND item_key = ?2"
        ),
        params![batch_id, item_key, state],
    )?;
    Ok(())
}

/// The item marks of one batch, in the order the items started.
pub fn item_marks(conn: &Connection, batch_id: &str) -> DbResult<Vec<ItemMarkRow>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {MARK_COLUMNS} FROM rename_item_mark WHERE batch_id = ?1 \
         ORDER BY started_ms, item_key"
    ))?;
    let rows = stmt.query_map([batch_id], mark_of)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Every item mark in `state`, in all batches, oldest first.
pub fn item_marks_in(conn: &Connection, state: &str) -> DbResult<Vec<ItemMarkRow>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {MARK_COLUMNS} FROM rename_item_mark WHERE state = ?1 \
         ORDER BY started_ms, batch_id, item_key"
    ))?;
    let rows = stmt.query_map([state], mark_of)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Every row still `planned`, oldest first: the operations a crash interrupted.
pub fn pending_rows(conn: &Connection) -> DbResult<Vec<JournalRow>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM rename_journal WHERE state = 'planned' ORDER BY id"
    ))?;
    let rows = stmt.query_map([], row_of)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// The old title folders of moves between data sources (batches `move-…`) whose title is in its
/// new place (the folder row is `done`) while some of its rows still have work: an original that
/// was in use or not allowed to go to the trash folder (`stopped: …`), or a row a kill left
/// `planned`. What is left in such a folder belongs to a title that lives elsewhere now, so a
/// scan must not take it for a new title.
pub fn left_behind_move_folders(conn: &Connection) -> DbResult<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT c.from_path FROM rename_journal c
         WHERE c.batch_id LIKE 'move-%' AND c.op = 'move' AND c.state = 'done'
           AND EXISTS (
             SELECT 1 FROM rename_journal r
             WHERE r.batch_id = c.batch_id AND r.item_id IS c.item_id
               AND (r.state = 'planned' OR (r.state = 'failed' AND r.error LIKE 'stopped: %')))
         ORDER BY c.from_path",
    )?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn split_path(path: &str) -> (&str, &str) {
    match path.rfind(['\\', '/']) {
        Some(at) => (&path[..at], &path[at + 1..]),
        None => ("", path),
    }
}

/// Points the `media_file` rows of the file at `from` to `to`. Matching ignores ASCII case (not on Linux), like
/// the volumes CineVault runs on. Returns the number of `media_file` rows changed.
///
/// When the file is a video and its item's folder (`item.path`) is the folder the video leaves,
/// the item's folder follows the video: a loose movie in the data source root that is moved into
/// a folder of its own then lives in that folder. The reverse move (an undo) brings the folder
/// back the same way.
pub fn repoint_file(conn: &Connection, from: &str, to: &str) -> DbResult<u64> {
    let (old_dir, old_name) = split_path(from);
    let (new_dir, new_name) = split_path(to);
    conn.execute(
        &format!(
            "UPDATE item SET path = ?1 \
             WHERE {path} = {dir2} AND ?1 <> ?2 AND id IN ( \
                 SELECT item_id FROM media_file \
                 WHERE kind = 'video' AND {directory} = {dir2} AND {filename} = {name3})",
            path = fold("path"),
            directory = fold("directory"),
            filename = fold("filename"),
            dir2 = fold("?2"),
            name3 = fold("?3"),
        ),
        params![new_dir, old_dir, old_name],
    )?;
    let changed = conn.execute(
        &format!(
            "UPDATE media_file SET directory = ?1, filename = ?2 \
             WHERE {directory} = {dir3} AND {filename} = {name4}",
            directory = fold("directory"),
            filename = fold("filename"),
            dir3 = fold("?3"),
            name4 = fold("?4"),
        ),
        params![new_dir, new_name, old_dir, old_name],
    )?;
    Ok(u64::try_from(changed).unwrap_or(u64::MAX))
}

/// Points everything stored under the folder `from` (`item.path`, `media_file.directory`) to the
/// folder `to`. A path matches when it is the folder or lies below it; `D:\Movies\Heat 2` is not
/// under `D:\Movies\Heat`. Returns the number of rows changed.
pub fn repoint_folder(conn: &Connection, from: &str, to: &str) -> DbResult<u64> {
    let old = from.trim_end_matches(['\\', '/']);
    let new = to.trim_end_matches(['\\', '/']);
    let old_len = i64::try_from(old.chars().count()).unwrap_or(i64::MAX);
    let mut changed = 0usize;
    for (table, column) in [("item", "path"), ("media_file", "directory")] {
        changed += conn.execute(
            &format!(
                "UPDATE {table} SET {column} = ?1 || substr({column}, ?2 + 1) \
                 WHERE {head} = {old3} \
                   AND (length({column}) = ?2 OR substr({column}, ?2 + 1, 1) IN ('\\', '/'))",
                head = fold(&format!("substr({column}, 1, ?2)")),
                old3 = fold("?3"),
            ),
            params![new, old_len, old],
        )?;
    }
    reassign_data_sources(conn, new)?;
    Ok(u64::try_from(changed).unwrap_or(u64::MAX))
}

/// After a folder moved, the items below it belong to the data source that now holds them. A
/// folder moved to another data source (change data source) changes `item.data_source`
/// with it; a rename inside one source finds it unchanged. Items whose path lies in no known
/// source keep their value.
fn reassign_data_sources(conn: &Connection, folder: &str) -> DbResult<()> {
    let mut stmt = conn.prepare("SELECT path FROM data_source")?;
    let roots = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);
    if roots.is_empty() {
        return Ok(());
    }
    let mut stmt = conn.prepare("SELECT id, path, data_source FROM item WHERE path <> ''")?;
    let items = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);
    for (id, path, current) in items {
        if !lies_under(&path, folder) {
            continue;
        }
        let best = roots
            .iter()
            .filter(|root| lies_under(&path, root))
            .max_by_key(|root| root.trim_end_matches(['\\', '/']).len());
        if let Some(best) = best
            && !lies_under(&path, &current)
        {
            conn.execute(
                "UPDATE item SET data_source = ?1 WHERE id = ?2",
                params![best, id],
            )?;
        }
    }
    Ok(())
}

/// The id of the library item that owns the file at `path` (absolute, native separators), if
/// the library knows the file. Matching follows the platform like [`repoint_file`]. Organize asks
/// it to tell a destination another item owns from a stray file.
pub fn owner_of_file(conn: &Connection, path: &str) -> DbResult<Option<i64>> {
    let (dir, name) = split_path(path);
    let found = conn
        .query_row(
            &format!(
                "SELECT item_id FROM media_file \
                 WHERE {} = {} AND {} = {} \
                 ORDER BY id LIMIT 1",
                fold("directory"),
                fold("?1"),
                fold("filename"),
                fold("?2"),
            ),
            params![dir, name],
            |row| row.get(0),
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })?;
    Ok(found)
}

/// The id of the library item that owns the folder at `path` (absolute): the item whose folder it
/// is, or one with a file directly inside it. Matching follows the platform like [`repoint_file`].
pub fn owner_of_folder(conn: &Connection, path: &str) -> DbResult<Option<i64>> {
    let folder = path.trim_end_matches(['\\', '/']);
    for sql in [
        format!(
            "SELECT id FROM item WHERE {} = {} ORDER BY id LIMIT 1",
            fold("path"),
            fold("?1")
        ),
        format!(
            "SELECT item_id FROM media_file WHERE {} = {} ORDER BY id LIMIT 1",
            fold("directory"),
            fold("?1")
        ),
    ] {
        match conn.query_row(&sql, [folder], |row| row.get(0)) {
            Ok(id) => return Ok(Some(id)),
            Err(rusqlite::Error::QueryReturnedNoRows) => {}
            Err(other) => return Err(other.into()),
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use cv_domain::{Item, MediaFile, MediaFileType};

    use super::*;
    use crate::repo::{get_item, insert_item};

    fn memory_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        crate::schema::migrate(&mut conn).unwrap();
        conn
    }

    fn row<'a>(batch: &'a str, seq: i64, from: &'a str, to: &'a str) -> NewJournalRow<'a> {
        NewJournalRow {
            batch_id: batch,
            seq,
            item_id: None,
            op: "move",
            from_path: from,
            to_path: to,
            created_parents: Some(0),
        }
    }

    #[test]
    fn an_item_mark_goes_running_then_settled_and_a_rerun_opens_it_again() {
        let conn = memory_db();
        open_item(&conn, "b1", "7", "{}").unwrap();
        open_item(&conn, "b1", "8", "").unwrap();
        assert_eq!(item_marks_in(&conn, "running").unwrap().len(), 2);
        close_item(&conn, "b1", "7", "done").unwrap();
        close_item(&conn, "b1", "8", "unfinished").unwrap();
        let marks = item_marks(&conn, "b1").unwrap();
        assert_eq!(marks.len(), 2);
        assert_eq!(marks[0].plan, "{}");
        assert!(marks.iter().all(|m| m.finished_ms.is_some()));
        assert_eq!(item_marks_in(&conn, "unfinished").unwrap()[0].item_key, "8");

        open_item(&conn, "b1", "8", "{\"again\":1}").unwrap();
        let again = item_marks_in(&conn, "running").unwrap();
        assert_eq!(again.len(), 1);
        assert_eq!(again[0].plan, "{\"again\":1}");
        assert_eq!(again[0].finished_ms, None);
        assert!(
            conn.execute(
                "UPDATE rename_item_mark SET state = 'odd' WHERE item_key = '8'",
                []
            )
            .is_err()
        );
    }

    #[test]
    fn a_row_goes_planned_then_done_and_pending_lists_the_unfinished() {
        let conn = memory_db();
        insert_planned(&conn, &row("b1", 0, "D:\\a", "D:\\b")).unwrap();
        insert_planned(&conn, &row("b1", 1, "D:\\c", "D:\\d")).unwrap();
        assert_eq!(pending_rows(&conn).unwrap().len(), 2);

        mark_done(&conn, "b1", 0, Some(10), Some(5)).unwrap();
        mark_failed(&conn, "b1", 1, "boom").unwrap();
        assert!(pending_rows(&conn).unwrap().is_empty());

        let rows = batch_rows(&conn, "b1").unwrap();
        assert_eq!(rows[0].state, "done");
        assert_eq!(
            (rows[0].size_bytes, rows[0].modified_ms),
            (Some(10), Some(5))
        );
        assert!(rows[0].finished_ms.is_some());
        assert_eq!(
            (rows[1].state.as_str(), rows[1].error.as_str()),
            ("failed", "boom")
        );

        mark_undone(&conn, "b1", 0).unwrap();
        assert_eq!(batch_rows(&conn, "b1").unwrap()[0].state, "undone");
    }

    #[test]
    fn batches_are_listed_newest_first_with_their_counts() {
        let conn = memory_db();
        insert_planned(&conn, &row("old", 0, "a", "b")).unwrap();
        insert_planned(&conn, &row("old", 1, "c", "d")).unwrap();
        insert_planned(&conn, &row("new", 0, "e", "f")).unwrap();
        mark_done(&conn, "old", 0, None, None).unwrap();
        mark_undone(&conn, "old", 1).unwrap();
        let list = batch_summaries(&conn).unwrap();
        let ids: Vec<_> = list.iter().map(|b| b.batch_id.as_str()).collect();
        assert_eq!(ids, ["new", "old"]);
        assert_eq!((list[0].operations, list[0].planned), (1, 1));
        assert_eq!(
            (list[1].operations, list[1].done, list[1].undone),
            (2, 1, 1)
        );
    }

    #[test]
    fn a_batch_cannot_reuse_a_sequence_number() {
        let conn = memory_db();
        insert_planned(&conn, &row("b1", 0, "x", "y")).unwrap();
        assert!(insert_planned(&conn, &row("b1", 0, "x", "z")).is_err());
        insert_planned(&conn, &row("b2", 0, "x", "z")).unwrap();
    }

    #[test]
    fn the_items_of_a_batch_are_listed_once_in_the_order_it_reached_them() {
        let conn = memory_db();
        let heat = insert_item(&conn, &movie("D:/Movies/Heat", "heat.mkv")).unwrap();
        let alien = insert_item(&conn, &movie("D:/Movies/Alien", "alien.mkv")).unwrap();
        for (seq, item) in [(0, alien), (1, heat), (2, alien)] {
            let mut new = row("b1", seq, "x", "y");
            new.item_id = Some(item.0);
            insert_planned(&conn, &new).unwrap();
            mark_done(&conn, "b1", seq, None, None).unwrap();
        }
        let mut failed = row("b1", 3, "x", "y");
        failed.item_id = Some(heat.0 + 100);
        insert_planned(&conn, &failed).unwrap();
        assert_eq!(batch_item_ids(&conn, "b1").unwrap(), [alien.0, heat.0]);
    }

    #[test]
    fn an_unknown_item_id_is_stored_as_none() {
        let conn = memory_db();
        let mut new = row("b1", 0, "x", "y");
        new.item_id = Some(9999);
        insert_planned(&conn, &new).unwrap();
        assert_eq!(batch_rows(&conn, "b1").unwrap()[0].item_id, None);
    }

    fn movie(folder: &str, file: &str) -> Item {
        let mut item = Item::movie("Heat");
        item.core.data_source = "D:\\Movies".into();
        item.core.path = folder.into();
        item.core
            .media_files
            .push(MediaFile::new(MediaFileType::Video, folder, file));
        item
    }

    #[test]
    fn repoint_file_follows_one_file_and_ignores_case_only_where_names_do() {
        let conn = memory_db();
        let id = insert_item(&conn, &movie("D:\\Movies\\Heat", "heat.mkv")).unwrap();
        let other = insert_item(&conn, &movie("D:\\Movies\\Heat", "other.mkv")).unwrap();
        // On Linux `D:\movies\HEAT\Heat.MKV` is another file than the stored one: nothing moves.
        let n = repoint_file(
            &conn,
            "D:\\movies\\HEAT\\Heat.MKV",
            "D:\\Movies\\Heat\\Heat (1995).mkv",
        )
        .unwrap();
        let moved = &get_item(&conn, id).unwrap().unwrap().core.media_files[0];
        if cv_fs::paths::CASE_SENSITIVE {
            assert_eq!(n, 0);
            assert_eq!(moved.filename, "heat.mkv");
            let n = repoint_file(
                &conn,
                "D:\\Movies\\Heat\\heat.mkv",
                "D:\\Movies\\Heat\\Heat (1995).mkv",
            )
            .unwrap();
            assert_eq!(n, 1);
        } else {
            assert_eq!(n, 1);
        }
        let moved = &get_item(&conn, id).unwrap().unwrap().core.media_files[0];
        assert_eq!(moved.filename, "Heat (1995).mkv");
        let kept = &get_item(&conn, other).unwrap().unwrap().core.media_files[0];
        assert_eq!(kept.filename, "other.mkv");
    }

    #[test]
    fn a_video_leaving_its_items_folder_takes_the_folder_along_and_back() {
        let conn = memory_db();
        // A loose movie in the root of its data source, with a poster beside it.
        let mut loose = movie("D:\\Movies", "Dune.2021.mkv");
        loose.core.media_files.push(MediaFile::new(
            MediaFileType::Poster,
            "D:\\Movies",
            "Dune.2021-poster.jpg",
        ));
        let dune = insert_item(&conn, &loose).unwrap();
        let other = insert_item(&conn, &movie("D:\\Movies", "Heat.mkv")).unwrap();
        let get = |id| get_item(&conn, id).unwrap().unwrap().core;

        // Artwork does not move the folder.
        repoint_file(
            &conn,
            "D:\\Movies\\Dune.2021-poster.jpg",
            "D:\\Movies\\Dune (2021)\\poster.jpg",
        )
        .unwrap();
        assert_eq!(get(dune).path, "D:\\Movies");

        repoint_file(
            &conn,
            "D:\\Movies\\Dune.2021.mkv",
            "D:\\Movies\\Dune (2021)\\Dune (2021).mkv",
        )
        .unwrap();
        assert_eq!(get(dune).path, "D:\\Movies\\Dune (2021)");
        assert_eq!(
            get(other).path,
            "D:\\Movies",
            "another movie keeps its folder"
        );

        // The undo moves it back.
        repoint_file(
            &conn,
            "D:\\Movies\\Dune (2021)\\Dune (2021).mkv",
            "D:\\Movies\\Dune.2021.mkv",
        )
        .unwrap();
        assert_eq!(get(dune).path, "D:\\Movies");
    }

    #[test]
    fn a_video_renamed_inside_a_subfolder_of_the_item_leaves_the_folder_alone() {
        let conn = memory_db();
        let mut disc = movie("D:\\Movies\\Heat", "x");
        disc.core.media_files = vec![MediaFile::new(
            MediaFileType::Video,
            "D:\\Movies\\Heat\\VIDEO_TS",
            "VIDEO_TS.IFO",
        )];
        let id = insert_item(&conn, &disc).unwrap();
        repoint_file(
            &conn,
            "D:\\Movies\\Heat\\VIDEO_TS\\VIDEO_TS.IFO",
            "D:\\Movies\\Heat (1995)\\VIDEO_TS\\VIDEO_TS.IFO",
        )
        .unwrap();
        assert_eq!(
            get_item(&conn, id).unwrap().unwrap().core.path,
            "D:\\Movies\\Heat"
        );
    }

    #[test]
    fn owners_are_found_by_file_and_by_folder_with_the_platforms_case_rule() {
        let conn = memory_db();
        let heat =
            insert_item(&conn, &movie("D:\\Movies\\Heat (1995)", "Heat (1995).mkv")).unwrap();
        // The same names, spelled as stored: found everywhere.
        assert_eq!(
            owner_of_file(&conn, "D:\\Movies\\Heat (1995)\\Heat (1995).mkv").unwrap(),
            Some(heat.0)
        );
        assert_eq!(
            owner_of_folder(&conn, "D:\\Movies\\Heat (1995)\\").unwrap(),
            Some(heat.0)
        );
        // Another case names the same file on Windows and macOS, and another file on Linux.
        let same = (!cv_fs::paths::CASE_SENSITIVE).then_some(heat.0);
        assert_eq!(
            owner_of_file(&conn, "d:\\movies\\heat (1995)\\HEAT (1995).mkv").unwrap(),
            same
        );
        assert_eq!(
            owner_of_folder(&conn, "D:\\MOVIES\\Heat (1995)\\").unwrap(),
            same
        );
        assert_eq!(
            owner_of_file(&conn, "D:\\Movies\\Heat (1995)\\other.mkv").unwrap(),
            None
        );
        assert_eq!(owner_of_folder(&conn, "D:\\Movies\\Nothing").unwrap(), None);
    }

    #[test]
    fn repoint_folder_moves_the_folder_and_below_but_not_a_sibling() {
        let conn = memory_db();
        let inside = insert_item(&conn, &movie("D:\\Movies\\Heat\\extras", "a.mkv")).unwrap();
        let exact = insert_item(&conn, &movie("D:\\Movies\\Heat", "b.mkv")).unwrap();
        let sibling = insert_item(&conn, &movie("D:\\Movies\\Heat 2", "c.mkv")).unwrap();
        repoint_folder(&conn, "D:\\Movies\\Heat", "D:\\Movies\\Heat (1995)").unwrap();

        let get = |id| get_item(&conn, id).unwrap().unwrap().core;
        assert_eq!(get(inside).path, "D:\\Movies\\Heat (1995)\\extras");
        assert_eq!(
            get(inside).media_files[0].directory,
            "D:\\Movies\\Heat (1995)\\extras"
        );
        assert_eq!(get(exact).path, "D:\\Movies\\Heat (1995)");
        assert_eq!(
            get(exact).media_files[0].directory,
            "D:\\Movies\\Heat (1995)"
        );
        assert_eq!(get(sibling).path, "D:\\Movies\\Heat 2");
        assert_eq!(get(sibling).media_files[0].directory, "D:\\Movies\\Heat 2");
    }

    #[test]
    fn repoint_folder_hands_the_items_to_the_data_source_that_now_holds_them() {
        let conn = memory_db();
        for root in [r"D:\Movies", r"E:\Archive"] {
            conn.execute(
                "INSERT INTO data_source (kind, path) VALUES ('movie', ?1)",
                [root],
            )
            .unwrap();
        }
        let moved = insert_item(&conn, &movie(r"D:\Movies\Heat", "heat.mkv")).unwrap();
        let kept = insert_item(&conn, &movie(r"D:\Movies\Ronin", "ronin.mkv")).unwrap();
        repoint_folder(&conn, r"D:\Movies\Heat", r"E:\Archive\Heat").unwrap();
        let get = |id| get_item(&conn, id).unwrap().unwrap().core;
        assert_eq!(get(moved).path, r"E:\Archive\Heat");
        assert_eq!(get(moved).data_source, r"E:\Archive");
        assert_eq!(get(kept).data_source, r"D:\Movies");

        // The reverse move (an undo) hands it back.
        repoint_folder(&conn, r"E:\Archive\Heat", r"D:\Movies\Heat").unwrap();
        assert_eq!(get(moved).data_source, r"D:\Movies");
    }
}
