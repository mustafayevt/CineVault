//! The bin as Settings › Deleted files chooses it (spec `title-deletion` "Where deleted files
//! go"): a [`SystemBin`] that reads the choice at every send and so serves Delete, a move's
//! originals and the files a match change sets aside alike.
//!
//! * **Recycle Bin**: the system bin, as before (the drive's trash folder where it has none).
//! * **Trash folder on each drive**: every path is refused with [`CHOSEN_FOLDER`], so
//!   [`super::send_or_fallback`] and the move engine put it in the drive's `.deletedByCineVault`
//!   without a note.
//! * **Custom trash folder**: a path on the folder's volume is renamed to
//!   `<folder>/.deletedByCineVault/<stamp>/<drive>/<path>` and gets a `folder:` id, so the usual rename restores it; a
//!   path on another volume is refused with [`OTHER_VOLUME`] and goes to its drive's trash folder.
//! * **Delete permanently**: the path is erased; its id is `erased:<path>`, never present and
//!   never restored.
//!
//! What was sent earlier under another choice is still found and restored by its id: the system
//! bin's ids go to the system bin whatever the choice is now.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::trash_bin::{
    BinnedItem, FOLDER_PREFIX, NOT_IN_BIN, PATH_TAKEN, SendOutcome, SystemBin, SystemTrash, size_of,
};
use crate::deleted_files::{DeletedFilesSource, DeletedMode};

/// The prefix of the id of an item that was erased instead of kept.
pub const ERASED_PREFIX: &str = "erased:";

/// The refusal that sends a path to the drive's trash folder because that is the choice; the
/// person chose it, so no note is shown.
pub const CHOSEN_FOLDER: &str = "Deleted files go to the trash folder of each drive";

/// The refusal of a path on another volume than the custom trash folder.
pub const OTHER_VOLUME: &str =
    "This drive is not the custom trash folder's drive, so its files go to its own trash folder";

/// What a restore says about an erased item.
pub const ERASED: &str = "It was erased: Settings › Deleted files is set to Delete permanently";

/// Whether an item was erased instead of kept.
#[must_use]
pub fn is_erased(item: &BinnedItem) -> bool {
    item.bin_id.starts_with(ERASED_PREFIX)
}

/// The bin of Settings › Deleted files over the system bin `system`.
pub struct PolicyBin {
    system: Arc<dyn SystemBin>,
    choice: Arc<dyn DeletedFilesSource>,
}

impl PolicyBin {
    #[must_use]
    pub fn new(system: Arc<dyn SystemBin>, choice: Arc<dyn DeletedFilesSource>) -> Self {
        Self { system, choice }
    }

    /// The real system bin under the choice of `choice`.
    #[must_use]
    pub fn os(choice: Arc<dyn DeletedFilesSource>) -> Arc<dyn SystemBin> {
        Arc::new(Self::new(Arc::new(super::OsTrash), choice))
    }

    fn mode(&self) -> DeletedMode {
        self.choice.deleted_files().mode
    }
}

/// Erases a file, link or folder (links are removed, never followed).
pub(crate) fn erase_path(path: &Path) -> std::io::Result<()> {
    let meta = path.symlink_metadata()?;
    if meta.is_dir() && !meta.file_type().is_symlink() {
        cv_fs::retry_folder("erase", path, || std::fs::remove_dir_all(path))
    } else {
        cv_fs::retry("erase", path, || match std::fs::remove_file(path) {
            // A link to a folder is removed as a folder on Windows.
            Err(e) if cfg!(windows) && meta.file_type().is_symlink() => {
                std::fs::remove_dir(path).or(Err(e))
            }
            other => other,
        })
    }
}

/// `path`, or `name (2).ext`... when taken.
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

impl SystemTrash for PolicyBin {
    fn supports(&self, path: &Path) -> bool {
        self.refusal(path).is_none()
    }

    fn refusal(&self, path: &Path) -> Option<String> {
        match self.mode() {
            DeletedMode::Bin => self.system.refusal(path),
            DeletedMode::Folder => Some(CHOSEN_FOLDER.to_owned()),
            DeletedMode::Custom(folder) => {
                let stamp = "x";
                cv_fs::volume::custom_trash_path(&folder, stamp, path)
                    .is_none()
                    .then(|| OTHER_VOLUME.to_owned())
            }
            DeletedMode::Erase => None,
        }
    }

    fn send(&self, paths: &[PathBuf]) -> Result<(), String> {
        self.send_tracked(paths).error.map_or(Ok(()), Err)
    }
}

impl SystemBin for PolicyBin {
    fn send_tracked(&self, paths: &[PathBuf]) -> SendOutcome {
        match self.mode() {
            DeletedMode::Bin => self.system.send_tracked(paths),
            DeletedMode::Folder => SendOutcome {
                sent: Vec::new(),
                error: Some(CHOSEN_FOLDER.to_owned()),
            },
            DeletedMode::Custom(folder) => {
                let stamp = cv_rename::exec::new_batch_id();
                let mut sent = Vec::new();
                for path in paths {
                    let Some(target) = cv_fs::volume::custom_trash_path(&folder, &stamp, path)
                    else {
                        return SendOutcome {
                            sent,
                            error: Some(OTHER_VOLUME.to_owned()),
                        };
                    };
                    let target = unused(&target);
                    let size = size_of(path);
                    let moved = target
                        .parent()
                        .map_or(Ok(()), std::fs::create_dir_all)
                        .and_then(|()| {
                            cv_fs::retry("trash", path, || std::fs::rename(path, &target))
                        });
                    match moved {
                        Ok(()) => sent.push(BinnedItem {
                            original: path.to_string_lossy().into_owned(),
                            bin_id: format!("{FOLDER_PREFIX}{}", target.to_string_lossy()),
                            size,
                        }),
                        Err(error) => {
                            return SendOutcome {
                                sent,
                                error: Some(format!(
                                    "{} could not go to the custom trash folder: {error}",
                                    path.display()
                                )),
                            };
                        }
                    }
                }
                SendOutcome { sent, error: None }
            }
            DeletedMode::Erase => {
                let mut sent = Vec::new();
                for path in paths {
                    let size = size_of(path);
                    if let Err(error) = erase_path(path) {
                        return SendOutcome {
                            sent,
                            error: Some(format!("{} could not be erased: {error}", path.display())),
                        };
                    }
                    sent.push(BinnedItem {
                        original: path.to_string_lossy().into_owned(),
                        bin_id: format!("{ERASED_PREFIX}{}", path.to_string_lossy()),
                        size,
                    });
                }
                SendOutcome { sent, error: None }
            }
        }
    }

    fn present(&self, items: &[BinnedItem]) -> Vec<bool> {
        let mut out = vec![false; items.len()];
        let mut system = Vec::new();
        for (index, item) in items.iter().enumerate() {
            if is_erased(item) {
                continue;
            }
            if let Some(place) = item.bin_id.strip_prefix(FOLDER_PREFIX) {
                out[index] = Path::new(place).symlink_metadata().is_ok();
            } else {
                system.push(index);
            }
        }
        if !system.is_empty() {
            let list: Vec<BinnedItem> = system.iter().map(|&i| items[i].clone()).collect();
            for (index, here) in system.into_iter().zip(self.system.present(&list)) {
                out[index] = here;
            }
        }
        out
    }

    fn restore(&self, items: &[BinnedItem]) -> Result<(), Vec<(String, String)>> {
        let erased: Vec<(String, String)> = items
            .iter()
            .filter(|item| is_erased(item))
            .map(|item| (item.original.clone(), ERASED.to_owned()))
            .collect();
        if !erased.is_empty() {
            return Err(erased);
        }
        let (folder, system): (Vec<BinnedItem>, Vec<BinnedItem>) = items
            .iter()
            .cloned()
            .partition(|item| item.bin_id.starts_with(FOLDER_PREFIX));
        let mut problems = Vec::new();
        for item in &folder {
            let place = Path::new(item.bin_id.trim_start_matches(FOLDER_PREFIX));
            if Path::new(&item.original).symlink_metadata().is_ok() {
                problems.push((item.original.clone(), PATH_TAKEN.to_owned()));
            } else if place.symlink_metadata().is_err() {
                problems.push((item.original.clone(), NOT_IN_BIN.to_owned()));
            }
        }
        if !problems.is_empty() {
            return Err(problems);
        }
        for item in &folder {
            let place = PathBuf::from(item.bin_id.trim_start_matches(FOLDER_PREFIX));
            let to = Path::new(&item.original);
            if let Some(parent) = to.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Err(error) = cv_fs::retry("restore", &place, || std::fs::rename(&place, to)) {
                problems.push((item.original.clone(), error.to_string()));
            }
        }
        if !problems.is_empty() {
            return Err(problems);
        }
        if system.is_empty() {
            Ok(())
        } else {
            self.system.restore(&system)
        }
    }

    fn purge(&self, items: &[BinnedItem]) -> Result<(), String> {
        let system: Vec<BinnedItem> = items
            .iter()
            .filter(|item| !is_erased(item) && !item.bin_id.starts_with(FOLDER_PREFIX))
            .cloned()
            .collect();
        if system.is_empty() {
            Ok(())
        } else {
            self.system.purge(&system)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::trash_bin::fake::FakeBin;
    use super::super::trash_bin::{present_all, restore_all, send_or_fallback};
    use super::*;
    use crate::deleted_files::{DeletedFiles, FixedDeletedFiles};

    fn bin_with(mode: DeletedMode, system: Arc<FakeBin>) -> PolicyBin {
        PolicyBin::new(
            system,
            Arc::new(FixedDeletedFiles(DeletedFiles {
                mode,
                auto_empty_days: None,
            })),
        )
    }

    fn file(root: &Path, rel: &str) -> PathBuf {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"data").unwrap();
        path
    }

    #[test]
    fn the_trash_folder_choice_never_uses_the_system_bin_and_says_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let system = FakeBin::new(dir.path());
        let bin = bin_with(DeletedMode::Folder, Arc::clone(&system));
        let root = dir.path().join("Movies");
        let path = file(&root, "Heat (1995)/movie.mkv");
        let binned = send_or_fallback(&bin, std::slice::from_ref(&path), &root, "b1").unwrap();
        assert!(binned.notes.is_empty());
        assert!(system.names().is_empty());
        assert!(
            root.join(".deletedByCineVault/b1/Heat (1995)/movie.mkv")
                .is_file()
        );
        assert_eq!(present_all(&bin, &binned.items), vec![true]);
        restore_all(&bin, &binned.items).unwrap();
        assert!(path.is_file());
    }

    #[test]
    fn the_custom_folder_takes_files_of_its_volume_and_gives_them_back() {
        let dir = tempfile::tempdir().unwrap();
        let system = FakeBin::new(dir.path());
        let custom = dir.path().join("MyTrash");
        let bin = bin_with(DeletedMode::Custom(custom.clone()), Arc::clone(&system));
        let root = dir.path().join("Movies");
        let path = file(&root, "Heat (1995)/movie.nfo");
        let binned = send_or_fallback(&bin, std::slice::from_ref(&path), &root, "b1").unwrap();
        assert!(binned.notes.is_empty());
        let place = binned.items[0].bin_id.strip_prefix(FOLDER_PREFIX).unwrap();
        assert!(Path::new(place).starts_with(&custom));
        assert!(place.ends_with("movie.nfo"));
        assert!(!path.exists());
        assert!(system.names().is_empty());
        assert_eq!(bin.present(&binned.items), vec![true]);
        bin.restore(&binned.items).unwrap();
        assert!(path.is_file());
    }

    #[test]
    fn delete_permanently_erases_and_nothing_comes_back() {
        let dir = tempfile::tempdir().unwrap();
        let system = FakeBin::new(dir.path());
        let bin = bin_with(DeletedMode::Erase, Arc::clone(&system));
        let root = dir.path().join("Movies");
        let folder = root.join("Heat (1995)");
        file(&root, "Heat (1995)/movie.mkv");
        let binned = send_or_fallback(&bin, std::slice::from_ref(&folder), &root, "b1").unwrap();
        assert!(!folder.exists());
        assert!(binned.notes.is_empty(), "{:?}", binned.notes);
        assert!(is_erased(&binned.items[0]));
        assert_eq!(binned.items[0].size, 4);
        assert_eq!(present_all(&bin, &binned.items), vec![false]);
        let problems = restore_all(&bin, &binned.items).unwrap_err();
        assert_eq!(problems[0].1, NOT_IN_BIN);
        assert_eq!(bin.restore(&binned.items).unwrap_err()[0].1, ERASED);
        assert!(!root.join(".deletedByCineVault").exists());
    }

    #[test]
    fn items_of_the_system_bin_stay_restorable_after_the_choice_changed() {
        let dir = tempfile::tempdir().unwrap();
        let system = FakeBin::new(dir.path());
        let root = dir.path().join("Movies");
        let path = file(&root, "Heat (1995)/movie.mkv");
        let before = bin_with(DeletedMode::Bin, Arc::clone(&system));
        let binned = send_or_fallback(&before, std::slice::from_ref(&path), &root, "b1").unwrap();
        assert_eq!(system.names().len(), 1);
        let after = bin_with(DeletedMode::Folder, Arc::clone(&system));
        assert_eq!(present_all(&after, &binned.items), vec![true]);
        restore_all(&after, &binned.items).unwrap();
        assert!(path.is_file());
    }

    #[test]
    fn purge_erases_only_the_system_bin_items_it_is_given() {
        let dir = tempfile::tempdir().unwrap();
        let system = FakeBin::new(dir.path());
        let root = dir.path().join("Movies");
        let a = file(&root, "A/a.mkv");
        let b = file(&root, "B/b.mkv");
        let bin = bin_with(DeletedMode::Bin, Arc::clone(&system));
        let first = send_or_fallback(&bin, &[a], &root, "b1").unwrap();
        let second = send_or_fallback(&bin, &[b], &root, "b2").unwrap();
        bin.purge(&first.items).unwrap();
        assert_eq!(bin.present(&first.items), vec![false]);
        assert_eq!(bin.present(&second.items), vec![true]);
    }
}
