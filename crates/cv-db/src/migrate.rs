//! The file-level migration runner.
//!
//! [`upgrade_file`] runs before the library opens. For a database that already holds data and
//! needs upgrading it
//!
//! 0. reads the file header without SQLite and refuses a file of an earlier build (schema
//!    version 1 to 99, or 0 with content) or of a newer one, untouched,
//! 1. takes a consistent snapshot into `backups/pre-migration/` next to the database and verifies
//!    it,
//! 2. applies every pending migration in one transaction on a plain (non-WAL) connection.
//!
//! If any step fails the transaction rolls back and the database file is byte-for-byte what it
//! was; the error carries the backup path so the app can offer to restore it. A fresh (empty)
//! database needs no backup, and an up-to-date or newer one is never touched.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OpenFlags};

use crate::error::{DbError, DbResult};
use crate::header::{self, LibraryFile};
use crate::schema::{self, Migration};

/// Folder (relative to the database's folder) that holds pre-migration snapshots.
///
/// Kept apart from the rolling daily backups so their retention never prunes it.
pub const PRE_MIGRATION_DIR: &str = "backups/pre-migration";

/// What the migration runner did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationReport {
    /// Schema version before (`0` for a new database).
    pub from: u32,
    /// Schema version after.
    pub to: u32,
    /// The pre-migration snapshot, when one was taken.
    pub backup: Option<PathBuf>,
}

/// Upgrades the database file at `path` to the newest schema in `migrations`.
///
/// The file is created when missing. See the module docs for the guarantees.
pub(crate) fn upgrade_file(path: &Path, migrations: &[Migration]) -> DbResult<MigrationReport> {
    // Read the header first, without SQLite: a file of an earlier build is never opened, so no
    // `-wal` or `-shm` is created next to it and it stays byte for byte as it was.
    let latest = schema::latest(migrations);
    match header::classify(path, latest).map_err(|source| DbError::Io {
        path: path.to_path_buf(),
        source,
    })? {
        LibraryFile::Earlier { version } => {
            return Err(DbError::SchemaFromEarlierVersion {
                found: version.unwrap_or(0),
            });
        }
        LibraryFile::Newer { version } => {
            return Err(DbError::SchemaTooNew {
                found: version,
                supported: latest,
            });
        }
        LibraryFile::Missing | LibraryFile::Empty | LibraryFile::Current { .. } => {}
    }
    let mut conn = Connection::open(path)?;
    conn.pragma_update(None, "busy_timeout", 5_000)?;
    let from = schema::current_version(&conn)?;
    if from > latest {
        return Err(DbError::SchemaTooNew {
            found: from,
            supported: latest,
        });
    }
    if from == latest {
        return Ok(MigrationReport {
            from,
            to: from,
            backup: None,
        });
    }

    // Version 0 is an empty file: nothing to protect (a file of an earlier build never gets here).
    let backup = if from == 0 {
        None
    } else {
        Some(snapshot(&conn, path, from)?)
    };
    schema::apply_all(&mut conn, migrations, backup.as_deref())?;
    Ok(MigrationReport {
        from,
        to: latest,
        backup,
    })
}

/// Copies the database into the pre-migration folder and checks the copy.
fn snapshot(conn: &Connection, db_path: &Path, version: u32) -> DbResult<PathBuf> {
    let parent = db_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let dir = parent.join(PRE_MIGRATION_DIR);
    std::fs::create_dir_all(&dir).map_err(|source| DbError::Io {
        path: dir.clone(),
        source,
    })?;
    let stem = db_path
        .file_stem()
        .map_or_else(|| "library".into(), |s| s.to_string_lossy().into_owned());
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let target = dir.join(format!("{stem}-v{version}-{millis}.db"));

    let result = conn
        .execute("VACUUM INTO ?1", [target.to_string_lossy().as_ref()])
        .map_err(|e| DbError::Backup(format!("cannot write {}: {e}", target.display())))
        .and_then(|_| verify(&target, version));
    if let Err(e) = result {
        let _ = std::fs::remove_file(&target);
        return Err(e);
    }
    tracing::info!(backup = %target.display(), version, "pre-migration backup written");
    Ok(target)
}

/// The copy must open, pass SQLite's integrity check and carry the version it was taken at.
fn verify(copy: &Path, version: u32) -> DbResult<()> {
    let unreadable =
        |e: rusqlite::Error| DbError::Backup(format!("{} is unreadable: {e}", copy.display()));
    let conn =
        Connection::open_with_flags(copy, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(unreadable)?;
    let integrity: String = conn
        .pragma_query_value(None, "integrity_check", |r| r.get(0))
        .map_err(unreadable)?;
    let found = schema::current_version(&conn).map_err(|e| DbError::Backup(e.to_string()))?;
    if integrity != "ok" || found != version {
        return Err(DbError::Backup(format!(
            "{} failed verification (integrity `{integrity}`, version {found})",
            copy.display()
        )));
    }
    Ok(())
}
