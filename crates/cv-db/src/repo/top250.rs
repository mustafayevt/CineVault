//! The IMDb Top 250 position of movies and shows.
//!
//! The rank is a plain number on the item (`item.top250`, `0` for "not on the list"). It is
//! written by [`apply_top250`] from a freshly fetched chart: titles on the chart get their
//! position, titles that were ranked before and are no longer on it are cleared. The write honours
//! the entity lock and the Top 250 field lock like every other metadata write, so a locked title
//! keeps the rank the user set.
//!
//! The rank is read by the Top 250 column, the sort and filters of the library views, the
//! `top250` renamer token and the `<top250>` NFO element.

use std::collections::{BTreeMap, BTreeSet};

use cv_domain::{ItemDetail, ItemId, ItemKind, MetaField, id_key};
use rusqlite::{Connection, params};

use super::items::{get_item, items_by_provider_id, update_item};
use super::locks::field_locks;
use crate::error::{DbError, DbResult};

/// What storing a chart did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Top250Report {
    /// Library items that now have a position (changed or already right).
    pub ranked: usize,
    /// Of those, how many had a different position before.
    pub changed: usize,
    /// Items that were ranked before and are not on the chart any more.
    pub cleared: usize,
    /// Items left as they were because the item or its Top 250 field is locked.
    pub locked: usize,
    /// Chart entries with no item in the library.
    pub not_in_library: usize,
}

/// The kinds that can be on a Top 250 chart.
fn has_rank(kind: ItemKind) -> bool {
    matches!(kind, ItemKind::Movie | ItemKind::Show)
}

/// Items of `kind` that currently have a position, with it.
fn ranked_items(conn: &Connection, kind: ItemKind) -> DbResult<BTreeMap<ItemId, u32>> {
    let kind = super::codec::enum_text(&kind)?;
    let mut stmt =
        conn.prepare_cached("SELECT id, top250 FROM item WHERE kind = ?1 AND top250 > 0")?;
    let rows = stmt
        .query_map(params![kind], |r| {
            Ok((ItemId(r.get(0)?), r.get::<_, i64>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows
        .into_iter()
        .map(|(id, rank)| (id, u32::try_from(rank).unwrap_or(0)))
        .collect())
}

/// Writes `rank` (`0` clears it) to one item. Returns `None` when the item is protected by its
/// entity lock or the Top 250 field lock, otherwise whether the stored rank changed.
fn write_rank(conn: &Connection, id: ItemId, rank: u32) -> DbResult<Option<bool>> {
    let mut item = get_item(conn, id)?.ok_or(DbError::ItemNotFound(id.0))?;
    if item.core.locked || field_locks(conn, id)?.contains(MetaField::Top250) {
        return Ok(None);
    }
    let slot = match &mut item.detail {
        ItemDetail::Movie(m) => &mut m.top250,
        ItemDetail::Show(s) => &mut s.top250,
        _ => return Ok(Some(false)),
    };
    if *slot == rank {
        return Ok(Some(false));
    }
    *slot = rank;
    update_item(conn, &item)?;
    Ok(Some(true))
}

/// Stores a chart: `ranks` pairs an IMDb title ID with its position (1 for the best).
///
/// Only items of `kind` (movies or shows) are touched, found by their IMDb ID. Everything is
/// done on the caller's connection, so run it inside one write transaction. An empty chart is
/// refused: it would clear every rank.
///
/// # Errors
/// [`DbError::InvalidData`] for an empty chart or a kind that has no Top 250; storage errors
/// otherwise.
pub fn apply_top250(
    conn: &Connection,
    kind: ItemKind,
    ranks: &[(String, u32)],
) -> DbResult<Top250Report> {
    if !has_rank(kind) {
        return Err(DbError::InvalidData(format!(
            "{kind:?} items have no Top 250 position"
        )));
    }
    if ranks.is_empty() {
        return Err(DbError::InvalidData("the Top 250 list is empty".into()));
    }
    let mut report = Top250Report::default();
    let mut on_chart = BTreeSet::new();
    for (imdb_id, rank) in ranks {
        let owners: Vec<ItemId> = items_by_provider_id(conn, id_key::IMDB, imdb_id)?
            .into_iter()
            .filter(|id| {
                get_item(conn, *id)
                    .ok()
                    .flatten()
                    .is_some_and(|item| item.kind() == kind)
            })
            .collect();
        if owners.is_empty() {
            report.not_in_library += 1;
            continue;
        }
        for id in owners {
            on_chart.insert(id);
            match write_rank(conn, id, *rank)? {
                None => report.locked += 1,
                Some(changed) => {
                    report.ranked += 1;
                    report.changed += usize::from(changed);
                }
            }
        }
    }
    for id in ranked_items(conn, kind)?.into_keys() {
        if on_chart.contains(&id) {
            continue;
        }
        match write_rank(conn, id, 0)? {
            None => report.locked += 1,
            Some(_) => report.cleared += 1,
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use cv_domain::Item;

    use super::*;
    use crate::repo::{insert_item, set_field_lock, set_item_locked};

    fn memory_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        crate::schema::migrate(&mut conn).unwrap();
        conn
    }

    fn movie(conn: &Connection, title: &str, imdb: &str, rank: u32) -> ItemId {
        let mut item = Item::movie(title);
        item.core.ids.set(id_key::IMDB, imdb);
        if let ItemDetail::Movie(m) = &mut item.detail {
            m.top250 = rank;
        }
        insert_item(conn, &item).unwrap()
    }

    fn rank_of(conn: &Connection, id: ItemId) -> u32 {
        match get_item(conn, id).unwrap().unwrap().detail {
            ItemDetail::Movie(m) => m.top250,
            ItemDetail::Show(s) => s.top250,
            _ => panic!("not ranked"),
        }
    }

    fn chart(entries: &[(&str, u32)]) -> Vec<(String, u32)> {
        entries
            .iter()
            .map(|(id, rank)| ((*id).to_owned(), *rank))
            .collect()
    }

    #[test]
    fn titles_on_the_chart_get_their_position_and_others_are_cleared() {
        let conn = memory_db();
        let heat = movie(&conn, "Heat", "tt0113277", 0);
        let drive = movie(&conn, "Drive", "tt0780504", 0);
        let old = movie(&conn, "Old favourite", "tt0000111", 17);

        let report = apply_top250(
            &conn,
            ItemKind::Movie,
            &chart(&[("tt0113277", 1), ("tt0780504", 2), ("tt9999999", 3)]),
        )
        .unwrap();

        assert_eq!(rank_of(&conn, heat), 1);
        assert_eq!(rank_of(&conn, drive), 2);
        assert_eq!(rank_of(&conn, old), 0, "no longer on the chart");
        assert_eq!(
            report,
            Top250Report {
                ranked: 2,
                changed: 2,
                cleared: 1,
                locked: 0,
                not_in_library: 1,
            }
        );
    }

    #[test]
    fn a_second_identical_chart_changes_nothing() {
        let conn = memory_db();
        let heat = movie(&conn, "Heat", "tt0113277", 0);
        let chart = chart(&[("tt0113277", 5)]);
        apply_top250(&conn, ItemKind::Movie, &chart).unwrap();
        let report = apply_top250(&conn, ItemKind::Movie, &chart).unwrap();
        assert_eq!((report.ranked, report.changed, report.cleared), (1, 0, 0));
        assert_eq!(rank_of(&conn, heat), 5);
    }

    #[test]
    fn a_locked_item_or_field_keeps_its_position() {
        let conn = memory_db();
        let locked_item = movie(&conn, "Locked", "tt0000001", 9);
        let locked_field = movie(&conn, "Field", "tt0000002", 8);
        let free = movie(&conn, "Free", "tt0000003", 0);
        set_item_locked(&conn, &[locked_item], true).unwrap();
        set_field_lock(&conn, locked_field, MetaField::Top250, true).unwrap();

        let report = apply_top250(
            &conn,
            ItemKind::Movie,
            &chart(&[("tt0000001", 1), ("tt0000002", 2), ("tt0000003", 3)]),
        )
        .unwrap();

        assert_eq!(rank_of(&conn, locked_item), 9);
        assert_eq!(rank_of(&conn, locked_field), 8);
        assert_eq!(rank_of(&conn, free), 3);
        assert_eq!((report.locked, report.ranked), (2, 1));
    }

    #[test]
    fn a_locked_ranked_item_missing_from_the_chart_is_not_cleared() {
        let conn = memory_db();
        let kept = movie(&conn, "Kept", "tt0000001", 9);
        let other = movie(&conn, "Other", "tt0000002", 0);
        set_item_locked(&conn, &[kept], true).unwrap();
        let report = apply_top250(&conn, ItemKind::Movie, &chart(&[("tt0000002", 1)])).unwrap();
        assert_eq!(rank_of(&conn, kept), 9);
        assert_eq!(rank_of(&conn, other), 1);
        assert_eq!((report.cleared, report.locked), (0, 1));
    }

    #[test]
    fn movies_and_shows_are_ranked_separately() {
        let conn = memory_db();
        let film = movie(&conn, "Film", "tt0000001", 4);
        let mut show = Item::show("Series");
        show.core.ids.set(id_key::IMDB, "tt0000002");
        let show = insert_item(&conn, &show).unwrap();

        apply_top250(&conn, ItemKind::Show, &chart(&[("tt0000002", 6)])).unwrap();
        assert_eq!(rank_of(&conn, show), 6);
        assert_eq!(rank_of(&conn, film), 4, "the movie chart was not involved");

        // A show's ID in the movie chart does not rank the show.
        apply_top250(&conn, ItemKind::Movie, &chart(&[("tt0000002", 1)])).unwrap();
        assert_eq!(rank_of(&conn, show), 6);
        assert_eq!(rank_of(&conn, film), 0);
    }

    #[test]
    fn an_empty_chart_and_other_kinds_are_refused() {
        let conn = memory_db();
        let id = movie(&conn, "Heat", "tt0113277", 3);
        assert!(apply_top250(&conn, ItemKind::Movie, &[]).is_err());
        assert!(apply_top250(&conn, ItemKind::Set, &chart(&[("tt0113277", 1)])).is_err());
        assert_eq!(rank_of(&conn, id), 3, "nothing was cleared");
    }
}
