//! Which videos still wait for their media info, and how many titles each data source holds.
//!
//! A video has its media info once the container format is recorded (`MediaFile::has_media_info`).
//! The scan stores new videos without it, and the media-info job fills it in, so "pending" is
//! simply "a video row with an empty container format". Only online items are listed: a title
//! whose folder is gone or whose drive is unplugged cannot be read.
//!
//! A video that could not be read is remembered as *unreadable at its size and modification time*
//! (`media_unreadable`). Such a video is not pending while the mark matches, so it is not tried
//! again at every launch; a file that changed no longer matches and is tried again. Only
//! [`clear_unreadable`], which "Reload media info" calls, removes a mark by hand.
//!
//! Two kinds of mark keep the video pending, so it is tried again on the next scan or start:
//! a read that ran out of time ([`TIMED_OUT`]: a busy or sleeping disk, not a broken file), and
//! the "not a recognised media file" of a disc structure's marker file (`BDMV/index.bdmv`), which
//! earlier versions read as a file of its own instead of through the disc's main title. The mark
//! stays, so the details page can say why the details are missing.

use cv_domain::{ItemId, MediaFile};
use rusqlite::{Connection, OptionalExtension, params};

use crate::error::DbResult;

/// The reason of a read that ran out of time. Such a video is tried again.
pub const TIMED_OUT: &str = "read timed out";

/// The reason earlier versions gave a disc structure's marker file. Such a video is tried again.
pub const NOT_MEDIA: &str = "not a recognised media file";

/// True for a `media_file` row `m` (SQL alias) whose video is not marked unreadable at its current
/// size and modification time, a mark that asks for another try aside.
const NOT_MARKED_UNREADABLE: &str = "NOT EXISTS (SELECT 1 FROM media_unreadable u \
     WHERE u.directory = m.directory AND u.filename = m.filename \
       AND u.size_bytes = m.size_bytes AND u.modified_ms = m.modified_ms \
       AND u.reason <> 'read timed out' \
       AND NOT (u.reason = 'not a recognised media file' \
                AND lower(u.filename) IN ('index.bdmv', 'video_ts.ifo', 'hv000i01.ifo')))";

/// True for a `media_file` row `m` (SQL alias) that is not the marker file of a disc structure
/// whose disc folder is a video of the same title (`MediaFile::is_covered_disc_marker`): the
/// folder is read, the marker is not.
const NOT_COVERED_DISC_MARKER: &str = "NOT (lower(m.filename) IN ('index.bdmv', 'video_ts.ifo', 'hv000i01.ifo') \
     AND EXISTS (SELECT 1 FROM media_file d WHERE d.item_id = m.item_id AND d.kind = 'video' \
       AND (d.directory || '\\' || d.filename = m.directory \
            OR d.directory || '/' || d.filename = m.directory)))";

/// An item with at least one video whose media info has not been read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingMedia {
    pub item_id: ItemId,
    pub title: String,
}

/// The online movies and episodes that have a video without media info, in item order. `limit`
/// caps the list (`usize::MAX` for all of them).
pub fn pending_media_info(conn: &Connection, limit: usize) -> DbResult<Vec<PendingMedia>> {
    let limit = i64::try_from(limit).unwrap_or(i64::MAX);
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT i.id, i.title FROM item i \
         WHERE i.availability = 'online' AND i.trashed_entry IS NULL \
           AND i.kind IN ('movie', 'episode') \
           AND EXISTS (SELECT 1 FROM media_file m \
                       WHERE m.item_id = i.id AND m.kind = 'video' AND m.container_format = '' \
                         AND {NOT_MARKED_UNREADABLE} AND {NOT_COVERED_DISC_MARKER}) \
         ORDER BY i.id LIMIT ?1"
    ))?;
    let mut rows = stmt.query([limit])?;
    let mut pending = Vec::new();
    while let Some(row) = rows.next()? {
        pending.push(PendingMedia {
            item_id: ItemId(row.get(0)?),
            title: row.get(1)?,
        });
    }
    Ok(pending)
}

/// What a data source holds, for the sidebar.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SourceCounts {
    /// Movies, or shows for a TV source.
    pub titles: u64,
    /// Titles whose folder or video is gone while the source itself is reachable.
    pub missing: u64,
    /// Titles that wait for media info.
    pub pending_media_info: u64,
    /// Titles the last scan or probe found offline (their library folder did not answer).
    pub offline: u64,
}

/// The counts of the data source with root `root` (compared ignoring case, like the item rows).
pub fn source_counts(conn: &Connection, root: &str) -> DbResult<SourceCounts> {
    let (titles, missing, offline): (i64, i64, i64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(availability = 'missing'), 0), \
                COALESCE(SUM(availability = 'offline'), 0) FROM item \
         WHERE data_source = ?1 COLLATE NOCASE AND kind IN ('movie', 'show') \
           AND trashed_entry IS NULL",
        [root],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let pending: i64 = conn.query_row(
        &format!(
            "SELECT COUNT(DISTINCT i.id) FROM item i JOIN media_file m ON m.item_id = i.id \
             WHERE i.data_source = ?1 COLLATE NOCASE AND i.availability = 'online' \
               AND i.trashed_entry IS NULL \
               AND i.kind IN ('movie', 'episode') AND m.kind = 'video' \
               AND m.container_format = '' AND {NOT_MARKED_UNREADABLE} \
               AND {NOT_COVERED_DISC_MARKER}"
        ),
        [root],
        |r| r.get(0),
    )?;
    Ok(SourceCounts {
        titles: u64::try_from(titles).unwrap_or(0),
        missing: u64::try_from(missing).unwrap_or(0),
        pending_media_info: u64::try_from(pending).unwrap_or(0),
        offline: u64::try_from(offline).unwrap_or(0),
    })
}

/// A video media info could not be read from, at the size and time it was tried at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnreadableMark {
    pub directory: String,
    pub filename: String,
    pub size_bytes: u64,
    pub modified_ms: i64,
    /// The short reason shown in the Activity drawer.
    pub reason: String,
}

/// Remembers that the video could not be read at this size and modification time. A mark that is
/// there already is replaced.
pub fn mark_unreadable(conn: &Connection, mark: &UnreadableMark, now_ms: i64) -> DbResult<()> {
    conn.prepare_cached(
        "INSERT INTO media_unreadable \
            (directory, filename, size_bytes, modified_ms, reason, marked_ms) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
         ON CONFLICT (directory, filename) DO UPDATE SET \
            size_bytes = excluded.size_bytes, modified_ms = excluded.modified_ms, \
            reason = excluded.reason, marked_ms = excluded.marked_ms",
    )?
    .execute(params![
        mark.directory,
        mark.filename,
        i64::try_from(mark.size_bytes).unwrap_or(i64::MAX),
        mark.modified_ms,
        mark.reason,
        now_ms,
    ])?;
    Ok(())
}

/// Removes the marks of these videos (folder, file name). Returns how many were there.
pub fn clear_unreadable(
    conn: &Connection,
    files: impl IntoIterator<Item = (String, String)>,
) -> DbResult<usize> {
    let mut stmt =
        conn.prepare_cached("DELETE FROM media_unreadable WHERE directory = ?1 AND filename = ?2")?;
    let mut removed = 0;
    for (directory, filename) in files {
        removed += stmt.execute(params![directory, filename])?;
    }
    Ok(removed)
}

/// Why the video could not be read, when it is marked unreadable at its current size and
/// modification time.
pub fn unreadable_reason(conn: &Connection, file: &MediaFile) -> DbResult<Option<String>> {
    Ok(conn
        .prepare_cached(
            "SELECT reason FROM media_unreadable \
             WHERE directory = ?1 AND filename = ?2 AND size_bytes = ?3 AND modified_ms = ?4",
        )?
        .query_row(
            params![
                file.directory,
                file.filename,
                i64::try_from(file.size_bytes).unwrap_or(i64::MAX),
                file.modified_ms,
            ],
            |r| r.get(0),
        )
        .optional()?)
}

/// Every mark, by folder and name.
pub fn list_unreadable(conn: &Connection) -> DbResult<Vec<UnreadableMark>> {
    let mut stmt = conn.prepare_cached(
        "SELECT directory, filename, size_bytes, modified_ms, reason FROM media_unreadable \
         ORDER BY directory, filename",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(UnreadableMark {
                directory: r.get(0)?,
                filename: r.get(1)?,
                size_bytes: u64::try_from(r.get::<_, i64>(2)?).unwrap_or(0),
                modified_ms: r.get(3)?,
                reason: r.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use cv_domain::{Item, MediaFile, MediaFileType};

    use super::*;
    use crate::Database;
    use crate::repo::{get_item, insert_item, set_availability, update_item};
    use cv_domain::Availability;

    fn movie(root: &str, name: &str, info: bool) -> Item {
        let mut item = Item::movie(name);
        item.core.data_source = root.to_owned();
        item.core.path = format!("{root}/{name}");
        let mut file = MediaFile::new(MediaFileType::Video, format!("{root}/{name}"), "v.mkv");
        if info {
            file.container_format = "Matroska".into();
        }
        item.core.media_files.push(file);
        item
    }

    #[test]
    fn pending_lists_online_videos_without_media_info() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(dir.path().join("l.db")).unwrap();
        let ids = db
            .write(|tx| {
                let a = insert_item(tx, &movie("/m", "A", false))?;
                let b = insert_item(tx, &movie("/m", "B", true))?;
                let c = insert_item(tx, &movie("/m", "C", false))?;
                set_availability(tx, c, Availability::Missing, "gone")?;
                Ok((a, b, c))
            })
            .unwrap();
        let pending = db.read(|c| pending_media_info(c, usize::MAX)).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].item_id, ids.0);
        assert_eq!(pending[0].title, "A");

        let counts = db.read(|c| source_counts(c, "/M")).unwrap();
        assert_eq!(
            counts,
            SourceCounts {
                titles: 3,
                missing: 1,
                pending_media_info: 1,
                offline: 0,
            }
        );
        let other = db.read(|c| source_counts(c, "/other")).unwrap();
        assert_eq!(other, SourceCounts::default());
    }

    #[test]
    fn an_unreadable_mark_hides_a_video_until_it_changes_or_the_mark_is_cleared() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(dir.path().join("l.db")).unwrap();
        let mut item = movie("/m", "A", false);
        item.core.media_files[0].size_bytes = 100;
        item.core.media_files[0].modified_ms = 5_000;
        let id = db.write(move |tx| insert_item(tx, &item)).unwrap();
        let mark = UnreadableMark {
            directory: "/m/A".into(),
            filename: "v.mkv".into(),
            size_bytes: 100,
            modified_ms: 5_000,
            reason: "media info unreadable".into(),
        };
        assert_eq!(db.read(|c| pending_media_info(c, 10)).unwrap().len(), 1);

        let stored = mark.clone();
        db.write(move |tx| mark_unreadable(tx, &stored, 1)).unwrap();
        assert!(db.read(|c| pending_media_info(c, 10)).unwrap().is_empty());
        let counts = db.read(|c| source_counts(c, "/m")).unwrap();
        assert_eq!(counts.pending_media_info, 0);
        assert_eq!(db.read(list_unreadable).unwrap(), vec![mark.clone()]);

        // The file was replaced: size and time no longer match, so it is tried again.
        let mut changed = db.read(|c| get_item(c, id)).unwrap().unwrap();
        changed.core.media_files[0].modified_ms = 6_000;
        let moved = changed.clone();
        db.write(move |tx| update_item(tx, &moved)).unwrap();
        assert_eq!(db.read(|c| pending_media_info(c, 10)).unwrap().len(), 1);

        // Back at the marked size and time, only clearing the mark lists it again.
        changed.core.media_files[0].modified_ms = 5_000;
        db.write(move |tx| update_item(tx, &changed)).unwrap();
        assert!(db.read(|c| pending_media_info(c, 10)).unwrap().is_empty());
        let removed = db
            .write(move |tx| clear_unreadable(tx, [("/M/a".to_owned(), "V.MKV".to_owned())]))
            .unwrap();
        assert_eq!(removed, 1, "the key ignores case");
        assert_eq!(db.read(|c| pending_media_info(c, 10)).unwrap().len(), 1);
    }

    #[test]
    fn a_read_that_timed_out_stays_pending_and_keeps_its_reason() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(dir.path().join("l.db")).unwrap();
        let mut item = movie("F:/Movie", "Excalibur", false);
        item.core.media_files[0].size_bytes = 93_321_653_492;
        item.core.media_files[0].modified_ms = 7;
        let file = item.core.media_files[0].clone();
        db.write(move |tx| insert_item(tx, &item)).unwrap();
        let mark = UnreadableMark {
            directory: file.directory.clone(),
            filename: file.filename.clone(),
            size_bytes: file.size_bytes,
            modified_ms: 7,
            reason: TIMED_OUT.into(),
        };
        db.write(move |tx| mark_unreadable(tx, &mark, 1)).unwrap();
        assert_eq!(
            db.read(|c| pending_media_info(c, 10)).unwrap().len(),
            1,
            "a timed-out read is tried again"
        );
        assert_eq!(
            db.read(|c| source_counts(c, "F:/Movie"))
                .unwrap()
                .pending_media_info,
            1
        );
        let probe = file.clone();
        assert_eq!(
            db.read(move |c| unreadable_reason(c, &probe))
                .unwrap()
                .as_deref(),
            Some(TIMED_OUT)
        );
        // A mark at another size does not describe the file as it is now.
        let mut changed = file;
        changed.size_bytes += 1;
        assert_eq!(
            db.read(move |c| unreadable_reason(c, &changed)).unwrap(),
            None
        );
    }

    #[test]
    fn the_marker_of_a_disc_whose_folder_is_read_is_not_pending() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(dir.path().join("l.db")).unwrap();
        let mut item = movie(r"D:\Media", "Spellbound", true);
        item.core.media_files[0].directory = r"D:\Media\Spellbound".into();
        item.core.media_files[0].filename = "BDMV".into();
        item.core.media_files.push(MediaFile::new(
            MediaFileType::Video,
            r"D:\Media\Spellbound\BDMV",
            "index.bdmv",
        ));
        db.write(move |tx| insert_item(tx, &item)).unwrap();
        assert!(db.read(|c| pending_media_info(c, 10)).unwrap().is_empty());
        assert_eq!(
            db.read(|c| source_counts(c, r"D:\Media"))
                .unwrap()
                .pending_media_info,
            0
        );
    }

    #[test]
    fn a_disc_marker_that_was_read_as_a_file_is_tried_again() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(dir.path().join("l.db")).unwrap();
        let mut item = movie("D:/Media", "Bridge of Spies", false);
        item.core.media_files[0].directory = "D:/Media/Bridge of Spies/BDMV".into();
        item.core.media_files[0].filename = "index.bdmv".into();
        item.core.media_files[0].size_bytes = 1200;
        db.write(move |tx| insert_item(tx, &item)).unwrap();
        let mark = |reason: &str| UnreadableMark {
            directory: "D:/Media/Bridge of Spies/BDMV".into(),
            filename: "index.bdmv".into(),
            size_bytes: 1200,
            modified_ms: 0,
            reason: reason.into(),
        };
        let old = mark(NOT_MEDIA);
        db.write(move |tx| mark_unreadable(tx, &old, 1)).unwrap();
        assert_eq!(db.read(|c| pending_media_info(c, 10)).unwrap().len(), 1);
        // A disc that still cannot be read for another reason is left alone.
        let now = mark("media info unreadable");
        db.write(move |tx| mark_unreadable(tx, &now, 2)).unwrap();
        assert!(db.read(|c| pending_media_info(c, 10)).unwrap().is_empty());
    }
}
