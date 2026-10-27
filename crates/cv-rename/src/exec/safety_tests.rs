//! Findings of the data-safety review of renaming, each pinned
//! by a test that failed before its fix.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use super::tests::{VIDEO, carried};
use super::*;
use crate::conflict::BatchItem;
use crate::plan::FolderChange;

/// The content of a file (`None` for a folder) and its modification time.
type Entry = (Option<Vec<u8>>, i64);

/// A case-sensitive volume in memory (ext4, a case-sensitive APFS volume, a case-sensitive
/// folder on NTFS): `heat.mkv` and `Heat.mkv` are two files. A rename replaces a file at the
/// destination, as POSIX `rename` does.
#[derive(Default)]
pub(super) struct CaseSensitiveFs {
    /// Normalised path to (content, modification time); `None` content is a folder.
    entries: RefCell<BTreeMap<String, Entry>>,
}

fn key(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_owned()
}

impl CaseSensitiveFs {
    pub(super) fn dir(&self, path: &str) {
        let mut at = String::new();
        for part in path.split('/').filter(|p| !p.is_empty()) {
            at = if at.is_empty() && path.starts_with('/') {
                format!("/{part}")
            } else if at.is_empty() {
                part.to_owned()
            } else {
                format!("{at}/{part}")
            };
            self.entries
                .borrow_mut()
                .entry(at.clone())
                .or_insert((None, 1));
        }
    }

    pub(super) fn file(&self, path: &str, content: &str, modified_ms: i64) {
        if let Some((parent, _)) = path.rsplit_once('/') {
            self.dir(parent);
        }
        self.entries.borrow_mut().insert(
            path.to_owned(),
            (Some(content.as_bytes().to_vec()), modified_ms),
        );
    }

    pub(super) fn files(&self) -> BTreeMap<String, String> {
        self.entries
            .borrow()
            .iter()
            .filter_map(|(k, (content, _))| {
                content
                    .as_ref()
                    .map(|c| (k.clone(), String::from_utf8_lossy(c).into_owned()))
            })
            .collect()
    }

    fn children(&self, dir: &str) -> Vec<String> {
        let prefix = format!("{dir}/");
        self.entries
            .borrow()
            .keys()
            .filter_map(|k| k.strip_prefix(&prefix))
            .filter(|rest| !rest.contains('/'))
            .map(str::to_owned)
            .collect()
    }
}

fn missing() -> io::Error {
    io::Error::from(io::ErrorKind::NotFound)
}

impl Fs for CaseSensitiveFs {
    fn meta(&self, path: &Path) -> io::Result<Option<Meta>> {
        Ok(self
            .entries
            .borrow()
            .get(&key(path))
            .map(|(content, modified_ms)| Meta {
                is_dir: content.is_none(),
                len: content.as_ref().map_or(0, |c| c.len() as u64),
                modified_ms: *modified_ms,
            }))
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        self.dir(&key(path));
        Ok(())
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        let (from, to) = (key(from), key(to));
        let mut entries = self.entries.borrow_mut();
        let Some(entry) = entries.get(&from).cloned() else {
            return Err(missing());
        };
        if entry.0.is_none() {
            if let Some((None, _)) = entries.get(&to) {
                let prefix = format!("{to}/");
                if entries.keys().any(|k| k.starts_with(&prefix)) {
                    return Err(io::Error::other("directory not empty"));
                }
            }
            let prefix = format!("{from}/");
            let below: Vec<String> = entries
                .keys()
                .filter(|k| k.starts_with(&prefix))
                .cloned()
                .collect();
            for old in below {
                let value = entries.remove(&old).unwrap();
                entries.insert(format!("{to}/{}", &old[prefix.len()..]), value);
            }
        }
        entries.remove(&from);
        entries.insert(to, entry);
        Ok(())
    }

    fn copy_file(&self, from: &Path, to: &Path) -> io::Result<()> {
        let entry = self
            .entries
            .borrow()
            .get(&key(from))
            .cloned()
            .ok_or_else(missing)?;
        self.entries.borrow_mut().insert(key(to), entry);
        Ok(())
    }

    fn same_content(&self, a: &Path, b: &Path) -> io::Result<bool> {
        let entries = self.entries.borrow();
        Ok(entries.get(&key(a)).map(|e| &e.0) == entries.get(&key(b)).map(|e| &e.0))
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

const ROOT: &str = "/mem/ds";

fn item(files: Vec<crate::plan::PlannedFile>, old: &str, new: &str) -> BatchItem {
    BatchItem {
        id: "1".to_owned(),
        root: ROOT.to_owned(),
        old_folder: old.to_owned(),
        new_folder: new.to_owned(),
        folder_change: FolderChange::Unchanged,
        base_name: None,
        version_label: None,
        shares_folder: true,
        files,
    }
}

// ---- finding: a "case-only" rename onto a second file of a case-sensitive volume ---------------

#[test]
fn on_a_case_sensitive_volume_a_file_named_in_another_case_is_not_overwritten() {
    // Two different files whose names differ in case only, with the same size and time (a
    // copy made by a tool that keeps times). The plan renames `heat.mkv` to `Heat.mkv`.
    let fs = CaseSensitiveFs::default();
    fs.file("/mem/ds/M/heat.mkv", "first", 1_000);
    fs.file("/mem/ds/M/Heat.mkv", "other", 1_000);
    let items = [item(
        vec![carried("M/heat.mkv", "M/Heat.mkv", VIDEO)],
        "M",
        "M",
    )];
    let mut journal = MemoryJournal::new();
    let report = Executor::new(&fs, &mut journal, ExecOptions::new("b1"))
        .unwrap()
        .run(&items)
        .unwrap();
    assert_eq!(report.done(), 1, "{report:?}");

    let files = fs.files();
    let contents: Vec<&String> = files.values().collect();
    assert!(contents.contains(&&"first".to_owned()), "{files:?}");
    assert!(
        contents.contains(&&"other".to_owned()),
        "the second file was overwritten: {files:?}"
    );
    assert_eq!(files["/mem/ds/M/Heat.mkv"], "first");
    // The file that was in the way went to the trash.
    assert!(
        files
            .iter()
            .any(|(k, v)| k.contains(TRASH_DIR) && v == "other"),
        "{files:?}"
    );
}

#[test]
fn on_a_case_insensitive_volume_a_case_only_rename_still_goes_through_a_temporary_name() {
    let tree = super::tests::Tree::new();
    tree.write("M/heat.mkv", "first");
    let mut item = item(vec![carried("M/heat.mkv", "M/Heat.mkv", VIDEO)], "M", "M");
    item.root = tree.root_str();
    let mut journal = MemoryJournal::new();
    let report = Executor::new(&StdFs, &mut journal, ExecOptions::new("b1"))
        .unwrap()
        .run(&[item])
        .unwrap();
    assert_eq!(report.done(), 1, "{report:?}");
    let names = StdFs.list_dir(&tree.root.join("M")).unwrap();
    if cfg!(any(windows, target_os = "macos")) {
        assert_eq!(names, ["Heat.mkv"]);
        assert_eq!(journal.all().len(), 2, "through a temporary name");
        assert!(tree.trashed().is_empty());
    }
}

// ---- finding: a case-only rename whose second half fails --------------------------------------

/// Fails the rename from the temporary name of a case-only rename once.
struct SecondHalfFails {
    inner: StdFs,
    failed: std::cell::Cell<bool>,
}

impl Fs for SecondHalfFails {
    fn meta(&self, path: &Path) -> io::Result<Option<Meta>> {
        self.inner.meta(path)
    }
    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        self.inner.create_dir_all(path)
    }
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        let from_tmp = from
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with(CASE_TMP_PREFIX));
        let to_tmp = to
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with(CASE_TMP_PREFIX));
        if from_tmp && !to_tmp && !self.failed.get() {
            // Not the rename back to the old name, which has the old name as its target.
            if to
                .file_name()
                .is_some_and(|n| n.to_string_lossy() == "Heat.mkv")
            {
                self.failed.set(true);
                return Err(io::Error::from(io::ErrorKind::NotFound));
            }
        }
        self.inner.rename(from, to)
    }
    fn copy_file(&self, from: &Path, to: &Path) -> io::Result<()> {
        self.inner.copy_file(from, to)
    }
    fn same_content(&self, a: &Path, b: &Path) -> io::Result<bool> {
        self.inner.same_content(a, b)
    }
    fn remove_file(&self, path: &Path) -> io::Result<()> {
        self.inner.remove_file(path)
    }
    fn remove_dir(&self, path: &Path) -> io::Result<()> {
        self.inner.remove_dir(path)
    }
    fn list_dir(&self, path: &Path) -> io::Result<Vec<String>> {
        self.inner.list_dir(path)
    }
}

#[test]
fn a_case_only_rename_that_fails_halfway_leaves_the_journal_matching_the_disk() {
    if !cfg!(any(windows, target_os = "macos")) {
        return; // A case-only rename needs a case-insensitive volume.
    }
    let tree = super::tests::Tree::new();
    tree.write("M/heat.mkv", "first");
    let mut item = item(vec![carried("M/heat.mkv", "M/Heat.mkv", VIDEO)], "M", "M");
    item.root = tree.root_str();
    let fs = SecondHalfFails {
        inner: StdFs,
        failed: std::cell::Cell::new(false),
    };
    let mut journal = MemoryJournal::new();
    let report = Executor::new(&fs, &mut journal, ExecOptions::new("b1"))
        .unwrap()
        .run(&[item])
        .unwrap();
    assert_eq!(report.failed(), 1, "{report:?}");
    // The file is back under its old name, nothing was reported as left behind, and no row
    // claims the file is under the temporary name.
    assert_eq!(StdFs.list_dir(&tree.root.join("M")).unwrap(), ["heat.mkv"]);
    assert!(report.items[0].notes.is_empty(), "{report:?}");
    for row in journal.all() {
        assert_ne!(row.state, JournalState::Done, "{row:?}");
    }
}

// ---- finding: a stop between two steps of a title left no trace -------------------------------

use std::panic::{AssertUnwindSafe, catch_unwind};

use super::tests::{
    Crash, FaultFs, Tree, count_mutations, run_with, shared_folder_scenario, whole_folder_scenario,
};

/// Crashes at every mutation of the scenario, before and after it. After the restart's
/// recovery the item must be known as unfinished; then `finish` brings the disk back to one
/// whole state, which `expected` gives.
fn stop_anywhere(
    scenario: fn() -> (Tree, Vec<BatchItem>),
    finish: &dyn Fn(&Tree, &mut MemoryJournal, &str),
    expected: &dyn Fn(&Tree) -> BTreeMap<String, String>,
) {
    let total = count_mutations(scenario, false);
    for at in 0..total {
        for after in [false, true] {
            let (tree, items) = scenario();
            let want = expected(&tree);
            let journal = MemoryJournal::new();
            let fault = FaultFs::crashing(at, after, false);
            let mut running = journal.clone();
            let label = format!(
                "stop {} mutation {at}",
                if after { "after" } else { "before" }
            );
            let crashed = catch_unwind(AssertUnwindSafe(|| {
                run_with(&fault, &mut running, ExecOptions::new("b1"), &items)
            }));
            assert!(
                crashed.expect_err(&label).downcast_ref::<Crash>().is_some(),
                "{label}"
            );

            let mut journal = journal;
            let recovery = recover(&mut journal, &StdFs).unwrap();
            assert_eq!(
                recovery.unfinished_items,
                [("b1".to_owned(), "1".to_owned())],
                "{label}: the stop must be found"
            );
            let marks = journal.all_marks();
            assert_eq!(marks[0].state, ItemState::Unfinished, "{label}");

            finish(&tree, &mut journal, &label);
            assert_eq!(tree.live(), want, "{label}");
            assert!(
                journal
                    .all_marks()
                    .iter()
                    .all(|m| m.state != ItemState::Unfinished && m.state != ItemState::Running),
                "{label}: {:?}",
                journal.all_marks()
            );
        }
    }
}

/// What an uninterrupted run leaves.
fn finished_state(scenario: fn() -> (Tree, Vec<BatchItem>)) -> BTreeMap<String, String> {
    let (tree, items) = scenario();
    run_with(
        &StdFs,
        &mut MemoryJournal::new(),
        ExecOptions::new("clean"),
        &items,
    );
    tree.live()
}

fn resume(tree: &Tree, journal: &mut MemoryJournal, label: &str) {
    let done = resume_unfinished(journal, &StdFs, "b1", std::slice::from_ref(&tree.root)).unwrap();
    assert!(done.skipped.is_empty(), "{label}: {done:?}");
    assert_eq!(done.items.len(), 1, "{label}");
    assert_eq!(done.items[0].status, ItemStatus::Done, "{label}: {done:?}");
}

fn roll_back(tree: &Tree, journal: &mut MemoryJournal, label: &str) {
    let options = UndoOptions {
        roots: vec![tree.root.clone()],
        only_items: Some(vec!["1".to_owned()]),
        ..UndoOptions::default()
    };
    let undone = undo_batch(journal, &StdFs, "b1", &options).unwrap();
    assert!(undone.is_complete(), "{label}: {undone:?}");
    settle_interrupted(journal, "b1").unwrap();
}

#[test]
fn a_stop_anywhere_in_a_folder_rename_is_found_and_resume_finishes_it() {
    let want = finished_state(whole_folder_scenario);
    stop_anywhere(whole_folder_scenario, &resume, &|_| want.clone());
}

#[test]
fn a_stop_anywhere_in_a_file_by_file_rename_is_found_and_resume_finishes_it() {
    let want = finished_state(shared_folder_scenario);
    stop_anywhere(shared_folder_scenario, &resume, &|_| want.clone());
}

#[test]
fn a_stop_anywhere_in_a_rename_is_found_and_a_roll_back_puts_every_file_back() {
    for scenario in [whole_folder_scenario, shared_folder_scenario] {
        stop_anywhere(scenario, &roll_back, &|tree| tree.live());
    }
}

#[test]
fn an_item_that_finished_or_was_rolled_back_whole_is_not_unfinished() {
    let (tree, items) = whole_folder_scenario();
    let mut journal = MemoryJournal::new();
    run_with(&StdFs, &mut journal, ExecOptions::new("b1"), &items);
    assert_eq!(journal.all_marks()[0].state, ItemState::Done);
    let plan = ItemPlan::parse(&journal.all_marks()[0].plan).expect("the plan is kept");
    assert_eq!(plan.item, items[0]);

    // A step that fails on the disk: the item is rolled back whole.
    let (_tree2, items) = whole_folder_scenario();
    let fs = FaultFs::new();
    fs.busy(usize::MAX);
    let mut journal = MemoryJournal::new();
    run_with(&fs, &mut journal, ExecOptions::new("b2"), &items);
    assert_eq!(journal.all_marks()[0].state, ItemState::RolledBack);
    assert!(
        recover(&mut journal, &StdFs)
            .unwrap()
            .unfinished_items
            .is_empty()
    );
    drop(tree);
}
