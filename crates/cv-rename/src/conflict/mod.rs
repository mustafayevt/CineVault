//! Conflict detection for a batch of planned renames, and the resolutions that clear them.
//!
//! A [`MoviePlan`] or [`TvPlan`] is correct for one item on its own. A batch is not: two movies
//! can render the same folder, a destination can already hold another item's file, a name can be
//! too long for the file system, and a change of case alone is not a rename on a case-insensitive
//! volume. [`detect`] looks at a whole batch ([`BatchItem`]s) and reports every [`Conflict`]. The
//! preview blocks the items involved until the user resolves them; Apply leaves them out
//! ([`applicable`]).
//!
//! | Conflict | Meaning | Resolutions |
//! |---|---|---|
//! | [`ConflictKind::DuplicateDestination`] | two files (of two items, or of one) end at one path, or two items that do not share folders end in one folder | versions, skip, edit |
//! | [`ConflictKind::ForeignDestination`] | the destination exists and is not the item's own file, or a folder to rename onto is not empty, or a folder to move files into holds another item | versions, skip, edit |
//! | [`ConflictKind::PathTooLong`], [`ConflictKind::NameTooLong`] | the path or one name is over the file-system limit ([`PathLimits`]) | skip, edit |
//! | [`ConflictKind::CaseOnlyRename`] | on a case-insensitive volume a copy or a second name differs from its source only in case, so it would overwrite itself | skip, edit |
//!
//! A case-only *move* (`heat.mkv` to `Heat.mkv`) is not a conflict: the executor does it through
//! a temporary name.
//!
//! Nothing here touches the disk. What exists on disk is asked through the [`Disk`] trait; the
//! tests use [`MemoryDisk`].
//!
//! Resolutions ([`resolve()`]) rewrite the plans of the items: [`Resolution::Skip`] leaves an
//! item out, [`Resolution::Edit`] renames its folder or its video base name, and
//! [`Resolution::KeepAsVersions`] puts the items side by side by adding a ` - <label>` to the
//! names that are built on the video name. After resolving, [`detect`] runs again; whatever is
//! left is still blocked.

mod disk;
mod numbered;
mod resolve;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};

pub use disk::{Disk, FsDisk, MemoryDisk, Occupant, OwnerLookup};
pub use numbered::suggest_numbered;
pub use resolve::{NameEdit, Resolution, ResolutionKind, Resolved, resolve};

use crate::movie::MoviePlan;
use crate::options::{NameOptions, SpaceSubstitution};
use crate::plan::{Fate, FolderChange, Op, PlannedFile, ops_of};
use crate::tv::TvPlan;

/// One item of a batch: the plan of a movie or a show with the data needed to judge it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BatchItem {
    /// The library id of the item. Unique in a batch.
    pub id: String,
    /// The absolute path of the data source root the plan's paths are relative to.
    pub root: String,
    pub old_folder: String,
    pub new_folder: String,
    pub folder_change: FolderChange,
    /// The base name of the video files (movies only). `None` for a show.
    pub base_name: Option<String>,
    /// A short name for this item next to its twin, such as `1080p`; used by
    /// [`Resolution::KeepAsVersions`]. `Version <n>` when empty.
    pub version_label: Option<String>,
    /// The destination folder may hold other items too: the movie folder pattern is not unique
    /// per movie (a shared folder such as a letter folder), or the user keeps the item as a
    /// version next to its twins. Two items that do not share their folder may not end in one.
    pub shares_folder: bool,
    pub files: Vec<PlannedFile>,
}

impl BatchItem {
    pub fn from_movie(id: impl Into<String>, root: impl Into<String>, plan: &MoviePlan) -> Self {
        BatchItem {
            id: id.into(),
            root: root.into(),
            old_folder: plan.old_folder.clone(),
            new_folder: plan.new_folder.clone(),
            folder_change: plan.folder_change,
            base_name: Some(plan.base_name.clone()),
            version_label: None,
            shares_folder: plan.shared_folder,
            files: plan.files.clone(),
        }
    }

    pub fn from_tv(id: impl Into<String>, root: impl Into<String>, plan: &TvPlan) -> Self {
        BatchItem {
            id: id.into(),
            root: root.into(),
            old_folder: plan.old_folder.clone(),
            new_folder: plan.new_folder.clone(),
            folder_change: plan.folder_change,
            base_name: None,
            version_label: None,
            shares_folder: false,
            files: plan.files.clone(),
        }
    }

    #[must_use]
    pub fn with_version_label(mut self, label: impl Into<String>) -> Self {
        self.version_label = Some(label.into());
        self
    }

    /// The operations an executor performs for this item.
    pub fn ops(&self) -> Vec<Op> {
        ops_of(&self.files)
    }

    /// Every path the item writes: the new folder and the targets of its files.
    fn destinations(&self) -> Vec<&str> {
        let mut paths = Vec::new();
        if !self.new_folder.is_empty() {
            paths.push(self.new_folder.as_str());
        }
        for file in &self.files {
            if let Fate::Carried { to } = &file.fate {
                paths.extend(to.iter().map(String::as_str));
            }
        }
        paths
    }
}

/// The length limits of a file system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathLimits {
    /// The longest full path, the data source root included.
    pub total: usize,
    /// The longest single name.
    pub component: usize,
    /// How lengths are counted.
    pub unit: LengthUnit,
}

/// How a name's length is counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LengthUnit {
    /// UTF-16 code units (NTFS, exFAT).
    Utf16,
    /// UTF-8 bytes (APFS, ext4, network shares).
    Utf8Bytes,
}

impl PathLimits {
    /// Windows without long path support: `MAX_PATH` is 260 including the terminator.
    pub const fn windows() -> Self {
        PathLimits {
            total: 259,
            component: 255,
            unit: LengthUnit::Utf16,
        }
    }

    /// Windows with `\\?\` paths.
    pub const fn windows_long_paths() -> Self {
        PathLimits {
            total: 32_766,
            component: 255,
            unit: LengthUnit::Utf16,
        }
    }

    /// macOS: `PATH_MAX` is 1024 including the terminator.
    pub const fn macos() -> Self {
        PathLimits {
            total: 1023,
            component: 255,
            unit: LengthUnit::Utf8Bytes,
        }
    }

    /// Linux: `PATH_MAX` is 4096 including the terminator.
    pub const fn linux() -> Self {
        PathLimits {
            total: 4095,
            component: 255,
            unit: LengthUnit::Utf8Bytes,
        }
    }

    /// The limits of the platform CineVault runs on.
    pub const fn native(long_paths: bool) -> Self {
        if cfg!(windows) {
            if long_paths {
                Self::windows_long_paths()
            } else {
                Self::windows()
            }
        } else if cfg!(target_os = "macos") {
            Self::macos()
        } else {
            Self::linux()
        }
    }

    fn measure(&self, text: &str) -> usize {
        match self.unit {
            LengthUnit::Utf16 => text.encode_utf16().count(),
            LengthUnit::Utf8Bytes => text.len(),
        }
    }
}

/// What detection and resolution need to know about the file system and the profile.
#[derive(Debug, Clone)]
pub struct ConflictOptions {
    /// Names that differ only in case are the same name (Windows, default macOS volumes).
    pub case_insensitive: bool,
    pub limits: PathLimits,
    /// Renaming a folder onto an existing non-empty folder merges into it. Off by default.
    pub allow_merge: bool,
    /// How an edited name is cleaned.
    pub name_options: NameOptions,
    pub space: SpaceSubstitution,
}

impl ConflictOptions {
    /// The defaults of the platform CineVault runs on.
    pub fn native() -> Self {
        ConflictOptions {
            case_insensitive: cfg!(any(windows, target_os = "macos")),
            limits: PathLimits::native(false),
            allow_merge: false,
            name_options: NameOptions::default(),
            space: SpaceSubstitution::default(),
        }
    }

    pub(crate) fn fold(&self, text: &str) -> String {
        if self.case_insensitive {
            text.to_lowercase()
        } else {
            text.to_owned()
        }
    }

    /// The identity of a path in a data source root, comparable across items: the full path,
    /// so two items whose roots nest (`D:/TV` and `D:/TV/Show`) are seen to claim one file.
    pub(crate) fn key(&self, root: &str, path: &str) -> String {
        let root = normalise_root(root);
        let full = match (root.is_empty(), path.is_empty()) {
            (true, _) => path.to_owned(),
            (false, true) => root,
            (false, false) => format!("{root}/{path}"),
        };
        self.fold(&full)
    }
}

impl Default for ConflictOptions {
    fn default() -> Self {
        Self::native()
    }
}

pub(crate) fn normalise_root(root: &str) -> String {
    root.replace('\\', "/").trim_end_matches('/').to_owned()
}

/// A problem that blocks one or more items.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub kind: ConflictKind,
    /// The ids of the items involved, in batch order.
    pub items: Vec<String>,
    /// The destination, relative to the data source root.
    pub path: String,
}

impl Conflict {
    /// The resolutions that can clear this conflict.
    pub fn resolutions(&self) -> &'static [ResolutionKind] {
        match self.kind {
            ConflictKind::DuplicateDestination | ConflictKind::ForeignDestination { .. } => &[
                ResolutionKind::KeepAsVersions,
                ResolutionKind::Skip,
                ResolutionKind::Edit,
            ],
            ConflictKind::PathTooLong { .. }
            | ConflictKind::NameTooLong { .. }
            | ConflictKind::CaseOnlyRename => &[ResolutionKind::Skip, ResolutionKind::Edit],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictKind {
    /// Several files would end at `path`.
    DuplicateDestination,
    /// `path` exists and is not a file of the item. `owner` is the library item it belongs to,
    /// if any.
    ForeignDestination { owner: Option<String>, is_dir: bool },
    /// The full path, data source root included, is `length` long; the limit is `limit`.
    PathTooLong { length: usize, limit: usize },
    /// One name in `path` is `length` long; the limit is `limit`.
    NameTooLong { length: usize, limit: usize },
    /// On a case-insensitive volume `path` is the source file itself, or another name of the same
    /// file, in a different case: writing it would overwrite the source.
    CaseOnlyRename,
}

/// Every conflict of a batch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConflictReport {
    pub conflicts: Vec<Conflict>,
}

impl ConflictReport {
    pub fn is_empty(&self) -> bool {
        self.conflicts.is_empty()
    }

    /// The conflicts that involve one item.
    pub fn for_item<'a>(&'a self, id: &'a str) -> impl Iterator<Item = &'a Conflict> {
        self.conflicts
            .iter()
            .filter(move |c| c.items.iter().any(|i| i == id))
    }

    /// The ids of the items Apply must leave out.
    pub fn blocked_items(&self) -> BTreeSet<&str> {
        self.conflicts
            .iter()
            .flat_map(|c| c.items.iter().map(String::as_str))
            .collect()
    }
}

/// The items of a batch that have no conflict: what Apply may run.
pub fn applicable<'a>(items: &'a [BatchItem], report: &ConflictReport) -> Vec<&'a BatchItem> {
    let blocked = report.blocked_items();
    items
        .iter()
        .filter(|item| !blocked.contains(item.id.as_str()))
        .collect()
}

/// Finds every conflict of a batch.
pub fn detect(items: &[BatchItem], disk: &dyn Disk, opts: &ConflictOptions) -> ConflictReport {
    let mut conflicts = Vec::new();
    let duplicate_keys = duplicates(items, opts, &mut conflicts);
    foreign(items, disk, opts, &duplicate_keys, &mut conflicts);
    lengths(items, opts, &mut conflicts);
    case_only(items, opts, &mut conflicts);
    ConflictReport { conflicts }
}

/// Destinations claimed by several files. Returns their keys.
fn duplicates(
    items: &[BatchItem],
    opts: &ConflictOptions,
    out: &mut Vec<Conflict>,
) -> BTreeSet<String> {
    struct Claim {
        path: String,
        claimants: Vec<(usize, String)>,
    }
    let mut claims: BTreeMap<String, Claim> = BTreeMap::new();
    for (index, item) in items.iter().enumerate() {
        for file in &item.files {
            let targets: &[String] = match &file.fate {
                Fate::Carried { to } => to,
                Fate::LeftBehind => std::slice::from_ref(&file.from),
                Fate::Trash => &[],
            };
            for target in targets {
                let claim = claims
                    .entry(opts.key(&item.root, target))
                    .or_insert_with(|| Claim {
                        path: target.clone(),
                        claimants: Vec::new(),
                    });
                let claimant = (index, file.from.clone());
                if !claim.claimants.contains(&claimant) {
                    claim.claimants.push(claimant);
                }
            }
        }
    }
    let mut keys = BTreeSet::new();
    shared_folders(items, opts, &mut keys, out);
    for (key, claim) in claims {
        if claim.claimants.len() < 2 {
            continue;
        }
        let mut ids: Vec<String> = Vec::new();
        for (index, _) in &claim.claimants {
            let id = &items[*index].id;
            if !ids.contains(id) {
                ids.push(id.clone());
            }
        }
        keys.insert(key);
        out.push(Conflict {
            kind: ConflictKind::DuplicateDestination,
            items: ids,
            path: claim.path,
        });
    }
    keys
}

/// Folders that two or more items end in although they do not share folders: their files would
/// be merged into one folder. Items that all stay where they are (already side by side) are left
/// alone. Adds the folder keys to `keys`.
fn shared_folders(
    items: &[BatchItem],
    opts: &ConflictOptions,
    keys: &mut BTreeSet<String>,
    out: &mut Vec<Conflict>,
) {
    let mut folders: BTreeMap<String, (String, Vec<usize>)> = BTreeMap::new();
    for (index, item) in items.iter().enumerate() {
        if item.new_folder.is_empty() || item.shares_folder {
            continue;
        }
        folders
            .entry(opts.key(&item.root, &item.new_folder))
            .or_insert_with(|| (item.new_folder.clone(), Vec::new()))
            .1
            .push(index);
    }
    for (key, (path, members)) in folders {
        if members.len() < 2
            || members
                .iter()
                .all(|&m| opts.fold(&items[m].old_folder) == opts.fold(&items[m].new_folder))
        {
            continue;
        }
        keys.insert(key);
        out.push(Conflict {
            kind: ConflictKind::DuplicateDestination,
            items: members.iter().map(|&m| items[m].id.clone()).collect(),
            path,
        });
    }
}

/// Destinations that exist already and are not the item's own.
fn foreign(
    items: &[BatchItem],
    disk: &dyn Disk,
    opts: &ConflictOptions,
    duplicate_keys: &BTreeSet<String>,
    out: &mut Vec<Conflict>,
) {
    // Paths the batch moves away or trashes, and folders it renames: free once the batch ran.
    let mut vacated_files: BTreeSet<String> = BTreeSet::new();
    let mut vacated_dirs: BTreeSet<String> = BTreeSet::new();
    for item in items {
        if item.folder_change == FolderChange::Rename && !item.old_folder.is_empty() {
            vacated_dirs.insert(opts.key(&item.root, &item.old_folder));
        }
        for file in &item.files {
            let leaves = match &file.fate {
                Fate::Carried { to } => to.first().is_some_and(|first| *first != file.from),
                Fate::Trash => true,
                Fate::LeftBehind => false,
            };
            if leaves {
                vacated_files.insert(opts.key(&item.root, &file.from));
            }
        }
    }
    let is_vacated = |root: &str, path: &str| {
        if vacated_files.contains(&opts.key(root, path))
            || vacated_dirs.contains(&opts.key(root, path))
        {
            return true;
        }
        let mut prefix = path;
        while let Some((parent, _)) = prefix.rsplit_once('/') {
            if vacated_dirs.contains(&opts.key(root, parent)) {
                return true;
            }
            prefix = parent;
        }
        false
    };

    for item in items {
        let own_sources: BTreeSet<String> = item
            .files
            .iter()
            .map(|f| opts.key(&item.root, &f.from))
            .collect();
        let mut reported: BTreeSet<String> = BTreeSet::new();
        let mut merged_folder: Option<String> = None;

        // Renaming the whole folder onto an existing one would merge into it.
        if item.folder_change == FolderChange::Rename
            && !item.new_folder.is_empty()
            && opts.fold(&item.new_folder) != opts.fold(&item.old_folder)
            && let Some(occupant) = disk.occupant(&item.root, &item.new_folder)
            && !is_vacated(&item.root, &item.new_folder)
            && occupant.owner.as_deref() != Some(item.id.as_str())
            && !(occupant.is_dir && (occupant.empty || opts.allow_merge))
        {
            let key = opts.key(&item.root, &item.new_folder);
            if !duplicate_keys.contains(&key) {
                out.push(Conflict {
                    kind: ConflictKind::ForeignDestination {
                        owner: occupant.owner,
                        is_dir: occupant.is_dir,
                    },
                    items: vec![item.id.clone()],
                    path: item.new_folder.clone(),
                });
            }
            reported.insert(key);
            merged_folder = Some(format!("{}/", item.new_folder));
        }

        // Moving the files into a folder another library item lives in would merge them into it.
        let in_batch = |id: &str| items.iter().any(|other| other.id == id);
        if item.folder_change == FolderChange::MoveFiles
            && !item.shares_folder
            && !item.new_folder.is_empty()
            && opts.fold(&item.new_folder) != opts.fold(&item.old_folder)
            && let Some(occupant) = disk.occupant(&item.root, &item.new_folder)
            && occupant.is_dir
            && !occupant.empty
            && !opts.allow_merge
            && !is_vacated(&item.root, &item.new_folder)
            && let Some(owner) = occupant.owner.as_deref()
            && owner != item.id
            && !in_batch(owner)
        {
            let key = opts.key(&item.root, &item.new_folder);
            if !duplicate_keys.contains(&key) && reported.insert(key) {
                out.push(Conflict {
                    kind: ConflictKind::ForeignDestination {
                        owner: occupant.owner,
                        is_dir: true,
                    },
                    items: vec![item.id.clone()],
                    path: item.new_folder.clone(),
                });
            }
        }

        for file in &item.files {
            let Fate::Carried { to } = &file.fate else {
                continue;
            };
            for target in to {
                if *target == file.from || opts.fold(target) == opts.fold(&file.from) {
                    continue;
                }
                if merged_folder
                    .as_deref()
                    .is_some_and(|prefix| target.starts_with(prefix))
                {
                    continue;
                }
                let key = opts.key(&item.root, target);
                if own_sources.contains(&key) || duplicate_keys.contains(&key) {
                    continue;
                }
                let Some(occupant) = disk.occupant(&item.root, target) else {
                    continue;
                };
                if is_vacated(&item.root, target)
                    || occupant.owner.as_deref() == Some(item.id.as_str())
                    || !reported.insert(key)
                {
                    continue;
                }
                out.push(Conflict {
                    kind: ConflictKind::ForeignDestination {
                        owner: occupant.owner,
                        is_dir: occupant.is_dir,
                    },
                    items: vec![item.id.clone()],
                    path: target.clone(),
                });
            }
        }
    }
}

/// The longest path and the longest name of each item, when over the limit.
fn lengths(items: &[BatchItem], opts: &ConflictOptions, out: &mut Vec<Conflict>) {
    let limits = opts.limits;
    for item in items {
        let root = normalise_root(&item.root);
        let root_len = if root.is_empty() {
            0
        } else {
            limits.measure(&root) + 1
        };
        let mut longest_path: Option<(usize, &str)> = None;
        let mut longest_name: Option<(usize, &str)> = None;
        for path in item.destinations() {
            let total = root_len + limits.measure(path);
            if total > limits.total && longest_path.is_none_or(|(len, _)| total > len) {
                longest_path = Some((total, path));
            }
            let name = path
                .split('/')
                .map(|part| limits.measure(part))
                .max()
                .unwrap_or(0);
            if name > limits.component && longest_name.is_none_or(|(len, _)| name > len) {
                longest_name = Some((name, path));
            }
        }
        if let Some((length, path)) = longest_path {
            out.push(Conflict {
                kind: ConflictKind::PathTooLong {
                    length,
                    limit: limits.total,
                },
                items: vec![item.id.clone()],
                path: path.to_owned(),
            });
        }
        if let Some((length, path)) = longest_name {
            out.push(Conflict {
                kind: ConflictKind::NameTooLong {
                    length,
                    limit: limits.component,
                },
                items: vec![item.id.clone()],
                path: path.to_owned(),
            });
        }
    }
}

/// Case-only changes that would overwrite their own source.
fn case_only(items: &[BatchItem], opts: &ConflictOptions, out: &mut Vec<Conflict>) {
    if !opts.case_insensitive {
        return;
    }
    for item in items {
        let mut reported: BTreeSet<String> = BTreeSet::new();
        for file in &item.files {
            let Fate::Carried { to } = &file.fate else {
                continue;
            };
            for (index, target) in to.iter().enumerate() {
                // The first target is moved (through a temporary name when only the case
                // changes); later ones are copies.
                let copies_source =
                    index > 0 && *target != file.from && opts.fold(target) == opts.fold(&file.from);
                let second_name = to[..index]
                    .iter()
                    .any(|earlier| earlier != target && opts.fold(earlier) == opts.fold(target));
                if (copies_source || second_name) && reported.insert(opts.fold(target)) {
                    out.push(Conflict {
                        kind: ConflictKind::CaseOnlyRename,
                        items: vec![item.id.clone()],
                        path: target.clone(),
                    });
                }
            }
        }
    }
}
