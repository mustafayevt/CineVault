//! Scan fingerprints and the stored view of a data source that an incremental rescan compares
//! against.
//!
//! A [`Fingerprint`] records what the scanner last saw of one file: its size, modification time
//! and (where the platform gives it cheaply) its file identity. A rescan re-reads only the files
//! whose fingerprint differs. Fingerprints are derived data and are never referenced by an item,
//! so deleting them loses nothing; they are not library rows.
//!
//! [`library_snapshot`] reads the items of one data source with the files recorded for them, in
//! two queries, so the rescan can decide what changed without loading whole items.

use std::collections::HashMap;

use cv_domain::{Availability, ItemId, ItemKind, MediaFileType};
use rusqlite::{Connection, params};

use super::codec::parse_enum;
use crate::error::{DbError, DbResult};

/// What the scanner last saw of one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fingerprint {
    /// The absolute path, in native syntax.
    pub path: String,
    pub size_bytes: u64,
    /// Modification time, epoch milliseconds.
    pub modified_ms: i64,
    /// Inode and device on Unix; empty where the platform does not expose it cheaply.
    pub file_id: String,
}

impl Fingerprint {
    /// True when `other` describes the same file contents as far as a scan can tell: the same
    /// size and modification time, and the same file identity when both sides know it.
    pub fn same_as(&self, other: &Fingerprint) -> bool {
        self.size_bytes == other.size_bytes
            && self.modified_ms == other.modified_ms
            && (self.file_id.is_empty()
                || other.file_id.is_empty()
                || self.file_id == other.file_id)
    }
}

fn to_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn to_u64(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

/// Lists the fingerprints recorded under a data-source root.
pub fn list_fingerprints(conn: &Connection, data_source: &str) -> DbResult<Vec<Fingerprint>> {
    let mut stmt = conn.prepare_cached(
        "SELECT path, size_bytes, modified_ms, file_id FROM fingerprint \
         WHERE data_source = ?1 COLLATE NOCASE ORDER BY path",
    )?;
    let rows = stmt
        .query_map([data_source], |r| {
            Ok(Fingerprint {
                path: r.get(0)?,
                size_bytes: to_u64(r.get(1)?),
                modified_ms: r.get(2)?,
                file_id: r.get(3)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Inserts or replaces fingerprints under a data-source root. Returns how many rows were
/// written. `recorded_ms` is the time stamped on them.
pub fn upsert_fingerprints(
    conn: &Connection,
    data_source: &str,
    fingerprints: &[Fingerprint],
    recorded_ms: i64,
) -> DbResult<usize> {
    let mut stmt = conn.prepare_cached(
        "INSERT INTO fingerprint (path, data_source, size_bytes, modified_ms, file_id, recorded_ms) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
         ON CONFLICT (path) DO UPDATE SET data_source = excluded.data_source, \
             size_bytes = excluded.size_bytes, modified_ms = excluded.modified_ms, \
             file_id = excluded.file_id, recorded_ms = excluded.recorded_ms",
    )?;
    for f in fingerprints {
        stmt.execute(params![
            f.path,
            data_source,
            to_i64(f.size_bytes),
            f.modified_ms,
            f.file_id,
            recorded_ms
        ])?;
    }
    Ok(fingerprints.len())
}

/// Deletes the fingerprints of the given paths. Returns how many rows were deleted.
pub fn delete_fingerprints(conn: &Connection, paths: &[String]) -> DbResult<usize> {
    let mut stmt = conn.prepare_cached("DELETE FROM fingerprint WHERE path = ?1")?;
    let mut deleted = 0;
    for p in paths {
        deleted += stmt.execute([p])?;
    }
    Ok(deleted)
}

/// Deletes every fingerprint under a data-source root. Returns how many rows were deleted.
pub fn delete_fingerprints_of_source(conn: &Connection, data_source: &str) -> DbResult<usize> {
    Ok(conn.execute(
        "DELETE FROM fingerprint WHERE data_source = ?1 COLLATE NOCASE",
        [data_source],
    )?)
}

/// The number of stored fingerprints.
pub fn count_fingerprints(conn: &Connection) -> DbResult<u64> {
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM fingerprint", [], |r| r.get(0))?;
    Ok(to_u64(n))
}

/// One file recorded for a stored item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredFile {
    pub kind: MediaFileType,
    pub directory: String,
    pub filename: String,
    pub size_bytes: u64,
    pub modified_ms: i64,
    /// Media info was read for this file.
    pub has_media_info: bool,
}

/// A movie, show, season or episode of a data source with its recorded files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredItem {
    pub id: ItemId,
    pub kind: ItemKind,
    pub parent_id: Option<ItemId>,
    pub path: String,
    pub title: String,
    pub year: u16,
    pub multi_movie_dir: bool,
    pub availability: Availability,
    /// The season number of a season.
    pub season_number: Option<u32>,
    /// The aired season and episode number of an episode.
    pub aired: Option<(u32, u32)>,
    /// In the item's own order.
    pub files: Vec<StoredFile>,
}

/// Reads the movies, shows, seasons and episodes stored under a data-source root, with their
/// files.
pub fn library_snapshot(conn: &Connection, data_source: &str) -> DbResult<Vec<StoredItem>> {
    let mut items: Vec<StoredItem> = Vec::new();
    let mut index: HashMap<i64, usize> = HashMap::new();
    {
        let mut stmt = conn.prepare_cached(
            "SELECT i.id, i.kind, i.parent_id, i.path, i.title, i.year, i.multi_movie_dir, \
                    i.availability, i.season_number, n.season, n.episode \
             FROM item i LEFT JOIN episode_number n \
                 ON n.item_id = i.id AND n.ordering = 'aired' AND n.group_name = '' \
             WHERE i.data_source = ?1 COLLATE NOCASE \
               AND i.kind IN ('movie', 'show', 'season', 'episode') ORDER BY i.id",
        )?;
        let mut rows = stmt.query([data_source])?;
        while let Some(r) = rows.next()? {
            let id: i64 = r.get(0)?;
            let kind: String = r.get(1)?;
            let availability: String = r.get(7)?;
            index.insert(id, items.len());
            items.push(StoredItem {
                id: ItemId(id),
                kind: parse_enum(&kind, "item kind")?,
                parent_id: r.get::<_, Option<i64>>(2)?.map(ItemId),
                path: r.get(3)?,
                title: r.get(4)?,
                year: u16::try_from(r.get::<_, i64>(5)?).unwrap_or(0),
                multi_movie_dir: r.get(6)?,
                availability: Availability::parse(&availability).ok_or_else(|| {
                    DbError::InvalidData(format!("unknown availability `{availability}`"))
                })?,
                season_number: r
                    .get::<_, Option<i64>>(8)?
                    .and_then(|n| u32::try_from(n).ok()),
                aired: match (r.get::<_, Option<i64>>(9)?, r.get::<_, Option<i64>>(10)?) {
                    (Some(season), Some(episode)) => {
                        u32::try_from(season).ok().zip(u32::try_from(episode).ok())
                    }
                    _ => None,
                },
                files: Vec::new(),
            });
        }
    }
    let mut stmt = conn.prepare_cached(
        "SELECT m.item_id, m.kind, m.directory, m.filename, m.size_bytes, m.modified_ms, \
                m.container_format <> '' \
         FROM media_file m JOIN item i ON i.id = m.item_id \
         WHERE i.data_source = ?1 COLLATE NOCASE \
           AND i.kind IN ('movie', 'show', 'season', 'episode') \
         ORDER BY m.item_id, m.position, m.id",
    )?;
    let mut rows = stmt.query([data_source])?;
    while let Some(r) = rows.next()? {
        let item_id: i64 = r.get(0)?;
        let kind: String = r.get(1)?;
        let Some(&slot) = index.get(&item_id) else {
            continue;
        };
        items[slot].files.push(StoredFile {
            kind: parse_enum(&kind, "media file kind")?,
            directory: r.get(2)?,
            filename: r.get(3)?,
            size_bytes: to_u64(r.get(4)?),
            modified_ms: r.get(5)?,
            has_media_info: r.get(6)?,
        });
    }
    Ok(items)
}
