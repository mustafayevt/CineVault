//! Reads for the library operations that move, remove or delete titles: which items
//! make up a title, which files they own, and whether anyone else shares the title's folder.
//!
//! Paths are absolute in the native syntax and compared the way the platform compares file names:
//! without regard to case on Windows and macOS, exactly on Linux (see [`super::path_case`]).

use cv_domain::ItemId;
use rusqlite::{Connection, OptionalExtension, params};

use super::path_case::fold;
use crate::error::DbResult;

/// A file recorded for a title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedFile {
    pub item_id: ItemId,
    pub directory: String,
    pub filename: String,
    pub size_bytes: u64,
}

impl OwnedFile {
    /// The full path with the separator the directory uses.
    #[must_use]
    pub fn path(&self) -> String {
        let separator = if self.directory.contains('\\') || !self.directory.contains('/') {
            std::path::MAIN_SEPARATOR
        } else {
            '/'
        };
        let directory = self.directory.trim_end_matches(['\\', '/']);
        format!("{directory}{separator}{}", self.filename)
    }
}

/// True when `path` is `root` or lies below it, ignoring the kind of separator and, where file
/// names ignore case (Windows, macOS), the case.
#[must_use]
pub fn lies_under(path: &str, root: &str) -> bool {
    let fold =
        |text: &str| cv_fs::paths::fold_name(text.trim_end_matches(['\\', '/'])).replace('\\', "/");
    let (path, root) = (fold(path), fold(root));
    path == root
        || path
            .strip_prefix(&root)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// The item and every item below it (a show with its seasons and episodes), parent first.
pub fn tree_ids(conn: &Connection, id: ItemId) -> DbResult<Vec<ItemId>> {
    let mut stmt = conn.prepare(
        "WITH RECURSIVE tree (id, depth) AS (
             SELECT ?1, 0
             UNION ALL
             SELECT i.id, t.depth + 1 FROM item i JOIN tree t ON i.parent_id = t.id
         )
         SELECT id FROM tree ORDER BY depth, id",
    )?;
    let ids = stmt
        .query_map([id.0], |row| row.get::<_, i64>(0).map(ItemId))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ids)
}

/// The files recorded for the given items (all kinds: video, NFO, artwork, subtitles).
pub fn files_of_items(conn: &Connection, ids: &[ItemId]) -> DbResult<Vec<OwnedFile>> {
    let mut stmt = conn.prepare_cached(
        "SELECT item_id, directory, filename, size_bytes FROM media_file \
         WHERE item_id = ?1 ORDER BY position, id",
    )?;
    let mut files = Vec::new();
    for id in ids {
        let rows = stmt.query_map([id.0], |row| {
            Ok(OwnedFile {
                item_id: ItemId(row.get(0)?),
                directory: row.get(1)?,
                filename: row.get(2)?,
                size_bytes: u64::try_from(row.get::<_, i64>(3)?).unwrap_or(0),
            })
        })?;
        for row in rows {
            files.push(row?);
        }
    }
    Ok(files)
}

/// `column` is the folder or lies below it, as an SQL condition over `?1` (the folder without a
/// trailing separator) and `?2` (its length in characters).
fn under(column: &str) -> String {
    let own = fold(column);
    let head = fold(&format!("substr({column}, 1, ?2 + 1)"));
    let folder = fold("?1");
    format!("({own} = {folder} OR {head} IN ({folder} || '\\', {folder} || '/'))")
}

/// The items outside the tree of `root` that live in `folder` or below it: an item whose own
/// folder is there, or one with a recorded file there. A title shares its folder when this is
/// not empty.
///
/// A collection is not a title and never counts: the Kodi formats put a copy of its artwork
/// (`movieset-poster.jpg`) in the folder of each member, and that copy belongs to the folder.
pub fn other_items_in_folder(
    conn: &Connection,
    root: ItemId,
    folder: &str,
) -> DbResult<Vec<ItemId>> {
    let folder = folder.trim_end_matches(['\\', '/']);
    let length = i64::try_from(folder.chars().count()).unwrap_or(i64::MAX);
    let sql = format!(
        "WITH RECURSIVE tree (id) AS (
             SELECT ?3
             UNION ALL
             SELECT i.id FROM item i JOIN tree t ON i.parent_id = t.id
         )
         SELECT id FROM item
          WHERE id NOT IN (SELECT id FROM tree) AND kind <> 'set' AND {items}
         UNION
         SELECT f.item_id FROM media_file f JOIN item i ON i.id = f.item_id
          WHERE f.item_id NOT IN (SELECT id FROM tree) AND i.kind <> 'set' AND {files}
         ORDER BY 1",
        items = under("path"),
        files = under("f.directory"),
    );
    let mut stmt = conn.prepare(&sql)?;
    let ids = stmt
        .query_map(params![folder, length, root.0], |row| {
            row.get::<_, i64>(0).map(ItemId)
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ids)
}

/// Every file the library records directly in `folder` (not below it), whoever owns it: the
/// titles, episodes and collections. Paths compare by the platform's case rule.
pub fn files_in_folder(conn: &Connection, folder: &str) -> DbResult<Vec<OwnedFile>> {
    let folder = folder.trim_end_matches(['\\', '/']);
    let sql = format!(
        "SELECT item_id, directory, filename, size_bytes FROM media_file \
          WHERE {} = {} ORDER BY id",
        fold(r"rtrim(directory, '\/')"),
        fold("?1"),
    );
    let mut stmt = conn.prepare(&sql)?;
    let files = stmt
        .query_map([folder], |row| {
            Ok(OwnedFile {
                item_id: ItemId(row.get(0)?),
                directory: row.get(1)?,
                filename: row.get(2)?,
                size_bytes: u64::try_from(row.get::<_, i64>(3)?).unwrap_or(0),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(files)
}

/// Deletes the scan fingerprints of every file under `folder`, so the next scan sees the files
/// as new. Returns how many rows were deleted.
pub fn delete_fingerprints_under(conn: &Connection, folder: &str) -> DbResult<usize> {
    let folder = folder.trim_end_matches(['\\', '/']);
    let length = i64::try_from(folder.chars().count()).unwrap_or(i64::MAX);
    let sql = format!("DELETE FROM fingerprint WHERE {}", under("path"));
    Ok(conn.execute(&sql, params![folder, length])?)
}

/// Forgets every file recorded under `folder`, whoever owns it, after the folder went to a
/// trash. Returns how many rows were deleted. The items themselves stay.
pub fn delete_files_under(conn: &Connection, folder: &str) -> DbResult<usize> {
    let folder = folder.trim_end_matches(['\\', '/']);
    let length = i64::try_from(folder.chars().count()).unwrap_or(i64::MAX);
    let sql = format!("DELETE FROM media_file WHERE {}", under("directory"));
    Ok(conn.execute(&sql, params![folder, length])?)
}

/// Hides the items `ids` (a deleted title and every row below it) under the History entry
/// `entry` of the Delete that sent their files to the bin. Hidden rows keep every detail; every
/// library list, count, facet and size total leaves them out.
///
/// # Errors
/// The library cannot be written.
pub fn hide_items(conn: &Connection, ids: &[ItemId], entry: i64) -> DbResult<usize> {
    let mut stmt = conn.prepare_cached("UPDATE item SET trashed_entry = ?2 WHERE id = ?1")?;
    let mut n = 0;
    for id in ids {
        n += stmt.execute(params![id.0, entry])?;
    }
    Ok(n)
}

/// The items hidden under `entry`, in id order.
///
/// # Errors
/// The library cannot be read.
pub fn hidden_by(conn: &Connection, entry: i64) -> DbResult<Vec<ItemId>> {
    let mut stmt =
        conn.prepare_cached("SELECT id FROM item WHERE trashed_entry = ?1 ORDER BY id")?;
    let rows = stmt.query_map([entry], |r| Ok(ItemId(r.get(0)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Brings hidden items back into the library after their files were restored: they are online
/// again (their files are back at the old paths).
///
/// # Errors
/// The library cannot be written.
pub fn unhide_items(conn: &Connection, ids: &[ItemId]) -> DbResult<usize> {
    let mut stmt = conn.prepare_cached(
        "UPDATE item SET trashed_entry = NULL, availability = 'online', availability_reason = ''
         WHERE id = ?1 AND trashed_entry IS NOT NULL",
    )?;
    let mut n = 0;
    for id in ids {
        n += stmt.execute([id.0])?;
    }
    Ok(n)
}

/// Whether `id` is hidden by a Delete.
///
/// # Errors
/// The library cannot be read.
pub fn is_hidden(conn: &Connection, id: ItemId) -> DbResult<bool> {
    Ok(conn
        .prepare_cached("SELECT trashed_entry IS NOT NULL FROM item WHERE id = ?1")?
        .query_row([id.0], |r| r.get::<_, bool>(0))
        .optional()?
        .unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use cv_domain::{Item, MediaFile, MediaFileType};

    use super::*;
    use crate::repo::fingerprint::{Fingerprint, upsert_fingerprints};
    use crate::repo::{delete_tree, insert_item};

    fn memory_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        crate::schema::migrate(&mut conn).unwrap();
        conn
    }

    fn movie(title: &str, folder: &str, file: &str) -> Item {
        let mut item = Item::movie(title);
        item.core.data_source = r"D:\Movies".into();
        item.core.path = folder.into();
        item.core
            .media_files
            .push(MediaFile::new(MediaFileType::Video, folder, file));
        item
    }

    #[test]
    fn a_show_tree_lists_the_show_its_seasons_and_episodes() {
        let conn = memory_db();
        let show = insert_item(&conn, &Item::show("Dark")).unwrap();
        let season = insert_item(&conn, &Item::season(show, 1)).unwrap();
        let episode = insert_item(&conn, &Item::episode(show, "Secrets")).unwrap();
        let other = insert_item(&conn, &Item::show("Lost")).unwrap();
        let ids = tree_ids(&conn, show).unwrap();
        assert_eq!(ids[0], show);
        assert!(ids.contains(&season) && ids.contains(&episode));
        assert!(!ids.contains(&other));
        assert_eq!(tree_ids(&conn, other).unwrap(), vec![other]);
    }

    #[test]
    fn files_are_listed_with_their_full_path_and_size() {
        let conn = memory_db();
        let mut item = movie("Heat", r"D:\Movies\Heat (1995)", "Heat (1995).mkv");
        item.core.media_files[0].size_bytes = 42;
        let id = insert_item(&conn, &item).unwrap();
        let files = files_of_items(&conn, &[id]).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].size_bytes, 42);
        assert!(files[0].path().ends_with("Heat (1995).mkv"));
    }

    #[test]
    fn a_folder_is_shared_when_another_title_has_a_file_or_a_folder_in_it() {
        let conn = memory_db();
        let alone = insert_item(&conn, &movie("Heat", r"D:\Movies\Heat", "heat.mkv")).unwrap();
        let first = insert_item(&conn, &movie("Alien", r"D:\Movies\Mixed", "alien.mkv")).unwrap();
        let second = insert_item(&conn, &movie("Heat 2", r"D:\Movies\Mixed", "two.mkv")).unwrap();
        let sibling =
            insert_item(&conn, &movie("Ronin", r"D:\Movies\Heat 2", "ronin.mkv")).unwrap();

        assert!(
            other_items_in_folder(&conn, alone, r"D:\Movies\Heat")
                .unwrap()
                .is_empty(),
            "a folder with a similar name is not inside"
        );
        assert_eq!(
            other_items_in_folder(&conn, first, r"D:\Movies\Mixed\").unwrap(),
            vec![second]
        );
        assert_eq!(
            other_items_in_folder(&conn, sibling, r"d:\movies\heat 2").unwrap(),
            Vec::<ItemId>::new()
        );
    }

    #[test]
    fn files_in_a_folder_are_listed_for_every_owner_but_not_below_it() {
        let conn = memory_db();
        let alien = insert_item(&conn, &movie("Alien", r"D:\Movies\Mixed", "alien.mkv")).unwrap();
        let heat = insert_item(&conn, &movie("Heat", r"D:\Movies\Mixed", "heat.mkv")).unwrap();
        insert_item(
            &conn,
            &movie("Ronin", r"D:\Movies\Mixed\Ronin", "ronin.mkv"),
        )
        .unwrap();
        insert_item(&conn, &movie("Up", r"D:\Movies\Mixed 2", "up.mkv")).unwrap();

        let files = files_in_folder(&conn, r"D:\Movies\Mixed\").unwrap();
        let owners: Vec<(ItemId, &str)> = files
            .iter()
            .map(|f| (f.item_id, f.filename.as_str()))
            .collect();
        assert_eq!(owners, [(alien, "alien.mkv"), (heat, "heat.mkv")]);
        let folded = files_in_folder(&conn, r"d:\movies\mixed").unwrap();
        assert_eq!(
            folded.len(),
            if cv_fs::paths::CASE_SENSITIVE { 0 } else { 2 }
        );
    }

    #[test]
    fn collection_artwork_in_a_member_folder_does_not_share_the_folder() {
        let conn = memory_db();
        let mut set = Item::set("Heat Collection");
        set.core.media_files.push(MediaFile::new(
            MediaFileType::Poster,
            r"D:\Movies\Heat (1995)",
            "movieset-poster.jpg",
        ));
        let set = insert_item(&conn, &set).unwrap();
        let mut heat = movie("Heat", r"D:\Movies\Heat (1995)", "Heat (1995).mkv");
        heat.parent_id = Some(set);
        let heat = insert_item(&conn, &heat).unwrap();
        assert!(
            other_items_in_folder(&conn, heat, r"D:\Movies\Heat (1995)")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_show_does_not_share_a_folder_with_its_own_episodes() {
        let conn = memory_db();
        let mut show = Item::show("Dark");
        show.core.path = r"D:\TV\Dark".into();
        let show = insert_item(&conn, &show).unwrap();
        let mut episode = Item::episode(show, "Secrets");
        episode.core.path = r"D:\TV\Dark\Season 1".into();
        insert_item(&conn, &episode).unwrap();
        assert!(
            other_items_in_folder(&conn, show, r"D:\TV\Dark")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn fingerprints_under_a_folder_go_and_the_rest_stay() {
        let conn = memory_db();
        let print = |path: &str| Fingerprint {
            path: path.into(),
            size_bytes: 1,
            modified_ms: 1,
            file_id: String::new(),
        };
        upsert_fingerprints(
            &conn,
            r"D:\Movies",
            &[
                print(r"D:\Movies\Heat\heat.mkv"),
                print(r"D:\Movies\Heat\extras\a.mkv"),
                print(r"D:\Movies\Heat 2\b.mkv"),
            ],
            1,
        )
        .unwrap();
        // Another spelling of the folder is the same folder on Windows and macOS, and another
        // folder on Linux, where it removes nothing.
        let wrong_case = delete_fingerprints_under(&conn, r"d:\movies\heat").unwrap();
        assert_eq!(wrong_case, if cv_fs::paths::CASE_SENSITIVE { 0 } else { 2 });
        let exact = delete_fingerprints_under(&conn, r"D:\Movies\Heat").unwrap();
        assert_eq!(exact, if cv_fs::paths::CASE_SENSITIVE { 2 } else { 0 });
        assert_eq!(
            crate::repo::fingerprint::count_fingerprints(&conn).unwrap(),
            1
        );
    }

    #[test]
    fn deleting_a_tree_keeps_the_helpers_honest() {
        let conn = memory_db();
        let id = insert_item(&conn, &movie("Heat", r"D:\Movies\Heat", "heat.mkv")).unwrap();
        delete_tree(&conn, id).unwrap();
        assert!(files_of_items(&conn, &[id]).unwrap().is_empty());
    }
}
