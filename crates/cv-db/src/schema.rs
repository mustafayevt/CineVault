//! The schema, its version and the migrations after it.
//!
//! The schema version is stored in SQLite's `user_version` pragma. An empty database gets the
//! whole schema of [`BASE_SCHEMA`] (version [`BASE_VERSION`], 100) and every migration after it in
//! one transaction. A database at version 100 or later gets its pending migrations (101 and up)
//! in one transaction together with their version bumps, so a failing migration leaves the
//! database exactly as it was. A database of an earlier build (versions 1 to 99, or 0 with tables)
//! is refused untouched; the app sets such a file aside before it opens the library. The
//! file-level runner in [`crate::migrate`] adds the pre-migration backup.

use std::path::Path;

use rusqlite::Connection;

use crate::error::{DbError, DbResult};

/// One schema step.
#[derive(Debug, Clone, Copy)]
pub struct Migration {
    /// The `user_version` the database has after this migration.
    pub version: u32,
    pub name: &'static str,
    pub sql: &'static str,
}

/// The schema of a new library, created in one transaction.
pub const BASE_SCHEMA: &str = include_str!("schema.sql");

/// The `user_version` [`BASE_SCHEMA`] creates. Versions below it (1 to 99) were written by
/// earlier builds and are never upgraded: the app moves such a file aside as `library-old.db`.
pub const BASE_VERSION: u32 = 100;

/// Migrations after the base schema, in order. Versions start at 101 and increase by exactly 1.
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 101,
        name: "search_keys",
        sql: include_str!("migrations/101_search_keys.sql"),
    },
    Migration {
        version: 102,
        name: "hdr_details",
        sql: include_str!("migrations/102_hdr_details.sql"),
    },
];

/// The schema version of this build: the last migration, or [`BASE_VERSION`] when there is none.
pub const SCHEMA_VERSION: u32 = latest(MIGRATIONS);

pub(crate) const fn latest(migrations: &[Migration]) -> u32 {
    match migrations.last() {
        Some(m) => m.version,
        None => BASE_VERSION,
    }
}

/// Every table (including the full-text virtual table) of the current schema.
pub const TABLES: &[&str] = &[
    "item",
    "media_file",
    "stream",
    "person",
    "credit",
    "rating",
    "ext_id",
    "tag",
    "item_tag",
    "genre",
    "item_genre",
    "artwork_ref",
    "trailer",
    "episode_number",
    "rename_journal",
    "job",
    "review_queue",
    "field_lock",
    "item_fts",
    "job_item",
    "data_source",
    "fingerprint",
    "media_unreadable",
    "item_step_outcome",
    "item_organize_state",
    "review_kept",
    "rename_item_mark",
    "torrent_indexers",
    "history_entry",
    "history_title",
];

/// Every table of the current schema ([`TABLES`]).
pub fn all_tables() -> impl Iterator<Item = &'static str> {
    TABLES.iter().copied()
}

/// Whether a `user_version` belongs to a database of an earlier build (before [`BASE_VERSION`]).
pub fn is_earlier_epoch(version: u32) -> bool {
    (1..BASE_VERSION).contains(&version)
}

/// Reads the schema version of a database (`0` for an empty one).
pub fn current_version(conn: &Connection) -> DbResult<u32> {
    let v: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    Ok(v)
}

/// Brings the database to [`SCHEMA_VERSION`]: the base schema for an empty database, then every
/// missing migration in order.
///
/// Returns the version the database had before. A database from a newer build or from an earlier
/// epoch is refused without being touched. For a library file use [`crate::migrate::upgrade_file`], which also
/// takes a backup first.
pub fn migrate(conn: &mut Connection) -> DbResult<u32> {
    migrate_with(conn, MIGRATIONS)
}

pub(crate) fn migrate_with(conn: &mut Connection, migrations: &[Migration]) -> DbResult<u32> {
    apply_all(conn, migrations, None)
}

/// Applies every pending migration inside ONE transaction, together with the version bump.
///
/// All-or-nothing: if any step fails the transaction rolls back, so the database keeps exactly the
/// content and version it had before, never a half-upgraded schema. `backup` is only recorded in
/// the error so the caller can offer a restore.
pub(crate) fn apply_all(
    conn: &mut Connection,
    migrations: &[Migration],
    backup: Option<&Path>,
) -> DbResult<u32> {
    // Migration 101 fills the search keys with these functions.
    crate::repo::search::register_functions(conn)?;
    let before = current_version(conn)?;
    let newest = latest(migrations);
    if before > newest {
        return Err(DbError::SchemaTooNew {
            found: before,
            supported: newest,
        });
    }
    if is_earlier_epoch(before) || (before == 0 && has_tables(conn)?) {
        return Err(DbError::SchemaFromEarlierVersion { found: before });
    }
    let pending: Vec<&Migration> = migrations.iter().filter(|m| m.version > before).collect();
    if before != 0 && pending.is_empty() {
        return Ok(before);
    }
    let tx = conn.transaction()?;
    if before == 0 {
        tracing::info!(version = BASE_VERSION, "creating the library schema");
        let step = tx
            .execute_batch(BASE_SCHEMA)
            .and_then(|()| tx.pragma_update(None, "user_version", BASE_VERSION));
        if let Err(source) = step {
            drop(tx);
            return Err(DbError::Migration {
                version: BASE_VERSION,
                name: "schema",
                source,
                backup: backup.map(Path::to_path_buf),
            });
        }
    }
    for m in pending {
        tracing::info!(version = m.version, name = m.name, "applying migration");
        let step = tx
            .execute_batch(m.sql)
            .and_then(|()| tx.pragma_update(None, "user_version", m.version));
        if let Err(source) = step {
            // Dropping the transaction rolls every step back.
            drop(tx);
            return Err(DbError::Migration {
                version: m.version,
                name: m.name,
                source,
                backup: backup.map(Path::to_path_buf),
            });
        }
    }
    tx.commit()?;
    Ok(before)
}

/// Whether the database holds any schema object (an empty file has none).
fn has_tables(conn: &Connection) -> DbResult<bool> {
    let n: i64 = conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'",
        [],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}
