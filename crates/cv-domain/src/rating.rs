//! Ratings from rating sources.

use crate::web::web_address;
use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Special rating keys.
pub mod rating_key {
    /// The rating the user entered by hand.
    pub const USER: &str = "user";
    /// A rating read from an NFO file.
    pub const NFO: &str = "NFO";
    /// The rating shown by default.
    pub const DEFAULT: &str = "default";

    /// IMDb (0-10).
    pub const IMDB: &str = "imdb";
    /// The Movie Database (0-10).
    pub const TMDB: &str = "tmdb";
    /// Trakt (0-10).
    pub const TRAKT: &str = "trakt";
    /// TVmaze (0-10).
    pub const TVMAZE: &str = "tvmaze";
    /// TheTVDB (0-10).
    pub const TVDB: &str = "tvdb";
    /// MyAnimeList (0-10).
    pub const MYANIMELIST: &str = "myanimelist";
    /// MovieMeter (0-5).
    pub const MOVIEMETER: &str = "moviemeter";
    /// Letterboxd (0-5).
    pub const LETTERBOXD: &str = "letterboxd";
    /// Roger Ebert (0-4).
    pub const ROGEREBERT: &str = "rogerebert";
    /// Metacritic (0-100).
    pub const METACRITIC: &str = "metacritic";
    /// Rotten Tomatoes Tomatometer, all critics (percentage).
    pub const TOMATOMETER_ALL_CRITICS: &str = "tomatometerallcritics";
    /// Rotten Tomatoes audience score, all audience (percentage).
    pub const TOMATOMETER_ALL_AUDIENCE: &str = "tomatometerallaudience";
    /// Rotten Tomatoes average critics rating (0-10).
    pub const TOMATOMETER_AVG_CRITICS: &str = "tomatometeravgcritics";
    /// Rotten Tomatoes average audience rating (0-5).
    pub const TOMATOMETER_AVG_AUDIENCE: &str = "tomatometeravgaudience";
}

/// The canonical form of a rating source key: trimmed and lower-cased, with the old and
/// alternative spellings mapped to the key the app stores (`themoviedb` and `tmdb`,
/// `tomatoes` and `tomatometerallcritics`, `metascore` and `metacritic`, ...). The special key
/// `NFO` keeps its case.
pub fn canonical_rating_id(id: &str) -> String {
    let id = id.trim();
    if id == rating_key::NFO {
        return id.to_owned();
    }
    let lower = id.to_lowercase();
    match lower.as_str() {
        "themoviedb" | "moviedb" => rating_key::TMDB,
        "tomatoes" | "rottentomatoes" | "tomatometer" => rating_key::TOMATOMETER_ALL_CRITICS,
        "audience" | "popcorn" => rating_key::TOMATOMETER_AVG_CRITICS,
        "metascore" => rating_key::METACRITIC,
        "rogerebert.com" | "ebert" => rating_key::ROGEREBERT,
        "mal" => rating_key::MYANIMELIST,
        "thetvdb" => rating_key::TVDB,
        _ => return lower,
    }
    .to_owned()
}

/// The top of the scale a known source always reports on, or `None` for a source the app has no
/// fixed scale for.
pub fn known_max_value(id: &str) -> Option<u32> {
    match canonical_rating_id(id).as_str() {
        rating_key::LETTERBOXD | rating_key::MOVIEMETER | rating_key::TOMATOMETER_AVG_AUDIENCE => {
            Some(5)
        }
        rating_key::ROGEREBERT => Some(4),
        rating_key::METACRITIC
        | rating_key::TOMATOMETER_ALL_CRITICS
        | rating_key::TOMATOMETER_ALL_AUDIENCE => Some(100),
        rating_key::IMDB
        | rating_key::TMDB
        | rating_key::TRAKT
        | rating_key::TVMAZE
        | rating_key::TVDB
        | rating_key::MYANIMELIST
        | rating_key::TOMATOMETER_AVG_CRITICS
        | rating_key::USER => Some(10),
        _ => None,
    }
}

/// Why a user rating was refused.
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum UserRatingError {
    /// The value is not a number between 0.5 and the top of the scale.
    #[error("a rating must be between 0.5 and {max}, got {value}")]
    OutOfRange { value: f32, max: u32 },
    /// The value is not a multiple of one half.
    #[error("a rating must be a multiple of 0.5, got {0}")]
    NotHalfStep(f32),
}

/// The user rating, on the 0-10 scale, checked: 0.5 to 10 in half steps.
pub fn check_user_rating(value: f32) -> Result<f32, UserRatingError> {
    if !value.is_finite() || !(0.5..=10.0).contains(&value) {
        return Err(UserRatingError::OutOfRange { value, max: 10 });
    }
    if (value * 2.0).fract() != 0.0 {
        return Err(UserRatingError::NotHalfStep(value));
    }
    Ok(value)
}

/// A star rating (0.5 to 5 stars in half steps) as a value on the 0-10 scale.
pub fn user_rating_from_stars(stars: f32) -> Result<f32, UserRatingError> {
    if !stars.is_finite() || !(0.5..=5.0).contains(&stars) {
        return Err(UserRatingError::OutOfRange {
            value: stars,
            max: 5,
        });
    }
    if (stars * 2.0).fract() != 0.0 {
        return Err(UserRatingError::NotHalfStep(stars));
    }
    check_user_rating(stars * 2.0)
}

/// One rating from one source (for example `imdb`, `themoviedb`, `user`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Rating {
    /// Rating source key; also the key in [`Ratings`].
    pub id: String,
    /// The rating on its own scale. Negative means "no rating".
    pub value: f32,
    pub votes: u32,
    /// The top of the scale (10 for a 0-10 rating, 100 for a percentage).
    pub max_value: u32,
    /// When the rating was fetched, in epoch milliseconds; `0` when not known. The library stamps
    /// a rating that arrives without one with the time it is stored.
    pub fetched_at_ms: i64,
    /// The title's page on the source's site, when the source named it (an absolute `http(s)`
    /// address); empty when not known. Not part of the rating's value.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub url: String,
}

impl Default for Rating {
    fn default() -> Self {
        Self {
            id: String::new(),
            value: -1.0,
            votes: 0,
            max_value: 10,
            fetched_at_ms: 0,
            url: String::new(),
        }
    }
}

impl Rating {
    pub fn new(id: impl Into<String>, value: f32, votes: u32, max_value: u32) -> Self {
        Self {
            id: id.into(),
            value,
            votes,
            max_value,
            fetched_at_ms: 0,
            url: String::new(),
        }
    }

    /// The same rating with the address of the title's page on the source's site. Anything but
    /// an absolute `http(s)` address is dropped.
    #[must_use]
    pub fn with_url(mut self, url: &str) -> Self {
        self.url = web_address(url).unwrap_or_default();
        self
    }

    /// The same rating, marked as fetched at `epoch_ms`.
    #[must_use]
    pub fn fetched_at(mut self, epoch_ms: i64) -> Self {
        self.fetched_at_ms = epoch_ms.max(0);
        self
    }

    /// A rating is usable when it has a source id, a non-negative value and a positive scale.
    pub fn is_valid(&self) -> bool {
        !self.id.trim().is_empty() && self.value >= 0.0 && self.max_value > 0
    }

    /// The user's own rating from a value on the 0-10 scale (0.5 to 10 in half steps).
    pub fn user(value_on_ten: f32) -> Result<Self, UserRatingError> {
        Ok(Self::new(
            rating_key::USER,
            check_user_rating(value_on_ten)?,
            0,
            10,
        ))
    }

    /// Whether this is the user's own rating.
    pub fn is_user(&self) -> bool {
        self.id == rating_key::USER
    }

    /// The rating in its canonical form: the id spelled the way the app stores it, and the scale
    /// the source is known to use.
    ///
    /// - A known source (Letterboxd out of 5, Roger Ebert out of 4, Metacritic and the
    ///   Tomatometer percentages out of 100, ...) gets its scale. A value that fits the scale is
    ///   only relabelled (4.4 out of a default 10 becomes 4.4 out of 5); a value that does not fit
    ///   is rescaled from the scale it declared (8.8 out of 10 becomes 4.4 out of 5).
    /// - For an unknown source, a value above a declared 10 is taken as a percentage.
    /// - The value never exceeds the scale.
    ///
    /// Invalid ratings are returned unchanged apart from the id.
    #[must_use]
    pub fn normalised(mut self) -> Self {
        self.id = canonical_rating_id(&self.id);
        if !self.is_valid() {
            return self;
        }
        match known_max_value(&self.id) {
            Some(max) if max != self.max_value => {
                if self.value > max as f32 {
                    self.value = self.value * max as f32 / self.max_value as f32;
                }
                self.max_value = max;
            }
            Some(_) => {}
            None if self.max_value == 10 && self.value > 10.0 => self.max_value = 100,
            None => {}
        }
        self.value = self.value.min(self.max_value as f32);
        self
    }

    /// The value rescaled to 0-10.
    pub fn on_ten_scale(&self) -> f32 {
        if self.max_value == 0 {
            0.0
        } else {
            self.value * 10.0 / self.max_value as f32
        }
    }
}

/// Ratings of one entity, keyed by rating id. Invalid ratings (blank id or negative value) are
/// dropped on insert and on load.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Ratings(BTreeMap<String, Rating>);

impl Ratings {
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts a rating under its id. Returns `false` when it is invalid and was not stored.
    pub fn insert(&mut self, rating: Rating) -> bool {
        if !rating.is_valid() {
            return false;
        }
        self.0.insert(rating.id.clone(), rating);
        true
    }

    pub fn get(&self, id: &str) -> Option<&Rating> {
        self.0.get(id)
    }

    /// Inserts the [normalised](Rating::normalised) rating. Returns `false` when it is invalid.
    pub fn insert_normalised(&mut self, rating: Rating) -> bool {
        self.insert(rating.normalised())
    }

    /// The user's own rating on the 0-10 scale, when one is set.
    pub fn user_rating(&self) -> Option<f32> {
        self.get(rating_key::USER).map(Rating::on_ten_scale)
    }

    /// Sets the user's own rating (0.5 to 10 in half steps).
    pub fn set_user_rating(&mut self, value_on_ten: f32) -> Result<(), UserRatingError> {
        self.insert(Rating::user(value_on_ten)?);
        Ok(())
    }

    /// Removes the user's own rating.
    pub fn clear_user_rating(&mut self) {
        self.0.remove(rating_key::USER);
    }

    /// A rating of `id` on the 0-10 scale (percentages and 5-point scales rescaled), for sorting
    /// and comparing sources.
    pub fn on_ten_scale(&self, id: &str) -> Option<f32> {
        self.get(id).map(Rating::on_ten_scale)
    }

    pub fn remove(&mut self, id: &str) -> Option<Rating> {
        self.0.remove(id)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Rating> {
        self.0.values()
    }
}

impl FromIterator<Rating> for Ratings {
    fn from_iter<T: IntoIterator<Item = Rating>>(iter: T) -> Self {
        let mut ratings = Self::new();
        for r in iter {
            ratings.insert(r);
        }
        ratings
    }
}

impl Serialize for Ratings {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(self.0.values())
    }
}

impl<'de> Deserialize<'de> for Ratings {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(Vec::<Rating>::deserialize(d)?.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_ratings_are_dropped() {
        let mut r = Ratings::new();
        assert!(r.insert(Rating::new("imdb", 8.7, 2_000_000, 10)));
        assert!(!r.insert(Rating::new("", 5.0, 1, 10)));
        assert!(!r.insert(Rating::new("tmdb", -1.0, 0, 10)));
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn round_trip_keeps_values() {
        let r: Ratings = [
            Rating::new("imdb", 8.7, 2_000_000, 10),
            Rating::new("rottentomatoes", 93.0, 500, 100),
            Rating::new(rating_key::USER, 9.5, 0, 10),
        ]
        .into_iter()
        .collect();
        let json = serde_json::to_string(&r).unwrap();
        let back: Ratings = serde_json::from_str(&json).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn loading_filters_invalid_entries_and_defaults_scale() {
        let r: Ratings = serde_json::from_str(
            r#"[{"id":"imdb","value":7.0},{"id":" ","value":3.0},{"id":"x","value":-1.0}]"#,
        )
        .unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r.get("imdb").unwrap().max_value, 10);
    }

    #[test]
    fn known_scales_are_applied() {
        let n = |id: &str, v: f32, max: u32| Rating::new(id, v, 3, max).normalised();
        // a value that fits is relabelled
        assert_eq!(
            n("letterboxd", 4.4, 10),
            Rating::new("letterboxd", 4.4, 3, 5)
        );
        assert_eq!(
            n("rogerebert", 3.5, 10),
            Rating::new("rogerebert", 3.5, 3, 4)
        );
        assert_eq!(
            n("metacritic", 71.0, 10),
            Rating::new("metacritic", 71.0, 3, 100)
        );
        // a value that does not fit is rescaled from the declared scale
        assert_eq!(n("letterboxd", 8.0, 10).value, 4.0);
        assert_eq!(n("tomatometerallcritics", 93.0, 100).value, 93.0);
        // moviemeter is out of 5, ids are canonical
        assert_eq!(n("moviemeter", 4.0, 10).max_value, 5);
        assert_eq!(n("themoviedb", 7.1, 10).id, "tmdb");
        assert_eq!(n("Tomatoes", 90.0, 10).id, "tomatometerallcritics");
        assert_eq!(n("metascore", 60.0, 10).max_value, 100);
    }

    #[test]
    fn unknown_sources_and_overshoot() {
        // a value above 10 on a default scale is a percentage
        assert_eq!(
            Rating::new("somewhere", 85.0, 0, 10).normalised().max_value,
            100
        );
        assert_eq!(
            Rating::new("somewhere", 7.0, 0, 10).normalised().max_value,
            10
        );
        // never above the scale
        assert_eq!(Rating::new("imdb", 11.0, 0, 10).normalised().value, 10.0);
        // invalid ratings stay invalid
        assert!(!Rating::new("imdb", -1.0, 0, 10).normalised().is_valid());
        let mut r = Ratings::new();
        assert!(r.insert_normalised(Rating::new("letterboxd", 4.4, 9, 10)));
        assert_eq!(r.get("letterboxd").unwrap().max_value, 5);
        assert_eq!(r.on_ten_scale("letterboxd"), Some(8.8));
    }

    #[test]
    fn user_rating_rules() {
        assert!(check_user_rating(0.5).is_ok() && check_user_rating(10.0).is_ok());
        assert!(check_user_rating(7.5).is_ok());
        assert_eq!(
            check_user_rating(0.0),
            Err(UserRatingError::OutOfRange {
                value: 0.0,
                max: 10
            })
        );
        assert!(check_user_rating(10.5).is_err() && check_user_rating(f32::NAN).is_err());
        assert_eq!(
            check_user_rating(7.3),
            Err(UserRatingError::NotHalfStep(7.3))
        );
        assert_eq!(user_rating_from_stars(3.5), Ok(7.0));
        assert_eq!(user_rating_from_stars(0.5), Ok(1.0));
        assert!(user_rating_from_stars(5.5).is_err() && user_rating_from_stars(2.25).is_err());

        let mut r = Ratings::new();
        assert_eq!(r.user_rating(), None);
        r.set_user_rating(9.0).unwrap();
        assert_eq!(r.user_rating(), Some(9.0));
        assert!(r.get("user").unwrap().is_user());
        assert!(r.set_user_rating(11.0).is_err());
        assert_eq!(r.user_rating(), Some(9.0));
        r.clear_user_rating();
        assert_eq!(r.user_rating(), None);
    }

    #[test]
    fn rescales_to_ten() {
        assert_eq!(Rating::new("rt", 93.0, 0, 100).on_ten_scale(), 9.3);
        assert_eq!(Rating::new("x", 4.0, 0, 5).on_ten_scale(), 8.0);
    }
}
