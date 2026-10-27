//! Tests of the executor: file operations in temp folders, ordering, trash, clean-up, the
//! database journal and, above all, a crash at every point of a batch.

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use cv_domain::{Item, MediaFile, MediaFileType};
use tempfile::TempDir;

use super::run::{order_items, order_ops};
use super::*;
use crate::conflict::BatchItem;
use crate::plan::{Fate, FolderChange, Op, OpKind, Owner, PlannedFile};

// ---- helpers -------------------------------------------------------------------------------

/// A data source in a temp folder.
pub(super) struct Tree {
    _dir: TempDir,
    pub(super) root: PathBuf,
}

impl Tree {
    pub(super) fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("ds");
        fs::create_dir_all(&root).unwrap();
        Tree { _dir: dir, root }
    }

    pub(super) fn write(&self, rel: &str, content: &str) {
        let path = self.root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    pub(super) fn exists(&self, rel: &str) -> bool {
        self.root.join(rel).exists()
    }

    pub(super) fn read(&self, rel: &str) -> String {
        fs::read_to_string(self.root.join(rel)).unwrap()
    }

    pub(super) fn root_str(&self) -> String {
        self.root.to_string_lossy().into_owned()
    }

    /// Every file below the root (the trash included): relative path to content.
    pub(super) fn files(&self) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        walk(&self.root, &self.root, &mut out);
        out
    }

    /// The files outside the trash.
    pub(super) fn live(&self) -> BTreeMap<String, String> {
        self.files()
            .into_iter()
            .filter(|(p, _)| !p.starts_with(TRASH_DIR))
            .collect()
    }

    pub(super) fn trashed(&self) -> BTreeMap<String, String> {
        self.files()
            .into_iter()
            .filter(|(p, _)| p.starts_with(TRASH_DIR))
            .collect()
    }

    pub(super) fn dirs(&self) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        walk_dirs(&self.root, &self.root, &mut out);
        out
    }
}

fn rel_of(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap()
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, String>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            walk(root, &path, out);
        } else {
            out.insert(rel_of(root, &path), fs::read_to_string(&path).unwrap());
        }
    }
}

fn walk_dirs(root: &Path, dir: &Path, out: &mut BTreeSet<String>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            out.insert(rel_of(root, &path));
            walk_dirs(root, &path, out);
        }
    }
}

pub(super) fn carried(from: &str, to: &str, kind: MediaFileType) -> PlannedFile {
    PlannedFile {
        from: from.to_owned(),
        kind,
        owner: Owner::Movie,
        fate: Fate::Carried {
            to: vec![to.to_owned()],
        },
    }
}

pub(super) fn trash_file(from: &str, kind: MediaFileType) -> PlannedFile {
    PlannedFile {
        from: from.to_owned(),
        kind,
        owner: Owner::Movie,
        fate: Fate::Trash,
    }
}

pub(super) fn batch_item(
    tree: &Tree,
    id: &str,
    old: &str,
    new: &str,
    change: FolderChange,
    files: Vec<PlannedFile>,
) -> BatchItem {
    BatchItem {
        id: id.to_owned(),
        root: tree.root_str(),
        old_folder: old.to_owned(),
        new_folder: new.to_owned(),
        folder_change: change,
        base_name: None,
        version_label: None,
        shares_folder: false,
        files,
    }
}

pub(super) fn run_with(
    fs: &dyn Fs,
    journal: &mut dyn JournalStore,
    options: ExecOptions,
    items: &[BatchItem],
) -> BatchReport {
    Executor::new(fs, journal, options)
        .unwrap()
        .run(items)
        .unwrap()
}

pub(super) fn run(tree: &Tree, journal: &mut MemoryJournal, items: &[BatchItem]) -> BatchReport {
    let _ = tree;
    run_with(&StdFs, journal, ExecOptions::new("b1"), items)
}

pub(super) const VIDEO: MediaFileType = MediaFileType::Video;
pub(super) const NFO: MediaFileType = MediaFileType::Nfo;
pub(super) const POSTER: MediaFileType = MediaFileType::Poster;

pub(super) const MANAGED_NFO: &str = "<movie><title>Heat</title><year>1995</year></movie>";
const FOREIGN_NFO: &str = "Release.Group.Presents\n(c) some release note";

// ---- scenarios -----------------------------------------------------------------------------

/// A movie folder renamed as a whole, with a stale artwork name, an extras folder and a note.
pub(super) fn whole_folder_scenario() -> (Tree, Vec<BatchItem>) {
    let tree = Tree::new();
    tree.write("Old/Heat/heat.mkv", "video bytes");
    tree.write("Old/Heat/heat.nfo", MANAGED_NFO);
    tree.write("Old/Heat/poster.jpg", "poster bytes");
    tree.write("Old/Heat/folder.jpg", "stale artwork");
    tree.write("Old/Heat/extras/behind.mkv", "extra bytes");
    tree.write("Old/Heat/notes.txt", "a note");
    let files = vec![
        carried("Old/Heat/heat.mkv", "Heat (1995)/Heat (1995).mkv", VIDEO),
        carried("Old/Heat/heat.nfo", "Heat (1995)/Heat (1995).nfo", NFO),
        carried(
            "Old/Heat/poster.jpg",
            "Heat (1995)/Heat (1995)-poster.jpg",
            POSTER,
        ),
        trash_file("Old/Heat/folder.jpg", MediaFileType::Thumb),
        carried(
            "Old/Heat/extras/behind.mkv",
            "Heat (1995)/extras/behind.mkv",
            MediaFileType::VideoExtra,
        ),
    ];
    let item = batch_item(
        &tree,
        "1",
        "Old/Heat",
        "Heat (1995)",
        FolderChange::Rename,
        files,
    );
    (tree, vec![item])
}

/// Files moved one by one out of a shared folder: a replaced destination, a move into a new
/// folder, a stale file and another movie that stays.
pub(super) fn shared_folder_scenario() -> (Tree, Vec<BatchItem>) {
    let tree = Tree::new();
    tree.write("Multi/a.mkv", "a video");
    tree.write("Multi/a.srt", "a subtitle");
    tree.write("Multi/p.jpg", "new poster");
    tree.write("Multi/A (2000)-poster.jpg", "old poster in the way");
    tree.write("Multi/a-old.jpg", "stale");
    tree.write("Multi/b.mkv", "another movie");
    let files = vec![
        carried("Multi/a.mkv", "Multi/A (2000).mkv", VIDEO),
        carried(
            "Multi/a.srt",
            "New/A (2000).en.srt",
            MediaFileType::Subtitle,
        ),
        carried("Multi/p.jpg", "Multi/A (2000)-poster.jpg", POSTER),
        trash_file("Multi/a-old.jpg", MediaFileType::Thumb),
    ];
    let item = batch_item(&tree, "1", "Multi", "Multi", FolderChange::MoveFiles, files);
    (tree, vec![item])
}

/// A folder and a file that change only in case.
pub(super) fn case_scenario() -> (Tree, Vec<BatchItem>) {
    let tree = Tree::new();
    tree.write("heat (1995)/heat (1995).mkv", "video bytes");
    tree.write("heat (1995)/poster.jpg", "poster bytes");
    let files = vec![
        carried(
            "heat (1995)/heat (1995).mkv",
            "Heat (1995)/Heat (1995).mkv",
            VIDEO,
        ),
        carried("heat (1995)/poster.jpg", "Heat (1995)/poster.jpg", POSTER),
    ];
    let item = batch_item(
        &tree,
        "1",
        "heat (1995)",
        "Heat (1995)",
        FolderChange::Rename,
        files,
    );
    (tree, vec![item])
}

// ---- fault injection -----------------------------------------------------------------------

/// The shared fault-injection file system; a stop of it is the simulated crash.
pub(super) use super::fault::{FaultFs, Stop as Crash};

/// Runs the batch and returns the number of mutations it made.
pub(super) fn count_mutations(
    scenario: fn() -> (Tree, Vec<BatchItem>),
    cross_volume: bool,
) -> usize {
    let (_tree, items) = scenario();
    let fs = FaultFs::new();
    fs.cross_volume(cross_volume);
    let mut journal = MemoryJournal::new();
    let report = run_with(&fs, &mut journal, ExecOptions::new("count"), &items);
    assert_eq!(report.failed(), 0, "{report:?}");
    fs.ops()
}

/// Crashes at every mutation, before and after it; recovers; checks the state; runs the batch
/// again and checks that it finishes to the same result as an uninterrupted run.
fn crash_everywhere(scenario: fn() -> (Tree, Vec<BatchItem>), cross_volume: bool) {
    let total = count_mutations(scenario, cross_volume);
    assert!(total > 3, "the scenario should do some work");

    // What an uninterrupted run leaves.
    let (clean_tree, items) = scenario();
    let clean_fs = FaultFs::new();
    clean_fs.cross_volume(cross_volume);
    run_with(
        &clean_fs,
        &mut MemoryJournal::new(),
        ExecOptions::new("clean"),
        &items,
    );
    let expected_live = clean_tree.live();

    for at in 0..total {
        for after in [false, true] {
            let (tree, items) = scenario();
            let before = tree.files();
            let journal = MemoryJournal::new();

            let fault = FaultFs::crashing(at, after, cross_volume);
            let mut running = journal.clone();
            let crashed = catch_unwind(AssertUnwindSafe(|| {
                run_with(&fault, &mut running, ExecOptions::new("crash"), &items)
            }));
            let label = format!(
                "crash {} mutation {at}",
                if after { "after" } else { "before" }
            );
            let payload = crashed.expect_err("the run should have crashed");
            assert!(payload.downcast_ref::<Crash>().is_some(), "{label}");

            // Restart: recover with a healthy file system.
            let mut recovering = journal.clone();
            let recovery = recover(&mut recovering, &StdFs).unwrap();
            assert_eq!(
                recovery.needs_attention().count(),
                0,
                "{label}: {recovery:?}"
            );
            assert!(journal.pending().unwrap().is_empty(), "{label}");

            // Consistent: nothing lost, no half-finished temporary file, journal agrees.
            let files = tree.files();
            let contents: BTreeSet<&String> = files.values().collect();
            for (path, content) in &before {
                assert!(contents.contains(content), "{label}: {path} was lost");
            }
            for path in files.keys() {
                assert!(
                    !path.ends_with(".cvpart") && !path.contains(".cvcase-"),
                    "{label}: leftover {path}"
                );
            }
            for row in journal.all() {
                let (from, to) = (Path::new(&row.entry.from), Path::new(&row.entry.to));
                let temporary = |p: &Path| p.to_string_lossy().contains(".cvcase-");
                match row.state {
                    JournalState::Done => {
                        assert!(to.exists() || temporary(to), "{label}: {row:?}");
                    }
                    JournalState::Failed => {
                        assert!(from.exists() || temporary(from), "{label}: {row:?}");
                    }
                    other => panic!("{label}: unexpected state {other:?}"),
                }
            }

            // The batch can be run again and finishes the job.
            let resume_fs = FaultFs::new();
            resume_fs.cross_volume(cross_volume);
            let report = run_with(
                &resume_fs,
                &mut journal.clone(),
                ExecOptions::new("resume"),
                &items,
            );
            assert_eq!(report.failed(), 0, "{label}: {report:?}");
            assert_eq!(tree.live(), expected_live, "{label}");
            let after_files = tree.files();
            let contents: BTreeSet<&String> = after_files.values().collect();
            for (path, content) in &before {
                assert!(contents.contains(content), "{label}: {path} lost on resume");
            }
        }
    }
}

// ---- the crash-in-the-middle verification --------------------------------------

#[test]
fn a_crash_at_every_point_of_a_folder_rename_leaves_a_consistent_state() {
    crash_everywhere(whole_folder_scenario, false);
}

#[test]
fn a_crash_at_every_point_of_a_file_by_file_batch_leaves_a_consistent_state() {
    crash_everywhere(shared_folder_scenario, false);
}

#[test]
fn a_crash_at_every_point_of_a_cross_volume_batch_leaves_a_consistent_state() {
    crash_everywhere(whole_folder_scenario, true);
    crash_everywhere(shared_folder_scenario, true);
}

#[test]
fn a_crash_in_a_case_only_rename_never_leaves_the_temporary_name() {
    crash_everywhere(case_scenario, false);
}

#[test]
fn recovery_removes_the_original_of_a_verified_cross_volume_copy() {
    // Died after the copy was verified and renamed into place, before the original was removed.
    let tree = Tree::new();
    tree.write("A/x.mkv", "bytes");
    tree.write("B/x.mkv", "bytes");
    let mut journal = MemoryJournal::new();
    let entry = JournalEntry {
        batch_id: "b".into(),
        seq: 0,
        item_id: None,
        op: JournalOp::Move,
        from: tree.root.join("A/x.mkv").to_string_lossy().into_owned(),
        to: tree.root.join("B/x.mkv").to_string_lossy().into_owned(),
        is_dir: false,
        created_parents: Some(0),
    };
    journal.begin(&entry).unwrap();
    let report = recover(&mut journal, &StdFs).unwrap();
    assert_eq!(report.rows[0].outcome, Recovery::Finished);
    assert!(!tree.exists("A/x.mkv"));
    assert_eq!(tree.read("B/x.mkv"), "bytes");
    assert_eq!(journal.all()[0].state, JournalState::Done);
}

#[test]
fn recovery_leaves_a_row_on_a_missing_drive_planned_until_the_drive_is_back() {
    let tree = Tree::new();
    tree.write("Away/x.mkv", "bytes");
    let mut journal = MemoryJournal::new();
    let entry = JournalEntry {
        batch_id: "b".into(),
        seq: 0,
        item_id: None,
        op: JournalOp::Move,
        from: tree.root.join("Away/x.mkv").to_string_lossy().into_owned(),
        to: tree.root.join("Away/y.mkv").to_string_lossy().into_owned(),
        is_dir: false,
        created_parents: Some(0),
    };
    journal.begin(&entry).unwrap();
    let away = tree.root.join("Away");
    let unplugged = |path: &Path| !path.starts_with(&away);

    let report = recover_with(&mut journal, &StdFs, &unplugged).unwrap();
    assert!(report.rows.is_empty(), "nothing was guessed");
    assert_eq!(report.deferred.len(), 1);
    assert_eq!(journal.all()[0].state, JournalState::Planned);

    // The drive is back: the row is settled from what the disk shows.
    let report = recover_with(&mut journal, &StdFs, &|_| true).unwrap();
    assert!(report.deferred.is_empty());
    assert_eq!(report.rows[0].outcome, Recovery::NotStarted);
    assert_eq!(journal.all()[0].state, JournalState::Failed);
}

#[test]
fn recovery_leaves_two_different_files_alone_and_reports_them() {
    let tree = Tree::new();
    tree.write("A/x.mkv", "one");
    tree.write("B/x.mkv", "two");
    let mut journal = MemoryJournal::new();
    let entry = JournalEntry {
        batch_id: "b".into(),
        seq: 0,
        item_id: None,
        op: JournalOp::Move,
        from: tree.root.join("A/x.mkv").to_string_lossy().into_owned(),
        to: tree.root.join("B/x.mkv").to_string_lossy().into_owned(),
        is_dir: false,
        created_parents: Some(0),
    };
    journal.begin(&entry).unwrap();
    let report = recover(&mut journal, &StdFs).unwrap();
    assert!(matches!(
        report.rows[0].outcome,
        Recovery::NeedsAttention(_)
    ));
    assert_eq!(tree.read("A/x.mkv"), "one");
    assert_eq!(tree.read("B/x.mkv"), "two");
}

#[test]
fn recovery_removes_a_half_written_copy() {
    let tree = Tree::new();
    tree.write("A/x.mkv", "whole");
    tree.write("B/x.mkv.cvpart", "wh");
    let mut journal = MemoryJournal::new();
    let entry = JournalEntry {
        batch_id: "b".into(),
        seq: 0,
        item_id: None,
        op: JournalOp::Move,
        from: tree.root.join("A/x.mkv").to_string_lossy().into_owned(),
        to: tree.root.join("B/x.mkv").to_string_lossy().into_owned(),
        is_dir: false,
        created_parents: Some(0),
    };
    journal.begin(&entry).unwrap();
    let report = recover(&mut journal, &StdFs).unwrap();
    assert_eq!(report.rows[0].outcome, Recovery::NotStarted);
    assert_eq!(tree.files().keys().collect::<Vec<_>>(), ["A/x.mkv"]);
}

// ---- transient errors (safety review 14.2) -------------------------------------------------

#[test]
fn a_rename_that_meets_a_handle_held_for_a_moment_is_tried_again() {
    let (tree, items) = whole_folder_scenario();
    let fs = FaultFs::new();
    fs.busy(3);
    let mut journal = MemoryJournal::new();
    let report = run_with(&fs, &mut journal, ExecOptions::new("b1"), &items);
    assert_eq!(report.done(), 1, "{report:?}");
    assert!(tree.exists("Heat (1995)/Heat (1995).mkv"));
    assert!(
        journal.all().iter().all(|r| r.state == JournalState::Done),
        "{:?}",
        journal.all()
    );
}

#[test]
fn a_handle_that_is_never_let_go_fails_the_item_and_leaves_it_whole() {
    let (tree, items) = whole_folder_scenario();
    let before = tree.files();
    let fs = FaultFs::new();
    fs.busy(usize::MAX);
    let mut journal = MemoryJournal::new();
    let report = run_with(&fs, &mut journal, ExecOptions::new("b1"), &items);
    assert_eq!(report.failed(), 1, "{report:?}");
    assert_eq!(tree.files(), before);
}

/// The reason an item failed with.
fn failure_of(report: &BatchReport) -> String {
    match &report.items[0].status {
        ItemStatus::Failed(why) => why.clone(),
        other => panic!("the item did not fail: {other:?}"),
    }
}

#[test]
fn a_rename_waits_for_cinevaults_own_read_inside_the_folder_to_end() {
    let (tree, items) = whole_folder_scenario();
    // MediaInfo is reading the video when the rename starts, and finishes a moment later.
    let read = cv_fs::reading(&tree.root.join("Old/Heat/heat.mkv"));
    let release = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        drop(read);
    });
    let mut journal = MemoryJournal::new();
    let report = run_with(&StdFs, &mut journal, ExecOptions::new("b1"), &items);
    release.join().unwrap();
    assert_eq!(report.done(), 1, "{report:?}");
    assert!(tree.exists("Heat (1995)/Heat (1995).mkv"));
}

#[test]
fn a_read_of_cinevaults_own_that_does_not_end_fails_the_item_with_a_plain_reason() {
    let (tree, items) = whole_folder_scenario();
    let before = tree.files();
    let _read = cv_fs::reading(&tree.root.join("Old/Heat/heat.mkv"));
    let mut journal = MemoryJournal::new();
    let mut options = ExecOptions::new("b1");
    options.own_read_wait = std::time::Duration::from_millis(50);
    let report = run_with(&StdFs, &mut journal, options, &items);
    assert_eq!(report.failed(), 1, "{report:?}");
    let why = failure_of(&report);
    assert!(
        why.contains("CineVault is still reading heat.mkv") && why.contains("Try again"),
        "{why}"
    );
    assert_eq!(tree.files(), before, "nothing moved");
}

/// A Blu-ray folder whose stream file another program has open (a player reading it over the
/// network: every kind of sharing allowed, which does not help a folder).
#[cfg(windows)]
fn held_stream(path: &Path) -> fs::File {
    use std::os::windows::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .read(true)
        .share_mode(0x7)
        .open(path)
        .unwrap()
}

#[cfg(windows)]
fn blu_ray_scenario() -> (Tree, Vec<BatchItem>) {
    let tree = Tree::new();
    let old = "Movies/Bridge of Spies (2015) [1080p] [Blu-ray] [AC3 2.0]";
    let new = "Movies/Bridge of Spies (2015) [1080p] [Blu-ray] [DTS-HD MA 7.1]";
    tree.write(&format!("{old}/BDMV/index.bdmv"), "index");
    tree.write(&format!("{old}/BDMV/PLAYLIST/00800.mpls"), "playlist");
    tree.write(&format!("{old}/BDMV/STREAM/00800.m2ts"), "stream bytes");
    let item = batch_item(&tree, "1", old, new, FolderChange::Rename, Vec::new());
    (tree, vec![item])
}

#[cfg(windows)]
#[test]
fn a_disc_folder_held_by_a_player_fails_with_what_to_do_and_stays_whole() {
    let (tree, items) = blu_ray_scenario();
    let before = tree.files();
    let _held =
        held_stream(&tree.root.join(
            "Movies/Bridge of Spies (2015) [1080p] [Blu-ray] [AC3 2.0]/BDMV/STREAM/00800.m2ts",
        ));
    let mut journal = MemoryJournal::new();
    let report = run_with(&StdFs, &mut journal, ExecOptions::new("b1"), &items);
    assert_eq!(report.failed(), 1, "{report:?}");
    let why = failure_of(&report);
    assert!(
        why.ends_with(
            "the folder is in use by another program (for example a media player reading it \
             over the network, or File Explorer). Close it and try again. If nothing has it \
             open, check that you may change this folder."
        ),
        "{why}"
    );
    assert_eq!(tree.files(), before);
}

#[cfg(windows)]
#[test]
fn a_disc_folder_a_player_lets_go_of_within_seconds_is_renamed() {
    let (tree, items) = blu_ray_scenario();
    let held =
        held_stream(&tree.root.join(
            "Movies/Bridge of Spies (2015) [1080p] [Blu-ray] [AC3 2.0]/BDMV/STREAM/00800.m2ts",
        ));
    let release = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(2_000));
        drop(held);
    });
    let mut journal = MemoryJournal::new();
    let report = run_with(&StdFs, &mut journal, ExecOptions::new("b1"), &items);
    release.join().unwrap();
    assert_eq!(report.done(), 1, "{report:?}");
    assert!(tree.exists(
        "Movies/Bridge of Spies (2015) [1080p] [Blu-ray] [DTS-HD MA 7.1]/BDMV/STREAM/00800.m2ts"
    ));
}

#[test]
fn undo_and_recovery_try_again_while_a_handle_is_held() {
    let (tree, items) = whole_folder_scenario();
    let before = tree.live();
    let mut journal = MemoryJournal::new();
    let report = run(&tree, &mut journal, &items);
    assert_eq!(report.done(), 1, "{report:?}");
    let fs = FaultFs::new();
    fs.busy(3);
    let options = UndoOptions {
        roots: vec![tree.root.clone()],
        ..UndoOptions::default()
    };
    let undone = undo_batch(&mut journal, &fs, "b1", &options).unwrap();
    assert!(undone.is_complete(), "{undone:?}");
    assert_eq!(tree.live(), before);

    // A crash left a verified cross-volume copy with its original; the removal meets a handle.
    let tree = Tree::new();
    tree.write("A/x.mkv", "bytes");
    tree.write("B/x.mkv", "bytes");
    let mut journal = MemoryJournal::new();
    journal
        .begin(&JournalEntry {
            batch_id: "b".into(),
            seq: 0,
            item_id: None,
            op: JournalOp::Move,
            from: tree.root.join("A/x.mkv").to_string_lossy().into_owned(),
            to: tree.root.join("B/x.mkv").to_string_lossy().into_owned(),
            is_dir: false,
            created_parents: Some(0),
        })
        .unwrap();
    let fs = FaultFs::new();
    fs.busy(2);
    let report = recover(&mut journal, &fs).unwrap();
    assert_eq!(report.rows[0].outcome, Recovery::Finished);
    assert!(!tree.exists("A/x.mkv"));
}

// ---- the happy path ------------------------------------------------------------------------

#[test]
fn a_folder_is_renamed_once_and_its_files_follow() {
    let (tree, items) = whole_folder_scenario();
    let mut journal = MemoryJournal::new();
    let report = run(&tree, &mut journal, &items);
    assert_eq!(report.done(), 1, "{report:?}");

    let live = tree.live();
    assert_eq!(
        live.keys().map(String::as_str).collect::<Vec<_>>(),
        [
            "Heat (1995)/Heat (1995)-poster.jpg",
            "Heat (1995)/Heat (1995).mkv",
            "Heat (1995)/Heat (1995).nfo",
            "Heat (1995)/extras/behind.mkv",
            "Heat (1995)/notes.txt",
        ]
    );
    assert_eq!(live["Heat (1995)/Heat (1995).mkv"], "video bytes");
    // The old parent folder is empty and goes; the data source root stays.
    assert!(!tree.exists("Old"));
    assert!(tree.root.exists());
}

#[test]
fn every_step_is_journaled_planned_first_and_done_after() {
    let (tree, items) = whole_folder_scenario();
    let mut journal = MemoryJournal::new();
    run(&tree, &mut journal, &items);
    let rows = journal.all();
    // Folder rename, three file moves, one trash. The extras file moved with the folder.
    let ops: Vec<(JournalOp, bool)> = rows.iter().map(|r| (r.entry.op, r.entry.is_dir)).collect();
    assert_eq!(
        ops,
        [
            (JournalOp::Move, true),
            (JournalOp::Move, false),
            (JournalOp::Move, false),
            (JournalOp::Move, false),
            (JournalOp::Trash, false),
        ]
    );
    assert!(rows.iter().all(|r| r.state == JournalState::Done));
    let seqs: Vec<u32> = rows.iter().map(|r| r.entry.seq).collect();
    assert_eq!(seqs, [0, 1, 2, 3, 4]);
    // The size and time undo checks are recorded.
    let video = rows
        .iter()
        .find(|r| r.entry.to.ends_with("Heat (1995).mkv"))
        .unwrap();
    assert_eq!(video.size_bytes, Some("video bytes".len() as u64));
    assert!(video.modified_ms.is_some());
}

#[test]
fn a_row_is_on_the_journal_before_the_disk_changes() {
    // A file system that looks at the journal each time it is asked to change something.
    struct Watching<'a> {
        inner: StdFs,
        journal: &'a MemoryJournal,
        seen: Cell<usize>,
    }
    impl Fs for Watching<'_> {
        fn meta(&self, p: &Path) -> io::Result<Option<Meta>> {
            self.inner.meta(p)
        }
        fn create_dir_all(&self, p: &Path) -> io::Result<()> {
            self.inner.create_dir_all(p)
        }
        fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
            let pending = self.journal.pending().unwrap();
            let from = from.to_string_lossy();
            assert!(
                pending.iter().any(|r| r.entry.from == from),
                "no planned row for {from}"
            );
            self.seen.set(self.seen.get() + 1);
            self.inner.rename(Path::new(&*from), to)
        }
        fn copy_file(&self, a: &Path, b: &Path) -> io::Result<()> {
            self.inner.copy_file(a, b)
        }
        fn same_content(&self, a: &Path, b: &Path) -> io::Result<bool> {
            self.inner.same_content(a, b)
        }
        fn remove_file(&self, p: &Path) -> io::Result<()> {
            self.inner.remove_file(p)
        }
        fn remove_dir(&self, p: &Path) -> io::Result<()> {
            self.inner.remove_dir(p)
        }
        fn list_dir(&self, p: &Path) -> io::Result<Vec<String>> {
            self.inner.list_dir(p)
        }
    }

    let (_tree, items) = shared_folder_scenario();
    let journal = MemoryJournal::new();
    let watching = Watching {
        inner: StdFs,
        journal: &journal,
        seen: Cell::new(0),
    };
    let mut writer = journal.clone();
    run_with(&watching, &mut writer, ExecOptions::new("w"), &items);
    assert!(watching.seen.get() >= 4);
}

#[test]
fn files_move_one_by_one_out_of_a_shared_folder_and_the_replaced_file_is_trashed() {
    let (tree, items) = shared_folder_scenario();
    let mut journal = MemoryJournal::new();
    let report = run(&tree, &mut journal, &items);
    assert_eq!(report.done(), 1, "{report:?}");

    assert_eq!(tree.read("Multi/A (2000).mkv"), "a video");
    assert_eq!(tree.read("New/A (2000).en.srt"), "a subtitle");
    assert_eq!(tree.read("Multi/A (2000)-poster.jpg"), "new poster");
    assert_eq!(tree.read("Multi/b.mkv"), "another movie");

    let trashed = tree.trashed();
    let mut kept: Vec<&str> = trashed.values().map(String::as_str).collect();
    kept.sort_unstable();
    assert_eq!(kept, ["old poster in the way", "stale"]);
    assert!(
        trashed
            .keys()
            .all(|p| p.starts_with(".deletedByCineVault/b1/Multi/")),
        "{trashed:?}"
    );
    assert_eq!(report.items[0].trashed, 2);
    assert_eq!(report.items[0].moved, 3);
}

#[test]
fn a_name_in_the_trash_is_never_overwritten() {
    let tree = Tree::new();
    tree.write("M/a.jpg", "first");
    tree.write("M/b.jpg", "second");
    let item = |from: &str| {
        batch_item(
            &tree,
            from,
            "M",
            "M",
            FolderChange::MoveFiles,
            vec![trash_file(from, MediaFileType::Thumb)],
        )
    };
    let items = [item("M/a.jpg"), item("M/b.jpg")];
    // Two batches trash the same relative path: the second takes a numbered name.
    tree.write("M/a.jpg", "first");
    let mut journal = MemoryJournal::new();
    run(&tree, &mut journal, &items[..1]);
    tree.write("M/a.jpg", "first again");
    run(&tree, &mut journal, &items[..1]);
    let trashed = tree.trashed();
    assert_eq!(trashed.len(), 2);
    assert!(trashed.contains_key(".deletedByCineVault/b1/M/a.jpg"));
    assert!(trashed.contains_key(".deletedByCineVault/b1/M/a (2).jpg"));
}

// ---- cross volume --------------------------------------------------------------------------

#[test]
fn across_volumes_files_are_copied_verified_and_then_removed() {
    let (tree, items) = shared_folder_scenario();
    let fs = FaultFs::across_volumes();
    let mut journal = MemoryJournal::new();
    let report = run_with(&fs, &mut journal, ExecOptions::new("x"), &items);
    assert_eq!(report.done(), 1, "{report:?}");
    assert_eq!(tree.read("New/A (2000).en.srt"), "a subtitle");
    assert!(!tree.exists("Multi/a.srt"));
    assert!(tree.files().keys().all(|p| !p.ends_with(".cvpart")));
}

#[test]
fn a_folder_on_another_volume_is_moved_file_by_file() {
    let (tree, items) = whole_folder_scenario();
    let fs = FaultFs::across_volumes();
    let mut journal = MemoryJournal::new();
    let report = run_with(&fs, &mut journal, ExecOptions::new("x"), &items);
    assert_eq!(report.done(), 1, "{report:?}");
    assert_eq!(tree.read("Heat (1995)/Heat (1995).mkv"), "video bytes");
    assert_eq!(tree.read("Heat (1995)/extras/behind.mkv"), "extra bytes");
    // The old folder holds only the note now; it stays because it is not empty.
    assert_eq!(tree.read("Old/Heat/notes.txt"), "a note");
    // The attempt at the folder is on the journal as failed.
    assert!(
        journal
            .all()
            .iter()
            .any(|r| r.entry.is_dir && r.state == JournalState::Failed)
    );
}

#[test]
fn a_copy_that_does_not_match_keeps_the_original_and_rolls_the_item_back() {
    let (tree, items) = shared_folder_scenario();
    let fs = FaultFs::across_volumes();
    fs.corrupt_copies(true);
    let before = tree.files();
    let mut journal = MemoryJournal::new();
    let report = run_with(&fs, &mut journal, ExecOptions::new("x"), &items);
    assert!(
        matches!(report.items[0].status, ItemStatus::Failed(_)),
        "{report:?}"
    );
    // Nothing is lost: the live files are where they were; no part file remains.
    assert_eq!(tree.live(), before);
    assert!(tree.trashed().is_empty());
}

// ---- case ----------------------------------------------------------------------------------

#[test]
fn a_case_only_rename_goes_through_a_temporary_name() {
    let (tree, items) = case_scenario();
    let mut journal = MemoryJournal::new();
    let report = run(&tree, &mut journal, &items);
    assert_eq!(report.done(), 1, "{report:?}");
    // The directory listing shows the new case.
    let names = StdFs.list_dir(&tree.root).unwrap();
    assert_eq!(names, ["Heat (1995)"]);
    let inner = StdFs.list_dir(&tree.root.join("Heat (1995)")).unwrap();
    assert_eq!(inner, ["Heat (1995).mkv", "poster.jpg"]);
    // The folder took two journal rows (old to temporary, temporary to new).
    let dir_rows: Vec<_> = journal
        .all()
        .into_iter()
        .filter(|r| r.entry.is_dir)
        .collect();
    assert_eq!(dir_rows.len(), 2);
    assert!(dir_rows[0].entry.to.contains(".cvcase-"));
    assert!(dir_rows[1].entry.from.contains(".cvcase-"));
}

#[test]
fn a_file_with_only_a_case_change_in_a_shared_folder_is_renamed() {
    let tree = Tree::new();
    tree.write("M/heat.mkv", "bytes");
    let item = batch_item(
        &tree,
        "1",
        "M",
        "M",
        FolderChange::MoveFiles,
        vec![carried("M/heat.mkv", "M/Heat.mkv", VIDEO)],
    );
    let mut journal = MemoryJournal::new();
    run(&tree, &mut journal, &[item]);
    assert_eq!(StdFs.list_dir(&tree.root.join("M")).unwrap(), ["Heat.mkv"]);
    assert_eq!(tree.read("M/Heat.mkv"), "bytes");
}

// ---- long paths ----------------------------------------------------------------------------

#[cfg(windows)]
#[test]
fn a_path_over_the_windows_limit_can_be_moved() {
    let tree = Tree::new();
    let deep = format!("{}/{}", "d".repeat(100), "e".repeat(100));
    let from = format!("{deep}/{}.mkv", "f".repeat(60));
    let to = format!("{deep}/{}.mkv", "g".repeat(60));
    // Created through the extended-length form; a plain path this long would fail.
    let abs = tree.root.join(&from);
    assert!(abs.to_string_lossy().len() > 260);
    StdFs.create_dir_all(abs.parent().unwrap()).unwrap();
    StdFs
        .copy_file(&write_source(&tree, "src.bin"), &abs)
        .unwrap();

    let item = batch_item(
        &tree,
        "1",
        &deep,
        &deep,
        FolderChange::MoveFiles,
        vec![carried(&from, &to, VIDEO)],
    );
    let mut journal = MemoryJournal::new();
    let report = run(&tree, &mut journal, &[item]);
    assert_eq!(report.done(), 1, "{report:?}");
    assert!(StdFs.meta(&tree.root.join(&to)).unwrap().is_some());
    assert!(StdFs.meta(&abs).unwrap().is_none());
}

#[cfg(windows)]
fn write_source(tree: &Tree, name: &str) -> PathBuf {
    tree.write(name, "long path bytes");
    tree.root.join(name)
}

// ---- NFO ownership -------------------------------------------------------------------------

#[test]
fn only_nfos_that_the_app_wrote_are_trashed() {
    let tree = Tree::new();
    tree.write("M/old.nfo", MANAGED_NFO);
    tree.write("M/release.nfo", FOREIGN_NFO);
    let item = batch_item(
        &tree,
        "1",
        "M",
        "M",
        FolderChange::MoveFiles,
        vec![
            trash_file("M/old.nfo", NFO),
            trash_file("M/release.nfo", NFO),
        ],
    );
    let mut journal = MemoryJournal::new();
    let report = run(&tree, &mut journal, &[item]);
    assert_eq!(report.done(), 1);
    assert!(!tree.exists("M/old.nfo"));
    assert_eq!(tree.read("M/release.nfo"), FOREIGN_NFO);
    assert_eq!(report.items[0].trashed, 1);
    assert_eq!(report.items[0].notes.len(), 1);
}

#[test]
fn a_foreign_nfo_at_the_destination_is_not_replaced() {
    let tree = Tree::new();
    tree.write("M/old.nfo", MANAGED_NFO);
    tree.write("M/Heat.nfo", FOREIGN_NFO);
    let item = batch_item(
        &tree,
        "1",
        "M",
        "M",
        FolderChange::MoveFiles,
        vec![carried("M/old.nfo", "M/Heat.nfo", NFO)],
    );
    let mut journal = MemoryJournal::new();
    let report = run(&tree, &mut journal, &[item]);
    assert_eq!(tree.read("M/Heat.nfo"), FOREIGN_NFO);
    assert_eq!(tree.read("M/old.nfo"), MANAGED_NFO);
    assert_eq!(report.items[0].notes.len(), 1);
    assert!(tree.trashed().is_empty());
}

#[test]
fn a_managed_nfo_at_the_destination_is_replaced_through_the_trash() {
    let tree = Tree::new();
    tree.write("M/old.nfo", MANAGED_NFO);
    tree.write("M/Heat.nfo", "<movie><title>Older</title></movie>");
    let item = batch_item(
        &tree,
        "1",
        "M",
        "M",
        FolderChange::MoveFiles,
        vec![carried("M/old.nfo", "M/Heat.nfo", NFO)],
    );
    let mut journal = MemoryJournal::new();
    run(&tree, &mut journal, &[item]);
    assert_eq!(tree.read("M/Heat.nfo"), MANAGED_NFO);
    assert_eq!(tree.trashed().len(), 1);
}

// ---- clean-up ------------------------------------------------------------------------------

#[test]
fn empty_folders_go_but_never_the_root_and_never_a_folder_with_other_files() {
    let tree = Tree::new();
    tree.write("A/B/C/x.mkv", "x");
    tree.write("A/other.txt", "someone else's file");
    tree.write("Z/y.mkv", "y");
    let items = [
        batch_item(
            &tree,
            "1",
            "A/B/C",
            "A/B/C",
            FolderChange::MoveFiles,
            vec![carried("A/B/C/x.mkv", "N/x.mkv", VIDEO)],
        ),
        batch_item(
            &tree,
            "2",
            "Z",
            "Z",
            FolderChange::MoveFiles,
            vec![carried("Z/y.mkv", "y.mkv", VIDEO)],
        ),
    ];
    let mut journal = MemoryJournal::new();
    let report = run(&tree, &mut journal, &items);
    assert_eq!(report.done(), 2, "{report:?}");
    // C and B are empty and go; A still holds a file.
    assert_eq!(tree.dirs().into_iter().collect::<Vec<_>>(), ["A", "N"]);
    assert_eq!(tree.read("A/other.txt"), "someone else's file");
    // y.mkv moved to the root of the data source, and the root is still there.
    assert_eq!(tree.read("y.mkv"), "y");
    assert!(!tree.exists("Z"));
}

#[test]
fn the_root_of_another_data_source_inside_this_one_is_never_removed() {
    // A TV data source lies in a folder of this movie data source, and its two episodes were
    // also taken for movies of the movie source.
    let tree = Tree::new();
    tree.write("Chernobyl/Season 1/Chernobyl S01E01.mkv", "one");
    tree.write("Chernobyl/Season 1/Chernobyl S01E02.mkv", "two");
    let tv_root = tree.root.join("Chernobyl");
    let items = [
        batch_item(
            &tree,
            "1",
            "Chernobyl/Season 1",
            "Chernobyl S01E01",
            FolderChange::MoveFiles,
            vec![carried(
                "Chernobyl/Season 1/Chernobyl S01E01.mkv",
                "Chernobyl S01E01/Chernobyl S01E01.mkv",
                VIDEO,
            )],
        ),
        batch_item(
            &tree,
            "2",
            "Chernobyl/Season 1",
            "Chernobyl S01E02",
            FolderChange::MoveFiles,
            vec![carried(
                "Chernobyl/Season 1/Chernobyl S01E02.mkv",
                "Chernobyl S01E02/Chernobyl S01E02.mkv",
                VIDEO,
            )],
        ),
    ];
    let mut journal = MemoryJournal::new();
    let options = ExecOptions::new("b1").with_protected_dirs([tree.root.clone(), tv_root]);
    let report = run_with(&StdFs, &mut journal, options, &items);
    assert_eq!(report.done(), 2, "{report:?}");
    assert!(tree.exists("Chernobyl S01E01/Chernobyl S01E01.mkv"));
    // The emptied season folder goes; the data source root stays.
    assert!(!tree.exists("Chernobyl/Season 1"));
    assert!(tree.exists("Chernobyl"), "{:?}", tree.dirs());
}

#[test]
fn a_folder_that_holds_another_data_source_is_moved_file_by_file() {
    let tree = Tree::new();
    tree.write("Old/heat.mkv", "video");
    tree.write("Old/Shows/x.mkv", "a show of another source");
    let shows = tree.root.join("Old").join("Shows");
    let item = batch_item(
        &tree,
        "1",
        "Old",
        "Heat (1995)",
        FolderChange::Rename,
        vec![carried(
            "Old/heat.mkv",
            "Heat (1995)/Heat (1995).mkv",
            VIDEO,
        )],
    );
    let mut journal = MemoryJournal::new();
    let options = ExecOptions::new("b1").with_protected_dirs([shows]);
    let report = run_with(&StdFs, &mut journal, options, &[item]);
    assert_eq!(report.done(), 1, "{report:?}");
    assert!(tree.exists("Heat (1995)/Heat (1995).mkv"));
    assert_eq!(tree.read("Old/Shows/x.mkv"), "a show of another source");
}

#[test]
fn the_trash_folder_is_never_cleaned_away() {
    let tree = Tree::new();
    tree.write("M/a.jpg", "stale");
    let item = batch_item(
        &tree,
        "1",
        "M",
        "M",
        FolderChange::MoveFiles,
        vec![trash_file("M/a.jpg", MediaFileType::Thumb)],
    );
    let mut journal = MemoryJournal::new();
    run(&tree, &mut journal, &[item]);
    assert!(!tree.exists("M"));
    assert_eq!(tree.trashed().len(), 1);
}

#[test]
fn unwanted_files_in_a_renamed_folder_go_to_the_trash() {
    let (tree, items) = whole_folder_scenario();
    let mut options = ExecOptions::new("u");
    options.unwanted_patterns = DEFAULT_UNWANTED.iter().map(|s| (*s).to_owned()).collect();
    tree.write("Old/Heat/RELEASE.URL", "[InternetShortcut]");
    let mut journal = MemoryJournal::new();
    let report = run_with(&StdFs, &mut journal, options, &items);
    assert_eq!(report.done(), 1, "{report:?}");
    assert!(!tree.exists("Heat (1995)/notes.txt"));
    assert!(!tree.exists("Heat (1995)/RELEASE.URL"));
    let trashed = tree.trashed();
    assert!(trashed.contains_key(".deletedByCineVault/u/Heat (1995)/notes.txt"));
    // The movie files are untouched.
    assert!(tree.exists("Heat (1995)/Heat (1995).mkv"));
}

#[test]
fn the_unwanted_files_of_a_folder_are_its_matching_files_and_never_its_folders() {
    let tree = Tree::new();
    tree.write("Heat/Heat.mkv", "v");
    tree.write("Heat/notes.TXT", "n");
    tree.write("Heat/release.url", "u");
    tree.write("Heat/Sample.txt/inner.mkv", "x");
    let dir = tree.root.join("Heat");
    let found = unwanted_files_in(
        &StdFs,
        &dir,
        &[".txt".to_owned(), r"^release\.url$".to_owned()],
    )
    .unwrap();
    assert_eq!(found, ["notes.TXT", "release.url"]);
    assert!(unwanted_files_in(&StdFs, &dir, &[]).unwrap().is_empty());
    assert!(
        unwanted_files_in(&StdFs, &tree.root.join("Nope"), &[".txt".into()])
            .unwrap()
            .is_empty()
    );
    assert!(matches!(
        unwanted_files_in(&StdFs, &dir, &["(".to_owned()]),
        Err(ExecError::BadPattern { .. })
    ));
}

#[test]
fn unwanted_files_do_not_take_a_planned_file_with_them() {
    let tree = Tree::new();
    tree.write("Old/m.mkv", "video");
    tree.write("Old/readme.txt", "read me");
    let item = batch_item(
        &tree,
        "1",
        "Old",
        "New",
        FolderChange::Rename,
        vec![
            carried("Old/m.mkv", "New/m.mkv", VIDEO),
            // A text file the plan keeps by name is the plan's file, not unwanted.
            carried("Old/readme.txt", "New/readme.txt", MediaFileType::Text),
        ],
    );
    let mut options = ExecOptions::new("u");
    options.unwanted_patterns = vec![".txt".into()];
    let mut journal = MemoryJournal::new();
    run_with(&StdFs, &mut journal, options, &[item]);
    assert_eq!(tree.read("New/readme.txt"), "read me");
}

#[test]
fn an_old_folder_with_only_unwanted_files_is_cleared_and_removed() {
    let tree = Tree::new();
    tree.write("Old/m.mkv", "video");
    tree.write("Old/readme.txt", "read me");
    let item = batch_item(
        &tree,
        "1",
        "Old",
        "New",
        FolderChange::MoveFiles,
        vec![carried("Old/m.mkv", "New/m.mkv", VIDEO)],
    );
    let mut options = ExecOptions::new("u");
    options.unwanted_patterns = vec![r"\.txt$".into()];
    let mut journal = MemoryJournal::new();
    run_with(&StdFs, &mut journal, options, &[item]);
    assert!(!tree.exists("Old"));
    assert_eq!(tree.trashed().len(), 1);
}

#[test]
fn an_old_folder_with_an_unrelated_file_is_left_alone_by_the_unwanted_clean_up() {
    let tree = Tree::new();
    tree.write("Old/m.mkv", "video");
    tree.write("Old/readme.txt", "read me");
    tree.write("Old/other.mkv", "another movie");
    let item = batch_item(
        &tree,
        "1",
        "Old",
        "New",
        FolderChange::MoveFiles,
        vec![carried("Old/m.mkv", "New/m.mkv", VIDEO)],
    );
    let mut options = ExecOptions::new("u");
    options.unwanted_patterns = vec![".txt".into()];
    let mut journal = MemoryJournal::new();
    run_with(&StdFs, &mut journal, options, &[item]);
    assert_eq!(tree.read("Old/readme.txt"), "read me");
    assert_eq!(tree.read("Old/other.mkv"), "another movie");
}

#[test]
fn a_bad_unwanted_pattern_is_refused_before_anything_runs() {
    let mut journal = MemoryJournal::new();
    let mut options = ExecOptions::new("u");
    options.unwanted_patterns = vec!["(".into()];
    let result = Executor::new(&StdFs, &mut journal, options);
    assert!(matches!(result, Err(ExecError::BadPattern { .. })));
}

// ---- failure, safety -------------------------------------------------------------------------

#[test]
fn a_failing_step_takes_the_item_back_and_the_others_go_on() {
    let tree = Tree::new();
    tree.write("A/a.mkv", "a");
    tree.write("A/a.nfo", MANAGED_NFO);
    tree.write("B/b.mkv", "b");
    let bad = batch_item(
        &tree,
        "bad",
        "A",
        "A",
        FolderChange::MoveFiles,
        vec![
            carried("A/a.mkv", "A/A1.mkv", VIDEO),
            // The source of this one is missing, so the item fails after its first move.
            carried("A/ghost.nfo", "A/A1.nfo", NFO),
        ],
    );
    let good = batch_item(
        &tree,
        "good",
        "B",
        "B",
        FolderChange::MoveFiles,
        vec![carried("B/b.mkv", "B/B1.mkv", VIDEO)],
    );
    let mut journal = MemoryJournal::new();
    let report = run(&tree, &mut journal, &[bad, good]);
    assert!(matches!(report.items[0].status, ItemStatus::Failed(_)));
    assert_eq!(report.items[1].status, ItemStatus::Done);
    assert_eq!(tree.read("A/a.mkv"), "a");
    assert!(!tree.exists("A/A1.mkv"));
    assert_eq!(tree.read("B/B1.mkv"), "b");
    let states: Vec<JournalState> = journal
        .all()
        .iter()
        .filter(|r| r.entry.item_id.as_deref() == Some("bad"))
        .map(|r| r.state)
        .collect();
    assert_eq!(states, [JournalState::Undone]);
}

#[test]
fn a_path_that_leaves_the_data_source_is_refused() {
    let tree = Tree::new();
    tree.write("M/a.mkv", "a");
    let item = batch_item(
        &tree,
        "1",
        "M",
        "M",
        FolderChange::MoveFiles,
        vec![carried("M/a.mkv", "../escaped.mkv", VIDEO)],
    );
    let mut journal = MemoryJournal::new();
    let report = run(&tree, &mut journal, &[item]);
    assert!(matches!(report.items[0].status, ItemStatus::Failed(_)));
    assert_eq!(tree.read("M/a.mkv"), "a");
    assert!(journal.all().is_empty());
}

#[test]
fn an_unavailable_data_source_changes_nothing() {
    let tree = Tree::new();
    let mut item = batch_item(
        &tree,
        "1",
        "M",
        "M",
        FolderChange::MoveFiles,
        vec![carried("M/a.mkv", "M/b.mkv", VIDEO)],
    );
    item.root = tree.root.join("gone").to_string_lossy().into_owned();
    let mut journal = MemoryJournal::new();
    let report = run(&tree, &mut journal, &[item]);
    assert!(matches!(report.items[0].status, ItemStatus::Failed(_)));
    assert!(journal.all().is_empty());
}

#[test]
fn a_canceled_batch_does_not_start_the_next_item() {
    let tree = Tree::new();
    tree.write("A/a.mkv", "a");
    tree.write("B/b.mkv", "b");
    let items = [
        batch_item(
            &tree,
            "1",
            "A",
            "A",
            FolderChange::MoveFiles,
            vec![carried("A/a.mkv", "A/A.mkv", VIDEO)],
        ),
        batch_item(
            &tree,
            "2",
            "B",
            "B",
            FolderChange::MoveFiles,
            vec![carried("B/b.mkv", "B/B.mkv", VIDEO)],
        ),
    ];
    let done = Cell::new(0);
    let cancel = || done.get() >= 1;
    let mut journal = MemoryJournal::new();
    let report = Executor::new(&StdFs, &mut journal, ExecOptions::new("c"))
        .unwrap()
        .with_cancel(&cancel)
        .with_progress(|_| done.set(done.get() + 1))
        .run(&items)
        .unwrap();
    assert_eq!(report.items[0].status, ItemStatus::Done);
    assert_eq!(report.items[1].status, ItemStatus::Canceled);
    assert_eq!(tree.read("B/b.mkv"), "b");
}

// ---- ordering ------------------------------------------------------------------------------

fn op(kind: OpKind, from: &str, to: Option<&str>) -> Op {
    Op {
        kind,
        file_type: VIDEO,
        from: Some(from.to_owned()),
        to: to.map(str::to_owned),
    }
}

#[test]
fn a_file_in_the_way_is_moved_away_first() {
    let ordered = order_ops(vec![
        op(OpKind::Move, "a", Some("b")),
        op(OpKind::Move, "b", Some("c")),
    ]);
    let froms: Vec<_> = ordered.iter().map(|o| o.from.clone().unwrap()).collect();
    assert_eq!(froms, ["b", "a"]);
}

#[test]
fn copies_are_made_before_their_source_moves() {
    let ordered = order_ops(vec![
        op(OpKind::Move, "a", Some("b")),
        op(OpKind::Copy, "a", Some("c")),
        op(OpKind::Trash, "a", None),
    ]);
    let kinds: Vec<_> = ordered.iter().map(|o| o.kind).collect();
    assert_eq!(kinds, [OpKind::Copy, OpKind::Move, OpKind::Trash]);
}

#[test]
fn ordering_keeps_the_plan_when_nothing_depends_and_survives_a_swap() {
    let plain = vec![
        op(OpKind::Move, "a", Some("b")),
        op(OpKind::Move, "c", Some("d")),
    ];
    assert_eq!(order_ops(plain.clone()), plain);
    let swap = vec![
        op(OpKind::Move, "a", Some("b")),
        op(OpKind::Move, "b", Some("a")),
    ];
    assert_eq!(order_ops(swap).len(), 2);
}

#[test]
fn an_item_moving_into_a_path_another_item_leaves_runs_after_it() {
    let tree = Tree::new();
    tree.write("M/1.mkv", "one");
    tree.write("M/2.mkv", "two");
    let first = batch_item(
        &tree,
        "first",
        "M",
        "M",
        FolderChange::MoveFiles,
        vec![carried("M/1.mkv", "M/2.mkv", VIDEO)],
    );
    let second = batch_item(
        &tree,
        "second",
        "M",
        "M",
        FolderChange::MoveFiles,
        vec![carried("M/2.mkv", "M/3.mkv", VIDEO)],
    );
    let items = [first, second];
    assert_eq!(order_items(&items), [1, 0]);

    let mut journal = MemoryJournal::new();
    let report = run(&tree, &mut journal, &items);
    assert_eq!(report.done(), 2, "{report:?}");
    assert_eq!(tree.read("M/2.mkv"), "one");
    assert_eq!(tree.read("M/3.mkv"), "two");
    assert!(tree.trashed().is_empty());
    // Reports keep the order of the input.
    assert_eq!(report.items[0].id, "first");
}

#[test]
fn a_folder_renamed_onto_a_name_another_item_frees_waits_for_it() {
    let tree = Tree::new();
    tree.write("Heat/a.mkv", "a");
    tree.write("Heat 2/b.mkv", "b");
    let one = batch_item(
        &tree,
        "one",
        "Heat 2",
        "Heat",
        FolderChange::Rename,
        vec![carried("Heat 2/b.mkv", "Heat/b.mkv", VIDEO)],
    );
    let two = batch_item(
        &tree,
        "two",
        "Heat",
        "Heat (1995)",
        FolderChange::Rename,
        vec![carried("Heat/a.mkv", "Heat (1995)/a.mkv", VIDEO)],
    );
    let items = [one, two];
    assert_eq!(order_items(&items), [1, 0]);
    let mut journal = MemoryJournal::new();
    let report = run(&tree, &mut journal, &items);
    assert_eq!(report.done(), 2, "{report:?}");
    assert_eq!(tree.read("Heat (1995)/a.mkv"), "a");
    assert_eq!(tree.read("Heat/b.mkv"), "b");
}

// ---- the library database --------------------------------------------------------------------

pub(super) fn db_with_movie(
    tree: &Tree,
    folder: &str,
    file: &str,
) -> (TempDir, Arc<cv_db::Database>, i64) {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(cv_db::Database::open(dir.path().join("lib.db")).unwrap());
    let mut item = Item::movie("Heat");
    item.core.data_source = tree.root_str();
    let folder_abs = folder
        .split('/')
        .fold(tree.root.clone(), |path, part| path.join(part))
        .to_string_lossy()
        .into_owned();
    item.core.path = folder_abs.clone();
    item.core
        .media_files
        .push(MediaFile::new(VIDEO, folder_abs, file));
    let id = db
        .write(move |tx| cv_db::repo::insert_item(tx, &item))
        .unwrap();
    (dir, db, id.0)
}

pub(super) fn db_movie(db: &cv_db::Database, id: i64) -> Item {
    db.read(|c| cv_db::repo::get_item(c, cv_domain::ItemId(id)))
        .unwrap()
        .unwrap()
}

pub(super) fn heat_scenario(tree: &Tree) -> Vec<PlannedFile> {
    tree.write("Old/Heat/heat.mkv", "video bytes");
    vec![carried(
        "Old/Heat/heat.mkv",
        "Heat (1995)/Heat (1995).mkv",
        VIDEO,
    )]
}

#[test]
fn the_database_journal_records_the_batch_and_repoints_the_library() {
    let tree = Tree::new();
    let files = heat_scenario(&tree);
    let (_dir, db, id) = db_with_movie(&tree, "Old/Heat", "heat.mkv");
    let item = batch_item(
        &tree,
        &id.to_string(),
        "Old/Heat",
        "Heat (1995)",
        FolderChange::Rename,
        files,
    );
    let mut journal = DbJournal::new(Arc::clone(&db));
    let report = run_with(&StdFs, &mut journal, ExecOptions::new("db1"), &[item]);
    assert_eq!(report.done(), 1, "{report:?}");

    let rows = journal.batch("db1").unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|r| r.state == JournalState::Done));
    assert_eq!(
        rows[0].entry.item_id.as_deref(),
        Some(id.to_string().as_str())
    );

    // The library points at the new place with no rescan.
    let movie = db_movie(&db, id);
    let new_folder = tree.root.join("Heat (1995)");
    assert_eq!(Path::new(&movie.core.path), new_folder);
    let file = &movie.core.media_files[0];
    assert_eq!(Path::new(&file.directory), new_folder);
    assert_eq!(file.filename, "Heat (1995).mkv");
    assert!(new_folder.join(&file.filename).exists());
}

#[derive(Default)]
struct Moves(std::sync::Mutex<Vec<(String, String, bool)>>);

impl PathObserver for Moves {
    fn path_moved(&self, from: &str, to: &str, is_dir: bool) {
        self.0
            .lock()
            .unwrap()
            .push((from.to_owned(), to.to_owned(), is_dir));
    }
}

#[test]
fn the_database_journal_tells_its_observer_about_every_move_and_every_undo() {
    let tree = Tree::new();
    let files = heat_scenario(&tree);
    let (_dir, db, id) = db_with_movie(&tree, "Old/Heat", "heat.mkv");
    let item = batch_item(
        &tree,
        &id.to_string(),
        "Old/Heat",
        "Heat (1995)",
        FolderChange::Rename,
        files,
    );
    let moves = Arc::new(Moves::default());
    let observer: Arc<dyn PathObserver> = moves.clone();
    let mut journal = DbJournal::new(Arc::clone(&db)).with_observer(observer);
    let report = run_with(&StdFs, &mut journal, ExecOptions::new("obs1"), &[item]);
    assert_eq!(report.done(), 1, "{report:?}");

    let seen = moves.0.lock().unwrap().clone();
    assert_eq!(seen.len(), 2, "{seen:?}");
    let folder = seen.iter().find(|m| m.2).expect("the folder move");
    assert!(folder.0.ends_with("Heat") && folder.1.ends_with("Heat (1995)"));
    assert!(
        seen.iter()
            .any(|m| !m.2 && m.1.ends_with("Heat (1995).mkv"))
    );
    moves.0.lock().unwrap().clear();

    undo_batch(&mut journal, &StdFs, "obs1", &UndoOptions::default()).unwrap();

    let seen = moves.0.lock().unwrap().clone();
    assert_eq!(seen.len(), 2, "{seen:?}");
    let folder = seen.iter().find(|m| m.2).expect("the folder move back");
    assert!(folder.0.ends_with("Heat (1995)") && folder.1.ends_with("Heat"));
}

#[test]
fn a_crash_with_the_database_journal_recovers_and_the_library_agrees_with_the_disk() {
    for at in 0..3 {
        for after in [false, true] {
            let tree = Tree::new();
            let files = heat_scenario(&tree);
            let (_dir, db, id) = db_with_movie(&tree, "Old/Heat", "heat.mkv");
            let item = batch_item(
                &tree,
                &id.to_string(),
                "Old/Heat",
                "Heat (1995)",
                FolderChange::Rename,
                files,
            );
            let fault = FaultFs::crashing(at, after, false);
            let mut journal = DbJournal::new(Arc::clone(&db));
            let crashed = catch_unwind(AssertUnwindSafe(|| {
                run_with(&fault, &mut journal, ExecOptions::new("db"), &[item])
            }));
            assert!(crashed.is_err(), "mutation {at}");

            // A new process: the journal still holds the planned row.
            let mut reopened = DbJournal::new(Arc::clone(&db));
            recover(&mut reopened, &StdFs).unwrap();
            assert!(reopened.pending().unwrap().is_empty());

            let movie = db_movie(&db, id);
            let located = Path::new(&movie.core.media_files[0].directory)
                .join(&movie.core.media_files[0].filename);
            assert!(
                located.exists(),
                "mutation {at} {after}: the library points at {located:?}"
            );
            // The video exists exactly once in the tree.
            let videos = tree.live().keys().filter(|p| p.ends_with(".mkv")).count();
            assert_eq!(videos, 1);
        }
    }
}
