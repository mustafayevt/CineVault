//! The orchestrator over real sources talking to the fake server: a fast source, a slow one
//! (30 seconds), a rate-limited one and one behind a challenge page.

mod support;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cv_torrent::CancelToken;
use cv_torrent::model::{SkipReason, SourceError, SourceId, SourceOutcome};
use cv_torrent::search::{Orchestrator, Outcome, ResultRow, Sink, Summary};
use cv_torrent::source::TorrentSource;
use cv_torrent::sources::apibay::ApibaySource;
use cv_torrent::sources::eztv::EztvSource;
use cv_torrent::sources::knaben::KnabenSource;
use cv_torrent::sources::torrentio::TorrentioSource;
use cv_torrent::sources::torznab::TorznabSource;
use cv_torrent::sources::yts::YtsSource;
use support::fake_server::{FakeServer, Reply};
use support::fast_client;
use support::queries::movie;

#[derive(Default)]
struct Timeline {
    events: Mutex<Vec<(Duration, String)>>,
    rows: Mutex<Vec<ResultRow>>,
    started_at: Mutex<Option<Instant>>,
}

impl Timeline {
    fn begin(&self) {
        *self.started_at.lock().unwrap() = Some(Instant::now());
    }

    fn note(&self, text: String) {
        let at = self
            .started_at
            .lock()
            .unwrap()
            .map_or(Duration::ZERO, |s| s.elapsed());
        self.events.lock().unwrap().push((at, text));
    }

    fn when(&self, text: &str) -> Option<Duration> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .find(|(_, e)| e == text)
            .map(|(at, _)| *at)
    }

    fn texts(&self) -> Vec<String> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .map(|(_, e)| e.clone())
            .collect()
    }
}

impl Sink for Timeline {
    fn started(&self, source: &SourceId) {
        self.note(format!("started {}", source.0));
    }
    fn partial(&self, source: &SourceId, rows: Vec<ResultRow>) {
        self.note(format!("partial {}", source.0));
        self.rows.lock().unwrap().extend(rows);
    }
    fn results(&self, source: &SourceId, rows: Vec<ResultRow>) {
        self.note(format!("results {}", source.0));
        self.rows.lock().unwrap().extend(rows);
    }
    fn failed(&self, source: &SourceId, error: &SourceError) {
        self.note(format!("failed {} {error}", source.0));
    }
    fn skipped(&self, source: &SourceId, reason: SkipReason) {
        self.note(format!("skipped {} {reason:?}", source.0));
    }
    fn done(&self, _summary: &Summary) {
        self.note("done".to_owned());
    }
}

struct World {
    _fast: FakeServer,
    _slow: FakeServer,
    _limited: FakeServer,
    _blocked: FakeServer,
    sources: Vec<Arc<dyn TorrentSource>>,
}

/// Torrentio answers at once, YTS takes 30 seconds, Knaben says 429, The Pirate Bay answers with
/// a challenge page, and EZTV (TV only) does not apply to a movie.
async fn world() -> World {
    let fast = FakeServer::start().await;
    fast.route(
        "/stream/movie/tt0063350.json",
        Reply::json_fixture("torrentio/movie.json"),
    );
    let slow = FakeServer::start().await;
    slow.route(
        "/api/v2/list_movies.json",
        Reply::json_fixture("yts/list_movies.json").delayed(Duration::from_secs(30)),
    );
    let limited = FakeServer::start().await;
    limited.route("/v2/search", Reply::too_many_requests());
    let blocked = FakeServer::start().await;
    blocked.route("/q.php", Reply::challenge_page());
    let sources: Vec<Arc<dyn TorrentSource>> = vec![
        Arc::new(TorrentioSource::new(fast_client(), &fast.base())),
        Arc::new(YtsSource::new(fast_client(), &slow.base())),
        Arc::new(KnabenSource::new(fast_client(), &limited.base())),
        Arc::new(ApibaySource::new(fast_client(), &blocked.base())),
        Arc::new(EztvSource::new(fast_client(), "http://127.0.0.1:9")),
    ];
    World {
        _fast: fast,
        _slow: slow,
        _limited: limited,
        _blocked: blocked,
        sources,
    }
}

fn query() -> cv_torrent::model::TorrentQuery {
    movie("Night of the Living Dead", Some("tt0063350"))
}

#[tokio::test]
async fn first_rows_arrive_long_before_the_slow_source_times_out() {
    let world = world().await;
    let timeline = Arc::new(Timeline::default());
    timeline.begin();
    let outcome = Orchestrator::new(world.sources.clone())
        .with_deadline(Duration::from_secs(2))
        .run(query(), timeline.clone(), &CancelToken::new())
        .await;
    let Outcome::Finished(summary) = outcome else {
        panic!("finished")
    };

    let first_rows = timeline
        .when("results torrentio")
        .expect("torrentio answered");
    let timed_out = timeline
        .when("failed yts timed out")
        .expect("yts timed out");
    assert!(first_rows < Duration::from_millis(1500), "{first_rows:?}");
    assert!(timed_out >= Duration::from_millis(1800), "{timed_out:?}");
    assert!(first_rows < timed_out);

    let texts = timeline.texts();
    assert!(
        texts.contains(&"failed knaben rate limited".to_owned()),
        "{texts:?}"
    );
    assert!(
        texts.contains(&"failed apibay blocked by a challenge page".to_owned()),
        "{texts:?}"
    );
    assert!(
        texts.contains(&"skipped eztv TvOnly".to_owned()),
        "{texts:?}"
    );
    assert_eq!(texts.last().map(String::as_str), Some("done"));

    assert_eq!(summary.answered(), 1);
    assert_eq!(summary.failed(), 3);
    assert!(
        summary.outcomes.iter().any(
            |(id, o)| id.0 == "eztv" && matches!(o, SourceOutcome::Skipped(SkipReason::TvOnly))
        )
    );
    // 45 streams, five of them the same torrent listed again under another tracker.
    assert_eq!(summary.total, 40);
    assert!(
        summary.matching > 0,
        "the 1968 releases match the title and year"
    );
}

#[tokio::test]
async fn cancelling_stops_every_request_within_a_second() {
    let world = world().await;
    let timeline = Arc::new(Timeline::default());
    let cancel = CancelToken::new();
    let orchestrator = Orchestrator::new(world.sources.clone());
    let handle = {
        let timeline = timeline.clone();
        let cancel = cancel.clone();
        tokio::spawn(async move { orchestrator.run(query(), timeline, &cancel).await })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    let cancelled_at = Instant::now();
    cancel.cancel();
    let outcome = handle.await.expect("joined");
    assert_eq!(outcome, Outcome::Cancelled);
    assert!(
        cancelled_at.elapsed() < Duration::from_secs(1),
        "{:?}",
        cancelled_at.elapsed()
    );
    let texts = timeline.texts();
    assert!(!texts.contains(&"done".to_owned()));
    assert!(
        !texts.iter().any(|t| t.starts_with("failed yts")),
        "the slow source was dropped, not timed out"
    );
}

#[tokio::test]
async fn the_default_deadline_is_twenty_five_seconds() {
    assert_eq!(cv_torrent::search::SOURCE_DEADLINE, Duration::from_secs(25));
}

#[tokio::test]
async fn the_users_own_instance_gets_longer_than_a_single_site() {
    // An all-indexers feed asks every site before it answers. With a 400 ms deadline a single
    // site that needs 900 ms times out; the user's own instance (four times the deadline) does not.
    let feed = FakeServer::start().await;
    feed.route_query(
        "/api",
        &[("t", "caps")],
        Reply::xml_fixture("torznab/caps.xml"),
    );
    for t in ["movie", "search"] {
        feed.route_query(
            "/api",
            &[("t", t)],
            Reply::xml_fixture("torznab/results.xml").delayed(Duration::from_millis(900)),
        );
    }
    let site = FakeServer::start().await;
    site.route(
        "/stream/movie/tt0063350.json",
        Reply::json_fixture("torrentio/movie.json").delayed(Duration::from_millis(900)),
    );
    let sources: Vec<Arc<dyn TorrentSource>> = vec![
        Arc::new(TorznabSource::user_instance(
            fast_client(),
            &feed.url("/api"),
            "key",
        )),
        Arc::new(TorrentioSource::new(fast_client(), &site.base())),
    ];
    let timeline = Arc::new(Timeline::default());
    let outcome = Orchestrator::new(sources)
        .with_deadline(Duration::from_millis(400))
        .run(query(), timeline.clone(), &CancelToken::new())
        .await;
    assert!(matches!(outcome, Outcome::Finished(_)));
    let texts = timeline.texts();
    assert!(
        texts.contains(&"failed torrentio timed out".to_owned()),
        "{texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.starts_with("results torznab")),
        "{texts:?}"
    );
}
