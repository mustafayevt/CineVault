//! Organize under the shared fault-injection file system: a stop between the journal and the executor at every step of a batch, an error at every
//! step, permission denied on one title's folder, a file in use, and a full disk while leftovers
//! go to the trash folder.
//!
//! After the restart (start-up recovery, then Resume or Roll back from Activity when a title was
//! left unfinished) every title must be at its old or its new name with all its companions
//! together, Undo must restore the batch exactly, and no file may be lost.

use std::collections::BTreeSet;
use std::io;
use std::panic::AssertUnwindSafe;

use cv_rename::exec::TRASH_DIR;
use cv_rename::exec::fault::{Fault, FaultFs, Op, Rule, When, is_stop};

use super::*;
use crate::library_ops::RecoveryService;

/// Every file below `dir` with its bytes, relative to `dir`, `/` separated.
fn snapshot(dir: &Path, with_trash: bool) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, dir: &Path, with_trash: bool, out: &mut BTreeMap<String, Vec<u8>>) {
        let Ok(read) = fs::read_dir(dir) else { return };
        for entry in read.flatten() {
            let path = entry.path();
            if !with_trash && entry.file_name() == TRASH_DIR {
                continue;
            }
            if path.is_dir() {
                walk(base, &path, with_trash, out);
            } else {
                let rel = path
                    .strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                out.insert(rel, fs::read(&path).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, with_trash, &mut out);
    out
}

/// The files of one folder below the root, by their names in it.
fn folder(lib: &Lib, rel: &str) -> BTreeMap<String, Vec<u8>> {
    snapshot(&lib.root.join(rel), false)
}

/// The titles of a batch: (old folder, base name, title, year).
const TITLES: [(&str, &str, &str, u16); 3] = [
    ("alpha.1995", "alpha", "Alpha", 1995),
    ("beta.1996", "beta", "Beta", 1996),
    ("gamma.1997", "gamma", "Gamma", 1997),
];

/// What a title's folder holds before the organize, besides its video, NFO and poster.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Leftovers {
    /// A subtitle that goes along, a second poster the profile drops (trashed with the item) and
    /// an unwanted `.txt` (trashed after it).
    All,
    /// Only the subtitle and the unwanted `.txt`.
    UnwantedOnly,
}

/// A library of three movies, as `Lib::movie` makes them plus companions and leftovers.
fn library(leftovers: Leftovers) -> (Lib, Vec<ItemId>) {
    let lib = Lib::new();
    let mut ids = Vec::new();
    for (folder, base, title, year) in TITLES {
        let mut item = Item::movie(title);
        item.core.year = year;
        item.core.data_source = lib.root_text();
        item.core.path = lib.root.join(folder).to_string_lossy().into_owned();
        super::mark_scraped(&mut item);
        let mut files = vec![
            (format!("{base}.mkv"), MediaFileType::Video),
            (format!("{base}.nfo"), MediaFileType::Nfo),
            (format!("{base}-poster.jpg"), MediaFileType::Poster),
            (format!("{base}.en.srt"), MediaFileType::Subtitle),
        ];
        if leftovers == Leftovers::All {
            files.push(("folder.jpg".to_owned(), MediaFileType::Poster));
        }
        for (name, kind) in files {
            lib.put(&format!("{folder}/{name}"));
            item.core
                .media_files
                .push(MediaFile::new(kind, item.core.path.clone(), name));
        }
        lib.put(&format!("{folder}/notes.txt"));
        ids.push(lib.db.write(move |tx| insert_item(tx, &item)).unwrap());
    }
    (lib, ids)
}

fn request(lib: &Lib, ids: &[ItemId]) -> ApplyRequest {
    let mut preview = lib.request(Scope::Everything);
    preview.profile.movie.cleanup_unwanted = true;
    ApplyRequest {
        preview,
        include: super::ids(ids),
    }
}

/// A title's new folder and what is in it.
type NewFolder = (String, BTreeMap<String, Vec<u8>>);
type News = Vec<NewFolder>;

/// What an organize over a healthy disk leaves: each title's new folder and what is in it.
fn clean_result(leftovers: Leftovers) -> (News, usize) {
    let (lib, ids) = library(leftovers);
    let disk = FaultFs::new();
    let service = lib.service.clone().with_disk(Arc::new(disk.clone()));
    let applied = service.apply(&request(&lib, &ids)).unwrap();
    assert_eq!(applied.done(), 3, "{applied:?}");
    let news = ids
        .iter()
        .map(|id| {
            let path = PathBuf::from(lib.item(*id).core.path);
            let rel = path.file_name().unwrap().to_string_lossy().into_owned();
            let files = folder(&lib, &rel);
            (rel, files)
        })
        .collect();
    (news, disk.ops())
}

/// Where a title is, whole.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Whole {
    Old,
    New,
}

struct Case {
    lib: Lib,
    ids: Vec<ItemId>,
    before: BTreeMap<String, Vec<u8>>,
    news: News,
}

impl Case {
    fn new(leftovers: Leftovers, news: &[NewFolder]) -> Self {
        let (lib, ids) = library(leftovers);
        let before = snapshot(&lib.root, true);
        Case {
            lib,
            ids,
            before,
            news: news.to_vec(),
        }
    }

    fn service(&self, disk: &FaultFs) -> OrganizeService {
        OrganizeService::new(Arc::clone(&self.lib.db)).with_disk(Arc::new(disk.clone()))
    }

    /// Runs the organize; a stop of the shim is caught and is `None`.
    fn apply(&self, disk: &FaultFs) -> Option<Applied> {
        let service = self.service(disk);
        let request = request(&self.lib, &self.ids);
        match std::panic::catch_unwind(AssertUnwindSafe(|| service.apply(&request))) {
            Ok(applied) => Some(applied.unwrap()),
            Err(payload) => {
                assert!(is_stop(payload.as_ref()), "a real panic, not a stop");
                None
            }
        }
    }

    /// The bytes the title had in its old folder, by name.
    fn old_files(&self, index: usize) -> BTreeMap<String, Vec<u8>> {
        let prefix = format!("{}/", TITLES[index].0);
        self.before
            .iter()
            .filter_map(|(k, v)| k.strip_prefix(&prefix).map(|n| (n.to_owned(), v.clone())))
            .collect()
    }

    /// Where title `index` is whole, or `None` when it is split.
    fn whole(&self, index: usize) -> Option<Whole> {
        let old = TITLES[index].0;
        let (new, new_files) = &self.news[index];
        let path = PathBuf::from(self.lib.item(self.ids[index]).core.path);
        let at_old = folder(&self.lib, old) == self.old_files(index)
            && !self.lib.root.join(new).exists()
            && path.ends_with(old);
        // In its new place it has every file a clean run gives it; a leftover that could not go
        // to the trash folder may stay beside them, never anything else.
        let here = folder(&self.lib, new);
        let originals: Vec<Vec<u8>> = self.old_files(index).into_values().collect();
        let at_new = new_files.iter().all(|(k, v)| here.get(k) == Some(v))
            && here
                .iter()
                .filter(|(k, _)| !new_files.contains_key(*k))
                .all(|(_, v)| originals.contains(v))
            && !self.lib.root.join(old).exists()
            && path.ends_with(new);
        match (at_old, at_new) {
            (true, false) => Some(Whole::Old),
            (false, true) => Some(Whole::New),
            _ => None,
        }
    }

    /// Every title whole, and every byte of every original still on the disk.
    fn check(&self, label: &str) -> Vec<Whole> {
        let now = snapshot(&self.lib.root, true);
        let contents: BTreeSet<&Vec<u8>> = now.values().collect();
        for (path, bytes) in &self.before {
            assert!(contents.contains(bytes), "{label}: {path} was lost");
        }
        (0..TITLES.len())
            .map(|i| {
                self.whole(i).unwrap_or_else(|| {
                    panic!(
                        "{label}: {} is split\n{:#?}",
                        TITLES[i].2,
                        now.keys().collect::<Vec<_>>()
                    )
                })
            })
            .collect()
    }

    /// The restart: start-up recovery, then Resume or Roll back of what it left unfinished.
    fn restart(&self, finish: Finish, label: &str) {
        crate::organize::recover_interrupted_batches(&self.lib.db).unwrap();
        let recovery = RecoveryService::new(Arc::clone(&self.lib.db));
        for batch in recovery.find().unwrap() {
            match finish {
                Finish::Resume => {
                    let done = recovery.resume(&batch.batch_id, None, &mut |_| {}).unwrap();
                    assert_eq!(done.stopped, None, "{label}: {done:?}");
                }
                Finish::RollBack => {
                    let back = recovery.roll_back(&batch.batch_id).unwrap();
                    assert!(back.problems.is_empty(), "{label}: {back:?}");
                }
            }
        }
        assert!(
            recovery.find().unwrap().is_empty(),
            "{label}: still offered"
        );
    }

    /// Undo of every batch of the journal puts the library back exactly as it was.
    fn undo_restores_everything(&self, label: &str) {
        let service = OrganizeService::new(Arc::clone(&self.lib.db));
        for batch in service.journal().unwrap() {
            let undone = service.undo(&batch.batch_id).unwrap();
            assert!(undone.is_complete(), "{label}: {undone:?}");
        }
        assert_eq!(
            snapshot(&self.lib.root, false),
            snapshot_live(&self.before),
            "{label}: undo restores the batch"
        );
        for (i, id) in self.ids.iter().enumerate() {
            assert!(
                PathBuf::from(self.lib.item(*id).core.path).ends_with(TITLES[i].0),
                "{label}"
            );
        }
    }
}

fn snapshot_live(all: &BTreeMap<String, Vec<u8>>) -> BTreeMap<String, Vec<u8>> {
    all.iter()
        .filter(|(k, _)| !k.starts_with(TRASH_DIR))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Finish {
    Resume,
    RollBack,
}

#[test]
fn a_stop_between_the_journal_and_the_disk_at_any_step_ends_with_every_title_whole() {
    let (news, total) = clean_result(Leftovers::All);
    assert!(total > 12, "the batch should do some work: {total}");
    for at in 0..total {
        // `Before`: the step's journal row is written and the disk is not changed yet.
        for when in [When::Before, When::After] {
            for finish in [Finish::Resume, Finish::RollBack] {
                let label = format!("stop {when:?} step {at}, then {finish:?}");
                let case = Case::new(Leftovers::All, &news);
                let disk = FaultFs::new();
                disk.stop_at(at, when);
                assert!(case.apply(&disk).is_none(), "{label}: the run was stopped");
                case.restart(finish, &label);
                let wholes = case.check(&label);
                // Resume finishes the whole batch, the titles that never started included
                //; Roll back takes back the title it stopped in and leaves
                // the ones before it renamed and the ones after it as they were.
                let first_old = wholes.iter().position(|w| *w == Whole::Old);
                match finish {
                    Finish::Resume => {
                        assert_eq!(wholes, [Whole::New; 3], "{label}");
                    }
                    Finish::RollBack => assert!(
                        first_old.is_none_or(|at| wholes[at..].iter().all(|w| *w == Whole::Old)),
                        "{label}: {wholes:?}"
                    ),
                }
                // The next organize renames what is left.
                let rest = case.apply(&FaultFs::new()).unwrap();
                assert_eq!(rest.failed(), 0, "{label}: {rest:?}");
                assert_eq!(case.check(&label), [Whole::New; 3], "{label}");
                case.undo_restores_everything(&label);
            }
        }
    }
}

#[test]
fn a_stop_before_the_first_change_leaves_every_name_and_resume_renames_them_all() {
    let (news, _) = clean_result(Leftovers::All);
    let case = Case::new(Leftovers::All, &news);
    let disk = FaultFs::new();
    disk.stop_at(0, When::Before);
    assert!(case.apply(&disk).is_none());
    // The journal has the step that was about to run; the disk does not.
    assert_eq!(snapshot(&case.lib.root, true), case.before);
    crate::organize::recover_interrupted_batches(&case.lib.db).unwrap();
    assert_eq!(case.check("stopped"), [Whole::Old, Whole::Old, Whole::Old]);
    // Activity counts the whole batch: three titles left, none renamed.
    let found = RecoveryService::new(Arc::clone(&case.lib.db))
        .find()
        .unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].items, 3, "{found:?}");
    assert_eq!((found[0].files_done, found[0].files_total), (0, 3));
    case.restart(Finish::Resume, "resume");
    assert_eq!(case.check("resumed"), [Whole::New; 3]);
    case.undo_restores_everything("undo");
}

#[test]
fn a_stop_inside_the_second_title_is_counted_by_title_and_each_way_out_covers_the_batch() {
    let (news, total) = clean_result(Leftovers::All);
    // A stop in the middle of the batch: the first title is renamed, the second half way.
    let at = total / 2;
    for finish in [Finish::Resume, Finish::RollBack] {
        let label = format!("{finish:?}");
        let case = Case::new(Leftovers::All, &news);
        let disk = FaultFs::new();
        disk.stop_at(at, When::After);
        assert!(case.apply(&disk).is_none(), "{label}");
        crate::organize::recover_interrupted_batches(&case.lib.db).unwrap();
        let found = RecoveryService::new(Arc::clone(&case.lib.db))
            .find()
            .unwrap();
        assert_eq!(found.len(), 1, "{label}: {found:?}");
        let done = found[0].files_done;
        assert_eq!(
            found[0].files_total, 3,
            "{label}: counted by title: {found:?}"
        );
        assert_eq!(found[0].items, 3 - done, "{label}: {found:?}");
        assert!(
            found[0].items >= 2,
            "{label}: the stopped title and the next: {found:?}"
        );
        case.restart(finish, &label);
        let wholes = case.check(&label);
        match finish {
            Finish::Resume => assert_eq!(wholes, [Whole::New; 3], "{label}"),
            Finish::RollBack => {
                // What finished stays renamed, the rest is as it was.
                assert_eq!(wholes.iter().filter(|w| **w == Whole::New).count(), done);
            }
        }
        case.undo_restores_everything(&label);
    }
}

#[test]
fn an_error_at_any_step_fails_only_its_title_and_leaves_every_title_whole() {
    let (news, total) = clean_result(Leftovers::All);
    for at in 0..total {
        let label = format!("error at step {at}");
        let case = Case::new(Leftovers::All, &news);
        let disk = FaultFs::new();
        disk.fail_at(at, Fault::Kind(io::ErrorKind::PermissionDenied));
        let applied = case.apply(&disk).expect("an error is not a stop");
        assert!(applied.failed() <= 1, "{label}: {applied:?}");
        case.restart(Finish::Resume, &label);
        let wholes = case.check(&label);
        // The title that failed is whole under its old names, the others are renamed.
        let failed: Vec<usize> = applied
            .items
            .iter()
            .filter(|i| matches!(i.status, AppliedStatus::Failed(_)))
            .map(|i| {
                case.ids
                    .iter()
                    .position(|id| id.0.to_string() == i.id)
                    .unwrap()
            })
            .collect();
        for (index, whole) in wholes.iter().enumerate() {
            let want = if failed.contains(&index) {
                Whole::Old
            } else {
                Whole::New
            };
            assert_eq!(*whole, want, "{label}: {} {applied:?}", TITLES[index].2);
        }
        case.undo_restores_everything(&label);
    }
}

#[test]
fn permission_denied_on_one_folder_leaves_that_title_whole_and_renames_the_others() {
    let (news, _) = clean_result(Leftovers::All);
    let case = Case::new(Leftovers::All, &news);
    let disk = FaultFs::new();
    disk.rule(Rule::on(&Op::MUTATIONS, Fault::PermissionDenied).within("/beta.1996"));
    let applied = case.apply(&disk).unwrap();
    assert_eq!((applied.done(), applied.failed()), (2, 1), "{applied:?}");
    let beta = applied
        .items
        .iter()
        .find(|i| i.id == case.ids[1].0.to_string())
        .unwrap();
    let AppliedStatus::Failed(why) = &beta.status else {
        panic!("{beta:?}");
    };
    // "Access denied" on a folder rename is what an open file below it gives on Windows, so the
    // reason says the folder is in use and what to do.
    assert!(
        why.contains("denied") || why.contains("allowed") || why.contains("in use"),
        "{why}"
    );
    assert_eq!(case.check("denied"), [Whole::New, Whole::Old, Whole::New]);
    // Nothing is left for Activity: the title that failed was taken back whole.
    assert!(
        RecoveryService::new(Arc::clone(&case.lib.db))
            .find()
            .unwrap()
            .is_empty()
    );
    case.undo_restores_everything("undo");
}

#[test]
fn a_file_in_use_rolls_its_title_back_whole_and_one_let_go_in_time_is_renamed() {
    let (news, _) = clean_result(Leftovers::All);
    // Held for good: Beta's NFO cannot be renamed after its folder and video were.
    let case = Case::new(Leftovers::All, &news);
    let disk = FaultFs::new();
    disk.rule(Rule::on(&[Op::Rename], Fault::InUse).named("beta.nfo"));
    let applied = case.apply(&disk).unwrap();
    assert_eq!((applied.done(), applied.failed()), (2, 1), "{applied:?}");
    assert_eq!(case.check("held"), [Whole::New, Whole::Old, Whole::New]);
    // Let go: the next organize renames it.
    disk.heal();
    let again = case.apply(&disk).unwrap();
    assert_eq!(again.failed(), 0, "{again:?}");
    assert_eq!(case.check("let go"), [Whole::New, Whole::New, Whole::New]);
    case.undo_restores_everything("undo");

    // Held for a moment: the rename is tried again and goes through.
    let case = Case::new(Leftovers::All, &news);
    let disk = FaultFs::new();
    disk.rule(
        Rule::on(&[Op::Rename], Fault::InUse)
            .named("beta.nfo")
            .times(2),
    );
    let applied = case.apply(&disk).unwrap();
    assert_eq!(applied.done(), 3, "{applied:?}");
    assert_eq!(case.check("a moment"), [Whole::New, Whole::New, Whole::New]);
    case.undo_restores_everything("undo");
}

fn full_trash(disk: &FaultFs) {
    disk.rule(Rule::on(&[Op::CreateDir], Fault::DiskFull).within(TRASH_DIR));
    disk.rule(Rule::on(&[Op::Rename, Op::Copy], Fault::DiskFull).into(TRASH_DIR));
}

#[test]
fn a_full_disk_while_leftovers_go_to_the_trash_folder_deletes_nothing() {
    // A leftover the plan trashes with the title: the title goes back whole.
    let (news, _) = clean_result(Leftovers::All);
    let case = Case::new(Leftovers::All, &news);
    let disk = FaultFs::new();
    full_trash(&disk);
    let applied = case.apply(&disk).unwrap();
    assert_eq!(applied.failed(), 3, "{applied:?}");
    assert_eq!(case.check("full"), [Whole::Old, Whole::Old, Whole::Old]);
    assert_eq!(snapshot(&case.lib.root, true), case.before, "nothing moved");
    disk.heal();
    let again = case.apply(&disk).unwrap();
    assert_eq!(again.done(), 3, "{again:?}");
    case.undo_restores_everything("undo");

    // Only unwanted files, cleared after the title: the title is renamed, the file stays
    // beside it and the note says so.
    let (news, _) = clean_result(Leftovers::UnwantedOnly);
    let case = Case::new(Leftovers::UnwantedOnly, &news);
    let disk = FaultFs::new();
    full_trash(&disk);
    let applied = case.apply(&disk).unwrap();
    assert_eq!(applied.done(), 3, "{applied:?}");
    assert!(
        applied.items.iter().all(|i| !i.notes.is_empty()),
        "{applied:?}"
    );
    assert_eq!(case.check("full"), [Whole::New, Whole::New, Whole::New]);
    for (new, _) in &case.news {
        assert!(
            case.lib.root.join(new).join("notes.txt").is_file(),
            "the unwanted file stays: {new}"
        );
    }
    case.undo_restores_everything("undo");
}
