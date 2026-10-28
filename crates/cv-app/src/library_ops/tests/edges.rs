//! Path edge cases of moves between data sources:
//! paths over the Windows limit, a title named in decomposed Unicode (NFD), read-only files,
//! a file another program holds open, network (UNC) paths, and a destination that holds the
//! title in another case. Each move is checked to land whole and verified, then undone to the
//! exact old state; a move that cannot run leaves every original where it was.

use std::panic::AssertUnwindSafe;

use cv_db::repo::rename_journal as table;

use super::*;
use crate::library_ops::RecoveryService;

/// Every file below `dir` with its bytes, relative to `dir`, `/` separated; the trash folder of a
/// data source is left out.
fn bytes_below(dir: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        let Ok(read) = fs::read_dir(dir) else { return };
        for entry in read.flatten() {
            let path = entry.path();
            if entry.file_name() == ".deletedByCineVault" {
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

fn journal_states(lab: &Lab, batch: &str) -> Vec<String> {
    lab.db
        .read(|c| table::batch_rows(c, batch))
        .unwrap()
        .into_iter()
        .map(|r| r.state)
        .collect()
}

/// Moves `id` from `movies` to `archive` with `service`, checks the title arrived whole with the
/// library pointing at it and the originals in the trash folder, then undoes the move and checks
/// the old folder is exactly as it was.
fn move_and_undo(lab: &Lab, service: &ChangeSourceService, id: ItemId, name: &str) {
    let old = Path::new(&lab.movies.path).join(name);
    let new = Path::new(&lab.archive.path).join(name);
    let before = bytes_below(&old);
    assert!(!before.is_empty());

    let applied = service
        .apply(&[id], lab.archive.id, &mut no_progress())
        .unwrap();
    assert_eq!(applied.moved(), 1, "{applied:?}");
    assert!(applied.items[0].notes.is_empty(), "{applied:?}");
    let batch = applied.batch_id.unwrap();
    assert_eq!(bytes_below(&new), before, "every file arrived");
    assert!(bytes_below(&old).is_empty(), "nothing is left behind");
    let item = lab.item(id);
    assert_eq!(item.core.data_source, lab.archive.path);
    assert_eq!(
        item.core.path,
        new.to_string_lossy(),
        "spelled as on the disk"
    );
    assert!(
        journal_states(lab, &batch).iter().all(|s| s == "done"),
        "{:?}",
        journal_states(lab, &batch)
    );

    let undone = OrganizeService::new(Arc::clone(&lab.db))
        .undo(&batch)
        .unwrap();
    assert!(undone.is_complete(), "{undone:?}");
    assert_eq!(
        bytes_below(&old),
        before,
        "every original is back, byte for byte"
    );
    assert!(!new.exists());
    let item = lab.item(id);
    assert_eq!(item.core.data_source, lab.movies.path);
    assert_eq!(item.core.path, old.to_string_lossy());
}

// ---- paths over the Windows limit ------------------------------------------------------------

/// A movie whose deepest path is far over 260 characters.
#[cfg(windows)]
fn long_movie(lab: &Lab) -> (ItemId, String) {
    let name = format!("Heat {} (1995)", "x".repeat(100));
    let id = lab.movie(&lab.movies, &name);
    let folder = Path::new(&lab.movies.path).join(&name);
    let deep = format!("extras/{}/{}.mkv", "e".repeat(80), "m".repeat(60));
    Lab::put(&folder, &deep, "a long extra");
    assert!(folder.join(&deep).to_string_lossy().len() > 260);
    (id, name)
}

#[cfg(windows)]
#[test]
fn a_title_with_paths_over_the_windows_limit_is_moved_to_another_volume_and_undone() {
    let lab = Lab::new();
    let (id, name) = long_movie(&lab);
    move_and_undo(&lab, &lab.cross(), id, &name);
}

#[cfg(windows)]
#[test]
fn a_title_with_paths_over_the_windows_limit_is_moved_on_one_volume_and_undone() {
    let lab = Lab::new();
    let (id, name) = long_movie(&lab);
    move_and_undo(&lab, &lab.change(), id, &name);
}

// ---- NFD names -------------------------------------------------------------------------------

/// "Amélie" decomposed (an e and a combining acute), as a Mac writes it.
const NFD: &str = "Ame\u{301}lie (2001)";

#[test]
fn a_title_named_in_decomposed_unicode_keeps_its_exact_name_through_a_copy_and_an_undo() {
    let lab = Lab::new();
    let id = lab.movie(&lab.movies, NFD);
    move_and_undo(&lab, &lab.cross(), id, NFD);
    move_and_undo(&lab, &lab.change(), id, NFD);
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

#[test]
fn read_only_files_are_moved_verified_and_undone_and_stay_read_only() {
    for across in [true, false] {
        let lab = Lab::new();
        let id = lab.movie(&lab.movies, "Heat (1995)");
        let old = Path::new(&lab.movies.path).join("Heat (1995)");
        for name in ["Heat (1995).mkv", "Heat (1995).nfo"] {
            set_read_only(&old.join(name), true);
        }
        let service = if across { lab.cross() } else { lab.change() };
        let applied = service
            .apply(&[id], lab.archive.id, &mut no_progress())
            .unwrap();
        assert_eq!(applied.moved(), 1, "across {across}: {applied:?}");
        let new = Path::new(&lab.archive.path).join("Heat (1995)");
        assert!(
            is_read_only(&new.join("Heat (1995).mkv")),
            "across {across}: the copy keeps the read-only flag"
        );
        assert!(!is_read_only(&new.join("poster.jpg")));
        let undone = OrganizeService::new(Arc::clone(&lab.db))
            .undo(applied.batch_id.as_deref().unwrap())
            .unwrap();
        assert!(undone.is_complete(), "across {across}: {undone:?}");
        assert!(
            is_read_only(&old.join("Heat (1995).mkv")),
            "across {across}"
        );
        assert_eq!(bytes_below(&old).len(), 3);
        writable(lab.dir.path());
    }
}

#[test]
fn a_move_of_read_only_files_stopped_before_the_commit_is_rolled_back() {
    let lab = Lab::new();
    let id = lab.movie(&lab.movies, "Heat (1995)");
    let old = Path::new(&lab.movies.path).join("Heat (1995)");
    for name in ["Heat (1995).mkv", "Heat (1995).nfo", "poster.jpg"] {
        set_read_only(&old.join(name), true);
    }
    let before = bytes_below(&old);
    // Killed when the third copy is renamed into place: two read-only copies are verified.
    let disk = super::moves::FaultFs::new(super::moves::Faults {
        kill_at_part_rename: Some(3),
        ..super::moves::Faults::default()
    });
    let service = lab.cross_with(&disk);
    let killed = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let _ = service.apply(&[id], lab.archive.id, &mut no_progress());
    }));
    assert!(killed.is_err());
    disk.heal();
    let recovery = RecoveryService::new(Arc::clone(&lab.db));
    let batch = recovery.find().unwrap().remove(0);
    let back = recovery.roll_back(&batch.batch_id).unwrap();
    assert!(back.problems.is_empty(), "{back:?}");
    assert_eq!(bytes_below(&old), before);
    assert!(
        bytes_below(Path::new(&lab.archive.path)).is_empty(),
        "the read-only copies were removed: {:?}",
        bytes_below(Path::new(&lab.archive.path))
    );
    writable(lab.dir.path());
}

// ---- a file held open by another program ---------------------------------------------------------

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
fn a_file_held_open_stops_a_copy_with_the_originals_intact_and_resume_finishes_it() {
    let lab = Lab::new();
    let id = lab.movie(&lab.movies, "Heat (1995)");
    let old = Path::new(&lab.movies.path).join("Heat (1995)");
    let before = bytes_below(&old);
    let held = hold(&old.join("Heat (1995).mkv"));

    let applied = lab
        .cross()
        .apply(&[id], lab.archive.id, &mut no_progress())
        .unwrap();
    let MoveResult::Stopped(why) = &applied.items[0].result else {
        panic!("{applied:?}");
    };
    assert!(why.contains("in use"), "{why}");
    drop(held);
    assert_eq!(bytes_below(&old), before, "every original is where it was");
    assert_eq!(lab.item(id).core.data_source, lab.movies.path);

    let recovery = RecoveryService::new(Arc::clone(&lab.db));
    let found = recovery.find().unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    let done = recovery
        .resume(&found[0].batch_id, None, &mut no_progress())
        .unwrap();
    assert_eq!(done.completed, 1, "{done:?}");
    assert_eq!(
        bytes_below(&Path::new(&lab.archive.path).join("Heat (1995)")),
        before
    );
    assert_eq!(lab.item(id).core.data_source, lab.archive.path);
    assert!(recovery.find().unwrap().is_empty());
}

#[cfg(windows)]
#[test]
fn a_file_held_open_stops_a_same_volume_move_and_the_title_stays_whole() {
    let lab = Lab::new();
    let id = lab.movie(&lab.movies, "Heat (1995)");
    let old = Path::new(&lab.movies.path).join("Heat (1995)");
    let before = bytes_below(&old);
    let held = hold(&old.join("Heat (1995).mkv"));

    let applied = lab
        .change()
        .apply(&[id], lab.archive.id, &mut no_progress())
        .unwrap();
    drop(held);
    assert_eq!(applied.moved(), 0, "{applied:?}");
    assert!(
        matches!(
            &applied.items[0].result,
            MoveResult::Stopped(_) | MoveResult::Failed(_)
        ),
        "{applied:?}"
    );
    assert_eq!(bytes_below(&old), before);
    assert!(!Path::new(&lab.archive.path).join("Heat (1995)").exists());
    assert_eq!(lab.item(id).core.data_source, lab.movies.path);

    // Once it is let go, the move is finished from Activity (or simply run again).
    let recovery = RecoveryService::new(Arc::clone(&lab.db));
    match recovery.find().unwrap().first() {
        Some(batch) => {
            let done = recovery
                .resume(&batch.batch_id, None, &mut no_progress())
                .unwrap();
            assert_eq!(done.completed, 1, "{done:?}");
        }
        None => {
            let again = lab
                .change()
                .apply(&[id], lab.archive.id, &mut no_progress())
                .unwrap();
            assert_eq!(again.moved(), 1, "{again:?}");
        }
    }
    assert_eq!(
        bytes_below(&Path::new(&lab.archive.path).join("Heat (1995)")),
        before
    );
    assert_eq!(lab.item(id).core.data_source, lab.archive.path);
}

// ---- network (UNC) paths -------------------------------------------------------------------------

/// The administrative share path (`\\localhost\C$\...`) of a local folder, when this machine
/// serves it and lets this session write through it.
#[cfg(windows)]
fn unc_of(path: &Path) -> Option<String> {
    let text = path.to_string_lossy();
    let bytes = text.as_bytes();
    if bytes.len() < 3 || bytes[1] != b':' {
        return None;
    }
    let unc = format!(r"\\localhost\{}$\{}", &text[..1], &text[3..]);
    fs::metadata(&unc).ok()?;
    // The share can exist and still refuse writes to a non-elevated session (a hosted CI runner
    // does): the test creates, writes, renames and removes through it, so try exactly that.
    let probe = format!(r"{unc}\share-probe");
    let worked = fs::create_dir_all(format!(r"{probe}\a"))
        .and_then(|()| fs::write(format!(r"{probe}\a\a.txt"), b"x"))
        .and_then(|()| fs::rename(format!(r"{probe}\a\a.txt"), format!(r"{probe}\a\b.txt")))
        .and_then(|()| fs::rename(format!(r"{probe}\a"), format!(r"{probe}\c")))
        .is_ok();
    let _ = fs::remove_dir_all(&probe);
    worked.then_some(unc)
}

#[cfg(windows)]
#[test]
fn a_title_on_a_network_share_is_moved_and_undone_through_unc_paths() {
    use cv_db::repo::data_sources::insert_source;
    let lab = Lab::new();
    let Some(movies) = unc_of(Path::new(&lab.movies.path)) else {
        eprintln!("skipped: no administrative share on this machine (manual check instead)");
        return;
    };
    let archive = unc_of(Path::new(&lab.archive.path)).unwrap();
    // The same two folders, as data sources reached over the network.
    let share_movies = {
        let path = format!(r"{movies}\share");
        fs::create_dir_all(&path).unwrap();
        lab.db
            .write(move |tx| insert_source(tx, SourceKind::Movie, &path))
            .unwrap()
    };
    let share_archive = {
        let path = format!(r"{archive}\share");
        fs::create_dir_all(&path).unwrap();
        lab.db
            .write(move |tx| insert_source(tx, SourceKind::Movie, &path))
            .unwrap()
    };
    for across in [true, false] {
        let name = format!("Heat {} {across} (1995)", "u".repeat(150));
        let id = lab.movie(&share_movies, &name);
        let old = Path::new(&share_movies.path).join(&name);
        let before = bytes_below(&old);
        let service = if across { lab.cross() } else { lab.change() };
        let applied = service
            .apply(&[id], share_archive.id, &mut no_progress())
            .unwrap();
        assert_eq!(applied.moved(), 1, "across {across}: {applied:?}");
        let new = Path::new(&share_archive.path).join(&name);
        assert_eq!(bytes_below(&new), before, "across {across}");
        assert_eq!(lab.item(id).core.path, new.to_string_lossy());
        let undone = OrganizeService::new(Arc::clone(&lab.db))
            .undo(applied.batch_id.as_deref().unwrap())
            .unwrap();
        assert!(undone.is_complete(), "across {across}: {undone:?}");
        assert_eq!(bytes_below(&old), before, "across {across}");
        assert_eq!(lab.item(id).core.data_source, share_movies.path);
    }
}

// ---- the title in another case at the destination ------------------------------------------------

#[cfg(any(windows, target_os = "macos"))]
#[test]
fn a_destination_that_holds_the_title_in_another_case_is_refused_and_nothing_is_touched() {
    let lab = Lab::new();
    let id = lab.movie(&lab.movies, "Heat (1995)");
    let old = Path::new(&lab.movies.path).join("Heat (1995)");
    let before = bytes_below(&old);
    // Someone keeps "heat (1995)" in the archive already.
    Lab::put(
        Path::new(&lab.archive.path),
        "heat (1995)/heat.mkv",
        "theirs",
    );
    let theirs = bytes_below(Path::new(&lab.archive.path));

    for service in [lab.cross(), lab.change()] {
        let applied = service
            .apply(&[id], lab.archive.id, &mut no_progress())
            .unwrap();
        assert_eq!(applied.moved(), 0, "{applied:?}");
        assert_eq!(
            applied.items[0].result,
            MoveResult::Skipped(MoveStatus::DestinationTaken),
            "{applied:?}"
        );
        assert_eq!(bytes_below(&old), before);
        assert_eq!(bytes_below(Path::new(&lab.archive.path)), theirs);
        assert_eq!(lab.item(id).core.data_source, lab.movies.path);
    }
}
