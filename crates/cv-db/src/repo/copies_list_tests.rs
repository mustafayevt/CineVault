//! The Copies filter of the library list: only the given movies, each film's copies together in
//! their rank order, the groups in the chosen sort; and the group sizes on every movie row.

use cv_domain::{Item, ItemId, ItemKind, MediaFile, MediaFileType, id_key};
use rusqlite::Connection;

use super::copies::keep_set;
use super::{
    CopyMember, Filter, ItemQuery, SortField, SortKey, count_items, get_summary, insert_item,
    list_items, list_items_at,
};

fn memory_db() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.pragma_update(None, "foreign_keys", true).unwrap();
    crate::schema::migrate(&mut conn).unwrap();
    conn
}

fn movie(conn: &Connection, title: &str, year: u16, tmdb: &str, root: &str) -> ItemId {
    let mut item = Item::movie(title);
    item.core.year = year;
    item.core.data_source = root.into();
    item.core.path = format!("{root}/{title} {tmdb}");
    item.core.ids.set(id_key::TMDB, tmdb);
    item.core.media_files.push(MediaFile::new(
        MediaFileType::Video,
        item.core.path.clone(),
        format!("{title}.mkv"),
    ));
    insert_item(conn, &item).unwrap()
}

fn member(id: ItemId, group: ItemId, rank: u32) -> CopyMember {
    CopyMember { id, group, rank }
}

/// Example Movie twice (D: and E:), Another Example three times, and a film with no copy.
struct Library {
    conn: Connection,
    example: [ItemId; 2],
    another: [ItemId; 3],
    members: Vec<CopyMember>,
}

fn library() -> Library {
    let conn = memory_db();
    let example = [
        movie(&conn, "Example Movie", 2020, "1", "D:/Movies"),
        movie(&conn, "Example Movie", 2020, "1", "E:/Films"),
    ];
    let another = [
        movie(&conn, "Another Example", 2018, "2", "D:/Movies"),
        movie(&conn, "Another Example", 2018, "2", "E:/Films"),
        movie(&conn, "Another Example", 2018, "2", "D:/Movies"),
    ];
    movie(&conn, "Alone", 2001, "3", "D:/Movies");
    // The second Example copy is the best; of Another Example the third, then the first.
    let members = vec![
        member(example[0], example[0], 1),
        member(example[1], example[0], 0),
        member(another[0], another[0], 1),
        member(another[1], another[0], 2),
        member(another[2], another[0], 0),
    ];
    Library {
        conn,
        example,
        another,
        members,
    }
}

fn ids(conn: &Connection, query: &ItemQuery) -> Vec<ItemId> {
    list_items(conn, query, None, 50)
        .unwrap()
        .items
        .into_iter()
        .map(|i| i.id)
        .collect()
}

#[test]
fn copies_are_listed_together_best_first_and_groups_follow_the_sort() {
    let lib = library();
    let by_title = ItemQuery::new(ItemKind::Movie)
        .sort_by(SortKey::asc(SortField::Title))
        .copies(lib.members.clone());

    assert_eq!(count_items(&lib.conn, &by_title).unwrap(), 5);
    assert_eq!(
        ids(&lib.conn, &by_title),
        vec![
            lib.another[2],
            lib.another[0],
            lib.another[1],
            lib.example[1],
            lib.example[0],
        ]
    );

    let by_year_desc = ItemQuery::new(ItemKind::Movie)
        .sort_by(SortKey::desc(SortField::Year))
        .copies(lib.members.clone());
    assert_eq!(
        ids(&lib.conn, &by_year_desc),
        vec![
            lib.example[1],
            lib.example[0],
            lib.another[2],
            lib.another[0],
            lib.another[1],
        ]
    );
}

#[test]
fn pages_and_windows_keep_the_copies_order() {
    let lib = library();
    let query = ItemQuery::new(ItemKind::Movie).copies(lib.members.clone());
    let all = ids(&lib.conn, &query);

    let first = list_items(&lib.conn, &query, None, 2).unwrap();
    let rest = list_items(&lib.conn, &query, first.next.as_ref(), 10).unwrap();
    let paged: Vec<ItemId> = first
        .items
        .iter()
        .chain(&rest.items)
        .map(|i| i.id)
        .collect();
    assert_eq!(paged, all);

    let window: Vec<ItemId> = list_items_at(&lib.conn, &query, 3, 2)
        .unwrap()
        .into_iter()
        .map(|i| i.id)
        .collect();
    assert_eq!(window, all[3..].to_vec());
}

#[test]
fn the_copies_filter_combines_with_the_drive_and_the_search() {
    let lib = library();
    let on_e = ItemQuery::new(ItemKind::Movie)
        .filter(Filter::DataSource {
            values: vec!["E:/Films".into()],
        })
        .copies(lib.members.clone());
    assert_eq!(ids(&lib.conn, &on_e), vec![lib.another[1], lib.example[1]]);

    let searched = ItemQuery::new(ItemKind::Movie)
        .filter(Filter::Search {
            text: "another".into(),
        })
        .copies(lib.members.clone());
    assert_eq!(
        ids(&lib.conn, &searched),
        vec![lib.another[2], lib.another[0], lib.another[1]]
    );
}

#[test]
fn no_open_copies_lists_nothing() {
    let lib = library();
    let query = ItemQuery::new(ItemKind::Movie).copies(Vec::new());

    assert_eq!(count_items(&lib.conn, &query).unwrap(), 0);
    assert!(ids(&lib.conn, &query).is_empty());
}

#[test]
fn every_movie_row_carries_its_group_size() {
    let lib = library();
    keep_set(&lib.conn, &lib.example, 1).unwrap();

    let rows = list_items(&lib.conn, &ItemQuery::new(ItemKind::Movie), None, 50)
        .unwrap()
        .items;
    let sizes = |id: ItemId| {
        let row = rows.iter().find(|r| r.id == id).unwrap();
        (row.copies, row.copies_total)
    };
    // A kept group shows no badge but still has its copies.
    assert_eq!(sizes(lib.example[0]), (0, 2));
    assert_eq!(sizes(lib.another[1]), (3, 3));
    let alone = rows.iter().find(|r| r.title == "Alone").unwrap();
    assert_eq!((alone.copies, alone.copies_total), (0, 0));

    let one = get_summary(&lib.conn, lib.another[0]).unwrap().unwrap();
    assert_eq!((one.copies, one.copies_total), (3, 3));
}
