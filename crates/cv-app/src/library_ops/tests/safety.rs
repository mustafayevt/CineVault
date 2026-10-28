//! Findings of the data-safety review of moves and recovery. Each
//! test names a way a move, its recovery or its undo could lose, leave behind or wrongly point
//! at a file, and checks that it does not.

use std::panic::AssertUnwindSafe;
use std::time::{Duration, SystemTime};

use cv_db::repo::rename_journal as table;
use cv_rename::exec::{DbJournal, JournalEntry, JournalOp, JournalStore};

use super::moves::{FaultFs, Faults, SharedFaultFs};
use super::*;
use crate::library_ops::RecoveryService;
use crate::library_ops::move_engine::{MoveEngine, MoveJob};
use crate::library_ops::move_plan::enumerate;

/// Every file below `dir` with its bytes, relative to `dir` with `/`.
fn files(dir: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        let Ok(read) = fs::read_dir(dir) else { return };
        for entry in read.flatten() {
            let path = entry.path();
            if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
                continue;
            }
            if path.is_dir() {
                walk(base, &path, out);
            } else {
                let rel = path
                    .strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push((rel, fs::read(&path).unwrap()));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

fn names(dir: &Path) -> Vec<String> {
    files(dir).into_iter().map(|(name, _)| name).collect()
}

impl Lab {
    fn recovery_on(&self, disk: &Arc<FaultFs>) -> RecoveryService {
        RecoveryService::new(Arc::clone(&self.db))
            .with_disk(Arc::new(SharedFaultFs(Arc::clone(disk))))
    }

    /// A cross-volume move of `id` on `disk` that is expected to panic (a killed process).
    fn killed(&self, id: ItemId, disk: &Arc<FaultFs>) {
        let service = self.cross_with(disk);
        let killed = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let _ = service.apply(&[id], self.archive.id, &mut no_progress());
        }));
        assert!(killed.is_err(), "the run was killed");
        disk.heal();
    }

    fn only_unfinished(&self, disk: &Arc<FaultFs>) -> String {
        let found = self.recovery_on(disk).find().unwrap();
        assert_eq!(found.len(), 1, "{found:?}");
        found[0].batch_id.clone()
    }
}

fn heat_folders(lab: &Lab) -> (PathBuf, PathBuf) {
    (
        Path::new(&lab.movies.path).join("Heat (1995)"),
        Path::new(&lab.archive.path).join("Heat (1995)"),
    )
}

// ---- roll back never removes the only copy -------------------------------------------------------

#[test]
fn a_roll_back_after_the_title_was_put_in_place_never_removes_the_only_copy() {
    let lab = Lab::new();
    let heat = lab.big_movie(&lab.movies, "Heat (1995)");
    let (old, new) = heat_folders(&lab);
    let before = files(&old);
    // Killed while the originals go to the trash folder: the title is whole in the new place.
    let disk = FaultFs::new(Faults {
        kill_at_trash: Some(4),
        ..Faults::default()
    });
    lab.killed(heat, &disk);
    assert_eq!(files(&new), before);
    // An original that was not in the trash folder yet is lost from the old folder (deleted by
    // hand, a failing disk): its copy in the new place is the only one left.
    let (lost, _) = files(&old).into_iter().next().unwrap();
    fs::remove_file(old.join(&lost)).unwrap();
    let batch = lab.only_unfinished(&disk);

    let outcome = lab.recovery_on(&disk).roll_back(&batch).unwrap();

    assert!(
        outcome.problems.iter().any(|p| p.contains("only one")),
        "{outcome:?}"
    );
    assert_eq!(files(&new), before, "the new place is left whole");
    assert_eq!(lab.item(heat).core.data_source, lab.archive.path);
}

#[test]
fn a_roll_back_after_the_title_was_put_in_place_sends_the_copies_to_the_trash_folder() {
    let lab = Lab::new();
    let heat = lab.big_movie(&lab.movies, "Heat (1995)");
    let (old, new) = heat_folders(&lab);
    let before = files(&old);
    let disk = FaultFs::new(Faults {
        kill_at_trash: Some(4),
        ..Faults::default()
    });
    lab.killed(heat, &disk);
    let batch = lab.only_unfinished(&disk);
    let found = lab.recovery_on(&disk).find().unwrap();
    assert!(found[0].in_new_place, "{found:?}");
    assert!(
        found[0]
            .notice()
            .detail
            .unwrap_or_default()
            .contains("new place")
    );

    let outcome = lab.recovery_on(&disk).roll_back(&batch).unwrap();

    assert!(outcome.problems.is_empty(), "{outcome:?}");
    assert_eq!(files(&old), before, "every original is back");
    assert!(!new.exists());
    let kept = Path::new(&lab.archive.path)
        .join(".deletedByCineVault")
        .join(format!("{batch}-undo"))
        .join("Heat (1995)");
    assert_eq!(
        files(&kept),
        before,
        "the copies are kept in the trash folder"
    );
    assert!(
        old.join("empty-folder").is_dir(),
        "the empty folder came back"
    );
}

#[test]
fn a_roll_back_before_the_commit_keeps_a_copy_whose_original_was_lost() {
    let lab = Lab::new();
    let heat = lab.big_movie(&lab.movies, "Heat (1995)");
    let (old, _) = heat_folders(&lab);
    let disk = FaultFs::new(Faults {
        kill_at_part_rename: Some(3),
        ..Faults::default()
    });
    lab.killed(heat, &disk);
    let batch = lab.only_unfinished(&disk);
    // The first copy is verified in the staging folder; its original then goes missing.
    let first = lab
        .journal(&batch)
        .into_iter()
        .find(|r| r.op == "copy" && r.state == "done")
        .unwrap();
    let content = fs::read(&first.from_path).unwrap();
    fs::remove_file(&first.from_path).unwrap();

    let outcome = lab.recovery_on(&disk).roll_back(&batch).unwrap();

    let rel = Path::new(&first.from_path)
        .strip_prefix(&old)
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/");
    let kept = Path::new(&lab.archive.path)
        .join(".deletedByCineVault")
        .join(format!("{batch}-rollback"))
        .join("Heat (1995)");
    assert_eq!(files(&kept), [(rel.clone(), content)], "{outcome:?}");
    assert!(
        outcome
            .problems
            .iter()
            .any(|p| p.contains(&rel) && p.contains("kept")),
        "{outcome:?}"
    );
    assert!(lab.recovery_on(&disk).find().unwrap().is_empty());
}

// ---- same-volume moves ---------------------------------------------------------------------------

#[test]
fn a_same_volume_move_killed_after_the_rename_is_rolled_back_to_the_old_folder() {
    let lab = Lab::new();
    let heat = lab.big_movie(&lab.movies, "Heat (1995)");
    let (old, new) = heat_folders(&lab);
    let before = files(&old);
    let disk = FaultFs::new(Faults {
        kill_after_folder_rename: Some("Heat (1995)".to_owned()),
        ..Faults::default()
    });
    let service = lab
        .change()
        .with_disk(Arc::new(SharedFaultFs(Arc::clone(&disk))));
    let killed = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let _ = service.apply(&[heat], lab.archive.id, &mut no_progress());
    }));
    assert!(killed.is_err());
    disk.heal();
    // The folder moved; the journal and the library did not hear about it.
    assert_eq!(files(&new), before);
    assert_eq!(lab.item(heat).core.data_source, lab.movies.path);
    let batch = lab.only_unfinished(&disk);

    let outcome = lab.recovery_on(&disk).roll_back(&batch).unwrap();

    assert!(outcome.problems.is_empty(), "{outcome:?}");
    assert_eq!(outcome.restored, 1);
    assert_eq!(
        files(&old),
        before,
        "the folder is back where the library says"
    );
    assert!(!new.exists());
    assert!(old.join("empty-folder").is_dir());
    assert_eq!(lab.item(heat).core.data_source, lab.movies.path);
    assert!(lab.recovery_on(&disk).find().unwrap().is_empty());
}

#[test]
fn a_same_volume_move_the_system_refuses_across_drives_is_copied_and_verified_instead() {
    let lab = Lab::new();
    let heat = lab.big_movie(&lab.movies, "Heat (1995)");
    let (old, new) = heat_folders(&lab);
    let before = files(&old);
    // The preview thinks it is one volume; a mount point on the way says otherwise.
    let disk = FaultFs::new(Faults {
        cross_device_folder: Some("Heat (1995)".to_owned()),
        ..Faults::default()
    });
    let applied = lab
        .change()
        .with_disk(Arc::new(SharedFaultFs(Arc::clone(&disk))))
        .apply(&[heat], lab.archive.id, &mut no_progress())
        .unwrap();

    assert_eq!(applied.moved(), 1, "{applied:?}");
    assert_eq!(files(&new), before);
    assert!(
        new.join("empty-folder").is_dir(),
        "empty folders arrive too"
    );
    assert!(!old.exists());
    let batch = applied.batch_id.unwrap();
    let trash = Path::new(&lab.movies.path)
        .join(".deletedByCineVault")
        .join(&batch)
        .join("Heat (1995)");
    assert_eq!(
        files(&trash),
        before,
        "the originals are in the trash folder"
    );
    let rows = lab.journal(&batch);
    assert!(rows.iter().any(|r| r.op == "copy"), "{rows:?}");
    assert!(rows.iter().all(|r| r.state == "done"), "{rows:?}");
    assert_eq!(lab.item(heat).core.data_source, lab.archive.path);

    // And it is undone like any copy.
    let undone = OrganizeService::new(Arc::clone(&lab.db))
        .undo(&batch)
        .unwrap();
    assert_eq!(undone.restored, 1, "{undone:?}");
    assert_eq!(files(&old), before);
}

// ---- the journal ---------------------------------------------------------------------------------

#[test]
fn a_batch_whose_journal_cannot_be_written_whole_leaves_no_rows() {
    let lab = Lab::new();
    let heat = lab.big_movie(&lab.movies, "Heat (1995)");
    let (old, new) = heat_folders(&lab);
    let batch = "move-1700000000000-77";
    // A row the batch's own fifth row collides with: the write fails part way.
    let mut journal = DbJournal::new(Arc::clone(&lab.db));
    journal
        .begin(&JournalEntry {
            batch_id: batch.to_owned(),
            seq: 5,
            item_id: None,
            op: JournalOp::Copy,
            from: "x".to_owned(),
            to: "y".to_owned(),
            is_dir: false,
            created_parents: None,
        })
        .unwrap();
    let entries = enumerate(&old).unwrap();
    let job = MoveJob {
        item: heat,
        title: "Heat".to_owned(),
        from_root: PathBuf::from(&lab.movies.path),
        from: old.clone(),
        to_root: PathBuf::from(&lab.archive.path),
        to: new.clone(),
        entries,
        across_volumes: true,
    };
    let fs = crate::library_ops::StdMoveFs;
    let result = MoveEngine::new(&fs, &mut journal).run(batch, &[job], &mut no_progress());

    assert!(result.is_err());
    assert_eq!(
        lab.journal(batch).len(),
        1,
        "none of the batch's rows were written"
    );
    assert!(!new.exists());
}

// ---- files that change while they move -----------------------------------------------------------

#[test]
fn a_file_that_arrives_in_the_old_folder_during_the_copy_moves_too() {
    let lab = Lab::new();
    let heat = lab.big_movie(&lab.movies, "Heat (1995)");
    let (old, new) = heat_folders(&lab);
    let arrived = old.join("Heat (1995).de.srt");
    let disk = FaultFs::new(Faults {
        on_create: Some(Box::new({
            let arrived = arrived.clone();
            move |_| {
                if !arrived.exists() {
                    fs::write(&arrived, "neue Untertitel").unwrap();
                    fs::create_dir_all(arrived.with_file_name("new-empty")).unwrap();
                }
            }
        })),
        ..Faults::default()
    });
    let applied = lab
        .cross_with(&disk)
        .apply(&[heat], lab.archive.id, &mut no_progress())
        .unwrap();

    assert_eq!(applied.moved(), 1, "{applied:?}");
    assert_eq!(
        fs::read_to_string(new.join("Heat (1995).de.srt")).unwrap(),
        "neue Untertitel"
    );
    assert!(new.join("new-empty").is_dir());
    assert!(!old.exists(), "nothing is left in the old folder");
}

#[test]
fn an_original_changed_in_place_after_its_copy_stops_the_move_and_resume_copies_it_again() {
    let lab = Lab::new();
    let heat = lab.big_movie(&lab.movies, "Heat (1995)");
    let (old, new) = heat_folders(&lab);
    let hidden = old.join(".hidden");
    // While the second file is copied, the first one (".hidden", copied already) is rewritten
    // with the same length.
    let disk = FaultFs::new(Faults {
        on_create: Some(Box::new({
            let hidden = hidden.clone();
            move |path| {
                if path
                    .to_string_lossy()
                    .ends_with("Heat (1995).en.srt.cvpart")
                {
                    fs::write(&hidden, "X").unwrap();
                    let later = SystemTime::now() + Duration::from_secs(3600);
                    fs::File::options()
                        .write(true)
                        .open(&hidden)
                        .unwrap()
                        .set_modified(later)
                        .unwrap();
                }
            }
        })),
        ..Faults::default()
    });
    let applied = lab
        .cross_with(&disk)
        .apply(&[heat], lab.archive.id, &mut no_progress())
        .unwrap();

    assert!(
        matches!(&applied.items[0].result, MoveResult::Stopped(why) if why.contains("changed")),
        "{applied:?}"
    );
    assert_eq!(
        fs::read_to_string(&hidden).unwrap(),
        "X",
        "the original is untouched"
    );
    assert_eq!(lab.item(heat).core.data_source, lab.movies.path);
    disk.heal();

    let batch = lab.only_unfinished(&disk);
    let outcome = lab
        .recovery_on(&disk)
        .resume(&batch, None, &mut no_progress())
        .unwrap();
    assert_eq!(outcome.completed, 1, "{outcome:?}");
    assert_eq!(fs::read_to_string(new.join(".hidden")).unwrap(), "X");
}

// ---- passing errors ------------------------------------------------------------------------------

#[test]
fn a_passing_in_use_error_on_a_rename_is_tried_again() {
    let lab = Lab::new();
    let heat = lab.big_movie(&lab.movies, "Heat (1995)");
    let (old, new) = heat_folders(&lab);
    let before = files(&old);
    let disk = FaultFs::new(Faults {
        transient_renames: 3,
        ..Faults::default()
    });
    let applied = lab
        .cross_with(&disk)
        .apply(&[heat], lab.archive.id, &mut no_progress())
        .unwrap();
    assert_eq!(applied.moved(), 1, "{applied:?}");
    assert!(applied.items[0].notes.is_empty(), "{applied:?}");
    assert_eq!(files(&new), before);
}

#[test]
fn an_original_in_use_when_it_goes_to_the_trash_folder_is_finished_by_resume() {
    let lab = Lab::new();
    let heat = lab.movie(&lab.movies, "Heat (1995)");
    let (old, new) = heat_folders(&lab);
    let disk = FaultFs::new(Faults {
        busy_original: Some("Heat (1995).mkv".to_owned()),
        ..Faults::default()
    });
    let applied = lab
        .cross_with(&disk)
        .apply(&[heat], lab.archive.id, &mut no_progress())
        .unwrap();
    let MoveResult::Stopped(why) = &applied.items[0].result else {
        panic!("{applied:?}");
    };
    assert!(why.contains("resume"), "{why}");
    assert!(old.join("Heat (1995).mkv").is_file());
    // Activity offers to finish it.
    disk.heal();
    let batch = lab.only_unfinished(&disk);
    let outcome = lab
        .recovery_on(&disk)
        .resume(&batch, None, &mut no_progress())
        .unwrap();
    assert_eq!(outcome.completed, 1, "{outcome:?}");
    assert!(!old.exists(), "nothing is left in the old folder");
    assert!(new.join("Heat (1995).mkv").is_file());
    assert!(lab.recovery_on(&disk).find().unwrap().is_empty());
}

// ---- undo ----------------------------------------------------------------------------------------

#[test]
fn undo_brings_back_the_empty_folders_and_leaves_files_added_after_the_move() {
    let lab = Lab::new();
    let heat = lab.big_movie(&lab.movies, "Heat (1995)");
    let (old, new) = heat_folders(&lab);
    let applied = lab
        .cross()
        .apply(&[heat], lab.archive.id, &mut no_progress())
        .unwrap();
    let batch = applied.batch_id.unwrap();
    fs::write(new.join("notes.txt"), "mine").unwrap();

    let outcome = OrganizeService::new(Arc::clone(&lab.db))
        .undo(&batch)
        .unwrap();

    assert_eq!(outcome.restored, 1, "{outcome:?}");
    assert!(
        old.join("empty-folder").is_dir(),
        "the empty folder came back"
    );
    assert_eq!(
        names(&new),
        ["notes.txt"],
        "a file the move did not make stays"
    );
}

#[test]
fn undo_after_the_trash_folder_was_emptied_moves_the_title_back_from_its_new_place() {
    let lab = Lab::new();
    let heat = lab.big_movie(&lab.movies, "Heat (1995)");
    let (old, new) = heat_folders(&lab);
    let before = files(&old);
    let applied = lab
        .cross()
        .apply(&[heat], lab.archive.id, &mut no_progress())
        .unwrap();
    let batch = applied.batch_id.unwrap();
    // Someone empties the trash folder of the old source by hand.
    fs::remove_dir_all(Path::new(&lab.movies.path).join(".deletedByCineVault")).unwrap();

    let outcome = OrganizeService::new(Arc::clone(&lab.db))
        .undo(&batch)
        .unwrap();

    assert_eq!(outcome.restored, 1, "{outcome:?}");
    assert!(outcome.is_complete(), "{outcome:?}");
    assert_eq!(files(&old), before, "every file is back, byte for byte");
    assert!(!new.exists());
    let item = lab.item(heat);
    assert_eq!(item.core.data_source, lab.movies.path);
    assert_eq!(PathBuf::from(&item.core.path), old);
    // The batch is taken back; the move back is a batch of its own.
    assert!(lab.journal(&batch).iter().all(|r| r.state != "done"));
    let batches: Vec<_> = lab
        .db
        .read(table::batch_summaries)
        .unwrap()
        .into_iter()
        .filter(|b| b.batch_id.starts_with("move-"))
        .collect();
    assert_eq!(batches.len(), 2, "{batches:?}");
}

// ---- what the preview allows ---------------------------------------------------------------------

#[test]
fn a_title_with_an_unfinished_move_is_not_moved_again() {
    let lab = Lab::new();
    let heat = lab.big_movie(&lab.movies, "Heat (1995)");
    let disk = FaultFs::new(Faults {
        kill_at_part_rename: Some(2),
        ..Faults::default()
    });
    lab.killed(heat, &disk);

    let plan = lab.cross().plan(&[heat], lab.archive.id).unwrap();

    assert!(
        matches!(&plan.rows[0].status, MoveStatus::NotMovable(why) if why.contains("did not finish")),
        "{:?}",
        plan.rows[0].status
    );
}

#[test]
fn a_batch_at_work_is_not_offered_for_resume_or_roll_back() {
    let lab = Lab::new();
    let heat = lab.big_movie(&lab.movies, "Heat (1995)");
    let seen = Arc::new(Mutex::new(None));
    let db = Arc::clone(&lab.db);
    let disk = FaultFs::new(Faults {
        on_create: Some(Box::new({
            let seen = Arc::clone(&seen);
            move |_| {
                let mut seen = seen.lock().unwrap();
                if seen.is_none() {
                    *seen = Some(RecoveryService::new(Arc::clone(&db)).find().unwrap().len());
                }
            }
        })),
        ..Faults::default()
    });
    let applied = lab
        .cross_with(&disk)
        .apply(&[heat], lab.archive.id, &mut no_progress())
        .unwrap();
    assert_eq!(applied.moved(), 1);
    assert_eq!(
        *seen.lock().unwrap(),
        Some(0),
        "the running move was not offered"
    );
}

#[test]
fn a_write_into_a_title_that_is_being_moved_is_refused_with_the_reason() {
    use crate::title_guard::{self, Work};
    let lab = Lab::new();
    let heat = lab.big_movie(&lab.movies, "Heat (1995)");
    let refused = Arc::new(Mutex::new(None));
    let disk = FaultFs::new(Faults {
        on_create: Some(Box::new({
            let refused = Arc::clone(&refused);
            let db = Arc::clone(&lab.db);
            move |_| {
                let mut refused = refused.lock().unwrap();
                if refused.is_none() {
                    *refused = Some(
                        title_guard::hold(&db, [heat], Work::Write, Duration::from_millis(20))
                            .map(|_| ()),
                    );
                }
            }
        })),
        ..Faults::default()
    });
    let applied = lab
        .cross_with(&disk)
        .apply(&[heat], lab.archive.id, &mut no_progress())
        .unwrap();
    assert_eq!(applied.moved(), 1);
    let refused = refused.lock().unwrap().clone().unwrap();
    assert_eq!(refused.unwrap_err().work, Work::Move);
    // Released when the move is over.
    assert!(title_guard::hold(&lab.db, [heat], Work::Write, Duration::ZERO).is_ok());
}

#[test]
fn a_move_waits_for_a_short_write_into_the_title_to_finish() {
    use crate::title_guard::{self, Work};
    let lab = Lab::new();
    let heat = lab.movie(&lab.movies, "Heat (1995)");
    let writing = title_guard::hold(&lab.db, [heat], Work::Write, Duration::ZERO).unwrap();
    let started = std::time::Instant::now();
    let applied = std::thread::scope(|scope| {
        let mover = scope.spawn(|| {
            lab.cross()
                .apply(&[heat], lab.archive.id, &mut no_progress())
        });
        std::thread::sleep(Duration::from_millis(200));
        assert!(
            Path::new(&lab.movies.path)
                .join("Heat (1995)/poster.jpg")
                .is_file(),
            "nothing moved while the write held the title"
        );
        drop(writing);
        mover.join().unwrap()
    })
    .unwrap();
    assert!(started.elapsed() >= Duration::from_millis(200));
    assert_eq!(applied.moved(), 1, "{applied:?}");
}

// ---- links ---------------------------------------------------------------------------------------

/// Makes `link` point at the folder `target`: a junction on Windows (no rights needed), a
/// symbolic link elsewhere. False when the system will not.
fn folder_link(target: &Path, link: &Path) -> bool {
    #[cfg(windows)]
    {
        std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .is_ok_and(|o| o.status.success())
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).is_ok()
    }
}

#[test]
fn a_title_folder_that_is_a_link_is_not_moved() {
    let lab = Lab::new();
    let heat = lab.movie(&lab.movies, "Heat (1995)");
    let (old, _) = heat_folders(&lab);
    let elsewhere = lab.dir.path().join("elsewhere");
    fs::rename(&old, &elsewhere).unwrap();
    if !folder_link(&elsewhere, &old) {
        return;
    }
    let plan = lab.change().plan(&[heat], lab.archive.id).unwrap();
    assert!(
        matches!(&plan.rows[0].status, MoveStatus::NotMovable(why) if why.contains("link")),
        "{:?}",
        plan.rows[0].status
    );
}

#[test]
fn undo_never_enters_or_removes_a_link_someone_put_in_the_new_place() {
    let lab = Lab::new();
    let heat = lab.big_movie(&lab.movies, "Heat (1995)");
    let (_, new) = heat_folders(&lab);
    let applied = lab
        .cross()
        .apply(&[heat], lab.archive.id, &mut no_progress())
        .unwrap();
    let batch = applied.batch_id.unwrap();
    let elsewhere = lab.dir.path().join("elsewhere");
    fs::create_dir_all(elsewhere.join("empty inside")).unwrap();
    if !folder_link(&elsewhere, &new.join("linked")) {
        return;
    }

    let outcome = OrganizeService::new(Arc::clone(&lab.db))
        .undo(&batch)
        .unwrap();

    assert_eq!(outcome.restored, 1, "{outcome:?}");
    assert!(
        elsewhere.join("empty inside").is_dir(),
        "nothing behind the link was touched"
    );
    assert!(
        fs::symlink_metadata(new.join("linked")).is_ok(),
        "the link itself stays"
    );
}

// ---- an original left in the old folder --------------------------------------

#[test]
fn a_scan_leaves_alone_the_old_folder_of_a_move_that_left_an_original_behind() {
    let lab = Lab::new();
    let heat = lab.movie(&lab.movies, "Heat (1995)");
    let disk = FaultFs::new(Faults {
        busy_original: Some("Heat (1995).mkv".to_owned()),
        ..Faults::default()
    });
    let applied = lab
        .cross_with(&disk)
        .apply(&[heat], lab.archive.id, &mut no_progress())
        .unwrap();
    assert!(
        matches!(applied.items[0].result, MoveResult::Stopped(_)),
        "{applied:?}"
    );
    let (old, new) = heat_folders(&lab);
    assert!(
        old.join("Heat (1995).mkv").is_file(),
        "the video in use stays"
    );
    assert!(new.join("Heat (1995).mkv").is_file());

    // The scan that follows must not take the video left behind for a new title.
    let options = cv_scan::rescan::RescanOptions::default();
    let report = cv_scan::rescan::rescan_source(&lab.db, &lab.movies, &options).unwrap();
    assert!(report.new_items.is_empty(), "a duplicate title: {report:?}");
    assert!(!lab.exists(ItemId(heat.0 + 1)), "no second Heat");
    let report = cv_scan::rescan::rescan_source(&lab.db, &lab.archive, &options).unwrap();
    assert!(report.new_items.is_empty(), "{report:?}");
    assert_eq!(lab.item(heat).core.data_source, lab.archive.path);

    // Resume finishes it once the file is let go; the old folder is then scanned as before.
    disk.heal();
    let batch = lab.only_unfinished(&disk);
    let done = lab
        .recovery_on(&disk)
        .resume(&batch, None, &mut no_progress())
        .unwrap();
    assert_eq!(done.stopped, None, "{done:?}");
    assert!(!old.exists());
    let report = cv_scan::rescan::rescan_source(&lab.db, &lab.movies, &options).unwrap();
    assert!(report.new_items.is_empty(), "{report:?}");
}

// ---- the trash folder of an undo or roll back --------------------------------

/// Two titles that each keep a `notes.txt` and a `movieset-poster.jpg` of their own.
fn two_titles_with_same_named_files(lab: &Lab) -> (ItemId, ItemId) {
    let jaws = lab.movie(&lab.movies, "Jaws (1975)");
    let psycho = lab.movie(&lab.movies, "Psycho (1960)");
    for (name, id) in [("Jaws (1975)", jaws), ("Psycho (1960)", psycho)] {
        let folder = Path::new(&lab.movies.path).join(name);
        Lab::put(&folder, "notes.txt", &format!("{name} notes"));
        Lab::put(
            &folder,
            "movieset-poster.jpg",
            &format!("{name} set poster"),
        );
        let _ = id;
    }
    (jaws, psycho)
}

/// Every file below the trash folder of `root`, with its bytes.
fn trash_files(root: &str) -> Vec<(String, Vec<u8>)> {
    files(&Path::new(root).join(".deletedByCineVault"))
}

fn has_content(list: &[(String, Vec<u8>)], content: &str) -> bool {
    list.iter().any(|(_, bytes)| bytes == content.as_bytes())
}

#[test]
fn an_undo_of_two_titles_keeps_every_copy_in_the_trash_folder_under_its_own_title() {
    let lab = Lab::new();
    let (jaws, psycho) = two_titles_with_same_named_files(&lab);
    let applied = lab
        .cross()
        .apply(&[jaws, psycho], lab.archive.id, &mut no_progress())
        .unwrap();
    assert_eq!(applied.moved(), 2, "{applied:?}");

    let undone = OrganizeService::new(Arc::clone(&lab.db))
        .undo(applied.batch_id.as_deref().unwrap())
        .unwrap();
    assert!(undone.is_complete(), "{undone:?}");

    // Each title's copies are in the destination's trash folder, none overwritten by the other's.
    let kept = trash_files(&lab.archive.path);
    for name in ["Jaws (1975)", "Psycho (1960)"] {
        assert!(
            has_content(&kept, &format!("{name} notes")),
            "{name}: {kept:?}"
        );
        assert!(
            has_content(&kept, &format!("{name} set poster")),
            "{name}: {kept:?}"
        );
        assert!(
            kept.iter()
                .any(|(path, _)| path.contains(&format!("/{name}/notes.txt"))),
            "under the title's folder: {:?}",
            kept.iter().map(|(p, _)| p).collect::<Vec<_>>()
        );
    }
    assert_eq!(kept.len(), 10, "five copies of each title");
}

#[test]
fn a_roll_back_of_two_titles_keeps_every_copy_under_its_own_title() {
    let lab = Lab::new();
    let (jaws, psycho) = two_titles_with_same_named_files(&lab);
    // Both titles reach their new place, but neither `notes.txt` may leave the old folder: the
    // batch is unfinished for both, and a roll back takes both back.
    let disk = FaultFs::new(Faults::default());
    disk.shim().rule(
        cv_rename::exec::fault::Rule::on(
            &[cv_rename::exec::fault::Op::Rename],
            cv_rename::exec::fault::Fault::Kind(std::io::ErrorKind::PermissionDenied),
        )
        .when(|from, to| {
            from.file_name().is_some_and(|n| n == "notes.txt")
                && to.is_some_and(|t| t.to_string_lossy().contains(".deletedByCineVault"))
        }),
    );
    let applied = lab
        .cross_with(&disk)
        .apply(&[jaws, psycho], lab.archive.id, &mut no_progress())
        .unwrap();
    // Both are whole in the new place, neither finished: the second title went on although
    // the first could not take its `notes.txt` along.
    assert!(
        applied
            .items
            .iter()
            .all(|i| matches!(&i.result, MoveResult::Stopped(why) if why.contains("notes.txt"))),
        "{applied:?}"
    );
    disk.heal();
    let batch = lab.only_unfinished(&disk);
    let back = lab.recovery_on(&disk).roll_back(&batch).unwrap();
    assert!(back.problems.is_empty(), "{back:?}");
    for name in ["Jaws (1975)", "Psycho (1960)"] {
        let old = Path::new(&lab.movies.path).join(name);
        assert_eq!(
            fs::read_to_string(old.join("notes.txt")).unwrap(),
            format!("{name} notes")
        );
    }
    let kept = trash_files(&lab.archive.path);
    for name in ["Jaws (1975)", "Psycho (1960)"] {
        assert!(
            has_content(&kept, &format!("{name} notes")),
            "{name}: {kept:?}"
        );
        assert!(
            has_content(&kept, &format!("{name} set poster")),
            "{name}: {kept:?}"
        );
    }
}

#[test]
fn nothing_already_in_the_trash_folder_is_ever_replaced_by_an_undo() {
    let lab = Lab::new();
    let heat = lab.movie(&lab.movies, "Heat (1995)");
    let applied = lab
        .cross()
        .apply(&[heat], lab.archive.id, &mut no_progress())
        .unwrap();
    let batch = applied.batch_id.unwrap();
    // Something is already where the undo would put the copy of the poster.
    let trash = Path::new(&lab.archive.path).join(".deletedByCineVault");
    for at in [
        trash.join(format!("{batch}-undo")).join("Heat (1995)"),
        trash.join(format!("{batch}-undo")),
    ] {
        Lab::put(&at, "poster.jpg", "was here first");
    }

    let undone = OrganizeService::new(Arc::clone(&lab.db))
        .undo(&batch)
        .unwrap();
    assert!(undone.is_complete(), "{undone:?}");
    let kept = trash_files(&lab.archive.path);
    assert_eq!(
        kept.iter().filter(|(_, b)| b == b"was here first").count(),
        2,
        "{kept:?}"
    );
    assert!(
        has_content(&kept, "Poster"),
        "the copy is kept too: {kept:?}"
    );
}

// ---- the system bin ----------------------------------------------

#[test]
fn undo_after_the_bin_was_emptied_copies_the_title_back_and_bins_the_copies() {
    let lab = Lab::new();
    let bin = lab.bin();
    let heat = lab.big_movie(&lab.movies, "Heat (1995)");
    let (old, new) = heat_folders(&lab);
    let before = files(&old);
    let batch = lab
        .cross_bin(&bin)
        .apply(&[heat], lab.archive.id, &mut no_progress())
        .unwrap()
        .batch_id
        .unwrap();
    bin.empty();

    let outcome = lab.organize_bin(&bin).undo(&batch).unwrap();

    assert_eq!(outcome.restored, 1, "{outcome:?}");
    assert!(outcome.is_complete(), "{outcome:?}");
    assert_eq!(
        files(&old),
        before,
        "copied back and verified, byte for byte"
    );
    assert!(!new.exists());
    assert_eq!(PathBuf::from(&lab.item(heat).core.path), old);
    // The move back is a move of its own: here both library folders share a volume, so the
    // folder is renamed back; across drives its verified copy sends the copies in the new place
    // to the bin like any other move (the forward test above).
    assert!(bin.names().is_empty(), "{:?}", bin.names());
}

#[test]
fn undo_with_part_of_the_originals_gone_from_the_bin_copies_the_title_back() {
    let lab = Lab::new();
    let bin = lab.bin();
    let heat = lab.big_movie(&lab.movies, "Heat (1995)");
    let (old, new) = heat_folders(&lab);
    let before = files(&old);
    let batch = lab
        .cross_bin(&bin)
        .apply(&[heat], lab.archive.id, &mut no_progress())
        .unwrap()
        .batch_id
        .unwrap();
    // One original is gone from the bin (deleted from it by hand).
    let gone = fs::read_dir(&bin.dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| p.to_string_lossy().ends_with("theme.mp3"))
        .unwrap();
    fs::remove_file(gone).unwrap();

    let outcome = lab.organize_bin(&bin).undo(&batch).unwrap();

    assert_eq!(outcome.restored, 1, "{outcome:?}");
    assert_eq!(files(&old), before, "the title is whole in the old place");
    assert!(!new.exists());
    // The seven originals still in the bin stay there; nothing was restored over the copies.
    assert_eq!(bin.names().len(), 7, "{:?}", bin.names());
}

#[test]
fn originals_the_bin_refuses_go_to_the_trash_folder() {
    let lab = Lab::new();
    let bin = lab.bin();
    bin.fail.store(true, Ordering::SeqCst);
    let heat = lab.big_movie(&lab.movies, "Heat (1995)");
    let (old, _) = heat_folders(&lab);
    let applied = lab
        .cross_bin(&bin)
        .apply(&[heat], lab.archive.id, &mut no_progress())
        .unwrap();
    assert_eq!(applied.moved(), 1, "{applied:?}");
    assert!(!old.exists());
    assert!(bin.names().is_empty());
    let trash = Path::new(&lab.movies.path).join(".deletedByCineVault");
    assert_eq!(files(&trash).len(), 8, "{:?}", names(&trash));
    assert!(
        applied.items[0]
            .notes
            .iter()
            .any(|n| n.contains(&format!("{} refused", crate::library_ops::BIN_NAME))),
        "{applied:?}"
    );
    let outcome = lab
        .organize_bin(&bin)
        .undo(applied.batch_id.as_deref().unwrap())
        .unwrap();
    assert!(outcome.is_complete(), "{outcome:?}");
    assert!(old.join("Heat (1995).mkv").is_file());
}
