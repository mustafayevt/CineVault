//! Tests of the daily backups: a verified copy under the day's name, and nothing left behind by
//! a failure.

use std::path::Path;

use chrono::NaiveDate;
use rusqlite::Connection;
use tempfile::TempDir;

use crate::backup;
use crate::{Database, DbError, SCHEMA_VERSION, schema};

fn day(d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
}

/// A library holding `titles` movies, and its backups folder.
fn library(titles: &[&str]) -> (TempDir, Database, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(dir.path().join("library.db")).unwrap();
    for title in titles {
        insert(&db, title);
    }
    let backups = dir.path().join(backup::BACKUPS_DIR);
    (dir, db, backups)
}

fn insert(db: &Database, title: &str) {
    let title = title.to_owned();
    db.write(move |tx| {
        tx.execute(
            "INSERT INTO item (kind, title) VALUES ('movie', ?1)",
            [title],
        )?;
        Ok(())
    })
    .unwrap();
}

fn titles(path: &Path) -> Vec<String> {
    let conn = Connection::open(path).unwrap();
    let mut stmt = conn.prepare("SELECT title FROM item ORDER BY id").unwrap();
    stmt.query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn daily_names_are_matched_exactly() {
    assert_eq!(backup::file_name(day(2)), "library-2026-11-02.db");
    assert!(backup::is_daily_name("library-2026-11-02.db"));
    for other in [
        "library-2026-11-2.db",
        "library-2026-11-02.db.partial",
        "library-2026-11-02.db-wal",
        "library-before-restore.db",
        "library.db",
        "Library-2026-11-02.db",
        "library-2026_11_02.db",
    ] {
        assert!(!backup::is_daily_name(other), "{other}");
    }
}

#[test]
fn a_backup_holds_the_library_and_its_version() {
    let (_dir, db, backups) = library(&["Alien", "Heat"]);
    let path = backup::write(&db, &backups, day(2)).unwrap();
    assert_eq!(path, backups.join("library-2026-11-02.db"));
    assert_eq!(titles(&path), ["Alien", "Heat"]);
    let conn = Connection::open(&path).unwrap();
    assert_eq!(schema::current_version(&conn).unwrap(), SCHEMA_VERSION);
    drop(conn);
    assert_eq!(names(&backups), ["library-2026-11-02.db"]);
    assert!(backup::newest(&backups).unwrap().is_some());
    // It opens as a library.
    let restored = Database::open(&path).unwrap();
    assert_eq!(restored.schema_version().unwrap(), SCHEMA_VERSION);
}

#[test]
fn writes_go_on_while_a_backup_is_written() {
    let (_dir, db, backups) = library(&["Alien"]);
    let db = std::sync::Arc::new(db);
    let copier = {
        let db = std::sync::Arc::clone(&db);
        let backups = backups.clone();
        std::thread::spawn(move || {
            for _ in 0..5 {
                backup::write(&db, &backups, day(2)).unwrap();
            }
        })
    };
    for i in 0..50 {
        insert(&db, &format!("Title {i}"));
    }
    copier.join().unwrap();
    assert_eq!(db.read(|c| Ok(titles_in(c))).unwrap(), 51);
    let copied = titles(&backups.join("library-2026-11-02.db")).len();
    assert!((1..=51).contains(&copied), "{copied}");
}

fn titles_in(conn: &Connection) -> i64 {
    conn.query_row("SELECT count(*) FROM item", [], |r| r.get(0))
        .unwrap()
}

#[test]
fn a_stale_partial_is_removed_and_a_same_day_backup_replaced() {
    let (_dir, db, backups) = library(&["Alien"]);
    backup::write(&db, &backups, day(2)).unwrap();
    insert(&db, "Heat");
    std::fs::write(backups.join("library-2026-11-01.db.partial"), b"half").unwrap();
    std::fs::write(backups.join("notes.partial"), b"mine").unwrap();
    backup::write(&db, &backups, day(2)).unwrap();
    assert_eq!(
        titles(&backups.join("library-2026-11-02.db")),
        ["Alien", "Heat"]
    );
    assert_eq!(names(&backups), ["library-2026-11-02.db", "notes.partial"]);
}

#[test]
fn a_copy_that_fails_its_check_leaves_nothing() {
    let (_dir, db, backups) = library(&["Alien"]);
    backup::write(&db, &backups, day(1)).unwrap();
    let error = backup::write_checked(&db, &backups, day(2), |_| {
        Err(DbError::Backup("integrity `damaged`".into()))
    })
    .unwrap_err();
    assert!(matches!(error, DbError::Backup(_)), "{error}");
    assert_eq!(names(&backups), ["library-2026-11-01.db"]);
}

#[test]
fn verification_needs_the_same_version() {
    let (_dir, db, backups) = library(&["Alien"]);
    let path = backup::write(&db, &backups, day(2)).unwrap();
    backup::verify(&path, SCHEMA_VERSION).unwrap();
    assert!(backup::verify(&path, SCHEMA_VERSION + 1).is_err());
}

#[test]
fn a_missing_folder_has_no_backup() {
    let dir = tempfile::tempdir().unwrap();
    let backups = dir.path().join("backups");
    assert_eq!(backup::newest(&backups).unwrap(), None);
}
