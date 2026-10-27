//! The IMDb page of a cast member: TMDB's credits carry no IMDb ID, so the Cast & crew
//! panel asks for it when it opens. The ID is read from TMDB's `external_ids` of the person,
//! stored with the person and asked only once.

#[path = "../../cv-providers/tests/support/mod.rs"]
mod mock_http;
mod support;

use std::sync::Arc;

use cv_app::app_settings::SettingsService;
use cv_app::match_service::MatchService;
use cv_app::notices::NoticeHub;
use cv_app::provider_credentials::ProviderCredentialsService;
use cv_domain::{Credit, CreditKind, Item, Person};
use cv_providers::tmdb::TmdbProvider;
use cv_providers::{CredentialResolver, HttpClient, MemoryUserCredentials, ProviderRegistry};
use mock_http::{MockServer, Reply};
use support::Lab;

const CONFIGURATION: &str =
    include_str!("../../cv-providers/tests/fixtures/tmdb/configuration.json");

async fn rig(with_key: bool) -> (Lab, Arc<MatchService>, MockServer) {
    let server = MockServer::start().await;
    server.route("/3/configuration", Reply::ok(CONFIGURATION));
    server.route(
        "/3/person/10205/external_ids",
        Reply::ok(r#"{"id":10205,"imdb_id":"nm0000244"}"#),
    );
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
    if with_key {
        credentials.set("tmdb", "api_key", "a-key").unwrap();
    }
    let lab = Lab::new();
    let service = MatchService::new(
        Arc::clone(&lab.db),
        lab.manager.clone(),
        credentials,
        Arc::new(SettingsService::in_memory("")),
        Arc::new(NoticeHub::new()),
    );
    (lab, service, server)
}

fn alien() -> Item {
    let mut person = Person::new("Sigourney Weaver");
    person.ids.set("tmdb", "10205");
    let mut item = Item::movie("Alien");
    item.core.credits.push(Credit {
        kind: CreditKind::Actor,
        role: "Ripley".to_owned(),
        person,
    });
    item
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_imdb_page_of_a_cast_member_is_read_once_and_stored() {
    let (lab, service, server) = rig(true).await;
    let id = lab.add(alien());

    let found = service.person_imdb_id("10205").await.unwrap();
    assert_eq!(found.as_deref(), Some("nm0000244"));
    let stored = lab.item(id);
    assert_eq!(
        stored.core.credits[0].person.ids.get("imdb"),
        Some("nm0000244")
    );
    assert_eq!(server.count("/3/person/10205/external_ids"), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn without_a_key_or_an_id_there_is_nothing_to_open_and_nothing_fails() {
    let (lab, service, server) = rig(false).await;
    lab.add(alien());
    assert_eq!(service.person_imdb_id("10205").await.unwrap(), None);
    assert_eq!(service.person_imdb_id("  ").await.unwrap(), None);
    assert_eq!(server.count("/3/person/10205/external_ids"), 0);
}
