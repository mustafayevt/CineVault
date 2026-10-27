//! Compares a walk of one data source with what the library stores for it and decides what has
//! to be written. Nothing here touches the database or the file system: the inputs are the walk
//! result, the stored snapshot and the stored fingerprints, so a rescan of an unchanged library
//! ends with an empty plan.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use cv_db::repo::fingerprint::{Fingerprint, StoredFile, StoredItem};
use cv_domain::{DataSource, Item, ItemId, ItemKind, MediaFileType};

use super::build::{
    file_key, is_stacked, new_episodes, new_movie, new_season, new_show, path_key, scanned_key,
    season_of,
};
use super::matcher::{Probe, match_moved, match_units};
use super::moved_shows::{MovedShow, match_moved_shows};
use crate::walk::{
    ProblemKind, ScanProblem, ScanReport, ScannedEpisode, ScannedFile, ScannedMovie, ScannedShow,
    TvScanReport,
};

/// Why a source was left alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skipped {
    /// The folder does not exist or cannot be read. Its items show as offline.
    SourceUnavailable,
    /// The folder exists but has no entries; a disconnected drive looks like this.
    SourceEmpty,
}

/// How many units and files a plan covers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlanStats {
    /// Movies, shows, seasons and episodes found on disk.
    pub units: usize,
    /// Files grouped to those units.
    pub files: usize,
    /// Units whose files and flags are exactly as stored.
    pub unchanged: usize,
    /// Units with no stored counterpart, which become new items.
    pub new_units: usize,
    /// Stored units with a changed file list.
    pub changed: usize,
    /// Of those, units whose video was replaced by another file.
    pub replaced: usize,
    pub files_added: usize,
    pub files_changed: usize,
    pub files_removed: usize,
    /// Movies whose files were moved or renamed outside the app and were relinked to the new
    /// place instead of being added a second time.
    pub relinked: usize,
    /// Files the fingerprint table did not know but whose stored size and time still match.
    pub fingerprints_seeded: usize,
}

/// A scanned file and whether the stored row (with its media info) can be kept for it.
#[derive(Debug, Clone)]
pub(crate) struct PlannedFile {
    pub file: ScannedFile,
    pub keep: bool,
}

/// Scan-owned flags of a stored item that follow the files.
#[derive(Debug, Clone, Default)]
pub(crate) enum Patch {
    #[default]
    None,
    Movie {
        stacked: bool,
        is_disc: bool,
        multi_movie_dir: bool,
    },
    Episode {
        stacked: bool,
        is_disc: bool,
    },
}

/// New file list for stored items that share it (one item, or the episodes of a multi-episode
/// file).
#[derive(Debug, Clone)]
pub(crate) struct UpdateUnit {
    pub ids: Vec<ItemId>,
    pub files: Vec<PlannedFile>,
    pub patch: Patch,
    /// A video was added or changed: the item gets the NEW flag again.
    pub flag_updated: bool,
    /// The folder the item now lives in, when its files were moved outside the app.
    pub path: Option<String>,
    /// For a moved item: the key of each new file mapped to the key its stored row had, so the
    /// row (and its media info) follows the file.
    pub moved_from: HashMap<String, String>,
}

/// A movie whose files were found at a new place and that kept its row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relinked {
    pub id: ItemId,
    pub title: String,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone)]
pub(crate) enum ShowRef {
    New(Box<Item>),
    Existing(ItemId),
}

#[derive(Debug, Clone)]
pub(crate) enum Write {
    NewMovie(Box<Item>),
    /// Seasons and episodes of a show, new or existing. The parent id is filled in on apply.
    Tree {
        show: ShowRef,
        seasons: Vec<Item>,
        episodes: Vec<Item>,
    },
    Update(UpdateUnit),
    /// A show folder renamed or moved outside the app: everything stored under `from` now lives
    /// under `to`. It comes before the updates of that show, which are planned against the new
    /// place.
    MoveFolder {
        from: String,
        to: String,
    },
}

/// What a rescan of one data source is going to do.
#[derive(Debug, Clone)]
pub struct RescanPlan {
    pub source: DataSource,
    /// Set when the source could not be read, in which case the plan is empty on purpose.
    pub skipped: Option<Skipped>,
    /// Some folders could not be read, so stale fingerprints were kept.
    pub partial: bool,
    pub problems: Vec<ScanProblem>,
    pub stats: PlanStats,
    pub(crate) writes: Vec<Write>,
    pub(crate) fp_upserts: Vec<Fingerprint>,
    pub(crate) fp_deletes: Vec<String>,
    pub(crate) now_ms: i64,
    /// The items that were stored for this source when the plan was made.
    pub(crate) known_ids: Vec<ItemId>,
    /// Stored shows (and their episodes) whose folder the walk now reads as part of another
    /// show, typically a `Season 1` folder that an earlier scan took for a show of its own, with
    /// the reason. They are marked missing, never deleted: the user removes them.
    pub(crate) superseded: Vec<(ItemId, String)>,
    /// Items of a removed library folder (or of a folder of the other kind at the same path)
    /// that lie inside this source: they become this source's items, so a folder that is added
    /// back finds its titles again instead of creating them a second time.
    pub(crate) adopted: Vec<ItemId>,
    /// Movies relinked to the place their files moved to.
    pub relinked: Vec<Relinked>,
}

impl RescanPlan {
    /// True when applying the plan would write nothing.
    pub fn is_empty(&self) -> bool {
        self.writes.is_empty()
            && self.fp_upserts.is_empty()
            && self.fp_deletes.is_empty()
            && self.adopted.is_empty()
    }
}

/// How the files of one unit differ from the stored ones.
struct FileDiff {
    files: Vec<PlannedFile>,
    added: usize,
    changed: usize,
    removed: usize,
    /// A video was added or changed.
    video_fresh: bool,
    video_removed: bool,
}

impl FileDiff {
    fn is_unchanged(&self) -> bool {
        self.added == 0 && self.changed == 0 && self.removed == 0
    }
}

struct Planner {
    plan: RescanPlan,
    root: String,
    fps: HashMap<String, Fingerprint>,
    seen: HashSet<String>,
}

fn skipped_of(problems: &[ScanProblem], root: &std::path::Path) -> Option<Skipped> {
    problems
        .iter()
        .filter(|p| p.path == root)
        .find_map(|p| match p.kind {
            ProblemKind::SourceUnavailable => Some(Skipped::SourceUnavailable),
            ProblemKind::SourceEmpty => Some(Skipped::SourceEmpty),
            _ => None,
        })
}

impl Planner {
    fn new(
        source: &DataSource,
        fps: Vec<Fingerprint>,
        snapshot: &[StoredItem],
        now_ms: i64,
    ) -> Self {
        Self {
            root: source.path.clone(),
            fps: fps.into_iter().map(|f| (path_key(&f.path), f)).collect(),
            seen: HashSet::new(),
            plan: RescanPlan {
                source: source.clone(),
                skipped: None,
                partial: false,
                problems: Vec::new(),
                stats: PlanStats::default(),
                writes: Vec::new(),
                fp_upserts: Vec::new(),
                fp_deletes: Vec::new(),
                now_ms,
                known_ids: snapshot.iter().map(|i| i.id).collect(),
                superseded: Vec::new(),
                adopted: Vec::new(),
                relinked: Vec::new(),
            },
        }
    }

    fn fingerprint_of(file: &ScannedFile) -> Fingerprint {
        Fingerprint {
            path: file.path.to_string_lossy().into_owned(),
            size_bytes: file.size_bytes,
            modified_ms: file.modified_ms,
            file_id: file.file_id.clone(),
        }
    }

    /// Records files that belong to a new item.
    fn register_new(&mut self, files: &[ScannedFile]) {
        for file in files {
            self.plan.stats.files += 1;
            self.plan.stats.files_added += 1;
            self.seen.insert(scanned_key(file));
            self.plan.fp_upserts.push(Self::fingerprint_of(file));
        }
    }

    /// Compares the scanned files of a unit with its stored files.
    fn diff(&mut self, scanned: &[ScannedFile], stored: &[StoredFile]) -> FileDiff {
        let by_key: HashMap<String, &StoredFile> = stored
            .iter()
            .map(|f| (file_key(&f.directory, &f.filename), f))
            .collect();
        let mut out = FileDiff {
            files: Vec::with_capacity(scanned.len()),
            added: 0,
            changed: 0,
            removed: 0,
            video_fresh: false,
            video_removed: false,
        };
        let mut present: HashSet<String> = HashSet::with_capacity(scanned.len());
        for file in scanned {
            let key = scanned_key(file);
            self.plan.stats.files += 1;
            let current = Self::fingerprint_of(file);
            let known = by_key.get(&key).copied();
            let keep = match (self.fps.get(&key), known) {
                (Some(fp), Some(_)) if fp.same_as(&current) => true,
                (None, Some(old))
                    if old.size_bytes == current.size_bytes
                        && old.modified_ms == current.modified_ms =>
                {
                    // Nothing recorded yet, but the stored row agrees: adopt it.
                    self.plan.stats.fingerprints_seeded += 1;
                    self.plan.fp_upserts.push(current);
                    true
                }
                (_, Some(_)) => {
                    out.changed += 1;
                    self.plan.fp_upserts.push(current);
                    false
                }
                (_, None) => {
                    out.added += 1;
                    self.plan.fp_upserts.push(current);
                    false
                }
            };
            if !keep && file.kind == MediaFileType::Video {
                out.video_fresh = true;
            }
            present.insert(key.clone());
            self.seen.insert(key);
            out.files.push(PlannedFile {
                file: file.clone(),
                keep,
            });
        }
        for old in stored {
            if !present.contains(&file_key(&old.directory, &old.filename)) {
                out.removed += 1;
                if old.kind == MediaFileType::Video {
                    out.video_removed = true;
                }
            }
        }
        self.plan.stats.files_added += out.added;
        self.plan.stats.files_changed += out.changed;
        self.plan.stats.files_removed += out.removed;
        out
    }

    /// Plans the update of a stored unit, or counts it as unchanged.
    fn unit(
        &mut self,
        ids: Vec<ItemId>,
        scanned: &[ScannedFile],
        stored: &[StoredFile],
        patch: Patch,
        patch_changed: bool,
        videos_flag: bool,
    ) {
        self.plan.stats.units += 1;
        let diff = self.diff(scanned, stored);
        if diff.is_unchanged() && !patch_changed {
            self.plan.stats.unchanged += 1;
            return;
        }
        self.plan.stats.changed += 1;
        if diff.video_fresh && diff.video_removed {
            self.plan.stats.replaced += 1;
        }
        self.plan.writes.push(Write::Update(UpdateUnit {
            ids,
            files: diff.files,
            patch,
            flag_updated: videos_flag && diff.video_fresh,
            path: None,
            moved_from: HashMap::new(),
        }));
    }

    /// A show or season: files only.
    fn container(&mut self, id: ItemId, scanned: &[ScannedFile], stored: &[StoredFile]) {
        self.unit(vec![id], scanned, stored, Patch::None, false, false);
    }

    fn finish(mut self, problems: Vec<ScanProblem>) -> RescanPlan {
        self.plan.partial = problems
            .iter()
            .any(|p| matches!(p.kind, ProblemKind::Unreadable));
        // A file that is no longer found loses its fingerprint, unless part of the tree could
        // not be read: then absence proves nothing.
        if !self.plan.partial {
            let mut stale: Vec<String> = self
                .fps
                .iter()
                .filter(|(key, _)| !self.seen.contains(*key))
                .map(|(_, fp)| fp.path.clone())
                .collect();
            stale.sort_unstable();
            self.plan.fp_deletes = stale;
        }
        self.plan.problems = problems;
        self.plan
    }
}

fn video_keys(files: &[ScannedFile]) -> (Vec<String>, Vec<(u64, i64)>) {
    files
        .iter()
        .filter(|f| f.kind == MediaFileType::Video)
        .map(|f| (scanned_key(f), (f.size_bytes, f.modified_ms)))
        .unzip()
}

fn stored_video_keys(files: &[StoredFile]) -> (Vec<String>, Vec<(u64, i64)>) {
    files
        .iter()
        .filter(|f| f.kind == MediaFileType::Video)
        .map(|f| {
            (
                file_key(&f.directory, &f.filename),
                (f.size_bytes, f.modified_ms),
            )
        })
        .unzip()
}

fn movie_title_key(title: &str, year: u16) -> String {
    format!("{}|{year}", title.trim().to_lowercase())
}

/// Plans the rescan of a movie data source.
pub(crate) fn plan_movies(
    source: &DataSource,
    report: ScanReport,
    snapshot: &[StoredItem],
    fingerprints: Vec<Fingerprint>,
    now_ms: i64,
) -> RescanPlan {
    let mut p = Planner::new(source, fingerprints, snapshot, now_ms);
    if let Some(skipped) = skipped_of(&report.problems, std::path::Path::new(&source.path)) {
        p.plan.skipped = Some(skipped);
        p.plan.problems = report.problems;
        return p.plan;
    }
    let stored: Vec<&StoredItem> = snapshot
        .iter()
        .filter(|i| i.kind == ItemKind::Movie)
        .collect();
    let stored_probes: Vec<Probe> = stored
        .iter()
        .map(|item| {
            let (videos, content) = stored_video_keys(&item.files);
            Probe {
                videos,
                content,
                group: path_key(&item.path),
                title_key: Some(movie_title_key(&item.title, item.year)),
                solo: !item.multi_movie_dir,
            }
        })
        .collect();
    let scanned_probes: Vec<Probe> = report
        .movies
        .iter()
        .map(|m| {
            let (videos, content) = video_keys(&m.files);
            Probe {
                videos,
                content,
                group: path_key(&m.path.to_string_lossy()),
                title_key: Some(movie_title_key(&m.title, m.year.unwrap_or(0))),
                solo: !m.multi_movie_dir,
            }
        })
        .collect();
    let mut pairs = match_units(&scanned_probes, &stored_probes, true);

    // Titles whose files moved to another folder (or got another name) while the app was not
    // looking: relink the stored row instead of adding the title a second time. Only when the
    // whole tree was read, since a folder that could not be read proves nothing.
    let mut relinks: HashSet<usize> = HashSet::new();
    if !report
        .problems
        .iter()
        .any(|problem| matches!(problem.kind, ProblemKind::Unreadable))
    {
        let open_scanned: Vec<usize> = (0..pairs.len()).filter(|&i| pairs[i].is_none()).collect();
        let used: HashSet<usize> = pairs.iter().flatten().copied().collect();
        let open_stored: Vec<usize> = (0..stored.len())
            .filter(|j| !used.contains(j) && files_are_gone(stored[*j]))
            .collect();
        if !open_scanned.is_empty() && !open_stored.is_empty() {
            let moved = match_moved(&scanned_probes, &stored_probes, &open_scanned, &open_stored);
            for (i, j) in moved.relinks {
                pairs[i] = Some(j);
                relinks.insert(i);
            }
            for (i, j) in moved.ambiguous {
                tracing::info!(
                    scanned = %report.movies[i].path.display(),
                    stored = %stored[j].path,
                    "a new movie folder fits more than one missing title; both are kept"
                );
            }
        }
    }

    for (index, (movie, pair)) in report.movies.iter().zip(pairs).enumerate() {
        match pair {
            Some(j) if relinks.contains(&index) => plan_relinked_movie(&mut p, movie, stored[j]),
            Some(j) => plan_known_movie(&mut p, movie, stored[j]),
            None => {
                p.plan.stats.units += 1;
                p.plan.stats.new_units += 1;
                p.register_new(&movie.files);
                p.plan.writes.push(Write::NewMovie(Box::new(new_movie(
                    movie,
                    &source.path,
                    now_ms,
                ))));
            }
        }
    }
    p.finish(report.problems)
}

/// True when the item has stored video files and none of them is on disk any more.
fn files_are_gone(item: &StoredItem) -> bool {
    let mut any = false;
    let gone = item
        .files
        .iter()
        .filter(|f| f.kind == MediaFileType::Video)
        .all(|f| {
            any = true;
            std::fs::symlink_metadata(std::path::Path::new(&f.directory).join(&f.filename)).is_err()
        });
    any && gone
}

/// The stored files of a moved movie as they are at the new place, and the map from the new key
/// of each file to the key its stored row has now. A file matches by name and kind; a video also
/// by size (it was renamed too).
fn rebase_stored(
    stored: &StoredItem,
    movie: &ScannedMovie,
) -> (Vec<StoredFile>, HashMap<String, String>) {
    let mut used = vec![false; movie.files.len()];
    let mut moved_from = HashMap::new();
    let mut slots: Vec<Option<StoredFile>> = vec![None; stored.files.len()];
    let place = |old: &StoredFile, new: &ScannedFile| {
        let mut row = old.clone();
        row.directory = new
            .path
            .parent()
            .map(|d| d.to_string_lossy().into_owned())
            .unwrap_or_default();
        row.filename = new
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        row
    };
    for (k, old) in stored.files.iter().enumerate() {
        let hit = movie.files.iter().enumerate().find(|(n, f)| {
            !used[*n]
                && f.kind == old.kind
                && f.path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case(&old.filename))
        });
        if let Some((n, f)) = hit {
            used[n] = true;
            slots[k] = Some(place(old, f));
            moved_from.insert(scanned_key(f), file_key(&old.directory, &old.filename));
        }
    }
    for (k, old) in stored.files.iter().enumerate() {
        if slots[k].is_some() || old.kind != MediaFileType::Video {
            continue;
        }
        let hit = movie.files.iter().enumerate().find(|(n, f)| {
            !used[*n] && f.kind == MediaFileType::Video && f.size_bytes == old.size_bytes
        });
        if let Some((n, f)) = hit {
            used[n] = true;
            slots[k] = Some(place(old, f));
            moved_from.insert(scanned_key(f), file_key(&old.directory, &old.filename));
        }
    }
    // Files that did not come along stay as stored, so they count as removed.
    let rebased = slots
        .into_iter()
        .enumerate()
        .map(|(k, slot)| slot.unwrap_or_else(|| stored.files[k].clone()))
        .collect();
    (rebased, moved_from)
}

/// A movie found at a new place: the stored row keeps its metadata, ids and edits and gets the
/// new folder and the files found there (artwork and NFO that moved with the folder included).
fn plan_relinked_movie(p: &mut Planner, movie: &ScannedMovie, stored: &StoredItem) {
    let patch = Patch::Movie {
        stacked: is_stacked(&movie.files),
        is_disc: movie.disc,
        multi_movie_dir: movie.multi_movie_dir,
    };
    let (rebased, moved_from) = rebase_stored(stored, movie);
    p.plan.stats.units += 1;
    p.plan.stats.changed += 1;
    p.plan.stats.relinked += 1;
    let diff = p.diff(&movie.files, &rebased);
    let to = movie.path.to_string_lossy().into_owned();
    p.plan.relinked.push(Relinked {
        id: stored.id,
        title: stored.title.clone(),
        from: stored.path.clone(),
        to: to.clone(),
    });
    p.plan.writes.push(Write::Update(UpdateUnit {
        ids: vec![stored.id],
        files: diff.files,
        patch,
        flag_updated: false,
        path: Some(to),
        moved_from,
    }));
}

fn plan_known_movie(p: &mut Planner, movie: &ScannedMovie, stored: &StoredItem) {
    let patch = Patch::Movie {
        stacked: is_stacked(&movie.files),
        is_disc: movie.disc,
        multi_movie_dir: movie.multi_movie_dir,
    };
    let patch_changed = stored.multi_movie_dir != movie.multi_movie_dir;
    p.unit(
        vec![stored.id],
        &movie.files,
        &stored.files,
        patch,
        patch_changed,
        true,
    );
}

/// The stored episodes that share one file, which count as one unit.
struct StoredEpisodeUnit<'a> {
    items: Vec<&'a StoredItem>,
}

fn episode_title_key(season: u32, number: u32) -> String {
    format!("s{season}e{number}")
}

/// Plans the rescan of a TV data source.
pub(crate) fn plan_tv(
    source: &DataSource,
    report: TvScanReport,
    snapshot: &[StoredItem],
    fingerprints: Vec<Fingerprint>,
    now_ms: i64,
) -> RescanPlan {
    let mut p = Planner::new(source, fingerprints, snapshot, now_ms);
    if let Some(skipped) = skipped_of(&report.problems, std::path::Path::new(&source.path)) {
        p.plan.skipped = Some(skipped);
        p.plan.problems = report.problems;
        return p.plan;
    }
    // One stored show per folder: the oldest. A second row on the same folder (left by a
    // rescan that once raced an organize) is flagged below instead of being paired at random.
    let mut shows: HashMap<String, &StoredItem> = HashMap::new();
    let mut doubles: Vec<(&StoredItem, ItemId)> = Vec::new();
    let mut stored_shows: Vec<&StoredItem> = snapshot
        .iter()
        .filter(|i| i.kind == ItemKind::Show)
        .collect();
    stored_shows.sort_by_key(|i| i.id);
    for show in stored_shows {
        match shows.entry(path_key(&show.path)) {
            std::collections::hash_map::Entry::Occupied(kept) => {
                doubles.push((show, kept.get().id));
            }
            std::collections::hash_map::Entry::Vacant(slot) => {
                slot.insert(show);
            }
        }
    }
    let mut children: HashMap<ItemId, Vec<&StoredItem>> = HashMap::new();
    for item in snapshot {
        if let (ItemKind::Season | ItemKind::Episode, Some(parent)) = (item.kind, item.parent_id) {
            children.entry(parent).or_default().push(item);
        }
    }

    // Show folders renamed or moved outside the app keep their row, when the fit is certain.
    // Only when the whole tree was read, since a folder that could not be read proves nothing.
    let readable = !report
        .problems
        .iter()
        .any(|p| matches!(p.kind, ProblemKind::Unreadable));
    let mut moved: HashMap<usize, MovedShow> = HashMap::new();
    if readable {
        let found: HashSet<String> = report
            .shows
            .iter()
            .map(|s| path_key(&s.path.to_string_lossy()))
            .collect();
        let new_shows: Vec<(usize, &ScannedShow)> = report
            .shows
            .iter()
            .enumerate()
            .filter(|(_, s)| !shows.contains_key(&path_key(&s.path.to_string_lossy())))
            .collect();
        let mut open: Vec<&StoredItem> = shows
            .iter()
            .filter(|(key, _)| !found.contains(*key))
            .map(|(_, stored)| *stored)
            .collect();
        open.sort_by_key(|stored| stored.id);
        if !new_shows.is_empty() && !open.is_empty() {
            moved = match_moved_shows(&new_shows, &open, &children);
        }
    }

    for (index, show) in report.shows.iter().enumerate() {
        if let Some(moved) = moved.get(&index) {
            p.plan.stats.relinked += 1;
            p.plan.relinked.push(Relinked {
                id: moved.show.id,
                title: moved.show.title.clone(),
                from: moved.from.clone(),
                to: moved.to.clone(),
            });
            p.plan.writes.push(Write::MoveFolder {
                from: moved.from.clone(),
                to: moved.to.clone(),
            });
            let kids: Vec<&StoredItem> = moved.kids.iter().collect();
            plan_known_show(&mut p, show, &moved.show, &kids);
            continue;
        }
        let key = path_key(&show.path.to_string_lossy());
        match shows.get(&key) {
            Some(stored) => {
                let kids = children.get(&stored.id).map_or(&[][..], Vec::as_slice);
                plan_known_show(&mut p, show, stored, kids);
            }
            None => plan_new_show(&mut p, show),
        }
    }
    if !report
        .problems
        .iter()
        .any(|p| matches!(p.kind, ProblemKind::Unreadable))
    {
        p.plan.superseded = superseded_shows(&report.shows, &shows, &children);
    }
    for (double, kept) in doubles {
        let reason = super::reasons::another_show(&double.path, kept);
        p.plan.superseded.push((double.id, reason.clone()));
        for kid in children.get(&double.id).into_iter().flatten() {
            if kid.kind == ItemKind::Episode {
                p.plan.superseded.push((kid.id, reason.clone()));
            }
        }
    }
    p.finish(report.problems)
}

fn season_files(show: &ScannedShow, number: u32) -> &[ScannedFile] {
    i32::try_from(number)
        .ok()
        .and_then(|n| show.season(n))
        .map_or(&[][..], |s| s.files.as_slice())
}

/// The seasons a show needs: every season with episodes or files, never the "all seasons" slot.
fn season_numbers(show: &ScannedShow) -> BTreeSet<u32> {
    let mut numbers: BTreeSet<u32> = show.episodes.iter().map(season_of).collect();
    numbers.extend(
        show.seasons
            .iter()
            .filter_map(|s| u32::try_from(s.number).ok()),
    );
    numbers
}

fn plan_new_show(p: &mut Planner, show: &ScannedShow) {
    let root = p.root.clone();
    let now = p.plan.now_ms;
    p.plan.stats.units += 1;
    p.plan.stats.new_units += 1;
    p.register_new(&show.files);
    let show_item = new_show(show, &root, now);
    let show_path = show_item.core.path.clone();

    let mut seasons = Vec::new();
    for number in season_numbers(show) {
        let files = season_files(show, number);
        p.plan.stats.units += 1;
        p.plan.stats.new_units += 1;
        p.register_new(files);
        seasons.push(new_season(number, files, &show_path, &root, now));
    }
    let mut episodes = Vec::new();
    for episode in &show.episodes {
        p.plan.stats.units += 1;
        p.plan.stats.new_units += 1;
        p.register_new(&episode.files);
        episodes.extend(new_episodes(episode, &root, now));
    }
    p.plan.writes.push(Write::Tree {
        show: ShowRef::New(Box::new(show_item)),
        seasons,
        episodes,
    });
}

/// The stored shows the walk no longer finds as shows because their folder lies inside a show it
/// found (an earlier scan read `Chernobyl/Season 1` as a show; now `Chernobyl` is the show and
/// `Season 1` its season), with their episodes and the reason for each. A walk never finds one
/// show inside another, so such a stored show is a leftover of an older reading of the tree.
fn superseded_shows(
    scanned: &[ScannedShow],
    stored: &HashMap<String, &StoredItem>,
    children: &HashMap<ItemId, Vec<&StoredItem>>,
) -> Vec<(ItemId, String)> {
    let found: Vec<(String, &ScannedShow)> = scanned
        .iter()
        .map(|s| (path_key(&s.path.to_string_lossy()), s))
        .collect();
    let mut out = Vec::new();
    let mut leftovers: Vec<&&StoredItem> = stored
        .iter()
        .filter(|(key, _)| !found.iter().any(|(k, _)| k == *key))
        .map(|(_, item)| item)
        .collect();
    leftovers.sort_by_key(|item| item.id);
    for item in leftovers {
        let key = path_key(&item.path);
        let Some((_, owner)) = found.iter().find(|(k, _)| {
            key.strip_prefix(k.as_str())
                .is_some_and(|rest| rest.starts_with('/'))
        }) else {
            continue;
        };
        let reason = super::reasons::part_of_show(&item.path, &owner.path.to_string_lossy());
        out.push((item.id, reason.clone()));
        for kid in children.get(&item.id).into_iter().flatten() {
            if kid.kind == ItemKind::Episode {
                out.push((kid.id, reason.clone()));
            }
        }
    }
    out
}

fn plan_known_show(p: &mut Planner, show: &ScannedShow, stored: &StoredItem, kids: &[&StoredItem]) {
    let root = p.root.clone();
    let now = p.plan.now_ms;
    p.container(stored.id, &show.files, &stored.files);

    // Seasons: update the ones that exist, create the ones that are new.
    let stored_seasons: BTreeMap<u32, &StoredItem> = kids
        .iter()
        .filter(|i| i.kind == ItemKind::Season)
        .filter_map(|i| i.season_number.map(|n| (n, *i)))
        .collect();
    let mut new_seasons = Vec::new();
    let mut needed = season_numbers(show);
    // A season that already exists is kept even when its episodes are gone.
    for (number, season) in &stored_seasons {
        needed.remove(number);
        let files = season_files(show, *number);
        if !files.is_empty() || !season.files.is_empty() {
            p.container(season.id, files, &season.files);
        }
    }
    for number in needed {
        let files = season_files(show, number);
        p.plan.stats.units += 1;
        p.plan.stats.new_units += 1;
        p.register_new(files);
        new_seasons.push(new_season(number, files, &stored.path, &root, now));
    }

    // Episodes: stored episodes that share a video file are one unit.
    let mut units: Vec<StoredEpisodeUnit<'_>> = Vec::new();
    let mut by_video: HashMap<String, usize> = HashMap::new();
    for item in kids.iter().filter(|i| i.kind == ItemKind::Episode) {
        let first_video = item
            .files
            .iter()
            .find(|f| f.kind == MediaFileType::Video)
            .map(|f| file_key(&f.directory, &f.filename))
            .unwrap_or_else(|| format!("#{}", item.id.0));
        match by_video.get(&first_video) {
            Some(&slot) => units[slot].items.push(item),
            None => {
                by_video.insert(first_video, units.len());
                units.push(StoredEpisodeUnit { items: vec![item] });
            }
        }
    }
    let stored_probes: Vec<Probe> = units
        .iter()
        .map(|u| {
            let first = u.items[0];
            let (videos, content) = stored_video_keys(&first.files);
            Probe {
                videos,
                content,
                group: String::new(),
                title_key: first.aired.map(|(s, e)| episode_title_key(s, e)),
                solo: false,
            }
        })
        .collect();
    let scanned_probes: Vec<Probe> = show
        .episodes
        .iter()
        .map(|e| {
            let (videos, content) = video_keys(&e.files);
            Probe {
                videos,
                content,
                group: String::new(),
                title_key: e
                    .numbers
                    .first()
                    .map(|n| episode_title_key(season_of(e), *n)),
                solo: false,
            }
        })
        .collect();
    let pairs = match_units(&scanned_probes, &stored_probes, false);

    let mut new_items = Vec::new();
    for (episode, pair) in show.episodes.iter().zip(pairs) {
        match pair {
            Some(j) => plan_known_episode(p, episode, &units[j]),
            None => {
                p.plan.stats.units += 1;
                p.plan.stats.new_units += 1;
                p.register_new(&episode.files);
                new_items.extend(new_episodes(episode, &root, now));
            }
        }
    }
    if !new_seasons.is_empty() || !new_items.is_empty() {
        p.plan.writes.push(Write::Tree {
            show: ShowRef::Existing(stored.id),
            seasons: new_seasons,
            episodes: new_items,
        });
    }
}

fn plan_known_episode(p: &mut Planner, episode: &ScannedEpisode, unit: &StoredEpisodeUnit<'_>) {
    let patch = Patch::Episode {
        stacked: is_stacked(&episode.files),
        is_disc: episode.disc,
    };
    let ids = unit.items.iter().map(|i| i.id).collect();
    p.unit(
        ids,
        &episode.files,
        &unit.items[0].files,
        patch,
        false,
        true,
    );
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn source() -> DataSource {
        DataSource {
            id: 1,
            kind: cv_domain::SourceKind::Movie,
            path: "/lib".into(),
            position: 0,
            label: String::new(),
            created_ms: 0,
        }
    }

    fn scanned(name: &str, kind: MediaFileType, size: u64, mtime: i64, id: &str) -> ScannedFile {
        ScannedFile {
            path: PathBuf::from(format!("/lib/M/{name}")),
            relative: format!("M/{name}"),
            kind,
            is_dir: false,
            size_bytes: size,
            modified_ms: mtime,
            file_id: id.into(),
            stacking: 0,
            stacking_marker: String::new(),
            disc_file: false,
        }
    }

    fn stored(name: &str, kind: MediaFileType, size: u64, mtime: i64) -> StoredFile {
        StoredFile {
            kind,
            directory: "/lib/M".into(),
            filename: name.into(),
            size_bytes: size,
            modified_ms: mtime,
            has_media_info: true,
        }
    }

    fn fingerprint(name: &str, size: u64, mtime: i64, id: &str) -> Fingerprint {
        Fingerprint {
            path: format!("/lib/M/{name}"),
            size_bytes: size,
            modified_ms: mtime,
            file_id: id.into(),
        }
    }

    fn planner(fps: Vec<Fingerprint>) -> Planner {
        Planner::new(&source(), fps, &[], 0)
    }

    #[test]
    fn a_matching_fingerprint_keeps_the_stored_row() {
        let mut p = planner(vec![fingerprint("m.mkv", 10, 5, "1:1")]);
        let d = p.diff(
            &[scanned("m.mkv", MediaFileType::Video, 10, 5, "1:1")],
            &[stored("m.mkv", MediaFileType::Video, 10, 5)],
        );
        assert!(d.is_unchanged());
        assert!(d.files[0].keep);
        assert!(
            p.plan.fp_upserts.is_empty(),
            "an unchanged file writes nothing"
        );
    }

    #[test]
    fn a_new_file_identity_with_the_same_size_and_time_is_a_replaced_file() {
        let mut p = planner(vec![fingerprint("m.mkv", 10, 5, "1:1")]);
        let d = p.diff(
            &[scanned("m.mkv", MediaFileType::Video, 10, 5, "1:2")],
            &[stored("m.mkv", MediaFileType::Video, 10, 5)],
        );
        assert_eq!((d.changed, d.added, d.removed), (1, 0, 0));
        assert!(d.video_fresh);
        assert!(!d.files[0].keep);
    }

    #[test]
    fn without_a_fingerprint_the_stored_size_and_time_are_adopted() {
        let mut p = planner(Vec::new());
        let d = p.diff(
            &[scanned("m.mkv", MediaFileType::Video, 10, 5, "")],
            &[stored("m.mkv", MediaFileType::Video, 10, 5)],
        );
        assert!(d.is_unchanged());
        assert_eq!(p.plan.stats.fingerprints_seeded, 1);
        assert_eq!(p.plan.fp_upserts.len(), 1);
    }

    #[test]
    fn added_changed_and_removed_files_are_told_apart() {
        let mut p = planner(vec![
            fingerprint("m.mkv", 10, 5, ""),
            fingerprint("m.nfo", 1, 1, ""),
            fingerprint("gone.srt", 1, 1, ""),
        ]);
        let d = p.diff(
            &[
                scanned("m.mkv", MediaFileType::Video, 10, 5, ""),
                scanned("m.nfo", MediaFileType::Nfo, 2, 9, ""),
                scanned("m-poster.jpg", MediaFileType::Poster, 3, 3, ""),
            ],
            &[
                stored("m.mkv", MediaFileType::Video, 10, 5),
                stored("m.nfo", MediaFileType::Nfo, 1, 1),
                stored("gone.srt", MediaFileType::Subtitle, 1, 1),
            ],
        );
        assert_eq!((d.added, d.changed, d.removed), (1, 1, 1));
        assert!(!d.video_fresh && !d.video_removed);
        assert_eq!(p.plan.stats.files_added, 1);
        // The vanished subtitle is no longer seen, so `finish` drops its fingerprint.
        let plan = p.finish(Vec::new());
        assert_eq!(plan.fp_deletes, vec!["/lib/M/gone.srt".to_owned()]);
    }

    #[test]
    fn unreadable_folders_keep_stale_fingerprints() {
        let p = planner(vec![fingerprint("old.mkv", 1, 1, "")]);
        let problem = ScanProblem {
            kind: ProblemKind::Unreadable,
            path: PathBuf::from("/lib/Locked"),
            message: "denied".into(),
        };
        let plan = p.finish(vec![problem]);
        assert!(plan.partial);
        assert!(plan.fp_deletes.is_empty());
    }
}
