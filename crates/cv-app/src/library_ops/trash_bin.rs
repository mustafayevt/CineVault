//! The system bin behind Delete and cross-drive moves, as traits so
//! tests never touch the real one.
//!
//! * [`SystemTrash`] sends paths to the Recycle Bin (Windows) or the Trash (macOS, and the
//!   freedesktop Trash on Linux).
//! * [`SystemBin`] also remembers where each path went ([`BinnedItem::bin_id`]), says whether it
//!   is still there and puts it back. A restore never replaces anything: when the old path is
//!   taken, or an item is no longer in the bin, nothing is restored and each problem is named.
//! * Where the system bin cannot take a path (a network share, a mapped network drive, a
//!   removable drive, a drive whose bin is off, an item larger than the bin; see
//!   [`super::bin_volumes`]) [`send_or_fallback`] moves it to the data source's own trash folder
//!   (`.deletedByCineVault/<batch>/`) instead; such an item has a `bin_id` of
//!   `folder:<path in the trash folder>` and is restored by a rename. After a send, every item is
//!   looked for in the bin, and one that left its place but is not there is reported.
//!
//! On Linux the send is CineVault's own ([`super::xdg_trash`]): one rename on the item's own file
//! system into the Trash that file system uses, never the `trash` crate's send, which can fall
//! back to a copy and a delete. A path that cannot be renamed into a Trash stays where it is and
//! [`send_or_fallback`] moves it to the drive's trash folder. The listing, restore and Empty
//! stay with `trash::os_limited`, working on the info files CineVault wrote.
//!
//! The Windows (and Linux) bin is read through `trash::os_limited`. macOS has no such listing in
//! the crate, so the Trash folder of the volume is listed before and after the send, and a
//! restore is a rename back on the same volume (checked by hand, see the manual checks).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::BIN_NAME;
use super::bin_volumes::{SystemVolumes, bin_refusal};

/// Sends files and folders to the system trash (the Recycle Bin on Windows, the Trash on macOS
/// and Linux).
pub trait SystemTrash: Send + Sync {
    /// False for a path the system trash cannot take, such as a file on a network share. Such a
    /// path is not sent; the caller moves it to the data source's own trash folder instead.
    fn supports(&self, path: &Path) -> bool;

    /// Why the system trash cannot take `path`, in words; `None` when it can.
    fn refusal(&self, path: &Path) -> Option<String> {
        (!self.supports(path))
            .then(|| format!("The {BIN_NAME} cannot take files from this location"))
    }

    /// Moves every path to the trash. On an error some paths may already be there; the caller
    /// looks at the disk to see which.
    ///
    /// # Errors
    /// The system's reason, in words.
    fn send(&self, paths: &[PathBuf]) -> Result<(), String>;
}

/// One path sent to the bin, with what it takes to find it there again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinnedItem {
    /// Where the file or folder was.
    pub original: String,
    /// The bin's own id of the item: the shell's parsing name on Windows, the path inside the
    /// Trash on macOS, `folder:<path>` in a data source's trash folder. Empty when the item went
    /// to the bin but could not be found there afterwards.
    pub bin_id: String,
    /// Bytes, the files of a folder summed.
    pub size: u64,
}

/// What [`SystemBin::send_tracked`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SendOutcome {
    /// The paths that left their place, in the order given.
    pub sent: Vec<BinnedItem>,
    /// The system's reason when some paths could not be sent; those are still in place.
    pub error: Option<String>,
}

/// The system bin with tracking and restore.
pub trait SystemBin: SystemTrash {
    /// Sends the paths to the bin and finds each one there.
    fn send_tracked(&self, paths: &[PathBuf]) -> SendOutcome;

    /// Whether each item is still in the bin, in the order given.
    fn present(&self, items: &[BinnedItem]) -> Vec<bool>;

    /// Puts every item back at its old path. Every item is checked first: when one is gone from
    /// the bin or its old path is taken, nothing is restored.
    ///
    /// # Errors
    /// Each item that could not be restored, with the reason in words.
    fn restore(&self, items: &[BinnedItem]) -> Result<(), Vec<(String, String)>>;

    /// Erases these items from the bin for good (the Drives page's Empty, with its typed
    /// confirmation). Only the items given are touched, never anything else in the bin; an item
    /// that is no longer there is skipped.
    ///
    /// # Errors
    /// The system's reason, in words.
    fn purge(&self, items: &[BinnedItem]) -> Result<(), String> {
        if items.is_empty() {
            Ok(())
        } else {
            Err("This bin cannot erase single items".to_owned())
        }
    }
}

/// The prefix of the `bin_id` of an item in a data source's trash folder.
pub const FOLDER_PREFIX: &str = "folder:";

/// What a restore says when an item is no longer in the bin.
pub const NOT_IN_BIN: &str = "It is no longer in the Recycle Bin or Trash";

/// What a restore says when the old path is taken.
pub const PATH_TAKEN: &str = "Something else is at this path now; nothing was replaced";

/// A bin that takes nothing: [`send_or_fallback`] moves every path to the library folder's own
/// trash folder, which is restored by a rename. For services that were not given the system bin
/// (tests, tools).
#[derive(Debug, Clone, Copy, Default)]
pub struct FolderOnly;

impl SystemTrash for FolderOnly {
    fn supports(&self, _path: &Path) -> bool {
        false
    }

    fn send(&self, _paths: &[PathBuf]) -> Result<(), String> {
        Err("There is no system bin here.".to_owned())
    }
}

impl SystemBin for FolderOnly {
    fn send_tracked(&self, _paths: &[PathBuf]) -> SendOutcome {
        SendOutcome {
            sent: Vec::new(),
            error: Some("There is no system bin here.".to_owned()),
        }
    }

    fn present(&self, items: &[BinnedItem]) -> Vec<bool> {
        vec![false; items.len()]
    }

    fn restore(&self, items: &[BinnedItem]) -> Result<(), Vec<(String, String)>> {
        Err(items
            .iter()
            .map(|item| (item.original.clone(), NOT_IN_BIN.to_owned()))
            .collect())
    }
}

/// The real system bin.
#[derive(Debug, Clone, Copy, Default)]
pub struct OsTrash;

impl SystemTrash for OsTrash {
    fn supports(&self, path: &Path) -> bool {
        self.refusal(path).is_none()
    }

    fn refusal(&self, path: &Path) -> Option<String> {
        bin_refusal(&SystemVolumes, path, size_of)
    }

    fn send(&self, paths: &[PathBuf]) -> Result<(), String> {
        send_paths(paths)
    }
}

/// The system send. On Linux it is one rename into the Trash of the item's own file system;
/// elsewhere the `trash` crate's send.
fn send_paths(paths: &[PathBuf]) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        super::xdg_trash::send_all(paths)
    }
    #[cfg(not(target_os = "linux"))]
    {
        trash::delete_all(paths).map_err(|error| error.to_string())
    }
}

#[cfg(not(target_os = "macos"))]
impl SystemBin for OsTrash {
    fn send_tracked(&self, paths: &[PathBuf]) -> SendOutcome {
        use std::collections::HashSet;
        use trash::os_limited;

        let sizes: Vec<u64> = paths.iter().map(|p| size_of(p)).collect();
        let before: HashSet<std::ffi::OsString> = match os_limited::list() {
            Ok(items) => items.into_iter().map(|i| i.id).collect(),
            Err(error) => {
                return SendOutcome {
                    sent: Vec::new(),
                    error: Some(format!("the {BIN_NAME} could not be read: {error}")),
                };
            }
        };
        let error = send_paths(paths).err();
        let new_items: Vec<trash::TrashItem> = os_limited::list()
            .map(|items| {
                items
                    .into_iter()
                    .filter(|i| !before.contains(&i.id))
                    .collect()
            })
            .unwrap_or_default();
        let mut used: HashSet<std::ffi::OsString> = HashSet::new();
        let mut sent = Vec::new();
        for (path, size) in paths.iter().zip(sizes) {
            if path.symlink_metadata().is_ok() {
                continue;
            }
            let found = new_items
                .iter()
                .filter(|i| !used.contains(&i.id))
                .filter(|i| same_path(&i.original_path(), path))
                .max_by_key(|i| i.time_deleted);
            let bin_id = found.map_or_else(String::new, |i| {
                used.insert(i.id.clone());
                i.id.to_string_lossy().into_owned()
            });
            sent.push(BinnedItem {
                original: path.to_string_lossy().into_owned(),
                bin_id,
                size,
            });
        }
        SendOutcome { sent, error }
    }

    fn present(&self, items: &[BinnedItem]) -> Vec<bool> {
        let ids: BTreeSet<String> = trash::os_limited::list()
            .map(|all| {
                all.into_iter()
                    .map(|i| i.id.to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        items
            .iter()
            .map(|item| !item.bin_id.is_empty() && ids.contains(&item.bin_id))
            .collect()
    }

    fn restore(&self, items: &[BinnedItem]) -> Result<(), Vec<(String, String)>> {
        let listed = trash::os_limited::list().map_err(|error| {
            vec![(
                String::new(),
                format!("the {BIN_NAME} could not be read: {error}"),
            )]
        })?;
        let mut problems = Vec::new();
        let mut chosen = Vec::new();
        for item in items {
            if Path::new(&item.original).symlink_metadata().is_ok() {
                problems.push((item.original.clone(), PATH_TAKEN.to_owned()));
                continue;
            }
            match listed
                .iter()
                .find(|i| !item.bin_id.is_empty() && i.id.to_string_lossy() == item.bin_id)
            {
                Some(found) if same_path(&found.original_path(), Path::new(&item.original)) => {
                    chosen.push(found.clone());
                }
                _ => problems.push((item.original.clone(), NOT_IN_BIN.to_owned())),
            }
        }
        if !problems.is_empty() {
            return Err(problems);
        }
        if chosen.is_empty() {
            return Ok(());
        }
        trash::os_limited::restore_all(chosen)
            .map_err(|error| vec![(String::new(), format!("the restore failed: {error}"))])
    }

    fn purge(&self, items: &[BinnedItem]) -> Result<(), String> {
        let listed = trash::os_limited::list()
            .map_err(|error| format!("the {BIN_NAME} could not be read: {error}"))?;
        // Only the bin's own entry recorded for each item, and only while it is still the
        // item that left that path: never another entry of the bin.
        let chosen: Vec<trash::TrashItem> = listed
            .into_iter()
            .filter(|found| {
                items.iter().any(|item| {
                    !item.bin_id.is_empty()
                        && found.id.to_string_lossy() == item.bin_id
                        && same_path(&found.original_path(), Path::new(&item.original))
                })
            })
            .collect();
        if chosen.is_empty() {
            return Ok(());
        }
        trash::os_limited::purge_all(chosen).map_err(|error| error.to_string())
    }
}

#[cfg(target_os = "macos")]
impl SystemBin for OsTrash {
    fn send_tracked(&self, paths: &[PathBuf]) -> SendOutcome {
        let sizes: Vec<u64> = paths.iter().map(|p| size_of(p)).collect();
        let folders: Vec<Option<PathBuf>> = paths.iter().map(|p| mac::trash_dir_of(p)).collect();
        let listing = |dir: &Option<PathBuf>| -> BTreeSet<String> { mac::names(dir.as_deref()) };
        let before: Vec<BTreeSet<String>> = folders.iter().map(listing).collect();
        let error = trash::delete_all(paths).err().map(|e| e.to_string());
        let mut used: BTreeSet<PathBuf> = BTreeSet::new();
        let mut sent = Vec::new();
        for (((path, size), dir), before) in paths.iter().zip(sizes).zip(&folders).zip(&before) {
            if path.symlink_metadata().is_ok() {
                continue;
            }
            let bin_id = dir
                .as_deref()
                .and_then(|dir| {
                    let name = path.file_name()?.to_string_lossy().into_owned();
                    let stem = path.file_stem()?.to_string_lossy().into_owned();
                    let after = mac::names(Some(dir));
                    let candidate = after
                        .difference(before)
                        .filter(|n| **n == name || n.starts_with(&stem))
                        .map(|n| dir.join(n))
                        .find(|p| !used.contains(p))?;
                    used.insert(candidate.clone());
                    Some(candidate.to_string_lossy().into_owned())
                })
                .unwrap_or_default();
            sent.push(BinnedItem {
                original: path.to_string_lossy().into_owned(),
                bin_id,
                size,
            });
        }
        SendOutcome { sent, error }
    }

    fn present(&self, items: &[BinnedItem]) -> Vec<bool> {
        items
            .iter()
            .map(|i| !i.bin_id.is_empty() && Path::new(&i.bin_id).symlink_metadata().is_ok())
            .collect()
    }

    fn restore(&self, items: &[BinnedItem]) -> Result<(), Vec<(String, String)>> {
        rename_back(items, |item| Some(PathBuf::from(&item.bin_id)))
    }

    fn purge(&self, items: &[BinnedItem]) -> Result<(), String> {
        for item in items {
            let place = Path::new(&item.bin_id);
            // Only a path inside a Trash folder is ever erased here.
            let in_trash = place
                .components()
                .any(|c| matches!(c.as_os_str().to_str(), Some(".Trash" | ".Trashes")));
            if item.bin_id.is_empty() || !in_trash || place.symlink_metadata().is_err() {
                continue;
            }
            super::policy_bin::erase_path(place).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use std::collections::BTreeSet;
    use std::os::unix::fs::MetadataExt;
    use std::path::{Path, PathBuf};

    /// The Trash folder the system uses for `path`: `~/.Trash` on the home volume, else
    /// `<volume>/.Trashes/<uid>`.
    pub(super) fn trash_dir_of(path: &Path) -> Option<PathBuf> {
        let home = PathBuf::from(std::env::var_os("HOME")?);
        let home_meta = std::fs::metadata(&home).ok()?;
        let dev = std::fs::symlink_metadata(path).ok()?.dev();
        if dev == home_meta.dev() {
            return Some(home.join(".Trash"));
        }
        let mut mount = path.to_path_buf();
        while let Some(parent) = mount.parent() {
            match std::fs::metadata(parent) {
                Ok(meta) if meta.dev() == dev => mount = parent.to_path_buf(),
                _ => break,
            }
        }
        Some(mount.join(".Trashes").join(home_meta.uid().to_string()))
    }

    pub(super) fn names(dir: Option<&Path>) -> BTreeSet<String> {
        dir.and_then(|dir| std::fs::read_dir(dir).ok())
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Puts items back by renaming them from where `place` says they are, after checking all of
/// them first. Used for the macOS Trash and the data source trash folder.
fn rename_back(
    items: &[BinnedItem],
    place: impl Fn(&BinnedItem) -> Option<PathBuf>,
) -> Result<(), Vec<(String, String)>> {
    let mut problems = Vec::new();
    for item in items {
        if Path::new(&item.original).symlink_metadata().is_ok() {
            problems.push((item.original.clone(), PATH_TAKEN.to_owned()));
        } else if place(item).is_none_or(|p| p.symlink_metadata().is_err()) {
            problems.push((item.original.clone(), NOT_IN_BIN.to_owned()));
        }
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    for item in items {
        let Some(from) = place(item) else { continue };
        let to = Path::new(&item.original);
        if let Some(parent) = to.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(error) = cv_fs::retry("restore", &from, || std::fs::rename(&from, to)) {
            problems.push((item.original.clone(), error.to_string()));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// The path in a data source's trash folder of a `folder:` item.
fn folder_place(item: &BinnedItem) -> Option<PathBuf> {
    item.bin_id.strip_prefix(FOLDER_PREFIX).map(PathBuf::from)
}

/// What [`send_or_fallback`] sent, with notes for the person (where the files went instead).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Binned {
    pub items: Vec<BinnedItem>,
    pub notes: Vec<String>,
}

/// A path [`send_or_fallback`] could send nowhere. `sent` lists what had gone already.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinError {
    pub sent: Vec<BinnedItem>,
    pub reason: String,
}

/// Sends `paths` to the system bin. A path the bin cannot take (a network share) or that the
/// bin refused goes to `<source_root>/.deletedByCineVault/<batch>/` instead, keeping its place
/// below the root. Nothing is deleted for good.
///
/// # Errors
/// A path that went nowhere, with the reason; the paths sent before it stay sent and are listed
/// in the error's items so the caller can still record them.
pub fn send_or_fallback(
    bin: &dyn SystemBin,
    paths: &[PathBuf],
    source_root: &Path,
    batch: &str,
) -> Result<Binned, BinError> {
    let mut notes = Vec::new();
    let mut sendable = Vec::new();
    let mut fallback = Vec::new();
    let mut quiet = false;
    for path in paths {
        match bin.refusal(path) {
            None => sendable.push(path.clone()),
            Some(why) => {
                // The person chose the trash folder (or a custom one on another drive): no note.
                if why == super::policy_bin::CHOSEN_FOLDER || why == super::policy_bin::OTHER_VOLUME
                {
                    quiet = true;
                } else {
                    let note =
                        format!("{why}; the files went to the library folder's trash folder.");
                    if !notes.contains(&note) {
                        notes.push(note);
                    }
                }
                fallback.push(path.clone());
            }
        }
    }
    let mut items = Vec::new();
    if !sendable.is_empty() {
        let outcome = bin.send_tracked(&sendable);
        // Each item that left its place must be in the bin now: the system may have deleted one
        // for good instead (a bin that filled up or was switched off after the check).
        for (item, here) in outcome.sent.iter().zip(bin.present(&outcome.sent)) {
            if !here && !super::policy_bin::is_erased(item) {
                tracing::warn!(path = %item.original, "a path sent to the bin is not in it");
                notes.push(format!(
                    "{} left its place but is not in the {BIN_NAME}; it may have been deleted \
                     for good.",
                    item.original
                ));
            }
        }
        items.extend(outcome.sent);
        if let Some(error) = outcome.error {
            let left: Vec<PathBuf> = sendable
                .into_iter()
                .filter(|path| path.symlink_metadata().is_ok())
                .collect();
            if !left.is_empty() {
                notes.push(format!(
                    "The {BIN_NAME} refused ({error}); the files went to the library folder's \
                     trash folder."
                ));
                fallback.extend(left);
            }
        }
    }
    if fallback.is_empty() {
        return Ok(Binned { items, notes });
    }
    if notes.is_empty() && !quiet {
        notes.push(format!(
            "The {BIN_NAME} cannot take files from this location; they went to the library \
             folder's trash folder."
        ));
    }
    let trash = source_root.join(cv_rename::exec::TRASH_DIR).join(batch);
    for path in fallback {
        let size = size_of(&path);
        let Some(rel) = super::parts_below(&source_root.to_string_lossy(), &path.to_string_lossy())
            .filter(|parts| !parts.is_empty())
        else {
            return Err(BinError {
                sent: items,
                reason: format!("{} is not inside its library folder", path.display()),
            });
        };
        let target = unused(&rel.iter().fold(trash.clone(), |acc, part| acc.join(part)));
        let moved = target
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| cv_fs::retry("trash", &path, || std::fs::rename(&path, &target)));
        match moved {
            Ok(()) => items.push(BinnedItem {
                original: path.to_string_lossy().into_owned(),
                bin_id: format!("{FOLDER_PREFIX}{}", target.to_string_lossy()),
                size,
            }),
            Err(error) => {
                return Err(BinError {
                    sent: items,
                    reason: format!(
                        "{} could not go to the trash folder: {error}",
                        path.display()
                    ),
                });
            }
        }
    }
    Ok(Binned { items, notes })
}

/// Whether each item is still where it was sent (the bin or a trash folder).
pub fn present_all(bin: &dyn SystemBin, items: &[BinnedItem]) -> Vec<bool> {
    let mut out = vec![false; items.len()];
    let mut system: Vec<usize> = Vec::new();
    for (index, item) in items.iter().enumerate() {
        if item.bin_id.starts_with(FOLDER_PREFIX) {
            out[index] = folder_place(item).is_some_and(|p| p.symlink_metadata().is_ok());
        } else {
            system.push(index);
        }
    }
    if !system.is_empty() {
        let list: Vec<BinnedItem> = system.iter().map(|&i| items[i].clone()).collect();
        for (index, here) in system.into_iter().zip(bin.present(&list)) {
            out[index] = here;
        }
    }
    out
}

/// Puts every item back at its old path, from the bin or a trash folder. Everything is checked
/// first: when an item is gone or its old path is taken, nothing is restored.
///
/// # Errors
/// Each item that could not be restored, with the reason.
pub fn restore_all(bin: &dyn SystemBin, items: &[BinnedItem]) -> Result<(), Vec<(String, String)>> {
    let mut problems = Vec::new();
    let mut seen = BTreeSet::new();
    for (item, here) in items.iter().zip(present_all(bin, items)) {
        if !seen.insert(item.original.to_lowercase()) {
            continue;
        }
        if Path::new(&item.original).symlink_metadata().is_ok() {
            problems.push((item.original.clone(), PATH_TAKEN.to_owned()));
        } else if !here {
            problems.push((item.original.clone(), NOT_IN_BIN.to_owned()));
        }
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    let (folder, system): (Vec<BinnedItem>, Vec<BinnedItem>) = items
        .iter()
        .cloned()
        .partition(|item| item.bin_id.starts_with(FOLDER_PREFIX));
    rename_back(&folder, folder_place)?;
    if system.is_empty() {
        Ok(())
    } else {
        bin.restore(&system)
    }
}

/// `path`, or `name (2).ext`, `name (3).ext`... when something is there already.
fn unused(path: &Path) -> PathBuf {
    if path.symlink_metadata().is_err() {
        return path.to_path_buf();
    }
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    (2_u32..)
        .map(|n| path.with_file_name(format!("{stem} ({n}){ext}")))
        .find(|candidate| candidate.symlink_metadata().is_err())
        .unwrap_or_else(|| path.to_path_buf())
}

/// The bytes of a file, or of every file below a folder (links are not followed).
pub(crate) fn size_of(path: &Path) -> u64 {
    let Ok(meta) = path.symlink_metadata() else {
        return 0;
    };
    if !meta.is_dir() {
        return meta.len();
    }
    std::fs::read_dir(path)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| size_of(&entry.path()))
                .sum()
        })
        .unwrap_or(0)
}

/// Two paths name the same place: separators, the `\?\` prefix and a trailing separator do not
/// matter, and case matters only where file names are case-sensitive (Linux), where
/// `Movie.mkv` and `movie.mkv` are two files.
#[cfg_attr(target_os = "macos", allow(dead_code))]
fn same_path(a: &Path, b: &Path) -> bool {
    cv_fs::paths::same_path(a, b)
}

/// A bin in a folder, for tests: items are renamed into it, can be "emptied", and a location can
/// be refused or every send made to fail.
#[cfg(test)]
#[allow(dead_code)]
pub(crate) mod fake {
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use super::super::BIN_NAME;
    use super::super::bin_volumes::{BinVolumes, bin_refusal};
    use super::{BinnedItem, SendOutcome, SystemBin, SystemTrash, rename_back, size_of};

    pub(crate) struct FakeBin {
        pub(crate) dir: PathBuf,
        pub(crate) fail: AtomicBool,
        pub(crate) refuse: Mutex<Option<PathBuf>>,
        /// What the drives say about their bins, when a test sets it.
        pub(crate) volumes: Mutex<Option<std::sync::Arc<dyn BinVolumes>>>,
        /// The "system" deletes what it is sent for good instead of keeping it, as Windows may
        /// on a drive without a bin.
        pub(crate) lose: AtomicBool,
        pub(crate) calls: AtomicUsize,
        next: AtomicUsize,
    }

    impl FakeBin {
        pub(crate) fn new(parent: &Path) -> std::sync::Arc<Self> {
            let dir = parent.join("bin");
            std::fs::create_dir_all(&dir).unwrap();
            std::sync::Arc::new(Self {
                dir,
                fail: AtomicBool::new(false),
                refuse: Mutex::new(None),
                volumes: Mutex::new(None),
                lose: AtomicBool::new(false),
                calls: AtomicUsize::new(0),
                next: AtomicUsize::new(1),
            })
        }

        /// The names of what is in the bin (without the numbering).
        pub(crate) fn names(&self) -> Vec<String> {
            let mut names: Vec<String> = std::fs::read_dir(&self.dir)
                .unwrap()
                .map(|e| {
                    let name = e.unwrap().file_name().to_string_lossy().into_owned();
                    name.split_once('-')
                        .map_or(name.clone(), |(_, n)| n.to_owned())
                })
                .collect();
            names.sort();
            names
        }

        /// Empties the bin, as the user would.
        pub(crate) fn empty(&self) {
            for entry in std::fs::read_dir(&self.dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    std::fs::remove_dir_all(path).unwrap();
                } else {
                    std::fs::remove_file(path).unwrap();
                }
            }
        }
    }

    impl SystemTrash for FakeBin {
        fn supports(&self, path: &Path) -> bool {
            self.refuse
                .lock()
                .unwrap()
                .as_ref()
                .is_none_or(|prefix| !path.starts_with(prefix))
        }

        fn refusal(&self, path: &Path) -> Option<String> {
            if !self.supports(path) {
                return Some(format!(
                    "The {BIN_NAME} cannot take files from this location"
                ));
            }
            let volumes = self.volumes.lock().unwrap().clone();
            volumes.and_then(|v| bin_refusal(v.as_ref(), path, size_of))
        }

        fn send(&self, paths: &[PathBuf]) -> Result<(), String> {
            let outcome = self.send_tracked(paths);
            outcome.error.map_or(Ok(()), Err)
        }
    }

    impl SystemBin for FakeBin {
        fn send_tracked(&self, paths: &[PathBuf]) -> SendOutcome {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.fail.load(Ordering::SeqCst) {
                return SendOutcome {
                    sent: Vec::new(),
                    error: Some("the bin is full".to_owned()),
                };
            }
            let mut sent = Vec::new();
            for path in paths {
                let size = size_of(path);
                if self.lose.load(Ordering::SeqCst) {
                    if path.is_dir() {
                        std::fs::remove_dir_all(path).unwrap();
                    } else {
                        std::fs::remove_file(path).unwrap();
                    }
                    sent.push(BinnedItem {
                        original: path.to_string_lossy().into_owned(),
                        bin_id: String::new(),
                        size,
                    });
                    continue;
                }
                let n = self.next.fetch_add(1, Ordering::SeqCst);
                let name = path.file_name().unwrap().to_string_lossy();
                let target = self.dir.join(format!("{n}-{name}"));
                if let Err(error) = std::fs::rename(path, &target) {
                    return SendOutcome {
                        sent,
                        error: Some(error.to_string()),
                    };
                }
                sent.push(BinnedItem {
                    original: path.to_string_lossy().into_owned(),
                    bin_id: target.to_string_lossy().into_owned(),
                    size,
                });
            }
            SendOutcome { sent, error: None }
        }

        fn present(&self, items: &[BinnedItem]) -> Vec<bool> {
            items
                .iter()
                .map(|i| !i.bin_id.is_empty() && Path::new(&i.bin_id).symlink_metadata().is_ok())
                .collect()
        }

        fn restore(&self, items: &[BinnedItem]) -> Result<(), Vec<(String, String)>> {
            rename_back(items, |item| Some(PathBuf::from(&item.bin_id)))
        }

        fn purge(&self, items: &[BinnedItem]) -> Result<(), String> {
            for item in items {
                let place = Path::new(&item.bin_id);
                if !item.bin_id.is_empty() && place.starts_with(&self.dir) && place.exists() {
                    if place.is_dir() {
                        std::fs::remove_dir_all(place).map_err(|e| e.to_string())?;
                    } else {
                        std::fs::remove_file(place).map_err(|e| e.to_string())?;
                    }
                }
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::FakeBin;
    use super::*;

    fn folder_with_file(root: &Path, name: &str) -> PathBuf {
        let folder = root.join(name);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("movie.mkv"), b"twelve bytes").unwrap();
        folder
    }

    #[test]
    fn a_sent_folder_is_tracked_present_and_restored() {
        let dir = tempfile::tempdir().unwrap();
        let bin = FakeBin::new(dir.path());
        let root = dir.path().join("movies");
        let heat = folder_with_file(&root, "Heat (1995)");
        let Binned { items, notes } =
            send_or_fallback(bin.as_ref(), std::slice::from_ref(&heat), &root, "b1").unwrap();
        assert!(notes.is_empty());
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].size, 12);
        assert!(!heat.exists());
        assert_eq!(present_all(bin.as_ref(), &items), [true]);
        restore_all(bin.as_ref(), &items).unwrap();
        assert!(heat.join("movie.mkv").is_file());
    }

    #[test]
    fn a_restore_never_replaces_what_is_at_the_old_path() {
        let dir = tempfile::tempdir().unwrap();
        let bin = FakeBin::new(dir.path());
        let root = dir.path().join("movies");
        let heat = folder_with_file(&root, "Heat (1995)");
        let Binned { items, .. } =
            send_or_fallback(bin.as_ref(), std::slice::from_ref(&heat), &root, "b1").unwrap();
        std::fs::create_dir_all(&heat).unwrap();
        std::fs::write(heat.join("new.txt"), b"new").unwrap();
        let problems = restore_all(bin.as_ref(), &items).unwrap_err();
        assert_eq!(problems[0].1, PATH_TAKEN);
        assert!(heat.join("new.txt").is_file());
        assert_eq!(
            present_all(bin.as_ref(), &items),
            [true],
            "the bin is untouched"
        );
    }

    #[test]
    fn an_emptied_bin_restores_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let bin = FakeBin::new(dir.path());
        let root = dir.path().join("movies");
        let a = folder_with_file(&root, "A");
        let b = folder_with_file(&root, "B");
        let Binned { items, .. } =
            send_or_fallback(bin.as_ref(), &[a.clone(), b.clone()], &root, "b").unwrap();
        // Only the second is gone: the first stays in the bin too.
        std::fs::remove_dir_all(&items[1].bin_id).unwrap();
        let problems = restore_all(bin.as_ref(), &items).unwrap_err();
        assert_eq!(
            problems,
            [(b.to_string_lossy().into_owned(), NOT_IN_BIN.to_owned())]
        );
        assert!(!a.exists(), "nothing was restored");
        assert_eq!(present_all(bin.as_ref(), &items), [true, false]);
    }

    #[test]
    fn a_location_the_bin_cannot_take_goes_to_the_trash_folder_and_comes_back() {
        let dir = tempfile::tempdir().unwrap();
        let bin = FakeBin::new(dir.path());
        let root = dir.path().join("movies");
        *bin.refuse.lock().unwrap() = Some(root.clone());
        let heat = folder_with_file(&root, "Heat (1995)");
        let Binned { items, notes } =
            send_or_fallback(bin.as_ref(), std::slice::from_ref(&heat), &root, "b9").unwrap();
        assert_eq!(notes.len(), 1);
        assert!(items[0].bin_id.starts_with(FOLDER_PREFIX));
        assert!(
            root.join(".deletedByCineVault/b9/Heat (1995)/movie.mkv")
                .is_file()
        );
        assert_eq!(bin.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        restore_all(bin.as_ref(), &items).unwrap();
        assert!(heat.join("movie.mkv").is_file());
    }

    #[test]
    fn a_refusing_bin_falls_back_to_the_trash_folder() {
        let dir = tempfile::tempdir().unwrap();
        let bin = FakeBin::new(dir.path());
        bin.fail.store(true, std::sync::atomic::Ordering::SeqCst);
        let root = dir.path().join("movies");
        let heat = folder_with_file(&root, "Heat (1995)");
        let Binned { items, notes } = send_or_fallback(bin.as_ref(), &[heat], &root, "b2").unwrap();
        assert!(notes[0].contains("bin is full"));
        assert!(items[0].bin_id.starts_with(FOLDER_PREFIX));
    }

    struct Volumes(super::super::bin_volumes::VolumeBin);

    impl super::super::bin_volumes::BinVolumes for Volumes {
        fn bin_of(&self, _path: &Path) -> super::super::bin_volumes::VolumeBin {
            self.0
        }
    }

    #[test]
    fn a_drive_without_a_bin_sends_to_the_trash_folder_with_the_reason() {
        use super::super::bin_volumes::{NoBin, VolumeBin};
        let dir = tempfile::tempdir().unwrap();
        let bin = FakeBin::new(dir.path());
        *bin.volumes.lock().unwrap() = Some(std::sync::Arc::new(Volumes(VolumeBin::None(
            NoBin::Removable,
        ))));
        let root = dir.path().join("movies");
        let heat = folder_with_file(&root, "Heat (1995)");
        let Binned { items, notes } =
            send_or_fallback(bin.as_ref(), std::slice::from_ref(&heat), &root, "b3").unwrap();
        assert_eq!(bin.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert!(items[0].bin_id.starts_with(FOLDER_PREFIX));
        assert!(notes[0].contains("removable drive"), "{notes:?}");
        assert!(
            root.join(".deletedByCineVault/b3/Heat (1995)/movie.mkv")
                .is_file()
        );
    }

    #[test]
    fn an_item_larger_than_the_bin_goes_to_the_trash_folder() {
        use super::super::bin_volumes::VolumeBin;
        let dir = tempfile::tempdir().unwrap();
        let bin = FakeBin::new(dir.path());
        *bin.volumes.lock().unwrap() = Some(std::sync::Arc::new(Volumes(VolumeBin::Takes {
            max_bytes: Some(11),
        })));
        let root = dir.path().join("movies");
        let heat = folder_with_file(&root, "Heat (1995)");
        let small = root.join("small.txt");
        std::fs::write(&small, b"tiny").unwrap();
        let Binned { items, notes } =
            send_or_fallback(bin.as_ref(), &[heat, small], &root, "b4").unwrap();
        assert_eq!(bin.names(), ["small.txt"], "the small file fits the bin");
        assert_eq!(items.len(), 2);
        assert!(notes[0].contains("is larger than the"), "{notes:?}");
        assert!(
            root.join(".deletedByCineVault/b4/Heat (1995)/movie.mkv")
                .is_file()
        );
    }

    #[test]
    fn a_sent_item_that_is_not_in_the_bin_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let bin = FakeBin::new(dir.path());
        bin.lose.store(true, std::sync::atomic::Ordering::SeqCst);
        let root = dir.path().join("movies");
        let heat = folder_with_file(&root, "Heat (1995)");
        let Binned { items, notes } =
            send_or_fallback(bin.as_ref(), std::slice::from_ref(&heat), &root, "b5").unwrap();
        assert_eq!(
            items.len(),
            1,
            "it is still recorded, so Undo can say what happened"
        );
        assert!(
            notes[0].contains(&format!("not in the {BIN_NAME}")),
            "{notes:?}"
        );
        assert_eq!(present_all(bin.as_ref(), &items), [false]);
    }

    /// The real Recycle Bin, opt-in: `CINEVAULT_LIVE_BIN=1 cargo test -p cv-app bin_live`.
    #[test]
    #[cfg(windows)]
    fn bin_live_round_trip_through_the_recycle_bin() {
        if std::env::var("CINEVAULT_LIVE_BIN").as_deref() != Ok("1") {
            eprintln!("skipped: set CINEVAULT_LIVE_BIN=1 to use the real Recycle Bin");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("cinevault-bin-live.txt");
        std::fs::write(&file, b"cinevault live bin test").unwrap();
        let outcome = OsTrash.send_tracked(std::slice::from_ref(&file));
        assert!(outcome.error.is_none(), "{outcome:?}");
        assert_eq!(outcome.sent.len(), 1);
        assert!(!outcome.sent[0].bin_id.is_empty(), "{outcome:?}");
        assert!(!file.exists());
        assert_eq!(OsTrash.present(&outcome.sent), [true]);
        // A file at the old path blocks the restore and is left alone.
        std::fs::write(&file, b"other").unwrap();
        assert!(OsTrash.restore(&outcome.sent).is_err());
        std::fs::remove_file(&file).unwrap();
        OsTrash.restore(&outcome.sent).unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"cinevault live bin test");
        assert_eq!(OsTrash.present(&outcome.sent), [false]);
    }

    /// Names that differ only by case are two files where file names are case-sensitive, so a
    /// Trash entry of one is never taken for the other.
    #[test]
    #[cfg(target_os = "linux")]
    fn same_path_keeps_case_on_linux() {
        assert!(!same_path(
            Path::new("/data/Movie.mkv"),
            Path::new("/data/movie.mkv")
        ));
        assert!(same_path(
            Path::new("/data/Movie.mkv"),
            Path::new("/data/Movie.mkv/")
        ));
    }

    /// The real freedesktop Trash of the user, opt-in: send a temp folder under `$HOME`, find it
    /// there, put it back, send it again and erase only it.
    /// `cargo test -p cv-app bin_live -- --ignored`.
    #[test]
    #[ignore = "uses the real Trash of the user"]
    #[cfg(target_os = "linux")]
    fn bin_live_round_trip_through_the_trash() {
        // Under $HOME, so the item is on the file system of the home Trash.
        let home = dirs::home_dir().expect("a home folder");
        let dir = tempfile::Builder::new()
            .prefix("cinevault-bin-live")
            .tempdir_in(home)
            .unwrap();
        let folder = dir.path().join("Live Title (2000)");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("movie.mkv"), b"cinevault live bin test").unwrap();
        let outcome = OsTrash.send_tracked(std::slice::from_ref(&folder));
        assert!(outcome.error.is_none(), "{outcome:?}");
        assert_eq!(outcome.sent.len(), 1);
        assert!(!outcome.sent[0].bin_id.is_empty(), "{outcome:?}");
        assert!(!folder.exists());
        assert_eq!(OsTrash.present(&outcome.sent), [true]);
        // Something at the old path blocks the restore and is left alone.
        std::fs::create_dir_all(&folder).unwrap();
        assert!(OsTrash.restore(&outcome.sent).is_err());
        std::fs::remove_dir_all(&folder).unwrap();
        OsTrash.restore(&outcome.sent).unwrap();
        assert_eq!(
            std::fs::read(folder.join("movie.mkv")).unwrap(),
            b"cinevault live bin test"
        );
        assert_eq!(OsTrash.present(&outcome.sent), [false]);
        // Empty touches only the item CineVault recorded.
        let again = OsTrash.send_tracked(std::slice::from_ref(&folder));
        assert!(again.error.is_none(), "{again:?}");
        assert_eq!(OsTrash.present(&again.sent), [true]);
        OsTrash.purge(&again.sent).unwrap();
        assert_eq!(OsTrash.present(&again.sent), [false]);
        assert!(!folder.exists());
    }
}
