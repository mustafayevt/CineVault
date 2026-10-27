//! The Trakt metadata provider against a local mock server that replays hand-built responses in
//! Trakt's documented shape (`tests/fixtures/trakt/`). Nothing here touches the network; the
//! opt-in live smoke test is `tests/trakt_live.rs`.

mod support;

use std::path::Path;
use std::sync::Arc;

use cv_domain::{
    AiredStatus, CalendarDate, CreditKind, EpisodeGroup, EpisodeOrdering, Genre, Item, ItemDetail,
    ProviderIds,
};
use cv_providers::model::{MediaKind, RequestOptions, SearchQuery};
use cv_providers::trakt::{RATING_ID, TraktProvider};
use cv_providers::{
    CredentialResolver, HttpClient, MemoryUserCredentials, MetadataProvider, Provider,
    ProviderError, ProviderRegistry, RatingProvider, ResolvedCredentials, catalogue,
};
use support::{MockServer, Recorded, Reply};

const CLIENT_ID: &str = "trakt-client-id-0123456789";

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/trakt")
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
    client_id: Option<&'static str>,
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            cache: false,
            client_id: Some(CLIENT_ID),
        }
    }
}

struct Harness {
    server: MockServer,
    provider: TraktProvider,
    resolver: CredentialResolver,
    cache_dir: Option<tempfile::TempDir>,
}

impl Harness {
    async fn new() -> Self {
        Self::build(Setup::default()).await
    }

    async fn build(setup: Setup) -> Self {
        let server = MockServer::start().await;
        server.route("/genres/movies", Reply::ok("[]"));
        for movie in ["3", "tt0113277"] {
            server.route(&format!("/movies/{movie}"), ok("movie_summary.json"));
            server.route(&format!("/movies/{movie}/people"), ok("movie_people.json"));
            server.route(
                &format!("/movies/{movie}/translations/tr"),
                ok("movie_translation_tr.json"),
            );
        }
        server.route("/movies/9", ok("movie_bare.json"));
        server.route("/search/movie", ok("search_movie.json"));
        server.route("/search/trakt/3", ok("id_lookup_movie.json"));
        server.route("/search/imdb/tt0113277", ok("id_lookup_movie.json"));
        server.route("/search/tmdb/949", ok("id_lookup_movie.json"));
        server.route("/shows/1388", ok("show_summary.json"));
        server.route("/shows/1388/people", ok("show_people.json"));
        server.route("/shows/1388/seasons", ok("show_seasons.json"));
        server.route(
            "/shows/1388/translations/tr",
            ok("show_translation_tr.json"),
        );
        let cache_dir = setup.cache.then(|| tempfile::tempdir().unwrap());
        let mut builder = HttpClient::builder();
        if let Some(dir) = &cache_dir {
            builder = builder.cache_dir(dir.path());
        }
        let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
        if let Some(id) = setup.client_id {
            resolver
                .set_user(catalogue::key::TRAKT, "client_id", id)
                .unwrap();
        }
        let provider = TraktProvider::with_base_url(
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

fn turkish() -> RequestOptions {
    RequestOptions {
        language: "tr".into(),
        country: "TR".into(),
        ..RequestOptions::default()
    }
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

fn movie_ids() -> ProviderIds {
    ids(&[("trakt", "3")])
}

fn show_ids() -> ProviderIds {
    ids(&[("trakt", "1388")])
}

fn query(kind: MediaKind, title: &str, year: Option<u16>) -> SearchQuery {
    SearchQuery {
        kind,
        title: title.to_owned(),
        year,
    }
}

fn number(item: &Item, group: &EpisodeGroup) -> Option<(u32, u32)> {
    let ItemDetail::Episode(fields) = &item.detail else {
        panic!("not an episode")
    };
    fields.numbers.get(group).map(|n| (n.season, n.episode))
}

// --- credentials -----------------------------------------------------------------------------

#[tokio::test]
async fn the_provider_describes_itself() {
    let h = Harness::new().await;
    assert_eq!(h.provider.id(), "trakt");
    assert_eq!(h.provider.name(), "Trakt");
    let schema = h.provider.credentials().expect("schema");
    assert!(schema.field("client_id").is_some_and(|f| f.required));

    let capabilities = h.provider.capabilities();
    assert!(capabilities.movies && capabilities.shows && !capabilities.best_effort);
    assert_eq!(
        capabilities.orderings,
        vec![EpisodeOrdering::Aired, EpisodeOrdering::Absolute]
    );
    assert_eq!(
        capabilities.lookup_ids,
        vec!["trakt", "imdb", "tmdb", "tvdb"]
    );
}

#[tokio::test]
async fn without_a_client_id_nothing_is_sent() {
    let h = Harness::build(Setup {
        client_id: None,
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
            provider, fields, ..
        } => {
            assert_eq!(provider, "Trakt");
            assert_eq!(fields, vec!["client_id".to_owned()]);
        }
        other => panic!("{other:?}"),
    }
    assert!(h.server.requests().is_empty());
}

#[tokio::test]
async fn metadata_needs_the_client_id_only() {
    let h = Harness::new().await;
    h.provider
        .details(MediaKind::Movie, &movie_ids(), &opts())
        .await
        .unwrap();

    for request in h.server.requests() {
        assert_eq!(
            request.headers.get("trakt-api-key").map(String::as_str),
            Some(CLIENT_ID)
        );
        assert_eq!(
            request.headers.get("trakt-api-version").map(String::as_str),
            Some("2")
        );
        assert!(!request.target.contains(CLIENT_ID));
        assert!(!request.headers.contains_key("authorization"));
    }
}

#[tokio::test]
async fn a_rejected_client_id_is_an_auth_error_without_the_id_in_the_message() {
    let h = Harness::new().await;
    h.server
        .route("/search/movie", status(403, "error_401.json"));
    let err = h
        .provider
        .search(&query(MediaKind::Movie, "Heat", None), &opts())
        .await
        .unwrap_err();
    assert!(matches!(err.root(), ProviderError::Auth { .. }), "{err:?}");
    assert!(err.to_string().contains("invalid client id"), "{err}");
    assert!(!err.to_string().contains(CLIENT_ID), "{err}");
}

#[tokio::test]
async fn a_rejected_client_id_links_to_the_card() {
    let h = Harness::new().await;
    h.server
        .route("/search/movie", status(401, "error_401.json"));
    let err = h
        .provider
        .search(&query(MediaKind::Movie, "Heat", None), &opts())
        .await
        .unwrap_err();
    assert_eq!(err.key_problem_provider(), Some("trakt"), "{err:?}");
    assert!(matches!(err.root(), ProviderError::Auth { .. }));
    let text = err.to_string();
    assert!(text.contains("rejected your key"), "{text}");
    assert!(!text.contains(CLIENT_ID), "{text}");
}

#[tokio::test]
async fn the_settings_test_uses_the_typed_client_id_and_skips_the_cache() {
    let h = Harness::build(Setup {
        client_id: None,
        cache: true,
    })
    .await;
    let schema = h.provider.credentials().expect("schema");
    let typed =
        ResolvedCredentials::from_user_values(schema, [("client_id", " typed-id-77 ")]).unwrap();
    h.provider.check_credentials(&typed).await.unwrap();
    h.provider.check_credentials(&typed).await.unwrap();
    let requests = h.server.requests();
    assert_eq!(requests.len(), 2, "never served from the cache");
    assert_eq!(
        requests[0].headers.get("trakt-api-key").map(String::as_str),
        Some("typed-id-77")
    );

    h.server
        .route("/genres/movies", status(403, "error_401.json"));
    let err = h.provider.check_credentials(&typed).await.unwrap_err();
    assert!(matches!(err.root(), ProviderError::Auth { .. }), "{err:?}");
}

#[tokio::test]
async fn the_provider_is_reachable_through_the_registry() {
    let h = Harness::new().await;
    let mut registry = ProviderRegistry::new();
    registry.register(Arc::new(h.provider.clone()));
    assert!(registry.get("trakt").is_some());
    let credentials = h.resolver.resolve("trakt").unwrap();
    registry
        .check_credentials("trakt", "Trakt", &credentials)
        .await
        .unwrap();
}

#[tokio::test]
async fn server_errors_are_request_errors_and_unknown_titles_are_not_found() {
    let h = Harness::new().await;
    h.server.route("/movies/500", Reply::status(503));
    let err = h
        .provider
        .details(MediaKind::Movie, &ids(&[("trakt", "500")]), &opts())
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::Request { .. }), "{err:?}");
    assert!(err.to_string().contains("503"), "{err}");

    // The mock answers 404 for paths it does not know.
    let err = h
        .provider
        .details(MediaKind::Movie, &ids(&[("trakt", "404")]), &opts())
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::NotFound(_)), "{err:?}");
}

// --- search and lookup -----------------------------------------------------------------------

#[tokio::test]
async fn a_movie_search_reads_hits_and_sends_the_year() {
    let h = Harness::new().await;
    let hits = h
        .provider
        .search(&query(MediaKind::Movie, "Heat", Some(1995)), &opts())
        .await
        .unwrap();
    // The result without a Trakt ID is left out.
    assert_eq!(hits.len(), 2);
    let heat = &hits[0];
    assert_eq!(heat.provider, "trakt");
    assert_eq!(heat.title, "Heat");
    assert_eq!(heat.year, 1995);
    assert_eq!(heat.ids.get("trakt"), Some("3"));
    assert_eq!(heat.ids.get("imdb"), Some("tt0113277"));
    assert_eq!(heat.ids.get("tmdb"), Some("949"));
    assert!(heat.overview.starts_with("Obsessive"));
    assert!(!heat.exact_id);
    assert_eq!(hits[1].overview, "");

    let request = &h.server.requests()[0];
    assert_eq!(param(request, "query").as_deref(), Some("Heat"));
    assert_eq!(param(request, "years").as_deref(), Some("1995"));
    assert_eq!(param(request, "extended").as_deref(), Some("full"));
}

#[tokio::test]
async fn a_show_search_uses_the_show_endpoint_and_a_blank_title_sends_nothing() {
    let h = Harness::new().await;
    h.server.route(
        "/search/show",
        Reply::ok(
            r#"[{"type":"show","score":1,"show":{"title":"Breaking Bad","year":2008,"ids":{"trakt":1388,"tvdb":81189}}},
                {"type":"movie","score":1,"movie":{"title":"Wrong kind","ids":{"trakt":5}}}]"#,
        ),
    );
    let hits = h
        .provider
        .search(&query(MediaKind::Show, "Breaking Bad", None), &opts())
        .await
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].ids.get("tvdb"), Some("81189"));
    assert_eq!(param(&h.server.requests()[0], "years"), None);

    let none = h
        .provider
        .search(&query(MediaKind::Show, "  ", None), &opts())
        .await
        .unwrap();
    assert!(none.is_empty());
    assert_eq!(h.server.requests().len(), 1);
}

#[tokio::test]
async fn an_id_lookup_returns_the_requested_title_without_a_text_search() {
    let h = Harness::new().await;
    let hit = h
        .provider
        .lookup(MediaKind::Movie, &ids(&[("imdb", "tt0113277")]), &opts())
        .await
        .unwrap()
        .expect("Heat");
    assert!(hit.exact_id);
    assert_eq!(hit.ids.get("trakt"), Some("3"));
    assert_eq!(h.server.count("/search/movie"), 0);
    let request = &h.server.requests()[0];
    assert_eq!(request.path(), "/search/imdb/tt0113277");
    assert_eq!(param(request, "type").as_deref(), Some("movie"));
}

#[tokio::test]
async fn lookup_tries_the_trakt_id_first_then_the_others() {
    let h = Harness::new().await;
    h.server
        .route("/search/trakt/12", ok("id_lookup_empty.json"));
    let hit = h
        .provider
        .lookup(
            MediaKind::Movie,
            &ids(&[("trakt", "12"), ("imdb", "tt0113277"), ("tmdb", "949")]),
            &opts(),
        )
        .await
        .unwrap()
        .expect("found through the IMDb ID");
    assert_eq!(hit.ids.get("trakt"), Some("3"));
    let paths: Vec<_> = h
        .server
        .requests()
        .iter()
        .map(|r| r.path().to_owned())
        .collect();
    assert_eq!(paths, ["/search/trakt/12", "/search/imdb/tt0113277"]);
}

#[tokio::test]
async fn lookups_that_cannot_match_are_none() {
    let h = Harness::new().await;
    // Nothing to ask.
    assert!(
        h.provider
            .lookup(MediaKind::Movie, &ProviderIds::new(), &opts())
            .await
            .unwrap()
            .is_none()
    );
    // A malformed IMDb ID is not sent.
    assert!(
        h.provider
            .lookup(MediaKind::Movie, &ids(&[("imdb", "nm0000199")]), &opts())
            .await
            .unwrap()
            .is_none()
    );
    // A TVDB ID is a show ID, so movies skip it.
    assert!(
        h.provider
            .lookup(MediaKind::Movie, &ids(&[("tvdb", "81189")]), &opts())
            .await
            .unwrap()
            .is_none()
    );
    assert!(h.server.requests().is_empty());
    // An ID Trakt does not know.
    assert!(
        h.provider
            .lookup(MediaKind::Movie, &ids(&[("tmdb", "1")]), &opts())
            .await
            .unwrap()
            .is_none()
    );
}

// --- details ---------------------------------------------------------------------------------

#[tokio::test]
async fn movie_details_cover_text_people_rating_and_trailer() {
    let h = Harness::new().await;
    let item = h
        .provider
        .details(MediaKind::Movie, &movie_ids(), &opts())
        .await
        .unwrap();
    assert_eq!(item.core.title, "Heat");
    assert_eq!(item.core.original_title, "Heat");
    assert_eq!(item.core.year, 1995);
    assert_eq!(item.core.ids.get("trakt"), Some("3"));
    assert_eq!(item.core.ids.get("imdb"), Some("tt0113277"));
    assert_eq!(item.core.ids.get("tmdb"), Some("949"));
    assert!(item.core.plot.starts_with("Obsessive master thief"));

    let ItemDetail::Movie(movie) = &item.detail else {
        panic!("movie");
    };
    assert_eq!(movie.english_title, "Heat");
    assert_eq!(movie.tagline, "A Los Angeles crime saga");
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
    assert_eq!(movie.spoken_languages, vec!["en"]);
    assert_eq!(movie.countries, vec!["US"]);
    assert_eq!(movie.trailers.len(), 1);
    assert_eq!(movie.trailers[0].id, "2GfZl4kuVNI");
    assert_eq!(movie.trailers[0].provider, "trakt");

    let rating = item.core.ratings.get(RATING_ID).unwrap();
    assert!((rating.value - 7.954_32).abs() < 1e-4);
    assert_eq!((rating.votes, rating.max_value), (21_034, 10));

    // Cast first, then directors, writers and producers; casting and costume are left out.
    let credits: Vec<_> = item
        .core
        .credits
        .iter()
        .map(|c| (c.kind, c.person.name.as_str(), c.role.as_str()))
        .collect();
    assert_eq!(
        credits,
        vec![
            (CreditKind::Actor, "Al Pacino", "Lt. Vincent Hanna"),
            (CreditKind::Actor, "Robert De Niro", "Neil McCauley"),
            (CreditKind::Actor, "Unnamed Extra", ""),
            (CreditKind::Director, "Michael Mann", "Director"),
            (CreditKind::Producer, "Art Linson", "Producer"),
            (CreditKind::Writer, "Michael Mann", "Writer"),
        ]
    );
    let pacino = &item.core.credits[0].person;
    assert_eq!(pacino.ids.get("trakt"), Some("100"));
    assert_eq!(pacino.ids.get("imdb"), Some("nm0000199"));
}

#[tokio::test]
async fn details_find_the_title_by_imdb_id_or_through_a_tmdb_id() {
    let h = Harness::new().await;
    let item = h
        .provider
        .details(MediaKind::Movie, &ids(&[("imdb", "tt0113277")]), &opts())
        .await
        .unwrap();
    assert_eq!(item.core.title, "Heat");
    let paths: Vec<_> = h
        .server
        .requests()
        .iter()
        .map(|r| r.path().to_owned())
        .collect();
    assert_eq!(paths, ["/movies/tt0113277", "/movies/tt0113277/people"]);

    let h = Harness::new().await;
    let item = h
        .provider
        .details(MediaKind::Movie, &ids(&[("tmdb", "949")]), &opts())
        .await
        .unwrap();
    assert_eq!(item.core.title, "Heat");
    let paths: Vec<_> = h
        .server
        .requests()
        .iter()
        .map(|r| r.path().to_owned())
        .collect();
    assert_eq!(paths, ["/search/tmdb/949", "/movies/3", "/movies/3/people"]);

    let err = h
        .provider
        .details(MediaKind::Movie, &ids(&[("tmdb", "1")]), &opts())
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::NotFound(_)), "{err:?}");
}

#[tokio::test]
async fn english_needs_no_translation_request() {
    let h = Harness::new().await;
    h.provider
        .details(MediaKind::Movie, &movie_ids(), &opts())
        .await
        .unwrap();
    assert_eq!(h.server.count("/movies/3/translations/tr"), 0);
}

#[tokio::test]
async fn titles_and_plots_follow_the_preferred_language_and_country() {
    let h = Harness::new().await;
    let item = h
        .provider
        .details(MediaKind::Movie, &movie_ids(), &turkish())
        .await
        .unwrap();
    assert_eq!(item.core.title, "Sıcak Takip");
    assert_eq!(item.core.original_title, "Heat");
    assert!(item.core.plot.starts_with("Usta hırsız"));
    let ItemDetail::Movie(movie) = &item.detail else {
        panic!("movie");
    };
    // The Turkish entry has no tagline, so the English one stays.
    assert_eq!(movie.tagline, "A Los Angeles crime saga");
    assert_eq!(movie.english_title, "Heat");

    // With another country, that country's translation is used.
    let cyprus = RequestOptions {
        country: "CY".into(),
        ..turkish()
    };
    let item = h
        .provider
        .details(MediaKind::Movie, &movie_ids(), &cyprus)
        .await
        .unwrap();
    assert_eq!(item.core.title, "Sıcak Takip (Kıbrıs)");
    let ItemDetail::Movie(movie) = &item.detail else {
        panic!("movie");
    };
    assert_eq!(movie.tagline, "Bir suç destanı");
}

#[tokio::test]
async fn a_missing_translation_keeps_the_english_text() {
    let h = Harness::new().await;
    let german = RequestOptions {
        language: "de".into(),
        ..opts()
    };
    let item = h
        .provider
        .details(MediaKind::Movie, &movie_ids(), &german)
        .await
        .unwrap();
    assert_eq!(item.core.title, "Heat");
    assert!(item.core.plot.starts_with("Obsessive"));
    assert_eq!(h.server.count("/movies/3/translations/de"), 1);
}

#[tokio::test]
async fn nulls_and_missing_values_leave_fields_empty() {
    let h = Harness::new().await;
    h.server.route("/movies/9/people", Reply::ok("{}"));
    let item = h
        .provider
        .details(MediaKind::Movie, &ids(&[("trakt", "9")]), &opts())
        .await
        .unwrap();
    assert_eq!(item.core.title, "Obscure Short");
    assert_eq!(item.core.year, 0);
    assert!(item.core.plot.is_empty());
    assert!(item.core.ratings.is_empty());
    assert!(item.core.credits.is_empty());
    let ItemDetail::Movie(movie) = &item.detail else {
        panic!("movie");
    };
    assert!(movie.genres.is_empty() && movie.trailers.is_empty());
}

#[tokio::test]
async fn people_the_title_lacks_are_not_an_error() {
    let h = Harness::new().await;
    // `/movies/9/people` is unknown to the mock and answers 404.
    let item = h
        .provider
        .details(MediaKind::Movie, &ids(&[("trakt", "9")]), &opts())
        .await
        .unwrap();
    assert!(item.core.credits.is_empty());
}

#[tokio::test]
async fn show_details_cover_status_network_and_groups() {
    let h = Harness::new().await;
    let item = h
        .provider
        .details(MediaKind::Show, &show_ids(), &opts())
        .await
        .unwrap();
    assert_eq!(item.core.title, "Breaking Bad");
    assert_eq!(item.core.year, 2008);
    assert_eq!(item.core.ids.get("tvdb"), Some("81189"));
    assert_eq!(item.core.production_companies, vec!["AMC"]);
    let ItemDetail::Show(show) = &item.detail else {
        panic!("show");
    };
    assert_eq!(show.status, AiredStatus::Ended);
    assert_eq!(show.first_aired, CalendarDate::new(2008, 1, 21));
    assert_eq!(show.runtime_minutes, 45);
    assert_eq!(show.certification.0, "US_TV_MA");
    assert_eq!(show.country, "US");
    assert_eq!(show.episode_groups, vec![EpisodeGroup::aired()]);
    // The creator is credited as a writer.
    assert!(
        item.core
            .credits
            .iter()
            .any(|c| c.kind == CreditKind::Writer && c.person.name == "Vince Gilligan")
    );
}

#[tokio::test]
async fn show_text_follows_the_preferred_language() {
    let h = Harness::new().await;
    let item = h
        .provider
        .details(MediaKind::Show, &show_ids(), &turkish())
        .await
        .unwrap();
    assert_eq!(item.core.title, "Kırılma Noktası");
    assert_eq!(item.core.original_title, "Breaking Bad");
}

#[tokio::test]
async fn repeated_details_come_from_the_cache_and_a_forced_rescrape_does_not() {
    let h = Harness::build(Setup {
        cache: true,
        ..Setup::default()
    })
    .await;
    for _ in 0..2 {
        h.provider
            .details(MediaKind::Movie, &movie_ids(), &opts())
            .await
            .unwrap();
    }
    assert_eq!(h.server.count("/movies/3"), 1);
    let forced = RequestOptions {
        bypass_cache: true,
        ..opts()
    };
    h.provider
        .details(MediaKind::Movie, &movie_ids(), &forced)
        .await
        .unwrap();
    assert_eq!(h.server.count("/movies/3"), 2);
}

#[tokio::test]
async fn no_credential_reaches_the_cache_files() {
    let h = Harness::build(Setup {
        cache: true,
        ..Setup::default()
    })
    .await;
    h.provider
        .details(MediaKind::Movie, &movie_ids(), &opts())
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
        assert!(
            !text.contains(CLIENT_ID),
            "{} holds a credential",
            file.display()
        );
        assert!(!file.to_string_lossy().contains(CLIENT_ID));
    }
}

// --- episodes and ratings --------------------------------------------------------------------

#[tokio::test]
async fn the_aired_list_has_specials_first_and_every_season_in_order() {
    let h = Harness::new().await;
    let items = h
        .provider
        .episodes(&show_ids(), &EpisodeGroup::aired(), &opts())
        .await
        .unwrap();
    let aired: Vec<_> = items
        .iter()
        .map(|i| number(i, &EpisodeGroup::aired()).unwrap())
        .collect();
    assert_eq!(aired, vec![(0, 1), (1, 1), (1, 2), (2, 1)]);

    let pilot = &items[1];
    assert_eq!(pilot.core.title, "Pilot");
    assert_eq!(pilot.core.plot, "Walter is diagnosed.");
    assert_eq!(pilot.core.ids.get("trakt"), Some("6001"));
    assert_eq!(pilot.core.ids.get("imdb"), Some("tt0959621"));
    assert_eq!(pilot.core.ids.get("tvdb"), Some("349232"));
    assert_eq!(pilot.core.year, 2008);
    assert_eq!(pilot.core.ratings.get(RATING_ID).unwrap().votes, 5000);
    let ItemDetail::Episode(fields) = &pilot.detail else {
        panic!("episode");
    };
    assert_eq!(fields.first_aired, CalendarDate::new(2008, 1, 21));
    assert_eq!(fields.runtime_minutes, 58);
    assert_eq!(number(pilot, &EpisodeGroup::absolute()), Some((1, 1)));

    let request = &h.server.requests()[0];
    assert_eq!(param(request, "extended").as_deref(), Some("episodes,full"));

    // Missing values stay empty.
    let sparse = &items[2];
    assert!(sparse.core.plot.is_empty() && sparse.core.ratings.is_empty());
}

#[tokio::test]
async fn the_absolute_list_holds_the_episodes_with_absolute_numbers() {
    let h = Harness::new().await;
    let items = h
        .provider
        .episodes(&show_ids(), &EpisodeGroup::absolute(), &opts())
        .await
        .unwrap();
    let absolute: Vec<_> = items
        .iter()
        .map(|i| number(i, &EpisodeGroup::absolute()).unwrap())
        .collect();
    assert_eq!(absolute, vec![(1, 1), (1, 2)]);
    // They keep their aired numbers too.
    assert_eq!(number(&items[1], &EpisodeGroup::aired()), Some((1, 2)));
}

#[tokio::test]
async fn orderings_trakt_lacks_are_not_found() {
    let h = Harness::new().await;
    let err = h
        .provider
        .episodes(&show_ids(), &EpisodeGroup::dvd(), &opts())
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::NotFound(_)), "{err:?}");
    assert!(h.server.requests().is_empty());

    h.server.route("/shows/1388/seasons", Reply::ok("[]"));
    let err = h
        .provider
        .episodes(&show_ids(), &EpisodeGroup::aired(), &opts())
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::NotFound(_)), "{err:?}");
}

#[tokio::test]
async fn the_trakt_rating_comes_from_the_summary() {
    let h = Harness::new().await;
    let ratings = h
        .provider
        .ratings(MediaKind::Movie, &movie_ids(), &opts())
        .await
        .unwrap();
    let rating = ratings.get(RATING_ID).unwrap();
    assert_eq!((rating.votes, rating.max_value), (21_034, 10));
    assert_eq!(
        h.server.count("/movies/3/people"),
        0,
        "ratings need no people"
    );

    let none = h
        .provider
        .ratings(MediaKind::Movie, &ids(&[("trakt", "9")]), &opts())
        .await
        .unwrap();
    assert!(none.is_empty());
}
