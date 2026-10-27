//! "Date added" of a new title is when its main video file was created, not when the scan saw it.

mod support;

use std::fs;
use std::path::Path;
use std::time::UNIX_EPOCH;

use cv_domain::ItemKind;
use cv_scan::rescan::RescanOptions;
use support::Lib;

/// A far-away "now", so a real file time is never clamped by it.
const LATER: i64 = 4_000_000_000_000;

fn options(now_ms: i64) -> RescanOptions {
    RescanOptions {
        now_ms: Some(now_ms),
        ..Default::default()
    }
}

fn ms(time: std::io::Result<std::time::SystemTime>) -> Option<i64> {
    let since = time.ok()?.duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(since.as_millis()).ok()
}

/// What the scan should read for a file: its creation time, else its modification time.
fn expected(path: &Path) -> i64 {
    let meta = fs::metadata(path).expect("metadata");
    ms(meta.created())
        .filter(|t| *t > 0)
        .or_else(|| ms(meta.modified()))
        .expect("a file time")
}

#[test]
fn a_new_movie_is_added_on_the_day_its_video_file_was_created() {
    let lib = Lib::movies();
    let video = lib.put("Alien (1979)/Alien (1979).mkv", "alien-video");
    lib.put("Alien (1979)/Alien (1979)-poster.jpg", "poster");
    cv_scan::rescan::rescan_source(&lib.db, &lib.source, &options(LATER)).expect("rescan");

    let movies = lib.items(ItemKind::Movie);
    assert_eq!(movies.len(), 1);
    assert_eq!(movies[0].core.date_added_ms, expected(&video));
    assert!(movies[0].core.date_added_ms < LATER);
}

#[test]
fn titles_found_by_one_scan_do_not_share_one_date_when_their_files_differ() {
    let lib = Lib::movies();
    let first = lib.put("Alien (1979)/Alien (1979).mkv", "a");
    std::thread::sleep(std::time::Duration::from_millis(30));
    let second = lib.put("Heat (1995)/Heat (1995).mkv", "h");
    cv_scan::rescan::rescan_source(&lib.db, &lib.source, &options(LATER)).expect("rescan");

    let movies = lib.items(ItemKind::Movie);
    let added = |title: &str| {
        movies
            .iter()
            .find(|m| m.core.title == title)
            .unwrap()
            .core
            .date_added_ms
    };
    assert_eq!(added("Alien"), expected(&first));
    assert_eq!(added("Heat"), expected(&second));
}

#[test]
fn a_date_is_never_in_the_future() {
    let lib = Lib::movies();
    lib.put("Alien (1979)/Alien (1979).mkv", "alien-video");
    // The clock of the scan is long before the file's time: the scan's time is used.
    cv_scan::rescan::rescan_source(&lib.db, &lib.source, &options(1_000_000)).expect("rescan");
    assert_eq!(lib.items(ItemKind::Movie)[0].core.date_added_ms, 1_000_000);
}

#[test]
fn a_show_is_as_old_as_its_oldest_episode_file_and_each_episode_has_its_own_file_date() {
    let lib = Lib::tv();
    let first = lib.put("Severance (2022)/Season 01/Severance.S01E01.mkv", "e1");
    std::thread::sleep(std::time::Duration::from_millis(30));
    let second = lib.put("Severance (2022)/Season 01/Severance.S01E02.mkv", "e2");
    cv_scan::rescan::rescan_source(&lib.db, &lib.source, &options(LATER)).expect("rescan");

    let shows = lib.items(ItemKind::Show);
    assert_eq!(shows.len(), 1);
    assert_eq!(
        shows[0].core.date_added_ms,
        expected(&first).min(expected(&second))
    );
    let mut episodes: Vec<i64> = lib
        .items(ItemKind::Episode)
        .iter()
        .map(|episode| episode.core.date_added_ms)
        .collect();
    episodes.sort_unstable();
    let mut files = vec![expected(&first), expected(&second)];
    files.sort_unstable();
    assert_eq!(episodes, files);
}
