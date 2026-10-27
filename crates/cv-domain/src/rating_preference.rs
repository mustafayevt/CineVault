//! The preferred rating source and what is shown when an item has no rating from it.
//!
//! The "rating" column, rating sorts, the quick-search `rating:` filter and the NFO default rating
//! all read one rating per item: the one from the preferred source, or, when the item has none,
//! the first available source in [`FALLBACK_ORDER`]. A rating that comes from a source other than
//! the preferred one is a *fallback* and is marked as such wherever it is shown.
//!
//! The user's own rating is never the "rating": it has a column of its own.

use serde::{Deserialize, Serialize};

use crate::rating::{Rating, Ratings, canonical_rating_id, rating_key};

/// The source preferred until the user chooses another. TMDB is present for every title the
/// default scraper identified, so a fresh library shows no fallback markers.
pub const DEFAULT_PREFERRED: &str = rating_key::TMDB;

/// The sources tried, in order, after the preferred one. Sources not listed (an unknown source
/// key, a rating read from an NFO) come after these, by key.
pub const FALLBACK_ORDER: &[&str] = &[
    rating_key::IMDB,
    rating_key::TMDB,
    rating_key::TRAKT,
    rating_key::TVMAZE,
    rating_key::TVDB,
    rating_key::MYANIMELIST,
    rating_key::MOVIEMETER,
    rating_key::LETTERBOXD,
    rating_key::TOMATOMETER_ALL_CRITICS,
    rating_key::METACRITIC,
    rating_key::TOMATOMETER_ALL_AUDIENCE,
    rating_key::TOMATOMETER_AVG_CRITICS,
    rating_key::TOMATOMETER_AVG_AUDIENCE,
    rating_key::ROGEREBERT,
];

/// The preferred rating source.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RatingPreference {
    preferred: String,
}

impl Default for RatingPreference {
    fn default() -> Self {
        Self {
            preferred: DEFAULT_PREFERRED.to_owned(),
        }
    }
}

/// A source key the library can store: lowercase letters, digits, `-` and `_`.
fn is_plain_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

impl RatingPreference {
    /// The preference for `source` (any spelling a rating source is known by). A blank or
    /// unusable key, and the user's own rating, fall back to [`DEFAULT_PREFERRED`].
    pub fn new(source: &str) -> Self {
        let key = canonical_rating_id(source);
        if is_plain_key(&key) && key != rating_key::USER {
            Self { preferred: key }
        } else {
            Self::default()
        }
    }

    /// The preferred source key.
    pub fn preferred(&self) -> &str {
        &self.preferred
    }

    /// The source keys in the order they are tried: the preferred one, then [`FALLBACK_ORDER`]
    /// without it.
    pub fn order(&self) -> Vec<&str> {
        let mut order = vec![self.preferred.as_str()];
        order.extend(
            FALLBACK_ORDER
                .iter()
                .copied()
                .filter(|source| *source != self.preferred),
        );
        order
    }

    /// Where a source ranks: lower wins. Sources outside [`Self::order`] rank after it.
    pub fn rank(&self, source: &str) -> usize {
        let order = self.order();
        order
            .iter()
            .position(|candidate| *candidate == source)
            .unwrap_or(order.len())
    }

    /// Whether a rating from `source` stands in for the preferred one.
    pub fn is_fallback(&self, source: &str) -> bool {
        source != self.preferred
    }

    /// SQL for the rank of `column` (a source key column): `CASE column WHEN 'imdb' THEN 0 ...
    /// ELSE n END`. The keys are plain, so they are written into the statement.
    pub fn rank_sql(&self, column: &str) -> String {
        let order = self.order();
        let mut sql = format!("CASE {column}");
        for (rank, source) in order.iter().enumerate() {
            sql.push_str(&format!(" WHEN '{source}' THEN {rank}"));
        }
        sql.push_str(&format!(" ELSE {} END", order.len()));
        sql
    }
}

/// The rating an item shows, and whether it stands in for the preferred source.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PickedRating<'a> {
    pub rating: &'a Rating,
    /// True when the item has no rating from the preferred source and this is the next one.
    pub fallback: bool,
}

impl Ratings {
    /// The rating to show for `preference`: the preferred source's, or else the first available
    /// one in the fallback order. The user's own rating is not considered.
    pub fn pick(&self, preference: &RatingPreference) -> Option<PickedRating<'_>> {
        self.iter()
            .filter(|rating| !rating.is_user() && rating.is_valid())
            .min_by(|a, b| {
                preference
                    .rank(&a.id)
                    .cmp(&preference.rank(&b.id))
                    .then_with(|| a.id.cmp(&b.id))
            })
            .map(|rating| PickedRating {
                rating,
                fallback: preference.is_fallback(&rating.id),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ratings(entries: &[(&str, f32)]) -> Ratings {
        entries
            .iter()
            .map(|(id, value)| Rating::new(*id, *value, 10, 10))
            .collect()
    }

    #[test]
    fn the_preferred_source_comes_first_and_is_not_repeated() {
        let preference = RatingPreference::new("Letterboxd");
        let order = preference.order();
        assert_eq!(order[0], "letterboxd");
        assert_eq!(order[1], "imdb");
        assert_eq!(order.iter().filter(|s| **s == "letterboxd").count(), 1);
        assert_eq!(order.len(), FALLBACK_ORDER.len());
    }

    #[test]
    fn spellings_are_canonical_and_unusable_keys_take_the_default() {
        assert_eq!(RatingPreference::new("themoviedb").preferred(), "tmdb");
        assert_eq!(RatingPreference::new(" IMDB ").preferred(), "imdb");
        assert_eq!(
            RatingPreference::new("Tomatoes").preferred(),
            "tomatometerallcritics"
        );
        for bad in ["", "user", "a'b", "x y", "NFO"] {
            assert_eq!(RatingPreference::new(bad).preferred(), DEFAULT_PREFERRED);
        }
    }

    #[test]
    fn the_preferred_rating_wins_and_is_not_a_fallback() {
        let r = ratings(&[("tmdb", 7.9), ("imdb", 8.3), ("trakt", 8.0)]);
        let picked = r.pick(&RatingPreference::new("imdb")).unwrap();
        assert_eq!(picked.rating.id, "imdb");
        assert!(!picked.fallback);
    }

    #[test]
    fn a_missing_preferred_source_falls_back_to_the_next_one_and_says_so() {
        let r = ratings(&[("tmdb", 7.9), ("trakt", 8.0)]);
        let picked = r.pick(&RatingPreference::new("imdb")).unwrap();
        assert_eq!(picked.rating.id, "tmdb");
        assert!(picked.fallback);
    }

    #[test]
    fn unlisted_sources_come_last_and_the_user_rating_never_counts() {
        let mut r = ratings(&[("zzz", 5.0), ("aaa", 6.0)]);
        r.set_user_rating(9.0).unwrap();
        let picked = r.pick(&RatingPreference::default()).unwrap();
        assert_eq!(picked.rating.id, "aaa");
        r.insert(Rating::new("metacritic", 70.0, 5, 100));
        assert_eq!(
            r.pick(&RatingPreference::default()).unwrap().rating.id,
            "metacritic"
        );
        let only_user: Ratings = {
            let mut u = Ratings::new();
            u.set_user_rating(9.0).unwrap();
            u
        };
        assert!(only_user.pick(&RatingPreference::default()).is_none());
        assert!(Ratings::new().pick(&RatingPreference::default()).is_none());
    }

    #[test]
    fn the_rank_sql_lists_every_source_in_order() {
        let sql = RatingPreference::new("imdb").rank_sql("r.source");
        assert!(sql.starts_with("CASE r.source WHEN 'imdb' THEN 0 WHEN 'tmdb' THEN 1"));
        assert!(sql.ends_with(&format!("ELSE {} END", FALLBACK_ORDER.len())));
    }

    #[test]
    fn the_preference_is_a_plain_string_on_the_wire() {
        let json = serde_json::to_string(&RatingPreference::new("imdb")).unwrap();
        assert_eq!(json, "\"imdb\"");
        let back: RatingPreference = serde_json::from_str(&json).unwrap();
        assert_eq!(back.preferred(), "imdb");
    }
}
