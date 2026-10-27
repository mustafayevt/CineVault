//! Availability states: online, offline, missing and invalid.
//!
//! Availability is *computed* from the file system and stored on the item with a reason. It is
//! the only thing that changes when a drive is unplugged, a folder is deleted outside the app or
//! a database is opened on another operating system. Nothing in this module (or in loading the
//! library) deletes a row; the user decides what to do with an unavailable item.
//!
//! Rules, checked in this order for movies, shows and episodes:
//!
//! 1. **invalid**: no path, or a path or data source that is not an absolute path on this
//!    operating system (for example `D:\Movies` on macOS).
//! 2. **offline**: the item's data source root cannot be reached (an unplugged drive, an
//!    unmounted share).
//! 3. **missing**: the data source is reachable but the item's folder is gone, or every recorded
//!    video file is gone. An item whose folder now lies inside another data source nested in its
//!    own (a TV folder added inside a movie folder after a scan) is missing too: that folder
//!    belongs to the other library now, so its files are no longer this item's.
//! 4. **invalid**: a show without any episode ("no episodes found").
//! 5. **online**: everything else.
//!
//! Sets and seasons have no files of their own and keep whatever state they have.
//!
//! Probing the file system can be slow (a dead network share), so it is split from writing:
//! [`plan_availability`] runs on a reader connection and touches no row; [`apply_availability`]
//! then updates only the rows whose state changed, in one short write transaction.
//! [`Database::refresh_availability`] does both.

use std::collections::HashMap;
use std::path::Path;

use cv_domain::{Availability, ItemId};
use rusqlite::{Connection, params};

use crate::Database;
use crate::error::{DbError, DbResult};

/// Read access to the file system, abstracted so tests can simulate drives coming and going.
pub trait FileProbe {
    /// True when `path` exists and is a directory.
    fn dir_exists(&self, path: &str) -> bool;
    /// True when `path` exists and is a file.
    fn file_exists(&self, path: &str) -> bool;
}

/// The real file system.
#[derive(Debug, Clone, Copy, Default)]
pub struct StdFileProbe;

impl FileProbe for StdFileProbe {
    fn dir_exists(&self, path: &str) -> bool {
        Path::new(path).is_dir()
    }

    fn file_exists(&self, path: &str) -> bool {
        Path::new(path).is_file()
    }
}

/// Which operating system's path syntax is valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathStyle {
    /// Drive-letter (`D:\Movies`) and UNC (`\\server\share`) paths.
    Windows,
    /// Paths rooted at `/`.
    Posix,
}

impl PathStyle {
    /// The style of the operating system this build runs on.
    pub const fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else {
            Self::Posix
        }
    }

    /// True when `path` is an absolute path in this style.
    pub fn is_absolute(self, path: &str) -> bool {
        match self {
            Self::Windows => {
                let b = path.as_bytes();
                let drive = b.len() >= 3
                    && b[0].is_ascii_alphabetic()
                    && b[1] == b':'
                    && (b[2] == b'\\' || b[2] == b'/');
                drive || path.starts_with("\\\\")
            }
            Self::Posix => path.starts_with('/'),
        }
    }
}

/// A computed availability for one item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailabilityChange {
    pub id: ItemId,
    pub state: Availability,
    /// Human-readable reason; empty for [`Availability::Online`].
    pub reason: String,
}

/// What a refresh found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AvailabilityReport {
    /// Movies, shows and episodes examined.
    pub checked: usize,
    /// Rows whose stored state or reason was updated.
    pub changed: usize,
    pub online: usize,
    pub offline: usize,
    pub missing: usize,
    pub invalid: usize,
}

impl AvailabilityReport {
    fn count(&mut self, state: Availability) {
        match state {
            Availability::Online => self.online += 1,
            Availability::Offline => self.offline += 1,
            Availability::Missing => self.missing += 1,
            Availability::Invalid => self.invalid += 1,
        }
    }
}

/// The result of [`plan_availability`]: the counts per state, and the items whose stored state
/// differs from the computed one.
#[derive(Debug, Clone, Default)]
pub struct AvailabilityPlan {
    /// Only the items whose stored state or reason differs.
    pub changes: Vec<AvailabilityChange>,
    pub report: AvailabilityReport,
}

/// Caches directory probes so a data source shared by thousands of items is checked once.
struct CachedProbe<'a> {
    inner: &'a dyn FileProbe,
    dirs: HashMap<String, bool>,
    /// Every data-source root, folded for comparison, with its stored spelling.
    sources: Vec<(String, String)>,
}

/// Lower-case, `/`-separated, without a trailing separator, for comparing folders.
fn fold_path(path: &str) -> String {
    let mut text = path.trim().replace('\\', "/").to_lowercase();
    while text.ends_with('/') {
        text.pop();
    }
    text
}

/// True when `path` (folded) is `root` (folded) or lies inside it.
fn within(path: &str, root: &str) -> bool {
    path == root
        || path
            .strip_prefix(root)
            .is_some_and(|rest| rest.starts_with('/'))
}

impl CachedProbe<'_> {
    /// The data source nested inside `own` that holds `folder`, when there is one.
    fn nested_owner(&self, own: &str, folder: &str) -> Option<&str> {
        let own = fold_path(own);
        let folder = fold_path(folder);
        self.sources
            .iter()
            .find(|(root, _)| *root != own && within(root, &own) && within(&folder, root))
            .map(|(_, shown)| shown.as_str())
    }
}

impl CachedProbe<'_> {
    fn dir_exists(&mut self, path: &str) -> bool {
        if let Some(&known) = self.dirs.get(path) {
            return known;
        }
        let found = self.inner.dir_exists(path);
        self.dirs.insert(path.to_owned(), found);
        found
    }
}

struct Candidate {
    id: i64,
    kind: String,
    path: String,
    data_source: String,
    availability: String,
    reason: String,
}

fn video_files(conn: &Connection, item_id: i64) -> DbResult<Vec<(String, String)>> {
    let mut stmt = conn.prepare_cached(
        "SELECT directory, filename FROM media_file \
         WHERE item_id = ?1 AND kind = 'video' ORDER BY position, id",
    )?;
    let rows = stmt
        .query_map([item_id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn verdict(
    conn: &Connection,
    c: &Candidate,
    style: PathStyle,
    probe: &mut CachedProbe<'_>,
) -> DbResult<(Availability, String)> {
    let invalid = |why: String| Ok((Availability::Invalid, why));

    // An episode whose own path is blank is located through its video file's folder.
    let mut files: Option<Vec<(String, String)>> = None;
    let mut folder = c.path.clone();
    if folder.is_empty() && c.kind == "episode" {
        let list = video_files(conn, c.id)?;
        if let Some((dir, _)) = list.first() {
            folder.clone_from(dir);
        }
        files = Some(list);
    }

    if folder.is_empty() {
        return invalid("the item has no path".into());
    }
    if !style.is_absolute(&folder) {
        return invalid(format!(
            "the path {folder} is not valid on this operating system"
        ));
    }
    if !c.data_source.is_empty() && !style.is_absolute(&c.data_source) {
        return invalid(format!(
            "the data source {} is not valid on this operating system",
            c.data_source
        ));
    }

    if !c.data_source.is_empty() && !probe.dir_exists(&c.data_source) {
        return Ok((
            Availability::Offline,
            format!("the data source {} cannot be reached", c.data_source),
        ));
    }

    if !c.data_source.is_empty()
        && let Some(owner) = probe.nested_owner(&c.data_source, &folder)
    {
        return Ok((
            Availability::Missing,
            format!("the folder {folder} belongs to the library folder {owner} now"),
        ));
    }

    if !probe.dir_exists(&folder) {
        return Ok((
            Availability::Missing,
            format!("the folder {folder} was not found"),
        ));
    }

    if c.kind == "show" {
        let episodes: i64 = conn
            .prepare_cached("SELECT COUNT(*) FROM item WHERE parent_id = ?1 AND kind = 'episode'")?
            .query_row([c.id], |r| r.get(0))?;
        if episodes == 0 {
            return invalid("no episodes found".into());
        }
    } else {
        let list = match files {
            Some(list) => list,
            None => video_files(conn, c.id)?,
        };
        let any_present = list.iter().any(|(dir, name)| {
            probe
                .inner
                .file_exists(&Path::new(dir).join(name).to_string_lossy())
        });
        if !list.is_empty() && !any_present {
            return Ok((
                Availability::Missing,
                "the video file was not found".to_owned(),
            ));
        }
    }

    Ok((Availability::Online, String::new()))
}

/// Computes the availability of every movie, show and episode without writing anything.
///
/// `style` is the path syntax of the running operating system ([`PathStyle::current`]); tests
/// pass the other one to simulate opening a database that came from another system.
pub fn plan_availability(
    conn: &Connection,
    probe: &dyn FileProbe,
    style: PathStyle,
) -> DbResult<AvailabilityPlan> {
    let candidates = conn
        .prepare(
            "SELECT id, kind, path, data_source, availability, availability_reason \
             FROM item WHERE kind IN ('movie', 'show', 'episode') ORDER BY id",
        )?
        .query_map([], |r| {
            Ok(Candidate {
                id: r.get(0)?,
                kind: r.get(1)?,
                path: r.get(2)?,
                data_source: r.get(3)?,
                availability: r.get(4)?,
                reason: r.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let sources = conn
        .prepare("SELECT path FROM data_source ORDER BY id")?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|path| (fold_path(&path), path))
        .collect();
    let mut cached = CachedProbe {
        inner: probe,
        dirs: HashMap::new(),
        sources,
    };
    let mut plan = AvailabilityPlan::default();
    for c in &candidates {
        let (state, reason) = verdict(conn, c, style, &mut cached)?;
        plan.report.checked += 1;
        plan.report.count(state);
        if c.availability != state.as_str() || c.reason != reason {
            plan.changes.push(AvailabilityChange {
                id: ItemId(c.id),
                state,
                reason,
            });
        }
    }
    plan.report.changed = plan.changes.len();
    Ok(plan)
}

/// Stores one item's availability and reason. Never deletes anything.
pub fn set_availability(
    conn: &Connection,
    id: ItemId,
    state: Availability,
    reason: &str,
) -> DbResult<()> {
    let changed = conn
        .prepare_cached(
            "UPDATE item SET availability = ?2, availability_reason = ?3 WHERE id = ?1",
        )?
        .execute(params![id.0, state.as_str(), reason])?;
    if changed == 0 {
        return Err(DbError::ItemNotFound(id.0));
    }
    Ok(())
}

/// Reads an item's stored availability and reason.
pub fn get_availability(conn: &Connection, id: ItemId) -> DbResult<(Availability, String)> {
    let (state, reason): (String, String) = conn
        .prepare_cached("SELECT availability, availability_reason FROM item WHERE id = ?1")?
        .query_row([id.0], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => DbError::ItemNotFound(id.0),
            other => other.into(),
        })?;
    let parsed = Availability::parse(&state)
        .ok_or_else(|| DbError::InvalidData(format!("unknown availability `{state}`")))?;
    Ok((parsed, reason))
}

/// Writes the changes of a plan. Only the availability columns are touched; the item's
/// `updated_ms` is left alone because availability is not a user edit.
pub fn apply_availability(conn: &Connection, changes: &[AvailabilityChange]) -> DbResult<()> {
    for change in changes {
        match set_availability(conn, change.id, change.state, &change.reason) {
            // The item was removed by the user between planning and applying.
            Err(DbError::ItemNotFound(_)) => {}
            other => other?,
        }
    }
    Ok(())
}

impl Database {
    /// Re-evaluates the availability of every movie, show and episode against the file system.
    ///
    /// The file system is probed on a reader connection, so slow or dead drives never hold the
    /// write lock; only rows whose state changed are then written. Rows are never deleted.
    pub fn refresh_availability(
        &self,
        probe: &dyn FileProbe,
        style: PathStyle,
    ) -> DbResult<AvailabilityReport> {
        let plan = self.read(|conn| plan_availability(conn, probe, style))?;
        let AvailabilityPlan { changes, report } = plan;
        if !changes.is_empty() {
            self.write(move |tx| apply_availability(tx, &changes))?;
        }
        Ok(report)
    }
}
