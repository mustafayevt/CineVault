//! The preferred rating: which rating the "rating" column, its sort and the quick-search filter
//! use, the fallback to the next available source, and the fetch date stored with each rating.

use cv_domain::{Item, ItemId, ItemKind, Rating, RatingPreference};
use rusqlite::Connection;

use super::table_facts::table_facts_for;
use super::*;

fn memory_db() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.pragma_update(None, "foreign_keys", true).unwrap();
    crate::schema::migrate(&mut conn).unwrap();
    conn
}

fn movie(conn: &Connection, title: &str, ratings: &[(&str, f32, u32)]) -> ItemId {
    let mut item = Item::movie(title);
    for (source, value, max) in ratings {
        item.core
            .ratings
            .insert(Rating::new(*source, *value, 1000, *max));
    }
    insert_item(conn, &item).unwrap()
}

/// A has both sources, B only TMDB, C both, D nothing but the user's own rating.
fn library(conn: &Connection) -> [ItemId; 4] {
    [
        movie(conn, "A", &[("imdb", 9.0, 10), ("tmdb", 5.0, 10)]),
        movie(conn, "B", &[("tmdb", 8.0, 10)]),
        movie(conn, "C", &[("imdb", 7.0, 10), ("tmdb", 9.0, 10)]),
        movie(conn, "D", &[("user", 10.0, 10)]),
    ]
}

fn titles_sorted(conn: &Connection, preference: &str, direction: Direction) -> Vec<String> {
    let query = ItemQuery::new(ItemKind::Movie)
        .rating_preference(RatingPreference::new(preference))
        .sort_by(SortKey {
            field: SortField::Rating,
            direction,
        })
        .sort_by(SortKey::asc(SortField::Title));
    list_items_at(conn, &query, 0, 10)
        .unwrap()
        .into_iter()
        .map(|i| i.title)
        .collect()
}

#[test]
fn sorting_by_rating_uses_the_preferred_source_and_falls_back_to_the_next() {
    let conn = memory_db();
    library(&conn);
    // IMDb preferred: A 9 (imdb), B 8 (tmdb, fallback), C 7 (imdb); D has no source rating.
    assert_eq!(
        titles_sorted(&conn, "imdb", Direction::Desc),
        ["A", "B", "C", "D"]
    );
    // TMDB preferred: C 9, B 8, A 5.
    assert_eq!(
        titles_sorted(&conn, "tmdb", Direction::Desc),
        ["C", "B", "A", "D"]
    );
    // Items without a rating sort last in both directions.
    assert_eq!(
        titles_sorted(&conn, "tmdb", Direction::Asc),
        ["A", "B", "C", "D"]
    );
}

#[test]
fn the_user_rating_is_never_the_rating_column() {
    let conn = memory_db();
    let [.., d] = library(&conn);
    let facts = table_facts_for(&conn, &[d], &RatingPreference::default()).unwrap();
    assert_eq!(facts[&d].rating, None);
    assert_eq!(facts[&d].user_rating, Some(10.0));
}

#[test]
fn the_page_pager_and_the_window_agree_under_a_preference() {
    let conn = memory_db();
    library(&conn);
    for preference in ["imdb", "tmdb", "letterboxd"] {
        let query = ItemQuery::new(ItemKind::Movie)
            .rating_preference(RatingPreference::new(preference))
            .sort_by(SortKey::desc(SortField::Rating))
            .sort_by(SortKey::asc(SortField::Title));
        let mut by_cursor = Vec::new();
        let mut cursor = None;
        loop {
            let page = list_items(&conn, &query, cursor.as_ref(), 1).unwrap();
            by_cursor.extend(page.items.iter().map(|i| i.id));
            match page.next {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }
        let window: Vec<ItemId> = list_items_at(&conn, &query, 0, 10)
            .unwrap()
            .iter()
            .map(|i| i.id)
            .collect();
        assert_eq!(by_cursor.len(), 4, "{preference}");
        assert_eq!(by_cursor, window, "{preference}");
    }
}

#[test]
fn votes_follow_the_same_source_as_the_rating() {
    let conn = memory_db();
    let a = movie(&conn, "A", &[]);
    let mut item = get_item(&conn, a).unwrap().unwrap();
    item.core.ratings.insert(Rating::new("imdb", 8.0, 500, 10));
    item.core.ratings.insert(Rating::new("tmdb", 7.0, 90, 10));
    update_item(&conn, &item).unwrap();
    let facts = |preference: &str| {
        table_facts_for(&conn, &[a], &RatingPreference::new(preference)).unwrap()[&a].clone()
    };
    assert_eq!(
        (facts("imdb").votes, facts("imdb").rating),
        (500, Some(8.0))
    );
    assert_eq!((facts("tmdb").votes, facts("tmdb").rating), (90, Some(7.0)));
}

#[test]
fn the_row_marks_a_rating_from_another_source_as_a_fallback() {
    let conn = memory_db();
    let [a, b, ..] = library(&conn);
    let facts = table_facts_for(&conn, &[a, b], &RatingPreference::new("imdb")).unwrap();
    // A has IMDb: shown as is.
    assert_eq!(facts[&a].rating_source.as_deref(), Some("imdb"));
    assert!(!facts[&a].rating_is_fallback);
    // B has no IMDb rating: TMDB stands in, and the row says so.
    assert_eq!(facts[&b].rating, Some(8.0));
    assert_eq!(facts[&b].rating_source.as_deref(), Some("tmdb"));
    assert!(facts[&b].rating_is_fallback);
    // The same item is not a fallback when TMDB is the preference.
    let facts = table_facts_for(&conn, &[b], &RatingPreference::new("tmdb")).unwrap();
    assert!(!facts[&b].rating_is_fallback);
}

fn matching(conn: &Connection, filter: Filter) -> Vec<String> {
    let query = ItemQuery::new(ItemKind::Movie)
        .filter(filter)
        .sort_by(SortKey::asc(SortField::Title));
    list_items_at(conn, &query, 0, 10)
        .unwrap()
        .into_iter()
        .map(|i| i.title)
        .collect()
}

#[test]
fn the_rating_filter_tests_the_preferred_rating_with_fallback() {
    let conn = memory_db();
    library(&conn);
    let at_least = |preference: &str, min: f64| Filter::PreferredRating {
        preference: RatingPreference::new(preference),
        min: Some(min),
        max: None,
    };
    // IMDb preferred: A 9, B 8 (TMDB fallback), C 7.
    assert_eq!(matching(&conn, at_least("imdb", 8.0)), ["A", "B"]);
    // TMDB preferred: A 5, B 8, C 9.
    assert_eq!(matching(&conn, at_least("tmdb", 8.0)), ["B", "C"]);
    // Both bounds.
    let range = Filter::PreferredRating {
        preference: RatingPreference::new("tmdb"),
        min: Some(5.0),
        max: Some(8.0),
    };
    assert_eq!(matching(&conn, range), ["A", "B"]);
    // Negated, it keeps everything else, including items with no rating at all.
    assert_eq!(
        matching(&conn, Filter::not(at_least("imdb", 8.0))),
        ["C", "D"]
    );
}

#[test]
fn a_rating_is_stamped_with_the_time_it_was_stored_and_keeps_it() {
    let conn = memory_db();
    let before = super::ratings::now_ms();
    let id = movie(&conn, "Heat", &[("imdb", 8.0, 10)]);
    let after = super::ratings::now_ms();
    let first = get_ratings(&conn, id).unwrap().get("imdb").unwrap().clone();
    assert!(
        (before..=after).contains(&first.fetched_at_ms),
        "{} not in {before}..={after}",
        first.fetched_at_ms
    );

    // Updating something else does not change the date of a rating that did not change.
    let mut item = get_item(&conn, id).unwrap().unwrap();
    item.core.plot = "A heist.".into();
    update_item(&conn, &item).unwrap();
    let again = get_ratings(&conn, id).unwrap().get("imdb").unwrap().clone();
    assert_eq!(again.fetched_at_ms, first.fetched_at_ms);
}

#[test]
fn an_explicit_fetch_date_is_stored_and_a_refetch_replaces_it() {
    let conn = memory_db();
    let id = movie(&conn, "Heat", &[]);
    upsert_ratings(
        &conn,
        id,
        &[Rating::new("letterboxd", 4.4, 1200, 5).fetched_at(1_700_000_000_000)],
    )
    .unwrap();
    let stored = get_ratings(&conn, id).unwrap();
    assert_eq!(
        stored.get("letterboxd").unwrap().fetched_at_ms,
        1_700_000_000_000
    );

    // The same value fetched again is a new fetch.
    let before = super::ratings::now_ms();
    upsert_ratings(&conn, id, &[Rating::new("letterboxd", 4.4, 1200, 5)]).unwrap();
    let stored = get_ratings(&conn, id).unwrap();
    assert!(stored.get("letterboxd").unwrap().fetched_at_ms >= before);
}
