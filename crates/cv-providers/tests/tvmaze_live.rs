//! Opt-in smoke test against the real TVmaze API.
//!
//! It runs only when `CV_TVMAZE_LIVE=1`; without it the test prints why it was skipped and
//! passes. TVmaze needs no key. It makes a handful of requests: a search, an IMDb lookup, the
//! show with its embedded people, and the aired and one alternate episode list.
//!
//! ```text
//! CV_TVMAZE_LIVE=1 cargo test -p cv-providers --test tvmaze_live -- --nocapture
//! ```

use cv_domain::{EpisodeGroup, ItemDetail, ProviderIds, id_key};
use cv_providers::model::{MediaKind, RequestOptions, SearchQuery};
use cv_providers::tvmaze::TvmazeProvider;
use cv_providers::{HttpClient, MetadataProvider};

#[tokio::test]
async fn live_smoke() {
    if std::env::var("CV_TVMAZE_LIVE").ok().as_deref() != Some("1") {
        eprintln!("skipped: set CV_TVMAZE_LIVE=1 to run the live TVmaze smoke test");
        return;
    }
    let provider = TvmazeProvider::new(HttpClient::builder().build().unwrap());
    let options = RequestOptions::default();

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
            .any(|h| h.ids.get(id_key::IMDB) == Some("tt0903747")),
        "{hits:#?}"
    );

    let mut by_imdb = ProviderIds::new();
    by_imdb.set(id_key::IMDB, "tt0903747");
    let hit = provider
        .lookup(MediaKind::Show, &by_imdb, &options)
        .await
        .unwrap()
        .expect("Breaking Bad by IMDb ID");
    let item = provider
        .details(MediaKind::Show, &hit.ids, &options)
        .await
        .unwrap();
    assert_eq!(item.core.title, "Breaking Bad");
    assert!(!item.core.credits.is_empty());
    let ItemDetail::Show(show) = &item.detail else {
        panic!("show");
    };
    let aired = provider
        .episodes(&hit.ids, &EpisodeGroup::aired(), &options)
        .await
        .unwrap();
    assert!(aired.len() >= 62, "{} episodes", aired.len());
    // Alternate lists vary by show; read the first one the show offers.
    if let Some(group) = show
        .episode_groups
        .iter()
        .find(|g| **g != EpisodeGroup::aired())
    {
        provider.episodes(&hit.ids, group, &options).await.unwrap();
    }
}
