//! The Torrentio source against recorded answers.

mod support;

use std::time::Duration;

use cv_torrent::CancelToken;
use cv_torrent::model::{Scope, SkipReason, SourceError, Support};
use cv_torrent::source::{SearchContext, TorrentSource};
use cv_torrent::sources::torrentio::TorrentioSource;
use support::fake_server::{FakeServer, Reply};
use support::fast_client;
use support::queries::{movie, show};

fn context() -> SearchContext {
    SearchContext::quiet(CancelToken::new())
}

#[tokio::test]
async fn a_movie_answer_gives_every_readable_stream() {
    let server = FakeServer::start().await;
    server.route(
        "/stream/movie/tt0063350.json",
        Reply::json_fixture("torrentio/movie.json"),
    );
    let source = TorrentioSource::new(fast_client(), &server.base());
    let rows = source
        .search(
            &movie("Night of the Living Dead", Some("tt0063350")),
            &context(),
        )
        .await
        .expect("rows");
    assert_eq!(rows.len(), 45);
    let first = &rows[0];
    assert!(first.name.starts_with("Night Of The Living Dead 1968"));
    assert_eq!(first.seeders, Some(6));
    assert!(first.size.is_some_and(|s| s > 15 * 1024 * 1024 * 1024));
    assert_eq!(
        first.info_hash.as_deref(),
        Some("1d71aeb11a149d7f2c2d5b5b05193cf80a5b927c")
    );
    assert!(
        rows.iter()
            .all(|r| r.source.as_ref().is_some_and(|s| s.0 == "torrentio"))
    );
    assert!(
        rows.iter()
            .all(|r| r.info_hash.as_deref().is_some_and(|h| h.len() == 40))
    );
}

#[tokio::test]
async fn an_episode_asks_for_that_episode_and_keeps_the_file_name() {
    let server = FakeServer::start().await;
    server.route(
        "/stream/series/tt0098904:1:2.json",
        Reply::json_fixture("torrentio/episode.json"),
    );
    let source = TorrentioSource::new(fast_client(), &server.base());
    let rows = source
        .search(
            &show("Seinfeld", Some("tt0098904"), Scope::Episode(1, 2)),
            &context(),
        )
        .await
        .expect("rows");
    assert_eq!(rows.len(), 32);
    assert!(rows[0].name.starts_with("Seinfeld S01"), "{}", rows[0].name);
    assert!(
        rows[0].files.iter().any(|f| f.contains("S01E02")),
        "the episode file is kept: {:?}",
        rows[0].files
    );
}

#[tokio::test]
async fn a_season_asks_for_the_first_episode_and_returns_the_packs() {
    let server = FakeServer::start().await;
    server.route(
        "/stream/series/tt0098904:2:1.json",
        Reply::json_fixture("torrentio/season_pack.json"),
    );
    let source = TorrentioSource::new(fast_client(), &server.base());
    let rows = source
        .search(
            &show("Seinfeld", Some("tt0098904"), Scope::Season(2)),
            &context(),
        )
        .await
        .expect("rows");
    assert!(rows.iter().any(|r| r.name.contains("S02")));
    assert_eq!(server.count("/stream/series/tt0098904:2:1.json"), 1);
}

#[tokio::test]
async fn a_malformed_stream_is_dropped_and_the_rest_are_kept() {
    let server = FakeServer::start().await;
    server.route(
        "/stream/movie/tt1.json",
        Reply::json_fixture("torrentio/malformed.json"),
    );
    let source = TorrentioSource::new(fast_client(), &server.base());
    let rows = source
        .search(&movie("Good Movie", Some("tt1")), &context())
        .await
        .expect("rows");
    let names: Vec<_> = rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "Good.Movie.2020.1080p.WEB-DL",
            "Second.Good.Movie.2020.720p"
        ]
    );
    assert_eq!(
        rows[1].info_hash.as_deref(),
        Some("3123456789abcdef0123456789abcdef01234567")
    );
}

#[tokio::test]
async fn an_empty_answer_is_no_rows() {
    let server = FakeServer::start().await;
    server.route(
        "/stream/movie/tt1.json",
        Reply::json_fixture("torrentio/empty.json"),
    );
    let source = TorrentioSource::new(fast_client(), &server.base());
    assert!(
        source
            .search(&movie("X", Some("tt1")), &context())
            .await
            .expect("rows")
            .is_empty()
    );
}

#[tokio::test]
async fn answers_are_reused_for_thirty_minutes_but_not_forever() {
    let server = FakeServer::start().await;
    server.route(
        "/stream/movie/tt1.json",
        Reply::json_fixture("torrentio/malformed.json"),
    );
    let source = TorrentioSource::new(fast_client(), &server.base());
    for _ in 0..3 {
        source
            .search(&movie("X", Some("tt1")), &context())
            .await
            .expect("rows");
    }
    assert_eq!(
        server.count("/stream/movie/tt1.json"),
        1,
        "served from memory"
    );

    let short = TorrentioSource::new(fast_client(), &server.base())
        .with_cache_ttl(Duration::from_millis(1));
    short
        .search(&movie("X", Some("tt1")), &context())
        .await
        .expect("rows");
    tokio::time::sleep(Duration::from_millis(20)).await;
    short
        .search(&movie("X", Some("tt1")), &context())
        .await
        .expect("rows");
    assert_eq!(
        server.count("/stream/movie/tt1.json"),
        3,
        "the short-lived cache asked again"
    );
}

#[tokio::test]
async fn it_is_skipped_without_an_imdb_id_and_for_a_whole_show() {
    let source = TorrentioSource::new(fast_client(), "http://127.0.0.1:9");
    assert_eq!(
        source.supports(&movie("X", None)),
        Support::Skip(SkipReason::NoImdbId)
    );
    assert_eq!(
        source.supports(&show("X", Some("tt1"), Scope::Show)),
        Support::Skip(SkipReason::ShowScope)
    );
    assert_eq!(source.supports(&movie("X", Some("tt1"))), Support::Yes);
    assert_eq!(
        source.supports(&show("X", Some("tt1"), Scope::Episode(1, 1))),
        Support::Yes
    );
    assert_eq!(
        source.supports(&show("X", Some("tt1"), Scope::Season(1))),
        Support::Yes
    );
}

#[tokio::test]
async fn failures_are_reported_with_their_reason() {
    let server = FakeServer::start().await;
    server.route("/stream/movie/tt429.json", Reply::too_many_requests());
    server.route("/stream/movie/tt403.json", Reply::challenge_403());
    server.route("/stream/movie/tt500.json", Reply::server_error());
    let source = TorrentioSource::new(fast_client(), &server.base());
    let run = |imdb: &'static str| {
        let source = &source;
        async move { source.search(&movie("X", Some(imdb)), &context()).await }
    };
    assert_eq!(run("tt429").await.unwrap_err(), SourceError::RateLimited);
    assert_eq!(run("tt403").await.unwrap_err(), SourceError::Blocked);
    assert!(matches!(
        run("tt500").await.unwrap_err(),
        SourceError::Unreachable(_)
    ));
}
