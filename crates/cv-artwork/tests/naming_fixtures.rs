//! The artwork naming fixtures.
//!
//! * `tests/compat/artwork/naming.json` is written for CineVault: every case lists the exact
//!   files `cv_artwork::naming` must produce (per type, multiple names, multi-movie folders,
//!   disc folders, presets, extras, TV, settings, and the locations of collection artwork).
//! * The rename fixtures (`tests/compat/fixtures/rename/trees*.json` with their
//!   `expected/` results, never edited) pin which artwork names the Kodi profiles produce when a
//!   movie or show is renamed. The writer must name the same files, so those profiles are fed to
//!   the naming module and the names compared.

#![allow(clippy::field_reassign_with_default)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use cv_artwork::naming::set::{SetArt, SetNames, SetPlace};
use cv_artwork::naming::{
    ArtworkNames, ArtworkPreset, FolderKind, MovieArt, MovieNames, STYLE_KEY, TvNames,
};
use cv_rename::naming::{SeasonArt, ShowArt};
use serde_json::Value;

fn compat_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/compat")
}

fn load(path: &Path) -> Value {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn folder_kind(name: &str) -> FolderKind {
    match name {
        "single" => FolderKind::Single,
        "multi-movie" => FolderKind::MultiMovie,
        "disc" => FolderKind::Disc,
        other => panic!("unknown folder kind {other}"),
    }
}

fn movie_list<'a>(names: &'a mut MovieNames, key: &str) -> &'a mut Vec<String> {
    match key {
        "poster" => &mut names.poster,
        "fanart" => &mut names.fanart,
        "extra-fanart" => &mut names.extra_fanart,
        "banner" => &mut names.banner,
        "clearart" => &mut names.clearart,
        "thumb" => &mut names.thumb,
        "clearlogo" => &mut names.clearlogo,
        "discart" => &mut names.discart,
        "keyart" => &mut names.keyart,
        "squareart" => &mut names.squareart,
        other => panic!("unknown movie type {other}"),
    }
}

fn show_art(key: &str) -> Option<ShowArt> {
    Some(match key {
        "poster" => ShowArt::Poster,
        "fanart" => ShowArt::Fanart,
        "banner" => ShowArt::Banner,
        "clearart" => ShowArt::Clearart,
        "clearlogo" => ShowArt::Clearlogo,
        "discart" => ShowArt::Discart,
        "characterart" => ShowArt::Characterart,
        "keyart" => ShowArt::Keyart,
        "thumb" => ShowArt::Thumb,
        "squareart" => ShowArt::Squareart,
        _ => return None,
    })
}

fn season_art(key: &str) -> Option<SeasonArt> {
    Some(match key {
        "season-poster" => SeasonArt::Poster,
        "season-fanart" => SeasonArt::Fanart,
        "season-banner" => SeasonArt::Banner,
        "season-thumb" => SeasonArt::Thumb,
        _ => return None,
    })
}

/// Replaces one TV list by the fixture's choices.
fn set_tv_list(tv: &mut TvNames, key: &str, choices: Vec<String>) {
    let list = match key {
        "poster" => &mut tv.poster,
        "fanart" => &mut tv.fanart,
        "banner" => &mut tv.banner,
        "clearart" => &mut tv.clearart,
        "clearlogo" => &mut tv.clearlogo,
        "discart" => &mut tv.discart,
        "characterart" => &mut tv.characterart,
        "keyart" => &mut tv.keyart,
        "thumb" => &mut tv.thumb,
        "squareart" => &mut tv.squareart,
        "season-poster" => &mut tv.season_poster,
        "season-fanart" => &mut tv.season_fanart,
        "season-banner" => &mut tv.season_banner,
        "season-thumb" => &mut tv.season_thumb,
        "episode-thumb" => &mut tv.episode_thumb,
        other => panic!("unknown TV type {other}"),
    };
    *list = choices;
}

fn text<'a>(case: &'a Value, key: &str) -> &'a str {
    case[key].as_str().unwrap_or("")
}

fn run_movie_case(case: &Value, names: &mut MovieNames) -> Vec<String> {
    let base = text(case, "base");
    let ext = text(case, "ext");
    let folder = folder_kind(text(case, "folder"));
    let count = case["count"].as_u64().unwrap_or(0) as u32;
    let kind = text(case, "kind");
    if let Some(choices) = case.get("names") {
        let key = match kind {
            "movie" => text(case, "art"),
            "movie-extra-fanart" => "extra-fanart",
            _ => "",
        };
        if !key.is_empty() {
            *movie_list(names, key) = strings(choices);
        }
    }
    match kind {
        "movie" => {
            let art = MovieArt::parse(text(case, "art")).expect("movie type");
            names.targets(art, base, ext, folder)
        }
        "movie-extra-fanart" => names.extra_fanart_targets(base, ext, folder, count),
        "movie-extra-thumbs" => names.extra_thumb_targets(folder, ext, count),
        other => panic!("not a movie case: {other}"),
    }
}

fn run_tv_case(case: &Value, names: &mut TvNames) -> Vec<String> {
    let ext = text(case, "ext");
    let art = text(case, "art");
    let kind = text(case, "kind");
    if let Some(choices) = case.get("names") {
        let key = if kind == "episode-thumb" {
            "episode-thumb"
        } else {
            art
        };
        set_tv_list(names, key, strings(choices));
    }
    match kind {
        "show" => names.show_targets(show_art(art).expect("show type"), ext),
        "season" => names.season_targets(
            season_art(art).expect("season type"),
            case["season"].as_i64().expect("season"),
            text(case, "season_folder"),
            ext,
        ),
        "episode-thumb" => names.episode_thumb_targets(text(case, "base"), ext),
        other => panic!("not a TV case: {other}"),
    }
}

/// A collection case: the files below `movie:` are written in each movie folder, the ones below
/// `set:` in the collection data folder.
fn run_set_case(case: &Value) -> Vec<String> {
    let values = case["values"]
        .as_object()
        .expect("values")
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let names = ArtworkNames::from_values(&values).set;
    assert_eq!(names, SetNames::from_values(&values));
    let art = SetArt::parse(text(case, "art")).expect("collection type");
    let tmdb = Some(text(case, "tmdb")).filter(|id| !id.is_empty());
    names
        .targets(art, text(case, "ext"), text(case, "title"), tmdb)
        .into_iter()
        .map(|target| match target.place {
            SetPlace::EachMovieFolder => format!("movie:{}", target.relative),
            SetPlace::CollectionFolder => format!("set:{}", target.relative),
        })
        .collect()
}

fn run_case(case: &Value) -> Vec<String> {
    let kind = text(case, "kind");
    if kind == "movie-set" {
        return run_set_case(case);
    }
    if kind == "settings" {
        let values = case["values"]
            .as_object()
            .expect("values")
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let names = ArtworkNames::from_values(&values);
        return match text(case, "target") {
            "movie" => {
                let art = MovieArt::parse(text(case, "art")).expect("movie type");
                names.movie.targets(
                    art,
                    text(case, "base"),
                    text(case, "ext"),
                    folder_kind(text(case, "folder")),
                )
            }
            "season" => names.tv.season_targets(
                season_art(text(case, "art")).expect("season type"),
                case["season"].as_i64().expect("season"),
                text(case, "season_folder"),
                text(case, "ext"),
            ),
            "episode-thumb" => names
                .tv
                .episode_thumb_targets(text(case, "base"), text(case, "ext")),
            other => panic!("unknown settings target {other}"),
        };
    }
    let preset = ArtworkPreset::parse(text(case, "preset")).expect("preset");
    if kind.starts_with("movie") {
        run_movie_case(case, &mut preset.movie_names())
    } else {
        run_tv_case(case, &mut preset.tv_names())
    }
}

#[test]
fn every_naming_fixture_produces_the_expected_files() {
    let fixture = load(&compat_dir().join("artwork/naming.json"));
    let cases = fixture["cases"].as_array().expect("cases");
    assert!(cases.len() >= 100, "{} cases", cases.len());
    let mut ids = BTreeSet::new();
    let mut failures = Vec::new();
    for case in cases {
        let id = text(case, "id");
        assert!(ids.insert(id.to_owned()), "duplicate id {id}");
        let got = run_case(case);
        let want = strings(&case["expected"]);
        if got != want {
            failures.push(format!("{id}: expected {want:?}, got {got:?}"));
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

#[test]
fn the_fixtures_cover_every_kind_of_case() {
    let fixture = load(&compat_dir().join("artwork/naming.json"));
    let cases = fixture["cases"].as_array().expect("cases");
    for kind in [
        "movie",
        "movie-extra-fanart",
        "movie-extra-thumbs",
        "show",
        "season",
        "episode-thumb",
        "settings",
        "movie-set",
    ] {
        assert!(cases.iter().any(|c| c["kind"] == kind), "no {kind} case");
    }
    for folder in ["single", "multi-movie", "disc"] {
        assert!(
            cases
                .iter()
                .any(|c| c["kind"] == "movie" && c["folder"] == folder),
            "no {folder} movie case"
        );
    }
    for art in SetArt::ALL {
        for style in ["movie:", "set:"] {
            assert!(
                cases.iter().any(|c| c["kind"] == "movie-set"
                    && c["art"] == art.key()
                    && c["expected"].as_array().is_some_and(|e| e
                        .iter()
                        .any(|f| f.as_str().is_some_and(|f| f.starts_with(style))))),
                "no {} collection case for {style}",
                art.key()
            );
        }
    }
    for preset in ArtworkPreset::ALL {
        assert!(
            cases.iter().any(|c| c["preset"] == preset.id()),
            "no case for the {} preset",
            preset.id()
        );
    }
}

// ---- the rename fixtures ---------------------------------------------------------

/// The `new` paths of every group of a rename result.
fn new_paths(result: &Value) -> BTreeSet<String> {
    result["groups"]
        .as_array()
        .expect("groups")
        .iter()
        .flat_map(|g| strings(&g["new"]))
        .collect()
}

fn is_image(path: &str) -> bool {
    path.ends_with(".jpg") || path.ends_with(".png")
}

/// Reads the movie lists of a rename-fixture profile (`posterFilenames`, ...).
fn movie_names_of(profile: &Value) -> MovieNames {
    let m = &profile["movie"];
    MovieNames {
        poster: strings(&m["posterFilenames"]),
        fanart: strings(&m["fanartFilenames"]),
        extra_fanart: strings(&m["extraFanartFilenames"]),
        banner: strings(&m["bannerFilenames"]),
        clearart: strings(&m["clearartFilenames"]),
        thumb: strings(&m["thumbFilenames"]),
        clearlogo: strings(&m["clearlogoFilenames"]),
        discart: strings(&m["discartFilenames"]),
        keyart: strings(&m["keyartFilenames"]),
        squareart: strings(&m["squareartFilenames"]),
    }
}

#[test]
fn the_writer_names_the_files_the_fixture_movie_rename_produces() {
    let root = compat_dir();
    let fixture = load(&root.join("fixtures/rename/trees.json"));
    let expected = load(&root.join("expected/rename/trees.json"));
    let mut checked = 0;
    // The trees that hold every artwork type (the first three cases, one per profile).
    for id in ["tree-001", "tree-002", "tree-003"] {
        let case = fixture["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == id)
            .unwrap_or_else(|| panic!("no fixture {id}"));
        let result = expected["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == id)
            .unwrap_or_else(|| panic!("no result {id}"));
        let profile = &fixture["profiles"][text(case, "profile")];
        let names = movie_names_of(profile);
        let paths = new_paths(result);
        let video = paths
            .iter()
            .find(|p| p.ends_with(".mkv") && !p.contains("-sample") && !p.contains('/'))
            .unwrap_or_else(|| panic!("{id}: no video"));
        let base = video.trim_end_matches(".mkv");

        let mut ours: BTreeSet<String> = BTreeSet::new();
        for art in MovieArt::ALL {
            ours.extend(names.targets(art, base, "", FolderKind::Single));
        }
        ours.extend(names.extra_fanart_targets(base, "jpg", FolderKind::Single, 2));
        let theirs: BTreeSet<String> = paths.iter().filter(|p| is_image(p)).cloned().collect();
        assert_eq!(ours, theirs, "{id}");
        checked += 1;
    }
    assert_eq!(checked, 3);
}

#[test]
fn the_writer_names_the_files_the_fixture_tv_rename_produces() {
    let root = compat_dir();
    let fixture = load(&root.join("fixtures/rename/trees-tv.json"));
    let expected = load(&root.join("expected/rename/trees-tv.json"));
    let mut checked = 0;
    for id in ["tvtree-001", "tvtree-002"] {
        let case = fixture["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == id)
            .unwrap_or_else(|| panic!("no fixture {id}"));
        let result = expected["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == id)
            .unwrap_or_else(|| panic!("no result {id}"));
        let p = &fixture["profiles"][text(case, "profile")]["tvShow"];
        let mut tv = TvNames::default();
        tv.poster = strings(&p["posterFilenames"]);
        tv.fanart = strings(&p["fanartFilenames"]);
        tv.banner = strings(&p["bannerFilenames"]);
        tv.clearlogo = strings(&p["clearlogoFilenames"]);
        tv.season_poster = strings(&p["seasonPosterFilenames"]);
        tv.season_fanart = strings(&p["seasonFanartFilenames"]);
        tv.season_banner = strings(&p["seasonBannerFilenames"]);
        tv.season_thumb = strings(&p["seasonThumbFilenames"]);
        tv.episode_thumb = strings(&p["episodeThumbFilenames"]);
        let paths = new_paths(result);

        let mut ours: BTreeSet<String> = BTreeSet::new();
        ours.extend(tv.show_targets(ShowArt::Poster, "jpg"));
        ours.extend(tv.show_targets(ShowArt::Fanart, "jpg"));
        ours.extend(tv.show_targets(ShowArt::Banner, "jpg"));
        ours.extend(tv.show_targets(ShowArt::Clearlogo, "png"));
        for art in [
            SeasonArt::Poster,
            SeasonArt::Fanart,
            SeasonArt::Banner,
            SeasonArt::Thumb,
        ] {
            ours.extend(tv.season_targets(art, 1, "Season 1", "jpg"));
        }
        ours.extend(tv.season_targets(SeasonArt::Poster, 0, "", "jpg"));
        ours.extend(
            tv.episode_thumb_targets("Season 1/Lighthouse Keepers - S01E01 - Arrival", "jpg"),
        );
        // The extra fanart in the fixture is a TV show extra the TV writer does not name here.
        let theirs: BTreeSet<String> = paths
            .iter()
            .filter(|p| (is_image(p) || p.ends_with(".tbn")) && !p.starts_with("extrafanart"))
            .filter(|p| !p.ends_with("S01E01 - Arrival-fanart.jpg"))
            .cloned()
            .collect();
        assert_eq!(ours, theirs, "{id}");
        checked += 1;
    }
    assert_eq!(checked, 2);
}

#[test]
fn the_default_style_value_is_the_one_the_settings_page_offers() {
    // `ui/src/features/settings/schema.ts` offers `prefixed` and `short`, default `prefixed`.
    let prefixed = ArtworkNames::from_style("prefixed");
    assert_eq!(prefixed, ArtworkNames::default());
    let short = ArtworkNames::from_style("short");
    assert_ne!(short, prefixed);
    assert_eq!(STYLE_KEY, "metadata.artwork-names");
}
