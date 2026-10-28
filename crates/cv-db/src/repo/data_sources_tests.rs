//! Tests of the data-source rows: order, removal and remapping. The library rows must survive
//! every one of them.

use cv_domain::{Item, ItemId, MediaFile, MediaFileType, SourceKind};
use rusqlite::{Connection, params};

use super::data_sources::*;
use super::{get_item, insert_item};

fn memory_db() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.pragma_update(None, "foreign_keys", true).unwrap();
    crate::schema::migrate(&mut conn).unwrap();
    conn
}

fn paths(conn: &Connection, kind: SourceKind) -> Vec<String> {
    list_sources(conn, Some(kind))
        .unwrap()
        .into_iter()
        .map(|s| s.path)
        .collect()
}

fn movie(conn: &Connection, source: &str, folder: &str) -> ItemId {
    let mut item = Item::movie(folder);
    item.core.data_source = source.into();
    item.core.path = format!("{}\\{folder}", source.trim_end_matches('\\'));
    item.core.media_files.push(MediaFile::new(
        MediaFileType::Video,
        item.core.path.clone(),
        "movie.mkv",
    ));
    insert_item(conn, &item).unwrap()
}

fn item_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT COUNT(*) FROM item", [], |r| r.get(0))
        .unwrap()
}

fn path_of(conn: &Connection, id: ItemId) -> (String, String, String) {
    let item = get_item(conn, id).unwrap().unwrap();
    (
        item.core.data_source,
        item.core.path,
        item.core.media_files[0].directory.clone(),
    )
}

#[test]
fn sources_keep_their_order_per_kind() {
    let conn = memory_db();
    let a = insert_source(&conn, SourceKind::Movie, "D:\\Movies").unwrap();
    let b = insert_source(&conn, SourceKind::Movie, "E:\\Films").unwrap();
    let tv = insert_source(&conn, SourceKind::Tv, "D:\\Shows").unwrap();
    assert_eq!((a.position, b.position, tv.position), (0, 1, 0));
    assert_eq!(paths(&conn, SourceKind::Movie), ["D:\\Movies", "E:\\Films"]);
    assert_eq!(paths(&conn, SourceKind::Tv), ["D:\\Shows"]);
    // Movies list before TV when both kinds are asked for.
    let all: Vec<_> = list_sources(&conn, None)
        .unwrap()
        .into_iter()
        .map(|s| s.path)
        .collect();
    assert_eq!(all, ["D:\\Movies", "E:\\Films", "D:\\Shows"]);
}

#[test]
fn the_same_path_cannot_be_stored_twice_for_a_kind() {
    let conn = memory_db();
    insert_source(&conn, SourceKind::Movie, "D:\\Movies").unwrap();
    assert!(insert_source(&conn, SourceKind::Movie, "d:\\movies").is_err());
    // The other library may hold it.
    assert!(insert_source(&conn, SourceKind::Tv, "D:\\Movies").is_ok());
}

#[test]
fn reorder_applies_a_full_permutation() {
    let conn = memory_db();
    let a = insert_source(&conn, SourceKind::Movie, "D:\\A").unwrap().id;
    let b = insert_source(&conn, SourceKind::Movie, "D:\\B").unwrap().id;
    let c = insert_source(&conn, SourceKind::Movie, "D:\\C").unwrap().id;
    reorder_sources(&conn, SourceKind::Movie, &[c, a, b]).unwrap();
    assert_eq!(paths(&conn, SourceKind::Movie), ["D:\\C", "D:\\A", "D:\\B"]);
}

#[test]
fn reorder_refuses_a_list_that_is_not_every_source_once() {
    let conn = memory_db();
    let a = insert_source(&conn, SourceKind::Movie, "D:\\A").unwrap().id;
    let b = insert_source(&conn, SourceKind::Movie, "D:\\B").unwrap().id;
    let tv = insert_source(&conn, SourceKind::Tv, "D:\\T").unwrap().id;
    for bad in [vec![a], vec![a, a], vec![a, b, tv], vec![a, b, 999], vec![]] {
        assert!(
            reorder_sources(&conn, SourceKind::Movie, &bad).is_err(),
            "{bad:?}"
        );
    }
    assert_eq!(paths(&conn, SourceKind::Movie), ["D:\\A", "D:\\B"]);
}

#[test]
fn removing_a_source_closes_the_gap_and_keeps_every_item() {
    let conn = memory_db();
    let a = insert_source(&conn, SourceKind::Movie, "D:\\A").unwrap();
    let b = insert_source(&conn, SourceKind::Movie, "D:\\B").unwrap();
    let c = insert_source(&conn, SourceKind::Movie, "D:\\C").unwrap();
    let kept = movie(&conn, "D:\\B", "Alien (1979)");
    movie(&conn, "D:\\B", "Heat (1995)");
    movie(&conn, "D:\\A", "Dune (2021)");

    // A show with two episodes is one title.
    let mut show = Item::show("Chernobyl");
    show.core.data_source = "D:\\B".into();
    let show = insert_item(&conn, &show).unwrap();
    for _ in 0..2 {
        let mut episode = Item::episode(show, "Episode");
        episode.core.data_source = "D:\\B".into();
        insert_item(&conn, &episode).unwrap();
    }
    let before = item_count(&conn);

    let removed = remove_source(&conn, b.id).unwrap().unwrap();
    assert_eq!(removed.source.path, "D:\\B");
    assert_eq!(removed.kept_items, 3);

    assert_eq!(item_count(&conn), before, "no library row may be deleted");
    assert_eq!(path_of(&conn, kept).0, "D:\\B");
    let rest = list_sources(&conn, Some(SourceKind::Movie)).unwrap();
    assert_eq!(
        rest.iter().map(|s| (s.id, s.position)).collect::<Vec<_>>(),
        [(a.id, 0), (c.id, 1)]
    );
    assert_eq!(remove_source(&conn, b.id).unwrap(), None);
}

#[test]
fn remap_repoints_items_files_and_journal_without_deleting_anything() {
    let conn = memory_db();
    let source = insert_source(&conn, SourceKind::Movie, "F:\\Movies").unwrap();
    let alien = movie(&conn, "F:\\Movies", "Alien (1979)");
    let heat = movie(&conn, "F:\\Movies", "Heat (1995)");
    // Same text prefix, different folder: must not move.
    let sibling = movie(&conn, "F:\\Movies2", "Dune (2021)");
    let other = insert_source(&conn, SourceKind::Movie, "F:\\Movies2").unwrap();
    conn.execute(
        "INSERT INTO rename_journal (batch_id, seq, op, from_path, to_path, state) VALUES \
         ('b1', 1, 'move', 'F:\\Movies\\alien', 'F:\\Movies\\Alien (1979)', 'done'), \
         ('b1', 2, 'move', 'F:\\Movies2\\dune', 'F:\\Movies2\\Dune (2021)', 'done')",
        [],
    )
    .unwrap();
    let before = item_count(&conn);

    let report = remap_source(&conn, source.id, "\\\\nas\\Movies")
        .unwrap()
        .unwrap();

    assert_eq!(report.old_path, "F:\\Movies");
    assert_eq!(report.source.path, "\\\\nas\\Movies");
    assert_eq!(report.items, 2);
    assert_eq!(report.files, 2);
    assert_eq!(report.journal_paths, 2);
    assert_eq!(item_count(&conn), before);
    assert_eq!(
        path_of(&conn, alien),
        (
            "\\\\nas\\Movies".into(),
            "\\\\nas\\Movies\\Alien (1979)".into(),
            "\\\\nas\\Movies\\Alien (1979)".into()
        )
    );
    assert_eq!(path_of(&conn, heat).1, "\\\\nas\\Movies\\Heat (1995)");
    assert_eq!(
        path_of(&conn, sibling),
        (
            "F:\\Movies2".into(),
            "F:\\Movies2\\Dune (2021)".into(),
            "F:\\Movies2\\Dune (2021)".into()
        )
    );
    assert_eq!(
        get_source(&conn, other.id).unwrap().unwrap().path,
        "F:\\Movies2"
    );
    let journal: Vec<(String, String)> = conn
        .prepare("SELECT from_path, to_path FROM rename_journal ORDER BY seq")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(
        journal,
        [
            (
                "\\\\nas\\Movies\\alien".into(),
                "\\\\nas\\Movies\\Alien (1979)".into()
            ),
            (
                "F:\\Movies2\\dune".into(),
                "F:\\Movies2\\Dune (2021)".into()
            ),
        ]
    );
}

#[test]
fn remap_leaves_metadata_untouched() {
    let conn = memory_db();
    let source = insert_source(&conn, SourceKind::Movie, "D:\\Movies").unwrap();
    let id = movie(&conn, "D:\\Movies", "Alien (1979)");
    conn.execute(
        "UPDATE item SET plot = 'In space', watched = 1, updated_ms = 5 WHERE id = ?1",
        [id.0],
    )
    .unwrap();

    remap_source(&conn, source.id, "E:\\Films")
        .unwrap()
        .unwrap();

    let (plot, watched, updated): (String, i64, i64) = conn
        .query_row(
            "SELECT plot, watched, updated_ms FROM item WHERE id = ?1",
            [id.0],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!((plot.as_str(), watched, updated), ("In space", 1, 5));
}

#[test]
fn remap_matches_the_old_root_with_the_platforms_case_rule() {
    let conn = memory_db();
    let source = insert_source(&conn, SourceKind::Movie, "D:\\Movies").unwrap();
    // An item written with different letter case belongs to the source on Windows and macOS;
    // on Linux that is another folder and the remap leaves it alone.
    let id = movie(&conn, "d:\\movies", "Alien (1979)");
    let report = remap_source(&conn, source.id, "E:\\Films")
        .unwrap()
        .unwrap();
    if cv_fs::paths::CASE_SENSITIVE {
        assert_eq!(report.items, 0);
        assert_eq!(path_of(&conn, id).1, "d:\\movies\\Alien (1979)");
    } else {
        assert_eq!(report.items, 1);
        assert_eq!(path_of(&conn, id).1, "E:\\Films\\Alien (1979)");
    }
}

#[test]
fn remap_between_path_styles_converts_the_separators() {
    let conn = memory_db();
    let source = insert_source(&conn, SourceKind::Movie, "D:\\Movies").unwrap();
    let id = movie(&conn, "D:\\Movies", "Alien (1979)");
    remap_source(&conn, source.id, "/Volumes/Movies")
        .unwrap()
        .unwrap();
    assert_eq!(
        path_of(&conn, id),
        (
            "/Volumes/Movies".into(),
            "/Volumes/Movies/Alien (1979)".into(),
            "/Volumes/Movies/Alien (1979)".into()
        )
    );
}

#[test]
fn remap_handles_drive_roots_and_leaves_empty_paths_alone() {
    let conn = memory_db();
    let source = insert_source(&conn, SourceKind::Movie, "E:\\").unwrap();
    let id = movie(&conn, "E:\\", "Alien (1979)");
    // An item without a path (a placeholder) must stay without one.
    let mut bare = Item::movie("Placeholder");
    bare.core.data_source = "E:\\".into();
    let bare_id = insert_item(&conn, &bare).unwrap();

    remap_source(&conn, source.id, "G:\\Films")
        .unwrap()
        .unwrap();

    assert_eq!(path_of(&conn, id).0, "G:\\Films");
    assert_eq!(path_of_core(&conn, id), "G:\\Films\\Alien (1979)");
    assert_eq!(path_of_core(&conn, bare_id), "");
}

fn path_of_core(conn: &Connection, id: ItemId) -> String {
    conn.query_row("SELECT path FROM item WHERE id = ?1", params![id.0], |r| {
        r.get(0)
    })
    .unwrap()
}

#[test]
fn remapping_an_unknown_source_does_nothing() {
    let conn = memory_db();
    assert_eq!(remap_source(&conn, 42, "D:\\Movies").unwrap(), None);
}

// ---- labels ----------------------------------------------------

fn label_list(conn: &Connection, kind: SourceKind) -> Vec<String> {
    list_sources(conn, Some(kind))
        .unwrap()
        .into_iter()
        .map(|s| s.label)
        .collect()
}

#[test]
fn a_new_source_is_labelled_with_its_folders_short_name() {
    let conn = memory_db();
    let a = insert_source(&conn, SourceKind::Movie, r"D:\Media\Movies 4K").unwrap();
    assert_eq!(a.label, "Movies 4K");
    let unix = insert_source(&conn, SourceKind::Tv, "/mnt/nas/Shows/").unwrap();
    assert_eq!(unix.label, "Shows");
}

#[test]
fn a_duplicate_short_name_gets_the_parent_folder_and_then_a_number() {
    let conn = memory_db();
    insert_source(&conn, SourceKind::Movie, r"D:\Media\Films").unwrap();
    let second = insert_source(&conn, SourceKind::Movie, r"E:\Media\Films").unwrap();
    assert_eq!(second.label, "Films (Media)");
    let third = insert_source(&conn, SourceKind::Movie, r"F:\Media\Films").unwrap();
    assert_eq!(third.label, "Films (Media) 2");
    // Another library type may reuse a label.
    let tv = insert_source(&conn, SourceKind::Tv, r"D:\Media\Films").unwrap();
    assert_eq!(tv.label, "Films");
}

#[test]
fn a_label_can_be_changed_but_not_to_one_another_source_of_the_type_uses() {
    let conn = memory_db();
    let a = insert_source(&conn, SourceKind::Movie, r"D:\Movies").unwrap();
    let b = insert_source(&conn, SourceKind::Movie, r"E:\Films").unwrap();
    let tv = insert_source(&conn, SourceKind::Tv, r"D:\Shows").unwrap();

    let renamed = set_source_label(&conn, a.id, "  NAS 4K ").unwrap();
    assert_eq!(renamed.label, "NAS 4K");
    // The same source keeps its own label, whatever the case.
    assert_eq!(
        set_source_label(&conn, a.id, "nas 4k").unwrap().label,
        "nas 4k"
    );

    let taken = set_source_label(&conn, b.id, "NAS 4K").unwrap_err();
    assert!(
        matches!(taken, crate::DbError::SourceLabelTaken { .. }),
        "{taken:?}"
    );
    assert_eq!(
        taken.to_string(),
        "Another movie source already uses this label"
    );
    assert_eq!(label_list(&conn, SourceKind::Movie), ["nas 4k", "Films"]);
    // The other library type is free to use it.
    assert_eq!(
        set_source_label(&conn, tv.id, "NAS 4K").unwrap().label,
        "NAS 4K"
    );

    assert!(matches!(
        set_source_label(&conn, b.id, "   "),
        Err(crate::DbError::SourceLabelEmpty)
    ));
    assert!(matches!(
        set_source_label(&conn, 999, "x"),
        Err(crate::DbError::SourceNotFound(999))
    ));
}

#[test]
fn relabelling_a_source_changes_no_item_and_no_path() {
    let conn = memory_db();
    let source = insert_source(&conn, SourceKind::Movie, r"D:\Movies").unwrap();
    let id = movie(&conn, r"D:\Movies", "Heat (1995)");
    let before = (path_of(&conn, id), item_count(&conn));
    let sources_before = paths(&conn, SourceKind::Movie);

    set_source_label(&conn, source.id, "Archive").unwrap();

    assert_eq!((path_of(&conn, id), item_count(&conn)), before);
    assert_eq!(paths(&conn, SourceKind::Movie), sources_before);
    let changed: i64 = conn
        .query_row("SELECT changes()", [], |r| r.get(0))
        .unwrap();
    assert_eq!(changed, 1, "only the label row was written");
}

#[test]
fn a_remapped_source_keeps_its_label() {
    let conn = memory_db();
    let source = insert_source(&conn, SourceKind::Movie, r"D:\Movies").unwrap();
    set_source_label(&conn, source.id, "NAS 4K").unwrap();
    remap_source(&conn, source.id, r"Z:\Movies").unwrap();
    assert_eq!(
        get_source(&conn, source.id).unwrap().unwrap().label,
        "NAS 4K"
    );
}

#[test]
fn a_source_changes_library_type_and_every_library_row_stays() {
    let conn = memory_db();
    let a = insert_source(&conn, SourceKind::Movie, "D:\\Library").unwrap();
    let b = insert_source(&conn, SourceKind::Movie, "E:\\Films").unwrap();
    let tv = insert_source(&conn, SourceKind::Tv, "D:\\Shows").unwrap();
    let heat = movie(&conn, "D:\\Library", "Heat (1995)");

    let moved = change_source_kind(&conn, a.id, SourceKind::Tv)
        .unwrap()
        .unwrap();
    assert_eq!((moved.kind, moved.position), (SourceKind::Tv, 1));
    assert_eq!(paths(&conn, SourceKind::Movie), ["E:\\Films"]);
    assert_eq!(paths(&conn, SourceKind::Tv), ["D:\\Shows", "D:\\Library"]);
    // The old library's order has no gap.
    assert_eq!(
        list_sources(&conn, Some(SourceKind::Movie)).unwrap()[0].position,
        0
    );
    assert_eq!((b.position, tv.position), (1, 0));
    // The title stays, with its files and its folder.
    assert_eq!(item_count(&conn), 1);
    assert_eq!(path_of(&conn, heat).0, "D:\\Library");

    // The same type again changes nothing; an unknown id is no source.
    let same = change_source_kind(&conn, a.id, SourceKind::Tv)
        .unwrap()
        .unwrap();
    assert_eq!(same.position, 1);
    assert!(
        change_source_kind(&conn, 404, SourceKind::Tv)
            .unwrap()
            .is_none()
    );
}

#[test]
fn a_changed_source_gets_a_new_label_when_the_other_library_has_its_label() {
    let conn = memory_db();
    let movies = insert_source(&conn, SourceKind::Movie, "D:\\Media").unwrap();
    let shows = insert_source(&conn, SourceKind::Tv, "E:\\Media").unwrap();
    assert_eq!(
        (movies.label.as_str(), shows.label.as_str()),
        ("Media", "Media")
    );
    let moved = change_source_kind(&conn, movies.id, SourceKind::Tv)
        .unwrap()
        .unwrap();
    assert_eq!(moved.label, "Media (D:)");
}
