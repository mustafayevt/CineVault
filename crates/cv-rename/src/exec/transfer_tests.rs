//! Tests of moving folders between data sources: the same volume, another volume,
//! refusals that leave everything in place, undo, and a crash at every point of a cross-volume
//! move.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use super::tests::{Crash, FaultFs};
use super::*;

/// Two data sources side by side in one temp folder.
struct Sources {
    _dir: TempDir,
    old: PathBuf,
    new: PathBuf,
}

impl Sources {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("movies");
        let new = dir.path().join("archive");
        fs::create_dir_all(&old).unwrap();
        fs::create_dir_all(&new).unwrap();
        Sources {
            _dir: dir,
            old,
            new,
        }
    }

    fn put(&self, rel: &str, content: &str) {
        let path = self.old.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    /// A movie folder with a video, an NFO, a poster and an extras sub-folder.
    fn heat(&self) {
        self.put("Heat (1995)/Heat (1995).mkv", "video bytes");
        self.put("Heat (1995)/Heat (1995).nfo", "<movie/>");
        self.put("Heat (1995)/poster.jpg", "poster");
        self.put("Heat (1995)/extras/making-of.mkv", "extra");
    }

    fn the_move(&self, name: &str) -> FolderMove {
        FolderMove {
            id: "1".to_owned(),
            from_root: self.old.clone(),
            from: self.old.join(name),
            to_root: self.new.clone(),
            to: self.new.join(name),
        }
    }
}

fn files_of(root: &Path) -> BTreeMap<String, String> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, String>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let rel = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                out.insert(rel, fs::read_to_string(&path).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

fn move_with(
    fs: &dyn Fs,
    journal: &mut dyn JournalStore,
    batch: &str,
    moves: &[FolderMove],
) -> BatchReport {
    Executor::new(fs, journal, ExecOptions::new(batch))
        .unwrap()
        .move_folders(moves)
        .unwrap()
}

fn undo(sources: &Sources, journal: &mut dyn JournalStore, fs: &dyn Fs, batch: &str) -> UndoReport {
    let options = UndoOptions {
        roots: vec![sources.old.clone(), sources.new.clone()],
        ..UndoOptions::default()
    };
    undo_batch(journal, fs, batch, &options).unwrap()
}

#[test]
fn on_the_same_volume_the_folder_is_renamed_into_the_other_data_source() {
    let sources = Sources::new();
    sources.heat();
    let before = files_of(&sources.old);
    let mut journal = MemoryJournal::new();
    let report = move_with(
        &StdFs,
        &mut journal,
        "b1",
        &[sources.the_move("Heat (1995)")],
    );

    assert_eq!(report.done(), 1, "{report:?}");
    assert_eq!(report.items[0].moved, 1);
    assert!(files_of(&sources.old).is_empty());
    let moved: BTreeMap<String, String> = files_of(&sources.new)
        .into_iter()
        .map(|(path, content)| (path.trim_start_matches("Heat (1995)/").to_owned(), content))
        .collect();
    let original: BTreeMap<String, String> = before
        .into_iter()
        .map(|(path, content)| (path.trim_start_matches("Heat (1995)/").to_owned(), content))
        .collect();
    assert_eq!(moved, original);

    let rows = journal.batch("b1").unwrap();
    assert_eq!(rows.len(), 1, "one row per folder: {rows:?}");
    assert_eq!(rows[0].state, JournalState::Done);
    assert!(rows[0].entry.is_dir);
}

#[test]
fn across_volumes_the_tree_is_copied_verified_and_the_original_removed() {
    let sources = Sources::new();
    sources.heat();
    sources.put("Action/Heat (1995)/Heat (1995).mkv", "bytes");
    let fs = FaultFs::across_volumes();
    let mut journal = MemoryJournal::new();
    let report = move_with(&fs, &mut journal, "b1", &[sources.the_move("Heat (1995)")]);

    assert_eq!(report.done(), 1, "{report:?}");
    let new = files_of(&sources.new);
    assert_eq!(new.len(), 4, "{new:?}");
    assert_eq!(new["Heat (1995)/extras/making-of.mkv"], "extra");
    assert_eq!(new["Heat (1995)/poster.jpg"], "poster");
    assert!(
        !sources.old.join("Heat (1995)").exists(),
        "the original is gone"
    );
    assert!(
        files_of(&sources.old).contains_key("Action/Heat (1995)/Heat (1995).mkv"),
        "another title is untouched"
    );
    assert!(
        !new.keys().any(|path| path.ends_with(".cvpart")),
        "no copy left: {new:?}"
    );
}

#[test]
fn a_destination_with_files_in_it_fails_the_item_and_changes_nothing() {
    let sources = Sources::new();
    sources.heat();
    fs::create_dir_all(sources.new.join("Heat (1995)")).unwrap();
    fs::write(sources.new.join("Heat (1995)/other.mkv"), "someone else").unwrap();
    let before = (files_of(&sources.old), files_of(&sources.new));
    let mut journal = MemoryJournal::new();
    let report = move_with(
        &StdFs,
        &mut journal,
        "b1",
        &[sources.the_move("Heat (1995)")],
    );

    assert_eq!(report.failed(), 1, "{report:?}");
    assert!(
        matches!(&report.items[0].status, ItemStatus::Failed(why) if why.contains("already holds files")),
        "{report:?}"
    );
    assert_eq!((files_of(&sources.old), files_of(&sources.new)), before);
    assert!(journal.all().is_empty(), "nothing was journaled");
}

#[test]
fn an_empty_destination_folder_is_not_in_the_way() {
    let sources = Sources::new();
    sources.heat();
    fs::create_dir_all(sources.new.join("Heat (1995)")).unwrap();
    let report = move_with(
        &StdFs,
        &mut MemoryJournal::new(),
        "b1",
        &[sources.the_move("Heat (1995)")],
    );
    assert_eq!(report.done(), 1, "{report:?}");
    assert!(sources.new.join("Heat (1995)/poster.jpg").exists());
}

#[test]
fn a_corrupt_copy_fails_the_item_and_leaves_the_original_whole() {
    let sources = Sources::new();
    sources.heat();
    let before = files_of(&sources.old);
    let fs = FaultFs::across_volumes();
    fs.corrupt_copies(true);
    let mut journal = MemoryJournal::new();
    let report = move_with(&fs, &mut journal, "b1", &[sources.the_move("Heat (1995)")]);

    assert_eq!(report.failed(), 1, "{report:?}");
    assert_eq!(files_of(&sources.old), before);
    assert!(files_of(&sources.new).is_empty(), "nothing is left over");
    assert!(!sources.new.join("Heat (1995).cvpart").exists());
    assert_eq!(journal.all()[0].state, JournalState::Failed);
}

#[test]
fn a_move_outside_the_data_sources_is_refused() {
    let sources = Sources::new();
    sources.heat();
    let mut the_move = sources.the_move("Heat (1995)");
    the_move.to = sources.new.parent().unwrap().join("elsewhere/Heat (1995)");
    let report = move_with(&StdFs, &mut MemoryJournal::new(), "b1", &[the_move]);
    assert_eq!(report.failed(), 1, "{report:?}");
    assert!(sources.old.join("Heat (1995)").exists());

    // The data source root itself is never moved.
    let mut root = sources.the_move("Heat (1995)");
    root.from = sources.old.clone();
    let report = move_with(&StdFs, &mut MemoryJournal::new(), "b2", &[root]);
    assert_eq!(report.failed(), 1, "{report:?}");
}

#[test]
fn a_missing_data_source_fails_the_item_alone() {
    let sources = Sources::new();
    sources.heat();
    sources.put("Ronin (1998)/Ronin (1998).mkv", "ronin");
    let mut gone = sources.the_move("Heat (1995)");
    gone.to_root = sources.new.join("unplugged");
    gone.to = gone.to_root.join("Heat (1995)");
    let mut journal = MemoryJournal::new();
    let mut ronin = sources.the_move("Ronin (1998)");
    ronin.id = "2".to_owned();
    let report = move_with(&StdFs, &mut journal, "b1", &[gone, ronin]);

    assert!(
        matches!(&report.items[0].status, ItemStatus::Failed(why) if why.contains("not available"))
    );
    assert_eq!(report.items[1].status, ItemStatus::Done);
    assert!(sources.old.join("Heat (1995)").exists());
    assert!(sources.new.join("Ronin (1998)").exists());
}

#[test]
fn the_empty_folders_the_move_leaves_are_removed_but_never_the_root() {
    let sources = Sources::new();
    sources.put("Action/Heat (1995)/Heat (1995).mkv", "bytes");
    let mut the_move = sources.the_move("Action/Heat (1995)");
    the_move.to = sources.new.join("Heat (1995)");
    let report = move_with(&StdFs, &mut MemoryJournal::new(), "b1", &[the_move]);
    assert_eq!(report.done(), 1, "{report:?}");
    assert!(
        !sources.old.join("Action").exists(),
        "the bucket folder is gone"
    );
    assert!(sources.old.exists(), "the data source stays");
}

#[test]
fn undo_takes_a_move_back_on_the_same_volume_and_across_volumes() {
    for across in [false, true] {
        let sources = Sources::new();
        sources.heat();
        let before = files_of(&sources.old);
        let fs = if across {
            FaultFs::across_volumes()
        } else {
            FaultFs::new()
        };
        let mut journal = MemoryJournal::new();
        let report = move_with(&fs, &mut journal, "b1", &[sources.the_move("Heat (1995)")]);
        assert_eq!(report.done(), 1, "{report:?}");
        assert!(files_of(&sources.old).is_empty());

        let undone = undo(&sources, &mut journal, &fs, "b1");
        assert!(undone.is_complete(), "across={across}: {undone:?}");
        assert_eq!(files_of(&sources.old), before, "across={across}");
        assert!(files_of(&sources.new).is_empty(), "across={across}");
    }
}

#[test]
fn undo_refuses_a_folder_that_was_taken_away_since() {
    let sources = Sources::new();
    sources.heat();
    let mut journal = MemoryJournal::new();
    move_with(
        &StdFs,
        &mut journal,
        "b1",
        &[sources.the_move("Heat (1995)")],
    );
    fs::remove_dir_all(sources.new.join("Heat (1995)")).unwrap();
    let undone = undo(&sources, &mut journal, &StdFs, "b1");
    assert!(!undone.is_complete(), "{undone:?}");
}

#[test]
fn a_crash_at_every_point_of_a_cross_volume_folder_move_leaves_a_consistent_state() {
    // Count the mutations of an uninterrupted run.
    let total = {
        let sources = Sources::new();
        sources.heat();
        let fs = FaultFs::across_volumes();
        let report = move_with(
            &fs,
            &mut MemoryJournal::new(),
            "count",
            &[sources.the_move("Heat (1995)")],
        );
        assert_eq!(report.failed(), 0, "{report:?}");
        fs.ops()
    };
    assert!(total > 6, "the move should do some work, did {total}");

    for at in 0..total {
        for after in [false, true] {
            let sources = Sources::new();
            sources.heat();
            let before = files_of(&sources.old);
            let journal = MemoryJournal::new();
            let fault = FaultFs::crashing(at, after, true);
            let mut running = journal.clone();
            let label = format!(
                "crash {} mutation {at}",
                if after { "after" } else { "before" }
            );
            let crashed = catch_unwind(AssertUnwindSafe(|| {
                move_with(
                    &fault,
                    &mut running,
                    "crash",
                    &[sources.the_move("Heat (1995)")],
                )
            }));
            let payload = crashed.expect_err("the run should have crashed");
            assert!(payload.downcast_ref::<Crash>().is_some(), "{label}");

            // Restart.
            let mut recovering = journal.clone();
            let recovery = recover(&mut recovering, &StdFs).unwrap();
            assert_eq!(
                recovery.needs_attention().count(),
                0,
                "{label}: {recovery:?}"
            );
            assert!(journal.pending().unwrap().is_empty(), "{label}");

            // Never lost, never half: the whole folder is at the old place or at the new one.
            let old = files_of(&sources.old);
            let new = files_of(&sources.new);
            let contents: BTreeSet<&String> = old.values().chain(new.values()).collect();
            for content in before.values() {
                assert!(contents.contains(content), "{label}: {content} was lost");
            }
            assert!(
                !new.keys().any(|path| path.contains(".cvpart")),
                "{label}: leftover {new:?}"
            );
            let at_old = old.len() == before.len() && new.is_empty();
            let at_new = old.is_empty() && new.len() == before.len();
            assert!(at_old || at_new, "{label}: old {old:?}, new {new:?}");

            // Run it again: it finishes.
            let report = move_with(
                &FaultFs::across_volumes(),
                &mut journal.clone(),
                "resume",
                &[sources.the_move("Heat (1995)")],
            );
            assert_eq!(report.failed(), 0, "{label}: {report:?}");
            assert!(files_of(&sources.old).is_empty(), "{label}");
            assert_eq!(files_of(&sources.new).len(), before.len(), "{label}");
        }
    }
}

// ---- the trash fallback of "delete with files" ----------------------------------------------

fn trash_request(sources: &Sources, paths: &[&str]) -> TrashRequest {
    TrashRequest {
        id: "1".to_owned(),
        root: sources.old.clone(),
        paths: paths.iter().map(|p| sources.old.join(p)).collect(),
    }
}

fn trash_with(
    fs: &dyn Fs,
    journal: &mut dyn JournalStore,
    batch: &str,
    requests: &[TrashRequest],
) -> BatchReport {
    Executor::new(fs, journal, ExecOptions::new(batch))
        .unwrap()
        .trash_paths(requests)
        .unwrap()
}

#[test]
fn a_folder_and_a_file_go_to_the_trash_folder_of_the_data_source_and_undo_brings_them_back() {
    let sources = Sources::new();
    sources.heat();
    sources.put("Ronin (1998)/Ronin (1998).mkv", "ronin");
    let before = files_of(&sources.old);
    let mut journal = MemoryJournal::new();
    let report = trash_with(
        &StdFs,
        &mut journal,
        "b1",
        &[trash_request(
            &sources,
            &["Heat (1995)", "Ronin (1998)/Ronin (1998).mkv"],
        )],
    );
    assert_eq!(report.done(), 1, "{report:?}");
    assert_eq!(report.items[0].trashed, 2);

    let after = files_of(&sources.old);
    assert!(!sources.old.join("Heat (1995)").exists());
    assert!(
        after.contains_key(".deletedByCineVault/b1/Heat (1995)/poster.jpg"),
        "{after:?}"
    );
    assert!(after.contains_key(".deletedByCineVault/b1/Ronin (1998)/Ronin (1998).mkv"));
    assert_eq!(after.len(), before.len(), "nothing was deleted for good");

    let undone = undo(&sources, &mut journal, &StdFs, "b1");
    assert!(undone.is_complete(), "{undone:?}");
    assert_eq!(files_of(&sources.old), before);
}

#[test]
fn the_trash_fallback_refuses_the_data_source_itself_and_paths_outside_it() {
    let sources = Sources::new();
    sources.heat();
    let outside = sources.new.join("elsewhere.mkv");
    fs::write(&outside, "x").unwrap();
    for bad in [sources.old.clone(), outside.clone()] {
        let request = TrashRequest {
            id: "1".to_owned(),
            root: sources.old.clone(),
            paths: vec![bad],
        };
        let report = trash_with(&StdFs, &mut MemoryJournal::new(), "b", &[request]);
        assert_eq!(report.failed(), 1, "{report:?}");
    }
    assert!(outside.exists());
    assert!(sources.old.join("Heat (1995)/poster.jpg").exists());
}

#[test]
fn a_path_that_is_already_gone_is_not_an_error() {
    let sources = Sources::new();
    sources.heat();
    let report = trash_with(
        &StdFs,
        &mut MemoryJournal::new(),
        "b1",
        &[trash_request(&sources, &["Heat (1995)/gone.nfo"])],
    );
    assert_eq!(report.done(), 1, "{report:?}");
    assert_eq!(report.items[0].already_applied, 1);
}
