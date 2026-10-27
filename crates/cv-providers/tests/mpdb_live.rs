//! Opt-in smoke test against the real MPDb.tv service.
//!
//! It runs only when `CV_MPDB_API_KEY`, `CV_MPDB_USERNAME` and `CV_MPDB_SUBSCRIPTION_KEY` are all
//! set; without them the test prints why it was skipped and passes. MPDb.tv is a private service
//! with no published schema and the provider reads its movie JSON leniently, so run this when a
//! subscription is at hand to confirm the field names. It makes four requests: a credential
//! check, a search, one movie and its artwork.
//!
//! ```text
//! CV_MPDB_API_KEY=... CV_MPDB_USERNAME=... CV_MPDB_SUBSCRIPTION_KEY=... \
//!     cargo test -p cv-providers --test mpdb_live -- --nocapture
//! ```

use std::sync::Arc;

use cv_domain::ItemDetail;
use cv_providers::model::{MediaKind, RequestOptions, SearchQuery};
use cv_providers::mpdb::MpdbProvider;
use cv_providers::{
    ArtworkProvider, CredentialResolver, HttpClient, MemoryUserCredentials, MetadataProvider,
    Provider, catalogue,
};

fn variable(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
}

#[tokio::test]
async fn live_smoke() {
    let (Some(key), Some(user), Some(subscription)) = (
        variable("CV_MPDB_API_KEY"),
        variable("CV_MPDB_USERNAME"),
        variable("CV_MPDB_SUBSCRIPTION_KEY"),
    ) else {
        eprintln!(
            "skipped: set CV_MPDB_API_KEY, CV_MPDB_USERNAME and CV_MPDB_SUBSCRIPTION_KEY to run the live MPDb.tv smoke test"
        );
        return;
    };
    let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
    for (field, value) in [
        (catalogue::API_KEY, key),
        (catalogue::MPDB_USERNAME, user),
        (catalogue::MPDB_SUBSCRIPTION_KEY, subscription),
    ] {
        resolver
            .set_user(catalogue::key::MPDB, field, &value)
            .unwrap();
    }
    let provider = MpdbProvider::new(HttpClient::builder().build().unwrap(), resolver.clone());
    let options = RequestOptions::default();

    provider
        .check_credentials(&resolver.resolve(catalogue::key::MPDB).unwrap())
        .await
        .expect("MPDb.tv accepted the account");

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
    assert!(matches!(item.detail, ItemDetail::Movie(_)));
    let artwork = provider
        .artwork(MediaKind::Movie, &hits[0].ids, &options)
        .await
        .unwrap();
    eprintln!("{} artwork candidates", artwork.len());
}
