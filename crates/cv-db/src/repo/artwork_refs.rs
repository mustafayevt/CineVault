//! The remembered source URLs of artwork, beyond what an item's own `artwork_urls` map holds.
//!
//! `artwork_ref` has one row per `(item, kind, position)`. The single-image types (poster, fanart,
//! ...) use position 0 and are read and written with the item itself. Extra fanart and extra
//! thumbs have one row per image at positions `1..=n`, which is what these functions manage, so
//! "Re-download artwork" can fetch each extra again from where it came from. The content hash and
//! size are filled in once the image has been fetched.

use cv_domain::{ItemId, MediaFileType};
use rusqlite::{Connection, params};

use super::codec::{enum_text, parse_enum};
use crate::error::DbResult;

/// One remembered artwork source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtworkRef {
    pub kind: MediaFileType,
    /// `0` for the single-image types, `1..` for extras.
    pub position: u32,
    pub url: String,
    /// Content hash of the downloaded image (the thumbnail cache key), once fetched.
    pub cache_hash: Option<String>,
    pub width: u32,
    pub height: u32,
}

impl ArtworkRef {
    /// A reference to an image that was just fetched.
    pub fn fetched(
        kind: MediaFileType,
        position: u32,
        url: impl Into<String>,
        cache_hash: impl Into<String>,
        width: u32,
        height: u32,
    ) -> Self {
        Self {
            kind,
            position,
            url: url.into(),
            cache_hash: Some(cache_hash.into()),
            width,
            height,
        }
    }
}

/// Every remembered source of an item, by kind then position.
pub fn artwork_refs(conn: &Connection, item: ItemId) -> DbResult<Vec<ArtworkRef>> {
    let mut stmt = conn.prepare_cached(
        "SELECT kind, position, url, cache_hash, width, height FROM artwork_ref \
         WHERE item_id = ?1 ORDER BY kind, position",
    )?;
    let rows = stmt
        .query_map([item.0], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, i64>(5)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(kind, position, url, cache_hash, width, height)| {
            Ok(ArtworkRef {
                kind: parse_enum(&kind, "artwork type")?,
                position: u32::try_from(position).unwrap_or(0),
                url,
                cache_hash,
                width: u32::try_from(width).unwrap_or(0),
                height: u32::try_from(height).unwrap_or(0),
            })
        })
        .collect()
}

/// Writes one remembered source, replacing the row at the same `(kind, position)`.
pub fn set_artwork_ref(conn: &Connection, item: ItemId, entry: &ArtworkRef) -> DbResult<()> {
    conn.prepare_cached(
        "INSERT INTO artwork_ref (item_id, kind, position, url, cache_hash, width, height) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
         ON CONFLICT (item_id, kind, position) DO UPDATE SET \
             url = excluded.url, cache_hash = excluded.cache_hash, \
             width = excluded.width, height = excluded.height",
    )?
    .execute(params![
        item.0,
        enum_text(&entry.kind)?,
        i64::from(entry.position),
        entry.url,
        entry.cache_hash,
        i64::from(entry.width),
        i64::from(entry.height),
    ])?;
    Ok(())
}

/// Forgets the remembered source at `(kind, position)`. Nothing happens when there is none.
pub fn delete_artwork_ref(
    conn: &Connection,
    item: ItemId,
    kind: MediaFileType,
    position: u32,
) -> DbResult<()> {
    conn.prepare_cached(
        "DELETE FROM artwork_ref WHERE item_id = ?1 AND kind = ?2 AND position = ?3",
    )?
    .execute(params![item.0, enum_text(&kind)?, i64::from(position)])?;
    Ok(())
}

/// Replaces the extras of `kind` (extra fanart or extra thumbs): the rows at positions of 1 and
/// above are removed and `entries` are stored. Position 0 is never touched.
pub fn replace_extra_refs(
    conn: &Connection,
    item: ItemId,
    kind: MediaFileType,
    entries: &[ArtworkRef],
) -> DbResult<()> {
    conn.prepare_cached(
        "DELETE FROM artwork_ref WHERE item_id = ?1 AND kind = ?2 AND position >= 1",
    )?
    .execute(params![item.0, enum_text(&kind)?])?;
    for entry in entries.iter().filter(|e| e.position >= 1) {
        set_artwork_ref(conn, item, entry)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::{get_item, insert_item, update_item};
    use cv_domain::Item;

    fn memory_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        crate::schema::migrate(&mut conn).unwrap();
        conn
    }

    #[test]
    fn extras_are_replaced_as_a_set_and_the_main_image_is_left_alone() {
        let conn = memory_db();
        let mut movie = Item::movie("Heat");
        movie
            .core
            .artwork_urls
            .insert(MediaFileType::Fanart, "https://i.test/main.jpg".into());
        let id = insert_item(&conn, &movie).unwrap();

        let extras = |urls: &[&str]| -> Vec<ArtworkRef> {
            urls.iter()
                .enumerate()
                .map(|(i, url)| {
                    ArtworkRef::fetched(MediaFileType::Extrafanart, i as u32 + 1, *url, "h", 10, 5)
                })
                .collect()
        };
        replace_extra_refs(
            &conn,
            id,
            MediaFileType::Extrafanart,
            &extras(&["a", "b", "c"]),
        )
        .unwrap();
        replace_extra_refs(&conn, id, MediaFileType::Extrafanart, &extras(&["x", "y"])).unwrap();

        let refs = artwork_refs(&conn, id).unwrap();
        let urls: Vec<(&str, u32)> = refs
            .iter()
            .filter(|r| r.kind == MediaFileType::Extrafanart)
            .map(|r| (r.url.as_str(), r.position))
            .collect();
        assert_eq!(urls, [("x", 1), ("y", 2)]);
        let main = refs
            .iter()
            .find(|r| r.kind == MediaFileType::Fanart)
            .unwrap();
        assert_eq!(
            (main.url.as_str(), main.position),
            ("https://i.test/main.jpg", 0)
        );

        // An item update (which rewrites the single-image rows) keeps the extras.
        let mut item = get_item(&conn, id).unwrap().unwrap();
        item.core.plot = "Cops and robbers".into();
        update_item(&conn, &item).unwrap();
        let extra_count = artwork_refs(&conn, id)
            .unwrap()
            .iter()
            .filter(|r| r.kind == MediaFileType::Extrafanart)
            .count();
        assert_eq!(extra_count, 2);
    }

    #[test]
    fn a_set_row_keeps_the_hash_and_size_of_the_fetch() {
        let conn = memory_db();
        let id = insert_item(&conn, &Item::movie("Alien")).unwrap();
        set_artwork_ref(
            &conn,
            id,
            &ArtworkRef::fetched(
                MediaFileType::Poster,
                0,
                "https://i.test/p.jpg",
                "abc",
                1000,
                1500,
            ),
        )
        .unwrap();
        let refs = artwork_refs(&conn, id).unwrap();
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].cache_hash.as_deref(), Some("abc"));
        assert_eq!((refs[0].width, refs[0].height), (1000, 1500));
    }
}
