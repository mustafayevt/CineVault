//! The AniDB provider against a local mock server that replays hand-built responses
//! (`tests/fixtures/anidb/`) in the shape of AniDB's HTTP API XML and its title dump. AniDB bans
//! clients that send unregistered or too many requests, so nothing here was recorded from the
//! service; the opt-in live test (`tests/anidb_live.rs`) needs a registered client. Nothing here
//! touches the network.

mod support;

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use cv_domain::{
    AiredStatus, CalendarDate, CreditKind, EpisodeGroup, Genre, ItemDetail, MediaFileType,
    ProviderIds, id_key,
};
use cv_providers::http::ProviderLimits;
use cv_providers::model::{MediaKind, RequestOptions, SearchQuery};
use cv_providers::{
    AnidbProvider, CredentialResolver, HttpClient, MemoryUserCredentials, MetadataProvider,
    Provider, ProviderError, ResolvedCredentials, anidb, catalogue,
};
use support::{MockServer, Reply};

const CLIENT: &str = "cvtestclient";

fn fixture_bytes(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/anidb")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("fixture {}: {e}", path.display()))
}

fn fixture(name: &str) -> String {
    String::from_utf8(fixture_bytes(name)).unwrap()
}

fn xml(name: &str) -> Reply {
    let mut reply = Reply::ok(&fixture(name));
    reply.headers = vec![("Content-Type".into(), "text/xml; charset=UTF-8".into())];
    reply
}

fn gz(name: &str) -> Reply {
    Reply {
        status: 200,
        headers: vec![("Content-Type".into(), "application/x-gzip".into())],
        body: fixture_bytes(name),
        delay: Duration::ZERO,
    }
}

struct Harness {
    server: MockServer,
    provider: AnidbProvider,
    resolver: CredentialResolver,
    http: HttpClient,
    cache_dir: tempfile::TempDir,
}

impl Harness {
    async fn new() -> Self {
        Self::build(Some(CLIENT)).await
    }

    async fn build(client: Option<&str>) -> Self {
        let server = MockServer::start().await;
        server.route_query("/httpapi", "aid=100", xml("anime_100.xml"));
        server.route_query("/httpapi", "aid=200", xml("anime_200.xml"));
        server.route_query("/httpapi", "aid=404", xml("error_not_found.xml"));
        server.route("/api/anime-titles.dat.gz", gz("anime-titles.dat.gz"));
        let cache_dir = tempfile::tempdir().unwrap();
        let http = HttpClient::builder()
            .cache_dir(cache_dir.path())
            // Tests do not wait two seconds between requests; the limit has its own test.
            .provider_limits("anidb", ProviderLimits::default())
            .build()
            .unwrap();
        let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
        if let Some(client) = client {
            resolver
                .set_user(catalogue::key::ANIDB, catalogue::API_KEY, client)
                .unwrap();
        }
        let provider = AnidbProvider::with_urls(
            http.clone(),
            resolver.clone(),
            &server.url("/httpapi"),
            &server.url("/api/anime-titles.dat.gz"),
        );
        Self {
            server,
            provider,
            resolver,
            http,
            cache_dir,
        }
    }

    fn cached_files(&self) -> usize {
        fn walk(dir: &Path) -> usize {
            std::fs::read_dir(dir).map_or(0, |entries| {
                entries
                    .flatten()
                    .map(|e| {
                        if e.path().is_dir() {
                            walk(&e.path())
                        } else {
                            1
                        }
                    })
                    .sum()
            })
        }
        walk(self.cache_dir.path())
    }

    fn api_requests(&self) -> usize {
        self.server.count("/httpapi")
    }
}

fn anidb_ids(id: &str) -> ProviderIds {
    let mut ids = ProviderIds::new();
    ids.set(id_key::ANIDB, id);
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

// --- identity and the lane --------------------------------------------------------------------

#[tokio::test]
async fn the_provider_offers_movies_and_shows_with_two_orderings() {
    let h = Harness::new().await;
    let capabilities = h.provider.capabilities();
    assert!(capabilities.movies && capabilities.shows && !capabilities.best_effort);
    assert_eq!(capabilities.lookup_ids, vec!["anidb"]);
    assert_eq!(capabilities.orderings.len(), 2);
    assert_eq!(h.provider.id(), "anidb");
    assert_eq!(h.provider.name(), "AniDB");
    assert!(h.provider.credentials().is_some());
}

#[tokio::test]
async fn new_limits_the_lane_to_one_request_every_two_seconds() {
    let http = HttpClient::builder().build().unwrap();
    let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
    let _provider = AnidbProvider::new(http.clone(), resolver);
    assert_eq!(http.lane_status("anidb").max_concurrent, 1);
    assert_eq!(anidb::default_limits().period, Duration::from_secs(2));
    assert_eq!(anidb::default_limits().requests, 1);
}

#[tokio::test]
async fn requests_are_spaced_two_seconds_apart_on_the_anidb_lane() {
    let h = Harness::new().await;
    // The real limit, on the lane that carries the API requests and the dump download alike.
    h.http.set_provider_limits("anidb", anidb::default_limits());
    let started = Instant::now();
    h.provider
        .details(MediaKind::Show, &anidb_ids("100"), &opts())
        .await
        .unwrap();
    h.provider
        .details(MediaKind::Movie, &anidb_ids("200"), &opts())
        .await
        .unwrap();
    assert!(
        started.elapsed() >= Duration::from_millis(1_900),
        "two requests took {:?}",
        started.elapsed()
    );
}

// --- client registration ----------------------------------------------------------------------

#[tokio::test]
async fn requests_carry_the_registered_client_and_version() {
    let h = Harness::new().await;
    h.resolver
        .set_user(catalogue::key::ANIDB, catalogue::ANIDB_CLIENT_VERSION, "3")
        .unwrap();
    h.provider
        .details(MediaKind::Show, &anidb_ids("100"), &opts())
        .await
        .unwrap();
    let request = &h.server.requests()[0];
    assert_eq!(request.method, "GET");
    assert_eq!(request.path(), "/httpapi");
    for expected in [
        "request=anime",
        "client=cvtestclient",
        "clientver=3",
        "protover=1",
        "aid=100",
    ] {
        assert!(
            request.target.contains(expected),
            "{expected} missing from {}",
            request.target
        );
    }
}

#[tokio::test]
async fn the_client_version_defaults_to_one() {
    let h = Harness::new().await;
    h.provider
        .details(MediaKind::Show, &anidb_ids("100"), &opts())
        .await
        .unwrap();
    assert!(h.server.requests()[0].target.contains("clientver=1"));
}

#[tokio::test]
async fn without_a_client_the_provider_is_inactive_and_sends_nothing() {
    let h = Harness::build(None).await;
    let status = h.resolver.status("anidb").unwrap();
    assert!(!status.active);
    assert_eq!(status.missing, ["api_key"]);
    let error = h
        .provider
        .details(MediaKind::Show, &anidb_ids("100"), &opts())
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        ProviderError::MissingCredentials { ref fields, .. } if fields == &["api_key".to_owned()]
    ));
    assert!(h.server.requests().is_empty());
}

// --- search over the title dump ---------------------------------------------------------------

#[tokio::test]
async fn search_matches_the_downloaded_title_dump_in_every_language() {
    let h = Harness::new().await;
    let hits = h
        .provider
        .search(&query(MediaKind::Show, "star voyage"), &opts())
        .await
        .unwrap();
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].provider, "anidb");
    assert_eq!(hits[0].ids.get("anidb"), Some("100"));
    assert_eq!(hits[0].title, "Star Voyage");
    assert_eq!(hits[0].original_title, "星の旅");
    assert!(!hits[0].exact_id);
    assert_eq!(hits[1].ids.get("anidb"), Some("101"));

    // A romaji title and a Japanese one find the same anime; a typo still matches.
    for text in ["Hoshi no Tabi", "星の旅", "Star Voyge"] {
        let found = h
            .provider
            .search(&query(MediaKind::Show, text), &opts())
            .await
            .unwrap();
        assert_eq!(found[0].ids.get("anidb"), Some("100"), "{text}");
    }
    // The same index serves films, and an anime without an English title shows its romaji one.
    let film = h
        .provider
        .search(&query(MediaKind::Movie, "Neko no Heya"), &opts())
        .await
        .unwrap();
    assert_eq!(film[0].title, "Neko no Heya");
    assert_eq!(film[0].original_title, "Neko no Heya");

    assert!(
        h.provider
            .search(&query(MediaKind::Show, "zzzzzz qqqqqq"), &opts())
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        h.provider
            .search(&query(MediaKind::Show, "  "), &opts())
            .await
            .unwrap()
            .is_empty()
    );
    // One download served every search: AniDB allows one a day.
    assert_eq!(h.server.count("/api/anime-titles.dat.gz"), 1);
}

#[tokio::test]
async fn a_dump_served_unpacked_is_read_too() {
    let h = Harness::new().await;
    let mut plain = Reply::ok(&fixture("anime-titles.dat"));
    plain.headers = vec![("Content-Type".into(), "text/plain".into())];
    h.server.route("/api/anime-titles.dat.gz", plain);
    let hits = h
        .provider
        .search(&query(MediaKind::Show, "Mirai no Eiga"), &opts())
        .await
        .unwrap();
    assert_eq!(hits[0].ids.get("anidb"), Some("200"));
}

#[tokio::test]
async fn a_blocked_dump_is_a_failed_request_and_is_not_cached() {
    let h = Harness::new().await;
    h.server.route(
        "/api/anime-titles.dat.gz",
        Reply::ok(&fixture("titles_blocked.html")),
    );
    let error = h
        .provider
        .search(&query(MediaKind::Show, "star"), &opts())
        .await
        .unwrap_err();
    assert!(matches!(error, ProviderError::Request { .. }), "{error:?}");
    assert_eq!(h.cached_files(), 0);

    // Once AniDB serves the dump again, the next search works.
    h.server
        .route("/api/anime-titles.dat.gz", gz("anime-titles.dat.gz"));
    assert!(
        !h.provider
            .search(&query(MediaKind::Show, "star"), &opts())
            .await
            .unwrap()
            .is_empty()
    );

    // A refused download is a failed request too.
    let refused = Harness::new().await;
    refused
        .server
        .route("/api/anime-titles.dat.gz", Reply::status(403));
    let error = refused
        .provider
        .search(&query(MediaKind::Show, "star"), &opts())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("403"), "{error}");
}

#[tokio::test]
async fn an_older_index_is_kept_when_a_refresh_fails() {
    let h = Harness::new().await;
    h.provider
        .search(&query(MediaKind::Show, "star"), &opts())
        .await
        .unwrap();
    h.server
        .route("/api/anime-titles.dat.gz", Reply::status(500));
    let mut forced = opts();
    forced.bypass_cache = true;
    let hits = h
        .provider
        .search(&query(MediaKind::Show, "star"), &forced)
        .await
        .unwrap();
    assert_eq!(hits[0].ids.get("anidb"), Some("100"));
    assert_eq!(h.server.count("/api/anime-titles.dat.gz"), 2);
}

// --- details ----------------------------------------------------------------------------------

#[tokio::test]
async fn an_anime_is_a_show_with_titles_ratings_ids_tags_and_cast() {
    let h = Harness::new().await;
    let item = h
        .provider
        .details(MediaKind::Show, &anidb_ids("100"), &opts())
        .await
        .unwrap();

    let core = &item.core;
    assert_eq!(core.title, "Star Voyage");
    assert_eq!(core.original_title, "星の旅");
    assert_eq!(core.year, 2001);
    assert_eq!(core.ids.get("anidb"), Some("100"));
    assert_eq!(core.ids.get("myanimelist"), Some("4242"));
    assert_eq!(core.ids.get(id_key::IMDB), Some("tt0111111"));
    assert_eq!(core.ids.get(id_key::TMDB), Some("5555"));
    assert_eq!(
        core.plot,
        "A crew travels between stars.\nCaptain Mika meets the stowaway.\nNote: Based on a novel."
    );
    assert_eq!(core.production_companies, ["Studio Beispiel"]);
    assert_eq!(
        core.artwork_urls
            .get(&MediaFileType::Poster)
            .map(String::as_str),
        Some("http://img7.anidb.net/pics/anime/100.jpg")
    );
    // The permanent rating wins over the temporary one.
    let rating = core.ratings.get(anidb::RATING_ID).expect("a rating");
    assert!((rating.value - 8.51).abs() < 1e-6);
    assert_eq!((rating.votes, rating.max_value), (1234, 10));

    let credits: Vec<_> = core
        .credits
        .iter()
        .map(|c| (c.kind, c.person.name.as_str(), c.role.as_str()))
        .collect();
    assert_eq!(
        credits,
        [
            (CreditKind::Director, "Taro Regie", "Direction"),
            (CreditKind::Writer, "Hanako Autorin", "Original Work"),
            (CreditKind::Composer, "Jiro Klang", "Music"),
            (CreditKind::Other, "Saburo Zeichner", "Character Design"),
            (CreditKind::Actor, "Yui Stimme", "Mika"),
            (CreditKind::Actor, "Ken Sprecher", "Stowaway"),
            (CreditKind::Actor, "Yui Stimme", "Narrator"),
        ]
    );
    let voice = &core.credits[4].person;
    assert_eq!(
        voice.thumb_url,
        "http://img7.anidb.net/pics/anime/seiyuu9.jpg"
    );
    assert_eq!(voice.ids.get("anidb"), Some("9"));

    let ItemDetail::Show(show) = &item.detail else {
        panic!("a show");
    };
    assert_eq!(show.english_title, "Star Voyage");
    assert_eq!(show.first_aired, CalendarDate::new(2001, 4, 2));
    assert_eq!(show.year_end, 2001);
    assert_eq!(show.status, AiredStatus::Ended);
    assert_eq!(show.runtime_minutes, 24);
    // Heaviest tags first; weak tags and spoilers are left out.
    assert_eq!(
        show.genres,
        [
            Genre::parse("Science Fiction").unwrap(),
            Genre::parse("Comedy").unwrap(),
            Genre::parse("Space Travel").unwrap(),
        ]
    );
    assert_eq!(
        show.episode_groups,
        [EpisodeGroup::aired(), EpisodeGroup::absolute()]
    );
}

#[tokio::test]
async fn titles_follow_the_requested_language() {
    let h = Harness::new().await;
    let mut german = opts();
    german.language = "de".to_owned();
    let item = h
        .provider
        .details(MediaKind::Show, &anidb_ids("100"), &german)
        .await
        .unwrap();
    assert_eq!(item.core.title, "Sternenreise");
    let hit = h
        .provider
        .lookup(MediaKind::Show, &anidb_ids("100"), &german)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(hit.title, "Sternenreise");
}

#[tokio::test]
async fn an_anime_film_is_a_movie_with_its_running_time() {
    let h = Harness::new().await;
    let item = h
        .provider
        .details(MediaKind::Movie, &anidb_ids("200"), &opts())
        .await
        .unwrap();
    assert_eq!(item.core.title, "Movie of the Future");
    assert_eq!(item.core.original_title, "Mirai no Eiga");
    assert_eq!(item.core.year, 2010);
    assert_eq!(item.core.plot, "A film & its sequel <3.");
    let rating = item.core.ratings.get("anidb").unwrap();
    assert_eq!(rating.votes, 40);
    let ItemDetail::Movie(movie) = &item.detail else {
        panic!("a movie");
    };
    assert_eq!(movie.runtime_minutes, 95);
    assert_eq!(movie.release_date, CalendarDate::new(2010, 7, 10));
    assert_eq!(movie.genres, [Genre::parse("Drama").unwrap()]);
}

#[tokio::test]
async fn lookup_returns_an_exact_hit_or_none() {
    let h = Harness::new().await;
    let hit = h
        .provider
        .lookup(MediaKind::Show, &anidb_ids("100"), &opts())
        .await
        .unwrap()
        .expect("a hit");
    assert!(hit.exact_id);
    assert_eq!(hit.year, 2001);
    assert_eq!(hit.original_title, "星の旅");
    assert_eq!(
        hit.poster_url.as_deref(),
        Some("http://img7.anidb.net/pics/anime/100.jpg")
    );

    assert_eq!(
        h.provider
            .lookup(MediaKind::Show, &anidb_ids("404"), &opts())
            .await
            .unwrap(),
        None
    );
    let before = h.api_requests();
    assert_eq!(
        h.provider
            .lookup(MediaKind::Show, &ProviderIds::new(), &opts())
            .await
            .unwrap(),
        None
    );
    assert_eq!(h.api_requests(), before);
}

// --- episodes ---------------------------------------------------------------------------------

fn numbered(items: &[cv_domain::Item], group: &EpisodeGroup) -> Vec<(u32, u32, String)> {
    items
        .iter()
        .map(|item| {
            let ItemDetail::Episode(episode) = &item.detail else {
                panic!("an episode");
            };
            let n = episode.numbers.get(group).expect("a number in the group");
            (n.season, n.episode, item.core.title.clone())
        })
        .collect()
}

#[tokio::test]
async fn episodes_are_numbered_aired_and_absolute_with_specials_in_season_zero() {
    let h = Harness::new().await;
    let items = h
        .provider
        .episodes(&anidb_ids("100"), &EpisodeGroup::aired(), &opts())
        .await
        .unwrap();
    // Credits (C1) are left out; the special comes first as season 0.
    assert_eq!(
        numbered(&items, &EpisodeGroup::aired()),
        [
            (0, 1, "Recap".to_owned()),
            (1, 1, "Departure".to_owned()),
            (1, 2, "Stowaway".to_owned()),
            // Only a romaji title: it is used when there is no English one.
            (1, 3, "Kikan".to_owned()),
            (1, 4, "Final Stop".to_owned()),
        ]
    );
    let first = &items[1];
    assert_eq!(first.core.plot, "Mika leaves.");
    assert_eq!(first.core.ids.get("anidb"), Some("1001"));
    let rating = first.core.ratings.get("anidb").unwrap();
    assert_eq!(rating.votes, 5);
    let ItemDetail::Episode(fields) = &first.detail else {
        panic!("an episode");
    };
    assert_eq!(fields.first_aired, CalendarDate::new(2001, 4, 2));
    assert_eq!(fields.runtime_minutes, 24);

    // The absolute order is AniDB's own order, without the special.
    let absolute = h
        .provider
        .episodes(&anidb_ids("100"), &EpisodeGroup::absolute(), &opts())
        .await
        .unwrap();
    assert_eq!(
        numbered(&absolute, &EpisodeGroup::absolute()),
        [
            (1, 1, "Departure".to_owned()),
            (1, 2, "Stowaway".to_owned()),
            (1, 3, "Kikan".to_owned()),
            (1, 4, "Final Stop".to_owned()),
        ]
    );
    // Both orderings come from one cached answer.
    assert_eq!(h.api_requests(), 1);
}

#[tokio::test]
async fn orderings_anidb_lacks_have_no_episodes() {
    let h = Harness::new().await;
    assert_eq!(
        h.provider
            .episodes(&anidb_ids("100"), &EpisodeGroup::dvd(), &opts())
            .await,
        Err(ProviderError::NotFound("AniDB".to_owned()))
    );
    assert_eq!(h.api_requests(), 0);
}

// --- error answers ----------------------------------------------------------------------------

#[tokio::test]
async fn an_unknown_anime_is_not_found_and_that_answer_may_be_cached() {
    let h = Harness::new().await;
    for _ in 0..2 {
        assert_eq!(
            h.provider
                .details(MediaKind::Show, &anidb_ids("404"), &opts())
                .await,
            Err(ProviderError::NotFound("AniDB".to_owned()))
        );
    }
    assert_eq!(h.api_requests(), 1);
}

#[tokio::test]
async fn a_banned_client_is_rate_limiting_and_is_never_cached() {
    let h = Harness::new().await;
    h.server
        .route_query("/httpapi", "aid=500", xml("error_banned.xml"));
    for _ in 0..2 {
        let error = h
            .provider
            .details(MediaKind::Show, &anidb_ids("500"), &opts())
            .await
            .unwrap_err();
        assert!(
            matches!(
                error.root(),
                ProviderError::RateLimited {
                    retry_after_secs: 86_400,
                    ..
                }
            ),
            "{error:?}"
        );
    }
    assert_eq!(h.api_requests(), 2);
    assert_eq!(h.cached_files(), 0);
}

#[tokio::test]
async fn a_banned_client_is_a_rate_limit_that_links_to_the_providers_card() {
    let h = Harness::new().await;
    h.server
        .route_query("/httpapi", "aid=500", xml("error_banned.xml"));
    let error = h
        .provider
        .details(MediaKind::Show, &anidb_ids("500"), &opts())
        .await
        .unwrap_err();
    assert!(matches!(error.root(), ProviderError::RateLimited { .. }));
    assert_eq!(error.key_problem_provider(), Some("anidb"));
    assert!(!error.to_string().contains(CLIENT));
}

#[tokio::test]
async fn an_unknown_client_is_an_authentication_error_without_the_name() {
    let h = Harness::new().await;
    h.server
        .route_query("/httpapi", "aid=302", xml("error_client.xml"));
    let error = h
        .provider
        .details(MediaKind::Show, &anidb_ids("302"), &opts())
        .await
        .unwrap_err();
    assert!(
        matches!(error.root(), ProviderError::Auth { .. }),
        "{error:?}"
    );
    assert!(!error.to_string().contains(CLIENT));
    assert_eq!(h.cached_files(), 0);
}

#[tokio::test]
async fn other_errors_and_broken_documents_are_failed_requests_and_not_cached() {
    let h = Harness::new().await;
    h.server
        .route_query("/httpapi", "aid=1", xml("error_other.xml"));
    h.server
        .route_query("/httpapi", "aid=2", xml("anime_truncated.xml"));
    h.server
        .route_query("/httpapi", "aid=3", Reply::ok("<html>maintenance</html>"));
    h.server
        .route_query("/httpapi", "aid=4", Reply::status(503));
    for aid in ["1", "2", "3", "4"] {
        let error = h
            .provider
            .details(MediaKind::Show, &anidb_ids(aid), &opts())
            .await
            .unwrap_err();
        assert!(
            matches!(error, ProviderError::Request { .. }),
            "{aid}: {error:?}"
        );
        assert!(!error.to_string().contains(CLIENT), "{aid}: {error}");
    }
    assert_eq!(h.cached_files(), 0);
}

#[tokio::test]
async fn an_answer_that_arrives_gzipped_without_saying_so_is_unpacked() {
    let h = Harness::new().await;
    h.server
        .route_query("/httpapi", "aid=100", gz("anime_100.xml.gz"));
    let item = h
        .provider
        .details(MediaKind::Show, &anidb_ids("100"), &opts())
        .await
        .unwrap();
    assert_eq!(item.core.title, "Star Voyage");
}

#[tokio::test]
async fn answers_are_cached_for_the_day_and_a_forced_rescrape_goes_to_the_network() {
    let h = Harness::new().await;
    for _ in 0..2 {
        h.provider
            .details(MediaKind::Show, &anidb_ids("100"), &opts())
            .await
            .unwrap();
    }
    assert_eq!(h.api_requests(), 1);
    let mut forced = opts();
    forced.bypass_cache = true;
    h.provider
        .details(MediaKind::Show, &anidb_ids("100"), &forced)
        .await
        .unwrap();
    assert_eq!(h.api_requests(), 2);
}

// --- testing the client -----------------------------------------------------------------------

#[tokio::test]
async fn test_credentials_asks_anidb_once_without_the_cache() {
    let h = Harness::new().await;
    h.server
        .route_query("/httpapi", "aid=1", xml("anime_100.xml"));
    let schema = h.provider.credentials().unwrap();
    let good =
        ResolvedCredentials::from_user_values(schema, [("api_key", "somenewclient")]).unwrap();
    h.provider.check_credentials(&good).await.unwrap();
    h.provider.check_credentials(&good).await.unwrap();
    assert_eq!(h.api_requests(), 2);
    assert!(
        h.server.requests()[0]
            .target
            .contains("client=somenewclient")
    );

    // "Anime not found" still means the client was accepted.
    let h = Harness::new().await;
    h.server
        .route_query("/httpapi", "aid=1", xml("error_not_found.xml"));
    h.provider.check_credentials(&good).await.unwrap();

    // A client AniDB does not know, a missing client and a ban are errors.
    let h = Harness::new().await;
    h.server
        .route_query("/httpapi", "aid=1", xml("error_client.xml"));
    assert!(matches!(
        h.provider
            .check_credentials(&good)
            .await
            .map_err(|e| e.root().clone()),
        Err(ProviderError::Auth { .. })
    ));
    assert!(matches!(
        h.provider
            .check_credentials(&ResolvedCredentials::default())
            .await,
        Err(ProviderError::MissingCredentials { .. })
    ));
    let h = Harness::new().await;
    h.server
        .route_query("/httpapi", "aid=1", xml("error_banned.xml"));
    assert!(matches!(
        h.provider
            .check_credentials(&good)
            .await
            .map_err(|e| e.root().clone()),
        Err(ProviderError::RateLimited { .. })
    ));
}
