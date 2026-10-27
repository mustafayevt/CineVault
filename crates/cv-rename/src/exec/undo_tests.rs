//! Tests of undo: the spec scenarios "undo yesterday's batch" and "partial undo",
//! restoring every scenario of the executor tests exactly, destination verification, copies,
//! idempotence, a disk failure in the middle and the database journal.

use std::cell::Cell;
use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::Path;
use std::sync::Arc;

use super::tests::{
    FaultFs, MANAGED_NFO, NFO, Tree, VIDEO, batch_item, carried, case_scenario, db_movie,
    db_with_movie, heat_scenario, run_with, shared_folder_scenario, whole_folder_scenario,
};
use super::*;
use crate::conflict::BatchItem;
use crate::plan::{FolderChange, PlannedFile};
use cv_domain::MediaFileType;

// ---- helpers -------------------------------------------------------------------------------

fn undo(tree: &Tree, journal: &mut dyn JournalStore, batch: &str) -> UndoReport {
    let options = UndoOptions {
        roots: vec![tree.root.clone()],
        ..UndoOptions::default()
    };
    undo_batch(journal, &StdFs, batch, &options).unwrap()
}

/// The folders outside the trash.
fn live_dirs(tree: &Tree) -> BTreeSet<String> {
    tree.dirs()
        .into_iter()
        .filter(|d| !d.starts_with(TRASH_DIR))
        .collect()
}

/// Runs a batch, undoes it, and checks the data source is exactly as it was.
fn round_trip(tree: &Tree, items: &[BatchItem], fs: &dyn Fs) -> UndoReport {
    let files = tree.live();
    let dirs = live_dirs(tree);
    let mut journal = MemoryJournal::new();
    let report = run_with(fs, &mut journal, ExecOptions::new("b1"), items);
    assert_eq!(report.failed(), 0, "{report:?}");
    assert_ne!(tree.live(), files, "the batch changed nothing");

    let undone = undo(tree, &mut journal, "b1");
    assert!(undone.is_complete(), "{undone:?}");
    assert_eq!(tree.live(), files);
    assert_eq!(live_dirs(tree), dirs);
    assert!(
        journal
            .batch("b1")
            .unwrap()
            .iter()
            .all(|r| r.state != JournalState::Done),
        "every done row is undone"
    );
    undone
}

fn two_folder_batch(tree: &Tree) -> Vec<BatchItem> {
    tree.write("Old/Heat/heat.mkv", "heat video");
    tree.write("Old/Heat/heat.nfo", MANAGED_NFO);
    tree.write("Old/Ronin/ronin.mkv", "ronin video");
    let heat = batch_item(
        tree,
        "heat",
        "Old/Heat",
        "Heat (1995)",
        FolderChange::Rename,
        vec![
            carried("Old/Heat/heat.mkv", "Heat (1995)/Heat (1995).mkv", VIDEO),
            carried("Old/Heat/heat.nfo", "Heat (1995)/Heat (1995).nfo", NFO),
        ],
    );
    let ronin = batch_item(
        tree,
        "ronin",
        "Old/Ronin",
        "Ronin (1998)",
        FolderChange::Rename,
        vec![carried(
            "Old/Ronin/ronin.mkv",
            "Ronin (1998)/Ronin (1998).mkv",
            VIDEO,
        )],
    );
    vec![heat, ronin]
}

// ---- spec: undo yesterday's batch ----------------------------------------------------------

#[test]
fn undoing_a_folder_batch_returns_every_folder_and_file_to_its_old_name() {
    let tree = Tree::new();
    let items = two_folder_batch(&tree);
    let undone = round_trip(&tree, &items, &StdFs);
    assert_eq!(undone.restored(), 2);
    assert!(undone.items.iter().all(|i| i.restored >= 2));
    assert_eq!(tree.read("Old/Heat/heat.mkv"), "heat video");
    assert_eq!(tree.read("Old/Ronin/ronin.mkv"), "ronin video");
}

#[test]
fn undo_of_a_renamed_show_leaves_no_season_folder_the_batch_created() {
    // The show folder is renamed as a whole, then each episode moves to a new season folder
    // inside it.
    let tree = Tree::new();
    tree.write("TV/The Office (US)/Season 1/ep1.mkv", "one");
    tree.write("TV/The Office (US)/Season 1/ep2.mkv", "two");
    tree.write("TV/The Office (US)/tvshow.nfo", MANAGED_NFO);
    let show = batch_item(
        &tree,
        "office",
        "TV/The Office (US)",
        "TV/The Office (2005)",
        FolderChange::Rename,
        vec![
            carried(
                "TV/The Office (US)/Season 1/ep1.mkv",
                "TV/The Office (2005)/Season 01/The Office - S01E01.mkv",
                VIDEO,
            ),
            carried(
                "TV/The Office (US)/Season 1/ep2.mkv",
                "TV/The Office (2005)/Season 01/The Office - S01E02.mkv",
                VIDEO,
            ),
            carried(
                "TV/The Office (US)/tvshow.nfo",
                "TV/The Office (2005)/tvshow.nfo",
                NFO,
            ),
        ],
    );
    round_trip(&tree, &[show], &StdFs);
    assert!(!tree.exists("TV/The Office (US)/Season 01"));
}

#[test]
fn undo_keeps_an_empty_season_folder_that_was_there_before_the_batch() {
    // `Season 01` exists, empty, next to `Season 1`; the batch moves the episodes into it.
    let tree = Tree::new();
    tree.write("TV/The Office (US)/Season 1/ep1.mkv", "one");
    tree.write("TV/The Office (US)/Season 1/ep2.mkv", "two");
    fs::create_dir_all(tree.root.join("TV/The Office (US)/Season 01")).unwrap();
    let show = batch_item(
        &tree,
        "office",
        "TV/The Office (US)",
        "TV/The Office (US)",
        FolderChange::Unchanged,
        vec![
            carried(
                "TV/The Office (US)/Season 1/ep1.mkv",
                "TV/The Office (US)/Season 01/The Office - S01E01.mkv",
                VIDEO,
            ),
            carried(
                "TV/The Office (US)/Season 1/ep2.mkv",
                "TV/The Office (US)/Season 01/The Office - S01E02.mkv",
                VIDEO,
            ),
        ],
    );
    // The round trip compares every folder, the empty one included.
    let undone = round_trip(&tree, &[show], &StdFs);
    assert!(tree.root.join("TV/The Office (US)/Season 01").is_dir());
    assert!(
        undone.items[0]
            .removed_folders
            .iter()
            .all(|d| !d.ends_with("Season 01")),
        "{:?}",
        undone.items[0].removed_folders
    );
}

#[test]
fn undo_restores_a_folder_with_stale_artwork_trash_and_extras() {
    let (tree, items) = whole_folder_scenario();
    let undone = round_trip(&tree, &items, &StdFs);
    // The stale artwork came out of the trash again and the trash batch folder is gone.
    assert_eq!(tree.read("Old/Heat/folder.jpg"), "stale artwork");
    assert!(
        tree.trashed().is_empty() && !tree.exists(".deletedByCineVault/b1"),
        "{:?}",
        tree.trashed()
    );
    assert!(undone.items[0].restored >= 5);
}

#[test]
fn undo_restores_a_replaced_file_and_removes_the_folder_it_created() {
    let (tree, items) = shared_folder_scenario();
    let undone = round_trip(&tree, &items, &StdFs);
    // The poster the batch replaced is back; the folder `New` is gone again.
    assert_eq!(
        tree.read("Multi/A (2000)-poster.jpg"),
        "old poster in the way"
    );
    assert_eq!(tree.read("Multi/p.jpg"), "new poster");
    assert!(!tree.exists("New"));
    let removed = &undone.items[0].removed_folders;
    assert!(removed.iter().any(|d| d.ends_with("New")), "{removed:?}");
    assert!(
        removed.iter().all(|d| !d.ends_with(TRASH_DIR)),
        "{removed:?}"
    );
}

#[test]
fn undo_restores_a_case_only_rename() {
    let (tree, items) = case_scenario();
    round_trip(&tree, &items, &StdFs);
    let names: Vec<String> = fs::read_dir(&tree.root)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["heat (1995)"]);
}

#[test]
fn undo_restores_files_moved_across_volumes() {
    let (tree, items) = shared_folder_scenario();
    round_trip(&tree, &items, &FaultFs::across_volumes());
    assert!(tree.files().keys().all(|p| !p.ends_with(".cvpart")));
}

#[test]
fn undo_restores_a_batch_whose_items_move_into_each_others_paths() {
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
    round_trip(&tree, &[first, second], &StdFs);
    assert_eq!(tree.read("M/1.mkv"), "one");
    assert_eq!(tree.read("M/2.mkv"), "two");
}

#[test]
fn undo_restores_a_swap_of_two_names() {
    let tree = Tree::new();
    tree.write("M/a.mkv", "aaa");
    tree.write("M/b.mkv", "bbbb");
    let a = batch_item(
        &tree,
        "a",
        "M",
        "M",
        FolderChange::MoveFiles,
        vec![carried("M/a.mkv", "M/b.mkv", VIDEO)],
    );
    let b = batch_item(
        &tree,
        "b",
        "M",
        "M",
        FolderChange::MoveFiles,
        vec![carried("M/b.mkv", "M/a.mkv", VIDEO)],
    );
    round_trip(&tree, &[a, b], &StdFs);
    assert_eq!(tree.read("M/a.mkv"), "aaa");
    assert_eq!(tree.read("M/b.mkv"), "bbbb");
}

#[test]
fn undo_runs_newest_operation_first() {
    let (tree, items) = shared_folder_scenario();
    let mut journal = MemoryJournal::new();
    run_with(&StdFs, &mut journal, ExecOptions::new("b1"), &items);
    let log = Cell::new(Vec::<String>::new());
    struct Spy<'a>(&'a Cell<Vec<String>>);
    impl Fs for Spy<'_> {
        fn meta(&self, p: &Path) -> io::Result<Option<Meta>> {
            StdFs.meta(p)
        }
        fn create_dir_all(&self, p: &Path) -> io::Result<()> {
            StdFs.create_dir_all(p)
        }
        fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
            let mut v = self.0.take();
            v.push(from.to_string_lossy().into_owned());
            self.0.set(v);
            StdFs.rename(from, to)
        }
        fn copy_file(&self, a: &Path, b: &Path) -> io::Result<()> {
            StdFs.copy_file(a, b)
        }
        fn same_content(&self, a: &Path, b: &Path) -> io::Result<bool> {
            StdFs.same_content(a, b)
        }
        fn remove_file(&self, p: &Path) -> io::Result<()> {
            StdFs.remove_file(p)
        }
        fn remove_dir(&self, p: &Path) -> io::Result<()> {
            StdFs.remove_dir(p)
        }
        fn list_dir(&self, p: &Path) -> io::Result<Vec<String>> {
            StdFs.list_dir(p)
        }
    }
    let rows = journal.batch("b1").unwrap();
    let expected: Vec<String> = rows
        .iter()
        .filter(|r| r.state == JournalState::Done)
        .rev()
        .map(|r| r.entry.to.clone())
        .collect();
    let options = UndoOptions::default();
    undo_batch(&mut journal, &Spy(&log), "b1", &options).unwrap();
    assert_eq!(log.take(), expected);
    let _ = tree;
}

// ---- spec: partial undo --------------------------------------------------------------------

#[test]
fn a_folder_renamed_by_hand_is_reported_and_the_other_items_are_restored() {
    let tree = Tree::new();
    let items = two_folder_batch(&tree);
    let mut journal = MemoryJournal::new();
    run_with(&StdFs, &mut journal, ExecOptions::new("b1"), &items);
    // The user renames one folder afterwards.
    fs::rename(
        tree.root.join("Heat (1995)"),
        tree.root.join("Heat - director's cut"),
    )
    .unwrap();

    let undone = undo(&tree, &mut journal, "b1");
    assert_eq!((undone.restored(), undone.not_restorable()), (1, 1));
    assert!(!undone.is_complete());
    // The other item is back; the modified one is untouched.
    assert_eq!(tree.read("Old/Ronin/ronin.mkv"), "ronin video");
    assert_eq!(
        tree.read("Heat - director's cut/Heat (1995).mkv"),
        "heat video"
    );
    assert!(
        undone
            .problems()
            .all(|(id, _)| id.as_deref() == Some("heat"))
    );
    // The folder itself is among the reasons: it is not where the organize put it.
    let problem = undone
        .problems()
        .map(|(_, p)| p)
        .find(|p| p.path.ends_with("Heat (1995)"))
        .expect("the folder is reported");
    assert!(problem.reason.contains("no longer exists"), "{problem:?}");
}

#[test]
fn a_file_edited_after_the_organize_is_reported_with_the_reason() {
    let tree = Tree::new();
    let items = two_folder_batch(&tree);
    let mut journal = MemoryJournal::new();
    run_with(&StdFs, &mut journal, ExecOptions::new("b1"), &items);
    fs::write(
        tree.root.join("Heat (1995)/Heat (1995).nfo"),
        "edited by hand, longer",
    )
    .unwrap();

    let undone = undo(&tree, &mut journal, "b1");
    assert_eq!((undone.restored(), undone.not_restorable()), (1, 1));
    let (_, problem) = undone.problems().next().unwrap();
    assert!(problem.reason.contains("was modified"), "{problem:?}");
    // Nothing of the modified item moved, not even the files that were fine.
    assert_eq!(tree.read("Heat (1995)/Heat (1995).mkv"), "heat video");
    assert!(!tree.exists("Old/Heat"));
    let states: Vec<_> = journal
        .batch("b1")
        .unwrap()
        .into_iter()
        .filter(|r| r.entry.item_id.as_deref() == Some("heat"))
        .map(|r| r.state)
        .collect();
    assert!(states.iter().all(|s| *s == JournalState::Done));
}

#[test]
fn a_file_touched_without_changing_its_size_is_still_detected() {
    let tree = Tree::new();
    tree.write("A/a.mkv", "aaaa");
    let item = batch_item(
        &tree,
        "a",
        "A",
        "A",
        FolderChange::MoveFiles,
        vec![carried("A/a.mkv", "A/A1.mkv", VIDEO)],
    );
    let mut journal = MemoryJournal::new();
    run_with(&StdFs, &mut journal, ExecOptions::new("b1"), &[item]);
    let path = tree.root.join("A/A1.mkv");
    fs::write(&path, "bbbb").unwrap();
    let file = fs::File::options().write(true).open(&path).unwrap();
    file.set_modified(std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000))
        .unwrap();
    drop(file);

    let undone = undo(&tree, &mut journal, "b1");
    assert_eq!(undone.not_restorable(), 1);
    let (_, problem) = undone.problems().next().unwrap();
    assert!(problem.reason.contains("modification time"), "{problem:?}");
}

#[test]
fn an_original_location_taken_by_something_else_is_reported() {
    let tree = Tree::new();
    tree.write("A/a.mkv", "aaa");
    let item = batch_item(
        &tree,
        "a",
        "A",
        "A",
        FolderChange::MoveFiles,
        vec![carried("A/a.mkv", "A/A1.mkv", VIDEO)],
    );
    let mut journal = MemoryJournal::new();
    run_with(&StdFs, &mut journal, ExecOptions::new("b1"), &[item]);
    tree.write("A/a.mkv", "a new file the user put there");

    let undone = undo(&tree, &mut journal, "b1");
    assert_eq!(undone.not_restorable(), 1);
    let (_, problem) = undone.problems().next().unwrap();
    assert!(problem.reason.contains("something else"), "{problem:?}");
    // Both files are still there; nothing was overwritten.
    assert_eq!(tree.read("A/a.mkv"), "a new file the user put there");
    assert_eq!(tree.read("A/A1.mkv"), "aaa");
}

#[test]
fn every_problem_of_an_item_is_listed_and_a_later_undo_finishes_the_rest() {
    let tree = Tree::new();
    let items = two_folder_batch(&tree);
    let mut journal = MemoryJournal::new();
    run_with(&StdFs, &mut journal, ExecOptions::new("b1"), &items);
    let moved = tree.root.join("Heat (1995)/Heat (1995).mkv");
    fs::rename(&moved, tree.root.join("Heat (1995)/elsewhere.mkv")).unwrap();
    fs::write(tree.root.join("Heat (1995)/Heat (1995).nfo"), "changed nfo").unwrap();

    let undone = undo(&tree, &mut journal, "b1");
    assert_eq!(undone.problems().count(), 2);

    // The user puts the video back; the NFO stays edited. The same batch can be undone again:
    // the item that was restored is not offered twice, and only the edited NFO is reported.
    fs::rename(tree.root.join("Heat (1995)/elsewhere.mkv"), &moved).unwrap();
    let again = undo(&tree, &mut journal, "b1");
    assert_eq!(again.items.len(), 1, "{again:?}");
    assert_eq!(again.items[0].id.as_deref(), Some("heat"));
    assert_eq!(again.problems().count(), 1);
    assert!(
        again
            .problems()
            .all(|(_, p)| p.path.ends_with("Heat (1995).nfo"))
    );
}

#[test]
fn rows_without_an_item_are_checked_one_by_one() {
    let tree = Tree::new();
    tree.write("A/a.mkv", "aaa");
    tree.write("A/b.mkv", "bbb");
    let mut journal = MemoryJournal::new();
    for (seq, (from, to)) in [("A/a.mkv", "A/a1.mkv"), ("A/b.mkv", "A/b1.mkv")]
        .into_iter()
        .enumerate()
    {
        let (from, to) = (tree.root.join(from), tree.root.join(to));
        fs::rename(&from, &to).unwrap();
        let meta = StdFs.meta(&to).unwrap().unwrap();
        let entry = JournalEntry {
            batch_id: "loose".into(),
            seq: u32::try_from(seq).unwrap(),
            item_id: None,
            op: JournalOp::Move,
            from: from.to_string_lossy().into_owned(),
            to: to.to_string_lossy().into_owned(),
            is_dir: false,
            created_parents: Some(0),
        };
        journal.begin(&entry).unwrap();
        journal
            .complete(
                &entry,
                &Completion::Done {
                    size_bytes: Some(meta.len),
                    modified_ms: Some(meta.modified_ms),
                },
            )
            .unwrap();
    }
    fs::write(tree.root.join("A/b1.mkv"), "changed").unwrap();
    let undone = undo(&tree, &mut journal, "loose");
    assert_eq!((undone.restored(), undone.not_restorable()), (1, 1));
    assert_eq!(tree.read("A/a.mkv"), "aaa");
}

// ---- copies --------------------------------------------------------------------------------

fn copy_batch(tree: &Tree) -> Vec<BatchItem> {
    tree.write("Old/Heat/heat.mkv", "video");
    tree.write("Old/Heat/poster.jpg", "poster bytes");
    let poster = PlannedFile {
        from: "Old/Heat/poster.jpg".into(),
        kind: MediaFileType::Poster,
        owner: crate::plan::Owner::Movie,
        fate: crate::plan::Fate::Carried {
            to: vec![
                "Heat (1995)/poster.jpg".into(),
                "Heat (1995)/folder.jpg".into(),
            ],
        },
    };
    vec![batch_item(
        tree,
        "heat",
        "Old/Heat",
        "Heat (1995)",
        FolderChange::Rename,
        vec![
            carried("Old/Heat/heat.mkv", "Heat (1995)/Heat (1995).mkv", VIDEO),
            poster,
        ],
    )]
}

#[test]
fn undo_moves_the_extra_copies_to_the_trash() {
    let tree = Tree::new();
    let items = copy_batch(&tree);
    let mut journal = MemoryJournal::new();
    run_with(&StdFs, &mut journal, ExecOptions::new("b1"), &items);
    assert!(
        journal
            .all()
            .iter()
            .any(|r| r.entry.op == JournalOp::Copy && r.state == JournalState::Done)
    );

    let undone = undo(&tree, &mut journal, "b1");
    assert!(undone.is_complete(), "{undone:?}");
    assert_eq!(undone.items[0].discarded, 1);
    assert_eq!(tree.read("Old/Heat/poster.jpg"), "poster bytes");
    assert_eq!(tree.read("Old/Heat/heat.mkv"), "video");
    // The copy is in the trash, not deleted.
    let trashed = tree.trashed();
    assert_eq!(trashed.len(), 1, "{trashed:?}");
    let (path, content) = trashed.iter().next().unwrap();
    assert!(path.starts_with(".deletedByCineVault/b1-undo/"), "{path}");
    assert_eq!(content, "poster bytes");
    assert!(!tree.exists("Heat (1995)"));
}

#[test]
fn a_copy_edited_afterwards_is_not_discarded() {
    let tree = Tree::new();
    let items = copy_batch(&tree);
    let mut journal = MemoryJournal::new();
    run_with(&StdFs, &mut journal, ExecOptions::new("b1"), &items);
    fs::write(tree.root.join("Heat (1995)/folder.jpg"), "my own artwork").unwrap();

    let undone = undo(&tree, &mut journal, "b1");
    assert_eq!(undone.not_restorable(), 1);
    assert_eq!(tree.read("Heat (1995)/folder.jpg"), "my own artwork");
    assert!(tree.trashed().is_empty());
}

#[test]
fn a_copy_that_is_already_gone_is_not_a_problem() {
    let tree = Tree::new();
    let items = copy_batch(&tree);
    let mut journal = MemoryJournal::new();
    run_with(&StdFs, &mut journal, ExecOptions::new("b1"), &items);
    fs::remove_file(tree.root.join("Heat (1995)/folder.jpg")).unwrap();
    let undone = undo(&tree, &mut journal, "b1");
    assert!(undone.is_complete(), "{undone:?}");
    assert_eq!(tree.read("Old/Heat/poster.jpg"), "poster bytes");
}

#[test]
fn the_trash_of_a_copy_is_found_from_the_batch_trash_rows_without_roots() {
    let tree = Tree::new();
    tree.write("Old/Heat/heat.mkv", "video");
    tree.write("Old/Heat/old.jpg", "stale");
    tree.write("Old/Heat/poster.jpg", "poster bytes");
    let poster = PlannedFile {
        from: "Old/Heat/poster.jpg".into(),
        kind: MediaFileType::Poster,
        owner: crate::plan::Owner::Movie,
        fate: crate::plan::Fate::Carried {
            to: vec!["Heat/poster.jpg".into(), "Heat/folder.jpg".into()],
        },
    };
    let item = batch_item(
        &tree,
        "heat",
        "Old/Heat",
        "Heat",
        FolderChange::Rename,
        vec![
            carried("Old/Heat/heat.mkv", "Heat/Heat.mkv", VIDEO),
            super::tests::trash_file("Old/Heat/old.jpg", MediaFileType::Thumb),
            poster,
        ],
    );
    let mut journal = MemoryJournal::new();
    run_with(&StdFs, &mut journal, ExecOptions::new("b1"), &[item]);
    let undone = undo_batch(&mut journal, &StdFs, "b1", &UndoOptions::default()).unwrap();
    assert!(undone.is_complete(), "{undone:?}");
    assert_eq!(tree.trashed().len(), 1);

    // With no trash row and no roots, the copy cannot be put anywhere safe.
    let tree = Tree::new();
    let items = copy_batch(&tree);
    let mut journal = MemoryJournal::new();
    run_with(&StdFs, &mut journal, ExecOptions::new("b1"), &items);
    let undone = undo_batch(&mut journal, &StdFs, "b1", &UndoOptions::default()).unwrap();
    assert_eq!(undone.not_restorable(), 1);
    assert!(tree.trashed().is_empty());
}

// ---- idempotence and refusals ---------------------------------------------------------------

#[test]
fn a_second_undo_has_nothing_to_do() {
    let tree = Tree::new();
    let items = two_folder_batch(&tree);
    let mut journal = MemoryJournal::new();
    run_with(&StdFs, &mut journal, ExecOptions::new("b1"), &items);
    undo(&tree, &mut journal, "b1");
    let again = undo(&tree, &mut journal, "b1");
    assert!(again.items.is_empty());
    assert!(again.is_complete());
    assert!(undo(&tree, &mut journal, "no-such-batch").items.is_empty());
}

#[test]
fn rows_whose_original_is_back_already_count_as_restored() {
    // A crash between the move back and the journal row.
    let tree = Tree::new();
    tree.write("A/a.mkv", "aaa");
    let item = batch_item(
        &tree,
        "a",
        "A",
        "A",
        FolderChange::MoveFiles,
        vec![carried("A/a.mkv", "A/A1.mkv", VIDEO)],
    );
    let mut journal = MemoryJournal::new();
    run_with(&StdFs, &mut journal, ExecOptions::new("b1"), &[item]);
    fs::rename(tree.root.join("A/A1.mkv"), tree.root.join("A/a.mkv")).unwrap();

    let undone = undo(&tree, &mut journal, "b1");
    assert!(undone.is_complete(), "{undone:?}");
    assert_eq!(undone.items[0].already_restored, 1);
    assert_eq!(undone.items[0].restored, 0);
    assert_eq!(tree.read("A/a.mkv"), "aaa");
}

#[test]
fn a_batch_with_unsettled_rows_asks_for_recovery_first() {
    let tree = Tree::new();
    let mut journal = MemoryJournal::new();
    journal
        .begin(&JournalEntry {
            batch_id: "b1".into(),
            seq: 0,
            item_id: None,
            op: JournalOp::Move,
            from: "x".into(),
            to: "y".into(),
            is_dir: false,
            created_parents: Some(0),
        })
        .unwrap();
    let options = UndoOptions::default();
    let error = undo_batch(&mut journal, &StdFs, "b1", &options).unwrap_err();
    assert!(
        matches!(error, UndoError::Unsettled { rows: 1, .. }),
        "{error}"
    );
    let _ = tree;
}

#[test]
fn a_failed_item_of_the_batch_is_not_offered_to_undo() {
    let tree = Tree::new();
    tree.write("A/a.mkv", "a");
    tree.write("A/a.jpg", "art");
    let bad = batch_item(
        &tree,
        "bad",
        "A",
        "A",
        FolderChange::MoveFiles,
        vec![
            carried("A/a.mkv", "A/A1.mkv", VIDEO),
            // Copied twice, then a missing source fails the item and the copy is taken back.
            PlannedFile {
                from: "A/a.jpg".into(),
                kind: MediaFileType::Poster,
                owner: crate::plan::Owner::Movie,
                fate: crate::plan::Fate::Carried {
                    to: vec!["A/p.jpg".into(), "A/q.jpg".into()],
                },
            },
            carried("A/ghost.nfo", "A/A1.nfo", NFO),
        ],
    );
    let mut journal = MemoryJournal::new();
    let report = run_with(&StdFs, &mut journal, ExecOptions::new("b1"), &[bad]);
    assert_eq!(report.failed(), 1, "{report:?}");
    let before = tree.live();
    let undone = undo(&tree, &mut journal, "b1");
    assert!(undone.items.is_empty(), "{undone:?}");
    assert_eq!(tree.live(), before);
}

// ---- a disk failure in the middle ------------------------------------------------------------

/// Fails the nth rename from now on.
struct FailingRename<'a> {
    fail_at: usize,
    renames: &'a Cell<usize>,
}

impl Fs for FailingRename<'_> {
    fn meta(&self, p: &Path) -> io::Result<Option<Meta>> {
        StdFs.meta(p)
    }
    fn create_dir_all(&self, p: &Path) -> io::Result<()> {
        StdFs.create_dir_all(p)
    }
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        let n = self.renames.get();
        self.renames.set(n + 1);
        if n == self.fail_at {
            return Err(io::Error::other("disk on fire"));
        }
        StdFs.rename(from, to)
    }
    fn copy_file(&self, a: &Path, b: &Path) -> io::Result<()> {
        StdFs.copy_file(a, b)
    }
    fn same_content(&self, a: &Path, b: &Path) -> io::Result<bool> {
        StdFs.same_content(a, b)
    }
    fn remove_file(&self, p: &Path) -> io::Result<()> {
        StdFs.remove_file(p)
    }
    fn remove_dir(&self, p: &Path) -> io::Result<()> {
        StdFs.remove_dir(p)
    }
    fn list_dir(&self, p: &Path) -> io::Result<Vec<String>> {
        StdFs.list_dir(p)
    }
}

#[test]
fn a_disk_failure_in_the_middle_puts_the_item_back_as_organized() {
    for fail_at in 0..3 {
        let tree = Tree::new();
        tree.write("A/a.mkv", "aaa");
        tree.write("A/a.nfo", MANAGED_NFO);
        tree.write("A/a.jpg", "art");
        let item = batch_item(
            &tree,
            "a",
            "A",
            "A",
            FolderChange::MoveFiles,
            vec![
                carried("A/a.mkv", "A/A1.mkv", VIDEO),
                carried("A/a.nfo", "A/A1.nfo", NFO),
                carried("A/a.jpg", "A/A1.jpg", MediaFileType::Poster),
            ],
        );
        let mut journal = MemoryJournal::new();
        run_with(&StdFs, &mut journal, ExecOptions::new("b1"), &[item]);
        let organized = tree.live();

        let renames = Cell::new(0);
        let fs = FailingRename {
            fail_at,
            renames: &renames,
        };
        let options = UndoOptions::default();
        let undone = undo_batch(&mut journal, &fs, "b1", &options).unwrap();
        assert_eq!(undone.failed(), 1, "step {fail_at}: {undone:?}");
        assert!(
            matches!(&undone.items[0].status, UndoStatus::Failed(m) if m.contains("disk on fire"))
        );
        assert_eq!(tree.live(), organized, "step {fail_at}");
        assert!(
            journal
                .batch("b1")
                .unwrap()
                .iter()
                .all(|r| r.state == JournalState::Done),
            "step {fail_at}"
        );
        // And a healthy undo afterwards works.
        let again = undo(&tree, &mut journal, "b1");
        assert!(again.is_complete(), "{again:?}");
        assert!(tree.exists("A/a.mkv") && tree.exists("A/a.nfo") && tree.exists("A/a.jpg"));
    }
}

// ---- the database journal ---------------------------------------------------------------------

#[test]
fn undo_with_the_database_journal_points_the_library_back_without_a_rescan() {
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
    run_with(&StdFs, &mut journal, ExecOptions::new("db1"), &[item]);
    let moved = db_movie(&db, id);
    assert!(moved.core.path.ends_with("Heat (1995)"));

    let undone = undo(&tree, &mut journal, "db1");
    assert!(undone.is_complete(), "{undone:?}");
    let back = db_movie(&db, id);
    assert!(back.core.path.ends_with("Old\\Heat") || back.core.path.ends_with("Old/Heat"));
    let file = &back.core.media_files[0];
    assert_eq!(file.filename, "heat.mkv");
    assert!(Path::new(&file.directory).join(&file.filename).exists());
    assert_eq!(
        Path::new(&back.core.path),
        tree.root.join("Old").join("Heat")
    );
    assert!(
        journal
            .batch("db1")
            .unwrap()
            .iter()
            .all(|r| r.state == JournalState::Undone)
    );
}

#[test]
fn the_database_journal_lists_batches_newest_first_with_counts() {
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
    run_with(&StdFs, &mut journal, ExecOptions::new("first"), &[item]);
    let list = journal.batches().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].batch_id, "first");
    assert_eq!((list[0].done, list[0].undone, list[0].items), (2, 0, 1));
    assert!(list[0].created_ms > 0);

    undo(&tree, &mut journal, "first");
    let list = journal.batches().unwrap();
    assert_eq!((list[0].done, list[0].undone), (0, 2));
}

#[test]
fn the_memory_journal_lists_batches_newest_first() {
    let tree = Tree::new();
    tree.write("A/a.mkv", "a");
    tree.write("A/b.mkv", "b");
    let mut journal = MemoryJournal::new();
    for (batch, from, to) in [
        ("one", "A/a.mkv", "A/a1.mkv"),
        ("two", "A/b.mkv", "A/b1.mkv"),
    ] {
        let item = batch_item(
            &tree,
            batch,
            "A",
            "A",
            FolderChange::MoveFiles,
            vec![carried(from, to, VIDEO)],
        );
        run_with(&StdFs, &mut journal, ExecOptions::new(batch), &[item]);
    }
    let list = journal.batches().unwrap();
    let ids: Vec<&str> = list.iter().map(|b| b.batch_id.as_str()).collect();
    assert_eq!(ids, ["two", "one"]);
    assert_eq!((list[0].operations, list[0].done, list[0].items), (1, 1, 1));

    // Undo the newest batch, then the older one.
    undo(&tree, &mut journal, "two");
    undo(&tree, &mut journal, "one");
    assert!(tree.exists("A/a.mkv") && tree.exists("A/b.mkv"));
}
