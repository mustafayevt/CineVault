//! Opt-in smoke test against the real MovieMeter API.
//!
//! It runs only when `CV_MOVIEMETER_API_KEY` is set to a MovieMeter key; without it the test
//! prints why it was skipped and passes. The provider reads MovieMeter's film JSON leniently
//! and its fixtures are hand-built, so run this when a key is at hand to confirm the field
//! names. It makes three requests: a credential check, a search and one film.
//!
//! ```text
//! CV_MOVIEMETER_API_KEY=... cargo test -p cv-providers --test moviemeter_live -- --nocapture
//! ```

use std::sync::Arc;

use cv_domain::ItemDetail;
use cv_providers::model::{MediaKind, RequestOptions, SearchQuery};
use cv_providers::moviemeter::{ID_KEY, MovieMeterProvider};
use cv_providers::{
    CredentialResolver, HttpClient, MemoryUserCredentials, MetadataProvider, Provider, catalogue,
};

const KEY_VARIABLE: &str = "CV_MOVIEMETER_API_KEY";

#[tokio::test]
async fn live_smoke() {
    let Some(key) = std::env::var(KEY_VARIABLE)
        .ok()
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
    else {
        eprintln!("skipped: set {KEY_VARIABLE} to run the live MovieMeter smoke test");
        return;
    };
    let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
    resolver
        .set_user(catalogue::key::MOVIEMETER, catalogue::API_KEY, &key)
        .unwrap();
    let provider =
        MovieMeterProvider::new(HttpClient::builder().build().unwrap(), resolver.clone());
    let options = RequestOptions::default();

    provider
        .check_credentials(&resolver.resolve(catalogue::key::MOVIEMETER).unwrap())
        .await
        .expect("MovieMeter accepted the key");

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
    assert!(!hits.is_empty(), "no hits for Heat");
    let item = provider
        .details(MediaKind::Movie, &hits[0].ids, &options)
        .await
        .unwrap();
    assert!(!item.core.title.is_empty());
    assert!(item.core.ids.get(ID_KEY).is_some());
    assert!(matches!(item.detail, ItemDetail::Movie(_)));
}
