//! Opt-in smoke test against the real TMDB API.
//!
//! It runs only when `CV_TMDB_API_KEY` is set to a TMDB API key (v3 "API Key", not the read
//! access token); without it the test prints why it was skipped and passes. It makes a handful
//! of requests: a configuration check, an IMDb lookup, a title search, movie details and
//! trailers.
//!
//! ```text
//! CV_TMDB_API_KEY=... cargo test -p cv-providers --test tmdb_live -- --nocapture
//! ```

use std::sync::Arc;

use cv_domain::{ItemDetail, ProviderIds, id_key};
use cv_providers::model::{MediaKind, RequestOptions, SearchQuery};
use cv_providers::tmdb::TmdbProvider;
use cv_providers::{
    ArtworkProvider, CredentialResolver, HttpClient, MemoryUserCredentials, MetadataProvider,
    Provider, catalogue,
};

const KEY_VARIABLE: &str = "CV_TMDB_API_KEY";

#[tokio::test]
async fn live_smoke() {
    let Some(key) = std::env::var(KEY_VARIABLE)
        .ok()
        .map(|k| k.trim().to_owned())
        .filter(|k| !k.is_empty())
    else {
        eprintln!("skipped: set {KEY_VARIABLE} to run the live TMDB smoke test");
        return;
    };

    let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
    resolver
        .set_user(catalogue::key::TMDB, catalogue::API_KEY, &key)
        .unwrap();
    let provider = TmdbProvider::new(HttpClient::builder().build().unwrap(), resolver.clone());
    let options = RequestOptions::default();

    // The key is accepted.
    provider
        .check_credentials(&resolver.resolve(catalogue::key::TMDB).unwrap())
        .await
        .expect("TMDB accepted the key");

    // IMDb ID first: Heat (1995).
    let mut ids = ProviderIds::new();
    ids.set(id_key::IMDB, "tt0113277");
    let hit = provider
        .lookup(MediaKind::Movie, &ids, &options)
        .await
        .unwrap()
        .expect("Heat by IMDb ID");
    assert_eq!(hit.ids.get(id_key::TMDB), Some("949"));
    assert_eq!(hit.year, 1995);

    // Text search finds it too.
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
    assert!(hits.iter().any(|h| h.ids.get(id_key::TMDB) == Some("949")));

    // Details.
    let item = provider
        .details(MediaKind::Movie, &hit.ids, &options)
        .await
        .unwrap();
    assert_eq!(item.core.title, "Heat");
    assert_eq!(item.core.ids.get(id_key::IMDB), Some("tt0113277"));
    assert!(matches!(item.detail, ItemDetail::Movie(_)));
    assert!(!item.core.credits.is_empty());

    // Artwork and trailers.
    let artwork = provider
        .artwork(MediaKind::Movie, &hit.ids, &options)
        .await
        .unwrap();
    assert!(!artwork.is_empty());
}
