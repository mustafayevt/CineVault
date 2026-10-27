//! The database step of the fresh start, on temporary folders. The old files are synthetic: a
//! SQLite header with the version, then bytes that are not a database, which also proves the step
//! never opens them with SQLite.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::*;

/// A file that starts with a SQLite header carrying `version`, followed by `tail`.
fn sqlite_bytes(version: u32, tail: &[u8]) -> Vec<u8> {
    let mut bytes = b"SQLite format 3\0".to_vec();
    bytes.resize(100, 0);
    bytes[60..64].copy_from_slice(&version.to_be_bytes());
    bytes.extend_from_slice(tail);
    bytes
}

fn hash(path: &Path) -> [u8; 32] {
    Sha256::digest(std::fs::read(path).unwrap()).into()
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// A v17 library with its `-wal` and `-shm`; returns the database path and the three hashes.
fn old_library(dir: &Path) -> (PathBuf, [[u8; 32]; 3]) {
    let db = dir.join("library.db");
    std::fs::write(&db, sqlite_bytes(17, b"pages of an old library")).unwrap();
    std::fs::write(dir.join("library.db-wal"), b"write-ahead log frames").unwrap();
    std::fs::write(dir.join("library.db-shm"), b"shared memory index").unwrap();
    let hashes = [
        hash(&db),
        hash(&dir.join("library.db-wal")),
        hash(&dir.join("library.db-shm")),
    ];
    (db, hashes)
}

#[test]
fn a_v17_library_moves_with_its_wal_and_shm_byte_for_byte() {
    let dir = tempfile::tempdir().unwrap();
    let (db, [main, wal, shm]) = old_library(dir.path());

    let step = database_step(&db).unwrap();

    let to = dir.path().join("library-old.db");
    assert_eq!(
        step,
        DatabaseStep::MovedAside {
            version: Some(17),
            to: to.clone()
        }
    );
    assert_eq!(
        names(dir.path()),
        ["library-old.db", "library-old.db-shm", "library-old.db-wal"]
    );
    assert_eq!(hash(&to), main);
    assert_eq!(hash(&dir.path().join("library-old.db-wal")), wal);
    assert_eq!(hash(&dir.path().join("library-old.db-shm")), shm);

    // Idempotent: the next start finds nothing to do.
    assert_eq!(database_step(&db).unwrap(), DatabaseStep::Unchanged);
}

#[test]
fn a_library_without_wal_or_shm_moves_alone() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("library.db");
    std::fs::write(&db, sqlite_bytes(9, b"old")).unwrap();

    database_step(&db).unwrap();

    assert_eq!(names(dir.path()), ["library-old.db"]);
}

#[test]
fn a_version_0_file_with_content_and_a_file_that_is_not_sqlite_are_set_aside() {
    for bytes in [
        sqlite_bytes(0, b"tables"),
        b"not a database at all".to_vec(),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("library.db");
        std::fs::write(&db, &bytes).unwrap();

        let step = database_step(&db).unwrap();

        assert!(matches!(step, DatabaseStep::MovedAside { .. }), "{step:?}");
        assert_eq!(
            std::fs::read(dir.path().join("library-old.db")).unwrap(),
            bytes
        );
    }
}

#[test]
fn an_existing_library_old_is_kept_and_the_next_free_name_is_used() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("library-old.db"), b"first attempt").unwrap();
    // A `-wal` alone also takes the number: the set must stay together.
    std::fs::write(dir.path().join("library-old-2.db-wal"), b"stray").unwrap();
    let (db, [main, wal, shm]) = old_library(dir.path());

    let step = database_step(&db).unwrap();

    let to = dir.path().join("library-old-3.db");
    assert_eq!(
        step,
        DatabaseStep::MovedAside {
            version: Some(17),
            to: to.clone()
        }
    );
    assert_eq!(
        std::fs::read(dir.path().join("library-old.db")).unwrap(),
        b"first attempt"
    );
    assert_eq!(
        std::fs::read(dir.path().join("library-old-2.db-wal")).unwrap(),
        b"stray"
    );
    assert_eq!(hash(&to), main);
    assert_eq!(hash(&dir.path().join("library-old-3.db-wal")), wal);
    assert_eq!(hash(&dir.path().join("library-old-3.db-shm")), shm);
    assert!(!db.exists());
}

#[test]
fn a_failed_rename_part_way_moves_everything_back_and_names_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let (db, [main, wal, shm]) = old_library(dir.path());
    let before = names(dir.path());
    let shm_path = dir.path().join("library.db-shm");

    // The database and the `-wal` move; the `-shm` is held (a stubbed error, on every system).
    let mut rename = |from: &Path, to: &Path| {
        if from == shm_path {
            Err(io::Error::other("held by another program"))
        } else {
            std::fs::rename(from, to)
        }
    };
    let err = database_step_with(&db, &mut rename).unwrap_err();

    let FreshStartError::Move { path, stranded, .. } = &err else {
        panic!("expected a move error, got {err:?}");
    };
    assert_eq!(path, &shm_path);
    assert!(stranded.is_empty());
    assert!(err.to_string().contains("library.db-shm"), "{err}");
    assert_eq!(names(dir.path()), before);
    assert_eq!(hash(&db), main);
    assert_eq!(hash(&dir.path().join("library.db-wal")), wal);
    assert_eq!(hash(&shm_path), shm);
}

#[cfg(windows)]
#[test]
fn a_file_held_open_without_delete_sharing_leaves_every_file_in_place() {
    use std::os::windows::fs::OpenOptionsExt;

    let dir = tempfile::tempdir().unwrap();
    let (db, [main, wal, shm]) = old_library(dir.path());
    let before = names(dir.path());
    let shm_path = dir.path().join("library.db-shm");
    // FILE_SHARE_READ | FILE_SHARE_WRITE, no FILE_SHARE_DELETE: the file cannot be renamed.
    let held = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0x1 | 0x2)
        .open(&shm_path)
        .unwrap();

    let err = database_step(&db).unwrap_err();
    drop(held);

    assert!(
        matches!(&err, FreshStartError::Move { path, stranded, .. }
            if path == &shm_path && stranded.is_empty()),
        "{err:?}"
    );
    assert!(err.to_string().contains("library.db-shm"), "{err}");
    assert_eq!(names(dir.path()), before);
    assert_eq!(hash(&db), main);
    assert_eq!(hash(&dir.path().join("library.db-wal")), wal);
    assert_eq!(hash(&shm_path), shm);
}

#[test]
fn a_version_100_library_is_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("library.db");
    std::fs::write(&db, sqlite_bytes(100, b"current")).unwrap();
    std::fs::write(dir.path().join("library.db-wal"), b"frames").unwrap();

    assert_eq!(database_step(&db).unwrap(), DatabaseStep::Unchanged);
    assert_eq!(names(dir.path()), ["library.db", "library.db-wal"]);
    assert_eq!(std::fs::read(&db).unwrap(), sqlite_bytes(100, b"current"));
}

#[test]
fn a_library_newer_than_this_build_is_left_for_opening_to_refuse_untouched() {
    let newer = cv_db::schema::SCHEMA_VERSION + 1;
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("library.db");
    let bytes = sqlite_bytes(newer, b"newer");
    std::fs::write(&db, &bytes).unwrap();

    assert_eq!(database_step(&db).unwrap(), DatabaseStep::Unchanged);
    assert_eq!(names(dir.path()), ["library.db"]);

    let err = cv_db::Database::open(&db).unwrap_err();
    assert!(
        matches!(err, cv_db::DbError::SchemaTooNew { found, .. } if found == newer),
        "{err:?}"
    );
    assert_eq!(names(dir.path()), ["library.db"]);
    assert_eq!(std::fs::read(&db).unwrap(), bytes);
}

#[test]
fn no_library_and_an_empty_file_are_new_libraries() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("library.db");
    assert_eq!(database_step(&db).unwrap(), DatabaseStep::Unchanged);
    assert!(names(dir.path()).is_empty());
    std::fs::write(&db, b"").unwrap();
    assert_eq!(database_step(&db).unwrap(), DatabaseStep::Unchanged);
    assert_eq!(names(dir.path()), ["library.db"]);
}

#[test]
fn a_stop_between_two_renames_is_finished_on_the_next_start() {
    let dir = tempfile::tempdir().unwrap();
    let (db, [main, wal, shm]) = old_library(dir.path());
    // The process stopped after moving the database only.
    std::fs::rename(&db, dir.path().join("library-old.db")).unwrap();

    let step = database_step(&db).unwrap();

    assert_eq!(
        step,
        DatabaseStep::Completed {
            to: dir.path().join("library-old.db")
        }
    );
    assert_eq!(
        names(dir.path()),
        ["library-old.db", "library-old.db-shm", "library-old.db-wal"]
    );
    assert_eq!(hash(&dir.path().join("library-old.db")), main);
    assert_eq!(hash(&dir.path().join("library-old.db-wal")), wal);
    assert_eq!(hash(&dir.path().join("library-old.db-shm")), shm);
}

#[test]
fn the_app_then_opens_a_new_empty_library() {
    let dir = tempfile::tempdir().unwrap();
    let (db, _) = old_library(dir.path());

    database_step(&db).unwrap();
    let opened = cv_db::Database::open(&db).unwrap();

    assert_eq!(opened.schema_version().unwrap(), cv_db::SCHEMA_VERSION);
    let titles: i64 = opened
        .read(|c| Ok(c.query_row("SELECT count(*) FROM item", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(titles, 0);
}
