//! SQLite storage: schema, migrations, repositories and full-text search.
//!
//! [`Database`] opens the library file in WAL mode with one writer thread and a pool of
//! read-only connections:
//!
//! - [`Database::write`] runs a closure inside an immediate transaction on the writer thread and
//!   commits when it returns `Ok`; an `Err` or a panic rolls back.
//! - [`Database::read`] runs a closure on a pooled read-only connection.
//!
//! The schema lives in `schema.sql` (version 100). When the database is opened, [`migrate`] first
//! reads the file header ([`header`]) and refuses a file of an earlier or a newer build untouched,
//! then upgrades the file: a verified backup goes to `backups/pre-migration/`, then all pending
//! migrations run in one transaction, so a failure leaves the file untouched.
//! [`repo`] holds the repositories on top of it: item CRUD, composable filters and sorts with
//! keyset paging, and the full-text index.

mod connection;
mod error;
pub mod header;
pub mod migrate;
mod pool;
pub mod repo;
pub mod schema;
mod writer;

use std::path::{Path, PathBuf};

use rusqlite::{Connection, Transaction, TransactionBehavior};

pub use error::{DbError, DbResult};
pub use migrate::MigrationReport;
pub use schema::SCHEMA_VERSION;

use pool::ReaderPool;
use writer::Writer;

/// Options for [`Database::open_with`].
#[derive(Debug, Clone)]
pub struct DbOptions {
    /// Number of pooled read connections (at least 1).
    pub readers: usize,
}

impl Default for DbOptions {
    fn default() -> Self {
        Self { readers: 4 }
    }
}

/// The library database.
pub struct Database {
    path: PathBuf,
    migration: MigrationReport,
    // Field order matters: the writer thread is joined first, then the readers close.
    writer: Writer,
    readers: ReaderPool,
}

impl Database {
    /// Opens (creating and migrating if needed) the database at `path` with default options.
    pub fn open(path: impl AsRef<Path>) -> DbResult<Self> {
        Self::open_with(path, &DbOptions::default())
    }

    /// Opens the database at `path`. The parent folder is created when missing.
    ///
    /// The schema is created from scratch for an empty file and migrated for an older one, before
    /// any reader or writer is handed out. A file from a newer or an earlier build is refused
    /// untouched (an earlier one is never opened with SQLite).
    pub fn open_with(path: impl AsRef<Path>, options: &DbOptions) -> DbResult<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|source| DbError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        // Upgrade on a plain connection before WAL is switched on or any reader exists, so a
        // failed migration leaves the file exactly as it was.
        let migration = migrate::upgrade_file(&path, schema::MIGRATIONS)?;
        let write_conn = connection::open_writer(&path)?;

        let readers = (0..options.readers.max(1))
            .map(|_| connection::open_reader(&path))
            .collect::<DbResult<Vec<_>>>()?;

        Ok(Self {
            path,
            migration,
            writer: Writer::spawn(write_conn)?,
            readers: ReaderPool::new(readers),
        })
    }

    /// The database file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// What the migration runner did when this database was opened.
    pub fn migration_report(&self) -> &MigrationReport {
        &self.migration
    }

    /// The schema version of the open database.
    pub fn schema_version(&self) -> DbResult<u32> {
        self.read(schema::current_version)
    }

    /// Runs `f` on a pooled read-only connection.
    ///
    /// Readers never block on the writer and never see uncommitted data.
    pub fn read<T>(&self, f: impl FnOnce(&Connection) -> DbResult<T>) -> DbResult<T> {
        let conn = self.readers.get()?;
        f(&conn)
    }

    /// Runs `f` in an immediate transaction on the writer thread.
    ///
    /// The transaction commits (durably) when `f` returns `Ok` and rolls back on `Err` or panic.
    /// The closure must own its data (`'static`) because it runs on another thread.
    pub fn write<T, F>(&self, f: F) -> DbResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&Transaction<'_>) -> DbResult<T> + Send + 'static,
    {
        self.writer.run(move |conn| {
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let out = f(&tx)?;
            tx.commit()?;
            Ok(out)
        })
    }
}

impl std::fmt::Debug for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Database")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod migrate_tests;
#[cfg(test)]
mod tests;
