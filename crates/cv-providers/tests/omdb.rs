//! The OMDb provider against a local mock server that replays hand-built responses in OMDb's
//! documented shape (`tests/fixtures/omdb/`). Nothing here touches the network; the opt-in live
//! smoke test is `tests/omdb_live.rs`.

mod support;

use std::path::Path;
use std::sync::Arc;

use cv_domain::{
    AiredStatus, CalendarDate, CreditKind, EpisodeGroup, EpisodeNumber, EpisodeOrdering, Genre,
    ItemDetail, MediaFileType, ProviderIds,
};
use cv_providers::model::{MediaKind, RequestOptions, SearchQuery};
use cv_providers::omdb::{
    IMDB_RATING_ID, METACRITIC_RATING_ID, OmdbProvider, RT_AUDIENCE_AVERAGE_RATING_ID,
    RT_AUDIENCE_RATING_ID, RT_CRITICS_AVERAGE_RATING_ID, RT_CRITICS_RATING_ID,
};
use cv_providers::{
    CredentialResolver, HttpClient, MemoryUserCredentials, MetadataProvider, Provider,
    ProviderError, ProviderRegistry, RatingProvider, ResolvedCredentials, catalogue,
};
use support::{MockServer, Recorded, Reply};

const KEY: &str = "omdb-test-key-0123456789";

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/omdb")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("fixture {}: {e}", path.display()))
}

fn ok(name: &str) -> Reply {
    Reply::ok(&fixture(name))
}

/// OMDb answers errors with HTTP 401 and a JSON body.
fn denied(name: &str) -> Reply {
    let mut reply = ok(name);
    reply.status = 401;
    reply
}

#[derive(Clone, Copy)]
struct Setup {
    cache: bool,
    key: Option<&'static str>,
    /// The fixtures the two season requests are answered with.
    seasons: [&'static str; 2],
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            cache: false,
            key: Some(KEY),
            seasons: ["season_1.json", "season_2.json"],
        }
    }
}

struct Harness {
    server: MockServer,
    provider: OmdbProvider,
    resolver: CredentialResolver,
    cache_dir: Option<tempfile::TempDir>,
}

impl Harness {
    async fn new() -> Self {
        Self::build(Setup::default()).await
    }

    async fn build(setup: Setup) -> Self {
        let server = MockServer::start().await;
        // Specific routes first: the season routes also contain the series ID.
        server.route_query("/", "Season=1", ok(setup.seasons[0]));
        server.route_query("/", "Season=2", ok(setup.seasons[1]));
        server.route_query("/", "i=tt0113277", ok("title_movie.json"));
        server.route_query("/", "i=tt0903747", ok("title_series.json"));
        server.route_query("/", "i=tt0959621", ok("title_episode.json"));
        server.route_query("/", "i=tt1234567", ok("title_bare.json"));
        server.route_query("/", "i=tt7654321", ok("error_not_found.json"));
        server.route_query("/", "s=Heat", ok("search_movies.json"));
        server.route_query("/", "s=Breaking", ok("search_series.json"));
        server.route_query("/", "s=Zzzz", ok("error_not_found.json"));
        server.route_query("/", "s=a", ok("error_too_many.json"));
        let cache_dir = setup.cache.then(|| tempfile::tempdir().unwrap());
        let mut builder = HttpClient::builder();
        if let Some(dir) = &cache_dir {
            builder = builder.cache_dir(dir.path());
        }
        let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
        if let Some(key) = setup.key {
            resolver
                .set_user(catalogue::key::OMDB, catalogue::API_KEY, key)
                .unwrap();
        }
        let provider = OmdbProvider::with_base_url(
            builder.build().unwrap(),
            resolver.clone(),
            &server.url(""),
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

fn query(kind: MediaKind, title: &str, year: Option<u16>) -> SearchQuery {
    SearchQuery {
        kind,
        title: title.to_owned(),
        year,
    }
}

fn numbers(items: &[cv_domain::Item]) -> Vec<(u32, u32)> {
    items
        .iter()
        .map(|item| {
            let ItemDetail::Episode(fields) = &item.detail else {
                panic!("not an episode")
            };
            let n: &EpisodeNumber = fields.numbers.get(&EpisodeGroup::aired()).unwrap();
            (n.season, n.episode)
        })
        .collect()
}

// --- credentials -----------------------------------------------------------------------------

#[tokio::test]
async fn the_provider_describes_itself() {
    let h = Harness::new().await;
    assert_eq!(h.provider.id(), "omdb");
    assert_eq!(h.provider.name(), "OMDb");
    let schema = h.provider.credentials().expect("schema");
    assert!(schema.field("api_key").is_some_and(|f| f.required));

    let capabilities = h.provider.capabilities();
    assert!(capabilities.movies && capabilities.shows && !capabilities.best_effort);
    assert_eq!(capabilities.orderings, vec![EpisodeOrdering::Aired]);
    assert_eq!(capabilities.lookup_ids, vec!["imdb"]);
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
        .search(&query(MediaKind::Movie, "Heat", None), &opts())
        .await
        .unwrap_err();
    match err {
        ProviderError::MissingCredentials {
            provider,
            fields,
            registration_url,
        } => {
            assert_eq!(provider, "OMDb");
            assert_eq!(fields, vec!["api_key".to_owned()]);
            assert!(registration_url.contains("omdbapi.com"));
        }
        other => panic!("{other:?}"),
    }
    assert!(h.server.requests().is_empty());
}

#[tokio::test]
async fn the_key_goes_as_a_query_parameter_and_json_is_requested() {
    let h = Harness::new().await;
    h.provider
        .details(MediaKind::Movie, &ids(&[("imdb", "tt0113277")]), &opts())
        .await
        .unwrap();
    let request = &h.server.requests()[0];
    assert_eq!(param(request, "apikey").as_deref(), Some(KEY));
    assert_eq!(param(request, "r").as_deref(), Some("json"));
    assert_eq!(param(request, "i").as_deref(), Some("tt0113277"));
    assert_eq!(param(request, "plot").as_deref(), Some("full"));
    assert_eq!(param(request, "tomatoes").as_deref(), Some("true"));
}

#[tokio::test]
async fn a_changed_key_applies_to_the_next_call() {
    let h = Harness::new().await;
    let wanted = ids(&[("imdb", "tt0113277")]);
    h.provider
        .details(MediaKind::Movie, &wanted, &opts())
        .await
        .unwrap();
    h.resolver
        .set_user(catalogue::key::OMDB, catalogue::API_KEY, "second-key-55")
        .unwrap();
    let forced = RequestOptions {
        bypass_cache: true,
        ..opts()
    };
    h.provider
        .details(MediaKind::Movie, &wanted, &forced)
        .await
        .unwrap();
    let requests = h.server.requests();
    assert_eq!(
        param(&requests[1], "apikey").as_deref(),
        Some("second-key-55")
    );
}

#[tokio::test]
async fn a_rejected_key_is_an_auth_error_without_the_key_in_the_message() {
    let h = Harness::new().await;
    h.server
        .route_query("/", "s=Bad", denied("error_invalid_key.json"));
    let err = h
        .provider
        .search(&query(MediaKind::Movie, "Bad", None), &opts())
        .await
        .unwrap_err();
    assert!(matches!(err.root(), ProviderError::Auth { .. }), "{err:?}");
    assert!(err.to_string().contains("Invalid API key"), "{err}");
    assert!(!err.to_string().contains(KEY), "{err}");
}

#[tokio::test]
async fn a_rejected_key_says_so_and_links_to_the_card() {
    let h = Harness::new().await;
    h.server
        .route_query("/", "s=Bad", denied("error_invalid_key.json"));
    let err = h
        .provider
        .search(&query(MediaKind::Movie, "Bad", None), &opts())
        .await
        .unwrap_err();
    assert_eq!(err.key_problem_provider(), Some("omdb"), "{err:?}");
    assert!(matches!(err.root(), ProviderError::Auth { .. }));
    let text = err.to_string();
    assert!(text.contains("rejected your key"), "{text}");
    assert!(!text.contains(KEY), "{text}");
}

#[tokio::test]
async fn the_daily_limit_is_a_rate_limit_that_links_to_the_card() {
    let h = Harness::new().await;
    h.server
        .route_query("/", "s=Busy", denied("error_limit.json"));
    let err = h
        .provider
        .search(&query(MediaKind::Movie, "Busy", None), &opts())
        .await
        .unwrap_err();
    assert_eq!(err.key_problem_provider(), Some("omdb"), "{err:?}");
    match err.root() {
        ProviderError::RateLimited {
            provider,
            retry_after_secs,
        } => {
            assert_eq!(provider, "OMDb");
            assert!((1..=86_400).contains(retry_after_secs));
        }
        other => panic!("{other:?}"),
    }
    assert!(err.to_string().contains("limiting requests for your key"));
}

#[tokio::test]
async fn the_settings_test_uses_the_typed_key_and_skips_the_cache() {
    let h = Harness::build(Setup {
        key: None,
        cache: true,
        ..Setup::default()
    })
    .await;
    h.server
        .route_query("/", "i=tt0133093", ok("title_bare.json"));
    let schema = h.provider.credentials().expect("schema");
    let typed =
        ResolvedCredentials::from_user_values(schema, [("api_key", " typed-key-77 ")]).unwrap();
    h.provider.check_credentials(&typed).await.unwrap();
    h.provider.check_credentials(&typed).await.unwrap();
    let requests = h.server.requests();
    assert_eq!(requests.len(), 2, "never served from the cache");
    assert_eq!(
        param(&requests[0], "apikey").as_deref(),
        Some("typed-key-77")
    );

    let h = Harness::build(Setup {
        key: None,
        ..Setup::default()
    })
    .await;
    h.server
        .route_query("/", "i=tt0133093", denied("error_invalid_key.json"));
    let err = h.provider.check_credentials(&typed).await.unwrap_err();
    assert!(matches!(err.root(), ProviderError::Auth { .. }), "{err:?}");
}

#[tokio::test]
async fn the_provider_is_reachable_through_the_registry() {
    let h = Harness::new().await;
    h.server
        .route_query("/", "i=tt0133093", ok("title_bare.json"));
    let mut registry = ProviderRegistry::new();
    registry.register(Arc::new(h.provider.clone()));
    assert!(registry.get("omdb").is_some());
    let credentials = h.resolver.resolve("omdb").unwrap();
    registry
        .check_credentials("omdb", "OMDb", &credentials)
        .await
        .unwrap();
}

#[tokio::test]
async fn http_failures_without_a_json_body_are_request_errors() {
    let h = Harness::new().await;
    h.server.route_query("/", "s=Down", Reply::status(503));
    let err = h
        .provider
        .search(&query(MediaKind::Movie, "Down", None), &opts())
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::Request { .. }), "{err:?}");
    assert!(err.to_string().contains("503"), "{err}");

    h.server
        .route_query("/", "s=Junk", Reply::ok("<html>not json</html>"));
    let err = h
        .provider
        .search(&query(MediaKind::Movie, "Junk", None), &opts())
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::Request { .. }), "{err:?}");
}

// --- search and lookup -----------------------------------------------------------------------

#[tokio::test]
async fn a_movie_search_reads_hits_of_the_right_kind() {
    let h = Harness::new().await;
    let hits = h
        .provider
        .search(&query(MediaKind::Movie, "Heat", Some(1995)), &opts())
        .await
        .unwrap();
    // The game and the entry without an ID are left out.
    assert_eq!(hits.len(), 2);
    let heat = &hits[0];
    assert_eq!(heat.provider, "omdb");
    assert_eq!(heat.title, "Heat");
    assert_eq!(heat.year, 1995);
    assert_eq!(heat.ids.get("imdb"), Some("tt0113277"));
    assert_eq!(
        heat.poster_url.as_deref(),
        Some("https://img.example.test/omdb/heat.jpg")
    );
    assert!(!heat.exact_id);
    assert_eq!(hits[1].poster_url, None, "N/A is no poster");

    let request = &h.server.requests()[0];
    assert_eq!(param(request, "s").as_deref(), Some("Heat"));
    assert_eq!(param(request, "type").as_deref(), Some("movie"));
    assert_eq!(param(request, "y").as_deref(), Some("1995"));
}

#[tokio::test]
async fn a_show_search_asks_for_series_and_drops_episodes() {
    let h = Harness::new().await;
    let hits = h
        .provider
        .search(&query(MediaKind::Show, "Breaking Bad", None), &opts())
        .await
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].ids.get("imdb"), Some("tt0903747"));
    assert_eq!(hits[0].year, 2008);
    let request = &h.server.requests()[0];
    assert_eq!(param(request, "type").as_deref(), Some("series"));
    assert_eq!(param(request, "y"), None);
}

#[tokio::test]
async fn a_search_without_results_is_empty_not_an_error() {
    let h = Harness::new().await;
    for title in ["Zzzz nothing", "a"] {
        let hits = h
            .provider
            .search(&query(MediaKind::Movie, title, None), &opts())
            .await
            .unwrap();
        assert!(hits.is_empty(), "{title}");
    }
    let none = h
        .provider
        .search(&query(MediaKind::Movie, "  ", None), &opts())
        .await
        .unwrap();
    assert!(none.is_empty());
    assert_eq!(h.server.requests().len(), 2, "a blank title sends nothing");
}

#[tokio::test]
async fn an_imdb_id_is_looked_up_as_the_requested_title() {
    let h = Harness::new().await;
    let hit = h
        .provider
        .lookup(MediaKind::Movie, &ids(&[("imdb", " TT0113277 ")]), &opts())
        .await
        .unwrap()
        .expect("Heat");
    assert!(hit.exact_id);
    assert_eq!(hit.title, "Heat");
    assert_eq!(hit.year, 1995);
    assert_eq!(hit.ids.get("imdb"), Some("tt0113277"));
    assert!(hit.overview.starts_with("A group of high-end"));
}

#[tokio::test]
async fn lookups_that_cannot_match_are_none() {
    let h = Harness::new().await;
    // No IMDb ID: nothing to ask, nothing sent.
    let none = h
        .provider
        .lookup(MediaKind::Movie, &ids(&[("tmdb", "949")]), &opts())
        .await
        .unwrap();
    assert!(none.is_none());
    assert!(h.server.requests().is_empty());

    // An unknown ID, a series asked for as a movie, and an episode.
    for (kind, id) in [
        (MediaKind::Movie, "tt7654321"),
        (MediaKind::Movie, "tt0903747"),
        (MediaKind::Show, "tt0113277"),
        (MediaKind::Show, "tt0959621"),
    ] {
        let found = h
            .provider
            .lookup(kind, &ids(&[("imdb", id)]), &opts())
            .await
            .unwrap();
        assert!(found.is_none(), "{kind:?} {id}");
    }
}

// --- details ---------------------------------------------------------------------------------

#[tokio::test]
async fn movie_details_cover_text_people_ratings_and_artwork() {
    let h = Harness::new().await;
    let item = h
        .provider
        .details(MediaKind::Movie, &ids(&[("imdb", "tt0113277")]), &opts())
        .await
        .unwrap();
    assert_eq!(item.core.title, "Heat");
    assert_eq!(item.core.year, 1995);
    assert_eq!(item.core.ids.get("imdb"), Some("tt0113277"));
    assert!(item.core.plot.starts_with("A group of high-end"));
    assert_eq!(
        item.core.production_companies,
        vec!["Regency Enterprises", "Warner Bros."]
    );
    assert_eq!(
        item.core
            .artwork_urls
            .get(&MediaFileType::Poster)
            .map(String::as_str),
        Some("https://img.example.test/omdb/heat.jpg")
    );

    let ItemDetail::Movie(movie) = &item.detail else {
        panic!("movie");
    };
    assert_eq!(movie.english_title, "Heat");
    assert_eq!(movie.runtime_minutes, 170);
    assert_eq!(movie.release_date, CalendarDate::new(1995, 12, 15));
    assert_eq!(movie.certification.0, "US_R");
    assert_eq!(
        movie.genres,
        vec![
            Genre::parse("Action").unwrap(),
            Genre::parse("Crime").unwrap(),
            Genre::parse("Drama").unwrap()
        ]
    );
    assert_eq!(movie.spoken_languages, vec!["en", "es"]);
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
            (CreditKind::Writer, "Michael Mann"),
            (CreditKind::Actor, "Al Pacino"),
            (CreditKind::Actor, "Robert De Niro"),
            (CreditKind::Actor, "Val Kilmer"),
        ]
    );

    let imdb = item.core.ratings.get(IMDB_RATING_ID).unwrap();
    assert_eq!((imdb.value, imdb.votes, imdb.max_value), (8.3, 700_123, 10));
    let metacritic = item.core.ratings.get(METACRITIC_RATING_ID).unwrap();
    assert_eq!((metacritic.value, metacritic.max_value), (76.0, 100));
}

#[tokio::test]
async fn a_title_with_nothing_but_a_name_has_empty_fields() {
    let h = Harness::new().await;
    let item = h
        .provider
        .details(MediaKind::Movie, &ids(&[("imdb", "tt1234567")]), &opts())
        .await
        .unwrap();
    assert_eq!(item.core.title, "Obscure Short");
    assert_eq!(item.core.year, 0);
    assert!(item.core.plot.is_empty());
    assert!(item.core.ratings.is_empty());
    assert!(item.core.credits.is_empty());
    assert!(item.core.artwork_urls.is_empty());
    let ItemDetail::Movie(movie) = &item.detail else {
        panic!("movie");
    };
    assert_eq!(movie.runtime_minutes, 0);
    assert_eq!(movie.release_date, None);
    assert!(movie.genres.is_empty() && movie.countries.is_empty());
}

#[tokio::test]
async fn show_details_read_the_year_range_and_status() {
    let h = Harness::new().await;
    let item = h
        .provider
        .details(MediaKind::Show, &ids(&[("imdb", "tt0903747")]), &opts())
        .await
        .unwrap();
    assert_eq!(item.core.title, "Breaking Bad");
    assert_eq!(item.core.year, 2008);
    let ItemDetail::Show(show) = &item.detail else {
        panic!("show");
    };
    assert_eq!(show.year_end, 2013);
    assert_eq!(show.status, AiredStatus::Ended);
    assert_eq!(show.runtime_minutes, 49);
    assert_eq!(show.certification.0, "US_TV_MA");
    assert_eq!(show.country, "US");
    assert_eq!(show.first_aired, CalendarDate::new(2008, 1, 20));
}

#[tokio::test]
async fn details_of_the_wrong_kind_or_an_unknown_title_are_not_found() {
    let h = Harness::new().await;
    for (kind, id) in [
        (MediaKind::Show, "tt0113277"),
        (MediaKind::Movie, "tt0903747"),
        (MediaKind::Movie, "tt7654321"),
        (MediaKind::Movie, "tt0959621"),
    ] {
        let err = h
            .provider
            .details(kind, &ids(&[("imdb", id)]), &opts())
            .await
            .unwrap_err();
        assert!(
            matches!(err, ProviderError::NotFound(_)),
            "{kind:?} {id}: {err:?}"
        );
    }
    // Without an IMDb ID there is nothing to ask.
    let before = h.server.requests().len();
    let err = h
        .provider
        .details(MediaKind::Movie, &ids(&[("tmdb", "949")]), &opts())
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::NotFound(_)), "{err:?}");
    assert_eq!(h.server.requests().len(), before);
}

#[tokio::test]
async fn repeated_details_come_from_the_cache_and_a_forced_rescrape_does_not() {
    let h = Harness::build(Setup {
        cache: true,
        ..Setup::default()
    })
    .await;
    let wanted = ids(&[("imdb", "tt0113277")]);
    for _ in 0..2 {
        h.provider
            .details(MediaKind::Movie, &wanted, &opts())
            .await
            .unwrap();
    }
    assert_eq!(h.server.count("/"), 1);
    let forced = RequestOptions {
        bypass_cache: true,
        ..opts()
    };
    h.provider
        .details(MediaKind::Movie, &wanted, &forced)
        .await
        .unwrap();
    assert_eq!(h.server.count("/"), 2);
}

#[tokio::test]
async fn no_credential_reaches_the_cache_files() {
    let h = Harness::build(Setup {
        cache: true,
        ..Setup::default()
    })
    .await;
    h.provider
        .details(MediaKind::Movie, &ids(&[("imdb", "tt0113277")]), &opts())
        .await
        .unwrap();

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
        assert!(!text.contains(KEY), "{} contains the key", file.display());
        assert!(!file.to_string_lossy().contains(KEY));
    }
}

// --- episodes and ratings --------------------------------------------------------------------

#[tokio::test]
async fn the_aired_list_asks_for_each_season() {
    let h = Harness::new().await;
    let items = h
        .provider
        .episodes(
            &ids(&[("imdb", "tt0903747")]),
            &EpisodeGroup::aired(),
            &opts(),
        )
        .await
        .unwrap();
    // The entry without an episode number is left out.
    assert_eq!(numbers(&items), vec![(1, 1), (1, 2), (2, 1)]);
    let pilot = &items[0];
    assert_eq!(pilot.core.title, "Pilot");
    assert_eq!(pilot.core.ids.get("imdb"), Some("tt0959621"));
    assert_eq!(pilot.core.year, 2008);
    assert_eq!(pilot.core.ratings.get(IMDB_RATING_ID).unwrap().value, 9.0);
    let ItemDetail::Episode(fields) = &pilot.detail else {
        panic!("episode");
    };
    assert_eq!(fields.first_aired, CalendarDate::new(2008, 1, 20));

    let seasons: Vec<_> = h
        .server
        .requests()
        .iter()
        .filter_map(|r| param(r, "Season"))
        .collect();
    assert_eq!(seasons, vec!["1", "2"]);
}

#[tokio::test]
async fn a_season_omdb_lacks_leaves_a_gap_and_no_seasons_is_not_found() {
    let h = Harness::build(Setup {
        seasons: ["season_1.json", "error_season.json"],
        ..Setup::default()
    })
    .await;
    let items = h
        .provider
        .episodes(
            &ids(&[("imdb", "tt0903747")]),
            &EpisodeGroup::aired(),
            &opts(),
        )
        .await
        .unwrap();
    assert_eq!(numbers(&items), vec![(1, 1), (1, 2)]);

    let h = Harness::build(Setup {
        seasons: ["error_season.json", "error_season.json"],
        ..Setup::default()
    })
    .await;
    let err = h
        .provider
        .episodes(
            &ids(&[("imdb", "tt0903747")]),
            &EpisodeGroup::aired(),
            &opts(),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::NotFound(_)), "{err:?}");
}

#[tokio::test]
async fn other_orderings_and_movies_have_no_episode_list() {
    let h = Harness::new().await;
    let err = h
        .provider
        .episodes(
            &ids(&[("imdb", "tt0903747")]),
            &EpisodeGroup::dvd(),
            &opts(),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::Unsupported { .. }), "{err:?}");
    let err = h
        .provider
        .episodes(
            &ids(&[("imdb", "tt0113277")]),
            &EpisodeGroup::aired(),
            &opts(),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::NotFound(_)), "{err:?}");
}

#[tokio::test]
async fn ratings_cover_imdb_metacritic_and_every_rotten_tomatoes_score() {
    let h = Harness::new().await;
    let ratings = h
        .provider
        .ratings(MediaKind::Movie, &ids(&[("imdb", "tt0113277")]), &opts())
        .await
        .unwrap();
    let get = |id: &str| {
        let r = ratings.get(id).unwrap_or_else(|| panic!("{id}"));
        (r.value, r.votes, r.max_value)
    };
    assert_eq!(get(IMDB_RATING_ID), (8.3, 700_123, 10));
    assert_eq!(get(METACRITIC_RATING_ID), (76.0, 0, 100));
    // The tomato fields win over the (rounded) list entry.
    assert_eq!(get(RT_CRITICS_RATING_ID), (87.0, 70, 100));
    assert_eq!(get(RT_AUDIENCE_RATING_ID), (94.0, 1_500_000, 100));
    assert_eq!(get(RT_CRITICS_AVERAGE_RATING_ID), (7.9, 0, 10));
    assert_eq!(get(RT_AUDIENCE_AVERAGE_RATING_ID), (4.3, 0, 5));
}

#[tokio::test]
async fn ratings_of_a_title_without_any_are_empty() {
    let h = Harness::new().await;
    let ratings = h
        .provider
        .ratings(MediaKind::Movie, &ids(&[("imdb", "tt1234567")]), &opts())
        .await
        .unwrap();
    assert!(ratings.is_empty());
}
