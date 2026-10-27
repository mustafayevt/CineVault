//! Signing in to the indexer service and provisioning its indexers, against a fake admin API (an
//! axum server on a loopback port that behaves the way the service's admin API does). No test
//! touches the network.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use cv_torrent::cancel::CancelToken;
use cv_torrent::indexer_service::provision::{
    IndexerRow, IndexerState, IndexerStore, PasswordStore, ProvisionError, ProvisionOptions,
    open_session, provision,
};
use cv_torrent::indexer_service::{AdminClient, AdminError};
use serde_json::{Value, json};

// ---- the fake service ------------------------------------------------------------------------

#[derive(Clone)]
enum Outcome {
    Pass,
    Fail(&'static str),
    /// Answers only after this long.
    Slow(Duration),
}

#[derive(Clone)]
struct Spec {
    id: &'static str,
    name: &'static str,
    kind: &'static str,
    configured: bool,
    caps: Vec<&'static str>,
    language: &'static str,
    /// What saving its settings does (this also tests the site in the real service).
    config: Outcome,
    test: Outcome,
}

fn public(id: &'static str) -> Spec {
    Spec {
        id,
        name: id,
        kind: "public",
        configured: false,
        caps: vec!["2000", "5000"],
        language: "en-US",
        config: Outcome::Pass,
        test: Outcome::Pass,
    }
}

#[derive(Default)]
struct Counter {
    now: AtomicUsize,
    max: AtomicUsize,
}

impl Counter {
    fn enter(&self) {
        let now = self.now.fetch_add(1, Ordering::SeqCst) + 1;
        self.max.fetch_max(now, Ordering::SeqCst);
    }
    fn leave(&self) {
        self.now.fetch_sub(1, Ordering::SeqCst);
    }
    fn max(&self) -> usize {
        self.max.load(Ordering::SeqCst)
    }
}

struct Fake {
    specs: Vec<Spec>,
    password: Mutex<Option<String>>,
    log: Mutex<Vec<String>>,
    deleted: Mutex<HashSet<String>>,
    configuring: Counter,
    testing: Counter,
}

type Shared = Arc<Fake>;

/// Like the real service, every admin call needs the session cookie, also while no password is set:
/// that service hands the cookie to anyone who completes its sign-in redirects (see `ui_login`).
fn authorised(_fake: &Fake, headers: &HeaderMap) -> bool {
    headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains("Jackett=session"))
}

fn denied() -> Response {
    // The real service redirects to its sign-in page.
    (StatusCode::FOUND, [(header::LOCATION, "/UI/Login")]).into_response()
}

fn note(fake: &Fake, line: String) {
    fake.log.lock().expect("lock").push(line);
}

fn error_body(message: &str) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        [(header::CONTENT_TYPE, "application/json")],
        json!({ "result": "error", "error": message }).to_string(),
    )
        .into_response()
}

/// `GET /UI/Dashboard`: the page for a session, a redirect to the sign-in page without one.
async fn dashboard(State(fake): State<Shared>, headers: HeaderMap) -> Response {
    note(&fake, "GET /UI/Dashboard".into());
    if authorised(&fake, &headers) {
        return StatusCode::OK.into_response();
    }
    (
        StatusCode::FOUND,
        [(header::LOCATION, "/UI/Login?ReturnUrl=%2FUI%2FDashboard")],
    )
        .into_response()
}

/// `GET /UI/Login`: the real service first sets a test cookie and bounces through `TestCookie`;
/// when the cookie came back (`cookiesChecked=1`) it signs in at once if no password is set, and
/// shows the sign-in page otherwise.
async fn ui_login(State(fake): State<Shared>, RawQuery(query): RawQuery) -> Response {
    note(&fake, "GET /UI/Login".into());
    if !query.is_some_and(|q| q.contains("cookiesChecked=1")) {
        return (
            StatusCode::FOUND,
            [
                (header::LOCATION, "TestCookie"),
                (header::SET_COOKIE, "TestCookie=1; path=/"),
            ],
        )
            .into_response();
    }
    if fake.password.lock().expect("lock").is_none() {
        return (
            StatusCode::FOUND,
            [
                (header::LOCATION, "Dashboard"),
                (header::SET_COOKIE, "Jackett=session; Path=/"),
            ],
        )
            .into_response();
    }
    StatusCode::OK.into_response()
}

async fn test_cookie(headers: HeaderMap) -> Response {
    let seen = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains("TestCookie=1"));
    if seen {
        (
            StatusCode::FOUND,
            [(header::LOCATION, "Login?cookiesChecked=1")],
        )
            .into_response()
    } else {
        StatusCode::OK.into_response()
    }
}

async fn login(State(fake): State<Shared>, body: String) -> Response {
    note(&fake, "POST /UI/Dashboard".into());
    let given = body
        .strip_prefix("password=")
        .unwrap_or_default()
        .to_owned();
    let matches = fake.password.lock().expect("lock").as_deref() == Some(given.as_str());
    if matches {
        let mut response =
            (StatusCode::FOUND, [(header::LOCATION, "/UI/Dashboard")]).into_response();
        response.headers_mut().insert(
            header::SET_COOKIE,
            HeaderValue::from_static("Jackett=session; Path=/"),
        );
        response
    } else {
        denied()
    }
}

async fn server_config(State(fake): State<Shared>, headers: HeaderMap) -> Response {
    note(&fake, "GET /api/v2.0/server/config".into());
    if !authorised(&fake, &headers) {
        return denied();
    }
    axum::Json(json!({ "api_key": "k", "port": 1 })).into_response()
}

async fn set_password(State(fake): State<Shared>, headers: HeaderMap, body: String) -> Response {
    note(&fake, "POST /api/v2.0/server/adminpassword".into());
    if !authorised(&fake, &headers) {
        return denied();
    }
    let password: String = serde_json::from_str(&body).expect("a JSON string");
    *fake.password.lock().expect("lock") = Some(password);
    StatusCode::NO_CONTENT.into_response()
}

async fn list(State(fake): State<Shared>, headers: HeaderMap) -> Response {
    note(&fake, "GET /api/v2.0/indexers".into());
    if !authorised(&fake, &headers) {
        return denied();
    }
    let deleted = fake.deleted.lock().expect("lock").clone();
    let items: Vec<Value> = fake
        .specs
        .iter()
        .map(|s| {
            let caps: Vec<Value> = s
                .caps
                .iter()
                .map(|c| json!({ "ID": c, "Name": "x" }))
                .collect();
            json!({
                "id": s.id, "name": s.name, "type": s.kind,
                "configured": s.configured && !deleted.contains(s.id), "caps": caps,
                "language": s.language
            })
        })
        .collect();
    axum::Json(items).into_response()
}

async fn get_config(
    State(fake): State<Shared>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    note(&fake, format!("GET config {id}"));
    if !authorised(&fake, &headers) {
        return denied();
    }
    axum::Json(json!([{ "id": "sitelink", "type": "inputstring", "value": format!("https://{id}.test/") }]))
        .into_response()
}

async fn post_config(
    State(fake): State<Shared>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: String,
) -> Response {
    note(&fake, format!("POST config {id}"));
    if !authorised(&fake, &headers) {
        return denied();
    }
    // The settings come back exactly as they were sent.
    let sent: Value = serde_json::from_str(&body).expect("JSON settings");
    assert_eq!(sent[0]["value"], format!("https://{id}.test/"));
    let spec = fake
        .specs
        .iter()
        .find(|s| s.id == id)
        .expect("known indexer")
        .clone();
    fake.configuring.enter();
    let response = match spec.config {
        Outcome::Pass => StatusCode::NO_CONTENT.into_response(),
        Outcome::Fail(message) => error_body(message),
        Outcome::Slow(delay) => {
            tokio::time::sleep(delay).await;
            StatusCode::NO_CONTENT.into_response()
        }
    };
    // Hold the slot a moment so overlapping requests are seen.
    tokio::time::sleep(Duration::from_millis(30)).await;
    fake.configuring.leave();
    response
}

async fn test_indexer(
    State(fake): State<Shared>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    note(&fake, format!("POST test {id}"));
    if !authorised(&fake, &headers) {
        return denied();
    }
    let spec = fake
        .specs
        .iter()
        .find(|s| s.id == id)
        .expect("known indexer")
        .clone();
    fake.testing.enter();
    let response = match spec.test {
        Outcome::Pass => {
            tokio::time::sleep(Duration::from_millis(30)).await;
            StatusCode::NO_CONTENT.into_response()
        }
        Outcome::Fail(message) => error_body(message),
        Outcome::Slow(delay) => {
            tokio::time::sleep(delay).await;
            StatusCode::NO_CONTENT.into_response()
        }
    };
    fake.testing.leave();
    response
}

async fn delete_indexer(
    State(fake): State<Shared>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    note(&fake, format!("DELETE {id}"));
    if !authorised(&fake, &headers) {
        return denied();
    }
    fake.deleted.lock().expect("lock").insert(id);
    StatusCode::NO_CONTENT.into_response()
}

async fn serve(specs: Vec<Spec>, password: Option<&str>) -> (Shared, AdminClient) {
    let fake = Arc::new(Fake {
        specs,
        password: Mutex::new(password.map(str::to_owned)),
        log: Mutex::new(Vec::new()),
        deleted: Mutex::new(HashSet::new()),
        configuring: Counter::default(),
        testing: Counter::default(),
    });
    let app = Router::new()
        .route("/UI/Dashboard", get(dashboard).post(login))
        .route("/UI/Login", get(ui_login))
        .route("/UI/TestCookie", get(test_cookie))
        .route("/api/v2.0/server/config", get(server_config))
        .route("/api/v2.0/server/adminpassword", post(set_password))
        .route("/api/v2.0/indexers", get(list))
        .route(
            "/api/v2.0/indexers/{id}/config",
            get(get_config).post(post_config),
        )
        .route("/api/v2.0/indexers/{id}/test", post(test_indexer))
        .route(
            "/api/v2.0/indexers/{id}",
            axum::routing::delete(delete_indexer),
        )
        .with_state(Arc::clone(&fake));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let port = listener.local_addr().expect("address").port();
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    let admin = AdminClient::new(&format!("http://127.0.0.1:{port}")).expect("client");
    (fake, admin)
}

// ---- stores ----------------------------------------------------------------------------------

#[derive(Default)]
struct MemoryRows(Mutex<HashMap<String, IndexerRow>>);

impl IndexerStore for MemoryRows {
    fn upsert(&self, row: &IndexerRow) -> Result<(), String> {
        self.0
            .lock()
            .expect("lock")
            .insert(row.id.clone(), row.clone());
        Ok(())
    }
}

impl MemoryRows {
    fn get(&self, id: &str) -> Option<IndexerRow> {
        self.0.lock().expect("lock").get(id).cloned()
    }
    fn len(&self) -> usize {
        self.0.lock().expect("lock").len()
    }
}

#[derive(Default)]
struct MemoryPassword {
    saved: Mutex<Option<String>>,
    refuse: bool,
}

impl MemoryPassword {
    fn holding(password: &str) -> Self {
        Self {
            saved: Mutex::new(Some(password.to_owned())),
            refuse: false,
        }
    }
    fn saved(&self) -> Option<String> {
        self.saved.lock().expect("lock").clone()
    }
}

impl PasswordStore for MemoryPassword {
    fn get(&self) -> Option<String> {
        self.saved()
    }
    fn set(&self, password: &str) -> Result<(), String> {
        if self.refuse {
            return Err("the credential store is locked".into());
        }
        *self.saved.lock().expect("lock") = Some(password.to_owned());
        Ok(())
    }
}

fn log(fake: &Fake) -> Vec<String> {
    fake.log.lock().expect("lock").clone()
}

fn options() -> ProvisionOptions {
    ProvisionOptions {
        test_timeout: Duration::from_secs(1),
        ..ProvisionOptions::default()
    }
}

async fn run(
    admin: &AdminClient,
    store: &Arc<MemoryRows>,
    user_off: &[&str],
    options: ProvisionOptions,
) -> Result<cv_torrent::indexer_service::provision::ProvisionReport, ProvisionError> {
    let off: HashSet<String> = user_off.iter().map(|s| (*s).to_owned()).collect();
    let store_dyn: Arc<dyn IndexerStore> = Arc::clone(store) as Arc<dyn IndexerStore>;
    provision(
        admin,
        store_dyn,
        &off,
        &english(),
        options,
        &CancelToken::new(),
        &|_| {},
    )
    .await
}

/// The tracker languages of most tests: English only.
fn english() -> HashSet<String> {
    HashSet::from(["en".to_owned()])
}

// ---- tests -----------------------------------------------------------------------------------

#[tokio::test]
async fn sites_in_other_languages_are_left_out_and_stale_ones_removed() {
    let mut russian = public("rutracker");
    russian.language = "ru-RU";
    let mut chinese_set_up = public("btzh");
    chinese_set_up.language = "zh-CN";
    chinese_set_up.configured = true;
    let mut adult_set_up = public("adultsite");
    adult_set_up.caps = vec!["6000", "6010", "2000"];
    adult_set_up.configured = true;
    let mut direct_set_up = public("yts");
    direct_set_up.configured = true;
    let mut unnamed = public("plain");
    unnamed.language = "";
    let (fake, admin) = serve(
        vec![
            public("alpha"),
            russian,
            chinese_set_up,
            adult_set_up,
            direct_set_up,
            unnamed,
        ],
        None,
    )
    .await;
    let passwords = MemoryPassword::default();
    open_session(&admin, &passwords).await.expect("session");
    let store = Arc::new(MemoryRows::default());
    let report = run(&admin, &store, &[], options())
        .await
        .expect("provision");
    let ids: Vec<&str> = report.rows.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["alpha", "plain"], "English and unnamed only");
    let mut removed = report.removed.clone();
    removed.sort();
    assert_eq!(removed, ["adultsite", "btzh", "yts"]);
    let other: Vec<(&str, &str)> = report
        .other_languages
        .iter()
        .map(|site| (site.id.as_str(), site.language.as_str()))
        .collect();
    assert_eq!(
        other,
        [("btzh", "zh"), ("rutracker", "ru")],
        "what another language would add"
    );
    let deleted = fake.deleted.lock().expect("lock").clone();
    for id in ["adultsite", "btzh", "yts"] {
        assert!(deleted.contains(id), "{id} removed from the service");
    }
    assert!(
        !deleted.contains("rutracker"),
        "never set up, nothing to remove"
    );
}

#[tokio::test]
async fn a_happy_run_keeps_every_candidate_and_leaves_the_others_alone() {
    let mut mixed = public("mixed");
    mixed.caps = vec!["2000", "5000", "6000"];
    let mut adult = public("adult");
    adult.caps = vec!["6000", "6010", "2000"];
    let mut private = public("private");
    private.kind = "private";
    let mut music = public("music");
    music.caps = vec!["3000"];
    let (fake, admin) = serve(
        vec![
            public("zulu"),
            public("alpha"),
            mixed,
            adult,
            private,
            music,
        ],
        None,
    )
    .await;
    let passwords = MemoryPassword::default();
    open_session(&admin, &passwords).await.expect("session");
    let store = Arc::new(MemoryRows::default());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen_in = Arc::clone(&seen);
    let report = provision(
        &admin,
        Arc::clone(&store) as Arc<dyn IndexerStore>,
        &HashSet::new(),
        &english(),
        options(),
        &CancelToken::new(),
        &move |row| seen_in.lock().expect("lock").push(row.id.clone()),
    )
    .await
    .expect("provision");

    let ids: Vec<&str> = report.rows.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["alpha", "mixed", "zulu"]);
    assert_eq!(report.count(IndexerState::Working), 3);
    for row in &report.rows {
        assert_eq!(row.state, IndexerState::Working);
        assert_eq!(row.reason, None);
        assert!(row.checked_at > 1_700_000_000_000);
        assert!(!row.user_off);
        assert_eq!(store.get(&row.id).as_ref(), Some(row));
    }
    assert_eq!(store.len(), 3);
    // Each row reached the callback as that indexer finished.
    let mut seen = seen.lock().expect("lock").clone();
    seen.sort();
    assert_eq!(seen, ["alpha", "mixed", "zulu"]);

    let log = log(&fake);
    for untouched in ["adult", "private", "music"] {
        assert!(
            !log.iter().any(|l| l.ends_with(untouched)),
            "{untouched} was touched: {log:?}"
        );
    }
    // Nothing that worked was deleted.
    assert!(!log.iter().any(|l| l.starts_with("DELETE")));
}

#[tokio::test]
async fn the_admin_password_is_set_and_kept_before_the_first_indexer_is_enabled() {
    let (fake, admin) = serve(vec![public("zulu")], None).await;
    let passwords = MemoryPassword::default();
    open_session(&admin, &passwords).await.expect("session");
    let saved = passwords.saved().expect("a password was kept");
    assert_eq!(saved.len(), 32);
    assert_eq!(
        fake.password.lock().expect("lock").as_deref(),
        Some(saved.as_str())
    );

    let store = Arc::new(MemoryRows::default());
    run(&admin, &store, &[], options())
        .await
        .expect("provision");
    let log = log(&fake);
    let set = log
        .iter()
        .position(|l| l == "POST /api/v2.0/server/adminpassword")
        .expect("set");
    let first_config = log
        .iter()
        .position(|l| l.starts_with("POST config"))
        .expect("enable");
    assert!(set < first_config, "{log:?}");
    // And the calls after it were signed in: the fake would have redirected them otherwise.
    assert_eq!(
        store.get("zulu").map(|r| r.state),
        Some(IndexerState::Working)
    );
}

#[tokio::test]
async fn a_later_session_signs_in_with_the_saved_password() {
    let (fake, admin) = serve(vec![public("zulu")], Some("known-password")).await;
    let passwords = MemoryPassword::holding("known-password");
    open_session(&admin, &passwords).await.expect("session");
    let log = log(&fake);
    assert!(log.iter().any(|l| l == "POST /UI/Dashboard"));
    assert!(!log.iter().any(|l| l.contains("adminpassword")), "{log:?}");
    let store = Arc::new(MemoryRows::default());
    run(&admin, &store, &[], options())
        .await
        .expect("provision");
    assert_eq!(store.len(), 1);
}

#[tokio::test]
async fn a_protected_service_with_no_saved_password_is_reported() {
    let (_fake, admin) = serve(vec![], Some("someone-elses")).await;
    let error = open_session(&admin, &MemoryPassword::default())
        .await
        .expect_err("locked out");
    assert!(
        matches!(error, ProvisionError::PasswordUnknown),
        "{error:?}"
    );

    // A wrong saved password is the same.
    let error = open_session(&admin, &MemoryPassword::holding("wrong"))
        .await
        .expect_err("wrong password");
    assert!(
        matches!(error, ProvisionError::PasswordUnknown),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_service_that_lost_its_password_gets_the_one_we_hold_again() {
    let (fake, admin) = serve(vec![], None).await;
    let passwords = MemoryPassword::holding("old-password");
    open_session(&admin, &passwords).await.expect("session");
    assert_eq!(passwords.saved().as_deref(), Some("old-password"));
    assert_eq!(
        fake.password.lock().expect("lock").as_deref(),
        Some("old-password")
    );
    // Signed in: the next call works without a redirect.
    assert_eq!(admin.list_indexers().await.expect("list").len(), 0);
}

#[tokio::test]
async fn a_password_that_cannot_be_kept_is_never_set() {
    let (fake, admin) = serve(vec![], None).await;
    let passwords = MemoryPassword {
        refuse: true,
        ..MemoryPassword::default()
    };
    let error = open_session(&admin, &passwords)
        .await
        .expect_err("not kept");
    assert!(
        matches!(error, ProvisionError::PasswordNotSaved(_)),
        "{error:?}"
    );
    assert!(fake.password.lock().expect("lock").is_none());
}

#[tokio::test]
async fn a_browser_challenge_is_needs_solver_and_the_indexer_is_deleted() {
    let mut blocked = public("blocked");
    blocked.test = Outcome::Fail("Exception (blocked): Challenge detected (CloudFlare)");
    let (fake, admin) = serve(vec![public("fine"), blocked], None).await;
    open_session(&admin, &MemoryPassword::default())
        .await
        .expect("session");
    let store = Arc::new(MemoryRows::default());
    let report = run(&admin, &store, &[], options())
        .await
        .expect("provision");
    let row = store.get("blocked").expect("row");
    assert_eq!(row.state, IndexerState::NeedsSolver);
    assert!(
        row.reason
            .as_deref()
            .unwrap_or_default()
            .contains("challenge")
    );
    assert_eq!(report.count(IndexerState::Working), 1);
    assert!(log(&fake).contains(&"DELETE blocked".to_owned()));
    assert!(!log(&fake).contains(&"DELETE fine".to_owned()));
}

#[tokio::test]
async fn a_site_that_does_not_answer_in_time_is_unreachable() {
    let mut slow = public("slow");
    slow.test = Outcome::Slow(Duration::from_secs(10));
    let (fake, admin) = serve(vec![slow], None).await;
    open_session(&admin, &MemoryPassword::default())
        .await
        .expect("session");
    let store = Arc::new(MemoryRows::default());
    let started = std::time::Instant::now();
    run(&admin, &store, &[], options())
        .await
        .expect("provision");
    assert!(
        started.elapsed() < Duration::from_secs(8),
        "the limit was not applied"
    );
    let row = store.get("slow").expect("row");
    assert_eq!(row.state, IndexerState::Unreachable);
    assert!(
        row.reason
            .as_deref()
            .unwrap_or_default()
            .contains("did not answer")
    );
    assert!(log(&fake).contains(&"DELETE slow".to_owned()));
}

#[tokio::test]
async fn a_connection_error_is_unreachable() {
    let mut gone = public("gone");
    gone.test = Outcome::Fail("Exception (gone): Unable to connect to the remote server");
    let (_fake, admin) = serve(vec![gone], None).await;
    open_session(&admin, &MemoryPassword::default())
        .await
        .expect("session");
    let store = Arc::new(MemoryRows::default());
    run(&admin, &store, &[], options())
        .await
        .expect("provision");
    assert_eq!(
        store.get("gone").map(|r| r.state),
        Some(IndexerState::Unreachable)
    );
}

#[tokio::test]
async fn a_server_error_while_enabling_is_an_error_row_and_the_test_is_skipped() {
    let mut broken = public("broken");
    broken.config = Outcome::Fail("Indexer definition is out of date");
    let (fake, admin) = serve(vec![broken], None).await;
    open_session(&admin, &MemoryPassword::default())
        .await
        .expect("session");
    let store = Arc::new(MemoryRows::default());
    run(&admin, &store, &[], options())
        .await
        .expect("provision");
    let row = store.get("broken").expect("row");
    assert_eq!(row.state, IndexerState::Error);
    assert_eq!(
        row.reason.as_deref(),
        Some("Indexer definition is out of date")
    );
    let log = log(&fake);
    assert!(log.contains(&"DELETE broken".to_owned()));
    assert!(!log.contains(&"POST test broken".to_owned()), "{log:?}");
}

#[tokio::test]
async fn an_indexer_a_person_switched_off_is_skipped_and_removed() {
    let mut configured = public("noisy");
    configured.configured = true;
    let (fake, admin) = serve(vec![public("zulu"), configured, public("never")], None).await;
    open_session(&admin, &MemoryPassword::default())
        .await
        .expect("session");
    let store = Arc::new(MemoryRows::default());
    let report = run(&admin, &store, &["noisy", "never"], options())
        .await
        .expect("provision");
    for id in ["noisy", "never"] {
        let row = store.get(id).expect("row");
        assert_eq!(row.state, IndexerState::Off);
        assert!(row.user_off);
    }
    assert_eq!(report.count(IndexerState::Working), 1);
    let log = log(&fake);
    // Removed from the service where it was set up; nothing is sent about the other.
    assert!(log.contains(&"DELETE noisy".to_owned()));
    assert!(!log.iter().any(|l| l.ends_with("never")), "{log:?}");
    assert!(
        !log.iter()
            .any(|l| l == "POST config noisy" || l == "POST test noisy")
    );
}

#[tokio::test]
async fn setting_up_and_testing_run_in_parallel_within_their_limits() {
    let specs: Vec<Spec> = [
        "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r",
        "s", "t",
    ]
    .into_iter()
    .map(public)
    .collect();
    let (fake, admin) = serve(specs, None).await;
    open_session(&admin, &MemoryPassword::default())
        .await
        .expect("session");
    let store = Arc::new(MemoryRows::default());
    let report = run(&admin, &store, &[], options())
        .await
        .expect("provision");
    assert_eq!(report.count(IndexerState::Working), 20);
    assert!(fake.configuring.max() <= 6, "{}", fake.configuring.max());
    assert!(fake.testing.max() <= 8, "{}", fake.testing.max());
    assert!(fake.configuring.max() > 1, "nothing ran in parallel");
    assert!(fake.testing.max() > 1, "nothing ran in parallel");
}

#[tokio::test]
async fn the_limits_can_be_lowered() {
    let specs: Vec<Spec> = ["a", "b", "c", "d", "e"].into_iter().map(public).collect();
    let (fake, admin) = serve(specs, None).await;
    open_session(&admin, &MemoryPassword::default())
        .await
        .expect("session");
    let store = Arc::new(MemoryRows::default());
    let one = ProvisionOptions {
        enable_parallel: 1,
        test_parallel: 1,
        ..options()
    };
    run(&admin, &store, &[], one).await.expect("provision");
    assert_eq!(fake.configuring.max(), 1);
    assert_eq!(fake.testing.max(), 1);
}

#[tokio::test]
async fn a_cancelled_run_stops_waiting() {
    let mut slow = public("slow");
    slow.test = Outcome::Slow(Duration::from_secs(30));
    let (_fake, admin) = serve(vec![slow], None).await;
    open_session(&admin, &MemoryPassword::default())
        .await
        .expect("session");
    let cancel = CancelToken::new();
    let later = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        later.cancel();
    });
    let store: Arc<dyn IndexerStore> = Arc::new(MemoryRows::default());
    let options = ProvisionOptions {
        test_timeout: Duration::from_secs(60),
        ..ProvisionOptions::default()
    };
    let started = std::time::Instant::now();
    let error = provision(
        &admin,
        store,
        &HashSet::new(),
        &english(),
        options,
        &cancel,
        &|_| {},
    )
    .await
    .expect_err("cancelled");
    assert!(matches!(error, ProvisionError::Cancelled), "{error:?}");
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[tokio::test]
async fn an_unreachable_service_is_reported_as_such() {
    let admin = AdminClient::new("http://127.0.0.1:9").expect("client");
    let error = admin.list_indexers().await.expect_err("nothing there");
    assert!(matches!(error, AdminError::Unreachable(_)), "{error:?}");
    let error = open_session(&admin, &MemoryPassword::default())
        .await
        .expect_err("nothing there");
    assert!(
        matches!(error, ProvisionError::Admin(AdminError::Unreachable(_))),
        "{error:?}"
    );
}
