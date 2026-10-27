use std::collections::BTreeSet;
use std::sync::mpsc;
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;

use rusqlite::{Connection, params};
use tempfile::TempDir;

use crate::schema::{self, MIGRATIONS, Migration, SCHEMA_VERSION};
use crate::{Database, DbError, DbOptions};

fn temp_db() -> (TempDir, Database) {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(dir.path().join("library.db")).unwrap();
    (dir, db)
}

fn object_names(conn: &Connection, kind: &str) -> BTreeSet<String> {
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = ?1 AND name NOT LIKE 'sqlite_%'")
        .unwrap();
    stmt.query_map([kind], |r| r.get::<_, String>(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn insert_item(conn: &Connection, kind: &str, title: &str) -> i64 {
    conn.execute(
        "INSERT INTO item (kind, title) VALUES (?1, ?2)",
        params![kind, title],
    )
    .unwrap();
    conn.last_insert_rowid()
}

// ---- migrations create the schema from scratch ----------------------------------------------

#[test]
fn migration_versions_are_consecutive_after_the_base() {
    for (i, m) in MIGRATIONS.iter().enumerate() {
        assert_eq!(
            m.version as usize,
            schema::BASE_VERSION as usize + i + 1,
            "migration {}",
            m.name
        );
    }
    assert_eq!(
        MIGRATIONS
            .last()
            .map_or(schema::BASE_VERSION, |m| m.version),
        SCHEMA_VERSION
    );
}

#[test]
fn a_version_0_database_with_tables_is_from_an_earlier_build() {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE item (id INTEGER PRIMARY KEY);")
        .unwrap();

    let err = schema::migrate(&mut conn).unwrap_err();

    assert!(
        matches!(err, DbError::SchemaFromEarlierVersion { found: 0 }),
        "{err:?}"
    );
    assert_eq!(schema::current_version(&conn).unwrap(), 0);
    assert_eq!(
        object_names(&conn, "table"),
        BTreeSet::from(["item".to_owned()])
    );
}

#[test]
fn migrations_create_the_schema_from_an_empty_database() {
    let mut conn = Connection::open_in_memory().unwrap();
    assert_eq!(schema::current_version(&conn).unwrap(), 0);

    let before = schema::migrate(&mut conn).unwrap();

    assert_eq!(before, 0);
    assert_eq!(schema::current_version(&conn).unwrap(), SCHEMA_VERSION);
    let tables = object_names(&conn, "table");
    for t in schema::all_tables() {
        assert!(tables.contains(t), "missing table {t}");
    }
    let integrity: String = conn
        .pragma_query_value(None, "integrity_check", |r| r.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");
}

#[test]
fn the_schema_has_every_design_d3_table_and_the_indexes_queries_rely_on() {
    let mut conn = Connection::open_in_memory().unwrap();
    schema::migrate(&mut conn).unwrap();
    let tables = object_names(&conn, "table");
    // every table of the schema
    for t in [
        "item",
        "media_file",
        "stream",
        "person",
        "credit",
        "rating",
        "ext_id",
        "tag",
        "genre",
        "item_tag",
        "item_genre",
        "artwork_ref",
        "trailer",
        "episode_number",
        "rename_journal",
        "job",
        "review_queue",
        "field_lock",
        "item_fts",
    ] {
        assert!(tables.contains(t), "missing table {t}");
    }
    let indexes = object_names(&conn, "index");
    for i in [
        "item_kind_title",
        "item_kind_year",
        "item_kind_added",
        "item_parent",
        "ext_id_lookup",
        "media_file_item",
    ] {
        assert!(indexes.contains(i), "missing index {i}");
    }
}

#[test]
fn migrating_twice_changes_nothing() {
    let mut conn = Connection::open_in_memory().unwrap();
    schema::migrate(&mut conn).unwrap();
    insert_item(&conn, "movie", "Alien");

    let before = schema::migrate(&mut conn).unwrap();

    assert_eq!(before, SCHEMA_VERSION);
    let count: i64 = conn
        .query_row("SELECT count(*) FROM item", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn a_database_from_a_newer_build_is_refused_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.db");
    {
        let conn = Connection::open(&path).unwrap();
        conn.pragma_update(None, "user_version", SCHEMA_VERSION + 5)
            .unwrap();
    }

    let err = Database::open(&path).unwrap_err();

    assert!(
        matches!(err, DbError::SchemaTooNew { found, supported }
            if found == SCHEMA_VERSION + 5 && supported == SCHEMA_VERSION),
        "{err:?}"
    );
    let conn = Connection::open(&path).unwrap();
    assert_eq!(schema::current_version(&conn).unwrap(), SCHEMA_VERSION + 5);
    assert!(object_names(&conn, "table").is_empty());
}

#[test]
fn a_failing_migration_rolls_back_and_keeps_the_version() {
    let mut conn = Connection::open_in_memory().unwrap();
    schema::migrate(&mut conn).unwrap();
    let broken = [
        Migration {
            version: SCHEMA_VERSION + 1,
            name: "fine",
            sql: "CREATE TABLE fine (id INTEGER);",
        },
        Migration {
            version: SCHEMA_VERSION + 2,
            name: "broken",
            sql: "CREATE TABLE extra (id INTEGER); INSERT INTO no_such_table VALUES (1);",
        },
    ];

    let err = schema::migrate_with(&mut conn, &broken).unwrap_err();

    assert!(
        matches!(err, DbError::Migration { version, .. } if version == SCHEMA_VERSION + 2),
        "{err:?}"
    );
    assert_eq!(schema::current_version(&conn).unwrap(), SCHEMA_VERSION);
    assert!(!object_names(&conn, "table").contains("fine"));
    assert!(!object_names(&conn, "table").contains("extra"));
    assert!(object_names(&conn, "table").contains("item"));
}

// ---- opening: WAL, pragmas, files ------------------------------------------------------------

#[test]
fn open_creates_the_file_and_folder_and_the_schema() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested").join("data").join("library.db");

    let db = Database::open(&path).unwrap();

    assert!(path.is_file());
    assert_eq!(db.path(), path);
    assert_eq!(db.schema_version().unwrap(), SCHEMA_VERSION);
    let tables = db.read(|c| Ok(object_names(c, "table"))).unwrap();
    for t in schema::all_tables() {
        assert!(tables.contains(t), "missing table {t}");
    }
}

#[test]
fn reopening_keeps_data_and_version() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.db");
    {
        let db = Database::open(&path).unwrap();
        db.write(|tx| {
            insert_item(tx, "movie", "Alien");
            Ok(())
        })
        .unwrap();
    }

    let db = Database::open(&path).unwrap();

    assert_eq!(db.schema_version().unwrap(), SCHEMA_VERSION);
    let title: String = db
        .read(|c| Ok(c.query_row("SELECT title FROM item", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(title, "Alien");
}

#[test]
fn connections_use_wal_foreign_keys_and_full_sync() {
    let (_dir, db) = temp_db();

    let mode: String = db
        .read(|c| Ok(c.pragma_query_value(None, "journal_mode", |r| r.get(0))?))
        .unwrap();
    assert_eq!(mode, "wal");
    let fk: i64 = db
        .read(|c| Ok(c.pragma_query_value(None, "foreign_keys", |r| r.get(0))?))
        .unwrap();
    assert_eq!(fk, 1);

    let (w_mode, w_fk, w_sync) = db
        .write(|tx| {
            Ok((
                tx.pragma_query_value(None, "journal_mode", |r| r.get::<_, String>(0))?,
                tx.pragma_query_value(None, "foreign_keys", |r| r.get::<_, i64>(0))?,
                tx.pragma_query_value(None, "synchronous", |r| r.get::<_, i64>(0))?,
            ))
        })
        .unwrap();
    assert_eq!(w_mode, "wal");
    assert_eq!(w_fk, 1);
    assert_eq!(w_sync, 2, "synchronous = FULL");
}

// ---- single writer thread --------------------------------------------------------------------

#[test]
fn every_write_runs_on_the_one_writer_thread() {
    let (_dir, db) = temp_db();
    let db = Arc::new(db);

    let names: Vec<String> = (0..6)
        .map(|_| {
            let db = Arc::clone(&db);
            thread::spawn(move || {
                db.write(|_| Ok(thread::current().name().unwrap_or("").to_owned()))
                    .unwrap()
            })
        })
        .collect::<Vec<_>>()
        .into_iter()
        .map(|h| h.join().unwrap())
        .collect();

    assert!(names.iter().all(|n| n == "cv-db-writer"), "{names:?}");
}

#[test]
fn concurrent_writers_are_serialised_without_busy_errors() {
    let (_dir, db) = temp_db();
    let db = Arc::new(db);
    let barrier = Arc::new(Barrier::new(8));

    let handles: Vec<_> = (0..8)
        .map(|t| {
            let db = Arc::clone(&db);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                for n in 0..50 {
                    db.write(move |tx| {
                        insert_item(tx, "movie", &format!("m{t}-{n}"));
                        Ok(())
                    })
                    .unwrap();
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }

    let count: i64 = db
        .read(|c| Ok(c.query_row("SELECT count(*) FROM item", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(count, 400);
}

#[test]
fn a_write_error_rolls_the_transaction_back() {
    let (_dir, db) = temp_db();

    let res: Result<(), DbError> = db.write(|tx| {
        insert_item(tx, "movie", "half written");
        // violates the kind CHECK constraint
        tx.execute("INSERT INTO item (kind) VALUES ('bogus')", [])?;
        Ok(())
    });

    assert!(matches!(res, Err(DbError::Sqlite(_))));
    let count: i64 = db
        .read(|c| Ok(c.query_row("SELECT count(*) FROM item", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn a_panicking_write_rolls_back_and_the_writer_keeps_working() {
    let (_dir, db) = temp_db();

    let res: Result<(), DbError> = db.write(|tx| {
        insert_item(tx, "movie", "doomed");
        panic!("boom");
    });
    assert!(matches!(res, Err(DbError::WriterPanicked)), "{res:?}");

    db.write(|tx| {
        insert_item(tx, "movie", "survivor");
        Ok(())
    })
    .unwrap();
    let titles: Vec<String> = db
        .read(|c| {
            let mut stmt = c.prepare("SELECT title FROM item")?;
            Ok(stmt
                .query_map([], |r| r.get(0))?
                .collect::<Result<_, _>>()?)
        })
        .unwrap();
    assert_eq!(titles, vec!["survivor"]);
}

// ---- reader pool -----------------------------------------------------------------------------

#[test]
fn readers_cannot_write() {
    let (_dir, db) = temp_db();

    let res = db.read(|c| {
        c.execute("INSERT INTO item (kind) VALUES ('movie')", [])?;
        Ok(())
    });

    assert!(matches!(res, Err(DbError::Sqlite(_))), "{res:?}");
}

#[test]
fn readers_see_committed_data_while_the_writer_holds_an_open_transaction() {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(Database::open(dir.path().join("library.db")).unwrap());
    db.write(|tx| {
        insert_item(tx, "movie", "committed");
        Ok(())
    })
    .unwrap();

    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let writer = {
        let db = Arc::clone(&db);
        thread::spawn(move || {
            db.write(move |tx| {
                insert_item(tx, "movie", "uncommitted");
                started_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                Ok(())
            })
            .unwrap();
        })
    };
    started_rx.recv_timeout(Duration::from_secs(10)).unwrap();

    // The writer is mid-transaction; reading must neither block nor see its row.
    let during: i64 = db
        .read(|c| Ok(c.query_row("SELECT count(*) FROM item", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(during, 1);

    release_tx.send(()).unwrap();
    writer.join().unwrap();
    let after: i64 = db
        .read(|c| Ok(c.query_row("SELECT count(*) FROM item", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(after, 2);
}

#[test]
fn more_concurrent_readers_than_connections_all_complete() {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(
        Database::open_with(dir.path().join("library.db"), &DbOptions { readers: 2 }).unwrap(),
    );
    db.write(|tx| {
        insert_item(tx, "movie", "Alien");
        Ok(())
    })
    .unwrap();

    let handles: Vec<_> = (0..12)
        .map(|_| {
            let db = Arc::clone(&db);
            thread::spawn(move || {
                db.read(|c| {
                    thread::sleep(Duration::from_millis(20));
                    Ok(c.query_row("SELECT count(*) FROM item", [], |r| r.get::<_, i64>(0))?)
                })
                .unwrap()
            })
        })
        .collect();

    for h in handles {
        assert_eq!(h.join().unwrap(), 1);
    }
}

#[test]
fn zero_readers_is_treated_as_one() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open_with(dir.path().join("library.db"), &DbOptions { readers: 0 }).unwrap();
    assert_eq!(db.schema_version().unwrap(), SCHEMA_VERSION);
}

// ---- schema behaviour ------------------------------------------------------------------------

#[test]
fn full_text_index_is_usable_and_follows_item_deletion() {
    let (_dir, db) = temp_db();
    db.write(|tx| {
        let id = insert_item(tx, "movie", "Blade Runner 2049");
        tx.execute(
            "INSERT INTO item_fts (rowid, title, people, filenames) VALUES (?1, ?2, ?3, ?4)",
            params![id, "Blade Runner 2049", "Denis Villeneuve", "br2049.mkv"],
        )?;
        Ok(())
    })
    .unwrap();

    let hits: i64 = db
        .read(|c| {
            Ok(c.query_row(
                "SELECT count(*) FROM item_fts WHERE item_fts MATCH 'villen*'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(hits, 1);

    db.write(|tx| {
        tx.execute("DELETE FROM item", [])?;
        Ok(())
    })
    .unwrap();
    let left: i64 = db
        .read(|c| Ok(c.query_row("SELECT count(*) FROM item_fts", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(left, 0);
}

#[test]
fn child_rows_follow_their_item_but_a_parent_with_children_cannot_be_deleted() {
    let (_dir, db) = temp_db();
    let res = db.write(|tx| {
        let show = insert_item(tx, "show", "Severance");
        tx.execute(
            "INSERT INTO item (kind, parent_id, title) VALUES ('episode', ?1, 'Pilot')",
            [show],
        )?;
        let ep = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO episode_number (item_id, ordering, season, episode) VALUES (?1, 'aired', 1, 1)",
            [ep],
        )?;
        tx.execute(
            "INSERT INTO ext_id (item_id, provider, value) VALUES (?1, 'kinopoisk', '12345')",
            [ep],
        )?;
        tx.execute(
            "INSERT INTO media_file (item_id, kind, filename) VALUES (?1, 'video', 'a.mkv')",
            [ep],
        )?;
        let file = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO stream (media_file_id, kind, language) VALUES (?1, 'audio', 'en')",
            [file],
        )?;
        Ok((show, ep))
    });
    let (show, ep) = res.unwrap();

    // Removing the show while episodes exist is refused: nothing is deleted implicitly.
    let refused = db.write(move |tx| {
        tx.execute("DELETE FROM item WHERE id = ?1", [show])?;
        Ok(())
    });
    assert!(matches!(refused, Err(DbError::Sqlite(_))), "{refused:?}");

    // Removing the episode removes everything hanging off it.
    db.write(move |tx| {
        tx.execute("DELETE FROM item WHERE id = ?1", [ep])?;
        Ok(())
    })
    .unwrap();
    for table in ["episode_number", "ext_id", "media_file", "stream"] {
        let n: i64 = db
            .read(|c| Ok(c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(n, 0, "{table} should be empty");
    }
    let shows: i64 = db
        .read(|c| Ok(c.query_row("SELECT count(*) FROM item", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(shows, 1);
}

#[test]
fn constraints_reject_invalid_values() {
    let (_dir, db) = temp_db();
    let bad_statements = [
        "INSERT INTO item (kind) VALUES ('album')",
        "INSERT INTO item (kind, availability) VALUES ('movie', 'gone')",
        "INSERT INTO item (kind, ext) VALUES ('movie', 'not json')",
        "INSERT INTO item (kind, parent_id) VALUES ('episode', 999)",
        "INSERT INTO ext_id (item_id, provider, value) VALUES (1, 'imdb', '')",
    ];
    for sql in bad_statements {
        let res = db.write(move |tx| {
            tx.execute(sql, [])?;
            Ok(())
        });
        assert!(res.is_err(), "should be rejected: {sql}");
    }
}

#[test]
fn provider_ids_accept_any_key_and_are_unique_per_item() {
    let (_dir, db) = temp_db();
    db.write(|tx| {
        let id = insert_item(tx, "movie", "Stalker");
        for (k, v) in [
            ("imdb", "tt0079944"),
            ("kinopoisk", "12345"),
            ("custom", "x"),
        ] {
            tx.execute(
                "INSERT INTO ext_id (item_id, provider, value) VALUES (?1, ?2, ?3)",
                params![id, k, v],
            )?;
        }
        Ok(())
    })
    .unwrap();

    let dup = db.write(|tx| {
        tx.execute(
            "INSERT INTO ext_id (item_id, provider, value) VALUES (1, 'imdb', 'tt1')",
            [],
        )?;
        Ok(())
    });
    assert!(dup.is_err());
}

#[test]
fn foreign_key_check_is_clean_on_a_fresh_database() {
    let (_dir, db) = temp_db();
    let violations: i64 = db
        .read(|c| {
            let mut stmt = c.prepare("PRAGMA foreign_key_check")?;
            Ok(stmt.query_map([], |_| Ok(()))?.count() as i64)
        })
        .unwrap();
    assert_eq!(violations, 0);
}
