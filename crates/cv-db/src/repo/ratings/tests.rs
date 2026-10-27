//! Tests of rating storage: several sources, scales, and the user rating that nothing overwrites.

use cv_domain::{Item, ItemId, ItemKind, MetaField, Rating, ScrapeOptions};
use rusqlite::Connection;

use super::*;
use crate::repo::{apply_scrape, get_item, insert_item, set_field_lock, update_item};

fn memory_db() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.pragma_update(None, "foreign_keys", true).unwrap();
    crate::schema::migrate(&mut conn).unwrap();
    conn
}

fn stored(conn: &Connection) -> ItemId {
    let mut item = Item::movie("Heat");
    item.core.ratings.insert(Rating::new("imdb", 8.3, 700, 10));
    insert_item(conn, &item).unwrap()
}

fn scraped() -> Item {
    let mut item = Item::movie("Heat");
    item.core.ratings.insert(Rating::new("imdb", 8.4, 900, 10));
    item.core.ratings.insert(Rating::new("tmdb", 7.9, 50, 10));
    // a scrape source must not be able to carry a user rating through
    item.core.ratings.insert(Rating::new("user", 1.0, 0, 10));
    item
}

#[test]
fn many_sources_are_stored_side_by_side_with_their_scales() {
    let conn = memory_db();
    let id = stored(&conn);
    let n = upsert_ratings(
        &conn,
        id,
        &[
            Rating::new("letterboxd", 4.4, 1200, 10),
            Rating::new("rogerebert", 3.5, 0, 10),
            Rating::new("tomatometerallcritics", 93.0, 300, 100),
            Rating::new("metacritic", 76.0, 40, 10),
            Rating::new("themoviedb", 7.9, 50, 10),
            Rating::new("trakt", -1.0, 0, 10),
        ],
    )
    .unwrap();
    assert_eq!(n, 5, "the invalid rating is skipped");

    let r = get_ratings(&conn, id).unwrap();
    assert_eq!(r.get("imdb").unwrap().votes, 700, "other sources stay");
    assert_eq!(r.get("letterboxd").unwrap().max_value, 5);
    assert_eq!(r.get("letterboxd").unwrap().value, 4.4);
    assert_eq!(r.get("rogerebert").unwrap().max_value, 4);
    assert_eq!(r.get("metacritic").unwrap().max_value, 100);
    assert_eq!(r.get("tomatometerallcritics").unwrap().max_value, 100);
    assert!(r.get("tmdb").is_some() && r.get("themoviedb").is_none());
    assert!(r.get("trakt").is_none());

    // an update replaces the row of that source only
    upsert_ratings(&conn, id, &[Rating::new("imdb", 8.5, 710, 10)]).unwrap();
    let r = get_ratings(&conn, id).unwrap();
    assert_eq!(r.get("imdb").unwrap().value, 8.5);
    assert_eq!(r.len(), 6);
    assert!(remove_rating(&conn, id, "letterboxd").unwrap());
    assert!(!remove_rating(&conn, id, "letterboxd").unwrap());
}

#[test]
fn the_user_rating_is_validated_set_and_cleared() {
    let conn = memory_db();
    let id = stored(&conn);
    assert_eq!(get_user_rating(&conn, id).unwrap(), None);
    set_user_rating(&conn, id, Some(9.0)).unwrap();
    assert_eq!(get_user_rating(&conn, id).unwrap(), Some(9.0));
    set_user_rating(&conn, id, Some(7.5)).unwrap();
    assert_eq!(get_user_rating(&conn, id).unwrap(), Some(7.5));

    for bad in [0.0, 0.25, 7.3, 10.5, -1.0, f32::NAN] {
        assert!(set_user_rating(&conn, id, Some(bad)).is_err(), "{bad}");
    }
    assert_eq!(get_user_rating(&conn, id).unwrap(), Some(7.5), "unchanged");

    set_user_rating(&conn, id, None).unwrap();
    assert_eq!(get_user_rating(&conn, id).unwrap(), None);
    assert!(set_user_rating(&conn, ItemId(9999), Some(5.0)).is_err());
    // it loads with the item
    set_user_rating(&conn, id, Some(10.0)).unwrap();
    let item = get_item(&conn, id).unwrap().unwrap();
    assert_eq!(item.core.ratings.user_rating(), Some(10.0));
}

#[test]
fn upsert_never_writes_the_user_rating() {
    let conn = memory_db();
    let id = stored(&conn);
    set_user_rating(&conn, id, Some(9.0)).unwrap();
    upsert_ratings(&conn, id, &[Rating::new("user", 2.0, 0, 10)]).unwrap();
    assert!(!remove_rating(&conn, id, "user").unwrap());
    assert_eq!(get_user_rating(&conn, id).unwrap(), Some(9.0));
}

#[test]
fn the_user_rating_survives_rescrape_locks_and_stale_updates() {
    let conn = memory_db();
    let id = stored(&conn);
    set_user_rating(&conn, id, Some(9.0)).unwrap();

    // a rescrape of everything, from a source that tries to carry a user rating
    let all = ScrapeOptions::all(ItemKind::Movie);
    let outcome = apply_scrape(&conn, id, &scraped(), &all).unwrap();
    assert!(outcome.changed());
    let r = get_ratings(&conn, id).unwrap();
    assert_eq!(r.user_rating(), Some(9.0));
    assert_eq!(r.get("imdb").unwrap().votes, 900);
    assert!(r.get("tmdb").is_some());

    // a rescrape of the rating field alone
    let only = ScrapeOptions::only([MetaField::Rating]);
    apply_scrape(&conn, id, &scraped(), &only).unwrap();
    assert_eq!(get_user_rating(&conn, id).unwrap(), Some(9.0));

    // a locked rating field: the source ratings stay, and so does the user rating
    set_field_lock(&conn, id, MetaField::Rating, true).unwrap();
    let mut other = scraped();
    other.core.ratings.insert(Rating::new("imdb", 1.0, 1, 10));
    apply_scrape(&conn, id, &other, &all).unwrap();
    let r = get_ratings(&conn, id).unwrap();
    assert_eq!(r.get("imdb").unwrap().votes, 900);
    assert_eq!(r.user_rating(), Some(9.0));

    // a stale copy of the item, read before the user rated, written back afterwards
    let mut stale = get_item(&conn, id).unwrap().unwrap();
    stale.core.ratings.clear_user_rating();
    stale
        .core
        .ratings
        .insert(Rating::new("imdb", 8.6, 1000, 10));
    update_item(&conn, &stale).unwrap();
    let r = get_ratings(&conn, id).unwrap();
    assert_eq!(r.user_rating(), Some(9.0));
    assert_eq!(r.get("imdb").unwrap().votes, 1000, "source ratings update");

    // removing a source rating through an update works; the user rating is untouched
    let mut edited = get_item(&conn, id).unwrap().unwrap();
    edited.core.ratings.remove("tmdb");
    update_item(&conn, &edited).unwrap();
    let r = get_ratings(&conn, id).unwrap();
    assert!(r.get("tmdb").is_none());
    assert_eq!(r.user_rating(), Some(9.0));
}

#[test]
fn a_new_item_keeps_the_user_rating_it_is_inserted_with() {
    let conn = memory_db();
    let mut item = Item::movie("Alien");
    item.core.ratings.set_user_rating(8.5).unwrap();
    let id = insert_item(&conn, &item).unwrap();
    assert_eq!(get_user_rating(&conn, id).unwrap(), Some(8.5));
}

#[test]
fn the_page_of_a_rating_is_stored_and_survives_a_write_without_one() {
    let conn = memory_db();
    let id = stored(&conn);
    let page = "https://www.rottentomatoes.com/m/heat";
    upsert_ratings(
        &conn,
        id,
        &[Rating::new("tomatometerallcritics", 87.0, 80, 100).with_url(page)],
    )
    .unwrap();
    assert_eq!(
        get_ratings(&conn, id)
            .unwrap()
            .get("tomatometerallcritics")
            .unwrap()
            .url,
        page
    );
    // A later fetch from a source that names no page keeps the known one.
    upsert_ratings(
        &conn,
        id,
        &[Rating::new("tomatometerallcritics", 88.0, 90, 100)],
    )
    .unwrap();
    let rt = get_ratings(&conn, id)
        .unwrap()
        .get("tomatometerallcritics")
        .unwrap()
        .clone();
    assert_eq!((rt.value, rt.url.as_str()), (88.0, page));

    // A scrape that merges ratings into the stored item keeps it too, and so does a full save.
    apply_scrape(
        &conn,
        id,
        &scraped(),
        &ScrapeOptions::only([MetaField::Rating]),
    )
    .unwrap();
    let item = get_item(&conn, id).unwrap().unwrap();
    assert_eq!(
        item.core.ratings.get("tomatometerallcritics").unwrap().url,
        page
    );
    update_item(&conn, &item).unwrap();
    assert_eq!(
        get_ratings(&conn, id)
            .unwrap()
            .get("tomatometerallcritics")
            .unwrap()
            .url,
        page
    );
    // An address that is not a web page is never stored.
    let odd = Rating::new("letterboxd", 4.0, 1, 5).with_url("file:///C:/x");
    assert_eq!(odd.url, "");
}
