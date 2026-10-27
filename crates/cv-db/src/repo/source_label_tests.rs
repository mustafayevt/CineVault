//! Source as a filter, a sort and a group key.

use cv_domain::{Item, SourceKind};
use rusqlite::Connection;

use super::data_sources::{insert_source, set_source_label};
use super::{Filter, ItemQuery, SortField, SortKey, insert_item, list_items, source_groups};

fn memory_db() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.pragma_update(None, "foreign_keys", true).unwrap();
    crate::schema::migrate(&mut conn).unwrap();
    conn
}

fn movie(conn: &Connection, title: &str, root: &str) {
    let mut item = Item::movie(title);
    item.core.data_source = root.into();
    item.core.path = format!("{root}\\{title}");
    insert_item(conn, &item).unwrap();
}

/// Three movies on "NAS 4K", one on "Archive", one whose source was removed.
fn library() -> Connection {
    let conn = memory_db();
    let nas = insert_source(&conn, SourceKind::Movie, r"D:\Movies").unwrap();
    set_source_label(&conn, nas.id, "NAS 4K").unwrap();
    insert_source(&conn, SourceKind::Movie, r"E:\Archive").unwrap();
    // A show on a TV source with the same label text is not mixed up with the movies.
    let tv = insert_source(&conn, SourceKind::Tv, r"D:\Movies").unwrap();
    set_source_label(&conn, tv.id, "NAS 4K").unwrap();
    for title in ["Heat", "Alien", "Brazil"] {
        movie(&conn, title, r"D:\Movies");
    }
    movie(&conn, "Zodiac", r"E:\Archive");
    movie(&conn, "Orphan", r"X:\Gone");
    conn
}

fn titles(conn: &Connection, query: &ItemQuery) -> Vec<String> {
    list_items(conn, query, None, 50)
        .unwrap()
        .items
        .into_iter()
        .map(|i| i.title)
        .collect()
}

#[test]
fn sorting_by_source_orders_by_label_then_the_rest() {
    let conn = library();
    let query = ItemQuery::new(cv_domain::ItemKind::Movie)
        .sort_by(SortKey::asc(SortField::Source))
        .sort_by(SortKey::asc(SortField::Title));
    // The removed source has an empty label and sorts first.
    assert_eq!(
        titles(&conn, &query),
        ["Orphan", "Zodiac", "Alien", "Brazil", "Heat"]
    );
    let page = list_items(&conn, &query, None, 50).unwrap();
    let labels: Vec<&str> = page.items.iter().map(|i| i.source_label.as_str()).collect();
    assert_eq!(labels, ["", "Archive", "NAS 4K", "NAS 4K", "NAS 4K"]);
}

#[test]
fn paging_a_source_sort_with_a_cursor_agrees_with_the_full_order() {
    let conn = library();
    let query = ItemQuery::new(cv_domain::ItemKind::Movie)
        .sort_by(SortKey::asc(SortField::Source))
        .sort_by(SortKey::asc(SortField::Title));
    let mut seen = Vec::new();
    let mut after = None;
    loop {
        let page = list_items(&conn, &query, after.as_ref(), 2).unwrap();
        seen.extend(page.items.into_iter().map(|i| i.title));
        match page.next {
            Some(next) => after = Some(next),
            None => break,
        }
    }
    assert_eq!(seen, titles(&conn, &query));
}

#[test]
fn the_source_filter_matches_labels_ignoring_case() {
    let conn = library();
    let query = ItemQuery::new(cv_domain::ItemKind::Movie).filter(Filter::SourceLabel {
        values: vec!["nas 4k".into()],
    });
    assert_eq!(titles(&conn, &query), ["Alien", "Brazil", "Heat"]);
    let none =
        ItemQuery::new(cv_domain::ItemKind::Movie).filter(Filter::SourceLabel { values: vec![] });
    assert!(titles(&conn, &none).is_empty());
}

#[test]
fn groups_by_source_count_the_matching_titles() {
    let conn = library();
    let groups = source_groups(&conn, &ItemQuery::new(cv_domain::ItemKind::Movie)).unwrap();
    let pairs: Vec<(&str, u64)> = groups.iter().map(|g| (g.label.as_str(), g.count)).collect();
    assert_eq!(pairs, [("", 1), ("Archive", 1), ("NAS 4K", 3)]);

    let filtered = ItemQuery::new(cv_domain::ItemKind::Movie).filter(Filter::TitleContains {
        column: super::filter::TitleColumn::Title,
        terms: vec!["r".into()],
    });
    let groups = source_groups(&conn, &filtered).unwrap();
    // Only the titles that contain an "r" are counted.
    let total: u64 = groups.iter().map(|g| g.count).sum();
    assert_eq!(total, titles(&conn, &filtered).len() as u64);
}

#[test]
fn relabelling_moves_the_titles_to_the_new_group_without_changing_them() {
    let conn = library();
    let before: Vec<String> = titles(&conn, &ItemQuery::new(cv_domain::ItemKind::Movie));
    let nas = super::data_sources::list_sources(&conn, Some(SourceKind::Movie))
        .unwrap()
        .into_iter()
        .find(|s| s.label == "NAS 4K")
        .unwrap();
    set_source_label(&conn, nas.id, "Zed").unwrap();
    let groups = source_groups(&conn, &ItemQuery::new(cv_domain::ItemKind::Movie)).unwrap();
    assert_eq!(groups.last().map(|g| g.label.as_str()), Some("Zed"));
    assert_eq!(
        titles(&conn, &ItemQuery::new(cv_domain::ItemKind::Movie)),
        before
    );
}
