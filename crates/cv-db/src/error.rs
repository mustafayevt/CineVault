//! Errors of the storage layer.

use std::path::PathBuf;

/// Everything that can go wrong while opening or using the library database.
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    /// An error reported by SQLite.
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    /// A file-system error, for example while creating the data folder.
    #[error("cannot prepare the database location {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// A schema migration failed. The transaction was rolled back, so the database is unchanged.
    /// `backup` is the pre-migration copy taken just before, when there was data to protect.
    #[error("migration {version} ({name}) failed and was rolled back; the library is unchanged{}: {source}", backup_hint(.backup))]
    Migration {
        version: u32,
        name: &'static str,
        #[source]
        source: rusqlite::Error,
        backup: Option<PathBuf>,
    },

    /// The pre-migration backup could not be made or did not verify, so the upgrade was not run.
    #[error("the pre-migration backup failed, so the library was not upgraded: {0}")]
    Backup(String),

    /// The database was written by a newer CineVault than this build.
    #[error("the database has schema version {found}, but this build supports up to {supported}")]
    SchemaTooNew { found: u32, supported: u32 },

    /// The database was written by an earlier CineVault (schema version 1 to 99, or 0 with
    /// content). It is never opened or upgraded; the app sets it aside as `library-old.db`.
    #[error(
        "the database has schema version {found}, from an earlier CineVault; it is never upgraded"
    )]
    SchemaFromEarlierVersion { found: u32 },

    /// SQLite did not switch the database to write-ahead logging.
    #[error("the database could not be switched to WAL mode (got `{mode}`)")]
    WalUnavailable { mode: String },

    /// The writer thread has stopped, so no more writes are possible.
    #[error("the database writer thread is not running")]
    WriterStopped,

    /// A write operation panicked; its transaction was rolled back.
    #[error("a database write panicked and was rolled back")]
    WriterPanicked,

    /// A stored value cannot be read back into the domain model (unknown enum text, bad JSON).
    #[error("stored data is invalid: {0}")]
    InvalidData(String),

    /// JSON encoding or decoding of a stored document failed.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// The item does not exist.
    #[error("item {0} does not exist")]
    ItemNotFound(i64),

    /// An update was attempted on an item that has not been stored yet.
    #[error("the item has no id yet, so it cannot be updated")]
    ItemWithoutId,

    /// A show, season or other parent still owns items; delete them (or the whole tree) first.
    #[error("item {id} still has {count} child item(s)")]
    HasChildren { id: i64, count: i64 },

    /// A paging cursor does not fit the query it was used with.
    #[error("the paging cursor does not match this query")]
    InvalidCursor,

    /// A filter cannot be applied: an unknown id, or one that does not exist for this library.
    #[error("invalid filter: {0}")]
    InvalidFilter(String),

    /// Another source of the same library type already has this label.
    #[error("Another {kind} source already uses this label")]
    SourceLabelTaken { kind: String, label: String },

    /// A source label must have a visible character.
    #[error("A source label cannot be empty")]
    SourceLabelEmpty,

    /// The data source does not exist.
    #[error("data source {0} does not exist")]
    SourceNotFound(i64),

    /// A reader connection was poisoned by a panic in another thread.
    #[error("the reader pool is unusable after a panic in another thread")]
    PoolPoisoned,
}

/// Result alias of this crate.
pub type DbResult<T> = Result<T, DbError>;

fn backup_hint(backup: &Option<PathBuf>) -> String {
    backup.as_ref().map_or_else(String::new, |p| {
        format!(" (a backup is at {})", p.display())
    })
}
