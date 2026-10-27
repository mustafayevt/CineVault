//! Optional file watching: copying a folder into a data source triggers one
//! incremental scan once the copy has been idle for the debounce.

mod support;

use std::fs;
use std::sync::mpsc;
use std::thread::sleep;
use std::time::{Duration, Instant};

use cv_domain::ItemKind;
use cv_scan::skip::LibraryKind;
use cv_scan::watch::{FolderWatcher, Settled, WatchOptions, WatchedRoot, rescan_settled};
use support::Lib;

/// Short enough to keep the test quick, long enough that a loaded CI machine does not pause a
/// whole debounce between two writes.
const DEBOUNCE: Duration = Duration::from_millis(1500);
const WAIT: Duration = Duration::from_secs(30);

fn start(lib: &Lib) -> (FolderWatcher, mpsc::Receiver<(Instant, Settled)>) {
    let (tx, rx) = mpsc::channel();
    let root = WatchedRoot::new(lib.source.id, &lib.root, LibraryKind::Movies);
    let options = WatchOptions::default().with_debounce_unchecked(DEBOUNCE);
    let watcher = FolderWatcher::start(&[root], &options, move |settled| {
        let _ = tx.send((Instant::now(), settled));
    })
    .expect("start watching");
    assert!(watcher.unwatched().is_empty());
    (watcher, rx)
}

#[test]
fn copying_a_folder_in_triggers_one_scan_after_the_copy_settles() {
    let lib = Lib::movies();
    lib.put("Alien (1979)/Alien (1979).mkv", "alien");
    lib.rescan();
    let known = lib.item_count();

    let (_watcher, rx) = start(&lib);

    // Copy a movie folder in, file by file, the way a slow copy does.
    let staged = lib.dir.path().join("staging").join("Heat (1995)");
    fs::create_dir_all(&staged).unwrap();
    for (name, content) in [
        ("Heat (1995).mkv", "heat-video"),
        ("Heat (1995)-poster.jpg", "poster"),
        ("Heat (1995).nfo", "<movie/>"),
    ] {
        fs::write(staged.join(name), content).unwrap();
    }
    let target = lib.root.join("Heat (1995)");
    fs::create_dir_all(&target).unwrap();
    let mut last_write = Instant::now();
    for entry in fs::read_dir(&staged).unwrap().flatten() {
        fs::copy(entry.path(), target.join(entry.file_name())).unwrap();
        last_write = Instant::now();
        sleep(Duration::from_millis(150));
    }

    // Nothing arrives before the copy has been idle for the debounce; then one batch does.
    let (arrived, settled) = rx.recv_timeout(WAIT).expect("a settled batch");
    assert!(
        arrived.duration_since(last_write) >= DEBOUNCE,
        "the batch came {:?} after the last write",
        arrived.duration_since(last_write)
    );
    assert_eq!(settled.source_id, lib.source.id);
    assert_eq!(
        settled.folders,
        vec![std::path::PathBuf::from("Heat (1995)")]
    );

    // The scan the batch asks for finds the whole movie, flagged NEW.
    let reports = rescan_settled(&lib.db, &[settled], &lib.options()).expect("rescan");
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].new_items.len(), 1);
    assert_eq!(lib.item_count(), known + 1);
    let heat = lib
        .items(ItemKind::Movie)
        .into_iter()
        .find(|m| m.core.title == "Heat")
        .expect("Heat is in the library");
    assert_eq!(heat.core.media_files.len(), 3);
    assert!(lib.is_new(heat.id.unwrap()));

    // The scan itself reads files but changes none: no second batch follows.
    assert!(
        rx.recv_timeout(DEBOUNCE * 2).is_err(),
        "one copy must give one batch"
    );
}

#[test]
fn a_quiet_library_and_ignored_files_trigger_nothing() {
    let lib = Lib::movies();
    lib.put("Alien (1979)/Alien (1979).mkv", "alien");
    let (_watcher, rx) = start(&lib);

    lib.put("Alien (1979)/Thumbs.db", "junk");
    lib.put(".hidden/note.txt", "junk");
    assert!(rx.recv_timeout(DEBOUNCE * 2).is_err());
}

#[test]
fn a_source_that_cannot_be_watched_is_reported_and_the_others_still_work() {
    let lib = Lib::movies();
    let (tx, rx) = mpsc::channel();
    let roots = [
        WatchedRoot::new(99, lib.dir.path().join("gone"), LibraryKind::Movies),
        WatchedRoot::new(lib.source.id, &lib.root, LibraryKind::Movies),
    ];
    let options = WatchOptions::default().with_debounce_unchecked(DEBOUNCE);
    let watcher = FolderWatcher::start(&roots, &options, move |s| {
        let _ = tx.send(s);
    })
    .expect("start watching");
    assert_eq!(watcher.unwatched().len(), 1);
    assert_eq!(watcher.unwatched()[0].0, 99);

    lib.put("Heat (1995)/Heat (1995).mkv", "heat");
    let settled = rx.recv_timeout(WAIT).expect("a settled batch");
    assert_eq!(settled.source_id, lib.source.id);
}

#[test]
fn dropping_the_watcher_stops_it() {
    let lib = Lib::movies();
    let (watcher, rx) = start(&lib);
    lib.put("Heat (1995)/Heat (1995).mkv", "heat");
    drop(watcher);
    // The channel is closed, not fed: the pending batch is discarded.
    assert!(matches!(
        rx.recv_timeout(DEBOUNCE * 2),
        Err(mpsc::RecvTimeoutError::Disconnected)
    ));
}

/// Watching a data source never holds the folders below it: a movie folder (here a Blu-ray
/// folder) is renamed on the first try while the watcher runs, also right after its files
/// changed. On Windows the watcher has one handle, on the data source root; a handle on the
/// movie folder or anything below it would make the rename fail with "access denied".
#[test]
fn a_watched_data_source_never_blocks_a_rename_of_a_folder_below_it() {
    let lib = Lib::movies();
    let old = "Bridge of Spies (2015) [1080p] [Blu-ray] [AC3 2.0]";
    let new = "Bridge of Spies (2015) [1080p] [Blu-ray] [DTS-HD MA 7.1]";
    lib.put(&format!("{old}/BDMV/index.bdmv"), "index");
    lib.put(&format!("{old}/BDMV/PLAYLIST/00800.mpls"), "playlist");
    lib.put(&format!("{old}/BDMV/STREAM/00800.m2ts"), "stream");
    let (_watcher, _rx) = start(&lib);
    // Events for the folder are on their way while it is renamed.
    fs::write(lib.root.join(old).join("Bridge of Spies.nfo"), "<movie/>").unwrap();
    fs::rename(lib.root.join(old), lib.root.join(new)).expect("the folder is not held");
    fs::rename(lib.root.join(new), lib.root.join(old)).expect("and back again");
    assert!(lib.root.join(old).join("BDMV/STREAM/00800.m2ts").is_file());
}
