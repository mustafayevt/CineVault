//! Opt-in smoke test against the real fernsehserien.de website.
//!
//! It runs only when `CV_FERNSEHSERIEN_LIVE=1`; without it the test prints why it was skipped and
//! passes. The site answers programs with a bot-check challenge (HTTP 202) and its pages are not
//! documented, so the provider's readers were built from schema.org structured data without
//! being recorded from the live site. Run this to confirm them. A "human check" failure means
//! the site refused the request; a "page format changed" failure means the readers need
//! adjusting. It makes three requests: a search, a show page and its episode guide.
//!
//! ```text
//! CV_FERNSEHSERIEN_LIVE=1 cargo test -p cv-providers --test fernsehserien_live -- --nocapture
//! ```

use cv_domain::EpisodeGroup;
use cv_providers::fernsehserien::FernsehserienProvider;
use cv_providers::model::{MediaKind, RequestOptions, SearchQuery};
use cv_providers::{HttpClient, MetadataProvider};

#[tokio::test]
async fn live_smoke() {
    if std::env::var("CV_FERNSEHSERIEN_LIVE").ok().as_deref() != Some("1") {
        eprintln!(
            "skipped: set CV_FERNSEHSERIEN_LIVE=1 to run the live fernsehserien.de smoke test"
        );
        return;
    }
    let provider = FernsehserienProvider::new(HttpClient::builder().build().unwrap());
    let options = RequestOptions::default();

    let hits = provider
        .search(
            &SearchQuery {
                kind: MediaKind::Show,
                title: "Tatort".to_owned(),
                year: None,
            },
            &options,
        )
        .await
        .expect("the search page is readable");
    let tatort = hits
        .iter()
        .find(|h| h.title.eq_ignore_ascii_case("Tatort"))
        .expect("Tatort is in the results");

    let item = provider
        .details(MediaKind::Show, &tatort.ids, &options)
        .await
        .expect("the show page is readable");
    assert_eq!(item.core.title, "Tatort");

    let episodes = provider
        .episodes(&tatort.ids, &EpisodeGroup::aired(), &options)
        .await
        .expect("the episode guide is readable");
    assert!(!episodes.is_empty());
}
