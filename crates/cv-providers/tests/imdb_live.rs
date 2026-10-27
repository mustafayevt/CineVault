//! Opt-in smoke test against the real IMDb website.
//!
//! It runs only when `CV_IMDB_LIVE=1`; without it the test prints why it was skipped and passes.
//! IMDb may answer with a human check or change its pages at any time, so a failure here means
//! "look at the provider", not "the build is broken". It makes three requests: a title page, a
//! search and the Top 250 movie chart.
//!
//! ```text
//! CV_IMDB_LIVE=1 cargo test -p cv-providers --test imdb_live -- --nocapture
//! ```

use cv_domain::{ProviderIds, id_key};
use cv_providers::imdb::{ImdbProvider, Top250List, Top250Source};
use cv_providers::model::{MediaKind, RequestOptions, SearchQuery};
use cv_providers::{HttpClient, MetadataProvider};

#[tokio::test]
async fn live_smoke() {
    if std::env::var("CV_IMDB_LIVE").ok().as_deref() != Some("1") {
        eprintln!("skipped: set CV_IMDB_LIVE=1 to run the live IMDb smoke test");
        return;
    }
    let provider = ImdbProvider::new(HttpClient::builder().build().unwrap());
    let options = RequestOptions::default();

    let mut ids = ProviderIds::new();
    ids.set(id_key::IMDB, "tt0113277");
    let item = provider
        .details(MediaKind::Movie, &ids, &options)
        .await
        .expect("the Heat title page is readable");
    assert_eq!(item.core.title, "Heat");
    assert_eq!(item.core.year, 1995);

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
        .expect("the find page is readable");
    assert!(
        hits.iter()
            .any(|h| h.ids.get(id_key::IMDB) == Some("tt0113277"))
    );

    let chart = provider
        .top250(Top250List::Movies, &options)
        .await
        .expect("the Top 250 chart is readable");
    assert!(chart.len() >= 100);
    assert_eq!(chart[0].rank, 1);
}
