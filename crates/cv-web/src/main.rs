//! CineVault's browser server.
//!
//! Boots the same `AppServices` the desktop app and the command line use, serves the built
//! interface and answers the typed command surface over HTTP through the shared `cv-ipc` layer.
//! The command state objects are built once, in [`state`], and every command is one arm of
//! [`dispatch`].

mod auth;
mod dispatch;
mod state;

use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{ConnectInfo, Path as UrlPath, Query, State};
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::sse::{KeepAlive, Sse};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use cv_app::data_location::DataLocation;
use cv_app::services::{AppServices, BootOptions};
use cv_artwork::img::{ImageRequest, ImageResponse};
use cv_db::repo::data_sources::list_sources;
use cv_ipc::detail::parse_id;
use cv_ipc::image::ImageContext;
use ipnet::IpNet;
use serde::Deserialize;
use serde_json::{Value, json};
use state::{Commands, WebState};
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncSeekExt, SeekFrom};
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::io::ReaderStream;

#[derive(Debug, Deserialize)]
struct InvokeBody {
    cmd: String,
    #[serde(default)]
    args: Value,
    #[serde(default)]
    client: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    install_logging();

    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments.first().is_some_and(|arg| arg == "passwd") {
        return passwd(&arguments[1..]);
    }

    let mut ui_dir = PathBuf::from("ui/dist");
    let mut bind = std::env::var("CINEVAULT_WEB_BIND").unwrap_or_else(|_| "127.0.0.1".to_owned());
    let mut port = std::env::var("CINEVAULT_WEB_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(8787_u16);
    let mut args = arguments.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--ui" => ui_dir = PathBuf::from(args.next().ok_or("--ui needs a folder")?),
            "--bind" => bind = args.next().ok_or("--bind needs an address")?,
            "--port" => port = args.next().ok_or("--port needs a number")?.parse()?,
            other => return Err(format!("unknown argument {other}").into()),
        }
    }

    let location = DataLocation::resolve_for_process()
        .ok_or("CineVault could not find a folder for its data.")?;
    let _owner = match cv_app::owner::acquire(location.root(), "server") {
        Ok(Some(guard)) => guard,
        Ok(None) => {
            let current = cv_app::owner::read_owner(location.root());
            let why = current.map_or_else(
                || "Another CineVault process is using this library.".to_owned(),
                |owner| owner.sentence(),
            );
            return Err(format!("{why} Stop it, or choose another data folder.").into());
        }
        Err(error) => return Err(error.into()),
    };
    let data_root = location.root().to_path_buf();
    let auth = Arc::new(auth::Auth::open(location.root())?);
    let (secrets, keys_locked) =
        cv_app::server_secrets::server_store(&data_root).map_err(std::io::Error::other)?;
    let services = AppServices::boot(BootOptions::new(location, Arc::new(secrets)))?;
    services.start();
    let images = Arc::new(ImageContext::from_services(&services));
    let progress = Arc::new(cv_ipc::jobs::ProgressHub::default());
    let notices = cv_ipc::notices::NoticeHub::from_app(Arc::clone(services.notices()));
    let streams = Arc::new(state::Streams::default());
    // Every command's state object, built once from the one set of services. The desktop builds
    // the same objects lazily; the server has no window and one lifetime.
    let commands = Arc::new(Commands::build(&services, &images));
    let changes = Arc::clone(services.changes());
    let change_streams = Arc::clone(&streams);
    changes.subscribe(Arc::new(move |change| {
        let payload = json!({
            "items": change.items.iter().map(|id| id.0.to_string()).collect::<Vec<_>>(),
            "parts": change.parts.iter().map(|part| part.as_str()).collect::<Vec<_>>(),
        });
        change_streams.broadcast_event("items-changed", &payload);
    }));
    let feed_hub = Arc::clone(&progress);
    let feed_manager = services.jobs().clone();
    let list_manager = feed_manager.clone();
    tokio::spawn(async move {
        feed_manager
            .progress_feed(cv_jobs::PROGRESS_INTERVAL)
            .run(move |tick| {
                feed_hub.publish(&tick, || list_manager.list());
            })
            .await;
    });

    let state = WebState {
        services,
        images,
        auth,
        progress,
        notices,
        streams,
        commands,
        data_root: data_root.clone(),
        bind: bind.clone(),
        port,
        keys_locked,
        trust_proxy: parse_trust_proxy(),
        ui_dir: ui_dir.clone(),
    };
    let app = Router::new()
        .route("/healthz", get(health))
        .route("/login", get(login_page))
        .route("/setup", get(setup_page))
        .route("/api/login", post(login))
        .route("/api/setup", post(setup))
        .route("/api/logout", post(logout))
        .route("/api/server/info", get(server_info))
        .route("/api/server/password", post(server_password))
        .route("/api/server/sign-out-all", post(server_sign_out_all))
        .route("/api/invoke", post(invoke))
        .route("/api/stream", get(stream))
        .route("/img/{key}/{size}", get(image))
        .route("/api/media/{item}/{part}", get(media).post(media_download))
        .route("/api/fs/list", get(fs_list))
        .route("/api/fs/mkdir", post(fs_mkdir))
        .fallback(get(static_file))
        .with_state(state);

    let addr: SocketAddr = format!("{bind}:{port}").parse()?;
    println!(
        "cinevault-server on http://{addr}/ (ui from {})",
        ui_dir.display()
    );
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown())
    .await?;
    Ok(())
}

/// `cinevault-server passwd` and `cinevault-server passwd --reset` on the machine that holds the
/// data folder. This is the recovery path when the browser password is lost.
fn passwd(arguments: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let location = DataLocation::resolve_for_process()
        .ok_or("CineVault could not find a folder for its data.")?;
    let path = location.root().join(auth::AUTH_FILE);
    if arguments.iter().any(|arg| arg == "--reset") {
        match std::fs::remove_file(&path) {
            Ok(()) => println!(
                "The password was cleared. Start the server and set a new one in a browser."
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                println!("There is no password to clear.");
            }
            Err(error) => return Err(error.into()),
        }
        return Ok(());
    }
    let auth = auth::Auth::open(location.root())?;
    if auth.has_password() {
        let current = rpassword::prompt_password("Current password: ")?;
        if !auth.check_password(&current) {
            return Err("That password is not right.".into());
        }
    }
    let password = rpassword::prompt_password("New password (at least 12 characters): ")?;
    let confirm = rpassword::prompt_password("Confirm the new password: ")?;
    if password != confirm {
        return Err("The passwords do not match.".into());
    }
    auth.set_password_unattended(&password)?;
    println!("Password set. Open the server in a browser and sign in.");
    Ok(())
}

fn install_logging() {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}

async fn shutdown() {
    let _ = tokio::signal::ctrl_c().await;
}

async fn health(State(state): State<WebState>) -> Response {
    Json(json!({
        "status": "ok",
        "keys": if state.keys_locked { "locked" } else { "configured" }
    }))
    .into_response()
}

fn parse_trust_proxy() -> Vec<IpNet> {
    let Ok(value) = std::env::var("CINEVAULT_WEB_TRUST_PROXY") else {
        return Vec::new();
    };
    value
        .split(',')
        .filter_map(|part| {
            let part = part.trim();
            part.parse::<IpNet>().ok().or_else(|| {
                part.parse::<IpAddr>().ok().map(|ip| {
                    IpNet::new(ip, if ip.is_ipv4() { 32 } else { 128 })
                        .unwrap_or_else(|_| IpNet::new(IpAddr::from([127, 0, 0, 1]), 32).unwrap())
                })
            })
        })
        .collect()
}

fn from_trusted_proxy(peer: IpAddr, trust: &[IpNet]) -> bool {
    trust.iter().any(|net| net.contains(&peer))
}

fn effective_secure(headers: &HeaderMap, peer: IpAddr, trust: &[IpNet]) -> bool {
    from_trusted_proxy(peer, trust)
        && headers
            .get("x-forwarded-proto")
            .and_then(|value| value.to_str().ok())
            == Some("https")
}

fn client_ip(headers: &HeaderMap, peer: IpAddr, trust: &[IpNet]) -> IpAddr {
    if !from_trusted_proxy(peer, trust) {
        return peer;
    }
    headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(peer)
}

fn effective_host(headers: &HeaderMap, peer: IpAddr, trust: &[IpNet]) -> Option<String> {
    let name = if from_trusted_proxy(peer, trust) {
        "x-forwarded-host"
    } else {
        "host"
    };
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

fn origin_matches(headers: &HeaderMap, peer: IpAddr, trust: &[IpNet]) -> bool {
    let Some(origin) = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    else {
        // A non-browser client may omit Origin; the CSRF token still protects the command.
        return true;
    };
    let Some(host) = effective_host(headers, peer, trust) else {
        return false;
    };
    let origin_host = origin.split("://").nth(1).unwrap_or(origin);
    origin_host.eq_ignore_ascii_case(&host)
}

fn session_of(state: &WebState, headers: &HeaderMap) -> Option<(String, auth::Session)> {
    let id = auth::cookie(headers, "cv_session")?;
    let session = state.auth.session(&id)?;
    Some((id, session))
}

fn signed_out() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({
            "ok": false,
            "error": { "code": "signed_out", "message": "Your session ended. Sign in again." }
        })),
    )
        .into_response()
}

fn set_cookie(response: &mut Response, value: String) {
    if let Ok(value) = axum::http::HeaderValue::from_str(&value) {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
}

async fn login_page(State(state): State<WebState>) -> Response {
    if state.auth.needs_setup() {
        return Redirect::to("/setup").into_response();
    }
    axum::response::Html(auth::login_page()).into_response()
}

async fn setup_page(
    State(state): State<WebState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    if state.auth.has_password() {
        return Redirect::to("/login").into_response();
    }
    if state.auth.setup_closed() {
        return (
            StatusCode::FORBIDDEN,
            "The setup window has closed. Run `cinevault-server passwd` on the server.",
        )
            .into_response();
    }
    let existing = auth::cookie(&headers, "cv_setup");
    match state.auth.claim_setup(existing.as_deref()) {
        Ok(claim) => {
            let mut response = axum::response::Html(auth::setup_page()).into_response();
            set_cookie(
                &mut response,
                auth::setup_cookie(
                    &claim,
                    effective_secure(&headers, peer.ip(), &state.trust_proxy),
                ),
            );
            response
        }
        Err(auth::AuthError::SetupClaimed) => (
            StatusCode::CONFLICT,
            "Setup is in progress in another browser. Finish it there.",
        )
            .into_response(),
        Err(_) => Redirect::to("/login").into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct PasswordBody {
    password: String,
}

#[derive(Debug, Deserialize)]
struct SetupBody {
    password: String,
    confirm: String,
}

async fn login(
    State(state): State<WebState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<PasswordBody>,
) -> Response {
    let client = client_ip(&headers, peer.ip(), &state.trust_proxy).to_string();
    match state.auth.login(&body.password, &client) {
        Ok((id, _)) => {
            let mut response = Json(json!({ "ok": true })).into_response();
            set_cookie(
                &mut response,
                auth::session_cookie(
                    &id,
                    effective_secure(&headers, peer.ip(), &state.trust_proxy),
                ),
            );
            response
        }
        Err(error) => {
            let status = match error {
                auth::AuthError::TooManyAttempts => StatusCode::TOO_MANY_REQUESTS,
                _ => StatusCode::UNAUTHORIZED,
            };
            (status, Json(json!({ "message": error.to_string() }))).into_response()
        }
    }
}

async fn setup(
    State(state): State<WebState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<SetupBody>,
) -> Response {
    let claim = auth::cookie(&headers, "cv_setup").unwrap_or_default();
    match state
        .auth
        .set_password(&body.password, &body.confirm, &claim)
    {
        Ok((id, _)) => {
            let mut response = Json(json!({ "ok": true })).into_response();
            set_cookie(
                &mut response,
                auth::session_cookie(
                    &id,
                    effective_secure(&headers, peer.ip(), &state.trust_proxy),
                ),
            );
            set_cookie(
                &mut response,
                auth::clear_cookie(
                    "cv_setup",
                    effective_secure(&headers, peer.ip(), &state.trust_proxy),
                ),
            );
            response
        }
        Err(error) => {
            let status = match error {
                auth::AuthError::TooShort | auth::AuthError::Mismatch => StatusCode::BAD_REQUEST,
                auth::AuthError::SetupClaimed => StatusCode::CONFLICT,
                _ => StatusCode::FORBIDDEN,
            };
            (status, Json(json!({ "message": error.to_string() }))).into_response()
        }
    }
}

async fn server_info(State(state): State<WebState>, headers: HeaderMap) -> Response {
    let Some((_, _)) = session_of(&state, &headers) else {
        return signed_out();
    };
    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "bind": state.bind,
        "port": state.port,
        "sessions": state.auth.live_sessions(),
        "keys": if state.keys_locked { "locked" } else { "configured" },
    }))
    .into_response()
}

#[derive(Debug, Deserialize)]
struct PasswordChangeBody {
    current: String,
    password: String,
    confirm: String,
}

async fn server_password(
    State(state): State<WebState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<PasswordChangeBody>,
) -> Response {
    let Some((session_id, session)) = session_of(&state, &headers) else {
        return signed_out();
    };
    let csrf = headers
        .get("x-cinevault-csrf")
        .and_then(|value| value.to_str().ok());
    if csrf != Some(session.csrf.as_str())
        || !origin_matches(&headers, peer.ip(), &state.trust_proxy)
    {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "message": "Reload the page and try again." })),
        )
            .into_response();
    }
    match state
        .auth
        .change_password(&body.current, &body.password, &body.confirm)
    {
        Ok(()) => {
            state.auth.logout_all_except(Some(&session_id));
            Json(json!({ "ok": true })).into_response()
        }
        Err(error) => {
            let status = match error {
                auth::AuthError::TooShort | auth::AuthError::Mismatch => StatusCode::BAD_REQUEST,
                auth::AuthError::WrongPassword => StatusCode::UNAUTHORIZED,
                _ => StatusCode::FORBIDDEN,
            };
            (status, Json(json!({ "message": error.to_string() }))).into_response()
        }
    }
}

async fn server_sign_out_all(State(state): State<WebState>, headers: HeaderMap) -> Response {
    let Some((session_id, session)) = session_of(&state, &headers) else {
        return signed_out();
    };
    let csrf = headers
        .get("x-cinevault-csrf")
        .and_then(|value| value.to_str().ok());
    if csrf != Some(session.csrf.as_str()) {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "message": "Reload the page and try again." })),
        )
            .into_response();
    }
    state.auth.logout_all_except(Some(&session_id));
    Json(json!({ "ok": true })).into_response()
}

async fn logout(
    State(state): State<WebState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    if let Some(id) = auth::cookie(&headers, "cv_session") {
        state.auth.logout(&id);
    }
    let mut response = Json(json!({ "ok": true })).into_response();
    set_cookie(
        &mut response,
        auth::clear_cookie(
            "cv_session",
            effective_secure(&headers, peer.ip(), &state.trust_proxy),
        ),
    );
    response
}

async fn invoke(
    State(state): State<WebState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<InvokeBody>,
) -> Response {
    let Some((_, session)) = session_of(&state, &headers) else {
        return signed_out();
    };
    let csrf = headers
        .get("x-cinevault-csrf")
        .and_then(|value| value.to_str().ok());
    if csrf != Some(session.csrf.as_str())
        || !origin_matches(&headers, peer.ip(), &state.trust_proxy)
    {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({
                "ok": false,
                "error": { "code": "csrf", "message": "Reload the page and try again." }
            })),
        )
            .into_response();
    }
    match dispatch::dispatch(&state, &body.cmd, &body.args, &body.client).await {
        Ok(data) => Json(json!({ "ok": true, "data": data })).into_response(),
        Err(error) => Json(json!({ "ok": false, "error": error })).into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct ClientQuery {
    #[serde(default)]
    client: String,
}

async fn stream(
    State(state): State<WebState>,
    Query(query): Query<ClientQuery>,
    headers: HeaderMap,
) -> Response {
    if session_of(&state, &headers).is_none() {
        return signed_out();
    }
    let client = if query.client.is_empty() {
        "default".to_owned()
    } else {
        query.client
    };
    let receiver = state.streams.connect(&client);
    Sse::new(ReceiverStream::new(receiver))
        .keep_alive(KeepAlive::default())
        .into_response()
}

#[derive(Debug, Deserialize)]
struct PathQuery {
    #[serde(default)]
    path: String,
}

#[derive(Debug, Deserialize)]
struct MkdirBody {
    path: String,
}

/// The folders the folder browser and the library-folder commands may reach: `CINEVAULT_WEB_ROOTS`
/// when set, otherwise the data folder, the existing library folders and the usual mount points
/// on Linux. `/` is never a root unless the operator asks for it.
pub(crate) fn allowed_roots(state: &WebState) -> Vec<PathBuf> {
    if let Some(value) = std::env::var_os("CINEVAULT_WEB_ROOTS") {
        let separator = if cfg!(windows) { ';' } else { ':' };
        let roots: Vec<PathBuf> = value
            .to_string_lossy()
            .split(separator)
            .filter(|part| !part.is_empty())
            .map(PathBuf::from)
            .collect();
        if !roots.is_empty() {
            return roots;
        }
    }
    let mut roots = vec![state.data_root.clone()];
    // An existing library folder is where the person already keeps media.
    let db = Arc::clone(state.services.db());
    let sources = db.read(|conn| list_sources(conn, None)).unwrap_or_default();
    roots.extend(sources.into_iter().map(|source| PathBuf::from(source.path)));
    let extra: &[&str] = if cfg!(target_os = "linux") {
        &["/media", "/mnt", "/srv"]
    } else {
        &[]
    };
    roots.extend(extra.iter().map(PathBuf::from));
    roots
}

/// Whether `path` is inside one of `roots`, compared a path component at a time.
pub(crate) fn path_inside_roots(path: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|root| {
        let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.clone());
        path.starts_with(root)
    })
}

async fn fs_list(
    State(state): State<WebState>,
    Query(query): Query<PathQuery>,
    headers: HeaderMap,
) -> Response {
    if session_of(&state, &headers).is_none() {
        return signed_out();
    }
    let roots = allowed_roots(&state);
    let requested = if query.path.trim().is_empty() {
        roots
            .first()
            .cloned()
            .unwrap_or_else(|| state.data_root.clone())
    } else {
        PathBuf::from(query.path.trim())
    };
    if !requested.is_absolute() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "message": "The folder must be an absolute path." })),
        )
            .into_response();
    }
    let Ok(folder) = tokio::fs::canonicalize(&requested).await else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "message": "That folder does not exist." })),
        )
            .into_response();
    };
    if !path_inside_roots(&folder, &roots) {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({
                "message": "That folder is outside the folders this server may browse."
            })),
        )
            .into_response();
    }
    let Ok(mut entries) = tokio::fs::read_dir(&folder).await else {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "message": "That folder cannot be read." })),
        )
            .into_response();
    };
    let mut folders = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let Ok(file_type) = entry.file_type().await else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        folders.push(json!({
            "name": entry.file_name().to_string_lossy(),
            "path": entry.path().to_string_lossy(),
        }));
    }
    folders.sort_by(|a, b| {
        a["name"]
            .as_str()
            .unwrap_or("")
            .to_lowercase()
            .cmp(&b["name"].as_str().unwrap_or("").to_lowercase())
    });
    let parent = folder
        .parent()
        .filter(|parent| path_inside_roots(parent, &roots))
        .map(|parent| parent.to_string_lossy().into_owned());
    Json(json!({
        "path": folder.to_string_lossy(),
        "parent": parent,
        "folders": folders,
    }))
    .into_response()
}

async fn fs_mkdir(
    State(state): State<WebState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<MkdirBody>,
) -> Response {
    let Some((_, session)) = session_of(&state, &headers) else {
        return signed_out();
    };
    let csrf = headers
        .get("x-cinevault-csrf")
        .and_then(|value| value.to_str().ok());
    if csrf != Some(session.csrf.as_str())
        || !origin_matches(&headers, peer.ip(), &state.trust_proxy)
    {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "message": "Reload the page and try again." })),
        )
            .into_response();
    }
    let path = PathBuf::from(body.path.trim());
    if !path.is_absolute() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "message": "The folder must be an absolute path." })),
        )
            .into_response();
    }
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "message": "That is not a folder name." })),
        )
            .into_response();
    };
    if name.starts_with('.') {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "message": "A folder name cannot start with a dot." })),
        )
            .into_response();
    }
    let roots = allowed_roots(&state);
    let Some(parent) = path.parent() else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "message": "That folder has no parent." })),
        )
            .into_response();
    };
    let Ok(parent) = tokio::fs::canonicalize(parent).await else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "message": "The parent folder does not exist." })),
        )
            .into_response();
    };
    if !path_inside_roots(&parent, &roots) {
        return (
            StatusCode::FORBIDDEN,
            Json(
                json!({ "message": "That folder is outside the folders this server may browse." }),
            ),
        )
            .into_response();
    }
    let target = parent.join(name);
    if target.exists() {
        return (
            StatusCode::CONFLICT,
            Json(json!({ "message": "That folder already exists." })),
        )
            .into_response();
    }
    match tokio::fs::create_dir(&target).await {
        Ok(()) => Json(json!({ "path": target.to_string_lossy() })).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "message": error.to_string() })),
        )
            .into_response(),
    }
}

/// Plays a title in the browser: the file streams inline, so a `<video>` element can seek.
async fn media(
    State(state): State<WebState>,
    UrlPath((item, part)): UrlPath<(String, u32)>,
    headers: HeaderMap,
) -> Response {
    serve_media(&state, &item, part, &headers, false).await
}

/// Downloads a title's file: the same bytes, always as an attachment, so a browser saves it
/// instead of opening the player. Ranges are answered like the inline route's.
async fn media_download(
    State(state): State<WebState>,
    UrlPath((item, part)): UrlPath<(String, u32)>,
    headers: HeaderMap,
) -> Response {
    serve_media(&state, &item, part, &headers, true).await
}

async fn serve_media(
    state: &WebState,
    item: &str,
    part: u32,
    headers: &HeaderMap,
    download: bool,
) -> Response {
    if session_of(state, headers).is_none() {
        return signed_out();
    }
    let Ok(id) = parse_id(item) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let files = match video_files(state, id).await {
        Ok(files) => files,
        Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };
    let Some(path) = files.get(part as usize) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let path = PathBuf::from(path);
    let Ok(canonical) = tokio::fs::canonicalize(&path).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !inside_library(state, &canonical).await {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Ok(mut file) = tokio::fs::File::open(&canonical).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Ok(metadata) = file.metadata().await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let total = metadata.len();
    if total == 0 {
        return StatusCode::NOT_FOUND.into_response();
    }
    let range = headers
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| parse_range(value, total));
    let (status, start, end) = match range {
        Some((start, end)) => (StatusCode::PARTIAL_CONTENT, start, end),
        None => (StatusCode::OK, 0, total - 1),
    };
    if file.seek(SeekFrom::Start(start)).await.is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let length = end - start + 1;
    let mut builder = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, media_type(&canonical))
        .header(header::CONTENT_LENGTH, length.to_string())
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CACHE_CONTROL, "private, no-store")
        .header("x-content-type-options", "nosniff")
        .header("content-security-policy", "sandbox");
    if download {
        builder = builder.header(header::CONTENT_DISPOSITION, attachment_name(&canonical));
    }
    if status == StatusCode::PARTIAL_CONTENT {
        builder = builder.header(
            header::CONTENT_RANGE,
            format!("bytes {start}-{end}/{total}"),
        );
    }
    builder
        .body(Body::from_stream(ReaderStream::new(file.take(length))))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

/// `attachment; filename="…"; filename*=UTF-8''…`, with a quoted and an RFC 5987 name so a
/// release name with spaces, quotes or accents saves under its own name.
fn attachment_name(path: &Path) -> String {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    let quoted: String = name
        .chars()
        .map(|character| match character {
            '"' | '\\' | '\r' | '\n' => '_',
            other => other,
        })
        .collect();
    let mut encoded = String::new();
    for byte in name.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(char::from(*byte));
            }
            other => encoded.push_str(&format!("%{other:02X}")),
        }
    }
    format!("attachment; filename=\"{quoted}\"; filename*=UTF-8''{encoded}")
}

fn parse_range(value: &str, total: u64) -> Option<(u64, u64)> {
    let value = value.strip_prefix("bytes=")?;
    let (start, end) = value.split_once('-')?;
    let start = start.trim().parse::<u64>().ok()?;
    let end = if end.trim().is_empty() {
        total - 1
    } else {
        end.trim().parse::<u64>().ok()?.min(total - 1)
    };
    (start <= end && start < total).then_some((start, end))
}

fn media_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "mkv" => "video/x-matroska",
        "mov" => "video/quicktime",
        "avi" => "video/x-msvideo",
        "ts" | "m2ts" => "video/mp2t",
        _ => "application/octet-stream",
    }
}

async fn video_files(state: &WebState, id: cv_domain::ItemId) -> Result<Vec<String>, Value> {
    let db = Arc::clone(state.services.db());
    tokio::task::spawn_blocking(move || {
        db.read(|conn| {
            let mut statement = conn.prepare(
                "SELECT directory, filename FROM media_file WHERE item_id = ?1 AND kind = 'video' \
                 ORDER BY position, id",
            )?;
            let rows = statement.query_map([id.0], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            let mut files = Vec::new();
            for row in rows {
                let (directory, filename) = row?;
                files.push(
                    std::path::Path::new(&directory)
                        .join(&filename)
                        .to_string_lossy()
                        .into_owned(),
                );
            }
            Ok(files)
        })
    })
    .await
    .map_err(|error| json!({ "code": "storage", "message": error.to_string() }))?
    .map_err(|error| json!({ "code": "storage", "message": error.to_string() }))
}

async fn inside_library(state: &WebState, path: &Path) -> bool {
    let db = Arc::clone(state.services.db());
    let sources =
        tokio::task::spawn_blocking(move || db.read(|conn| list_sources(conn, None))).await;
    let Ok(Ok(sources)) = sources else {
        return false;
    };
    sources.iter().any(|source| {
        let root =
            std::fs::canonicalize(&source.path).unwrap_or_else(|_| PathBuf::from(&source.path));
        path.starts_with(root)
    })
}

async fn image(State(state): State<WebState>, uri: Uri, headers: HeaderMap) -> Response {
    if session_of(&state, &headers).is_none() {
        return signed_out();
    }
    let Some(service) = state.images.service().cloned() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let path = uri.path().to_owned();
    let if_none_match = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let answer = tokio::task::spawn_blocking(move || {
        service.handle(&ImageRequest {
            method: "GET",
            path: &path,
            if_none_match: if_none_match.as_deref(),
        })
    })
    .await;
    let Ok(answer) = answer else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    image_response(answer)
}

fn image_response(answer: ImageResponse) -> Response {
    let mut builder = Response::builder().status(answer.status);
    for (name, value) in &answer.headers {
        builder = builder.header(*name, value);
    }
    builder
        .body(Body::from(answer.body))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

async fn static_file(State(state): State<WebState>, uri: Uri, headers: HeaderMap) -> Response {
    let requested = uri.path().trim_start_matches('/');
    let is_asset = requested.starts_with("assets/")
        || requested == "favicon.ico"
        || requested == "favicon.svg"
        || requested == "robots.txt"
        || [
            ".js", ".css", ".png", ".jpg", ".jpeg", ".webp", ".svg", ".woff", ".woff2", ".map",
        ]
        .iter()
        .any(|extension| requested.ends_with(extension));
    let session = if is_asset {
        None
    } else {
        session_of(&state, &headers)
    };
    if !is_asset && session.is_none() {
        return Redirect::to(if state.auth.needs_setup() {
            "/setup"
        } else {
            "/login"
        })
        .into_response();
    }
    let mut file = state.ui_dir.join(if requested.is_empty() {
        "index.html"
    } else {
        requested
    });
    if !file.starts_with(&state.ui_dir) {
        return StatusCode::NOT_FOUND.into_response();
    }
    if !file.is_file() {
        file = state.ui_dir.join("index.html");
    }
    let Ok(mut body) = fs::read(&file).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if file.file_name().is_some_and(|name| name == "index.html") {
        let csrf = session
            .as_ref()
            .map(|(_, session)| session.csrf.as_str())
            .unwrap_or("");
        let marker = format!(
            "<script>window.__CINEVAULT_TRANSPORT__=\"server\";window.__CINEVAULT_CSRF__=\"{csrf}\";</script>"
        );
        let text = String::from_utf8_lossy(&body).replace("</head>", &format!("{marker}</head>"));
        body = text.into_bytes();
    }
    let mime = match file
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
    {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        _ => "application/octet-stream",
    };
    ([(header::CONTENT_TYPE, mime)], body).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn ranges_are_closed_and_clamped() {
        assert_eq!(parse_range("bytes=0-99", 1000), Some((0, 99)));
        assert_eq!(parse_range("bytes=500-", 1000), Some((500, 999)));
        assert_eq!(parse_range("bytes=900-2000", 1000), Some((900, 999)));
        assert_eq!(parse_range("bytes=1000-", 1000), None);
        assert_eq!(parse_range("bytes=99-1", 1000), None);
        assert_eq!(parse_range("not a range", 1000), None);
    }

    #[test]
    fn media_types_are_an_allowlist() {
        assert_eq!(media_type(Path::new("film.mkv")), "video/x-matroska");
        assert_eq!(media_type(Path::new("film.MP4")), "video/mp4");
        assert_eq!(
            media_type(Path::new("notes.strm")),
            "application/octet-stream"
        );
        assert_eq!(
            media_type(Path::new("no-extension")),
            "application/octet-stream"
        );
    }

    #[test]
    fn a_download_saves_under_the_file_name() {
        let name = attachment_name(Path::new("/media/Film (2019)/Film.mkv"));
        assert_eq!(
            name,
            "attachment; filename=\"Film.mkv\"; filename*=UTF-8''Film.mkv"
        );
        let awkward = attachment_name(Path::new("/media/Blade Runner \"Final\".mkv"));
        assert!(awkward.contains("filename=\"Blade Runner _Final_.mkv\""));
        assert!(awkward.contains("Blade%20Runner%20%22Final%22.mkv"));
    }

    #[test]
    fn a_same_origin_matches_the_host() {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, HeaderValue::from_static("nas:8787"));
        headers.insert(header::ORIGIN, HeaderValue::from_static("http://nas:8787"));
        let peer = IpAddr::from([127, 0, 0, 1]);
        assert!(origin_matches(&headers, peer, &[]));
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("http://evil.example"),
        );
        assert!(!origin_matches(&headers, peer, &[]));
    }
}
