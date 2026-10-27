use std::fs;
use std::path::Path;

use super::*;

fn touch(root: &Path, relative: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, b"x").unwrap();
}

fn tree(files: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for file in files {
        touch(dir.path(), file);
    }
    dir
}

#[test]
fn a_folder_of_episode_files_is_tv_shows() {
    let dir = tree(&[
        "Severance/Season 1/Severance - S01E01 - Good News.mkv",
        "Severance/Season 1/Severance - S01E02 - Half Loop.mkv",
        "Andor/Andor.S01E01.1080p.mkv",
        "Andor/Andor.S01E02.1080p.mkv",
    ]);
    let found = analyze_folder(dir.path()).unwrap();
    assert_eq!(found.guess, FolderGuess::Tv);
    assert_eq!(found.suggested, SuggestedKind::Tv);
    assert_eq!((found.videos, found.episodes), (4, 4));
}

#[test]
fn a_folder_of_films_is_movies_even_when_a_title_has_digits() {
    let dir = tree(&[
        "Blade Runner 2049 (2017)/Blade Runner 2049 (2017).mkv",
        "1917 (2019)/1917 (2019).mp4",
        "Se7en (1995)/Se7en (1995).avi",
        "12 Angry Men (1957)/12 Angry Men (1957).mkv",
    ]);
    let found = analyze_folder(dir.path()).unwrap();
    assert_eq!(found.guess, FolderGuess::Movies);
    assert_eq!((found.videos, found.episodes), (4, 0));
}

#[test]
fn episodes_and_films_together_make_a_mixed_folder_that_can_be_split() {
    let dir = tree(&[
        "Movies/Heat (1995)/Heat (1995).mkv",
        "Movies/Alien (1979)/Alien (1979).mkv",
        "Shows/Lost/Season 1/Lost - S01E01.mkv",
        "Shows/Lost/Season 1/Lost - S01E02.mkv",
    ]);
    let found = analyze_folder(dir.path()).unwrap();
    assert_eq!(found.guess, FolderGuess::Mixed);
    assert_eq!(found.suggested, SuggestedKind::Undecided);
    let kinds: Vec<(&str, FolderGuess)> = found
        .children
        .iter()
        .map(|c| (c.name.as_str(), c.guess))
        .collect();
    assert_eq!(
        kinds,
        vec![("Movies", FolderGuess::Movies), ("Shows", FolderGuess::Tv)]
    );
}

#[test]
fn videos_loose_in_the_root_of_a_mixed_folder_are_counted_and_named() {
    let dir = tree(&[
        "Movies/Heat (1995)/Heat (1995).mkv",
        "Shows/Lost/Season 1/Lost - S01E01.mkv",
        "Shows/Lost/Season 1/Lost - S01E02.mkv",
        "Zodiac (2007).mkv",
        "Arrival (2016).mkv",
        "notes.txt",
    ]);
    let found = analyze_folder(dir.path()).unwrap();
    assert_eq!(found.loose_videos, 2);
    assert_eq!(
        found.loose_examples,
        vec!["Arrival (2016).mkv", "Zodiac (2007).mkv"]
    );
    // The sub-folders carry no loose videos of their own.
    assert!(found.children.iter().all(|child| child.loose_videos == 0));
    // Nothing loose: nothing to say.
    let tidy = tree(&["Movies/Heat (1995)/Heat (1995).mkv"]);
    assert_eq!(analyze_folder(tidy.path()).unwrap().loose_videos, 0);
}

#[test]
fn season_folders_mark_episodes_whatever_the_file_names_say() {
    let dir = tree(&[
        "Show/Season 02/episode one.mkv",
        "Show/Season 02/episode two.mkv",
        "Show/Specials/behind.mkv",
    ]);
    let found = analyze_folder(dir.path()).unwrap();
    assert_eq!(found.guess, FolderGuess::Tv);
}

#[test]
fn trailers_samples_extras_and_artwork_are_not_counted() {
    let dir = tree(&[
        "Heat (1995)/Heat (1995).mkv",
        "Heat (1995)/Heat (1995)-trailer.mkv",
        "Heat (1995)/Sample/sample.mkv",
        "Heat (1995)/Extras/Making of.mkv",
        "Heat (1995)/poster.jpg",
        "Heat (1995)/Heat (1995).nfo",
    ]);
    let found = analyze_folder(dir.path()).unwrap();
    assert_eq!(found.videos, 1);
}

#[test]
fn a_disc_structure_counts_once() {
    let dir = tree(&[
        "Disc Movie (2001)/VIDEO_TS/VIDEO_TS.IFO",
        "Disc Movie (2001)/VIDEO_TS/VTS_01_1.VOB",
        "Disc Movie (2001)/VIDEO_TS/VTS_01_2.VOB",
    ]);
    let found = analyze_folder(dir.path()).unwrap();
    assert_eq!(found.videos, 1);
    assert_eq!(found.guess, FolderGuess::Movies);
}

#[test]
fn an_empty_folder_takes_its_suggestion_from_its_name() {
    let dir = tempfile::tempdir().unwrap();
    let shows = dir.path().join("TVShows");
    let films = dir.path().join("Films");
    fs::create_dir_all(&shows).unwrap();
    fs::create_dir_all(&films).unwrap();
    let shows = analyze_folder(&shows).unwrap();
    assert_eq!(shows.guess, FolderGuess::Empty);
    assert_eq!(shows.suggested, SuggestedKind::Tv);
    assert_eq!(
        analyze_folder(&films).unwrap().suggested,
        SuggestedKind::Movies
    );
}

#[test]
fn skipped_and_hidden_folders_are_not_entered() {
    let dir = tree(&[
        "Heat (1995)/Heat (1995).mkv",
        "$RECYCLE.BIN/old.mkv",
        ".hidden/other.mkv",
    ]);
    assert_eq!(analyze_folder(dir.path()).unwrap().videos, 1);
}

#[test]
fn a_missing_path_or_a_file_is_refused() {
    let dir = tree(&["a.mkv"]);
    assert!(analyze_folder(&dir.path().join("nope")).is_err());
    assert!(analyze_folder(&dir.path().join("a.mkv")).is_err());
}

#[test]
fn the_label_follows_the_share_of_episodes() {
    assert_eq!(label(0, 0), FolderGuess::Empty);
    assert_eq!(label(100, 0), FolderGuess::Movies);
    assert_eq!(label(100, 14), FolderGuess::Movies);
    assert_eq!(label(100, 15), FolderGuess::Mixed);
    assert_eq!(label(100, 84), FolderGuess::Mixed);
    assert_eq!(label(100, 85), FolderGuess::Tv);
}

#[test]
fn episode_markers_inside_a_title_are_not_episodes() {
    assert!(looks_like_episode("Show/Show.S02E05.mkv"));
    assert!(looks_like_episode("Show/Show 3x07.mkv"));
    assert!(!looks_like_episode("Movie (2010).mkv"));
    assert!(!looks_like_episode("Ocean's 8 (2018)/Ocean's 8 (2018).mkv"));
    assert!(!looks_like_episode("Apollo 13 (1995)/Apollo 13 (1995).mkv"));
}

#[test]
fn a_tv_folder_with_an_absolute_numbered_anime_show_is_tv_shows() {
    let dir = tree(&[
        "Breaking Bad (2008)/Season 01/Breaking.Bad.S01E01.Pilot.mkv",
        "Breaking Bad (2008)/Season 01/Breaking.Bad.S01E02.mkv",
        "The Office (US)/Season 1/The.Office.US.S01E01.mkv",
        "One Piece/[SubGroup] One Piece - 001 [1080p].mkv",
        "One Piece/[SubGroup] One Piece - 002 [1080p].mkv",
        "One Piece/[SubGroup] One Piece - 003 [1080p].mkv",
        "One Piece/[SubGroup] One Piece - 100 [1080p].mkv",
        "One Piece/[SubGroup] One Piece - 1001 [1080p].mkv",
    ]);
    let found = analyze_folder(dir.path()).unwrap();
    assert_eq!(found.guess, FolderGuess::Tv);
    assert_eq!((found.videos, found.episodes), (8, 8));
    let one_piece = found
        .children
        .iter()
        .find(|c| c.name == "One Piece")
        .unwrap();
    assert_eq!(one_piece.guess, FolderGuess::Tv);
    assert_eq!(one_piece.episodes, 5);
}

#[test]
fn numbered_films_are_not_a_series() {
    // A film series carries years, and differently named files are separate films.
    assert!(!numbered_series(
        &[
            "Rocky 1 (1976).mkv",
            "Rocky 2 (1979).mkv",
            "Rocky 3 (1982).mkv"
        ],
        "Rocky"
    ));
    assert!(!numbered_series(
        &["Alien 3.mkv", "Heat 2.mkv", "Saw 4.mkv"],
        "Films"
    ));
    // Too few files to tell.
    assert!(!numbered_series(
        &["Show - 01.mkv", "Show - 02.mkv"],
        "Show"
    ));
    assert!(numbered_series(
        &["Show - 01.mkv", "Show - 02.mkv", "Show - 03.mkv"],
        "Show"
    ));
}

#[test]
fn a_sub_folder_that_is_one_show_is_told_from_a_folder_of_shows() {
    let dir = tree(&[
        "Chernobyl/Season 1/Chernobyl S01E01.mkv",
        "Chernobyl/Season 1/Chernobyl S01E02.mkv",
        "Andor/Andor.S01E01.mkv",
        "Series/Lost/Season 1/Lost - S01E01.mkv",
        "Films/Heat (1995)/Heat (1995).mkv",
        "Arrival (2016).mkv",
    ]);
    let found = analyze_folder(dir.path()).unwrap();
    let shows: Vec<(&str, bool)> = found
        .children
        .iter()
        .map(|c| (c.name.as_str(), c.show))
        .collect();
    assert_eq!(
        shows,
        vec![
            ("Andor", true),
            ("Chernobyl", true),
            ("Films", false),
            ("Series", false)
        ]
    );
    assert!(!found.show);
}

#[test]
fn typescript_sources_are_not_counted_as_videos() {
    let dir = tree(&["media/Heat (1995)/Heat (1995).mkv"]);
    let code = dir.path().join("driver/node_modules/pkg");
    fs::create_dir_all(&code).unwrap();
    fs::write(
        code.join("index.d.ts"),
        b"export declare const x: number;\n",
    )
    .unwrap();
    fs::write(code.join("index.ts"), b"export const x = 1;\n").unwrap();
    let mut stream = vec![0u8; 400];
    stream[0] = 0x47;
    stream[188] = 0x47;
    fs::write(dir.path().join("media/Recording.ts"), &stream).unwrap();

    let found = analyze_folder(dir.path()).unwrap();
    assert_eq!(found.videos, 2, "the .mkv and the real transport stream");
    let names: Vec<&str> = found.children.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["media"]);
}

#[test]
fn a_folder_that_is_one_show_is_flagged_so_its_parent_can_be_offered() {
    let dir = tree(&[
        "Season 1/Chernobyl - S01E01.mkv",
        "Season 1/Chernobyl - S01E02.mkv",
    ]);
    let found = analyze_folder(dir.path()).unwrap();
    assert_eq!(found.guess, FolderGuess::Tv);
    assert!(found.single_show);

    let episodes_inside = tree(&["Andor.S01E01.mkv", "Andor.S01E02.mkv"]);
    assert!(analyze_folder(episodes_inside.path()).unwrap().single_show);
}

#[test]
fn a_folder_of_shows_or_of_films_is_not_a_single_show() {
    let shows = tree(&[
        "Severance/Season 1/Severance - S01E01.mkv",
        "Andor/Season 1/Andor - S01E01.mkv",
    ]);
    assert!(!analyze_folder(shows.path()).unwrap().single_show);
    let films = tree(&["Heat (1995)/Heat (1995).mkv"]);
    assert!(!analyze_folder(films.path()).unwrap().single_show);
}

#[test]
fn the_video_with_the_most_tags_that_is_not_an_episode_is_kept_for_the_naming_example() {
    let dir = tree(&[
        "Zodiac (2007)/Zodiac.2007.1080p.mkv",
        "Alien (1979)/Alien.1979.720p.mkv",
        "Show/Season 1/Show - S01E01.mkv",
    ]);
    let found = analyze_folder(dir.path()).unwrap();
    // As many tags each: the first by name, as a full path so its NFO can be read.
    assert_eq!(
        found.first_video.as_deref().map(std::path::Path::new),
        Some(
            dir.path()
                .join("Alien (1979)/Alien.1979.720p.mkv")
                .as_path()
        )
    );
    // A name that says more wins over an earlier bare one.
    let tagged = tree(&[
        "Alien (1979)/Alien (1979).mkv",
        "Heat (1995)/Heat.1995.1080p.BluRay.DTS.5.1.mkv",
    ]);
    let found = analyze_folder(tagged.path()).unwrap();
    assert!(
        found
            .first_video
            .as_deref()
            .is_some_and(|path| path.ends_with("Heat.1995.1080p.BluRay.DTS.5.1.mkv")),
        "{:?}",
        found.first_video
    );
    let none = tree(&["Show/Season 1/Show - S01E01.mkv"]);
    assert_eq!(analyze_folder(none.path()).unwrap().first_video, None);
}
