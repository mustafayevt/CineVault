//! Opt-in smoke test against the real OMDb API.
//!
//! It runs only when `CV_OMDB_API_KEY` is set to an OMDb key; without it the test prints why it
//! was skipped and passes. It makes a handful of requests (the free key allows 1,000 a day): a
//! credential check, a title search, one title with its ratings and one series.
//!
//! ```text
//! CV_OMDB_API_KEY=... cargo test -p cv-providers --test omdb_live -- --nocapture
//! ```

use std::sync::Arc;

use cv_domain::{ItemDetail, ProviderIds, id_key};
use cv_providers::model::{MediaKind, RequestOptions, SearchQuery};
use cv_providers::omdb::{IMDB_RATING_ID, OmdbProvider};
use cv_providers::{
    CredentialResolver, HttpClient, MemoryUserCredentials, MetadataProvider, Provider,
    RatingProvider, catalogue,
};

const KEY_VARIABLE: &str = "CV_OMDB_API_KEY";

#[tokio::test]
async fn live_smoke() {
    let Some(key) = std::env::var(KEY_VARIABLE)
        .ok()
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
    else {
        eprintln!("skipped: set {KEY_VARIABLE} to run the live OMDb smoke test");
        return;
    };
    let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
    resolver
        .set_user(catalogue::key::OMDB, catalogue::API_KEY, &key)
        .unwrap();
    let provider = OmdbProvider::new(HttpClient::builder().build().unwrap(), resolver.clone());
    let options = RequestOptions::default();

    provider
        .check_credentials(&resolver.resolve(catalogue::key::OMDB).unwrap())
        .await
        .expect("OMDb accepted the key");

    let hits = provider
        .search(
            &SearchQuery {
                kind: MediaKind::Movie,
                title: "Heat".to_owned(),
                year: Some(1995),
            },
            &options,
        )
        .await
        .unwrap();
    assert!(
        hits.iter()
            .any(|h| h.ids.get(id_key::IMDB) == Some("tt0113277")),
        "{hits:#?}"
    );

    let mut heat = ProviderIds::new();
    heat.set(id_key::IMDB, "tt0113277");
    let item = provider
        .details(MediaKind::Movie, &heat, &options)
        .await
        .unwrap();
    assert_eq!(item.core.title, "Heat");
    assert_eq!(item.core.year, 1995);
    let ratings = provider
        .ratings(MediaKind::Movie, &heat, &options)
        .await
        .unwrap();
    assert!(ratings.get(IMDB_RATING_ID).is_some());

    let mut series = ProviderIds::new();
    series.set(id_key::IMDB, "tt0903747");
    let show = provider
        .details(MediaKind::Show, &series, &options)
        .await
        .unwrap();
    assert!(matches!(show.detail, ItemDetail::Show(_)));
}
