//! Path edge cases of renaming: paths over the
//! Windows limit, Unicode names in their composed (NFC) and decomposed (NFD) forms, a volume that
//! ignores the difference (as APFS does), read-only files, files another program holds open, and
//! network (UNC) paths. Each runs the executor and then the undo, and checks every file is at its
//! old or its new name, byte for byte, with nothing lost.
//!
//! Case-only renames are covered next to the executor (`tests::a_case_only_rename_*`,
//! `safety_tests::on_a_case_*`) and through Organize in cv-app.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::tests::{FaultFs, NFO, POSTER, Tree, VIDEO, batch_item, carried, run_with, trash_file};
use super::*;
use crate::conflict::BatchItem;
use crate::plan::FolderChange;

fn undo_all(journal: &mut dyn JournalStore, roots: &[PathBuf], batch: &str) -> UndoReport {
    let options = UndoOptions {
        roots: roots.to_vec(),
        ..UndoOptions::default()
    };
    undo_batch(journal, &StdFs, batch, &options).unwrap()
}

/// Runs `items` over `fs`, checks they were all done and changed the tree, undoes the batch and
/// checks the tree is exactly as before (names spelled the same, bytes the same).
#[cfg(windows)]
fn rename_and_undo(tree: &Tree, fs: &dyn Fs, items: &[BatchItem]) {
    let before = tree.live();
    let mut journal = MemoryJournal::new();
    let report = run_with(fs, &mut journal, ExecOptions::new("b1"), items);
    assert_eq!(report.failed(), 0, "{report:?}");
    assert_ne!(tree.live(), before, "the batch changed nothing");
    assert!(
        journal.all().iter().all(|r| r.state == JournalState::Done),
        "{:?}",
        journal.all()
    );
    let undone = undo_all(&mut journal, std::slice::from_ref(&tree.root), "b1");
    assert!(undone.is_complete(), "{undone:?}");
    assert_eq!(tree.live(), before);
}

// ---- paths over the Windows limit ------------------------------------------------------------

#[cfg(windows)]
#[test]
fn a_folder_deeper_than_the_windows_limit_is_renamed_its_leftover_trashed_and_undone() {
    let tree = Tree::new();
    let deep = format!("{}/{}", "d".repeat(110), "e".repeat(90));
    let old = format!("{deep}/heat");
    let new = format!("{deep}/Heat (1995)");
    let video = format!("{}.mkv", "h".repeat(40));
    tree.write(&format!("{old}/{video}"), "video bytes");
    tree.write(&format!("{old}/heat.nfo"), super::tests::MANAGED_NFO);
    tree.write(&format!("{old}/folder.jpg"), "stale artwork");
    let longest = tree.root.join(format!("{new}/Heat (1995).mkv"));
    assert!(
        longest.to_string_lossy().len() > 260,
        "{}",
        longest.display()
    );
    let item = batch_item(
        &tree,
        "1",
        &old,
        &new,
        FolderChange::Rename,
        vec![
            carried(
                &format!("{old}/{video}"),
                &format!("{new}/Heat (1995).mkv"),
                VIDEO,
            ),
            carried(
                &format!("{old}/heat.nfo"),
                &format!("{new}/Heat (1995).nfo"),
                NFO,
            ),
            trash_file(
                &format!("{old}/folder.jpg"),
                cv_domain::MediaFileType::Thumb,
            ),
        ],
    );
    let before = tree.live();
    let mut journal = MemoryJournal::new();
    let report = run_with(&StdFs, &mut journal, ExecOptions::new("b1"), &[item]);
    assert_eq!(report.done(), 1, "{report:?}");
    assert_eq!(tree.read(&format!("{new}/Heat (1995).mkv")), "video bytes");
    assert!(StdFs.meta(&longest).unwrap().is_some());
    assert_eq!(
        tree.trashed().len(),
        1,
        "the leftover went to the trash folder"
    );

    let undone = undo_all(&mut journal, std::slice::from_ref(&tree.root), "b1");
    assert!(undone.is_complete(), "{undone:?}");
    assert_eq!(tree.live(), before);
}

// ---- NFC and NFD -------------------------------------------------------------------------------

/// "Amélie" with a composed é (U+00E9) and with an e and a combining acute (U+0301).
const NFC: &str = "Am\u{e9}lie (2001)";
const NFD: &str = "Ame\u{301}lie (2001)";

/// A title folder spelled in NFD (as a Mac writes it) renamed to the NFC spelling the
/// templates give.
fn nfd_to_nfc(tree: &Tree) -> BatchItem {
    tree.write(&format!("{NFD}/{NFD}.mkv"), "video bytes");
    tree.write(&format!("{NFD}/poster.jpg"), "poster bytes");
    batch_item(
        tree,
        "1",
        NFD,
        NFC,
        FolderChange::Rename,
        vec![
            carried(
                &format!("{NFD}/{NFD}.mkv"),
                &format!("{NFC}/{NFC}.mkv"),
                VIDEO,
            ),
            carried(
                &format!("{NFD}/poster.jpg"),
                &format!("{NFC}/poster.jpg"),
                POSTER,
            ),
        ],
    )
}

/// On NTFS (and ext4) the two spellings are two different names: the rename is an ordinary one,
/// and the undo gives back the decomposed spelling exactly.
#[cfg(not(target_os = "macos"))]
#[test]
fn nfd_and_nfc_spellings_are_two_names_here_and_a_rename_between_them_is_undone_exactly() {
    let tree = Tree::new();
    let item = nfd_to_nfc(&tree);
    let mut journal = MemoryJournal::new();
    let report = run_with(
        &StdFs,
        &mut journal,
        ExecOptions::new("b1"),
        std::slice::from_ref(&item),
    );
    assert_eq!(report.done(), 1, "{report:?}");
    assert_eq!(StdFs.list_dir(&tree.root).unwrap(), [NFC]);
    assert_eq!(
        StdFs.list_dir(&tree.root.join(NFC)).unwrap(),
        [format!("{NFC}.mkv"), "poster.jpg".to_owned()]
    );
    assert!(tree.trashed().is_empty());
    let undone = undo_all(&mut journal, std::slice::from_ref(&tree.root), "b1");
    assert!(undone.is_complete(), "{undone:?}");
    assert_eq!(StdFs.list_dir(&tree.root).unwrap(), [NFD]);
    assert_eq!(tree.read(&format!("{NFD}/{NFD}.mkv")), "video bytes");
}

/// Composes a name and folds its case, as a volume that ignores both compares names.
fn folded(text: &str) -> String {
    icu_normalizer::ComposingNormalizerBorrowed::new_nfc()
        .normalize(text)
        .to_lowercase()
}

/// The content of a file (`None` for a folder) and its modification time.
type Entry = (Option<Vec<u8>>, i64);

/// A volume in memory that ignores case and Unicode normalisation when it looks a name up but
/// keeps the spelling it was given, as APFS and HFS+ do on a Mac: `Ame\u{301}lie` and `Amélie`
/// are one entry, listed under the spelling it was last renamed to. A rename onto another file
/// replaces it.
#[derive(Default)]
struct FoldingFs {
    /// Folded path to (spelled path, entry).
    entries: RefCell<BTreeMap<String, (String, Entry)>>,
}

fn spelled(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_owned()
}

fn key(path: &Path) -> String {
    folded(&spelled(path))
}

fn missing() -> io::Error {
    io::Error::from(io::ErrorKind::NotFound)
}

impl FoldingFs {
    fn dir(&self, path: &str) {
        let mut at = String::new();
        for part in path.split('/').filter(|p| !p.is_empty()) {
            at = format!("{at}/{part}");
            self.entries
                .borrow_mut()
                .entry(folded(&at))
                .or_insert((at.clone(), (None, 1)));
        }
    }

    fn file(&self, path: &str, content: &str) {
        if let Some((parent, _)) = path.rsplit_once('/') {
            self.dir(parent);
        }
        self.entries.borrow_mut().insert(
            folded(path),
            (path.to_owned(), (Some(content.as_bytes().to_vec()), 1_000)),
        );
    }

    /// Every file, by its spelled path.
    fn files(&self) -> BTreeMap<String, String> {
        self.entries
            .borrow()
            .values()
            .filter_map(|(name, (content, _))| {
                content
                    .as_ref()
                    .map(|c| (name.clone(), String::from_utf8_lossy(c).into_owned()))
            })
            .collect()
    }

    fn children(&self, dir: &str) -> Vec<String> {
        let prefix = format!("{dir}/");
        self.entries
            .borrow()
            .iter()
            .filter(|(k, _)| k.strip_prefix(&prefix).is_some_and(|r| !r.contains('/')))
            .map(|(_, (name, _))| name.rsplit('/').next().unwrap_or_default().to_owned())
            .collect()
    }
}

impl Fs for FoldingFs {
    fn meta(&self, path: &Path) -> io::Result<Option<Meta>> {
        Ok(self
            .entries
            .borrow()
            .get(&key(path))
            .map(|(_, (content, modified_ms))| Meta {
                is_dir: content.is_none(),
                len: content.as_ref().map_or(0, |c| c.len() as u64),
                modified_ms: *modified_ms,
            }))
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        self.dir(&spelled(path));
        Ok(())
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        let (from_key, to_key) = (key(from), key(to));
        let (from_name, to_name) = (spelled(from), spelled(to));
        let mut entries = self.entries.borrow_mut();
        let Some((_, entry)) = entries.get(&from_key).cloned() else {
            return Err(missing());
        };
        let prefix = format!("{from_key}/");
        let below: Vec<String> = entries
            .keys()
            .filter(|k| k.starts_with(&prefix))
            .cloned()
            .collect();
        if from_key != to_key && entry.0.is_none() && entries.contains_key(&to_key) {
            let target = format!("{to_key}/");
            if entries.keys().any(|k| k.starts_with(&target)) {
                return Err(io::Error::other("directory not empty"));
            }
        }
        for old in below {
            let (name, value) = entries.remove(&old).unwrap();
            let rest = &name[from_name.len()..];
            entries.insert(
                format!("{to_key}{}", &old[from_key.len()..]),
                (format!("{to_name}{rest}"), value),
            );
        }
        entries.remove(&from_key);
        entries.insert(to_key, (to_name, entry));
        Ok(())
    }

    fn copy_file(&self, from: &Path, to: &Path) -> io::Result<()> {
        let (_, entry) = self
            .entries
            .borrow()
            .get(&key(from))
            .cloned()
            .ok_or_else(missing)?;
        self.entries
            .borrow_mut()
            .insert(key(to), (spelled(to), entry));
        Ok(())
    }

    fn same_content(&self, a: &Path, b: &Path) -> io::Result<bool> {
        let entries = self.entries.borrow();
        Ok(entries.get(&key(a)).map(|e| &e.1.0) == entries.get(&key(b)).map(|e| &e.1.0))
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        self.entries
            .borrow_mut()
            .remove(&key(path))
            .map(|_| ())
            .ok_or_else(missing)
    }

    fn remove_dir(&self, path: &Path) -> io::Result<()> {
        let k = key(path);
        if !self.children(&k).is_empty() {
            return Err(io::Error::other("directory not empty"));
        }
        self.entries
            .borrow_mut()
            .remove(&k)
            .map(|_| ())
            .ok_or_else(missing)
    }

    fn list_dir(&self, path: &Path) -> io::Result<Vec<String>> {
        let k = key(path);
        if !self.entries.borrow().contains_key(&k) {
            return Err(missing());
        }
        let mut names = self.children(&k);
        names.sort();
        Ok(names)
    }
}

const MEM: &str = "/mem/ds";

fn mem_item(
    old: &str,
    new: &str,
    change: FolderChange,
    files: Vec<crate::plan::PlannedFile>,
) -> BatchItem {
    BatchItem {
        id: "1".to_owned(),
        root: MEM.to_owned(),
        old_folder: old.to_owned(),
        new_folder: new.to_owned(),
        folder_change: change,
        base_name: None,
        version_label: None,
        shares_folder: change == FolderChange::MoveFiles,
        files,
    }
}

#[test]
fn on_a_volume_that_ignores_normalisation_an_nfd_folder_is_renamed_to_nfc_in_place() {
    let fs = FoldingFs::default();
    fs.file(&format!("{MEM}/{NFD}/{NFD}.mkv"), "video bytes");
    fs.file(&format!("{MEM}/{NFD}/poster.jpg"), "poster bytes");
    let item = mem_item(
        NFD,
        NFC,
        FolderChange::Rename,
        vec![
            carried(
                &format!("{NFD}/{NFD}.mkv"),
                &format!("{NFC}/{NFC}.mkv"),
                VIDEO,
            ),
            carried(
                &format!("{NFD}/poster.jpg"),
                &format!("{NFC}/poster.jpg"),
                POSTER,
            ),
        ],
    );
    let mut journal = MemoryJournal::new();
    let report = run_with(&fs, &mut journal, ExecOptions::new("b1"), &[item]);

    assert_eq!(report.done(), 1, "{report:?}");
    let files = fs.files();
    assert_eq!(
        files,
        BTreeMap::from([
            (format!("{MEM}/{NFC}/{NFC}.mkv"), "video bytes".to_owned()),
            (format!("{MEM}/{NFC}/poster.jpg"), "poster bytes".to_owned()),
        ]),
        "renamed in place, nothing in the trash folder"
    );
    assert_eq!(fs.list_dir(Path::new(MEM)).unwrap(), [NFC]);

    let options = UndoOptions {
        roots: vec![PathBuf::from(MEM)],
        ..UndoOptions::default()
    };
    let undone = undo_batch(&mut journal, &fs, "b1", &options).unwrap();
    assert!(undone.is_complete(), "{undone:?}");
    assert_eq!(
        fs.files(),
        BTreeMap::from([
            (format!("{MEM}/{NFD}/{NFD}.mkv"), "video bytes".to_owned()),
            (format!("{MEM}/{NFD}/poster.jpg"), "poster bytes".to_owned()),
        ])
    );
}

#[test]
fn on_a_volume_that_ignores_normalisation_an_nfd_file_is_renamed_to_nfc_and_back() {
    let fs = FoldingFs::default();
    fs.file(&format!("{MEM}/Shared/{NFD}.mkv"), "video bytes");
    fs.file(&format!("{MEM}/Shared/other.mkv"), "another movie");
    let item = mem_item(
        "Shared",
        "Shared",
        FolderChange::MoveFiles,
        vec![carried(
            &format!("Shared/{NFD}.mkv"),
            &format!("Shared/{NFC}.mkv"),
            VIDEO,
        )],
    );
    let mut journal = MemoryJournal::new();
    let report = run_with(&fs, &mut journal, ExecOptions::new("b1"), &[item]);
    assert_eq!(report.done(), 1, "{report:?}");
    assert_eq!(
        fs.list_dir(&Path::new(MEM).join("Shared")).unwrap(),
        [format!("{NFC}.mkv"), "other.mkv".to_owned()]
    );
    assert_eq!(fs.files().len(), 2, "nothing went to the trash folder");

    let options = UndoOptions {
        roots: vec![PathBuf::from(MEM)],
        ..UndoOptions::default()
    };
    let undone = undo_batch(&mut journal, &fs, "b1", &options).unwrap();
    assert!(undone.is_complete(), "{undone:?}");
    assert_eq!(
        fs.list_dir(&Path::new(MEM).join("Shared")).unwrap(),
        [format!("{NFD}.mkv"), "other.mkv".to_owned()]
    );
}

// ---- read-only files ---------------------------------------------------------------------------

fn set_read_only(path: &Path, on: bool) {
    let mut perms = fs::metadata(path).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(on);
    fs::set_permissions(path, perms).unwrap();
}

fn is_read_only(path: &Path) -> bool {
    fs::metadata(path).unwrap().permissions().readonly()
}

/// Clears the read-only flag of every file below `dir`, so the temporary folder can go.
fn writable(dir: &Path) {
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            writable(&path);
        } else if is_read_only(&path) {
            set_read_only(&path, false);
        }
    }
}

/// A title whose video and NFO are read-only (a copy from optical media, a share mounted
/// read-only file by file), with a read-only leftover to trash.
fn read_only_title(tree: &Tree) -> BatchItem {
    tree.write("Old/heat.mkv", "video bytes");
    tree.write("Old/heat.nfo", super::tests::MANAGED_NFO);
    tree.write("Old/folder.jpg", "stale artwork");
    for rel in ["Old/heat.mkv", "Old/heat.nfo", "Old/folder.jpg"] {
        set_read_only(&tree.root.join(rel), true);
    }
    batch_item(
        tree,
        "1",
        "Old",
        "Heat (1995)",
        FolderChange::Rename,
        vec![
            carried("Old/heat.mkv", "Heat (1995)/Heat (1995).mkv", VIDEO),
            carried("Old/heat.nfo", "Heat (1995)/Heat (1995).nfo", NFO),
            trash_file("Old/folder.jpg", cv_domain::MediaFileType::Thumb),
        ],
    )
}

#[test]
fn read_only_files_are_renamed_trashed_and_undone_and_stay_read_only() {
    let tree = Tree::new();
    let item = read_only_title(&tree);
    let before = tree.live();
    let mut journal = MemoryJournal::new();
    let report = run_with(&StdFs, &mut journal, ExecOptions::new("b1"), &[item]);
    assert_eq!(report.done(), 1, "{report:?}");
    assert!(is_read_only(&tree.root.join("Heat (1995)/Heat (1995).mkv")));
    assert_eq!(tree.trashed().len(), 1);
    let undone = undo_all(&mut journal, std::slice::from_ref(&tree.root), "b1");
    assert!(undone.is_complete(), "{undone:?}");
    assert_eq!(tree.live(), before);
    assert!(is_read_only(&tree.root.join("Old/heat.mkv")));
    writable(&tree.root);
}

#[test]
fn read_only_files_moved_to_another_volume_are_copied_verified_and_stay_read_only() {
    let tree = Tree::new();
    let item = read_only_title(&tree);
    let before = tree.live();
    let fs = FaultFs::across_volumes();
    let mut journal = MemoryJournal::new();
    let report = run_with(&fs, &mut journal, ExecOptions::new("b1"), &[item]);
    assert_eq!(report.done(), 1, "{report:?}");
    assert!(
        !tree.exists("Old/heat.mkv"),
        "the original went after its copy"
    );
    assert_eq!(tree.read("Heat (1995)/Heat (1995).mkv"), "video bytes");
    assert!(is_read_only(&tree.root.join("Heat (1995)/Heat (1995).mkv")));
    let undone = undo_all(&mut journal, std::slice::from_ref(&tree.root), "b1");
    assert!(undone.is_complete(), "{undone:?}");
    assert_eq!(tree.live(), before);
    writable(&tree.root);
}

// ---- files held open by another program --------------------------------------------------------

/// Opens `path` the way a player or a scanner that shares nothing does.
#[cfg(windows)]
fn hold(path: &Path) -> fs::File {
    use std::os::windows::fs::OpenOptionsExt;
    fs::File::options()
        .read(true)
        .share_mode(0)
        .open(path)
        .unwrap()
}

#[cfg(windows)]
#[test]
fn a_title_with_a_file_held_open_is_left_whole_and_renamed_once_it_is_let_go() {
    let tree = Tree::new();
    tree.write("Old/heat.mkv", "video bytes");
    tree.write("Old/heat.nfo", super::tests::MANAGED_NFO);
    let items = [batch_item(
        &tree,
        "1",
        "Old",
        "Heat (1995)",
        FolderChange::Rename,
        vec![
            carried("Old/heat.mkv", "Heat (1995)/Heat (1995).mkv", VIDEO),
            carried("Old/heat.nfo", "Heat (1995)/Heat (1995).nfo", NFO),
        ],
    )];
    let before = tree.live();
    let held = hold(&tree.root.join("Old/heat.mkv"));
    let mut journal = MemoryJournal::new();
    let report = run_with(&StdFs, &mut journal, ExecOptions::new("b1"), &items);
    assert_eq!(report.failed(), 1, "{report:?}");
    drop(held);
    assert_eq!(
        tree.live(),
        before,
        "the title is whole under its old names"
    );
    assert!(
        journal
            .all()
            .iter()
            .all(|r| r.state != JournalState::Planned && r.state != JournalState::Done),
        "{:?}",
        journal.all()
    );
    rename_and_undo(&tree, &StdFs, &items);
}

#[cfg(windows)]
#[test]
fn a_file_held_open_in_a_shared_folder_rolls_its_title_back_and_the_others_go_on() {
    let tree = Tree::new();
    tree.write("Multi/a.mkv", "a video");
    tree.write("Multi/a.srt", "a subtitle");
    tree.write("Multi/b.mkv", "b video");
    let a = batch_item(
        &tree,
        "a",
        "Multi",
        "Multi",
        FolderChange::MoveFiles,
        vec![
            carried("Multi/a.mkv", "Multi/A (2000).mkv", VIDEO),
            carried(
                "Multi/a.srt",
                "Multi/A (2000).srt",
                cv_domain::MediaFileType::Subtitle,
            ),
        ],
    );
    let b = batch_item(
        &tree,
        "b",
        "Multi",
        "Multi",
        FolderChange::MoveFiles,
        vec![carried("Multi/b.mkv", "Multi/B (2001).mkv", VIDEO)],
    );
    let held = hold(&tree.root.join("Multi/a.srt"));
    let mut journal = MemoryJournal::new();
    let report = run_with(&StdFs, &mut journal, ExecOptions::new("b1"), &[a, b]);
    drop(held);
    assert_eq!((report.done(), report.failed()), (1, 1), "{report:?}");
    let live = tree.live();
    assert_eq!(
        live.keys().collect::<Vec<_>>(),
        ["Multi/B (2001).mkv", "Multi/a.mkv", "Multi/a.srt"],
        "the title with the open file is whole under its old names"
    );
}

// ---- network (UNC) paths -------------------------------------------------------------------------

/// The administrative share path (`\\localhost\C$\...`) of a local folder, when this machine
/// serves it; `None` otherwise (the test then has nothing to run against). Also `None` in CI: on a
/// hosted runner the share exists, but a folder renamed through it is often held open by the
/// system's own scanners ("in use by another program"), so the result depends on timing. The
/// share path is checked by hand on a real machine.
#[cfg(windows)]
fn unc_of(path: &Path) -> Option<PathBuf> {
    if std::env::var_os("CI").is_some() {
        return None;
    }
    let text = path.to_string_lossy();
    let bytes = text.as_bytes();
    if bytes.len() < 3 || bytes[1] != b':' {
        return None;
    }
    let unc = PathBuf::from(format!(r"\\localhost\{}$\{}", &text[..1], &text[3..]));
    fs::metadata(&unc).is_ok().then_some(unc)
}

#[cfg(windows)]
#[test]
fn a_title_on_a_network_share_is_renamed_and_undone_through_its_unc_path() {
    let tree = Tree::new();
    let Some(share) = unc_of(&tree.root) else {
        eprintln!("skipped: no administrative share on this machine (manual check instead)");
        return;
    };
    // A long path too: the UNC path gets the `\\?\UNC\` form.
    let deep = "n".repeat(120);
    tree.write(&format!("{deep}/heat/heat.mkv"), "video bytes");
    tree.write(&format!("{deep}/heat/folder.jpg"), "stale artwork");
    let mut item = batch_item(
        &tree,
        "1",
        &format!("{deep}/heat"),
        &format!("{deep}/Heat (1995)"),
        FolderChange::Rename,
        vec![
            carried(
                &format!("{deep}/heat/heat.mkv"),
                &format!("{deep}/Heat (1995)/{}.mkv", "H".repeat(100)),
                VIDEO,
            ),
            trash_file(
                &format!("{deep}/heat/folder.jpg"),
                cv_domain::MediaFileType::Thumb,
            ),
        ],
    );
    item.root = share.to_string_lossy().into_owned();
    let longest = share.join(format!("{deep}/Heat (1995)/{}.mkv", "H".repeat(100)));
    assert!(longest.to_string_lossy().len() > 260);
    let before = tree.live();
    let mut journal = MemoryJournal::new();
    let report = run_with(&StdFs, &mut journal, ExecOptions::new("b1"), &[item]);
    assert_eq!(report.done(), 1, "{report:?}");
    assert!(StdFs.meta(&longest).unwrap().is_some());
    assert_eq!(tree.trashed().len(), 1);
    assert!(
        journal
            .all()
            .iter()
            .all(|r| r.entry.from.starts_with(r"\\localhost\")),
        "the journal keeps the share paths"
    );
    let undone = undo_all(&mut journal, &[share], "b1");
    assert!(undone.is_complete(), "{undone:?}");
    assert_eq!(tree.live(), before);
}

// ---- names that differ only in case, on a case-sensitive file system --------------------------

#[cfg(target_os = "linux")]
#[test]
fn on_linux_two_files_differing_only_in_case_are_two_files_and_neither_is_lost() {
    let tree = Tree::new();
    tree.write("Example (2020)/movie.mkv", "first");
    tree.write("Example (2020)/Movie.mkv", "other");
    let item = batch_item(
        &tree,
        "1",
        "Example (2020)",
        "Example (2020)",
        FolderChange::Unchanged,
        vec![carried(
            "Example (2020)/movie.mkv",
            "Example (2020)/Movie.mkv",
            VIDEO,
        )],
    );
    let mut journal = MemoryJournal::new();
    let report = run_with(&StdFs, &mut journal, ExecOptions::new("b1"), &[item]);
    assert_eq!(report.done(), 1, "{report:?}");
    // Not a case-only rename through a temporary name: the second file was a different file,
    // so it went to the trash before the first took its name.
    assert_eq!(tree.read("Example (2020)/Movie.mkv"), "first");
    assert!(!tree.exists("Example (2020)/movie.mkv"));
    let trashed: Vec<String> = tree.trashed().into_values().collect();
    assert_eq!(trashed, ["other"], "the file in the way must be kept");
    // Undo puts both back where they were, with their bytes.
    let undone = undo_all(&mut journal, std::slice::from_ref(&tree.root), "b1");
    assert!(undone.is_complete(), "{undone:?}");
    assert_eq!(tree.read("Example (2020)/movie.mkv"), "first");
    assert_eq!(tree.read("Example (2020)/Movie.mkv"), "other");
}
