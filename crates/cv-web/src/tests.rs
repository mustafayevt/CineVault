//! HTTP-level tests of the sign-in rules and of the server inside the desktop app.
//!
//! The router tests set the peer address by hand (the `ConnectInfo` a real connection would
//! carry), so a public, a private and a proxied client can be checked without a network. The
//! embed tests start the real server on a free port and talk to it over TCP.

use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode, header};
use cv_app::data_location::{DataLocation, LocationInputs};
use cv_app::secrets::SecretStore;
use cv_app::services::{AppServices, BootOptions};
use cv_ipc::image::ImageContext;
use ipnet::IpNet;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tower::ServiceExt;

use crate::auth::{self, Auth, HostMode};
use crate::embed::{self, BrowserAccessConfig, EmbedOptions, Shared, StartError};
use crate::server;
use crate::state::{Borrowed, Commands, DeleteGuard, Streams, UiAssets, UiFile, WebState};

const PASSWORD: &str = "a long enough password";
const INDEX: &str =
    "<!doctype html><html><head><title>CineVault</title></head><body>app</body></html>";

fn boot(root: &Path) -> Arc<AppServices> {
    let inputs = LocationInputs {
        exe_dir: root.join("app"),
        user_data_base: root.join("data"),
        user_config_base: root.join("config"),
    };
    std::fs::create_dir_all(&inputs.exe_dir).unwrap();
    let location = DataLocation::resolve(&inputs);
    let options =
        BootOptions::new(location, Arc::new(SecretStore::in_memory())).without_providers();
    AppServices::boot(options).expect("the services boot")
}

fn ui() -> UiAssets {
    UiAssets::Embedded(Arc::new(|path: &str| {
        (path == "index.html").then(|| UiFile {
            bytes: INDEX.as_bytes().to_vec(),
            mime: "text/html".to_owned(),
        })
    }))
}

fn web_state(
    services: &Arc<AppServices>,
    host: HostMode,
    lan_only: bool,
    trust_proxy: Vec<IpNet>,
) -> WebState {
    let data_root = services.location().root().to_path_buf();
    let images = Arc::new(ImageContext::from_services(services));
    WebState {
        services: Arc::clone(services),
        commands: Arc::new(Commands::build(services, &images, Borrowed::default())),
        images,
        auth: Arc::new(Auth::open_as(&data_root, host).unwrap()),
        progress: Arc::new(cv_ipc::jobs::ProgressHub::default()),
        notices: cv_ipc::notices::NoticeHub::from_app(Arc::clone(services.notices())),
        streams: Arc::new(Streams::default()),
        delete_guard: Arc::new(DeleteGuard::default()),
        data_root,
        bind: "0.0.0.0".to_owned(),
        port: 8787,
        keys_locked: false,
        trust_proxy,
        ui: ui(),
        lan_only,
    }
}

struct Answer {
    status: StatusCode,
    cookies: Vec<String>,
    body: String,
}

impl Answer {
    fn session(&self) -> Option<String> {
        self.cookies
            .iter()
            .find_map(|cookie| cookie.strip_prefix("cv_session="))
            .and_then(|rest| rest.split(';').next())
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    }
}

async fn call(
    state: &WebState,
    method: &str,
    uri: &str,
    peer: &str,
    headers: &[(&str, &str)],
    body: Option<serde_json::Value>,
) -> Answer {
    let peer: IpAddr = peer.parse().unwrap();
    let mut request = Request::builder().method(method).uri(uri);
    request = request.header(header::HOST, "cinevault.test:8787");
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let body = match body {
        Some(json) => {
            request = request.header(header::CONTENT_TYPE, "application/json");
            Body::from(json.to_string())
        }
        None => Body::empty(),
    };
    let mut request = request.body(body).unwrap();
    request
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::new(peer, 50_000)));
    let response = server::router(state.clone())
        .oneshot(request)
        .await
        .unwrap();
    let status = response.status();
    let cookies = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok().map(str::to_owned))
        .collect();
    let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    Answer {
        status,
        cookies,
        body: String::from_utf8_lossy(&bytes).into_owned(),
    }
}

fn set_password(state: &WebState) {
    state.auth.set_password_unattended(PASSWORD).unwrap();
}

fn pin_of(state: &WebState) -> String {
    let text = std::fs::read_to_string(state.data_root.join(auth::PIN_FILE)).unwrap();
    text.split("PIN: ")
        .nth(1)
        .unwrap()
        .chars()
        .take(8)
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_welcome_page_is_for_the_home_network_only() {
    let dir = tempfile::tempdir().unwrap();
    let services = boot(dir.path());
    let state = web_state(&services, HostMode::Server, false, Vec::new());

    let home = call(&state, "GET", "/setup", "192.168.1.30", &[], None).await;
    assert_eq!(home.status, StatusCode::OK);
    assert!(home.body.contains("Create password"));
    assert!(home.cookies.iter().any(|c| c.starts_with("cv_setup=")));

    let public = call(&state, "GET", "/setup", "8.8.8.8", &[], None).await;
    assert_eq!(public.status, StatusCode::FORBIDDEN);
    assert!(public.body.contains("Finish setup from your home network"));
    assert!(public.cookies.is_empty());

    // A public address cannot set the password even with a claim it guessed.
    let posted = call(
        &state,
        "POST",
        "/api/setup",
        "8.8.8.8",
        &[],
        Some(serde_json::json!({ "password": PASSWORD, "confirm": PASSWORD })),
    )
    .await;
    assert_eq!(posted.status, StatusCode::FORBIDDEN);
    assert!(!state.auth.has_password());
    services.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_forwarded_address_counts_only_from_a_trusted_proxy() {
    let dir = tempfile::tempdir().unwrap();
    let services = boot(dir.path());
    let trusted: Vec<IpNet> = vec!["10.0.0.0/8".parse().unwrap()];
    let state = web_state(&services, HostMode::Server, false, trusted);

    // A public peer that is not a trusted proxy cannot claim a home address.
    let spoofed = call(
        &state,
        "GET",
        "/setup",
        "203.0.113.9",
        &[("x-forwarded-for", "192.168.1.30")],
        None,
    )
    .await;
    assert_eq!(spoofed.status, StatusCode::FORBIDDEN);

    // Through the trusted proxy, the forwarded client decides, both ways.
    let proxied_public = call(
        &state,
        "GET",
        "/setup",
        "10.0.0.2",
        &[("x-forwarded-for", "198.51.100.7")],
        None,
    )
    .await;
    assert_eq!(proxied_public.status, StatusCode::FORBIDDEN);
    let proxied_home = call(
        &state,
        "GET",
        "/setup",
        "10.0.0.2",
        &[("x-forwarded-for", "192.168.1.30")],
        None,
    )
    .await;
    assert_eq!(proxied_home.status, StatusCode::OK);
    services.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pin_reset_works_from_home_and_ends_the_old_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let services = boot(dir.path());
    let state = web_state(&services, HostMode::Server, false, Vec::new());
    set_password(&state);

    let signed_in = call(
        &state,
        "POST",
        "/api/login",
        "192.168.1.40",
        &[],
        Some(serde_json::json!({ "password": PASSWORD })),
    )
    .await;
    let old = signed_in.session().expect("a session cookie");
    let old_cookie = format!("cv_session={old}");

    let away = call(&state, "POST", "/api/reset/start", "8.8.8.8", &[], None).await;
    assert_eq!(away.status, StatusCode::FORBIDDEN);
    assert!(away.body.contains("home network"));
    assert!(!state.data_root.join(auth::PIN_FILE).exists());

    let started = call(
        &state,
        "POST",
        "/api/reset/start",
        "192.168.1.30",
        &[],
        None,
    )
    .await;
    assert_eq!(started.status, StatusCode::OK, "{}", started.body);
    assert!(started.body.contains(auth::PIN_FILE));
    let pin = pin_of(&state);
    // The answer and nothing the page sees carries the PIN itself.
    assert!(!started.body.contains(&pin));

    let wrong = if pin == "00000000" {
        "00000001"
    } else {
        "00000000"
    };
    let refused = call(
        &state,
        "POST",
        "/api/reset/finish",
        "192.168.1.30",
        &[],
        Some(serde_json::json!({ "pin": wrong, "password": "a new long password", "confirm": "a new long password" })),
    )
    .await;
    assert_eq!(refused.status, StatusCode::UNAUTHORIZED);
    assert!(refused.body.contains("4 tries left"));

    let done = call(
        &state,
        "POST",
        "/api/reset/finish",
        "192.168.1.30",
        &[],
        Some(serde_json::json!({ "pin": pin, "password": "a new long password", "confirm": "a new long password" })),
    )
    .await;
    assert_eq!(done.status, StatusCode::OK, "{}", done.body);
    let new = done.session().expect("signed in after the reset");
    assert!(!state.data_root.join(auth::PIN_FILE).exists());

    let old_info = call(
        &state,
        "GET",
        "/api/server/info",
        "192.168.1.40",
        &[("cookie", &old_cookie)],
        None,
    )
    .await;
    assert_eq!(old_info.status, StatusCode::UNAUTHORIZED);
    let new_cookie = format!("cv_session={new}");
    let new_info = call(
        &state,
        "GET",
        "/api/server/info",
        "192.168.1.30",
        &[("cookie", &new_cookie)],
        None,
    )
    .await;
    assert_eq!(new_info.status, StatusCode::OK);
    assert!(new_info.body.contains("\"host\":\"server\""));
    services.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_desktop_host_has_no_welcome_and_no_pin() {
    let dir = tempfile::tempdir().unwrap();
    let services = boot(dir.path());
    let host = HostMode::Desktop {
        computer: "STUDY-PC".to_owned(),
    };
    let state = web_state(&services, host, true, Vec::new());

    let setup = call(&state, "GET", "/setup", "192.168.1.30", &[], None).await;
    assert_eq!(setup.status, StatusCode::SEE_OTHER);

    let login = call(&state, "GET", "/login", "192.168.1.30", &[], None).await;
    assert_eq!(login.status, StatusCode::OK);
    assert!(
        login
            .body
            .contains("Change it in CineVault's Settings on STUDY-PC.")
    );

    let reset = call(
        &state,
        "POST",
        "/api/reset/start",
        "192.168.1.30",
        &[],
        None,
    )
    .await;
    assert_eq!(reset.status, StatusCode::FORBIDDEN);
    assert!(reset.body.contains("STUDY-PC"));
    assert!(!state.data_root.join(auth::PIN_FILE).exists());
    services.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn home_network_only_refuses_public_peers() {
    let dir = tempfile::tempdir().unwrap();
    let services = boot(dir.path());
    let host = HostMode::Desktop {
        computer: "STUDY-PC".to_owned(),
    };
    let state = web_state(&services, host.clone(), true, Vec::new());
    set_password(&state);

    for uri in ["/login", "/healthz", "/", "/api/stream"] {
        let public = call(&state, "GET", uri, "8.8.8.8", &[], None).await;
        assert_eq!(public.status, StatusCode::FORBIDDEN, "{uri}");
        assert_eq!(public.body, server::HOME_ONLY);
    }
    // Even a forwarded header does not help: the desktop trusts no proxy.
    let forwarded = call(
        &state,
        "GET",
        "/login",
        "8.8.8.8",
        &[("x-forwarded-for", "192.168.1.30")],
        None,
    )
    .await;
    assert_eq!(forwarded.status, StatusCode::FORBIDDEN);
    for peer in [
        "127.0.0.1",
        "192.168.1.30",
        "10.1.2.3",
        "fe80::1",
        "100.64.1.1",
    ] {
        let home = call(&state, "GET", "/login", peer, &[], None).await;
        assert_eq!(home.status, StatusCode::OK, "{peer}");
    }

    // With the option off, a public peer reaches the sign-in page and nothing more.
    let open = web_state(&services, host, false, Vec::new());
    let page = call(&open, "GET", "/login", "8.8.8.8", &[], None).await;
    assert_eq!(page.status, StatusCode::OK);
    let library = call(&open, "GET", "/", "8.8.8.8", &[], None).await;
    assert_eq!(library.status, StatusCode::SEE_OTHER);
    services.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_embedded_interface_is_served_with_the_session_token() {
    let dir = tempfile::tempdir().unwrap();
    let services = boot(dir.path());
    let host = HostMode::Desktop {
        computer: "STUDY-PC".to_owned(),
    };
    let state = web_state(&services, host, true, Vec::new());
    set_password(&state);
    let signed_in = call(
        &state,
        "POST",
        "/api/login",
        "192.168.1.30",
        &[],
        Some(serde_json::json!({ "password": PASSWORD })),
    )
    .await;
    let cookie = format!("cv_session={}", signed_in.session().unwrap());
    let page = call(
        &state,
        "GET",
        "/movies/1",
        "192.168.1.30",
        &[("cookie", &cookie)],
        None,
    )
    .await;
    assert_eq!(page.status, StatusCode::OK);
    assert!(page.body.contains("__CINEVAULT_TRANSPORT__=\"server\""));
    assert!(page.body.contains("<body>app</body>"));
    let traversal = call(
        &state,
        "GET",
        "/..%2F..%2Fserver-auth.json",
        "192.168.1.30",
        &[("cookie", &cookie)],
        None,
    )
    .await;
    assert!(!traversal.body.contains("argon2"));
    services.shutdown();
}

// --- The server inside the desktop app, over real TCP. ---

/// A one-request HTTP/1.1 client: enough for the tests, with `Connection: close`.
async fn http(
    port: u16,
    method: &str,
    path: &str,
    cookie: Option<&str>,
    extra: &[(&str, &str)],
    body: Option<&str>,
) -> std::io::Result<(u16, String, String)> {
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port)).await?;
    let mut request = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nUser-Agent: test-browser\r\n"
    );
    if let Some(cookie) = cookie {
        request.push_str(&format!("Cookie: {cookie}\r\n"));
    }
    for (name, value) in extra {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    let body = body.unwrap_or("");
    if !body.is_empty() {
        request.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            body.len()
        ));
    }
    request.push_str("\r\n");
    request.push_str(body);
    stream.write_all(request.as_bytes()).await?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await?;
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, rest) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .unwrap_or(0);
    let body = if head
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        dechunk(rest)
    } else {
        rest.to_owned()
    };
    Ok((status, head.to_owned(), body))
}

fn dechunk(mut rest: &str) -> String {
    let mut out = String::new();
    while let Some((size, after)) = rest.split_once("\r\n") {
        let Ok(size) = usize::from_str_radix(size.trim(), 16) else {
            break;
        };
        if size == 0 || after.len() < size {
            break;
        }
        out.push_str(&after[..size]);
        rest = after[size..].trim_start_matches("\r\n");
    }
    out
}

fn cookie_of(head: &str) -> Option<String> {
    head.lines()
        .filter_map(|line| line.strip_prefix("set-cookie: "))
        .find_map(|value| value.strip_prefix("cv_session="))
        .and_then(|rest| rest.split(';').next())
        .map(|id| format!("cv_session={id}"))
}

fn shared(services: &Arc<AppServices>) -> Shared {
    Shared {
        services: Arc::clone(services),
        images: ImageContext::from_services(services),
        borrowed: Borrowed::default(),
    }
}

fn options(port: u16) -> EmbedOptions {
    EmbedOptions {
        port,
        lan_only: true,
        computer: "STUDY-PC".to_owned(),
        ui: ui(),
    }
}

/// A port nothing listens on now.
fn free_port() -> u16 {
    std::net::TcpListener::bind(("0.0.0.0", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

async fn sign_in(port: u16) -> String {
    let body = serde_json::json!({ "password": PASSWORD }).to_string();
    let (status, head, answer) = http(port, "POST", "/api/login", None, &[], Some(&body))
        .await
        .unwrap();
    assert_eq!(status, 200, "{answer}");
    cookie_of(&head).expect("a session cookie")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_embedded_server_needs_a_password() {
    let dir = tempfile::tempdir().unwrap();
    let services = boot(dir.path());
    let result = embed::start(options(free_port()), shared(&services));
    assert!(matches!(result, Err(StartError::NoPassword)));
    services.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn start_sign_in_invoke_and_stop_closes_the_port() {
    let dir = tempfile::tempdir().unwrap();
    let services = boot(dir.path());
    let root = services.location().root().to_path_buf();
    embed::set_password(&root, PASSWORD, PASSWORD).unwrap();
    let port = free_port();
    let running = embed::start(options(port), shared(&services)).expect("it starts");
    assert_eq!(running.port(), port);

    let (status, _, page) = http(port, "GET", "/login", None, &[], None).await.unwrap();
    assert_eq!(status, 200);
    assert!(page.contains("Change it in CineVault's Settings on STUDY-PC."));

    let cookie = sign_in(port).await;
    let (status, _, index) = http(port, "GET", "/", Some(&cookie), &[], None)
        .await
        .unwrap();
    assert_eq!(status, 200);
    let csrf = index
        .split("__CINEVAULT_CSRF__=\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .unwrap()
        .to_owned();
    let invoke =
        serde_json::json!({ "cmd": "library_counts", "args": {}, "client": "t" }).to_string();
    let (status, _, answer) = http(
        port,
        "POST",
        "/api/invoke",
        Some(&cookie),
        &[
            ("x-cinevault-csrf", &csrf),
            ("origin", &format!("http://127.0.0.1:{port}")),
        ],
        Some(&invoke),
    )
    .await
    .unwrap();
    assert_eq!(status, 200);
    assert!(answer.contains("\"ok\":true"), "{answer}");
    // Without the token the command does not run.
    let (status, _, _) = http(
        port,
        "POST",
        "/api/invoke",
        Some(&cookie),
        &[],
        Some(&invoke),
    )
    .await
    .unwrap();
    assert_eq!(status, 403);

    let sessions = running.sessions();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].address, "127.0.0.1");
    assert_eq!(sessions[0].agent, "test-browser");

    running.stop().await;
    assert!(
        tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_err(),
        "the port is closed after stop"
    );

    // A new start begins with no sessions: the old cookie is signed out.
    let again = embed::start(options(port), shared(&services)).expect("it starts again");
    let (status, _, _) = http(port, "GET", "/api/server/info", Some(&cookie), &[], None)
        .await
        .unwrap();
    assert_eq!(status, 401);
    again.stop().await;
    services.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_port_in_use_is_reported_with_a_free_one() {
    let dir = tempfile::tempdir().unwrap();
    let services = boot(dir.path());
    let root = services.location().root().to_path_buf();
    embed::set_password(&root, PASSWORD, PASSWORD).unwrap();
    let holder = std::net::TcpListener::bind(("0.0.0.0", 0)).unwrap();
    let busy = holder.local_addr().unwrap().port();
    match embed::start(options(busy), shared(&services)) {
        Err(StartError::PortInUse { port, free }) => {
            assert_eq!(port, busy);
            assert_ne!(free, busy);
            let message = StartError::PortInUse { port, free }.to_string();
            assert!(message.contains(&format!("Port {busy} is in use by another program")));
        }
        Err(other) => panic!("expected port in use, got {other}"),
        Ok(running) => {
            running.stop().await;
            panic!("expected port in use");
        }
    }
    assert!(matches!(
        embed::start(options(80), shared(&services)),
        Err(StartError::BadPort)
    ));
    drop(holder);
    services.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_restart_on_another_port_closes_the_old_one_and_ends_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let services = boot(dir.path());
    let root = services.location().root().to_path_buf();
    embed::set_password(&root, PASSWORD, PASSWORD).unwrap();
    let first = free_port();
    let running = embed::start(options(first), shared(&services)).unwrap();
    let cookie = sign_in(first).await;
    running.stop().await;
    let second = free_port();
    let running = embed::start(options(second), shared(&services)).unwrap();
    assert!(
        tokio::net::TcpStream::connect(("127.0.0.1", first))
            .await
            .is_err()
    );
    let (status, _, _) = http(second, "GET", "/login", None, &[], None)
        .await
        .unwrap();
    assert_eq!(status, 200);
    let (status, _, _) = http(second, "GET", "/api/server/info", Some(&cookie), &[], None)
        .await
        .unwrap();
    assert_eq!(status, 401);
    running.stop().await;
    services.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn browsers_are_signed_out_one_by_one_or_all_together() {
    let dir = tempfile::tempdir().unwrap();
    let services = boot(dir.path());
    let root = services.location().root().to_path_buf();
    embed::set_password(&root, PASSWORD, PASSWORD).unwrap();
    let port = free_port();
    let running = embed::start(options(port), shared(&services)).unwrap();
    let phone = sign_in(port).await;
    let tv = sign_in(port).await;
    let sessions = running.sessions();
    assert_eq!(sessions.len(), 2);
    assert!(running.sign_out(&sessions[0].handle));
    let alive = [
        futures_status(port, &phone).await,
        futures_status(port, &tv).await,
    ];
    assert_eq!(alive.iter().filter(|status| **status == 200).count(), 1);
    assert_eq!(alive.iter().filter(|status| **status == 401).count(), 1);
    running.sign_out_all();
    assert_eq!(futures_status(port, &phone).await, 401);
    assert_eq!(futures_status(port, &tv).await, 401);

    // Changing the password from the desktop works at once for the running server.
    embed::set_password(
        &root,
        "the second long password",
        "the second long password",
    )
    .unwrap();
    let body = serde_json::json!({ "password": "the second long password" }).to_string();
    let (status, _, _) = http(port, "POST", "/api/login", None, &[], Some(&body))
        .await
        .unwrap();
    assert_eq!(status, 200);
    running.stop().await;
    services.shutdown();
}

async fn futures_status(port: u16, cookie: &str) -> u16 {
    http(port, "GET", "/api/server/info", Some(cookie), &[], None)
        .await
        .unwrap()
        .0
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_open_stream_ends_when_browser_access_stops() {
    let dir = tempfile::tempdir().unwrap();
    let services = boot(dir.path());
    let root = services.location().root().to_path_buf();
    embed::set_password(&root, PASSWORD, PASSWORD).unwrap();
    let port = free_port();
    let running = embed::start(options(port), shared(&services)).unwrap();
    let cookie = sign_in(port).await;
    let reader = tokio::spawn({
        let cookie = cookie.clone();
        async move {
            http(
                port,
                "GET",
                "/api/stream?client=a",
                Some(&cookie),
                &[],
                None,
            )
            .await
        }
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let started = std::time::Instant::now();
    running.stop().await;
    // The stream's response ended (the read returned) instead of keeping the stop waiting.
    let answer = tokio::time::timeout(std::time::Duration::from_secs(5), reader)
        .await
        .expect("the stream ends")
        .unwrap();
    assert!(answer.is_ok());
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    services.shutdown();
}

#[test]
fn the_settings_file_round_trips_and_defaults_to_off() {
    let dir = tempfile::tempdir().unwrap();
    let defaults = BrowserAccessConfig::load(dir.path());
    assert!(!defaults.enabled);
    assert!(defaults.lan_only);
    assert_eq!(defaults.port, embed::DEFAULT_PORT);
    let config = BrowserAccessConfig {
        enabled: true,
        port: 8790,
        lan_only: false,
        keep_in_tray: true,
    };
    config.save(dir.path()).unwrap();
    assert_eq!(BrowserAccessConfig::load(dir.path()), config);
    std::fs::write(dir.path().join(embed::CONFIG_FILE), "{ broken").unwrap();
    assert_eq!(
        BrowserAccessConfig::load(dir.path()),
        BrowserAccessConfig::default()
    );
}

#[test]
fn the_addresses_end_with_localhost() {
    let list = embed::addresses(8787);
    assert_eq!(list.last().unwrap(), "http://localhost:8787");
    for address in &list {
        assert!(address.starts_with("http://") && address.ends_with(":8787"));
    }
}
