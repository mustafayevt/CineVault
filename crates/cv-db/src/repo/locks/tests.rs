//! Tests of item locks, field locks and the scrape write path.

use cv_domain::{
    Credit, CreditKind, FieldSet, Genre, Item, ItemDetail, ItemId, MediaFileType, MetaField,
    Rating, ScrapeBlock, ScrapeOptions,
};
use rusqlite::Connection;

use super::*;
use crate::repo::{get_item, insert_item};

fn memory_db() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.pragma_update(None, "foreign_keys", true).unwrap();
    crate::schema::migrate(&mut conn).unwrap();
    conn
}

fn stored(conn: &Connection) -> ItemId {
    let mut item = Item::movie("My title");
    item.core.plot = "My own plot".into();
    item.core.year = 1999;
    if let ItemDetail::Movie(m) = &mut item.detail {
        m.genres = vec![Genre::Drama];
    }
    insert_item(conn, &item).unwrap()
}

fn scraped() -> Item {
    let mut item = Item::movie("Scraped title");
    item.core.plot = "Scraped plot".into();
    item.core.year = 2001;
    item.core.ratings.insert(Rating::new("imdb", 8.0, 99, 10));
    item.core
        .artwork_urls
        .insert(MediaFileType::Fanart, "https://new/fanart".into());
    item.core
        .credits
        .push(Credit::new(CreditKind::Actor, "A. Actor", "Hero"));
    if let ItemDetail::Movie(m) = &mut item.detail {
        m.genres = vec![Genre::Comedy];
    }
    item
}

#[test]
fn field_locks_are_stored_per_item_and_can_be_toggled() {
    let conn = memory_db();
    let a = stored(&conn);
    let b = stored(&conn);
    assert!(set_field_lock(&conn, a, MetaField::Plot, true).unwrap());
    assert!(!set_field_lock(&conn, a, MetaField::Plot, true).unwrap());
    assert!(set_field_lock(&conn, a, MetaField::Genres, true).unwrap());
    assert_eq!(field_locks(&conn, a).unwrap().to_keys(), "plot,genres");
    assert!(field_locks(&conn, b).unwrap().is_empty());
    assert!(set_field_lock(&conn, a, MetaField::Plot, false).unwrap());
    assert_eq!(field_locks(&conn, a).unwrap().to_keys(), "genres");

    let many = field_locks_of(&conn, &[a, b]).unwrap();
    assert_eq!(many.len(), 1);

    let all: FieldSet = [MetaField::Title, MetaField::Tags].into_iter().collect();
    set_field_locks(&conn, a, &all).unwrap();
    assert_eq!(field_locks(&conn, a).unwrap(), all);
    assert!(matches!(
        set_field_lock(&conn, ItemId(9999), MetaField::Plot, true),
        Err(DbError::ItemNotFound(9999))
    ));
}

#[test]
fn unknown_keys_from_a_newer_version_are_kept_and_ignored() {
    let conn = memory_db();
    let id = stored(&conn);
    conn.execute(
        "INSERT INTO field_lock (item_id, field) VALUES (?1, 'from_the_future')",
        [id.0],
    )
    .unwrap();
    assert!(field_locks(&conn, id).unwrap().is_empty());
    set_field_locks(&conn, id, &FieldSet::new()).unwrap();
    let kept: i64 = conn
        .query_row("SELECT COUNT(*) FROM field_lock", [], |r| r.get(0))
        .unwrap();
    assert_eq!(kept, 1);
}

#[test]
fn locks_go_away_with_their_item() {
    let conn = memory_db();
    let id = stored(&conn);
    set_field_lock(&conn, id, MetaField::Plot, true).unwrap();
    crate::repo::delete_item(&conn, id).unwrap();
    let left: i64 = conn
        .query_row("SELECT COUNT(*) FROM field_lock", [], |r| r.get(0))
        .unwrap();
    assert_eq!(left, 0);
}

#[test]
fn a_rescrape_updates_every_selected_field_except_a_locked_one() {
    let conn = memory_db();
    let id = stored(&conn);
    set_field_lock(&conn, id, MetaField::Plot, true).unwrap();
    let outcome = apply_scrape(
        &conn,
        id,
        &scraped(),
        &ScrapeOptions::all(cv_domain::ItemKind::Movie),
    )
    .unwrap();
    assert_eq!(outcome.locked, [MetaField::Plot]);

    let item = get_item(&conn, id).unwrap().unwrap();
    assert_eq!(item.core.plot, "My own plot");
    assert_eq!(item.core.title, "Scraped title");
    assert_eq!(item.core.year, 2001);
    assert_eq!(item.core.ratings.get("imdb").map(|r| r.votes), Some(99));
    assert_eq!(item.core.credits.len(), 1);
    assert!(matches!(&item.detail, ItemDetail::Movie(m) if m.genres == [Genre::Comedy]));
}

#[test]
fn a_locked_genre_field_keeps_the_users_genres_after_a_rescrape() {
    let conn = memory_db();
    let id = stored(&conn);
    set_field_lock(&conn, id, MetaField::Genres, true).unwrap();
    apply_scrape(
        &conn,
        id,
        &scraped(),
        &ScrapeOptions::all(cv_domain::ItemKind::Movie),
    )
    .unwrap();
    let item = get_item(&conn, id).unwrap().unwrap();
    assert!(matches!(&item.detail, ItemDetail::Movie(m) if m.genres == [Genre::Drama]));
    assert_eq!(item.core.plot, "Scraped plot");
}

#[test]
fn a_locked_item_is_not_written_at_all() {
    let conn = memory_db();
    let id = stored(&conn);
    assert_eq!(
        set_item_locked(&conn, &[id, ItemId(9999)], true).unwrap(),
        1
    );
    assert_eq!(set_item_locked(&conn, &[id], true).unwrap(), 0);
    let before = get_item(&conn, id).unwrap().unwrap();
    assert!(before.core.locked);
    let outcome = apply_scrape(
        &conn,
        id,
        &scraped(),
        &ScrapeOptions::all(cv_domain::ItemKind::Movie),
    )
    .unwrap();
    assert_eq!(outcome.blocked, Some(ScrapeBlock::ItemLocked));
    assert_eq!(get_item(&conn, id).unwrap().unwrap(), before);

    set_item_locked(&conn, &[id], false).unwrap();
    apply_scrape(
        &conn,
        id,
        &scraped(),
        &ScrapeOptions::all(cv_domain::ItemKind::Movie),
    )
    .unwrap();
    assert_eq!(
        get_item(&conn, id).unwrap().unwrap().core.title,
        "Scraped title"
    );
}

#[test]
fn a_scrape_stores_the_original_language_even_when_no_field_is_selected() {
    let conn = memory_db();
    let id = stored(&conn);
    let mut scraped = scraped();
    scraped.core.original_language = "fr".into();

    let outcome = apply_scrape(&conn, id, &scraped, &ScrapeOptions::only([])).unwrap();

    assert!(outcome.original_language_set && !outcome.changed());
    let item = get_item(&conn, id).unwrap().unwrap();
    assert_eq!(item.core.original_language, "fr");
    assert_eq!(item.core.title, "My title", "no field was selected");
    // A scrape that does not know the language leaves the stored one alone.
    scraped.core.original_language.clear();
    apply_scrape(
        &conn,
        id,
        &scraped,
        &ScrapeOptions::all(cv_domain::ItemKind::Movie),
    )
    .unwrap();
    assert_eq!(
        get_item(&conn, id).unwrap().unwrap().core.original_language,
        "fr"
    );
}

#[test]
fn only_the_selected_fields_and_do_not_overwrite_apply_through_the_database() {
    let conn = memory_db();
    let id = stored(&conn);
    let only = ScrapeOptions::only([MetaField::Rating, MetaField::Artwork(MediaFileType::Fanart)]);
    apply_scrape(&conn, id, &scraped(), &only).unwrap();
    let item = get_item(&conn, id).unwrap().unwrap();
    assert_eq!(item.core.title, "My title");
    assert_eq!(item.core.plot, "My own plot");
    assert_eq!(item.core.ratings.get("imdb").map(|r| r.votes), Some(99));
    assert!(item.core.artwork_urls.contains_key(&MediaFileType::Fanart));

    let fill_only = ScrapeOptions {
        do_not_overwrite: true,
        ..ScrapeOptions::all(cv_domain::ItemKind::Movie)
    };
    apply_scrape(&conn, id, &scraped(), &fill_only).unwrap();
    let item = get_item(&conn, id).unwrap().unwrap();
    assert_eq!(item.core.title, "My title");
    assert_eq!(item.core.credits.len(), 1, "the empty cast was filled");
}
