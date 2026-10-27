//! The best match for a title several films share: the real TMDB provider answers a
//! search for "Memento" with real-shaped JSON, and the whole path (the provider's mapping of
//! `vote_count` and `popularity`, scoring, the merged order sent to the interface and the entry
//! the workspace preselects, which is the first one) puts the well-known film first.

#[path = "../../cv-providers/tests/support/mod.rs"]
mod mock_http;
mod support;

use std::sync::Arc;

use cv_app::app_settings::SettingsService;
use cv_app::match_service::{MatchService, ScrapeAnswer};
use cv_app::notices::NoticeHub;
use cv_app::provider_credentials::ProviderCredentialsService;
use cv_domain::ItemKind;
use cv_providers::tmdb::TmdbProvider;
use cv_providers::{CredentialResolver, HttpClient, MemoryUserCredentials, ProviderRegistry};
use mock_http::{MockServer, Reply};
use support::Lab;

const CONFIGURATION: &str =
    include_str!("../../cv-providers/tests/fixtures/tmdb/configuration.json");

/// What TMDB's `search/movie` answers for "Memento" (fields trimmed to the ones that matter),
/// in TMDB's own order: the recent obscure releases first.
const SEARCH_MEMENTO: &str = r#"{
  "page": 1,
  "total_pages": 1,
  "total_results": 4,
  "results": [
    {"id": 1327819, "title": "Memento", "original_title": "Memento", "original_language": "en",
     "release_date": "2025-03-14", "overview": "A short about a keepsake.",
     "poster_path": "/b7Fq1zqk2A2dZ1zjzP2bC9c5X1a.jpg", "popularity": 3.2, "vote_average": 5.5,
     "vote_count": 21},
    {"id": 1002111, "title": "Memento", "original_title": "Memento", "original_language": "en",
     "release_date": "2022-09-02", "overview": "A drama.",
     "poster_path": "/m22.jpg", "popularity": 1.4, "vote_average": 6.0, "vote_count": 9},
    {"id": 77, "title": "Memento", "original_title": "Memento", "original_language": "en",
     "release_date": "2000-10-11",
     "overview": "Leonard Shelby is unable to form new memories.",
     "poster_path": "/yuNs09hvpHVU1cBTCAk9zxsL2oW.jpg", "popularity": 41.7, "vote_average": 8.2,
     "vote_count": 14873},
    {"id": 214330, "title": "Memento", "original_title": "Memento", "original_language": "fr",
     "release_date": "1967-05-01", "overview": "An old short.",
     "poster_path": null, "popularity": 0.6, "vote_average": 5.0, "vote_count": 3}
  ]
}"#;

struct Rig {
    lab: Lab,
    service: Arc<MatchService>,
    _server: MockServer,
}

async fn rig() -> Rig {
    let server = MockServer::start().await;
    server.route("/3/configuration", Reply::ok(CONFIGURATION));
    server.route("/3/search/movie", Reply::ok(SEARCH_MEMENTO));
    let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
    let provider = TmdbProvider::with_base_url(
        HttpClient::builder().build().unwrap(),
        resolver.clone(),
        &server.url("/3"),
    );
    let mut registry = ProviderRegistry::new();
    registry.register_metadata(Arc::new(provider));
    let credentials = Arc::new(ProviderCredentialsService::with_resolver(
        resolver, registry,
    ));
    credentials.set("tmdb", "api_key", "a-key").unwrap();
    let lab = Lab::new();
    let service = MatchService::new(
        Arc::clone(&lab.db),
        lab.manager.clone(),
        credentials,
        Arc::new(SettingsService::in_memory("")),
        Arc::new(NoticeHub::new()),
    );
    Rig {
        lab,
        service,
        _server: server,
    }
}

async fn years(rig: &Rig, local_year: u16) -> Vec<(u16, f64)> {
    let item = rig.lab.movie("Memento", local_year);
    let stream = rig
        .service
        .scrape_candidate_stream(item, "Memento", ItemKind::Movie, "")
        .unwrap();
    let ScrapeAnswer::Hits(hits) = stream.sources[0].fetch().await else {
        panic!("tmdb did not answer");
    };
    // The order sent to the interface, which preselects the first entry.
    hits.iter()
        .map(|h| (h.candidate.year.unwrap_or(0), h.candidate.confidence))
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn without_a_year_the_famous_film_is_first() {
    let rig = rig().await;
    let order = years(&rig, 0).await;
    let list: Vec<u16> = order.iter().map(|(y, _)| *y).collect();
    assert_eq!(list[0], 2000, "{list:?}");
    assert_eq!(list.len(), 4);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_wrong_local_year_does_not_make_the_newest_obscure_film_the_best_match() {
    let rig = rig().await;
    // The folder says 2020: the 2025 short is the nearest year, the 2000 film the one meant.
    let order = years(&rig, 2020).await;
    assert_eq!(order[0].0, 2000, "{order:?}");
    // Every year conflicts with 2020: none shows more than 74%, so none is accepted unseen.
    assert!(order.iter().all(|(_, c)| *c <= 0.74 + 1e-9), "{order:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_matching_year_beats_popularity() {
    let rig = rig().await;
    let order = years(&rig, 2022).await;
    assert_eq!(order[0].0, 2022, "{order:?}");
    assert!(order[0].1 > 0.9);
}
