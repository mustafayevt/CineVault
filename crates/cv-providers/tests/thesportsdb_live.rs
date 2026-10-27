//! Opt-in smoke test against the real TheSportsDB API.
//!
//! It runs only when `CV_THESPORTSDB_LIVE=1`; without it the test prints why it was skipped and
//! passes. It uses the service's public test key (or `CV_THESPORTSDB_API_KEY` when set), which
//! lists only a few leagues, so it looks the English Premier League (4328) up by ID. It makes
//! five requests: a credential check, a league lookup, its seasons, one season of events and the
//! league list.
//!
//! ```text
//! CV_THESPORTSDB_LIVE=1 cargo test -p cv-providers --test thesportsdb_live -- --nocapture
//! ```

use std::sync::Arc;

use cv_domain::{EpisodeGroup, ItemDetail, ProviderIds};
use cv_providers::model::{MediaKind, RequestOptions};
use cv_providers::thesportsdb::{ID_KEY, TheSportsDbProvider};
use cv_providers::{
    CredentialResolver, HttpClient, MemoryUserCredentials, MetadataProvider, Provider, catalogue,
};

#[tokio::test]
async fn live_smoke() {
    if std::env::var("CV_THESPORTSDB_LIVE").ok().as_deref() != Some("1") {
        eprintln!("skipped: set CV_THESPORTSDB_LIVE=1 to run the live TheSportsDB smoke test");
        return;
    }
    let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
    if let Ok(key) = std::env::var("CV_THESPORTSDB_API_KEY")
        && !key.trim().is_empty()
    {
        resolver
            .set_user(catalogue::key::THESPORTSDB, catalogue::API_KEY, key.trim())
            .unwrap();
    }
    let provider =
        TheSportsDbProvider::new(HttpClient::builder().build().unwrap(), resolver.clone());
    let options = RequestOptions::default();

    provider
        .check_credentials(&resolver.resolve(catalogue::key::THESPORTSDB).unwrap())
        .await
        .expect("TheSportsDB accepted the key");

    let mut ids = ProviderIds::new();
    ids.set(ID_KEY, "4328");
    let item = provider
        .details(MediaKind::Show, &ids, &options)
        .await
        .expect("the league is readable");
    assert_eq!(item.core.title, "English Premier League");
    assert!(matches!(item.detail, ItemDetail::Show(_)));

    let episodes = provider
        .episodes(&ids, &EpisodeGroup::aired(), &options)
        .await
        .expect("the events are readable");
    assert!(!episodes.is_empty());
}
