//! The TVmaze provider against a local mock server that replays hand-built responses in
//! TVmaze's documented shape (`tests/fixtures/tvmaze/`). Nothing here touches the network; the
//! opt-in live smoke test is `tests/tvmaze_live.rs`.

mod support;

use std::path::Path;
use std::sync::Arc;

use cv_domain::{
    AiredStatus, CalendarDate, CreditKind, EpisodeGroup, EpisodeOrdering, Genre, Item, ItemDetail,
    MediaFileType, ProviderIds,
};
use cv_providers::model::{MediaKind, RequestOptions, SearchQuery};
use cv_providers::tvmaze::{RATING_ID, TvmazeProvider};
use cv_providers::{HttpClient, MetadataProvider, Provider, ProviderError, ProviderRegistry};
use support::{MockServer, Recorded, Reply};

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/tvmaze")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("fixture {}: {e}", path.display()))
}

fn ok(name: &str) -> Reply {
    Reply::ok(&fixture(name))
}

fn redirect_to(location: &str) -> Reply {
    let mut reply = Reply::status(301);
    reply.headers.push(("Location".into(), location.into()));
    reply
}

struct Harness {
    server: MockServer,
    provider: TvmazeProvider,
    cache_dir: Option<tempfile::TempDir>,
}

impl Harness {
    async fn new() -> Self {
        Self::build(false).await
    }

    async fn build(cache: bool) -> Self {
        let server = MockServer::start().await;
        server.route_query("/shows/169", "embed", ok("show_embedded.json"));
        server.route("/shows/169", ok("show_plain.json"));
        server.route("/shows/7", ok("show_bare.json"));
        server.route("/search/shows", ok("search_shows.json"));
        server.route("/shows/169/episodes", ok("episodes_aired.json"));
        server.route("/shows/169/alternatelists", ok("alternatelists.json"));
        server.route(
            "/alternatelists/31/alternateepisodes",
            ok("alternateepisodes_dvd.json"),
        );
        server.route(
            "/alternatelists/32/alternateepisodes",
            ok("alternateepisodes_streaming.json"),
        );
        server.route("/shows/7/alternatelists", ok("alternatelists_none.json"));
        server.route_query("/lookup/shows", "imdb=tt0903747", redirect_to("/shows/169"));
        server.route_query("/lookup/shows", "thetvdb=81189", redirect_to("/shows/169"));
        let cache_dir = cache.then(|| tempfile::tempdir().unwrap());
        let mut builder = HttpClient::builder();
        if let Some(dir) = &cache_dir {
            builder = builder.cache_dir(dir.path());
        }
        let provider = TvmazeProvider::with_base_url(builder.build().unwrap(), &server.url(""));
        Self {
            server,
            provider,
            cache_dir,
        }
    }

    fn paths(&self) -> Vec<String> {
        self.server
            .requests()
            .iter()
            .map(|r| r.path().to_owned())
            .collect()
    }
}

fn opts() -> RequestOptions {
    RequestOptions::default()
}

fn ids(pairs: &[(&str, &str)]) -> ProviderIds {
    pairs.iter().copied().collect()
}

fn show_ids() -> ProviderIds {
    ids(&[("tvmaze", "169")])
}

fn query(kind: MediaKind, title: &str) -> SearchQuery {
    SearchQuery {
        kind,
        title: title.to_owned(),
        year: None,
    }
}

fn params(request: &Recorded, name: &str) -> Vec<String> {
    let url = reqwest::Url::parse(&format!("http://mock{}", request.target)).unwrap();
    url.query_pairs()
        .filter(|(k, _)| k == name)
        .map(|(_, v)| v.into_owned())
        .collect()
}

fn number(item: &Item, group: &EpisodeGroup) -> Option<(u32, u32)> {
    let ItemDetail::Episode(fields) = &item.detail else {
        panic!("not an episode")
    };
    fields.numbers.get(group).map(|n| (n.season, n.episode))
}

fn dvd_group() -> EpisodeGroup {
    EpisodeGroup::dvd()
}

fn streaming_group() -> EpisodeGroup {
    EpisodeGroup::new(EpisodeOrdering::Alternate, "Streaming order")
}

// --- the provider ----------------------------------------------------------------------------

#[tokio::test]
async fn the_provider_describes_itself() {
    let h = Harness::new().await;
    assert_eq!(h.provider.id(), "tvmaze");
    assert_eq!(h.provider.name(), "TVmaze");
    assert!(h.provider.credentials().is_none(), "no key needed");

    let capabilities = h.provider.capabilities();
    assert!(!capabilities.movies && capabilities.shows && !capabilities.best_effort);
    assert_eq!(
        capabilities.orderings,
        vec![
            EpisodeOrdering::Aired,
            EpisodeOrdering::Dvd,
            EpisodeOrdering::Alternate
        ]
    );
    assert_eq!(capabilities.lookup_ids, vec!["tvmaze", "imdb", "tvdb"]);
}

#[tokio::test]
async fn the_provider_is_reachable_through_the_registry_but_has_no_credentials_to_test() {
    let h = Harness::new().await;
    let mut registry = ProviderRegistry::new();
    registry.register(Arc::new(h.provider.clone()));
    assert!(registry.get("tvmaze").is_some());
    let err = registry
        .check_credentials("tvmaze", "TVmaze", &Default::default())
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::Unsupported { .. }), "{err:?}");
}

#[tokio::test]
async fn new_sets_the_documented_lane_limits() {
    let http = HttpClient::builder().build().unwrap();
    let _provider = TvmazeProvider::new(http.clone());
    assert_eq!(http.lane_status("tvmaze").max_concurrent, 2);
}

#[tokio::test]
async fn a_rate_limited_request_is_retried_and_other_failures_are_request_errors() {
    let h = Harness::new().await;
    h.server.script(
        "/search/shows",
        vec![Reply::rate_limited(Some("0")), ok("search_shows.json")],
    );
    let hits = h
        .provider
        .search(&query(MediaKind::Show, "Breaking Bad"), &opts())
        .await
        .unwrap();
    assert_eq!(hits.len(), 2);
    assert_eq!(h.server.count("/search/shows"), 2);

    h.server.route("/shows/500", Reply::status(503));
    let err = h
        .provider
        .details(MediaKind::Show, &ids(&[("tvmaze", "500")]), &opts())
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::Request { .. }), "{err:?}");

    h.server.route("/shows/501", Reply::ok("<html>oops</html>"));
    let err = h
        .provider
        .details(MediaKind::Show, &ids(&[("tvmaze", "501")]), &opts())
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
        .search(&query(MediaKind::Show, "Breaking Bad"), &opts())
        .await
        .unwrap();
    // The entry without an ID is left out.
    assert_eq!(hits.len(), 2);
    let bb = &hits[0];
    assert_eq!(bb.provider, "tvmaze");
    assert_eq!(bb.title, "Breaking Bad");
    assert_eq!(bb.year, 2008);
    assert_eq!(bb.ids.get("tvmaze"), Some("169"));
    assert_eq!(bb.ids.get("imdb"), Some("tt0903747"));
    assert_eq!(bb.ids.get("tvdb"), Some("81189"));
    assert!(bb.overview.starts_with("Breaking Bad follows"));
    assert_eq!(
        bb.poster_url.as_deref(),
        Some("https://img.example.test/tvmaze/bb_original.jpg")
    );
    assert!(!bb.exact_id);
    assert_eq!(hits[1].ids.get("imdb"), None);
    assert_eq!(
        params(&h.server.requests()[0], "q"),
        vec!["Breaking Bad".to_owned()]
    );
}

#[tokio::test]
async fn movies_and_blank_titles_search_nothing() {
    let h = Harness::new().await;
    for q in [
        query(MediaKind::Movie, "Heat"),
        query(MediaKind::Show, "   "),
    ] {
        assert!(h.provider.search(&q, &opts()).await.unwrap().is_empty());
    }
    assert!(
        h.provider
            .lookup(MediaKind::Movie, &show_ids(), &opts())
            .await
            .unwrap()
            .is_none()
    );
    let err = h
        .provider
        .details(MediaKind::Movie, &show_ids(), &opts())
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::Unsupported { .. }), "{err:?}");
    assert!(h.server.requests().is_empty());
}

#[tokio::test]
async fn a_tvmaze_id_is_looked_up_directly() {
    let h = Harness::new().await;
    let hit = h
        .provider
        .lookup(MediaKind::Show, &show_ids(), &opts())
        .await
        .unwrap()
        .expect("Breaking Bad");
    assert!(hit.exact_id);
    assert_eq!(hit.ids.get("tvdb"), Some("81189"));
    assert_eq!(h.paths(), ["/shows/169"]);
}

#[tokio::test]
async fn imdb_and_tvdb_ids_follow_the_lookup_redirect() {
    let h = Harness::new().await;
    let hit = h
        .provider
        .lookup(MediaKind::Show, &ids(&[("imdb", "tt0903747")]), &opts())
        .await
        .unwrap()
        .expect("found by IMDb ID");
    assert_eq!(hit.ids.get("tvmaze"), Some("169"));
    assert_eq!(h.paths(), ["/lookup/shows", "/shows/169"]);
    assert_eq!(
        params(&h.server.requests()[0], "imdb"),
        vec!["tt0903747".to_owned()]
    );

    let h = Harness::new().await;
    let hit = h
        .provider
        .lookup(MediaKind::Show, &ids(&[("tvdb", "81189")]), &opts())
        .await
        .unwrap()
        .expect("found by TVDB ID");
    assert_eq!(hit.title, "Breaking Bad");
    assert_eq!(
        params(&h.server.requests()[0], "thetvdb"),
        vec!["81189".to_owned()]
    );
}

#[tokio::test]
async fn ids_tvmaze_does_not_know_are_none() {
    let h = Harness::new().await;
    for pairs in [
        vec![("tvmaze", "999")],
        vec![("imdb", "tt7654321")],
        vec![("tvdb", "1")],
        vec![("tmdb", "1396")],
        vec![],
    ] {
        let found = h
            .provider
            .lookup(MediaKind::Show, &ids(&pairs), &opts())
            .await
            .unwrap();
        assert!(found.is_none(), "{pairs:?}");
    }
    let imdb_lookups = h
        .server
        .requests()
        .iter()
        .filter(|r| params(r, "imdb") == ["tt7654321"])
        .count();
    assert_eq!(imdb_lookups, 1);
}

// --- details ---------------------------------------------------------------------------------

#[tokio::test]
async fn show_details_embed_people_images_and_seasons() {
    let h = Harness::new().await;
    let item = h
        .provider
        .details(MediaKind::Show, &show_ids(), &opts())
        .await
        .unwrap();
    let request = h
        .server
        .requests()
        .into_iter()
        .find(|r| r.path() == "/shows/169")
        .unwrap();
    assert_eq!(
        params(&request, "embed[]"),
        vec!["cast", "crew", "images", "seasons"]
    );

    assert_eq!(item.core.title, "Breaking Bad");
    assert_eq!(item.core.original_title, "Breaking Bad");
    assert_eq!(item.core.year, 2008);
    assert_eq!(item.core.ids.get("tvmaze"), Some("169"));
    assert_eq!(item.core.ids.get("imdb"), Some("tt0903747"));
    assert_eq!(item.core.ids.get("tvdb"), Some("81189"));
    assert_eq!(
        item.core.plot,
        "Breaking Bad follows protagonist Walter White & his partner.\n\nA second paragraph."
    );
    assert_eq!(item.core.production_companies, vec!["AMC"]);
    let rating = item.core.ratings.get(RATING_ID).unwrap();
    assert_eq!((rating.value, rating.max_value), (9.3, 10));

    let ItemDetail::Show(show) = &item.detail else {
        panic!("show");
    };
    assert_eq!(show.english_title, "Breaking Bad");
    assert_eq!(show.runtime_minutes, 60);
    assert_eq!(show.first_aired, CalendarDate::new(2008, 1, 20));
    assert_eq!(show.year_end, 2013);
    assert_eq!(show.status, AiredStatus::Ended);
    assert_eq!(show.country, "US");
    assert_eq!(
        show.genres,
        vec![
            Genre::parse("Drama").unwrap(),
            Genre::parse("Crime").unwrap(),
            Genre::parse("Thriller").unwrap()
        ]
    );
    assert_eq!(show.episode_group, EpisodeGroup::aired());
    assert_eq!(
        show.episode_groups,
        vec![EpisodeGroup::aired(), dvd_group(), streaming_group()]
    );
    // Season 3 has neither a name nor a plot, so it has no text.
    let seasons: Vec<_> = show
        .season_texts
        .iter()
        .map(|s| (s.season, s.name.as_str(), s.overview.as_str()))
        .collect();
    assert_eq!(seasons, vec![(1, "", "Season one."), (2, "The Fly", "")]);
}

#[tokio::test]
async fn show_details_carry_people_and_artwork() {
    let h = Harness::new().await;
    let item = h
        .provider
        .details(MediaKind::Show, &show_ids(), &opts())
        .await
        .unwrap();
    let credits: Vec<_> = item
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
            (CreditKind::Other, "Vince Gilligan", "Creator"),
            (CreditKind::Producer, "Vince Gilligan", "Executive Producer"),
            (CreditKind::Composer, "Dave Porter", "Composer"),
        ]
    );
    let cranston = &item.core.credits[0].person;
    assert_eq!(cranston.ids.get("tvmaze"), Some("1"));
    assert_eq!(
        cranston.thumb_url,
        "https://img.example.test/tvmaze/p1_o.jpg"
    );
    assert_eq!(item.core.credits[1].person.thumb_url, "");

    let art = |kind| item.core.artwork_urls.get(&kind).map(String::as_str);
    assert_eq!(
        art(MediaFileType::Poster),
        Some("https://img.example.test/tvmaze/poster_main.jpg")
    );
    assert_eq!(
        art(MediaFileType::Fanart),
        Some("https://img.example.test/tvmaze/bg_main.jpg")
    );
    assert_eq!(
        art(MediaFileType::Banner),
        Some("https://img.example.test/tvmaze/banner.jpg")
    );
    assert_eq!(
        art(MediaFileType::Clearlogo),
        Some("https://img.example.test/tvmaze/logo.png")
    );
    let ItemDetail::Show(show) = &item.detail else {
        panic!("show");
    };
    assert_eq!(
        show.extra_fanart_urls,
        vec!["https://img.example.test/tvmaze/bg_extra.jpg"]
    );
}

#[tokio::test]
async fn a_show_with_nothing_but_a_name_has_empty_fields() {
    let h = Harness::new().await;
    h.server.route("/shows/7", ok("show_bare.json"));
    let item = h
        .provider
        .details(MediaKind::Show, &ids(&[("tvmaze", "7")]), &opts())
        .await
        .unwrap();
    assert_eq!(item.core.title, "Bare Show");
    assert_eq!(item.core.year, 0);
    assert!(item.core.plot.is_empty() && item.core.credits.is_empty());
    assert!(item.core.artwork_urls.is_empty());
    let ItemDetail::Show(show) = &item.detail else {
        panic!("show");
    };
    assert_eq!(show.status, AiredStatus::Unknown);
    assert_eq!(show.episode_groups, vec![EpisodeGroup::aired()]);
    assert_eq!(show.english_title, "");
}

#[tokio::test]
async fn details_resolve_an_imdb_id_first_and_unknown_shows_are_not_found() {
    let h = Harness::new().await;
    let item = h
        .provider
        .details(MediaKind::Show, &ids(&[("imdb", "tt0903747")]), &opts())
        .await
        .unwrap();
    assert_eq!(item.core.title, "Breaking Bad");
    assert_eq!(
        h.paths(),
        [
            "/lookup/shows",
            "/shows/169",
            "/shows/169",
            "/shows/169/alternatelists"
        ]
    );

    let err = h
        .provider
        .details(MediaKind::Show, &ids(&[("imdb", "tt7654321")]), &opts())
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::NotFound(_)), "{err:?}");
    let err = h
        .provider
        .details(MediaKind::Show, &ids(&[("tvmaze", "404")]), &opts())
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::NotFound(_)), "{err:?}");
}

#[tokio::test]
async fn repeated_details_come_from_the_cache_and_a_forced_rescrape_does_not() {
    let h = Harness::build(true).await;
    for _ in 0..2 {
        h.provider
            .details(MediaKind::Show, &show_ids(), &opts())
            .await
            .unwrap();
    }
    assert_eq!(h.server.count("/shows/169"), 1);
    assert_eq!(h.server.count("/shows/169/alternatelists"), 1);
    let forced = RequestOptions {
        bypass_cache: true,
        ..opts()
    };
    h.provider
        .details(MediaKind::Show, &show_ids(), &forced)
        .await
        .unwrap();
    assert_eq!(h.server.count("/shows/169"), 2);
    assert!(h.cache_dir.is_some());
}

// --- episodes in every ordering --------------------------------------------------------------

#[tokio::test]
async fn the_aired_list_includes_specials_in_season_zero() {
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
    assert_eq!(items[0].core.title, "Season 1 recap");
    assert_eq!(
        params(
            h.server
                .requests()
                .iter()
                .find(|r| r.path() == "/shows/169/episodes")
                .unwrap(),
            "specials"
        ),
        vec!["1".to_owned()]
    );

    let pilot = &items[1];
    assert_eq!(pilot.core.title, "Pilot");
    assert_eq!(pilot.core.plot, "Walt is diagnosed.");
    assert_eq!(pilot.core.ids.get("tvmaze"), Some("900"));
    assert_eq!(pilot.core.year, 2008);
    assert_eq!(pilot.core.ratings.get(RATING_ID).unwrap().value, 8.8);
    assert_eq!(
        pilot
            .core
            .artwork_urls
            .get(&MediaFileType::Thumb)
            .map(String::as_str),
        Some("https://img.example.test/tvmaze/e900_o.jpg")
    );
    let ItemDetail::Episode(fields) = &pilot.detail else {
        panic!("episode");
    };
    assert_eq!(fields.first_aired, CalendarDate::new(2008, 1, 20));
    assert_eq!(fields.runtime_minutes, 58);
    // Missing values stay empty.
    assert!(items[2].core.plot.is_empty() && items[2].core.ratings.is_empty());
}

#[tokio::test]
async fn the_dvd_list_keeps_the_aired_numbers() {
    let h = Harness::new().await;
    let items = h
        .provider
        .episodes(&show_ids(), &dvd_group(), &opts())
        .await
        .unwrap();
    // The entry without an embedded episode is skipped; the rest is in DVD order.
    let dvd: Vec<_> = items
        .iter()
        .map(|i| number(i, &dvd_group()).unwrap())
        .collect();
    assert_eq!(dvd, vec![(0, 1), (1, 1), (1, 2)]);
    assert_eq!(items[0].core.title, "Season 1 recap");
    assert_eq!(number(&items[0], &EpisodeGroup::aired()), None);
    assert_eq!(number(&items[1], &EpisodeGroup::aired()), Some((1, 1)));
    assert_eq!(h.server.count("/alternatelists/31/alternateepisodes"), 1);
    let request = h
        .server
        .requests()
        .into_iter()
        .find(|r| r.path() == "/alternatelists/31/alternateepisodes")
        .unwrap();
    assert_eq!(params(&request, "embed"), vec!["episodes".to_owned()]);
}

#[tokio::test]
async fn an_alternate_list_is_requested_by_its_name() {
    let h = Harness::new().await;
    let items = h
        .provider
        .episodes(&show_ids(), &streaming_group(), &opts())
        .await
        .unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(number(&items[1], &streaming_group()), Some((1, 2)));
    assert_eq!(h.server.count("/alternatelists/32/alternateepisodes"), 1);
    assert_eq!(h.server.count("/alternatelists/31/alternateepisodes"), 0);
}

#[tokio::test]
async fn an_ordering_the_show_lacks_is_not_found() {
    let h = Harness::new().await;
    for group in [
        EpisodeGroup::absolute(),
        EpisodeGroup::new(EpisodeOrdering::Alternate, "Production order"),
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
    assert_eq!(h.server.count("/alternatelists/31/alternateepisodes"), 0);

    // A show with no alternate lists has only the aired order.
    let err = h
        .provider
        .episodes(&ids(&[("tvmaze", "7")]), &dvd_group(), &opts())
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::NotFound(_)), "{err:?}");
}

#[tokio::test]
async fn an_empty_episode_list_is_not_found() {
    let h = Harness::new().await;
    h.server.route("/shows/169/episodes", Reply::ok("[]"));
    let err = h
        .provider
        .episodes(&show_ids(), &EpisodeGroup::aired(), &opts())
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::NotFound(_)), "{err:?}");
}
