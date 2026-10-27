//! The full-text index (`item_fts`, rowid = `item.id`).
//!
//! One row per item covers its titles, plot, people, tags and file names. [`reindex_item`] runs
//! inside every `insert_item` / `update_item`, so the index follows edits automatically;
//! deleting an item removes its row through a trigger. Code that changes people, tags or files
//! by other means calls [`reindex_item`] itself, and [`rebuild`] recreates the whole index. The
//! folded title keys of [`super::search`] are written at the same time.

use cv_domain::ItemId;
use rusqlite::{Connection, params};

use crate::error::DbResult;

/// Column weights for ranking, in the column order of `item_fts`:
/// title, original title, sort title, plot, people, tags, file names. A lower bm25 is better, and
/// larger weights make a column count for more.
const RANK_WEIGHTS: &str = "10.0, 6.0, 3.0, 1.0, 4.0, 3.0, 2.0";

const INDEX_SELECT: &str = "
    SELECT i.id,
           trim(i.title || ' ' || i.english_title),
           i.original_title,
           i.sort_title,
           i.plot,
           COALESCE((SELECT group_concat(p.name, ' ')
                       FROM credit c JOIN person p ON p.id = c.person_id
                      WHERE c.item_id = i.id), ''),
           COALESCE((SELECT group_concat(t.name, ' ')
                       FROM item_tag it JOIN tag t ON t.id = it.tag_id
                      WHERE it.item_id = i.id), ''),
           trim(i.original_filename || ' ' ||
                COALESCE((SELECT group_concat(m.filename, ' ')
                            FROM media_file m WHERE m.item_id = i.id), ''))
      FROM item i";

/// Recomputes the index row of one item from its current data.
pub fn reindex_item(conn: &Connection, id: ItemId) -> DbResult<()> {
    conn.prepare_cached("DELETE FROM item_fts WHERE rowid = ?1")?
        .execute([id.0])?;
    let sql = format!(
        "INSERT INTO item_fts (rowid, title, original_title, sort_title, plot, people, tags, filenames)
         {INDEX_SELECT} WHERE i.id = ?1"
    );
    conn.prepare_cached(&sql)?.execute([id.0])?;
    super::search::write_keys(conn, id.0)?;
    Ok(())
}

/// Recreates the whole index from the item tables. Returns the number of indexed items.
pub fn rebuild(conn: &Connection) -> DbResult<usize> {
    conn.execute("DELETE FROM item_fts", [])?;
    let sql = format!(
        "INSERT INTO item_fts (rowid, title, original_title, sort_title, plot, people, tags, filenames)
         {INDEX_SELECT}"
    );
    let indexed = conn.execute(&sql, [])?;
    super::search::rewrite_all_keys(conn)?;
    Ok(indexed)
}

/// Turns what a user typed into an FTS5 query: every word must match, as a prefix, so partial
/// words work while typing (`villen` finds Villeneuve). Punctuation and FTS operators in the
/// input are neutralised by quoting. Returns `None` when there is nothing to search for.
pub fn match_expression(text: &str) -> Option<String> {
    let terms: Vec<String> = text
        .split_whitespace()
        .filter(|w| w.chars().any(char::is_alphanumeric))
        .map(|w| format!("\"{}\"*", w.replace('"', "\"\"")))
        .collect();
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" "))
    }
}

/// An item found by [`search`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SearchHit {
    pub id: ItemId,
    /// bm25 score; lower is a better match.
    pub rank: f64,
}

/// Full-text search, best matches first. An empty query returns no hits.
pub fn search(conn: &Connection, text: &str, limit: usize) -> DbResult<Vec<SearchHit>> {
    let Some(expr) = match_expression(text) else {
        return Ok(Vec::new());
    };
    let sql = format!(
        "SELECT rowid, bm25(item_fts, {RANK_WEIGHTS}) AS rank
           FROM item_fts WHERE item_fts MATCH ?1
          ORDER BY rank, rowid LIMIT ?2"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let limit = i64::try_from(limit).unwrap_or(i64::MAX);
    let hits = stmt
        .query_map(params![expr, limit], |r| {
            Ok(SearchHit {
                id: ItemId(r.get(0)?),
                rank: r.get(1)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(hits)
}
