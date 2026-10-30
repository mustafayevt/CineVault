//! The "Latest releases" feed of each source against hand-made answers in the shape the live
//! sites gave on 2026-10-31 (`fixtures/latest/`), and the feed session over real sources.

mod support;

use std::sync::Arc;

use cv_torrent::CancelToken;
use cv_torrent::latest::{
    CursorState, FeedKind, FeedSession, FeedSkip, FeedSupport, LatestAnswer, PageOutcome,
};
use cv_torrent::model::{SourceError, SourceKind};
use cv_torrent::source::{SearchContext, SourceHttp, TorrentSource};
use cv_torrent::sources::apibay::ApibaySource;
use cv_torrent::sources::eztv::EztvSource;
use cv_torrent::sources::knaben::KnabenSource;
use cv_torrent::sources::nyaa::NyaaSource;
use cv_torrent::sources::torrentio::TorrentioSource;
use cv_torrent::sources::torznab::TorznabSource;
use cv_torrent::sources::yts::YtsSource;
use support::fake_server::{FakeServer, Reply};
use support::fast_client;

fn context() -> SearchContext {
    SearchContext::quiet(CancelToken::new())
}

fn page(answer: LatestAnswer) -> (Vec<cv_torrent::model::RawRelease>, bool) {
    match answer {
        LatestAnswer::Page { rows, more } => (rows, more),
        LatestAnswer::Unsupported => panic!("expected a page"),
    }
}

#[tokio::test]
async fn yts_lists_the_newest_movies_page_by_page() {
    let server = FakeServer::start().await;
    server.route_query(
        "/api/v2/list_movies.json",
        &[
            ("sort_by", "date_added"),
            ("order_by", "desc"),
            ("page", "1"),
        ],
        Reply::json_fixture("latest/yts_page1.json"),
    );
    server.route_query(
        "/api/v2/list_movies.json",
        &[
            ("sort_by", "date_added"),
            ("order_by", "desc"),
            ("page", "2"),
        ],
        Reply::json_fixture("latest/yts_page2.json"),
    );
    let source = YtsSource::new(fast_client(), &server.base());
    assert_eq!(source.latest_support(FeedKind::Movies), FeedSupport::Yes);
    assert_eq!(
        source.latest_support(FeedKind::Tv),
        FeedSupport::Skip(FeedSkip::MoviesOnly)
    );
    // 3 movies in all, 50 a page: one page says there is no more.
    let (rows, more) = page(
        source
            .latest(FeedKind::Movies, 0, &context())
            .await
            .expect("page"),
    );
    assert_eq!(rows.len(), 3, "every torrent of every movie");
    assert!(!more);
    assert_eq!(rows[0].name, "The Quiet Harbour (2026) 720p web x264 YTS");
    assert_eq!(rows[0].imdb.as_deref(), Some("tt9900001"));
    let (rows, _) = page(
        source
            .latest(FeedKind::Movies, 1, &context())
            .await
            .expect("page"),
    );
    assert_eq!(rows[0].name, "Winter Orchard (2024) 1080p bluray x264 YTS");
    let asked = server.requests();
    assert_eq!(asked[0].query("limit").as_deref(), Some("50"));
}

#[tokio::test]
async fn eztv_lists_the_newest_episodes_while_the_count_says_there_are_more() {
    let server = FakeServer::start().await;
    server.route_query(
        "/api/get-torrents",
        &[("page", "1")],
        Reply::json_fixture("latest/eztv_page1.json"),
    );
    server.route_query(
        "/api/get-torrents",
        &[("page", "2")],
        Reply::json_fixture("latest/eztv_page2.json"),
    );
    let source = EztvSource::new(fast_client(), &server.base());
    assert_eq!(
        source.latest_support(FeedKind::Movies),
        FeedSupport::Skip(FeedSkip::TvOnly)
    );
    let (rows, more) = page(source.latest(FeedKind::Tv, 0, &context()).await.unwrap());
    assert_eq!(rows.len(), 2);
    // 3 in all, 100 a page.
    assert!(!more);
    assert_eq!(
        rows[0].name,
        "Garden.Rescue.Crew.S04E05.1080p.HEVC.x265-GRP"
    );
    assert_eq!(rows[0].imdb.as_deref(), Some("tt9900101"));
    assert_eq!(rows[1].imdb, None, "an IMDb ID of 0 is no ID");
    assert!(
        rows[0]
            .details_url
            .as_deref()
            .is_some_and(|u| u.starts_with(&server.base()))
    );
    let request = &server.requests()[0];
    assert_eq!(request.query("imdb_id"), None);
    assert_eq!(request.query("limit").as_deref(), Some("100"));
}

#[tokio::test]
async fn the_pirate_bay_lists_each_category_once_with_no_further_pages() {
    let server = FakeServer::start().await;
    server.route_query(
        "/q.php",
        &[("q", "category:207"), ("page", "0")],
        Reply::json_fixture("latest/apibay_207.json"),
    );
    server.route_query(
        "/q.php",
        &[("q", "category:208"), ("page", "0")],
        Reply::json_fixture("latest/apibay_208.json"),
    );
    server.route("/q.php", Reply::json_fixture("apibay/empty.json"));
    let source = ApibaySource::new(fast_client(), &server.base());
    let (rows, more) = page(source.latest(FeedKind::All, 0, &context()).await.unwrap());
    assert!(!more);
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    assert!(names.contains(&"The.Quiet.Harbour.2026.1080p.WEBRip.x264-GRP"));
    assert!(names.contains(&"Garden Rescue Crew S04E05 1080p WEB-DL DDP5.1 x265-GRP"));
    let asked: Vec<String> = server
        .requests()
        .iter()
        .filter_map(|r| r.query("q"))
        .collect();
    // Movies and TV: the three video categories of each.
    assert_eq!(asked.len(), 6, "{asked:?}");
    // A later page asks nothing.
    let before = server.requests().len();
    let (rows, more) = page(source.latest(FeedKind::All, 1, &context()).await.unwrap());
    assert!(rows.is_empty() && !more);
    assert_eq!(server.requests().len(), before);
}

#[test]
fn the_pirate_bay_asks_the_video_categories_of_the_kind() {
    use cv_torrent::sources::apibay::latest_queries;
    assert_eq!(
        latest_queries(FeedKind::Movies),
        ["category:201", "category:207", "category:211"]
    );
    assert_eq!(
        latest_queries(FeedKind::Tv),
        ["category:205", "category:208", "category:212"]
    );
    assert_eq!(latest_queries(FeedKind::All).len(), 6);
}

#[tokio::test]
async fn knaben_lists_by_date_with_adult_and_unsafe_hidden_and_pages_by_offset() {
    let server = FakeServer::start().await;
    server.route_method(
        "POST",
        "/v1",
        Reply::json_fixture("latest/knaben_page.json"),
    );
    let source = KnabenSource::new(fast_client(), &server.base());
    let (rows, more) = page(source.latest(FeedKind::All, 2, &context()).await.unwrap());
    assert_eq!(rows.len(), 2);
    assert!(!more, "fewer than a full page");
    assert_eq!(rows[1].category.as_deref(), Some("TV / HD"));
    let request = &server.requests()[0];
    assert_eq!(request.method, "POST");
    let body: serde_json::Value = serde_json::from_slice(&request.body).expect("JSON body");
    assert_eq!(body["order_by"], "date");
    assert_eq!(body["hide_xxx"], true);
    assert_eq!(body["hide_unsafe"], true);
    assert_eq!(body["from"], 200);
    assert_eq!(body["size"], 100);
    assert_eq!(
        body["categories"],
        serde_json::json!([3_000_000, 2_000_000])
    );
}

#[tokio::test]
async fn nyaa_lists_its_newest_anime_on_one_page_as_tv() {
    let server = FakeServer::start().await;
    server.route_query(
        "/",
        &[("page", "rss"), ("c", "1_2")],
        Reply::xml_fixture("latest/nyaa_rss.xml"),
    );
    let source = NyaaSource::new(fast_client(), &server.base());
    assert_eq!(
        source.latest_support(FeedKind::Movies),
        FeedSupport::Skip(FeedSkip::TvOnly)
    );
    let (rows, more) = page(source.latest(FeedKind::Tv, 0, &context()).await.unwrap());
    assert_eq!(rows.len(), 1);
    assert!(!more);
    assert_eq!(rows[0].category.as_deref(), Some("TV / Anime"));
    assert_eq!(rows[0].seeders, Some(120));
}

#[tokio::test]
async fn torrentio_has_no_feed() {
    let source = TorrentioSource::new(fast_client(), "http://127.0.0.1:9");
    assert_eq!(
        source.latest_support(FeedKind::All),
        FeedSupport::Skip(FeedSkip::NotOffered)
    );
}

fn torznab(server: &FakeServer) -> TorznabSource {
    TorznabSource::new(
        SourceHttp::new(fast_client(), "torznab"),
        "torznab",
        "Your Torznab instance",
        SourceKind::Torznab,
        &format!("{}/api", server.base()),
        Some("secret-key"),
    )
}

#[tokio::test]
async fn torznab_lists_without_words_in_the_video_categories_with_offset_paging() {
    let server = FakeServer::start().await;
    server.route("/api", Reply::xml_fixture("latest/torznab_page.xml"));
    let source = torznab(&server);
    let (rows, more) = page(
        source
            .latest(FeedKind::Movies, 1, &context())
            .await
            .unwrap(),
    );
    assert_eq!(rows.len(), 2);
    assert!(more);
    let request = &server.requests()[0];
    assert_eq!(request.query("t").as_deref(), Some("search"));
    assert_eq!(request.query("q"), None);
    assert_eq!(request.query("cat").as_deref(), Some("2000"));
    assert_eq!(request.query("offset").as_deref(), Some("100"));
    assert_eq!(request.query("limit").as_deref(), Some("100"));
    assert_eq!(request.query("apikey").as_deref(), Some("secret-key"));
}

#[tokio::test]
async fn a_torznab_feed_that_needs_words_takes_no_part_without_an_error() {
    for reply in [
        Reply::xml_fixture("latest/torznab_no_query.xml"),
        {
            let mut reply = Reply::xml_fixture("latest/torznab_no_query.xml");
            reply.status = 400;
            reply
        },
        Reply::status(400),
    ] {
        let server = FakeServer::start().await;
        server.route("/api", reply);
        let answer = torznab(&server)
            .latest(FeedKind::All, 0, &context())
            .await
            .expect("not an error");
        assert_eq!(answer, LatestAnswer::Unsupported);
    }
}

#[tokio::test]
async fn a_rejected_torznab_key_is_still_an_error() {
    let server = FakeServer::start().await;
    server.route(
        "/api",
        Reply::xml(r#"<?xml version="1.0"?><error code="100" description="Invalid API Key" />"#),
    );
    let error = torznab(&server)
        .latest(FeedKind::All, 0, &context())
        .await
        .unwrap_err();
    assert_eq!(error, SourceError::KeyRejected);
}

#[tokio::test]
async fn the_session_pages_a_real_source_and_keeps_each_release_once() {
    let server = FakeServer::start().await;
    server.route_query(
        "/api/v2/list_movies.json",
        &[("page", "1")],
        Reply::json_fixture("latest/yts_page1.json"),
    );
    let yts: Arc<dyn TorrentSource> = Arc::new(YtsSource::new(fast_client(), &server.base()));
    let apibay_server = FakeServer::start().await;
    apibay_server.route_query(
        "/q.php",
        &[("q", "category:207")],
        Reply::json_fixture("latest/apibay_207.json"),
    );
    apibay_server.route("/q.php", Reply::json_fixture("apibay/empty.json"));
    let apibay: Arc<dyn TorrentSource> =
        Arc::new(ApibaySource::new(fast_client(), &apibay_server.base()));
    let session = FeedSession::new(FeedKind::Movies, 7);
    let cancel = CancelToken::new();
    let first = session.read_page(Arc::clone(&yts), &cancel, None).await;
    assert!(matches!(
        first,
        PageOutcome::Read {
            added: 3,
            more: false,
            ..
        }
    ));
    assert_eq!(
        session.cursor(yts.id()).state,
        CursorState::Exhausted,
        "no more from YTS"
    );
    let second = session.read_page(Arc::clone(&apibay), &cancel, None).await;
    assert!(matches!(second, PageOutcome::Read { added: 2, .. }));
    assert_eq!(session.len(), 5);
    assert_eq!(
        session.read_page(yts, &cancel, None).await,
        PageOutcome::NotAsked
    );
}
