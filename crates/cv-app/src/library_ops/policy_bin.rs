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
            if let Err(why) = super::trash_bin::put_back(&place, to) {
                problems.push((item.original.clone(), why));
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

    // ---- The four Deleted-files modes with refusals, fallbacks and nothing lost ----

    use super::super::bin_volumes::{BinVolumes, NoBin, VolumeBin};
    use super::super::trash_bin::Binned;

    /// What the drives say about their bins.
    struct Drives(VolumeBin);

    impl BinVolumes for Drives {
        fn bin_of(&self, _path: &Path) -> VolumeBin {
            self.0
        }
    }

    /// A folder with a film and an NFO of known sizes (1,008 bytes).
    fn title(root: &Path, name: &str) -> PathBuf {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("movie.mkv"), vec![7_u8; 1000]).unwrap();
        std::fs::write(dir.join("movie.nfo"), b"<movie/>").unwrap();
        dir
    }

    /// The bytes found below all of these folders: what is kept, in place, in a bin or in a
    /// trash folder.
    fn kept(folders: &[&Path]) -> u64 {
        folders.iter().map(|folder| size_of(folder)).sum()
    }

    /// The system bin refuses the drive for `why`.
    fn refusing(dir: &Path, why: NoBin) -> Arc<FakeBin> {
        let system = FakeBin::new(dir);
        *system.volumes.lock().unwrap() = Some(Arc::new(Drives(VolumeBin::None(why))));
        system
    }

    /// A system bin that takes the first `take` paths of a send and then stops with an error,
    /// as a bin that fills up or loses its drive part way through a batch.
    struct StopsAfter {
        inner: Arc<FakeBin>,
        take: usize,
    }

    impl SystemTrash for StopsAfter {
        fn supports(&self, _path: &Path) -> bool {
            true
        }

        fn send(&self, paths: &[PathBuf]) -> Result<(), String> {
            self.send_tracked(paths).error.map_or(Ok(()), Err)
        }
    }

    impl SystemBin for StopsAfter {
        fn send_tracked(&self, paths: &[PathBuf]) -> SendOutcome {
            let take = self.take.min(paths.len());
            let mut outcome = self.inner.send_tracked(&paths[..take]);
            if take < paths.len() {
                outcome.error = Some("the bin stopped part way".to_owned());
            }
            outcome
        }

        fn present(&self, items: &[BinnedItem]) -> Vec<bool> {
            self.inner.present(items)
        }

        fn restore(&self, items: &[BinnedItem]) -> Result<(), Vec<(String, String)>> {
            self.inner.restore(items)
        }

        fn purge(&self, items: &[BinnedItem]) -> Result<(), String> {
            self.inner.purge(items)
        }
    }

    #[test]
    fn a_drive_the_bin_refuses_sends_to_its_trash_folder_and_says_why() {
        for (why, words) in [
            (NoBin::NoTrashFolder, "no Trash folder"),
            (NoBin::ReadOnly, "read-only"),
            (NoBin::Network, "network drive"),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let system = refusing(dir.path(), why);
            let bin = bin_with(DeletedMode::Bin, Arc::clone(&system));
            let root = dir.path().join("Movies");
            let heat = title(&root, "Heat (1995)");
            let before = kept(&[&root, &system.dir]);
            assert!(bin.refusal(&heat).is_some(), "{why:?}");

            let Binned { items, notes } =
                send_or_fallback(&bin, std::slice::from_ref(&heat), &root, "b1").unwrap();

            assert_eq!(system.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
            assert!(notes[0].contains(words), "{why:?}: {notes:?}");
            if !cfg!(windows) {
                assert!(notes[0].to_lowercase().contains("trash"), "{notes:?}");
            }
            assert!(items[0].bin_id.starts_with(FOLDER_PREFIX));
            assert!(
                root.join(".deletedByCineVault/b1/Heat (1995)/movie.mkv")
                    .is_file()
            );
            assert!(!heat.exists());
            assert_eq!(kept(&[&root, &system.dir]), before, "{why:?}");
            restore_all(&bin, &items).unwrap();
            assert_eq!(kept(&[&heat]), 1008);
        }
    }

    #[test]
    fn a_bin_that_stops_part_way_leaves_the_rest_in_the_trash_folder_and_erases_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let system = FakeBin::new(dir.path());
        let bin = PolicyBin::new(
            Arc::new(StopsAfter {
                inner: Arc::clone(&system),
                take: 1,
            }),
            Arc::new(FixedDeletedFiles(DeletedFiles {
                mode: DeletedMode::Bin,
                auto_empty_days: None,
            })),
        );
        let root = dir.path().join("Movies");
        let paths: Vec<PathBuf> = ["A (2001)", "B (2002)", "C (2003)"]
            .iter()
            .map(|name| title(&root, name))
            .collect();
        let before = kept(&[&root, &system.dir]);

        let Binned { items, notes } = send_or_fallback(&bin, &paths, &root, "b1").unwrap();

        assert_eq!(items.len(), 3, "every path is accounted for");
        assert_eq!(system.names(), ["A (2001)"]);
        assert!(notes[0].contains("refused"), "{notes:?}");
        for name in ["B (2002)", "C (2003)"] {
            assert!(
                root.join(".deletedByCineVault/b1")
                    .join(name)
                    .join("movie.mkv")
                    .is_file(),
                "{name}"
            );
        }
        assert!(paths.iter().all(|p| !p.exists()));
        assert!(!items.iter().any(is_erased));
        assert_eq!(kept(&[&root, &system.dir]), before, "nothing was lost");
        assert_eq!(present_all(&bin, &items), [true, true, true]);
        restore_all(&bin, &items).unwrap();
        assert_eq!(kept(&[&root, &system.dir]), before, "all three are back");
        assert!(paths.iter().all(|p| p.join("movie.mkv").is_file()));
    }

    /// Two folders on different devices, for the cross-file-system cases: the first is the
    /// usual temp folder, the second a folder on `/dev/shm` (a memory file system). `None`
    /// (with a message) where the machine does not have two.
    #[cfg(unix)]
    fn two_devices() -> Option<(tempfile::TempDir, tempfile::TempDir)> {
        use std::os::unix::fs::MetadataExt;
        let shm = Path::new("/dev/shm");
        if !shm.is_dir() {
            eprintln!("skipped: no second file system (/dev/shm) here");
            return None;
        }
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir_in(shm).unwrap();
        if std::fs::metadata(a.path()).unwrap().dev() == std::fs::metadata(b.path()).unwrap().dev()
        {
            eprintln!("skipped: the temp folder and /dev/shm are one file system here");
            return None;
        }
        Some((a, b))
    }

    #[test]
    #[cfg(unix)]
    fn a_custom_folder_on_another_device_falls_back_to_the_drives_own_trash_folder() {
        let Some((custom_dev, library_dev)) = two_devices() else {
            return;
        };
        let system = FakeBin::new(custom_dev.path());
        let custom = custom_dev.path().join("MyTrash");
        std::fs::create_dir_all(&custom).unwrap();
        let bin = bin_with(DeletedMode::Custom(custom.clone()), Arc::clone(&system));
        let root = library_dev.path().join("Movies");
        let heat = title(&root, "Heat (1995)");
        let before = kept(&[&root, &custom, &system.dir]);

        assert_eq!(bin.refusal(&heat).as_deref(), Some(OTHER_VOLUME));
        let Binned { items, notes } =
            send_or_fallback(&bin, std::slice::from_ref(&heat), &root, "b1").unwrap();

        assert!(notes.is_empty(), "the person is not told: {notes:?}");
        assert!(
            root.join(".deletedByCineVault/b1/Heat (1995)/movie.mkv")
                .is_file()
        );
        assert_eq!(kept(&[&custom]), 0, "nothing crossed to the custom folder");
        assert_eq!(kept(&[&root, &custom, &system.dir]), before);
        restore_all(&bin, &items).unwrap();
        assert!(heat.join("movie.mkv").is_file());
    }

    #[test]
    fn a_custom_folder_on_the_same_device_keeps_every_byte_and_gives_them_back() {
        let dir = tempfile::tempdir().unwrap();
        let system = FakeBin::new(dir.path());
        let custom = dir.path().join("MyTrash");
        let bin = bin_with(DeletedMode::Custom(custom.clone()), Arc::clone(&system));
        let root = dir.path().join("Movies");
        let heat = title(&root, "Heat (1995)");
        let before = kept(&[&root, &custom, &system.dir]);

        let Binned { items, notes } =
            send_or_fallback(&bin, std::slice::from_ref(&heat), &root, "b1").unwrap();

        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(kept(&[&custom]), 1008);
        assert_eq!(kept(&[&root, &custom, &system.dir]), before);
        // Something at the old path blocks the restore; it is left as it is.
        std::fs::create_dir_all(&heat).unwrap();
        std::fs::write(heat.join("other.txt"), b"mine").unwrap();
        let problems = restore_all(&bin, &items).unwrap_err();
        assert_eq!(problems[0].1, PATH_TAKEN);
        assert_eq!(std::fs::read(heat.join("other.txt")).unwrap(), b"mine");
        assert_eq!(kept(&[&custom]), 1008, "the trashed copy is still there");
    }

    #[test]
    fn delete_permanently_erases_only_what_it_is_given() {
        let dir = tempfile::tempdir().unwrap();
        let system = FakeBin::new(dir.path());
        let root = dir.path().join("Movies");
        let heat = title(&root, "Heat (1995)");
        let keep = title(&root, "Keep (2000)");
        // Earlier deletes under other choices, in a trash folder and in the system bin.
        let old = title(&root.join(".deletedByCineVault/b0"), "Old (1990)");
        let binned = title(dir.path(), "Binned (1991)");
        let in_bin = bin_with(DeletedMode::Bin, Arc::clone(&system))
            .send_tracked(std::slice::from_ref(&binned));
        assert!(in_bin.error.is_none());
        let others = kept(&[&keep, &old, &system.dir]);

        let bin = bin_with(DeletedMode::Erase, Arc::clone(&system));
        let Binned { items, notes } =
            send_or_fallback(&bin, std::slice::from_ref(&heat), &root, "b1").unwrap();

        assert!(notes.is_empty(), "{notes:?}");
        assert!(is_erased(&items[0]));
        assert_eq!(items[0].size, 1008);
        assert!(!heat.exists());
        assert_eq!(kept(&[&keep, &old, &system.dir]), others, "only Heat went");
        assert!(old.join("movie.mkv").is_file());
        assert_eq!(
            bin.present(&in_bin.sent),
            [true],
            "the bin item is untouched"
        );
        bin.restore(&in_bin.sent).unwrap();
        assert!(binned.join("movie.mkv").is_file());
    }

    /// A file that differs from another only by case is its own file where names are
    /// case-sensitive: its restore goes to its own path and the other file is not touched.
    #[test]
    #[cfg(target_os = "linux")]
    fn a_restore_leaves_a_file_that_differs_only_by_case_alone() {
        let dir = tempfile::tempdir().unwrap();
        let system = FakeBin::new(dir.path());
        let bin = bin_with(DeletedMode::Folder, Arc::clone(&system));
        let root = dir.path().join("Movies");
        let movie = file(&root, "Heat (1995)/Movie.mkv");
        std::fs::write(&movie, b"the big one").unwrap();

        let Binned { items, .. } =
            send_or_fallback(&bin, std::slice::from_ref(&movie), &root, "b1").unwrap();
        assert!(!movie.exists());
        let other = root.join("Heat (1995)/movie.mkv");
        std::fs::write(&other, b"someone else's").unwrap();

        restore_all(&bin, &items).unwrap();

        assert_eq!(std::fs::read(&movie).unwrap(), b"the big one");
        assert_eq!(std::fs::read(&other).unwrap(), b"someone else's");
        assert_eq!(
            present_all(&bin, &items),
            [false],
            "it left the trash folder"
        );

        // The same name taken is still refused, and nothing is replaced.
        let Binned { items, .. } =
            send_or_fallback(&bin, std::slice::from_ref(&movie), &root, "b2").unwrap();
        std::fs::write(&movie, b"a new file").unwrap();
        let problems = restore_all(&bin, &items).unwrap_err();
        assert_eq!(problems[0].1, PATH_TAKEN);
        assert_eq!(std::fs::read(&movie).unwrap(), b"a new file");
        assert_eq!(present_all(&bin, &items), [true]);
    }

    /// The freedesktop send, as the Linux bin does it, over a Trash folder of the test: one
    /// rename into `files/` with an info file, nothing else.
    #[cfg(unix)]
    struct XdgBin {
        trash: PathBuf,
    }

    #[cfg(unix)]
    impl XdgBin {
        /// Where an item sits in the Trash (`files/<name>` next to its info file).
        fn stored(item: &BinnedItem) -> PathBuf {
            let info = Path::new(&item.bin_id);
            info.parent()
                .and_then(Path::parent)
                .unwrap()
                .join("files")
                .join(info.file_stem().unwrap())
        }
    }

    #[cfg(unix)]
    impl SystemTrash for XdgBin {
        fn supports(&self, _path: &Path) -> bool {
            true
        }

        fn send(&self, paths: &[PathBuf]) -> Result<(), String> {
            self.send_tracked(paths).error.map_or(Ok(()), Err)
        }
    }

    #[cfg(unix)]
    impl SystemBin for XdgBin {
        fn send_tracked(&self, paths: &[PathBuf]) -> SendOutcome {
            let mut sent = Vec::new();
            for path in paths {
                let size = size_of(path);
                match super::super::xdg_trash::move_into(&self.trash, path, "2026-01-01T00:00:00") {
                    Ok(info) => sent.push(BinnedItem {
                        original: path.to_string_lossy().into_owned(),
                        bin_id: info.to_string_lossy().into_owned(),
                        size,
                    }),
                    Err(error) => {
                        return SendOutcome {
                            sent,
                            error: Some(error),
                        };
                    }
                }
            }
            SendOutcome { sent, error: None }
        }

        fn present(&self, items: &[BinnedItem]) -> Vec<bool> {
            items
                .iter()
                .map(|item| {
                    Path::new(&item.bin_id).is_file()
                        && Self::stored(item).symlink_metadata().is_ok()
                })
                .collect()
        }

        fn restore(&self, items: &[BinnedItem]) -> Result<(), Vec<(String, String)>> {
            let mut problems = Vec::new();
            for item in items {
                if Path::new(&item.original).symlink_metadata().is_ok() {
                    problems.push((item.original.clone(), PATH_TAKEN.to_owned()));
                } else if !self.present(std::slice::from_ref(item))[0] {
                    problems.push((item.original.clone(), NOT_IN_BIN.to_owned()));
                }
            }
            if !problems.is_empty() {
                return Err(problems);
            }
            for item in items {
                std::fs::rename(Self::stored(item), &item.original).unwrap();
                std::fs::remove_file(&item.bin_id).unwrap();
            }
            Ok(())
        }
    }

    #[cfg(unix)]
    fn xdg_policy(trash: &Path) -> PolicyBin {
        PolicyBin::new(
            Arc::new(XdgBin {
                trash: trash.to_path_buf(),
            }),
            Arc::new(FixedDeletedFiles(DeletedFiles {
                mode: DeletedMode::Bin,
                auto_empty_days: None,
            })),
        )
    }

    /// Entries directly below `trash/<sub>`; zero when the folder is not there.
    #[cfg(unix)]
    fn entries(trash: &Path, sub: &str) -> usize {
        std::fs::read_dir(trash.join(sub)).map_or(0, Iterator::count)
    }

    #[test]
    #[cfg(unix)]
    fn the_trash_takes_an_item_of_its_own_device_by_one_rename_and_gives_it_back() {
        let dir = tempfile::tempdir().unwrap();
        let trash = dir.path().join("Trash");
        let bin = xdg_policy(&trash);
        let root = dir.path().join("Movies");
        let heat = title(&root, "Heat (1995)");
        let before = kept(&[&root]);

        let Binned { items, notes } =
            send_or_fallback(&bin, std::slice::from_ref(&heat), &root, "b1").unwrap();

        assert!(notes.is_empty(), "{notes:?}");
        assert!(!heat.exists());
        assert!(trash.join("files/Heat (1995)/movie.mkv").is_file());
        let info = std::fs::read_to_string(trash.join("info/Heat (1995).trashinfo")).unwrap();
        assert!(info.contains("Path="), "{info}");
        assert!(!root.join(".deletedByCineVault").exists());
        // The info file is the Trash's own record; the item's bytes are all in files/.
        assert_eq!(kept(&[&root, &trash.join("files")]), before);
        assert_eq!(present_all(&bin, &items), [true]);

        // Something at the old path: refused, nothing replaced, the item stays in the Trash.
        std::fs::create_dir_all(&heat).unwrap();
        std::fs::write(heat.join("other.txt"), b"mine").unwrap();
        let problems = restore_all(&bin, &items).unwrap_err();
        assert_eq!(problems[0].1, PATH_TAKEN);
        assert_eq!(std::fs::read(heat.join("other.txt")).unwrap(), b"mine");
        assert_eq!(present_all(&bin, &items), [true]);

        std::fs::remove_dir_all(&heat).unwrap();
        restore_all(&bin, &items).unwrap();
        assert_eq!(kept(&[&heat]), 1008);
        assert_eq!(entries(&trash, "info"), 0);
    }

    #[test]
    #[cfg(unix)]
    fn an_item_on_another_device_than_the_trash_is_never_copied_into_it() {
        let Some((trash_dev, library_dev)) = two_devices() else {
            return;
        };
        let trash = trash_dev.path().join("Trash");
        let bin = xdg_policy(&trash);
        let root = library_dev.path().join("Movies");
        let heat = title(&root, "Heat (1995)");
        let before = kept(&[&root, &trash]);

        let Binned { items, notes } =
            send_or_fallback(&bin, std::slice::from_ref(&heat), &root, "b1").unwrap();

        assert!(notes[0].contains("refused"), "{notes:?}");
        // The failed rename left nothing in the Trash: no info file, no placeholder, no copy.
        assert_eq!(entries(&trash, "info"), 0);
        assert_eq!(entries(&trash, "files"), 0);
        assert!(
            root.join(".deletedByCineVault/b1/Heat (1995)/movie.mkv")
                .is_file()
        );
        assert_eq!(kept(&[&root, &trash]), before);
        restore_all(&bin, &items).unwrap();
        assert_eq!(kept(&[&heat]), 1008);
    }

    /// Makes a folder read-only for the test and puts the mode back when the test ends, so the
    /// temp folder can be removed.
    #[cfg(unix)]
    struct ReadOnly(PathBuf);

    #[cfg(unix)]
    impl ReadOnly {
        /// `None` (with a message) when the user can write there anyway, as root can.
        fn new(folder: &Path) -> Option<Self> {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(folder, std::fs::Permissions::from_mode(0o555)).unwrap();
            let guard = Self(folder.to_path_buf());
            let probe = folder.join("probe");
            if std::fs::File::create(&probe).is_ok() {
                let _ = std::fs::remove_file(probe);
                eprintln!("skipped: this user can write to a read-only folder");
                return None;
            }
            Some(guard)
        }
    }

    #[cfg(unix)]
    impl Drop for ReadOnly {
        fn drop(&mut self) {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
        }
    }

    #[test]
    #[cfg(unix)]
    fn a_trash_that_cannot_be_written_sends_to_the_trash_folder_and_leaves_no_trace() {
        let dir = tempfile::tempdir().unwrap();
        let trash = dir.path().join("Trash");
        std::fs::create_dir_all(&trash).unwrap();
        let Some(_read_only) = ReadOnly::new(&trash) else {
            return;
        };
        let bin = xdg_policy(&trash);
        let root = dir.path().join("Movies");
        let heat = title(&root, "Heat (1995)");
        let before = kept(&[&root, &trash]);

        let Binned { items, notes } =
            send_or_fallback(&bin, std::slice::from_ref(&heat), &root, "b1").unwrap();

        assert!(notes[0].contains("refused"), "{notes:?}");
        assert_eq!(entries(&trash, ""), 0, "nothing was made in the Trash");
        assert!(
            root.join(".deletedByCineVault/b1/Heat (1995)/movie.nfo")
                .is_file()
        );
        assert_eq!(kept(&[&root, &trash]), before);
        assert_eq!(present_all(&bin, &items), [true]);
    }

    #[test]
    #[cfg(unix)]
    fn a_read_only_library_folder_loses_nothing_when_neither_bin_can_take_the_item() {
        let dir = tempfile::tempdir().unwrap();
        let trash = dir.path().join("Trash");
        let bin = xdg_policy(&trash);
        let root = dir.path().join("Movies");
        let heat = title(&root, "Heat (1995)");
        let before = kept(&[&root, &trash]);
        // The item cannot leave a folder nobody may change: not for the Trash, not for the
        // trash folder.
        let Some(_read_only) = ReadOnly::new(&root) else {
            return;
        };

        let error = send_or_fallback(&bin, std::slice::from_ref(&heat), &root, "b1").unwrap_err();

        assert!(error.sent.is_empty(), "{:?}", error.sent);
        assert!(!error.reason.is_empty());
        assert_eq!(kept(&[&heat]), 1008, "the title is untouched");
        assert_eq!(kept(&[&root, &trash]), before);
        assert_eq!(entries(&trash, "info"), 0);
        assert_eq!(entries(&trash, "files"), 0);
    }
}
