//! "Find missing": videos of a data source that belong to no library item.

mod support;

use cv_scan::unmanaged::{UnmanagedReason, UnmanagedReport, find_unmanaged};
use cv_scan::walk::WalkOptions;
use support::Lib;

fn find(lib: &Lib) -> UnmanagedReport {
    let options = match lib.source.kind {
        cv_domain::SourceKind::Movie => WalkOptions::with_defaults(),
        cv_domain::SourceKind::Tv => WalkOptions::tv_defaults(),
    };
    find_unmanaged(&lib.db, &lib.source, &options).expect("find missing")
}

fn relatives(report: &UnmanagedReport) -> Vec<&str> {
    report.videos.iter().map(|v| v.relative.as_str()).collect()
}

#[test]
fn a_fully_scanned_library_has_no_unmanaged_videos() {
    let lib = Lib::movies();
    lib.put("Alien (1979)/Alien (1979).mkv", "a");
    lib.put("Alien (1979)/Alien (1979)-trailer.mkv", "t");
    lib.put("Alien (1979)/Extras/Making of.mkv", "e");
    lib.put("Heat (1995)/Heat (1995) CD1.avi", "h1");
    lib.put("Heat (1995)/Heat (1995) CD2.avi", "h2");
    lib.put("Heat (1995)/Heat (1995).srt", "s");
    lib.rescan();

    let report = find(&lib);
    assert!(!report.unreadable);
    assert!(relatives(&report).is_empty(), "{:?}", report.videos);
}

#[test]
fn a_video_added_after_the_scan_is_listed_with_its_folder() {
    let lib = Lib::movies();
    lib.put("Alien (1979)/Alien (1979).mkv", "a");
    lib.rescan();
    let path = lib.put("Heat (1995)/Heat (1995).mkv", "heat!");

    let report = find(&lib);
    assert_eq!(relatives(&report), vec!["Heat (1995)/Heat (1995).mkv"]);
    let video = &report.videos[0];
    assert_eq!(video.path, path);
    assert_eq!(video.folder, path.parent().unwrap());
    assert_eq!(video.size, 5);
    assert_eq!(video.reason, UnmanagedReason::NotGrouped);
    assert!(!video.is_disc);

    // Once the folder is scanned it is managed.
    lib.rescan();
    assert!(find(&lib).videos.is_empty());
}

#[test]
fn videos_in_skipped_locations_are_listed_with_that_reason() {
    let lib = Lib::movies();
    lib.put("Alien (1979)/Alien (1979).mkv", "a");
    lib.put("Hidden (2001)/.nomedia", "");
    lib.put("Hidden (2001)/Hidden (2001).mkv", "h");
    lib.put("MAKEMKV/Unnamed.mkv", "m");
    lib.put("Sorted/Deep/.cinevaultignore", "");
    lib.put("Sorted/Deep/Below/Film (2010)/Film (2010).mkv", "f");
    // Hidden and system folders are not the user's content.
    lib.put("@eaDir/Alien (1979).mkv/SYNOFILE_THUMB.mkv", "x");
    lib.put(".git/video.mkv", "x");
    lib.rescan();

    let report = find(&lib);
    assert_eq!(
        relatives(&report),
        vec![
            "Hidden (2001)/Hidden (2001).mkv",
            "MAKEMKV/Unnamed.mkv",
            "Sorted/Deep/Below/Film (2010)/Film (2010).mkv",
        ]
    );
    assert!(
        report
            .videos
            .iter()
            .all(|v| v.reason == UnmanagedReason::SkippedLocation)
    );
    // Listing changes nothing.
    assert_eq!(lib.item_count(), 1);
}

#[test]
fn disc_structures_are_listed_once_as_their_folder_and_managed_discs_are_not() {
    let lib = Lib::movies();
    lib.put("Managed Disc (2000)/VIDEO_TS/VIDEO_TS.IFO", "i");
    lib.put("Managed Disc (2000)/VIDEO_TS/VTS_01_1.VOB", "v");
    lib.rescan();
    assert_eq!(lib.item_count(), 1);
    assert!(find(&lib).videos.is_empty());

    lib.put("New Disc (2001)/BDMV/index.bdmv", "i");
    lib.put("New Disc (2001)/BDMV/STREAM/00000.m2ts", "v");
    let report = find(&lib);
    assert_eq!(relatives(&report), vec!["New Disc (2001)/BDMV"]);
    assert!(report.videos[0].is_disc);
}

#[test]
fn an_episode_in_the_root_of_a_tv_source_is_listed() {
    let lib = Lib::tv();
    lib.put("Show/Season 1/Show S01E01.mkv", "e");
    lib.put("Stray S01E02.mkv", "e");
    lib.rescan();

    let report = find(&lib);
    assert_eq!(relatives(&report), vec!["Stray S01E02.mkv"]);
}

#[test]
fn a_missing_source_folder_is_reported_not_listed() {
    let lib = Lib::movies();
    lib.put("Alien (1979)/Alien (1979).mkv", "a");
    lib.rescan();
    std::fs::remove_dir_all(&lib.root).unwrap();

    let report = find(&lib);
    assert!(report.unreadable);
    assert!(report.videos.is_empty());
}
