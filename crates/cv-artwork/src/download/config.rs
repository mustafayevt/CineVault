//! The artwork settings, read from the settings map.
//!
//! The keys are the ids of the settings page (`ui/src/features/settings/schema.ts`). Anything
//! missing or of the wrong type takes its default, so a hand-edited settings file never stops
//! artwork from being written.
//!
//! | Key | Values | Meaning |
//! |---|---|---|
//! | `artwork.auto-select` | bool (default on) | choose images from the providers' candidates; off uses only the URLs already recorded |
//! | `artwork.extra-fanart-enabled` | bool (default off) | download extra fanart |
//! | `artwork.extra-fanart` | number (default 5, at most 50) | how many extra fanart images |
//! | `artwork.extra-thumbs` | bool (default off) | download extra thumbs |
//! | `artwork.extra-thumbs-count` | number (default 5, at most 50) | how many extra thumbs |
//! | `artwork.extra-thumbs-resize` | bool (default off) | scale extra thumbs down |
//! | `artwork.extra-thumbs-width` | number (default 480) | the widest an extra thumb may be when resizing |
//! | `artwork.actor-images` | bool (default off) | save actor portraits to `.actors` |
//! | `artwork.languages` | list or comma separated text | preferred image languages, best first (`none` for images without text); default the metadata language, then English |
//! | `artwork.poster-size`, `artwork.fanart-size` | `small`, `medium`, `big`, `large`, `xlarge` (default `large`) | the preferred size |
//! | `artwork.other-resolutions` | bool (default on) | fall back to the other sizes |
//! | `artwork.fanart-without-text` | bool (default on) | put a fanart without text first |
//! | `artwork.any-language` | bool (default on) | as a last resort take any language |
//!
//! The file names come from [`crate::naming::ArtworkNames::from_values`].

use std::collections::BTreeMap;

use cv_providers::SizeBucket;
use serde_json::Value;

use crate::naming::ArtworkNames;
use crate::select::SelectionSettings;

type Values = BTreeMap<String, Value>;

/// The most extra images a setting may ask for.
pub const MAX_EXTRAS: u32 = 50;

/// The widest an extra thumb is made when the setting is missing.
pub const DEFAULT_THUMB_WIDTH: u32 = 480;

/// What is downloaded besides the main images.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadOptions {
    /// Choose from the providers' candidates (the best-image selection).
    pub auto_select: bool,
    /// How many extra fanart images; `None` switches them off.
    pub extra_fanart: Option<u32>,
    /// How many extra thumbs; `None` switches them off.
    pub extra_thumbs: Option<u32>,
    /// Scale extra thumbs down to this width; `None` keeps them as they are.
    pub extra_thumb_width: Option<u32>,
    /// Save actor portraits to the `.actors` folder.
    pub actor_images: bool,
}

impl Default for DownloadOptions {
    fn default() -> Self {
        Self {
            auto_select: true,
            extra_fanart: None,
            extra_thumbs: None,
            extra_thumb_width: None,
            actor_images: false,
        }
    }
}

/// Everything the artwork downloads read from the settings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArtworkConfig {
    pub names: ArtworkNames,
    pub selection: SelectionSettings,
    pub download: DownloadOptions,
}

fn flag(values: &Values, key: &str, default: bool) -> bool {
    values.get(key).and_then(Value::as_bool).unwrap_or(default)
}

fn count(values: &Values, key: &str, default: u32) -> u32 {
    values
        .get(key)
        .and_then(Value::as_u64)
        .map_or(default, |n| u32::try_from(n).unwrap_or(MAX_EXTRAS))
        .min(MAX_EXTRAS)
}

fn text<'a>(values: &'a Values, key: &str) -> Option<&'a str> {
    values
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
}

fn bucket(values: &Values, key: &str, default: SizeBucket) -> SizeBucket {
    match text(values, key).map(str::to_ascii_lowercase).as_deref() {
        Some("small") => SizeBucket::Small,
        Some("medium") => SizeBucket::Medium,
        Some("big") => SizeBucket::Big,
        Some("large") => SizeBucket::Large,
        Some("xlarge") => SizeBucket::XLarge,
        _ => default,
    }
}

fn languages(value: &Value) -> Option<Vec<String>> {
    let items: Vec<String> = match value {
        Value::Array(list) => list
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        Value::String(text) => text.split(',').map(str::to_owned).collect(),
        _ => return None,
    };
    let items: Vec<String> = items
        .into_iter()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect();
    (!items.is_empty()).then_some(items)
}

impl SelectionSettings {
    /// Reads the selection keys of the table in [`crate::download::config`]; the default language
    /// list follows `metadata.language`.
    pub fn from_values(values: &Values) -> Self {
        let ui = text(values, "metadata.language").unwrap_or("en");
        let mut selection = Self::for_ui_language(ui);
        if let Some(list) = values.get("artwork.languages").and_then(languages) {
            selection.languages = list;
        }
        selection.poster_size = bucket(values, "artwork.poster-size", selection.poster_size);
        selection.fanart_size = bucket(values, "artwork.fanart-size", selection.fanart_size);
        selection.other_resolutions = flag(
            values,
            "artwork.other-resolutions",
            selection.other_resolutions,
        );
        selection.prefer_fanart_without_text = flag(
            values,
            "artwork.fanart-without-text",
            selection.prefer_fanart_without_text,
        );
        selection.any_language_fallback = flag(
            values,
            "artwork.any-language",
            selection.any_language_fallback,
        );
        selection
    }
}

impl DownloadOptions {
    /// Reads the download keys of the table above.
    pub fn from_values(values: &Values) -> Self {
        let extra_fanart = flag(values, "artwork.extra-fanart-enabled", false)
            .then(|| count(values, "artwork.extra-fanart", 5))
            .filter(|n| *n > 0);
        let extra_thumbs = flag(values, "artwork.extra-thumbs", false)
            .then(|| count(values, "artwork.extra-thumbs-count", 5))
            .filter(|n| *n > 0);
        let width = values
            .get("artwork.extra-thumbs-width")
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
            .filter(|n| *n > 0)
            .unwrap_or(DEFAULT_THUMB_WIDTH);
        Self {
            auto_select: flag(values, "artwork.auto-select", true),
            extra_fanart,
            extra_thumbs,
            extra_thumb_width: flag(values, "artwork.extra-thumbs-resize", false).then_some(width),
            actor_images: flag(values, "artwork.actor-images", false),
        }
    }
}

impl ArtworkConfig {
    /// Reads names, selection and download options.
    pub fn from_values(values: &Values) -> Self {
        Self {
            names: ArtworkNames::from_values(values),
            selection: SelectionSettings::from_values(values),
            download: DownloadOptions::from_values(values),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn values(pairs: &[(&str, Value)]) -> Values {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect()
    }

    #[test]
    fn a_fresh_install_fills_gaps_after_a_scrape_and_writes_no_extras_or_portraits() {
        let config = ArtworkConfig::from_values(&Values::new());
        assert_eq!(config.download, DownloadOptions::default());
        assert!(config.download.auto_select);
        assert_eq!(config.download.extra_fanart, None);
        assert_eq!(config.download.extra_thumbs, None);
        assert!(!config.download.actor_images);
        assert_eq!(config.selection, SelectionSettings::default());
    }

    #[test]
    fn extras_need_their_switch_and_a_positive_count() {
        let o = DownloadOptions::from_values(&values(&[
            ("artwork.extra-fanart-enabled", json!(true)),
            ("artwork.extra-thumbs", json!(true)),
            ("artwork.extra-thumbs-resize", json!(true)),
        ]));
        assert_eq!(o.extra_fanart, Some(5), "the default count is 5");
        assert_eq!(o.extra_thumbs, Some(5));
        assert_eq!(o.extra_thumb_width, Some(DEFAULT_THUMB_WIDTH));

        let o = DownloadOptions::from_values(&values(&[
            ("artwork.extra-fanart-enabled", json!(true)),
            ("artwork.extra-fanart", json!(4)),
            ("artwork.extra-thumbs", json!(true)),
            ("artwork.extra-thumbs-count", json!(0)),
            ("artwork.extra-thumbs-width", json!(300)),
            ("artwork.extra-thumbs-resize", json!(true)),
        ]));
        assert_eq!(o.extra_fanart, Some(4));
        assert_eq!(o.extra_thumbs, None, "a count of zero is off");
        assert_eq!(o.extra_thumb_width, Some(300));

        // The count alone does nothing; a huge count is capped.
        let o = DownloadOptions::from_values(&values(&[("artwork.extra-fanart", json!(9))]));
        assert_eq!(o.extra_fanart, None);
        let o = DownloadOptions::from_values(&values(&[
            ("artwork.extra-fanart-enabled", json!(true)),
            ("artwork.extra-fanart", json!(5000)),
        ]));
        assert_eq!(o.extra_fanart, Some(MAX_EXTRAS));
    }

    #[test]
    fn selection_keys_are_read_and_wrong_types_are_ignored() {
        let s = SelectionSettings::from_values(&values(&[
            ("metadata.language", json!("tr")),
            ("artwork.poster-size", json!("Big")),
            ("artwork.fanart-size", json!("huge")),
            ("artwork.other-resolutions", json!(false)),
            ("artwork.fanart-without-text", json!("yes")),
        ]));
        assert_eq!(s.languages, ["tr", "en"]);
        assert_eq!(s.poster_size, SizeBucket::Big);
        assert_eq!(
            s.fanart_size,
            SizeBucket::Large,
            "an unknown size is the default"
        );
        assert!(!s.other_resolutions);
        assert!(s.prefer_fanart_without_text, "not a bool, so the default");

        let s = SelectionSettings::from_values(&values(&[(
            "artwork.languages",
            json!("ja, none , en"),
        )]));
        assert_eq!(s.languages, ["ja", "none", "en"]);
        let s = SelectionSettings::from_values(&values(&[("artwork.languages", json!([]))]));
        assert_eq!(s.languages, ["en"], "an empty list is the default");
    }
}
