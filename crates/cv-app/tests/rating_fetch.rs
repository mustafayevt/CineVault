//! Ratings fetched for a stored item are written through the scrape write path, so the
//! user's rating survives, locks hold, and a source without credentials is skipped with a notice.
//! The sources are in-memory fakes, or a real provider without a key (which never sends a
//! request), so nothing here touches the network.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use cv_app::ratings::{RatingFetchError, fetch_and_store_ratings, rating_settings_from_values};
use cv_db::Database;
use cv_db::repo::{get_item, insert_item, set_field_lock, set_item_locked};
use cv_domain::{Item, ItemId, MetaField, ProviderIds, Rating, Ratings, ScrapeBlock};
use cv_providers::model::{MediaKind, RequestOptions};
use cv_providers::{
    CredentialResolver, HttpClient, MemoryUserCredentials, OmdbProvider, Provider, ProviderError,
    RatingFetcher, RatingProvider, RatingSource, SkipReason, SourceStatus,
};

struct Fixed {
    id: &'static str,
    ratings: Vec<Rating>,
}

#[async_trait]
impl Provider for Fixed {
    fn id(&self) -> &'static str {
        self.id
    }
    fn name(&self) -> &'static str {
        self.id
    }
}

#[async_trait]
impl RatingProvider for Fixed {
    async fn ratings(
        &self,
        _kind: MediaKind,
        _ids: &ProviderIds,
        _options: &RequestOptions,
    ) -> Result<Ratings, ProviderError> {
        Ok(self.ratings.iter().cloned().collect())
    }
}

fn fixed(id: &'static str, ratings: &[Rating]) -> Arc<Fixed> {
    Arc::new(Fixed {
        id,
        ratings: ratings.to_vec(),
    })
}

fn fetcher() -> RatingFetcher {
    let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
    RatingFetcher::new(resolver)
        .with_omdb(fixed(
            "omdb",
            &[
                Rating::new("imdb", 8.3, 700_123, 10),
                Rating::new("metacritic", 76.0, 0, 100),
            ],
        ))
        .with_mdblist(fixed(
            "mdblist",
            &[Rating::new("letterboxd", 4.1, 640_000, 10)],
        ))
        .with_dataset(fixed("dataset", &[Rating::new("imdb", 8.3, 710_000, 10)]))
}

fn library() -> (tempfile::TempDir, Database, ItemId) {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(dir.path().join("library.db")).unwrap();
    let mut item = Item::movie("Heat");
    item.core.ids.set("imdb", "tt0113277");
    item.core.ratings.set_user_rating(9.0).unwrap();
    item.core.ratings.insert(Rating::new("trakt", 6.0, 1, 10));
    let id = db.write(move |tx| insert_item(tx, &item)).unwrap();
    (dir, db, id)
}

fn settings(sources: &[RatingSource]) -> cv_providers::RatingFetchSettings {
    cv_providers::RatingFetchSettings {
        sources: sources.to_vec(),
        after_scrape: false,
    }
}

#[tokio::test]
async fn fetched_ratings_are_stored_next_to_the_user_rating_and_the_other_sources() {
    let (_dir, db, id) = library();
    let stored = fetch_and_store_ratings(
        &db,
        &fetcher(),
        None,
        id,
        &settings(&RatingSource::ALL[1..]),
        &RequestOptions::default(),
    )
    .await
    .unwrap();
    assert!(stored.outcome.unwrap().written.contains(&MetaField::Rating));

    let item = db.read(|c| get_item(c, id)).unwrap().unwrap();
    let ratings = &item.core.ratings;
    assert_eq!(ratings.user_rating(), Some(9.0));
    assert_eq!(
        ratings.get("trakt").unwrap().value,
        6.0,
        "a source nobody answered stays"
    );
    assert_eq!(ratings.get("imdb").unwrap().value, 8.3);
    assert_eq!(ratings.get("metacritic").unwrap().max_value, 100);
    let letterboxd = ratings.get("letterboxd").unwrap();
    assert_eq!((letterboxd.value, letterboxd.max_value), (4.1, 5));
}

#[tokio::test]
async fn a_fetched_rating_is_stamped_with_the_fetch_time_and_others_keep_theirs() {
    let (_dir, db, id) = library();
    let stored_date = |source: &str| {
        let item = db.read(|c| get_item(c, id)).unwrap().unwrap();
        item.core.ratings.get(source).unwrap().fetched_at_ms
    };
    // The trakt rating was stored with the item; its date is when that happened.
    let trakt_before = stored_date("trakt");
    assert!(trakt_before > 0);
    std::thread::sleep(std::time::Duration::from_millis(5));
    let before_fetch = trakt_before + 1;

    fetch_and_store_ratings(
        &db,
        &fetcher(),
        None,
        id,
        &settings(&RatingSource::ALL[1..]),
        &RequestOptions::default(),
    )
    .await
    .unwrap();

    assert!(
        stored_date("imdb") >= before_fetch,
        "a fetched rating has the fetch time"
    );
    assert!(stored_date("letterboxd") >= before_fetch);
    assert_eq!(
        stored_date("trakt"),
        trakt_before,
        "a source nobody answered keeps its date"
    );
}

#[tokio::test]
async fn a_source_without_credentials_is_skipped_and_the_rest_are_stored() {
    let (_dir, db, id) = library();
    let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
    // A real OMDb provider with no key: it must not send anything.
    let omdb = OmdbProvider::with_base_url(
        HttpClient::builder().build().unwrap(),
        resolver.clone(),
        "http://127.0.0.1:9",
    );
    let fetcher = RatingFetcher::new(resolver)
        .with_omdb(Arc::new(omdb))
        .with_dataset(fixed("dataset", &[Rating::new("imdb", 8.3, 710_000, 10)]));

    let stored = fetch_and_store_ratings(
        &db,
        &fetcher,
        None,
        id,
        &settings(&[RatingSource::Omdb, RatingSource::ImdbDataset]),
        &RequestOptions::default(),
    )
    .await
    .unwrap();
    let omdb_outcome = stored
        .fetched
        .outcomes
        .iter()
        .find(|o| o.source == RatingSource::Omdb)
        .unwrap();
    assert!(matches!(
        omdb_outcome.status,
        SourceStatus::Skipped(SkipReason::MissingCredentials { .. })
    ));
    assert_eq!(stored.fetched.notices().len(), 1);
    let item = db.read(|c| get_item(c, id)).unwrap().unwrap();
    assert_eq!(item.core.ratings.get("imdb").unwrap().votes, 710_000);
    assert!(item.core.ratings.get("metacritic").is_none());
}

#[tokio::test]
async fn a_locked_item_or_a_locked_rating_field_is_left_unchanged() {
    let sources = settings(&RatingSource::ALL[1..]);

    let (_dir, db, id) = library();
    db.write(move |tx| set_item_locked(tx, &[id], true).map(|_| ()))
        .unwrap();
    let stored = fetch_and_store_ratings(
        &db,
        &fetcher(),
        None,
        id,
        &sources,
        &RequestOptions::default(),
    )
    .await
    .unwrap();
    assert_eq!(
        stored.outcome.unwrap().blocked,
        Some(ScrapeBlock::ItemLocked)
    );
    let item = db.read(|c| get_item(c, id)).unwrap().unwrap();
    assert!(item.core.ratings.get("imdb").is_none());

    let (_dir, db, id) = library();
    db.write(move |tx| set_field_lock(tx, id, MetaField::Rating, true).map(|_| ()))
        .unwrap();
    let stored = fetch_and_store_ratings(
        &db,
        &fetcher(),
        None,
        id,
        &sources,
        &RequestOptions::default(),
    )
    .await
    .unwrap();
    assert_eq!(stored.outcome.unwrap().locked, [MetaField::Rating]);
    let item = db.read(|c| get_item(c, id)).unwrap().unwrap();
    assert!(item.core.ratings.get("imdb").is_none());
    assert_eq!(item.core.ratings.user_rating(), Some(9.0));
}

#[tokio::test]
async fn nothing_is_written_when_no_source_has_a_rating() {
    let (_dir, db, id) = library();
    let empty = RatingFetcher::new(CredentialResolver::new(Arc::new(
        MemoryUserCredentials::default(),
    )))
    .with_dataset(fixed("dataset", &[]));
    let stored = fetch_and_store_ratings(
        &db,
        &empty,
        None,
        id,
        &settings(&[RatingSource::ImdbDataset]),
        &RequestOptions::default(),
    )
    .await
    .unwrap();
    assert!(stored.outcome.is_none());
}

#[tokio::test]
async fn missing_items_and_unsupported_kinds_are_errors() {
    let (_dir, db, id) = library();
    let options = RequestOptions::default();
    let sources = settings(&[RatingSource::ImdbDataset]);

    let missing =
        fetch_and_store_ratings(&db, &fetcher(), None, ItemId(9_999), &sources, &options).await;
    assert!(matches!(missing, Err(RatingFetchError::NotFound)));

    let show = db
        .write(|tx| insert_item(tx, &Item::show("Some show")))
        .unwrap();
    let episode = db
        .write(move |tx| insert_item(tx, &Item::episode(show, "Pilot")))
        .unwrap();
    let unsupported =
        fetch_and_store_ratings(&db, &fetcher(), None, episode, &sources, &options).await;
    assert!(matches!(
        unsupported,
        Err(RatingFetchError::UnsupportedKind(_))
    ));
    let _ = id;
}

#[test]
fn the_settings_map_drives_the_fetch_settings() {
    let values: BTreeMap<String, serde_json::Value> = [(
        "ratings.sources-movie".to_owned(),
        serde_json::json!("omdb,mdblist"),
    )]
    .into_iter()
    .collect();
    let settings = rating_settings_from_values(&values);
    assert_eq!(
        settings.movie.sources,
        [RatingSource::Omdb, RatingSource::Mdblist]
    );
}
