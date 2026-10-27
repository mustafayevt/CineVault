//! The YTS and EZTV sources against recorded and hand-made answers.

mod support;

use cv_torrent::CancelToken;
use cv_torrent::model::{Scope, SkipReason, SourceError, Support};
use cv_torrent::source::{SearchContext, TorrentSource};
use cv_torrent::sources::eztv::EztvSource;
use cv_torrent::sources::yts::YtsSource;
use support::fake_server::{FakeServer, Reply};
use support::fast_client;
use support::queries::{movie, show};

fn context() -> SearchContext {
    SearchContext::quiet(CancelToken::new())
}

#[tokio::test]
async fn yts_gives_one_release_per_quality() {
    // Recorded from the live API on 2026-10-26 (Blade Runner, tt0083658).
    let server = FakeServer::start().await;
    server.route_query(
        "/api/v2/list_movies.json",
        &[("query_term", "tt0083658")],
        Reply::json_fixture("yts/list_movies.json"),
    );
    let source = YtsSource::new(fast_client(), &server.base());
    let rows = source
        .search(&movie("Blade Runner", Some("tt0083658")), &context())
        .await
        .expect("rows");
    assert_eq!(rows.len(), 3);
    let names: Vec<_> = rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "Blade Runner (1982) 720p bluray x264 YTS",
            "Blade Runner (1982) 1080p bluray x264 YTS",
            "Blade Runner (1982) 2160p bluray x265 10bit YTS",
        ]
    );
    // Every quality of the movie points at the movie's page on the site.
    assert!(
        rows.iter()
            .all(|r| r.details_url.as_deref() == Some("https://yts.gg/movies/blade-runner-1982"))
    );
    assert_eq!(rows[1].seeders, Some(100));
    assert_eq!(rows[1].size, Some(2_050_846_884));
    assert_eq!(
        rows[1].info_hash.as_deref(),
        Some("46378ea47454427d4d2b7a7a0d903f1e3b5732f2")
    );
    assert!(
        rows[1]
            .torrent_url
            .as_deref()
            .is_some_and(|u| u.ends_with("46378EA47454427D4D2B7A7A0D903F1E3B5732F2"))
    );
}

#[tokio::test]
async fn yts_empty_and_error_answers() {
    let server = FakeServer::start().await;
    server.route_query(
        "/api/v2/list_movies.json",
        &[("query_term", "tt0")],
        Reply::json_fixture("yts/empty.json"),
    );
    server.route_query(
        "/api/v2/list_movies.json",
        &[("query_term", "tt1")],
        Reply::json_fixture("yts/error.json"),
    );
    let source = YtsSource::new(fast_client(), &server.base());
    assert!(
        source
            .search(&movie("X", Some("tt0")), &context())
            .await
            .expect("rows")
            .is_empty()
    );
    assert_eq!(
        source
            .search(&movie("X", Some("tt1")), &context())
            .await
            .unwrap_err(),
        SourceError::BadAnswer("Query term is too short".into())
    );
}

#[tokio::test]
async fn yts_failures_keep_their_reason() {
    let server = FakeServer::start().await;
    server.route("/api/v2/list_movies.json", Reply::challenge_page());
    let source = YtsSource::new(fast_client(), &server.base());
    assert_eq!(
        source
            .search(&movie("X", Some("tt1")), &context())
            .await
            .unwrap_err(),
        SourceError::Blocked
    );
}

#[tokio::test]
async fn yts_is_for_movies_with_an_imdb_id_only() {
    let source = YtsSource::new(fast_client(), "http://127.0.0.1:9");
    assert_eq!(source.supports(&movie("X", Some("tt1"))), Support::Yes);
    assert_eq!(
        source.supports(&movie("X", None)),
        Support::Skip(SkipReason::NoImdbId)
    );
    assert_eq!(
        source.supports(&show("X", Some("tt1"), Scope::Show)),
        Support::Skip(SkipReason::MoviesOnly)
    );
}

#[tokio::test]
async fn eztv_reads_a_recorded_page() {
    let server = FakeServer::start().await;
    server.route_query(
        "/api/get-torrents",
        &[("imdb_id", "0098904"), ("page", "1"), ("limit", "100")],
        Reply::json_fixture("eztv/page1.json"),
    );
    let source = EztvSource::new(fast_client(), &server.base());
    let rows = source
        .search(
            &show("Seinfeld", Some("tt0098904"), Scope::Show),
            &context(),
        )
        .await
        .expect("rows");
    assert_eq!(rows.len(), 5);
    assert_eq!(server.count("/api/get-torrents"), 1, "5 of 5 is one page");
    let first = &rows[0];
    assert_eq!(first.name, "Seinfeld.S09E05.MULTi.1080p.WEB.x265-MACK4");
    // The recorded answer has the episode's id and no address; the page is built from both.
    assert_eq!(
        first.details_url,
        Some(format!("{}/ep/1736394/", server.base()))
    );
    assert_eq!((first.season, first.episode), (Some(9), Some(5)));
    assert!(
        first
            .magnet
            .as_deref()
            .is_some_and(|m| m.starts_with("magnet:?xt=urn:btih:"))
    );
    assert!(first.published.is_some());
    assert!(
        rows.iter()
            .all(|r| r.source.as_ref().is_some_and(|s| s.0 == "eztv"))
    );
}

fn page(total: usize, from: usize, count: usize) -> String {
    let items: Vec<String> = (from..from + count)
        .map(|i| {
            format!(
                r#"{{"hash":"{i:040x}","filename":"Show.S01E{n:02}.720p.mkv","season":"1","episode":"{n}","seeds":1,"peers":1,"size_bytes":"1000"}}"#,
                n = i % 99 + 1
            )
        })
        .collect();
    format!(
        r#"{{"torrents_count":{total},"limit":100,"page":1,"torrents":[{}]}}"#,
        items.join(",")
    )
}

#[tokio::test]
async fn eztv_reads_more_pages_up_to_three() {
    let server = FakeServer::start().await;
    for (number, from) in [("1", 1), ("2", 101), ("3", 201), ("4", 301)] {
        server.route_query(
            "/api/get-torrents",
            &[("page", number)],
            Reply::json(&page(450, from, 100)),
        );
    }
    let source = EztvSource::new(fast_client(), &server.base());
    let rows = source
        .search(&show("Show", Some("tt5"), Scope::Show), &context())
        .await
        .expect("rows");
    assert_eq!(rows.len(), 300, "three pages of 100, never a fourth");
    assert_eq!(server.count("/api/get-torrents"), 3);
}

#[tokio::test]
async fn eztv_stops_after_the_last_page_and_on_cancel() {
    let server = FakeServer::start().await;
    server.route_query(
        "/api/get-torrents",
        &[("page", "1")],
        Reply::json(&page(130, 1, 100)),
    );
    server.route_query(
        "/api/get-torrents",
        &[("page", "2")],
        Reply::json(&page(130, 101, 30)),
    );
    let source = EztvSource::new(fast_client(), &server.base());
    let rows = source
        .search(&show("Show", Some("tt5"), Scope::Show), &context())
        .await
        .expect("rows");
    assert_eq!(rows.len(), 130);
    assert_eq!(server.count("/api/get-torrents"), 2);

    let cancelled = CancelToken::new();
    cancelled.cancel();
    let rows = source
        .search(
            &show("Show", Some("tt5"), Scope::Show),
            &SearchContext::quiet(cancelled),
        )
        .await
        .expect("rows");
    assert!(rows.is_empty());
    assert_eq!(
        server.count("/api/get-torrents"),
        2,
        "a cancelled search asks nothing"
    );
}

#[tokio::test]
async fn eztv_empty_error_and_blocked_answers() {
    let server = FakeServer::start().await;
    server.route_query(
        "/api/get-torrents",
        &[("imdb_id", "1")],
        Reply::json_fixture("eztv/empty.json"),
    );
    server.route_query(
        "/api/get-torrents",
        &[("imdb_id", "2")],
        Reply::json_fixture("eztv/error.json"),
    );
    server.route_query(
        "/api/get-torrents",
        &[("imdb_id", "3")],
        Reply::challenge_page(),
    );
    let source = EztvSource::new(fast_client(), &server.base());
    let run = |imdb: &'static str| {
        let source = &source;
        async move {
            source
                .search(&show("X", Some(imdb), Scope::Show), &context())
                .await
        }
    };
    assert!(run("tt1").await.expect("rows").is_empty());
    assert_eq!(
        run("tt2").await.unwrap_err(),
        SourceError::BadAnswer("Invalid imdb_id".into())
    );
    assert_eq!(run("tt3").await.unwrap_err(), SourceError::Blocked);
}

#[tokio::test]
async fn eztv_is_for_shows_with_an_imdb_id_only() {
    let source = EztvSource::new(fast_client(), "http://127.0.0.1:9");
    assert_eq!(
        source.supports(&show("X", Some("tt1"), Scope::Episode(1, 1))),
        Support::Yes
    );
    assert_eq!(
        source.supports(&show("X", None, Scope::Show)),
        Support::Skip(SkipReason::NoImdbId)
    );
    assert_eq!(
        source.supports(&movie("X", Some("tt1"))),
        Support::Skip(SkipReason::TvOnly)
    );
}

#[tokio::test]
async fn eztv_sends_partial_rows_after_each_page() {
    use std::sync::{Arc, Mutex};
    let server = FakeServer::start().await;
    server.route_query(
        "/api/get-torrents",
        &[("page", "1")],
        Reply::json(&page(130, 1, 100)),
    );
    server.route_query(
        "/api/get-torrents",
        &[("page", "2")],
        Reply::json(&page(130, 101, 30)),
    );
    let seen: Arc<Mutex<Vec<usize>>> = Arc::default();
    let sink = Arc::clone(&seen);
    let context = SearchContext::with_partial(
        CancelToken::new(),
        Arc::new(move |rows| sink.lock().unwrap().push(rows.len())),
    );
    let source = EztvSource::new(fast_client(), &server.base());
    source
        .search(&show("Show", Some("tt5"), Scope::Show), &context)
        .await
        .expect("rows");
    assert_eq!(*seen.lock().unwrap(), vec![100, 30]);
}
