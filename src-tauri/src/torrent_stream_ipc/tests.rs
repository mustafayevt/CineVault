//! The Torrent search streams through the generic engine with fake sources: events arrive
//! source by source, a release found by several sources is one row with `absorbs`, a slow source
//! times out with Retry, a skipped source is listed and never asked, cancel stops everything, and
//! with the feature off the commands refuse without contacting anything. No network.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::boot::Boot;
use async_trait::async_trait;
use cv_app::data_location::{DataLocation, LocationInputs};
use cv_app::secrets::SecretStore;
use cv_app::services::{AppServices, BootOptions};
use cv_app::torrents::{MovieCopy, TitleHit, TorrentServiceError};
use cv_ipc::matching::PosterKeys;
use cv_ipc::provider_stream::ProviderEvent;
use cv_jobs::{LaneConfig, Lanes};
use cv_torrent::TorrentError;
use cv_torrent::model::{
    Hdr, MediaKind, RawRelease, Resolution, Scope, SkipReason, SourceError, SourceId, SourceKind,
    Support, TitleIds, TorrentQuery,
};
use cv_torrent::search::{ResultRow, SearchSession};
use cv_torrent::source::{SearchContext, TorrentSource};
use tauri::Manager;
use tauri::ipc::Channel;
use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};

use super::*;
use crate::provider_stream::{RetryOutcome, SearchRegistry, Sink as EventSink};

const HASH_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HASH_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn raw(name: &str, hash: Option<&str>, seeders: u32) -> RawRelease {
    RawRelease {
        name: name.to_owned(),
        info_hash: hash.map(str::to_owned),
        size: Some(2_000_000_000),
        seeders: Some(seeders),
        ..RawRelease::default()
    }
}

fn query() -> TorrentQuery {
    TorrentQuery {
        kind: MediaKind::Movie,
        ids: TitleIds {
            tmdb: Some(1),
            imdb: Some("tt0063350".into()),
            tvdb: None,
        },
        title: "Dune".into(),
        original_title: None,
        alternative_titles: Vec::new(),
        year: Some(2021),
        scope: Scope::Movie,
        runtime_minutes: Some(155),
        anime: false,
        library_copy: None,
    }
}

enum Behaviour {
    /// Sends these rows as a partial batch, waits, then answers with these.
    Rows {
        early: Vec<RawRelease>,
        last: Vec<RawRelease>,
    },
    Fail(SourceError),
    Sleep(Duration),
    /// Fails the first time and answers the second.
    FailOnce(SourceError, Vec<RawRelease>),
    TvOnly,
}

struct Fake {
    sid: SourceId,
    name: &'static str,
    kind: SourceKind,
    behaviour: Behaviour,
    asked: AtomicUsize,
}

fn fake(id: &'static str, behaviour: Behaviour) -> Arc<Fake> {
    Arc::new(Fake {
        sid: SourceId(id.to_owned()),
        name: id,
        kind: SourceKind::BuiltIn,
        behaviour,
        asked: AtomicUsize::new(0),
    })
}

#[async_trait]
impl TorrentSource for Fake {
    fn id(&self) -> &SourceId {
        &self.sid
    }

    fn name(&self) -> &str {
        self.name
    }

    fn kind(&self) -> SourceKind {
        self.kind
    }

    fn supports(&self, _query: &TorrentQuery) -> Support {
        match self.behaviour {
            Behaviour::TvOnly => Support::Skip(SkipReason::TvOnly),
            _ => Support::Yes,
        }
    }

    async fn search(
        &self,
        _query: &TorrentQuery,
        context: &SearchContext,
    ) -> Result<Vec<RawRelease>, SourceError> {
        let time = self.asked.fetch_add(1, Ordering::SeqCst);
        match &self.behaviour {
            Behaviour::Rows { early, last } => {
                context.partial(early.clone());
                tokio::time::sleep(Duration::from_millis(30)).await;
                Ok(last.clone())
            }
            Behaviour::Fail(error) => Err(error.clone()),
            Behaviour::Sleep(duration) => {
                tokio::time::sleep(*duration).await;
                Ok(Vec::new())
            }
            Behaviour::FailOnce(error, rows) => {
                if time == 0 {
                    Err(error.clone())
                } else {
                    Ok(rows.clone())
                }
            }
            Behaviour::TvOnly => Ok(Vec::new()),
        }
    }
}

type Events = Arc<Mutex<Vec<ProviderEvent<TorrentReleaseDto>>>>;

fn collector() -> (Events, EventSink<TorrentReleaseDto>) {
    let events: Events = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&events);
    (
        events,
        Arc::new(move |event| sink.lock().unwrap().push(event)),
    )
}

fn describe(events: &[ProviderEvent<TorrentReleaseDto>]) -> Vec<String> {
    events
        .iter()
        .map(|event| match event {
            ProviderEvent::Started { provider } => format!("started {provider}"),
            ProviderEvent::Results { provider, items } => {
                format!("results {provider} {}", items.len())
            }
            ProviderEvent::Partial { provider, items } => {
                format!("partial {provider} {}", items.len())
            }
            ProviderEvent::Update { provider, .. } => format!("update {provider}"),
            ProviderEvent::Failed {
                provider,
                reason,
                retryable,
            } => format!("failed {provider} {reason} retryable={retryable}"),
            ProviderEvent::NeedsKey { provider } => format!("needs-key {provider}"),
            ProviderEvent::Done => "done".to_owned(),
        })
        .collect()
}

async fn until(what: &str, mut check: impl FnMut() -> bool) {
    for _ in 0..600 {
        if check() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("timed out waiting for {what}");
}

struct Run {
    events: Events,
    registry: SearchRegistry,
    listed: Vec<TorrentSourceDto>,
}

fn start(sources: Vec<Arc<Fake>>, deadline: Duration, id: &str) -> Run {
    let session = SearchSession::with_deadline(query(), deadline);
    let sources: Vec<Arc<dyn TorrentSource>> = sources
        .into_iter()
        .map(|s| s as Arc<dyn TorrentSource>)
        .collect();
    let (listed, providers) = plan_sources(&session, sources, None);
    let registry = SearchRegistry::default();
    let (events, sink) = collector();
    registry.start(&Lanes::new(LaneConfig::default()), id, providers, sink);
    Run {
        events,
        registry,
        listed,
    }
}

fn done(run: &Run) -> bool {
    run.events.lock().unwrap().last() == Some(&ProviderEvent::Done)
}

fn rows_of(run: &Run, provider: &str) -> Vec<TorrentReleaseDto> {
    run.events
        .lock()
        .unwrap()
        .iter()
        .flat_map(|event| match event {
            ProviderEvent::Results { provider: p, items }
            | ProviderEvent::Partial { provider: p, items }
                if p == provider =>
            {
                items.clone()
            }
            _ => Vec::new(),
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sources_answer_one_by_one_and_a_shared_hash_is_one_row_with_absorbs() {
    let run = start(
        vec![
            // Finds the release without a hash first (sent early), then with its hash.
            fake(
                "text",
                Behaviour::Rows {
                    early: vec![raw("Dune.2021.2160p.WEB-DL", None, 4)],
                    last: vec![raw("Dune.2021.2160p.WEB-DL", Some(HASH_A), 4)],
                },
            ),
            fake(
                "ids",
                Behaviour::Rows {
                    early: Vec::new(),
                    last: vec![
                        raw("Dune 2021 2160p WEB-DL", Some(HASH_A), 40),
                        raw("Dune.1984.1080p.BluRay", Some(HASH_B), 9),
                    ],
                },
            ),
            fake("tv", Behaviour::TvOnly),
        ],
        Duration::from_secs(5),
        "s1",
    );
    until("done", || done(&run)).await;

    // The skipped source is listed with its reason and never started.
    assert_eq!(run.listed.len(), 3);
    assert_eq!(run.listed[2].skipped.as_deref(), Some("tv-only"));
    assert_eq!(run.listed[0].skipped, None);
    let seen = describe(&run.events.lock().unwrap());
    assert!(!seen.iter().any(|e| e == "started tv"), "{seen:?}");
    assert_eq!(seen.first().map(String::as_str), Some("started text"));
    assert!(seen.contains(&"partial text 1".to_owned()), "{seen:?}");

    // Every row of the shared release carries the same id once the hash is known; the early
    // hashless row is named in `absorbs`.
    let all: Vec<TorrentReleaseDto> = ["text", "ids"]
        .iter()
        .flat_map(|p| rows_of(&run, p))
        .collect();
    let early = all
        .iter()
        .find(|r| r.id.starts_with("n:"))
        .expect("the hashless row");
    let merged = all
        .iter()
        .rev()
        .find(|r| r.id == HASH_A)
        .expect("the merged row");
    assert!(merged.absorbs.contains(&early.id), "{merged:?}");
    assert_eq!(merged.sources.len(), 2);
    assert_eq!(merged.seeders, Some(40));
    assert!(merged.matching);
    assert!(merged.magnet.as_deref().unwrap().contains(HASH_A));
    assert_eq!(merged.resolution, "2160p");
    assert_eq!(merged.source.as_deref(), Some("WEB-DL"));

    // The 1984 film is judged not matching, not dropped.
    let other = all.iter().find(|r| r.id == HASH_B).expect("the other film");
    assert!(!other.matching);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_slow_source_times_out_with_retry_and_a_failed_one_says_why() {
    let run = start(
        vec![
            fake("slow", Behaviour::Sleep(Duration::from_secs(30))),
            fake("limited", Behaviour::Fail(SourceError::RateLimited)),
            fake("blocked", Behaviour::Fail(SourceError::Blocked)),
            fake(
                "fine",
                Behaviour::Rows {
                    early: Vec::new(),
                    last: vec![raw("Dune.2021.1080p", Some(HASH_A), 1)],
                },
            ),
        ],
        Duration::from_millis(300),
        "s2",
    );
    until("done", || done(&run)).await;
    let seen = describe(&run.events.lock().unwrap());
    assert!(
        seen.contains(&"failed slow timeout retryable=true".to_owned()),
        "{seen:?}"
    );
    assert!(
        seen.contains(&"failed limited rate-limited retryable=true".to_owned()),
        "{seen:?}"
    );
    assert!(
        seen.contains(&"failed blocked blocked retryable=false".to_owned()),
        "{seen:?}"
    );
    assert!(seen.contains(&"results fine 1".to_owned()), "{seen:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retry_asks_one_source_again_into_the_same_merge() {
    let run = start(
        vec![
            fake(
                "flaky",
                Behaviour::FailOnce(
                    SourceError::Unreachable("down".into()),
                    vec![raw("Dune.2021.1080p", Some(HASH_A), 7)],
                ),
            ),
            fake(
                "steady",
                Behaviour::Rows {
                    early: Vec::new(),
                    last: vec![raw("Dune 2021 1080p", Some(HASH_A), 3)],
                },
            ),
        ],
        Duration::from_secs(5),
        "s3",
    );
    until("done", || done(&run)).await;
    run.events.lock().unwrap().clear();
    assert_eq!(run.registry.retry("s3", "flaky"), RetryOutcome::Started);
    until("done again", || done(&run)).await;
    let seen = describe(&run.events.lock().unwrap());
    assert_eq!(seen, vec!["started flaky", "results flaky 1", "done"]);
    let row = rows_of(&run, "flaky").pop().unwrap();
    assert_eq!(
        row.sources.len(),
        2,
        "merged with the source that answered earlier"
    );
    assert_eq!(row.seeders, Some(7));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_stops_the_search_and_sends_nothing_more() {
    let run = start(
        vec![fake("slow", Behaviour::Sleep(Duration::from_secs(30)))],
        Duration::from_secs(60),
        "s4",
    );
    until("started", || !run.events.lock().unwrap().is_empty()).await;
    assert!(run.registry.cancel("s4"));
    tokio::time::sleep(Duration::from_millis(150)).await;
    let seen = describe(&run.events.lock().unwrap());
    assert_eq!(seen, vec!["started slow"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_search_with_only_skipped_sources_is_done_at_once() {
    let run = start(
        vec![fake("tv", Behaviour::TvOnly)],
        Duration::from_secs(1),
        "s5",
    );
    until("done", || done(&run)).await;
    assert_eq!(describe(&run.events.lock().unwrap()), vec!["done"]);
    assert_eq!(run.listed[0].skipped.as_deref(), Some("tv-only"));
}

// ------------------------------------------------------------------ mapping

#[test]
fn failures_map_to_reason_codes_and_retryability() {
    assert_eq!(
        failure(&SourceError::Unreachable("x".into())),
        ("unreachable".into(), true)
    );
    assert_eq!(
        failure(&SourceError::RateLimited),
        ("rate-limited".into(), true)
    );
    assert_eq!(failure(&SourceError::Timeout), ("timeout".into(), true));
    assert_eq!(failure(&SourceError::Blocked), ("blocked".into(), false));
    assert_eq!(
        failure(&SourceError::BadAnswer("x".into())),
        ("bad-answer: x".into(), false)
    );
    assert_eq!(
        failure(&SourceError::BadAnswer("  ".into())),
        ("bad-answer".into(), false)
    );
    let long = format!("Unknown indexer error {}", "word ".repeat(40));
    let (reason, _) = failure(&SourceError::BadAnswer(long));
    assert!(reason.ends_with('…') && reason.chars().count() <= "bad-answer: ".len() + 80);
    assert!(!failure(&SourceError::KeyRejected).1);
}

#[test]
fn a_release_becomes_a_dto_with_text_badges() {
    use cv_torrent::merge::MergeState;
    use cv_torrent::rank::assess;
    let mut merge = MergeState::new();
    let mut r = raw(
        "Dune.2021.2160p.WEB-DL.DV.HDR10+.Atmos.7.1.H.265.exe",
        Some(HASH_A),
        0,
    );
    r.source = Some(SourceId("a".into()));
    r.published = Some("2024-03-01T00:00:00Z".parse().unwrap());
    let merged = merge.add(r).remove(0);
    let q = query();
    let assessment = assess(&merged.release, &q);
    let dto = release_dto(ResultRow {
        id: merged.id,
        absorbs: merged.absorbs,
        release: merged.release,
        assessment,
    });
    assert_eq!(dto.resolution, "2160p");
    assert_eq!(dto.source.as_deref(), Some("WEB-DL"));
    assert_eq!(dto.codec.as_deref(), Some("HEVC"));
    assert_eq!(dto.hdr.as_deref(), Some("DV"));
    assert_eq!(dto.audio.as_deref(), Some("Atmos 7.1"));
    assert_eq!(dto.published, Some(1_709_251_200_000.0));
    assert_eq!(dto.size, Some(2_000_000_000.0));
    assert!(dto.flags.iter().any(|f| f.kind == "payload"));
    assert!(
        dto.flags
            .iter()
            .any(|f| f.kind == "no-seeders" && f.text == "No seeders")
    );
    assert_eq!(dto.upgrade, None);
    assert_eq!(dto.pack, None);
}

#[test]
fn scopes_come_from_season_and_episode() {
    assert_eq!(
        scope_of(MediaKind::Movie, Some(1), Some(2)).unwrap(),
        Scope::Movie
    );
    assert_eq!(scope_of(MediaKind::Show, None, None).unwrap(), Scope::Show);
    assert_eq!(
        scope_of(MediaKind::Show, Some(2), None).unwrap(),
        Scope::Season(2)
    );
    assert_eq!(
        scope_of(MediaKind::Show, Some(2), Some(5)).unwrap(),
        Scope::Episode(2, 5)
    );
    assert_eq!(
        scope_of(MediaKind::Show, None, Some(5)).unwrap_err().code,
        "invalid"
    );
    assert_eq!(parse_kind("movie").unwrap(), MediaKind::Movie);
    assert_eq!(parse_kind("episode").unwrap_err().code, "invalid");
}

#[test]
fn a_title_hit_becomes_a_dto_with_its_poster_key() {
    let hit = TitleHit {
        kind: MediaKind::Movie,
        tmdb_id: 603,
        title: "The Matrix".into(),
        original_title: "The Matrix".into(),
        year: Some(1999),
        overview: " ".into(),
        poster_url: Some("https://img.example/p.jpg".into()),
        imdb_id: None,
        confidence: 0.9,
        in_library: None,
    };
    let mut posters = PosterKeys::new();
    posters.insert("https://img.example/p.jpg".into(), "key-1".into());
    let dto = title_hit_dto(hit, &posters).expect("a dto");
    assert_eq!(dto.id, "movie:603");
    assert_eq!(dto.poster_key.as_deref(), Some("key-1"));
    assert_eq!(dto.original_title, None, "same as the title");
    assert_eq!(dto.overview, None, "blank");
}

#[test]
fn a_movie_copy_becomes_the_header_words() {
    let copy = MovieCopy {
        resolution: Resolution::R1080,
        source: cv_domain::MediaSource::WebDl,
        hdr: Hdr::None,
        size_bytes: 5_600_000_000,
        file_name: "Blade.Runner.2049.2017.1080p.WEB-DL.mkv".into(),
    };
    let dto = library_copy_dto(&copy);
    assert_eq!(dto.resolution, "1080p");
    assert_eq!(dto.source.as_deref(), Some("WEB-DL"));
    assert_eq!(dto.hdr, None);
    assert!((dto.size - 5.6e9).abs() < 1.0);
    assert_eq!(dto.file_name, "Blade.Runner.2049.2017.1080p.WEB-DL.mkv");
}

#[test]
fn the_backdrop_is_asked_for_at_1280_wide() {
    assert_eq!(
        smaller_backdrop("https://image.tmdb.org/t/p/original/abc.jpg"),
        "https://image.tmdb.org/t/p/w1280/abc.jpg"
    );
    assert_eq!(
        smaller_backdrop("https://img.example/abc.jpg"),
        "https://img.example/abc.jpg"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_landing_lists_the_library_movies_below_2160p_with_their_tmdb_ids() {
    use cv_domain::{Item, MediaFile, MediaFileType};
    let temp = tempfile::tempdir().unwrap();
    let (app, services) = app(temp.path());
    let error = torrent_landing(
        app.state::<TorrentState>(),
        app.state::<crate::img_ipc::ImageState>(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "notice_not_accepted");

    services
        .settings()
        .set("torrents.notice-accepted", serde_json::json!(true))
        .unwrap();
    for (title, tmdb, height, added) in [("Heat", "949", 720, 1), ("Dune", "438631", 2160, 2)] {
        let mut item = Item::movie(title);
        item.core.ids.set("tmdb", tmdb);
        item.core.date_added_ms = added;
        let mut video = MediaFile::new(MediaFileType::Video, "C:/m", "v.mkv");
        video.height = height;
        video.size_bytes = 10;
        item.core.media_files = vec![video];
        services
            .db()
            .write(move |c| cv_db::repo::insert_item(c, &item))
            .unwrap();
    }
    let landing = torrent_landing(
        app.state::<TorrentState>(),
        app.state::<crate::img_ipc::ImageState>(),
    )
    .await
    .expect("landing");
    assert_eq!(landing.upgrade_total, 1);
    assert_eq!(landing.upgrades[0].title, "Heat");
    assert_eq!(landing.upgrades[0].tmdb_id, 949);
    assert_eq!(landing.upgrades[0].kind, "movie");
    assert_eq!(landing.upgrades[0].resolution.as_deref(), Some("720p"));
    assert_eq!(
        landing
            .recent
            .iter()
            .map(|t| t.title.as_str())
            .collect::<Vec<_>>(),
        ["Dune", "Heat"]
    );
    services.shutdown();
}

#[test]
fn service_errors_map_to_codes() {
    let code = |e: TorrentServiceError| TorrentCommandError::from(e).code;
    assert_eq!(
        code(TorrentServiceError::Torrent(
            TorrentError::NoticeNotAccepted
        )),
        "notice_not_accepted"
    );
    assert_eq!(code(TorrentServiceError::NeedsKey), "needs_key");
    assert_eq!(code(TorrentServiceError::KeyRejected), "key_rejected");
    assert_eq!(code(TorrentServiceError::NotFound), "not_found");
    assert_eq!(
        code(TorrentServiceError::Provider {
            message: "x".into(),
            retryable: true
        }),
        "provider"
    );
    assert_eq!(code(TorrentServiceError::Library("x".into())), "library");
}

// ------------------------------------------------------------------ commands

fn app(root: &Path) -> (tauri::App<MockRuntime>, Arc<AppServices>) {
    let inputs = LocationInputs {
        exe_dir: root.join("app"),
        user_data_base: root.join("userdata"),
        user_config_base: root.join("userconfig"),
    };
    std::fs::create_dir_all(&inputs.exe_dir).unwrap();
    let options = BootOptions::new(
        DataLocation::resolve(&inputs),
        Arc::new(SecretStore::in_memory()),
    )
    .without_providers();
    let services = AppServices::boot(options).expect("boot");
    let boot = Boot::for_services(Arc::clone(&services));
    let app = mock_builder()
        .build(mock_context(noop_assets()))
        .expect("mock app builds");
    app.manage(TorrentState::from_services(boot.services()));
    app.manage(SearchState::from_services(boot.services()));
    app.manage(crate::img_ipc::ImageState::from_boot(&boot));
    app.manage(boot);
    (app, services)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn before_the_notice_is_accepted_every_command_refuses_and_nothing_is_started() {
    let temp = tempfile::tempdir().unwrap();
    let (app, services) = app(temp.path());
    assert!(!services.torrents().notice_accepted());

    let events: Channel<ProviderEvent<TorrentTitleHitDto>> = Channel::new(|_| Ok(()));
    let error = torrent_title_search(
        app.state::<TorrentState>(),
        app.state::<SearchState>(),
        TorrentTitleSearchRequestDto {
            search_id: "t1".into(),
            query: "dune".into(),
        },
        events,
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "notice_not_accepted");

    let error = torrent_title_details(app.state::<TorrentState>(), "movie".into(), 1)
        .await
        .unwrap_err();
    assert_eq!(error.code, "notice_not_accepted");
    let error = torrent_title_seasons(app.state::<TorrentState>(), 1)
        .await
        .unwrap_err();
    assert_eq!(error.code, "notice_not_accepted");
    let error = torrent_title_episodes(app.state::<TorrentState>(), 1, 1)
        .await
        .unwrap_err();
    assert_eq!(error.code, "notice_not_accepted");

    let events: Channel<ProviderEvent<TorrentReleaseDto>> = Channel::new(|_| Ok(()));
    let error = torrent_release_search(
        app.state::<TorrentState>(),
        app.state::<SearchState>(),
        TorrentSearchRequestDto {
            search_id: "r1".into(),
            kind: "movie".into(),
            tmdb_id: 1,
            season: None,
            episode: None,
        },
        events,
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "notice_not_accepted");
    services.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn with_the_notice_accepted_but_no_tmdb_key_the_title_search_starts_and_each_provider_asks_for_it()
 {
    let temp = tempfile::tempdir().unwrap();
    let (app, services) = app(temp.path());
    services
        .settings()
        .set("torrents.notice-accepted", serde_json::json!(true))
        .unwrap();

    let events: Arc<Mutex<Vec<String>>> = Arc::default();
    let sink = Arc::clone(&events);
    let channel: Channel<ProviderEvent<TorrentTitleHitDto>> = Channel::new(move |body| {
        let tauri::ipc::InvokeResponseBody::Json(json) = body else {
            return Ok(());
        };
        sink.lock().unwrap().push(json);
        Ok(())
    });
    let start = torrent_title_search(
        app.state::<TorrentState>(),
        app.state::<SearchState>(),
        TorrentTitleSearchRequestDto {
            search_id: "t2".into(),
            query: "dune".into(),
        },
        channel,
    )
    .await
    .expect("starts");
    assert_eq!(
        start
            .providers
            .iter()
            .map(|p| p.key.as_str())
            .collect::<Vec<_>>(),
        vec!["movies", "shows"]
    );
    until("done", || {
        events
            .lock()
            .unwrap()
            .iter()
            .any(|e| e.contains("\"done\""))
    })
    .await;
    let all = events.lock().unwrap().join("\n");
    assert!(
        all.contains("\"started\"") && all.contains("movies") && all.contains("shows"),
        "{all}"
    );
    // This build has no TMDB provider (tests boot without providers): the providers say so.
    assert!(all.contains("failed"), "{all}");
    services.shutdown();
}
