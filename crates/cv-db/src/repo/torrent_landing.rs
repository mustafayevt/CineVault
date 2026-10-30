//! The library rows behind the Torrent search landing: the movies whose copy could be better and
//! the newest additions, each with its TMDB ID; and every movie and show for the "Latest
//! releases" feed, which matches releases by title and year.
//!
//! Both reads are one query each over `item`, `ext_id` and `media_file`, so the landing costs the
//! same for a library of fifty titles and one of five thousand.

use cv_domain::{ItemId, ItemKind, id_key};
use rusqlite::Connection;

use super::codec::parse_enum;
use crate::error::DbResult;

/// A library movie or show for a landing strip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LandingRow {
    pub id: ItemId,
    /// `Movie` or `Show`.
    pub kind: ItemKind,
    pub tmdb_id: u64,
    pub title: String,
    /// `0` when unknown.
    pub year: u16,
    /// Epoch milliseconds; `0` when unknown.
    pub date_added_ms: i64,
    /// The frame size of the movie's largest video, `(0, 0)` when unknown or for a show.
    pub width: u32,
    pub height: u32,
}

/// One row of the landing query, as read: id, kind, title, TMDB ID, year, added, width, height.
type RawRow = (i64, String, String, String, i64, i64, i64, i64);

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<RawRow> {
    Ok((
        r.get(0)?,
        r.get(1)?,
        r.get(2)?,
        r.get(3)?,
        r.get(4)?,
        r.get(5)?,
        r.get(6)?,
        r.get(7)?,
    ))
}

const SELECT: &str = "SELECT i.id, i.kind, i.title, e.value, i.year, i.date_added_ms, \
       COALESCE((SELECT m.width FROM media_file m WHERE m.item_id = i.id AND m.kind = 'video' \
                 ORDER BY m.size_bytes DESC, m.id LIMIT 1), 0), \
       COALESCE((SELECT m.height FROM media_file m WHERE m.item_id = i.id AND m.kind = 'video' \
                 ORDER BY m.size_bytes DESC, m.id LIMIT 1), 0) \
     FROM item i JOIN ext_id e ON e.item_id = i.id AND e.provider = ?1 \
       AND i.trashed_entry IS NULL";

fn collect(rows: impl Iterator<Item = rusqlite::Result<RawRow>>) -> DbResult<Vec<LandingRow>> {
    let mut found = Vec::new();
    for entry in rows {
        let (id, kind, title, tmdb, year, added, width, height) = entry?;
        // A damaged or non-numeric TMDB ID cannot be searched; the row is left out.
        let Ok(tmdb_id) = tmdb.trim().parse::<u64>() else {
            continue;
        };
        if tmdb_id == 0 {
            continue;
        }
        found.push(LandingRow {
            id: ItemId(id),
            kind: parse_enum(&kind, "item kind")?,
            tmdb_id,
            title,
            year: u16::try_from(year).unwrap_or(0),
            date_added_ms: added,
            width: u32::try_from(width).unwrap_or(0),
            height: u32::try_from(height).unwrap_or(0),
        });
    }
    Ok(found)
}

/// Every library movie with a TMDB ID and a video whose frame size is known, with that size, in
/// title order. The caller decides which ones count as an upgrade.
pub fn movies_with_video(conn: &Connection) -> DbResult<Vec<LandingRow>> {
    let sql = format!(
        "{SELECT} WHERE i.kind = 'movie' AND EXISTS (SELECT 1 FROM media_file m \
           WHERE m.item_id = i.id AND m.kind = 'video' AND (m.width > 0 OR m.height > 0)) \
         ORDER BY i.title COLLATE NOCASE, i.id"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map([id_key::TMDB], row)?;
    collect(rows)
}

/// The newest library movies and shows with a TMDB ID, newest first, at most `limit`.
pub fn recently_added(conn: &Connection, limit: usize) -> DbResult<Vec<LandingRow>> {
    let sql = format!(
        "{SELECT} WHERE i.kind IN ('movie', 'show') \
         ORDER BY i.date_added_ms DESC, i.id DESC LIMIT ?2"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let limit = i64::try_from(limit).unwrap_or(i64::MAX);
    let rows = stmt.query_map(rusqlite::params![id_key::TMDB, limit], row)?;
    collect(rows)
}

/// A library movie or show as the "Latest releases" feed matches releases against it: by title
/// and year, with no provider ID needed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryTitleRow {
    pub id: ItemId,
    /// `Movie` or `Show`.
    pub kind: ItemKind,
    pub title: String,
    pub original_title: String,
    /// `0` when unknown.
    pub year: u16,
    /// The movie's source (`BluRay`, `WEB-DL`, ...), empty when unknown or for a show.
    pub media_source: String,
    /// The frame size of the movie's largest video, `(0, 0)` when unknown or for a show.
    pub width: u32,
    pub height: u32,
    /// The HDR formats of that video.
    pub hdr_formats: Vec<String>,
}

/// One row of the library-titles query, as read.
type TitleRaw = (i64, String, String, String, i64, String, i64, i64, String);

/// Every movie and show in the library (not in the bin), each with the frame size and HDR formats
/// of a movie's largest video, in one query.
pub fn library_titles(conn: &Connection) -> DbResult<Vec<LibraryTitleRow>> {
    const LARGEST: &str = "FROM media_file m WHERE m.item_id = i.id AND m.kind = 'video'                            ORDER BY m.size_bytes DESC, m.id LIMIT 1";
    let sql = format!(
        "SELECT i.id, i.kind, i.title, i.original_title, i.year, i.media_source,            COALESCE((SELECT m.width {LARGEST}), 0),            COALESCE((SELECT m.height {LARGEST}), 0),            COALESCE((SELECT m.hdr_formats {LARGEST}), '[]')          FROM item i WHERE i.kind IN ('movie', 'show') AND i.trashed_entry IS NULL          ORDER BY i.id"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map([], |r| -> rusqlite::Result<TitleRaw> {
        Ok((
            r.get(0)?,
            r.get(1)?,
            r.get(2)?,
            r.get(3)?,
            r.get(4)?,
            r.get(5)?,
            r.get(6)?,
            r.get(7)?,
            r.get(8)?,
        ))
    })?;
    let mut found = Vec::new();
    for entry in rows {
        let (id, kind, title, original_title, year, media_source, width, height, hdr) = entry?;
        found.push(LibraryTitleRow {
            id: ItemId(id),
            kind: parse_enum(&kind, "item kind")?,
            title,
            original_title,
            year: u16::try_from(year).unwrap_or(0),
            media_source,
            width: u32::try_from(width).unwrap_or(0),
            height: u32::try_from(height).unwrap_or(0),
            // A damaged list only loses the HDR mark.
            hdr_formats: serde_json::from_str(&hdr).unwrap_or_default(),
        });
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use cv_domain::{Item, MediaFile, MediaFileType};

    use super::*;
    use crate::repo::insert_item;

    fn memory_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        crate::schema::migrate(&mut conn).unwrap();
        conn
    }

    fn video(width: u32, height: u32, size: u64) -> MediaFile {
        let mut file = MediaFile::new(MediaFileType::Video, "d", "v.mkv");
        file.width = width;
        file.height = height;
        file.size_bytes = size;
        file
    }

    fn movie(
        conn: &Connection,
        title: &str,
        tmdb: &str,
        added: i64,
        files: Vec<MediaFile>,
    ) -> ItemId {
        let mut item = Item::movie(title);
        if !tmdb.is_empty() {
            item.core.ids.set(id_key::TMDB, tmdb);
        }
        item.core.date_added_ms = added;
        item.core.media_files = files;
        insert_item(conn, &item).unwrap()
    }

    #[test]
    fn movies_carry_the_frame_size_of_their_largest_video() {
        let conn = memory_db();
        let heat = movie(
            &conn,
            "Heat",
            "949",
            10,
            vec![video(1920, 1080, 5), video(1280, 720, 9)],
        );
        movie(&conn, "No id", "", 11, vec![video(1280, 720, 1)]);
        movie(&conn, "Unread", "12", 12, vec![video(0, 0, 1)]);
        movie(&conn, "Damaged id", "abc", 13, vec![video(1280, 720, 1)]);
        let rows = movies_with_video(&conn).unwrap();
        assert_eq!(rows.len(), 1, "{rows:?}");
        let row = &rows[0];
        assert_eq!(row.id, heat);
        assert_eq!(row.tmdb_id, 949);
        assert_eq!((row.width, row.height), (1280, 720), "the largest file");
        assert_eq!(row.kind, ItemKind::Movie);
    }

    #[test]
    fn the_newest_titles_come_first_and_shows_count() {
        let conn = memory_db();
        movie(&conn, "Old", "1", 100, vec![]);
        movie(&conn, "New", "2", 300, vec![video(3840, 2160, 1)]);
        let mut show = Item::show("Severance");
        show.core.ids.set(id_key::TMDB, "95396");
        show.core.date_added_ms = 200;
        insert_item(&conn, &show).unwrap();
        movie(&conn, "Unmatched", "", 400, vec![]);
        let rows = recently_added(&conn, 2).unwrap();
        let titles: Vec<&str> = rows.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(titles, ["New", "Severance"]);
        assert_eq!(rows[0].height, 2160);
        assert_eq!(rows[1].kind, ItemKind::Show);
    }

    #[test]
    fn library_titles_list_movies_and_shows_with_the_largest_video() {
        let conn = memory_db();
        let mut hdr = video(3840, 2160, 9);
        hdr.hdr_formats = vec!["HDR10".to_owned()];
        movie(
            &conn,
            "Paper Lanterns",
            "",
            1,
            vec![video(1280, 720, 1), hdr],
        );
        let mut show = Item::show("Night Bakers");
        show.core.year = 2024;
        insert_item(&conn, &show).unwrap();
        let rows = library_titles(&conn).unwrap();
        assert_eq!(rows.len(), 2);
        let film = rows.iter().find(|r| r.kind == ItemKind::Movie).unwrap();
        assert_eq!(film.title, "Paper Lanterns");
        assert_eq!((film.width, film.height), (3840, 2160));
        assert_eq!(film.hdr_formats, ["HDR10"]);
        let series = rows.iter().find(|r| r.kind == ItemKind::Show).unwrap();
        assert_eq!(series.year, 2024);
        assert_eq!((series.width, series.height), (0, 0));
    }
}
