//! Tests of the file-level migration runner: a new file gets the base schema (version 100), an
//! up-to-date file is left alone, a newer or an earlier one is refused untouched, and a later
//! migration (101 and up) is backed up first and applied all or nothing.

use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::migrate::{PRE_MIGRATION_DIR, upgrade_file};
use crate::schema::{self, MIGRATIONS, Migration, SCHEMA_VERSION};
use crate::{Database, DbError};

const ADD_NOTE_TABLE: Migration = Migration {
    version: SCHEMA_VERSION + 1,
    name: "add_note",
    sql: "CREATE TABLE note (id INTEGER PRIMARY KEY, body TEXT);",
};

/// Fails part-way: it creates a table first, then touches one that does not exist.
const BROKEN: Migration = Migration {
    version: SCHEMA_VERSION + 2,
    name: "broken",
    sql: "CREATE TABLE half_done (id INTEGER); INSERT INTO no_such_table VALUES (1);",
};

fn chain(extra: &[Migration]) -> Vec<Migration> {
    let mut all = MIGRATIONS.to_vec();
    all.extend_from_slice(extra);
    all
}

/// Creates a library at the current schema version holding one movie; `wal` selects the journal mode it was left in.
fn old_library(dir: &Path, wal: bool) -> PathBuf {
    let path = dir.join("library.db");
    if wal {
        drop(Database::open(&path).unwrap());
    } else {
        let mut conn = Connection::open(&path).unwrap();
        schema::migrate(&mut conn).unwrap();
    }
    let conn = Connection::open(&path).unwrap();
    conn.execute(
        "INSERT INTO item (kind, title) VALUES ('movie', 'Alien')",
        [],
    )
    .unwrap();
    path
}

fn titles(path: &Path) -> Vec<String> {
    let conn = Connection::open(path).unwrap();
    let mut stmt = conn.prepare("SELECT title FROM item ORDER BY id").unwrap();
    stmt.query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn has_table(path: &Path, name: &str) -> bool {
    let conn = Connection::open(path).unwrap();
    conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [name],
        |r| r.get::<_, i64>(0),
    )
    .unwrap()
        > 0
}

fn version_of(path: &Path) -> u32 {
    schema::current_version(&Connection::open(path).unwrap()).unwrap()
}

fn backups_in(dir: &Path) -> Vec<PathBuf> {
    let folder = dir.join(PRE_MIGRATION_DIR);
    match std::fs::read_dir(folder) {
        Ok(rd) => rd.map(|e| e.unwrap().path()).collect(),
        Err(_) => Vec::new(),
    }
}

fn failing_migration_leaves_the_original_untouched(wal: bool) {
    let dir = tempfile::tempdir().unwrap();
    let path = old_library(dir.path(), wal);
    let original = std::fs::read(&path).unwrap();

    // The note table would succeed on its own; the next step fails, so the whole upgrade must be undone.
    let err = upgrade_file(&path, &chain(&[ADD_NOTE_TABLE, BROKEN])).unwrap_err();

    let DbError::Migration {
        version, backup, ..
    } = &err
    else {
        panic!("expected a migration error, got {err:?}");
    };
    assert_eq!(*version, SCHEMA_VERSION + 2);
    // The database file is byte-for-byte what it was, at the old version, without the earlier step's table.
    assert_eq!(std::fs::read(&path).unwrap(), original, "file changed");
    assert_eq!(version_of(&path), SCHEMA_VERSION);
    assert!(!has_table(&path, "note"), "an earlier step leaked through");
    assert!(!has_table(&path, "half_done"));
    assert_eq!(titles(&path), ["Alien"]);
    // A verified backup exists, the error names it, and it holds the pre-migration data.
    let backup = backup.clone().expect("error carries the backup path");
    assert_eq!(backups_in(dir.path()), std::slice::from_ref(&backup));
    assert_eq!(version_of(&backup), SCHEMA_VERSION);
    assert_eq!(titles(&backup), ["Alien"]);
    assert!(err.to_string().contains(&backup.display().to_string()));
}

#[test]
fn a_failing_migration_leaves_the_original_database_untouched() {
    failing_migration_leaves_the_original_untouched(false);
}

#[test]
fn a_failing_migration_leaves_a_wal_mode_database_untouched() {
    failing_migration_leaves_the_original_untouched(true);
}

#[test]
fn a_successful_upgrade_backs_up_first_and_keeps_the_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = old_library(dir.path(), false);

    let report = upgrade_file(&path, &chain(&[ADD_NOTE_TABLE])).unwrap();

    assert_eq!(
        (report.from, report.to),
        (SCHEMA_VERSION, SCHEMA_VERSION + 1)
    );
    let backup = report.backup.expect("a backup was taken");
    assert!(backup.starts_with(dir.path().join(PRE_MIGRATION_DIR)));
    // The backup is the old schema and data; the library is upgraded.
    assert_eq!(version_of(&backup), SCHEMA_VERSION);
    assert!(!has_table(&backup, "note"));
    assert_eq!(titles(&backup), ["Alien"]);
    assert_eq!(version_of(&path), SCHEMA_VERSION + 1);
    assert!(has_table(&path, "note"));
    assert_eq!(titles(&path), ["Alien"]);
}

#[test]
fn a_new_database_needs_no_backup() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.db");

    let report = upgrade_file(&path, MIGRATIONS).unwrap();

    assert_eq!(
        (report.from, report.to, report.backup),
        (0, SCHEMA_VERSION, None)
    );
    assert!(backups_in(dir.path()).is_empty());
}

#[test]
fn an_up_to_date_database_is_not_backed_up_or_changed() {
    let dir = tempfile::tempdir().unwrap();
    let path = old_library(dir.path(), false);
    let original = std::fs::read(&path).unwrap();

    let report = upgrade_file(&path, MIGRATIONS).unwrap();

    assert_eq!(
        (report.from, report.to, report.backup),
        (SCHEMA_VERSION, SCHEMA_VERSION, None)
    );
    assert!(backups_in(dir.path()).is_empty());
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[test]
fn a_database_from_a_newer_build_is_refused_and_not_backed_up() {
    let dir = tempfile::tempdir().unwrap();
    let path = old_library(dir.path(), false);
    Connection::open(&path)
        .unwrap()
        .pragma_update(None, "user_version", SCHEMA_VERSION + 1)
        .unwrap();
    let original = std::fs::read(&path).unwrap();

    let err = upgrade_file(&path, MIGRATIONS).unwrap_err();

    assert!(
        matches!(err, DbError::SchemaTooNew { found, supported }
            if found == SCHEMA_VERSION + 1 && supported == SCHEMA_VERSION),
        "{err:?}"
    );
    assert!(backups_in(dir.path()).is_empty());
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

/// The files in `dir`, by name.
fn names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn a_database_of_an_earlier_build_is_refused_without_being_opened() {
    for version in [1, 17, 99] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch("CREATE TABLE item (id INTEGER PRIMARY KEY);")
                .unwrap();
            conn.pragma_update(None, "user_version", version).unwrap();
        }
        let original = std::fs::read(&path).unwrap();

        let err = Database::open(&path).unwrap_err();

        assert!(
            matches!(err, DbError::SchemaFromEarlierVersion { found } if found == version),
            "{err:?}"
        );
        assert_eq!(std::fs::read(&path).unwrap(), original, "file changed");
        // Never opened with SQLite: no -wal, -shm or journal appeared, and nothing was backed up.
        assert_eq!(names_in(dir.path()), ["library.db"]);
    }
}

#[test]
fn a_new_database_is_created_at_the_newest_version() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.db");

    upgrade_file(&path, MIGRATIONS).unwrap();

    assert_eq!(version_of(&path), SCHEMA_VERSION);
    assert_eq!(
        SCHEMA_VERSION,
        MIGRATIONS
            .last()
            .map_or(schema::BASE_VERSION, |m| m.version)
    );
    assert!(has_column(&path, "item", "search_title"));
    for t in schema::all_tables() {
        assert!(has_table(&path, t), "missing table {t}");
    }
    assert!(!has_column(&path, "item", "offline_stub"));
}

fn has_column(path: &Path, table: &str, column: &str) -> bool {
    let conn = Connection::open(path).unwrap();
    let mut stmt = conn
        .prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
        .unwrap();
    stmt.query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .map(Result::unwrap)
        .any(|n| n == column)
}

#[test]
fn opening_reports_the_upgrade_and_a_restart_does_not_repeat_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.db");

    let first = Database::open(&path).unwrap();
    assert_eq!(first.migration_report().from, 0);
    assert_eq!(first.migration_report().to, SCHEMA_VERSION);
    drop(first);

    let second = Database::open(&path).unwrap();
    assert_eq!(second.migration_report().from, SCHEMA_VERSION);
    assert!(second.migration_report().backup.is_none());
}
