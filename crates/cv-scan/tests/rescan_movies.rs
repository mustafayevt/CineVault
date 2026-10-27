//! Incremental rescans of movie data sources: add, change, replace, remove.

mod support;

use cv_db::repo::data_sources::{list_sources, remap_source, remove_source};
use cv_db::repo::fingerprint::{count_fingerprints, delete_fingerprints_of_source};
use cv_domain::{ItemDetail, ItemKind, MediaFileType};
use cv_scan::rescan::{Skipped, refresh_detached, rescan_all};
use cv_scan::walk::{WalkOptions, scan_movie_source};
use support::{Lib, set_mtime};

fn video(item: &cv_domain::Item) -> &cv_domain::MediaFile {
    item.core
        .media_files
        .iter()
        .find(|f| f.kind == MediaFileType::Video)
        .expect("a video")
}

fn two_movies() -> Lib {
    let lib = Lib::movies();
    lib.put("Alien (1979)/Alien (1979).mkv", "alien-video");
    lib.put("Alien (1979)/Alien (1979)-poster.jpg", "poster");
    lib.put("Heat (1995)/Heat (1995).mkv", "heat-video");
    lib
}

#[test]
fn first_scan_adds_movies_flagged_new_with_their_files_and_fingerprints() {
    let lib = two_movies();
    let report = lib.rescan();

    assert_eq!(report.new_items.len(), 2);
    assert_eq!(report.stats.new_units, 2);
    assert_eq!(report.stats.files_added, 3);
    assert!(report.wrote);
    // Every new movie has a video whose media info has not been read.
    assert_eq!(report.needs_media_info.len(), 2);

    let movies = lib.items(ItemKind::Movie);
    assert_eq!(movies.len(), 2);
    let alien = movies.iter().find(|m| m.core.title == "Alien").unwrap();
    assert_eq!(alien.core.year, 1979);
    assert_eq!(alien.core.media_files.len(), 2);
    assert_eq!(alien.core.data_source, lib.source.path);
    assert_eq!(alien.core.date_added_ms, 1_700_000_000_000);
    assert!(lib.is_new(alien.id.unwrap()));
    assert_eq!(lib.count("fingerprint"), 3);
}

#[test]
fn unchanged_rescan_modifies_nothing_and_keeps_media_info() {
    let lib = two_movies();
    lib.rescan();
    lib.read_media_info();
    let before = lib.content_hash();

    let report = lib.rescan();

    assert!(report.is_unchanged(), "{report:?}");
    assert!(!report.wrote);
    assert!(report.needs_media_info.is_empty());
    assert_eq!(report.stats.unchanged, 2);
    assert_eq!(lib.content_hash(), before);
    for movie in lib.items(ItemKind::Movie) {
        assert!(video(&movie).has_media_info());
    }
}

#[test]
fn a_video_replaced_by_a_better_version_keeps_the_movie_and_rereads_the_media_info() {
    let lib = two_movies();
    lib.rescan();
    lib.read_media_info();
    // The user's own edits must survive.
    let alien_id = lib
        .items(ItemKind::Movie)
        .into_iter()
        .find(|m| m.core.title == "Alien")
        .and_then(|m| m.id)
        .unwrap();
    lib.db
        .write(move |tx| {
            let mut item = cv_db::repo::get_item(tx, alien_id)?.unwrap();
            item.core.plot = "In space...".into();
            cv_db::repo::update_item(tx, &item)?;
            cv_db::repo::set_new(tx, &[alien_id], false)?;
            Ok(())
        })
        .unwrap();

    lib.remove("Alien (1979)/Alien (1979).mkv");
    lib.put(
        "Alien (1979)/Alien.1979.2160p.UHD.BluRay.mkv",
        "a much larger 2160p video",
    );
    let report = lib.rescan();

    assert!(report.new_items.is_empty(), "no second Alien: {report:?}");
    assert_eq!(report.updated, vec![alien_id]);
    assert_eq!(report.needs_media_info, vec![alien_id]);
    assert_eq!(report.stats.replaced, 1);
    assert_eq!(lib.items(ItemKind::Movie).len(), 2);

    let alien = lib.item(alien_id);
    assert_eq!(alien.core.plot, "In space...");
    assert_eq!(video(&alien).filename, "Alien.1979.2160p.UHD.BluRay.mkv");
    assert!(!video(&alien).has_media_info());
    assert!(lib.is_new(alien_id), "flagged as updated");
    // The other movie was not touched.
    let heat = lib
        .items(ItemKind::Movie)
        .into_iter()
        .find(|m| m.core.title == "Heat")
        .unwrap();
    assert!(video(&heat).has_media_info());
}

#[test]
fn a_video_edited_in_place_is_a_change_and_loses_its_media_info() {
    let lib = two_movies();
    lib.rescan();
    lib.read_media_info();
    let path = lib.put(
        "Heat (1995)/Heat (1995).mkv",
        "heat video, remuxed and longer",
    );
    set_mtime(&path, 1_600_000_000);

    let report = lib.rescan();

    assert_eq!(report.stats.changed, 1);
    assert_eq!(report.stats.files_changed, 1);
    assert_eq!(report.stats.replaced, 0);
    assert_eq!(report.updated.len(), 1);
    let heat = lib
        .items(ItemKind::Movie)
        .into_iter()
        .find(|m| m.core.title == "Heat")
        .unwrap();
    assert!(!video(&heat).has_media_info());
    assert_eq!(video(&heat).modified_ms, 1_600_000_000_000);
}

#[test]
fn a_changed_poster_updates_the_files_without_touching_the_video_or_the_flag() {
    let lib = two_movies();
    lib.rescan();
    lib.read_media_info();
    let alien_id = lib.item_ids(ItemKind::Movie)[0];
    lib.db
        .write(move |tx| cv_db::repo::set_new(tx, &[alien_id], false))
        .unwrap();

    lib.put(
        "Alien (1979)/Alien (1979)-poster.jpg",
        "a new, larger poster",
    );
    let report = lib.rescan();

    assert_eq!(report.stats.files_changed, 1);
    assert!(report.updated.is_empty());
    assert!(report.needs_media_info.is_empty());
    let alien = lib.item(alien_id);
    assert!(video(&alien).has_media_info());
    let poster = alien
        .core
        .media_files
        .iter()
        .find(|f| f.kind == MediaFileType::Poster)
        .unwrap();
    assert_eq!(poster.size_bytes, "a new, larger poster".len() as u64);
    assert!(!lib.is_new(alien_id));
}

#[test]
fn a_deleted_companion_file_leaves_the_list_and_its_fingerprint() {
    let lib = two_movies();
    lib.rescan();
    assert_eq!(lib.count("fingerprint"), 3);

    lib.remove("Alien (1979)/Alien (1979)-poster.jpg");
    let report = lib.rescan();

    assert_eq!(report.stats.files_removed, 1);
    assert_eq!(lib.count("fingerprint"), 2);
    let alien = lib
        .items(ItemKind::Movie)
        .into_iter()
        .find(|m| m.core.title == "Alien")
        .unwrap();
    assert_eq!(alien.core.media_files.len(), 1);
}

#[test]
fn a_deleted_movie_folder_becomes_missing_and_is_never_deleted() {
    let lib = two_movies();
    lib.rescan();
    let ids = lib.item_ids(ItemKind::Movie);
    let count = lib.item_count();

    lib.remove("Heat (1995)");
    let report = lib.rescan();

    assert_eq!(lib.item_count(), count, "no row is deleted");
    let heat = lib
        .items(ItemKind::Movie)
        .into_iter()
        .find(|m| m.core.title == "Heat")
        .unwrap();
    assert_eq!(report.availability.became_missing, vec![heat.id.unwrap()]);
    assert_eq!(lib.availability(heat.id.unwrap()), "missing");
    // The "Missing files" filter lists exactly that movie.
    let listed = lib
        .db
        .read(|conn| {
            let filter = cv_db::repo::Filter::Availability {
                values: vec!["missing".into()],
            };
            let query = cv_db::repo::ItemQuery::new(ItemKind::Movie).filter(filter);
            cv_db::repo::count_items(conn, &query)
        })
        .unwrap();
    assert_eq!(listed, 1);
    let alien = ids.iter().find(|id| **id != heat.id.unwrap()).unwrap();
    assert_eq!(lib.availability(*alien), "online");

    // Rescanning again changes nothing more.
    let again = lib.rescan();
    assert!(again.is_unchanged());

    // The folder comes back (restored from a backup): the same item is online again.
    lib.put("Heat (1995)/Heat (1995).mkv", "heat-video");
    let back = lib.rescan();
    assert_eq!(back.availability.restored, vec![heat.id.unwrap()]);
    assert_eq!(lib.availability(heat.id.unwrap()), "online");
    assert_eq!(lib.item_count(), count, "no duplicate was created");
}

#[test]
fn a_removed_video_with_a_surviving_folder_is_missing_too() {
    let lib = two_movies();
    lib.rescan();

    lib.remove("Alien (1979)/Alien (1979).mkv");
    let report = lib.rescan();

    // The poster is all that is left, so the walk finds no movie there.
    assert_eq!(report.availability.became_missing.len(), 1);
    assert_eq!(lib.count("item"), 2);
}

#[test]
fn an_unreachable_source_marks_items_offline_and_compares_nothing() {
    let lib = two_movies();
    lib.rescan();
    let hash_files = lib.count("media_file");
    let gone = lib.dir.path().join("library-away");
    std::fs::rename(&lib.root, &gone).unwrap();

    let report = lib.rescan();

    assert_eq!(report.skipped, Some(Skipped::SourceUnavailable));
    assert_eq!(report.availability.became_offline.len(), 2);
    assert_eq!(lib.count("media_file"), hash_files);
    assert_eq!(lib.count("fingerprint"), 3, "fingerprints are kept");
    for id in lib.item_ids(ItemKind::Movie) {
        assert_eq!(lib.availability(id), "offline");
    }

    // Plugged back in: everything is online again, nothing was re-read.
    std::fs::rename(&gone, &lib.root).unwrap();
    lib.read_media_info();
    let report = lib.rescan();
    assert_eq!(report.availability.restored.len(), 2);
    assert!(report.needs_media_info.is_empty());
}

#[test]
fn an_empty_source_is_not_trusted_and_nothing_goes_missing() {
    let lib = two_movies();
    lib.rescan();
    std::fs::remove_dir_all(lib.root.join("Alien (1979)")).unwrap();
    std::fs::remove_dir_all(lib.root.join("Heat (1995)")).unwrap();

    let report = lib.rescan();

    assert_eq!(report.skipped, Some(Skipped::SourceEmpty));
    assert!(report.availability.became_missing.is_empty());
    for id in lib.item_ids(ItemKind::Movie) {
        assert_eq!(lib.availability(id), "online");
    }
    assert_eq!(lib.count("fingerprint"), 3);
}

#[test]
fn an_emptied_fingerprint_table_is_rebuilt_without_changing_any_item() {
    let lib = two_movies();
    lib.rescan();
    lib.read_media_info();
    let path = lib.source.path.clone();
    lib.db
        .write(move |tx| delete_fingerprints_of_source(tx, &path))
        .unwrap();
    assert_eq!(lib.count("fingerprint"), 0);
    let items_before: Vec<_> = lib.items(ItemKind::Movie);

    let report = lib.rescan();

    assert_eq!(report.stats.fingerprints_seeded, 3);
    assert!(report.new_items.is_empty());
    assert!(report.updated.is_empty());
    assert!(report.needs_media_info.is_empty());
    assert_eq!(lib.items(ItemKind::Movie), items_before);
    assert_eq!(lib.count("fingerprint"), 3);
    // And now the table is complete, the next rescan writes nothing.
    assert!(lib.rescan().is_unchanged());
}

#[test]
fn a_new_movie_in_a_multi_movie_folder_adds_only_that_movie() {
    let lib = Lib::movies();
    lib.put("Films/Alien (1979).mkv", "a");
    lib.put("Films/Aliens (1986).mkv", "b");
    lib.rescan();
    assert_eq!(lib.items(ItemKind::Movie).len(), 2);

    lib.put("Films/Alien 3 (1992).mkv", "c");
    let report = lib.rescan();

    assert_eq!(report.new_items.len(), 1);
    assert_eq!(report.stats.unchanged, 2);
    assert_eq!(lib.items(ItemKind::Movie).len(), 3);
}

#[test]
fn a_movie_renamed_in_place_keeps_its_item() {
    let lib = Lib::movies();
    lib.put("Films/alien.mkv", "alien");
    lib.put("Films/aliens.mkv", "aliens");
    lib.rescan();
    let before = lib.item_ids(ItemKind::Movie);

    // Same bytes and time, new name: the organiser or the user renamed the file.
    let old = lib.root.join("Films/alien.mkv");
    set_mtime(&old, 1_500_000_000);
    lib.rescan();
    std::fs::rename(&old, lib.root.join("Films/Alien (1979).mkv")).unwrap();
    let report = lib.rescan();

    assert!(report.new_items.is_empty(), "{report:?}");
    assert_eq!(lib.item_ids(ItemKind::Movie), before);
}

#[test]
fn a_hidden_by_marker_folder_is_not_added() {
    let lib = two_movies();
    lib.put("Skipped (2001)/Skipped (2001).mkv", "x");
    lib.put("Skipped (2001)/.cinevaultignore", "");
    let report = lib.rescan();
    assert_eq!(report.new_items.len(), 2);
}

#[test]
fn a_cancelled_rescan_writes_nothing() {
    let lib = two_movies();
    let before = lib.content_hash();
    let result =
        cv_scan::rescan::rescan_source_with(&lib.db, &lib.source, &lib.options(), &|| true);
    assert!(matches!(
        result,
        Err(cv_scan::rescan::RescanError::Cancelled)
    ));
    assert_eq!(lib.content_hash(), before);
}

#[test]
fn a_remap_keeps_the_tree_unchanged_when_the_folder_moved_with_its_times() {
    let lib = two_movies();
    lib.rescan();
    lib.read_media_info();

    let moved = lib.dir.path().join("nas-movies");
    std::fs::rename(&lib.root, &moved).unwrap();
    let new_path = moved.to_string_lossy().into_owned();
    let id = lib.source.id;
    let report = lib
        .db
        .write({
            let new_path = new_path.clone();
            move |tx| remap_source(tx, id, &new_path)
        })
        .unwrap()
        .unwrap();
    assert_eq!(report.fingerprints, 3);
    let source = lib
        .db
        .read(|conn| Ok(list_sources(conn, None)?.remove(0)))
        .unwrap();
    assert_eq!(source.path, new_path);

    let rescan = cv_scan::rescan::rescan_source(&lib.db, &source, &lib.options()).unwrap();

    assert!(rescan.is_unchanged(), "{rescan:?}");
    assert!(rescan.needs_media_info.is_empty());
}

#[test]
fn a_remap_to_a_missing_folder_refreshes_availability_on_the_next_rescan() {
    let lib = two_movies();
    lib.rescan();
    let id = lib.source.id;
    let elsewhere = lib
        .dir
        .path()
        .join("not-there")
        .to_string_lossy()
        .into_owned();
    lib.db
        .write(move |tx| remap_source(tx, id, &elsewhere))
        .unwrap();
    let source = lib
        .db
        .read(|conn| Ok(list_sources(conn, None)?.remove(0)))
        .unwrap();

    let report = cv_scan::rescan::rescan_source(&lib.db, &source, &lib.options()).unwrap();

    assert_eq!(report.skipped, Some(Skipped::SourceUnavailable));
    assert_eq!(report.availability.became_offline.len(), 2);
}

#[test]
fn items_of_a_removed_source_are_refreshed_as_offline_and_kept() {
    let lib = two_movies();
    lib.rescan();
    let id = lib.source.id;
    lib.db.write(move |tx| remove_source(tx, id)).unwrap();
    assert_eq!(lib.count("fingerprint"), 0);
    std::fs::remove_dir_all(&lib.root).unwrap();

    let outcome = refresh_detached(&lib.db).unwrap();

    assert_eq!(lib.item_count(), 2);
    assert_eq!(
        outcome.became_missing.len() + outcome.became_offline.len(),
        2
    );
    assert!(rescan_all(&lib.db, &lib.options()).unwrap().is_empty());
}

#[test]
fn rescan_all_runs_every_source() {
    let lib = two_movies();
    let reports = rescan_all(&lib.db, &lib.options()).unwrap();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].new_items.len(), 2);
    assert_eq!(count_fingerprints_of(&lib), 3);
}

fn count_fingerprints_of(lib: &Lib) -> u64 {
    lib.db.read(count_fingerprints).unwrap()
}

#[test]
fn the_walk_gives_the_same_groups_on_one_thread_and_on_many() {
    let lib = Lib::movies();
    for n in 0..60 {
        lib.put(
            &format!("Movie {n:02} (20{n:02})/Movie {n:02} (20{n:02}).mkv"),
            "v",
        );
        lib.put(&format!("Movie {n:02} (20{n:02})/poster.jpg"), "p");
    }
    lib.put("A/Alpha (1999).mkv", "a");
    lib.put("A/Apex (2000).mkv", "a");

    let one = scan_movie_source(
        &lib.root,
        None,
        &WalkOptions::with_defaults().with_threads(1),
    );
    let many = scan_movie_source(
        &lib.root,
        None,
        &WalkOptions::with_defaults().with_threads(8),
    );

    assert_eq!(one.movies.len(), 62);
    assert_eq!(one, many);
}

#[test]
fn movie_details_follow_the_names() {
    let lib = Lib::movies();
    lib.put(
        "Dune Part Two (2024) {tmdb-693134}/Dune Part Two (2024).cd1.mkv",
        "1",
    );
    lib.put(
        "Dune Part Two (2024) {tmdb-693134}/Dune Part Two (2024).cd2.mkv",
        "2",
    );
    lib.rescan();

    let dune = &lib.items(ItemKind::Movie)[0];
    assert_eq!(dune.core.ids.get("tmdb"), Some("693134"));
    let ItemDetail::Movie(movie) = &dune.detail else {
        panic!("a movie");
    };
    assert!(movie.stacked);
    assert!(!movie.multi_movie_dir);
    assert_eq!(dune.core.media_files.len(), 2);
}

#[test]
fn a_tv_folder_inside_a_movie_folder_is_left_to_the_tv_library() {
    let lib = Lib::movies();
    lib.put("Arrival (2016).mkv", "arrival");
    lib.put("Chernobyl/Season 1/Chernobyl S01E01.mkv", "e1");
    lib.put("Chernobyl/Season 1/Chernobyl S01E02.mkv", "e2");
    let shows = lib.root.join("Chernobyl").to_string_lossy().into_owned();
    let tv = lib
        .db
        .write(move |tx| {
            cv_db::repo::data_sources::insert_source(tx, cv_domain::SourceKind::Tv, &shows)
        })
        .unwrap();

    lib.rescan();
    let movies = lib.items(ItemKind::Movie);
    assert_eq!(
        movies
            .iter()
            .map(|m| m.core.title.as_str())
            .collect::<Vec<_>>(),
        ["Arrival"],
        "the episodes must not become movies"
    );

    // The TV library still finds its show and episodes.
    cv_scan::rescan::rescan_source(&lib.db, &tv, &lib.options()).unwrap();
    let shows = lib.items(ItemKind::Show);
    assert_eq!(shows.len(), 1);
    // The TV folder is the show folder itself, so its season folder is not a show.
    assert_eq!(shows[0].core.title, "Chernobyl");
    assert_eq!(lib.items(ItemKind::Season).len(), 1);
    assert_eq!(lib.items(ItemKind::Episode).len(), 2);
    assert_eq!(lib.items(ItemKind::Movie).len(), 1);
}

#[test]
fn movies_scanned_before_a_tv_folder_was_nested_become_missing() {
    let lib = Lib::movies();
    lib.put("Arrival (2016).mkv", "arrival");
    lib.put("Chernobyl/Season 1/Chernobyl S01E01.mkv", "e1");
    lib.put("Chernobyl/Season 1/Chernobyl S01E02.mkv", "e2");
    lib.rescan();
    assert_eq!(lib.items(ItemKind::Movie).len(), 3);

    let shows = lib.root.join("Chernobyl").to_string_lossy().into_owned();
    lib.db
        .write(move |tx| {
            cv_db::repo::data_sources::insert_source(tx, cv_domain::SourceKind::Tv, &shows)
        })
        .unwrap();
    lib.rescan();
    let movies = lib.items(ItemKind::Movie);
    let mut states: Vec<(String, String)> = movies
        .iter()
        .map(|m| (m.core.title.clone(), lib.availability(m.id.unwrap())))
        .collect();
    states.sort();
    // The rows stay (nothing is deleted), but the episodes are no longer online movies.
    assert_eq!(
        states,
        [
            ("Arrival".to_owned(), "online".to_owned()),
            ("Chernobyl S01E01".to_owned(), "missing".to_owned()),
            ("Chernobyl S01E02".to_owned(), "missing".to_owned()),
        ]
    );
}

#[test]
fn a_title_gets_its_added_date_and_its_video_the_files_modified_time_and_keeps_them() {
    let lib = Lib::movies();
    let video_path = lib.put("Heat (1995)/Heat (1995).mkv", "heat-video");
    set_mtime(&video_path, 1_500_000_000);
    lib.put("Alien (1979)/Alien (1979).mkv", "alien-video");

    lib.rescan();

    // Never empty: the first scan stamps every new title with the time it saw it, and the sort
    // columns read the main video's modified time from the file system.
    let movies = lib.items(ItemKind::Movie);
    assert!(
        movies
            .iter()
            .all(|m| m.core.date_added_ms == 1_700_000_000_000)
    );
    let heat = movies.iter().find(|m| m.core.title == "Heat").unwrap();
    assert_eq!(video(heat).modified_ms, 1_500_000_000_000);

    // A later scan that sees a newer title does not move the first one's date.
    lib.put("Dune (2021)/Dune (2021).mkv", "dune-video");
    let later = cv_scan::rescan::RescanOptions {
        now_ms: Some(1_800_000_000_000),
        ..Default::default()
    };
    cv_scan::rescan::rescan_source(&lib.db, &lib.source, &later).expect("rescan");
    let movies = lib.items(ItemKind::Movie);
    let added = |title: &str| {
        movies
            .iter()
            .find(|m| m.core.title == title)
            .unwrap()
            .core
            .date_added_ms
    };
    assert_eq!(added("Heat"), 1_700_000_000_000);
    // Dune's date is when its file was created, no later than the scan's clock.
    assert!(added("Dune") > added("Heat") && added("Dune") <= 1_800_000_000_000);
}

#[test]
fn a_drive_that_is_not_mounted_is_not_read_even_with_a_movie_in_its_folder() {
    let lib = two_movies();
    lib.rescan();
    let hash_files = lib.count("media_file");
    // The drive is unplugged, yet its mount-point folder still holds a movie of its own.
    lib.put("Stray (2001)/Stray (2001).mkv", "x");

    let mut options = lib.options();
    options.movies.drive_is_mounted = |_| false;
    let report = cv_scan::rescan::rescan_source(&lib.db, &lib.source, &options).expect("rescan");

    assert_eq!(report.skipped, Some(Skipped::SourceUnavailable));
    assert!(
        report.new_items.is_empty(),
        "the stray movie is not a title"
    );
    assert!(report.availability.became_missing.is_empty());
    assert_eq!(lib.count("item"), 2);
    assert_eq!(lib.count("media_file"), hash_files);
    assert_eq!(lib.count("fingerprint"), 3, "fingerprints are kept");
}
