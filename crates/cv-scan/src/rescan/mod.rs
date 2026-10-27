//! Incremental, parallel rescans of the data sources.
//!
//! A rescan keeps the library in step with the disk without rereading what has not changed:
//!
//! 1. **Walk.** The source is walked with the folder reader of [`crate::walk`], which lists
//!    sub-folders on several threads. Size and modification time come with the directory
//!    listing, so an unchanged file costs no extra system call on Windows.
//! 2. **Compare.** The walk is compared with the stored items of the source and with the
//!    `fingerprint` table (size, modification time and, on Unix, inode). A file is *unchanged*
//!    when its fingerprint matches, and only then keeps its stored row with its media info. A
//!    file with no fingerprint whose stored row still has the same size and time is adopted
//!    (this covers a fingerprint table that was emptied).
//! 3. **Pair.** Scanned movies, episodes and shows are paired with stored ones by video path,
//!    then by renamed-in-place content, title and, for a lone movie in its folder, by being the
//!    only pair left (see [`matcher`]). A video replaced by a different file keeps the item and
//!    all its metadata, gets a fresh media-file row with no media info (so it is read again) and
//!    the NEW flag again.
//!    A movie (or a show, see `moved_shows`) that no pass pairs but whose videos are gone from disk is relinked to a new folder
//!    holding videos of the same sizes with the same file name stem or the same title and year,
//!    when exactly one such pair exists (a folder or file renamed outside the app); the row
//!    keeps its metadata and edits. An ambiguous fit relinks nothing and adds the new title as
//!    a separate row, which the missing/duplicate review then lists.
//! 4. **Write.** Only differences are written, in one transaction. A rescan of an unchanged
//!    library produces an empty plan and does not open a write transaction at all.
//! 5. **Availability.** The availability of the source's items is recomputed. Items whose folder
//!    or video is gone become *Missing* (the "Missing files" state): they stay in the library
//!    with their metadata until the user removes or relocates them. Nothing is ever deleted.
//!
//! # Safety rules
//!
//! - A source that is missing, unreadable or empty is not compared at all
//!   ([`Skipped`]): a disconnected drive must not look like a deleted library. Its items become
//!   *Offline* when the folder is unreachable, and keep their state when the folder is merely
//!   empty.
//! - When part of a tree cannot be read, stale fingerprints are kept ([`RescanPlan::partial`]).
//! - Items are changed from a fresh copy read inside the write transaction, so edits made while
//!   the walk ran are not overwritten.
//!
//! # Jobs
//!
//! [`job`] runs one source per job item on the disk lane of the job manager, persistently, so
//! an interrupted rescan resumes with the sources it had not finished.

mod apply;
mod build;
pub mod job;
mod matcher;
mod moved_shows;
mod plan;
pub mod reasons;

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use cv_db::repo::availability::{AvailabilityReport, StdFileProbe};
use cv_db::repo::data_sources::list_sources;
use cv_db::repo::fingerprint::{StoredItem, library_snapshot, list_fingerprints};
use cv_db::repo::{
    AvailabilityChange, PathStyle, apply_availability, get_availability, plan_availability,
};
use cv_db::{Database, DbError};
use cv_domain::{Availability, DataSource, ItemId, ItemKind, SourceKind};

use crate::walk::{ScanProblem, WalkOptions, nested_sources, scan_movie_source, scan_tv_source};

pub use plan::{PlanStats, Relinked, RescanPlan, Skipped};

/// Everything that can stop a rescan.
#[derive(Debug, thiserror::Error)]
pub enum RescanError {
    #[error(transparent)]
    Db(#[from] DbError),
    /// The cancel check fired before anything was written.
    #[error("the rescan was cancelled")]
    Cancelled,
}

/// How a rescan walks and when it believes it is.
#[derive(Debug, Clone)]
pub struct RescanOptions {
    /// Skip rules and parser settings for movie sources.
    pub movies: WalkOptions,
    /// Skip rules and parser settings for TV sources.
    pub tv: WalkOptions,
    /// The clock for `date_added` and fingerprint stamps, epoch milliseconds. `None` reads the
    /// system clock.
    pub now_ms: Option<i64>,
}

impl Default for RescanOptions {
    fn default() -> Self {
        Self {
            movies: WalkOptions::with_defaults(),
            tv: WalkOptions::tv_defaults(),
            now_ms: None,
        }
    }
}

impl RescanOptions {
    fn now(&self) -> i64 {
        self.now_ms.unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .ok()
                .and_then(|d| i64::try_from(d.as_millis()).ok())
                .unwrap_or(0)
        })
    }
}

/// Items whose availability a rescan changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AvailabilityOutcome {
    /// Counts over every item of the library.
    pub report: AvailabilityReport,
    /// Items whose folder or video is gone: the "Missing files" state.
    pub became_missing: Vec<ItemId>,
    /// Items whose data source cannot be reached.
    pub became_offline: Vec<ItemId>,
    /// Items that were unavailable before and are online again.
    pub restored: Vec<ItemId>,
}

/// What a rescan of one data source found and did.
#[derive(Debug, Clone)]
pub struct RescanReport {
    pub source: DataSource,
    /// Set when the source could not be read, in which case nothing was compared.
    pub skipped: Option<Skipped>,
    /// Some folders could not be read.
    pub partial: bool,
    pub stats: PlanStats,
    /// Items created by this rescan (movies, shows and episodes, flagged NEW).
    pub new_items: Vec<ItemId>,
    /// Of those, the movies and shows.
    pub new_titles: usize,
    /// Of those, the episodes.
    pub new_episodes: usize,
    /// Existing items whose video was added, changed or replaced, flagged NEW again.
    pub updated: Vec<ItemId>,
    /// Items with a video whose media info has not been read: every new item, and every item
    /// whose video changed. Feed these to the media-info step.
    pub needs_media_info: Vec<ItemId>,
    pub availability: AvailabilityOutcome,
    /// Movies whose files were moved or renamed outside the app and that kept their row: the
    /// row now points at the new place.
    pub relinked: Vec<Relinked>,
    pub problems: Vec<ScanProblem>,
    /// True when a write transaction ran. An unchanged library leaves this `false`.
    pub wrote: bool,
    pub elapsed: Duration,
}

impl RescanReport {
    /// True when the rescan found the library exactly as stored.
    pub fn is_unchanged(&self) -> bool {
        !self.wrote
            && self.new_items.is_empty()
            && self.updated.is_empty()
            && self.availability.became_missing.is_empty()
            && self.availability.became_offline.is_empty()
            && self.availability.restored.is_empty()
    }
}

/// Walks a data source and plans what to write. Reads the database, writes nothing.
///
/// `cancelled` is polled after the walk, the slow part.
pub fn plan_rescan(
    db: &Database,
    source: &DataSource,
    options: &RescanOptions,
    cancelled: &dyn Fn() -> bool,
) -> Result<RescanPlan, RescanError> {
    let root = std::path::Path::new(&source.path);
    let now = options.now();
    // Another library folder inside this one is walked by its own library only.
    let all = db.read(|conn| list_sources(conn, None))?;
    let nested = nested_sources(source, &all);
    // The old folder of a move that put its title in another source but could not take every
    // original to the trash folder yet holds what is left of a title that lives elsewhere now:
    // it is not walked, so no duplicate title is made of it (and nothing is scraped into it).
    let mut skipped = nested.clone();
    skipped.extend(db.read(cv_db::repo::rename_journal::left_behind_move_folders)?);
    match source.kind {
        SourceKind::Movie => {
            let walk = options.movies.excluding(&skipped);
            let report = scan_movie_source(root, Some(source.id), &walk);
            if cancelled() {
                return Err(RescanError::Cancelled);
            }
            let (snapshot, fps, adopted) = load_state(db, source, &all, &nested)?;
            let mut plan = plan::plan_movies(source, report, &snapshot, fps, now);
            plan.adopted = adopted;
            Ok(plan)
        }
        SourceKind::Tv => {
            let walk = options.tv.excluding(&skipped);
            let report = scan_tv_source(root, Some(source.id), &walk);
            if cancelled() {
                return Err(RescanError::Cancelled);
            }
            let (snapshot, fps, adopted) = load_state(db, source, &all, &nested)?;
            let mut plan = plan::plan_tv(source, report, &snapshot, fps, now);
            plan.adopted = adopted;
            Ok(plan)
        }
    }
}

type State = (
    Vec<StoredItem>,
    Vec<cv_db::repo::fingerprint::Fingerprint>,
    Vec<ItemId>,
);

/// The item kinds a library folder of `kind` holds.
fn kinds_of(kind: SourceKind) -> &'static [ItemKind] {
    match kind {
        SourceKind::Movie => &[ItemKind::Movie],
        SourceKind::Tv => &[ItemKind::Show, ItemKind::Season, ItemKind::Episode],
    }
}

/// The stored items of a source, the detached items it takes over, and its fingerprints.
///
/// Only items of the source's own kinds count: a removed TV folder whose path is now a movie
/// folder leaves shows behind that the movie folder must not treat as its own. The adopted
/// items are in the snapshot too, so the walk pairs them with what it finds.
fn load_state(
    db: &Database,
    source: &DataSource,
    all: &[DataSource],
    nested: &[String],
) -> Result<State, DbError> {
    db.read(|conn| {
        let kinds = kinds_of(source.kind);
        let mut snapshot: Vec<StoredItem> = library_snapshot(conn, &source.path)?
            .into_iter()
            .filter(|item| kinds.contains(&item.kind))
            .collect();
        let adopted = detached_under(conn, source, all, nested)?;
        let ids: Vec<ItemId> = adopted.iter().map(|item| item.id).collect();
        snapshot.extend(adopted);
        Ok((snapshot, list_fingerprints(conn, &source.path)?, ids))
    })
}

/// The `/` separated form of a path without a trailing separator, lower-cased where the
/// platform's file names ignore case (Windows and macOS; not Linux).
fn folded(path: &str) -> String {
    folded_with(path, cv_fs::paths::CASE_SENSITIVE)
}

fn folded_with(path: &str, case_sensitive: bool) -> String {
    let mut text = path.trim().replace('\\', "/");
    if !case_sensitive {
        text = text.to_lowercase();
    }
    while text.len() > 1 && text.ends_with('/') {
        text.pop();
    }
    text
}

/// True when `path` is `root` or lies below it.
fn is_under(path: &str, root: &str) -> bool {
    let (path, root) = (folded(path), folded(root));
    path == root
        || path
            .strip_prefix(root.as_str())
            .is_some_and(|rest| rest.starts_with('/'))
}

/// True when no library folder of the item's kind has the item's data source as its path: the
/// folder was removed (its titles stay, offline) or its path now belongs to the other kind.
fn is_detached(data_source: &str, kind: ItemKind, all: &[DataSource]) -> bool {
    !all.iter()
        .any(|s| kinds_of(s.kind).contains(&kind) && folded(&s.path) == folded(data_source))
}

/// The detached items whose folder lies inside `source`, which the source takes over: movies
/// for a movie folder; shows with their seasons and episodes for a TV folder. Items inside a
/// library folder nested in this one are left to that folder. A removed folder that is added
/// back, or one of its sub-folders added on its own, so finds the titles it had instead of
/// creating them a second time.
fn detached_under(
    conn: &rusqlite::Connection,
    source: &DataSource,
    all: &[DataSource],
    nested: &[String],
) -> Result<Vec<StoredItem>, DbError> {
    let kinds = kinds_of(source.kind);
    let mut roots: Vec<String> = {
        let mut stmt = conn.prepare_cached(
            "SELECT DISTINCT data_source FROM item \
             WHERE kind IN ('movie', 'show', 'season', 'episode') AND data_source <> ''",
        )?;
        stmt.query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?
    };
    roots.sort();
    let mut out = Vec::new();
    for root in roots {
        if folded(&root) == folded(&source.path) {
            continue;
        }
        let items = library_snapshot(conn, &root)?;
        let top: HashSet<ItemId> = items
            .iter()
            .filter(|item| {
                matches!(item.kind, ItemKind::Movie | ItemKind::Show)
                    && kinds.contains(&item.kind)
                    && is_detached(&root, item.kind, all)
                    && is_under(&item.path, &source.path)
                    && !nested.iter().any(|inner| is_under(&item.path, inner))
            })
            .map(|item| item.id)
            .collect();
        out.extend(items.into_iter().filter(|item| {
            top.contains(&item.id) || item.parent_id.is_some_and(|parent| top.contains(&parent))
        }));
    }
    Ok(out)
}

/// Which items a availability refresh may change.
enum Scope {
    /// The items of one data source. When `hold_missing`, a "missing" verdict is not trusted
    /// (the root is reachable but empty, as an unmounted drive's mount point is).
    Source {
        ids: HashSet<ItemId>,
        hold_missing: bool,
    },
    /// Items no library folder of their kind holds: the folder was removed or remapped away,
    /// or its path is now a library folder of the other kind.
    Detached {
        sources: Vec<DataSource>,
        owners: HashMap<ItemId, (String, ItemKind)>,
    },
}

/// Recomputes availability inside `scope` and writes only the rows that changed. The items of
/// `superseded` are missing whatever their files say, with the reason given.
fn refresh_scope(
    db: &Database,
    scope: &Scope,
    superseded: &[(ItemId, String)],
) -> Result<AvailabilityOutcome, DbError> {
    let plan = db.read(|conn| plan_availability(conn, &StdFileProbe, PathStyle::current()))?;
    let mut outcome = AvailabilityOutcome {
        report: plan.report,
        ..AvailabilityOutcome::default()
    };
    let changes: Vec<_> = plan
        .changes
        .into_iter()
        .filter(|c| !superseded.iter().any(|(id, _)| *id == c.id))
        .filter(|c| match scope {
            Scope::Source { ids, hold_missing } => {
                ids.contains(&c.id) && !(*hold_missing && c.state == Availability::Missing)
            }
            Scope::Detached { sources, owners } => owners
                .get(&c.id)
                .is_some_and(|(root, kind)| is_detached(root, *kind, sources)),
        })
        .collect();
    for c in &changes {
        match c.state {
            Availability::Missing => outcome.became_missing.push(c.id),
            Availability::Offline => outcome.became_offline.push(c.id),
            Availability::Online => outcome.restored.push(c.id),
            Availability::Invalid => {}
        }
    }
    if !changes.is_empty() {
        db.write(move |tx| apply_availability(tx, &changes))?;
    }
    let stale: Vec<AvailabilityChange> = db.read(|conn| {
        let mut stale = Vec::new();
        for (id, reason) in superseded {
            let (state, stored) = get_availability(conn, *id)?;
            if state != Availability::Missing || stored != *reason {
                stale.push(AvailabilityChange {
                    id: *id,
                    state: Availability::Missing,
                    reason: reason.clone(),
                });
            }
        }
        Ok(stale)
    })?;
    if !stale.is_empty() {
        outcome.became_missing.extend(stale.iter().map(|c| c.id));
        db.write(move |tx| apply_availability(tx, &stale))?;
    }
    Ok(outcome)
}

/// Applies a plan and recomputes the availability of the source's items.
pub fn apply_rescan(
    db: &Database,
    plan: RescanPlan,
    cancelled: &dyn Fn() -> bool,
) -> Result<RescanReport, RescanError> {
    let started = Instant::now();
    if cancelled() {
        return Err(RescanError::Cancelled);
    }
    let source = plan.source.clone();
    let stats = plan.stats.clone();
    let skipped = plan.skipped;
    let partial = plan.partial;
    let problems = plan.problems.clone();
    let wrote = !plan.is_empty();

    // Who may have their availability changed: the items that existed before this rescan.
    let ids: HashSet<ItemId> = plan.known_ids.iter().copied().collect();
    let superseded = plan.superseded.clone();
    let relinked = plan.relinked.clone();
    for moved in &relinked {
        tracing::info!(title = %moved.title, from = %moved.from, to = %moved.to, "relinked a moved title");
    }

    let applied = if wrote {
        db.write(move |tx| apply::apply_plan(tx, &plan))?
    } else {
        apply::Applied::default()
    };

    let availability = refresh_scope(
        db,
        &Scope::Source {
            ids,
            hold_missing: skipped == Some(Skipped::SourceEmpty),
        },
        &superseded,
    )?;
    Ok(RescanReport {
        source,
        skipped,
        partial,
        stats,
        new_items: applied.new_items,
        new_titles: applied.new_titles,
        new_episodes: applied.new_episodes,
        updated: applied.updated,
        needs_media_info: applied.needs_media_info,
        availability,
        relinked,
        problems,
        wrote,
        elapsed: started.elapsed(),
    })
}

/// Rescans one data source: walk, compare, write the differences, refresh availability.
pub fn rescan_source(
    db: &Database,
    source: &DataSource,
    options: &RescanOptions,
) -> Result<RescanReport, RescanError> {
    rescan_source_with(db, source, options, &|| false)
}

/// Like [`rescan_source`], stopping with [`RescanError::Cancelled`] when `cancelled` turns true
/// before the write. The write itself is one transaction, so a cancel never leaves half a rescan.
///
/// The source is held ([`crate::lock`]) from the walk to the write, so files an organize moves
/// meanwhile are seen either before or after the move, never half way.
pub fn rescan_source_with(
    db: &Database,
    source: &DataSource,
    options: &RescanOptions,
    cancelled: &dyn Fn() -> bool,
) -> Result<RescanReport, RescanError> {
    let _held = crate::lock::hold_sources([source.id]);
    rescan_held_source(db, source, options, cancelled)
}

/// [`rescan_source_with`] for a caller that holds the source already.
pub(crate) fn rescan_held_source(
    db: &Database,
    source: &DataSource,
    options: &RescanOptions,
    cancelled: &dyn Fn() -> bool,
) -> Result<RescanReport, RescanError> {
    let started = Instant::now();
    let plan = plan_rescan(db, source, options, cancelled)?;
    let mut report = apply_rescan(db, plan, cancelled)?;
    report.elapsed = started.elapsed();
    Ok(report)
}

/// Recomputes the availability of items whose data source is no longer in the list (a removed
/// or remapped folder), so they show as offline or missing instead of keeping a stale state.
pub fn refresh_detached(db: &Database) -> Result<AvailabilityOutcome, RescanError> {
    let (sources, owners) = detached_scope(db)?;
    Ok(refresh_scope(
        db,
        &Scope::Detached { sources, owners },
        &[],
    )?)
}

/// Whether any title sits outside every library folder of its kind (its folder was removed or
/// remapped away), so a scan has detached titles to look at.
pub fn has_detached(db: &Database) -> Result<bool, DbError> {
    let (sources, owners) = detached_scope(db)?;
    Ok(owners
        .values()
        .any(|(root, kind)| is_detached(root, *kind, &sources)))
}

type Owners = HashMap<ItemId, (String, ItemKind)>;

fn detached_scope(db: &Database) -> Result<(Vec<DataSource>, Owners), DbError> {
    let sources: Vec<DataSource> = db.read(|conn| list_sources(conn, None))?;
    let owners = db.read(|conn| {
        let mut stmt = conn.prepare_cached(
            "SELECT id, data_source, kind FROM item WHERE kind IN ('movie', 'show', 'episode')",
        )?;
        let rows = stmt
            .query_map([], |r| {
                let kind = match r.get::<_, String>(2)?.as_str() {
                    "movie" => ItemKind::Movie,
                    "show" => ItemKind::Show,
                    _ => ItemKind::Episode,
                };
                Ok((ItemId(r.get(0)?), (r.get::<_, String>(1)?, kind)))
            })?
            .collect::<Result<HashMap<_, _>, _>>()?;
        Ok(rows)
    })?;
    Ok((sources, owners))
}

/// Rescans every data source (movies first, in their saved order), then the detached items.
pub fn rescan_all(
    db: &Database,
    options: &RescanOptions,
) -> Result<Vec<RescanReport>, RescanError> {
    let sources = db.read(|conn| list_sources(conn, None))?;
    let mut reports = Vec::with_capacity(sources.len());
    for source in &sources {
        reports.push(rescan_source(db, source, options)?);
    }
    refresh_detached(db)?;
    Ok(reports)
}

#[cfg(test)]
mod fold_tests {
    use super::*;

    #[test]
    fn folding_keeps_case_only_when_asked_to() {
        assert_eq!(folded_with(r" D:\Movies\ ", false), "d:/movies");
        assert_eq!(folded_with(r" D:\Movies\ ", true), "D:/Movies");
    }

    #[test]
    fn a_folder_with_another_case_is_under_the_root_only_where_the_platform_folds_case() {
        let folds = !cv_fs::paths::CASE_SENSITIVE;
        assert_eq!(is_under("/lib/Movies/Heat", "/lib/movies"), folds);
        assert!(is_under("/lib/Movies/Heat", "/lib/Movies"));
        assert!(!is_under("/lib/Movies2/Heat", "/lib/Movies"));
    }
}
