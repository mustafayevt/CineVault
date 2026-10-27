//! Persisted runtime flags: the NEW flag and the review state.
//!
//! These two columns belong to the scanner, the matcher and the user, not to the metadata an
//! [`Item`](cv_domain::Item) carries, so [`update_item`](super::update_item) never touches them
//! and they have their own setters here. The last scrape provider and language and the rename
//! history are part of the item and are stored by `insert_item` / `update_item`.
//!
//! All setters leave `updated_ms` alone: a flag is not a metadata edit.

use cv_domain::{ItemId, ItemKind, ReviewState};
use rusqlite::{Connection, params, params_from_iter};

use super::codec::enum_text;
use crate::error::{DbError, DbResult};

/// The persisted runtime flags of one item.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ItemFlags {
    /// Added by a scan and not opened yet.
    pub is_new: bool,
    pub review_state: ReviewState,
}

/// Reads an item's flags.
pub fn get_flags(conn: &Connection, id: ItemId) -> DbResult<ItemFlags> {
    let (is_new, state): (bool, String) = conn
        .prepare_cached("SELECT is_new, review_state FROM item WHERE id = ?1")?
        .query_row([id.0], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => DbError::ItemNotFound(id.0),
            other => other.into(),
        })?;
    let review_state = ReviewState::parse(&state)
        .ok_or_else(|| DbError::InvalidData(format!("unknown review state `{state}`")))?;
    Ok(ItemFlags {
        is_new,
        review_state,
    })
}

/// Sets or clears the NEW flag on the given items. Unknown ids are ignored. Returns how many
/// items changed.
pub fn set_new(conn: &Connection, ids: &[ItemId], value: bool) -> DbResult<usize> {
    let mut stmt =
        conn.prepare_cached("UPDATE item SET is_new = ?2 WHERE id = ?1 AND is_new <> ?2")?;
    let mut changed = 0;
    for id in ids {
        changed += stmt.execute(params![id.0, value])?;
    }
    Ok(changed)
}

/// Clears the NEW flag on every item ("Clear NEW" for all), or only on one kind. Returns how many
/// items changed.
pub fn clear_all_new(conn: &Connection, kind: Option<ItemKind>) -> DbResult<usize> {
    let changed = match kind {
        Some(k) => conn.execute(
            "UPDATE item SET is_new = 0 WHERE is_new = 1 AND kind = ?1",
            [enum_text(&k)?],
        )?,
        None => conn.execute("UPDATE item SET is_new = 0 WHERE is_new = 1", [])?,
    };
    Ok(changed)
}

/// The ids of the items that carry the NEW flag, optionally of one kind, in id order.
pub fn new_ids(conn: &Connection, kind: Option<ItemKind>) -> DbResult<Vec<ItemId>> {
    let ids = match kind {
        Some(k) => conn
            .prepare_cached("SELECT id FROM item WHERE is_new = 1 AND trashed_entry IS NULL AND kind = ?1 ORDER BY id")?
            .query_map([enum_text(&k)?], |r| r.get::<_, i64>(0).map(ItemId))?
            .collect::<Result<Vec<_>, _>>()?,
        None => conn
            .prepare_cached("SELECT id FROM item WHERE is_new = 1 AND trashed_entry IS NULL ORDER BY id")?
            .query_map([], |r| r.get::<_, i64>(0).map(ItemId))?
            .collect::<Result<Vec<_>, _>>()?,
    };
    Ok(ids)
}

/// Clears the NEW flag on the children of `parent` only (for example a show's episodes).
pub fn clear_new_children(conn: &Connection, parent: ItemId) -> DbResult<usize> {
    Ok(conn.execute(
        "UPDATE item SET is_new = 0 WHERE is_new = 1 AND parent_id = ?1",
        [parent.0],
    )?)
}

/// Counts NEW items, optionally of one kind (for example the "new episodes" count of a show is
/// [`count_new_children`]).
pub fn count_new(conn: &Connection, kind: Option<ItemKind>) -> DbResult<u64> {
    let n: i64 = match kind {
        Some(k) => conn.query_row(
            "SELECT COUNT(*) FROM item WHERE is_new = 1 AND trashed_entry IS NULL AND kind = ?1",
            [enum_text(&k)?],
            |r| r.get(0),
        )?,
        None => conn.query_row(
            "SELECT COUNT(*) FROM item WHERE is_new = 1 AND trashed_entry IS NULL",
            [],
            |r| r.get(0),
        )?,
    };
    Ok(u64::try_from(n).unwrap_or(0))
}

/// How many titles (movies and shows) the items `ids` make up: a movie or a show counts once,
/// and episodes count as the show they belong to. Seasons and sets are not titles.
pub fn count_titles_of(conn: &Connection, ids: &[ItemId]) -> DbResult<u64> {
    if ids.is_empty() {
        return Ok(0);
    }
    let list = serde_json::to_string(&ids.iter().map(|id| id.0).collect::<Vec<_>>())?;
    let n: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT CASE WHEN kind = 'episode' THEN parent_id ELSE id END) \
         FROM item WHERE id IN (SELECT value FROM json_each(?1)) \
           AND kind IN ('movie', 'show', 'episode')",
        [list],
        |r| r.get(0),
    )?;
    Ok(u64::try_from(n).unwrap_or(0))
}

/// How many shows have NEW episodes: the shows the TV library's "New episodes" filter lists.
pub fn count_shows_with_new_episodes(conn: &Connection) -> DbResult<u64> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM item s WHERE s.kind = 'show' AND s.trashed_entry IS NULL AND EXISTS \
         (SELECT 1 FROM item e WHERE e.parent_id = s.id AND e.kind = 'episode' AND e.is_new = 1 AND e.trashed_entry IS NULL)",
        [],
        |r| r.get(0),
    )?;
    Ok(u64::try_from(n).unwrap_or(0))
}

/// Counts NEW children of `parent`.
pub fn count_new_children(conn: &Connection, parent: ItemId) -> DbResult<u64> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM item WHERE is_new = 1 AND parent_id = ?1",
        [parent.0],
        |r| r.get(0),
    )?;
    Ok(u64::try_from(n).unwrap_or(0))
}

/// Sets the review state of an item.
pub fn set_review_state(conn: &Connection, id: ItemId, state: ReviewState) -> DbResult<()> {
    let changed = conn
        .prepare_cached("UPDATE item SET review_state = ?2 WHERE id = ?1")?
        .execute(params![id.0, state.as_str()])?;
    if changed == 0 {
        return Err(DbError::ItemNotFound(id.0));
    }
    Ok(())
}

/// Sets the review state on several items at once. Unknown ids are ignored.
pub fn set_review_states(conn: &Connection, ids: &[ItemId], state: ReviewState) -> DbResult<usize> {
    if ids.is_empty() {
        return Ok(0);
    }
    let marks = vec!["?"; ids.len()].join(", ");
    let sql = format!("UPDATE item SET review_state = ?1 WHERE id IN ({marks})");
    let params = std::iter::once(rusqlite::types::Value::Text(state.as_str().into()))
        .chain(ids.iter().map(|i| rusqlite::types::Value::Integer(i.0)));
    Ok(conn.execute(&sql, params_from_iter(params))?)
}
