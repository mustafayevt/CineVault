//! The organize-state cache: the `item_organize_state` table.
//!
//! For each movie and show it holds whether the folder and file names already match the active
//! naming profile: `tidy`, `pending` (a rename would change something), `conflict` (with the
//! reason), `locked` or `unplanned`. A background job fills it by running the renamer planner as a
//! dry run, so the lifecycle and the pipeline counts need no planning at read time.
//!
//! Every row carries the key of the profile it was computed for. A row with another key is stale:
//! [`organize_state`] still returns it, and [`stale_items`] lists the items that need a new
//! computation (no row, or a row of another profile). Rows go with their item
//! (`ON DELETE CASCADE`); nothing else is touched.

use std::collections::BTreeMap;

use cv_domain::{ItemId, ItemKind};
use rusqlite::{Connection, OptionalExtension, Row, params};

use super::codec::enum_text;
use crate::error::DbResult;

/// Whether an item's names match the naming profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum OrganizeStateKind {
    /// The names already match the profile.
    Tidy,
    /// A rename to the profile's names would change something.
    Pending,
    /// A rename would collide with another title or file; the reason says how.
    Conflict,
    /// The title is locked, so nothing is planned for it.
    Locked,
    /// It cannot be planned now: files missing, folder offline or a template problem.
    Unplanned,
}

impl OrganizeStateKind {
    /// The text stored in the table.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tidy => "tidy",
            Self::Pending => "pending",
            Self::Conflict => "conflict",
            Self::Locked => "locked",
            Self::Unplanned => "unplanned",
        }
    }

    /// Reads the stored text; `None` for anything else.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "tidy" => Self::Tidy,
            "pending" => Self::Pending,
            "conflict" => Self::Conflict,
            "locked" => Self::Locked,
            "unplanned" => Self::Unplanned,
            _ => return None,
        })
    }
}

/// One state to store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrganizeStateEntry {
    pub item: ItemId,
    pub state: OrganizeStateKind,
    /// Why, for a conflict or an unplanned item; empty otherwise.
    pub reason: String,
}

/// A stored state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrganizeStateRow {
    pub item: ItemId,
    pub state: OrganizeStateKind,
    pub reason: String,
    /// The profile the row was computed for.
    pub profile_key: String,
    /// When it was computed, epoch milliseconds.
    pub computed_ms: i64,
}

fn row_of(row: &Row<'_>) -> rusqlite::Result<OrganizeStateRow> {
    let state: String = row.get(1)?;
    Ok(OrganizeStateRow {
        item: ItemId(row.get(0)?),
        // The CHECK constraint allows only the known values; a newer build's value reads as
        // "cannot be planned" instead of failing the whole read.
        state: OrganizeStateKind::parse(&state).unwrap_or(OrganizeStateKind::Unplanned),
        reason: row.get(2)?,
        profile_key: row.get(3)?,
        computed_ms: row.get(4)?,
    })
}

const COLUMNS: &str = "item_id, state, reason, profile_key, computed_ms";

/// Stores the states computed for `profile_key`, replacing the rows of the same items. An item
/// that no longer exists is ignored.
pub fn set_organize_states(
    conn: &Connection,
    entries: &[OrganizeStateEntry],
    profile_key: &str,
    computed_ms: i64,
) -> DbResult<()> {
    let mut stmt = conn.prepare_cached(
        "INSERT INTO item_organize_state (item_id, state, reason, profile_key, computed_ms)
         SELECT ?1, ?2, ?3, ?4, ?5 WHERE EXISTS (SELECT 1 FROM item WHERE id = ?1)
         ON CONFLICT (item_id) DO UPDATE SET
             state = excluded.state, reason = excluded.reason,
             profile_key = excluded.profile_key, computed_ms = excluded.computed_ms",
    )?;
    for entry in entries {
        stmt.execute(params![
            entry.item.0,
            entry.state.as_str(),
            entry.reason,
            profile_key,
            computed_ms
        ])?;
    }
    Ok(())
}

/// The stored state of one item, whatever profile it was computed for.
pub fn organize_state(conn: &Connection, item: ItemId) -> DbResult<Option<OrganizeStateRow>> {
    Ok(conn
        .prepare_cached(&format!(
            "SELECT {COLUMNS} FROM item_organize_state WHERE item_id = ?1"
        ))?
        .query_row(params![item.0], row_of)
        .optional()?)
}

/// The stored states of many items (items without a row are absent).
pub fn organize_states_for(
    conn: &Connection,
    items: &[ItemId],
) -> DbResult<BTreeMap<ItemId, OrganizeStateRow>> {
    let mut found = BTreeMap::new();
    for chunk in items.chunks(500) {
        let marks = vec!["?"; chunk.len()].join(",");
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM item_organize_state WHERE item_id IN ({marks})"
        ))?;
        let rows = stmt.query_map(
            rusqlite::params_from_iter(chunk.iter().map(|id| id.0)),
            row_of,
        )?;
        for row in rows {
            let row = row?;
            found.insert(row.item, row);
        }
    }
    Ok(found)
}

/// Forgets the stored states of `items`, so they read as "checking" until recomputed. Returns how
/// many rows were removed.
pub fn clear_organize_states(conn: &Connection, items: &[ItemId]) -> DbResult<usize> {
    let mut stmt = conn.prepare_cached("DELETE FROM item_organize_state WHERE item_id = ?1")?;
    let mut removed = 0;
    for item in items {
        removed += stmt.execute(params![item.0])?;
    }
    Ok(removed)
}

/// The movies and shows that have no state for `profile_key` (no row, or a row of another
/// profile), in library order.
pub fn stale_items(conn: &Connection, profile_key: &str) -> DbResult<Vec<ItemId>> {
    let mut stmt = conn.prepare_cached(
        "SELECT i.id FROM item i
         LEFT JOIN item_organize_state s ON s.item_id = i.id AND s.profile_key = ?1
         WHERE i.kind IN ('movie', 'show') AND s.item_id IS NULL AND i.trashed_entry IS NULL
         ORDER BY i.kind, i.title COLLATE NOCASE, i.id",
    )?;
    let rows = stmt.query_map(params![profile_key], |r| Ok(ItemId(r.get(0)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// How many titles of `kind` are in each state for `profile_key`, plus how many have no current
/// state yet.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OrganizeStateCounts {
    pub tidy: u32,
    pub pending: u32,
    pub conflict: u32,
    pub locked: u32,
    pub unplanned: u32,
    /// Titles whose state is still being computed.
    pub checking: u32,
}

/// The state counts of the titles of `kind` (movies or shows).
pub fn organize_state_counts(
    conn: &Connection,
    kind: ItemKind,
    profile_key: &str,
) -> DbResult<OrganizeStateCounts> {
    let kind = enum_text(&kind)?;
    let mut stmt = conn.prepare_cached(
        "SELECT s.state, count(*) FROM item i
         LEFT JOIN item_organize_state s ON s.item_id = i.id AND s.profile_key = ?2
         WHERE i.kind = ?1 AND i.trashed_entry IS NULL GROUP BY s.state",
    )?;
    let rows = stmt.query_map(params![kind, profile_key], |r| {
        Ok((r.get::<_, Option<String>>(0)?, r.get::<_, i64>(1)?))
    })?;
    let mut counts = OrganizeStateCounts::default();
    for row in rows {
        let (state, n) = row?;
        let n = u32::try_from(n).unwrap_or(u32::MAX);
        match state.as_deref().and_then(OrganizeStateKind::parse) {
            Some(OrganizeStateKind::Tidy) => counts.tidy = n,
            Some(OrganizeStateKind::Pending) => counts.pending = n,
            Some(OrganizeStateKind::Conflict) => counts.conflict = n,
            Some(OrganizeStateKind::Locked) => counts.locked = n,
            Some(OrganizeStateKind::Unplanned) => counts.unplanned = n,
            None => counts.checking += n,
        }
    }
    Ok(counts)
}

#[cfg(test)]
mod tests {
    use cv_domain::Item;

    use super::*;
    use crate::Database;
    use crate::repo::insert_item;

    fn library() -> (tempfile::TempDir, Database, Vec<ItemId>) {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(dir.path().join("library.db")).unwrap();
        let ids = db
            .write(|tx| {
                Ok(vec![
                    insert_item(tx, &Item::movie("Alien"))?,
                    insert_item(tx, &Item::movie("Heat"))?,
                    insert_item(tx, &Item::show("Dark"))?,
                ])
            })
            .unwrap();
        (dir, db, ids)
    }

    fn entry(item: ItemId, state: OrganizeStateKind, reason: &str) -> OrganizeStateEntry {
        OrganizeStateEntry {
            item,
            state,
            reason: reason.to_owned(),
        }
    }

    #[test]
    fn states_read_back_and_a_new_computation_replaces_them() {
        let (_dir, db, ids) = library();
        let first = ids[0];
        db.write(move |tx| {
            set_organize_states(
                tx,
                &[entry(first, OrganizeStateKind::Conflict, "same folder")],
                "p1",
                10,
            )
        })
        .unwrap();
        let row = db.read(move |c| organize_state(c, first)).unwrap().unwrap();
        assert_eq!(
            (row.state, row.reason.as_str(), row.profile_key.as_str()),
            (OrganizeStateKind::Conflict, "same folder", "p1")
        );

        db.write(move |tx| {
            set_organize_states(tx, &[entry(first, OrganizeStateKind::Tidy, "")], "p2", 20)
        })
        .unwrap();
        let row = db.read(move |c| organize_state(c, first)).unwrap().unwrap();
        assert_eq!((row.state, row.computed_ms), (OrganizeStateKind::Tidy, 20));
    }

    #[test]
    fn stale_items_are_those_without_a_row_of_the_current_profile() {
        let (_dir, db, ids) = library();
        let (alien, heat, dark) = (ids[0], ids[1], ids[2]);
        db.write(move |tx| {
            set_organize_states(tx, &[entry(alien, OrganizeStateKind::Tidy, "")], "p1", 1)?;
            set_organize_states(tx, &[entry(heat, OrganizeStateKind::Pending, "")], "old", 1)
        })
        .unwrap();
        let stale = db.read(|c| stale_items(c, "p1")).unwrap();
        assert_eq!(stale, vec![heat, dark]);
        // A new profile makes every row stale.
        assert_eq!(db.read(|c| stale_items(c, "p2")).unwrap().len(), 3);
    }

    #[test]
    fn counts_split_by_state_and_count_missing_rows_as_checking() {
        let (_dir, db, ids) = library();
        let (alien, heat) = (ids[0], ids[1]);
        db.write(move |tx| {
            set_organize_states(
                tx,
                &[
                    entry(alien, OrganizeStateKind::Tidy, ""),
                    entry(heat, OrganizeStateKind::Locked, ""),
                ],
                "p1",
                1,
            )
        })
        .unwrap();
        let movies = db
            .read(|c| organize_state_counts(c, ItemKind::Movie, "p1"))
            .unwrap();
        assert_eq!((movies.tidy, movies.locked, movies.checking), (1, 1, 0));
        let shows = db
            .read(|c| organize_state_counts(c, ItemKind::Show, "p1"))
            .unwrap();
        assert_eq!(shows.checking, 1);
        let other = db
            .read(|c| organize_state_counts(c, ItemKind::Movie, "p2"))
            .unwrap();
        assert_eq!((other.tidy, other.checking), (0, 2));
    }

    #[test]
    fn clearing_and_deleting_remove_rows_and_unknown_items_store_nothing() {
        let (_dir, db, ids) = library();
        let (alien, heat) = (ids[0], ids[1]);
        db.write(move |tx| {
            set_organize_states(
                tx,
                &[
                    entry(alien, OrganizeStateKind::Tidy, ""),
                    entry(heat, OrganizeStateKind::Tidy, ""),
                    entry(ItemId(9_999), OrganizeStateKind::Tidy, ""),
                ],
                "p1",
                1,
            )
        })
        .unwrap();
        assert!(
            db.read(|c| organize_state(c, ItemId(9_999)))
                .unwrap()
                .is_none()
        );
        assert_eq!(
            db.write(move |tx| clear_organize_states(tx, &[alien]))
                .unwrap(),
            1
        );
        db.write(move |tx| {
            tx.execute("DELETE FROM item WHERE id = ?1", [heat.0])?;
            Ok(())
        })
        .unwrap();
        let both = db
            .read(move |c| organize_states_for(c, &[alien, heat]))
            .unwrap();
        assert!(both.is_empty());
    }

    #[test]
    fn the_state_text_round_trips() {
        for state in [
            OrganizeStateKind::Tidy,
            OrganizeStateKind::Pending,
            OrganizeStateKind::Conflict,
            OrganizeStateKind::Locked,
            OrganizeStateKind::Unplanned,
        ] {
            assert_eq!(OrganizeStateKind::parse(state.as_str()), Some(state));
        }
        assert_eq!(OrganizeStateKind::parse("other"), None);
    }
}
