//! The repair of "Date added" for titles a first scan stamped all at once.
//!
//! Earlier builds dated every title with the time the scan saw it, so a library added in one scan
//! has hundreds of titles sharing one value and the Date added sort and column say nothing. A
//! shared value is the sign: many movies and shows with exactly the same `date_added_ms`. This
//! module lists those titles with the videos that can date them and applies the new dates; the
//! file times themselves are read by the caller, outside the database.

use cv_domain::ItemId;
use rusqlite::{Connection, params};

use crate::error::DbResult;

/// A title whose date added is shared with many others.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedDateTitle {
    pub item: ItemId,
    /// `movie` or `show`.
    pub kind: String,
    /// The date it has now, shared.
    pub date_added_ms: i64,
    /// The folder and name of its video files: the main one for a movie, every episode's for a
    /// show.
    pub videos: Vec<(String, String)>,
}

/// The movies and shows whose `date_added_ms` is held by at least `min_rows` titles, with their
/// videos. Titles in the Recycle Bin and titles without a date (0) are not listed.
///
/// # Errors
/// The library cannot be read.
pub fn titles_with_shared_date(
    conn: &Connection,
    min_rows: usize,
) -> DbResult<Vec<SharedDateTitle>> {
    let mut stmt = conn.prepare(
        "SELECT i.id, i.kind, i.date_added_ms FROM item i
         WHERE i.kind IN ('movie', 'show') AND i.trashed_entry IS NULL
           AND i.date_added_ms IN (
               SELECT date_added_ms FROM item
               WHERE kind IN ('movie', 'show') AND trashed_entry IS NULL AND date_added_ms > 0
               GROUP BY date_added_ms HAVING COUNT(*) >= ?1)
         ORDER BY i.id",
    )?;
    let titles = stmt
        .query_map([i64::try_from(min_rows).unwrap_or(i64::MAX)], |row| {
            Ok((
                ItemId(row.get(0)?),
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut movie_video = conn.prepare(
        "SELECT directory, filename FROM media_file
         WHERE item_id = ?1 AND kind = 'video' ORDER BY position, id LIMIT 1",
    )?;
    let mut show_videos = conn.prepare(
        "WITH RECURSIVE tree (id) AS (
             SELECT ?1
             UNION ALL
             SELECT i.id FROM item i JOIN tree t ON i.parent_id = t.id)
         SELECT m.directory, m.filename FROM media_file m JOIN tree t ON m.item_id = t.id
         WHERE m.kind = 'video'",
    )?;
    let mut listed = Vec::with_capacity(titles.len());
    for (item, kind, date_added_ms) in titles {
        let query = if kind == "movie" {
            &mut movie_video
        } else {
            &mut show_videos
        };
        let videos = query
            .query_map([item.0], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<Vec<(String, String)>, _>>()?;
        listed.push(SharedDateTitle {
            item,
            kind,
            date_added_ms,
            videos,
        });
    }
    Ok(listed)
}

/// Sets the new dates, `(item, old date, new date)`: a title whose date changed meanwhile keeps
/// it. Returns how many titles changed.
///
/// # Errors
/// The library cannot be written.
pub fn apply_date_added(conn: &Connection, changes: &[(ItemId, i64, i64)]) -> DbResult<usize> {
    let mut stmt = conn.prepare_cached(
        "UPDATE item SET date_added_ms = ?1 WHERE id = ?2 AND date_added_ms = ?3",
    )?;
    let mut changed = 0;
    for (item, old, new) in changes {
        changed += stmt.execute(params![new, item.0, old])?;
    }
    Ok(changed)
}
