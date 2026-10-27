//! Rating fetch settings and the stored fetch.
//!
//! The settings are read from the settings map, like the scrape options:
//!
//! | Key | Values | Meaning |
//! |---|---|---|
//! | `ratings.sources-movie` | comma-separated source keys (default `imdb-dataset,omdb,mdblist`) | where "Fetch ratings" and the automatic fetch look for a movie; a source without its key is skipped |
//! | `ratings.sources-show` | same (same default) | the same for shows |
//! | `ratings.fetch-after-scrape-movie` | bool (default on) | fetch ratings after a movie scrape |
//! | `ratings.fetch-after-scrape-show` | bool (default on) | fetch ratings after a show scrape |
//! | `ratings.preferred-source` | rating source key (default `tmdb`) | the source of the "rating" column, rating sorts, the `rating:` filter and the NFO default rating; items without it show the next available source |
//!
//! The source keys are `scraper`, `omdb`, `mdblist` and `imdb-dataset`
//! ([`cv_providers::RatingSource::key`]); unknown keys are ignored.
//!
//! [`fetch_and_store_ratings`] fetches the ratings of one stored item and writes them through the
//! scrape write path, so the entity lock and the rating field lock hold, a source that answered
//! replaces only its own rows, and the user's own rating is never touched.

use std::collections::BTreeMap;

use cv_db::repo::{apply_scrape, get_item};
use cv_db::{Database, DbError};
use cv_domain::{
    Item, ItemId, ItemKind, MetaField, RatingPreference, ScrapeOptions, ScrapeOutcome,
};
use cv_providers::model::RequestOptions;
use cv_providers::ratings::media_kind_of;
use cv_providers::{
    FetchedRatings, RatingFetchSettings, RatingFetcher, RatingProvider, RatingSettings,
    RatingSource,
};
use serde_json::Value;

type Values = BTreeMap<String, Value>;

fn sources(values: &Values, key: &str, default: &[RatingSource]) -> Vec<RatingSource> {
    match values.get(key).and_then(Value::as_str) {
        Some(text) => RatingSource::parse_list(text),
        None => default.to_vec(),
    }
}

fn toggle(values: &Values, key: &str, default: bool) -> bool {
    values.get(key).and_then(Value::as_bool).unwrap_or(default)
}

/// Reads the rating fetch settings; anything missing or of the wrong type takes its default.
pub fn rating_settings_from_values(values: &Values) -> RatingSettings {
    let defaults = RatingSettings::default();
    RatingSettings {
        movie: RatingFetchSettings {
            sources: sources(values, "ratings.sources-movie", &defaults.movie.sources),
            after_scrape: toggle(
                values,
                "ratings.fetch-after-scrape-movie",
                defaults.movie.after_scrape,
            ),
        },
        show: RatingFetchSettings {
            sources: sources(values, "ratings.sources-show", &defaults.show.sources),
            after_scrape: toggle(
                values,
                "ratings.fetch-after-scrape-show",
                defaults.show.after_scrape,
            ),
        },
    }
}

/// The rating source a provider's key unlocks: OMDb and MDBList.
fn source_of_provider(provider: &str) -> Option<RatingSource> {
    match provider {
        cv_providers::catalogue::key::OMDB => Some(RatingSource::Omdb),
        cv_providers::catalogue::key::MDBLIST => Some(RatingSource::Mdblist),
        _ => None,
    }
}

/// Turns on the rating source a newly working key unlocks (OMDb or MDBList) for movies and shows.
/// Only a list the user stored is changed: the default asks both sources already. A source that
/// is in the list stays where it is. Returns the setting keys that changed.
///
/// # Errors
/// The settings could not be saved.
pub fn choose_source_for_key(
    settings: &crate::app_settings::SettingsService,
    provider: &str,
) -> Result<Vec<&'static str>, crate::app_settings::AppSettingsError> {
    let Some(source) = source_of_provider(provider) else {
        return Ok(Vec::new());
    };
    let values = settings.values();
    let mut set = Vec::new();
    let mut changed = Vec::new();
    for key in ["ratings.sources-movie", "ratings.sources-show"] {
        let Some(text) = values.get(key).and_then(Value::as_str) else {
            continue;
        };
        let mut sources = RatingSource::parse_list(text);
        if sources.contains(&source) {
            continue;
        }
        sources.push(source);
        set.push((
            key.to_owned(),
            Value::from(RatingSource::list_key(&sources)),
        ));
        changed.push(key);
    }
    if !set.is_empty() {
        settings.apply(set, &[])?;
    }
    Ok(changed)
}

/// The preferred rating source from the settings; a missing, mistyped or unusable value gives the
/// default (TMDB).
pub fn rating_preference_from_values(values: &Values) -> RatingPreference {
    values
        .get("ratings.preferred-source")
        .and_then(Value::as_str)
        .map(RatingPreference::new)
        .unwrap_or_default()
}

/// Why the ratings of an item could not be fetched and stored.
#[derive(Debug, thiserror::Error)]
pub enum RatingFetchError {
    /// The item does not exist.
    #[error("the item does not exist")]
    NotFound,
    /// Only movies and shows have fetched ratings.
    #[error("ratings are only fetched for movies and shows")]
    UnsupportedKind(ItemKind),
    /// The library could not be read or written.
    #[error(transparent)]
    Storage(#[from] DbError),
}

/// What a stored fetch did.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredRatings {
    /// What each source answered.
    pub fetched: FetchedRatings,
    /// What the write did; `None` when no source had a rating to write. `blocked` is set when
    /// the item is locked, and `locked` holds the rating field when it is locked.
    pub outcome: Option<ScrapeOutcome>,
}

/// Fetches the ratings of the stored item `id` from the sources in `settings` and stores them.
///
/// `scraper` is the rating side of the item's scraping provider, used for the `scraper` source.
/// The write adds or replaces one row per answering source; other sources and the user's own
/// rating stay. A locked item or a locked rating field is left unchanged (see
/// [`StoredRatings::outcome`]).
pub async fn fetch_and_store_ratings(
    db: &Database,
    fetcher: &RatingFetcher,
    scraper: Option<&dyn RatingProvider>,
    id: ItemId,
    settings: &RatingFetchSettings,
    options: &RequestOptions,
) -> Result<StoredRatings, RatingFetchError> {
    let item = db
        .read(|conn| get_item(conn, id))?
        .ok_or(RatingFetchError::NotFound)?;
    let kind = media_kind_of(item.kind()).ok_or(RatingFetchError::UnsupportedKind(item.kind()))?;
    let fetched = fetcher
        .fetch(kind, &item.core.ids, scraper, &settings.sources, options)
        .await;
    if !fetched.found_any() {
        return Ok(StoredRatings {
            fetched,
            outcome: None,
        });
    }
    let mut carrier = match item.kind() {
        ItemKind::Show => Item::show(""),
        _ => Item::movie(""),
    };
    carrier.core.ratings = fetched.ratings.clone();
    let write_options = ScrapeOptions::only([MetaField::Rating]);
    let outcome = db.write(move |tx| apply_scrape(tx, id, &carrier, &write_options))?;
    Ok(StoredRatings {
        fetched,
        outcome: Some(outcome),
    })
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
    fn the_defaults_fetch_movies_and_shows_after_a_scrape() {
        let s = rating_settings_from_values(&Values::new());
        assert_eq!(s, RatingSettings::default());
        assert!(s.movie.after_scrape && s.show.after_scrape);
        assert_eq!(s.movie.sources, RatingFetchSettings::DEFAULT_SOURCES);
        assert_eq!(s.show.sources, RatingFetchSettings::DEFAULT_SOURCES);
    }

    #[test]
    fn chosen_sources_and_toggles_are_read() {
        let s = rating_settings_from_values(&values(&[
            ("ratings.sources-movie", json!("omdb,mdblist,bogus,omdb")),
            ("ratings.sources-show", json!("")),
            ("ratings.fetch-after-scrape-movie", json!(false)),
            ("ratings.fetch-after-scrape-show", json!(true)),
        ]));
        assert_eq!(s.movie.sources, [RatingSource::Omdb, RatingSource::Mdblist]);
        assert!(s.show.sources.is_empty());
        assert!(!s.movie.after_scrape && s.show.after_scrape);
    }

    #[test]
    fn the_preferred_source_is_read_and_defaults_to_tmdb() {
        assert_eq!(
            rating_preference_from_values(&Values::new()).preferred(),
            "tmdb"
        );
        let chosen = values(&[("ratings.preferred-source", json!("IMDb"))]);
        assert_eq!(rating_preference_from_values(&chosen).preferred(), "imdb");
        for bad in [json!(3), json!(""), json!("user"), json!("a b")] {
            let v = values(&[("ratings.preferred-source", bad)]);
            assert_eq!(rating_preference_from_values(&v).preferred(), "tmdb");
        }
    }

    #[test]
    fn a_new_mdblist_key_turns_mdblist_on_where_the_user_chose_the_sources() {
        let settings = crate::app_settings::SettingsService::in_memory("");
        // Nothing stored: the default asks MDBList already, nothing is written.
        assert!(
            choose_source_for_key(&settings, "mdblist")
                .unwrap()
                .is_empty()
        );
        assert_eq!(settings.get("ratings.sources-movie"), None);

        settings
            .set("ratings.sources-movie", json!("imdb-dataset"))
            .unwrap();
        settings
            .set("ratings.sources-show", json!("imdb-dataset,mdblist"))
            .unwrap();
        assert_eq!(
            choose_source_for_key(&settings, "mdblist").unwrap(),
            ["ratings.sources-movie"]
        );
        assert_eq!(
            settings.get("ratings.sources-movie"),
            Some(json!("imdb-dataset,mdblist"))
        );
        assert_eq!(
            settings.get("ratings.sources-show"),
            Some(json!("imdb-dataset,mdblist"))
        );
        // A key that unlocks no rating source changes nothing.
        assert!(choose_source_for_key(&settings, "tmdb").unwrap().is_empty());
    }

    #[test]
    fn wrong_types_fall_back_to_the_defaults() {
        let s = rating_settings_from_values(&values(&[
            ("ratings.sources-movie", json!(7)),
            ("ratings.fetch-after-scrape-movie", json!("yes")),
        ]));
        assert_eq!(s, RatingSettings::default());
    }
}
