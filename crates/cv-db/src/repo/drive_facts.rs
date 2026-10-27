//! What the Drives page reads from the library: per library folder (data source) how many titles
//! and bytes it holds, how they split by resolution class, and when the folder last answered.

use std::collections::HashMap;

use rusqlite::{Connection, params};

use crate::error::DbResult;

/// What one library folder holds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SourceUsage {
    /// Movies, or shows for a TV folder (hidden titles are not counted).
    pub titles: u64,
    /// The distinct files of its titles, each counted once.
    pub bytes: u64,
}

/// How many titles of a library folder fall in each resolution class by their best video.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SourceQuality {
    /// 2000 pixels high or more.
    pub p2160: u64,
    /// 1000 to 1999.
    pub p1080: u64,
    /// 700 to 999.
    pub p720: u64,
    /// Under 700.
    pub sd: u64,
}

fn count(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

/// The file path of a `media_file` row `m`, written with `/`: the grouping key of "the same file".
const fn path_key() -> &'static str {
    if cfg!(windows) {
        r"(replace(rtrim(m.directory, '/\'), '\', '/') || '/' || m.filename) COLLATE NOCASE"
    } else {
        r"(replace(rtrim(m.directory, '/\'), '\', '/') || '/' || m.filename)"
    }
}

/// Titles and bytes per library folder, by the folder's path as the items store it (compared
/// ignoring case). Titles whose drive is offline still count, so an offline drive's card keeps
/// its numbers; missing titles and hidden ones do not. A file shared by several rows counts once.
///
/// # Errors
/// The library cannot be read.
pub fn source_usage(conn: &Connection) -> DbResult<HashMap<String, SourceUsage>> {
    let mut usage: HashMap<String, SourceUsage> = HashMap::new();
    let mut titles = conn.prepare(
        "SELECT lower(data_source), COUNT(*) FROM item \
         WHERE kind IN ('movie', 'show') AND availability IN ('online', 'offline') AND trashed_entry IS NULL \
         GROUP BY lower(data_source)",
    )?;
    for row in titles.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
        let (root, n) = row?;
        usage.entry(root).or_default().titles = count(n);
    }
    // A show's files hang off its episodes (and their seasons): the title a file counts for is
    // the movie or show above it, and a file shared by several rows counts once per title.
    let mut bytes = conn.prepare(&format!(
        "SELECT lower(data_source), SUM(size) FROM ( \
             SELECT t.data_source AS data_source, MAX(m.size_bytes) AS size \
             FROM media_file m \
             JOIN item mi ON mi.id = m.item_id \
             JOIN item t ON t.id = mi.id \
                         OR t.id = mi.parent_id \
                         OR t.id = (SELECT p.parent_id FROM item p WHERE p.id = mi.parent_id) \
             WHERE t.kind IN ('movie', 'show') AND t.availability IN ('online', 'offline') \
               AND t.trashed_entry IS NULL \
             GROUP BY t.id, {key}) \
         GROUP BY lower(data_source)",
        key = path_key()
    ))?;
    for row in bytes.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
        let (root, n) = row?;
        usage.entry(root).or_default().bytes = count(n);
    }
    Ok(usage)
}

/// The resolution classes of the titles per library folder, by the best video of the title (a
/// show's best episode). Titles without a known video height are left out.
///
/// # Errors
/// The library cannot be read.
pub fn source_quality(conn: &Connection) -> DbResult<HashMap<String, SourceQuality>> {
    let mut stmt = conn.prepare(
        "SELECT lower(data_source), \
                CASE WHEN best >= 2000 THEN 0 WHEN best >= 1000 THEN 1 WHEN best >= 700 THEN 2 \
                     ELSE 3 END AS class, COUNT(*) \
         FROM ( \
             SELECT t.data_source AS data_source, \
                    (SELECT MAX(m.height) FROM media_file m \
                       JOIN item mi ON mi.id = m.item_id \
                      WHERE m.kind = 'video' \
                        AND (mi.id = t.id OR mi.parent_id = t.id \
                             OR mi.parent_id IN (SELECT s.id FROM item s WHERE s.parent_id = t.id) \
                            )) AS best \
             FROM item t \
             WHERE t.kind IN ('movie', 'show') AND t.availability IN ('online', 'offline') \
               AND t.trashed_entry IS NULL) \
         WHERE best > 0 \
         GROUP BY lower(data_source), class",
    )?;
    let mut quality: HashMap<String, SourceQuality> = HashMap::new();
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, i64>(2)?,
        ))
    })?;
    for row in rows {
        let (root, class, n) = row?;
        let entry = quality.entry(root).or_default();
        match class {
            0 => entry.p2160 = count(n),
            1 => entry.p1080 = count(n),
            2 => entry.p720 = count(n),
            _ => entry.sd = count(n),
        }
    }
    Ok(quality)
}

/// When each library folder last answered, epoch milliseconds, by data source id. A folder that
/// never answered has no entry.
///
/// # Errors
/// The library cannot be read.
pub fn sources_last_seen(conn: &Connection) -> DbResult<HashMap<i64, i64>> {
    let mut stmt =
        conn.prepare("SELECT id, last_seen_ms FROM data_source WHERE last_seen_ms IS NOT NULL")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
    rows.collect::<Result<HashMap<_, _>, _>>()
        .map_err(Into::into)
}

/// Records that these library folders answered at `now_ms`. A folder seen less than `min_age_ms`
/// ago is left as it is, so a health check that runs often does not write each time. Returns how
/// many rows changed.
///
/// # Errors
/// The library cannot be written.
pub fn mark_sources_seen(
    conn: &Connection,
    ids: &[i64],
    now_ms: i64,
    min_age_ms: i64,
) -> DbResult<usize> {
    let mut stmt = conn.prepare_cached(
        "UPDATE data_source SET last_seen_ms = ?2 \
         WHERE id = ?1 AND (last_seen_ms IS NULL OR last_seen_ms <= ?2 - ?3)",
    )?;
    let mut changed = 0;
    for id in ids {
        changed += stmt.execute(params![id, now_ms, min_age_ms])?;
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use cv_domain::{Availability, Item, MediaFile, MediaFileType, SourceKind};

    use super::*;
    use crate::repo::data_sources::insert_source;
    use crate::repo::{insert_item, set_availability};

    fn memory_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        crate::schema::migrate(&mut conn).unwrap();
        conn
    }

    fn video(dir: &str, name: &str, size: u64, height: u32) -> MediaFile {
        MediaFile {
            kind: MediaFileType::Video,
            directory: dir.into(),
            filename: name.into(),
            size_bytes: size,
            height,
            ..MediaFile::default()
        }
    }

    fn movie(
        conn: &Connection,
        root: &str,
        title: &str,
        size: u64,
        height: u32,
    ) -> cv_domain::ItemId {
        let mut m = Item::movie(title);
        m.core.data_source = root.into();
        m.core.media_files = vec![video(&format!("{root}/{title}"), "film.mkv", size, height)];
        insert_item(conn, &m).unwrap()
    }

    #[test]
    fn titles_bytes_and_quality_are_counted_per_folder() {
        let conn = memory_db();
        movie(&conn, "D:/Movies", "Alien", 4_000, 2160);
        movie(&conn, "D:/Movies", "Aliens", 2_000, 1080);
        movie(&conn, "D:/Movies", "Old", 700, 480);
        movie(&conn, "E:/Archive", "Heat", 1_500, 720);
        let offline = movie(&conn, "E:/Archive", "Gone", 9_999, 1080);
        set_availability(&conn, offline, Availability::Offline, "").unwrap();
        let missing = movie(&conn, "E:/Archive", "Lost", 5_000, 1080);
        set_availability(&conn, missing, Availability::Missing, "").unwrap();

        let usage = source_usage(&conn).unwrap();
        assert_eq!(
            usage["d:/movies"],
            SourceUsage {
                titles: 3,
                bytes: 6_700
            }
        );
        assert_eq!(
            usage["e:/archive"],
            SourceUsage {
                titles: 2,
                bytes: 11_499
            },
            "an offline title still counts, so the card of an offline drive keeps its numbers"
        );
        let quality = source_quality(&conn).unwrap();
        assert_eq!(
            quality["d:/movies"],
            SourceQuality {
                p2160: 1,
                p1080: 1,
                p720: 0,
                sd: 1
            }
        );
        assert_eq!(quality["e:/archive"].p720, 1);
    }

    #[test]
    fn a_show_counts_its_episode_files_once_and_takes_its_best_resolution() {
        let conn = memory_db();
        let mut show = Item::show("Severance");
        show.core.data_source = "D:/TV".into();
        let show = insert_item(&conn, &show).unwrap();
        let season = insert_item(&conn, &Item::season(show, 1)).unwrap();
        for (n, height) in [(1, 1080), (2, 2160)] {
            let mut episode = Item::episode(season, format!("Episode {n}"));
            episode.core.data_source = "D:/TV".into();
            episode.core.media_files = vec![video(
                "D:/TV/Severance",
                &format!("e{n}.mkv"),
                1_000,
                height,
            )];
            insert_item(&conn, &episode).unwrap();
        }
        let usage = source_usage(&conn).unwrap();
        assert_eq!(
            usage["d:/tv"],
            SourceUsage {
                titles: 1,
                bytes: 2_000
            }
        );
        assert_eq!(source_quality(&conn).unwrap()["d:/tv"].p2160, 1);
    }

    #[test]
    fn the_last_time_a_folder_answered_is_kept_and_not_written_every_time() {
        let conn = memory_db();
        let source = insert_source(&conn, SourceKind::Movie, "D:/Movies").unwrap();
        assert!(sources_last_seen(&conn).unwrap().is_empty());

        assert_eq!(
            mark_sources_seen(&conn, &[source.id], 1_000, 60_000).unwrap(),
            1
        );
        assert_eq!(sources_last_seen(&conn).unwrap()[&source.id], 1_000);
        // Seen again a few seconds later: nothing is written.
        assert_eq!(
            mark_sources_seen(&conn, &[source.id], 5_000, 60_000).unwrap(),
            0
        );
        assert_eq!(sources_last_seen(&conn).unwrap()[&source.id], 1_000);
        // A minute later it is.
        assert_eq!(
            mark_sources_seen(&conn, &[source.id], 61_000, 60_000).unwrap(),
            1
        );
        assert_eq!(sources_last_seen(&conn).unwrap()[&source.id], 61_000);
    }
}
