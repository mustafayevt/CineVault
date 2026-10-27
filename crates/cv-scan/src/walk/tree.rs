//! Reads a data source into an in-memory tree, applying the skip rules on the way.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

use super::{ProblemKind, ScanProblem};
use crate::classify::{MediaFileType, classify, is_video_content};
use crate::gate::{VolumeGate, gate_for};
use crate::par::par_map;
use crate::skip::SkipRules;

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
    };
    let (dir, found) = reader.read_dir(root, name, String::new(), 0, threads.max(1));
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
}

struct Reader<'a> {
    skip: &'a SkipRules,
    /// Canonical targets of folders already entered through a link.
    links: Mutex<HashSet<PathBuf>>,
    /// Listings of the source's drive that may run at once, shared with other walks.
    gate: std::sync::Arc<VolumeGate>,
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
            });
        }

        // Sub-folders are independent: read them side by side, sharing the thread budget.
        let workers = budget.min(pending.len()).max(1);
        let child_budget = (budget / workers).max(1);
        let children = par_map(&pending, workers, |p| {
            self.read_dir(
                &p.path,
                p.name.clone(),
                p.rel.clone(),
                depth + 1,
                child_budget,
            )
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
        let mut files: Vec<Entry> = entries
            .flatten()
            .filter_map(|e| {
                let file_name = e.file_name().to_string_lossy().into_owned();
                if self.skip.is_builtin_skipped(&file_name) {
                    return None;
                }
                let meta = e.metadata().ok().filter(fs::Metadata::is_file)?;
                Some(entry_from(
                    e.path(),
                    join_rel(&rel, &file_name),
                    file_name,
                    false,
                    Some(&meta),
                ))
            })
            .collect();
        files.sort_by_key(|f| f.name.to_lowercase());
        let has_video = files
            .iter()
            .any(|f| classify(&f.rel).file_type == MediaFileType::Video);
        let dir = entry_of(path, rel, name, true);
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
