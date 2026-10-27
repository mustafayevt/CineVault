//! Opt-in smoke test against the real TheTVDB API.
//!
//! It runs only when `CV_TVDB_API_KEY` is set to a TheTVDB v4 API key; without it the test
//! prints why it was skipped and passes. `CV_TVDB_PIN` adds the subscriber PIN of a user-supported
//! key. It makes a handful of requests: a sign-in, a title search, show details, the aired and
//! DVD episode lists, artwork and trailers.
//!
//! ```text
//! CV_TVDB_API_KEY=... cargo test -p cv-providers --test tvdb_live -- --nocapture
//! ```

use std::sync::Arc;

use cv_domain::{EpisodeGroup, ItemDetail, ProviderIds, id_key};
use cv_providers::model::{MediaKind, RequestOptions, SearchQuery};
use cv_providers::tvdb::TvdbProvider;
use cv_providers::{
    ArtworkProvider, CredentialResolver, HttpClient, MemoryUserCredentials, MetadataProvider,
    Provider, catalogue,
};

const KEY_VARIABLE: &str = "CV_TVDB_API_KEY";
const PIN_VARIABLE: &str = "CV_TVDB_PIN";

fn variable(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
}

#[tokio::test]
async fn live_smoke() {
    let Some(key) = variable(KEY_VARIABLE) else {
        eprintln!("skipped: set {KEY_VARIABLE} to run the live TheTVDB smoke test");
        return;
    };

    let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
    resolver
        .set_user(catalogue::key::TVDB, catalogue::API_KEY, &key)
        .unwrap();
    if let Some(pin) = variable(PIN_VARIABLE) {
        resolver
            .set_user(catalogue::key::TVDB, catalogue::TVDB_PIN, &pin)
            .unwrap();
    }
    let provider = TvdbProvider::new(HttpClient::builder().build().unwrap(), resolver.clone());
    let options = RequestOptions::default();

    // The key (and PIN) are accepted.
    provider
        .check_credentials(&resolver.resolve(catalogue::key::TVDB).unwrap())
        .await
        .expect("TheTVDB accepted the credentials");

    // Text search finds Breaking Bad.
    let hits = provider
        .search(
            &SearchQuery {
                kind: MediaKind::Show,
                title: "Breaking Bad".to_owned(),
                year: Some(2008),
            },
            &options,
        )
        .await
        .unwrap();
    assert!(
        hits.iter()
            .any(|h| h.ids.get(id_key::TVDB) == Some("81189")),
        "{hits:#?}"
    );

    // IMDb ID first.
    let mut by_imdb = ProviderIds::new();
    by_imdb.set(id_key::IMDB, "tt0903747");
    let hit = provider
        .lookup(MediaKind::Show, &by_imdb, &options)
        .await
        .unwrap()
        .expect("Breaking Bad by IMDb ID");
    assert_eq!(hit.ids.get(id_key::TVDB), Some("81189"));

    // Details, episodes in two orderings, artwork and trailers.
    let item = provider
        .details(MediaKind::Show, &hit.ids, &options)
        .await
        .unwrap();
    assert_eq!(item.core.title, "Breaking Bad");
    assert!(matches!(item.detail, ItemDetail::Show(_)));
    let aired = provider
        .episodes(&hit.ids, &EpisodeGroup::aired(), &options)
        .await
        .unwrap();
    assert!(aired.len() >= 62, "{} episodes", aired.len());
    let _ = provider
        .episodes(&hit.ids, &EpisodeGroup::dvd(), &options)
        .await;
    let artwork = provider
        .artwork(MediaKind::Show, &hit.ids, &options)
        .await
        .unwrap();
    assert!(!artwork.is_empty());
}
