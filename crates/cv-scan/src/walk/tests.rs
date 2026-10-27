//! Walker behaviour beyond the grouping fixtures in `tests/compat/scan/movie-trees.json`
//! (run by `tests/movie_trees.rs`).

use std::fs;
use std::path::Path;

use cv_domain::{DataSource, MediaFileType, SourceKind};
use tempfile::TempDir;

use super::*;
use crate::skip::SkipRules;

/// A temporary data source; the default temp name starts with a dot, which the skip rules hide.
fn source(files: &[&str]) -> TempDir {
    let dir = tempfile::Builder::new()
        .prefix("cvwalk")
        .tempdir()
        .expect("temp dir");
    for file in files {
        let path = dir.path().join(file);
        fs::create_dir_all(path.parent().expect("parent")).expect("create folder");
        fs::write(path, b"").expect("create file");
    }
    dir
}

fn data_source(id: i64, kind: SourceKind, path: &Path, position: i64) -> DataSource {
    DataSource {
        id,
        kind,
        path: path.to_string_lossy().into_owned(),
        position,
        label: String::new(),
        created_ms: 0,
    }
}

fn titles(report: &ScanReport) -> Vec<&str> {
    report.movies.iter().map(|m| m.title.as_str()).collect()
}

#[test]
fn sources_are_walked_in_position_order_and_tv_sources_are_ignored() {
    let first = source(&["Zulu (1964)/Zulu (1964).mkv"]);
    let second = source(&["Alien (1979)/Alien (1979).mkv"]);
    let tv = source(&["Show/Show.S01E01.mkv"]);
    let sources = [
        data_source(1, SourceKind::Movie, second.path(), 1),
        data_source(2, SourceKind::Tv, tv.path(), 0),
        data_source(3, SourceKind::Movie, first.path(), 0),
    ];
    let report = scan_movie_sources(&sources, &WalkOptions::default());
    assert_eq!(titles(&report), ["Zulu", "Alien"]);
    assert_eq!(report.movies[0].source_id, Some(3));
    assert_eq!(report.movies[1].source_id, Some(1));
    assert_eq!(report.movies[1].source_root, second.path());
}

#[test]
fn missing_source_is_reported_and_marks_the_source_unreliable() {
    let dir = source(&[]);
    let gone = dir.path().join("gone");
    let report = scan_movie_source(&gone, None, &WalkOptions::default());
    assert!(report.movies.is_empty());
    assert_eq!(report.problems.len(), 1);
    assert_eq!(report.problems[0].kind, ProblemKind::SourceUnavailable);
    assert!(report.source_unreliable(&gone));
}

#[test]
fn empty_source_is_reported_as_possibly_disconnected() {
    let dir = source(&[]);
    let report = scan_movie_source(dir.path(), None, &WalkOptions::default());
    assert!(report.movies.is_empty());
    assert_eq!(report.problems[0].kind, ProblemKind::SourceEmpty);
    assert!(report.source_unreliable(dir.path()));
}

#[test]
fn a_source_with_movies_is_reliable() {
    let dir = source(&["Heat (1995)/Heat (1995).mkv"]);
    let report = scan_movie_source(dir.path(), None, &WalkOptions::default());
    assert!(report.problems.is_empty());
    assert!(!report.source_unreliable(dir.path()));
}

#[test]
fn disc_folder_in_the_root_is_an_error_not_a_movie() {
    let dir = source(&["BDMV/index.bdmv", "Heat (1995)/Heat (1995).mkv"]);
    let report = scan_movie_source(dir.path(), None, &WalkOptions::default());
    assert_eq!(titles(&report), ["Heat"]);
    assert_eq!(report.problems.len(), 1);
    assert_eq!(report.problems[0].kind, ProblemKind::DiscFolderInRoot);
}

#[test]
fn a_skipped_or_marked_source_yields_nothing() {
    let marked = source(&["Heat (1995)/Heat (1995).mkv", ".cinevaultignore"]);
    let report = scan_movie_source(marked.path(), None, &WalkOptions::default());
    assert!(report.movies.is_empty());
    assert!(report.problems.is_empty());

    let plain = source(&["Heat (1995)/Heat (1995).mkv"]);
    let name = plain.path().file_name().unwrap().to_string_lossy();
    let options = WalkOptions::new(
        SkipRules::new(LibraryKind::Movies, &[format!("^{name}$")], true),
        Vec::new(),
    );
    assert!(
        scan_movie_source(plain.path(), None, &options)
            .movies
            .is_empty()
    );
}

#[test]
fn nomedia_marker_is_ignored_when_the_setting_is_off() {
    let dir = source(&["Heat (1995)/Heat (1995).mkv", "Heat (1995)/.nomedia"]);
    let on = scan_movie_source(dir.path(), None, &WalkOptions::default());
    assert!(on.movies.is_empty());

    let options = WalkOptions::new(
        SkipRules::new::<&str>(LibraryKind::Movies, &[], false),
        Vec::new(),
    );
    let off = scan_movie_source(dir.path(), None, &options);
    assert_eq!(titles(&off), ["Heat"]);
}

#[test]
fn user_skip_patterns_and_bad_words_apply() {
    let dir = source(&[
        "Heat (1995) GRP/Heat (1995) GRP.mkv",
        "Samples Pack/Clip (2000)/Clip (2000).mkv",
    ]);
    let options = WalkOptions::new(
        SkipRules::new(LibraryKind::Movies, &["Samples.*"], true),
        vec!["grp".to_owned()],
    );
    let report = scan_movie_source(dir.path(), None, &options);
    assert_eq!(titles(&report), ["Heat"]);
}

#[test]
fn generic_graphics_attach_to_their_movie_in_multi_movie_folders() {
    // A folder with several movies gets the same companion pass as a single-movie folder.
    let dir = source(&[
        "Movies/Alien (1979).mkv",
        "Movies/Alien (1979).jpg",
        "Movies/Aliens (1986).mkv",
        "Movies/Aliens (1986)-concept-fanart.jpg",
        "Movies/Aliens (1986)-concept-poster.jpg",
    ]);
    let report = scan_movie_source(dir.path(), None, &WalkOptions::default());
    assert_eq!(titles(&report), ["Alien", "Aliens"]);
    let alien = &report.movies[0];
    assert!(alien.multi_movie_dir);
    assert_eq!(alien.files_of(MediaFileType::Poster).count(), 1);
    assert_eq!(
        alien
            .files_of(MediaFileType::Poster)
            .next()
            .unwrap()
            .relative,
        "Movies/Alien (1979).jpg"
    );
    // A downgraded `<other>-poster` is promoted back to the first artwork type still missing.
    let aliens = &report.movies[1];
    assert_eq!(aliens.files_of(MediaFileType::Fanart).count(), 1);
    assert_eq!(aliens.files_of(MediaFileType::Poster).count(), 1);
}

#[test]
fn files_record_size_and_modification_time() {
    let dir = source(&[]);
    let folder = dir.path().join("Heat (1995)");
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("Heat (1995).mkv"), vec![0_u8; 1234]).unwrap();
    let report = scan_movie_source(dir.path(), None, &WalkOptions::default());
    let file = &report.movies[0].files[0];
    assert_eq!(file.size_bytes, 1234);
    assert!(file.modified_ms > 0);
    assert_eq!(file.path, folder.join("Heat (1995).mkv"));
}

#[test]
fn walking_changes_nothing_on_disk() {
    let files = [
        "Heat (1995)/Heat (1995).mkv",
        "Movies/Alien (1979).mkv",
        "Movies/Aliens (1986).mkv",
        "Brazil (1985)/VIDEO_TS/VIDEO_TS.IFO",
    ];
    let dir = source(&files);
    let before = listing(dir.path());
    let _ = scan_movie_source(dir.path(), None, &WalkOptions::default());
    assert_eq!(listing(dir.path()), before);
    assert!(files.iter().all(|f| dir.path().join(f).exists()));
}

fn listing(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            out.push(
                path.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
            );
            if path.is_dir() {
                stack.push(path);
            }
        }
    }
    out.sort();
    out
}

#[test]
fn unicode_names_are_grouped() {
    let dir = source(&[
        "Amélie (2001)/Amélie (2001).mkv",
        "Amélie (2001)/Amélie (2001)-poster.jpg",
    ]);
    let report = scan_movie_source(dir.path(), None, &WalkOptions::default());
    assert_eq!(report.movies.len(), 1);
    assert_eq!(report.movies[0].files.len(), 2);
}

// ---- TV show walker (file-tree groupings are in tests/compat/scan/tv-trees.json) ----

fn show_titles(report: &TvScanReport) -> Vec<&str> {
    report.shows.iter().map(|s| s.title.as_str()).collect()
}

#[test]
fn tv_sources_are_walked_in_position_order_and_movie_sources_are_ignored() {
    let first = source(&["Zulu Show/Zulu.Show.S01E01.mkv"]);
    let second = source(&["Alpha Show/Alpha.Show.S01E01.mkv"]);
    let movies = source(&["Heat (1995)/Heat (1995).mkv"]);
    let sources = [
        data_source(1, SourceKind::Tv, second.path(), 1),
        data_source(2, SourceKind::Movie, movies.path(), 0),
        data_source(3, SourceKind::Tv, first.path(), 0),
    ];
    let report = scan_tv_sources(&sources, &WalkOptions::tv_defaults());
    assert_eq!(show_titles(&report), ["Zulu Show", "Alpha Show"]);
    assert_eq!(report.shows[0].source_id, Some(3));
    assert_eq!(report.shows[1].source_root, second.path());
    assert_eq!(report.shows[1].path, second.path().join("Alpha Show"));
}

#[test]
fn missing_and_empty_tv_sources_are_unreliable_not_empty_libraries() {
    let dir = source(&[]);
    let gone = dir.path().join("gone");
    let missing = scan_tv_source(&gone, None, &WalkOptions::tv_defaults());
    assert!(missing.shows.is_empty());
    assert_eq!(missing.problems[0].kind, ProblemKind::SourceUnavailable);
    assert!(missing.source_unreliable(&gone));

    let empty = scan_tv_source(dir.path(), None, &WalkOptions::tv_defaults());
    assert_eq!(empty.problems[0].kind, ProblemKind::SourceEmpty);
    assert!(empty.source_unreliable(dir.path()));

    let full = source(&["Show/Show.S01E01.mkv"]);
    let ok = scan_tv_source(full.path(), None, &WalkOptions::tv_defaults());
    assert!(ok.problems.is_empty());
    assert!(!ok.source_unreliable(full.path()));
}

#[test]
fn tv_skip_rules_apply_including_user_patterns_and_the_nomedia_setting() {
    let dir = source(&[
        "Keep/Keep.S01E01.mkv",
        "Samples Pack/Clip.S01E01.mkv",
        "Marked/Marked.S01E01.mkv",
        "Marked/.nomedia",
    ]);
    let skip = SkipRules::new(LibraryKind::TvShows, &["Samples.*"], true);
    let report = scan_tv_source(dir.path(), None, &WalkOptions::new(skip, Vec::new()));
    assert_eq!(show_titles(&report), ["Keep"]);

    let skip = SkipRules::new(LibraryKind::TvShows, &["Samples.*"], false);
    let report = scan_tv_source(dir.path(), None, &WalkOptions::new(skip, Vec::new()));
    assert_eq!(show_titles(&report), ["Keep", "Marked"]);
}

#[test]
fn a_disc_folder_in_the_tv_root_is_reported() {
    let dir = source(&["BDMV/index.bdmv", "Show/Show.S01E01.mkv"]);
    let report = scan_tv_source(dir.path(), None, &WalkOptions::tv_defaults());
    assert_eq!(show_titles(&report), ["Show"]);
    assert_eq!(report.problems.len(), 1);
    assert_eq!(report.problems[0].kind, ProblemKind::DiscFolderInRoot);
}

#[test]
fn user_bad_words_apply_to_show_and_episode_names() {
    let dir = source(&["Show GRP/Show GRP S01E01.mkv"]);
    let options = WalkOptions::new(
        SkipRules::with_defaults(LibraryKind::TvShows),
        vec!["grp".to_owned()],
    );
    let report = scan_tv_source(dir.path(), None, &options);
    assert_eq!(show_titles(&report), ["Show"]);
    assert_eq!(report.shows[0].episodes[0].numbers, [1]);
}

#[test]
fn season_numbers_join_episodes_and_season_files() {
    let dir = source(&[
        "Show/Season 1/Show.S01E01.mkv",
        "Show/Season 3/Show.S03E01.mkv",
        "Show/season05-poster.jpg",
        "Show/season-specials-poster.jpg",
    ]);
    let report = scan_tv_source(dir.path(), None, &WalkOptions::tv_defaults());
    let show = &report.shows[0];
    assert_eq!(show.season_numbers(), [0, 1, 3, 5]);
    assert_eq!(show.season(5).unwrap().files.len(), 1);
    assert!(show.season(1).is_none());
    assert_eq!(
        show.season(0).unwrap().files[0].kind,
        MediaFileType::SeasonPoster
    );
}

#[test]
fn vsmeta_artwork_extraction_follows_the_setting_for_tv_like_for_movies() {
    // TV extraction follows the setting: it extracts when the setting is on, and only then.
    let dir = source(&[
        "Show/Season 1/Show.S01E01.mkv",
        "Show/Season 1/Show.S01E01.mkv.vsmeta",
        "Show/Season 1/Show.S01E02.mkv",
    ]);
    let off = WalkOptions::tv_defaults();
    let on = WalkOptions::tv_defaults().with_vsmeta_artwork(true);
    assert!(!off.extract_vsmeta_artwork);

    let report = scan_tv_source(dir.path(), None, &off);
    assert!(report.shows[0].vsmeta_artwork_sources(&off).is_empty());
    assert!(report.shows[0].vsmeta_artwork_sources(&on).len() == 1);

    let report = scan_tv_source(dir.path(), None, &on);
    let sources = report.shows[0].vsmeta_artwork_sources(&on);
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].relative, "Show/Season 1/Show.S01E01.mkv.vsmeta");
    assert!(report.shows[0].vsmeta_artwork_sources(&off).is_empty());
}

#[test]
fn tv_files_record_size_and_modification_time() {
    let dir = source(&[]);
    let folder = dir.path().join("Show");
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("Show.S01E01.mkv"), vec![0_u8; 321]).unwrap();
    let report = scan_tv_source(dir.path(), None, &WalkOptions::tv_defaults());
    let file = &report.shows[0].episodes[0].files[0];
    assert_eq!(file.size_bytes, 321);
    assert!(file.modified_ms > 0);
    assert_eq!(file.path, folder.join("Show.S01E01.mkv"));
}

#[test]
fn walking_a_tv_source_changes_nothing_on_disk() {
    let files = [
        "Show/Season 1/Show.S01E01.mkv",
        "Show/Season 1/Show.S01E01.cd2.mkv",
        "Show/poster.jpg",
        "A/Alpha/Season 1/Alpha.S01E01.mkv",
        "Loose.S01E01.mkv",
    ];
    let dir = source(&files);
    let before = listing(dir.path());
    let _ = scan_tv_source(dir.path(), None, &WalkOptions::tv_defaults());
    assert_eq!(listing(dir.path()), before);
    assert!(files.iter().all(|f| dir.path().join(f).exists()));
}

#[test]
fn multi_episode_files_expose_all_numbers() {
    let dir = source(&["Show/Show.S02E05-E07.mkv", "Show/Show.S02E09.mkv"]);
    let report = scan_tv_source(dir.path(), None, &WalkOptions::tv_defaults());
    let episodes = &report.shows[0].episodes;
    assert!(episodes[0].is_multi_episode());
    assert_eq!(episodes[0].numbers, [5, 6, 7]);
    assert!(episodes[0].is_numbered());
    assert!(!episodes[1].is_multi_episode());
}

#[test]
fn unicode_show_and_season_names_are_grouped() {
    let dir = source(&[
        "Amélie Série/Saison 2/Amélie Série - S02E01.mkv",
        "Amélie Série/Saison 2/poster.jpg",
    ]);
    let report = scan_tv_source(dir.path(), None, &WalkOptions::tv_defaults());
    let show = &report.shows[0];
    assert_eq!(show.episodes.len(), 1);
    assert_eq!(show.season(2).unwrap().files.len(), 1);
}

#[test]
fn a_walk_of_several_sources_leaves_out_the_sources_nested_in_each() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Mixed");
    for rel in [
        "Arrival (2016).mkv",
        "Chernobyl/Season 1/Chernobyl S01E01.mkv",
        "Films/Heat (1995)/Heat (1995).mkv",
    ] {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"x").unwrap();
    }
    let source = |id: i64, kind, rel: &str| DataSource {
        id,
        kind,
        path: root.join(rel).to_string_lossy().into_owned(),
        position: 0,
        label: String::new(),
        created_ms: 0,
    };
    let sources = [
        source(1, SourceKind::Movie, ""),
        source(2, SourceKind::Tv, "Chernobyl"),
        source(3, SourceKind::Movie, "Films"),
    ];
    assert_eq!(
        nested_sources(&sources[0], &sources),
        [sources[1].path.clone(), sources[2].path.clone()]
    );
    assert!(nested_sources(&sources[1], &sources).is_empty());

    let movies = scan_movie_sources(&sources, &WalkOptions::with_defaults()).movies;
    let mut titles: Vec<&str> = movies.iter().map(|m| m.title.as_str()).collect();
    titles.sort_unstable();
    // Heat comes from its own source only; nothing comes from the TV folder.
    assert_eq!(titles, ["Arrival", "Heat"]);
}

#[test]
fn typescript_files_are_not_movies_but_transport_streams_are() {
    let dir = tempfile::tempdir().unwrap();
    let root = &dir.path().join("library");
    let code = root.join("tool/node_modules/pkg");
    fs::create_dir_all(&code).unwrap();
    fs::write(code.join("index.ts"), b"export const x = 1;\n").unwrap();
    fs::write(
        code.join("index.d.ts"),
        b"export declare const x: number;\n",
    )
    .unwrap();
    let mut stream = vec![0u8; 400];
    stream[0] = 0x47;
    stream[188] = 0x47;
    fs::create_dir_all(root.join("Recording (2020)")).unwrap();
    fs::write(root.join("Recording (2020)/Recording (2020).ts"), &stream).unwrap();

    let report = scan_movie_source(root, None, &WalkOptions::with_defaults());
    let titles: Vec<&str> = report.movies.iter().map(|m| m.title.as_str()).collect();
    assert_eq!(titles, ["Recording"]);
}

#[test]
fn a_tv_library_folder_that_is_one_show_is_read_as_that_show() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Chernobyl");
    for rel in [
        "Season 1/Chernobyl S01E01.mkv",
        "Season 1/Chernobyl S01E02.mkv",
        "poster.jpg",
    ] {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"x").unwrap();
    }
    let report = scan_tv_source(&root, None, &WalkOptions::tv_defaults());
    let titles: Vec<&str> = report.shows.iter().map(|s| s.title.as_str()).collect();
    assert_eq!(titles, ["Chernobyl"]);
    let show = &report.shows[0];
    assert_eq!(show.relative_path, "");
    assert_eq!(show.path, root);
    assert_eq!(show.episodes.len(), 2);
    assert_eq!(show.season_numbers(), [1]);
    assert!(show.files_of(MediaFileType::Poster).next().is_some());
    assert!(report.problems.is_empty(), "{:?}", report.problems);
}

#[test]
fn a_folder_of_shows_is_still_read_show_by_show() {
    let dir = source(&[
        "Chernobyl/Season 1/Chernobyl S01E01.mkv",
        "Dark/Season 2/Dark S02E01.mkv",
        "Seasons of Love/Seasons of Love S01E01.mkv",
    ]);
    let report = scan_tv_source(dir.path(), None, &WalkOptions::tv_defaults());
    let titles: Vec<&str> = report.shows.iter().map(|s| s.title.as_str()).collect();
    assert_eq!(titles, ["Chernobyl", "Dark", "Seasons of Love"]);
}

#[test]
fn season_folder_names_are_whole_names_only() {
    use crate::episode::is_season_folder_name;
    for name in [
        "Season 1",
        "season.02",
        "Staffel 3",
        "S01",
        "Specials",
        "Saison 10",
    ] {
        assert!(is_season_folder_name(name), "{name}");
    }
    for name in [
        "Seasons of Love",
        "Season of the Witch (1972)",
        "Show S01",
        "Extras",
    ] {
        assert!(!is_season_folder_name(name), "{name}");
    }
}

#[test]
fn tv_absolute_numbered_anime_files_are_episodes_of_season_one() {
    let dir = source(&[
        "One Piece/[SubGroup] One Piece - 001 [1080p].mkv",
        "One Piece/[SubGroup] One Piece - 001 [1080p].srt",
        "One Piece/[SubGroup] One Piece - 002 [1080p].mkv",
        "One Piece/[SubGroup] One Piece - 003 [1080p].mkv",
        "One Piece/[SubGroup] One Piece - 100 [1080p].mkv",
        "One Piece/[SubGroup] One Piece - 1001 [1080p].mkv",
    ]);
    let report = scan_tv_source(dir.path(), None, &WalkOptions::tv_defaults());
    let show = &report.shows[0];
    let numbers: Vec<(Option<u32>, Vec<u32>)> = show
        .episodes
        .iter()
        .map(|e| (e.season, e.numbers.clone()))
        .collect();
    assert_eq!(
        numbers,
        [
            (Some(1), vec![1]),
            (Some(1), vec![2]),
            (Some(1), vec![3]),
            (Some(1), vec![100]),
            (Some(1), vec![1001]),
        ]
    );
    // The subtitle follows its video into the absolute numbering.
    assert_eq!(show.episodes[0].files.len(), 2);
}

/// reg3-1: names that carry only the release group, the show name and the number give
/// episodes without a title; a name with a title keeps it.
#[test]
fn tv_episode_names_without_a_title_give_no_title() {
    let dir = source(&[
        "One Piece/[SubGroup] One Piece - 001 [1080p].mkv",
        "One Piece/[SubGroup] One Piece - 002 [1080p].mkv",
        "One Piece/[SubGroup] One Piece - 1001 [1080p].mkv",
        "Amélie Show (2015)/Season 1/Amelie S01E01.mkv",
        "Amélie Show (2015)/Season 1/Amelie S01E02 - Le Café.mkv",
        "Dark/Dark - S01E01.mkv",
    ]);
    let report = scan_tv_source(dir.path(), None, &WalkOptions::tv_defaults());
    let titles: Vec<(&str, Vec<&str>)> = report
        .shows
        .iter()
        .map(|show| {
            (
                show.title.as_str(),
                show.episodes.iter().map(|e| e.title.as_str()).collect(),
            )
        })
        .collect();
    assert_eq!(
        titles,
        [
            ("Amélie Show", vec!["", "Le Café"]),
            ("Dark", vec![""]),
            ("One Piece", vec!["", "", ""]),
        ]
    );
}

#[test]
fn tv_three_digit_season_episode_names_keep_the_season_reading() {
    let dir = source(&[
        "Show/Show - 101.mkv",
        "Show/Show - 102.mkv",
        "Show/Show - 103.mkv",
        "Show/Show - 201.mkv",
    ]);
    let report = scan_tv_source(dir.path(), None, &WalkOptions::tv_defaults());
    let numbers: Vec<(Option<u32>, Vec<u32>)> = report.shows[0]
        .episodes
        .iter()
        .map(|e| (e.season, e.numbers.clone()))
        .collect();
    assert_eq!(
        numbers,
        [
            (Some(1), vec![1]),
            (Some(1), vec![2]),
            (Some(1), vec![3]),
            (Some(2), vec![1]),
        ]
    );
}

/// A stand-in for a drive that is not mounted.
fn unmounted(_: &Path) -> bool {
    false
}

#[test]
fn a_source_on_a_drive_that_is_not_mounted_is_unreadable_even_with_files_in_it() {
    // The mount-point folder of an unplugged drive can hold files of its own: it is not the library.
    let dir = source(&["Heat (1995)/Heat (1995).mkv"]);
    let options = WalkOptions {
        drive_is_mounted: unmounted,
        ..WalkOptions::default()
    };
    let report = scan_movie_source(dir.path(), None, &options);
    assert!(report.movies.is_empty());
    assert_eq!(report.problems.len(), 1);
    assert_eq!(report.problems[0].kind, ProblemKind::SourceUnavailable);
    assert!(report.source_unreliable(dir.path()));

    let tv = source(&["Show/Show.S01E01.mkv"]);
    let options = WalkOptions {
        drive_is_mounted: unmounted,
        ..WalkOptions::tv_defaults()
    };
    let report = scan_tv_source(tv.path(), None, &options);
    assert!(report.shows.is_empty());
    assert_eq!(report.problems[0].kind, ProblemKind::SourceUnavailable);
    assert!(report.source_unreliable(tv.path()));

    // The same folders on a mounted drive are read.
    let report = scan_movie_source(dir.path(), None, &WalkOptions::default());
    assert_eq!(titles(&report), ["Heat"]);
}
