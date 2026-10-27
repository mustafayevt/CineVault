//! Opt-in smoke test against the real AniDB service.
//!
//! It runs only when `CV_ANIDB_CLIENT` is set to the name of an AniDB HTTP API client you
//! registered (`CV_ANIDB_CLIENT_VERSION` for its version, `1` when unset); without it the test
//! prints why it was skipped and passes. AniDB bans clients that send unregistered or too many
//! requests and allows one download of the title dump a day, so this makes exactly two
//! requests, two seconds apart: the title dump (a search) and one anime.
//!
//! ```text
//! CV_ANIDB_CLIENT=... cargo test -p cv-providers --test anidb_live -- --nocapture
//! ```

use std::sync::Arc;

use cv_domain::ItemDetail;
use cv_providers::AnidbProvider;
use cv_providers::model::{MediaKind, RequestOptions, SearchQuery};
use cv_providers::{
    CredentialResolver, HttpClient, MemoryUserCredentials, MetadataProvider, catalogue,
};

#[tokio::test]
async fn live_smoke() {
    let Some(client) = std::env::var("CV_ANIDB_CLIENT")
        .ok()
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
    else {
        eprintln!("skipped: set CV_ANIDB_CLIENT to run the live AniDB smoke test");
        return;
    };
    let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
    resolver
        .set_user(catalogue::key::ANIDB, catalogue::API_KEY, &client)
        .unwrap();
    if let Ok(version) = std::env::var("CV_ANIDB_CLIENT_VERSION")
        && !version.trim().is_empty()
    {
        resolver
            .set_user(
                catalogue::key::ANIDB,
                catalogue::ANIDB_CLIENT_VERSION,
                version.trim(),
            )
            .unwrap();
    }
    let provider = AnidbProvider::new(HttpClient::builder().build().unwrap(), resolver);
    let options = RequestOptions::default();

    let hits = provider
        .search(
            &SearchQuery {
                kind: MediaKind::Show,
                title: "Cowboy Bebop".to_owned(),
                year: None,
            },
            &options,
        )
        .await
        .expect("the title dump is readable");
    let bebop = hits
        .iter()
        .find(|h| h.title.eq_ignore_ascii_case("Cowboy Bebop"))
        .expect("Cowboy Bebop is in the dump");

    let item = provider
        .details(MediaKind::Show, &bebop.ids, &options)
        .await
        .expect("the anime is readable");
    assert!(matches!(item.detail, ItemDetail::Show(_)));
    assert_eq!(item.core.year, 1998);
}
