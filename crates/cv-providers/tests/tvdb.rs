//! The TheTVDB provider against a local mock server that replays hand-built responses in
//! TheTVDB v4's documented shape (`tests/fixtures/tvdb/`). Nothing here touches the network;
//! the opt-in live smoke test is `tests/tvdb_live.rs`.

mod support;

use std::path::Path;
use std::sync::Arc;

use cv_domain::{
    AiredStatus, CalendarDate, CreditKind, EpisodeGroup, EpisodeOrdering, Genre, Item, ItemDetail,
    MediaFileType, ProviderIds, id_key,
};
use cv_providers::model::{MediaKind, RequestOptions, SearchQuery};
use cv_providers::tvdb::TvdbProvider;
use cv_providers::{
    ArtworkProvider, CredentialResolver, HttpClient, MemoryUserCredentials, MetadataProvider,
    Provider, ProviderError, ProviderRegistry, ResolvedCredentials, catalogue,
};
use support::{MockServer, Recorded, Reply};

const KEY: &str = "tvdb-test-key-0123456789";
const PIN: &str = "tvdb-pin-ABCDEF";
const TOKEN_ONE: &str = "jwt-token-one";
const TOKEN_TWO: &str = "jwt-token-two";

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/tvdb")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("fixture {}: {e}", path.display()))
}

fn ok(name: &str) -> Reply {
    Reply::ok(&fixture(name))
}

fn status(code: u16, name: &str) -> Reply {
    let mut reply = Reply::ok(&fixture(name));
    reply.status = code;
    reply
}

#[derive(Clone, Copy)]
struct Setup {
    cache: bool,
    key: Option<&'static str>,
    pin: Option<&'static str>,
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            cache: false,
            key: Some(KEY),
            pin: None,
        }
    }
}

struct Harness {
    server: MockServer,
    provider: TvdbProvider,
    resolver: CredentialResolver,
    cache_dir: Option<tempfile::TempDir>,
}

impl Harness {
    async fn new() -> Self {
        Self::build(Setup::default()).await
    }

    async fn build(setup: Setup) -> Self {
        let server = MockServer::start().await;
        server.route("/v4/login", ok("login.json"));
        server.route("/v4/artwork/types", ok("artwork_types.json"));
        let cache_dir = setup.cache.then(|| tempfile::tempdir().unwrap());
        let mut builder = HttpClient::builder();
        if let Some(dir) = &cache_dir {
            builder = builder.cache_dir(dir.path());
        }
        let http = builder.build().unwrap();
        let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
        if let Some(key) = setup.key {
            resolver
                .set_user(catalogue::key::TVDB, catalogue::API_KEY, key)
                .unwrap();
        }
        if let Some(pin) = setup.pin {
            resolver
                .set_user(catalogue::key::TVDB, catalogue::TVDB_PIN, pin)
                .unwrap();
        }
        let provider = TvdbProvider::with_base_url(http, resolver.clone(), &server.url("/v4"));
        Self {
            server,
            provider,
            resolver,
            cache_dir,
        }
    }

    /// Requests that were not the sign-in or the one-time artwork type list.
    fn calls(&self) -> Vec<Recorded> {
        self.server
            .requests()
            .into_iter()
            .filter(|r| r.path() != "/v4/login" && r.path() != "/v4/artwork/types")
            .collect()
    }

    fn logins(&self) -> Vec<Recorded> {
        self.server
            .requests()
            .into_iter()
            .filter(|r| r.path() == "/v4/login")
            .collect()
    }

    fn route_show(&self) {
        self.server.route(
            "/v4/series/81189/extended",
            ok("series_81189_extended.json"),
        );
    }

    fn route_official_pages(&self) {
        self.server.script(
            "/v4/series/81189/episodes/official/eng",
            vec![
                ok("episodes_official_eng_p0.json"),
                ok("episodes_official_eng_p1.json"),
            ],
        );
    }
}

fn opts() -> RequestOptions {
    RequestOptions::default()
}

fn turkish() -> RequestOptions {
    RequestOptions {
        language: "tr".into(),
        fallback_language: "en".into(),
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

fn show_ids() -> ProviderIds {
    ids(&[("tvdb", "81189")])
}

fn query(kind: MediaKind, title: &str, year: Option<u16>) -> SearchQuery {
    SearchQuery {
        kind,
        title: title.to_owned(),
        year,
    }
}

fn login_body(request: &Recorded) -> serde_json::Value {
    serde_json::from_slice(&request.body).expect("login body is JSON")
}

fn episode(item: &Item) -> &cv_domain::EpisodeFields {
    let ItemDetail::Episode(fields) = &item.detail else {
        panic!("not an episode")
    };
    fields
}

fn number(item: &Item, group: &EpisodeGroup) -> Option<(u32, u32)> {
    episode(item)
        .numbers
        .get(group)
        .map(|n| (n.season, n.episode))
}

// --- sign-in ---------------------------------------------------------------------------------

#[tokio::test]
async fn it_signs_in_once_and_sends_the_token_as_a_bearer_token() {
    let h = Harness::new().await;
    h.route_show();
    h.server.route("/v4/search", ok("search_series.json"));

    h.provider
        .details(MediaKind::Show, &show_ids(), &opts())
        .await
        .unwrap();
    h.provider
        .search(&query(MediaKind::Show, "Breaking Bad", None), &opts())
        .await
        .unwrap();

    let logins = h.logins();
    assert_eq!(logins.len(), 1, "one sign-in serves every request");
    assert_eq!(logins[0].method, "POST");
    let body = login_body(&logins[0]);
    assert_eq!(body["apikey"], KEY);
    assert!(body.get("pin").is_none(), "{body}");
    for call in h.calls() {
        assert_eq!(
            call.headers.get("authorization").map(String::as_str),
            Some(format!("Bearer {TOKEN_ONE}").as_str()),
            "{}",
            call.target
        );
    }
}

#[tokio::test]
async fn the_subscriber_pin_goes_with_the_users_own_key() {
    let h = Harness::build(Setup {
        pin: Some(PIN),
        ..Setup::default()
    })
    .await;
    h.server.route("/v4/search", ok("search_empty.json"));
    h.provider
        .search(&query(MediaKind::Show, "Nothing", None), &opts())
        .await
        .unwrap();
    let body = login_body(&h.logins()[0]);
    assert_eq!(body["apikey"], KEY);
    assert_eq!(body["pin"], PIN);
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
        .search(&query(MediaKind::Show, "Breaking Bad", None), &opts())
        .await
        .unwrap_err();
    match err {
        ProviderError::MissingCredentials {
            provider, fields, ..
        } => {
            assert_eq!(provider, "TheTVDB");
            assert_eq!(fields, vec!["api_key".to_owned()]);
        }
        other => panic!("{other:?}"),
    }
    assert!(h.server.requests().is_empty());
}

#[tokio::test]
async fn a_rejected_key_is_an_auth_error_without_the_key_in_the_message() {
    let h = Harness::new().await;
    h.server.route("/v4/login", status(401, "login_401.json"));
    let err = h
        .provider
        .search(&query(MediaKind::Show, "Breaking Bad", None), &opts())
        .await
        .unwrap_err();
    assert!(matches!(err.root(), ProviderError::Auth { .. }), "{err:?}");
    assert!(!err.to_string().contains(KEY), "{err}");
    assert!(h.calls().is_empty());
}

#[tokio::test]
async fn a_rejected_pin_is_named_in_the_message() {
    let h = Harness::build(Setup {
        pin: Some(PIN),
        ..Setup::default()
    })
    .await;
    h.server.route("/v4/login", status(401, "login_401.json"));
    let err = h
        .provider
        .search(&query(MediaKind::Show, "Breaking Bad", None), &opts())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("subscriber PIN"), "{err}");
    assert!(!err.to_string().contains(PIN), "{err}");
}

#[tokio::test]
async fn a_rejected_key_links_to_the_card() {
    let h = Harness::new().await;
    h.server.route("/v4/login", status(401, "login_401.json"));
    let err = h
        .provider
        .search(&query(MediaKind::Show, "Breaking Bad", None), &opts())
        .await
        .unwrap_err();
    assert_eq!(err.key_problem_provider(), Some("tvdb"), "{err:?}");
    assert!(matches!(err.root(), ProviderError::Auth { .. }));
    let text = err.to_string();
    assert!(text.contains("rejected your key"), "{text}");
    assert!(!text.contains(KEY), "{text}");
}

#[tokio::test]
async fn an_expired_token_is_replaced_once() {
    let h = Harness::new().await;
    h.server
        .script("/v4/login", vec![ok("login.json"), ok("login_second.json")]);
    h.server.script(
        "/v4/search",
        vec![status(401, "error_401.json"), ok("search_series.json")],
    );

    let hits = h
        .provider
        .search(&query(MediaKind::Show, "Breaking Bad", None), &opts())
        .await
        .unwrap();
    assert!(!hits.is_empty());
    assert_eq!(h.logins().len(), 2);
    let searches: Vec<Recorded> = h.calls();
    assert_eq!(searches.len(), 2);
    assert_eq!(
        searches[0].headers.get("authorization").map(String::as_str),
        Some(format!("Bearer {TOKEN_ONE}").as_str())
    );
    assert_eq!(
        searches[1].headers.get("authorization").map(String::as_str),
        Some(format!("Bearer {TOKEN_TWO}").as_str())
    );
}

#[tokio::test]
async fn a_token_that_keeps_failing_is_an_auth_error() {
    let h = Harness::new().await;
    h.server.route("/v4/search", status(401, "error_401.json"));
    let err = h
        .provider
        .search(&query(MediaKind::Show, "Breaking Bad", None), &opts())
        .await
        .unwrap_err();
    assert!(matches!(err.root(), ProviderError::Auth { .. }), "{err:?}");
    assert_eq!(h.logins().len(), 2, "one retry only");
}

#[tokio::test]
async fn changing_the_key_signs_in_again() {
    let h = Harness::new().await;
    h.server.route("/v4/search", ok("search_empty.json"));
    h.provider
        .search(&query(MediaKind::Show, "One", None), &opts())
        .await
        .unwrap();
    h.resolver
        .set_user(catalogue::key::TVDB, catalogue::API_KEY, "another-key-5555")
        .unwrap();
    h.provider
        .search(&query(MediaKind::Show, "Two", None), &opts())
        .await
        .unwrap();
    let logins = h.logins();
    assert_eq!(logins.len(), 2);
    assert_eq!(login_body(&logins[1])["apikey"], "another-key-5555");
}

#[tokio::test]
async fn the_settings_test_signs_in_with_the_typed_credentials() {
    let h = Harness::build(Setup {
        key: None,
        ..Setup::default()
    })
    .await;
    let schema = h.provider.credentials().expect("schema");
    let typed = ResolvedCredentials::from_user_values(
        schema,
        [("api_key", " typed-key-77 "), ("pin", "typed-pin-1")],
    )
    .unwrap();
    h.provider.check_credentials(&typed).await.unwrap();
    let body = login_body(&h.logins()[0]);
    assert_eq!(body["apikey"], "typed-key-77");
    assert_eq!(body["pin"], "typed-pin-1");
    assert!(
        h.calls().is_empty(),
        "the test needs nothing but the sign-in"
    );

    h.server.route("/v4/login", status(401, "login_401.json"));
    let err = h.provider.check_credentials(&typed).await.unwrap_err();
    assert!(matches!(err.root(), ProviderError::Auth { .. }), "{err:?}");
}

#[tokio::test]
async fn the_provider_is_reachable_through_the_registry() {
    let h = Harness::new().await;
    let mut registry = ProviderRegistry::new();
    registry.register(Arc::new(h.provider.clone()));
    assert!(registry.get("tvdb").is_some());
    let credentials = h.resolver.resolve("tvdb").unwrap();
    registry
        .check_credentials("tvdb", "TheTVDB", &credentials)
        .await
        .unwrap();
}

#[tokio::test]
async fn the_provider_describes_itself() {
    let h = Harness::new().await;
    assert_eq!(h.provider.id(), "tvdb");
    assert_eq!(h.provider.name(), "TheTVDB");
    let schema = h.provider.credentials().expect("schema");
    assert!(schema.field("api_key").is_some_and(|f| f.required));
    assert!(schema.field("pin").is_some_and(|f| !f.required));

    let capabilities = h.provider.capabilities();
    assert!(capabilities.movies && capabilities.shows && !capabilities.best_effort);
    for ordering in [
        EpisodeOrdering::Aired,
        EpisodeOrdering::Dvd,
        EpisodeOrdering::Absolute,
        EpisodeOrdering::Alternate,
    ] {
        assert!(capabilities.orderings.contains(&ordering), "{ordering:?}");
    }
    assert_eq!(capabilities.lookup_ids, vec!["tvdb", "imdb"]);
}

#[tokio::test]
async fn server_errors_are_request_errors() {
    let h = Harness::new().await;
    h.server.route("/v4/search", status(500, "error_500.json"));
    let err = h
        .provider
        .search(&query(MediaKind::Show, "Breaking Bad", None), &opts())
        .await
        .unwrap_err();
    assert!(
        matches!(&err, ProviderError::Request { message, .. } if message.contains("500") && message.contains("Server exploded")),
        "{err:?}"
    );
}

// --- search ----------------------------------------------------------------------------------

#[tokio::test]
async fn a_text_search_reads_series_hits() {
    let h = Harness::new().await;
    h.server.route("/v4/search", ok("search_series.json"));

    let hits = h
        .provider
        .search(&query(MediaKind::Show, "Breaking Bad", Some(2008)), &opts())
        .await
        .unwrap();
    // The duplicate, the row without an ID and the row without a name are dropped.
    assert_eq!(hits.len(), 2, "{hits:#?}");
    let hit = &hits[0];
    assert_eq!(hit.provider, "tvdb");
    assert_eq!(hit.title, "Breaking Bad");
    assert_eq!(hit.original_title, "Breaking Bad");
    assert_eq!(hit.year, 2008);
    assert!(hit.overview.starts_with("When Walter White"));
    assert_eq!(hit.ids.get(id_key::TVDB), Some("81189"));
    assert_eq!(hit.ids.get(id_key::IMDB), Some("tt0903747"));
    assert_eq!(hit.ids.get(id_key::TMDB), Some("1396"));
    assert!(!hit.exact_id);
    assert_eq!(
        hit.poster_url.as_deref(),
        Some("https://artworks.thetvdb.com/banners/posters/81189-10.jpg")
    );
    // A hit with only a first-air date still has its year.
    assert_eq!(hits[1].year, 2009);

    let calls = h.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(param(&calls[0], "query").as_deref(), Some("Breaking Bad"));
    assert_eq!(param(&calls[0], "type").as_deref(), Some("series"));
    assert_eq!(param(&calls[0], "year").as_deref(), Some("2008"));
    assert!(
        !calls[0].target.contains(KEY) && !calls[0].target.contains(TOKEN_ONE),
        "credentials stay out of the URL: {}",
        calls[0].target
    );
}

#[tokio::test]
async fn titles_and_plots_follow_the_language() {
    let h = Harness::new().await;
    h.server.route("/v4/search", ok("search_series.json"));
    let hits = h
        .provider
        .search(&query(MediaKind::Show, "Breaking Bad", None), &turkish())
        .await
        .unwrap();
    assert_eq!(hits[0].title, "Breaking Bad: K\u{131}r\u{131}lma");
    assert!(hits[0].overview.starts_with("Kimya"));
    assert_eq!(hits[0].original_title, "Breaking Bad");
    // No translation at all: the record's own name.
    assert_eq!(hits[1].title, "Breaking Bad: Minisodes");
}

#[tokio::test]
async fn a_movie_search_asks_for_movies() {
    let h = Harness::new().await;
    h.server.route("/v4/search", ok("search_movie.json"));
    let hits = h
        .provider
        .search(&query(MediaKind::Movie, "Heat", Some(1995)), &opts())
        .await
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].title, "Heat");
    assert_eq!(hits[0].year, 1995);
    assert_eq!(hits[0].ids.get(id_key::TVDB), Some("1229"));
    assert_eq!(h.calls()[0].path(), "/v4/search");
    assert_eq!(param(&h.calls()[0], "type").as_deref(), Some("movie"));
}

#[tokio::test]
async fn a_year_in_parentheses_is_taken_off_the_title() {
    let h = Harness::new().await;
    h.server.route("/v4/search", ok("search_series.json"));
    h.provider
        .search(
            &query(MediaKind::Show, "Breaking Bad (2008)", None),
            &opts(),
        )
        .await
        .unwrap();
    let call = &h.calls()[0];
    assert_eq!(param(call, "query").as_deref(), Some("Breaking Bad"));
    assert_eq!(param(call, "year").as_deref(), Some("2008"));

    // An explicit year wins over the one in the title.
    let h = Harness::new().await;
    h.server.route("/v4/search", ok("search_series.json"));
    h.provider
        .search(
            &query(MediaKind::Show, "Breaking Bad (2008)", Some(2009)),
            &opts(),
        )
        .await
        .unwrap();
    assert_eq!(param(&h.calls()[0], "year").as_deref(), Some("2009"));
}

#[tokio::test]
async fn a_search_that_finds_nothing_is_repeated_without_the_year() {
    let h = Harness::new().await;
    h.server.script(
        "/v4/search",
        vec![ok("search_empty.json"), ok("search_series.json")],
    );
    let hits = h
        .provider
        .search(&query(MediaKind::Show, "Breaking Bad", Some(1999)), &opts())
        .await
        .unwrap();
    assert_eq!(hits.len(), 2);
    let calls = h.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(param(&calls[0], "year").as_deref(), Some("1999"));
    assert_eq!(param(&calls[1], "year"), None);
}

#[tokio::test]
async fn an_empty_title_searches_nothing() {
    let h = Harness::new().await;
    let hits = h
        .provider
        .search(&query(MediaKind::Show, "  ", None), &opts())
        .await
        .unwrap();
    assert!(hits.is_empty());
    assert!(h.server.requests().is_empty());
}

#[tokio::test]
async fn a_tvdb_id_resolves_the_title_without_a_text_search() {
    let h = Harness::new().await;
    h.route_show();
    let hit = h
        .provider
        .lookup(MediaKind::Show, &show_ids(), &opts())
        .await
        .unwrap()
        .expect("Breaking Bad");
    assert_eq!(hit.title, "Breaking Bad");
    assert_eq!(hit.year, 2008);
    assert!(hit.exact_id);
    assert_eq!(hit.ids.get(id_key::IMDB), Some("tt0903747"));
    assert!(h.calls().iter().all(|c| c.path() != "/v4/search"));

    // search() with the ID settles on that one hit, too.
    let hits = h
        .provider
        .search_with_ids(
            &query(MediaKind::Show, "anything", None),
            &show_ids(),
            &opts(),
        )
        .await
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert!(hits[0].exact_id);
}

#[tokio::test]
async fn an_unknown_tvdb_id_is_no_match() {
    let h = Harness::new().await;
    let hit = h
        .provider
        .lookup(MediaKind::Show, &ids(&[("tvdb", "404404")]), &opts())
        .await
        .unwrap();
    assert!(hit.is_none());
}

#[tokio::test]
async fn an_imdb_id_goes_through_the_remote_id_search() {
    let h = Harness::new().await;
    h.server
        .route("/v4/search/remoteid/tt0903747", ok("remoteid_series.json"));
    h.route_show();
    let hit = h
        .provider
        .lookup(MediaKind::Show, &ids(&[("imdb", "tt0903747")]), &opts())
        .await
        .unwrap()
        .expect("Breaking Bad");
    assert_eq!(hit.ids.get(id_key::TVDB), Some("81189"));
    assert!(hit.exact_id);

    // A movie asks for the movie side of the answer.
    h.server
        .route("/v4/search/remoteid/tt0113277", ok("remoteid_movie.json"));
    h.server
        .route("/v4/movies/1229/extended", ok("movie_1229_extended.json"));
    let movie = h
        .provider
        .lookup(MediaKind::Movie, &ids(&[("imdb", "tt0113277")]), &opts())
        .await
        .unwrap()
        .expect("Heat");
    assert_eq!(movie.title, "Heat");
    assert_eq!(movie.year, 1995);
    assert_eq!(movie.ids.get(id_key::TVDB), Some("1229"));
}

#[tokio::test]
async fn an_imdb_id_with_no_match_of_the_right_kind_is_no_match() {
    let h = Harness::new().await;
    h.server
        .route("/v4/search/remoteid/tt0903747", ok("remoteid_series.json"));
    let hit = h
        .provider
        .lookup(MediaKind::Movie, &ids(&[("imdb", "tt0903747")]), &opts())
        .await
        .unwrap();
    assert!(hit.is_none());

    h.server
        .route("/v4/search/remoteid/tt0000000", ok("remoteid_empty.json"));
    let hit = h
        .provider
        .lookup(MediaKind::Show, &ids(&[("imdb", "tt0000000")]), &opts())
        .await
        .unwrap();
    assert!(hit.is_none());

    // An ID that cannot stand in a URL path is never sent.
    let before = h.server.requests().len();
    let hit = h
        .provider
        .lookup(MediaKind::Show, &ids(&[("imdb", "../login")]), &opts())
        .await
        .unwrap();
    assert!(hit.is_none());
    assert_eq!(h.server.requests().len(), before);
}

// --- details ---------------------------------------------------------------------------------

#[tokio::test]
async fn show_details_cover_ids_people_artwork_and_orderings() {
    let h = Harness::new().await;
    h.route_show();
    let item = h
        .provider
        .details(MediaKind::Show, &show_ids(), &opts())
        .await
        .unwrap();

    assert_eq!(item.core.title, "Breaking Bad");
    assert_eq!(item.core.original_title, "Breaking Bad");
    // TVDB says `eng`; CineVault stores the two-letter code.
    assert_eq!(item.core.original_language, "en");
    assert_eq!(item.core.year, 2008);
    assert!(item.core.plot.starts_with("When Walter White"));
    assert_eq!(item.core.ids.get(id_key::TVDB), Some("81189"));
    assert_eq!(item.core.ids.get(id_key::IMDB), Some("tt0903747"));
    assert_eq!(item.core.ids.get(id_key::TMDB), Some("1396"));
    assert_eq!(item.core.ids.get(id_key::WIKIDATA), Some("Q1079"));
    assert_eq!(item.core.ids.get(id_key::TVMAZE), Some("169"));
    assert_eq!(
        item.core.production_companies,
        vec!["AMC".to_owned(), "Sony Pictures Television".to_owned()]
    );

    let ItemDetail::Show(show) = &item.detail else {
        panic!("not a show")
    };
    assert_eq!(show.english_title, "Breaking Bad");
    assert_eq!(show.first_aired, CalendarDate::new(2008, 1, 20));
    assert_eq!(show.status, AiredStatus::Ended);
    assert_eq!(show.year_end, 2013);
    assert_eq!(show.runtime_minutes, 45);
    assert_eq!(show.country, "US");
    assert_eq!(show.certification.0, "US_TV_MA");
    assert_eq!(
        show.genres,
        vec![
            Genre::parse("Drama").unwrap(),
            Genre::parse("Crime").unwrap(),
            Genre::Other("Cosmic Horror".into())
        ]
    );
    assert_eq!(show.episode_group, EpisodeGroup::aired());
    assert_eq!(
        show.episode_groups,
        vec![
            EpisodeGroup::aired(),
            EpisodeGroup::dvd(),
            EpisodeGroup::absolute(),
            EpisodeGroup::new(EpisodeOrdering::Alternate, "Regional Order"),
        ]
    );

    // Cast in billing order, then crew; a repeated person and unnamed rows are dropped.
    let credits: Vec<(CreditKind, &str, &str)> = item
        .core
        .credits
        .iter()
        .map(|c| (c.kind, c.person.name.as_str(), c.role.as_str()))
        .collect();
    assert_eq!(
        credits,
        vec![
            (CreditKind::Actor, "Bryan Cranston", "Walter White"),
            (CreditKind::Actor, "Aaron Paul", "Jesse Pinkman"),
            (CreditKind::Director, "Vince Gilligan", "Director"),
            (CreditKind::Writer, "Vince Gilligan", "Writer"),
            (CreditKind::Producer, "Mark Johnson", "Executive Producer"),
        ]
    );
    let cranston = &item.core.credits[0].person;
    assert_eq!(cranston.ids.get(id_key::TVDB), Some("10"));
    assert_eq!(
        cranston.thumb_url,
        "https://artworks.thetvdb.com/banners/actors/1.jpg"
    );

    // Artwork URLs: the best of each type for English.
    let url = |kind| item.core.artwork_urls.get(&kind).map(String::as_str);
    assert_eq!(
        url(MediaFileType::Poster),
        Some("https://artworks.thetvdb.com/banners/posters/81189-eng-a.jpg")
    );
    assert_eq!(
        url(MediaFileType::Fanart),
        Some("https://artworks.thetvdb.com/banners/backgrounds/81189-eng.jpg")
    );
    assert_eq!(
        url(MediaFileType::Banner),
        Some("https://artworks.thetvdb.com/banners/graphical/81189-g.jpg")
    );
    assert_eq!(
        url(MediaFileType::Clearlogo),
        Some("https://artworks.thetvdb.com/banners/clearlogo/81189.png")
    );
    assert_eq!(
        url(MediaFileType::Clearart),
        Some("https://artworks.thetvdb.com/banners/clearart/81189.png")
    );
    assert_eq!(
        url(MediaFileType::Squareart),
        Some("https://artworks.thetvdb.com/banners/icons/81189.jpg")
    );
    assert_eq!(
        show.extra_fanart_urls,
        vec!["https://artworks.thetvdb.com/banners/backgrounds/81189-none.jpg".to_owned()]
    );

    // The request asked for translations.
    let call = &h.calls()[0];
    assert_eq!(call.path(), "/v4/series/81189/extended");
    assert_eq!(param(call, "meta").as_deref(), Some("translations"));
}

#[tokio::test]
async fn show_details_use_the_preferred_language_and_its_artwork() {
    let h = Harness::new().await;
    h.route_show();
    let item = h
        .provider
        .details(MediaKind::Show, &show_ids(), &turkish())
        .await
        .unwrap();
    assert_eq!(item.core.title, "Breaking Bad: K\u{131}r\u{131}lma");
    assert!(item.core.plot.starts_with("Kimya"));
    let ItemDetail::Show(show) = &item.detail else {
        panic!("not a show")
    };
    // The English title is kept for the NFO's alternate title.
    assert_eq!(show.english_title, "Breaking Bad");
    assert_eq!(
        item.core.artwork_urls[&MediaFileType::Poster],
        "https://artworks.thetvdb.com/banners/posters/81189-tur.jpg"
    );
}

#[tokio::test]
async fn a_title_falls_back_to_the_fallback_language_then_english_then_the_record() {
    let h = Harness::new().await;
    h.route_show();
    // German is not translated; the fallback is English.
    let options = RequestOptions {
        language: "de".into(),
        fallback_language: "en".into(),
        ..RequestOptions::default()
    };
    let item = h
        .provider
        .details(MediaKind::Show, &show_ids(), &options)
        .await
        .unwrap();
    assert_eq!(item.core.title, "Breaking Bad");

    // Neither language is translated: Turkish (the second choice here) is used.
    let options = RequestOptions {
        language: "de".into(),
        fallback_language: "tr".into(),
        ..RequestOptions::default()
    };
    let item = h
        .provider
        .details(MediaKind::Show, &show_ids(), &options)
        .await
        .unwrap();
    assert_eq!(item.core.title, "Breaking Bad: K\u{131}r\u{131}lma");

    // A record with no translations at all keeps its own name and plot.
    h.server.route(
        "/v4/series/55555/extended",
        ok("series_55555_extended.json"),
    );
    let item = h
        .provider
        .details(MediaKind::Show, &ids(&[("tvdb", "55555")]), &opts())
        .await
        .unwrap();
    assert_eq!(item.core.title, "Orijinal Dizi");
    assert_eq!(item.core.plot, "Orijinal \u{f6}zet.");
    assert_eq!(item.core.year, 2019);
    let ItemDetail::Show(show) = &item.detail else {
        panic!("not a show")
    };
    assert_eq!(show.status, AiredStatus::Continuing);
    assert_eq!(show.country, "TR");
    assert_eq!(show.episode_groups, vec![EpisodeGroup::aired()]);
    assert!(item.core.credits.is_empty());
}

#[tokio::test]
async fn movie_details_cover_release_certification_and_companies() {
    let h = Harness::new().await;
    h.server
        .route("/v4/movies/1229/extended", ok("movie_1229_extended.json"));
    let item = h
        .provider
        .details(MediaKind::Movie, &ids(&[("tvdb", "1229")]), &opts())
        .await
        .unwrap();

    assert_eq!(item.core.title, "Heat");
    assert_eq!(item.core.original_language, "en");
    assert_eq!(item.core.year, 1995);
    assert!(item.core.plot.starts_with("A group of professional"));
    assert_eq!(item.core.ids.get(id_key::TVDB), Some("1229"));
    assert_eq!(item.core.ids.get(id_key::IMDB), Some("tt0113277"));
    assert_eq!(item.core.ids.get(id_key::TMDB), Some("949"));
    // Studios and production companies, not distributors or effects houses.
    assert_eq!(
        item.core.production_companies,
        vec!["Warner Bros.".to_owned(), "Regency Enterprises".to_owned()]
    );
    let ItemDetail::Movie(movie) = &item.detail else {
        panic!("not a movie")
    };
    assert_eq!(movie.runtime_minutes, 170);
    assert_eq!(movie.release_date, CalendarDate::new(1995, 12, 15));
    assert_eq!(movie.certification.0, "US_R");
    assert_eq!(movie.countries, vec!["US".to_owned()]);
    assert_eq!(
        movie.spoken_languages,
        vec!["en".to_owned(), "es".to_owned()]
    );
    assert_eq!(
        movie.genres,
        vec![
            Genre::parse("Crime").unwrap(),
            Genre::parse("Thriller").unwrap()
        ]
    );
    assert_eq!(movie.english_title, "Heat");
    assert_eq!(movie.trailers.len(), 1);
    assert_eq!(item.core.credits.len(), 3);
    assert_eq!(item.core.credits[2].kind, CreditKind::Director);
    assert_eq!(
        item.core.artwork_urls[&MediaFileType::Poster],
        "https://artworks.thetvdb.com/banners/movies/1229/posters/heat-eng.jpg"
    );
    assert_eq!(
        item.core.artwork_urls[&MediaFileType::Fanart],
        "https://artworks.thetvdb.com/banners/movies/1229/backgrounds/heat.jpg"
    );
    assert_eq!(h.calls()[0].path(), "/v4/movies/1229/extended");
}

#[tokio::test]
async fn movie_release_and_certification_follow_the_country() {
    let h = Harness::new().await;
    h.server
        .route("/v4/movies/1229/extended", ok("movie_1229_extended.json"));
    let options = RequestOptions {
        country: "DE".into(),
        language: "tr".into(),
        ..RequestOptions::default()
    };
    let item = h
        .provider
        .details(MediaKind::Movie, &ids(&[("tvdb", "1229")]), &options)
        .await
        .unwrap();
    let ItemDetail::Movie(movie) = &item.detail else {
        panic!("not a movie")
    };
    assert_eq!(movie.release_date, CalendarDate::new(1996, 2, 22));
    assert_eq!(movie.certification.0, "DE_FSK16");
    assert_eq!(item.core.title, "Hararet");
    assert_eq!(item.core.year, 1996);
}

#[tokio::test]
async fn details_find_the_title_by_imdb_id_when_there_is_no_tvdb_id() {
    let h = Harness::new().await;
    h.server
        .route("/v4/search/remoteid/tt0903747", ok("remoteid_series.json"));
    h.route_show();
    let item = h
        .provider
        .details(MediaKind::Show, &ids(&[("imdb", "tt0903747")]), &opts())
        .await
        .unwrap();
    assert_eq!(item.core.ids.get(id_key::TVDB), Some("81189"));

    let err = h
        .provider
        .details(MediaKind::Show, &ProviderIds::new(), &opts())
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::NotFound(_)), "{err:?}");
}

#[tokio::test]
async fn details_still_work_when_the_artwork_types_cannot_be_read() {
    let h = Harness::new().await;
    h.route_show();
    h.server.route("/v4/artwork/types", Reply::status(500));
    let item = h
        .provider
        .details(MediaKind::Show, &show_ids(), &opts())
        .await
        .unwrap();
    // Only the record's main image is left.
    assert_eq!(
        item.core.artwork_urls[&MediaFileType::Poster],
        "https://artworks.thetvdb.com/banners/posters/81189-main.jpg"
    );
    assert_eq!(item.core.artwork_urls.len(), 1);

    // Asking for artwork itself reports the failure.
    let err = h
        .provider
        .artwork(MediaKind::Show, &show_ids(), &opts())
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::Request { .. }), "{err:?}");
}

#[tokio::test]
async fn repeated_details_come_from_the_cache() {
    let h = Harness::build(Setup {
        cache: true,
        ..Setup::default()
    })
    .await;
    h.route_show();
    for _ in 0..2 {
        h.provider
            .details(MediaKind::Show, &show_ids(), &opts())
            .await
            .unwrap();
    }
    assert_eq!(h.server.count("/v4/series/81189/extended"), 1);
    assert_eq!(h.logins().len(), 1);

    // A forced rescrape goes to the network again.
    let forced = RequestOptions {
        bypass_cache: true,
        ..RequestOptions::default()
    };
    h.provider
        .details(MediaKind::Show, &show_ids(), &forced)
        .await
        .unwrap();
    assert_eq!(h.server.count("/v4/series/81189/extended"), 2);
}

#[tokio::test]
async fn no_credential_reaches_the_cache_files() {
    let h = Harness::build(Setup {
        cache: true,
        pin: Some(PIN),
        ..Setup::default()
    })
    .await;
    h.route_show();
    h.provider
        .details(MediaKind::Show, &show_ids(), &opts())
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
        let bytes = std::fs::read(&file).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        for secret in [KEY, PIN, TOKEN_ONE] {
            assert!(
                !text.contains(secret),
                "{} contains a credential",
                file.display()
            );
        }
        let name = file.to_string_lossy();
        assert!(!name.contains(KEY) && !name.contains(TOKEN_ONE));
    }
}

// --- episodes in every ordering --------------------------------------------------------------

#[tokio::test]
async fn the_aired_list_reads_every_page_and_numbers_the_episodes() {
    let h = Harness::new().await;
    h.route_official_pages();

    let episodes = h
        .provider
        .episodes(&show_ids(), &EpisodeGroup::aired(), &opts())
        .await
        .unwrap();
    let aired = EpisodeGroup::aired();
    let numbers: Vec<(u32, u32)> = episodes
        .iter()
        .map(|e| number(e, &aired).unwrap())
        .collect();
    assert_eq!(numbers, vec![(0, 1), (1, 1), (1, 2), (1, 3)]);

    let pilot = &episodes[1];
    assert_eq!(pilot.core.title, "Pilot");
    assert_eq!(pilot.core.plot, "Walter White receives a diagnosis.");
    assert_eq!(pilot.core.ids.get(id_key::TVDB), Some("349232"));
    assert_eq!(pilot.core.year, 2008);
    assert_eq!(episode(pilot).first_aired, CalendarDate::new(2008, 1, 20));
    assert_eq!(episode(pilot).runtime_minutes, 58);
    assert_eq!(
        pilot.core.artwork_urls[&MediaFileType::Thumb],
        "https://artworks.thetvdb.com/banners/episodes/81189/349232.jpg"
    );
    assert_eq!(episode(&episodes[0]).runtime_minutes, 3);

    let calls = h.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].path(), "/v4/series/81189/episodes/official/eng");
    assert_eq!(param(&calls[0], "page").as_deref(), Some("0"));
    assert_eq!(param(&calls[1], "page").as_deref(), Some("1"));
}

#[tokio::test]
async fn episodes_missing_a_name_or_plot_are_filled_from_the_fallback_language() {
    let h = Harness::new().await;
    h.server.route(
        "/v4/series/81189/episodes/official/tur",
        ok("episodes_official_tur.json"),
    );
    h.route_official_pages();

    let episodes = h
        .provider
        .episodes(&show_ids(), &EpisodeGroup::aired(), &turkish())
        .await
        .unwrap();
    let titles: Vec<&str> = episodes.iter().map(|e| e.core.title.as_str()).collect();
    assert_eq!(
        titles,
        vec![
            "Good Cop Bad Cop",
            "Pilot (TR)",
            "Cat's in the Bag...",
            "...Ve \u{c7}anta Nehirde"
        ]
    );
    // The Turkish plot stays where there is one; the rest comes from English.
    assert_eq!(
        episodes[1].core.plot,
        "Walter White te\u{15f}his al\u{131}r."
    );
    assert_eq!(episodes[2].core.plot, "Walt and Jesse clean up.");
    assert_eq!(episodes[3].core.plot, "Walt decides Krazy-8's fate.");
}

#[tokio::test]
async fn a_complete_preferred_language_needs_no_fallback_request() {
    let h = Harness::new().await;
    h.route_official_pages();
    h.provider
        .episodes(&show_ids(), &EpisodeGroup::aired(), &opts())
        .await
        .unwrap();
    let paths: Vec<String> = h.calls().iter().map(|c| c.path().to_owned()).collect();
    assert!(
        paths.iter().all(|p| p.ends_with("/official/eng")),
        "{paths:?}"
    );
}

#[tokio::test]
async fn a_show_with_only_the_default_order_still_has_an_aired_list() {
    let h = Harness::new().await;
    h.server.route(
        "/v4/series/81189/episodes/default/eng",
        ok("episodes_official_plain.json"),
    );
    let episodes = h
        .provider
        .episodes(&show_ids(), &EpisodeGroup::aired(), &opts())
        .await
        .unwrap();
    assert_eq!(episodes.len(), 4);
}

#[tokio::test]
async fn the_dvd_list_keeps_the_aired_numbers() {
    let h = Harness::new().await;
    h.server.route(
        "/v4/series/81189/episodes/dvd/eng",
        ok("episodes_dvd_eng.json"),
    );
    h.server.route(
        "/v4/series/81189/episodes/official",
        ok("episodes_official_plain.json"),
    );

    let episodes = h
        .provider
        .episodes(&show_ids(), &EpisodeGroup::dvd(), &opts())
        .await
        .unwrap();
    assert_eq!(episodes.len(), 3);
    let aired = EpisodeGroup::aired();
    let dvd = EpisodeGroup::dvd();
    // Sorted by DVD number: the special, then "Cat's in the Bag..." as 1x01 (aired 1x02).
    assert_eq!(episodes[0].core.title, "Good Cop Bad Cop");
    assert_eq!(number(&episodes[0], &dvd), Some((0, 1)));
    assert_eq!(number(&episodes[0], &aired), Some((0, 1)));
    assert_eq!(episodes[1].core.title, "Cat's in the Bag...");
    assert_eq!(number(&episodes[1], &dvd), Some((1, 1)));
    assert_eq!(number(&episodes[1], &aired), Some((1, 2)));
    assert_eq!(episodes[2].core.title, "Pilot");
    assert_eq!(number(&episodes[2], &dvd), Some((1, 2)));
    assert_eq!(number(&episodes[2], &aired), Some((1, 1)));

    // A stored DVD ordering that carries a name finds the same list.
    let named = EpisodeGroup::new(EpisodeOrdering::Dvd, "DVD Order");
    let again = h
        .provider
        .episodes(&show_ids(), &named, &opts())
        .await
        .unwrap();
    assert_eq!(again.len(), 3);
    // The numbers are stored under the show's DVD ordering.
    assert_eq!(number(&again[0], &dvd), Some((0, 1)));
}

#[tokio::test]
async fn the_absolute_list_numbers_episodes_across_the_show() {
    let h = Harness::new().await;
    h.server.route(
        "/v4/series/81189/episodes/absolute/eng",
        ok("episodes_absolute_eng.json"),
    );
    h.server.route(
        "/v4/series/81189/episodes/official",
        ok("episodes_official_plain.json"),
    );
    let episodes = h
        .provider
        .episodes(&show_ids(), &EpisodeGroup::absolute(), &opts())
        .await
        .unwrap();
    let absolute = EpisodeGroup::absolute();
    let aired = EpisodeGroup::aired();
    assert_eq!(episodes.len(), 3);
    assert_eq!(number(&episodes[2], &absolute), Some((1, 3)));
    assert_eq!(number(&episodes[2], &aired), Some((1, 3)));
    assert_eq!(number(&episodes[1], &absolute), Some((1, 2)));
}

#[tokio::test]
async fn an_absolute_list_is_built_from_the_aired_list_when_the_show_has_no_such_order() {
    let h = Harness::new().await;
    // No `absolute` route: TheTVDB answers 404 for it.
    h.server.route(
        "/v4/series/81189/episodes/official",
        ok("episodes_official_plain.json"),
    );
    let episodes = h
        .provider
        .episodes(&show_ids(), &EpisodeGroup::absolute(), &opts())
        .await
        .unwrap();
    let absolute = EpisodeGroup::absolute();
    let aired = EpisodeGroup::aired();
    // The special has no absolute number and is left out.
    assert_eq!(episodes.len(), 3);
    let both: Vec<_> = episodes
        .iter()
        .map(|e| (number(e, &absolute).unwrap(), number(e, &aired).unwrap()))
        .collect();
    assert_eq!(
        both,
        vec![((1, 1), (1, 1)), ((1, 2), (1, 2)), ((1, 3), (1, 3))]
    );
}

#[tokio::test]
async fn alternate_orderings_are_requested_by_their_season_type() {
    let h = Harness::new().await;
    h.server.route(
        "/v4/series/81189/episodes/regional/eng",
        ok("episodes_regional_eng.json"),
    );
    h.server.route(
        "/v4/series/81189/episodes/official",
        ok("episodes_official_plain.json"),
    );
    let regional = EpisodeGroup::new(EpisodeOrdering::Alternate, "Regional Order");
    let episodes = h
        .provider
        .episodes(&show_ids(), &regional, &opts())
        .await
        .unwrap();
    assert_eq!(episodes.len(), 2);
    assert_eq!(number(&episodes[0], &regional), Some((1, 1)));
    assert_eq!(number(&episodes[0], &EpisodeGroup::aired()), Some((1, 1)));
    // An episode that exists only in this order has no aired number.
    assert_eq!(episodes[1].core.title, "Regional only");
    assert_eq!(number(&episodes[1], &regional), Some((1, 9)));
    assert_eq!(number(&episodes[1], &EpisodeGroup::aired()), None);
}

#[tokio::test]
async fn an_ordering_the_show_lacks_is_not_found() {
    let h = Harness::new().await;
    h.server.route(
        "/v4/series/81189/episodes/official",
        ok("episodes_official_plain.json"),
    );
    for group in [
        EpisodeGroup::new(EpisodeOrdering::Alternate, "Alternate DVD Order"),
        EpisodeGroup::dvd(),
    ] {
        let err = h
            .provider
            .episodes(&show_ids(), &group, &opts())
            .await
            .unwrap_err();
        assert!(
            matches!(err, ProviderError::NotFound(_)),
            "{group:?}: {err:?}"
        );
    }
}

#[tokio::test]
async fn an_ordering_name_the_provider_does_not_have_sends_nothing() {
    let h = Harness::new().await;
    let story_arcs = EpisodeGroup::new(EpisodeOrdering::Alternate, "Story Arcs");
    let err = h
        .provider
        .episodes(&show_ids(), &story_arcs, &opts())
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::NotFound(_)), "{err:?}");
    assert!(h.server.requests().is_empty());
}

// --- seasons ---------------------------------------------------------------------------------

#[tokio::test]
async fn a_season_carries_its_name_plot_artwork_and_episodes() {
    let h = Harness::new().await;
    h.route_show();
    h.server
        .route("/v4/seasons/3572/extended", ok("season_3572_extended.json"));
    h.route_official_pages();

    let season = h
        .provider
        .season_details(&show_ids(), 1, &opts())
        .await
        .unwrap();
    assert_eq!(season.season.core.title, "Season One");
    assert_eq!(season.season.core.plot, "Walt starts cooking.");
    assert_eq!(season.season.core.year, 2008);
    assert_eq!(season.season.core.ids.get(id_key::TVDB), Some("3572"));
    assert!(matches!(
        &season.season.detail,
        ItemDetail::Season(s) if s.season == 1
    ));
    assert_eq!(
        season.season.core.artwork_urls[&MediaFileType::SeasonPoster],
        "https://artworks.thetvdb.com/banners/seasons/3572-eng.jpg"
    );
    assert_eq!(
        season.season.core.artwork_urls[&MediaFileType::SeasonFanart],
        "https://artworks.thetvdb.com/banners/seasons/3572-bg.jpg"
    );
    // The season's episodes only, in order.
    let numbers: Vec<_> = season
        .episodes
        .iter()
        .map(|e| number(e, &EpisodeGroup::aired()).unwrap())
        .collect();
    assert_eq!(numbers, vec![(1, 1), (1, 2), (1, 3)]);

    let record = h
        .provider
        .season_artwork(&show_ids(), 1, &opts())
        .await
        .unwrap();
    let kinds: Vec<MediaFileType> = record.iter().map(|c| c.kind).collect();
    assert_eq!(
        kinds,
        vec![
            MediaFileType::SeasonPoster,
            MediaFileType::SeasonFanart,
            MediaFileType::SeasonBanner
        ]
    );
    // A banner without a size takes the artwork type's size.
    assert_eq!((record[2].width, record[2].height), (758, 140));
    // The artwork registry hands out trait objects: the same call is on the trait.
    let as_trait: &dyn cv_providers::ArtworkProvider = &h.provider;
    let via_trait = as_trait
        .season_artwork(&show_ids(), 1, &opts())
        .await
        .unwrap();
    assert_eq!(via_trait, record);
}

#[tokio::test]
async fn a_season_the_show_lacks_is_not_found() {
    let h = Harness::new().await;
    h.route_show();
    let err = h
        .provider
        .season_details(&show_ids(), 9, &opts())
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::NotFound(_)), "{err:?}");
}

// --- artwork and trailers --------------------------------------------------------------------

#[tokio::test]
async fn artwork_is_grouped_by_type_and_ordered_by_language_then_score() {
    let h = Harness::new().await;
    h.route_show();
    let artwork = h
        .provider
        .artwork(MediaKind::Show, &show_ids(), &opts())
        .await
        .unwrap();

    let summary: Vec<(MediaFileType, &str, Option<&str>)> = artwork
        .iter()
        .map(|a| {
            (
                a.kind,
                a.url.rsplit('/').next().unwrap(),
                a.language.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        vec![
            (MediaFileType::Poster, "81189-eng-a.jpg", Some("en")),
            (MediaFileType::Poster, "81189-eng-b.jpg", Some("en")),
            (MediaFileType::Poster, "81189-none.jpg", None),
            (MediaFileType::Poster, "81189-fra.jpg", Some("fr")),
            (MediaFileType::Poster, "81189-tur.jpg", Some("tr")),
            (MediaFileType::Fanart, "81189-eng.jpg", Some("en")),
            (MediaFileType::Fanart, "81189-none.jpg", None),
            (MediaFileType::Banner, "81189-g.jpg", Some("en")),
            (MediaFileType::Clearart, "81189.png", Some("en")),
            (MediaFileType::Clearlogo, "81189.png", Some("en")),
            (MediaFileType::Squareart, "81189.jpg", None),
        ],
        "cinemagraphs and unknown types are not offered"
    );
    let first = &artwork[0];
    assert_eq!((first.width, first.height), (680, 1000));
    assert!((first.score - 500.0).abs() < f32::EPSILON);
    // No size on the artwork itself: the artwork type's size.
    let textless = &artwork[2];
    assert_eq!((textless.width, textless.height), (680, 1000));
    // A relative path becomes an absolute URL.
    assert_eq!(
        artwork[7].url,
        "https://artworks.thetvdb.com/banners/graphical/81189-g.jpg"
    );
}

#[tokio::test]
async fn artwork_puts_the_preferred_language_first() {
    let h = Harness::new().await;
    h.route_show();
    let artwork = h
        .provider
        .artwork(MediaKind::Show, &show_ids(), &turkish())
        .await
        .unwrap();
    let posters: Vec<&str> = artwork
        .iter()
        .filter(|a| a.kind == MediaFileType::Poster)
        .map(|a| a.url.rsplit('/').next().unwrap())
        .collect();
    assert_eq!(
        posters,
        vec![
            "81189-tur.jpg",
            "81189-eng-a.jpg",
            "81189-eng-b.jpg",
            "81189-none.jpg",
            "81189-fra.jpg"
        ]
    );
}

#[tokio::test]
async fn movie_artwork_uses_the_movie_types() {
    let h = Harness::new().await;
    h.server
        .route("/v4/movies/1229/extended", ok("movie_1229_extended.json"));
    let artwork = h
        .provider
        .artwork(MediaKind::Movie, &ids(&[("tvdb", "1229")]), &opts())
        .await
        .unwrap();
    let kinds: Vec<MediaFileType> = artwork.iter().map(|a| a.kind).collect();
    assert_eq!(
        kinds,
        vec![
            MediaFileType::Poster,
            MediaFileType::Poster,
            MediaFileType::Fanart,
            MediaFileType::Clearlogo
        ]
    );
    assert!(artwork[0].url.ends_with("heat-eng.jpg"));
}

#[tokio::test]
async fn the_artwork_types_are_read_once() {
    let h = Harness::new().await;
    h.route_show();
    h.server
        .route("/v4/movies/1229/extended", ok("movie_1229_extended.json"));
    h.provider
        .artwork(MediaKind::Show, &show_ids(), &opts())
        .await
        .unwrap();
    h.provider
        .artwork(MediaKind::Movie, &ids(&[("tvdb", "1229")]), &opts())
        .await
        .unwrap();
    assert_eq!(h.server.count("/v4/artwork/types"), 1);
}

#[tokio::test]
async fn show_trailers_have_an_address_are_unique_and_lead_with_the_preferred_language() {
    let h = Harness::new().await;
    h.route_show();
    let show_trailers = |item: cv_domain::Item| {
        let ItemDetail::Show(show) = item.detail else {
            panic!("not a show")
        };
        show.trailers
    };
    let trailers = show_trailers(
        h.provider
            .details(MediaKind::Show, &show_ids(), &opts())
            .await
            .unwrap(),
    );
    let summary: Vec<(&str, &str, &str)> = trailers
        .iter()
        .map(|t| (t.id.as_str(), t.language.as_str(), t.url.as_str()))
        .collect();
    assert_eq!(
        summary,
        vec![
            ("321", "en", "https://www.youtube.com/watch?v=HhesaQXLuRY"),
            ("326", "", "https://vimeo.com/123456"),
            ("322", "tr", "https://www.youtube.com/watch?v=TRKEY12"),
        ]
    );
    assert_eq!(trailers[0].name, "Breaking Bad Official Trailer");
    assert_eq!(trailers[0].provider, "tvdb");
    assert_eq!(trailers[0].scraped_by, "tvdb");

    let turkish_first = show_trailers(
        h.provider
            .details(MediaKind::Show, &show_ids(), &turkish())
            .await
            .unwrap(),
    );
    assert_eq!(turkish_first[0].id, "322");
}

#[tokio::test]
async fn movie_trailers_come_from_the_movie_record() {
    let h = Harness::new().await;
    h.server
        .route("/v4/movies/1229/extended", ok("movie_1229_extended.json"));
    let item = h
        .provider
        .details(MediaKind::Movie, &ids(&[("tvdb", "1229")]), &opts())
        .await
        .unwrap();
    let ItemDetail::Movie(movie) = item.detail else {
        panic!("not a movie")
    };
    assert_eq!(movie.trailers.len(), 1);
    assert_eq!(
        movie.trailers[0].url,
        "https://www.youtube.com/watch?v=2GfZl4kuVNI"
    );
}
