//! Drives over a real library and a scripted set of volumes.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cv_db::Database;
use cv_db::repo::data_sources::insert_source;
use cv_db::repo::drive_facts::mark_sources_seen;
use cv_db::repo::insert_item;
use cv_domain::{Item, MediaFile, MediaFileType, SourceKind};

use super::*;

const GB: u64 = 1_000_000_000;
const TB: u64 = 1_000 * GB;

// ---- the low rule -------------------------------------------------------------------------------

#[test]
fn the_low_space_rule_follows_the_spec_table() {
    // 400 GB of 10.9 TB is under 5 percent.
    assert!(is_low(10_900 * GB, 400 * GB));
    // 60 GB of 931 GB: over 5 percent (46.5 GB) and over 50 GB.
    assert!(!is_low(931 * GB, 60 * GB));
    // 41 GB of 931 GB: under both.
    assert!(is_low(931 * GB, 41 * GB));
    // Exactly 5 percent, and exactly 50 GB, are not low.
    assert!(!is_low(TB, 50 * GB));
    assert!(!is_low(100 * GB, 50 * GB));
    // Under 50 GB is low whatever the size of the drive.
    assert!(is_low(100 * GB, 49 * GB));
    // A drive with no known size is not judged.
    assert!(!is_low(0, 0));
}

#[test]
fn a_folder_is_grouped_by_the_volume_its_path_names() {
    assert_eq!(volume_key(r"d:\Movies"), "D:");
    assert_eq!(volume_key(r"D:/Archive/Movies"), "D:");
    assert_eq!(volume_key(r"\\nas\media\Shows"), r"\\nas\media");
    assert_eq!(volume_key("//nas/media/Shows"), r"\\nas\media");
    assert_eq!(volume_key("/Volumes/Media/Films"), "/Volumes/Media");
    assert_eq!(volume_key("/home/me/Movies"), "/");
    // A Linux mount point is its own drive; any other Linux path is on the root drive.
    assert_eq!(volume_key("/media/alex/Films/Movies"), "/media/alex/Films");
    assert_eq!(
        volume_key("/run/media/alex/Films/TV"),
        "/run/media/alex/Films"
    );
    assert_eq!(volume_key("/mnt/nas/x"), "/mnt/nas");
    assert_eq!(volume_key("/home/alex/x"), "/");
    assert_eq!(volume_key("/media/alex"), "/");
}

#[test]
fn a_linux_mount_is_named_by_its_device_or_else_by_its_last_part() {
    assert_eq!(linux_label("/dev/sdb1", "/media/alex/Films"), "Films");
    assert_eq!(linux_label("", "/mnt/nas"), "nas");
    assert_eq!(linux_label("/dev/nvme0n1p2", "/"), "");
    assert_eq!(linux_label("", "/"), "");
    assert_eq!(linux_label("server:/export", "/mnt/nas"), "server:/export");
}

// ---- grouping -----------------------------------------------------------------------------------

/// Volumes by drive letter, and nothing for the rest.
struct Fake {
    volumes: Mutex<HashMap<String, Volume>>,
    refreshed: AtomicUsize,
}

impl Fake {
    fn new(volumes: &[Volume]) -> Arc<Self> {
        Arc::new(Self {
            volumes: Mutex::new(volumes.iter().map(|v| (v.key.clone(), v.clone())).collect()),
            refreshed: AtomicUsize::new(0),
        })
    }

    fn remove(&self, key: &str) {
        self.volumes.lock().unwrap().remove(key);
    }
}

impl Volumes for Fake {
    fn refresh(&self) {
        self.refreshed.fetch_add(1, Ordering::Relaxed);
    }

    fn locate(&self, path: &str) -> Option<Volume> {
        self.volumes.lock().unwrap().get(&volume_key(path)).cloned()
    }
}

struct Answers(Mutex<HashMap<i64, bool>>);

impl SourceStates for Answers {
    fn states(&self) -> Result<HashMap<i64, bool>, DbError> {
        Ok(self.0.lock().unwrap().clone())
    }
}

fn volume(key: &str, label: &str, kind: DriveKind, total: u64, free: u64) -> Volume {
    Volume {
        key: key.to_owned(),
        label: label.to_owned(),
        kind,
        total_bytes: total,
        free_bytes: free,
    }
}

fn title(db: &Database, root: &str, name: &str, bytes: u64, height: u32, kind: &str) {
    let mut item = if kind == "show" {
        Item::show(name)
    } else {
        Item::movie(name)
    };
    item.core.data_source = root.to_owned();
    item.core.media_files = vec![MediaFile {
        kind: MediaFileType::Video,
        directory: format!("{root}/{name}"),
        filename: "video.mkv".to_owned(),
        size_bytes: bytes,
        height,
        ..MediaFile::default()
    }];
    db.write(move |tx| insert_item(tx, &item)).unwrap();
}

struct Rig {
    db: Arc<Database>,
    service: Arc<DrivesService>,
    volumes: Arc<Fake>,
    answers: Arc<Answers>,
    ids: Vec<i64>,
    _dir: tempfile::TempDir,
}

fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(Database::open(dir.path().join("library.db")).unwrap());
    let ids: Vec<i64> = [
        (SourceKind::Movie, "E:/Archive/Movies"),
        (SourceKind::Movie, "D:/Movies"),
        (SourceKind::Tv, "E:/TV"),
        (SourceKind::Movie, r"\\nas\media\Films"),
    ]
    .into_iter()
    .map(|(kind, path)| {
        db.write(move |tx| insert_source(tx, kind, path))
            .unwrap()
            .id
    })
    .collect();
    let volumes = Fake::new(&[
        volume("D:", "Media", DriveKind::Hdd, 7_280 * GB, 1_210 * GB),
        volume("E:", "Archive", DriveKind::Ssd, 10_900 * GB, 400 * GB),
        volume(r"\\nas\media", "", DriveKind::Network, 16 * TB, 9 * TB),
    ]);
    let answers = Arc::new(Answers(Mutex::new(
        ids.iter().map(|id| (*id, true)).collect(),
    )));
    let service = DrivesService::with_window(
        Arc::clone(&db),
        volumes.clone(),
        answers.clone(),
        Duration::from_millis(60),
    );
    Rig {
        db,
        service,
        volumes,
        answers,
        ids,
        _dir: dir,
    }
}

#[test]
fn two_folders_on_one_drive_are_one_card_with_the_numbers_of_each() {
    let rig = rig();
    title(&rig.db, "E:/Archive/Movies", "Alien", 4 * GB, 2160, "movie");
    title(
        &rig.db,
        "E:/Archive/Movies",
        "Aliens",
        2 * GB,
        1080,
        "movie",
    );
    title(&rig.db, "E:/TV", "Severance", 3 * GB, 1080, "show");
    title(&rig.db, "D:/Movies", "Heat", GB, 720, "movie");

    let overview = rig.service.overview().unwrap();
    let names: Vec<_> = overview.drives.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, ["E: Archive", "D: Media", r"\\nas\media"]);

    let archive = &overview.drives[0];
    assert_eq!(archive.id, "E:");
    assert_eq!(archive.kind, DriveKind::Ssd);
    assert_eq!(archive.folders.len(), 2);
    assert_eq!(
        (archive.folders[0].titles, archive.folders[0].bytes),
        (2, 6 * GB)
    );
    assert_eq!(archive.folders[0].kind, SourceKind::Movie);
    assert_eq!(
        (archive.folders[1].titles, archive.folders[1].bytes),
        (1, 3 * GB)
    );
    assert_eq!(archive.folders[1].kind, SourceKind::Tv);
    assert_eq!(
        archive.quality,
        QualityCounts {
            p2160: 1,
            p1080: 2,
            p720: 0,
            sd: 0
        }
    );
    // 10.5 TB used, 9 GB of it titles: the rest is not the library's.
    assert_eq!(archive.not_in_library_bytes, Some(10_500 * GB - 9 * GB));
    assert!(archive.low, "400 GB of 10.9 TB is under 5 percent");
    assert!(archive.online);
    assert_eq!(archive.last_seen_ms, None);

    let media = &overview.drives[1];
    assert!(!media.low);
    assert_eq!(media.folders.len(), 1);
    assert_eq!(media.quality.p720, 1);
    assert_eq!(rig.volumes.refreshed.load(Ordering::Relaxed), 1);
}

#[test]
fn a_drive_that_went_away_is_offline_with_its_folders_and_the_time_it_was_last_seen() {
    let rig = rig();
    title(&rig.db, r"\\nas\media\Films", "Heat", 5 * GB, 1080, "movie");
    let nas = rig.ids[3];
    let two_days_ago = 1_700_000_000_000_i64;
    rig.db
        .write(move |tx| mark_sources_seen(tx, &[nas], two_days_ago, 0))
        .unwrap();

    rig.volumes.remove(r"\\nas\media");
    rig.answers.0.lock().unwrap().insert(nas, false);
    let overview = rig.service.overview().unwrap();

    let drive = overview
        .drives
        .iter()
        .find(|d| d.id == r"\\nas\media")
        .unwrap();
    assert!(!drive.online);
    assert_eq!(drive.kind, DriveKind::Network);
    assert_eq!(drive.last_seen_ms, Some(two_days_ago));
    assert!(!drive.low, "an offline drive is not judged");
    assert_eq!(drive.not_in_library_bytes, None);
    // Its folder and titles stay.
    assert_eq!(drive.folders.len(), 1);
    assert_eq!(drive.folders[0].path, r"\\nas\media\Films");
    assert_eq!(drive.folders[0].titles, 1);
}

#[test]
fn a_drive_answers_when_one_of_its_folders_does() {
    let rig = rig();
    // E: has two folders; one stops answering, the other still does.
    rig.answers.0.lock().unwrap().insert(rig.ids[0], false);
    let overview = rig.service.overview().unwrap();
    assert!(overview.drives[0].online);
    rig.answers.0.lock().unwrap().insert(rig.ids[2], false);
    let overview = rig.service.overview().unwrap();
    assert!(!overview.drives[0].online);
}

#[test]
fn linux_mount_points_group_their_folders_by_the_mount() {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(Database::open(dir.path().join("library.db")).unwrap());
    let ids: Vec<i64> = [
        "/media/alex/Films/Movies",
        "/media/alex/Films/TV",
        "/mnt/nas/Movies",
    ]
    .into_iter()
    .map(|path| {
        db.write(move |tx| insert_source(tx, SourceKind::Movie, path))
            .unwrap()
            .id
    })
    .collect();
    let volumes = Fake::new(&[
        volume("/media/alex/Films", "Films", DriveKind::Hdd, 2 * TB, TB),
        volume("/mnt/nas", "nas", DriveKind::Network, 16 * TB, 9 * TB),
    ]);
    let answers = Arc::new(Answers(Mutex::new(
        ids.iter().map(|id| (*id, true)).collect(),
    )));
    let service = DrivesService::with(db, volumes, answers);

    let overview = service.overview().unwrap();
    let found: Vec<(&str, &str, usize)> = overview
        .drives
        .iter()
        .map(|d| (d.id.as_str(), d.name.as_str(), d.folders.len()))
        .collect();
    assert_eq!(
        found,
        [("/media/alex/Films", "Films", 2), ("/mnt/nas", "nas", 1)]
    );
    assert!(overview.drives.iter().all(|d| d.online));
}

#[test]
fn mount_points_that_differ_only_by_case_are_two_drives_on_linux_only() {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(Database::open(dir.path().join("library.db")).unwrap());
    let ids: Vec<i64> = ["/media/alex/Films/Movies", "/media/alex/films/TV"]
        .into_iter()
        .map(|path| {
            db.write(move |tx| insert_source(tx, SourceKind::Movie, path))
                .unwrap()
                .id
        })
        .collect();
    let volumes = Fake::new(&[
        volume("/media/alex/Films", "Films", DriveKind::Hdd, 2 * TB, TB),
        volume("/media/alex/films", "films", DriveKind::Hdd, 2 * TB, TB),
    ]);
    let answers = Arc::new(Answers(Mutex::new(
        ids.iter().map(|id| (*id, true)).collect(),
    )));
    let service = DrivesService::with(db, volumes, answers);

    // Linux keeps case in names, so the two mounts are two drives; Windows and macOS fold it.
    let overview = service.overview().unwrap();
    let expected = if cv_fs::paths::CASE_SENSITIVE { 2 } else { 1 };
    assert_eq!(overview.drives.len(), expected);
}

// ---- telling the interface ----------------------------------------------------------------------

#[test]
fn announcements_are_joined_and_a_drive_going_is_announced_once() {
    let rig = rig();
    let count = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&count);
    rig.service.on_changed(Arc::new(move || {
        seen.fetch_add(1, Ordering::Relaxed);
    }));

    // The first goes out at once; the ones in the window become one trailing announcement.
    rig.service.notify_changed();
    rig.service.notify_changed();
    rig.service.notify_changed();
    assert_eq!(count.load(Ordering::Relaxed), 1);
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(count.load(Ordering::Relaxed), 2);

    // A first overview only remembers which drives answer.
    rig.service.overview().unwrap();
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(count.load(Ordering::Relaxed), 2);
    // A drive going away is announced.
    for id in [rig.ids[0], rig.ids[2]] {
        rig.answers.0.lock().unwrap().insert(id, false);
    }
    rig.service.overview().unwrap();
    std::thread::sleep(Duration::from_millis(200));
    assert!(count.load(Ordering::Relaxed) >= 3);
    // The same drives again: nothing new.
    let before = count.load(Ordering::Relaxed);
    rig.service.overview().unwrap();
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(count.load(Ordering::Relaxed), before);
}

// ---- trash ---------------------------------------------------------------------------------------

#[test]
fn a_drive_shows_its_trash_folder_and_what_cinevault_sent_to_the_bin_and_empties_both() {
    use crate::history::{NewEntry, NewTitle, record};
    use crate::library_ops::trash_bin::fake::FakeBin;
    use crate::library_ops::{FolderOnly, send_or_fallback};
    use cv_db::repo::history::{HistoryKind, UndoState, get_entry};

    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(Database::open(dir.path().join("library.db")).unwrap());
    let root = dir.path().join("Movies");
    std::fs::create_dir_all(&root).unwrap();
    let text = root.to_string_lossy().into_owned();
    let id = db
        .write({
            let text = text.clone();
            move |tx| insert_source(tx, SourceKind::Movie, &text)
        })
        .unwrap()
        .id;
    let key = volume_key(&text);
    let volumes = Fake::new(&[volume(&key, "Local", DriveKind::Ssd, TB, 500 * GB)]);
    let answers = Arc::new(Answers(Mutex::new([(id, true)].into_iter().collect())));
    let service = DrivesService::with(Arc::clone(&db), volumes, answers);
    let bin = FakeBin::new(dir.path());
    service.set_trash(bin.clone(), crate::deleted_files::default_source());

    let put = |rel: &str, bytes: usize| {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, vec![1_u8; bytes]).unwrap();
        path
    };
    // A delete to the trash folder, and one to the bin.
    let a = put("Heat (1995)/movie.mkv", 40);
    let b = put("Alien (1979)/movie.mkv", 60);
    let folder = send_or_fallback(
        &FolderOnly,
        &[a.parent().unwrap().to_path_buf()],
        &root,
        "b1",
    )
    .unwrap();
    let binned = send_or_fallback(
        bin.as_ref(),
        &[b.parent().unwrap().to_path_buf()],
        &root,
        "b2",
    )
    .unwrap();
    let record_delete = |items: Vec<crate::library_ops::BinnedItem>| {
        record(
            &db,
            &NewEntry {
                kind: HistoryKind::Delete,
                summary: "Deleted".to_owned(),
                line: String::new(),
                bytes: None,
                extra: serde_json::json!({ "binned": [{ "item_id": 1, "items": items }] }),
                batch_ref: None,
                titles: vec![NewTitle {
                    item_id: cv_domain::ItemId(1),
                    title: "x".to_owned(),
                    before: String::new(),
                    after: String::new(),
                }],
            },
        )
        .unwrap()
    };
    let in_folder = record_delete(folder.items);
    let in_bin = record_delete(binned.items);

    let overview = service.overview().unwrap();
    let drive = &overview.drives[0];
    assert_eq!(drive.trash_bytes, 40);
    assert_eq!(drive.bin_bytes, 60);
    let contents = service.trash_contents(Some(&drive.id)).unwrap();
    assert_eq!(contents.batches.len(), 1);
    assert_eq!(contents.bin.len(), 1);
    assert_eq!(contents.bytes(), 100);

    assert!(matches!(
        service.empty_trash(None, "delete"),
        Err(EmptyError::Confirmation)
    ));
    assert_eq!(service.overview().unwrap().drives[0].trash_bytes, 40);

    let report = service.empty_trash(Some(&drive.id), "DELETE").unwrap();
    assert_eq!(
        (report.batches, report.bin_items, report.bytes),
        (1, 1, 100)
    );
    assert!(report.problems.is_empty(), "{:?}", report.problems);
    let after = &service.overview().unwrap().drives[0];
    assert_eq!((after.trash_bytes, after.bin_bytes), (0, 0));
    for id in [in_folder, in_bin] {
        let row = db.read(move |c| get_entry(c, id)).unwrap().unwrap();
        assert_eq!(row.undo_state, UndoState::None);
    }
    // The library folder itself is untouched.
    assert!(root.is_dir());
}

#[test]
fn emptying_never_touches_the_persons_own_files_in_the_custom_trash_folder() {
    use crate::deleted_files::{DeletedFiles, DeletedMode, FixedDeletedFiles};
    use crate::library_ops::trash_bin::fake::FakeBin;
    use crate::library_ops::{PolicyBin, send_or_fallback};

    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(Database::open(dir.path().join("library.db")).unwrap());
    let root = dir.path().join("Movies");
    std::fs::create_dir_all(&root).unwrap();
    let text = root.to_string_lossy().into_owned();
    let id = db
        .write({
            let text = text.clone();
            move |tx| insert_source(tx, SourceKind::Movie, &text)
        })
        .unwrap()
        .id;
    let key = volume_key(&text);
    let volumes = Fake::new(&[volume(&key, "Local", DriveKind::Ssd, TB, 500 * GB)]);
    let answers = Arc::new(Answers(Mutex::new([(id, true)].into_iter().collect())));
    let service = DrivesService::with(Arc::clone(&db), volumes, answers);

    // The person picks a folder that already holds their own files, some of them old.
    let custom = dir.path().join("Stuff");
    let own = custom.join("Holiday.mkv");
    let own_dir = custom.join("Projects").join("notes.txt");
    for path in [&own, &own_dir] {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"mine").unwrap();
        let old = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(old)
            .unwrap();
    }
    let choice = Arc::new(FixedDeletedFiles(DeletedFiles {
        mode: DeletedMode::Custom(custom.clone()),
        auto_empty_days: Some(7),
    }));
    let system = FakeBin::new(dir.path());
    let bin = PolicyBin::new(system, choice.clone());
    service.set_trash(
        Arc::new(PolicyBin::new(FakeBin::new(dir.path()), choice.clone())),
        choice,
    );

    let movie = root.join("Heat (1995)").join("movie.mkv");
    std::fs::create_dir_all(movie.parent().unwrap()).unwrap();
    std::fs::write(&movie, vec![1_u8; 40]).unwrap();
    let binned =
        send_or_fallback(&bin, &[movie.parent().unwrap().to_path_buf()], &root, "b1").unwrap();
    let place = binned.items[0]
        .bin_id
        .strip_prefix(crate::library_ops::FOLDER_PREFIX)
        .unwrap()
        .to_owned();
    assert!(Path::new(&place).starts_with(cv_fs::volume::custom_trash_root(&custom)));

    // The automatic empty finds only CineVault's batch (too new) and leaves the old own files.
    let auto = service.auto_empty().unwrap().unwrap();
    assert_eq!(auto.batches, 0);
    assert!(own.is_file() && own_dir.is_file());

    // The listing and Empty all name and erase CineVault's batch only.
    let contents = service.trash_contents(None).unwrap();
    assert_eq!(contents.batches.len(), 1, "{:?}", contents.batches);
    let report = service.empty_trash(None, "DELETE").unwrap();
    assert_eq!(report.batches, 1);
    assert!(!Path::new(&place).exists());
    assert!(
        own.is_file(),
        "the person's own file in the custom folder stays"
    );
    assert!(
        own_dir.is_file(),
        "the person's own folder in the custom folder stays"
    );
}

/// A bin whose purge erases the first item it is given and then fails.
struct StopsPartWay(Arc<crate::library_ops::trash_bin::fake::FakeBin>);

impl crate::library_ops::SystemTrash for StopsPartWay {
    fn supports(&self, path: &Path) -> bool {
        self.0.supports(path)
    }
    fn send(&self, paths: &[PathBuf]) -> Result<(), String> {
        self.0.send(paths)
    }
}

impl crate::library_ops::SystemBin for StopsPartWay {
    fn send_tracked(&self, paths: &[PathBuf]) -> crate::library_ops::trash_bin::SendOutcome {
        self.0.send_tracked(paths)
    }
    fn present(&self, items: &[crate::library_ops::BinnedItem]) -> Vec<bool> {
        self.0.present(items)
    }
    fn restore(
        &self,
        items: &[crate::library_ops::BinnedItem],
    ) -> Result<(), Vec<(String, String)>> {
        self.0.restore(items)
    }
    fn purge(&self, items: &[crate::library_ops::BinnedItem]) -> Result<(), String> {
        self.0.purge(&items[..1])?;
        Err("the bin stopped".to_owned())
    }
}

#[test]
fn a_bin_empty_that_stops_part_way_switches_off_the_undo_of_what_it_erased_only() {
    use crate::history::{NewEntry, NewTitle, record};
    use crate::library_ops::send_or_fallback;
    use crate::library_ops::trash_bin::fake::FakeBin;
    use cv_db::repo::history::{HistoryKind, UndoState, get_entry};

    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(Database::open(dir.path().join("library.db")).unwrap());
    let root = dir.path().join("Movies");
    std::fs::create_dir_all(&root).unwrap();
    let text = root.to_string_lossy().into_owned();
    let id = db
        .write({
            let text = text.clone();
            move |tx| insert_source(tx, SourceKind::Movie, &text)
        })
        .unwrap()
        .id;
    let key = volume_key(&text);
    let volumes = Fake::new(&[volume(&key, "Local", DriveKind::Ssd, TB, 500 * GB)]);
    let answers = Arc::new(Answers(Mutex::new([(id, true)].into_iter().collect())));
    let service = DrivesService::with(Arc::clone(&db), volumes, answers);
    let fake = FakeBin::new(dir.path());
    service.set_trash(
        Arc::new(StopsPartWay(Arc::clone(&fake))),
        crate::deleted_files::default_source(),
    );
    let mut entries = Vec::new();
    for (name, bytes) in [("Heat (1995)", 60_usize), ("Alien (1979)", 40)] {
        let path = root.join(name).join("movie.mkv");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, vec![1_u8; bytes]).unwrap();
        let binned = send_or_fallback(
            fake.as_ref(),
            &[path.parent().unwrap().to_path_buf()],
            &root,
            name,
        )
        .unwrap();
        let entry = record(
            &db,
            &NewEntry {
                kind: HistoryKind::Delete,
                summary: "Deleted".to_owned(),
                line: String::new(),
                bytes: None,
                extra: serde_json::json!({ "binned": [{ "item_id": 1, "items": binned.items }] }),
                batch_ref: None,
                titles: vec![NewTitle {
                    item_id: cv_domain::ItemId(1),
                    title: name.to_owned(),
                    before: String::new(),
                    after: String::new(),
                }],
            },
        )
        .unwrap();
        entries.push(entry);
    }
    let report = service.empty_trash(None, "DELETE").unwrap();
    assert_eq!(report.bin_items, 1, "{report:?}");
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.problems.len(), 1);
    let states: Vec<UndoState> = entries
        .iter()
        .map(|&id| {
            db.read(move |c| get_entry(c, id))
                .unwrap()
                .unwrap()
                .undo_state
        })
        .collect();
    assert_eq!(
        states.iter().filter(|s| **s == UndoState::None).count(),
        1,
        "only the entry whose files were erased loses its Undo"
    );
}

/// A bin laid out like the freedesktop Trash (`files/` and `info/*.trashinfo`), filled by the
/// same sender CineVault uses on Linux. Its `bin_id` is the info file, as the system's own
/// listing names it. It stands in for the real Trash, which a test never touches.
#[cfg(unix)]
struct TrashFolder {
    trash: PathBuf,
}

#[cfg(unix)]
impl TrashFolder {
    fn info_of(&self, item: &crate::library_ops::BinnedItem) -> PathBuf {
        PathBuf::from(&item.bin_id)
    }

    fn stored_of(&self, item: &crate::library_ops::BinnedItem) -> PathBuf {
        let info = self.info_of(item);
        let name = info.file_stem().unwrap().to_owned();
        self.trash.join("files").join(name)
    }
}

#[cfg(unix)]
impl crate::library_ops::SystemTrash for TrashFolder {
    fn supports(&self, _path: &Path) -> bool {
        true
    }
    fn send(&self, paths: &[PathBuf]) -> Result<(), String> {
        for path in paths {
            crate::library_ops::xdg_trash::move_into(&self.trash, path, "2026-10-28T10:00:00")?;
        }
        Ok(())
    }
}

#[cfg(unix)]
impl crate::library_ops::SystemBin for TrashFolder {
    fn send_tracked(&self, paths: &[PathBuf]) -> crate::library_ops::trash_bin::SendOutcome {
        let mut sent = Vec::new();
        for path in paths {
            let size = crate::library_ops::trash_bin::size_of(path);
            let moved =
                crate::library_ops::xdg_trash::move_into(&self.trash, path, "2026-10-28T10:00:00");
            match moved {
                Ok(info) => sent.push(crate::library_ops::BinnedItem {
                    original: path.to_string_lossy().into_owned(),
                    bin_id: info.to_string_lossy().into_owned(),
                    size,
                }),
                Err(error) => {
                    return crate::library_ops::trash_bin::SendOutcome {
                        sent,
                        error: Some(error),
                    };
                }
            }
        }
        crate::library_ops::trash_bin::SendOutcome { sent, error: None }
    }
    fn present(&self, items: &[crate::library_ops::BinnedItem]) -> Vec<bool> {
        items
            .iter()
            .map(|item| !item.bin_id.is_empty() && self.info_of(item).is_file())
            .collect()
    }
    fn restore(
        &self,
        _items: &[crate::library_ops::BinnedItem],
    ) -> Result<(), Vec<(String, String)>> {
        Err(vec![(String::new(), "not needed here".to_owned())])
    }
    fn purge(&self, items: &[crate::library_ops::BinnedItem]) -> Result<(), String> {
        for item in items {
            let stored = self.stored_of(item);
            let _ = if stored.is_dir() {
                std::fs::remove_dir_all(&stored)
            } else {
                std::fs::remove_file(&stored)
            };
            let _ = std::fs::remove_file(self.info_of(item));
        }
        Ok(())
    }
}

#[cfg(unix)]
#[test]
fn a_drive_counts_and_empties_only_the_trash_items_cinevault_recorded() {
    use crate::history::{NewEntry, NewTitle, record};
    use crate::library_ops::send_or_fallback;
    use cv_db::repo::history::{HistoryKind, UndoState, get_entry};

    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(Database::open(dir.path().join("library.db")).unwrap());
    let root = dir.path().join("Movies");
    std::fs::create_dir_all(&root).unwrap();
    let text = root.to_string_lossy().into_owned();
    let id = db
        .write({
            let text = text.clone();
            move |tx| insert_source(tx, SourceKind::Movie, &text)
        })
        .unwrap()
        .id;
    let key = volume_key(&text);
    let volumes = Fake::new(&[volume(&key, "Local", DriveKind::Ssd, TB, 500 * GB)]);
    let answers = Arc::new(Answers(Mutex::new([(id, true)].into_iter().collect())));
    let service = DrivesService::with(Arc::clone(&db), volumes, answers);
    let trash = TrashFolder {
        trash: dir.path().join("Trash"),
    };
    let bin = Arc::new(trash);
    service.set_trash(bin.clone(), crate::deleted_files::default_source());

    // The person's own earlier deletes sit in the same Trash; CineVault did not send them.
    let foreign = dir.path().join("Foreign.mkv");
    std::fs::write(&foreign, vec![9_u8; 500]).unwrap();
    crate::library_ops::xdg_trash::move_into(&bin.trash, &foreign, "2026-10-01T08:00:00").unwrap();

    let put = |rel: &str, bytes: usize| {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, vec![1_u8; bytes]).unwrap();
        path
    };
    let a = put("Heat (1995)/movie.mkv", 40);
    let b = put("Alien (1979)/movie.mkv", 60);
    let binned = send_or_fallback(
        bin.as_ref(),
        &[
            a.parent().unwrap().to_path_buf(),
            b.parent().unwrap().to_path_buf(),
        ],
        &root,
        "b1",
    )
    .unwrap();
    assert_eq!(binned.items.len(), 2, "{:?}", binned.notes);
    assert!(binned.items.iter().all(|i| bin.info_of(i).is_file()));
    let recorded = record(
        &db,
        &NewEntry {
            kind: HistoryKind::Delete,
            summary: "Deleted".to_owned(),
            line: String::new(),
            bytes: None,
            extra: serde_json::json!({ "binned": [{ "item_id": 1, "items": binned.items }] }),
            batch_ref: None,
            titles: vec![NewTitle {
                item_id: cv_domain::ItemId(1),
                title: "x".to_owned(),
                before: String::new(),
                after: String::new(),
            }],
        },
    )
    .unwrap();

    // The size is the two folders CineVault sent, not the person's 500 bytes.
    let overview = service.overview().unwrap();
    assert_eq!(overview.drives[0].bin_bytes, 100);
    let contents = service.trash_contents(None).unwrap();
    assert_eq!(contents.bin.len(), 2);

    let report = service.empty_trash(None, "DELETE").unwrap();
    assert_eq!((report.bin_items, report.bytes), (2, 100));
    assert!(report.problems.is_empty(), "{:?}", report.problems);
    assert_eq!(service.overview().unwrap().drives[0].bin_bytes, 0);
    // CineVault's items are gone; the person's own file and its info file are untouched.
    assert!(bin.trash.join("files/Foreign.mkv").is_file());
    assert!(bin.trash.join("info/Foreign.mkv.trashinfo").is_file());
    assert!(!bin.trash.join("files/Heat (1995)").exists());
    assert!(!bin.trash.join("files/Alien (1979)").exists());
    let row = db.read(move |c| get_entry(c, recorded)).unwrap().unwrap();
    assert_eq!(row.undo_state, UndoState::None);
}
