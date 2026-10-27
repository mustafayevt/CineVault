//! Availability states are computed from the file system and never delete
//! anything. In particular, when a data source path is unreachable, every row is kept.

use std::fs;
use std::path::Path;

use cv_db::repo::{
    self, FileProbe, ItemQuery, PathStyle, StdFileProbe, get_availability, set_availability,
};
use cv_db::schema::TABLES;
use cv_db::{Database, DbError};
use cv_domain::{Availability, Item, ItemId, ItemKind, MediaFile, MediaFileType};

fn path_str(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn video(dir: &Path, name: &str) -> MediaFile {
    MediaFile {
        kind: MediaFileType::Video,
        directory: path_str(dir),
        filename: name.into(),
        ..MediaFile::default()
    }
}

/// Creates the folder and video file on disk and returns the stored movie.
fn make_movie(source: &Path, title: &str) -> Item {
    let dir = source.join(title);
    fs::create_dir_all(&dir).unwrap();
    let file = format!("{title}.mkv");
    fs::write(dir.join(&file), b"video").unwrap();
    let mut item = Item::movie(title);
    item.core.data_source = path_str(source);
    item.core.path = path_str(&dir);
    item.core.media_files.push(video(&dir, &file));
    item
}

struct Library {
    movies: Vec<ItemId>,
    show: ItemId,
    episodes: Vec<ItemId>,
}

fn populate(db: &Database, movies_root: &Path, shows_root: &Path) -> Library {
    let movie_items: Vec<Item> = (0..25)
        .map(|n| make_movie(movies_root, &format!("Movie {n:02} (2020)")))
        .collect();

    let show_dir = shows_root.join("Some Show");
    fs::create_dir_all(&show_dir).unwrap();
    let mut show = Item::show("Some Show");
    show.core.data_source = path_str(shows_root);
    show.core.path = path_str(&show_dir);

    let shows_root = shows_root.to_path_buf();
    db.write(move |tx| {
        let movies = movie_items
            .iter()
            .map(|m| repo::insert_item(tx, m))
            .collect::<Result<Vec<_>, _>>()?;
        let show_id = repo::insert_item(tx, &show)?;
        let mut episodes = Vec::new();
        for n in 1..=3 {
            let name = format!("Some Show S01E0{n}.mkv");
            fs::write(shows_root.join("Some Show").join(&name), b"video").unwrap();
            let mut ep = Item::episode(show_id, format!("Episode {n}"));
            ep.core.data_source = path_str(&shows_root);
            // The episode has no folder of its own: it is located through its video file.
            ep.core
                .media_files
                .push(video(&shows_root.join("Some Show"), &name));
            episodes.push(repo::insert_item(tx, &ep)?);
        }
        Ok(Library {
            movies,
            show: show_id,
            episodes,
        })
    })
    .unwrap()
}

fn row_counts(db: &Database) -> Vec<(&'static str, i64)> {
    db.read(|conn| {
        TABLES
            .iter()
            .map(|t| {
                let n: i64 =
                    conn.query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| r.get(0))?;
                Ok((*t, n))
            })
            .collect()
    })
    .unwrap()
}

fn all_items(db: &Database, lib: &Library) -> Vec<Item> {
    let ids = lib
        .movies
        .iter()
        .chain([&lib.show])
        .chain(&lib.episodes)
        .copied();
    db.read(|conn| {
        ids.map(|id| Ok(repo::get_item(conn, id)?.expect("row present")))
            .collect()
    })
    .unwrap()
}

fn state(db: &Database, id: ItemId) -> (Availability, String) {
    db.read(|conn| get_availability(conn, id)).unwrap()
}

#[test]
fn an_unreachable_data_source_keeps_every_row() {
    let dir = tempfile::tempdir().unwrap();
    let movies_root = dir.path().join("F-drive").join("Movies");
    let shows_root = dir.path().join("Shows");
    let db_path = dir.path().join("app").join("library.db");

    let db = Database::open(&db_path).unwrap();
    let lib = populate(&db, &movies_root, &shows_root);
    let report = db
        .refresh_availability(&StdFileProbe, PathStyle::current())
        .unwrap();
    assert_eq!(report.checked, 25 + 1 + 3);
    assert_eq!(report.online, 29, "{report:?}");

    let items_before = all_items(&db, &lib);
    let counts_before = row_counts(&db);
    drop(db);

    // The drive holding the movies is unplugged.
    let unplugged = dir.path().join("F-drive-unplugged");
    fs::rename(dir.path().join("F-drive"), &unplugged).unwrap();
    assert!(!movies_root.exists());

    // Restart: opening and loading the library deletes nothing.
    let db = Database::open(&db_path).unwrap();
    assert_eq!(row_counts(&db), counts_before);

    let report = db
        .refresh_availability(&StdFileProbe, PathStyle::current())
        .unwrap();
    assert_eq!(report.offline, 25, "{report:?}");
    assert_eq!(report.online, 4, "{report:?}");
    assert_eq!(report.changed, 25);

    // Every row and every related row is still there, byte for byte.
    assert_eq!(row_counts(&db), counts_before);
    assert_eq!(all_items(&db, &lib), items_before);
    for id in &lib.movies {
        let (st, reason) = state(&db, *id);
        assert_eq!(st, Availability::Offline);
        assert!(reason.contains("cannot be reached"), "{reason}");
    }
    assert_eq!(state(&db, lib.show).0, Availability::Online);

    // Offline items are listed by the availability filter, and the library lists every movie.
    let (offline, total) = db
        .read(|conn| {
            let filter = repo::Filter::Availability {
                values: vec![Availability::Offline.as_str().into()],
            };
            let offline = repo::count_items(conn, &ItemQuery::new(ItemKind::Movie).filter(filter))?;
            let total = repo::count_items(conn, &ItemQuery::new(ItemKind::Movie))?;
            Ok((offline, total))
        })
        .unwrap();
    assert_eq!((offline, total), (25, 25));

    // A second refresh while the drive is still away changes nothing.
    let again = db
        .refresh_availability(&StdFileProbe, PathStyle::current())
        .unwrap();
    assert_eq!(again.changed, 0);

    // The drive comes back: everything is online again, with the reason cleared.
    fs::rename(&unplugged, dir.path().join("F-drive")).unwrap();
    let back = db
        .refresh_availability(&StdFileProbe, PathStyle::current())
        .unwrap();
    assert_eq!((back.online, back.changed), (29, 25), "{back:?}");
    assert_eq!(
        state(&db, lib.movies[0]),
        (Availability::Online, String::new())
    );
    assert_eq!(all_items(&db, &lib), items_before);
}

#[test]
fn a_removed_folder_or_video_on_an_online_source_is_missing_not_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let movies_root = dir.path().join("Movies");
    let shows_root = dir.path().join("Shows");
    let db = Database::open(dir.path().join("library.db")).unwrap();
    let lib = populate(&db, &movies_root, &shows_root);
    let counts_before = row_counts(&db);

    fs::remove_dir_all(movies_root.join("Movie 00 (2020)")).unwrap();
    fs::remove_file(
        movies_root
            .join("Movie 01 (2020)")
            .join("Movie 01 (2020).mkv"),
    )
    .unwrap();
    fs::remove_file(shows_root.join("Some Show").join("Some Show S01E02.mkv")).unwrap();

    let report = db
        .refresh_availability(&StdFileProbe, PathStyle::current())
        .unwrap();
    assert_eq!(report.missing, 3, "{report:?}");
    assert_eq!(report.offline, 0);

    let (folder_state, folder_reason) = state(&db, lib.movies[0]);
    assert_eq!(folder_state, Availability::Missing);
    assert!(folder_reason.contains("folder"), "{folder_reason}");
    let (video_state, video_reason) = state(&db, lib.movies[1]);
    assert_eq!(video_state, Availability::Missing);
    assert!(video_reason.contains("video"), "{video_reason}");
    assert_eq!(state(&db, lib.episodes[1]).0, Availability::Missing);
    assert_eq!(state(&db, lib.episodes[0]).0, Availability::Online);
    assert_eq!(row_counts(&db), counts_before);
}

/// A file system where every path exists.
struct EverythingExists;

impl FileProbe for EverythingExists {
    fn dir_exists(&self, _: &str) -> bool {
        true
    }
    fn file_exists(&self, _: &str) -> bool {
        true
    }
}

fn plain_movie(title: &str, source: &str, path: &str) -> Item {
    let mut item = Item::movie(title);
    item.core.data_source = source.into();
    item.core.path = path.into();
    item
}

#[test]
fn paths_from_another_operating_system_and_empty_paths_are_invalid_not_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(dir.path().join("library.db")).unwrap();
    let ids = db
        .write(|tx| {
            Ok(vec![
                repo::insert_item(
                    tx,
                    &plain_movie("Win", "D:\\Movies", "D:\\Movies\\Win (2001)"),
                )?,
                repo::insert_item(
                    tx,
                    &plain_movie("Unix", "/mnt/movies", "/mnt/movies/Unix (2002)"),
                )?,
                repo::insert_item(tx, &plain_movie("Nowhere", "", ""))?,
            ])
        })
        .unwrap();
    let counts_before = row_counts(&db);

    let on_mac = db
        .refresh_availability(&EverythingExists, PathStyle::Posix)
        .unwrap();
    assert_eq!((on_mac.invalid, on_mac.online), (2, 1), "{on_mac:?}");
    assert_eq!(state(&db, ids[0]).0, Availability::Invalid);
    assert!(state(&db, ids[0]).1.contains("operating system"));
    assert_eq!(state(&db, ids[1]).0, Availability::Online);
    assert_eq!(
        state(&db, ids[2]),
        (Availability::Invalid, "the item has no path".into())
    );

    let on_windows = db
        .refresh_availability(&EverythingExists, PathStyle::Windows)
        .unwrap();
    assert_eq!(
        (on_windows.invalid, on_windows.online),
        (2, 1),
        "{on_windows:?}"
    );
    assert_eq!(state(&db, ids[0]).0, Availability::Online);
    assert_eq!(state(&db, ids[1]).0, Availability::Invalid);
    assert_eq!(row_counts(&db), counts_before);
}

#[test]
fn a_show_without_episodes_is_flagged_but_kept() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(dir.path().join("library.db")).unwrap();
    let show_dir = dir.path().join("Empty Show");
    fs::create_dir_all(&show_dir).unwrap();
    let mut show = Item::show("Empty Show");
    show.core.path = path_str(&show_dir);
    let id = db.write(move |tx| repo::insert_item(tx, &show)).unwrap();

    db.refresh_availability(&StdFileProbe, PathStyle::current())
        .unwrap();
    assert_eq!(
        state(&db, id),
        (Availability::Invalid, "no episodes found".into())
    );

    // Once the show's folder disappears, "missing" is the more useful answer.
    fs::remove_dir_all(&show_dir).unwrap();
    db.refresh_availability(&StdFileProbe, PathStyle::current())
        .unwrap();
    assert_eq!(state(&db, id).0, Availability::Missing);
    assert!(db.read(|c| repo::item_exists(c, id)).unwrap());
}

#[test]
fn refreshing_touches_only_availability_columns() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(dir.path().join("library.db")).unwrap();
    let movie = plain_movie("Gone", "/mnt/gone", "/mnt/gone/Gone (2000)");
    let id = db.write(move |tx| repo::insert_item(tx, &movie)).unwrap();
    // Pin `updated_ms` so any write that touched it would show.
    db.write(move |tx| {
        tx.execute("UPDATE item SET updated_ms = 42 WHERE id = ?1", [id.0])?;
        Ok(())
    })
    .unwrap();
    let before = db.read(|c| repo::get_item(c, id)).unwrap();

    db.refresh_availability(&StdFileProbe, PathStyle::Posix)
        .unwrap();
    let updated: i64 = db
        .read(|c| {
            Ok(
                c.query_row("SELECT updated_ms FROM item WHERE id = ?1", [id.0], |r| {
                    r.get(0)
                })?,
            )
        })
        .unwrap();
    assert_eq!(updated, 42);
    assert_eq!(db.read(|c| repo::get_item(c, id)).unwrap(), before);
}

#[test]
fn set_and_get_availability_round_trip_and_reject_unknown_items() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(dir.path().join("library.db")).unwrap();
    let id = db
        .write(|tx| repo::insert_item(tx, &Item::movie("Manual")))
        .unwrap();
    assert_eq!(state(&db, id), (Availability::Online, String::new()));

    db.write(move |tx| set_availability(tx, id, Availability::Missing, "moved by hand"))
        .unwrap();
    assert_eq!(
        state(&db, id),
        (Availability::Missing, "moved by hand".into())
    );

    let err = db
        .write(|tx| set_availability(tx, ItemId(9_999), Availability::Offline, ""))
        .unwrap_err();
    assert!(matches!(err, DbError::ItemNotFound(9_999)));
}

#[test]
fn path_styles_recognise_their_own_absolute_paths() {
    for ok in ["C:\\Movies", "d:/Movies", "\\\\nas\\media"] {
        assert!(PathStyle::Windows.is_absolute(ok), "{ok}");
        assert!(!PathStyle::Posix.is_absolute(ok), "{ok}");
    }
    for ok in ["/Volumes/Movies", "/mnt/x"] {
        assert!(PathStyle::Posix.is_absolute(ok), "{ok}");
        assert!(!PathStyle::Windows.is_absolute(ok), "{ok}");
    }
    for bad in ["", "Movies/x", "C:Movies"] {
        assert!(!PathStyle::Windows.is_absolute(bad), "{bad}");
        assert!(!PathStyle::Posix.is_absolute(bad), "{bad}");
    }
}
