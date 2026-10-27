//! Opening and configuring SQLite connections.

use std::path::Path;

use rusqlite::{Connection, OpenFlags};

use crate::error::{DbError, DbResult};

/// Pragmas every connection gets.
fn configure_common(conn: &Connection) -> rusqlite::Result<()> {
    conn.pragma_update(None, "foreign_keys", true)?;
    conn.pragma_update(None, "busy_timeout", 5_000)?;
    conn.pragma_update(None, "temp_store", "MEMORY")?;
    // Saving one item runs ~40 distinct statements; the default cache (16) would re-prepare most
    // of them on every call.
    conn.set_prepared_statement_cache_capacity(128);
    crate::repo::search::register_functions(conn)?;
    Ok(())
}

fn flags() -> OpenFlags {
    OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_CREATE
        | OpenFlags::SQLITE_OPEN_NO_MUTEX
        | OpenFlags::SQLITE_OPEN_URI
}

/// Opens the single write connection: WAL, full synchronous commits.
///
/// `synchronous = FULL` makes a committed change survive a crash or power loss, which the
/// storage requirements demand; WAL keeps readers unblocked while it writes.
pub(crate) fn open_writer(path: &Path) -> DbResult<Connection> {
    let conn = Connection::open_with_flags(path, flags())?;
    configure_common(&conn)?;
    let mode: String = conn.pragma_update_and_check(None, "journal_mode", "WAL", |r| r.get(0))?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(DbError::WalUnavailable { mode });
    }
    conn.pragma_update(None, "synchronous", "FULL")?;
    conn.pragma_update(None, "journal_size_limit", 64 * 1024 * 1024)?;
    conn.pragma_update(None, "cache_size", -32_768)?;
    Ok(conn)
}

/// Opens a reader: it can never write, and uses memory-mapped reads.
pub(crate) fn open_reader(path: &Path) -> DbResult<Connection> {
    let conn = Connection::open_with_flags(path, flags())?;
    configure_common(&conn)?;
    conn.pragma_update(None, "query_only", true)?;
    conn.pragma_update(None, "cache_size", -16_384)?;
    conn.pragma_update(None, "mmap_size", 256 * 1024 * 1024)?;
    Ok(conn)
}
