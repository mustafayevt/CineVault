#![allow(clippy::field_reassign_with_default)]

use serde_json::json;

use super::*;

fn values(pairs: &[(&str, Value)]) -> Values {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), v.clone()))
        .collect()
}

#[test]
fn kodi_defaults_name_the_files_after_the_video() {
    let names = MovieNames::default();
    let t = |art| names.targets(art, "Heat (1995)", "", FolderKind::Single);
    assert_eq!(t(MovieArt::Poster), ["Heat (1995)-poster.jpg"]);
    assert_eq!(t(MovieArt::Fanart), ["Heat (1995)-fanart.jpg"]);
    assert_eq!(t(MovieArt::Clearlogo), ["Heat (1995)-clearlogo.png"]);
    assert_eq!(t(MovieArt::Thumb), ["Heat (1995)-landscape.jpg"]);
    assert_eq!(t(MovieArt::Clearart), ["Heat (1995)-clearart.png"]);
    assert_eq!(t(MovieArt::Discart), ["Heat (1995)-discart.png"]);
    assert_eq!(t(MovieArt::Banner), ["Heat (1995)-banner.jpg"]);
    assert_eq!(t(MovieArt::Keyart), ["Heat (1995)-keyart.jpg"]);
    assert_eq!(t(MovieArt::Squareart), ["Heat (1995)-squareart.jpg"]);
}

#[test]
fn the_extension_follows_the_image() {
    let names = MovieNames::default();
    assert_eq!(
        names.targets(MovieArt::Poster, "Heat", ".PNG", FolderKind::Single),
        ["Heat-poster.png"]
    );
    assert_eq!(
        names.targets(MovieArt::Clearlogo, "Heat", "webp", FolderKind::Single),
        ["Heat-clearlogo.webp"]
    );
}

#[test]
fn a_multi_movie_folder_forces_names_built_on_the_video() {
    let mut names = MovieNames::default();
    names.poster = vec!["POSTER".to_owned()];
    // Only a fixed name is configured: the filename-based default takes its place.
    assert_eq!(
        names.targets(
            MovieArt::Poster,
            "Heat (1995)",
            "jpg",
            FolderKind::MultiMovie
        ),
        ["Heat (1995)-poster.jpg"]
    );
    // In its own folder the fixed name stays.
    assert_eq!(
        names.targets(MovieArt::Poster, "Heat (1995)", "jpg", FolderKind::Single),
        ["poster.jpg"]
    );
    // Both configured: only the filename-based one remains.
    names.poster = vec!["POSTER".to_owned(), "FILENAME_POSTER".to_owned()];
    assert_eq!(
        names.targets(
            MovieArt::Poster,
            "Heat (1995)",
            "jpg",
            FolderKind::MultiMovie
        ),
        ["Heat (1995)-poster.jpg"]
    );
    assert_eq!(
        names.targets(MovieArt::Poster, "Heat (1995)", "jpg", FolderKind::Single),
        ["poster.jpg", "Heat (1995)-poster.jpg"]
    );
}

#[test]
fn a_type_with_no_names_stays_off_in_every_folder() {
    let mut names = MovieNames::default();
    names.banner.clear();
    for folder in [FolderKind::Single, FolderKind::MultiMovie, FolderKind::Disc] {
        assert!(
            names
                .targets(MovieArt::Banner, "Heat", "jpg", folder)
                .is_empty()
        );
    }
}

#[test]
fn a_disc_structure_prefers_fixed_names() {
    let names = MovieNames::default();
    let t = |art| names.targets(art, "VIDEO_TS", "", FolderKind::Disc);
    assert_eq!(t(MovieArt::Poster), ["poster.jpg"]);
    assert_eq!(t(MovieArt::Fanart), ["fanart.jpg"]);
    assert_eq!(t(MovieArt::Thumb), ["landscape.jpg"]);
    assert_eq!(t(MovieArt::Clearlogo), ["clearlogo.png"]);
    // A fixed name is already what the disc wants; the video name form of the poster is kept
    // only when the profile asks for exactly that.
    let mut names = MovieNames::default();
    names.poster = vec!["FOLDER".to_owned(), "FILENAME_POSTER".to_owned()];
    assert_eq!(
        names.targets(MovieArt::Poster, "VIDEO_TS", "jpg", FolderKind::Disc),
        ["folder.jpg", "poster.jpg"]
    );
}

#[test]
fn several_names_all_receive_the_image_in_list_order() {
    let mut names = MovieNames::default();
    names.poster = ["FILENAME_POSTER", "POSTER", "FOLDER", "COVER", "MOVIE"]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    assert_eq!(
        names.targets(MovieArt::Poster, "Heat", "jpg", FolderKind::Single),
        [
            "Heat-poster.jpg",
            "poster.jpg",
            "folder.jpg",
            "cover.jpg",
            "movie.jpg"
        ]
    );
}

#[test]
fn unknown_choices_and_duplicates_are_ignored() {
    let mut names = MovieNames::default();
    names.fanart = ["FILENAME_FANART", "bogus", "filename_fanart", "FANART"]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    assert_eq!(
        names.targets(MovieArt::Fanart, "Heat", "jpg", FolderKind::Single),
        ["Heat-fanart.jpg", "fanart.jpg"]
    );
}

#[test]
fn every_choice_of_the_naming_tables_builds_a_name() {
    let table: &[(MovieArt, &[(&str, &str)])] = &[
        (
            MovieArt::Poster,
            &[
                ("FILENAME", "B.jpg"),
                ("FILENAME_POSTER", "B-poster.jpg"),
                ("MOVIE", "movie.jpg"),
                ("POSTER", "poster.jpg"),
                ("FOLDER", "folder.jpg"),
                ("COVER", "cover.jpg"),
            ],
        ),
        (
            MovieArt::Fanart,
            &[
                ("FILENAME_FANART", "B-fanart.jpg"),
                ("FILENAME_FANART2", "B.fanart.jpg"),
                ("FANART", "fanart.jpg"),
                ("FILENAME_BACKDROP", "B-backdrop.jpg"),
                ("FILENAME_BACKDROP2", "B.backdrop.jpg"),
                ("BACKDROP", "backdrop.jpg"),
                ("FILENAME_BACKGROUND", "B-background.jpg"),
                ("FILENAME_BACKGROUND2", "B.background.jpg"),
                ("BACKGROUND", "background.jpg"),
            ],
        ),
        (
            MovieArt::Thumb,
            &[
                ("FILENAME_THUMB", "B-thumb.jpg"),
                ("FILENAME_LANDSCAPE", "B-landscape.jpg"),
                ("THUMB", "thumb.jpg"),
                ("LANDSCAPE", "landscape.jpg"),
            ],
        ),
        (
            MovieArt::Clearlogo,
            &[
                ("FILENAME_CLEARLOGO", "B-clearlogo.png"),
                ("CLEARLOGO", "clearlogo.png"),
                ("FILENAME_LOGO", "B-logo.png"),
                ("LOGO", "logo.png"),
            ],
        ),
        (
            MovieArt::Discart,
            &[
                ("FILENAME_DISC", "B-disc.png"),
                ("DISC", "disc.png"),
                ("FILENAME_DISCART", "B-discart.png"),
                ("DISCART", "discart.png"),
            ],
        ),
        (
            MovieArt::Squareart,
            &[
                ("FILENAME_SQUAREART", "B-squareart.jpg"),
                ("SQUAREART", "squareart.jpg"),
                ("FILENAME_SQUARE", "B-square.jpg"),
                ("SQUARE", "square.jpg"),
                ("BACKGROUND_SQUARE", "backgroundSquare.jpg"),
            ],
        ),
    ];
    for (art, choices) in table {
        for (choice, expected) in *choices {
            let mut names = MovieNames::default();
            *names.choices_mut(*art) = vec![(*choice).to_owned()];
            assert_eq!(
                names.targets(*art, "B", "", FolderKind::Single),
                [*expected],
                "{choice}"
            );
        }
    }
}

#[test]
fn extra_fanart_uses_the_first_choice_with_numbers_from_one() {
    let mut names = MovieNames::default();
    assert_eq!(
        names.extra_fanart_targets("Heat", "jpg", FolderKind::Single, 3),
        ["Heat-fanart1.jpg", "Heat-fanart2.jpg", "Heat-fanart3.jpg"]
    );
    assert_eq!(names.extra_fanart_folder(FolderKind::Single), None);

    names.extra_fanart = vec!["FOLDER_EXTRAFANART".to_owned(), "EXTRAFANART".to_owned()];
    assert_eq!(
        names.extra_fanart_targets("Heat", "jpg", FolderKind::Single, 2),
        ["extrafanart/fanart1.jpg", "extrafanart/fanart2.jpg"]
    );
    assert_eq!(
        names.extra_fanart_folder(FolderKind::Single),
        Some("extrafanart")
    );
    assert!(
        names
            .extra_fanart_targets("Heat", "jpg", FolderKind::Single, 0)
            .is_empty()
    );
}

#[test]
fn extra_fanart_and_thumbs_are_not_written_for_a_multi_movie_folder() {
    let names = MovieNames::default();
    assert!(
        names
            .extra_fanart_targets("Heat", "jpg", FolderKind::MultiMovie, 5)
            .is_empty()
    );
    assert_eq!(names.extra_fanart_folder(FolderKind::MultiMovie), None);
    assert!(
        names
            .extra_thumb_targets(FolderKind::MultiMovie, "jpg", 5)
            .is_empty()
    );
}

#[test]
fn extra_thumbs_go_to_their_folder() {
    let names = MovieNames::default();
    assert_eq!(
        names.extra_thumb_targets(FolderKind::Single, "jpg", 2),
        ["extrathumbs/thumb1.jpg", "extrathumbs/thumb2.jpg"]
    );
    assert_eq!(
        names.extra_thumb_targets(FolderKind::Disc, "png", 1),
        ["extrathumbs/thumb1.png"]
    );
}

#[test]
fn extra_fanart_on_a_disc_uses_fixed_names() {
    let names = MovieNames::default();
    assert_eq!(
        names.extra_fanart_targets("VIDEO_TS", "jpg", FolderKind::Disc, 2),
        ["fanart1.jpg", "fanart2.jpg"]
    );
}

#[test]
fn an_empty_video_name_drops_the_names_built_on_it() {
    let mut names = MovieNames::default();
    names.poster = vec!["FILENAME_POSTER".to_owned(), "POSTER".to_owned()];
    assert_eq!(
        names.targets(MovieArt::Poster, "", "jpg", FolderKind::Single),
        ["poster.jpg"]
    );
}

#[test]
fn presets_fill_every_list() {
    let jellyfin = ArtworkPreset::Jellyfin.movie_names();
    assert_eq!(
        jellyfin.targets(MovieArt::Poster, "Heat", "jpg", FolderKind::Single),
        ["poster.jpg"]
    );
    assert_eq!(
        jellyfin.targets(MovieArt::Fanart, "Heat", "jpg", FolderKind::Single),
        ["backdrop.jpg"]
    );
    assert_eq!(
        jellyfin.extra_fanart_targets("Heat", "jpg", FolderKind::Single, 2),
        ["backdrop1.jpg", "backdrop2.jpg"]
    );
    let emby = ArtworkPreset::Emby.movie_names();
    assert_eq!(
        emby.targets(MovieArt::Fanart, "Heat", "jpg", FolderKind::Single),
        ["fanart.jpg"]
    );
    assert_eq!(
        emby.extra_fanart_targets("Heat", "jpg", FolderKind::Single, 1),
        ["extrafanart/fanart1.jpg"]
    );
    let xbmc = ArtworkPreset::Xbmc.movie_names();
    assert_eq!(
        xbmc.targets(MovieArt::Poster, "Heat", "jpg", FolderKind::Single),
        ["Heat-poster.jpg"]
    );
    assert_eq!(
        xbmc.extra_fanart_folder(FolderKind::Single),
        Some("extrafanart")
    );
    // Every choice of every preset is one the tables know.
    for preset in ArtworkPreset::ALL {
        let names = preset.movie_names();
        for art in MovieArt::ALL {
            assert!(!names.choices(art).is_empty(), "{} {:?}", preset.id(), art);
            for choice in names.choices(art) {
                assert!(MovieNames::is_valid_choice(art, choice), "{choice}");
            }
        }
        for choice in &names.extra_fanart {
            assert!(
                movie_rule(ArtKind::ExtraFanart, choice).is_some(),
                "{choice}"
            );
        }
        assert_eq!(ArtworkPreset::parse(preset.id()), Some(preset));
    }
}

#[test]
fn every_preset_keeps_a_multi_movie_folder_collision_free() {
    for preset in ArtworkPreset::ALL {
        let names = preset.movie_names();
        for art in MovieArt::ALL {
            let a = names.targets(art, "Movie A", "jpg", FolderKind::MultiMovie);
            let b = names.targets(art, "Movie B", "jpg", FolderKind::MultiMovie);
            assert!(!a.is_empty(), "{} {:?}", preset.id(), art);
            assert!(
                a.iter().all(|n| !b.contains(n)),
                "{} {:?}",
                preset.id(),
                art
            );
        }
    }
}

#[test]
fn tv_show_season_and_episode_names() {
    let tv = TvNames::default();
    assert_eq!(tv.show_targets(ShowArt::Poster, "jpg"), ["poster.jpg"]);
    assert_eq!(tv.show_targets(ShowArt::Thumb, ""), ["landscape.jpg"]);
    assert_eq!(
        tv.season_targets(SeasonArt::Poster, 1, "Season 1", "jpg"),
        ["season01-poster.jpg"]
    );
    assert_eq!(
        tv.season_targets(SeasonArt::Fanart, 0, "Specials", "jpg"),
        ["season-specials-fanart.jpg"]
    );
    assert_eq!(
        tv.season_targets(SeasonArt::Banner, -1, "", "jpg"),
        ["season-all-banner.jpg"]
    );
    assert_eq!(
        tv.episode_thumb_targets("Show - S01E02 - Pilot", "jpg"),
        ["Show - S01E02 - Pilot-thumb.jpg"]
    );

    let mut tv = TvNames::default();
    tv.season_poster = vec![
        "SEASON_POSTER".to_owned(),
        "SEASON_FOLDER".to_owned(),
        "FOLDER".to_owned(),
    ];
    tv.episode_thumb = vec!["FILENAME".to_owned(), "FILENAME_TBN".to_owned()];
    assert_eq!(
        tv.season_targets(SeasonArt::Poster, 2, "Season 2", "png"),
        [
            "season02-poster.png",
            "Season 2/season02-poster.png",
            "Season 2/folder.png"
        ]
    );
    assert_eq!(tv.episode_thumb_targets("E", "jpg"), ["E.jpg", "E.tbn"]);
}

#[test]
fn the_style_setting_picks_the_starting_lists() {
    let kodi = ArtworkNames::from_values(&Values::new());
    assert_eq!(kodi, ArtworkNames::preset(ArtworkPreset::Kodi));

    let short = ArtworkNames::from_values(&values(&[(STYLE_KEY, json!("short"))]));
    assert_eq!(short.movie.poster, ["POSTER"]);

    let both = ArtworkNames::from_values(&values(&[(STYLE_KEY, json!("both"))]));
    assert_eq!(both.movie.poster, ["FILENAME_POSTER", "POSTER"]);
    assert_eq!(
        both.movie
            .targets(MovieArt::Poster, "Heat", "jpg", FolderKind::Single),
        ["Heat-poster.jpg", "poster.jpg"]
    );
    assert_eq!(
        both.movie
            .targets(MovieArt::Poster, "Heat", "jpg", FolderKind::MultiMovie),
        ["Heat-poster.jpg"]
    );

    let jellyfin = ArtworkNames::from_values(&values(&[(STYLE_KEY, json!("Jellyfin"))]));
    assert_eq!(jellyfin.movie.fanart, ["BACKDROP"]);

    // A hand-edited unknown or wrongly typed style gives the default.
    for bad in [json!("nope"), json!(7), json!(null)] {
        let names = ArtworkNames::from_values(&values(&[(STYLE_KEY, bad)]));
        assert_eq!(names, kodi);
    }
}

#[test]
fn per_type_settings_replace_a_list() {
    let names = ArtworkNames::from_values(&values(&[
        ("artwork.names.movie.poster", json!(["poster", "folder"])),
        (
            "artwork.names.movie.fanart",
            json!("FILENAME_FANART, fanart"),
        ),
        (
            "artwork.names.movie.extra-fanart",
            json!(["folder_extrafanart"]),
        ),
        ("artwork.names.tv.season-poster", json!(["season_folder"])),
        ("artwork.names.tv.episode-thumb", json!(["filename_tbn"])),
        ("artwork.names.tv.fanart", json!(["backdrop"])),
    ]));
    assert_eq!(names.movie.poster, ["POSTER", "FOLDER"]);
    assert_eq!(names.movie.fanart, ["FILENAME_FANART", "FANART"]);
    assert_eq!(names.movie.extra_fanart, ["FOLDER_EXTRAFANART"]);
    assert_eq!(names.tv.season_poster, ["SEASON_FOLDER"]);
    assert_eq!(names.tv.episode_thumb, ["FILENAME_TBN"]);
    assert_eq!(names.tv.fanart, ["BACKDROP"]);
    // The others keep the starting lists.
    assert_eq!(names.movie.banner, ["FILENAME_BANNER"]);
}

#[test]
fn a_bad_or_empty_override_is_ignored_and_the_type_keeps_its_names() {
    let names = ArtworkNames::from_values(&values(&[
        ("artwork.names.movie.poster", json!(["bogus", "also-bogus"])),
        ("artwork.names.movie.banner", json!([])),
        ("artwork.names.movie.keyart", json!(12)),
        ("artwork.names.tv.poster", json!(["FILENAME_POSTER"])),
    ]));
    assert_eq!(names.movie.poster, ["FILENAME_POSTER"]);
    assert_eq!(
        names.movie.banner,
        ["FILENAME_BANNER"],
        "an empty list is ignored"
    );
    assert_eq!(names.movie.keyart, ["FILENAME_KEYART"]);
    // A movie-only choice is not valid for a show.
    assert_eq!(names.tv.poster, ["POSTER"]);
}

#[test]
fn file_types_map_to_movie_types() {
    assert_eq!(
        MovieArt::from_file_type(MediaFileType::Disc),
        Some(MovieArt::Discart)
    );
    assert_eq!(MovieArt::from_file_type(MediaFileType::Extrafanart), None);
    assert_eq!(MovieArt::from_file_type(MediaFileType::SeasonPoster), None);
    for art in MovieArt::ALL {
        assert_eq!(MovieArt::parse(art.key()), Some(art));
    }
}
