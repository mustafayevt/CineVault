//! An indexer of the managed service as a source: its own Torznab feed on the loopback port, asked
//! by IMDb ID where its caps allow it and by text otherwise.

mod support;

use cv_torrent::CancelToken;
use cv_torrent::model::{MediaKind, Scope, SourceError, SourceKind, Support};
use cv_torrent::source::{INDEXER_LANE, SearchContext, TorrentSource};
use cv_torrent::sources::indexer::{IndexerSource, feed_url};
use support::fake_server::{FakeServer, Reply};
use support::fast_client;
use support::queries::{movie, query, show};

const KEY: &str = "0123456789abcdef0123456789abcdef";
const FEED: &str = "/api/v2.0/indexers/yts/results/torznab/api";

fn context() -> SearchContext {
    SearchContext::quiet(CancelToken::new())
}

fn source(server: &FakeServer) -> IndexerSource {
    IndexerSource::new(fast_client(), &server.base(), KEY, "yts", "YTS (indexer)")
}

async fn server_with_caps(caps: &str) -> FakeServer {
    let server = FakeServer::start().await;
    server.route_query(FEED, &[("t", "caps")], Reply::xml_fixture(caps));
    server
}

#[tokio::test]
async fn it_describes_itself_as_an_indexer() {
    let server = FakeServer::start().await;
    let source = source(&server);
    assert_eq!(source.id().0, "indexer:yts");
    assert_eq!(source.name(), "YTS (indexer)");
    assert_eq!(source.kind(), SourceKind::Indexer);
    assert_eq!(source.supports(&movie("Heat", None)), Support::Yes);
    assert_eq!(INDEXER_LANE, "torrent:indexer");
    assert!(feed_url(&server.base(), "yts").ends_with(FEED));
}

#[tokio::test]
async fn a_movie_is_asked_by_imdb_id_on_the_indexers_own_feed_with_the_key() {
    let server = server_with_caps("torznab/caps.xml").await;
    server.route_query(
        FEED,
        &[("t", "movie")],
        Reply::xml_fixture("torznab/results.xml"),
    );
    let rows = source(&server)
        .search(
            &movie("Night of the Living Dead", Some("tt0063350")),
            &context(),
        )
        .await
        .expect("rows");
    assert_eq!(rows.len(), 2);
    assert!(
        rows.iter()
            .all(|r| r.source.as_ref().is_some_and(|s| s.0 == "indexer:yts"))
    );
    let requests = server.requests();
    assert!(requests.iter().all(|r| r.path() == FEED));
    let by_id = requests
        .iter()
        .find(|r| r.query("imdbid").is_some())
        .expect("an id search");
    assert_eq!(by_id.query("imdbid").as_deref(), Some("0063350"));
    assert_eq!(by_id.query("apikey").as_deref(), Some(KEY));
    assert!(
        requests
            .iter()
            .any(|r| r.query("t").as_deref() == Some("search")),
        "the title search runs next to the id search"
    );
}

#[tokio::test]
async fn an_episode_is_asked_with_season_and_episode_where_the_caps_allow() {
    let server = server_with_caps("torznab/caps.xml").await;
    server.route_query(
        FEED,
        &[("t", "tvsearch")],
        Reply::xml_fixture("torznab/results.xml"),
    );
    let q = show("Seinfeld", Some("tt0098904"), Scope::Episode(1, 2));
    source(&server).search(&q, &context()).await.expect("rows");
    let sent = server
        .requests()
        .into_iter()
        .find(|r| r.query("t").as_deref() == Some("tvsearch"))
        .expect("a tvsearch");
    assert_eq!(sent.query("imdbid").as_deref(), Some("0098904"));
    assert_eq!(sent.query("season").as_deref(), Some("1"));
    assert_eq!(sent.query("ep").as_deref(), Some("2"));
}

#[tokio::test]
async fn an_indexer_that_cannot_search_by_id_gets_the_text_queries() {
    let server = server_with_caps("torznab/caps_text_only.xml").await;
    server.route_query(
        FEED,
        &[("t", "search")],
        Reply::xml_fixture("torznab/results.xml"),
    );
    let mut q = query(
        MediaKind::Movie,
        "Night of the Living Dead",
        Some("tt0063350"),
        Scope::Show,
    );
    q.year = Some(1968);
    let rows = source(&server).search(&q, &context()).await.expect("rows");
    assert!(!rows.is_empty());
    assert!(
        server
            .requests()
            .iter()
            .all(|r| r.query("imdbid").is_none()),
        "no id search without the caps for it"
    );
}

#[tokio::test]
async fn a_rejected_key_is_reported_as_such() {
    let server = FakeServer::start().await;
    server.route(FEED, Reply::xml_fixture("torznab/error_key.xml"));
    let error = source(&server)
        .search(&movie("Heat", Some("tt0113277")), &context())
        .await
        .expect_err("rejected");
    assert_eq!(error, SourceError::KeyRejected);
}
