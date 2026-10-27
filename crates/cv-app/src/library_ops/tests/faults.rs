//! The move engine under the shared fault-injection file system: a kill at every
//! operation of a move, an error at every operation, a kill in the middle of a copy, and access
//! denied on the trash step. After the restart, Resume or Roll back, every test finds the title
//! whole in exactly one place, never a mix: every original intact in the old folder with the
//! library pointing at it, or the move complete and verified with the originals in the trash
//! folder, and the journal saying so.
//!
//! A full destination during a copy is `recovery::a_full_destination_*` (through the same shim).

use std::io;
use std::panic::AssertUnwindSafe;

use cv_db::repo::rename_journal as table;
use cv_rename::exec::fault::{Fault, Op, Rule, When, is_stop};

use super::moves::{FaultFs, Faults, SharedFaultFs};
use super::*;
use crate::library_ops::RecoveryService;

/// Where the title is, whole.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Whole {
    Old,
    New,
}

/// How the user finishes an unfinished move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Finish {
    Resume,
    RollBack,
}

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

/// Every name below `dir`, the trash folder included.
fn all_names(dir: &Path) -> Vec<String> {
    fn walk(dir: &Path, out: &mut Vec<String>) {
        let Ok(read) = fs::read_dir(dir) else { return };
        for entry in read.flatten() {
            out.push(entry.path().to_string_lossy().replace('\\', "/"));
            if entry.path().is_dir() {
                walk(&entry.path(), out);
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, &mut out);
    out
}

/// A title of four files (a 1 MiB video in several chunks, an NFO, a poster and an extra in a
/// subfolder) to move from `movies` to `archive`.
struct Case {
    lab: Lab,
    id: ItemId,
    before: Vec<(String, Vec<u8>)>,
    old: PathBuf,
    new: PathBuf,
}

impl Case {
    fn new() -> Self {
        let lab = Lab::new();
        let id = lab.movie(&lab.movies, "Heat (1995)");
        let old = Path::new(&lab.movies.path).join("Heat (1995)");
        // Bytes that differ along the file, so a chunk in the wrong place is seen.
        let video: Vec<u8> = (0..1024 * 1024_u32).map(|i| (i % 251) as u8).collect();
        fs::write(old.join("Heat (1995).mkv"), video).unwrap();
        Lab::put(&old, "extras/making-of.mkv", "making of");
        let new = Path::new(&lab.archive.path).join("Heat (1995)");
        let before = bytes_below(&old);
        assert_eq!(before.len(), 4);
        Case {
            lab,
            id,
            before,
            old,
            new,
        }
    }

    /// Moves the title over `disk` (on "another volume") and returns what the run did; a stop of
    /// the shim is caught and is `None`.
    fn run(&self, disk: &Arc<FaultFs>) -> Option<MoveApplied> {
        let service = self.lab.cross_with(disk);
        match std::panic::catch_unwind(AssertUnwindSafe(|| {
            service.apply(&[self.id], self.lab.archive.id, &mut no_progress())
        })) {
            Ok(applied) => Some(applied.unwrap()),
            Err(payload) => {
                assert!(is_stop(payload.as_ref()), "a real panic, not a stop");
                None
            }
        }
    }

    fn move_batches(&self) -> Vec<String> {
        self.lab
            .db
            .read(table::batch_summaries)
            .unwrap()
            .into_iter()
            .map(|b| b.batch_id)
            .filter(|b| b.starts_with("move-"))
            .collect()
    }

    /// The originals in the trash folder of the old source.
    fn trashed_originals(&self) -> Vec<(String, Vec<u8>)> {
        let mut out = Vec::new();
        for batch in self.move_batches() {
            out.extend(bytes_below(
                &Path::new(&self.lab.movies.path)
                    .join(".deletedByCineVault")
                    .join(batch)
                    .join("Heat (1995)"),
            ));
        }
        out.sort();
        out
    }

    /// Where the title is whole, or `None` when it is a mix.
    fn whole(&self) -> Option<Whole> {
        let item = self.lab.item(self.id);
        let staged: Vec<String> = all_names(Path::new(&self.lab.archive.path))
            .into_iter()
            .filter(|n| !n.contains(".deletedByCineVault"))
            .filter(|n| n.contains(".cvmove-") || n.contains(".cvpart"))
            .collect();
        let at_old = bytes_below(&self.old) == self.before
            && !self.new.exists()
            && staged.is_empty()
            && item.core.data_source == self.lab.movies.path
            && Path::new(&item.core.path) == self.old;
        let at_new = bytes_below(&self.new) == self.before
            && bytes_below(&self.old).is_empty()
            && staged.is_empty()
            && self.trashed_originals() == self.before
            && item.core.data_source == self.lab.archive.path
            && Path::new(&item.core.path) == self.new
            && item
                .core
                .media_files
                .iter()
                .all(|f| Path::new(&f.directory).starts_with(&self.new));
        match (at_old, at_new) {
            (true, false) => Some(Whole::Old),
            (false, true) => Some(Whole::New),
            _ => None,
        }
    }

    fn describe(&self) -> String {
        format!(
            "old {:?}\nnew {:?}\narchive {:?}\nitem {}",
            bytes_below(&self.old)
                .iter()
                .map(|(n, b)| (n, b.len()))
                .collect::<Vec<_>>(),
            bytes_below(&self.new)
                .iter()
                .map(|(n, b)| (n, b.len()))
                .collect::<Vec<_>>(),
            all_names(Path::new(&self.lab.archive.path)),
            self.lab.item(self.id).core.path,
        )
    }

    /// The restart after the fault: start-up recovery, then the unfinished move (if any) is
    /// finished the way the user chose with a healthy disk. Returns where the title ended up.
    fn settle(&self, disk: &Arc<FaultFs>, finish: Finish, label: &str) -> Whole {
        disk.heal();
        crate::organize::recover_interrupted_batches(&self.lab.db).unwrap();
        let service = RecoveryService::new(Arc::clone(&self.lab.db))
            .with_disk(Arc::new(SharedFaultFs(Arc::clone(disk))));
        let found = service.find().unwrap();
        assert!(found.len() <= 1, "{label}: {found:?}");
        if let Some(batch) = found.first() {
            match finish {
                Finish::Resume => {
                    let done = service
                        .resume(&batch.batch_id, None, &mut no_progress())
                        .unwrap();
                    assert_eq!(done.stopped, None, "{label}: {done:?}");
                }
                Finish::RollBack => {
                    let back = service.roll_back(&batch.batch_id).unwrap();
                    assert!(back.problems.is_empty(), "{label}: {back:?}");
                }
            }
        }
        assert!(service.find().unwrap().is_empty(), "{label}: still offered");
        let whole = self
            .whole()
            .unwrap_or_else(|| panic!("{label}: a mix\n{}", self.describe()));
        if !found.is_empty() {
            let want = match finish {
                Finish::Resume => Whole::New,
                Finish::RollBack => Whole::Old,
            };
            assert_eq!(whole, want, "{label}");
        }
        // The journal says what the disk shows.
        for batch in self.move_batches() {
            let rows = self.lab.journal(&batch);
            assert!(
                rows.iter().all(|r| r.state != "planned"),
                "{label}: {rows:?}"
            );
            match whole {
                Whole::New => assert!(rows.iter().all(|r| r.state == "done"), "{label}: {rows:?}"),
                Whole::Old => assert!(rows.iter().all(|r| r.state != "done"), "{label}: {rows:?}"),
            }
        }
        whole
    }
}

/// The operations that change the disk in an uninterrupted move of the case.
fn operations_of_a_move() -> usize {
    let case = Case::new();
    let disk = FaultFs::new(Faults::default());
    let applied = case.run(&disk).unwrap();
    assert_eq!(applied.moved(), 1, "{applied:?}");
    assert_eq!(case.whole(), Some(Whole::New), "{}", case.describe());
    let total = disk.shim().ops();
    assert!(total > 10, "the move should do some work: {total}");
    total
}

#[test]
fn a_kill_at_any_operation_of_a_move_is_resumed_or_rolled_back_to_one_whole_place() {
    let total = operations_of_a_move();
    for at in 0..total {
        for when in [When::Before, When::After] {
            for finish in [Finish::Resume, Finish::RollBack] {
                let label = format!("kill {when:?} operation {at}, then {finish:?}");
                let case = Case::new();
                let disk = FaultFs::new(Faults::default());
                disk.shim().stop_at(at, when);
                assert!(case.run(&disk).is_none(), "{label}: the run was killed");
                // Before the restart the originals are all still somewhere: none was removed.
                let mut seen: Vec<Vec<u8>> = Vec::new();
                for root in [&case.lab.movies.path, &case.lab.archive.path] {
                    for name in all_names(Path::new(root)) {
                        if Path::new(&name).is_file() {
                            seen.push(fs::read(&name).unwrap());
                        }
                    }
                }
                for (name, bytes) in &case.before {
                    assert!(seen.contains(bytes), "{label}: {name} was lost");
                }
                case.settle(&disk, finish, &label);
            }
        }
    }
}

#[test]
fn an_error_at_any_operation_of_a_move_ends_in_one_whole_place() {
    let total = operations_of_a_move();
    for at in 0..total {
        for finish in [Finish::Resume, Finish::RollBack] {
            let label = format!("error at operation {at}, then {finish:?}");
            let case = Case::new();
            let disk = FaultFs::new(Faults::default());
            disk.shim()
                .fail_at(at, Fault::Kind(io::ErrorKind::PermissionDenied));
            let applied = case.run(&disk).expect("an error is not a kill");
            match &applied.items[0].result {
                // A title that moved is whole in its new place, even when an original could not
                // go to the trash folder (it is told, and Resume finishes it).
                MoveResult::Moved => {}
                MoveResult::Stopped(why) | MoveResult::Failed(why) => {
                    assert!(!why.is_empty(), "{label}");
                }
                other => panic!("{label}: {other:?}"),
            }
            case.settle(&disk, finish, &label);
        }
    }
}

#[test]
fn a_kill_in_the_middle_of_a_copy_is_resumed_or_rolled_back() {
    const MIB: u64 = 1024 * 1024;
    // In the first chunk of the video, in its middle, one byte before its end, and in the small
    // files after it.
    for bytes in [1, MIB / 2 + 7, MIB - 1, MIB + 3, MIB + 12] {
        for finish in [Finish::Resume, Finish::RollBack] {
            let label = format!("kill after {bytes} bytes, then {finish:?}");
            let case = Case::new();
            let disk = FaultFs::new(Faults::default());
            disk.shim().stop_after_bytes(bytes);
            assert!(case.run(&disk).is_none(), "{label}: the run was killed");
            // A half-written copy is in the hidden staging folder; the originals are untouched.
            let parts: Vec<String> = all_names(Path::new(&case.lab.archive.path))
                .into_iter()
                .filter(|n| n.ends_with(".cvpart"))
                .collect();
            assert_eq!(parts.len(), 1, "{label}: {parts:?}");
            assert!(parts[0].contains(".cvmove-"), "{label}: {parts:?}");
            assert_eq!(bytes_below(&case.old), case.before, "{label}");
            assert_eq!(
                case.lab.item(case.id).core.data_source,
                case.lab.movies.path
            );
            assert_eq!(disk.shim().bytes(), bytes, "{label}");

            case.settle(&disk, finish, &label);
        }
    }
}

#[test]
fn access_denied_on_the_trash_step_stops_the_move_and_resume_or_roll_back_ends_it() {
    for finish in [Finish::Resume, Finish::RollBack] {
        let label = format!("{finish:?}");
        let case = Case::new();
        let disk = FaultFs::new(Faults::default());
        let deny =
            || Rule::on(&[Op::Rename], Fault::PermissionDenied).into("/.deletedByCineVault/");
        disk.shim().rule(deny());

        let applied = case.run(&disk).unwrap();

        // The title is whole and verified in the new place and the library points there; the
        // originals the system would not let go of are still in the old folder, none removed.
        // That is not a finished move: it is reported stopped, with the reason.
        let MoveResult::Stopped(why) = &applied.items[0].result else {
            panic!("{label}: {applied:?}");
        };
        assert!(
            why.contains("not allowed") && why.contains("resume"),
            "{label}: {why}"
        );
        assert_eq!(bytes_below(&case.new), case.before, "{label}");
        assert_eq!(bytes_below(&case.old), case.before, "{label}");
        assert_eq!(
            case.lab.item(case.id).core.data_source,
            case.lab.archive.path
        );
        // The journal keeps the trash rows open, so Activity offers the batch.
        let batch = applied.batch_id.clone().unwrap();
        let rows = case.lab.journal(&batch);
        let open: Vec<_> = rows
            .iter()
            .filter(|r| r.op == "trash" && r.state == "failed")
            .collect();
        assert_eq!(open.len(), 4, "{label}: {rows:?}");
        assert!(
            open.iter().all(|r| r.error.starts_with("stopped: ")),
            "{label}: {rows:?}"
        );
        assert!(
            rows.iter()
                .filter(|r| r.op == "copy")
                .all(|r| r.state == "done"),
            "{label}: {rows:?}"
        );
        let service = RecoveryService::new(Arc::clone(&case.lab.db))
            .with_disk(Arc::new(SharedFaultFs(Arc::clone(&disk))));
        assert_eq!(service.find().unwrap().len(), 1, "{label}");

        // Still denied: Resume stops again and changes nothing.
        if finish == Finish::Resume {
            let again = service.resume(&batch, None, &mut no_progress()).unwrap();
            assert!(
                again.stopped.is_some() || !again.notes.is_empty(),
                "{label}: {again:?}"
            );
            assert_eq!(bytes_below(&case.old), case.before, "{label}");
            assert_eq!(bytes_below(&case.new), case.before, "{label}");
            assert_eq!(service.find().unwrap().len(), 1, "{label}");
        }

        let whole = case.settle(&disk, finish, &label);
        assert_eq!(
            whole,
            match finish {
                Finish::Resume => Whole::New,
                Finish::RollBack => Whole::Old,
            }
        );
    }
}
