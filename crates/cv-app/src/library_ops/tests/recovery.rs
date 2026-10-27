//! Tests of the recovery of unfinished moves and renames: a stop after 3 of 9 files
//! is resumed to completion and, separately, rolled back to the original state, with every
//! original intact in both.

use std::panic::AssertUnwindSafe;

use cv_db::repo::rename_journal as table;
use cv_rename::exec::{Completion, DbJournal, JournalEntry, JournalOp, JournalStore};

use super::moves::{FaultFs, Faults};
use super::*;
use crate::library_ops::{BatchKind, RecoveryService};

impl Lab {
    /// A movie folder of exactly nine files, on disk and in the library.
    fn nine_file_movie(&self, source: &DataSource, name: &str) -> ItemId {
        let id = self.movie(source, name);
        let folder = Path::new(&source.path).join(name);
        fs::write(
            folder.join(format!("{name}.mkv")),
            vec![7_u8; 2 * 1024 * 1024],
        )
        .unwrap();
        for (rel, content) in [
            (format!("{name}.en.srt"), "subtitle"),
            (format!("{name}-trailer.mkv"), "trailer"),
            ("theme.mp3".to_owned(), "theme"),
            (".hidden".to_owned(), "h"),
            ("extras/making-of.mkv".to_owned(), "making of"),
            ("extras/deleted/scene.mkv".to_owned(), "scene"),
        ] {
            Lab::put(&folder, &rel, content);
        }
        id
    }

    fn recovery(&self, disk: &Arc<FaultFs>) -> RecoveryService {
        RecoveryService::new(Arc::clone(&self.db))
            .with_disk(Arc::new(super::moves::SharedFaultFs(Arc::clone(disk))))
    }

    /// Runs a cross-volume move that is killed right after the staging folder became the
    /// destination, before the journal and the library heard about it.
    fn killed_at_the_commit(&self, id: ItemId) -> Arc<FaultFs> {
        let disk = FaultFs::new(Faults {
            kill_after_stage_rename: true,
            ..Faults::default()
        });
        let service = self.cross_with(&disk);
        let killed = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let _ = service.apply(&[id], self.archive.id, &mut no_progress());
        }));
        assert!(killed.is_err(), "the run was killed");
        disk.heal();
        disk
    }

    /// Runs a cross-volume move that is killed when the fourth original goes to the trash.
    fn killed_in_the_trash_step(&self, id: ItemId) -> Arc<FaultFs> {
        let disk = FaultFs::new(Faults {
            kill_at_trash: Some(4),
            ..Faults::default()
        });
        let service = self.cross_with(&disk);
        let killed = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let _ = service.apply(&[id], self.archive.id, &mut no_progress());
        }));
        assert!(killed.is_err(), "the run was killed");
        disk.heal();
        disk
    }

    /// Runs a cross-volume move that is killed at the fourth file.
    fn killed_move(&self, id: ItemId) -> Arc<FaultFs> {
        let disk = FaultFs::new(Faults {
            kill_at_part_rename: Some(4),
            ..Faults::default()
        });
        let service = self.cross_with(&disk);
        let killed = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let _ = service.apply(&[id], self.archive.id, &mut no_progress());
        }));
        assert!(killed.is_err(), "the run was killed");
        disk.heal();
        disk
    }
}

fn files_below(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
        let Ok(read) = fs::read_dir(dir) else { return };
        for entry in read.flatten() {
            if entry.path().is_dir() {
                walk(base, &entry.path(), out);
            } else {
                out.push(
                    entry
                        .path()
                        .strip_prefix(base)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
    }
    walk(dir, dir, &mut out);
    out.sort();
    out
}

fn bytes_below(dir: &Path) -> Vec<(String, Vec<u8>)> {
    files_below(dir)
        .into_iter()
        .map(|rel| {
            let bytes = fs::read(dir.join(&rel)).unwrap();
            (rel, bytes)
        })
        .collect()
}

#[test]
fn a_move_stopped_after_three_of_nine_files_is_found_and_resumed_to_completion() {
    let lab = Lab::new();
    let heat = lab.nine_file_movie(&lab.movies, "Heat (1995)");
    let old_folder = Path::new(&lab.movies.path).join("Heat (1995)");
    let before = bytes_below(&old_folder);
    assert_eq!(before.len(), 9, "{:?}", files_below(&old_folder));
    let disk = lab.killed_move(heat);
    assert_eq!(bytes_below(&old_folder), before, "every original is intact");
    assert_eq!(lab.item(heat).core.data_source, lab.movies.path);

    // Start-up recovery settles renames and leaves the move to the user.
    let planned_before = lab.db.read(table::pending_rows).unwrap().len();
    assert!(planned_before > 0);
    crate::organize::recover_interrupted_batches(&lab.db).unwrap();
    assert_eq!(
        lab.db.read(table::pending_rows).unwrap().len(),
        planned_before,
        "recovery does not touch the rows of a move"
    );

    // It is found before any new file work, with how far it got.
    let service = lab.recovery(&disk);
    let found = service.find().unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    let batch = &found[0];
    assert_eq!(batch.kind, BatchKind::Move);
    assert_eq!((batch.files_done, batch.files_total), (3, 9));
    assert_eq!(batch.titles, ["Heat"]);
    assert!(batch.reason.contains("closed"), "{}", batch.reason);
    let notice = batch.notice();
    assert_eq!(notice.batch_id.as_deref(), Some(batch.batch_id.as_str()));
    assert_eq!(
        notice.action,
        Some(crate::notices::NoticeAction::ResolveUnfinished)
    );

    // An unfinished move cannot be undone: resume it or roll it back first.
    let refused = OrganizeService::new(Arc::clone(&lab.db)).undo(&batch.batch_id);
    assert!(refused.is_err());

    // Resume checks the three copies again, copies the other six and puts the title in place.
    let mut seen = Vec::new();
    let outcome = service
        .resume(&batch.batch_id, None, &mut |p| seen.push(p.clone()))
        .unwrap();
    assert_eq!(outcome.completed, 1, "{outcome:?}");
    assert_eq!(outcome.stopped, None);

    let new_folder = Path::new(&lab.archive.path).join("Heat (1995)");
    assert_eq!(bytes_below(&new_folder), before, "all nine files arrived");
    assert!(!old_folder.exists());
    let trash = Path::new(&lab.movies.path)
        .join(".deletedByCineVault")
        .join(&batch.batch_id)
        .join("Heat (1995)");
    assert_eq!(
        bytes_below(&trash),
        before,
        "the originals are in the trash folder"
    );
    let item = lab.item(heat);
    assert_eq!(item.core.data_source, lab.archive.path);
    assert_eq!(PathBuf::from(&item.core.path), new_folder);
    assert!(lab.exists(heat));
    assert!(
        lab.journal(&batch.batch_id)
            .iter()
            .all(|r| r.state == "done"),
        "{:?}",
        lab.journal(&batch.batch_id)
    );
    assert!(service.find().unwrap().is_empty());
    assert!(seen.iter().any(|p| p.bytes_total > 0));
    let leftovers: Vec<String> = files_below(Path::new(&lab.archive.path))
        .into_iter()
        .filter(|f| f.contains(".cvpart") || f.contains(".cvmove-"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn resume_checks_the_copies_that_were_done_and_copies_a_damaged_one_again() {
    let lab = Lab::new();
    let heat = lab.nine_file_movie(&lab.movies, "Heat (1995)");
    let old_folder = Path::new(&lab.movies.path).join("Heat (1995)");
    let before = bytes_below(&old_folder);
    let disk = lab.killed_move(heat);
    // Damage one of the three verified copies in the staging folder.
    let stage = fs::read_dir(&lab.archive.path)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".cvmove-")
        })
        .expect("the staging folder");
    let damaged = files_below(&stage)
        .into_iter()
        .find(|f| !f.ends_with(".cvpart"))
        .expect("a verified copy");
    let target = stage.join(&damaged);
    let mut bytes = fs::read(&target).unwrap();
    bytes[0] ^= 0xFF;
    fs::write(&target, bytes).unwrap();

    let service = lab.recovery(&disk);
    let batch = service.find().unwrap().remove(0);
    let outcome = service
        .resume(&batch.batch_id, None, &mut no_progress())
        .unwrap();

    assert_eq!(outcome.completed, 1, "{outcome:?}");
    let new_folder = Path::new(&lab.archive.path).join("Heat (1995)");
    assert_eq!(
        bytes_below(&new_folder),
        before,
        "the damaged copy was made again"
    );
}

#[test]
fn rolling_back_removes_only_what_the_move_made_and_leaves_the_originals() {
    let lab = Lab::new();
    let heat = lab.nine_file_movie(&lab.movies, "Heat (1995)");
    let old_folder = Path::new(&lab.movies.path).join("Heat (1995)");
    let before = bytes_below(&old_folder);
    let disk = lab.killed_move(heat);
    // A file of the user's in the destination source must survive.
    let theirs = Path::new(&lab.archive.path).join("notes.txt");
    fs::write(&theirs, "mine").unwrap();
    let service = lab.recovery(&disk);
    let batch = service.find().unwrap().remove(0);

    let outcome = service.roll_back(&batch.batch_id).unwrap();

    assert!(outcome.problems.is_empty(), "{outcome:?}");
    assert_eq!(
        outcome.removed, 3,
        "the three copies; the half-written part file too"
    );
    assert_eq!(bytes_below(&old_folder), before, "every original is intact");
    assert_eq!(lab.item(heat).core.data_source, lab.movies.path);
    assert_eq!(files_below(Path::new(&lab.archive.path)), ["notes.txt"]);
    assert!(
        fs::read_dir(&lab.archive.path)
            .unwrap()
            .flatten()
            .all(|e| e.file_name() == "notes.txt"),
        "no staging folder is left"
    );
    assert_eq!(fs::read_to_string(&theirs).unwrap(), "mine");
    assert!(
        lab.journal(&batch.batch_id)
            .iter()
            .all(|r| r.state != "planned" && r.state != "done"),
        "{:?}",
        lab.journal(&batch.batch_id)
    );
    assert!(service.find().unwrap().is_empty());
    assert!(
        !Path::new(&lab.movies.path)
            .join(".deletedByCineVault")
            .exists(),
        "nothing of the originals was moved"
    );
}

#[test]
fn a_full_destination_is_resumed_once_there_is_room() {
    let lab = Lab::new();
    let heat = lab.nine_file_movie(&lab.movies, "Heat (1995)");
    let old_folder = Path::new(&lab.movies.path).join("Heat (1995)");
    let before = bytes_below(&old_folder);
    let disk = FaultFs::new(Faults {
        disk_full_after: Some(2 * 1024 * 1024 + 8),
        ..Faults::default()
    });
    let applied = lab
        .cross_with(&disk)
        .apply(&[heat], lab.archive.id, &mut no_progress())
        .unwrap();
    assert_eq!(
        applied.items[0].result,
        MoveResult::Stopped("Destination is full".to_owned())
    );

    let service = lab.recovery(&disk);
    let found = service.find().unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].reason, "Destination is full.");

    // Still full: the resume stops again with the same plain reason and removes nothing.
    let again = service
        .resume(&found[0].batch_id, None, &mut no_progress())
        .unwrap();
    assert_eq!(again.completed, 0);
    assert_eq!(again.stopped.as_deref(), Some("Destination is full"));
    assert_eq!(bytes_below(&old_folder), before);

    // Room is made.
    disk.heal();
    let done = service
        .resume(&found[0].batch_id, None, &mut no_progress())
        .unwrap();
    assert_eq!(done.completed, 1, "{done:?}");
    assert_eq!(
        bytes_below(&Path::new(&lab.archive.path).join("Heat (1995)")),
        before
    );
    assert!(service.find().unwrap().is_empty());
}

#[test]
fn a_full_destination_can_be_rolled_back() {
    let lab = Lab::new();
    let heat = lab.nine_file_movie(&lab.movies, "Heat (1995)");
    let old_folder = Path::new(&lab.movies.path).join("Heat (1995)");
    let before = bytes_below(&old_folder);
    let disk = FaultFs::new(Faults {
        disk_full_after: Some(2 * 1024 * 1024 + 8),
        ..Faults::default()
    });
    lab.cross_with(&disk)
        .apply(&[heat], lab.archive.id, &mut no_progress())
        .unwrap();
    let service = lab.recovery(&disk);
    let batch = service.find().unwrap().remove(0);

    service.roll_back(&batch.batch_id).unwrap();

    assert_eq!(bytes_below(&old_folder), before);
    assert!(files_below(Path::new(&lab.archive.path)).is_empty());
    assert!(service.find().unwrap().is_empty());
}

#[test]
fn a_second_title_is_resumed_after_the_first_finished() {
    let lab = Lab::new();
    let heat = lab.nine_file_movie(&lab.movies, "Heat (1995)");
    let ronin = lab.movie(&lab.movies, "Ronin (1998)");
    // The first title is fine; the destination fills up during the second.
    let disk = FaultFs::new(Faults {
        disk_full_after: Some(2 * 1024 * 1024 + 44 + 6),
        ..Faults::default()
    });
    let applied = lab
        .cross_with(&disk)
        .apply(&[heat, ronin], lab.archive.id, &mut no_progress())
        .unwrap();
    assert_eq!(applied.moved(), 1, "{applied:?}");
    assert!(matches!(
        applied.items.iter().find(|i| i.id == ronin).unwrap().result,
        MoveResult::Stopped(_)
    ));
    disk.heal();
    let service = lab.recovery(&disk);
    let found = service.find().unwrap();
    assert_eq!(found[0].items, 1, "only the title with work left");
    assert_eq!(found[0].titles, ["Ronin"]);
    let batch = found[0].batch_id.clone();
    let first = entry_of(&lab, &batch).expect("the run recorded the title it moved");
    assert_eq!(first.titles_total, 1);
    let done = service.resume(&batch, None, &mut no_progress()).unwrap();
    assert_eq!(done.completed, 1);
    assert_eq!(lab.item(heat).core.data_source, lab.archive.path);
    assert_eq!(lab.item(ronin).core.data_source, lab.archive.path);
    // The resumed title joins the same History entry (safety review finding 5).
    let entry = entry_of(&lab, &batch).unwrap();
    assert_eq!((entry.id, entry.titles_total), (first.id, 2), "{entry:?}");
    assert!(entry.summary.starts_with("Moved 2 titles"), "{entry:?}");
}

#[test]
fn a_kill_between_the_staging_folder_rename_and_the_journal_is_resumed_to_completion() {
    let lab = Lab::new();
    let heat = lab.nine_file_movie(&lab.movies, "Heat (1995)");
    let old_folder = Path::new(&lab.movies.path).join("Heat (1995)");
    let before = bytes_below(&old_folder);
    let disk = lab.killed_at_the_commit(heat);
    // The copies are in the destination; the library still points at the originals.
    let new_folder = Path::new(&lab.archive.path).join("Heat (1995)");
    assert_eq!(bytes_below(&new_folder), before);
    assert_eq!(bytes_below(&old_folder), before);
    assert_eq!(lab.item(heat).core.data_source, lab.movies.path);

    let service = lab.recovery(&disk);
    let batch = service.find().unwrap().remove(0);
    assert_eq!((batch.files_done, batch.files_total), (9, 9));
    let outcome = service
        .resume(&batch.batch_id, None, &mut no_progress())
        .unwrap();

    assert_eq!(outcome.completed, 1, "{outcome:?}");
    assert_eq!(outcome.stopped, None);
    assert_eq!(bytes_below(&new_folder), before);
    assert!(!old_folder.exists());
    assert_eq!(lab.item(heat).core.data_source, lab.archive.path);
    assert_eq!(
        files_below(Path::new(&lab.archive.path))
            .into_iter()
            .filter(|f| f.contains(".cvmove-"))
            .count(),
        0,
        "no second staging folder was made"
    );
    assert!(service.find().unwrap().is_empty());
}

#[test]
fn a_kill_between_the_staging_folder_rename_and_the_journal_can_be_rolled_back() {
    let lab = Lab::new();
    let heat = lab.nine_file_movie(&lab.movies, "Heat (1995)");
    let old_folder = Path::new(&lab.movies.path).join("Heat (1995)");
    let before = bytes_below(&old_folder);
    let disk = lab.killed_at_the_commit(heat);
    let service = lab.recovery(&disk);
    let batch = service.find().unwrap().remove(0);

    let outcome = service.roll_back(&batch.batch_id).unwrap();

    assert!(outcome.problems.is_empty(), "{outcome:?}");
    assert_eq!(bytes_below(&old_folder), before, "every original is intact");
    assert!(
        files_below(Path::new(&lab.archive.path)).is_empty(),
        "{:?}",
        files_below(Path::new(&lab.archive.path))
    );
    assert!(!Path::new(&lab.archive.path).join("Heat (1995)").exists());
    assert_eq!(lab.item(heat).core.data_source, lab.movies.path);
    assert!(service.find().unwrap().is_empty());
}

#[test]
fn a_file_somebody_else_put_in_the_destination_is_never_removed_by_a_roll_back() {
    let lab = Lab::new();
    let heat = lab.nine_file_movie(&lab.movies, "Heat (1995)");
    let disk = lab.killed_at_the_commit(heat);
    // After the crash a person drops a file into the new folder.
    let new_folder = Path::new(&lab.archive.path).join("Heat (1995)");
    fs::write(new_folder.join("my-notes.txt"), "keep me").unwrap();
    let service = lab.recovery(&disk);
    let batch = service.find().unwrap().remove(0);

    service.roll_back(&batch.batch_id).unwrap();

    assert_eq!(files_below(&new_folder), ["my-notes.txt"]);
    assert_eq!(
        fs::read_to_string(new_folder.join("my-notes.txt")).unwrap(),
        "keep me"
    );
}

#[test]
fn a_kill_while_the_originals_go_to_the_trash_folder_is_resumed_to_completion() {
    let lab = Lab::new();
    let heat = lab.nine_file_movie(&lab.movies, "Heat (1995)");
    let old_folder = Path::new(&lab.movies.path).join("Heat (1995)");
    let before = bytes_below(&old_folder);
    let disk = lab.killed_in_the_trash_step(heat);
    // The title is whole in the new place; three originals went to the trash folder.
    assert_eq!(lab.item(heat).core.data_source, lab.archive.path);
    let new_folder = Path::new(&lab.archive.path).join("Heat (1995)");
    assert_eq!(bytes_below(&new_folder), before);
    assert_eq!(files_below(&old_folder).len(), 6);

    let service = lab.recovery(&disk);
    let batch = service.find().unwrap().remove(0);
    let outcome = service
        .resume(&batch.batch_id, None, &mut no_progress())
        .unwrap();

    assert_eq!(outcome.completed, 1, "{outcome:?}");
    assert!(!old_folder.exists(), "nothing is left in the old folder");
    let trash = Path::new(&lab.movies.path)
        .join(".deletedByCineVault")
        .join(&batch.batch_id)
        .join("Heat (1995)");
    assert_eq!(
        bytes_below(&trash),
        before,
        "every original is in the trash folder"
    );
    assert!(
        lab.journal(&batch.batch_id)
            .iter()
            .all(|r| r.state == "done"),
        "{:?}",
        lab.journal(&batch.batch_id)
    );
}

#[test]
fn a_kill_while_the_originals_go_to_the_trash_folder_can_be_rolled_back() {
    let lab = Lab::new();
    let heat = lab.nine_file_movie(&lab.movies, "Heat (1995)");
    let old_folder = Path::new(&lab.movies.path).join("Heat (1995)");
    let before = bytes_below(&old_folder);
    let disk = lab.killed_in_the_trash_step(heat);
    let service = lab.recovery(&disk);
    let batch = service.find().unwrap().remove(0);

    let outcome = service.roll_back(&batch.batch_id).unwrap();

    assert!(outcome.problems.is_empty(), "{outcome:?}");
    assert_eq!(
        outcome.restored, 3,
        "the three originals that were trashed came back"
    );
    assert_eq!(bytes_below(&old_folder), before, "every original is back");
    assert_eq!(lab.item(heat).core.data_source, lab.movies.path);
    // The title was whole in the new place: its copies are kept in the trash folder there,
    // never removed for good (safety review 14.3).
    assert!(!Path::new(&lab.archive.path).join("Heat (1995)").exists());
    let kept = Path::new(&lab.archive.path)
        .join(".deletedByCineVault")
        .join(format!("{}-undo", batch.batch_id))
        .join("Heat (1995)");
    assert_eq!(bytes_below(&kept), before);
    assert!(service.find().unwrap().is_empty());
}

#[test]
fn a_journal_cut_short_while_it_was_written_changes_nothing_and_is_closed() {
    let lab = Lab::new();
    let heat = lab.movie(&lab.movies, "Heat (1995)");
    // Only the copy rows reached the journal before the kill: no folder row, no file touched.
    let mut journal = DbJournal::new(Arc::clone(&lab.db));
    let batch = "move-1700000000000-9".to_owned();
    for (seq, name) in [(1, "Heat (1995).mkv"), (2, "poster.jpg")] {
        journal
            .begin(&JournalEntry {
                batch_id: batch.clone(),
                seq,
                item_id: Some(heat.0.to_string()),
                op: JournalOp::Copy,
                from: Path::new(&lab.movies.path)
                    .join("Heat (1995)")
                    .join(name)
                    .to_string_lossy()
                    .into_owned(),
                to: Path::new(&lab.archive.path)
                    .join(".cvmove-x-1")
                    .join(name)
                    .to_string_lossy()
                    .into_owned(),
                is_dir: false,
                created_parents: None,
            })
            .unwrap();
    }
    let service = RecoveryService::new(Arc::clone(&lab.db));
    assert_eq!(service.find().unwrap().len(), 1);

    service.roll_back(&batch).unwrap();

    assert!(service.find().unwrap().is_empty());
    assert!(
        Path::new(&lab.movies.path)
            .join("Heat (1995)/poster.jpg")
            .is_file()
    );
    assert!(files_below(Path::new(&lab.archive.path)).is_empty());
}

// ---- renames ---------------------------------------------------------------------------------------

/// A rename batch a crash cut short: three steps were written ahead; the first two did not
/// happen, the third did.
fn interrupted_rename(lab: &Lab) -> (String, PathBuf) {
    let dir = Path::new(&lab.movies.path).join("Rename");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("a.mkv"), "first").unwrap();
    fs::write(dir.join("c.mkv"), "second").unwrap();
    fs::write(dir.join("e.mkv"), "third").unwrap();
    let mut journal = DbJournal::new(Arc::clone(&lab.db));
    let batch = "1700000000000-7".to_owned();
    let entry = |seq: u32, from: &str, to: &str| JournalEntry {
        batch_id: batch.clone(),
        seq,
        item_id: None,
        op: JournalOp::Move,
        from: dir.join(from).to_string_lossy().into_owned(),
        to: dir.join(to).to_string_lossy().into_owned(),
        is_dir: false,
        created_parents: None,
    };
    let rows = [
        entry(1, "a.mkv", "b.mkv"),
        entry(2, "c.mkv", "d.mkv"),
        entry(3, "e.mkv", "f.mkv"),
    ];
    for row in &rows {
        journal.begin(row).unwrap();
    }
    // The third step happened before the crash.
    fs::rename(dir.join("e.mkv"), dir.join("f.mkv")).unwrap();
    (batch, dir)
}

#[test]
fn an_interrupted_rename_is_settled_at_startup_then_resumed() {
    let lab = Lab::new();
    let (batch, dir) = interrupted_rename(&lab);

    let report = crate::organize::recover_interrupted_batches(&lab.db).unwrap();
    assert_eq!(report.rows.len(), 3);

    let service = RecoveryService::new(Arc::clone(&lab.db));
    let found = service.find().unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].kind, BatchKind::Rename);
    assert_eq!(found[0].batch_id, batch);

    let outcome = service.resume(&batch, None, &mut no_progress()).unwrap();
    assert_eq!(outcome.completed, 2, "{outcome:?}");
    assert_eq!(
        files_below(&dir),
        ["b.mkv", "d.mkv", "f.mkv"],
        "the two steps that never started were run"
    );
    assert_eq!(fs::read_to_string(dir.join("b.mkv")).unwrap(), "first");
    assert!(service.find().unwrap().is_empty());
    // The resumed steps are rows of the same batch, so one undo takes everything back.
    OrganizeService::new(Arc::clone(&lab.db))
        .undo(&batch)
        .unwrap();
    assert_eq!(files_below(&dir), ["a.mkv", "c.mkv", "e.mkv"]);
}

#[test]
fn an_interrupted_rename_can_be_rolled_back_to_the_old_names() {
    let lab = Lab::new();
    let (batch, dir) = interrupted_rename(&lab);
    crate::organize::recover_interrupted_batches(&lab.db).unwrap();
    let service = RecoveryService::new(Arc::clone(&lab.db));

    let outcome = service.roll_back(&batch).unwrap();

    assert!(outcome.problems.is_empty(), "{outcome:?}");
    assert_eq!(files_below(&dir), ["a.mkv", "c.mkv", "e.mkv"]);
    assert_eq!(fs::read_to_string(dir.join("e.mkv")).unwrap(), "third");
    assert!(service.find().unwrap().is_empty());
}

#[test]
fn a_finished_batch_is_not_offered_and_a_wrong_id_is_refused() {
    let lab = Lab::new();
    let heat = lab.movie(&lab.movies, "Heat (1995)");
    let applied = lab
        .cross()
        .apply(&[heat], lab.archive.id, &mut no_progress())
        .unwrap();
    let service = RecoveryService::new(Arc::clone(&lab.db));
    assert!(service.find().unwrap().is_empty());
    let batch = applied.batch_id.unwrap();
    assert!(service.resume(&batch, None, &mut no_progress()).is_err());
    assert!(service.roll_back(&batch).is_err());
    assert!(service.roll_back("nope").is_err());
    let _ = Completion::Undone;
}

/// A move with the bin that is stopped before its commit is rolled
/// back as before; nothing reached the bin.
#[test]
fn a_move_with_the_bin_stopped_before_the_commit_rolls_back_and_leaves_the_bin_alone() {
    let lab = Lab::new();
    let bin = lab.bin();
    let heat = lab.nine_file_movie(&lab.movies, "Heat (1995)");
    let old_folder = Path::new(&lab.movies.path).join("Heat (1995)");
    let before = bytes_below(&old_folder);
    let disk = FaultFs::new(Faults {
        kill_at_part_rename: Some(4),
        ..Faults::default()
    });
    let service = lab
        .cross_with(&disk)
        .with_bin(Arc::clone(&bin) as Arc<dyn SystemBin>);
    let killed = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let _ = service.apply(&[heat], lab.archive.id, &mut no_progress());
    }));
    assert!(killed.is_err());
    disk.heal();

    let recovery = lab.recovery(&disk);
    let found = recovery.find().unwrap();
    assert_eq!(found.len(), 1);
    recovery.roll_back(&found[0].batch_id).unwrap();

    assert_eq!(bytes_below(&old_folder), before, "every original is intact");
    assert!(bin.names().is_empty());
    assert_eq!(lab.item(heat).core.data_source, lab.movies.path);
}

// ---- History entries of resumed and unrecorded batches (safety review finding 5) -------------

fn entry_of(lab: &Lab, batch: &str) -> Option<crate::history::HistoryEntry> {
    let history = crate::history::HistoryService::new(Arc::clone(&lab.db));
    let id = history.entry_for_batch(batch).unwrap()?;
    Some(history.entry(id).unwrap())
}

#[test]
fn a_move_finished_by_resume_gets_a_history_entry_that_undoes_it() {
    let lab = Lab::new();
    let heat = lab.nine_file_movie(&lab.movies, "Heat (1995)");
    let old_folder = Path::new(&lab.movies.path).join("Heat (1995)");
    let before = bytes_below(&old_folder);
    let disk = lab.killed_move(heat);
    let service = lab.recovery(&disk);
    let batch = service.find().unwrap()[0].batch_id.clone();
    assert!(
        entry_of(&lab, &batch).is_none(),
        "the killed run wrote none"
    );

    service.resume(&batch, None, &mut no_progress()).unwrap();

    let entry = entry_of(&lab, &batch).expect("Resume writes the entry");
    assert_eq!(entry.kind, crate::history::HistoryKind::Move);
    assert!(
        entry.summary.starts_with("Moved Heat (1995) to "),
        "{entry:?}"
    );
    assert_eq!(entry.titles_total, 1);
    assert!(entry.detail.contains("9 files"), "{}", entry.detail);
    // Start-up finds nothing more to write.
    assert_eq!(service.record_unrecorded().unwrap(), 0);

    let report = crate::history::HistoryService::new(Arc::clone(&lab.db))
        .undo(entry.id)
        .unwrap();
    assert_eq!(report.undone, [heat], "{report:?}");
    assert_eq!(
        bytes_below(&old_folder),
        before,
        "History took the move back"
    );
    assert_eq!(lab.item(heat).core.data_source, lab.movies.path);
}

#[test]
fn a_finished_move_whose_entry_was_never_written_gets_one_at_start_up() {
    let lab = Lab::new();
    let heat = lab.movie(&lab.movies, "Heat (1995)");
    let applied = lab
        .cross()
        .apply(&[heat], lab.archive.id, &mut no_progress())
        .unwrap();
    let batch = applied.batch_id.unwrap();
    // The app stopped between the end of the batch and its History entry.
    lab.db
        .write(|tx| {
            tx.execute("DELETE FROM history_entry", [])?;
            Ok(())
        })
        .unwrap();
    assert!(entry_of(&lab, &batch).is_none());

    let service = RecoveryService::new(Arc::clone(&lab.db));
    assert_eq!(service.record_unrecorded().unwrap(), 1);
    let entry = entry_of(&lab, &batch).expect("start-up wrote it");
    assert_eq!(entry.kind, crate::history::HistoryKind::Move);
    assert_eq!(entry.titles_total, 1);
    assert_eq!(service.record_unrecorded().unwrap(), 0, "only once");

    crate::history::HistoryService::new(Arc::clone(&lab.db))
        .undo(entry.id)
        .unwrap();
    assert_eq!(lab.item(heat).core.data_source, lab.movies.path);
}

#[test]
fn a_rolled_back_move_and_a_batch_without_plans_get_no_entry() {
    let lab = Lab::new();
    let heat = lab.nine_file_movie(&lab.movies, "Heat (1995)");
    let disk = lab.killed_move(heat);
    let service = lab.recovery(&disk);
    let batch = service.find().unwrap()[0].batch_id.clone();
    service.roll_back(&batch).unwrap();
    // The rename batch of `interrupted_rename` has no plans (no title marks).
    let (rename, _) = interrupted_rename(&lab);
    crate::organize::recover_interrupted_batches(&lab.db).unwrap();
    service.resume(&rename, None, &mut no_progress()).unwrap();

    assert_eq!(service.record_unrecorded().unwrap(), 0);
    assert!(entry_of(&lab, &batch).is_none());
    assert!(entry_of(&lab, &rename).is_none());
}

#[test]
fn a_copy_back_is_a_move_batch_without_a_history_entry() {
    let id = crate::library_ops::move_engine::new_move_back_batch_id();
    assert!(crate::library_ops::is_move_batch(&id));
    assert!(!crate::library_ops::move_engine::wants_history(&id));
    assert!(crate::library_ops::move_engine::wants_history(
        &crate::library_ops::new_move_batch_id()
    ));
}
