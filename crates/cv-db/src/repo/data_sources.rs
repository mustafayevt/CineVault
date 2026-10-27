//! Data sources: the movie and TV folders the library is built from.
//!
//! This layer stores sources and keeps their order; it does not judge paths. The nested-folder
//! and system-folder guards live in `cv-scan` and run before anything is written here.
//!
//! Nothing in this module deletes an item. [`remove_source`] drops only the source row; the items
//! found under it keep their `data_source` and show as offline. [`remap_source`] rewrites the
//! stored paths of those items (and of their media files and rename history) in place, so
//! nothing has to be scraped again and the next scan verifies the new location.

use cv_domain::{DataSource, SourceKind};
use rusqlite::{Connection, OptionalExtension, params};

use crate::error::{DbError, DbResult};

const SELECT: &str = "SELECT id, kind, path, position, created_ms, label FROM data_source";

type Raw = (i64, String, String, i64, i64, String);

fn read(row: &rusqlite::Row<'_>) -> rusqlite::Result<Raw> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
    ))
}

fn decode((id, kind, path, position, created_ms, label): Raw) -> DbResult<DataSource> {
    let kind = SourceKind::parse(&kind)
        .ok_or_else(|| DbError::InvalidData(format!("unknown data source kind `{kind}`")))?;
    Ok(DataSource {
        id,
        kind,
        path,
        label,
        position,
        created_ms,
    })
}

/// The sources of one kind (or all), in display order: movies first, then by position.
pub fn list_sources(conn: &Connection, kind: Option<SourceKind>) -> DbResult<Vec<DataSource>> {
    let mut stmt = conn.prepare(&format!(
        "{SELECT} WHERE (?1 IS NULL OR kind = ?1) ORDER BY kind = 'tv', position, id"
    ))?;
    let rows = stmt
        .query_map(params![kind.map(SourceKind::as_str)], read)?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter().map(decode).collect()
}

/// One source by id.
pub fn get_source(conn: &Connection, id: i64) -> DbResult<Option<DataSource>> {
    conn.query_row(&format!("{SELECT} WHERE id = ?1"), [id], read)
        .optional()?
        .map(decode)
        .transpose()
}

/// Adds a source at the end of its kind's list.
///
/// # Errors
/// Fails on the unique index when the same path (ignoring case) is already a source of that kind.
pub fn insert_source(conn: &Connection, kind: SourceKind, path: &str) -> DbResult<DataSource> {
    let label = default_label(conn, kind, path)?;
    conn.execute(
        "INSERT INTO data_source (kind, path, label, position) VALUES (?1, ?2, ?3, \
         (SELECT COALESCE(MAX(position) + 1, 0) FROM data_source WHERE kind = ?1))",
        params![kind.as_str(), path, label],
    )?;
    let id = conn.last_insert_rowid();
    get_source(conn, id)?.ok_or_else(|| DbError::InvalidData("the new data source vanished".into()))
}

/// The path segments of `path`, split on both separators, without empty ones.
fn segments(path: &str) -> Vec<&str> {
    path.split(['\\', '/']).filter(|s| !s.is_empty()).collect()
}

/// The label a new source of `kind` at `path` gets: the folder's short name; when another source
/// of the library type has it, the parent folder is added ("Movies (D:)"), and then a number.
pub fn default_label(conn: &Connection, kind: SourceKind, path: &str) -> DbResult<String> {
    let parts = segments(path);
    let short = parts.last().copied().unwrap_or("Folder").to_owned();
    let mut candidates = vec![short.clone()];
    if parts.len() >= 2 {
        candidates.push(format!("{short} ({})", parts[parts.len() - 2]));
    }
    for candidate in &candidates {
        if !label_in_use(conn, kind, candidate, None)? {
            return Ok(candidate.clone());
        }
    }
    let base = candidates.last().cloned().unwrap_or(short);
    (2..)
        .map(|n| format!("{base} {n}"))
        .find_map(
            |candidate| match label_in_use(conn, kind, &candidate, None) {
                Ok(true) => None,
                Ok(false) => Some(Ok(candidate)),
                Err(error) => Some(Err(error)),
            },
        )
        .unwrap_or_else(|| Err(DbError::InvalidData("no free source label".into())))
}

/// Whether a source of `kind` other than `except` already has `label` (ignoring case).
fn label_in_use(
    conn: &Connection,
    kind: SourceKind,
    label: &str,
    except: Option<i64>,
) -> DbResult<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM data_source          WHERE kind = ?1 AND label = ?2 COLLATE NOCASE AND id <> ?3",
        params![kind.as_str(), label, except.unwrap_or(-1)],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

/// Gives a source a new label. Only the label changes: no file is touched and no item moves.
///
/// # Errors
/// [`DbError::SourceLabelEmpty`] for a blank label, [`DbError::SourceLabelTaken`] when another
/// source of the same library type has it (ignoring case), [`DbError::SourceNotFound`] for an
/// unknown id.
pub fn set_source_label(conn: &Connection, id: i64, label: &str) -> DbResult<DataSource> {
    let label = label.trim();
    if label.is_empty() {
        return Err(DbError::SourceLabelEmpty);
    }
    let source = get_source(conn, id)?.ok_or(DbError::SourceNotFound(id))?;
    if label_in_use(conn, source.kind, label, Some(id))? {
        return Err(DbError::SourceLabelTaken {
            kind: source.kind.to_string(),
            label: label.to_owned(),
        });
    }
    conn.execute(
        "UPDATE data_source SET label = ?1 WHERE id = ?2",
        params![label, id],
    )?;
    get_source(conn, id)?.ok_or(DbError::SourceNotFound(id))
}

/// The label of the source whose root is `data_source` (an item's `data_source`), if it exists.
pub fn label_of_root(
    conn: &Connection,
    kind: SourceKind,
    data_source: &str,
) -> DbResult<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT label FROM data_source WHERE kind = ?1 AND path = ?2 COLLATE NOCASE",
            params![kind.as_str(), data_source],
            |r| r.get(0),
        )
        .optional()?)
}

/// The number of items that live under a source root (movies, shows, seasons and episodes).
pub fn count_items_under(conn: &Connection, path: &str) -> DbResult<u64> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM item WHERE data_source = ?1 COLLATE NOCASE",
        [path],
        |r| r.get(0),
    )?;
    Ok(u64::try_from(n).unwrap_or(0))
}

/// The number of titles (movies and shows) that live under a source root. Seasons and episodes
/// belong to their show and are not counted.
pub fn count_titles_under(conn: &Connection, path: &str) -> DbResult<u64> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM item WHERE data_source = ?1 COLLATE NOCASE \
         AND kind IN ('movie', 'show') AND trashed_entry IS NULL",
        [path],
        |r| r.get(0),
    )?;
    Ok(u64::try_from(n).unwrap_or(0))
}

/// What [`remove_source`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovedSource {
    /// The source as it was.
    pub source: DataSource,
    /// The titles (movies and shows) that still point at the removed root. They are kept, with
    /// their seasons and episodes.
    pub kept_items: u64,
}

/// Removes a source and closes the gap in the order. Items are not touched.
pub fn remove_source(conn: &Connection, id: i64) -> DbResult<Option<RemovedSource>> {
    let Some(source) = get_source(conn, id)? else {
        return Ok(None);
    };
    let kept_items = count_titles_under(conn, &source.path)?;
    conn.execute("DELETE FROM data_source WHERE id = ?1", [id])?;
    // Fingerprints are the scanner's cache of this folder, not library rows.
    super::fingerprint::delete_fingerprints_of_source(conn, &source.path)?;
    renumber(conn, source.kind)?;
    Ok(Some(RemovedSource { source, kept_items }))
}

/// Moves a source to the other library type, at the end of that type's list. Its label is kept
/// unless the other type already has it. Items are not touched: the next scan of the folder
/// finds what the new type holds, and the titles of the old type stay in the library.
///
/// # Errors
/// Fails on the unique index when the same path (ignoring case) is already a source of `kind`.
pub fn change_source_kind(
    conn: &Connection,
    id: i64,
    kind: SourceKind,
) -> DbResult<Option<DataSource>> {
    let Some(source) = get_source(conn, id)? else {
        return Ok(None);
    };
    if source.kind == kind {
        return Ok(Some(source));
    }
    let label = if label_in_use(conn, kind, &source.label, Some(id))? {
        default_label(conn, kind, &source.path)?
    } else {
        source.label.clone()
    };
    conn.execute(
        "UPDATE data_source SET kind = ?1, label = ?2,          position = (SELECT COALESCE(MAX(position) + 1, 0) FROM data_source WHERE kind = ?1)          WHERE id = ?3",
        params![kind.as_str(), label, id],
    )?;
    renumber(conn, source.kind)?;
    get_source(conn, id)
}

/// Puts the sources of `kind` in the given order.
///
/// # Errors
/// [`DbError::InvalidData`] unless `ids` lists every source of that kind exactly once.
pub fn reorder_sources(conn: &Connection, kind: SourceKind, ids: &[i64]) -> DbResult<()> {
    let mut current: Vec<i64> = list_sources(conn, Some(kind))?
        .into_iter()
        .map(|s| s.id)
        .collect();
    let mut wanted = ids.to_vec();
    current.sort_unstable();
    wanted.sort_unstable();
    if current != wanted {
        return Err(DbError::InvalidData(format!(
            "the order must list every {kind} data source exactly once"
        )));
    }
    for (position, id) in ids.iter().enumerate() {
        conn.execute(
            "UPDATE data_source SET position = ?1 WHERE id = ?2",
            params![i64::try_from(position).unwrap_or(i64::MAX), id],
        )?;
    }
    Ok(())
}

/// Makes the positions of `kind` gapless again, keeping their order.
fn renumber(conn: &Connection, kind: SourceKind) -> DbResult<()> {
    conn.execute(
        "UPDATE data_source SET position = ( \
             SELECT COUNT(*) FROM data_source other \
             WHERE other.kind = data_source.kind \
               AND (other.position < data_source.position \
                    OR (other.position = data_source.position AND other.id < data_source.id))) \
         WHERE kind = ?1",
        [kind.as_str()],
    )?;
    Ok(())
}

/// What [`remap_source`] re-pointed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemapReport {
    /// The source with its new path.
    pub source: DataSource,
    /// The path it had before.
    pub old_path: String,
    /// Items whose `data_source` (and `path`) now point at the new root.
    pub items: u64,
    /// Media-file rows whose folder moved with them.
    pub files: u64,
    /// Rename-journal paths that moved, so a later undo still finds its files.
    pub journal_paths: u64,
    /// Scan fingerprints that moved, so an unchanged tree is still unchanged at its new root.
    pub fingerprints: u64,
}

/// The separator a path uses: `\` for drive-letter and UNC paths, else `/`.
fn separator_of(path: &str) -> char {
    let b = path.as_bytes();
    if path.starts_with("\\\\") || (b.len() >= 2 && b[1] == b':') {
        '\\'
    } else {
        '/'
    }
}

fn trim_separators(path: &str) -> &str {
    path.trim_end_matches(['\\', '/'])
}

/// Rewrites `column` of `table` for rows whose value is the old root or lies under it.
///
/// `scope` is an extra `AND ...` condition that may use `:old_root`, to limit the rows to the
/// items of that root; `None` for no limit. Matching ignores ASCII case and requires a folder boundary, so
/// `D:\Movies2` is not under `D:\Movies`. Empty values are left alone.
fn repoint(
    conn: &Connection,
    table: &str,
    column: &str,
    scope: Option<&str>,
    rewrite: &Rewrite,
    old_root: &str,
) -> DbResult<u64> {
    let scope_sql = scope.unwrap_or("");
    let sql = format!(
        "UPDATE {table} SET {column} = CASE \
             WHEN length({column}) = :old_len THEN :new_root \
             ELSE :new_core || replace(substr({column}, :old_len + 1), :old_sep, :new_sep) END \
         WHERE {column} <> '' \
           AND lower(substr({column}, 1, :old_len)) = lower(:old_core) \
           AND (length({column}) = :old_len OR substr({column}, :old_len + 1, 1) IN ('\\', '/')) \
           {scope_sql}"
    );
    let old_sep = rewrite.old_sep.to_string();
    let new_sep = rewrite.new_sep.to_string();
    let mut args: Vec<(&str, &dyn rusqlite::ToSql)> = vec![
        (":old_len", &rewrite.old_len),
        (":old_core", &rewrite.old_core),
        (":new_root", &rewrite.new_root),
        (":new_core", &rewrite.new_core),
        (":old_sep", &old_sep),
        (":new_sep", &new_sep),
    ];
    if scope.is_some() {
        args.push((":old_root", &old_root));
    }
    let changed = conn.execute(&sql, args.as_slice())?;
    Ok(u64::try_from(changed).unwrap_or(u64::MAX))
}

struct Rewrite<'a> {
    old_core: &'a str,
    /// Length of `old_core` in characters (SQLite counts characters).
    old_len: i64,
    new_root: &'a str,
    new_core: &'a str,
    old_sep: char,
    new_sep: char,
}

/// Points a source at a new folder and re-points everything stored under the old one.
///
/// Items, media files, scan fingerprints and the rename journal are rewritten in place; no row is deleted and no
/// metadata is touched. The caller has already checked the new path with the guards.
///
/// Returns `None` when the source does not exist.
pub fn remap_source(conn: &Connection, id: i64, new_path: &str) -> DbResult<Option<RemapReport>> {
    let Some(old) = get_source(conn, id)? else {
        return Ok(None);
    };
    let old_core = trim_separators(&old.path);
    let rewrite = Rewrite {
        old_core,
        old_len: i64::try_from(old_core.chars().count()).unwrap_or(i64::MAX),
        new_root: new_path,
        new_core: trim_separators(new_path),
        old_sep: separator_of(&old.path),
        new_sep: separator_of(new_path),
    };

    // Items found under this source. Paths first: the scope reads `data_source`, which is
    // rewritten last.
    let old_root = old.path.as_str();
    let in_source = "data_source = :old_root COLLATE NOCASE";
    repoint(
        conn,
        "item",
        "path",
        Some(&format!("AND {in_source}")),
        &rewrite,
        old_root,
    )?;
    let files = repoint(
        conn,
        "media_file",
        "directory",
        Some(&format!(
            "AND item_id IN (SELECT id FROM item WHERE {in_source})"
        )),
        &rewrite,
        old_root,
    )?;
    let from = repoint(
        conn,
        "rename_journal",
        "from_path",
        None,
        &rewrite,
        old_root,
    )?;
    let to = repoint(conn, "rename_journal", "to_path", None, &rewrite, old_root)?;

    let fingerprints = repoint(
        conn,
        "fingerprint",
        "path",
        Some("AND data_source = :old_root COLLATE NOCASE"),
        &rewrite,
        old_root,
    )?;
    conn.execute(
        "UPDATE fingerprint SET data_source = ?1 WHERE data_source = ?2 COLLATE NOCASE",
        params![new_path, old.path],
    )?;

    let items = conn.execute(
        "UPDATE item SET data_source = ?1 WHERE data_source = ?2 COLLATE NOCASE",
        params![new_path, old.path],
    )?;

    conn.execute(
        "UPDATE data_source SET path = ?1 WHERE id = ?2",
        params![new_path, id],
    )?;
    let source = get_source(conn, id)?
        .ok_or_else(|| DbError::InvalidData("the remapped data source vanished".into()))?;
    Ok(Some(RemapReport {
        source,
        old_path: old.path,
        items: u64::try_from(items).unwrap_or(u64::MAX),
        files,
        journal_paths: from + to,
        fingerprints,
    }))
}
