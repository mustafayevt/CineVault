//! Daily backups of the library.
//!
//! [`write`] copies the open library into `backups/library-YYYY-MM-DD.db` next to it:
//!
//! 1. a `.partial` file an earlier, interrupted backup left behind is removed,
//! 2. `VACUUM INTO` writes one consistent snapshot to `library-YYYY-MM-DD.db.partial` from a
//!    dedicated read-only connection, so the writer and the readers go on meanwhile (WAL),
//! 3. the copy is flushed to the disk and [verified](verify),
//! 4. and only then renamed to its final name in one step.
//!
//! Any failure removes the partial file, so a file with a backup's name always holds a complete,
//! checked copy. The library file itself is only read. [`prune`] then keeps the newest seven
//! daily files; it never touches `pre-migration/` or any other name.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::NaiveDate;
use rusqlite::{Connection, OpenFlags};

use crate::Database;
use crate::error::{DbError, DbResult};
use crate::schema;

/// Folder (relative to the database's folder) that holds the daily backups.
pub const BACKUPS_DIR: &str = "backups";

/// How many daily backups are kept.
pub const KEEP: usize = 7;

const PREFIX: &str = "library-";
const SUFFIX: &str = ".db";
const PARTIAL: &str = ".partial";

/// The name of the backup taken on `date`: `library-YYYY-MM-DD.db`.
#[must_use]
pub fn file_name(date: NaiveDate) -> String {
    format!("{PREFIX}{}{SUFFIX}", date.format("%Y-%m-%d"))
}

/// Whether `name` is exactly a daily backup's name, `library-YYYY-MM-DD.db`.
#[must_use]
pub fn is_daily_name(name: &str) -> bool {
    name.strip_prefix(PREFIX)
        .and_then(|rest| rest.strip_suffix(SUFFIX))
        .is_some_and(is_date)
}

/// `YYYY-MM-DD`, digits only.
fn is_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 10
        && bytes.iter().enumerate().all(|(i, b)| match i {
            4 | 7 => *b == b'-',
            _ => b.is_ascii_digit(),
        })
}

/// The daily backup files in `dir`, sorted by name (oldest date first). A missing folder has
/// none.
fn daily_files(dir: &Path) -> DbResult<Vec<PathBuf>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => {
            return Err(DbError::Io {
                path: dir.to_path_buf(),
                source,
            });
        }
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter(|e| e.file_name().to_str().is_some_and(is_daily_name))
        .map(|e| e.path())
        .collect();
    files.sort();
    Ok(files)
}

/// When the newest daily backup in `dir` was written (its modified time), or `None` when there
/// is none.
///
/// # Errors
/// The folder exists but cannot be read.
pub fn newest(dir: &Path) -> DbResult<Option<SystemTime>> {
    Ok(daily_files(dir)?
        .iter()
        .filter_map(|path| path.metadata().and_then(|m| m.modified()).ok())
        .max())
}

/// Removes the `library-YYYY-MM-DD.db.partial` files an interrupted backup left in `dir`.
/// Nothing else is touched.
pub fn remove_partials(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let partial = entry
            .file_name()
            .to_str()
            .and_then(|name| name.strip_suffix(PARTIAL))
            .is_some_and(is_daily_name);
        if partial && entry.file_type().is_ok_and(|t| t.is_file()) {
            let path = entry.path();
            match cv_fs::remove_own_file(&path) {
                Ok(()) => tracing::info!(path = %path.display(), "removed an unfinished backup"),
                Err(error) => {
                    tracing::warn!(%error, path = %path.display(), "cannot remove an unfinished backup");
                }
            }
        }
    }
}

/// Writes the backup of `date` into `dir` (created when missing) and returns its path. A backup
/// of the same date is replaced. See the module docs for the steps.
///
/// # Errors
/// The copy could not be written, flushed, verified or put in place. Nothing is left behind and
/// the earlier backups are unchanged.
pub fn write(db: &Database, dir: &Path, date: NaiveDate) -> DbResult<PathBuf> {
    let version = db.schema_version()?;
    write_checked(db, dir, date, |copy| {
        verify(copy, version).inspect_err(|_| {
            // A copy is only as sound as what it was copied from.
            tracing::warn!(db = %db.path().display(), "the backup failed its check; the library itself may need checking");
        })
    })
}

/// [`write`] with the check of the copy given, so a test can make it fail.
pub(crate) fn write_checked(
    db: &Database,
    dir: &Path,
    date: NaiveDate,
    check: impl FnOnce(&Path) -> DbResult<()>,
) -> DbResult<PathBuf> {
    std::fs::create_dir_all(dir).map_err(|source| DbError::Io {
        path: dir.to_path_buf(),
        source,
    })?;
    remove_partials(dir);
    let target = dir.join(file_name(date));
    let partial = dir.join(format!("{}{PARTIAL}", file_name(date)));

    let written = copy(db.path(), &partial)
        .and_then(|()| {
            cv_fs::sync_file(&partial).map_err(|source| DbError::Io {
                path: partial.clone(),
                source,
            })
        })
        .and_then(|()| check(&partial))
        .and_then(|()| {
            cv_fs::persist(&partial, &target).map_err(|source| DbError::Io {
                path: target.clone(),
                source,
            })
        });
    if let Err(e) = written {
        let _ = std::fs::remove_file(&partial);
        return Err(e);
    }
    // The rename is only durable once the folder's entry is on the disk.
    #[cfg(unix)]
    if let Err(error) = std::fs::File::open(dir).and_then(|d| d.sync_all()) {
        tracing::warn!(%error, dir = %dir.display(), "cannot flush the backups folder");
    }
    tracing::info!(backup = %target.display(), "library backup written");
    Ok(target)
}

/// One consistent snapshot of the database at `db_path` into `to`, from a read-only connection
/// of its own.
fn copy(db_path: &Path, to: &Path) -> DbResult<()> {
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let conn = Connection::open_with_flags(db_path, flags)?;
    conn.pragma_update(None, "busy_timeout", 5_000)?;
    conn.execute("VACUUM INTO ?1", [to.to_string_lossy().as_ref()])
        .map_err(|e| DbError::Backup(format!("cannot write {}: {e}", to.display())))?;
    Ok(())
}

/// Keeps the newest `keep` daily backups in `dir` and removes the older ones. Only files named
/// exactly `library-YYYY-MM-DD.db` count; folders such as `pre-migration`, partial files and any
/// other name are never touched. A file that cannot be removed is logged and tried again next
/// time. Returns how many were removed.
///
/// # Errors
/// The folder cannot be read.
pub fn prune(dir: &Path, keep: usize) -> DbResult<usize> {
    let files = daily_files(dir)?;
    let old = files.len().saturating_sub(keep);
    let mut removed = 0;
    for path in &files[..old] {
        match cv_fs::remove_own_file(path) {
            Ok(()) => removed += 1,
            Err(error) => {
                tracing::warn!(%error, path = %path.display(), "cannot remove an old backup");
            }
        }
    }
    Ok(removed)
}

/// The copy must open, pass SQLite's integrity check and carry the schema `version`.
pub(crate) fn verify(copy: &Path, version: u32) -> DbResult<()> {
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
