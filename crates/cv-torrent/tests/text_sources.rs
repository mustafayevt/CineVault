//! The Pirate Bay (apibay) and Knaben sources against recorded answers: the text queries go one
//! after another, rows arrive as partial results after each, and failures keep their reason.

mod support;

use std::sync::{Arc, Mutex};

use cv_torrent::CancelToken;
use cv_torrent::model::{MediaKind, Scope, SourceError, Support};
use cv_torrent::source::{SearchContext, TorrentSource};
use cv_torrent::sources::apibay::ApibaySource;
use cv_torrent::sources::knaben::KnabenSource;
use support::fake_server::{FakeServer, Reply};
use support::fast_client;
use support::queries::{movie, query, show};

fn context() -> SearchContext {
    SearchContext::quiet(CancelToken::new())
}

fn watching() -> (SearchContext, Arc<Mutex<Vec<usize>>>) {
    let seen: Arc<Mutex<Vec<usize>>> = Arc::default();
    let sink = Arc::clone(&seen);
    let context = SearchContext::with_partial(
        CancelToken::new(),
        Arc::new(move |rows| sink.lock().unwrap().push(rows.len())),
    );
    (context, seen)
}

#[tokio::test]
async fn apibay_reads_a_recorded_answer_and_sends_the_movie_categories() {
    let server = FakeServer::start().await;
    server.route("/q.php", Reply::json_fixture("apibay/search.json"));
    let source = ApibaySource::new(fast_client(), &server.base());
    let mut q = movie("Night of the Living Dead", Some("tt0063350"));
    q.year = Some(1968);
    let (ctx, partials) = watching();
    let rows = source.search(&q, &ctx).await.expect("rows");

    // One query ("Night of the Living Dead 1968") answered with the fixture's rows.
    let sent = server.requests();
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0].query("q").as_deref(),
        Some("Night of the Living Dead 1968")
    );
    assert_eq!(sent[0].query("cat").as_deref(), Some("201,207,211"));
    assert!(!rows.is_empty());
    assert!(rows.iter().all(|r| {
        r.info_hash
            .as_deref()
            .is_some_and(|h| h.len() == 40 && h == h.to_lowercase())
    }));
    assert!(
        rows.iter()
            .all(|r| r.source.as_ref().is_some_and(|s| s.0 == "apibay"))
    );
    assert_eq!(*partials.lock().unwrap(), vec![rows.len()]);
    assert!(rows.iter().any(|r| r.imdb.as_deref() == Some("tt0063350")));
    // The first recorded row is torrent 25550850.
    assert_eq!(
        rows[0].details_url.as_deref(),
        Some("https://thepiratebay.org/description.php?id=25550850")
    );
    assert!(rows.iter().all(|r| r.details_url.is_some()));
}

#[tokio::test]
async fn apibay_uses_tv_categories_for_a_show() {
    let server = FakeServer::start().await;
    server.route("/q.php", Reply::json_fixture("apibay/empty.json"));
    let source = ApibaySource::new(fast_client(), &server.base());
    let q = query(MediaKind::Show, "Severance", None, Scope::Episode(1, 2));
    let rows = source.search(&q, &context()).await.expect("rows");
    assert!(rows.is_empty(), "the no-results row is not a release");
    let sent = server.requests();
    assert_eq!(sent[0].query("cat").as_deref(), Some("205,208,212"));
    assert_eq!(sent[0].query("q").as_deref(), Some("Severance S01E02"));
    assert_eq!(sent[1].query("q").as_deref(), Some("Severance S01"));
}

#[tokio::test]
async fn apibay_runs_each_spelling_in_order_with_partials() {
    let server = FakeServer::start().await;
    server.route("/q.php", Reply::json_fixture("apibay/search.json"));
    let source = ApibaySource::new(fast_client(), &server.base());
    let mut q = movie("Widow's Bay", None);
    q.year = Some(2019);
    let (ctx, partials) = watching();
    let rows = source.search(&q, &ctx).await.expect("rows");
    let asked: Vec<_> = server
        .requests()
        .iter()
        .map(|r| r.query("q").unwrap())
        .collect();
    assert_eq!(
        asked,
        vec!["Widow's Bay 2019", "Widows Bay 2019", "Widow s Bay 2019"]
    );
    let partials = partials.lock().unwrap().clone();
    assert_eq!(partials.len(), 3, "one batch per spelling");
    assert_eq!(rows.len(), partials.iter().sum::<usize>());
}

#[tokio::test]
async fn a_later_failure_keeps_the_rows_found_and_a_first_failure_is_reported() {
    let server = FakeServer::start().await;
    server.script(
        "/q.php",
        vec![
            Reply::json_fixture("apibay/search.json"),
            Reply::too_many_requests(),
        ],
    );
    let source = ApibaySource::new(fast_client(), &server.base());
    let mut q = movie("Widow's Bay", None);
    q.year = Some(2019);
    let rows = source
        .search(&q, &context())
        .await
        .expect("the first answer is kept");
    assert!(!rows.is_empty());
    assert_eq!(server.requests().len(), 2, "stopped at the failing query");

    let failing = FakeServer::start().await;
    failing.route("/q.php", Reply::too_many_requests());
    let source = ApibaySource::new(fast_client(), &failing.base());
    assert_eq!(
        source.search(&q, &context()).await.unwrap_err(),
        SourceError::RateLimited
    );
    assert_eq!(failing.requests().len(), 1);
}

#[tokio::test]
async fn an_html_page_is_blocked_and_a_cancelled_search_asks_nothing() {
    let server = FakeServer::start().await;
    server.route("/q.php", Reply::challenge_page());
    let source = ApibaySource::new(fast_client(), &server.base());
    assert_eq!(
        source
            .search(&movie("X", None), &context())
            .await
            .unwrap_err(),
        SourceError::Blocked
    );

    let cancelled = CancelToken::new();
    cancelled.cancel();
    let before = server.requests().len();
    let rows = source
        .search(&movie("X", None), &SearchContext::quiet(cancelled))
        .await
        .expect("no error");
    assert!(rows.is_empty());
    assert_eq!(server.requests().len(), before);
}

#[tokio::test]
async fn knaben_reads_the_recorded_v2_answer() {
    let server = FakeServer::start().await;
    server.route("/v2/search", Reply::json_fixture("knaben/search.json"));
    let source = KnabenSource::new(fast_client(), &server.base());
    let mut q = movie("Night of the Living Dead", Some("tt0063350"));
    q.year = Some(1968);
    let rows = source.search(&q, &context()).await.expect("rows");
    assert_eq!(rows.len(), 18);

    let sent = &server.requests()[0];
    assert_eq!(
        sent.query("q").as_deref(),
        Some("Night of the Living Dead 1968")
    );
    assert_eq!(sent.query("sf").as_deref(), Some("title"));
    assert_eq!(sent.query("o").as_deref(), Some("seeders"));
    assert_eq!(sent.query("d").as_deref(), Some("desc"));
    assert_eq!(sent.query("s").as_deref(), Some("300"));

    let first = &rows[0];
    assert_eq!(
        first.details_url.as_deref(),
        Some("https://knaben.xyz/thepiratebay/description.php?id=25550850")
    );
    assert_eq!(
        first.info_hash.as_deref(),
        Some("ea7dcdfd19ae0be46e3bf52ef310225f7aa97855")
    );
    assert!(first.name.starts_with("Night.of.the.Living.Dead.1968"));
    assert!(
        first
            .magnet
            .as_deref()
            .is_some_and(|m| m.starts_with("magnet:?xt=urn:btih:"))
    );
    assert!(first.seeders.is_some() && first.size.is_some() && first.published.is_some());
}

#[tokio::test]
async fn knaben_empty_blocked_and_unreachable() {
    let server = FakeServer::start().await;
    server.route_query(
        "/v2/search",
        &[("q", "Empty")],
        Reply::json(r#"{"hits":[]}"#),
    );
    server.route_query("/v2/search", &[("q", "Html")], Reply::challenge_page());
    server.route_query("/v2/search", &[("q", "Down")], Reply::server_error());
    let source = KnabenSource::new(fast_client(), &server.base());
    assert!(
        source
            .search(&movie("Empty", None).with_no_year(), &context())
            .await
            .expect("rows")
            .is_empty()
    );
    assert_eq!(
        source
            .search(&movie("Html", None).with_no_year(), &context())
            .await
            .unwrap_err(),
        SourceError::Blocked
    );
    assert!(matches!(
        source
            .search(&movie("Down", None).with_no_year(), &context())
            .await
            .unwrap_err(),
        SourceError::Unreachable(_)
    ));
}

#[tokio::test]
async fn text_sources_apply_to_everything() {
    let a = ApibaySource::new(fast_client(), "http://127.0.0.1:9");
    let k = KnabenSource::new(fast_client(), "http://127.0.0.1:9");
    for q in [movie("X", None), show("X", None, Scope::Show)] {
        assert_eq!(a.supports(&q), Support::Yes);
        assert_eq!(k.supports(&q), Support::Yes);
    }
}

trait NoYear {
    fn with_no_year(self) -> Self;
}

impl NoYear for cv_torrent::model::TorrentQuery {
    fn with_no_year(mut self) -> Self {
        self.year = None;
        self
    }
}
