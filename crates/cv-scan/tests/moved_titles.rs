//! Titles moved or renamed outside the app: the stored row follows its files when the
//! match is certain, and nothing is guessed when it is not. No row is ever deleted.

mod support;

use cv_domain::{ItemKind, MediaFileType};
use support::Lib;

fn movie_by_title(lib: &Lib, title: &str) -> cv_domain::Item {
    lib.items(ItemKind::Movie)
        .into_iter()
        .find(|m| m.core.title == title)
        .unwrap_or_else(|| panic!("no movie {title}"))
}

fn edit_plot(lib: &Lib, id: cv_domain::ItemId, plot: &'static str) {
    lib.db
        .write(move |tx| {
            let mut item = cv_db::repo::get_item(tx, id)?.unwrap();
            item.core.plot = plot.into();
            cv_db::repo::update_item(tx, &item)?;
            cv_db::repo::set_new(tx, &[id], false)?;
            Ok(())
        })
        .unwrap();
}

#[test]
fn a_renamed_folder_keeps_its_row_with_metadata_and_artwork() {
    let lib = Lib::movies();
    lib.put("Parasite (2019)/Parasite (2019).mkv", "parasite-video");
    lib.put("Parasite (2019)/Parasite (2019)-poster.jpg", "poster");
    lib.put("Heat (1995)/Heat (1995).mkv", "heat-video");
    lib.rescan();
    lib.read_media_info();
    let id = movie_by_title(&lib, "Parasite").id.unwrap();
    edit_plot(&lib, id, "Class war.");
    let count = lib.item_count();

    std::fs::rename(
        lib.root.join("Parasite (2019)"),
        lib.root.join("Parasite moved folder"),
    )
    .unwrap();
    let report = lib.rescan();

    assert!(report.new_items.is_empty(), "{report:?}");
    assert_eq!(report.stats.relinked, 1);
    assert_eq!(report.relinked.len(), 1);
    assert_eq!(report.relinked[0].id, id);
    assert_eq!(lib.item_count(), count, "no row added, none deleted");
    let item = lib.item(id);
    assert_eq!(item.core.plot, "Class war.");
    assert!(item.core.path.ends_with("Parasite moved folder"));
    assert_eq!(lib.availability(id), "online");
    assert!(report.availability.became_missing.is_empty());
    let video = item
        .core
        .media_files
        .iter()
        .find(|f| f.kind == MediaFileType::Video)
        .unwrap();
    assert!(video.directory.ends_with("Parasite moved folder"));
    assert!(video.has_media_info(), "media info moved with the file");
    assert!(
        item.core
            .media_files
            .iter()
            .any(|f| f.kind == MediaFileType::Poster
                && f.directory.ends_with("Parasite moved folder")),
        "the poster is found at the new place"
    );
    // The next scan has nothing to do.
    assert!(lib.rescan().is_unchanged());
}

#[test]
fn a_moved_and_renamed_file_keeps_its_row_by_title_and_size() {
    let lib = Lib::movies();
    lib.put(
        "Alien (1979)/Alien.1979.480p.DVDRip.avi",
        "the same bytes of alien",
    );
    lib.put("Heat (1995)/Heat (1995).mkv", "heat-video");
    lib.rescan();
    let id = movie_by_title(&lib, "Alien").id.unwrap();
    edit_plot(&lib, id, "In space...");
    let count = lib.item_count();

    std::fs::create_dir_all(lib.root.join("Sci-Fi/Alien (1979)")).unwrap();
    std::fs::rename(
        lib.root.join("Alien (1979)/Alien.1979.480p.DVDRip.avi"),
        lib.root.join("Sci-Fi/Alien (1979)/Alien (1979).avi"),
    )
    .unwrap();
    std::fs::remove_dir(lib.root.join("Alien (1979)")).unwrap();
    let report = lib.rescan();

    assert!(report.new_items.is_empty(), "{report:?}");
    assert_eq!(report.relinked.len(), 1);
    assert_eq!(lib.item_count(), count);
    let item = lib.item(id);
    assert_eq!(item.core.plot, "In space...");
    assert!(item.core.path.ends_with("Alien (1979)"));
    assert!(item.core.path.contains("Sci-Fi"));
    assert_eq!(lib.availability(id), "online");
}

#[test]
fn two_missing_titles_that_fit_make_the_new_folder_a_separate_row() {
    let lib = Lib::movies();
    // Two rows with identical bytes in different folders, both then moved away.
    lib.put("Heat (1995)/Heat (1995).mkv", "identical bytes");
    lib.put("Heat (1995) copy/Heat (1995).mkv", "identical bytes");
    lib.rescan();
    let count = lib.item_count();
    assert_eq!(count, 2);
    std::fs::create_dir_all(lib.root.join("Moved")).unwrap();
    std::fs::rename(
        lib.root.join("Heat (1995)"),
        lib.root.join("Moved/Heat (1995)"),
    )
    .unwrap();
    std::fs::remove_dir_all(lib.root.join("Heat (1995) copy")).unwrap();
    let report = lib.rescan();

    assert!(report.relinked.is_empty(), "{report:?}");
    assert_eq!(report.new_items.len(), 1, "the new folder gets its own row");
    assert_eq!(lib.item_count(), count + 1, "nothing was deleted");
    assert_eq!(report.availability.became_missing.len(), 2);
}

#[test]
fn a_new_folder_with_other_bytes_is_a_new_title() {
    let lib = Lib::movies();
    lib.put("Heat (1995)/Heat (1995).mkv", "heat-video");
    lib.put("Alien (1979)/Alien (1979).mkv", "alien-video");
    lib.rescan();
    let heat = movie_by_title(&lib, "Heat").id.unwrap();

    std::fs::remove_dir_all(lib.root.join("Heat (1995)")).unwrap();
    lib.put(
        "Heat Remake (1995)/Heat (1995).mkv",
        "a different and longer file",
    );
    let report = lib.rescan();

    assert!(report.relinked.is_empty());
    assert_eq!(report.new_items.len(), 1);
    assert_eq!(lib.availability(heat), "missing");
    assert_eq!(lib.items(ItemKind::Movie).len(), 3);
}

// ---- shows -------------------------------------------------------------------------------------

fn show_by_title(lib: &Lib, title: &str) -> cv_domain::Item {
    lib.items(ItemKind::Show)
        .into_iter()
        .find(|s| s.core.title == title)
        .unwrap_or_else(|| panic!("no show {title}"))
}

fn tv_lib() -> Lib {
    let lib = Lib::tv();
    lib.put("Severance (2022)/tvshow.nfo", "<tvshow/>");
    lib.put("Severance (2022)/poster.jpg", "poster");
    lib.put(
        "Severance (2022)/Season 01/Severance.S01E01.mkv",
        "episode one bytes",
    );
    lib.put(
        "Severance (2022)/Season 01/Severance.S01E02.mkv",
        "episode two bytes!",
    );
    lib.put(
        "Severance (2022)/Season 02/Severance.S02E01.mkv",
        "episode three bytes!!",
    );
    lib.put("Dark (2017)/Season 01/Dark.S01E01.mkv", "dark one");
    lib
}

#[test]
fn a_renamed_show_folder_keeps_its_show_seasons_and_episodes() {
    let lib = tv_lib();
    lib.rescan();
    let id = show_by_title(&lib, "Severance").id.unwrap();
    edit_plot(&lib, id, "Work and life, split.");
    let count = lib.item_count();
    let episodes = lib.items(ItemKind::Episode).len();

    std::fs::rename(
        lib.root.join("Severance (2022)"),
        lib.root.join("Severance moved folder"),
    )
    .unwrap();
    let report = lib.rescan();

    assert!(report.new_items.is_empty(), "{report:?}");
    assert_eq!(report.stats.relinked, 1);
    assert_eq!(report.relinked.len(), 1);
    assert_eq!(report.relinked[0].id, id);
    assert_eq!(lib.item_count(), count, "no row added, none deleted");
    assert_eq!(lib.items(ItemKind::Episode).len(), episodes);
    let show = lib.item(id);
    assert_eq!(show.core.plot, "Work and life, split.");
    assert!(show.core.path.ends_with("Severance moved folder"));
    assert_eq!(lib.availability(id), "online");
    assert!(report.availability.became_missing.is_empty(), "{report:?}");
    for episode in lib.items(ItemKind::Episode) {
        if episode.core.path.contains("Severance") {
            assert!(episode.core.path.contains("Severance moved folder"));
            assert_eq!(lib.availability(episode.id.unwrap()), "online");
        }
    }
    assert!(
        show.core
            .media_files
            .iter()
            .any(|f| f.kind == MediaFileType::Poster
                && f.directory.ends_with("Severance moved folder")),
        "the poster is found at the new place"
    );
    // The next scan has nothing to do.
    assert!(lib.rescan().is_unchanged());
}

#[test]
fn a_show_folder_with_other_episode_bytes_is_a_new_show() {
    let lib = tv_lib();
    lib.rescan();
    let id = show_by_title(&lib, "Dark").id.unwrap();

    std::fs::remove_dir_all(lib.root.join("Dark (2017)")).unwrap();
    lib.put(
        "Dark Remake (2017)/Season 01/Dark.S01E01.mkv",
        "a different and much longer file",
    );
    let report = lib.rescan();

    assert!(report.relinked.is_empty(), "{report:?}");
    assert_eq!(lib.availability(id), "missing");
    assert_eq!(lib.items(ItemKind::Show).len(), 3);
}

#[test]
fn two_missing_shows_that_fit_make_the_new_folder_a_separate_show() {
    let lib = Lib::tv();
    lib.put("Twin A (2020)/Season 01/Twin.S01E01.mkv", "identical bytes");
    lib.put("Twin B (2020)/Season 01/Twin.S01E01.mkv", "identical bytes");
    lib.rescan();
    let count = lib.item_count();
    std::fs::create_dir_all(lib.root.join("Moved")).unwrap();
    std::fs::rename(
        lib.root.join("Twin A (2020)"),
        lib.root.join("Moved/Twin (2020)"),
    )
    .unwrap();
    std::fs::remove_dir_all(lib.root.join("Twin B (2020)")).unwrap();
    let report = lib.rescan();

    assert!(report.relinked.is_empty(), "{report:?}");
    assert!(lib.item_count() > count, "the new folder gets its own rows");
    assert_eq!(lib.items(ItemKind::Show).len(), 3, "nothing was deleted");
}
