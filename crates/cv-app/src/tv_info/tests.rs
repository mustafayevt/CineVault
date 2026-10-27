use std::collections::BTreeMap;

use cv_artwork::naming::TvNames;
use cv_domain::{AiredStatus, CalendarDate};
use cv_rename::naming::SeasonArt;
use serde_json::json;

use super::*;

fn date(year: i32, month: u8, day: u8) -> CalendarDate {
    CalendarDate::new(year, month, day).unwrap()
}

fn air(season: u32, y: i32, m: u8, d: u8) -> EpisodeAir {
    EpisodeAir {
        season,
        date: date(y, m, d),
    }
}

// ---- season artwork names ----------------------------------------------------------------------

#[test]
fn season_names_follow_the_season_number() {
    let names = TvNames::default();
    let targets =
        |art, season, ext| season_targets(names.season_choices(art), art, season, "", ext);
    let poster = |season| targets(SeasonArt::Poster, season, "jpg");
    assert_eq!(poster(1), ["season01-poster.jpg"]);
    assert_eq!(poster(12), ["season12-poster.jpg"]);
    // Specials: the scenario of the spec.
    assert_eq!(poster(0), ["season-specials-poster.jpg"]);
    assert_eq!(poster(-1), ["season-all-poster.jpg"]);

    assert_eq!(
        targets(SeasonArt::Fanart, 2, "png"),
        ["season02-fanart.png"]
    );
    assert_eq!(
        targets(SeasonArt::Banner, 0, ".JPG"),
        ["season-specials-banner.jpg"]
    );
    assert_eq!(targets(SeasonArt::Thumb, 3, ""), ["season03-thumb.jpg"]);
}

#[test]
fn season_names_can_sit_in_the_season_folder() {
    let choices: Vec<String> = [
        "SEASON_POSTER",
        "SEASON_FOLDER",
        "FOLDER",
        "folder",
        "BOGUS",
    ]
    .map(str::to_owned)
    .to_vec();
    let targets = season_targets(&choices, SeasonArt::Poster, 1, "Season 1", "jpg");
    assert_eq!(
        targets,
        [
            "season01-poster.jpg",
            "Season 1/season01-poster.jpg",
            "Season 1/folder.jpg",
        ],
        "known choices in order, duplicates collapsed, unknown ones ignored"
    );

    // Without a season folder the in-folder names stay in the show folder.
    let flat = season_targets(&choices, SeasonArt::Poster, 1, "", "jpg");
    assert_eq!(flat, ["season01-poster.jpg", "folder.jpg"]);

    // Specials in their own folder.
    let specials = season_targets(
        &["SEASON_FOLDER".to_owned()],
        SeasonArt::Fanart,
        0,
        "Specials",
        "jpg",
    );
    assert_eq!(specials, ["Specials/season-specials-fanart.jpg"]);

    // A poster-only choice does not name a fanart; an empty list writes nothing.
    assert!(season_targets(&["FOLDER".to_owned()], SeasonArt::Fanart, 1, "S", "jpg").is_empty());
    assert!(season_targets(&[], SeasonArt::Poster, 1, "", "jpg").is_empty());
}

// ---- show artwork fallback ---------------------------------------------------------------------

#[test]
fn a_season_without_artwork_falls_back_to_the_show_when_allowed() {
    let own = with_show_fallback(Some("season"), Some("show"), true).unwrap();
    assert_eq!((own.image, own.from_show), ("season", false));

    let fallback = with_show_fallback(None, Some("show"), true).unwrap();
    assert_eq!((fallback.image, fallback.from_show), ("show", true));

    assert_eq!(with_show_fallback(None, Some("show"), false), None);
    assert_eq!(with_show_fallback::<&str>(None, None, true), None);
    // The season's own image wins even when the fallback is off.
    assert!(with_show_fallback(Some(1), None, false).is_some());
}

#[test]
fn the_fallback_setting_is_on_unless_switched_off() {
    let mut values = BTreeMap::new();
    assert!(season_art_fallback(&values));
    values.insert(SEASON_ART_FALLBACK_KEY.to_owned(), json!(false));
    assert!(!season_art_fallback(&values));
    values.insert(SEASON_ART_FALLBACK_KEY.to_owned(), json!("no"));
    assert!(
        season_art_fallback(&values),
        "a value of the wrong type is ignored"
    );
}

// ---- air information ---------------------------------------------------------------------------

#[test]
fn a_running_show_has_a_last_and_a_next_air_date_but_no_end() {
    let episodes = [
        air(1, 2024, 1, 5),
        air(1, 2024, 1, 12),
        air(1, 2024, 1, 19),
        air(0, 2023, 12, 1),
    ];
    let info = air_info(AiredStatus::Continuing, None, &episodes, date(2024, 1, 15));
    assert_eq!(
        info.first_aired,
        Some(date(2024, 1, 5)),
        "specials do not start a show"
    );
    assert_eq!(info.last_aired, Some(date(2024, 1, 12)));
    assert_eq!(info.next_aired, Some(date(2024, 1, 19)));
    assert_eq!(info.end_date, None);
    assert_eq!(info.end_year, 0);
}

#[test]
fn an_ended_show_ends_with_its_last_regular_episode() {
    let episodes = [
        air(1, 2016, 7, 15),
        air(3, 2019, 5, 25),
        // A reunion special years later does not move the end.
        air(0, 2021, 1, 1),
    ];
    let info = air_info(
        AiredStatus::Ended,
        Some(date(2016, 7, 1)),
        &episodes,
        date(2024, 1, 1),
    );
    assert_eq!(
        info.first_aired,
        Some(date(2016, 7, 1)),
        "the provider's date wins"
    );
    assert_eq!(info.end_date, Some(date(2019, 5, 25)));
    assert_eq!(info.end_year, 2019);
    assert_eq!(info.next_aired, None);

    let canceled = air_info(AiredStatus::Canceled, None, &episodes, date(2024, 1, 1));
    assert_eq!(canceled.end_year, 2019);
}

#[test]
fn a_show_of_specials_only_uses_them_and_unknown_shows_have_no_end() {
    let info = air_info(
        AiredStatus::Ended,
        None,
        &[air(0, 2020, 1, 1), air(0, 2020, 6, 1)],
        date(2024, 1, 1),
    );
    assert_eq!(info.end_date, Some(date(2020, 6, 1)));

    let unknown = air_info(
        AiredStatus::Unknown,
        None,
        &[air(1, 2020, 1, 1)],
        date(2024, 1, 1),
    );
    assert_eq!(unknown.end_date, None);
    assert_eq!(
        air_info(AiredStatus::Ended, None, &[], date(2024, 1, 1)),
        AirInfo::default()
    );
}

// ---- duplicates --------------------------------------------------------------------------------

#[test]
fn numbers_claimed_twice_are_duplicates() {
    // S01E03.mkv and S01E03.REPACK.mkv, and a multi-episode file covering E04 and E05 beside
    // a single E05.
    let found = duplicate_numbers([(1, 1), (1, 2), (1, 3), (1, 3), (1, 4), (1, 5), (1, 5)]);
    assert_eq!(
        found.into_iter().collect::<Vec<_>>(),
        [((1, 3), 2), ((1, 5), 2)]
    );

    assert!(duplicate_numbers([(1, 1), (1, 2), (2, 1)]).is_empty());
    assert!(duplicate_numbers(Vec::<(u32, u32)>::new()).is_empty());
}
