//! Parser rules. The byte-exact fixtures live in `tests/compat` and run through
//! `cv-compat`; these tests pin the individual rules and the edge cases next to the code.

use cv_domain::{Edition, MediaSource};

use super::*;

const NO_WORDS: &[&str] = &[];

fn parse(name: &str) -> (String, Option<u16>) {
    let found = detect_clean_title_and_year_at(name, NO_WORDS, 2026);
    (found.title, found.year)
}

fn parse_with(name: &str, bad_words: &[&str]) -> (String, Option<u16>) {
    let found = detect_clean_title_and_year_at(name, bad_words, 2026);
    (found.title, found.year)
}

fn is(name: &str, title: &str, year: Option<u16>) {
    assert_eq!(parse(name), (title.to_owned(), year), "{name}");
}

#[test]
fn year_in_parentheses_brackets_or_dots() {
    is("Heat (1995).mkv", "Heat", Some(1995));
    is("Heat.1995.mkv", "Heat", Some(1995));
    is("Heat [1995].mkv", "Heat", Some(1995));
    is("Heat - 1995.mkv", "Heat", Some(1995));
    is("Heat.mkv", "Heat", None);
}

#[test]
fn the_first_token_is_never_the_year() {
    is("1917 (2019).mkv", "1917", Some(2019));
    is("2010 Movie Title.mkv", "2010 Movie Title", None);
    is("(2010) Movie Title.mkv", "2010 Movie Title", None);
}

#[test]
fn the_last_plausible_year_wins_and_earlier_numbers_stay_in_the_title() {
    is(
        "Blade Runner 2049 (2017).mkv",
        "Blade Runner 2049",
        Some(2017),
    );
    is("Movie.Title.1998.2008.mkv", "Movie Title 1998", Some(2008));
}

#[test]
fn year_limits() {
    is("Movie Title (1800).mkv", "Movie Title 1800", None);
    is("Movie Title (1801).mkv", "Movie Title", Some(1801));
    is("Movie Title (2030).mkv", "Movie Title", Some(2030));
    is("Movie Title (2031).mkv", "Movie Title 2031", None);
    let in_2040 = detect_clean_title_and_year_at("Movie Title (2044)", NO_WORDS, 2040);
    assert_eq!(in_2040.year, Some(2044));
}

#[test]
fn release_tags_end_the_title_from_the_third_token() {
    is(
        "The.Matrix.1999.1080p.BluRay.x264-GRP.mkv",
        "The Matrix",
        Some(1999),
    );
    is("Movie Title.1080p.mkv", "Movie Title", None);
    is("1080p.mkv", "1080p", None);
    is("Saw VII 3D (2010).mkv", "Saw VII", Some(2010));
}

#[test]
fn resolution_and_clean_words_are_removed() {
    is("Movie Title 1280x720 2010.mkv", "Movie Title", Some(2010));
    is("Movie Title 1920x1080.mkv", "Movie Title", None);
    is("Movie.Title.2010.WEB-DL.mkv", "Movie Title", Some(2010));
    is("Movie.Title.2010.24.000.mkv", "Movie Title", Some(2010));
}

#[test]
fn release_tags_with_an_inner_delimiter_never_reach_the_title() {
    is("Movie.Title.WEB-DL.mkv", "Movie Title", None);
    is("Movie.Title.Blu-ray.Remux.mkv", "Movie Title", None);
    is("Movie Title DVD-Rip.avi", "Movie Title", None);
    is("Movie.Title.H.264.DDP5.1.mkv", "Movie Title", None);
    is("Movie.Title.x.265.AAC2.0.mkv", "Movie Title", None);
    is("Movie.Title.DTS-HD.23.976.mkv", "Movie Title", None);
    is("Movie.Title.29.94.mkv", "Movie Title", None);
    // Only whole tags go: a word that merely starts like one stays.
    is("Blu-Rayders.2010.mkv", "Blu Rayders", Some(2010));
    is("The.Web-Dlx.Story.mkv", "The Web Dlx Story", None);
    // The misspelled source tag is not a known tag.
    is("Movie.Title.Blue-Ray.mkv", "Movie Title Blue Ray", None);
}

#[test]
fn brackets_braces_and_the_year_in_optionals() {
    is(
        "[GRP] Movie Title (2010) [1080p].mkv",
        "Movie Title",
        Some(2010),
    );
    is("Movie.Title.[2010].720p.mkv", "Movie Title", Some(2010));
    is("Movie {edition-Extended} (2010).mkv", "Movie", Some(2010));
    is(
        "[GRP] Movie Title [BD 1080p x265 10bit FLAC].mkv",
        "Movie Title",
        None,
    );
}

#[test]
fn editions_before_the_year_are_removed() {
    is(
        "Movie.Title.Director's.Cut.2010.1080p.mkv",
        "Movie Title",
        Some(2010),
    );
    is(
        "Movie Title Extended Edition 2010.mkv",
        "Movie Title",
        Some(2010),
    );
}

#[test]
fn edition_detection() {
    assert_eq!(
        detect_edition("Movie (2010) - Directors Cut.mkv"),
        Edition::DirectorsCut
    );
    assert_eq!(
        detect_edition("Movie.2010.Extended.Cut.mkv"),
        Edition::ExtendedEdition
    );
    assert_eq!(detect_edition("Movie.2010.IMAX.mkv"), Edition::Imax);
    assert_eq!(
        detect_edition("Movie.2010.FINAL.CUT.mkv"),
        Edition::FinalCut
    );
    assert_eq!(detect_edition("Movie (2010)"), Edition::None);
    assert_eq!(
        detect_edition("Movie {edition-Ultimate Archive} (2010)"),
        Edition::Other("Ultimate Archive".into())
    );
    assert_eq!(
        detect_edition("Movie {edition-Remastered} (2010)"),
        Edition::Remastered
    );
}

#[test]
fn roman_numerals_are_upper_cased() {
    is("Final Destination vi.mkv", "Final Destination VI", None);
    is("Rocky iv 1985.mkv", "Rocky IV", Some(1985));
}

#[test]
fn trailing_aka_is_dropped() {
    is(
        "Movie Title aka Other Title (2010).mkv",
        "Movie Title",
        Some(2010),
    );
}

#[test]
fn video_ts_stays_one_word() {
    is("VIDEO_TS", "VIDEO_TS", None);
    is("Movie Title VIDEO_TS", "Movie Title VIDEO_TS", None);
    is("Movie Title 2010 DVD VIDEO_TS", "Movie Title", Some(2010));
}

#[test]
fn tv_recording_marker_is_cut_after_character_ten() {
    is(
        "Movie Title_16.09.15_21-00_ard_30_TVOON_DE.mpg.avi",
        "Movie Title",
        None,
    );
}

#[test]
fn bad_words_are_regexes_applied_to_the_whole_name() {
    assert_eq!(
        parse_with("Movie Title GRP (2010).mkv", &["grp"]),
        ("Movie Title".to_owned(), Some(2010))
    );
    assert_eq!(
        parse_with("[Release] Movie Title (2010).mkv", &["\\[release\\]"]),
        ("Movie Title".to_owned(), Some(2010))
    );
    assert_eq!(
        parse_with("Movie Title Extra Tag (2010).mkv", &["extra tag", "foo"]),
        ("Movie Title".to_owned(), Some(2010))
    );
}

#[test]
fn a_bad_word_that_empties_the_name_is_ignored() {
    assert_eq!(parse_with("Heat.mkv", &["heat"]), ("Heat".to_owned(), None));
}

#[test]
fn an_invalid_bad_word_regex_is_matched_literally() {
    assert_eq!(
        parse_with("Movie Title [x (2010).mkv", &["[x"]),
        ("Movie Title".to_owned(), Some(2010))
    );
}

#[test]
fn folder_names_keep_a_trailing_year_after_a_dot() {
    let file = detect_clean_title_and_year_at("Heat.1995", NO_WORDS, 2026);
    assert_eq!((file.title.as_str(), file.year), ("Heat", None));
    let folder = detect_clean_title_and_year_of_folder("Heat.1995", NO_WORDS);
    assert_eq!((folder.title.as_str(), folder.year), ("Heat", Some(1995)));
}

#[test]
fn empty_names_give_an_empty_result() {
    assert_eq!(parse(""), (String::new(), None));
    assert_eq!(parse("   "), (String::new(), None));
}

#[test]
fn tv_stop_words_need_delimiters_on_both_sides() {
    assert_eq!(
        remove_stop_and_bad_words_tv("Show.Name.hdtv.x264", NO_WORDS),
        "Show.Name."
    );
    assert_eq!(
        remove_stop_and_bad_words_tv("Hdtvshow.Name", NO_WORDS),
        "Hdtvshow.Name"
    );
    assert_eq!(
        remove_stop_and_bad_words_tv("Show Name grp", &["grp"]),
        "Show Name "
    );
}

#[test]
fn ids_from_names_urls_and_nfo_text() {
    let ids = detect_ids("Movie [imdbid-tt0103639] [tmdbid-812] [tvdbid-55]");
    assert_eq!(ids.imdb.as_deref(), Some("tt0103639"));
    assert_eq!(ids.tmdb, Some(812));
    assert_eq!(ids.tvdb.as_deref(), Some("55"));

    assert_eq!(
        detect_ids("https://www.imdb.com/Title?0103639")
            .imdb
            .as_deref(),
        Some("tt0103639")
    );
    assert_eq!(
        detect_ids("https://www.themoviedb.org/tv/1396-breaking-bad").tmdb,
        Some(1396)
    );
    assert_eq!(
        detect_ids("https://thetvdb.com/movies/12345")
            .tvdb
            .as_deref(),
        Some("12345")
    );
}

#[test]
fn ids_that_are_too_short_zero_or_too_large_are_missing() {
    assert_eq!(detect_ids("tt12345"), ParsedIds::default());
    assert_eq!(detect_ids("Movie [tmdbid-0]").tmdb, None);
    assert_eq!(detect_ids("Movie [tmdbid-123456789012]").tmdb, None);
    assert_eq!(detect_ids("Movie tmdbid-abc"), ParsedIds::default());
    assert_eq!(detect_ids(""), ParsedIds::default());
}

#[test]
fn the_first_id_of_a_kind_wins() {
    assert_eq!(
        detect_ids("Movie tt1234567 and tt7654321").imdb.as_deref(),
        Some("tt1234567")
    );
}

#[test]
fn id_keywords_ignore_case_and_accept_every_separator() {
    for name in [
        "tmdb 12345",
        "tmdb.12345",
        "tmdb_12345",
        "tmdb=12345",
        "{tmdb-12345}",
        "[TMDBID-12345]",
    ] {
        assert_eq!(detect_ids(name).tmdb, Some(12345), "{name}");
    }
    assert_eq!(detect_ids("https://thetvdb.com/series/81189").tmdb, None);
}

#[test]
fn media_source_from_release_names() {
    let source = |name| detect_media_source(name);
    assert_eq!(
        source("Dune.Part.Two.2024.2160p.WEB-DL.DV.HDR10.TrueHD.Atmos.7.1-GRP"),
        MediaSource::WebDl
    );
    assert_eq!(
        source("Movie.2010.1080p.BluRay.x264.mkv"),
        MediaSource::Bluray
    );
    assert_eq!(
        source("Movie.2010.UHD.BluRay.2160p.mkv"),
        MediaSource::UhdBluray
    );
    assert_eq!(source("Movie.2010.WEBRip.x265.mkv"), MediaSource::WebRip);
    assert_eq!(source("Movie.Title.2010.DVDRip.XviD.avi"), MediaSource::Dvd);
    assert_eq!(source("Movie.2010.HDTV.x264.mkv"), MediaSource::Tv);
    assert_eq!(source("Movie.Title.2010.mkv"), MediaSource::Unknown);
    assert_eq!(source("Tiger.Movie.mkv"), MediaSource::Unknown);
}
