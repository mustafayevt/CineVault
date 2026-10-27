//! Size totals: one rule for the per-row Total size, the Total size sort and the library total.
//!
//! The rule: the sum of the *distinct* files of the items that are online, each file
//! path counted once. Paths are compared without regard to case on Windows, where the file
//! system does the same. A file shared by several rows is therefore counted once: an episode
//! file that holds two episodes has one `media_file` row per episode, and a show that a rescan
//! stored twice has two sets of identical episode rows. Offline, missing (this includes the
//! show rows that a rescan superseded) and invalid rows are not counted; the offline ones are
//! reported as a separate remainder so Home can say "and 1.2 TB offline".
//!
//! Every sum in the app is built from [`counted_size_sql`], so what a row displays and what the
//! column sorts by cannot drift apart.

use cv_domain::ItemKind;
use rusqlite::Connection;

use crate::error::{DbError, DbResult};

/// The full path of a `media_file` row `m`, written with `/` and no doubled separator, as the
/// grouping key of "the same file". Case-insensitive on Windows only.
const fn path_key() -> &'static str {
    if cfg!(windows) {
        r"(replace(rtrim(m.directory, '/\'), '\', '/') || '/' || m.filename) COLLATE NOCASE"
    } else {
        r"(replace(rtrim(m.directory, '/\'), '\', '/') || '/' || m.filename)"
    }
}

/// SQL for the bytes counted for the item row `owner` (a column such as `item.id`): the distinct
/// files of the item and of its direct children (the episodes of a show, the movies of a set)
/// whose own row is online.
pub(crate) fn counted_size_sql(owner: &str) -> String {
    format!(
        "(SELECT COALESCE(SUM(f.size), 0) FROM ( \
             SELECT MAX(m.size_bytes) AS size FROM media_file m \
             JOIN item mi ON mi.id = m.item_id \
             WHERE (m.item_id = {owner} OR mi.parent_id = {owner}) \
               AND mi.availability = 'online' \
             GROUP BY {key}) f)",
        key = path_key()
    )
}

/// SQL for the bytes and for the number of distinct files recorded for the item row `owner` and
/// its direct children, whatever their availability: what a missing or offline row used to hold.
/// Returns `(bytes, files)`.
pub(crate) fn recorded_size_sql(owner: &str) -> (String, String) {
    let one = |select: &str| {
        format!(
            "(SELECT {select} FROM ( \
                 SELECT MAX(m.size_bytes) AS size FROM media_file m \
                 JOIN item mi ON mi.id = m.item_id \
                 WHERE (m.item_id = {owner} OR mi.parent_id = {owner}) \
                 GROUP BY {key}) f)",
            key = path_key()
        )
    };
    (one("COALESCE(SUM(f.size), 0)"), one("COUNT(*)"))
}

/// What the library holds, in bytes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LibrarySizeTotal {
    /// Distinct files of online titles: what Home shows.
    pub counted_bytes: u64,
    /// Distinct files of offline titles that no online title also holds, so Home can add
    /// "and N offline" without double counting.
    pub offline_bytes: u64,
}

fn bytes(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

/// The size of the movie library (`Movie`: movies and sets) or of the TV library (`Show`: shows,
/// seasons and episodes), under the one counting rule.
///
/// # Errors
/// The kind is not `Movie` or `Show`, or the library cannot be read.
pub fn library_size_total(conn: &Connection, kind: ItemKind) -> DbResult<LibrarySizeTotal> {
    let kinds = match kind {
        ItemKind::Movie => "'movie', 'set'",
        ItemKind::Show => "'show', 'season', 'episode'",
        ItemKind::Set | ItemKind::Season | ItemKind::Episode => {
            return Err(DbError::InvalidData(format!(
                "the size total is kept per library, not for `{kind:?}`"
            )));
        }
    };
    let key = path_key();
    let sql = format!(
        "WITH counted AS ( \
             SELECT {key} AS k, MAX(m.size_bytes) AS size FROM media_file m \
             JOIN item mi ON mi.id = m.item_id \
             WHERE mi.kind IN ({kinds}) AND mi.availability = 'online' \
                   AND mi.trashed_entry IS NULL GROUP BY k), \
         offline AS ( \
             SELECT {key} AS k, MAX(m.size_bytes) AS size FROM media_file m \
             JOIN item mi ON mi.id = m.item_id \
             WHERE mi.kind IN ({kinds}) AND mi.availability = 'offline' \
                   AND mi.trashed_entry IS NULL GROUP BY k) \
         SELECT (SELECT COALESCE(SUM(size), 0) FROM counted), \
                (SELECT COALESCE(SUM(size), 0) FROM offline \
                 WHERE k NOT IN (SELECT k FROM counted))"
    );
    let (counted, offline): (i64, i64) =
        conn.query_row(&sql, [], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(LibrarySizeTotal {
        counted_bytes: bytes(counted),
        offline_bytes: bytes(offline),
    })
}

#[cfg(test)]
mod tests {
    use cv_domain::{Availability, Item, ItemId, MediaFile, MediaFileType, RatingPreference};

    use super::*;
    use crate::repo::{
        Direction, ItemQuery, SortField, SortKey, insert_item, list_items_at, set_availability,
        table_facts::table_facts_for,
    };

    fn memory_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        crate::schema::migrate(&mut conn).unwrap();
        conn
    }

    fn video(dir: &str, name: &str, size: u64) -> MediaFile {
        MediaFile {
            kind: MediaFileType::Video,
            directory: dir.into(),
            filename: name.into(),
            size_bytes: size,
            ..MediaFile::default()
        }
    }

    fn movie(conn: &Connection, title: &str, files: Vec<MediaFile>) -> ItemId {
        let mut item = Item::movie(title);
        item.core.media_files = files;
        insert_item(conn, &item).unwrap()
    }

    fn episode(conn: &Connection, show: ItemId, title: &str, files: Vec<MediaFile>) -> ItemId {
        let mut item = Item::episode(show, title);
        item.core.media_files = files;
        insert_item(conn, &item).unwrap()
    }

    fn total_of(conn: &Connection, id: ItemId) -> u64 {
        table_facts_for(conn, &[id], &RatingPreference::default()).unwrap()[&id].total_size_bytes
    }

    #[test]
    fn a_file_shared_by_two_episodes_counts_once() {
        let conn = memory_db();
        let show = insert_item(&conn, &Item::show("Dark")).unwrap();
        // One file holding episodes 1 and 2 has a row per episode.
        let shared = video("/tv/Dark/Season 1", "Dark.S01E01-E02.mkv", 3_000);
        episode(&conn, show, "S01E01", vec![shared.clone()]);
        episode(&conn, show, "S01E02", vec![shared]);
        episode(
            &conn,
            show,
            "S01E03",
            vec![video("/tv/Dark/Season 1", "Dark.S01E03.mkv", 1_000)],
        );

        assert_eq!(total_of(&conn, show), 4_000);
        assert_eq!(
            library_size_total(&conn, ItemKind::Show).unwrap(),
            LibrarySizeTotal {
                counted_bytes: 4_000,
                offline_bytes: 0
            }
        );
    }

    #[test]
    fn offline_missing_and_superseded_rows_are_not_counted() {
        let conn = memory_db();
        let online = movie(&conn, "Heat", vec![video("/m/Heat", "heat.mkv", 5_000)]);
        let offline = movie(
            &conn,
            "Ronin",
            vec![video("/off/Ronin", "ronin.mkv", 7_000)],
        );
        let missing = movie(&conn, "Gone", vec![video("/m/Gone", "gone.mkv", 11_000)]);
        set_availability(&conn, offline, Availability::Offline, "drive unplugged").unwrap();
        set_availability(&conn, missing, Availability::Missing, "folder gone").unwrap();

        assert_eq!(total_of(&conn, online), 5_000);
        assert_eq!(total_of(&conn, offline), 0);
        assert_eq!(total_of(&conn, missing), 0);
        assert_eq!(
            library_size_total(&conn, ItemKind::Movie).unwrap(),
            LibrarySizeTotal {
                counted_bytes: 5_000,
                offline_bytes: 7_000
            }
        );
    }

    #[test]
    fn duplicate_show_rows_count_their_files_once() {
        let conn = memory_db();
        let first = insert_item(&conn, &Item::show("Chernobyl")).unwrap();
        let second = insert_item(&conn, &Item::show("Chernobyl")).unwrap();
        for show in [first, second] {
            episode(
                &conn,
                show,
                "S01E01",
                vec![video("/tv/Chernobyl/Season 1", "e01.mkv", 2_500)],
            );
        }

        // The library figure counts the file once; each row still shows its own files.
        assert_eq!(
            library_size_total(&conn, ItemKind::Show)
                .unwrap()
                .counted_bytes,
            2_500
        );
        assert_eq!(total_of(&conn, first), 2_500);
    }

    #[test]
    fn paths_that_differ_in_separator_are_one_file() {
        let conn = memory_db();
        movie(&conn, "A", vec![video(r"C:\m\Heat\", "heat.mkv", 900)]);
        movie(&conn, "B", vec![video("C:/m/Heat", "heat.mkv", 900)]);

        assert_eq!(
            library_size_total(&conn, ItemKind::Movie)
                .unwrap()
                .counted_bytes,
            900
        );
    }

    #[test]
    fn the_sort_follows_the_displayed_totals() {
        let conn = memory_db();
        let small = movie(&conn, "Small", vec![video("/m/s", "s.mkv", 100)]);
        let gone = movie(&conn, "Gone", vec![video("/m/g", "g.mkv", 9_000)]);
        let big = movie(&conn, "Big", vec![video("/m/b", "b.mkv", 5_000)]);
        set_availability(&conn, gone, Availability::Missing, "folder gone").unwrap();
        let show = insert_item(&conn, &Item::show("Show")).unwrap();
        episode(&conn, show, "S01E01", vec![video("/t/s", "e.mkv", 700)]);

        let query = ItemQuery::new(ItemKind::Movie).sort_by(SortKey {
            field: SortField::TotalFileSize,
            direction: Direction::Desc,
        });
        let order: Vec<ItemId> = list_items_at(&conn, &query, 0, 10)
            .unwrap()
            .iter()
            .map(|i| i.id)
            .collect();
        assert_eq!(order, vec![big, small, gone]);

        // A show sorts by its episodes' files, like its row shows.
        let query = ItemQuery::new(ItemKind::Show).sort_by(SortKey {
            field: SortField::TotalFileSize,
            direction: Direction::Desc,
        });
        assert_eq!(list_items_at(&conn, &query, 0, 10).unwrap()[0].id, show);
        assert_eq!(total_of(&conn, show), 700);
    }

    #[test]
    fn only_the_movie_and_tv_libraries_have_a_total() {
        let conn = memory_db();
        assert!(library_size_total(&conn, ItemKind::Episode).is_err());
    }
}
