//! The artwork name catalogue Settings › Naming shows: for every artwork type, the name choices it
//! offers with an example file name each, and the lists every `metadata.artwork-names` style
//! starts from.
//!
//! Spec: `settings` (Artwork file names in the Naming section). The examples are built by the same
//! code that names the files ([`MovieNames::targets`], [`TvNames::show_targets`], ...), so what the
//! page previews is what a scrape writes and what Rename moves the files to. The page reads the
//! catalogue from `ui/src/features/settings/artworkNameCatalog.ts`, which the test of this module
//! writes (`CINEVAULT_UPDATE_ARTWORK_NAMES=1 cargo test -p cv-artwork`) and keeps current: the
//! test fails when the file differs from [`catalog`].
//!
//! A choice is **common** when the page lists it on every row; the others (`<name>.fanart`,
//! `background`, ...) show only while they are in use, so a hand-edited or older list still reads
//! correctly without crowding the rows.

use serde::Serialize;

use super::{ArtworkNames, FolderKind, MovieArt, MovieNames, SEASON_KEYS, SHOW_KEYS, TvNames};
use cv_rename::naming::{SeasonArt, ShowArt};

/// The placeholder of the video name in the labels.
pub const NAME_TOKEN: &str = "{name}";

/// The example movie of the previews.
pub const EXAMPLE_MOVIE: &str = "Example Movie (2020)";
/// The example episode video of the previews.
pub const EXAMPLE_EPISODE: &str = "Example Show S01E01";
/// The season folder of the previews.
pub const EXAMPLE_SEASON_FOLDER: &str = "Season 1";

/// The `metadata.artwork-names` values the page offers or still recognises, in its order.
pub const STYLES: [&str; 8] = [
    "prefixed", "short", "both", "jellyfin", "emby", "kodi", "xbmc", "plex",
];

/// One name choice of a type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChoiceEntry {
    /// The stored choice: `FILENAME_POSTER`.
    pub id: String,
    /// The name without its extension, the video name as [`NAME_TOKEN`]: `{name}-poster`.
    pub label: String,
    /// The file it gives for the example title: `Example Movie (2020)-poster.jpg`.
    pub example: String,
    /// Listed on every row; the others show only while they are in use.
    pub common: bool,
}

/// One artwork type and its choices.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeEntry {
    /// The setting key after `artwork.names.`: `movie.poster`, `tv.season-poster`.
    pub key: String,
    /// `movie` or `tv`.
    pub scope: &'static str,
    /// Only the first name is used (extra fanart); the page offers one choice, not several.
    pub single: bool,
    pub choices: Vec<ChoiceEntry>,
}

/// The lists one style starts from, by type key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StyleEntry {
    pub id: &'static str,
    pub lists: std::collections::BTreeMap<String, Vec<String>>,
}

/// What the page needs to show and preview the artwork names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    pub types: Vec<TypeEntry>,
    pub styles: Vec<StyleEntry>,
}

/// The choices of each movie type, common ones first. Every one is a choice
/// [`MovieNames::is_valid_choice`] accepts (checked by the tests).
const MOVIE_CHOICES: [(MovieArt, &[(&str, bool)]); 9] = [
    (
        MovieArt::Poster,
        &[
            ("FILENAME_POSTER", true),
            ("POSTER", true),
            ("FOLDER", true),
            ("COVER", true),
            ("FILENAME", false),
            ("MOVIE", false),
        ],
    ),
    (
        MovieArt::Fanart,
        &[
            ("FILENAME_FANART", true),
            ("FANART", true),
            ("BACKDROP", true),
            ("FILENAME_BACKDROP", false),
            ("FILENAME_FANART2", false),
            ("FILENAME_BACKDROP2", false),
            ("FILENAME_BACKGROUND", false),
            ("FILENAME_BACKGROUND2", false),
            ("BACKGROUND", false),
        ],
    ),
    (
        MovieArt::Banner,
        &[("FILENAME_BANNER", true), ("BANNER", true)],
    ),
    (
        MovieArt::Clearart,
        &[("FILENAME_CLEARART", true), ("CLEARART", true)],
    ),
    (
        MovieArt::Thumb,
        &[
            ("FILENAME_LANDSCAPE", true),
            ("LANDSCAPE", true),
            ("FILENAME_THUMB", true),
            ("THUMB", true),
        ],
    ),
    (
        MovieArt::Clearlogo,
        &[
            ("FILENAME_CLEARLOGO", true),
            ("CLEARLOGO", true),
            ("LOGO", true),
            ("FILENAME_LOGO", false),
        ],
    ),
    (
        MovieArt::Discart,
        &[
            ("FILENAME_DISCART", true),
            ("DISCART", true),
            ("DISC", true),
            ("FILENAME_DISC", false),
        ],
    ),
    (
        MovieArt::Keyart,
        &[("FILENAME_KEYART", true), ("KEYART", true)],
    ),
    (
        MovieArt::Squareart,
        &[
            ("FILENAME_SQUAREART", true),
            ("SQUAREART", true),
            ("FILENAME_SQUARE", false),
            ("SQUARE", false),
            ("BACKGROUND_SQUARE", false),
        ],
    ),
];

/// The choices of extra fanart; only the first name of the list is used.
const EXTRA_FANART_CHOICES: [(&str, bool); 12] = [
    ("FILENAME_EXTRAFANART", true),
    ("FOLDER_EXTRAFANART", true),
    ("EXTRAFANART", true),
    ("EXTRABACKDROP", true),
    ("FOLDER_EXTRABACKDROP", true),
    ("FILENAME_EXTRABACKDROP", true),
    ("FILENAME_EXTRAFANART2", false),
    ("FILENAME_EXTRABACKDROP2", false),
    ("FILENAME_EXTRABACKGROUND", false),
    ("FILENAME_EXTRABACKGROUND2", false),
    ("EXTRABACKGROUND", false),
    ("FOLDER_EXTRABACKGROUND", false),
];

fn show_choices(art: ShowArt) -> &'static [(&'static str, bool)] {
    match art {
        ShowArt::Poster => &[("POSTER", true), ("FOLDER", true), ("COVER", true)],
        ShowArt::Fanart => &[("FANART", true), ("BACKDROP", true), ("BACKGROUND", false)],
        ShowArt::Banner => &[("BANNER", true)],
        ShowArt::Clearart => &[("CLEARART", true)],
        ShowArt::Clearlogo => &[("CLEARLOGO", true), ("LOGO", true)],
        ShowArt::Discart => &[("DISCART", true), ("DISC", true)],
        ShowArt::Characterart => &[("CHARACTERART", true)],
        ShowArt::Keyart => &[("KEYART", true)],
        ShowArt::Thumb => &[("LANDSCAPE", true), ("THUMB", true)],
        ShowArt::Squareart => &[
            ("SQUAREART", true),
            ("SQUARE", false),
            ("BACKGROUND_SQUARE", false),
        ],
    }
}

fn season_choices(art: SeasonArt) -> Vec<(&'static str, bool)> {
    let own = match art {
        SeasonArt::Poster => "SEASON_POSTER",
        SeasonArt::Fanart => "SEASON_FANART",
        SeasonArt::Banner => "SEASON_BANNER",
        SeasonArt::Thumb => "SEASON_THUMB",
    };
    let mut list = vec![(own, true), ("SEASON_FOLDER", true)];
    if art == SeasonArt::Poster {
        list.extend([("FOLDER", true), ("POSTER", true), ("COVER", true)]);
    }
    list
}

const EPISODE_THUMB_CHOICES: [(&str, bool); 3] = [
    ("FILENAME_THUMB", true),
    ("FILENAME", true),
    ("FILENAME_TBN", true),
];

/// `name` without `.ext`.
fn without_ext(name: &str, ext: &str) -> String {
    name.strip_suffix(&format!(".{ext}"))
        .unwrap_or(name)
        .to_owned()
}

fn entry(id: &str, common: bool, label: Option<String>, example: Option<String>) -> ChoiceEntry {
    ChoiceEntry {
        id: id.to_owned(),
        label: label.unwrap_or_default(),
        example: example.unwrap_or_default(),
        common,
    }
}

fn movie_entry(art: MovieArt, id: &str, common: bool) -> ChoiceEntry {
    let mut names = MovieNames::default();
    *names.choices_mut(art) = vec![id.to_owned()];
    let first = |base: &str| {
        names
            .targets(art, base, "", FolderKind::Single)
            .into_iter()
            .next()
    };
    let ext = art.default_ext();
    entry(
        id,
        common,
        first(NAME_TOKEN).map(|n| without_ext(&n, ext)),
        first(EXAMPLE_MOVIE),
    )
}

fn extra_fanart_entry(id: &str, common: bool) -> ChoiceEntry {
    let names = MovieNames {
        extra_fanart: vec![id.to_owned()],
        ..MovieNames::default()
    };
    let first = |base: &str| {
        names
            .extra_fanart_targets(base, "", FolderKind::Single, 1)
            .into_iter()
            .next()
    };
    entry(
        id,
        common,
        first(NAME_TOKEN).map(|n| without_ext(&n, "jpg")),
        first(EXAMPLE_MOVIE),
    )
}

fn show_entry(art: ShowArt, id: &str, common: bool) -> ChoiceEntry {
    let mut names = TvNames::default();
    *names.show_choices_mut(art) = vec![id.to_owned()];
    // The images that are mostly transparent come as PNG, like the movie ones.
    let ext = match art {
        ShowArt::Clearart | ShowArt::Clearlogo | ShowArt::Discart => "png",
        _ => "jpg",
    };
    let example = names.show_targets(art, ext).into_iter().next();
    entry(
        id,
        common,
        example.as_deref().map(|n| without_ext(n, ext)),
        example,
    )
}

fn season_entry(art: SeasonArt, id: &str, common: bool) -> ChoiceEntry {
    let mut names = TvNames::default();
    *names.season_choices_mut(art) = vec![id.to_owned()];
    let example = names
        .season_targets(art, 1, EXAMPLE_SEASON_FOLDER, "")
        .into_iter()
        .next();
    entry(
        id,
        common,
        example.as_deref().map(|n| without_ext(n, "jpg")),
        example,
    )
}

fn episode_entry(id: &str, common: bool) -> ChoiceEntry {
    let names = TvNames {
        episode_thumb: vec![id.to_owned()],
        ..TvNames::default()
    };
    let first = |base: &str| names.episode_thumb_targets(base, "").into_iter().next();
    entry(
        id,
        common,
        first(NAME_TOKEN).map(|n| without_ext(&n, "jpg")),
        first(EXAMPLE_EPISODE),
    )
}

fn type_entry(key: String, scope: &'static str, choices: Vec<ChoiceEntry>) -> TypeEntry {
    TypeEntry {
        key,
        scope,
        single: false,
        choices,
    }
}

/// Every type the page lists, movies first, in the order of the settings.
fn types() -> Vec<TypeEntry> {
    let mut out = Vec::new();
    for (art, choices) in MOVIE_CHOICES {
        out.push(type_entry(
            format!("movie.{}", art.key()),
            "movie",
            choices
                .iter()
                .map(|(id, common)| movie_entry(art, id, *common))
                .collect(),
        ));
    }
    out.push(TypeEntry {
        single: true,
        ..type_entry(
            "movie.extra-fanart".to_owned(),
            "movie",
            EXTRA_FANART_CHOICES
                .iter()
                .map(|(id, common)| extra_fanart_entry(id, *common))
                .collect(),
        )
    });
    for (key, art) in SHOW_KEYS {
        out.push(type_entry(
            format!("tv.{key}"),
            "tv",
            show_choices(art)
                .iter()
                .map(|(id, common)| show_entry(art, id, *common))
                .collect(),
        ));
    }
    for (key, art) in SEASON_KEYS {
        out.push(type_entry(
            format!("tv.{key}"),
            "tv",
            season_choices(art)
                .iter()
                .map(|(id, common)| season_entry(art, id, *common))
                .collect(),
        ));
    }
    out.push(type_entry(
        "tv.episode-thumb".to_owned(),
        "tv",
        EPISODE_THUMB_CHOICES
            .iter()
            .map(|(id, common)| episode_entry(id, *common))
            .collect(),
    ));
    out
}

/// The list of every type for a set of names, by type key.
pub fn lists_of(names: &ArtworkNames) -> std::collections::BTreeMap<String, Vec<String>> {
    let mut lists = std::collections::BTreeMap::new();
    for art in MovieArt::ALL {
        lists.insert(
            format!("movie.{}", art.key()),
            names.movie.choices(art).to_vec(),
        );
    }
    lists.insert(
        "movie.extra-fanart".to_owned(),
        names.movie.extra_fanart.clone(),
    );
    for (key, art) in SHOW_KEYS {
        lists.insert(format!("tv.{key}"), names.tv.show_choices(art).to_vec());
    }
    for (key, art) in SEASON_KEYS {
        lists.insert(format!("tv.{key}"), names.tv.season_choices(art).to_vec());
    }
    lists.insert(
        "tv.episode-thumb".to_owned(),
        names.tv.episode_thumb.clone(),
    );
    lists
}

/// The catalogue the page reads.
#[must_use]
pub fn catalog() -> Catalog {
    Catalog {
        types: types(),
        styles: STYLES
            .iter()
            .map(|id| StyleEntry {
                id,
                lists: lists_of(&ArtworkNames::from_style(id)),
            })
            .collect(),
    }
}

/// The TypeScript module the page imports: the catalogue as a typed constant.
#[must_use]
pub fn catalog_module() -> String {
    let json = serde_json::to_string_pretty(&catalog()).unwrap_or_else(|_| "{}".to_owned());
    format!(
        "// Generated by cv-artwork (src/naming/catalog.rs). Do not edit; run `cargo test -p cv-artwork` with\n\
         // CINEVAULT_UPDATE_ARTWORK_NAMES=1 to regenerate.\n\
         import type {{ ArtworkNameCatalog }} from \"./artworkNames\";\n\
         \n\
         export const ARTWORK_NAME_CATALOG: ArtworkNameCatalog = {json};\n"
    )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::naming::ArtworkPreset;
    use cv_rename::naming::{episode_thumb_name, season_art_path, show_stem};

    #[test]
    fn every_listed_choice_is_one_the_names_accept_and_gives_a_file() {
        for entry in types() {
            assert!(!entry.choices.is_empty(), "{}", entry.key);
            for choice in &entry.choices {
                assert!(!choice.example.is_empty(), "{} {}", entry.key, choice.id);
                assert!(!choice.label.is_empty(), "{} {}", entry.key, choice.id);
            }
        }
        for (art, choices) in MOVIE_CHOICES {
            for (id, _) in choices {
                assert!(MovieNames::is_valid_choice(art, id), "{id}");
            }
        }
        for (key, art) in SHOW_KEYS {
            for (id, _) in show_choices(art) {
                assert!(show_stem(art, id).is_some(), "{key} {id}");
            }
        }
        for (key, art) in SEASON_KEYS {
            for (id, _) in season_choices(art) {
                assert!(
                    season_art_path(art, id, 1, "S", "jpg").is_some(),
                    "{key} {id}"
                );
            }
        }
        for (id, _) in EPISODE_THUMB_CHOICES {
            assert!(episode_thumb_name(id, "E", "jpg").is_some(), "{id}");
        }
    }

    #[test]
    fn the_examples_are_the_files_the_writer_names() {
        let types = types();
        let find = |key: &str, id: &str| {
            types
                .iter()
                .find(|t| t.key == key)
                .and_then(|t| t.choices.iter().find(|c| c.id == id))
                .cloned()
                .unwrap()
        };
        let poster = find("movie.poster", "FILENAME_POSTER");
        assert_eq!(poster.label, "{name}-poster");
        assert_eq!(poster.example, "Example Movie (2020)-poster.jpg");
        assert_eq!(find("movie.poster", "FOLDER").example, "folder.jpg");
        assert_eq!(find("movie.clearlogo", "LOGO").example, "logo.png");
        assert_eq!(find("movie.thumb", "LANDSCAPE").label, "landscape");
        let extra = find("movie.extra-fanart", "FOLDER_EXTRAFANART");
        assert_eq!(extra.example, "extrafanart/fanart1.jpg");
        assert_eq!(
            find("tv.season-poster", "SEASON_FOLDER").example,
            "Season 1/season01-poster.jpg"
        );
        assert_eq!(
            find("tv.episode-thumb", "FILENAME_THUMB").example,
            "Example Show S01E01-thumb.jpg"
        );
        assert_eq!(find("tv.episode-thumb", "FILENAME_TBN").label, "{name}.tbn");
        assert_eq!(find("tv.fanart", "BACKDROP").example, "backdrop.jpg");
    }

    #[test]
    fn every_style_lists_every_type_with_choices_the_page_offers() {
        let catalog = catalog();
        for style in &catalog.styles {
            for entry in &catalog.types {
                let list = style
                    .lists
                    .get(&entry.key)
                    .unwrap_or_else(|| panic!("{} has no {}", style.id, entry.key));
                assert!(!list.is_empty(), "{} {}", style.id, entry.key);
                for id in list {
                    assert!(
                        entry.choices.iter().any(|c| &c.id == id),
                        "{} {}: {id} is not offered",
                        style.id,
                        entry.key
                    );
                }
            }
        }
        let jellyfin = catalog.styles.iter().find(|s| s.id == "jellyfin").unwrap();
        assert_eq!(
            jellyfin.lists["movie.fanart"],
            ArtworkPreset::Jellyfin.movie_names().fanart
        );
    }

    #[test]
    fn the_lists_of_a_style_read_back_as_the_same_names() {
        // Custom starts from the lists of the style in use: written as per-type settings they
        // give exactly the names the style gave, so nothing changes until a row is edited.
        for style in STYLES {
            let names = ArtworkNames::from_style(style);
            let values = lists_of(&names)
                .into_iter()
                .map(|(key, list)| (format!("artwork.names.{key}"), serde_json::json!(list)))
                .chain([(super::super::STYLE_KEY.to_owned(), serde_json::json!(style))])
                .collect();
            assert_eq!(ArtworkNames::from_values(&values), names, "{style}");
        }
    }

    /// Fails when the page's copy of the catalogue is stale; `CINEVAULT_UPDATE_ARTWORK_NAMES=1`
    /// rewrites it.
    #[test]
    fn the_page_catalogue_is_up_to_date() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../ui/src/features/settings/artworkNameCatalog.ts");
        let wanted = catalog_module();
        if std::env::var_os("CINEVAULT_UPDATE_ARTWORK_NAMES").is_some() {
            std::fs::write(&path, &wanted).unwrap();
            return;
        }
        let current = std::fs::read_to_string(&path)
            .unwrap_or_default()
            .replace("\r\n", "\n");
        assert!(
            current == wanted,
            "{} is stale: run `CINEVAULT_UPDATE_ARTWORK_NAMES=1 cargo test -p cv-artwork`",
            path.display()
        );
    }
}
