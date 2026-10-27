//! Where the artwork of a collection (movie set) is written.
//!
//! Spec: `movie-collections` (Collection artwork and NFO locations).
//!
//! # Location styles
//!
//! | Style | Setting value | File |
//! |---|---|---|
//! | [`SetLocation::MovieFolder`] | `movie-folder` | `movieset-<type>.ext` in the folder of **each movie** of the collection |
//! | [`SetLocation::KodiStyle`] | `kodi` | `<collection folder>/<set>/<type>.ext` |
//! | [`SetLocation::AutomatorStyle`] | `automator` | `<collection folder>/<set>-<type>.ext` |
//!
//! `<set>` is the collection name with the characters a file name cannot hold replaced (see
//! [`SetNames::name_replacement`]), plus `-tmdb-<id>` when [`SetNames::append_tmdb_id`] is on and
//! the collection has a TMDB ID. The NFO of a collection uses the same folder name
//! (`cv_nfo::movie_set_folder_name_with`), so the artwork and the NFO of a collection always sit
//! together. `<collection folder>` is the collection data folder of the settings.
//!
//! The `<type>` of the movie-folder style is `poster`, `fanart`, `banner`, `clearart`, `thumb`,
//! `clearlogo`, `discart` (the names the scanner knows as `movieset-*`). The two folder styles
//! write a thumb as `landscape`, the name media centres read for it.
//!
//! # Settings
//!
//! | Key | Values | Meaning |
//! |---|---|---|
//! | `artwork.collection-locations` | `movie-folder` (default), `kodi`, `automator`, `both` (movie folder and Kodi), or a list / comma separated text of styles | where collection artwork goes; an empty list switches it off |
//! | `metadata.collection-name-replacement` | text (default `_`) | what replaces each character a file name cannot hold; empty removes them |
//! | `metadata.nfo-collection-tmdb-id` | bool (default off) | append `-tmdb-<id>` to the collection name |
//!
//! The collection data folder (`metadata.nfo-collection-folder`) is read by the caller; the
//! folder styles are skipped while it is not set.

use std::collections::BTreeMap;

use cv_domain::MediaFileType;
use cv_nfo::{DEFAULT_NAME_REPLACEMENT, movie_set_folder_name_with};
use serde_json::Value;

type Values = BTreeMap<String, Value>;

/// The key of the location styles.
pub const LOCATIONS_KEY: &str = "artwork.collection-locations";
/// The key of the replacement for characters a file name cannot hold.
pub const REPLACEMENT_KEY: &str = "metadata.collection-name-replacement";
/// The key of the TMDB ID suffix (shared with the collection NFO).
pub const APPEND_TMDB_ID_KEY: &str = "metadata.nfo-collection-tmdb-id";

/// The artwork types of a collection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SetArt {
    Poster,
    Fanart,
    Banner,
    Clearart,
    Thumb,
    Clearlogo,
    Discart,
}

impl SetArt {
    /// Every type, in the order they are written.
    pub const ALL: [SetArt; 7] = [
        SetArt::Poster,
        SetArt::Fanart,
        SetArt::Banner,
        SetArt::Clearart,
        SetArt::Thumb,
        SetArt::Clearlogo,
        SetArt::Discart,
    ];

    /// The name used in settings and fixtures.
    pub fn key(self) -> &'static str {
        match self {
            SetArt::Poster => "poster",
            SetArt::Fanart => "fanart",
            SetArt::Banner => "banner",
            SetArt::Clearart => "clearart",
            SetArt::Thumb => "thumb",
            SetArt::Clearlogo => "clearlogo",
            SetArt::Discart => "discart",
        }
    }

    /// The type of a settings or fixture name.
    pub fn parse(key: &str) -> Option<SetArt> {
        SetArt::ALL.into_iter().find(|a| a.key() == key.trim())
    }

    /// The library file type the image is recorded as.
    pub fn file_type(self) -> MediaFileType {
        match self {
            SetArt::Poster => MediaFileType::Poster,
            SetArt::Fanart => MediaFileType::Fanart,
            SetArt::Banner => MediaFileType::Banner,
            SetArt::Clearart => MediaFileType::Clearart,
            SetArt::Thumb => MediaFileType::Thumb,
            SetArt::Clearlogo => MediaFileType::Clearlogo,
            SetArt::Discart => MediaFileType::Disc,
        }
    }

    /// The collection artwork type of a library file type.
    pub fn from_file_type(kind: MediaFileType) -> Option<SetArt> {
        SetArt::ALL.into_iter().find(|a| a.file_type() == kind)
    }

    /// The file name stem in a movie folder.
    fn movie_folder_stem(self) -> String {
        format!("movieset-{}", self.key())
    }

    /// The file name stem in a collection folder.
    fn folder_stem(self) -> &'static str {
        match self {
            SetArt::Thumb => "landscape",
            other => other.key(),
        }
    }
}

/// Where collection artwork goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SetLocation {
    /// `movieset-<type>.ext` in the folder of each movie of the collection.
    MovieFolder,
    /// `<collection folder>/<set>/<type>.ext`.
    KodiStyle,
    /// `<collection folder>/<set>-<type>.ext`.
    AutomatorStyle,
}

impl SetLocation {
    /// The name used in settings and fixtures.
    pub fn key(self) -> &'static str {
        match self {
            SetLocation::MovieFolder => "movie-folder",
            SetLocation::KodiStyle => "kodi",
            SetLocation::AutomatorStyle => "automator",
        }
    }

    /// A style of a settings value: the keys above, or the names `MOVIE_FOLDER`,
    /// `KODI_STYLE_FOLDER` and `AUTOMATOR_STYLE_FOLDER`, ignoring case, `-` and `_`.
    pub fn parse(text: &str) -> Option<SetLocation> {
        let key: String = text
            .trim()
            .chars()
            .filter(|c| !matches!(c, '-' | '_' | ' '))
            .map(|c| c.to_ascii_lowercase())
            .collect();
        Some(match key.as_str() {
            "moviefolder" => SetLocation::MovieFolder,
            "kodi" | "kodistyle" | "kodistylefolder" => SetLocation::KodiStyle,
            "automator" | "automatorstyle" | "automatorstylefolder" => SetLocation::AutomatorStyle,
            _ => return None,
        })
    }

    /// Whether the style needs the collection data folder.
    pub fn needs_collection_folder(self) -> bool {
        self != SetLocation::MovieFolder
    }
}

/// What a path of [`SetTarget`] is relative to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SetPlace {
    /// The folder of each movie of the collection.
    EachMovieFolder,
    /// The collection data folder.
    CollectionFolder,
}

/// One file a collection image is written to.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SetTarget {
    pub location: SetLocation,
    pub place: SetPlace,
    /// The path below the place, with `/` separators.
    pub relative: String,
}

/// The collection artwork settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetNames {
    /// The styles artwork is written in, all with the same images. Empty writes none.
    pub locations: Vec<SetLocation>,
    /// What replaces each character a file name cannot hold in the collection name.
    pub name_replacement: String,
    /// Append `-tmdb-<id>` to the collection name of the folder styles.
    pub append_tmdb_id: bool,
}

impl Default for SetNames {
    fn default() -> Self {
        SetNames {
            locations: vec![SetLocation::MovieFolder],
            name_replacement: DEFAULT_NAME_REPLACEMENT.to_owned(),
            append_tmdb_id: false,
        }
    }
}

/// The styles of a settings value, or `None` when the value does not apply. An empty list
/// switches the artwork off; a list with no valid style is ignored.
fn locations_of(value: &Value) -> Option<Vec<SetLocation>> {
    let items: Vec<String> = match value {
        Value::Array(list) => list
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        Value::String(text) => text.split(',').map(str::to_owned).collect(),
        _ => return None,
    };
    let items: Vec<&str> = items
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    if items.is_empty() {
        return Some(Vec::new());
    }
    let mut kept: Vec<SetLocation> = Vec::new();
    for item in items {
        let found: &[SetLocation] = if item.eq_ignore_ascii_case("both") {
            &[SetLocation::MovieFolder, SetLocation::KodiStyle]
        } else if let Some(location) = SetLocation::parse(item) {
            &[location]
        } else {
            &[]
        };
        for location in found {
            if !kept.contains(location) {
                kept.push(*location);
            }
        }
    }
    (!kept.is_empty()).then_some(kept)
}

impl SetNames {
    /// Reads the settings; anything missing or unknown takes its default, so a hand-edited
    /// settings file can never stop artwork from being written.
    pub fn from_values(values: &Values) -> Self {
        let mut names = SetNames::default();
        if let Some(list) = values.get(LOCATIONS_KEY).and_then(locations_of) {
            names.locations = list;
        }
        if let Some(text) = values.get(REPLACEMENT_KEY).and_then(Value::as_str) {
            names.name_replacement = text.to_owned();
        }
        if let Some(flag) = values.get(APPEND_TMDB_ID_KEY).and_then(Value::as_bool) {
            names.append_tmdb_id = flag;
        }
        names
    }

    /// Whether any style needs the collection data folder.
    pub fn uses_collection_folder(&self) -> bool {
        self.locations.iter().any(|l| l.needs_collection_folder())
    }

    /// The name of the collection's folder (and the stem of the automator-style files): the
    /// collection name with the replacement, plus the TMDB ID suffix when asked for. Empty when
    /// nothing is left of the name.
    pub fn folder_name(&self, title: &str, tmdb_id: Option<&str>) -> String {
        movie_set_folder_name_with(title, tmdb_id, self.append_tmdb_id, &self.name_replacement)
    }

    /// The files an image of `art` with the extension `ext` (without the dot) is written to, in
    /// the order of the styles. The folder styles give nothing when no name is left of the title.
    pub fn targets(
        &self,
        art: SetArt,
        ext: &str,
        title: &str,
        tmdb_id: Option<&str>,
    ) -> Vec<SetTarget> {
        let folder = self.folder_name(title, tmdb_id);
        let mut targets: Vec<SetTarget> = Vec::new();
        for location in &self.locations {
            let target = match location {
                SetLocation::MovieFolder => SetTarget {
                    location: *location,
                    place: SetPlace::EachMovieFolder,
                    relative: format!("{}.{ext}", art.movie_folder_stem()),
                },
                _ if folder.is_empty() => continue,
                SetLocation::KodiStyle => SetTarget {
                    location: *location,
                    place: SetPlace::CollectionFolder,
                    relative: format!("{folder}/{}.{ext}", art.folder_stem()),
                },
                SetLocation::AutomatorStyle => SetTarget {
                    location: *location,
                    place: SetPlace::CollectionFolder,
                    relative: format!("{folder}-{}.{ext}", art.folder_stem()),
                },
            };
            if !targets.contains(&target) {
                targets.push(target);
            }
        }
        targets
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn relatives(names: &SetNames, art: SetArt, title: &str, tmdb: Option<&str>) -> Vec<String> {
        names
            .targets(art, "jpg", title, tmdb)
            .into_iter()
            .map(|t| t.relative)
            .collect()
    }

    fn with(locations: &[SetLocation]) -> SetNames {
        SetNames {
            locations: locations.to_vec(),
            ..SetNames::default()
        }
    }

    #[test]
    fn the_default_writes_into_the_movie_folders() {
        let names = SetNames::default();
        assert_eq!(
            relatives(&names, SetArt::Poster, "Dune Collection", Some("726871")),
            ["movieset-poster.jpg"]
        );
        assert_eq!(
            names.targets(SetArt::Fanart, "jpg", "Dune Collection", None)[0].place,
            SetPlace::EachMovieFolder
        );
        assert!(!names.uses_collection_folder());
    }

    #[test]
    fn kodi_style_puts_the_files_in_a_folder_named_after_the_collection() {
        let names = with(&[SetLocation::KodiStyle]);
        assert_eq!(
            relatives(&names, SetArt::Poster, "Dune Collection", None),
            ["Dune Collection/poster.jpg"]
        );
        assert_eq!(
            relatives(&names, SetArt::Thumb, "Dune Collection", None),
            ["Dune Collection/landscape.jpg"]
        );
        assert!(names.uses_collection_folder());
    }

    #[test]
    fn automator_style_puts_the_name_in_front_of_the_type() {
        let names = with(&[SetLocation::AutomatorStyle]);
        assert_eq!(
            relatives(&names, SetArt::Clearlogo, "Dune Collection", None),
            ["Dune Collection-clearlogo.jpg"]
        );
        assert_eq!(
            relatives(&names, SetArt::Discart, "Dune Collection", None),
            ["Dune Collection-discart.jpg"]
        );
    }

    #[test]
    fn several_styles_give_several_files_in_order() {
        let names = with(&[
            SetLocation::AutomatorStyle,
            SetLocation::MovieFolder,
            SetLocation::KodiStyle,
            SetLocation::KodiStyle,
        ]);
        assert_eq!(
            relatives(&names, SetArt::Banner, "Dune Collection", None),
            [
                "Dune Collection-banner.jpg",
                "movieset-banner.jpg",
                "Dune Collection/banner.jpg"
            ]
        );
    }

    #[test]
    fn the_tmdb_id_is_appended_to_the_folder_styles_only() {
        let mut names = with(&[
            SetLocation::MovieFolder,
            SetLocation::KodiStyle,
            SetLocation::AutomatorStyle,
        ]);
        names.append_tmdb_id = true;
        assert_eq!(
            relatives(&names, SetArt::Poster, "Dune Collection", Some("726871")),
            [
                "movieset-poster.jpg",
                "Dune Collection-tmdb-726871/poster.jpg",
                "Dune Collection-tmdb-726871-poster.jpg"
            ]
        );
        // No ID, no suffix.
        assert_eq!(
            relatives(&names, SetArt::Poster, "Dune Collection", None)[1],
            "Dune Collection/poster.jpg"
        );
    }

    #[test]
    fn characters_a_file_name_cannot_hold_are_replaced() {
        let mut names = with(&[SetLocation::KodiStyle]);
        assert_eq!(
            relatives(&names, SetArt::Poster, "Alien: Covenant? Saga", None),
            ["Alien_ Covenant_ Saga/poster.jpg"]
        );
        names.name_replacement = "-".to_owned();
        assert_eq!(
            relatives(
                &names,
                SetArt::Poster,
                "Mission: Impossible Collection",
                None
            ),
            ["Mission- Impossible Collection/poster.jpg"]
        );
        names.name_replacement = String::new();
        assert_eq!(
            relatives(&names, SetArt::Poster, "Who? What/Where", None),
            ["Who WhatWhere/poster.jpg"]
        );
    }

    #[test]
    fn a_name_with_nothing_left_gets_no_folder_files() {
        let mut names = with(&[SetLocation::MovieFolder, SetLocation::KodiStyle]);
        names.name_replacement = String::new();
        assert_eq!(
            relatives(&names, SetArt::Poster, "???", None),
            ["movieset-poster.jpg"]
        );
    }

    #[test]
    fn the_settings_choose_the_styles() {
        let read = |value: Value| {
            let mut values = Values::new();
            values.insert(LOCATIONS_KEY.to_owned(), value);
            SetNames::from_values(&values).locations
        };
        use SetLocation::{AutomatorStyle, KodiStyle, MovieFolder};
        assert_eq!(SetNames::from_values(&Values::new()), SetNames::default());
        assert_eq!(read(json!("kodi")), [KodiStyle]);
        assert_eq!(read(json!("automator")), [AutomatorStyle]);
        assert_eq!(read(json!("both")), [MovieFolder, KodiStyle]);
        assert_eq!(
            read(json!("KODI_STYLE_FOLDER, movie-folder")),
            [KodiStyle, MovieFolder]
        );
        assert_eq!(
            read(json!(["automator", "bogus", "automator"])),
            [AutomatorStyle]
        );
        assert_eq!(read(json!([])), []);
        assert_eq!(
            read(json!("bogus")),
            [MovieFolder],
            "nothing valid is ignored"
        );
        assert_eq!(read(json!(7)), [MovieFolder]);
    }

    #[test]
    fn the_settings_choose_the_replacement_and_the_suffix() {
        let mut values = Values::new();
        values.insert(REPLACEMENT_KEY.to_owned(), json!(" -"));
        values.insert(APPEND_TMDB_ID_KEY.to_owned(), json!(true));
        let names = SetNames::from_values(&values);
        assert_eq!(names.name_replacement, " -");
        assert!(names.append_tmdb_id);
        assert_eq!(names.folder_name("A:B", Some("1")), "A -B-tmdb-1");
        values.insert(REPLACEMENT_KEY.to_owned(), json!(5));
        assert_eq!(SetNames::from_values(&values).name_replacement, "_");
    }

    #[test]
    fn the_types_map_to_library_file_types_and_back() {
        for art in SetArt::ALL {
            assert_eq!(SetArt::from_file_type(art.file_type()), Some(art));
            assert_eq!(SetArt::parse(art.key()), Some(art));
        }
        assert_eq!(SetArt::from_file_type(MediaFileType::Keyart), None);
        assert_eq!(SetArt::file_type(SetArt::Discart), MediaFileType::Disc);
    }
}
