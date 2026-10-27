//! Item locks, field locks and the scrape write path that honours them.
//!
//! An item is protected in two ways. The entity lock is the `locked` column of the item
//! (`ItemCore::locked`): nothing may scrape or merge into a locked item. A field lock is one row
//! of `field_lock` per `(item, field)`: that single field is kept as the user set it. Scraping
//! goes through [`apply_scrape`], which enforces both; the Read NFO action reverts locked fields
//! with [`cv_domain::revert_locked`].
//!
//! The setters here do not touch `updated_ms`: a lock is not a metadata edit. Rows with a field
//! key this build does not know are kept and ignored, so a newer version's locks survive.

use std::collections::BTreeMap;

use cv_domain::{FieldSet, Item, ItemId, MetaField, ScrapeOptions, ScrapeOutcome};
use rusqlite::{Connection, params};

use super::items::{get_item, update_item};
use crate::error::{DbError, DbResult};

/// The locked fields of one item.
pub fn field_locks(conn: &Connection, id: ItemId) -> DbResult<FieldSet> {
    let mut stmt = conn.prepare_cached("SELECT field FROM field_lock WHERE item_id = ?1")?;
    let rows = stmt.query_map([id.0], |r| r.get::<_, String>(0))?;
    let mut set = FieldSet::new();
    for key in rows {
        if let Some(field) = MetaField::from_key(&key?) {
            set.insert(field);
        }
    }
    Ok(set)
}

/// The locked fields of several items. Items without a lock are not in the map.
pub fn field_locks_of(conn: &Connection, ids: &[ItemId]) -> DbResult<BTreeMap<ItemId, FieldSet>> {
    let mut out: BTreeMap<ItemId, FieldSet> = BTreeMap::new();
    for id in ids {
        let set = field_locks(conn, *id)?;
        if !set.is_empty() {
            out.insert(*id, set);
        }
    }
    Ok(out)
}

fn require_item(conn: &Connection, id: ItemId) -> DbResult<()> {
    if super::items::item_exists(conn, id)? {
        Ok(())
    } else {
        Err(DbError::ItemNotFound(id.0))
    }
}

/// Locks or unlocks one field. Returns whether the stored state changed.
///
/// # Errors
/// Fails when the item does not exist.
pub fn set_field_lock(
    conn: &Connection,
    id: ItemId,
    field: MetaField,
    locked: bool,
) -> DbResult<bool> {
    require_item(conn, id)?;
    let changed = if locked {
        conn.prepare_cached("INSERT OR IGNORE INTO field_lock (item_id, field) VALUES (?1, ?2)")?
            .execute(params![id.0, field.key()])?
    } else {
        conn.prepare_cached("DELETE FROM field_lock WHERE item_id = ?1 AND field = ?2")?
            .execute(params![id.0, field.key()])?
    };
    Ok(changed > 0)
}

/// Replaces the known locked fields of an item with `fields` (unknown keys already stored by a
/// newer version stay).
///
/// # Errors
/// Fails when the item does not exist.
pub fn set_field_locks(conn: &Connection, id: ItemId, fields: &FieldSet) -> DbResult<()> {
    require_item(conn, id)?;
    for field in cv_domain::all_fields() {
        set_field_lock(conn, id, field, fields.contains(field))?;
    }
    Ok(())
}

/// Locks or unlocks whole items. Unknown ids are ignored. Returns how many items changed.
pub fn set_item_locked(conn: &Connection, ids: &[ItemId], locked: bool) -> DbResult<usize> {
    let mut stmt =
        conn.prepare_cached("UPDATE item SET locked = ?2 WHERE id = ?1 AND locked <> ?2")?;
    let mut changed = 0;
    for id in ids {
        changed += stmt.execute(params![id.0, locked])?;
    }
    Ok(changed)
}

/// Applies a scraped item to a stored one: the write path of every scrape.
///
/// Loads the item and its field locks, merges under the options (see
/// [`cv_domain::apply_scrape`]) and stores the result when something changed. A locked item is
/// not written.
///
/// # Errors
/// Fails when the item does not exist or the database fails.
pub fn apply_scrape(
    conn: &Connection,
    id: ItemId,
    scraped: &Item,
    options: &ScrapeOptions,
) -> DbResult<ScrapeOutcome> {
    let mut item = get_item(conn, id)?.ok_or(DbError::ItemNotFound(id.0))?;
    let locks = field_locks(conn, id)?;
    let outcome = cv_domain::apply_scrape(&mut item, scraped, options, &locks);
    if outcome.touched() {
        update_item(conn, &item)?;
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests;
