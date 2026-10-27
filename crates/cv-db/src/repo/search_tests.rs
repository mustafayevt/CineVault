//! Tests of the forgiving quick search: what is found and in which order.

use cv_domain::{Credit, CreditKind, Item, ItemKind};
use rusqlite::Connection;

use super::filter::Filter;
use super::items::insert_item;
use super::query::{ItemQuery, count_items, list_items};

fn memory_db() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.pragma_update(None, "foreign_keys", true).unwrap();
    crate::schema::migrate(&mut conn).unwrap();
    conn
}

fn movie(title: &str, year: u16) -> Item {
    let mut m = Item::movie(title);
    m.core.year = year;
    m
}

fn library(titles: &[(&str, u16)]) -> Connection {
    let conn = memory_db();
    for (title, year) in titles {
        insert_item(&conn, &movie(title, *year)).unwrap();
    }
    conn
}

/// The titles a quick search lists, in the order the library shows them (relevance first).
fn search(conn: &Connection, text: &str) -> Vec<String> {
    let query = ItemQuery::new(ItemKind::Movie)
        .filter(Filter::Search { text: text.into() })
        .title_first(text);
    let page = list_items(conn, &query, None, 1000).unwrap();
    assert_eq!(
        usize::try_from(count_items(conn, &query).unwrap()).unwrap(),
        page.items.len(),
        "count agrees with the listing for {text:?}"
    );
    page.items.into_iter().map(|i| i.title).collect()
}

fn finds(conn: &Connection, text: &str, title: &str) {
    let found = search(conn, text);
    assert!(
        found.iter().any(|t| t == title),
        "{text:?} should find {title:?}, found {found:?}"
    );
}

#[test]
fn punctuation_and_spacing_do_not_matter() {
    let conn = library(&[
        ("Spider-Man", 2002),
        ("WALL\u{b7}E", 2008),
        ("Mission: Impossible", 1996),
        ("X-Men", 2000),
        ("Se7en", 1995),
        ("Schindler's List", 1993),
        ("Alien", 1979),
    ]);
    for text in [
        "spider man",
        "spiderman",
        "Spider-Man",
        "SPIDER  MAN",
        "spider-m",
    ] {
        finds(&conn, text, "Spider-Man");
    }
    for text in ["wall e", "walle", "WALL\u{b7}E", "wall-e", "wall"] {
        finds(&conn, text, "WALL\u{b7}E");
    }
    for text in [
        "Mission: Impossible",
        "mission impossible",
        "missionimpossible",
    ] {
        finds(&conn, text, "Mission: Impossible");
    }
    for text in ["x-men", "x men", "xmen"] {
        finds(&conn, text, "X-Men");
    }
    finds(&conn, "se7en", "Se7en");
    finds(&conn, "schindlers list", "Schindler's List");
    finds(&conn, "schindler's list", "Schindler's List");
    finds(&conn, "schindler list", "Schindler's List");
    assert_eq!(search(&conn, "spiderman"), ["Spider-Man"]);
}

#[test]
fn diacritics_do_not_matter() {
    let conn = library(&[("Amélie", 2001), ("Léon", 1994), ("Alien", 1979)]);
    assert_eq!(search(&conn, "amelie"), ["Amélie"]);
    assert_eq!(search(&conn, "leon"), ["Léon"]);
    assert_eq!(search(&conn, "AMÉLIE"), ["Amélie"]);
    assert_eq!(search(&conn, "léon"), ["Léon"]);
}

#[test]
fn and_and_ampersand_are_the_same_and_numerals_are_interchangeable() {
    let conn = library(&[
        ("Pride & Prejudice", 2005),
        ("Rocky II", 1979),
        ("Rocky", 1976),
        ("Sense and Sensibility", 1995),
    ]);
    finds(&conn, "pride and prejudice", "Pride & Prejudice");
    finds(&conn, "pride & prejudice", "Pride & Prejudice");
    finds(&conn, "sense & sensibility", "Sense and Sensibility");
    finds(&conn, "rocky 2", "Rocky II");
    finds(&conn, "rocky ii", "Rocky II");
}

#[test]
fn the_year_narrows_the_search() {
    let conn = library(&[("Dune", 2021), ("Dune", 1984), ("Dune: Part Two", 2024)]);
    let query = ItemQuery::new(ItemKind::Movie).filter(Filter::Search {
        text: "dune 2021".into(),
    });
    let years: Vec<u32> = list_items(&conn, &query, None, 10)
        .unwrap()
        .items
        .into_iter()
        .map(|i| u32::from(i.year))
        .collect();
    assert_eq!(years, [2021]);
    assert!(search(&conn, "dune 1999").is_empty());
}

#[test]
fn original_titles_and_people_are_searched() {
    let conn = memory_db();
    let mut leon = movie("The Professional", 1994);
    leon.core.original_title = "Léon".into();
    insert_item(&conn, &leon).unwrap();
    let mut dune = movie("Dune", 2021);
    dune.core
        .credits
        .push(Credit::new(CreditKind::Director, "Denis Villeneuve", ""));
    insert_item(&conn, &dune).unwrap();
    assert_eq!(search(&conn, "leon"), ["The Professional"]);
    assert_eq!(search(&conn, "villeneuve"), ["Dune"]);
    assert_eq!(search(&conn, "denis villen"), ["Dune"]);
}

#[test]
fn a_small_typo_is_found_only_when_nothing_matches() {
    let conn = library(&[
        ("Interstellar", 2014),
        ("Inception", 2010),
        ("Gladiator", 2000),
    ]);
    assert_eq!(search(&conn, "interstelar"), ["Interstellar"]);
    assert_eq!(search(&conn, "gladiatro"), ["Gladiator"]);
    assert_eq!(search(&conn, "incepton"), ["Inception"]);
    // an exact hit exists, so the near misses are not offered
    let conn = library(&[("Heat", 1995), ("Heart", 2001), ("Hear", 2002)]);
    assert_eq!(search(&conn, "heat"), ["Heat"]);
    // not a typo of anything
    assert!(search(&conn, "zzzzzz").is_empty());
}

#[test]
fn results_are_ranked_exact_prefix_word_alternative_people_then_text() {
    let conn = memory_db();
    // found by the plot only
    let mut by_plot = movie("Zzz Sleeper", 2020);
    by_plot.core.plot = "A story about a heat wave.".into();
    insert_item(&conn, &by_plot).unwrap();
    // found by a director
    let mut by_person = movie("Aaa Director Cut", 2019);
    by_person
        .core
        .credits
        .push(Credit::new(CreditKind::Director, "Heat Person", ""));
    insert_item(&conn, &by_person).unwrap();
    // found by the original title
    let mut by_original = movie("Bbb Original", 2018);
    by_original.core.original_title = "Heat Original".into();
    insert_item(&conn, &by_original).unwrap();
    // a word of the title, a prefix of the title, the whole title
    insert_item(&conn, &movie("The Last Heat Wave", 2017)).unwrap();
    insert_item(&conn, &movie("Heat Wave", 2016)).unwrap();
    insert_item(&conn, &movie("Heat", 1995)).unwrap();

    assert_eq!(
        search(&conn, "heat"),
        [
            "Heat",
            "Heat Wave",
            "The Last Heat Wave",
            "Bbb Original",
            "Aaa Director Cut",
            "Zzz Sleeper"
        ]
    );
}

#[test]
fn hostile_and_empty_input_stays_harmless() {
    let conn = library(&[("Alien", 1979)]);
    assert!(search(&conn, "x') OR 1=1 --").is_empty());
    assert_eq!(search(&conn, "\"alien\""), ["Alien"]);
    assert_eq!(
        search(&conn, "  ").len(),
        1,
        "a blank search does not restrict"
    );
    assert_eq!(
        search(&conn, "-").len(),
        1,
        "punctuation alone does not restrict"
    );
}

#[test]
fn titles_edited_later_are_searched_by_their_new_name() {
    let conn = memory_db();
    let id = insert_item(&conn, &movie("Old Name", 2000)).unwrap();
    let mut item = movie("Brand New", 2000);
    item.id = Some(id);
    super::items::update_item(&conn, &item).unwrap();
    assert!(search(&conn, "old name").is_empty());
    assert_eq!(search(&conn, "brandnew"), ["Brand New"]);
}

#[test]
fn existing_libraries_get_their_keys_from_migration_101() {
    // A 1.0.0 library: schema 100, titles already stored, no search keys yet.
    let mut conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(crate::schema::BASE_SCHEMA).unwrap();
    conn.pragma_update(None, "user_version", crate::schema::BASE_VERSION)
        .unwrap();
    conn.execute(
        "INSERT INTO item (kind, title, original_title, year)
         VALUES ('movie', 'Amélie', 'Le Fabuleux Destin d''Amélie Poulain', 2001)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO item (kind, title, year) VALUES ('movie', 'Spider-Man', 2002)",
        [],
    )
    .unwrap();
    // the full-text rows of a 1.0.0 library
    conn.execute(
        "INSERT INTO item_fts (rowid, title, original_title)
         SELECT id, title, original_title FROM item",
        [],
    )
    .unwrap();

    let before = crate::schema::migrate(&mut conn).unwrap();

    assert_eq!(before, crate::schema::BASE_VERSION);
    assert_eq!(search(&conn, "amelie"), ["Amélie"]);
    assert_eq!(search(&conn, "destin poulain"), ["Amélie"]);
    assert_eq!(search(&conn, "spiderman"), ["Spider-Man"]);
}
