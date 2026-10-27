//! Episode parser tests. The byte-exact fixtures live in `tests/compat` (`episodes`
//! family) and run through `cv-compat`; these tests pin the rules and the edge cases next to the
//! code.

use super::*;

fn parse(path: &str) -> EpisodeParse {
    detect_episode_from_filename(path, "Show Name")
}

fn numbers(path: &str) -> (Option<u32>, Vec<u32>) {
    let found = parse(path);
    (found.season, found.episodes)
}

#[test]
fn season_and_episode_markers() {
    assert_eq!(
        numbers("Show Name - S01E02 - Title.mkv"),
        (Some(1), vec![2])
    );
    assert_eq!(numbers("Show.Name.s10e15.mkv"), (Some(10), vec![15]));
    assert_eq!(numbers("Show Name S2024E100.mkv"), (Some(2024), vec![100]));
    assert_eq!(numbers("Show Name - 1x02.mkv"), (Some(1), vec![2]));
    assert_eq!(numbers("Show Name S01_E02.mkv"), (Some(1), vec![2]));
}

#[test]
fn multi_episode_files() {
    assert_eq!(numbers("Show Name S01E02E03.mkv"), (Some(1), vec![2, 3]));
    assert_eq!(numbers("Show Name S01E02-E03.mkv"), (Some(1), vec![2, 3]));
    assert_eq!(numbers("Show Name 1x02x03.mkv"), (Some(1), vec![2, 3]));
    assert_eq!(
        numbers("Show Name 01x02 - 01x03.mkv"),
        (Some(1), vec![2, 3])
    );
}

#[test]
fn ranges_fill_up_to_five_episodes() {
    assert_eq!(
        numbers("Show Name S01E05-E07.mkv"),
        (Some(1), vec![5, 6, 7])
    );
    assert_eq!(numbers("Show Name S01E05-07.mkv"), (Some(1), vec![5, 6, 7]));
    assert_eq!(
        numbers("Show Name S01E05-E10.mkv"),
        (Some(1), vec![5, 6, 7, 8, 9, 10])
    );
    assert_eq!(numbers("Show Name S01E05-E20.mkv"), (Some(1), vec![5, 20]));
}

#[test]
fn season_words_in_other_languages() {
    assert_eq!(
        numbers("Show Name Staffel 2 Folge 5.mkv"),
        (Some(2), vec![5])
    );
    assert_eq!(
        numbers("Show Name Saison 2 Episode 5.mkv"),
        (Some(2), vec![5])
    );
    assert_eq!(
        numbers("Show Name Temporada 2 Episodio 5.mkv"),
        (Some(2), vec![5])
    );
}

#[test]
fn bare_numbers() {
    assert_eq!(numbers("Show Name 102.mkv"), (Some(1), vec![2]));
    assert_eq!(numbers("Show Name 215.mkv"), (Some(2), vec![15]));
    assert_eq!(numbers("Show Name - 05.mkv"), (Some(1), vec![5]));
    // four digits count only when the season is known
    assert_eq!(numbers("Show Name 1002.mkv"), (None, vec![]));
    assert_eq!(numbers("Show Name S10 1002.mkv"), (Some(10), vec![2]));
    // the last number of one length wins
    assert_eq!(numbers("Show Name 3.1.mkv"), (Some(1), vec![1]));
}

#[test]
fn numbers_in_the_show_name_are_not_episodes() {
    let found = detect_episode_from_filename("24 - 1x05.mkv", "24");
    assert_eq!((found.season, found.episodes), (Some(1), vec![5]));
    let found = detect_episode_from_filename("9-1-1 S01E02.mkv", "9-1-1");
    assert_eq!((found.season, found.episodes), (Some(1), vec![2]));
    let found = detect_episode_from_filename("The.100.102.mkv", "The 100");
    assert_eq!((found.season, found.episodes), (Some(1), vec![2]));
}

#[test]
fn roman_parts_and_specials() {
    assert_eq!(numbers("Show Name Part IV.mkv"), (Some(1), vec![4]));
    assert_eq!(numbers("Show Name Pt. IX.mkv"), (Some(1), vec![9]));
    assert_eq!(numbers("Show Name OVA 2.mkv"), (Some(0), vec![2]));
    assert_eq!(numbers("Show Name S00E01 Special.mkv"), (Some(0), vec![1]));
}

#[test]
fn dates_set_the_season_to_the_year() {
    let found = parse("Show Name 2020-05-17.mkv");
    assert_eq!(
        found.date.map(|d| d.to_string()).as_deref(),
        Some("2020-05-17")
    );
    assert_eq!(
        (found.season, found.episodes.is_empty()),
        (Some(2020), true)
    );
    let found = parse("Show Name 17-05-2020.mkv");
    assert_eq!(
        found.date.map(|d| d.to_string()).as_deref(),
        Some("2020-05-17")
    );
    let found = parse("Show Name S02 2020-05-17.mkv");
    assert_eq!(found.season, Some(2));
    assert_eq!(found.date.map(|d| d.year), Some(2020));
}

#[test]
fn out_of_range_dates_roll_over() {
    assert_eq!(
        AiredDate::new_lenient(2020, 13, 45).to_string(),
        "2021-02-14"
    );
    assert_eq!(
        AiredDate::new_lenient(2020, 2, 30).to_string(),
        "2020-03-01"
    );
    assert_eq!(
        AiredDate::new_lenient(2021, 2, 29).to_string(),
        "2021-03-01"
    );
    assert_eq!(
        AiredDate::new_lenient(2020, 12, 31).to_string(),
        "2020-12-31"
    );
}

#[test]
fn anime_with_hash() {
    let found = parse("[GRP] Show Name - 105 [1080p][12345678].mkv");
    assert_eq!((found.season, found.episodes), (Some(1), vec![105]));
    assert!(found.cleaned_title.is_empty());
    let found = parse("[GRP] Show Name - OVA 02 [ABCD1234].mkv");
    assert_eq!((found.season, found.episodes), (Some(0), vec![2]));
    let found = parse("[GRP] Show Name - Picture Drama 01 [ABCD1234].mkv");
    assert_eq!((found.season, found.episodes), (Some(0), vec![1]));
}

#[test]
fn season_comes_from_the_folders_when_the_name_has_none() {
    assert_eq!(numbers("Season 2/Show Name - 03.mkv"), (Some(2), vec![3]));
    assert_eq!(numbers("S02/03.mkv"), (Some(2), vec![3]));
    assert_eq!(numbers("Show Name/Season 3/04.mkv"), (Some(3), vec![4]));
    // the name wins over the folder
    assert_eq!(numbers("Season 1/Show Name S02E03.mkv"), (Some(2), vec![3]));
    // a folder is a season only after a delimiter
    assert_eq!(numbers("Show Name/S03/04 - Title.mkv"), (Some(1), vec![4]));
}

#[test]
fn disc_structures_are_read_from_their_folders() {
    let found = parse("Show Name S01E03/BDMV/index.bdmv");
    assert_eq!((found.season, found.episodes), (Some(1), vec![3]));
    assert!(found.cleaned_title.is_empty());
    let found = parse("Season 1/Episode 3/VIDEO_TS/VIDEO_TS.IFO");
    assert_eq!((found.season, found.episodes), (Some(1), vec![]));
}

#[test]
fn stacking_markers_are_reported() {
    assert!(parse("Show Name S01E02 cd1.avi").stacking_marker_found);
    assert!(parse("Show Name S01E02 part1.avi").stacking_marker_found);
    assert!(!parse("Show Name S01E02.avi").stacking_marker_found);
}

#[test]
fn nothing_found() {
    let found = parse("Show Name - Title.mkv");
    assert_eq!(
        (found.season, found.episodes.is_empty(), found.date),
        (None, true, None)
    );
    assert_eq!(found.cleaned_title, "Title");
}

#[test]
fn display_is_compact() {
    assert_eq!(parse("Show Name S01E02E03.mkv").to_string(), "S01E02E03");
    assert_eq!(
        parse("Show Name 2020-05-17.mkv").to_string(),
        "S2020 2020-05-17"
    );
}

#[test]
fn bad_words_are_removed_from_titles() {
    let found = parse_episode("Show Name - S01E02 - Title GRP.mkv", "Show Name", &["GRP"]);
    assert_eq!(found.cleaned_title, "Title");
}

#[test]
fn titles_keep_what_is_not_a_marker() {
    assert_eq!(
        clean_episode_title(
            "Show Name - S01E02 - A Fine Title.mkv",
            "Show Name",
            &[] as &[&str]
        ),
        "A Fine Title"
    );
    // only a marker: the marker is the title
    assert_eq!(
        clean_episode_title("Show Name S01E02.mkv", "Show Name", &[] as &[&str]),
        "S01E02"
    );
    // only the show name and tags: nothing
    assert_eq!(
        clean_episode_title("Show Name 1080p.mkv", "Show Name", &[] as &[&str]),
        ""
    );
    // a year in brackets goes, the show name only at the start
    assert_eq!(
        clean_episode_title("Show Name - Title (2015).mkv", "Show Name", &[] as &[&str]),
        "Title"
    );
}

/// reg2-7: a file name that spells the show without its accents (`Amelie` for `Amélie`) is still
/// the show name, not a title.
#[test]
fn the_show_name_is_found_without_its_accents() {
    let none: &[&str] = &[];
    assert_eq!(
        clean_episode_title("Amelie S01E01.mkv", "Amélie", none),
        "S01E01"
    );
    assert_eq!(
        clean_episode_title("Amélie S01E01.mkv", "Amelie", none),
        "S01E01"
    );
    assert_eq!(clean_episode_title("AMELIE 1080p.mkv", "Amélie", none), "");
    assert_eq!(
        clean_episode_title("Amelie - S01E02 - Le Café.mkv", "Amélie", none),
        "Le Café",
        "the title keeps its own accents"
    );
    let found = parse_episode("Amelie S01E03.mkv", "Amélie", none);
    assert_eq!((found.season, found.episodes), (Some(1), vec![3]));
}

#[test]
fn roman_values() {
    assert_eq!(roman_value("IV"), Some(4));
    assert_eq!(roman_value("xix"), Some(19));
    assert_eq!(roman_value("MCM"), Some(1900));
    assert_eq!(roman_value("ABC"), None);
}

// --- absolute order -----------------------------------------------------------

fn absolute(path: &str) -> Option<u32> {
    parse_absolute_episode::<&str>(path, "Show Name", &[])
}

#[test]
fn absolute_numbers_are_not_split_into_season_and_episode() {
    // The regular parser reads three digits as a season and an episode.
    assert_eq!(numbers("Show Name - 137.mkv"), (Some(1), vec![37]));
    assert_eq!(absolute("Show Name - 137.mkv"), Some(137));
    assert_eq!(absolute("Show Name 1052.mkv"), Some(1052));
    assert_eq!(absolute("Show.Name.137.1080p.BluRay.x264.mkv"), Some(137));
    assert_eq!(absolute("Show Name - 05.mkv"), Some(5));
    assert_eq!(absolute("Show Name - 137v2 - A Title.mkv"), Some(137));
    assert_eq!(absolute("Season 1/Show Name - 137.mkv"), Some(137));
    assert_eq!(absolute(r"Show Name\Show Name - 137.mkv"), Some(137));
}

#[test]
fn absolute_numbers_in_anime_release_names() {
    assert_eq!(
        absolute("[Group] Show Name - 137 [1080p][ABCD1234].mkv"),
        Some(137)
    );
    assert_eq!(absolute("[Group] Show Name - 137 (720p).mkv"), Some(137));
    assert_eq!(
        absolute("[Group] Show Name - 137v2 [ABCD1234].mkv"),
        Some(137)
    );
    assert_eq!(absolute("Show Name - Episode 137.mkv"), Some(137));
    assert_eq!(absolute("Show Name E137.mkv"), Some(137));
}

#[test]
fn absolute_numbers_prefer_the_number_after_a_dash() {
    assert_eq!(absolute("Show Name 2 - 05.mkv"), Some(5));
    assert_eq!(absolute("Show Name - 12 - Title 2.mkv"), Some(12));
}

#[test]
fn the_show_name_is_not_an_absolute_number() {
    assert_eq!(
        parse_absolute_episode::<&str>("The 100 - 07.mkv", "The 100", &[]),
        Some(7)
    );
    assert_eq!(
        parse_absolute_episode::<&str>("The 100.mkv", "The 100", &[]),
        None
    );
}

#[test]
fn names_without_an_absolute_number_give_none() {
    assert_eq!(absolute("Show Name - S02E05.mkv"), None);
    assert_eq!(absolute("Show Name - 2x05.mkv"), None);
    assert_eq!(absolute("Show Name - OVA 2.mkv"), None);
    assert_eq!(absolute("[Group] Show Name - OVA 2 [ABCD1234].mkv"), None);
    assert_eq!(absolute("Show Name - 2020-05-17.mkv"), None);
    assert_eq!(absolute("Show Name - Title.mkv"), None);
    assert_eq!(absolute("Show Name/VIDEO_TS/VTS_01_1.VOB"), None);
}

/// The title of an episode as the scan stores it, from its file name.
fn file_title(path: &str, show: &str) -> String {
    let none: &[&str] = &[];
    let found = parse_episode(path, show, none);
    file_episode_title(
        &found.cleaned_title,
        path,
        show,
        found.season,
        &found.episodes,
    )
}

/// reg3-1: the release group, the show name (or a part of it) and the episode number are no
/// title, so an episode named only by them has none.
#[test]
fn a_file_name_without_a_title_gives_no_title() {
    assert_eq!(file_title("Dark - S01E01.mkv", "Dark"), "");
    assert_eq!(file_title("S01E01.mkv", "Dark"), "");
    assert_eq!(file_title("Dark 101.mkv", "Dark"), "");
    assert_eq!(file_title("Season 1/03.mkv", "Dark"), "");
    assert_eq!(file_title("Amelie S01E01.mkv", "Amélie Show"), "");
    assert_eq!(file_title("Show S01E01.mkv", "Ünïcødé Show"), "");
    assert_eq!(
        file_title("Dark.S01E01.720p.HDTV.x264-KILLERS.mkv", "Dark"),
        ""
    );
    assert_eq!(
        file_title("[SubGroup] One Piece - 003 [1080p].mkv", "One Piece"),
        ""
    );
    // An absolute-numbered show: the scan settles the number (1001) before the title.
    let none: &[&str] = &[];
    let name = "[SubGroup] One Piece - 1001 [1080p].mkv";
    let cleaned = parse_episode(name, "One Piece", none).cleaned_title;
    assert_eq!(
        cleaned, "SubGroup One Piece 1001",
        "the compatible text is unchanged"
    );
    assert_eq!(
        file_episode_title(&cleaned, name, "One Piece", Some(1), &[1001]),
        ""
    );
}

/// reg3-1: a real title stays, without the group, the show name and the number around it.
#[test]
fn a_file_name_title_keeps_its_words() {
    assert_eq!(file_title("Dark - S01E01 - Secrets.mkv", "Dark"), "Secrets");
    assert_eq!(
        file_title("Dark.S01E01.Secrets.720p.HDTV.x264-KILLERS.mkv", "Dark"),
        "Secrets"
    );
    assert_eq!(
        file_title("[Grp] Dark - 05 - Truths [720p].mkv", "Dark"),
        "Truths"
    );
    assert_eq!(
        file_title("Show.S01E01.Spider-Man.mkv", "Show"),
        "Spider Man",
        "a dash inside the title is no release group"
    );
    assert_eq!(
        file_title("Amelie - S01E02 - Le Café.mkv", "Amélie"),
        "Le Café"
    );
    assert_eq!(
        file_title("Doctor - S20E01 - The 5 Doctors.mkv", "Doctor"),
        "The 5 Doctors"
    );
    assert_eq!(
        file_title("Show - S01E03 - 1984.mkv", "Show"),
        "1984",
        "a number that is not the episode's is a title"
    );
}
