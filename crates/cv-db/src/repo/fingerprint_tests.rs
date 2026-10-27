//! Tests of the fingerprint table and the library snapshot.

use cv_domain::{
    Availability, EpisodeNumber, Item, ItemDetail, ItemId, ItemKind, MediaFile, MediaFileType,
    SourceKind,
};
use rusqlite::Connection;

use super::data_sources::{insert_source, remap_source, remove_source};
use super::fingerprint::*;
use super::{insert_item, set_availability};

fn memory_db() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.pragma_update(None, "foreign_keys", true).unwrap();
    crate::schema::migrate(&mut conn).unwrap();
    conn
}

fn fp(path: &str, size: u64, modified_ms: i64, file_id: &str) -> Fingerprint {
    Fingerprint {
        path: path.into(),
        size_bytes: size,
        modified_ms,
        file_id: file_id.into(),
    }
}

#[test]
fn fingerprints_round_trip_and_replace_by_path_ignoring_case() {
    let conn = memory_db();
    upsert_fingerprints(
        &conn,
        "D:\\Movies",
        &[
            fp("D:\\Movies\\A\\a.mkv", 10, 1, "7:1"),
            fp("D:\\Movies\\B\\b.mkv", 20, 2, ""),
        ],
        100,
    )
    .unwrap();
    assert_eq!(count_fingerprints(&conn).unwrap(), 2);

    // The same file spelled in another case is the same row.
    upsert_fingerprints(
        &conn,
        "D:\\Movies",
        &[fp("d:\\movies\\a\\A.MKV", 11, 5, "7:1")],
        200,
    )
    .unwrap();
    let listed = list_fingerprints(&conn, "d:\\movies").unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].size_bytes, 11);
    assert_eq!(listed[0].modified_ms, 5);

    assert!(list_fingerprints(&conn, "E:\\Other").unwrap().is_empty());
    assert_eq!(
        delete_fingerprints(&conn, &["D:\\MOVIES\\B\\B.mkv".into(), "nope".into()]).unwrap(),
        1
    );
    assert_eq!(count_fingerprints(&conn).unwrap(), 1);
    assert_eq!(
        delete_fingerprints_of_source(&conn, "D:\\Movies").unwrap(),
        1
    );
    assert_eq!(count_fingerprints(&conn).unwrap(), 0);
}

#[test]
fn same_as_compares_size_time_and_a_known_file_id() {
    let a = fp("p", 10, 1, "7:1");
    assert!(a.same_as(&fp("p", 10, 1, "7:1")));
    assert!(!a.same_as(&fp("p", 11, 1, "7:1")));
    assert!(!a.same_as(&fp("p", 10, 2, "7:1")));
    // A different identity is a replaced file, even with the same size and time.
    assert!(!a.same_as(&fp("p", 10, 1, "7:2")));
    // An unknown identity never counts as a difference.
    assert!(a.same_as(&fp("p", 10, 1, "")));
    assert!(fp("p", 10, 1, "").same_as(&a));
}

#[test]
fn removing_a_source_drops_its_fingerprints_and_keeps_the_items() {
    let conn = memory_db();
    let source = insert_source(&conn, SourceKind::Movie, "D:\\Movies").unwrap();
    let other = insert_source(&conn, SourceKind::Movie, "E:\\Films").unwrap();
    upsert_fingerprints(&conn, "D:\\Movies", &[fp("D:\\Movies\\a.mkv", 1, 1, "")], 1).unwrap();
    upsert_fingerprints(&conn, "E:\\Films", &[fp("E:\\Films\\b.mkv", 1, 1, "")], 1).unwrap();
    let mut item = Item::movie("A");
    item.core.data_source = "D:\\Movies".into();
    insert_item(&conn, &item).unwrap();

    remove_source(&conn, source.id).unwrap();

    assert_eq!(count_fingerprints(&conn).unwrap(), 1);
    assert_eq!(list_fingerprints(&conn, &other.path).unwrap().len(), 1);
    let items: i64 = conn
        .query_row("SELECT COUNT(*) FROM item", [], |r| r.get(0))
        .unwrap();
    assert_eq!(items, 1);
}

#[test]
fn remapping_a_source_moves_its_fingerprints_with_it() {
    let conn = memory_db();
    let source = insert_source(&conn, SourceKind::Movie, "F:\\Movies").unwrap();
    insert_source(&conn, SourceKind::Movie, "F:\\Movies2").unwrap();
    upsert_fingerprints(
        &conn,
        "F:\\Movies",
        &[fp("F:\\Movies\\Alien\\alien.mkv", 5, 9, "")],
        1,
    )
    .unwrap();
    // Same text prefix, other folder: must stay.
    upsert_fingerprints(
        &conn,
        "F:\\Movies2",
        &[fp("F:\\Movies2\\Dune\\dune.mkv", 5, 9, "")],
        1,
    )
    .unwrap();

    let report = remap_source(&conn, source.id, "\\\\nas\\Movies")
        .unwrap()
        .unwrap();

    assert_eq!(report.fingerprints, 1);
    let moved = list_fingerprints(&conn, "\\\\nas\\Movies").unwrap();
    assert_eq!(moved.len(), 1);
    assert_eq!(moved[0].path, "\\\\nas\\Movies\\Alien\\alien.mkv");
    assert!(list_fingerprints(&conn, "F:\\Movies").unwrap().is_empty());
    let untouched = list_fingerprints(&conn, "F:\\Movies2").unwrap();
    assert_eq!(untouched[0].path, "F:\\Movies2\\Dune\\dune.mkv");
}

fn file(kind: MediaFileType, dir: &str, name: &str, size: u64, container: &str) -> MediaFile {
    let mut f = MediaFile::new(kind, dir, name);
    f.size_bytes = size;
    f.modified_ms = 42;
    f.container_format = container.into();
    f
}

#[test]
fn the_snapshot_lists_items_with_files_numbers_and_availability() {
    let conn = memory_db();
    let mut show = Item::show("Andor");
    show.core.data_source = "D:\\Shows".into();
    show.core.path = "D:\\Shows\\Andor".into();
    let show_id = insert_item(&conn, &show).unwrap();

    let mut season = Item::season(show_id, 1);
    season.core.data_source = "D:\\Shows".into();
    let season_id = insert_item(&conn, &season).unwrap();

    let mut episode = Item::episode(show_id, "Kassa");
    episode.core.data_source = "D:\\SHOWS".into();
    episode.core.path = "D:\\Shows\\Andor\\Season 01".into();
    if let ItemDetail::Episode(e) = &mut episode.detail {
        e.numbers.set(EpisodeNumber::aired(1, 2));
    }
    episode.core.media_files.push(file(
        MediaFileType::Video,
        "D:\\Shows\\Andor\\Season 01",
        "andor.s01e02.mkv",
        99,
        "Matroska",
    ));
    episode.core.media_files.push(file(
        MediaFileType::Nfo,
        "D:\\Shows\\Andor\\Season 01",
        "andor.s01e02.nfo",
        3,
        "",
    ));
    let episode_id = insert_item(&conn, &episode).unwrap();
    set_availability(&conn, episode_id, Availability::Missing, "gone").unwrap();

    // Another root and a set are not part of this snapshot.
    let mut elsewhere = Item::movie("Elsewhere");
    elsewhere.core.data_source = "E:\\Films".into();
    insert_item(&conn, &elsewhere).unwrap();
    let mut set = Item::set("Collection");
    set.core.data_source = "D:\\Shows".into();
    insert_item(&conn, &set).unwrap();

    let snapshot = library_snapshot(&conn, "D:\\Shows").unwrap();
    let kinds: Vec<(ItemId, ItemKind)> = snapshot.iter().map(|i| (i.id, i.kind)).collect();
    assert_eq!(
        kinds,
        [
            (show_id, ItemKind::Show),
            (season_id, ItemKind::Season),
            (episode_id, ItemKind::Episode)
        ]
    );
    assert_eq!(snapshot[1].season_number, Some(1));
    let ep = &snapshot[2];
    assert_eq!(ep.aired, Some((1, 2)));
    assert_eq!(ep.parent_id, Some(show_id));
    assert_eq!(ep.availability, Availability::Missing);
    assert_eq!(ep.files.len(), 2);
    assert_eq!(ep.files[0].kind, MediaFileType::Video);
    assert_eq!(ep.files[0].size_bytes, 99);
    assert!(ep.files[0].has_media_info);
    assert!(!ep.files[1].has_media_info);
}
