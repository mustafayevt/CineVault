//! The settings the automation actions and the after-scrape steps read.
//!
//! | Key | Values | Meaning |
//! |---|---|---|
//! | `media.checksums-after-scrape` | `off`, `crc32`, `phash`, `all` (default `off`) | which checksums to calculate after a scrape |
//!
//! The settings that already exist keep their keys: `ratings.fetch-after-scrape-movie` and
//! `ratings.fetch-after-scrape-show` (see [`crate::ratings`]) and
//! the artwork download that always follows a scrape.

use std::collections::BTreeMap;

use cv_media::checksum::ChecksumKind;
use serde_json::Value;

type Values = BTreeMap<String, Value>;

/// The checksums calculated after a scrape.
pub const CHECKSUMS_AFTER_SCRAPE_KEY: &str = "media.checksums-after-scrape";

fn text<'a>(values: &'a Values, key: &str) -> Option<&'a str> {
    values
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
}

/// Which after-scrape steps are switched on, read from the settings at the moment a title was
/// scraped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AfterScrape {
    pub ratings_movie: bool,
    pub ratings_show: bool,
    pub checksums: Option<ChecksumKind>,
}

impl AfterScrape {
    /// Reads the switches. Ratings after a movie scrape are on by default, as in
    /// [`crate::ratings::rating_settings_from_values`]; everything else is off.
    #[must_use]
    pub fn from_values(values: &Values) -> Self {
        let ratings = crate::ratings::rating_settings_from_values(values);
        Self {
            ratings_movie: ratings.movie.after_scrape,
            ratings_show: ratings.show.after_scrape,
            checksums: text(values, CHECKSUMS_AFTER_SCRAPE_KEY)
                .filter(|t| !t.eq_ignore_ascii_case("off"))
                .and_then(ChecksumKind::parse),
        }
    }

    /// Whether any step is on for a movie.
    #[must_use]
    pub fn any_for_movie(&self) -> bool {
        self.ratings_movie || self.checksums.is_some()
    }

    /// Whether any step is on for a show.
    #[must_use]
    pub fn any_for_show(&self) -> bool {
        self.ratings_show || self.checksums.is_some()
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
    fn only_the_ratings_run_after_a_scrape_by_default() {
        let a = AfterScrape::from_values(&Values::new());
        assert!(a.ratings_movie && a.ratings_show);
        assert_eq!(a.checksums, None);
    }

    #[test]
    fn the_switches_are_read_and_a_bad_value_is_off() {
        let a = AfterScrape::from_values(&values(&[(CHECKSUMS_AFTER_SCRAPE_KEY, json!("PHASH"))]));
        assert_eq!(a.checksums, Some(ChecksumKind::Phash));
        let off = AfterScrape::from_values(&values(&[(CHECKSUMS_AFTER_SCRAPE_KEY, json!("off"))]));
        assert_eq!(off.checksums, None);
        let junk = AfterScrape::from_values(&values(&[(CHECKSUMS_AFTER_SCRAPE_KEY, json!("md5"))]));
        assert_eq!(junk.checksums, None);
    }
}
