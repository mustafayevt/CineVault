//! A delete takes the files named after a title that has no folder of its own (spec
//! `title-deletion` "Delete takes the files named after the title"), and never a file of another
//! title ("Deleting never touches other titles").

use super::*;

impl Lab {
    /// A movie whose only recorded file is `video`, in `folder` (inside `source`), written to disk.
    fn movie_in(&self, source: &DataSource, folder: &Path, title: &str, video: &str) -> ItemId {
        self.movie_with(source, folder, title, &[video])
    }

    /// A movie that records `files` in `folder`, all written to disk (10 bytes each).
    fn movie_with(
        &self,
        source: &DataSource,
        folder: &Path,
        title: &str,
        files: &[&str],
    ) -> ItemId {
        let mut item = Item::movie(title);
        item.core.data_source.clone_from(&source.path);
        item.core.path = folder.to_string_lossy().into_owned();
        for file in files {
            Self::put(folder, file, "0123456789");
            let kind = if file.ends_with(".mkv") || file.ends_with(".avi") {
                MediaFileType::Video
            } else {
                MediaFileType::Subtitle
            };
            let mut row = MediaFile::new(kind, folder.to_string_lossy(), *file);
            row.size_bytes = 10;
            item.core.media_files.push(row);
        }
        self.insert(item)
    }
}

/// Writes files the library does not know (5 bytes each).
fn drop_files(folder: &Path, names: &[&str]) {
    for name in names {
        Lab::put(folder, name, "extra");
    }
}

/// The names of the files directly in `folder`, sorted.
fn left_in(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(folder)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

const SIDECARS: &[&str] = &[
    "Example Movie (2020).en.srt",
    "Example Movie (2020).en.forced.srt",
    "Example Movie (2020).de.sdh.srt",
    "Example Movie (2020).ass",
    "Example Movie (2020).sup",
    "Example Movie (2020).idx",
    "Example Movie (2020).sub",
    "Example Movie (2020).nfo",
    "Example Movie (2020).jpg",
    "Example Movie (2020)-poster.jpg",
    "Example Movie (2020)-fanart.jpg",
    "Example Movie (2020)-clearlogo.png",
    "Example Movie (2020)-trailer.mkv",
    "Example Movie (2020).chapters.xml",
];

#[test]
fn a_shared_folder_loses_the_files_named_after_the_title_and_undo_brings_them_back() {
    let lab = Lab::new();
    let trash = FakeTrash::new(lab.dir.path());
    let folder = Path::new(&lab.movies.path).join("Mixed");
    let movie = lab.movie_in(
        &lab.movies,
        &folder,
        "Example Movie",
        "Example Movie (2020).mkv",
    );
    let other = lab.movie_in(
        &lab.movies,
        &folder,
        "Other Movie",
        "Other Movie (2019).mkv",
    );
    drop_files(&folder, SIDECARS);
    drop_files(
        &folder,
        &["Other Movie (2019).en.srt", "poster.jpg", "folder.jpg"],
    );
    // A sub-folder of a shared folder belongs to no title by name.
    drop_files(&folder.join("Subs"), &["Example Movie (2020).en.srt"]);

    let report = lab.removal(&trash).delete_with_files(&[movie]).unwrap();
    assert!(report.problems.is_empty(), "{report:?}");
    assert_eq!(
        report.deleted[0].paths as usize,
        1 + SIDECARS.len(),
        "{report:?}"
    );
    assert_eq!(
        left_in(&folder),
        [
            "Other Movie (2019).en.srt",
            "Other Movie (2019).mkv",
            "Subs",
            "folder.jpg",
            "poster.jpg"
        ]
    );
    assert!(folder.join("Subs/Example Movie (2020).en.srt").is_file());
    assert!(lab.hidden(movie) && !lab.hidden(other));

    let outcome = lab
        .history(&trash)
        .undo(report.history_id.unwrap())
        .unwrap();
    assert_eq!(outcome.undone, [movie], "{outcome:?}");
    for name in SIDECARS.iter().chain(&["Example Movie (2020).mkv"]) {
        assert!(folder.join(name).is_file(), "{name} is back");
    }
    assert!(!lab.hidden(movie));
}

#[test]
fn a_longer_name_of_another_title_keeps_its_files() {
    let lab = Lab::new();
    let trash = FakeTrash::new(lab.dir.path());
    let folder = Path::new(&lab.movies.path).join("Mixed");
    let alien = lab.movie_in(&lab.movies, &folder, "Alien", "Alien (1979).mkv");
    let cut = lab.movie_in(
        &lab.movies,
        &folder,
        "Alien",
        "Alien (1979) Director's Cut.mkv",
    );
    let heat = lab.movie_in(&lab.movies, &folder, "Heat", "Heat.mkv");
    let movie = lab.movie_in(&lab.movies, &folder, "Movie", "Movie.mkv");
    let hd = lab.movie_in(&lab.movies, &folder, "Movie", "Movie.1080p.mkv");
    // `Heat 2.mkv` was added after the last Scan: the library does not know it, but its name
    // still keeps its files from `Heat`.
    drop_files(
        &folder,
        &[
            "Alien (1979).en.srt",
            "Alien (1979)-poster.jpg",
            "Alien (1979) Director's Cut.en.srt",
            "Alien (1979) Director's Cut-poster.jpg",
            "Heat.en.srt",
            "Heat 2.mkv",
            "Heat 2.en.srt",
            "Heat 2-fanart.jpg",
            "Movie.en.srt",
            "Movie-poster.jpg",
            "Movie.1080p.en.srt",
            "Movie.1080p.nfo",
        ],
    );

    let report = lab
        .removal(&trash)
        .delete_with_files(&[alien, heat, movie])
        .unwrap();
    assert!(report.problems.is_empty(), "{report:?}");
    assert_eq!(report.deleted.len(), 3);
    assert_eq!(
        left_in(&folder),
        [
            "Alien (1979) Director's Cut-poster.jpg",
            "Alien (1979) Director's Cut.en.srt",
            "Alien (1979) Director's Cut.mkv",
            "Heat 2-fanart.jpg",
            "Heat 2.en.srt",
            "Heat 2.mkv",
            "Movie.1080p.en.srt",
            "Movie.1080p.mkv",
            "Movie.1080p.nfo",
        ]
    );
    assert!(!lab.hidden(cut) && !lab.hidden(hd));
}

#[test]
fn a_name_shared_with_another_titles_video_stays_and_the_delete_goes_on() {
    let lab = Lab::new();
    let trash = FakeTrash::new(lab.dir.path());
    let folder = Path::new(&lab.movies.path).join("Mixed");
    let first = lab.movie_in(&lab.movies, &folder, "Movie", "Movie.mkv");
    let second = lab.movie_in(&lab.movies, &folder, "Movie", "Movie.avi");
    drop_files(&folder, &["Movie.en.srt", "Movie-poster.jpg"]);

    let report = lab.removal(&trash).delete_with_files(&[first]).unwrap();
    assert!(report.problems.is_empty(), "{report:?}");
    assert_eq!(report.deleted[0].paths, 1, "only its own video");
    assert_eq!(
        left_in(&folder),
        ["Movie-poster.jpg", "Movie.avi", "Movie.en.srt"]
    );
    assert!(lab.hidden(first) && !lab.hidden(second));
}

#[test]
fn a_file_another_title_records_is_never_taken_by_name() {
    let lab = Lab::new();
    let trash = FakeTrash::new(lab.dir.path());
    let folder = Path::new(&lab.movies.path).join("Mixed");
    let heat = lab.movie_in(&lab.movies, &folder, "Heat", "Heat.mkv");
    // Another title records a file whose name continues `Heat`.
    let other = lab.movie_with(
        &lab.movies,
        &folder,
        "Ronin",
        &["Ronin.mkv", "Heat.commentary.srt"],
    );
    drop_files(&folder, &["Heat.en.srt"]);
    // A video that continues the name is never taken by name, even unknown to the library.
    drop_files(&folder, &["Heat.extended.mkv"]);

    lab.removal(&trash).delete_with_files(&[heat]).unwrap();
    assert_eq!(
        left_in(&folder),
        ["Heat.commentary.srt", "Heat.extended.mkv", "Ronin.mkv"]
    );
    assert!(!lab.hidden(other));
}

#[test]
fn a_title_in_the_root_of_its_library_folder_takes_its_named_files_and_the_root_stays() {
    let lab = Lab::new();
    let trash = FakeTrash::new(lab.dir.path());
    let root = PathBuf::from(&lab.movies.path);
    let movie = lab.movie_in(
        &lab.movies,
        &root,
        "Example Movie",
        "Example Movie (2020).mkv",
    );
    drop_files(
        &root,
        &[
            "Example Movie (2020).nfo",
            "Example Movie (2020)-fanart.jpg",
            "Unrelated.srt",
        ],
    );
    let heat = lab.movie(&lab.movies, "Heat (1995)");

    let report = lab.removal(&trash).delete_with_files(&[movie]).unwrap();
    assert_eq!(report.deleted[0].paths, 3, "{report:?}");
    assert!(root.is_dir());
    assert_eq!(left_in(&root), ["Heat (1995)", "Unrelated.srt"]);
    assert!(!lab.hidden(heat));
}

#[test]
fn an_episode_takes_only_its_own_named_files() {
    let lab = Lab::new();
    let trash = FakeTrash::new(lab.dir.path());
    let (show, _, episodes) = lab.show("Dark");
    let season = Path::new(&lab.tv.path).join("Dark/Season 1");
    drop_files(
        &season,
        &[
            "Dark S01E01.en.forced.srt",
            "Dark S01E01-thumb.jpg",
            "Dark S01E02.en.srt",
            "season01-poster.jpg",
        ],
    );

    let report = lab
        .removal(&trash)
        .delete_with_files(&[episodes[0]])
        .unwrap();
    assert_eq!(report.deleted[0].paths, 3, "{report:?}");
    assert_eq!(
        left_in(&season),
        [
            "Dark S01E02.en.srt",
            "Dark S01E02.mkv",
            "season01-poster.jpg"
        ]
    );
    assert!(!lab.hidden(show) && !lab.hidden(episodes[1]));
}

#[test]
fn the_preview_counts_the_named_files_and_their_size() {
    let lab = Lab::new();
    let trash = FakeTrash::new(lab.dir.path());
    let folder = Path::new(&lab.movies.path).join("Mixed");
    let movie = lab.movie_in(
        &lab.movies,
        &folder,
        "Example Movie",
        "Example Movie (2020).mkv",
    );
    lab.movie_in(
        &lab.movies,
        &folder,
        "Other Movie",
        "Other Movie (2019).mkv",
    );
    drop_files(
        &folder,
        &["Example Movie (2020).en.srt", "Example Movie (2020).fr.srt"],
    );

    let plan = lab.removal(&trash).plan(&[movie]).unwrap();
    assert_eq!(plan.rows[0].folder_use, FolderUse::Shared);
    assert_eq!(plan.rows[0].files, 3);
    assert_eq!(plan.rows[0].bytes, 10 + 5 + 5);
    assert!(trash.names().is_empty(), "the preview changes nothing");
    assert_eq!(left_in(&folder).len(), 4);
}

#[test]
fn a_title_with_its_own_folder_still_goes_as_the_folder() {
    let lab = Lab::new();
    let trash = FakeTrash::new(lab.dir.path());
    let heat = lab.movie(&lab.movies, "Heat (1995)");
    let folder = Path::new(&lab.movies.path).join("Heat (1995)");
    drop_files(&folder, &["Heat (1995).en.srt"]);

    let plan = lab.removal(&trash).plan(&[heat]).unwrap();
    assert_eq!(plan.rows[0].folder_use, FolderUse::Own);
    assert_eq!(plan.rows[0].files, 3, "the recorded files");
    let report = lab.removal(&trash).delete_with_files(&[heat]).unwrap();
    assert_eq!(report.deleted[0].paths, 1, "the folder goes as one");
    assert_eq!(trash.names(), ["Heat (1995)"]);
}

#[test]
fn a_permanent_delete_erases_the_named_files_and_lists_them_first() {
    let lab = Lab::new();
    let trash = FakeTrash::new(lab.dir.path());
    let folder = Path::new(&lab.movies.path).join("Mixed");
    let movie = lab.movie_in(
        &lab.movies,
        &folder,
        "Example Movie",
        "Example Movie (2020).mkv",
    );
    let cut = lab.movie_in(
        &lab.movies,
        &folder,
        "Example Movie",
        "Example Movie (2020) Director's Cut.mkv",
    );
    drop_files(
        &folder,
        &[
            "Example Movie (2020).en.srt",
            "Example Movie (2020)-poster.jpg",
            "Example Movie (2020) Director's Cut.en.srt",
        ],
    );

    let report = lab.removal(&trash).erase(&[movie], "DELETE").unwrap();
    assert!(
        report.problems.is_empty() && report.stopped.is_empty(),
        "{report:?}"
    );
    assert_eq!(report.erased[0].erased.len(), 3, "{report:?}");
    assert!(
        report.erased[0]
            .erased
            .iter()
            .any(|p| p.ends_with("Example Movie (2020).en.srt"))
    );
    assert_eq!(
        left_in(&folder),
        [
            "Example Movie (2020) Director's Cut.en.srt",
            "Example Movie (2020) Director's Cut.mkv",
        ]
    );
    assert!(trash.names().is_empty(), "nothing went to the bin");
    assert!(!lab.exists(movie) && lab.exists(cut));

    let detail: String = lab
        .db
        .read(move |c| {
            Ok(c.query_row(
                "SELECT detail_json FROM history_entry WHERE id = ?1",
                [report.history_id.unwrap()],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert!(
        detail.contains("Example Movie (2020)-poster.jpg"),
        "the entry lists the named files: {detail}"
    );
}
