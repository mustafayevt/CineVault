//! "Find missing": video files inside a data source that belong to no library item.
//!
//! The scan walks the data source on its own, without the grouping rules, and compares every
//! video it meets with the files recorded for the library items. It reads the file system and the
//! database and writes nothing.
//!
//! # What counts as a video
//!
//! A file whose [`classify`] type is [`MediaFileType::Video`]. Trailers, samples and extras are
//! companions of a movie and are never listed. A disc structure (`VIDEO_TS`, `BDMV`,
//! `HVDVD_TS`) is listed once, as its disc folder, never file by file.
//!
//! # What counts as managed
//!
//! A video is managed when a stored media file points at it. A stored disc video is a folder, and
//! everything below it is managed too. Paths are compared without regard to case or separator
//! style, as the rescan does.
//!
//! # Where it looks
//!
//! Everywhere below the data source, including folders the skip rules hide (a user pattern or a
//! `.cinevaultignore` / `.nomedia` marker): a video there is exactly what the user wants to hear
//! about, and [`UnmanagedReason::SkippedLocation`] says why a rescan never finds it. System and
//! hidden folders (`$RECYCLE.BIN`, `@eaDir`, `.git`) are not entered.
//!
//! # Adding a listed video
//!
//! [`UnmanagedVideo::folder`] is the folder to scan again. A video in a skipped location is added
//! by changing the skip rule or removing the marker, then rescanning.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use cv_db::repo::data_sources::list_sources;
use cv_db::repo::fingerprint::library_snapshot;
use cv_db::{Database, DbError};
use cv_domain::{DataSource, MediaFileType, SourceKind};

use crate::classify::classify;
use crate::skip::SkipRules;
use crate::walk::WalkOptions;

/// Folders nested deeper than this are not entered (a guard against link loops).
const MAX_DEPTH: usize = 48;

/// Why a video belongs to no item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnmanagedReason {
    /// The scan reads this folder but did not group the video into any movie or episode, for
    /// example an episode file directly in a TV data source, or a file added since the last scan.
    NotGrouped,
    /// A skip rule (user pattern or `.cinevaultignore` / `.nomedia` marker) hides the folder.
    SkippedLocation,
}

/// A video file that is not part of any library item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnmanagedVideo {
    /// The file, or for a disc structure the disc folder.
    pub path: PathBuf,
    /// The folder to scan again to add it: the parent of `path`.
    pub folder: PathBuf,
    /// `path` relative to the data source, `/` separated.
    pub relative: String,
    /// Size in bytes; `0` for a disc folder.
    pub size: u64,
    pub reason: UnmanagedReason,
    /// `path` is a disc folder (`VIDEO_TS`, `BDMV`, `HVDVD_TS`).
    pub is_disc: bool,
}

/// The result of a "Find missing" over one data source.
#[derive(Debug, Clone)]
pub struct UnmanagedReport {
    pub source: DataSource,
    /// Sorted by relative path, case-insensitively.
    pub videos: Vec<UnmanagedVideo>,
    /// The data source folder could not be read, so nothing was compared.
    pub unreadable: bool,
    /// Some sub-folders could not be read.
    pub partial: bool,
}

/// Finds the unmanaged videos of one data source. `options` are the walk options of the
/// source's kind: their skip rules decide which folders count as skipped locations.
pub fn find_unmanaged(
    db: &Database,
    source: &DataSource,
    options: &WalkOptions,
) -> Result<UnmanagedReport, DbError> {
    let root = Path::new(&source.path);
    let managed = db.read(|conn| Ok(managed_keys(&library_snapshot(conn, &source.path)?)))?;
    let mut walk = Walk {
        root,
        rules: &options.skip,
        managed: &managed,
        videos: Vec::new(),
        partial: false,
    };
    let unreadable = fs::read_dir(root).is_err();
    if !unreadable {
        walk.dir(root, "", false, 0);
    }
    let mut videos = walk.videos;
    videos.sort_by_key(|v| v.relative.to_lowercase());
    Ok(UnmanagedReport {
        source: source.clone(),
        videos,
        unreadable,
        partial: walk.partial,
    })
}

/// Finds the unmanaged videos of every data source, movies first.
///
/// `movies` applies to movie sources and `tv` to TV sources.
pub fn find_unmanaged_all(
    db: &Database,
    movies: &WalkOptions,
    tv: &WalkOptions,
) -> Result<Vec<UnmanagedReport>, DbError> {
    let sources = db.read(|conn| list_sources(conn, None))?;
    sources
        .iter()
        .map(|s| {
            let options = match s.kind {
                SourceKind::Movie => movies,
                SourceKind::Tv => tv,
            };
            find_unmanaged(db, s, options)
        })
        .collect()
}

/// A lower-cased path with `/` separators and no trailing separator.
fn key(path: &str) -> String {
    let mut key = path.replace('\\', "/").to_lowercase();
    while key.len() > 1 && key.ends_with('/') {
        key.pop();
    }
    key
}

/// The keys of the stored videos, with the folders of stored discs.
struct Managed {
    files: HashSet<String>,
    /// Keys of stored video entries that are folders (a disc). Not known from the database, so
    /// every video key doubles as a possible folder prefix.
    prefixes: Vec<String>,
}

impl Managed {
    fn contains(&self, path: &str) -> bool {
        let k = key(path);
        self.files.contains(&k)
            || self.prefixes.iter().any(|p| {
                k.len() > p.len() && k.starts_with(p.as_str()) && k[p.len()..].starts_with('/')
            })
    }
}

fn managed_keys(items: &[cv_db::repo::fingerprint::StoredItem]) -> Managed {
    let mut files = HashSet::new();
    let mut prefixes = Vec::new();
    for item in items {
        for f in item.files.iter().filter(|f| f.kind == MediaFileType::Video) {
            let k = format!("{}/{}", key(&f.directory), f.filename.to_lowercase());
            // A disc is stored as a video named like its disc folder.
            if is_disc_name(&f.filename) {
                prefixes.push(k.clone());
            }
            files.insert(k);
        }
    }
    Managed { files, prefixes }
}

fn is_disc_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    ["VIDEO_TS", "BDMV", "HVDVD_TS"]
        .iter()
        .any(|d| upper.ends_with(d))
}

struct Walk<'a> {
    root: &'a Path,
    rules: &'a SkipRules,
    managed: &'a Managed,
    videos: Vec<UnmanagedVideo>,
    partial: bool,
}

impl Walk<'_> {
    fn dir(&mut self, path: &Path, rel: &str, hidden: bool, depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        let Ok(entries) = fs::read_dir(path) else {
            self.partial = true;
            return;
        };
        let hidden = hidden || self.rules.dir_has_marker(path);
        let mut listing: Vec<(String, PathBuf, fs::FileType)> = entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                Some((name, e.path(), e.file_type().ok()?))
            })
            .collect();
        listing.sort_by_key(|(n, ..)| n.to_lowercase());
        for (name, entry_path, file_type) in listing {
            let child_rel = if rel.is_empty() {
                name.clone()
            } else {
                format!("{rel}/{name}")
            };
            if file_type.is_dir() {
                if is_disc_name(&name) {
                    self.disc(&entry_path, &child_rel, hidden);
                } else if !self.rules.is_builtin_skipped(&name) {
                    let user_skipped = self
                        .rules
                        .is_skipped_folder(&name, &entry_path.to_string_lossy());
                    self.dir(&entry_path, &child_rel, hidden || user_skipped, depth + 1);
                }
            } else if file_type.is_file() && !self.rules.is_builtin_skipped(&name) {
                self.file(&entry_path, &child_rel, hidden);
            }
        }
    }

    fn file(&mut self, path: &Path, rel: &str, hidden: bool) {
        if classify(rel).file_type != MediaFileType::Video {
            return;
        }
        if self.managed.contains(&path.to_string_lossy()) {
            return;
        }
        self.push(
            path,
            rel,
            fs::metadata(path).map_or(0, |m| m.len()),
            hidden,
            false,
        );
    }

    fn disc(&mut self, path: &Path, rel: &str, hidden: bool) {
        if self.managed.contains(&path.to_string_lossy()) {
            return;
        }
        // Only a folder that holds something playable is a disc.
        let has_video = fs::read_dir(path).is_ok_and(|entries| {
            entries.flatten().any(|e| {
                let name = e.file_name().to_string_lossy().to_lowercase();
                [".vob", ".ifo", ".m2ts", ".bdmv", ".evo", ".mpls"]
                    .iter()
                    .any(|ext| name.ends_with(ext))
                    || e.file_type().is_ok_and(|t| t.is_dir()) && name == "stream"
            })
        });
        if has_video {
            self.push(path, rel, 0, hidden, true);
        }
    }

    fn push(&mut self, path: &Path, rel: &str, size: u64, hidden: bool, is_disc: bool) {
        debug_assert!(path.starts_with(self.root));
        self.videos.push(UnmanagedVideo {
            path: path.to_path_buf(),
            folder: path.parent().unwrap_or(self.root).to_path_buf(),
            relative: rel.to_owned(),
            size,
            reason: if hidden {
                UnmanagedReason::SkippedLocation
            } else {
                UnmanagedReason::NotGrouped
            },
            is_disc,
        });
    }
}
