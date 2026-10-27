//! Reads the version of a library file from its header, without opening it with SQLite.
//!
//! Opening a database with SQLite can create `-wal` and `-shm` files next to it and checkpoint
//! them, which changes the folder and possibly the file. A file of an earlier build must stay
//! exactly as it was, so its version is read from the raw header: the
//! 16-byte magic string `SQLite format 3\0`, and `user_version` as a big-endian `u32` at
//! offset 60.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use crate::schema;

/// The magic string at the start of every SQLite database file.
const MAGIC: &[u8; 16] = b"SQLite format 3\0";

/// Where `user_version` sits in the header.
const USER_VERSION_OFFSET: usize = 60;

/// What the file at the library path is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryFile {
    /// No file: a new library is created.
    Missing,
    /// A file of zero bytes: SQLite treats it as an empty database, a new library is created.
    Empty,
    /// A database of this build's epoch (`user_version` 100 or later, up to what it supports).
    Current { version: u32 },
    /// A database written by a newer build: refused untouched.
    Newer { version: u32 },
    /// A file of an earlier build: `user_version` 1 to 99, or 0 with content, or content that is
    /// not a SQLite database (`version` is `None` then). It is set aside, never opened.
    Earlier { version: Option<u32> },
}

/// Classifies the file at `path` for a build that supports schema versions up to `supported`.
///
/// Reads at most 100 bytes; never writes, never opens the file with SQLite.
pub fn classify(path: &Path, supported: u32) -> io::Result<LibraryFile> {
    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(LibraryFile::Missing),
        Err(e) => return Err(e),
    };
    let mut head = Vec::with_capacity(100);
    (&mut file).take(100).read_to_end(&mut head)?;
    if head.is_empty() {
        return Ok(LibraryFile::Empty);
    }
    let Some(version) = user_version(&head) else {
        return Ok(LibraryFile::Earlier { version: None });
    };
    Ok(if version > supported {
        LibraryFile::Newer { version }
    } else if version < schema::BASE_VERSION {
        // 1 to 99, or 0 on a file with content: both come from an earlier build.
        LibraryFile::Earlier {
            version: Some(version),
        }
    } else {
        LibraryFile::Current { version }
    })
}

/// The `user_version` in a SQLite header, or `None` when `head` is not one.
fn user_version(head: &[u8]) -> Option<u32> {
    if head.len() < USER_VERSION_OFFSET + 4 || &head[..MAGIC.len()] != MAGIC {
        return None;
    }
    let bytes: [u8; 4] = head[USER_VERSION_OFFSET..USER_VERSION_OFFSET + 4]
        .try_into()
        .ok()?;
    Some(u32::from_be_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn db_with_version(dir: &Path, version: u32) -> std::path::PathBuf {
        let path = dir.join("library.db");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE t (x INTEGER);").unwrap();
        conn.pragma_update(None, "user_version", version).unwrap();
        path
    }

    #[test]
    fn missing_and_empty_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        assert_eq!(classify(&path, 100).unwrap(), LibraryFile::Missing);
        std::fs::write(&path, b"").unwrap();
        assert_eq!(classify(&path, 100).unwrap(), LibraryFile::Empty);
    }

    #[test]
    fn versions_by_epoch() {
        for (version, expected) in [
            (0, LibraryFile::Earlier { version: Some(0) }),
            (17, LibraryFile::Earlier { version: Some(17) }),
            (99, LibraryFile::Earlier { version: Some(99) }),
            (100, LibraryFile::Current { version: 100 }),
            (101, LibraryFile::Newer { version: 101 }),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = db_with_version(dir.path(), version);
            assert_eq!(classify(&path, 100).unwrap(), expected, "version {version}");
        }
    }

    #[test]
    fn content_that_is_not_sqlite_is_from_an_earlier_build() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        std::fs::write(&path, b"not a database").unwrap();
        assert_eq!(
            classify(&path, 100).unwrap(),
            LibraryFile::Earlier { version: None }
        );
    }
}
