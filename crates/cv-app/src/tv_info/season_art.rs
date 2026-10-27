//! Season artwork: file names per settings, and the show-artwork fallback.
//!
//! The names come from the renamer's tables (`cv_rename::naming::season_art_path`), so the writer
//! and the renamer always plan the same files. A choice list says which names a type is written
//! to (`SEASON_POSTER` gives `season01-poster.jpg` in the show folder, `SEASON_FOLDER` the same
//! name inside the season folder, `FOLDER`/`POSTER`/`COVER` the poster as `folder.jpg` and so on).
//!
//! | Season | Name |
//! |---|---|
//! | `N` > 0 | `seasonNN-<type>` |
//! | `0` (specials) | `season-specials-<type>` |
//! | `-1` (all seasons) | `season-all-<type>` |
//!
//! The choice lists come from the artwork settings (`cv_artwork::naming::TvNames`, whose
//! `season_choices` gives the list of one type); `TvNames::season_targets` builds the same paths
//! from the same tables.

use std::collections::BTreeMap;

use cv_rename::naming::{SeasonArt, season_art_path};
use serde_json::Value;

/// The setting that lets a season without artwork show the show's.
pub const SEASON_ART_FALLBACK_KEY: &str = "tv.season-art-fallback";

/// Reads [`SEASON_ART_FALLBACK_KEY`]; on unless switched off.
#[must_use]
pub fn season_art_fallback(values: &BTreeMap<String, Value>) -> bool {
    values
        .get(SEASON_ART_FALLBACK_KEY)
        .and_then(Value::as_bool)
        .unwrap_or(true)
}

/// The files a season's image is written to, relative to the show folder, one per distinct name
/// of the `choices`. `season` is `0` for the specials and `-1` for the all-seasons image;
/// `season_folder` is the season's folder name, empty when the episodes sit in the show folder.
/// Choices this version does not know are ignored; an empty list writes nothing.
#[must_use]
pub fn season_targets(
    choices: &[String],
    art: SeasonArt,
    season: i64,
    season_folder: &str,
    ext: &str,
) -> Vec<String> {
    let ext = ext.trim().trim_start_matches('.').to_ascii_lowercase();
    let ext = if ext.is_empty() {
        "jpg".to_owned()
    } else {
        ext
    };
    let mut out: Vec<String> = Vec::new();
    for choice in choices {
        if let Some(path) = season_art_path(art, choice, season, season_folder, &ext)
            && !out.contains(&path)
        {
            out.push(path);
        }
    }
    out
}

/// An image shown for a season, and whose it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeasonImage<T> {
    pub image: T,
    /// `true` when the season has no image of its own and this one is the show's.
    pub from_show: bool,
}

/// The season's own image, else (when `fallback` is on) the show's. `None` when neither applies.
#[must_use]
pub fn with_show_fallback<T>(
    own: Option<T>,
    show: Option<T>,
    fallback: bool,
) -> Option<SeasonImage<T>> {
    match own {
        Some(image) => Some(SeasonImage {
            image,
            from_show: false,
        }),
        None if fallback => show.map(|image| SeasonImage {
            image,
            from_show: true,
        }),
        None => None,
    }
}
