//! Safety checks on Delete and permanent delete: a
//! title path with `.` or `..` parts is never taken for a folder inside its library folder, and
//! both deletes hold their titles, so no NFO or artwork write lands in a folder that is going.

use std::time::Duration;

use super::*;
use crate::title_guard::{self, Work};

/// A movie whose folder path climbs out of the library folder (`<movies>/<rel>`), with one file.
fn stray_movie(lab: &Lab, rel: &str, file: &str) -> ItemId {
    let mut item = Item::movie("Stray");
    item.core.data_source.clone_from(&lab.movies.path);
    item.core.path = format!("{}{}{rel}", lab.movies.path, std::path::MAIN_SEPARATOR);
    item.core.media_files.push(MediaFile::new(
        MediaFileType::Video,
        item.core.path.clone(),
        file,
    ));
    lab.insert(item)
}

#[test]
fn a_folder_path_that_climbs_out_of_its_library_folder_is_never_erased() {
    let lab = Lab::new();
    let trash = FakeTrash::new(lab.dir.path());
    let outside = Lab::put(lab.dir.path(), "elsewhere/precious.mkv", "keep");
    let stray = stray_movie(&lab, "../elsewhere", "precious.mkv");

    let report = lab.removal(&trash).erase(&[stray], "DELETE").unwrap();
    assert!(report.erased.is_empty(), "{report:?}");
    assert_eq!(report.problems.len(), 1, "{report:?}");
    assert!(
        outside.is_file(),
        "nothing outside the library folder is erased"
    );

    let report = lab.removal(&trash).delete_with_files(&[stray]).unwrap();
    assert!(report.deleted.is_empty(), "{report:?}");
    assert!(outside.is_file(), "nor sent to the bin");
    assert!(lab.exists(stray) && !lab.hidden(stray));
}

#[test]
fn a_folder_path_that_names_the_library_folder_itself_is_never_erased() {
    let lab = Lab::new();
    let trash = FakeTrash::new(lab.dir.path());
    let heat = lab.movie(&lab.movies, "Heat (1995)");
    let video = Path::new(&lab.movies.path).join("Heat (1995)/Heat (1995).mkv");
    let stray = stray_movie(&lab, ".", "Heat (1995).mkv");

    let report = lab.removal(&trash).erase(&[stray], "DELETE").unwrap();
    assert!(report.erased.is_empty(), "{report:?}");
    assert!(video.is_file(), "the library folder and its titles stay");
    assert!(lab.exists(heat));
}

#[test]
fn a_permanent_delete_waits_for_a_write_into_the_title_and_erases_what_it_wrote() {
    let lab = Lab::new();
    let trash = FakeTrash::new(lab.dir.path());
    let heat = lab.movie(&lab.movies, "Heat (1995)");
    let folder = Path::new(&lab.movies.path).join("Heat (1995)");

    // A background NFO write holds the title and finishes a moment after the erase was asked.
    let writing = title_guard::hold(&lab.db, [heat], Work::Write, Duration::ZERO).unwrap();
    let late = folder.join("late.nfo");
    let writer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        fs::create_dir_all(late.parent().unwrap()).unwrap();
        fs::write(&late, "written by the NFO writer").unwrap();
        drop(writing);
    });
    std::thread::sleep(Duration::from_millis(50));
    let report = lab.removal(&trash).erase(&[heat], "DELETE").unwrap();
    writer.join().unwrap();

    assert_eq!(report.erased.len(), 1, "{report:?}");
    assert!(
        !folder.exists(),
        "nothing is left behind or written back: {:?}",
        fs::read_dir(&folder).map(|d| d.flatten().map(|e| e.file_name()).collect::<Vec<_>>())
    );
    assert!(!lab.exists(heat));
}

#[test]
fn a_title_whose_scrape_steps_still_write_is_not_deleted() {
    let lab = Lab::new();
    let trash = FakeTrash::new(lab.dir.path());
    let heat = lab.movie(&lab.movies, "Heat (1995)");
    let folder = Path::new(&lab.movies.path).join("Heat (1995)");
    let pinned = title_guard::pin(&lab.db, [heat]);

    let report = lab.removal(&trash).erase(&[heat], "DELETE").unwrap();
    assert!(
        report.erased.is_empty() && report.history_id.is_none(),
        "{report:?}"
    );
    assert_eq!(report.problems.len(), 1, "{report:?}");
    assert!(report.problems[0].reason.contains("Nothing was deleted"));

    let report = lab.removal(&trash).delete_with_files(&[heat]).unwrap();
    assert!(
        report.deleted.is_empty() && report.history_id.is_none(),
        "{report:?}"
    );
    assert!(folder.join("Heat (1995).mkv").is_file());
    assert!(lab.exists(heat) && !lab.hidden(heat));
    drop(pinned);

    let report = lab.removal(&trash).delete_with_files(&[heat]).unwrap();
    assert_eq!(report.deleted.len(), 1, "{report:?}");
}
