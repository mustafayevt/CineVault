//! The Torznab (own instance) and Nyaa sources against recorded and hand-made feeds.

mod support;

use cv_torrent::CancelToken;
use cv_torrent::model::{MediaKind, Scope, SkipReason, SourceError, Support};
use cv_torrent::source::{SearchContext, TorrentSource};
use cv_torrent::sources::nyaa::NyaaSource;
use cv_torrent::sources::torznab::{
    TorznabSource, detect_jackett, jackett_all_feed, test_connection,
};
use support::fake_server::{FakeServer, Reply};
use support::fast_client;
use support::queries::{movie, query, show};

const KEY: &str = "secret-key-12345";

fn context() -> SearchContext {
    SearchContext::quiet(CancelToken::new())
}

fn anime(title: &str, scope: Scope) -> cv_torrent::model::TorrentQuery {
    let mut q = query(MediaKind::Show, title, None, scope);
    q.anime = true;
    q.year = None;
    q
}

// ------------------------------------------------------------------ Nyaa

#[tokio::test]
async fn nyaa_reads_the_recorded_feed() {
    let server = FakeServer::start().await;
    server.route("/", Reply::xml_fixture("nyaa/rss.xml"));
    let source = NyaaSource::new(fast_client(), &server.base());
    let rows = source
        .search(&anime("Sousou no Frieren 1080p", Scope::Show), &context())
        .await
        .expect("rows");
    assert_eq!(rows.len(), 75);
    assert!(
        rows.iter()
            .all(|r| r.info_hash.as_deref().is_some_and(|h| h.len() == 40))
    );
    assert!(
        rows.iter()
            .all(|r| r.source.as_ref().is_some_and(|s| s.0 == "nyaa"))
    );
    assert!(rows.iter().all(|r| {
        r.torrent_url
            .as_deref()
            .is_some_and(|u| u.contains("/download/"))
    }));
    let first = &rows[0];
    // Nyaa's item has no comments address; its guid is the view page.
    assert_eq!(
        first.details_url.as_deref(),
        Some("https://nyaa.si/view/2175192")
    );
    assert_eq!((first.seeders, first.leechers), (Some(67), Some(2)));
    assert!(first.size.is_some_and(|s| s > 7 * 1024 * 1024 * 1024));

    let sent = &server.requests()[0];
    assert_eq!(sent.query("page").as_deref(), Some("rss"));
    assert_eq!(sent.query("c").as_deref(), Some("1_0"));
    assert_eq!(sent.query("f").as_deref(), Some("0"));
    assert_eq!(sent.query("q").as_deref(), Some("Sousou no Frieren 1080p"));
}

#[tokio::test]
async fn nyaa_asks_for_the_episode_number_then_the_title() {
    let server = FakeServer::start().await;
    server.route("/", Reply::xml_fixture("nyaa/rss.xml"));
    let source = NyaaSource::new(fast_client(), &server.base());
    source
        .search(&anime("Frieren", Scope::Episode(1, 5)), &context())
        .await
        .expect("rows");
    let asked: Vec<_> = server
        .requests()
        .iter()
        .map(|r| r.query("q").unwrap())
        .collect();
    assert_eq!(asked, vec!["Frieren 05", "Frieren"]);
}

#[tokio::test]
async fn nyaa_is_only_for_anime() {
    let source = NyaaSource::new(fast_client(), "http://127.0.0.1:9");
    assert_eq!(
        source.supports(&movie("X", None)),
        Support::Skip(SkipReason::AnimeOnly)
    );
    assert_eq!(source.supports(&anime("X", Scope::Show)), Support::Yes);
}

#[tokio::test]
async fn nyaa_keeps_good_items_of_a_broken_feed_and_refuses_pages() {
    let server = FakeServer::start().await;
    server.route_query(
        "/",
        &[("q", "Broken")],
        Reply::xml_fixture("nyaa/malformed.xml"),
    );
    server.route_query("/", &[("q", "Html")], Reply::challenge_page());
    server.route_query("/", &[("q", "Json")], Reply::json("{\"a\":1}"));
    let source = NyaaSource::new(fast_client(), &server.base());
    let rows = source
        .search(&anime("Broken", Scope::Show), &context())
        .await
        .expect("rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].name, "[G] Good - 01 [1080p]");
    assert_eq!(
        source
            .search(&anime("Html", Scope::Show), &context())
            .await
            .unwrap_err(),
        SourceError::Blocked
    );
    assert!(matches!(
        source
            .search(&anime("Json", Scope::Show), &context())
            .await
            .unwrap_err(),
        SourceError::BadAnswer(_)
    ));
}

// ------------------------------------------------------------------ Torznab

fn instance(server: &FakeServer) -> TorznabSource {
    TorznabSource::user_instance(fast_client(), &server.url("/api"), KEY)
}

async fn server_with_caps(caps: &str) -> FakeServer {
    let server = FakeServer::start().await;
    server.route_query("/api", &[("t", "caps")], Reply::xml_fixture(caps));
    server
}

#[tokio::test]
async fn a_movie_with_an_imdb_id_uses_the_id_search_when_the_caps_allow() {
    let server = server_with_caps("torznab/caps.xml").await;
    server.route_query(
        "/api",
        &[("t", "movie")],
        Reply::xml_fixture("torznab/results.xml"),
    );
    let source = instance(&server);
    let rows = source
        .search(
            &movie("Night of the Living Dead", Some("tt0063350")),
            &context(),
        )
        .await
        .expect("rows");

    assert_eq!(
        rows.len(),
        2,
        "the two readable items; the two broken ones are skipped"
    );
    assert!(
        rows.iter()
            .all(|r| r.source.as_ref().is_some_and(|s| s.0 == "torznab"))
    );
    let with_hash = &rows[0];
    assert_eq!(with_hash.info_hash.as_deref(), Some(&*"b".repeat(40)));
    assert!(with_hash.magnet.is_some());
    assert_eq!(
        with_hash.details_url.as_deref(),
        Some("https://t.example/details/1")
    );
    assert_eq!(with_hash.seeders, Some(52));
    assert_eq!(with_hash.leechers, Some(9));
    let without = &rows[1];
    assert_eq!(without.info_hash, None);
    assert_eq!(without.magnet, None);
    assert_eq!(
        without.torrent_url.as_deref(),
        Some("https://t.example/dl/2.torrent")
    );

    let requests = server.requests();
    let by_id = requests
        .iter()
        .find(|r| r.query("imdbid").is_some())
        .expect("an id search");
    assert_eq!(by_id.query("imdbid").as_deref(), Some("0063350"));
    assert_eq!(by_id.query("apikey").as_deref(), Some(KEY));
    // The title search runs next to it: an all-indexers feed passes an ID search only to the
    // sites that understand IDs.
    let by_title = requests
        .iter()
        .find(|r| r.query("t").as_deref() == Some("search"))
        .expect("a title search too");
    assert_eq!(
        by_title.query("q").as_deref(),
        Some("Night of the Living Dead 1968")
    );
}

#[tokio::test]
async fn an_episode_searches_by_season_and_episode_and_caps_are_asked_once() {
    let server = server_with_caps("torznab/caps.xml").await;
    server.route_query(
        "/api",
        &[("t", "tvsearch")],
        Reply::xml_fixture("torznab/results.xml"),
    );
    let source = instance(&server);
    let q = show("Seinfeld", Some("tt0098904"), Scope::Episode(1, 2));
    source.search(&q, &context()).await.expect("rows");
    source.search(&q, &context()).await.expect("rows");
    let requests = server.requests();
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.query("t").as_deref() == Some("caps"))
            .count(),
        1
    );
    let tv = requests
        .iter()
        .find(|r| r.query("t").as_deref() == Some("tvsearch"))
        .unwrap();
    assert_eq!(tv.query("season").as_deref(), Some("1"));
    assert_eq!(tv.query("ep").as_deref(), Some("2"));
    assert_eq!(tv.query("imdbid").as_deref(), Some("0098904"));
}

#[tokio::test]
async fn an_empty_id_search_falls_back_to_text_queries() {
    let server = server_with_caps("torznab/caps.xml").await;
    server.route_query(
        "/api",
        &[("t", "movie")],
        Reply::xml("<rss version=\"2.0\"><channel></channel></rss>"),
    );
    server.route_query(
        "/api",
        &[("t", "search")],
        Reply::xml_fixture("torznab/results.xml"),
    );
    let source = instance(&server);
    let rows = source
        .search(
            &movie("Night of the Living Dead", Some("tt0063350")),
            &context(),
        )
        .await
        .expect("rows");
    assert_eq!(rows.len(), 2);
    let text = server
        .requests()
        .into_iter()
        .find(|r| r.query("t").as_deref() == Some("search"))
        .unwrap();
    assert_eq!(
        text.query("q").as_deref(),
        Some("Night of the Living Dead 1968")
    );
}

#[tokio::test]
async fn an_instance_without_id_search_gets_text_queries_only() {
    let server = server_with_caps("torznab/caps_text_only.xml").await;
    server.route_query(
        "/api",
        &[("t", "search")],
        Reply::xml_fixture("torznab/results.xml"),
    );
    let source = instance(&server);
    source
        .search(
            &movie("Night of the Living Dead", Some("tt0063350")),
            &context(),
        )
        .await
        .expect("rows");
    assert!(
        server
            .requests()
            .iter()
            .all(|r| r.query("imdbid").is_none())
    );
    // And without any IMDb ID the same.
    source
        .search(&movie("Night of the Living Dead", None), &context())
        .await
        .expect("rows");
}

#[tokio::test]
async fn an_id_search_that_fails_on_the_site_falls_back_to_the_title() {
    // A real indexer (2026-10-26) lists the ID search in its caps and then fails on it with the
    // site's own redirect; the same site answers the title search.
    let server = server_with_caps("torznab/caps.xml").await;
    server.route_query(
        "/api",
        &[("t", "movie")],
        Reply::xml(
            r#"<?xml version="1.0"?><error code="900" description="Jackett.Common.IndexerException: Exception (therarbg): Error Parsing Json Response: Status=Found Response=" />"#,
        ),
    );
    server.route_query(
        "/api",
        &[("t", "search")],
        Reply::xml_fixture("torznab/results.xml"),
    );
    let rows = instance(&server)
        .search(
            &movie("Night of the Living Dead", Some("tt0063350")),
            &context(),
        )
        .await
        .expect("rows from the title search");
    assert_eq!(rows.len(), 2);
}

#[tokio::test]
async fn a_challenge_on_the_id_search_leaves_the_title_search_its_rows() {
    let server = server_with_caps("torznab/caps.xml").await;
    server.route_query(
        "/api",
        &[("t", "movie")],
        Reply::xml_fixture("torznab/indexer_challenge_error.xml"),
    );
    server.route_query(
        "/api",
        &[("t", "search")],
        Reply::xml_fixture("torznab/results.xml"),
    );
    let rows = instance(&server)
        .search(
            &movie("Night of the Living Dead", Some("tt0063350")),
            &context(),
        )
        .await
        .expect("rows from the title search");
    assert_eq!(rows.len(), 2);
}

#[tokio::test]
async fn the_id_and_the_title_search_are_merged_into_one_list() {
    // The ID search finds one release, the title search the same one and another.
    let server = server_with_caps("torznab/caps.xml").await;
    server.route_query(
        "/api",
        &[("t", "movie")],
        Reply::xml(&format!(
            r#"<rss version="2.0" xmlns:torznab="http://torznab.com/schemas/2015/feed"><channel>
            <item><title>Heat.1995.1080p.BluRay.x264</title><size>100</size>
            <torznab:attr name="infohash" value="{}"/></item>
            </channel></rss>"#,
            "a".repeat(40)
        )),
    );
    server.route_query(
        "/api",
        &[("t", "search")],
        Reply::xml(&format!(
            r#"<rss version="2.0" xmlns:torznab="http://torznab.com/schemas/2015/feed"><channel>
            <item><title>Heat 1995 1080p BluRay x264</title><size>100</size>
            <torznab:attr name="infohash" value="{}"/></item>
            <item><title>Heat.1995.2160p.UHD.BluRay</title><size>200</size>
            <torznab:attr name="infohash" value="{}"/></item>
            </channel></rss>"#,
            "A".repeat(40),
            "c".repeat(40)
        )),
    );
    let rows = instance(&server)
        .search(&movie("Heat", Some("tt0113277")), &context())
        .await
        .expect("rows");
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["Heat.1995.1080p.BluRay.x264", "Heat.1995.2160p.UHD.BluRay"],
        "each release once, the ID search's row first"
    );
}

#[tokio::test]
async fn a_challenge_on_both_searches_is_a_blocked_source() {
    let server = server_with_caps("torznab/caps.xml").await;
    for t in ["movie", "search"] {
        server.route_query(
            "/api",
            &[("t", t)],
            Reply::xml_fixture("torznab/indexer_challenge_error.xml"),
        );
    }
    let error = instance(&server)
        .search(
            &movie("Night of the Living Dead", Some("tt0063350")),
            &context(),
        )
        .await
        .unwrap_err();
    assert_eq!(error, SourceError::Blocked);
}

#[tokio::test]
async fn an_indexer_that_hit_a_browser_challenge_is_a_blocked_source() {
    // Recorded from the indexer service (2026-10-26): error 900 with the whole exception text.
    let server = server_with_caps("torznab/caps.xml").await;
    server.route_query(
        "/api",
        &[("t", "search")],
        Reply::xml_fixture("torznab/indexer_challenge_error.xml"),
    );
    server.route_query(
        "/api",
        &[("t", "movie")],
        Reply::xml_fixture("torznab/indexer_challenge_error.xml"),
    );
    let error = instance(&server)
        .search(&movie("Blade Runner", None), &context())
        .await
        .unwrap_err();
    assert_eq!(error, SourceError::Blocked);
}

#[tokio::test]
async fn a_wrong_key_is_rejected_whichever_way_the_instance_says_so() {
    for reply in [
        Reply::xml_fixture("torznab/error_key.xml"),
        Reply::status(401),
        Reply::status(403),
    ] {
        let server = FakeServer::start().await;
        server.route("/api", reply);
        let error = instance(&server)
            .search(&movie("X", Some("tt1")), &context())
            .await
            .unwrap_err();
        assert_eq!(error, SourceError::KeyRejected);
    }
}

#[tokio::test]
async fn a_challenge_page_and_a_server_error_keep_their_reasons() {
    let server = FakeServer::start().await;
    server.route("/api", Reply::challenge_page());
    assert_eq!(
        instance(&server)
            .search(&movie("X", None), &context())
            .await
            .unwrap_err(),
        SourceError::Blocked
    );
    let down = FakeServer::start().await;
    down.route("/api", Reply::server_error());
    assert!(matches!(
        instance(&down)
            .search(&movie("X", None), &context())
            .await
            .unwrap_err(),
        SourceError::Unreachable(_)
    ));
}

#[tokio::test]
async fn the_key_never_appears_in_an_error_or_a_debug_print() {
    let server = FakeServer::start().await;
    server.route("/api", Reply::dropped());
    let error = instance(&server)
        .search(&movie("X", None), &context())
        .await
        .unwrap_err();
    assert!(!format!("{error:?} {error}").contains(KEY));
    assert!(!format!("{:?}", instance(&server).name()).contains(KEY));
}

#[tokio::test]
async fn the_connection_test_summarises_a_good_instance() {
    let server = server_with_caps("torznab/caps.xml").await;
    server.route_query(
        "/api",
        &[("t", "indexers")],
        Reply::xml_fixture("torznab/indexers.xml"),
    );
    let summary = test_connection(fast_client(), &server.url("/api"), KEY)
        .await
        .expect("connected");
    assert!(summary.search && summary.movie_search && summary.tv_search && summary.imdb_ids);
    assert_eq!(summary.indexers, Some(3));
}

#[tokio::test]
async fn the_connection_test_works_without_an_indexer_list() {
    let server = server_with_caps("torznab/caps_text_only.xml").await;
    let summary = test_connection(fast_client(), &server.url("/api"), KEY)
        .await
        .expect("connected");
    assert!(summary.search && !summary.imdb_ids);
    assert_eq!(summary.indexers, None);
}

#[tokio::test]
async fn the_connection_test_names_each_failure() {
    let bad_key = FakeServer::start().await;
    bad_key.route("/api", Reply::status(401));
    assert_eq!(
        test_connection(fast_client(), &bad_key.url("/api"), KEY)
            .await
            .unwrap_err(),
        SourceError::KeyRejected
    );

    let wrong_body = FakeServer::start().await;
    wrong_body.route("/api", Reply::xml_fixture("torznab/error_key.xml"));
    assert_eq!(
        test_connection(fast_client(), &wrong_body.url("/api"), KEY)
            .await
            .unwrap_err(),
        SourceError::KeyRejected
    );

    let not_torznab = FakeServer::start().await;
    not_torznab.route(
        "/api",
        Reply::xml("<rss version=\"2.0\"><channel></channel></rss>"),
    );
    assert_eq!(
        test_connection(fast_client(), &not_torznab.url("/api"), KEY)
            .await
            .unwrap_err(),
        SourceError::BadAnswer("This is not a Torznab feed".into())
    );

    let page = FakeServer::start().await;
    page.route("/api", Reply::challenge_page());
    assert_eq!(
        test_connection(fast_client(), &page.url("/api"), KEY)
            .await
            .unwrap_err(),
        SourceError::Blocked
    );

    let closed = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        format!("http://127.0.0.1:{port}/api")
    };
    assert!(matches!(
        test_connection(fast_client(), &closed, KEY)
            .await
            .unwrap_err(),
        SourceError::Unreachable(_)
    ));
}

#[tokio::test]
async fn the_own_instance_applies_to_everything() {
    let source = TorznabSource::user_instance(fast_client(), "http://127.0.0.1:9/api", KEY);
    assert_eq!(source.id().0, "torznab");
    assert_eq!(source.name(), "Your Torznab instance");
    assert_eq!(source.supports(&movie("X", None)), Support::Yes);
    assert_eq!(source.supports(&show("X", None, Scope::Show)), Support::Yes);
}

#[test]
fn a_recorded_jackett_item_gives_its_page_on_the_site() {
    // Two items recorded from the indexer service (LimeTorrents, 2026-10-26): no info-hash, a
    // download address that redirects to a magnet link, and the site's page as `comments`.
    let feed = cv_torrent::torznab::parse_feed(&support::fake_server::fixture_text(
        "torznab/jackett_items.xml",
    ))
    .expect("feed");
    assert_eq!(feed.items.len(), 2);
    for row in &feed.items {
        assert!(
            row.details_url.as_deref().is_some_and(|u| u
                .starts_with("https://www.limetorrents.fun/")
                && u.ends_with(".html")),
            "{:?}",
            row.details_url
        );
        assert!(
            row.torrent_url
                .as_deref()
                .is_some_and(|u| u.contains("/dl/limetorrents/"))
        );
    }
}

#[tokio::test]
async fn a_jackett_is_recognised_by_its_all_indexers_feed_refusing_a_missing_key() {
    const ALL: &str = "/api/v2.0/indexers/all/results/torznab/api";
    assert_eq!(
        jackett_all_feed("http://127.0.0.1:9117/"),
        format!("http://127.0.0.1:9117{ALL}")
    );
    // As a real Jackett answers (2026-10-26): status 200 and error 100, "Invalid API Key".
    let jackett = FakeServer::start().await;
    jackett.route(
        ALL,
        Reply::xml(r#"<?xml version="1.0" encoding="UTF-8"?><error code="100" description="Invalid API Key" />"#),
    );
    assert!(detect_jackett(fast_client(), &jackett.base()).await);
    let sent = jackett.requests();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].query("t").as_deref(), Some("caps"));
    assert_eq!(sent[0].query("apikey"), None, "no key is sent");

    // A web page, a feed that needs no key, nothing on that path, and nothing at all are not one.
    let page = FakeServer::start().await;
    page.route(ALL, Reply::challenge_page());
    assert!(!detect_jackett(fast_client(), &page.base()).await);
    let open = FakeServer::start().await;
    open.route(ALL, Reply::xml_fixture("torznab/caps.xml"));
    assert!(!detect_jackett(fast_client(), &open.base()).await);
    let other = FakeServer::start().await;
    assert!(!detect_jackett(fast_client(), &other.base()).await);
    assert!(!detect_jackett(fast_client(), "http://127.0.0.1:9").await);
}
