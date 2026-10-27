//! The TheSportsDB provider against a local mock server that replays hand-built responses
//! (`tests/fixtures/thesportsdb/`) in the shape of the service's v1 JSON API. Nothing here
//! touches the network; the opt-in live smoke test is `tests/thesportsdb_live.rs`.

mod support;

use std::path::Path;
use std::sync::Arc;

use cv_domain::{
    AiredStatus, CalendarDate, EpisodeGroup, Genre, ItemDetail, MediaFileType, ProviderIds,
};
use cv_providers::model::{MediaKind, RequestOptions, SearchQuery};
use cv_providers::thesportsdb::{FREE_KEY, TheSportsDbProvider};
use cv_providers::{
    CredentialResolver, HttpClient, MemoryUserCredentials, MetadataProvider, Provider,
    ProviderError, catalogue,
};
use support::{MockServer, Reply};

const USER_KEY: &str = "supporter-key-0123456789";

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/thesportsdb")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("fixture {}: {e}", path.display()))
}

fn ok(name: &str) -> Reply {
    Reply::ok(&fixture(name))
}

struct Harness {
    server: MockServer,
    provider: TheSportsDbProvider,
    resolver: CredentialResolver,
    _cache_dir: tempfile::TempDir,
}

impl Harness {
    async fn new() -> Self {
        Self::build(None).await
    }

    async fn with_user_key() -> Self {
        Self::build(Some(USER_KEY)).await
    }

    async fn build(key: Option<&str>) -> Self {
        let server = MockServer::start().await;
        let cache_dir = tempfile::tempdir().unwrap();
        let http = HttpClient::builder()
            .cache_dir(cache_dir.path())
            .build()
            .unwrap();
        let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
        let used = key.unwrap_or(FREE_KEY);
        if let Some(key) = key {
            resolver
                .set_user(catalogue::key::THESPORTSDB, catalogue::API_KEY, key)
                .unwrap();
        }
        let base = format!("/json/{used}");
        server.route(&format!("{base}/all_leagues.php"), ok("all_leagues.json"));
        server.route_query(
            &format!("{base}/lookupleague.php"),
            "id=9001",
            ok("league_9001.json"),
        );
        server.route_query(
            &format!("{base}/lookupleague.php"),
            "id=9005",
            ok("league_bare.json"),
        );
        server.route_query(
            &format!("{base}/lookupleague.php"),
            "id=1",
            ok("league_none.json"),
        );
        server.route_query(
            &format!("{base}/search_all_seasons.php"),
            "id=9001",
            ok("seasons_9001.json"),
        );
        server.route_query(
            &format!("{base}/eventsseason.php"),
            "s=2014-2015",
            ok("events_2014-2015.json"),
        );
        server.route_query(
            &format!("{base}/eventsseason.php"),
            "s=2015-2016",
            ok("events_2015-2016.json"),
        );
        let provider =
            TheSportsDbProvider::with_base_url(http, resolver.clone(), &server.url("/json"));
        Self {
            server,
            provider,
            resolver,
            _cache_dir: cache_dir,
        }
    }
}

fn league_ids(id: &str) -> ProviderIds {
    let mut ids = ProviderIds::new();
    ids.set("thesportsdb", id);
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

// --- identity ---------------------------------------------------------------------------------

#[tokio::test]
async fn the_provider_offers_shows_and_needs_no_key() {
    let h = Harness::new().await;
    let capabilities = h.provider.capabilities();
    assert!(capabilities.shows && !capabilities.movies && !capabilities.best_effort);
    assert_eq!(capabilities.lookup_ids, vec!["thesportsdb"]);
    assert_eq!(h.provider.id(), "thesportsdb");
    assert_eq!(h.provider.name(), "TheSportsDB");
    // It declares an optional key, so Settings can offer a supporter's key; it is active without.
    let schema = h.provider.credentials().expect("a credential schema");
    assert!(schema.fields.iter().all(|f| !f.required));
    let status = h.resolver.status(catalogue::key::THESPORTSDB).unwrap();
    assert!(status.active);
}

#[tokio::test]
async fn new_registers_the_free_key_limits_on_its_lane() {
    let http = HttpClient::builder().build().unwrap();
    let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
    let _provider = TheSportsDbProvider::new(http.clone(), resolver);
    assert_eq!(http.lane_status("thesportsdb").max_concurrent, 2);
}

// --- search and lookup ------------------------------------------------------------------------

#[tokio::test]
async fn search_filters_the_league_list_by_name() {
    let h = Harness::new().await;
    let hits = h
        .provider
        .search(&query(MediaKind::Show, "northern football league"), &opts())
        .await
        .unwrap();
    let titles: Vec<_> = hits.iter().map(|h| h.title.as_str()).collect();
    // The exact name first, then the longer one.
    assert_eq!(
        titles,
        ["Northern Football League", "Northern Football League Cup"]
    );
    assert_eq!(hits[0].ids.get("thesportsdb"), Some("9001"));
    assert_eq!(hits[0].provider, "thesportsdb");
    assert!(!hits[0].exact_id);

    // Alternate names count.
    let by_alias = h
        .provider
        .search(&query(MediaKind::Show, "GPS"), &opts())
        .await
        .unwrap();
    assert_eq!(by_alias.len(), 1);
    assert_eq!(by_alias[0].title, "Grand Prix Series");

    // The league list is cached for the day: three searches, one request.
    assert_eq!(h.server.count("/json/123/all_leagues.php"), 1);
}

#[tokio::test]
async fn search_for_movies_or_blank_titles_makes_no_request() {
    let h = Harness::new().await;
    assert!(
        h.provider
            .search(&query(MediaKind::Movie, "Northern"), &opts())
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        h.provider
            .search(&query(MediaKind::Show, " "), &opts())
            .await
            .unwrap()
            .is_empty()
    );
    assert!(h.server.requests().is_empty());
}

#[tokio::test]
async fn lookup_returns_an_exact_hit_in_the_preferred_language() {
    let h = Harness::new().await;
    let mut german = opts();
    german.language = "de".to_owned();
    let hit = h
        .provider
        .lookup(MediaKind::Show, &league_ids("9001"), &german)
        .await
        .unwrap()
        .expect("a hit");
    assert!(hit.exact_id);
    assert_eq!(hit.title, "Northern Football League");
    assert_eq!(hit.year, 1992);
    assert!(hit.overview.starts_with("Die Northern Football League"));
    assert_eq!(
        hit.poster_url.as_deref(),
        Some("https://img.example.test/league/poster/poster.jpg")
    );

    // An unknown league (the service answers `{"leagues": null}`), an unsupported kind and no ID
    // are all "no match".
    for (kind, ids) in [
        (MediaKind::Show, league_ids("1")),
        (MediaKind::Movie, league_ids("9001")),
        (MediaKind::Show, ProviderIds::new()),
    ] {
        assert_eq!(h.provider.lookup(kind, &ids, &opts()).await.unwrap(), None);
    }
}

// --- details ----------------------------------------------------------------------------------

#[tokio::test]
async fn a_league_is_a_show_with_seasons_and_artwork() {
    let h = Harness::new().await;
    let item = h
        .provider
        .details(MediaKind::Show, &league_ids("9001"), &opts())
        .await
        .unwrap();

    let core = &item.core;
    assert_eq!(core.title, "Northern Football League");
    assert_eq!(core.original_title, "Northern Football League");
    assert_eq!(core.year, 1992);
    assert_eq!(core.ids.get("thesportsdb"), Some("9001"));
    assert_eq!(
        core.plot,
        "The Northern Football League is the top level of the example football system.\n\nIt is contested by twenty clubs."
    );
    for (kind, url) in [
        (MediaFileType::Poster, "poster/poster.jpg"),
        (MediaFileType::Fanart, "fanart/one.jpg"),
        (MediaFileType::Banner, "banner/banner.jpg"),
        (MediaFileType::Clearlogo, "logo/logo.png"),
        (MediaFileType::Thumb, "badge/badge.png"),
        (MediaFileType::Clearart, "trophy/trophy.png"),
    ] {
        let found = core.artwork_urls.get(&kind).expect("artwork");
        assert!(found.ends_with(url), "{kind:?}: {found}");
    }

    let ItemDetail::Show(show) = &item.detail else {
        panic!("a show");
    };
    assert_eq!(show.first_aired, CalendarDate::new(1992, 8, 15));
    assert_eq!(show.status, AiredStatus::Continuing);
    assert_eq!(show.genres, [Genre::parse("Soccer").unwrap()]);
    assert_eq!(show.country, "England");
    assert_eq!(show.episode_groups, [EpisodeGroup::aired()]);
    assert_eq!(
        show.extra_fanart_urls,
        [
            "https://img.example.test/league/fanart/two.jpg",
            "https://img.example.test/league/fanart/four.jpg"
        ]
    );
    // A season called by a year gets its number; `Playoffs` has none and is left out.
    let seasons: Vec<_> = show
        .season_texts
        .iter()
        .map(|s| (s.season, s.name.as_str()))
        .collect();
    assert_eq!(seasons, [(2014, "2014-2015"), (2015, "2015-2016")]);
}

#[tokio::test]
async fn a_bare_league_still_gives_a_show() {
    let h = Harness::new().await;
    let item = h
        .provider
        .details(MediaKind::Show, &league_ids("9005"), &opts())
        .await
        .unwrap();
    assert_eq!(item.core.title, "Bare League");
    assert_eq!(item.core.year, 0);
    assert!(item.core.plot.is_empty());
    assert!(item.core.artwork_urls.is_empty());
    let ItemDetail::Show(show) = &item.detail else {
        panic!("a show");
    };
    assert_eq!(show.status, AiredStatus::Unknown);
    assert!(show.genres.is_empty());
    assert!(show.season_texts.is_empty());
}

#[tokio::test]
async fn movies_and_unknown_leagues_are_not_details() {
    let h = Harness::new().await;
    assert!(matches!(
        h.provider
            .details(MediaKind::Movie, &league_ids("9001"), &opts())
            .await,
        Err(ProviderError::Unsupported { .. })
    ));
    assert_eq!(
        h.provider
            .details(MediaKind::Show, &league_ids("1"), &opts())
            .await,
        Err(ProviderError::NotFound("TheSportsDB".to_owned()))
    );
    assert_eq!(
        h.provider
            .details(MediaKind::Show, &ProviderIds::new(), &opts())
            .await,
        Err(ProviderError::NotFound("TheSportsDB".to_owned()))
    );
}

// --- episodes ---------------------------------------------------------------------------------

#[tokio::test]
async fn events_are_episodes_numbered_in_the_order_they_were_played() {
    let h = Harness::new().await;
    let items = h
        .provider
        .episodes(&league_ids("9001"), &EpisodeGroup::aired(), &opts())
        .await
        .unwrap();

    let rows: Vec<_> = items
        .iter()
        .map(|item| {
            let ItemDetail::Episode(episode) = &item.detail else {
                panic!("an episode");
            };
            let number = episode.numbers.get(&EpisodeGroup::aired()).unwrap();
            (
                number.season,
                number.episode,
                item.core.title.as_str(),
                item.core.ids.get("thesportsdb").unwrap(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            (2014, 1, "Wanderers vs Athletic", "5001"),
            (2014, 2, "City vs Albion", "5002"),
            (2014, 3, "Rovers vs United", "5003"),
            (2014, 4, "Postponed vs Match", "5004"),
            (2015, 1, "Albion vs Rovers", "6001"),
            (2015, 2, "United vs City", "6002"),
        ]
    );
    assert_eq!(items[1].core.plot, "The opening weekend.");
    assert_eq!(items[2].core.year, 2014);
    assert_eq!(
        items[2]
            .core
            .artwork_urls
            .get(&MediaFileType::Thumb)
            .map(String::as_str),
        Some("https://img.example.test/event/thumb3.jpg")
    );
    assert_eq!(
        items[5]
            .core
            .artwork_urls
            .get(&MediaFileType::Thumb)
            .map(String::as_str),
        Some("https://img.example.test/event/poster2.jpg")
    );
    let ItemDetail::Episode(first) = &items[0].detail else {
        panic!("an episode");
    };
    assert_eq!(first.first_aired, CalendarDate::new(2014, 8, 16));

    // The season called `Playoffs` has no number, so its events were not requested.
    assert!(
        h.server
            .requests()
            .iter()
            .all(|r| !r.target.contains("Playoffs"))
    );
}

#[tokio::test]
async fn orderings_the_service_lacks_and_empty_leagues_are_not_found() {
    let h = Harness::new().await;
    assert_eq!(
        h.provider
            .episodes(&league_ids("9001"), &EpisodeGroup::dvd(), &opts())
            .await,
        Err(ProviderError::NotFound("TheSportsDB".to_owned()))
    );
    assert_eq!(
        h.provider
            .episodes(&ProviderIds::new(), &EpisodeGroup::aired(), &opts())
            .await,
        Err(ProviderError::NotFound("TheSportsDB".to_owned()))
    );

    // A league whose every season answers `{"events": null}`.
    let fresh = Harness::new().await;
    fresh.server.route_query(
        "/json/123/search_all_seasons.php",
        "id=9005",
        Reply::ok(r#"{"seasons":[{"strSeason":"2016-2017"}]}"#),
    );
    fresh.server.route_query(
        "/json/123/eventsseason.php",
        "id=9005",
        ok("events_empty.json"),
    );
    assert_eq!(
        fresh
            .provider
            .episodes(&league_ids("9005"), &EpisodeGroup::aired(), &opts())
            .await,
        Err(ProviderError::NotFound("TheSportsDB".to_owned()))
    );
}

// --- the key ----------------------------------------------------------------------------------

#[tokio::test]
async fn the_users_key_replaces_the_free_key_in_every_request() {
    let h = Harness::with_user_key().await;
    h.provider
        .details(MediaKind::Show, &league_ids("9001"), &opts())
        .await
        .unwrap();
    let requests = h.server.requests();
    assert!(!requests.is_empty());
    assert!(
        requests
            .iter()
            .all(|r| r.path().starts_with(&format!("/json/{USER_KEY}/")))
    );
}

#[tokio::test]
async fn without_a_key_the_free_key_is_used() {
    let h = Harness::new().await;
    h.provider
        .lookup(MediaKind::Show, &league_ids("9001"), &opts())
        .await
        .unwrap();
    assert!(
        h.server
            .requests()
            .iter()
            .all(|r| r.path().starts_with("/json/123/"))
    );
}

#[tokio::test]
async fn a_rejected_key_is_an_authentication_error() {
    const REJECTED: &str = "rejected-key-0123456789";
    let h = Harness::build(Some(REJECTED)).await;
    let mut reply = Reply::ok(&fixture("error_key_disabled.txt"));
    reply.status = 403;
    h.server.route_query(
        &format!("/json/{REJECTED}/lookupleague.php"),
        "id=4328",
        reply,
    );
    let schema = catalogue::CREDENTIAL_SCHEMAS
        .iter()
        .find(|s| s.provider == catalogue::key::THESPORTSDB)
        .unwrap();
    let credentials =
        cv_providers::ResolvedCredentials::from_user_values(schema, [("api_key", REJECTED)])
            .unwrap();
    let error = h
        .provider
        .check_credentials(&credentials)
        .await
        .unwrap_err();
    assert!(matches!(error, ProviderError::Auth { .. }), "{error:?}");
    assert!(!error.to_string().contains(REJECTED));
}

#[tokio::test]
async fn test_credentials_accepts_a_key_the_service_answers_for() {
    let h = Harness::new().await;
    h.server.route_query(
        "/json/123/lookupleague.php",
        "id=4328",
        ok("league_9001.json"),
    );
    h.provider
        .check_credentials(&cv_providers::ResolvedCredentials::default())
        .await
        .unwrap();
    // Testing never reads the cache.
    h.provider
        .check_credentials(&cv_providers::ResolvedCredentials::default())
        .await
        .unwrap();
    assert_eq!(h.server.count("/json/123/lookupleague.php"), 2);
}

#[tokio::test]
async fn the_key_never_reaches_the_cache_or_the_logs_of_a_failed_request() {
    let h = Harness::with_user_key().await;
    h.server.route(
        &format!("/json/{USER_KEY}/all_leagues.php"),
        Reply::status(500),
    );
    let error = h
        .provider
        .search(&query(MediaKind::Show, "Northern"), &opts())
        .await
        .unwrap_err();
    assert!(!error.to_string().contains(USER_KEY), "{error}");
}
