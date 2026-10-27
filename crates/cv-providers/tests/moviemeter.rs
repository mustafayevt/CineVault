//! The MovieMeter provider against a local mock server that replays hand-built responses
//! (`tests/fixtures/moviemeter/`). The fixtures follow the film fields the provider reads; the
//! opt-in live smoke test (`tests/moviemeter_live.rs`) confirms them against the real service.
//! Nothing here touches the network.

mod support;

use std::path::Path;
use std::sync::Arc;

use cv_domain::{CreditKind, Genre, ItemDetail, MediaFileType, ProviderIds};
use cv_providers::model::{MediaKind, RequestOptions, SearchQuery};
use cv_providers::moviemeter::{MovieMeterProvider, RATING_ID};
use cv_providers::{
    CredentialResolver, HttpClient, MemoryUserCredentials, MetadataProvider, Provider,
    ProviderError, ProviderRegistry, ResolvedCredentials, catalogue,
};
use support::{MockServer, Recorded, Reply};

const KEY: &str = "moviemeter-test-key-0123456789";

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/moviemeter")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("fixture {}: {e}", path.display()))
}

fn ok(name: &str) -> Reply {
    Reply::ok(&fixture(name))
}

fn status(code: u16, name: &str) -> Reply {
    let mut reply = ok(name);
    reply.status = code;
    reply
}

#[derive(Clone, Copy)]
struct Setup {
    cache: bool,
    key: Option<&'static str>,
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            cache: false,
            key: Some(KEY),
        }
    }
}

struct Harness {
    server: MockServer,
    provider: MovieMeterProvider,
    resolver: CredentialResolver,
    cache_dir: Option<tempfile::TempDir>,
}

impl Harness {
    async fn new() -> Self {
        Self::build(Setup::default()).await
    }

    async fn build(setup: Setup) -> Self {
        let server = MockServer::start().await;
        server.route_query("/api/film/", "q=Heat", ok("search_heat.json"));
        server.route_query("/api/film/", "q=Nothing", ok("search_empty.json"));
        server.route("/api/film/1234", ok("film_heat.json"));
        server.route("/api/film/tt0113277", ok("film_heat.json"));
        server.route("/api/film/55", ok("film_bare.json"));
        let cache_dir = setup.cache.then(|| tempfile::tempdir().unwrap());
        let mut builder = HttpClient::builder();
        if let Some(dir) = &cache_dir {
            builder = builder.cache_dir(dir.path());
        }
        let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
        if let Some(key) = setup.key {
            resolver
                .set_user(catalogue::key::MOVIEMETER, catalogue::API_KEY, key)
                .unwrap();
        }
        let provider = MovieMeterProvider::with_base_url(
            builder.build().unwrap(),
            resolver.clone(),
            &server.url("/api"),
        );
        Self {
            server,
            provider,
            resolver,
            cache_dir,
        }
    }
}

fn opts() -> RequestOptions {
    RequestOptions::default()
}

fn param(request: &Recorded, name: &str) -> Option<String> {
    let url = reqwest::Url::parse(&format!("http://mock{}", request.target)).unwrap();
    url.query_pairs()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.into_owned())
}

fn ids(pairs: &[(&str, &str)]) -> ProviderIds {
    pairs.iter().copied().collect()
}

fn query(kind: MediaKind, title: &str) -> SearchQuery {
    SearchQuery {
        kind,
        title: title.to_owned(),
        year: None,
    }
}

// --- credentials -----------------------------------------------------------------------------

#[tokio::test]
async fn the_provider_describes_itself() {
    let h = Harness::new().await;
    assert_eq!(h.provider.id(), "moviemeter");
    assert_eq!(h.provider.name(), "MovieMeter");
    let schema = h.provider.credentials().expect("schema");
    assert!(schema.field("api_key").is_some_and(|f| f.required));

    let capabilities = h.provider.capabilities();
    assert!(capabilities.movies && !capabilities.shows && !capabilities.best_effort);
    assert!(capabilities.orderings.is_empty());
    assert_eq!(capabilities.lookup_ids, vec!["moviemeter", "imdb"]);
}

#[tokio::test]
async fn without_a_key_nothing_is_sent() {
    let h = Harness::build(Setup {
        key: None,
        ..Setup::default()
    })
    .await;
    let err = h
        .provider
        .search(&query(MediaKind::Movie, "Heat"), &opts())
        .await
        .unwrap_err();
    match err {
        ProviderError::MissingCredentials {
            provider,
            fields,
            registration_url,
        } => {
            assert_eq!(provider, "MovieMeter");
            assert_eq!(fields, vec!["api_key".to_owned()]);
            assert!(registration_url.contains("moviemeter.nl"));
        }
        other => panic!("{other:?}"),
    }
    assert!(h.server.requests().is_empty());
}

#[tokio::test]
async fn the_key_goes_as_the_api_key_parameter_and_follows_changes() {
    let h = Harness::new().await;
    h.provider
        .search(&query(MediaKind::Movie, "Heat"), &opts())
        .await
        .unwrap();
    assert_eq!(
        param(&h.server.requests()[0], "api_key").as_deref(),
        Some(KEY)
    );
    h.resolver
        .set_user(
            catalogue::key::MOVIEMETER,
            catalogue::API_KEY,
            "second-key-55",
        )
        .unwrap();
    let forced = RequestOptions {
        bypass_cache: true,
        ..opts()
    };
    h.provider
        .search(&query(MediaKind::Movie, "Heat"), &forced)
        .await
        .unwrap();
    assert_eq!(
        param(&h.server.requests()[1], "api_key").as_deref(),
        Some("second-key-55")
    );
}

#[tokio::test]
async fn a_rejected_key_is_an_auth_error_and_a_built_in_one_suggests_the_users_own() {
    let h = Harness::new().await;
    h.server
        .route_query("/api/film/", "q=Bad", status(403, "error_forbidden.json"));
    let err = h
        .provider
        .search(&query(MediaKind::Movie, "Bad"), &opts())
        .await
        .unwrap_err();
    assert!(matches!(err.root(), ProviderError::Auth { .. }), "{err:?}");
    assert_eq!(err.key_problem_provider(), Some("moviemeter"));
    assert!(err.to_string().contains("rejected your key"), "{err}");
    assert!(!err.to_string().contains(KEY), "{err}");
}

#[tokio::test]
async fn the_settings_test_uses_the_typed_key_and_skips_the_cache() {
    let h = Harness::build(Setup {
        key: None,
        cache: true,
    })
    .await;
    h.server
        .route_query("/api/film/", "q=heat", ok("search_heat.json"));
    let schema = h.provider.credentials().expect("schema");
    let typed =
        ResolvedCredentials::from_user_values(schema, [("api_key", " typed-key-77 ")]).unwrap();
    h.provider.check_credentials(&typed).await.unwrap();
    h.provider.check_credentials(&typed).await.unwrap();
    let requests = h.server.requests();
    assert_eq!(requests.len(), 2, "never served from the cache");
    assert_eq!(
        param(&requests[0], "api_key").as_deref(),
        Some("typed-key-77")
    );

    let h = Harness::build(Setup {
        key: None,
        ..Setup::default()
    })
    .await;
    h.server
        .route_query("/api/film/", "q=heat", status(403, "error_forbidden.json"));
    let err = h.provider.check_credentials(&typed).await.unwrap_err();
    assert!(matches!(err.root(), ProviderError::Auth { .. }), "{err:?}");
}

#[tokio::test]
async fn the_provider_is_reachable_through_the_registry() {
    let h = Harness::new().await;
    h.server
        .route_query("/api/film/", "q=heat", ok("search_heat.json"));
    let mut registry = ProviderRegistry::new();
    registry.register(Arc::new(h.provider.clone()));
    assert!(registry.get("moviemeter").is_some());
    let credentials = h.resolver.resolve("moviemeter").unwrap();
    registry
        .check_credentials("moviemeter", "MovieMeter", &credentials)
        .await
        .unwrap();
}

#[tokio::test]
async fn server_errors_and_unreadable_answers_are_request_errors() {
    let h = Harness::new().await;
    h.server.route("/api/film/500", Reply::status(503));
    let err = h
        .provider
        .details(MediaKind::Movie, &ids(&[("moviemeter", "500")]), &opts())
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::Request { .. }), "{err:?}");
    assert!(err.to_string().contains("503"), "{err}");

    h.server
        .route("/api/film/501", Reply::ok("<html>oops</html>"));
    let err = h
        .provider
        .details(MediaKind::Movie, &ids(&[("moviemeter", "501")]), &opts())
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::Request { .. }), "{err:?}");
}

// --- search and lookup -----------------------------------------------------------------------

#[tokio::test]
async fn a_text_search_reads_hits() {
    let h = Harness::new().await;
    let hits = h
        .provider
        .search(&query(MediaKind::Movie, "Heat"), &opts())
        .await
        .unwrap();
    // The entry without an ID is left out.
    assert_eq!(hits.len(), 2);
    let heat = &hits[0];
    assert_eq!(heat.provider, "moviemeter");
    assert_eq!(heat.title, "Heat");
    assert_eq!(heat.original_title, "Heat (1995)");
    assert_eq!(heat.year, 1995);
    assert_eq!(heat.ids.get("moviemeter"), Some("1234"));
    assert_eq!(heat.ids.get("imdb"), Some("tt0113277"));
    assert_eq!(
        heat.poster_url.as_deref(),
        Some("https://img.example.test/mm/heat_s.jpg")
    );
    assert!(!heat.exact_id);
    assert_eq!(hits[1].ids.get("imdb"), None);

    let request = &h.server.requests()[0];
    assert_eq!(request.path(), "/api/film/");
    assert_eq!(param(request, "q").as_deref(), Some("Heat"));
}

#[tokio::test]
async fn shows_blank_titles_and_empty_answers_find_nothing() {
    let h = Harness::new().await;
    for q in [
        query(MediaKind::Show, "Heat"),
        query(MediaKind::Movie, "  "),
    ] {
        assert!(h.provider.search(&q, &opts()).await.unwrap().is_empty());
    }
    assert!(h.server.requests().is_empty());
    assert!(
        h.provider
            .search(&query(MediaKind::Movie, "Nothing"), &opts())
            .await
            .unwrap()
            .is_empty()
    );
    // A show request for details or lookup is not served.
    assert!(
        h.provider
            .lookup(MediaKind::Show, &ids(&[("moviemeter", "1234")]), &opts())
            .await
            .unwrap()
            .is_none()
    );
    let err = h
        .provider
        .details(MediaKind::Show, &ids(&[("moviemeter", "1234")]), &opts())
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::Unsupported { .. }), "{err:?}");
}

#[tokio::test]
async fn the_moviemeter_id_is_preferred_and_the_imdb_id_works_too() {
    let h = Harness::new().await;
    let hit = h
        .provider
        .lookup(
            MediaKind::Movie,
            &ids(&[("moviemeter", "1234"), ("imdb", "tt0113277")]),
            &opts(),
        )
        .await
        .unwrap()
        .expect("Heat");
    assert!(hit.exact_id);
    assert_eq!(hit.ids.get("imdb"), Some("tt0113277"));
    assert_eq!(h.server.requests()[0].path(), "/api/film/1234");

    let hit = h
        .provider
        .lookup(MediaKind::Movie, &ids(&[("imdb", " TT0113277 ")]), &opts())
        .await
        .unwrap()
        .expect("Heat by IMDb ID");
    assert_eq!(hit.ids.get("moviemeter"), Some("1234"));
    assert_eq!(h.server.requests()[1].path(), "/api/film/tt0113277");
}

#[tokio::test]
async fn ids_moviemeter_does_not_know_are_none_or_not_found() {
    let h = Harness::new().await;
    assert!(
        h.provider
            .lookup(MediaKind::Movie, &ids(&[("tmdb", "949")]), &opts())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        h.provider
            .lookup(MediaKind::Movie, &ids(&[("moviemeter", "999")]), &opts())
            .await
            .unwrap()
            .is_none()
    );
    let err = h
        .provider
        .details(MediaKind::Movie, &ids(&[("moviemeter", "999")]), &opts())
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::NotFound(_)), "{err:?}");
    // The first lookup had nothing to ask.
    assert_eq!(h.server.requests().len(), 2);
}

// --- details ---------------------------------------------------------------------------------

#[tokio::test]
async fn movie_details_cover_text_people_rating_and_poster() {
    let h = Harness::new().await;
    let item = h
        .provider
        .details(MediaKind::Movie, &ids(&[("moviemeter", "1234")]), &opts())
        .await
        .unwrap();
    assert_eq!(item.core.title, "Heat");
    assert_eq!(item.core.original_title, "Heat (1995)");
    assert_eq!(item.core.year, 1995);
    assert_eq!(item.core.ids.get("moviemeter"), Some("1234"));
    assert_eq!(item.core.ids.get("imdb"), Some("tt0113277"));
    assert!(item.core.plot.starts_with("Een groep professionele dieven"));
    assert_eq!(
        item.core
            .artwork_urls
            .get(&MediaFileType::Poster)
            .map(String::as_str),
        Some("https://img.example.test/mm/heat_l.jpg")
    );

    let ItemDetail::Movie(movie) = &item.detail else {
        panic!("movie");
    };
    assert_eq!(movie.runtime_minutes, 170);
    // Dutch genre names become the standard ones.
    assert_eq!(
        movie.genres,
        vec![
            Genre::parse("Action").unwrap(),
            Genre::parse("Crime").unwrap(),
            Genre::parse("Drama").unwrap()
        ]
    );
    assert_eq!(movie.countries, vec!["US"]);

    let credits: Vec<_> = item
        .core
        .credits
        .iter()
        .map(|c| (c.kind, c.person.name.as_str()))
        .collect();
    assert_eq!(
        credits,
        vec![
            (CreditKind::Director, "Michael Mann"),
            (CreditKind::Actor, "Al Pacino"),
            (CreditKind::Actor, "Robert De Niro"),
        ]
    );

    let rating = item.core.ratings.get(RATING_ID).unwrap();
    assert_eq!(
        (rating.value, rating.votes, rating.max_value),
        (4.2, 12_345, 5)
    );
}

#[tokio::test]
async fn a_film_with_nothing_but_a_title_has_empty_fields() {
    let h = Harness::new().await;
    let item = h
        .provider
        .details(MediaKind::Movie, &ids(&[("moviemeter", "55")]), &opts())
        .await
        .unwrap();
    assert_eq!(item.core.title, "Kort Filmpje");
    assert_eq!(item.core.ids.get("moviemeter"), Some("55"));
    assert_eq!(item.core.ids.get("imdb"), None);
    assert_eq!(item.core.year, 0);
    assert!(item.core.plot.is_empty());
    assert!(item.core.ratings.is_empty());
    assert!(item.core.credits.is_empty());
    assert!(item.core.artwork_urls.is_empty());
    let ItemDetail::Movie(movie) = &item.detail else {
        panic!("movie");
    };
    assert!(movie.genres.is_empty() && movie.countries.is_empty());
}

#[tokio::test]
async fn repeated_details_come_from_the_cache_and_no_key_reaches_the_cache_files() {
    let h = Harness::build(Setup {
        cache: true,
        ..Setup::default()
    })
    .await;
    let wanted = ids(&[("moviemeter", "1234")]);
    for _ in 0..2 {
        h.provider
            .details(MediaKind::Movie, &wanted, &opts())
            .await
            .unwrap();
    }
    assert_eq!(h.server.count("/api/film/1234"), 1);
    let forced = RequestOptions {
        bypass_cache: true,
        ..opts()
    };
    h.provider
        .details(MediaKind::Movie, &wanted, &forced)
        .await
        .unwrap();
    assert_eq!(h.server.count("/api/film/1234"), 2);

    fn walk(dir: &Path, found: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, found);
            } else {
                found.push(path);
            }
        }
    }
    let mut files = Vec::new();
    walk(h.cache_dir.as_ref().unwrap().path(), &mut files);
    assert!(!files.is_empty(), "the answer was cached");
    for file in files {
        let text = String::from_utf8_lossy(&std::fs::read(&file).unwrap()).into_owned();
        assert!(!text.contains(KEY), "{} holds the key", file.display());
        assert!(!file.to_string_lossy().contains(KEY));
    }
}
