//! Which items a batch match job works on.
//!
//! The batch modes of the matcher select movies and shows only (seasons and episodes follow
//! their show, and sets are not matched online):
//!
//! - **new**: items the scanner flagged NEW;
//! - **unmatched**: items that no provider has scraped yet (`last_scraper_id` is empty);
//! - **selection**: the items the user picked, for a rescrape or a forced best match.
//!
//! Items the user ignored ("not a catalogued title") are left out of the first two. A selection
//! is explicit, so it keeps them.
//!
//! These functions only read; they never change an item.

use cv_domain::{ItemId, ItemKind};
use rusqlite::Connection;

use super::codec::parse_enum;
use crate::error::DbResult;

/// What a batch needs to know about an item to plan work for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchTargetRow {
    pub id: ItemId,
    /// `Movie` or `Show`.
    pub kind: ItemKind,
    pub title: String,
    pub year: u16,
    /// The item's folder.
    pub path: String,
    /// The provider that scraped the item last, empty when none did.
    pub last_scraper_id: String,
    pub locked: bool,
}

/// Which automatic batch set to list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchScope {
    /// Items flagged NEW by a scan.
    New,
    /// Items no provider has scraped.
    Unmatched,
}

const COLUMNS: &str = "id, kind, title, year, path, last_scraper_id, locked";

fn row_of(r: &rusqlite::Row<'_>) -> DbResult<MatchTargetRow> {
    let kind: String = r.get(1)?;
    Ok(MatchTargetRow {
        id: ItemId(r.get(0)?),
        kind: parse_enum(&kind, "item kind")?,
        title: r.get(2)?,
        year: u16::try_from(r.get::<_, i64>(3)?).unwrap_or(0),
        path: r.get(4)?,
        last_scraper_id: r.get(5)?,
        locked: r.get(6)?,
    })
}

fn collect(mut rows: rusqlite::Rows<'_>) -> DbResult<Vec<MatchTargetRow>> {
    let mut out = Vec::new();
    while let Some(r) = rows.next()? {
        out.push(row_of(r)?);
    }
    Ok(out)
}

/// The movies and shows of `scope`, in id order, without dismissed items.
pub fn match_targets(conn: &Connection, scope: MatchScope) -> DbResult<Vec<MatchTargetRow>> {
    let condition = match scope {
        MatchScope::New => "is_new = 1",
        MatchScope::Unmatched => "last_scraper_id = ''",
    };
    let sql = format!(
        "SELECT {COLUMNS} FROM item
          WHERE kind IN ('movie', 'show') AND trashed_entry IS NULL AND {condition}
            AND NOT EXISTS (SELECT 1 FROM review_queue q
                             WHERE q.item_id = item.id AND q.state = 'dismissed')
          ORDER BY id"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    collect(stmt.query([])?)
}

/// The movies and shows among `ids`, in id order. Ids of other kinds and unknown ids are left
/// out; dismissed items are kept.
pub fn match_targets_of(conn: &Connection, ids: &[ItemId]) -> DbResult<Vec<MatchTargetRow>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let wanted = serde_json::to_string(&ids.iter().map(|id| id.0).collect::<Vec<_>>())?;
    let sql = format!(
        "SELECT {COLUMNS} FROM item
          WHERE kind IN ('movie', 'show') AND id IN (SELECT value FROM json_each(?1))
          ORDER BY id"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    collect(stmt.query([&wanted])?)
}

#[cfg(test)]
mod tests {
    use cv_domain::Item;

    use super::*;
    use crate::Database;
    use crate::repo::flags::set_new;
    use crate::repo::items::{insert_item, update_item};
    use crate::repo::review;

    fn db() -> (tempfile::TempDir, Database) {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(dir.path().join("library.db")).unwrap();
        (dir, db)
    }

    fn add(db: &Database, item: Item) -> ItemId {
        db.write(move |tx| insert_item(tx, &item)).unwrap()
    }

    #[test]
    fn new_and_unmatched_select_movies_and_shows_only() {
        let (_dir, db) = db();
        let movie = add(&db, Item::movie("Solaris"));
        let show = add(&db, Item::show("Dark"));
        let set = add(&db, Item::set("Saga"));
        let mut scraped = Item::movie("Dune");
        scraped.core.last_scraper_id = "tmdb".to_owned();
        let scraped = add(&db, scraped);
        db.write(move |tx| set_new(tx, &[movie, set, scraped], true))
            .unwrap();

        let new = db
            .read(|c| match_targets(c, MatchScope::New))
            .unwrap()
            .into_iter()
            .map(|r| r.id)
            .collect::<Vec<_>>();
        assert_eq!(new, vec![movie, scraped]);

        let unmatched = db
            .read(|c| match_targets(c, MatchScope::Unmatched))
            .unwrap()
            .into_iter()
            .map(|r| r.id)
            .collect::<Vec<_>>();
        assert_eq!(unmatched, vec![movie, show]);
    }

    #[test]
    fn dismissed_items_are_left_out_of_the_automatic_sets() {
        let (_dir, db) = db();
        let home_video = add(&db, Item::movie("Holiday 2019"));
        let film = add(&db, Item::movie("Solaris"));
        db.write(move |tx| review::dismiss(tx, home_video)).unwrap();
        let unmatched = db
            .read(|c| match_targets(c, MatchScope::Unmatched))
            .unwrap();
        assert_eq!(unmatched.len(), 1);
        assert_eq!(unmatched[0].id, film);
        // An explicit selection keeps it.
        let picked = db
            .read(|c| match_targets_of(c, &[home_video, film]))
            .unwrap();
        assert_eq!(picked.len(), 2);
    }

    #[test]
    fn a_selection_keeps_movies_and_shows_and_skips_the_rest() {
        let (_dir, db) = db();
        let mut item = Item::movie("Solaris");
        item.core.year = 1972;
        item.core.locked = false;
        let movie = add(&db, item);
        let set = add(&db, Item::set("Saga"));
        db.write(move |tx| {
            let mut m = stored(tx, movie);
            m.core.last_scraper_id = "tmdb".to_owned();
            m.core.locked = true;
            update_item(tx, &m)
        })
        .unwrap();
        let rows = db
            .read(|c| match_targets_of(c, &[set, movie, ItemId(9999)]))
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, ItemKind::Movie);
        assert_eq!(rows[0].year, 1972);
        assert_eq!(rows[0].last_scraper_id, "tmdb");
        assert!(rows[0].locked);
        assert!(db.read(|c| match_targets_of(c, &[])).unwrap().is_empty());
    }

    fn stored(conn: &Connection, id: ItemId) -> Item {
        crate::repo::items::get_item(conn, id).unwrap().unwrap()
    }
}
