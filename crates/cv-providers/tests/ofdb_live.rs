//! Opt-in smoke test against the real OFDb website.
//!
//! It runs only when `CV_OFDB_LIVE=1`; without it the test prints why it was skipped and passes.
//! OFDb is read from its pages and may change them at any time, so a failure here means "look at
//! the provider", not "the build is broken". It makes four requests: a search, a film page, its
//! plot page and an IMDb ID search.
//!
//! ```text
//! CV_OFDB_LIVE=1 cargo test -p cv-providers --test ofdb_live -- --nocapture
//! ```

use cv_domain::{ProviderIds, id_key};
use cv_providers::model::{MediaKind, RequestOptions, SearchQuery};
use cv_providers::ofdb::OfdbProvider;
use cv_providers::{HttpClient, MetadataProvider};

#[tokio::test]
async fn live_smoke() {
    if std::env::var("CV_OFDB_LIVE").ok().as_deref() != Some("1") {
        eprintln!("skipped: set CV_OFDB_LIVE=1 to run the live OFDb smoke test");
        return;
    }
    let provider = OfdbProvider::new(HttpClient::builder().build().unwrap());
    let options = RequestOptions::default();

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
        .expect("the search page is readable");
    let heat = hits
        .iter()
        .find(|h| h.title == "Heat" && h.year == 1995)
        .expect("Heat (1995) is in the results");

    let item = provider
        .details(MediaKind::Movie, &heat.ids, &options)
        .await
        .expect("the film page is readable");
    assert_eq!(item.core.title, "Heat");
    assert_eq!(item.core.year, 1995);
    assert_eq!(item.core.ids.get(id_key::IMDB), Some("tt0113277"));
    assert!(!item.core.plot.is_empty());

    let mut by_imdb = ProviderIds::new();
    by_imdb.set(id_key::IMDB, "tt0113277");
    let found = provider
        .lookup(MediaKind::Movie, &by_imdb, &options)
        .await
        .expect("the IMDb ID search is readable")
        .expect("OFDb knows Heat by its IMDb ID");
    assert_eq!(found.ids.get("ofdb"), heat.ids.get("ofdb"));
}
