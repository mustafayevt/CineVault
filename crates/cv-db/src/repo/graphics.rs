//! Image files recorded for items, for the background thumbnail job.

use std::collections::{HashMap, HashSet};

use cv_domain::{ItemId, MediaFileType};
use rusqlite::Connection;

use super::codec::{enum_text, parse_enum};
use crate::error::DbResult;

/// An image file recorded for an item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphicFile {
    pub item_id: ItemId,
    pub kind: MediaFileType,
    pub directory: String,
    pub filename: String,
}

/// The files of the given `kinds` recorded for the items in `roots` **and their descendants**
/// (a show brings its seasons and episodes), in item then file order.
///
/// A rescan reports new shows and episodes but not their seasons, so following the parent links
/// is what makes a new show's season posters reach the thumbnail job.
pub fn graphic_files_of(
    conn: &Connection,
    roots: &[ItemId],
    kinds: &[MediaFileType],
) -> DbResult<Vec<GraphicFile>> {
    if roots.is_empty() || kinds.is_empty() {
        return Ok(Vec::new());
    }
    let root_ids = serde_json::to_string(&roots.iter().map(|id| id.0).collect::<Vec<_>>())?;
    let kind_names = serde_json::to_string(
        &kinds
            .iter()
            .map(enum_text)
            .collect::<DbResult<Vec<String>>>()?,
    )?;
    let mut stmt = conn.prepare_cached(
        "WITH RECURSIVE tree(id) AS ( \
             SELECT value FROM json_each(?1) \
             UNION \
             SELECT i.id FROM item i JOIN tree t ON i.parent_id = t.id \
         ) \
         SELECT m.item_id, m.kind, m.directory, m.filename \
         FROM media_file m \
         WHERE m.item_id IN (SELECT id FROM tree) \
           AND m.kind IN (SELECT value FROM json_each(?2)) \
         ORDER BY m.item_id, m.position, m.id",
    )?;
    let mut rows = stmt.query([&root_ids, &kind_names])?;
    let mut files = Vec::new();
    while let Some(r) = rows.next()? {
        let kind: String = r.get(1)?;
        files.push(GraphicFile {
            item_id: ItemId(r.get(0)?),
            kind: parse_enum(&kind, "media file kind")?,
            directory: r.get(2)?,
            filename: r.get(3)?,
        });
    }
    Ok(files)
}

/// The poster-like image each of `items` shows in the grid: its first poster file, else (for a
/// season) its season poster, else key art, else square art. Items without one are absent.
///
/// Only the items themselves are read, not their descendants, so a page of 100 cards costs one
/// indexed query.
pub fn poster_files_for(
    conn: &Connection,
    items: &[ItemId],
) -> DbResult<HashMap<ItemId, GraphicFile>> {
    Ok(poster_files_with_state(conn, items)?
        .into_iter()
        .map(|(item, poster)| (item, poster.file))
        .collect())
}

/// A grid poster with what the library knows about it without touching the disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PosterFile {
    pub file: GraphicFile,
    /// The size the scan or the artwork writer recorded, 0 when unknown.
    pub size_bytes: u64,
    /// The modification time they recorded (epoch milliseconds), 0 when unknown.
    pub modified_ms: i64,
    /// The item's folder is not reachable (offline, missing or invalid).
    pub unavailable: bool,
}

/// Like [`poster_files_for`], with each file's recorded size and modification time and whether
/// the item is available, so the grid can trust what it learned about a file without reading
/// the disk.
pub fn poster_files_with_state(
    conn: &Connection,
    items: &[ItemId],
) -> DbResult<HashMap<ItemId, PosterFile>> {
    if items.is_empty() {
        return Ok(HashMap::new());
    }
    let ids = serde_json::to_string(&items.iter().map(|id| id.0).collect::<Vec<_>>())?;
    let mut stmt = conn.prepare_cached(
        "SELECT m.item_id, m.kind, m.directory, m.filename, m.size_bytes, m.modified_ms, \
                i.availability \
         FROM media_file m JOIN item i ON i.id = m.item_id \
         WHERE m.item_id IN (SELECT value FROM json_each(?1)) \
           AND m.kind IN ('poster', 'season_poster', 'keyart', 'squareart') \
         ORDER BY m.item_id, \
           CASE m.kind WHEN 'poster' THEN 0 WHEN 'season_poster' THEN 1 \
                       WHEN 'keyart' THEN 2 ELSE 3 END, \
           m.position, m.id",
    )?;
    let mut rows = stmt.query([&ids])?;
    let mut found: HashMap<ItemId, PosterFile> = HashMap::new();
    while let Some(r) = rows.next()? {
        let item_id = ItemId(r.get(0)?);
        if found.contains_key(&item_id) {
            continue;
        }
        let kind: String = r.get(1)?;
        let size: i64 = r.get(4)?;
        let availability: String = r.get(6)?;
        found.insert(
            item_id,
            PosterFile {
                file: GraphicFile {
                    item_id,
                    kind: parse_enum(&kind, "media file kind")?,
                    directory: r.get(2)?,
                    filename: r.get(3)?,
                },
                size_bytes: u64::try_from(size).unwrap_or(0),
                modified_ms: r.get(5)?,
                unavailable: availability != "online",
            },
        );
    }
    Ok(found)
}

/// The folders of the movies of each collection (movie set) among `items`, earliest release
/// first: where a `movieset-poster.*` of the Kodi movie-folder style sits. The scan does not
/// record those files (they belong to the collection, not the movie), so the grid looks for them
/// in these folders when a collection has no poster of its own. Items that are not collections,
/// and collections without a movie with a video, are absent.
pub fn set_member_folders(
    conn: &Connection,
    items: &[ItemId],
) -> DbResult<HashMap<ItemId, Vec<String>>> {
    if items.is_empty() {
        return Ok(HashMap::new());
    }
    let ids = serde_json::to_string(&items.iter().map(|id| id.0).collect::<Vec<_>>())?;
    let mut stmt = conn.prepare_cached(
        "SELECT s.id, m.directory \
         FROM item s \
         JOIN item c ON c.parent_id = s.id \
         JOIN media_file m ON m.item_id = c.id AND m.kind = 'video' \
         WHERE s.id IN (SELECT value FROM json_each(?1)) AND s.kind = 'set' \
         ORDER BY s.id, c.year = 0, c.year, c.id, m.position, m.id",
    )?;
    let mut rows = stmt.query([&ids])?;
    let mut folders: HashMap<ItemId, Vec<String>> = HashMap::new();
    while let Some(r) = rows.next()? {
        let set = ItemId(r.get(0)?);
        let directory: String = r.get(1)?;
        let list = folders.entry(set).or_default();
        if !list.contains(&directory) {
            list.push(directory);
        }
    }
    Ok(folders)
}

/// How many movies and shows no provider has identified (no row in `ext_id`): the titles the
/// grid marks unmatched.
pub fn count_unmatched(conn: &Connection) -> DbResult<u64> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM item i WHERE i.kind IN ('movie', 'show') AND i.trashed_entry IS NULL \
         AND NOT EXISTS (SELECT 1 FROM ext_id e WHERE e.item_id = i.id)",
        [],
        |r| r.get(0),
    )?;
    Ok(u64::try_from(n).unwrap_or(0))
}

/// The items among `items` that no provider has identified: they have no row in `ext_id`.
/// The grid marks them as unmatched.
pub fn unmatched_among(conn: &Connection, items: &[ItemId]) -> DbResult<HashSet<ItemId>> {
    if items.is_empty() {
        return Ok(HashSet::new());
    }
    let ids = serde_json::to_string(&items.iter().map(|id| id.0).collect::<Vec<_>>())?;
    let mut stmt = conn.prepare_cached(
        "SELECT j.value FROM json_each(?1) j          WHERE NOT EXISTS (SELECT 1 FROM ext_id e WHERE e.item_id = j.value)",
    )?;
    let ids = stmt
        .query_map([&ids], |r| r.get::<_, i64>(0))?
        .map(|r| r.map(ItemId).map_err(Into::into))
        .collect::<DbResult<HashSet<_>>>()?;
    Ok(ids)
}

/// The name an unmatched card shows for each of `items`: the item's folder name, or for a movie
/// that shares its folder with others (a multi-movie folder, loose videos in a `Season 1` folder)
/// its main video's file name, since the folder name is the same on every such card.
pub fn card_file_names(conn: &Connection, items: &[ItemId]) -> DbResult<HashMap<ItemId, String>> {
    if items.is_empty() {
        return Ok(HashMap::new());
    }
    let ids = serde_json::to_string(&items.iter().map(|id| id.0).collect::<Vec<_>>())?;
    let mut stmt = conn.prepare_cached(
        "SELECT i.id, i.path, i.multi_movie_dir, \
                (SELECT m.filename FROM media_file m \
                 WHERE m.item_id = i.id AND m.kind = 'video' \
                 ORDER BY m.position, m.id LIMIT 1) \
         FROM item i WHERE i.id IN (SELECT value FROM json_each(?1))",
    )?;
    let mut rows = stmt.query([&ids])?;
    let mut found = HashMap::new();
    while let Some(r) = rows.next()? {
        let id = ItemId(r.get(0)?);
        let path: String = r.get(1)?;
        let shared_folder: bool = r.get(2)?;
        let video: Option<String> = r.get(3)?;
        let folder = path
            .trim_end_matches(['/', '\\'])
            .rsplit(['/', '\\'])
            .next()
            .filter(|name| !name.is_empty())
            .map(str::to_owned);
        let name = match (shared_folder, video) {
            (true, Some(video)) if !video.is_empty() => Some(video),
            _ => folder,
        };
        if let Some(name) = name {
            found.insert(id, name);
        }
    }
    Ok(found)
}

/// What a grid card says about an item's main video file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoFacts {
    /// Frame width in pixels, `0` when unknown.
    pub width: u32,
    /// Frame height in pixels, `0` when unknown.
    pub height: u32,
    /// HDR formats such as `Dolby Vision` or `HDR10`; empty for SDR.
    pub hdr_formats: Vec<String>,
}

/// The first video file of each of `items` (by position), for the card's meta line. Items without
/// a video file are absent.
pub fn video_facts_for(
    conn: &Connection,
    items: &[ItemId],
) -> DbResult<HashMap<ItemId, VideoFacts>> {
    if items.is_empty() {
        return Ok(HashMap::new());
    }
    let ids = serde_json::to_string(&items.iter().map(|id| id.0).collect::<Vec<_>>())?;
    let mut stmt = conn.prepare_cached(
        "SELECT m.item_id, m.height, m.hdr_formats, m.width          FROM media_file m          WHERE m.item_id IN (SELECT value FROM json_each(?1)) AND m.kind = 'video'          ORDER BY m.item_id, m.position, m.id",
    )?;
    let mut rows = stmt.query([&ids])?;
    let mut found: HashMap<ItemId, VideoFacts> = HashMap::new();
    while let Some(r) = rows.next()? {
        let item_id = ItemId(r.get(0)?);
        if found.contains_key(&item_id) {
            continue;
        }
        let hdr: String = r.get(2)?;
        found.insert(
            item_id,
            VideoFacts {
                width: u32::try_from(r.get::<_, i64>(3)?).unwrap_or(0),
                height: u32::try_from(r.get::<_, i64>(1)?).unwrap_or(0),
                hdr_formats: serde_json::from_str(&hdr)?,
            },
        );
    }
    Ok(found)
}
