//! The reads and writes behind the missing/duplicate review.
//!
//! The review lists the titles that need a decision: rows whose files are missing on disk or
//! offline, rows that duplicate another row (the same video path, or the same show folder), and
//! the show rows a rescan set aside. [`review_candidates`] finds them; what the problem is called
//! and what can be done about it is decided one layer up, in `cv-app`.
//!
//! Nothing here removes a row by itself. [`keep`] records that the user chose to leave a row as
//! it is, so it leaves the list (`review_kept`); [`relocate_tree`] re-points a title whose files
//! were moved by hand.

use std::collections::{BTreeMap, HashMap};

use cv_domain::{Availability, ItemId, ItemKind};
use rusqlite::{Connection, params};

use super::codec::{parse_enum, to_u16, to_u32, to_u64};
use super::library_ops::{lies_under, tree_ids};
use super::size_totals::recorded_size_sql;
use crate::error::{DbError, DbResult};

/// A title that may need a decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewCandidate {
    pub id: ItemId,
    pub kind: ItemKind,
    pub parent_id: Option<ItemId>,
    pub title: String,
    pub year: u16,
    pub path: String,
    pub data_source: String,
    pub availability: Availability,
    /// The reason stored with the availability (a rescan's wording for set-aside shows).
    pub availability_reason: String,
    /// Distinct files recorded for the title and the titles below it.
    pub files: u32,
    /// Their size in bytes.
    pub bytes: u64,
    /// When a scan last saw the title's files, else when the row last changed (epoch ms).
    pub last_seen_ms: i64,
    /// The problem the user chose to keep this row for, when they did.
    pub kept_reason: Option<String>,
    /// The row this one duplicates (the same video path or the same show folder), when it does.
    pub duplicate_of: Option<ItemId>,
}

fn base_select() -> String {
    let (bytes, files) = recorded_size_sql("i.id");
    format!(
        "SELECT i.id, i.kind, i.parent_id, i.title, i.year, i.path, i.data_source, \
                i.availability, i.availability_reason, {files}, {bytes}, \
                COALESCE((SELECT MAX(f.recorded_ms) FROM fingerprint f \
                          WHERE i.path <> '' \
                            AND substr(f.path, 1, length(i.path)) = i.path COLLATE NOCASE), \
                         i.updated_ms), \
                k.reason \
         FROM item i LEFT JOIN review_kept k ON k.item_id = i.id"
    )
}

fn read(row: &rusqlite::Row<'_>) -> DbResult<ReviewCandidate> {
    let kind: String = row.get(1)?;
    let availability: String = row.get(7)?;
    Ok(ReviewCandidate {
        id: ItemId(row.get(0)?),
        kind: parse_enum(&kind, "item kind")?,
        parent_id: row.get::<_, Option<i64>>(2)?.map(ItemId),
        title: row.get(3)?,
        year: to_u16(row.get(4)?),
        path: row.get(5)?,
        data_source: row.get(6)?,
        availability: Availability::parse(&availability).ok_or_else(|| {
            DbError::InvalidData(format!("unknown availability `{availability}`"))
        })?,
        availability_reason: row.get(8)?,
        files: to_u32(row.get(9)?),
        bytes: to_u64(row.get(10)?),
        last_seen_ms: row.get(11)?,
        kept_reason: row.get(12)?,
        duplicate_of: None,
    })
}

fn query(conn: &Connection, condition: &str) -> DbResult<Vec<ReviewCandidate>> {
    let sql = format!(
        "{} WHERE i.trashed_entry IS NULL AND ({condition}) ORDER BY i.id",
        base_select()
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(read(row)?);
    }
    Ok(out)
}

/// The key that makes two folders "the same" for the review: separators unified, no trailing
/// separator, and case folded where the platform's file names ignore it (not on Linux).
fn fold(path: &str) -> String {
    fold_with(path, cv_fs::paths::CASE_SENSITIVE)
}

fn fold_with(path: &str, case_sensitive: bool) -> String {
    let text = path.trim_end_matches(['\\', '/']).replace('\\', "/");
    if case_sensitive {
        text
    } else {
        text.to_lowercase()
    }
}

/// A file name as part of a review key, folded like [`fold`].
fn fold_name(name: &str) -> String {
    if cv_fs::paths::CASE_SENSITIVE {
        name.to_owned()
    } else {
        name.to_lowercase()
    }
}

/// Groups of two or more titles that hold the same thing: the same video file for movies, the
/// same folder for shows.
fn duplicate_groups(conn: &Connection) -> DbResult<Vec<Vec<ItemId>>> {
    let mut groups: HashMap<(bool, String), Vec<ItemId>> = HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT m.item_id, m.directory, m.filename FROM media_file m \
             JOIN item i ON i.id = m.item_id \
             WHERE i.kind = 'movie' AND m.kind = 'video' AND i.trashed_entry IS NULL \
             ORDER BY m.item_id",
        )?;
        let mut rows = stmt.query([])?;
        while let Some(r) = rows.next()? {
            let id = ItemId(r.get(0)?);
            let key = format!(
                "{}/{}",
                fold(&r.get::<_, String>(1)?),
                fold_name(&r.get::<_, String>(2)?)
            );
            let members = groups.entry((false, key)).or_default();
            if !members.contains(&id) {
                members.push(id);
            }
        }
    }
    {
        let mut stmt = conn.prepare(
            "SELECT id, path FROM item WHERE kind = 'show' AND path <> '' \
             AND trashed_entry IS NULL ORDER BY id",
        )?;
        let mut rows = stmt.query([])?;
        while let Some(r) = rows.next()? {
            let id = ItemId(r.get(0)?);
            let key = fold(&r.get::<_, String>(1)?);
            groups.entry((true, key)).or_default().push(id);
        }
    }
    // Movies or shows with the same title and year where one of the rows has lost its files: a
    // title that was moved or renamed outside the app and then found again as a new row looks
    // like this. Two rows that both have their files are two editions, not a duplicate.
    {
        let mut stmt = conn.prepare(
            "SELECT id, kind, lower(trim(title)), year, availability FROM item              WHERE kind IN ('movie', 'show') AND year > 0 AND trim(title) <> '' \
             AND trashed_entry IS NULL ORDER BY id",
        )?;
        let mut rows = stmt.query([])?;
        let mut same: HashMap<(String, String, i64), (Vec<ItemId>, bool)> = HashMap::new();
        while let Some(r) = rows.next()? {
            let entry = same.entry((r.get(1)?, r.get(2)?, r.get(3)?)).or_default();
            entry.0.push(ItemId(r.get(0)?));
            entry.1 |= r.get::<_, String>(4)? == "missing";
        }
        for ((kind, title, year), (members, has_missing)) in same {
            if has_missing && members.len() > 1 {
                groups.insert((false, format!("title:{kind}:{title}|{year}")), members);
            }
        }
    }
    let mut out: Vec<Vec<ItemId>> = groups
        .into_values()
        .filter(|members| members.len() > 1)
        .collect();
    out.sort();
    Ok(out)
}

fn id_list(ids: &[ItemId]) -> String {
    ids.iter()
        .map(|id| id.0.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

/// The row a rescan's reason says a set-aside row duplicates: `(#7)` at the end of the text.
fn named_original(reason: &str) -> Option<ItemId> {
    let tail = reason.strip_suffix(')')?;
    let (_, id) = tail.rsplit_once("(#")?;
    id.parse::<i64>().ok().map(ItemId)
}

/// Every title that may need a decision, in id order, with the row it duplicates when it does.
///
/// A row is a candidate when it is offline or missing (a movie, a show, or an episode whose show
/// is online), or when it duplicates another row. Of a group of duplicates the best row stays out
/// of the list: the one that is online, then the one with more files, then the oldest (the row a
/// rescan keeps). Rows the user already kept come back with their `kept_reason`, so the caller
/// decides whether the problem is still the one they kept.
pub fn review_candidates(conn: &Connection) -> DbResult<Vec<ReviewCandidate>> {
    let mut by_id: BTreeMap<ItemId, ReviewCandidate> = BTreeMap::new();
    for candidate in query(
        conn,
        "(i.kind IN ('movie', 'show') AND i.availability IN ('offline', 'missing')) \
         OR (i.kind = 'episode' AND i.availability IN ('offline', 'missing') AND EXISTS ( \
                SELECT 1 FROM item p WHERE p.id = i.parent_id AND p.availability = 'online'))",
    )? {
        by_id.insert(candidate.id, candidate);
    }

    for group in duplicate_groups(conn)? {
        let members = query(conn, &format!("i.id IN ({})", id_list(&group)))?;
        let keeper = members
            .iter()
            .min_by_key(|m| {
                (
                    m.availability != Availability::Online,
                    std::cmp::Reverse(m.files),
                    m.id,
                )
            })
            .map(|m| m.id);
        let Some(keeper) = keeper else { continue };
        for member in members {
            if member.id == keeper {
                continue;
            }
            by_id.entry(member.id).or_insert(member).duplicate_of = Some(keeper);
        }
    }

    // A set-aside show names the row it duplicates in its reason; trust it when that row exists.
    let named: Vec<(ItemId, ItemId)> = by_id
        .values()
        .filter(|c| c.duplicate_of.is_none() && c.availability == Availability::Missing)
        .filter_map(|c| named_original(&c.availability_reason).map(|original| (c.id, original)))
        .collect();
    for (id, original) in named {
        if original != id
            && super::items::item_exists(conn, original)?
            && let Some(candidate) = by_id.get_mut(&id)
        {
            candidate.duplicate_of = Some(original);
        }
    }
    Ok(by_id.into_values().collect())
}

/// Records that the user keeps `id` as it is, for the problem `reason` (`missing`, `offline`,
/// `duplicate` or `superseded`). The row stays in the library.
pub fn keep(conn: &Connection, id: ItemId, reason: &str, now_ms: i64) -> DbResult<()> {
    if !super::items::item_exists(conn, id)? {
        return Err(DbError::ItemNotFound(id.0));
    }
    conn.execute(
        "INSERT INTO review_kept (item_id, reason, kept_ms) VALUES (?1, ?2, ?3) \
         ON CONFLICT (item_id) DO UPDATE SET reason = excluded.reason, kept_ms = excluded.kept_ms",
        params![id.0, reason, now_ms],
    )?;
    Ok(())
}

/// Takes back [`keep`]: the row is listed again if it still has a problem.
pub fn unkeep(conn: &Connection, id: ItemId) -> DbResult<()> {
    conn.execute("DELETE FROM review_kept WHERE item_id = ?1", [id.0])?;
    Ok(())
}

/// Re-points one title, and the titles below it, from the folder `from` to the folder `to`:
/// `item.path` and `media_file.directory` of exactly that tree change (a duplicate on the same
/// folder is left alone), the stale scan fingerprints of the old files are dropped, and the
/// title joins the data source whose root holds `to`. Returns how many rows changed.
///
/// # Errors
/// The title does not exist.
pub fn relocate_tree(conn: &Connection, id: ItemId, from: &str, to: &str) -> DbResult<u64> {
    let tree = tree_ids(conn, id)?;
    if tree.is_empty() {
        return Err(DbError::ItemNotFound(id.0));
    }
    let ids = serde_json::to_string(&tree.iter().map(|i| i.0).collect::<Vec<_>>())?;
    let old = from.trim_end_matches(['\\', '/']);
    let new = to.trim_end_matches(['\\', '/']);
    let old_len = i64::try_from(old.chars().count()).unwrap_or(i64::MAX);

    // The old files' fingerprints go first, while their paths still say where the files were.
    {
        let mut stmt = conn.prepare(
            "SELECT directory, filename FROM media_file \
             WHERE item_id IN (SELECT value FROM json_each(?1))",
        )?;
        let files = stmt
            .query_map([&ids], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);
        let paths: Vec<String> = files
            .iter()
            .map(|(dir, name)| {
                let separator = if dir.contains('\\') || !dir.contains('/') {
                    '\\'
                } else {
                    '/'
                };
                format!("{}{separator}{name}", dir.trim_end_matches(['\\', '/']))
            })
            .collect();
        super::fingerprint::delete_fingerprints(conn, &paths)?;
    }

    let mut changed = 0usize;
    for (table, column, owner) in [
        ("item", "path", "id"),
        ("media_file", "directory", "item_id"),
    ] {
        // The old folder is matched the way the file system does: ignoring case on Windows and
        // macOS, exactly on Linux, where `/lib/Movies` and `/lib/movies` are two folders.
        let same_start = if cv_fs::paths::CASE_SENSITIVE {
            format!("substr({column}, 1, ?2) = ?3")
        } else {
            format!("lower(substr({column}, 1, ?2)) = lower(?3)")
        };
        changed += conn.execute(
            &format!(
                "UPDATE {table} SET {column} = ?1 || substr({column}, ?2 + 1) \
                 WHERE {owner} IN (SELECT value FROM json_each(?4)) \
                   AND {same_start} \
                   AND (length({column}) = ?2 OR substr({column}, ?2 + 1, 1) IN ('\\', '/'))"
            ),
            params![new, old_len, old, ids],
        )?;
    }

    // The data source that now holds the folder: the deepest root above it.
    let mut stmt = conn.prepare("SELECT path FROM data_source")?;
    let roots = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);
    if let Some(root) = roots
        .iter()
        .filter(|root| lies_under(new, root))
        .max_by_key(|root| root.trim_end_matches(['\\', '/']).len())
    {
        conn.execute(
            "UPDATE item SET data_source = ?1 WHERE id IN (SELECT value FROM json_each(?2))",
            params![root, ids],
        )?;
    }
    Ok(u64::try_from(changed).unwrap_or(u64::MAX))
}

#[cfg(test)]
mod tests {
    use cv_domain::Item;

    use super::*;
    use crate::repo::{get_item, insert_item};

    fn memory_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        crate::schema::migrate(&mut conn).unwrap();
        conn
    }

    #[test]
    fn the_review_key_keeps_case_only_when_asked_to() {
        assert_eq!(fold_with(r"D:\Movies\Heat\", false), "d:/movies/heat");
        assert_eq!(fold_with(r"D:\Movies\Heat\", true), "D:/Movies/Heat");
    }

    #[test]
    fn a_folder_in_another_case_is_relocated_only_where_the_platform_folds_case() {
        let conn = memory_db();
        let mut movie = Item::movie("Heat");
        movie.core.path = "/lib/Movies/Heat".into();
        let id = insert_item(&conn, &movie).unwrap();

        // `from` differs from the stored folder in case only.
        relocate_tree(&conn, id, "/lib/movies/Heat", "/lib/Films/Heat").unwrap();
        let path = get_item(&conn, id).unwrap().unwrap().core.path;
        if cv_fs::paths::CASE_SENSITIVE {
            assert_eq!(
                path, "/lib/Movies/Heat",
                "another folder must not be re-pointed"
            );
        } else {
            assert_eq!(path, "/lib/Films/Heat");
        }

        // The exact spelling is always re-pointed.
        relocate_tree(&conn, id, &path, "/lib/Films/Heat 2").unwrap();
        assert_eq!(
            get_item(&conn, id).unwrap().unwrap().core.path,
            "/lib/Films/Heat 2"
        );
    }
}
