//! The anime offline ID cross-reference against a local mock server that serves a tiny sample of
//! the database (`tests/fixtures/anime_offline_db/`): the real file is about 60 MB and is never
//! committed. Nothing here touches the network.

mod support;

use std::path::Path;
use std::time::Duration;

use cv_domain::{ProviderIds, id_key};
use cv_providers::anime_db::{AnimeDbError, AnimeOfflineDb, LoadOutcome, Meta};
use cv_providers::model::{MediaKind, RequestOptions};
use cv_providers::{HttpClient, ProviderError, complete_ids};
use support::{MockServer, Reply};

const PATH: &str = "/releases/latest/download/anime-offline-database.jsonl";

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/anime_offline_db")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("fixture {}: {e}", path.display()))
}

fn release(name: &str, etag: &str) -> Reply {
    let mut reply = Reply::ok(&fixture(name));
    reply.headers = vec![
        ("Content-Type".into(), "application/octet-stream".into()),
        ("ETag".into(), etag.into()),
    ];
    reply
}

struct Harness {
    server: MockServer,
    db: AnimeOfflineDb,
    _dir: tempfile::TempDir,
}

impl Harness {
    async fn new(refresh: Duration) -> Self {
        let server = MockServer::start().await;
        let dir = tempfile::tempdir().unwrap();
        let db = AnimeOfflineDb::with_http(
            HttpClient::builder().build().unwrap(),
            dir.path().join("anime-offline-database"),
            &server.url(PATH),
        )
        .refresh_interval(refresh);
        Self {
            server,
            db,
            _dir: dir,
        }
    }

    fn files(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(self.db.dir())
            .map(|entries| {
                entries
                    .flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }
}

fn ids(pairs: &[(&str, &str)]) -> ProviderIds {
    pairs.iter().copied().collect()
}

#[tokio::test]
async fn the_first_load_downloads_converts_and_keeps_a_compact_copy() {
    let h = Harness::new(Duration::from_secs(3600)).await;
    h.server.route(PATH, release("sample.jsonl", "\"v1\""));
    assert!(h.db.load_cached().is_none());
    assert!(h.db.meta().is_none());

    let loaded = h.db.load().await.unwrap();
    assert_eq!(loaded.outcome, LoadOutcome::Updated);
    // Three anime link two or more sites; the one with a single address is dropped.
    assert_eq!(loaded.crosswalk.len(), 3);

    // Any ID of a row finds the others, in both directions.
    let by_mal = loaded.crosswalk.lookup(&ids(&[("myanimelist", "4242")]));
    assert_eq!(by_mal.get("anidb"), Some("100"));
    assert_eq!(by_mal.get("anilist"), Some("2100"));
    assert_eq!(by_mal.get("kitsu"), Some("310"));
    assert_eq!(by_mal.get("animeplanet"), Some("star-voyage"));
    assert_eq!(by_mal.get("simkl"), Some("555"));
    let by_anidb = loaded.crosswalk.lookup(&ids(&[("anidb", "300")]));
    assert_eq!(by_anidb.get("myanimelist"), Some("4300"));
    assert_eq!(by_anidb.get("livechart"), Some("77"));
    assert_eq!(by_anidb.get("notify"), Some("AbC-1"));
    assert_eq!(by_anidb.get("anisearch"), Some("4400"));
    assert_eq!(by_anidb.get("animecountdown"), Some("9001"));
    assert_eq!(by_anidb.get("animenewsnetwork"), Some("2808"));
    // Addresses of unknown sites are ignored, the known ones still link.
    let other = loaded.crosswalk.lookup(&ids(&[("anilist", "5001")]));
    assert_eq!(other.get("myanimelist"), Some("5000"));
    assert!(!loaded.crosswalk.knows(&ids(&[("myanimelist", "9999")])));

    // The copy and its metadata are kept; no temporary file stays behind.
    assert_eq!(h.files(), ["crosswalk.json", "meta.json"]);
    let meta = h.db.meta().expect("metadata");
    assert_eq!(meta.etag.as_deref(), Some("\"v1\""));
    assert_eq!(meta.rows, 3);
    assert_eq!(meta.sha256.len(), 64);
    assert_eq!(meta.url, h.server.url(PATH));
    assert!(meta.downloaded_at_ms > 0 && meta.checked_at_ms >= meta.downloaded_at_ms);
    let size = std::fs::metadata(h.db.dir().join("crosswalk.json"))
        .unwrap()
        .len();
    assert!(
        size < fixture("sample.jsonl").len() as u64,
        "the copy is compact"
    );

    // It can be read again without the network.
    assert_eq!(h.db.load_cached().unwrap().len(), 3);
}

#[tokio::test]
async fn a_young_copy_is_used_without_a_request() {
    let h = Harness::new(Duration::from_secs(3600)).await;
    h.server.route(PATH, release("sample.jsonl", "\"v1\""));
    h.db.load().await.unwrap();
    assert_eq!(h.server.count(PATH), 1);

    for _ in 0..3 {
        let again = h.db.load().await.unwrap();
        assert_eq!(again.outcome, LoadOutcome::Fresh);
        assert_eq!(again.crosswalk.len(), 3);
    }
    assert_eq!(h.server.count(PATH), 1);
}

#[tokio::test]
async fn an_old_copy_asks_with_its_etag_and_a_304_keeps_it() {
    let h = Harness::new(Duration::ZERO).await;
    h.server.script(
        PATH,
        vec![release("sample.jsonl", "\"v1\""), Reply::status(304)],
    );
    h.db.load().await.unwrap();
    let before = h.db.meta().unwrap();

    std::thread::sleep(Duration::from_millis(5));
    let again = h.db.load().await.unwrap();
    assert_eq!(again.outcome, LoadOutcome::Unchanged);
    assert_eq!(again.crosswalk.len(), 3);

    let requests = h.server.requests();
    assert_eq!(requests.len(), 2);
    assert!(!requests[0].headers.contains_key("if-none-match"));
    assert_eq!(
        requests[1].headers.get("if-none-match").map(String::as_str),
        Some("\"v1\"")
    );

    // The check is recorded; the download is not.
    let after = h.db.meta().unwrap();
    assert!(after.checked_at_ms > before.checked_at_ms);
    assert_eq!(after.downloaded_at_ms, before.downloaded_at_ms);
    assert_eq!(after.sha256, before.sha256);
}

#[tokio::test]
async fn the_same_file_under_a_new_etag_is_not_converted_again() {
    let h = Harness::new(Duration::ZERO).await;
    h.server.script(
        PATH,
        vec![
            release("sample.jsonl", "\"v1\""),
            release("sample.jsonl", "\"v2\""),
        ],
    );
    h.db.load().await.unwrap();
    let before = h.db.meta().unwrap();
    let modified = std::fs::metadata(h.db.dir().join("crosswalk.json"))
        .unwrap()
        .modified()
        .unwrap();

    std::thread::sleep(Duration::from_millis(20));
    let again = h.db.load().await.unwrap();
    assert_eq!(again.outcome, LoadOutcome::Unchanged);
    let after = h.db.meta().unwrap();
    assert_eq!(after.etag.as_deref(), Some("\"v2\""));
    assert_eq!(after.sha256, before.sha256);
    assert_eq!(
        std::fs::metadata(h.db.dir().join("crosswalk.json"))
            .unwrap()
            .modified()
            .unwrap(),
        modified
    );
}

#[tokio::test]
async fn a_new_release_replaces_the_copy() {
    let h = Harness::new(Duration::ZERO).await;
    h.server.script(
        PATH,
        vec![
            release("sample.jsonl", "\"v1\""),
            release("sample_next.jsonl", "\"v2\""),
        ],
    );
    h.db.load().await.unwrap();
    let before = h.db.meta().unwrap();

    let updated = h.db.load().await.unwrap();
    assert_eq!(updated.outcome, LoadOutcome::Updated);
    assert_eq!(updated.crosswalk.len(), 4);
    assert_eq!(
        updated
            .crosswalk
            .lookup(&ids(&[("myanimelist", "6000")]))
            .get("anidb"),
        Some("400")
    );
    let after = h.db.meta().unwrap();
    assert_ne!(after.sha256, before.sha256);
    assert_eq!(after.etag.as_deref(), Some("\"v2\""));
    assert_eq!(after.rows, 4);
    assert_eq!(h.db.load_cached().unwrap().len(), 4);
    assert_eq!(h.files(), ["crosswalk.json", "meta.json"]);
}

#[tokio::test]
async fn the_older_copy_is_used_when_the_server_fails() {
    let h = Harness::new(Duration::ZERO).await;
    h.server.script(
        PATH,
        vec![release("sample.jsonl", "\"v1\""), Reply::status(503)],
    );
    h.db.load().await.unwrap();

    let kept = h.db.load().await.unwrap();
    assert_eq!(kept.crosswalk.len(), 3);
    let LoadOutcome::KeptAfterError(ProviderError::Request { message, .. }) = &kept.outcome else {
        panic!(
            "expected the older copy after an error, got {:?}",
            kept.outcome
        );
    };
    assert_eq!(message, "HTTP 503");
}

#[tokio::test]
async fn without_a_copy_a_failed_download_is_an_error_and_writes_nothing() {
    let h = Harness::new(Duration::ZERO).await;
    h.server.route(PATH, Reply::status(404));
    let error = h.db.load().await.unwrap_err();
    assert!(matches!(error, AnimeDbError::Download(_)), "{error:?}");
    assert!(h.files().is_empty());
    assert!(error.to_string().contains("could not be downloaded"));
}

#[tokio::test]
async fn a_file_that_is_not_the_database_is_rejected_and_the_copy_stays() {
    let h = Harness::new(Duration::ZERO).await;
    h.server
        .route(PATH, Reply::ok(&fixture("not_the_database.html")));
    let error = h.db.load().await.unwrap_err();
    assert!(matches!(error, AnimeDbError::Format(_)), "{error:?}");
    assert!(h.files().is_empty());

    // With a good copy in place, a bad release leaves it as it was.
    h.server.script(
        PATH,
        vec![
            release("sample.jsonl", "\"v1\""),
            Reply::ok(&fixture("not_the_database.html")),
        ],
    );
    h.db.load().await.unwrap();
    let error = h.db.load().await.unwrap_err();
    assert!(matches!(error, AnimeDbError::Format(_)), "{error:?}");
    assert_eq!(h.db.load_cached().unwrap().len(), 3);
    assert_eq!(h.db.meta().unwrap().etag.as_deref(), Some("\"v1\""));
}

#[tokio::test]
async fn a_damaged_copy_is_downloaded_again() {
    let h = Harness::new(Duration::from_secs(3600)).await;
    h.server.route(PATH, release("sample.jsonl", "\"v1\""));
    h.db.load().await.unwrap();
    std::fs::write(h.db.dir().join("crosswalk.json"), "{not json").unwrap();
    assert!(h.db.load_cached().is_none());

    let loaded = h.db.load().await.unwrap();
    assert_eq!(loaded.outcome, LoadOutcome::Updated);
    assert_eq!(loaded.crosswalk.len(), 3);
    // No ETag is sent for a copy that is not there.
    assert!(!h.server.requests()[1].headers.contains_key("if-none-match"));
}

#[tokio::test]
async fn an_etag_from_another_address_is_not_sent() {
    let h = Harness::new(Duration::ZERO).await;
    h.server.route(PATH, release("sample.jsonl", "\"v1\""));
    h.db.load().await.unwrap();
    // The same folder, now pointed at another release address.
    let moved = AnimeOfflineDb::with_http(
        HttpClient::builder().build().unwrap(),
        h.db.dir(),
        &h.server.url("/other/anime-offline-database.jsonl"),
    )
    .refresh_interval(Duration::ZERO);
    h.server.route(
        "/other/anime-offline-database.jsonl",
        release("sample_next.jsonl", "\"x\""),
    );
    let loaded = moved.load().await.unwrap();
    assert_eq!(loaded.outcome, LoadOutcome::Updated);
    let requests = h.server.requests();
    assert!(!requests[1].headers.contains_key("if-none-match"));
}

#[tokio::test]
async fn refreshing_checks_the_server_even_when_the_copy_is_young() {
    let h = Harness::new(Duration::from_secs(3600)).await;
    h.server.script(
        PATH,
        vec![release("sample.jsonl", "\"v1\""), Reply::status(304)],
    );
    h.db.load().await.unwrap();
    let again = h.db.refresh_now().await.unwrap();
    assert_eq!(again.outcome, LoadOutcome::Unchanged);
    assert_eq!(h.server.count(PATH), 2);
}

#[tokio::test]
async fn the_loaded_cross_reference_completes_anime_ids() {
    let h = Harness::new(Duration::from_secs(3600)).await;
    h.server.route(PATH, release("sample.jsonl", "\"v1\""));
    let loaded = h.db.load().await.unwrap();

    // An item known only by its MyAnimeList ID gets the AniDB ID (which AniDB's provider can then
    // resolve), and the other sites' IDs, without a request.
    let done = complete_ids(
        MediaKind::Show,
        &ids(&[("myanimelist", "4300")]),
        &[],
        Some(&loaded.crosswalk),
        &RequestOptions::default(),
    )
    .await;
    assert_eq!(done.ids.get(id_key::ANIDB), Some("300"));
    assert_eq!(done.ids.get("livechart"), Some("77"));
    assert!(done.failures.is_empty());
    assert_eq!(h.server.count(PATH), 1);
}

#[tokio::test]
async fn the_default_address_is_the_latest_release_and_the_client_waits_long_enough() {
    assert!(
        cv_providers::anime_db::DEFAULT_URL.starts_with(
            "https://github.com/manami-project/anime-offline-database/releases/latest/"
        )
    );
    assert_eq!(
        cv_providers::anime_db::DEFAULT_REFRESH,
        Duration::from_secs(30 * 24 * 60 * 60)
    );
    let dir = tempfile::tempdir().unwrap();
    let db = AnimeOfflineDb::new(dir.path().join("db")).unwrap();
    assert!(db.load_cached().is_none());
    // Metadata can be read back as written.
    let meta = Meta {
        url: "u".into(),
        etag: None,
        sha256: "0".repeat(64),
        checked_at_ms: 1,
        downloaded_at_ms: 1,
        rows: 0,
    };
    let json = serde_json::to_string(&meta).unwrap();
    assert_eq!(serde_json::from_str::<Meta>(&json).unwrap(), meta);
}

#[test]
fn the_cache_folder_is_where_the_caller_put_it() {
    let h = tempfile::tempdir().unwrap();
    let db = AnimeOfflineDb::new(h.path().join("anime")).unwrap();
    assert_eq!(db.dir(), h.path().join("anime"));
}
