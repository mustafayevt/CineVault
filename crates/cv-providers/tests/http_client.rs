//! The shared HTTP client against a local mock server: rate limits and concurrency, 429
//! handling, the disk cache with forced TTLs, request coalescing, cache bypass and secret
//! redaction. Nothing here touches the network.

mod support;

use std::future::Future;
use std::io;
use std::path::Path;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cv_providers::ProviderError;
use cv_providers::http::{
    CachePolicy, HttpClient, HttpClientBuilder, HttpRequest, Pause, ProviderLimits, RetryPolicy,
    ThrottleEvent,
};
use support::{MockServer, Reply};

/// Records the waits the client asks for instead of sleeping.
#[derive(Default)]
struct RecordedPauses(Mutex<Vec<Duration>>);

impl RecordedPauses {
    fn waits(&self) -> Vec<Duration> {
        self.0.lock().unwrap().clone()
    }
}

impl Pause for RecordedPauses {
    fn pause(&self, duration: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        self.0.lock().unwrap().push(duration);
        Box::pin(async {})
    }
}

fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

fn builder_with_pauses() -> (HttpClientBuilder, Arc<RecordedPauses>) {
    let pauses = Arc::new(RecordedPauses::default());
    let builder = HttpClient::builder().pause(pauses.clone());
    (builder, pauses)
}

/// Every byte of every file below `dir`.
fn read_tree(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push((
                    path.display().to_string(),
                    std::fs::read(&path).unwrap_or_default(),
                ));
            }
        }
    }
    out
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle.as_bytes())
}

// ---------------------------------------------------------------- basic requests

#[tokio::test]
async fn sends_query_headers_and_credentials_and_returns_the_body() {
    let server = MockServer::start().await;
    server.route("/3/search", Reply::ok(r#"{"results":[1,2]}"#));
    let client = HttpClient::builder().build().unwrap();

    let response = client
        .send(
            HttpRequest::get("tmdb", &server.url("/3/search"))
                .query("query", "heat")
                .header("Accept-Language", "de-DE")
                .credential_query("api_key", "test-api-key-1")
                .credential_header("Authorization", "Bearer test-token-2"),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    assert!(response.is_success());
    assert!(!response.from_cache());
    assert_eq!(response.content_type(), Some("application/json"));
    let parsed: serde_json::Value = response.json().unwrap();
    assert_eq!(parsed["results"][1], 2);

    let seen = &server.requests()[0];
    assert!(seen.target.contains("query=heat"), "{}", seen.target);
    assert!(seen.target.contains("api_key=test-api-key-1"));
    assert_eq!(seen.headers["accept-language"], "de-DE");
    assert_eq!(seen.headers["authorization"], "Bearer test-token-2");
    assert!(seen.headers["user-agent"].starts_with("CineVault/"));
}

#[tokio::test]
async fn non_success_statuses_are_returned_not_raised() {
    let server = MockServer::start().await;
    server.route("/missing", Reply::status(404));
    server.route("/denied", Reply::status(401));
    let client = HttpClient::builder().build().unwrap();

    let missing = client
        .send(HttpRequest::get("tmdb", &server.url("/missing")))
        .await
        .unwrap();
    assert_eq!(missing.status(), 404);
    assert!(!missing.is_success());
    let denied = client
        .send(HttpRequest::get("tmdb", &server.url("/denied")))
        .await
        .unwrap();
    assert_eq!(denied.status(), 401);
}

#[tokio::test]
async fn post_sends_its_body_and_is_never_cached() {
    let server = MockServer::start().await;
    server.route("/login", Reply::ok(r#"{"token":"x"}"#));
    let cache = tempfile::tempdir().unwrap();
    let client = HttpClient::builder()
        .cache_dir(cache.path())
        .build()
        .unwrap();

    for _ in 0..2 {
        client
            .send(HttpRequest::post(
                "tvdb",
                &server.url("/login"),
                b"{\"a\":1}".to_vec(),
            ))
            .await
            .unwrap();
    }
    assert_eq!(server.count("/login"), 2);
    assert_eq!(server.requests()[0].method, "POST");
    assert_eq!(server.requests()[0].body, b"{\"a\":1}");
    assert!(read_tree(cache.path()).is_empty());
}

// ---------------------------------------------------------------- rate limiter and concurrency

#[tokio::test]
async fn requests_are_spaced_by_the_provider_rate_limit() {
    let server = MockServer::start().await;
    server.route("/a", Reply::ok("1"));
    let client = HttpClient::builder()
        .provider_limits("anidb", ProviderLimits::one_per(Duration::from_millis(250)))
        .build()
        .unwrap();

    let start = Instant::now();
    for i in 0..3 {
        client
            .send(HttpRequest::get("anidb", &server.url("/a")).query("n", &i.to_string()))
            .await
            .unwrap();
    }
    assert!(
        start.elapsed() >= Duration::from_millis(480),
        "three requests at one per 250 ms took {:?}",
        start.elapsed()
    );
}

#[tokio::test]
async fn concurrency_per_provider_is_capped() {
    let server = MockServer::start().await;
    server.route("/slow", Reply::ok("x").delayed(Duration::from_millis(150)));
    let client = HttpClient::builder()
        .provider_limits("fanart", ProviderLimits::new(2, 1000, secs(1)))
        .build()
        .unwrap();

    let mut tasks = Vec::new();
    for i in 0..6 {
        let client = client.clone();
        let url = server.url("/slow");
        tasks.push(tokio::spawn(async move {
            client
                .send(HttpRequest::get("fanart", &url).query("n", &i.to_string()))
                .await
        }));
    }
    for task in tasks {
        task.await.unwrap().unwrap();
    }
    assert_eq!(server.count("/slow"), 6);
    assert_eq!(server.peak_concurrency(), 2);
}

#[tokio::test]
async fn a_busy_provider_does_not_delay_another() {
    let server = MockServer::start().await;
    // The slow provider's answers are held far longer than the test takes; they are never
    // waited for (the tasks are aborted at the end).
    server.route("/slow", Reply::ok("x").delayed(Duration::from_secs(60)));
    server.route("/fast", Reply::ok("y"));
    let client = HttpClient::builder()
        .provider_limits("slow", ProviderLimits::new(1, 1000, secs(1)))
        .build()
        .unwrap();

    let mut slow = Vec::new();
    for i in 0..2 {
        let client = client.clone();
        let url = server.url("/slow");
        slow.push(tokio::spawn(async move {
            client
                .send(HttpRequest::get("slow", &url).query("n", &i.to_string()))
                .await
        }));
    }
    // Wait for the event, not for a delay: the slow lane has its one request in flight.
    let deadline = Instant::now() + Duration::from_secs(60);
    while client.lane_status("slow").in_flight != 1 {
        assert!(Instant::now() < deadline, "the slow request never started");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    client
        .send(HttpRequest::get("fast", &server.url("/fast")))
        .await
        .unwrap();
    // The fast provider was answered while the slow one is still held.
    assert!(
        slow.iter().all(|task| !task.is_finished()),
        "the slow provider answered before the other one"
    );
    assert_eq!(client.lane_status("slow").in_flight, 1);
    for task in slow {
        task.abort();
    }
}

// ---------------------------------------------------------------- 429 handling

#[tokio::test]
async fn retries_after_the_retry_after_wait_and_marks_the_lane_throttled() {
    let server = MockServer::start().await;
    server.script("/art", vec![Reply::rate_limited(Some("2"))]);
    server.route("/art", Reply::ok("images"));
    server.route("/other", Reply::ok("fine"));
    let events = Arc::new(Mutex::new(Vec::<ThrottleEvent>::new()));
    let seen = Arc::clone(&events);
    let (builder, pauses) = builder_with_pauses();
    let client = builder
        .on_throttle(move |event| seen.lock().unwrap().push(event.clone()))
        .build()
        .unwrap();

    let response = client
        .send(HttpRequest::get("fanart", &server.url("/art")))
        .await
        .unwrap();

    assert_eq!(response.text(), "images");
    assert_eq!(server.count("/art"), 2);
    assert_eq!(pauses.waits(), vec![secs(2)]);
    let events = events.lock().unwrap().clone();
    assert_eq!(
        events,
        vec![ThrottleEvent {
            provider: "fanart".into(),
            wait: secs(2),
            retry: 1
        }]
    );
    // The drawer sees the lane as throttled; other providers are untouched.
    assert!(client.lane_status("fanart").throttled_for.is_some());
    assert_eq!(client.lane_status("tmdb").throttled_for, None);
    client
        .send(HttpRequest::get("tmdb", &server.url("/other")))
        .await
        .unwrap();
    assert_eq!(pauses.waits(), vec![secs(2)], "tmdb did not wait");
}

#[tokio::test]
async fn waits_for_real_when_the_server_asks_for_one_second() {
    let server = MockServer::start().await;
    server.script("/x", vec![Reply::rate_limited(Some("1"))]);
    server.route("/x", Reply::ok("done"));
    let client = HttpClient::builder().build().unwrap();

    let start = Instant::now();
    let response = client
        .send(HttpRequest::get("fanart", &server.url("/x")))
        .await
        .unwrap();
    assert_eq!(response.text(), "done");
    assert!(
        start.elapsed() >= Duration::from_millis(950),
        "{:?}",
        start.elapsed()
    );
}

#[tokio::test]
async fn gives_up_after_three_retries_with_a_rate_limited_error() {
    let server = MockServer::start().await;
    server.route("/x", Reply::rate_limited(Some("2")));
    let (builder, pauses) = builder_with_pauses();
    let client = builder
        .provider_limits("fanart", ProviderLimits::default().named("Fanart.tv"))
        .build()
        .unwrap();

    let err = client
        .send(HttpRequest::get("fanart", &server.url("/x")))
        .await
        .unwrap_err();

    assert_eq!(
        err,
        ProviderError::RateLimited {
            provider: "Fanart.tv".into(),
            retry_after_secs: 2
        }
    );
    assert_eq!(server.count("/x"), 4, "the first try and three retries");
    assert_eq!(pauses.waits(), vec![secs(2), secs(2), secs(2)]);
}

#[tokio::test]
async fn a_long_retry_after_is_capped_at_thirty_seconds() {
    let server = MockServer::start().await;
    server.script("/x", vec![Reply::rate_limited(Some("600"))]);
    server.route("/x", Reply::ok("ok"));
    let (builder, pauses) = builder_with_pauses();
    let client = builder.build().unwrap();

    client
        .send(HttpRequest::get("fanart", &server.url("/x")))
        .await
        .unwrap();
    assert_eq!(pauses.waits(), vec![secs(30)]);
}

#[tokio::test]
async fn without_retry_after_the_wait_backs_off() {
    let server = MockServer::start().await;
    server.route("/x", Reply::rate_limited(None));
    let (builder, pauses) = builder_with_pauses();
    let client = builder.build().unwrap();

    let err = client
        .send(HttpRequest::get("fanart", &server.url("/x")))
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::RateLimited { .. }));
    assert_eq!(pauses.waits(), vec![secs(1), secs(2), secs(4)]);
}

#[tokio::test]
async fn the_retry_policy_is_configurable() {
    let server = MockServer::start().await;
    server.route("/x", Reply::rate_limited(Some("5")));
    let (builder, pauses) = builder_with_pauses();
    let client = builder
        .retry_policy(RetryPolicy {
            max_retries: 1,
            max_wait: secs(3),
            ..RetryPolicy::default()
        })
        .build()
        .unwrap();
    client
        .send(HttpRequest::get("fanart", &server.url("/x")))
        .await
        .unwrap_err();
    assert_eq!(server.count("/x"), 2);
    assert_eq!(pauses.waits(), vec![secs(3)]);
}

#[tokio::test]
async fn rate_limited_responses_are_not_cached() {
    let server = MockServer::start().await;
    server.script("/x", vec![Reply::rate_limited(Some("1"))]);
    server.route("/x", Reply::ok("fresh"));
    let cache = tempfile::tempdir().unwrap();
    let (builder, _) = builder_with_pauses();
    let client = builder.cache_dir(cache.path()).build().unwrap();

    let first = client
        .send(HttpRequest::get("fanart", &server.url("/x")))
        .await
        .unwrap();
    assert_eq!(first.text(), "fresh");
    let second = client
        .send(HttpRequest::get("fanart", &server.url("/x")))
        .await
        .unwrap();
    assert!(second.from_cache());
    assert_eq!(second.text(), "fresh");
}

// ---------------------------------------------------------------- disk cache

#[tokio::test]
async fn a_repeat_request_within_the_ttl_makes_no_network_call() {
    let server = MockServer::start().await;
    server.route("/movie/603", Reply::ok(r#"{"title":"The Matrix"}"#));
    let cache = tempfile::tempdir().unwrap();
    let client = HttpClient::builder()
        .cache_dir(cache.path())
        .build()
        .unwrap();
    let request = || {
        HttpRequest::get("tmdb", &server.url("/movie/603"))
            .credential_query("api_key", "key-one-111")
    };

    let first = client.send(request()).await.unwrap();
    assert!(!first.from_cache());
    let second = client.send(request()).await.unwrap();
    assert!(second.from_cache());
    assert_eq!(second.body(), first.body());
    assert_eq!(second.content_type(), Some("application/json"));
    assert_eq!(server.count("/movie/603"), 1);

    // A different key for the same request still hits: credentials are not part of identity.
    let rekeyed = client
        .send(
            HttpRequest::get("tmdb", &server.url("/movie/603"))
                .credential_query("api_key", "key-two-222"),
        )
        .await
        .unwrap();
    assert!(rekeyed.from_cache());
    assert_eq!(server.count("/movie/603"), 1);

    // A different query is a different entry.
    client
        .send(request().query("language", "de"))
        .await
        .unwrap();
    assert_eq!(server.count("/movie/603"), 2);
}

#[tokio::test]
async fn the_cache_survives_a_new_client() {
    let server = MockServer::start().await;
    server.route("/x", Reply::ok("kept"));
    let cache = tempfile::tempdir().unwrap();
    for expect_cached in [false, true] {
        let client = HttpClient::builder()
            .cache_dir(cache.path())
            .build()
            .unwrap();
        let response = client
            .send(HttpRequest::get("tmdb", &server.url("/x")))
            .await
            .unwrap();
        assert_eq!(response.from_cache(), expect_cached);
    }
    assert_eq!(server.count("/x"), 1);
}

#[tokio::test]
async fn ttls_are_forced_per_kind_whatever_the_server_says() {
    let server = MockServer::start().await;
    let mut api = Reply::ok("api");
    api.headers
        .push(("Cache-Control".into(), "max-age=86400".into()));
    server.route("/api", api);
    let mut page = Reply::ok("<html>");
    page.headers
        .push(("Cache-Control".into(), "no-store".into()));
    server.route("/page", page);
    let cache = tempfile::tempdir().unwrap();
    let client = HttpClient::builder()
        .cache_dir(cache.path())
        .cache_policy(CachePolicy {
            api_ttl: Duration::from_millis(300),
            html_ttl: secs(3600),
        })
        .build()
        .unwrap();
    let api_request = || HttpRequest::get("tmdb", &server.url("/api"));
    let page_request = || HttpRequest::get("imdb", &server.url("/page")).html();

    client.send(api_request()).await.unwrap();
    client.send(page_request()).await.unwrap();
    tokio::time::sleep(Duration::from_millis(450)).await;

    let api_again = client.send(api_request()).await.unwrap();
    assert!(!api_again.from_cache(), "api entry outlived its forced TTL");
    let page_again = client.send(page_request()).await.unwrap();
    assert!(
        page_again.from_cache(),
        "server no-store does not change the HTML TTL"
    );
    assert_eq!(server.count("/api"), 2);
    assert_eq!(server.count("/page"), 1);
}

#[tokio::test]
async fn the_ttl_can_be_changed_at_run_time() {
    let server = MockServer::start().await;
    server.route("/x", Reply::ok("v"));
    let cache = tempfile::tempdir().unwrap();
    let client = HttpClient::builder()
        .cache_dir(cache.path())
        .build()
        .unwrap();
    assert_eq!(client.cache_policy(), CachePolicy::default());
    assert_eq!(client.cache_policy().api_ttl, secs(15 * 60));
    assert_eq!(client.cache_policy().html_ttl, secs(24 * 3600));

    client
        .send(HttpRequest::get("tmdb", &server.url("/x")))
        .await
        .unwrap();
    client.set_cache_policy(CachePolicy {
        api_ttl: Duration::ZERO,
        ..CachePolicy::default()
    });
    let response = client
        .send(HttpRequest::get("tmdb", &server.url("/x")))
        .await
        .unwrap();
    assert!(!response.from_cache());
    assert_eq!(server.count("/x"), 2);
}

#[tokio::test]
async fn bypass_skips_the_cache_read_but_refreshes_the_entry() {
    let server = MockServer::start().await;
    server.script("/x", vec![Reply::ok("old")]);
    server.route("/x", Reply::ok("new"));
    let cache = tempfile::tempdir().unwrap();
    let client = HttpClient::builder()
        .cache_dir(cache.path())
        .build()
        .unwrap();
    let plain = || HttpRequest::get("tmdb", &server.url("/x"));

    assert_eq!(client.send(plain()).await.unwrap().text(), "old");
    assert_eq!(client.send(plain()).await.unwrap().text(), "old");
    let forced = client.send(plain().bypass_cache(true)).await.unwrap();
    assert!(!forced.from_cache());
    assert_eq!(forced.text(), "new");
    assert_eq!(server.count("/x"), 2);

    let after = client.send(plain()).await.unwrap();
    assert!(after.from_cache());
    assert_eq!(after.text(), "new", "the forced fetch refreshed the cache");
    assert_eq!(server.count("/x"), 2);
}

#[tokio::test]
async fn uncached_requests_and_failures_are_not_stored() {
    let server = MockServer::start().await;
    server.script("/flaky", vec![Reply::status(500)]);
    server.route("/flaky", Reply::ok("recovered"));
    server.route("/live", Reply::ok("live"));
    let cache = tempfile::tempdir().unwrap();
    let client = HttpClient::builder()
        .cache_dir(cache.path())
        .build()
        .unwrap();

    let first = client
        .send(HttpRequest::get("tmdb", &server.url("/flaky")))
        .await
        .unwrap();
    assert_eq!(first.status(), 500);
    let second = client
        .send(HttpRequest::get("tmdb", &server.url("/flaky")))
        .await
        .unwrap();
    assert_eq!(second.text(), "recovered");
    assert_eq!(server.count("/flaky"), 2);

    for _ in 0..2 {
        client
            .send(HttpRequest::get("tmdb", &server.url("/live")).uncached())
            .await
            .unwrap();
    }
    assert_eq!(server.count("/live"), 2);
}

#[tokio::test]
async fn expired_entries_can_be_purged_and_the_cache_cleared() {
    let server = MockServer::start().await;
    server.route("/a", Reply::ok("a"));
    server.route("/b", Reply::ok("b"));
    let cache = tempfile::tempdir().unwrap();
    let client = HttpClient::builder()
        .cache_dir(cache.path())
        .cache_policy(CachePolicy {
            api_ttl: Duration::from_millis(200),
            html_ttl: secs(3600),
        })
        .build()
        .unwrap();
    client
        .send(HttpRequest::get("tmdb", &server.url("/a")))
        .await
        .unwrap();
    client
        .send(HttpRequest::get("imdb", &server.url("/b")).html())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;

    assert_eq!(client.purge_expired_cache().await, 1);
    assert_eq!(read_tree(cache.path()).len(), 1);
    client.cache().unwrap().clear(Some("imdb")).await.unwrap();
    assert!(read_tree(cache.path()).is_empty());
}

#[tokio::test]
async fn a_client_without_a_cache_dir_never_caches() {
    let server = MockServer::start().await;
    server.route("/x", Reply::ok("v"));
    let client = HttpClient::builder().build().unwrap();
    for _ in 0..2 {
        client
            .send(HttpRequest::get("tmdb", &server.url("/x")))
            .await
            .unwrap();
    }
    assert_eq!(server.count("/x"), 2);
}

// ---------------------------------------------------------------- coalescing

#[tokio::test]
async fn identical_concurrent_requests_make_one_network_call() {
    let server = MockServer::start().await;
    server.route(
        "/movie",
        Reply::ok("shared").delayed(Duration::from_secs(1)),
    );
    let client = HttpClient::builder().build().unwrap();

    let mut tasks = Vec::new();
    for _ in 0..8 {
        let client = client.clone();
        let url = server.url("/movie");
        tasks.push(tokio::spawn(async move {
            client
                .send(HttpRequest::get("tmdb", &url).credential_query("api_key", "coalesce-key-1"))
                .await
        }));
    }
    for task in tasks {
        assert_eq!(task.await.unwrap().unwrap().text(), "shared");
    }
    assert_eq!(server.count("/movie"), 1);
}

#[tokio::test]
async fn different_requests_are_not_coalesced() {
    let server = MockServer::start().await;
    server.route("/movie", Reply::ok("m").delayed(Duration::from_millis(100)));
    let client = HttpClient::builder().build().unwrap();

    let mut tasks = Vec::new();
    for id in 0..4 {
        let client = client.clone();
        let url = server.url("/movie");
        tasks.push(tokio::spawn(async move {
            client
                .send(HttpRequest::get("tmdb", &url).query("id", &id.to_string()))
                .await
        }));
    }
    for task in tasks {
        task.await.unwrap().unwrap();
    }
    assert_eq!(server.count("/movie"), 4);
}

#[tokio::test]
async fn coalesced_callers_share_a_failure_and_later_calls_retry() {
    let server = MockServer::start().await;
    server.script(
        "/x",
        vec![Reply::status(500).delayed(Duration::from_millis(600))],
    );
    server.route("/x", Reply::ok("back"));
    let client = HttpClient::builder().build().unwrap();

    let mut tasks = Vec::new();
    for _ in 0..3 {
        let client = client.clone();
        let url = server.url("/x");
        tasks.push(tokio::spawn(async move {
            client.send(HttpRequest::get("tmdb", &url)).await
        }));
    }
    for task in tasks {
        assert_eq!(task.await.unwrap().unwrap().status(), 500);
    }
    assert_eq!(server.count("/x"), 1);

    let later = client
        .send(HttpRequest::get("tmdb", &server.url("/x")))
        .await
        .unwrap();
    assert_eq!(later.text(), "back");
}

// ---------------------------------------------------------------- redaction

const QUERY_KEY: &str = "query-secret-key-1111";
const HEADER_TOKEN: &str = "header-secret-token-2222";
const PATH_KEY: &str = "path-secret-key-3333";
const SECOND_KEY: &str = "second-secret-key-4444";

#[derive(Clone, Default)]
struct LogBuffer(Arc<Mutex<Vec<u8>>>);

impl io::Write for LogBuffer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl LogBuffer {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }
}

fn capture_logs() -> (LogBuffer, tracing::subscriber::DefaultGuard) {
    let buffer = LogBuffer::default();
    let writer = buffer.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    (buffer, tracing::subscriber::set_default(subscriber))
}

fn secret_request(url: &str) -> HttpRequest {
    HttpRequest::get("tmdb", url)
        .credential_query("api_key", QUERY_KEY)
        .credential_header("Authorization", &format!("Bearer {HEADER_TOKEN}"))
        .secret(PATH_KEY)
}

#[tokio::test]
async fn secrets_never_reach_logs_cache_files_or_debug_output() {
    let server = MockServer::start().await;
    let path = format!("/v1/{PATH_KEY}/items");
    server.script(&path, vec![Reply::rate_limited(Some("1"))]);
    server.route(&path, Reply::ok(r#"{"ok":true}"#));
    let cache = tempfile::tempdir().unwrap();
    let (logs, _guard) = capture_logs();
    let (builder, _) = builder_with_pauses();
    let client = builder.cache_dir(cache.path()).build().unwrap();

    let request = secret_request(&server.url(&path));
    let debug_before_send = format!("{request:?}");
    let first = client.send(request.clone()).await.unwrap();
    assert_eq!(first.status(), 200);
    let second = client.send(request.clone()).await.unwrap();
    assert!(second.from_cache());

    // The server did receive the real values.
    let seen = &server.requests()[0];
    assert!(seen.target.contains(QUERY_KEY));
    assert_eq!(
        seen.headers["authorization"],
        format!("Bearer {HEADER_TOKEN}")
    );

    let log_text = logs.text();
    assert!(!log_text.is_empty(), "the client should log at debug level");
    assert!(log_text.contains("[REDACTED]"), "{log_text}");
    let files = read_tree(cache.path());
    assert!(!files.is_empty());
    for secret in [QUERY_KEY, HEADER_TOKEN, PATH_KEY] {
        assert!(
            !log_text.contains(secret),
            "{secret} leaked into the logs:\n{log_text}"
        );
        assert!(
            !debug_before_send.contains(secret),
            "{secret} leaked into Debug"
        );
        for (name, bytes) in &files {
            assert!(
                !contains(bytes, secret),
                "{secret} leaked into cache file {name}"
            );
        }
    }
    for (name, _) in &files {
        assert!(
            !name.contains(PATH_KEY),
            "cache file names must not carry secrets: {name}"
        );
    }
}

#[tokio::test]
async fn transport_errors_do_not_echo_credentials() {
    // A port nobody listens on.
    let closed = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap()
    };
    let (logs, _guard) = capture_logs();
    let client = HttpClient::builder().build().unwrap();
    let url = format!("http://{closed}/v1/{SECOND_KEY}/items");

    let err = client
        .send(
            HttpRequest::get("tmdb", &url)
                .credential_query("api_key", QUERY_KEY)
                .secret(SECOND_KEY),
        )
        .await
        .unwrap_err();

    let shown = format!("{err} / {err:?}");
    assert!(matches!(err, ProviderError::Request { .. }), "{shown}");
    for secret in [QUERY_KEY, SECOND_KEY] {
        assert!(
            !shown.contains(secret),
            "{secret} leaked into the error: {shown}"
        );
        assert!(!logs.text().contains(secret));
    }
}

#[tokio::test]
async fn invalid_urls_do_not_echo_credentials() {
    let client = HttpClient::builder().build().unwrap();
    let err = client
        .send(HttpRequest::get(
            "tmdb",
            &format!("::not a url:: api_key={QUERY_KEY}"),
        ))
        .await
        .unwrap_err();
    assert!(!format!("{err}").contains(QUERY_KEY));
}

#[tokio::test]
async fn a_key_in_the_url_is_masked() {
    // Keys reach the client as ordinary credential values from the resolver, so the same
    // masking covers them, including when the provider puts one in a hand-built URL parameter.
    let server = MockServer::start().await;
    server.route("/x", Reply::rate_limited(Some("1")));
    let (logs, _guard) = capture_logs();
    let (builder, _) = builder_with_pauses();
    let client = builder.build().unwrap();

    let url = server.url(&format!("/x?apikey={SECOND_KEY}&q=1"));
    let err = client
        .send(HttpRequest::get("fanart", &url).secret(SECOND_KEY))
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::RateLimited { .. }));
    let log_text = logs.text();
    assert!(!log_text.contains(SECOND_KEY), "{log_text}");
    assert!(
        log_text.contains("q=1"),
        "non-secret parts stay readable: {log_text}"
    );
}

#[tokio::test]
async fn the_application_redactor_is_applied_to_log_output() {
    let server = MockServer::start().await;
    let path = "/v1/registered-elsewhere-5555/items";
    server.route(path, Reply::ok("{}"));
    let (logs, _guard) = capture_logs();
    let client = HttpClient::builder()
        .redactor(Arc::new(|text: &str| {
            text.replace("registered-elsewhere-5555", "[APP-REDACTED]")
        }))
        .build()
        .unwrap();

    client
        .send(HttpRequest::get("tmdb", &server.url(path)))
        .await
        .unwrap();
    let log_text = logs.text();
    assert!(
        !log_text.contains("registered-elsewhere-5555"),
        "{log_text}"
    );
    assert!(log_text.contains("[APP-REDACTED]"), "{log_text}");
}

// ---------------------------------------------------------------- answer size limit

#[tokio::test]
async fn an_answer_past_the_body_limit_fails_and_a_smaller_one_passes() {
    let server = MockServer::start().await;
    server.route("/small", Reply::ok(&"x".repeat(1000)));
    server.route("/big", Reply::ok(&"x".repeat(5000)));
    let client = HttpClient::builder().max_body_bytes(2000).build().unwrap();

    let small = client
        .send(HttpRequest::get("p", &server.url("/small")).uncached())
        .await
        .unwrap();
    assert_eq!(small.body().len(), 1000);

    let error = client
        .send(HttpRequest::get("p", &server.url("/big")).uncached())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("too large"), "{error}");
}

#[tokio::test]
async fn the_default_body_limit_reads_ordinary_answers_whole() {
    let server = MockServer::start().await;
    server.route("/big", Reply::ok(&"x".repeat(5000)));
    let client = HttpClient::builder().build().unwrap();
    let big = client
        .send(HttpRequest::get("p", &server.url("/big")).uncached())
        .await
        .unwrap();
    assert_eq!(big.body().len(), 5000);
}

#[tokio::test]
async fn the_body_limit_counts_the_decompressed_size() {
    use std::io::Write as _;

    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    encoder.write_all(&vec![b'x'; 50_000]).unwrap();
    let compressed = encoder.finish().unwrap();
    assert!(compressed.len() < 2000, "the bomb is small on the wire");
    let server = MockServer::start().await;
    let mut reply = Reply::ok("");
    reply.body = compressed;
    reply
        .headers
        .push(("Content-Encoding".into(), "gzip".into()));
    server.route("/bomb", reply);
    let client = HttpClient::builder().max_body_bytes(2000).build().unwrap();
    let error = client
        .send(HttpRequest::get("p", &server.url("/bomb")).uncached())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("too large"), "{error}");
}

#[tokio::test]
async fn a_request_can_set_its_own_body_limit() {
    let server = MockServer::start().await;
    server.route("/big", Reply::ok(&"x".repeat(5000)));
    let client = HttpClient::builder().max_body_bytes(2000).build().unwrap();
    let big = client
        .send(
            HttpRequest::get("p", &server.url("/big"))
                .uncached()
                .max_body_bytes(10_000),
        )
        .await
        .unwrap();
    assert_eq!(big.body().len(), 5000);
    let error = client
        .send(
            HttpRequest::get("p", &server.url("/big"))
                .uncached()
                .max_body_bytes(100),
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("too large"), "{error}");
}

#[test]
fn the_default_body_limit_is_sixteen_megabytes() {
    assert_eq!(cv_providers::http::DEFAULT_MAX_BODY_BYTES, 16 * 1024 * 1024);
}

// ---------------------------------------------------------------- per-request timeout

#[tokio::test]
async fn a_request_may_wait_longer_or_shorter_than_the_client_timeout() {
    let server = MockServer::start().await;
    server.route(
        "/slow",
        Reply::ok("done").delayed(Duration::from_millis(600)),
    );
    let client = HttpClient::builder()
        .timeout(Duration::from_millis(200))
        .build()
        .unwrap();

    let error = client
        .send(HttpRequest::get("p", &server.url("/slow")).uncached())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("timed out"), "{error}");

    let answer = client
        .send(
            HttpRequest::get("p", &server.url("/slow"))
                .uncached()
                .timeout(Duration::from_secs(5)),
        )
        .await
        .unwrap();
    assert_eq!(answer.body(), b"done");
}
