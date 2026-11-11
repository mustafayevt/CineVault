//! Scanning one title's folder ("Scan again") and the scan's progress in titles.

mod support;

use std::sync::Mutex;

use cv_domain::{Availability, ItemKind};
use cv_scan::rescan::rescan_folder;
use cv_scan::walk::{WalkObserver, WalkOptions, WalkScope, count_titles, scan_movie_source_in};
use support::Lib;

fn three_movies() -> Lib {
    let lib = Lib::movies();
    lib.put("Alien (1979)/Alien (1979).mkv", "alien-video");
    lib.put("Heat (1995)/Heat (1995).mkv", "heat-video");
    lib.put("Ronin (1998)/Ronin (1998).mkv", "ronin-video");
    lib.rescan();
    lib
}

fn folder(lib: &Lib, rel: &str) -> String {
    lib.root.join(rel).to_string_lossy().into_owned()
}

fn movie(lib: &Lib, title: &str) -> cv_domain::Item {
    lib.items(ItemKind::Movie)
        .into_iter()
        .find(|m| m.core.title == title)
        .unwrap_or_else(|| panic!("{title} is in the library"))
}

fn state(lib: &Lib, title: &str) -> String {
    lib.availability(movie(lib, title).id.unwrap())
}

#[test]
fn scanning_one_folder_never_marks_titles_in_other_folders_missing() {
    let lib = three_movies();
    // Two folders go; only Heat is scanned again.
    lib.remove("Alien (1979)");
    lib.remove("Ronin (1998)");
    let report = rescan_folder(
        &lib.db,
        &lib.source,
        &folder(&lib, "Heat (1995)"),
        &lib.options(),
    )
    .expect("scan again");
    assert!(report.availability.became_missing.is_empty(), "{report:?}");
    assert_eq!(state(&lib, "Alien"), Availability::Online.as_str());
    assert_eq!(state(&lib, "Ronin"), Availability::Online.as_str());
    assert_eq!(state(&lib, "Heat"), Availability::Online.as_str());
    // The fingerprints of what was not read are kept.
    assert_eq!(lib.count("fingerprint"), 3);
}

#[test]
fn scanning_a_title_whose_folder_is_gone_marks_it_missing_and_back_when_it_returns() {
    let lib = three_movies();
    let heat = movie(&lib, "Heat").id.unwrap();
    lib.remove("Heat (1995)");
    let report = rescan_folder(
        &lib.db,
        &lib.source,
        &folder(&lib, "Heat (1995)"),
        &lib.options(),
    )
    .expect("scan again");
    assert_eq!(report.availability.became_missing, vec![heat]);
    assert_eq!(lib.availability(heat), Availability::Missing.as_str());
    // Nothing is deleted.
    assert_eq!(lib.items(ItemKind::Movie).len(), 3);
    // The others are untouched.
    assert_eq!(state(&lib, "Alien"), Availability::Online.as_str());

    lib.put("Heat (1995)/Heat (1995).mkv", "heat-video");
    let report = rescan_folder(
        &lib.db,
        &lib.source,
        &folder(&lib, "Heat (1995)"),
        &lib.options(),
    )
    .expect("scan again");
    assert_eq!(report.availability.restored, vec![heat]);
    assert_eq!(lib.availability(heat), Availability::Online.as_str());
    assert!(
        report.new_items.is_empty(),
        "the title is found again, not added"
    );
}

#[test]
fn scanning_a_title_reads_its_new_files_and_nothing_outside_it() {
    let lib = three_movies();
    lib.put("Heat (1995)/Heat (1995).en.srt", "subtitle");
    lib.put("Alien (1979)/Alien (1979)-poster.jpg", "poster");
    lib.put("Brand New (2024)/Brand New (2024).mkv", "new-video");
    let report = rescan_folder(
        &lib.db,
        &lib.source,
        &folder(&lib, "Heat (1995)"),
        &lib.options(),
    )
    .expect("scan again");
    assert!(report.new_items.is_empty(), "{report:?}");
    assert_eq!(movie(&lib, "Heat").core.media_files.len(), 2);
    assert_eq!(movie(&lib, "Alien").core.media_files.len(), 1);
    assert_eq!(lib.items(ItemKind::Movie).len(), 3);
    // A whole scan afterwards still finds what the folder scan left alone.
    let full = lib.rescan();
    assert_eq!(full.new_items.len(), 1);
    assert_eq!(movie(&lib, "Alien").core.media_files.len(), 2);
}

#[test]
fn a_title_in_a_group_folder_is_read_as_a_whole_scan_reads_it() {
    let lib = Lib::movies();
    lib.put("Collection/Heat (1995)/Heat (1995).mkv", "heat");
    lib.put("Collection/Alien (1979)/Alien (1979).mkv", "alien");
    lib.rescan();
    let before = movie(&lib, "Heat");
    lib.put("Collection/Heat (1995)/Heat (1995)-fanart.jpg", "fanart");
    let report = rescan_folder(
        &lib.db,
        &lib.source,
        &folder(&lib, "Collection/Heat (1995)"),
        &lib.options(),
    )
    .expect("scan again");
    assert!(report.new_items.is_empty());
    let after = movie(&lib, "Heat");
    assert_eq!(after.id, before.id);
    assert_eq!(after.core.path, before.core.path);
    assert_eq!(after.core.media_files.len(), 2);
}

#[test]
fn scanning_one_show_leaves_the_other_shows_alone() {
    let lib = Lib::tv();
    lib.put("Example Show/Season 1/Example Show S01E01.mkv", "e1");
    lib.put("Other Show/Season 1/Other Show S01E01.mkv", "o1");
    lib.rescan();
    lib.remove("Other Show");
    lib.put("Example Show/Season 1/Example Show S01E02.mkv", "e2");
    let report = rescan_folder(
        &lib.db,
        &lib.source,
        &folder(&lib, "Example Show"),
        &lib.options(),
    )
    .expect("scan again");
    assert_eq!(report.new_episodes, 1);
    assert!(report.availability.became_missing.is_empty(), "{report:?}");
    let other = lib
        .items(ItemKind::Show)
        .into_iter()
        .find(|s| s.core.title.contains("Other"))
        .unwrap();
    assert_eq!(
        lib.availability(other.id.unwrap()),
        Availability::Online.as_str()
    );
}

#[derive(Default)]
struct Heard {
    found: Mutex<usize>,
    read: Mutex<usize>,
    names: Mutex<Vec<String>>,
}

impl WalkObserver for Heard {
    fn found(&self, count: usize) {
        *self.found.lock().unwrap() += count;
    }
    fn reading(&self, name: &str) {
        self.names.lock().unwrap().push(name.to_owned());
    }
    fn read(&self, count: usize) {
        *self.read.lock().unwrap() += count;
    }
}

#[test]
fn a_walk_tells_its_titles_and_the_count_agrees() {
    let lib = Lib::movies();
    lib.put("Alien (1979)/Alien (1979).mkv", "a");
    lib.put("Heat (1995)/Heat (1995).mkv", "h");
    lib.put("Loose Movie (2001).mkv", "l");
    // A bucket folder: its own folders and videos are the titles.
    lib.put("R/Ronin (1998)/Ronin (1998).mkv", "r");
    lib.put("R/Rocky (1976).mkv", "r2");
    // A skipped folder is no title.
    lib.put("MAKEMKV/title.mkv", "m");
    let options = WalkOptions::with_defaults();
    assert_eq!(count_titles(&lib.root, &options), Some(5));

    let heard = Heard::default();
    let report = scan_movie_source_in(
        &lib.root,
        None,
        &options,
        WalkScope {
            focus: None,
            observer: Some(&heard),
        },
    );
    assert_eq!(report.movies.len(), 5);
    assert_eq!(*heard.found.lock().unwrap(), 5);
    assert_eq!(*heard.read.lock().unwrap(), 5);
    let mut names = heard.names.lock().unwrap().clone();
    names.sort();
    assert_eq!(names, ["Alien (1979)", "Heat (1995)", "Ronin (1998)"]);
}

#[test]
fn a_folder_that_cannot_be_listed_has_no_count() {
    let lib = Lib::movies();
    let gone = lib.root.join("gone");
    assert_eq!(count_titles(&gone, &WalkOptions::with_defaults()), None);
}
