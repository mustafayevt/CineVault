//! `SourceHttp` against the fake server: every way a request can end maps to the error the page
//! shows, and nothing is retried behind the user's back.

mod support;

use std::time::{Duration, Instant};

use cv_providers::http::ProviderLimits;
use cv_torrent::SourceError;
use cv_torrent::source::{MAX_ANSWER_BYTES, SourceHttp, client_builder};
use serde::Deserialize;
use support::fake_server::{FakeServer, Reply};
use support::fast_client;

#[derive(Debug, Deserialize, PartialEq)]
struct Answer {
    name: String,
}

fn http() -> SourceHttp {
    SourceHttp::new(fast_client(), "test")
}

#[tokio::test]
async fn a_good_answer_is_read_and_the_request_carries_query_and_user_agent() {
    let server = FakeServer::start().await;
    server.route("/api", Reply::json(r#"{"name":"Widow's Bay"}"#));
    let answer: Answer = http()
        .get_json(
            &server.url("/api"),
            &[("q", "Widow's Bay 2019"), ("cat", "201,207")],
        )
        .await
        .expect("an answer");
    assert_eq!(answer.name, "Widow's Bay");

    let seen = &server.requests()[0];
    assert_eq!(seen.query("q").as_deref(), Some("Widow's Bay 2019"));
    assert_eq!(seen.query("cat").as_deref(), Some("201,207"));
    let agent = seen.headers.get("user-agent").expect("a User-Agent");
    assert!(agent.starts_with("CineVault/") && agent.contains("github.com/mustafayevt/CineVault"));
}

#[tokio::test]
async fn text_answers_are_returned_as_text() {
    let server = FakeServer::start().await;
    server.route("/feed", Reply::xml("<rss version=\"2.0\"></rss>"));
    let text = http()
        .get_text(&server.url("/feed"), &[])
        .await
        .expect("text");
    assert!(text.contains("<rss"));
}

#[tokio::test]
async fn http_429_is_rate_limited_and_is_not_retried() {
    let server = FakeServer::start().await;
    server.route("/api", Reply::too_many_requests());
    let started = Instant::now();
    let error = http()
        .get_json::<Answer>(&server.url("/api"), &[])
        .await
        .unwrap_err();
    assert_eq!(error, SourceError::RateLimited);
    assert_eq!(server.count("/api"), 1, "no automatic retry");
    assert!(
        started.elapsed() < Duration::from_millis(900),
        "no waiting for Retry-After"
    );
}

#[tokio::test]
async fn a_challenge_page_where_json_was_expected_is_blocked() {
    let server = FakeServer::start().await;
    server.route("/ok-status", Reply::challenge_page());
    server.route("/forbidden", Reply::challenge_403());
    for path in ["/ok-status", "/forbidden"] {
        let error = http()
            .get_json::<Answer>(&server.url(path), &[])
            .await
            .unwrap_err();
        assert_eq!(error, SourceError::Blocked, "{path}");
    }
    let error = http()
        .get_text(&server.url("/ok-status"), &[])
        .await
        .unwrap_err();
    assert_eq!(error, SourceError::Blocked, "text where data was expected");
}

#[tokio::test]
async fn server_errors_are_unreachable_and_missing_pages_are_bad_answers() {
    let server = FakeServer::start().await;
    server.route("/down", Reply::server_error());
    server.route("/bad-gateway", Reply::status(502));
    let error = http()
        .get_json::<Answer>(&server.url("/down"), &[])
        .await
        .unwrap_err();
    assert!(
        matches!(error, SourceError::Unreachable(ref m) if m.contains("500")),
        "{error:?}"
    );
    let error = http()
        .get_json::<Answer>(&server.url("/bad-gateway"), &[])
        .await
        .unwrap_err();
    assert!(matches!(error, SourceError::Unreachable(_)), "{error:?}");
    let error = http()
        .get_json::<Answer>(&server.url("/missing"), &[])
        .await
        .unwrap_err();
    assert!(
        matches!(error, SourceError::BadAnswer(ref m) if m.contains("404")),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_dropped_connection_and_a_closed_port_are_unreachable() {
    let server = FakeServer::start().await;
    server.route("/api", Reply::dropped());
    let error = http()
        .get_json::<Answer>(&server.url("/api"), &[])
        .await
        .unwrap_err();
    assert!(matches!(error, SourceError::Unreachable(_)), "{error:?}");

    let closed = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("address").port();
        drop(listener);
        format!("http://127.0.0.1:{port}/api")
    };
    let error = http().get_json::<Answer>(&closed, &[]).await.unwrap_err();
    assert!(matches!(error, SourceError::Unreachable(_)), "{error:?}");
}

#[tokio::test]
async fn an_answer_that_takes_too_long_is_a_timeout() {
    let server = FakeServer::start().await;
    server.route(
        "/slow",
        Reply::json(r#"{"name":"x"}"#).delayed(Duration::from_secs(5)),
    );
    let client = client_builder()
        .timeout(Duration::from_millis(300))
        .default_limits(ProviderLimits::new(8, 1000, Duration::from_secs(1)))
        .build()
        .expect("client");
    let started = Instant::now();
    let error = SourceHttp::new(client, "slow")
        .get_json::<Answer>(&server.url("/slow"), &[])
        .await
        .unwrap_err();
    assert_eq!(error, SourceError::Timeout);
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[tokio::test]
async fn broken_or_cut_off_json_is_a_bad_answer() {
    let server = FakeServer::start().await;
    server.route("/cut", Reply::json(r#"{"name":"abc"#));
    server.route("/wrong-shape", Reply::json(r#"{"title":"x"}"#));
    for path in ["/cut", "/wrong-shape"] {
        let error = http()
            .get_json::<Answer>(&server.url(path), &[])
            .await
            .unwrap_err();
        assert!(
            matches!(error, SourceError::BadAnswer(_)),
            "{path}: {error:?}"
        );
    }
}

#[tokio::test]
async fn a_body_cut_off_on_the_wire_is_an_error_not_a_panic() {
    let server = FakeServer::start().await;
    server.route(
        "/half",
        Reply::json(&format!(r#"{{"name":"{}"}}"#, "x".repeat(2000))).truncated(100),
    );
    let error = http()
        .get_json::<Answer>(&server.url("/half"), &[])
        .await
        .unwrap_err();
    assert!(
        matches!(
            error,
            SourceError::Unreachable(_) | SourceError::BadAnswer(_)
        ),
        "{error:?}"
    );
}

#[tokio::test]
async fn an_answer_past_the_size_limit_is_refused() {
    let server = FakeServer::start().await;
    let big = format!(r#"{{"name":"{}"}}"#, "x".repeat(MAX_ANSWER_BYTES + 1024));
    server.route("/big", Reply::json(&big));
    let error = http()
        .get_json::<Answer>(&server.url("/big"), &[])
        .await
        .unwrap_err();
    assert!(
        matches!(error, SourceError::BadAnswer(ref m) if m.contains("too large")),
        "{error:?}"
    );
}

#[tokio::test]
async fn an_unusable_address_is_a_bad_answer_that_does_not_echo_it() {
    let error = http()
        .get_json::<Answer>("not a url", &[])
        .await
        .unwrap_err();
    let SourceError::BadAnswer(message) = error else {
        panic!("expected a bad answer");
    };
    assert!(!message.contains("not a url"));
}

#[tokio::test]
async fn raw_answers_keep_their_status_for_sources_that_read_404_as_empty() {
    let server = FakeServer::start().await;
    server.route("/none", Reply::status(404));
    let response = http()
        .get_raw(&server.url("/none"), &[])
        .await
        .expect("an answer");
    assert_eq!(response.status, 404);
}

#[tokio::test]
async fn each_source_has_its_own_lane() {
    let server = FakeServer::start().await;
    server.route("/api", Reply::json(r#"{"name":"x"}"#));
    let a = SourceHttp::new(fast_client(), "yts");
    let b = SourceHttp::on_lane(fast_client(), cv_torrent::source::INDEXER_LANE);
    assert_eq!(a.lane(), "torrent:yts");
    assert_eq!(b.lane(), "torrent:indexer");
    a.get_json::<Answer>(&server.url("/api"), &[])
        .await
        .expect("a");
    b.get_json::<Answer>(&server.url("/api"), &[])
        .await
        .expect("b");
}
