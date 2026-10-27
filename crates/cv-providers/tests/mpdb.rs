//! The MPDb.tv provider against a local mock server that replays hand-built responses
//! (`tests/fixtures/mpdb/`). MPDb.tv is a private service with no published schema, so the movie
//! fixtures follow the fields the provider reads; only the 401 body was seen on the real service.
//! The opt-in live smoke test (`tests/mpdb_live.rs`) confirms the names with a subscription.
//! Nothing here touches the network.

mod support;

use std::path::Path;
use std::sync::Arc;

use cv_domain::{CreditKind, Genre, ItemDetail, MediaFileType, ProviderIds, id_key};
use cv_providers::model::{MediaKind, RequestOptions, SearchQuery};
use cv_providers::mpdb::{MpdbProvider, RATING_ID};
use cv_providers::{
    ArtworkProvider, CredentialResolver, HttpClient, MemoryUserCredentials, MetadataProvider,
    Provider, ProviderError, ProviderRegistry, ResolvedCredentials, catalogue,
};
use support::{MockServer, Reply};

const API_KEY: &str = "mpdb-test-key-0123456789";
const USERNAME: &str = "jean.dupont";
const SUBSCRIPTION: &str = "abo-secret-9876";
/// `sha1(USERNAME + API_KEY + SUBSCRIPTION)` and `base64url(USERNAME)`, worked out independently
/// of the provider.
const SIGNATURE: &str = "90c08e0969e7b9040d7fabb3d23ebae4777aec5a";
const USER_SEGMENT: &str = "amVhbi5kdXBvbnQ=";

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/mpdb")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("fixture {}: {e}", path.display()))
}

fn ok(name: &str) -> Reply {
    Reply::ok(&fixture(name))
}

fn account_path(kind: &str, tail: &str) -> String {
    format!("/api/v1/{kind}/{API_KEY}/{USER_SEGMENT}/{SIGNATURE}/{tail}")
}

struct Harness {
    server: MockServer,
    provider: MpdbProvider,
    resolver: CredentialResolver,
}

impl Harness {
    async fn new() -> Self {
        Self::build(true).await
    }

    async fn build(with_credentials: bool) -> Self {
        let server = MockServer::start().await;
        server.route(
            &account_path("search", "movies/La%20Nuit"),
            ok("search_nuit.json"),
        );
        server.route(
            &account_path("search", "movies/rien"),
            ok("search_empty.json"),
        );
        server.route(&account_path("movies", "4711"), ok("movie_4711.json"));
        server.route(&account_path("movies", "4712"), ok("movie_4712.json"));
        let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
        if with_credentials {
            for (field, value) in [
                (catalogue::API_KEY, API_KEY),
                (catalogue::MPDB_USERNAME, USERNAME),
                (catalogue::MPDB_SUBSCRIPTION_KEY, SUBSCRIPTION),
            ] {
                resolver
                    .set_user(catalogue::key::MPDB, field, value)
                    .unwrap();
            }
        }
        let provider = MpdbProvider::with_base_url(
            HttpClient::builder().build().unwrap(),
            resolver.clone(),
            &server.url("/api/v1"),
        );
        Self {
            server,
            provider,
            resolver,
        }
    }
}

fn mpdb_ids(id: &str) -> ProviderIds {
    let mut ids = ProviderIds::new();
    ids.set("mpdb", id);
    ids
}

fn query(kind: MediaKind, title: &str) -> SearchQuery {
    SearchQuery {
        kind,
        title: title.to_owned(),
        year: None,
    }
}

fn opts() -> RequestOptions {
    RequestOptions::default()
}

// --- identity and credentials -----------------------------------------------------------------

#[tokio::test]
async fn the_provider_is_a_movie_provider_that_needs_the_users_account() {
    let h = Harness::build(false).await;
    let capabilities = h.provider.capabilities();
    assert!(capabilities.movies && !capabilities.shows && !capabilities.best_effort);
    assert_eq!(capabilities.lookup_ids, vec!["mpdb"]);
    assert_eq!(h.provider.id(), "mpdb");
    assert_eq!(h.provider.name(), "MPDb.tv");
    let schema = h.provider.credentials().expect("a credential schema");
    assert_eq!(schema.fields.len(), 3);

    // Without the account the provider is inactive, and says which fields are missing.
    let status = h.resolver.status("mpdb").unwrap();
    assert!(!status.active);
    assert_eq!(status.missing, ["api_key", "username", "subscription_key"]);
    let error = h
        .provider
        .search(&query(MediaKind::Movie, "La Nuit"), &opts())
        .await
        .unwrap_err();
    let ProviderError::MissingCredentials {
        fields, provider, ..
    } = error
    else {
        panic!("should be missing credentials");
    };
    assert_eq!(provider, "MPDb.tv");
    assert_eq!(fields, ["api_key", "username", "subscription_key"]);
    assert!(h.server.requests().is_empty());
}

#[tokio::test]
async fn a_partly_filled_account_still_names_what_is_missing() {
    let h = Harness::build(false).await;
    h.resolver
        .set_user("mpdb", catalogue::API_KEY, API_KEY)
        .unwrap();
    let error = h
        .provider
        .details(MediaKind::Movie, &mpdb_ids("4711"), &opts())
        .await
        .unwrap_err();
    let ProviderError::MissingCredentials { fields, .. } = error else {
        panic!("should be missing credentials");
    };
    assert_eq!(fields, ["username", "subscription_key"]);
}

#[tokio::test]
async fn requests_carry_the_account_in_the_path() {
    let h = Harness::new().await;
    h.provider
        .search(&query(MediaKind::Movie, "La Nuit"), &opts())
        .await
        .unwrap();
    let requests = h.server.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "GET");
    // The signature is the SHA-1 of name, app key and subscription key; the name is base64url.
    assert_eq!(
        requests[0].path(),
        account_path("search", "movies/La%20Nuit")
    );
    // The subscription key itself is never sent.
    assert!(!requests[0].target.contains(SUBSCRIPTION));
}

// --- search and lookup ------------------------------------------------------------------------

#[tokio::test]
async fn search_returns_the_movies_of_the_list() {
    let h = Harness::new().await;
    let hits = h
        .provider
        .search(&query(MediaKind::Movie, "La Nuit"), &opts())
        .await
        .unwrap();
    // The entry without a title is dropped.
    assert_eq!(hits.len(), 2);
    let first = &hits[0];
    assert_eq!(first.provider, "mpdb");
    assert_eq!(first.ids.get("mpdb"), Some("4711"));
    assert_eq!(first.ids.get(id_key::IMDB), Some("tt0111222"));
    assert_eq!(first.ids.get(id_key::TMDB), Some("9001"));
    assert_eq!(first.title, "La Nuit des Exemples");
    assert_eq!(first.original_title, "The Night of Examples");
    assert_eq!(first.year, 1998);
    assert_eq!(first.overview, "Une nuit, un exemple.");
    assert_eq!(
        first.poster_url.as_deref(),
        Some("https://img.example.test/mpdb/p1.jpg")
    );
    assert!(!first.exact_id);
    // Text values: a title called `titre`, a year as text, an IMDb ID without its prefix.
    assert_eq!(hits[1].title, "La Nuit des Exemples 2");
    assert_eq!(hits[1].year, 2003);
    assert_eq!(hits[1].ids.get(id_key::IMDB), Some("tt0222333"));
}

#[tokio::test]
async fn a_search_without_results_is_empty_and_other_kinds_make_no_request() {
    let h = Harness::new().await;
    assert!(
        h.provider
            .search(&query(MediaKind::Movie, "rien"), &opts())
            .await
            .unwrap()
            .is_empty()
    );
    let before = h.server.requests().len();
    for (kind, title) in [(MediaKind::Show, "La Nuit"), (MediaKind::Movie, "  ")] {
        assert!(
            h.provider
                .search(&query(kind, title), &opts())
                .await
                .unwrap()
                .is_empty()
        );
    }
    assert_eq!(h.server.requests().len(), before);
}

#[tokio::test]
async fn lookup_returns_an_exact_hit_or_none() {
    let h = Harness::new().await;
    let hit = h
        .provider
        .lookup(MediaKind::Movie, &mpdb_ids("4711"), &opts())
        .await
        .unwrap()
        .expect("a hit");
    assert!(hit.exact_id);
    assert_eq!(hit.title, "La Nuit des Exemples");

    // An answer wrapped in `data` is read too.
    let wrapped = h
        .provider
        .lookup(MediaKind::Movie, &mpdb_ids("4712"), &opts())
        .await
        .unwrap()
        .expect("a hit");
    assert_eq!(wrapped.year, 2003);

    // An ID the mock does not know (404), a show, and no ID at all.
    assert_eq!(
        h.provider
            .lookup(MediaKind::Movie, &mpdb_ids("999"), &opts())
            .await
            .unwrap(),
        None
    );
    let before = h.server.requests().len();
    assert_eq!(
        h.provider
            .lookup(MediaKind::Show, &mpdb_ids("4711"), &opts())
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        h.provider
            .lookup(MediaKind::Movie, &ProviderIds::new(), &opts())
            .await
            .unwrap(),
        None
    );
    assert_eq!(h.server.requests().len(), before);
}

// --- details ----------------------------------------------------------------------------------

#[tokio::test]
async fn movie_details_carry_the_french_data_and_artwork() {
    let h = Harness::new().await;
    let item = h
        .provider
        .details(MediaKind::Movie, &mpdb_ids("4711"), &opts())
        .await
        .unwrap();

    let core = &item.core;
    assert_eq!(core.title, "La Nuit des Exemples");
    assert_eq!(core.original_title, "The Night of Examples");
    assert_eq!(core.year, 1998);
    assert_eq!(core.plot, "Une nuit, un exemple devient une règle.");
    assert_eq!(core.ids.get("mpdb"), Some("4711"));
    assert_eq!(core.ids.get(id_key::IMDB), Some("tt0111222"));
    assert_eq!(core.ids.get(id_key::TMDB), Some("9001"));

    let rating = core.ratings.get(RATING_ID).expect("a rating");
    assert!((rating.value - 7.8).abs() < 1e-6);
    assert_eq!((rating.votes, rating.max_value), (2210, 10));

    // The director first; each actor once even when listed twice.
    let credits: Vec<_> = core
        .credits
        .iter()
        .map(|c| (c.kind, c.person.name.as_str(), c.role.as_str()))
        .collect();
    assert_eq!(
        credits,
        [
            (CreditKind::Director, "Claire Exemple", ""),
            (CreditKind::Actor, "Jean Modèle", "Le commissaire"),
            (CreditKind::Actor, "Marie Échantillon", ""),
        ]
    );

    for (kind, url) in [
        (MediaFileType::Poster, "poster-fr.jpg"),
        (MediaFileType::Fanart, "fanart1.jpg"),
        (MediaFileType::Disc, "disc.png"),
        (MediaFileType::Clearart, "clearart.png"),
        (MediaFileType::Clearlogo, "logo.png"),
        (MediaFileType::Banner, "banner.jpg"),
    ] {
        let found = core.artwork_urls.get(&kind).expect("artwork");
        assert!(found.ends_with(url), "{kind:?}: {found}");
    }

    let ItemDetail::Movie(movie) = &item.detail else {
        panic!("a movie");
    };
    assert_eq!(movie.tagline, "Tout commence la nuit.");
    assert_eq!(movie.runtime_minutes, 112);
    assert_eq!(
        movie.genres,
        [
            Genre::parse("Crime").unwrap(),
            Genre::parse("Drama").unwrap(),
            Genre::parse("Thriller").unwrap(),
        ]
    );
    assert_eq!(movie.countries, ["FR", "US"]);
    assert_eq!(
        movie.release_date,
        cv_domain::CalendarDate::new(1998, 6, 17)
    );
    assert_eq!(
        movie.extra_fanarts,
        [
            "https://img.example.test/mpdb/fanart2.jpg",
            "https://img.example.test/mpdb/fanart3.jpg"
        ]
    );
}

#[tokio::test]
async fn a_bare_movie_still_gives_an_item() {
    let h = Harness::new().await;
    let item = h
        .provider
        .details(MediaKind::Movie, &mpdb_ids("4712"), &opts())
        .await
        .unwrap();
    assert_eq!(item.core.title, "La Nuit des Exemples 2");
    assert_eq!(item.core.original_title, "La Nuit des Exemples 2");
    assert_eq!(item.core.year, 2003);
    assert!(item.core.ratings.is_empty());
    assert!(item.core.credits.is_empty());
    assert!(item.core.artwork_urls.is_empty());
}

#[tokio::test]
async fn shows_and_unknown_movies_are_not_details() {
    let h = Harness::new().await;
    assert!(matches!(
        h.provider
            .details(MediaKind::Show, &mpdb_ids("4711"), &opts())
            .await,
        Err(ProviderError::Unsupported { .. })
    ));
    assert_eq!(
        h.provider
            .details(MediaKind::Movie, &mpdb_ids("999"), &opts())
            .await,
        Err(ProviderError::NotFound("MPDb.tv".to_owned()))
    );
    assert_eq!(
        h.provider
            .details(MediaKind::Movie, &ProviderIds::new(), &opts())
            .await,
        Err(ProviderError::NotFound("MPDb.tv".to_owned()))
    );
}

// --- artwork ----------------------------------------------------------------------------------

#[tokio::test]
async fn artwork_lists_every_image_with_its_language_and_size() {
    let h = Harness::new().await;
    let candidates = h
        .provider
        .artwork(MediaKind::Movie, &mpdb_ids("4711"), &opts())
        .await
        .unwrap();
    let of = |kind: MediaFileType| {
        candidates
            .iter()
            .filter(|c| c.kind == kind)
            .collect::<Vec<_>>()
    };
    assert_eq!(of(MediaFileType::Poster).len(), 2);
    assert_eq!(of(MediaFileType::Fanart).len(), 3);
    assert_eq!(of(MediaFileType::Disc).len(), 1);
    assert_eq!(of(MediaFileType::Clearart).len(), 1);
    assert_eq!(of(MediaFileType::Clearlogo).len(), 1);
    assert_eq!(of(MediaFileType::Banner).len(), 1);

    let french = of(MediaFileType::Poster)[0];
    assert_eq!(french.language.as_deref(), Some("fr"));
    assert_eq!((french.width, french.height), (1000, 1500));
    assert!((french.score - 12.0).abs() < 1e-6);
    // `xx` marks an image without text.
    assert_eq!(of(MediaFileType::Disc)[0].language, None);

    assert!(
        h.provider
            .artwork(MediaKind::Show, &mpdb_ids("4711"), &opts())
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        h.provider
            .artwork(MediaKind::Movie, &mpdb_ids("999"), &opts())
            .await
            .unwrap()
            .is_empty()
    );
}

// --- errors -----------------------------------------------------------------------------------

#[tokio::test]
async fn rejected_credentials_are_an_authentication_error_without_secrets() {
    let h = Harness::new().await;
    h.server.route(&account_path("movies", "4711"), {
        let mut reply = ok("error_401.json");
        reply.status = 401;
        reply
    });
    let error = h
        .provider
        .details(MediaKind::Movie, &mpdb_ids("4711"), &opts())
        .await
        .unwrap_err();
    assert!(matches!(error, ProviderError::Auth { .. }), "{error:?}");
    let message = error.to_string();
    for secret in [API_KEY, SUBSCRIPTION, SIGNATURE, USER_SEGMENT, USERNAME] {
        assert!(!message.contains(secret), "{message}");
    }
}

#[tokio::test]
async fn a_server_error_is_a_failed_request_and_does_not_leak_the_path() {
    let h = Harness::new().await;
    h.server
        .route(&account_path("movies", "4711"), Reply::status(500));
    let error = h
        .provider
        .details(MediaKind::Movie, &mpdb_ids("4711"), &opts())
        .await
        .unwrap_err();
    let message = error.to_string();
    assert!(message.contains("HTTP 500"), "{message}");
    assert!(
        !message.contains(API_KEY) && !message.contains(SIGNATURE),
        "{message}"
    );
}

#[tokio::test]
async fn an_unexpected_body_is_a_failed_request() {
    let h = Harness::new().await;
    h.server.route(
        &account_path("movies", "4711"),
        Reply::ok("<html>maintenance</html>"),
    );
    let error = h
        .provider
        .details(MediaKind::Movie, &mpdb_ids("4711"), &opts())
        .await
        .unwrap_err();
    assert!(matches!(error, ProviderError::Request { .. }), "{error:?}");
}

#[tokio::test]
async fn test_credentials_checks_the_values_it_is_given_without_the_cache() {
    let h = Harness::build(false).await;
    h.server.route(
        &account_path("search", "movies/heat"),
        ok("search_empty.json"),
    );
    let schema = h.provider.credentials().unwrap();
    let good = ResolvedCredentials::from_user_values(
        schema,
        [
            ("api_key", API_KEY),
            ("username", USERNAME),
            ("subscription_key", SUBSCRIPTION),
        ],
    )
    .unwrap();
    h.provider.check_credentials(&good).await.unwrap();
    h.provider.check_credentials(&good).await.unwrap();
    assert_eq!(h.server.requests().len(), 2);

    let incomplete = ResolvedCredentials::from_user_values(schema, [("api_key", API_KEY)]).unwrap();
    assert!(matches!(
        h.provider.check_credentials(&incomplete).await,
        Err(ProviderError::MissingCredentials { .. })
    ));

    // The service says no: another key signs to another path, which answers 401.
    let other = ResolvedCredentials::from_user_values(
        schema,
        [
            ("api_key", "other-key-0123456789"),
            ("username", USERNAME),
            ("subscription_key", SUBSCRIPTION),
        ],
    )
    .unwrap();
    let mut rejected = ok("error_401.json");
    rejected.status = 401;
    h.server.route(
        &format!(
            "/api/v1/search/other-key-0123456789/{USER_SEGMENT}/aafdccec2759de27623afe8624a08ca0b7508153/movies/heat"
        ),
        rejected,
    );
    let error = h.provider.check_credentials(&other).await.unwrap_err();
    assert!(matches!(error, ProviderError::Auth { .. }), "{error:?}");
}

#[tokio::test]
async fn the_registry_tests_credentials_through_the_provider() {
    let h = Harness::build(false).await;
    h.server.route(
        &account_path("search", "movies/heat"),
        ok("search_empty.json"),
    );
    let mut registry = ProviderRegistry::new();
    registry.register(Arc::new(h.provider.clone()));
    let schema = h.provider.credentials().unwrap();
    let good = ResolvedCredentials::from_user_values(
        schema,
        [
            ("api_key", API_KEY),
            ("username", USERNAME),
            ("subscription_key", SUBSCRIPTION),
        ],
    )
    .unwrap();
    registry
        .check_credentials("mpdb", "MPDb.tv", &good)
        .await
        .unwrap();
}
