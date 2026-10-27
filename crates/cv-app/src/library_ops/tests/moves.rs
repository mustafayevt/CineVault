//! Tests of the move engine through `ChangeSourceService`: a copy to another volume
//! is simulated by a volume probe that says the folders are on different drives, and the disk
//! is the real one, wrapped to fail on purpose.

use std::io::{self, Read, Write};
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use cv_db::repo::rename_journal as table;
use cv_db::repo::watched::{WatchState, set_watch_state};
use cv_db::repo::{get_item, update_item};
use cv_domain::id_key;
use cv_rename::exec::fault::{self, Fault, Op, Rule};
use cv_rename::exec::{Fs, Meta};

use super::*;
use crate::library_ops::{MoveControl, MoveFs, MovePhase, StdMoveFs};

// ---- a disk that fails on purpose ----------------------------------------------------------------

/// The shared fault-injection file system (`cv_rename::exec::fault`) over the real
/// disk of the move engine.
pub(super) type Shim = fault::FaultFs<StdMoveFs>;

/// The move engine's streams through the shim: rules for [`Op::Open`] and [`Op::Create`] apply,
/// and what is written counts against its byte limits (a stop or a full disk after N bytes).
impl MoveFs for Shim {
    fn open_read(&self, path: &Path) -> io::Result<Box<dyn Read + Send>> {
        self.open(path, || self.inner().open_read(path))
    }

    fn create_write(
        &self,
        path: &Path,
    ) -> io::Result<Box<dyn crate::library_ops::move_engine::CopyWrite>> {
        let writer = self.create(path, || self.inner().create_write(path))?;
        Ok(Box::new(ShimWrite(writer)))
    }
    fn keep_read_only(&self, from: &Path, to: &Path) -> io::Result<()> {
        self.inner().keep_read_only(from, to)
    }
}

struct ShimWrite(fault::FaultWriter<Box<dyn crate::library_ops::move_engine::CopyWrite>>);

impl Write for ShimWrite {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

impl crate::library_ops::move_engine::CopyWrite for ShimWrite {
    fn finish(self: Box<Self>, modified_ms: i64) -> io::Result<()> {
        self.0.into_inner().finish(modified_ms)
    }
}

type CreateHook = Box<dyn Fn(&Path) + Send + Sync>;

/// Every write to one file, in order.
pub(super) type WriteLog = Arc<Mutex<Vec<Vec<u8>>>>;

/// What the disk does wrong. The general faults (a full disk, a file in use, access denied,
/// passing errors, across devices) become rules of the shared shim; the hooks that need to know
/// the engine's steps (a kill at the n-th part or trash rename, at the staging rename) stay here.
#[derive(Default)]
pub(super) struct Faults {
    /// Writes fail with "the disk is full" once this many bytes were written in all.
    pub disk_full_after: Option<u64>,
    /// The file with this name gets one byte flipped in its copy.
    pub corrupt: Option<String>,
    /// Called with the path of every file the engine starts to write.
    pub on_create: Option<CreateHook>,
    /// Cancels the move when the file with this name is about to be written.
    pub cancel_on_create: Option<(String, Arc<MoveControl>)>,
    /// Panics (a killed process) at the n-th rename of a `.cvpart` file into place.
    pub kill_at_part_rename: Option<usize>,
    /// Panics (a killed process) when the n-th original is about to go to the trash folder.
    pub kill_at_trash: Option<usize>,
    /// Panics right after the staging folder was renamed to the destination, before the journal
    /// says so.
    pub kill_after_stage_rename: bool,
    /// Moving a file with this name into the trash folder fails with "in use".
    pub busy_original: Option<String>,
    /// Creating a file with this name fails with "access denied".
    pub deny_write: Option<String>,
    /// Renaming a folder with this name says the two places are on different volumes.
    pub cross_device_folder: Option<String>,
    /// Panics right after a folder with this name was renamed, before the journal says so.
    pub kill_after_folder_rename: Option<String>,
    /// The next this many renames fail with a passing "in use" error.
    pub transient_renames: usize,
    /// The bytes the engine reads and writes at a time (its pipeline chunks).
    pub chunk: Option<usize>,
    /// Reading back the copy of the file with this name gives one byte flipped (the disk kept
    /// something else than what was written).
    pub corrupt_read_back: Option<String>,
    /// Cancels the move after this many writes to the file with this name.
    pub cancel_after_writes: Option<(String, usize, Arc<MoveControl>)>,
    /// Records the length of every write to the file with this name, in order.
    pub record_writes: Option<(String, WriteLog)>,
}

pub(super) struct FaultFs {
    shim: Shim,
    pub faults: Mutex<Faults>,
    part_renames: AtomicUsize,
    trash_renames: AtomicUsize,
}

fn into_trash(to: Option<&Path>) -> bool {
    to.is_some_and(|to| to.to_string_lossy().contains(".deletedByCineVault"))
}

impl FaultFs {
    pub(super) fn new(faults: Faults) -> Arc<Self> {
        let shim = Shim::over(StdMoveFs);
        if let Some(limit) = faults.disk_full_after {
            shim.disk_full_after(limit);
        }
        if let Some(busy) = faults.busy_original.clone() {
            shim.rule(Rule::on(&[Op::Rename], Fault::InUse).when(move |from, to| {
                into_trash(to) && from.file_name().is_some_and(|n| *n == *busy.as_str())
            }));
        }
        if let Some(name) = &faults.cross_device_folder {
            shim.rule(Rule::on(&[Op::Rename], Fault::CrossesDevices).named(name));
        }
        if faults.transient_renames > 0 {
            shim.rule(Rule::on(&[Op::Rename], Fault::InUse).times(faults.transient_renames));
        }
        if let Some(name) = &faults.deny_write {
            shim.rule(
                Rule::on(&[Op::Create], Fault::Kind(io::ErrorKind::PermissionDenied)).named(name),
            );
        }
        Arc::new(Self {
            shim,
            faults: Mutex::new(faults),
            part_renames: AtomicUsize::new(0),
            trash_renames: AtomicUsize::new(0),
        })
    }

    /// The shared shim, for faults the `Faults` above do not name (a stop after N operations or
    /// N bytes, rules on any operation and path).
    pub(super) fn shim(&self) -> &Shim {
        &self.shim
    }

    /// Stops failing: the disk after room was made.
    pub(super) fn heal(&self) {
        *self.faults.lock().unwrap() = Faults::default();
        self.shim.heal();
    }
}

impl Fs for FaultFs {
    fn meta(&self, path: &Path) -> io::Result<Option<Meta>> {
        self.shim.meta(path)
    }
    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        self.shim.create_dir_all(path)
    }
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        let name = from.file_name().unwrap().to_string_lossy().into_owned();
        let (kill_folder, kill_after) = {
            let faults = self.faults.lock().unwrap();
            if name.ends_with(".cvpart")
                && let Some(kill) = faults.kill_at_part_rename
                && self.part_renames.fetch_add(1, Ordering::SeqCst) + 1 == kill
            {
                drop(faults);
                panic!("simulated kill");
            }
            if let Some(kill) = faults.kill_at_trash
                && into_trash(Some(to))
                && self.trash_renames.fetch_add(1, Ordering::SeqCst) + 1 == kill
            {
                drop(faults);
                panic!("simulated kill while the originals went to the trash folder");
            }
            (
                faults.kill_after_folder_rename.as_deref() == Some(name.as_str()),
                name.starts_with(".cvmove-") && faults.kill_after_stage_rename,
            )
        };
        self.shim.rename(from, to)?;
        if kill_folder {
            panic!("simulated kill after the folder was renamed");
        }
        if kill_after {
            panic!("simulated kill after the staging folder was put in place");
        }
        Ok(())
    }
    fn copy_file(&self, from: &Path, to: &Path) -> io::Result<()> {
        self.shim.copy_file(from, to)
    }
    fn same_content(&self, a: &Path, b: &Path) -> io::Result<bool> {
        self.shim.same_content(a, b)
    }
    fn remove_file(&self, path: &Path) -> io::Result<()> {
        self.shim.remove_file(path)
    }
    fn remove_dir(&self, path: &Path) -> io::Result<()> {
        self.shim.remove_dir(path)
    }
    fn list_dir(&self, path: &Path) -> io::Result<Vec<String>> {
        self.shim.list_dir(path)
    }
}

/// Gives the first byte of a file flipped.
struct FlipFirst {
    inner: Box<dyn Read + Send>,
    flip: bool,
}

impl Read for FlipFirst {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        if self.flip && n > 0 {
            self.flip = false;
            buf[0] ^= 0xFF;
        }
        Ok(n)
    }
}

/// Flips the first byte of a copy; cancels the move after some writes; records the writes.
struct Corrupting {
    inner: Box<dyn crate::library_ops::move_engine::CopyWrite>,
    corrupt: bool,
    cancel: Option<(usize, Arc<MoveControl>)>,
    record: Option<WriteLog>,
}

impl Write for Corrupting {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if let Some(log) = &self.record {
            log.lock().unwrap().push(buf.to_vec());
        }
        if let Some((left, control)) = &mut self.cancel {
            *left = left.saturating_sub(1);
            if *left == 0 {
                control.cancel();
            }
        }
        if self.corrupt && !buf.is_empty() {
            self.corrupt = false;
            let mut flipped = buf.to_vec();
            flipped[0] ^= 0xFF;
            self.inner.write_all(&flipped)?;
            return Ok(buf.len());
        }
        self.inner.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl crate::library_ops::move_engine::CopyWrite for Corrupting {
    fn finish(self: Box<Self>, modified_ms: i64) -> io::Result<()> {
        self.inner.finish(modified_ms)
    }
}

/// A `MoveFs` that shares its faults with the test.
pub(super) struct SharedFaultFs(pub Arc<FaultFs>);

impl Fs for SharedFaultFs {
    fn meta(&self, path: &Path) -> io::Result<Option<Meta>> {
        self.0.meta(path)
    }
    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        self.0.create_dir_all(path)
    }
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        self.0.rename(from, to)
    }
    fn copy_file(&self, from: &Path, to: &Path) -> io::Result<()> {
        self.0.copy_file(from, to)
    }
    fn same_content(&self, a: &Path, b: &Path) -> io::Result<bool> {
        self.0.same_content(a, b)
    }
    fn remove_file(&self, path: &Path) -> io::Result<()> {
        self.0.remove_file(path)
    }
    fn remove_dir(&self, path: &Path) -> io::Result<()> {
        self.0.remove_dir(path)
    }
    fn list_dir(&self, path: &Path) -> io::Result<Vec<String>> {
        self.0.list_dir(path)
    }
}

impl MoveFs for SharedFaultFs {
    fn open_read(&self, path: &Path) -> io::Result<Box<dyn Read + Send>> {
        self.0.shim.open_read(path)
    }

    fn create_write(
        &self,
        path: &Path,
    ) -> io::Result<Box<dyn crate::library_ops::move_engine::CopyWrite>> {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let bare = name.strip_suffix(".cvpart").unwrap_or(&name).to_owned();
        let corrupt;
        {
            let faults = self.0.faults.lock().unwrap();
            if let Some(hook) = &faults.on_create {
                hook(path);
            }
            if let Some((target, control)) = &faults.cancel_on_create
                && *target == bare
            {
                control.cancel();
            }
            corrupt = faults.corrupt.as_deref() == Some(bare.as_str());
        }
        let (cancel, record) = {
            let faults = self.0.faults.lock().unwrap();
            (
                faults
                    .cancel_after_writes
                    .clone()
                    .filter(|(target, _, _)| *target == bare)
                    .map(|(_, n, control)| (n, control)),
                faults
                    .record_writes
                    .clone()
                    .filter(|(target, _)| *target == bare)
                    .map(|(_, log)| log),
            )
        };
        let inner = self.0.shim.create_write(path)?;
        Ok(Box::new(Corrupting {
            inner,
            corrupt,
            cancel,
            record,
        }))
    }

    fn open_verify(&self, path: &Path) -> io::Result<Box<dyn Read + Send>> {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let bare = name.strip_suffix(".cvpart").unwrap_or(&name).to_owned();
        let flip = self.0.faults.lock().unwrap().corrupt_read_back.as_deref() == Some(&bare);
        let inner = self.0.shim.inner().open_verify(path)?;
        Ok(Box::new(FlipFirst { inner, flip }))
    }

    fn chunk_bytes(&self) -> usize {
        self.0
            .faults
            .lock()
            .unwrap()
            .chunk
            .unwrap_or(crate::library_ops::move_engine::CHUNK_BYTES)
    }
    fn keep_read_only(&self, from: &Path, to: &Path) -> io::Result<()> {
        self.0.shim.keep_read_only(from, to)
    }
}

// ---- fixtures -------------------------------------------------------------------------------------

impl Lab {
    /// A movie folder with a 3 MB video (several chunks), an NFO, a poster, subtitles, a theme,
    /// a hidden file and an `extras` folder with a nested folder, plus an empty folder.
    pub(super) fn big_movie(&self, source: &DataSource, name: &str) -> ItemId {
        let id = self.movie(source, name);
        let folder = Path::new(&source.path).join(name);
        fs::write(
            folder.join(format!("{name}.mkv")),
            vec![7_u8; 3 * 1024 * 1024],
        )
        .unwrap();
        for (rel, content) in [
            (format!("{name}.en.srt"), "subtitle"),
            ("theme.mp3".to_owned(), "theme"),
            (".hidden".to_owned(), "h"),
            ("extras/making-of.mkv".to_owned(), "making of"),
            ("extras/deleted/scene.mkv".to_owned(), "scene"),
        ] {
            Lab::put(&folder, &rel, content);
        }
        fs::create_dir_all(folder.join("empty-folder")).unwrap();
        id
    }

    pub(super) fn cross(&self) -> ChangeSourceService {
        self.change().with_volume_probe(|_, _| false)
    }

    pub(super) fn cross_with(&self, fs: &Arc<FaultFs>) -> ChangeSourceService {
        self.cross()
            .with_disk(Arc::new(SharedFaultFs(Arc::clone(fs))))
    }

    pub(super) fn journal(&self, batch: &str) -> Vec<table::JournalRow> {
        self.db.read(|c| table::batch_rows(c, batch)).unwrap()
    }
}

/// Every file below `dir` with its content, relative to `dir`.
fn tree(dir: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        let Ok(read) = fs::read_dir(dir) else { return };
        for entry in read.flatten() {
            let path = entry.path();
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

fn names_below(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(dir: &Path, out: &mut Vec<String>) {
        let Ok(read) = fs::read_dir(dir) else { return };
        for entry in read.flatten() {
            out.push(entry.path().to_string_lossy().into_owned());
            if entry.path().is_dir() {
                walk(&entry.path(), out);
            }
        }
    }
    walk(dir, &mut out);
    out.sort();
    out
}

// ---- tests ---------------------------------------------------------------------------------------

#[test]
fn a_verified_copy_keeps_the_row_id_matches_and_watched_state_and_the_originals_go_to_the_trash() {
    let lab = Lab::new();
    let heat = lab.big_movie(&lab.movies, "Heat (1995)");
    // The title has a match and a watched state that must survive.
    lab.db
        .write(move |tx| {
            let mut item = get_item(tx, heat)?.unwrap();
            item.core.ids.set(id_key::TMDB, "949");
            update_item(tx, &item)?;
            set_watch_state(
                tx,
                &WatchState {
                    id: heat,
                    watched: true,
                    play_count: 3,
                    last_watched_ms: Some(1_700_000_000_000),
                },
            )
        })
        .unwrap();
    let old_folder = Path::new(&lab.movies.path).join("Heat (1995)");
    let before = tree(&old_folder);
    assert_eq!(before.len(), 8);

    // While files are copied the destination does not exist: the copy is in a hidden folder.
    let archive = PathBuf::from(&lab.archive.path);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let disk = FaultFs::new(Faults {
        on_create: Some(Box::new({
            let seen = Arc::clone(&seen);
            let archive = archive.clone();
            move |path| {
                seen.lock().unwrap().push((
                    archive.join("Heat (1995)").exists(),
                    path.to_string_lossy().into_owned(),
                ));
            }
        })),
        ..Faults::default()
    });
    let mut events = Vec::new();
    let applied = lab
        .cross_with(&disk)
        .apply(&[heat], lab.archive.id, &mut |p| events.push(p.clone()))
        .unwrap();

    assert_eq!(applied.moved(), 1, "{applied:?}");
    let batch = applied.batch_id.clone().unwrap();
    assert!(crate::library_ops::is_move_batch(&batch));
    assert!(
        seen.lock().unwrap().iter().all(|(placed, path)| !placed
            && path.contains(".cvmove-")
            && path.ends_with(".cvpart")),
        "{:?}",
        seen.lock().unwrap()
    );

    // The same row, with its match and watched state, now points at the new folder.
    let item = lab.item(heat);
    let new_folder = archive.join("Heat (1995)");
    assert_eq!(item.core.data_source, lab.archive.path);
    assert_eq!(PathBuf::from(&item.core.path), new_folder);
    assert_eq!(item.core.ids.get(id_key::TMDB), Some("949"));
    assert!(lab.exists(heat));
    let state = lab
        .db
        .read(|c| cv_db::repo::get_watch_state(c, heat))
        .unwrap()
        .unwrap();
    assert!(state.watched && state.play_count == 3);
    assert!(
        item.core
            .media_files
            .iter()
            .all(|f| Path::new(&f.directory) == new_folder),
        "{:?}",
        item.core.media_files
    );

    // Every file arrived with its bytes, and nothing else is in the new place.
    assert_eq!(tree(&new_folder), before);
    assert!(
        new_folder.join("empty-folder").is_dir(),
        "empty folders arrive too"
    );
    let leftovers: Vec<String> = names_below(&archive)
        .into_iter()
        .filter(|n| n.contains(".cvpart") || n.contains(".cvmove-"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");

    // The originals are in the trash folder of the old source, none deleted, none left behind.
    assert!(!old_folder.exists(), "nothing is left in the old folder");
    let trash = Path::new(&lab.movies.path)
        .join(".deletedByCineVault")
        .join(&batch)
        .join("Heat (1995)");
    assert_eq!(tree(&trash), before);

    // The journal says copy x8, one move, trash x8, all done.
    let rows = lab.journal(&batch);
    assert_eq!(rows.len(), 8 + 1 + 8);
    assert!(rows.iter().all(|r| r.state == "done"), "{rows:?}");
    assert_eq!(rows.iter().filter(|r| r.op == "copy").count(), 8);
    assert_eq!(rows.iter().filter(|r| r.op == "trash").count(), 8);

    // Progress named the file and counted the bytes.
    let last = events.last().unwrap();
    assert_eq!((last.done, last.total), (1, 1));
    assert_eq!(last.bytes_done, last.bytes_total);
    assert!(last.bytes_total >= 3 * 1024 * 1024);
    assert!(
        events
            .iter()
            .any(|e| e.phase == MovePhase::Copying && e.current_file.ends_with("Heat (1995).mkv"))
    );
    assert!(events.iter().any(|e| e.phase == MovePhase::Verifying));
}

#[test]
fn a_hash_mismatch_stops_with_the_originals_intact_and_the_copies_removed() {
    let lab = Lab::new();
    let heat = lab.big_movie(&lab.movies, "Heat (1995)");
    let ronin = lab.movie(&lab.movies, "Ronin (1998)");
    let old_folder = Path::new(&lab.movies.path).join("Heat (1995)");
    let before = tree(&old_folder);
    let disk = FaultFs::new(Faults {
        corrupt: Some("making-of.mkv".to_owned()),
        ..Faults::default()
    });

    let applied = lab
        .cross_with(&disk)
        .apply(&[heat, ronin], lab.archive.id, &mut no_progress())
        .unwrap();

    assert_eq!(applied.moved(), 0, "{applied:?}");
    let heat_outcome = applied.items.iter().find(|i| i.id == heat).unwrap();
    let MoveResult::Failed(why) = &heat_outcome.result else {
        panic!("{:?}", heat_outcome.result);
    };
    assert!(
        why.contains("making-of.mkv"),
        "the failed file is named: {why}"
    );
    // The batch stopped: the second title did not start.
    let ronin_outcome = applied.items.iter().find(|i| i.id == ronin).unwrap();
    assert!(matches!(ronin_outcome.result, MoveResult::Stopped(_)));

    // The originals are whole where they were; the copies are gone.
    assert_eq!(tree(&old_folder), before);
    assert!(Path::new(&lab.movies.path).join("Ronin (1998)").is_dir());
    assert!(
        names_below(Path::new(&lab.archive.path)).is_empty(),
        "{:?}",
        names_below(Path::new(&lab.archive.path))
    );
    assert!(
        !Path::new(&lab.movies.path)
            .join(".deletedByCineVault")
            .exists()
    );
    // The library still points at the originals.
    let item = lab.item(heat);
    assert_eq!(item.core.data_source, lab.movies.path);
    assert_eq!(PathBuf::from(&item.core.path), old_folder);
    // The journal has nothing half done for the title that failed.
    let batch = applied.batch_id.unwrap();
    let rows = lab.journal(&batch);
    let heat_rows: Vec<_> = rows.iter().filter(|r| r.item_id == Some(heat.0)).collect();
    assert!(
        heat_rows.iter().all(|r| r.state != "planned"),
        "{heat_rows:?}"
    );
    assert!(heat_rows.iter().all(|r| r.state != "done"), "{heat_rows:?}");
}

#[test]
fn cancel_in_the_middle_of_the_copy_leaves_the_original_state() {
    let lab = Lab::new();
    let heat = lab.big_movie(&lab.movies, "Heat (1995)");
    let old_folder = Path::new(&lab.movies.path).join("Heat (1995)");
    let before = tree(&old_folder);
    let control = MoveControl::new();
    let disk = FaultFs::new(Faults {
        cancel_on_create: Some(("making-of.mkv".to_owned(), Arc::clone(&control))),
        ..Faults::default()
    });

    let applied = lab
        .cross_with(&disk)
        .apply_with(&[heat], lab.archive.id, Some(control), &mut no_progress())
        .unwrap();

    assert_eq!(applied.moved(), 0);
    assert_eq!(applied.items[0].result, MoveResult::Canceled);
    assert_eq!(tree(&old_folder), before);
    assert!(names_below(Path::new(&lab.archive.path)).is_empty());
    assert_eq!(lab.item(heat).core.data_source, lab.movies.path);
    let rows = lab.journal(&applied.batch_id.unwrap());
    assert!(
        rows.iter()
            .all(|r| r.state != "planned" && r.state != "done"),
        "{rows:?}"
    );
}

#[test]
fn undo_restores_the_old_source_and_the_library() {
    let lab = Lab::new();
    let heat = lab.big_movie(&lab.movies, "Heat (1995)");
    let old_folder = Path::new(&lab.movies.path).join("Heat (1995)");
    let before = tree(&old_folder);
    let applied = lab
        .cross()
        .apply(&[heat], lab.archive.id, &mut no_progress())
        .unwrap();
    let batch = applied.batch_id.unwrap();
    assert!(!old_folder.exists());

    let outcome = OrganizeService::new(Arc::clone(&lab.db))
        .undo(&batch)
        .unwrap();

    assert_eq!(outcome.restored, 1, "{outcome:?}");
    assert!(outcome.is_complete());
    assert_eq!(
        tree(&old_folder),
        before,
        "every original is back, byte for byte"
    );
    let item = lab.item(heat);
    assert_eq!(item.core.data_source, lab.movies.path);
    assert_eq!(PathBuf::from(&item.core.path), old_folder);
    assert!(
        item.core
            .media_files
            .iter()
            .all(|f| Path::new(&f.directory) == old_folder)
    );
    // The new place is empty; the copies went to the trash folder of the destination.
    let archive = Path::new(&lab.archive.path);
    assert!(!archive.join("Heat (1995)").exists());
    let undo_trash = archive
        .join(".deletedByCineVault")
        .join(format!("{batch}-undo"));
    assert_eq!(tree(&undo_trash).len(), 8, "{:?}", names_below(archive));
    // The journal follows: nothing is `done` any more.
    assert!(lab.journal(&batch).iter().all(|r| r.state == "undone"));
    // And the batch cannot be undone twice.
    let again = OrganizeService::new(Arc::clone(&lab.db))
        .undo(&batch)
        .unwrap();
    assert_eq!(again.restored, 0);
}

#[test]
fn a_file_changed_after_the_move_is_reported_and_nothing_is_touched() {
    let lab = Lab::new();
    let heat = lab.movie(&lab.movies, "Heat (1995)");
    let applied = lab
        .cross()
        .apply(&[heat], lab.archive.id, &mut no_progress())
        .unwrap();
    let batch = applied.batch_id.unwrap();
    // Someone edits a file in the new place.
    let edited = Path::new(&lab.archive.path).join("Heat (1995)/poster.jpg");
    fs::write(&edited, "a different poster, longer than before").unwrap();

    let outcome = OrganizeService::new(Arc::clone(&lab.db))
        .undo(&batch)
        .unwrap();

    assert_eq!(outcome.restored, 0);
    assert_eq!(outcome.not_restorable.len(), 1, "{outcome:?}");
    assert!(outcome.not_restorable[0].reason.contains("changed"));
    assert!(edited.is_file(), "the new place is left as it is");
    assert!(!Path::new(&lab.movies.path).join("Heat (1995)").exists());
    assert_eq!(lab.item(heat).core.data_source, lab.archive.path);
}

#[test]
fn a_full_destination_stops_the_move_and_removes_nothing() {
    let lab = Lab::new();
    let heat = lab.big_movie(&lab.movies, "Heat (1995)");
    let ronin = lab.movie(&lab.movies, "Ronin (1998)");
    let old_folder = Path::new(&lab.movies.path).join("Heat (1995)");
    let before = tree(&old_folder);
    // Room for the 3 MB video and a little more.
    let disk = FaultFs::new(Faults {
        disk_full_after: Some(3 * 1024 * 1024 + 10),
        ..Faults::default()
    });

    let applied = lab
        .cross_with(&disk)
        .apply(&[heat, ronin], lab.archive.id, &mut no_progress())
        .unwrap();

    assert_eq!(applied.moved(), 0);
    let heat_outcome = applied.items.iter().find(|i| i.id == heat).unwrap();
    assert_eq!(
        heat_outcome.result,
        MoveResult::Stopped("Destination is full".to_owned())
    );
    assert_eq!(tree(&old_folder), before, "the originals are untouched");
    assert_eq!(lab.item(heat).core.data_source, lab.movies.path);
    // The journal keeps the reason and what is left to do.
    let rows = lab.journal(&applied.batch_id.unwrap());
    let stopped: Vec<_> = rows
        .iter()
        .filter(|r| r.state == "failed" && r.error.starts_with("stopped: "))
        .collect();
    assert_eq!(stopped.len(), 1, "{rows:?}");
    assert_eq!(stopped[0].error, "stopped: Destination is full");
    assert!(rows.iter().any(|r| r.state == "done" && r.op == "copy"));
    assert!(rows.iter().any(|r| r.state == "planned"));
}

#[test]
fn access_denied_on_the_destination_stops_with_a_plain_reason() {
    let lab = Lab::new();
    let heat = lab.movie(&lab.movies, "Heat (1995)");
    let disk = FaultFs::new(Faults {
        deny_write: Some("poster.jpg".to_owned()),
        ..Faults::default()
    });
    let applied = lab
        .cross_with(&disk)
        .apply(&[heat], lab.archive.id, &mut no_progress())
        .unwrap();
    assert_eq!(
        applied.items[0].result,
        MoveResult::Stopped("CineVault is not allowed to write poster.jpg".to_owned())
    );
    assert!(
        Path::new(&lab.movies.path)
            .join("Heat (1995)/poster.jpg")
            .is_file()
    );
}

#[test]
fn an_original_in_use_stays_in_the_old_folder_and_is_told() {
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

    // The title is whole in the new place; the one file that is open elsewhere stays behind.
    // That is not a finished move: it is reported stopped, with the reason,
    // and Activity offers Resume and Roll back at once.
    assert_eq!(applied.moved(), 0, "{applied:?}");
    let MoveResult::Stopped(why) = &applied.items[0].result else {
        panic!("{applied:?}");
    };
    assert!(
        why.contains("Heat (1995).mkv") && why.contains("in use"),
        "{why}"
    );
    let found = crate::library_ops::RecoveryService::new(Arc::clone(&lab.db))
        .find()
        .unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        Path::new(&lab.movies.path)
            .join("Heat (1995)/Heat (1995).mkv")
            .is_file()
    );
    assert_eq!(lab.item(heat).core.data_source, lab.archive.path);
    assert!(
        Path::new(&lab.archive.path)
            .join("Heat (1995)/Heat (1995).mkv")
            .is_file()
    );
}

#[test]
fn a_same_volume_move_is_one_journaled_folder_rename() {
    let lab = Lab::new();
    let heat = lab.big_movie(&lab.movies, "Heat (1995)");
    let applied = lab
        .change()
        .apply(&[heat], lab.archive.id, &mut no_progress())
        .unwrap();
    let batch = applied.batch_id.unwrap();
    let rows = lab.journal(&batch);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(
        (rows[0].op.as_str(), rows[0].state.as_str()),
        ("move", "done")
    );
    assert!(
        Path::new(&lab.archive.path)
            .join("Heat (1995)/extras/deleted/scene.mkv")
            .is_file()
    );
    // No trash folder was needed.
    assert!(
        !Path::new(&lab.movies.path)
            .join(".deletedByCineVault")
            .exists()
    );

    let outcome = OrganizeService::new(Arc::clone(&lab.db))
        .undo(&batch)
        .unwrap();
    assert_eq!(outcome.restored, 1);
    assert!(
        Path::new(&lab.movies.path)
            .join("Heat (1995)/extras/deleted/scene.mkv")
            .is_file()
    );
    assert_eq!(lab.item(heat).core.data_source, lab.movies.path);
}

#[test]
fn the_journal_holds_every_file_before_the_first_one_is_copied() {
    let lab = Lab::new();
    let heat = lab.big_movie(&lab.movies, "Heat (1995)");
    let rows_when_copying = Arc::new(Mutex::new(Vec::new()));
    let db = Arc::clone(&lab.db);
    let disk = FaultFs::new(Faults {
        on_create: Some(Box::new({
            let rows = Arc::clone(&rows_when_copying);
            move |_| {
                let mut all = db.read(table::pending_rows).unwrap();
                all.retain(|r| r.batch_id.starts_with("move-"));
                rows.lock().unwrap().push(all.len());
            }
        })),
        ..Faults::default()
    });
    let applied = lab
        .cross_with(&disk)
        .apply(&[heat], lab.archive.id, &mut no_progress())
        .unwrap();
    assert_eq!(applied.moved(), 1);
    let counts = rows_when_copying.lock().unwrap();
    // At the first file all 17 rows are on record as planned; they settle one by one.
    assert_eq!(counts.first(), Some(&17), "{counts:?}");
    assert!(counts.windows(2).all(|w| w[1] <= w[0]));
}

#[test]
fn a_killed_process_leaves_the_originals_and_a_journal_that_says_what_is_left() {
    let lab = Lab::new();
    let heat = lab.big_movie(&lab.movies, "Heat (1995)");
    let old_folder = Path::new(&lab.movies.path).join("Heat (1995)");
    let before = tree(&old_folder);
    let disk = FaultFs::new(Faults {
        kill_at_part_rename: Some(4),
        ..Faults::default()
    });
    let service = lab.cross_with(&disk);
    let killed = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let _ = service.apply(&[heat], lab.archive.id, &mut no_progress());
    }));
    assert!(killed.is_err(), "the run was killed");

    assert_eq!(tree(&old_folder), before, "every original is intact");
    assert_eq!(lab.item(heat).core.data_source, lab.movies.path);
    let batch = lab
        .db
        .read(table::batch_summaries)
        .unwrap()
        .into_iter()
        .find(|b| b.batch_id.starts_with("move-"))
        .unwrap();
    assert_eq!(batch.done, 3, "three files were copied and verified");
    assert!(batch.planned > 0);
}

#[test]
fn the_staging_folder_is_a_name_scans_skip() {
    use cv_scan::skip::{LibraryKind, SkipRules};
    let stage = crate::library_ops::move_engine::stage_dir(
        Path::new("Z:/NAS 4K/Action/Heat (1995)"),
        "move-1700000000000-3",
        Some("42"),
    );
    let name = stage.file_name().unwrap().to_string_lossy().into_owned();
    assert!(name.starts_with(".cvmove-"), "{name}");
    assert_eq!(stage.parent(), Some(Path::new("Z:/NAS 4K/Action")));
    for kind in [LibraryKind::Movies, LibraryKind::TvShows] {
        assert!(
            SkipRules::with_defaults(kind).is_builtin_skipped(&name),
            "a scan must never enter a half-copied title ({kind:?})"
        );
    }
}

// ---- the system bin ----------------------------------------------

impl Lab {
    pub(super) fn bin(&self) -> Arc<FakeTrash> {
        FakeTrash::new(self.dir.path())
    }

    pub(super) fn cross_bin(&self, bin: &Arc<FakeTrash>) -> ChangeSourceService {
        self.cross().with_bin(Arc::clone(bin) as Arc<dyn SystemBin>)
    }

    pub(super) fn organize_bin(&self, bin: &Arc<FakeTrash>) -> OrganizeService {
        OrganizeService::new(Arc::clone(&self.db)).with_bin(Arc::clone(bin) as Arc<dyn SystemBin>)
    }
}

#[test]
fn a_cross_drive_move_sends_the_verified_originals_to_the_bin_and_undo_restores_them() {
    let lab = Lab::new();
    let bin = lab.bin();
    let heat = lab.big_movie(&lab.movies, "Heat (1995)");
    let old_folder = Path::new(&lab.movies.path).join("Heat (1995)");
    let before = tree(&old_folder);
    let applied = lab
        .cross_bin(&bin)
        .apply(&[heat], lab.archive.id, &mut no_progress())
        .unwrap();
    assert_eq!(applied.moved(), 1, "{applied:?}");
    let batch = applied.batch_id.unwrap();
    assert!(!old_folder.exists());
    assert_eq!(
        bin.names().len(),
        8,
        "every original is in the bin: {:?}",
        bin.names()
    );
    assert!(
        !Path::new(&lab.movies.path)
            .join(".deletedByCineVault")
            .exists(),
        "nothing went to the trash folder"
    );
    let key = batch.clone();
    let binned = lab.db.read(move |c| table::binned_rows(c, &key)).unwrap();
    assert_eq!(binned.len(), 8, "each trash row keeps its bin id");
    // History counts what is still in the bin.
    let history = crate::history::HistoryService::new(Arc::clone(&lab.db))
        .with_bin(Arc::clone(&bin) as Arc<dyn SystemBin>);
    assert!(history.bin_total().unwrap() >= 3 * 1024 * 1024);

    let outcome = lab.organize_bin(&bin).undo(&batch).unwrap();
    assert_eq!(outcome.restored, 1, "{outcome:?}");
    assert!(outcome.is_complete(), "{outcome:?}");
    assert_eq!(
        tree(&old_folder),
        before,
        "every original is back, byte for byte"
    );
    assert_eq!(lab.item(heat).core.data_source, lab.movies.path);
    assert!(!Path::new(&lab.archive.path).join("Heat (1995)").exists());
    // The copies went to the bin in their turn.
    assert_eq!(bin.names().len(), 8, "{:?}", bin.names());
    assert!(
        !Path::new(&lab.archive.path)
            .join(".deletedByCineVault")
            .exists()
    );
    assert!(lab.journal(&batch).iter().all(|r| r.state == "undone"));
}

#[test]
fn a_cancel_during_the_copy_sends_nothing_to_the_bin() {
    let lab = Lab::new();
    let bin = lab.bin();
    let heat = lab.big_movie(&lab.movies, "Heat (1995)");
    let old_folder = Path::new(&lab.movies.path).join("Heat (1995)");
    let before = tree(&old_folder);
    let control = MoveControl::new();
    let disk = FaultFs::new(Faults {
        cancel_on_create: Some(("making-of.mkv".to_owned(), Arc::clone(&control))),
        ..Faults::default()
    });
    let applied = lab
        .cross_with(&disk)
        .with_bin(Arc::clone(&bin) as Arc<dyn SystemBin>)
        .apply_with(&[heat], lab.archive.id, Some(control), &mut no_progress())
        .unwrap();
    assert_eq!(applied.items[0].result, MoveResult::Canceled);
    assert_eq!(tree(&old_folder), before);
    assert!(bin.names().is_empty());
    assert_eq!(bin.calls.load(Ordering::SeqCst), 0);
    assert!(names_below(Path::new(&lab.archive.path)).is_empty());
}

// ---- the copy pipeline ------------------------------------------------------------------------------

/// A video whose bytes all differ, so a chunk out of order or lost shows.
fn patterned(lab: &Lab, len: usize) -> (ItemId, Vec<u8>) {
    let heat = lab.movie(&lab.movies, "Heat (1995)");
    let data: Vec<u8> = (0..len)
        .map(|i| u8::try_from((i * 7 + i / 251) % 256).unwrap())
        .collect();
    fs::write(
        Path::new(&lab.movies.path)
            .join("Heat (1995)")
            .join("Heat (1995).mkv"),
        &data,
    )
    .unwrap();
    (heat, data)
}

#[test]
fn the_pipeline_writes_every_chunk_in_order_and_reports_speed() {
    let lab = Lab::new();
    // 37 chunks of 4 KiB and a short last one.
    let (heat, data) = patterned(&lab, 37 * 4096 + 1000);
    let log = Arc::new(Mutex::new(Vec::new()));
    let disk = FaultFs::new(Faults {
        chunk: Some(4096),
        record_writes: Some(("Heat (1995).mkv".to_owned(), Arc::clone(&log))),
        ..Faults::default()
    });
    let mut events = Vec::new();
    let applied = lab
        .cross_with(&disk)
        .apply(&[heat], lab.archive.id, &mut |p: &MoveProgress| {
            events.push(p.clone());
        })
        .unwrap();
    assert_eq!(applied.moved(), 1, "{applied:?}");

    let writes = log.lock().unwrap().clone();
    assert_eq!(writes.len(), 38);
    assert!(writes[..37].iter().all(|w| w.len() == 4096));
    assert_eq!(writes[37].len(), 1000);
    assert!(
        writes.concat() == data,
        "the chunks arrive in the order they were read"
    );
    let copy = fs::read(
        Path::new(&lab.archive.path)
            .join("Heat (1995)")
            .join("Heat (1995).mkv"),
    )
    .unwrap();
    assert!(copy == data);
    // Every event carries the speed fields; a time left is never more than nothing left.
    let last = events.last().unwrap();
    assert_eq!(last.bytes_done, last.bytes_total);
    assert!(
        events
            .iter()
            .all(|e| e.seconds_left.is_none() || e.bytes_per_second > 0)
    );
}

#[test]
fn a_copy_that_reads_back_different_from_the_disk_is_rolled_back() {
    let lab = Lab::new();
    let (heat, data) = patterned(&lab, 3 * 4096 + 17);
    let old_folder = Path::new(&lab.movies.path).join("Heat (1995)");
    let before = tree(&old_folder);
    let disk = FaultFs::new(Faults {
        chunk: Some(4096),
        corrupt_read_back: Some("Heat (1995).mkv".to_owned()),
        ..Faults::default()
    });
    let applied = lab
        .cross_with(&disk)
        .apply(&[heat], lab.archive.id, &mut no_progress())
        .unwrap();
    let MoveResult::Failed(why) = &applied.items[0].result else {
        panic!("{:?}", applied.items[0].result);
    };
    assert!(why.contains("does not match"), "{why}");
    assert_eq!(tree(&old_folder), before);
    assert!(fs::read(old_folder.join("Heat (1995).mkv")).unwrap() == data);
    assert!(names_below(Path::new(&lab.archive.path)).is_empty());
    assert_eq!(lab.item(heat).core.data_source, lab.movies.path);
}

#[test]
fn cancel_between_two_chunks_of_one_file_removes_the_part_and_keeps_the_original() {
    let lab = Lab::new();
    let (heat, data) = patterned(&lab, 20 * 4096);
    let old_folder = Path::new(&lab.movies.path).join("Heat (1995)");
    let control = MoveControl::new();
    let log = Arc::new(Mutex::new(Vec::new()));
    let disk = FaultFs::new(Faults {
        chunk: Some(4096),
        cancel_after_writes: Some(("Heat (1995).mkv".to_owned(), 3, Arc::clone(&control))),
        record_writes: Some(("Heat (1995).mkv".to_owned(), Arc::clone(&log))),
        ..Faults::default()
    });
    let applied = lab
        .cross_with(&disk)
        .apply_with(&[heat], lab.archive.id, Some(control), &mut no_progress())
        .unwrap();
    assert_eq!(applied.items[0].result, MoveResult::Canceled);
    // It stopped in the middle of the file, not after it.
    let written = log.lock().unwrap().len();
    assert!((3..20).contains(&written), "{written} writes");
    assert!(fs::read(old_folder.join("Heat (1995).mkv")).unwrap() == data);
    let leftovers = names_below(Path::new(&lab.archive.path));
    assert!(leftovers.is_empty(), "{leftovers:?}");
    let rows = lab.journal(&applied.batch_id.unwrap());
    assert!(
        rows.iter()
            .all(|r| r.state != "planned" && r.state != "done"),
        "{rows:?}"
    );
}
