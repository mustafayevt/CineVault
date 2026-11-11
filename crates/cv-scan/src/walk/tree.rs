//! Reads a data source into an in-memory tree, applying the skip rules on the way.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

use super::{ProblemKind, ScanProblem, WalkObserver};
use crate::classify::{MediaFileType, classify, is_video_content};
use crate::gate::{VolumeGate, gate_for};
use crate::par::par_map;
use crate::skip::{LibraryKind, SkipRules};

/// Folders nested deeper than this are not entered (a guard against link loops).
const MAX_DEPTH: usize = 48;

/// A file or folder with the facts the scan keeps.
#[derive(Debug, Clone)]
pub(super) struct Entry {
    pub path: PathBuf,
    /// Relative to the data source, `/` separated.
    pub rel: String,
    pub name: String,
    pub size: u64,
    pub modified_ms: i64,
    /// Creation (birth) time, epoch milliseconds; 0 when the file system gives none.
    pub created_ms: i64,
    /// The file's identity where the platform gives it for free (inode and device on Unix),
    /// else empty.
    pub file_id: String,
    pub is_dir: bool,
}

/// A `VIDEO_TS`, `BDMV` or `HVDVD_TS` folder. Only its own files are read.
#[derive(Debug)]
pub(super) struct Disc {
    pub dir: Entry,
    pub files: Vec<Entry>,
    /// The folder holds at least one file classified as video.
    pub has_video: bool,
}

#[derive(Debug)]
pub(super) struct Dir {
    pub path: PathBuf,
    pub name: String,
    /// Relative to the data source; empty for the root.
    pub rel: String,
    pub files: Vec<Entry>,
    pub dirs: Vec<Dir>,
    pub discs: Vec<Disc>,
    /// The listing kept a file, folder or disc before a focus narrowed it (see [`Scope`]).
    pub had_entries: bool,
}

/// What a walk reads and whom it tells how far it got.
#[derive(Clone, Copy, Default)]
pub(super) struct Scope<'a> {
    /// The folder names, folded, from the root down to the one folder that is read whole (a
    /// title's folder). Its ancestors are passed through without their own files, so only the
    /// way down is listed. Empty reads the whole root.
    pub focus: &'a [String],
    /// Told about the titles of a whole-root walk ([`WalkObserver`]); not used with a focus.
    pub observer: Option<&'a dyn WalkObserver>,
}

/// Whether a folder directly in a library folder groups titles instead of being one, as the
/// walk and [`count_units`] count titles: a movie bucket (`A`, `1999`, `1990s`, `Horror`) or a
/// TV A to Z folder.
fn groups_titles(kind: LibraryKind, name: &str) -> bool {
    match kind {
        LibraryKind::Movies => super::movies::is_bucket_name(name),
        LibraryKind::TvShows => name.chars().count() == 1,
    }
}

/// Whether a loose video counts as a title: in a movie library, yes (each is a movie); in a TV
/// library it belongs to no show.
fn loose_videos_count(kind: LibraryKind) -> bool {
    kind == LibraryKind::Movies
}

fn is_video_file(name: &str, path: &Path, skip: &SkipRules) -> bool {
    !skip.is_builtin_skipped(name)
        && is_video_content(path)
        && classify(name).file_type == MediaFileType::Video
}

/// How many titles a whole walk of `root` reads, counted as the walk counts them for its
/// progress: every folder directly in it is one, except a folder that groups titles, whose own
/// folders (and, in a movie library, loose videos) count instead; in a movie library each loose
/// video directly in the root is one too. Skipped and disc folders do not count. Only the root
/// and those grouping folders are listed. `None` when the root cannot be listed.
pub(super) fn count_units(root: &Path, skip: &SkipRules) -> Option<usize> {
    let gate = gate_for(root);
    let kind = skip.kind();
    let listing = {
        let _permit = gate.enter();
        fs::read_dir(root).map(list).ok()?
    };
    if skip.folder_has_marker(
        listing
            .iter()
            .filter(|(_, _, is_dir, ..)| !is_dir)
            .map(|(n, ..)| n.as_str()),
    ) {
        return Some(0);
    }
    let mut units = 0;
    for (name, path, is_dir, ..) in listing {
        if !is_dir {
            if loose_videos_count(kind) && is_video_file(&name, &path, skip) {
                units += 1;
            }
            continue;
        }
        if skip.is_skipped_folder(&name, &path.to_string_lossy()) || is_disc_folder_name(&name) {
            continue;
        }
        units += if groups_titles(kind, &name) {
            count_group(&path, skip)
        } else {
            1
        };
    }
    Some(units)
}

/// The titles of a folder that groups titles: its folders, and in a movie library its videos.
fn count_group(path: &Path, skip: &SkipRules) -> usize {
    let gate = gate_for(path);
    let Ok(listing) = ({
        let _permit = gate.enter();
        fs::read_dir(path).map(list)
    }) else {
        return 0;
    };
    if skip.folder_has_marker(
        listing
            .iter()
            .filter(|(_, _, is_dir, ..)| !is_dir)
            .map(|(n, ..)| n.as_str()),
    ) {
        return 0;
    }
    let kind = skip.kind();
    listing
        .iter()
        .filter(|(name, entry_path, is_dir, ..)| {
            if *is_dir {
                !skip.is_skipped_folder(name, &entry_path.to_string_lossy())
                    && !is_disc_folder_name(name)
            } else {
                loose_videos_count(kind) && is_video_file(name, entry_path, skip)
            }
        })
        .count()
}

/// True when the name ends in a disc folder name (`VIDEO_TS`, `BDMV`, `HVDVD_TS`).
pub(super) fn is_disc_folder_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    ["VIDEO_TS", "BDMV", "HVDVD_TS"]
        .iter()
        .any(|disc| upper.ends_with(disc))
}

/// Reads the data source with up to `threads` workers. `None` when it is missing, unreadable,
/// skipped by the rules or hidden by a marker file; a missing or unreadable source is added to
/// `problems`.
///
/// Sub-folders are read in parallel and joined in name order, so the result does not depend on
/// the number of threads. The one exception is a link that several folders point at: the first
/// folder to reach it keeps it, which may differ between runs.
pub(super) fn read_tree(
    root: &Path,
    skip: &SkipRules,
    threads: usize,
    problems: &mut Vec<ScanProblem>,
    scope: Scope<'_>,
) -> Option<Dir> {
    if !root.is_dir() {
        problems.push(ScanProblem {
            kind: ProblemKind::SourceUnavailable,
            path: root.to_path_buf(),
            message: "The folder does not exist or cannot be reached.".into(),
        });
        return None;
    }
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if skip.is_skipped_folder(&name, &root.to_string_lossy()) {
        return None;
    }
    let mut links = HashSet::new();
    if let Ok(canonical) = fs::canonicalize(root) {
        links.insert(canonical);
    }
    let reader = Reader {
        skip,
        links: Mutex::new(links),
        gate: gate_for(root),
        focus: scope.focus,
        // The titles of a focused walk are the focus itself; the caller counts it.
        observer: scope.observer.filter(|_| scope.focus.is_empty()),
    };
    let (dir, found) =
        reader.read_dir(root, name, String::new(), 0, threads.max(1), Counting::Root);
    problems.extend(found);
    dir
}

/// The entries of one folder listing, with the metadata the scan needs.
fn list(entries: fs::ReadDir) -> Vec<Listed> {
    entries
        .flatten()
        .filter_map(|e| {
            let file_name = e.file_name().to_string_lossy().into_owned();
            let entry_path = e.path();
            let ft = e.file_type().ok()?;
            let is_link = ft.is_symlink();
            if is_link {
                let meta = fs::metadata(&entry_path).ok()?;
                return Some((file_name, entry_path, meta.is_dir(), true, Some(meta)));
            }
            // The listing already carries the size and time on Windows, so this is free
            // there and one `stat` elsewhere.
            let meta = if ft.is_dir() { None } else { e.metadata().ok() };
            Some((file_name, entry_path, ft.is_dir(), false, meta))
        })
        .collect()
}

/// A folder found while listing its parent, read once the listing is done.
struct Pending {
    path: PathBuf,
    name: String,
    rel: String,
    /// What the folder is for the progress of the walk.
    counting: Counting,
}

/// What a folder is to the titles the walk counts ([`Scope::observer`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Counting {
    /// The root: its folders and loose videos are the titles.
    Root,
    /// A folder in the root that groups titles: its folders and loose videos are the titles.
    Group,
    /// One title: read whole, then counted once.
    Title,
    /// Inside a title, or nobody listens.
    Inside,
}

struct Reader<'a> {
    skip: &'a SkipRules,
    /// Canonical targets of folders already entered through a link.
    links: Mutex<HashSet<PathBuf>>,
    /// Listings of the source's drive that may run at once, shared with other walks.
    gate: std::sync::Arc<VolumeGate>,
    /// See [`Scope::focus`].
    focus: &'a [String],
    /// See [`Scope::observer`].
    observer: Option<&'a dyn WalkObserver>,
}

/// One listed entry: name, path, is a folder, is a link, and the metadata of a file.
type Listed = (String, PathBuf, bool, bool, Option<fs::Metadata>);

impl Reader<'_> {
    /// Reads one folder and everything below it. `budget` is the number of threads this folder
    /// may use; its sub-folders share it.
    fn read_dir(
        &self,
        path: &Path,
        name: String,
        rel: String,
        depth: usize,
        budget: usize,
        counting: Counting,
    ) -> (Option<Dir>, Vec<ScanProblem>) {
        let mut problems = Vec::new();
        let is_root = depth == 0;
        // The listing holds a permit of the drive; the sub-folders below take their own.
        let listed = {
            let _permit = self.gate.enter();
            fs::read_dir(path).map(list)
        };
        let mut listing = match listed {
            Ok(listing) => listing,
            Err(e) => {
                problems.push(ScanProblem {
                    kind: if is_root {
                        ProblemKind::SourceUnavailable
                    } else {
                        ProblemKind::Unreadable
                    },
                    path: path.to_path_buf(),
                    message: format!("The folder cannot be read: {e}"),
                });
                return (None, problems);
            }
        };
        listing.sort_by_key(|(n, ..)| n.to_lowercase());

        // A marker file hides the folder and everything below it.
        if self.skip.folder_has_marker(
            listing
                .iter()
                .filter(|(_, _, is_dir, ..)| !is_dir)
                .map(|(n, ..)| n.as_str()),
        ) {
            return (None, problems);
        }

        let mut dir = Dir {
            path: path.to_path_buf(),
            name,
            rel,
            files: Vec::new(),
            dirs: Vec::new(),
            discs: Vec::new(),
            had_entries: false,
        };
        let mut pending: Vec<Pending> = Vec::new();
        for (file_name, entry_path, is_dir, is_link, meta) in listing {
            let child_rel = join_rel(&dir.rel, &file_name);
            if !is_dir {
                // A `.ts` file that is TypeScript source and not a transport stream is no media
                // file at all; only `.ts` files are read for this.
                if !self.skip.is_builtin_skipped(&file_name) && is_video_content(&entry_path) {
                    dir.files.push(entry_from(
                        entry_path,
                        child_rel,
                        file_name,
                        false,
                        meta.as_ref(),
                    ));
                }
                continue;
            }
            if self
                .skip
                .is_skipped_folder(&file_name, &entry_path.to_string_lossy())
            {
                continue;
            }
            if is_disc_folder_name(&file_name) {
                let (disc, found) = self.read_disc(entry_path, child_rel, file_name);
                problems.extend(found);
                dir.discs.extend(disc);
                continue;
            }
            if depth + 1 > MAX_DEPTH {
                continue;
            }
            if is_link {
                let entered = fs::canonicalize(&entry_path).is_ok_and(|target| {
                    self.links
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .insert(target)
                });
                if !entered {
                    continue;
                }
            }
            pending.push(Pending {
                path: entry_path,
                name: file_name,
                rel: child_rel,
                counting: Counting::Inside,
            });
        }
        dir.had_entries = !(dir.files.is_empty() && dir.discs.is_empty() && pending.is_empty());
        if let Some(want) = self.focus.get(depth) {
            // An ancestor of the focused folder: only the way down is read.
            dir.files.clear();
            dir.discs.clear();
            pending.retain(|p| cv_fs::paths::fold_name(&p.name) == *want);
        }
        let observer = self
            .observer
            .filter(|_| matches!(counting, Counting::Root | Counting::Group));
        if let Some(observer) = observer {
            let kind = self.skip.kind();
            let loose = if loose_videos_count(kind) {
                dir.files
                    .iter()
                    .filter(|f| classify(&f.rel).file_type == MediaFileType::Video)
                    .count()
            } else {
                0
            };
            for p in &mut pending {
                p.counting = if counting == Counting::Root && groups_titles(kind, &p.name) {
                    Counting::Group
                } else {
                    Counting::Title
                };
            }
            let titles = pending
                .iter()
                .filter(|p| p.counting == Counting::Title)
                .count();
            observer.found(loose + titles);
            // Loose videos are read with the listing.
            observer.read(loose);
        }

        // Sub-folders are independent: read them side by side, sharing the thread budget.
        let workers = budget.min(pending.len()).max(1);
        let child_budget = (budget / workers).max(1);
        let children = par_map(&pending, workers, |p| {
            let title = p.counting == Counting::Title;
            if let Some(observer) = self.observer.filter(|_| title) {
                observer.reading(&p.name);
            }
            let read = self.read_dir(
                &p.path,
                p.name.clone(),
                p.rel.clone(),
                depth + 1,
                child_budget,
                if p.counting == Counting::Group {
                    Counting::Group
                } else {
                    Counting::Inside
                },
            );
            if let Some(observer) = self.observer.filter(|_| title) {
                observer.read(1);
            }
            read
        });
        for (child, found) in children {
            problems.extend(found);
            dir.dirs.extend(child);
        }
        (Some(dir), problems)
    }

    fn read_disc(
        &self,
        path: PathBuf,
        rel: String,
        name: String,
    ) -> (Option<Disc>, Vec<ScanProblem>) {
        // A disc folder is listed in one go and holds no other listing, so the permit is held
        // until the end.
        let _permit = self.gate.enter();
        let entries = match fs::read_dir(&path) {
            Ok(entries) => entries,
            Err(e) => {
                let problem = ScanProblem {
                    kind: ProblemKind::Unreadable,
                    path,
                    message: format!("The folder cannot be read: {e}"),
                };
                return (None, vec![problem]);
            }
        };
        // The folder stands for the disc's video, so it carries the size of everything in it
        // (a Blu-ray keeps its streams in `BDMV/STREAM`).
        let mut size = 0;
        let mut files: Vec<Entry> = Vec::new();
        for e in entries.flatten() {
            if e.file_type().is_ok_and(|ft| ft.is_dir()) {
                size += folder_size(&e.path(), 1);
                continue;
            }
            let Some(meta) = e.metadata().ok().filter(fs::Metadata::is_file) else {
                continue;
            };
            size += meta.len();
            let file_name = e.file_name().to_string_lossy().into_owned();
            if self.skip.is_builtin_skipped(&file_name) {
                continue;
            }
            files.push(entry_from(
                e.path(),
                join_rel(&rel, &file_name),
                file_name,
                false,
                Some(&meta),
            ));
        }
        files.sort_by_key(|f| f.name.to_lowercase());
        let has_video = files
            .iter()
            .any(|f| classify(&f.rel).file_type == MediaFileType::Video);
        let mut dir = entry_of(path, rel, name, true);
        dir.size = size;
        (
            Some(Disc {
                dir,
                files,
                has_video,
            }),
            Vec::new(),
        )
    }
}

/// The size of every file below a folder inside a disc structure. Links are not followed.
fn folder_size(path: &Path, depth: usize) -> u64 {
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| match e.file_type() {
            Ok(ft) if ft.is_dir() && depth < MAX_DEPTH => folder_size(&e.path(), depth + 1),
            Ok(ft) if ft.is_file() => e.metadata().map_or(0, |m| m.len()),
            _ => 0,
        })
        .sum()
}

fn join_rel(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_owned()
    } else {
        format!("{parent}/{name}")
    }
}

fn entry_of(path: PathBuf, rel: String, name: String, is_dir: bool) -> Entry {
    let meta = fs::metadata(&path).ok();
    entry_from(path, rel, name, is_dir, meta.as_ref())
}

fn entry_from(
    path: PathBuf,
    rel: String,
    name: String,
    is_dir: bool,
    meta: Option<&fs::Metadata>,
) -> Entry {
    let size = meta.filter(|m| m.is_file()).map_or(0, fs::Metadata::len);
    let modified_ms = meta
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
    let created_ms = meta
        .and_then(|m| m.created().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
    let file_id = meta
        .filter(|m| m.is_file())
        .map(file_id_of)
        .unwrap_or_default();
    Entry {
        path,
        rel,
        name,
        size,
        modified_ms,
        created_ms,
        file_id,
        is_dir,
    }
}

/// Inode and device, which survive a rename and tell a replaced file from an edited one.
#[cfg(unix)]
fn file_id_of(meta: &fs::Metadata) -> String {
    use std::os::unix::fs::MetadataExt;
    format!("{:x}:{:x}", meta.dev(), meta.ino())
}

/// Windows exposes the file index only through an open handle (and only on nightly Rust), which
/// would cost one `open` per file. The scan compares size and time there instead.
#[cfg(not(unix))]
fn file_id_of(_meta: &fs::Metadata) -> String {
    String::new()
}
