//! CineVault's browser server.
//!
//! Boots the same `AppServices` the desktop app and the command line use, serves the built
//! interface and answers the typed command surface over HTTP through the shared `cv-ipc` layer.
//! This is the read-path skeleton: sign-in, the write commands and the SSE stream land next.

mod auth;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use cv_app::data_location::DataLocation;
use cv_app::drives::DrivesService;
use cv_app::history::HistoryService;
use cv_app::scanning::SourceState;
use cv_app::secrets::SecretStore;
use cv_app::services::{AppServices, BootOptions};
use cv_artwork::img::{ImageRequest, ImageResponse};
use cv_db::repo::data_sources::list_sources;
use cv_domain::ItemKind;
use cv_ipc::detail::{DetailReader, get_movie_detail, parse_id};
use cv_ipc::drives::drives_overview as drives_read;
use cv_ipc::history::{HistoryFilterDto, history_for_title, history_list};
use cv_ipc::image::ImageContext;
use cv_ipc::library::{
    LibraryKindDto, LibraryQueryDto, LibraryReader, library_counts, library_facet_values,
    library_filter_catalogue, library_size_total, library_source_groups, list_library_rows,
    list_library_window, search_library_people,
};
use cv_ipc::lifecycle::{LifecycleCountsDto, service as lifecycle_service};
use cv_ipc::tv::{TvReader, get_episode_detail, get_show_detail};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::fs;

#[derive(Clone)]
struct WebState {
    services: Arc<AppServices>,
    images: Arc<ImageContext>,
    auth: Arc<auth::Auth>,
    ui_dir: PathBuf,
}

#[derive(Debug, Deserialize)]
struct InvokeBody {
    cmd: String,
    #[serde(default)]
    args: Value,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    install_logging();

    let mut ui_dir = PathBuf::from("ui/dist");
    let mut bind = "127.0.0.1".to_owned();
    let mut port = 8787_u16;
    let mut args = std::env::args().skip(1);
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
    let auth = Arc::new(auth::Auth::open(location.root())?);
    let services = AppServices::boot(
        BootOptions::new(location, Arc::new(SecretStore::in_memory()))
            .without_providers()
            .without_background_nfo(),
    )?;
    services.start();
    let images = Arc::new(ImageContext::from_services(&services));

    let state = WebState {
        services,
        images,
        auth,
        ui_dir: ui_dir.clone(),
    };
    let app = Router::new()
        .route("/healthz", get(health))
        .route("/login", get(login_page))
        .route("/setup", get(setup_page))
        .route("/api/login", post(login))
        .route("/api/setup", post(setup))
        .route("/api/logout", post(logout))
        .route("/api/invoke", post(invoke))
        .route("/api/stream", get(stream))
        .route("/img/{key}/{size}", get(image))
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

fn install_logging() {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}

async fn shutdown() {
    let _ = tokio::signal::ctrl_c().await;
}

async fn health(State(state): State<WebState>) -> Response {
    let _ = &state.services;
    Json(json!({ "status": "ok" })).into_response()
}

fn is_secure(headers: &HeaderMap) -> bool {
    headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        == Some("https")
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

async fn setup_page(State(state): State<WebState>, headers: HeaderMap) -> Response {
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
                auth::setup_cookie(&claim, is_secure(&headers)),
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
    match state.auth.login(&body.password, &peer.ip().to_string()) {
        Ok((id, _)) => {
            let mut response = Json(json!({ "ok": true })).into_response();
            set_cookie(
                &mut response,
                auth::session_cookie(&id, is_secure(&headers)),
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
                auth::session_cookie(&id, is_secure(&headers)),
            );
            set_cookie(
                &mut response,
                auth::clear_cookie("cv_setup", is_secure(&headers)),
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

async fn logout(State(state): State<WebState>, headers: HeaderMap) -> Response {
    if let Some(id) = auth::cookie(&headers, "cv_session") {
        state.auth.logout(&id);
    }
    let mut response = Json(json!({ "ok": true })).into_response();
    set_cookie(
        &mut response,
        auth::clear_cookie("cv_session", is_secure(&headers)),
    );
    response
}

fn reader(state: &WebState) -> LibraryReader {
    LibraryReader::from_parts(
        Arc::clone(state.services.db()),
        Arc::clone(state.images.sources()),
        state
            .images
            .service()
            .map(|service| Arc::clone(service.cache())),
        Some(Arc::clone(state.services.lifecycle())),
    )
}

fn detail_reader(state: &WebState) -> DetailReader {
    DetailReader::new(
        Arc::clone(state.services.db()),
        Arc::clone(state.images.sources()),
        state
            .images
            .service()
            .map(|service| Arc::clone(service.cache())),
    )
    .with_lifecycle(Some(Arc::clone(state.services.lifecycle())))
    .with_rating_status(Some(Arc::clone(state.services.rating_status())))
}

fn tv_reader(state: &WebState) -> TvReader {
    TvReader::new(
        Arc::clone(state.services.db()),
        Arc::clone(state.images.sources()),
        state
            .images
            .service()
            .map(|service| Arc::clone(service.cache())),
    )
    .with_lifecycle(Some(Arc::clone(state.services.lifecycle())))
    .with_rating_status(Some(Arc::clone(state.services.rating_status())))
}

fn history(state: &WebState) -> Arc<HistoryService> {
    Arc::new(HistoryService::new(Arc::clone(state.services.db())))
}

fn drives(state: &WebState) -> Option<Arc<DrivesService>> {
    Some(DrivesService::new(
        Arc::clone(state.services.db()),
        Arc::clone(state.services.scan()),
    ))
}

async fn invoke(
    State(state): State<WebState>,
    headers: HeaderMap,
    Json(body): Json<InvokeBody>,
) -> Response {
    let Some((_, session)) = session_of(&state, &headers) else {
        return signed_out();
    };
    let csrf = headers
        .get("x-cinevault-csrf")
        .and_then(|value| value.to_str().ok());
    if csrf != Some(session.csrf.as_str()) {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({
                "ok": false,
                "error": { "code": "csrf", "message": "Reload the page and try again." }
            })),
        )
            .into_response();
    }
    match dispatch(&state, &body.cmd, &body.args).await {
        Ok(data) => Json(json!({ "ok": true, "data": data })).into_response(),
        Err(error) => Json(json!({ "ok": false, "error": error })).into_response(),
    }
}

type DispatchResult = Result<Value, Value>;

fn bad_request(error: impl std::fmt::Display) -> Value {
    json!({ "code": "bad_request", "message": format!("bad argument: {error}") })
}

fn arg<T: serde::de::DeserializeOwned>(args: &Value, key: &str) -> Result<T, Value> {
    serde_json::from_value(args.get(key).cloned().unwrap_or(Value::Null))
        .map_err(|error| bad_request(format!("{key}: {error}")))
}

fn to_value<T: serde::Serialize>(value: T) -> DispatchResult {
    serde_json::to_value(value).map_err(bad_request)
}

fn command_error(error: impl serde::Serialize) -> Value {
    serde_json::to_value(error).unwrap_or_else(
        |_| json!({ "code": "storage", "message": "the library could not be read" }),
    )
}

async fn dispatch(state: &WebState, cmd: &str, args: &Value) -> DispatchResult {
    match cmd {
        "list_library_window" => {
            let query: LibraryQueryDto = arg(args, "query")?;
            let offset: u32 = arg(args, "offset")?;
            let limit: u32 = arg(args, "limit")?;
            list_library_window(reader(state), query, offset, limit)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "list_library_rows" => {
            let query: LibraryQueryDto = arg(args, "query")?;
            let offset: u32 = arg(args, "offset")?;
            let limit: u32 = arg(args, "limit")?;
            list_library_rows(reader(state), query, offset, limit)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "library_counts" => library_counts(reader(state))
            .await
            .map_err(command_error)
            .and_then(to_value),
        "library_size_total" => {
            let kind: LibraryKindDto = arg(args, "kind")?;
            library_size_total(reader(state), kind)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "library_source_groups" => {
            let query: LibraryQueryDto = arg(args, "query")?;
            library_source_groups(reader(state), query)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "library_facet_values" => {
            let query: LibraryQueryDto = arg(args, "query")?;
            let facet: String = arg(args, "facet")?;
            library_facet_values(reader(state), query, facet)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "search_library_people" => {
            let term: String = arg(args, "term")?;
            let limit: u32 = arg(args, "limit")?;
            search_library_people(reader(state), term, limit)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "library_filter_catalogue" => {
            let kind: LibraryKindDto = arg(args, "kind")?;
            to_value(library_filter_catalogue(kind))
        }
        "list_data_sources" => {
            let db = Arc::clone(state.services.db());
            let sources =
                tokio::task::spawn_blocking(move || db.read(|conn| list_sources(conn, None)))
                    .await
                    .map_err(|error| json!({ "code": "storage", "message": error.to_string() }))?
                    .map_err(|error| json!({ "code": "storage", "message": error.to_string() }))?;
            to_value(
                sources
                    .into_iter()
                    .map(|source| {
                        json!({
                            "id": u32::try_from(source.id).unwrap_or(u32::MAX),
                            "kind": match source.kind {
                                cv_domain::SourceKind::Movie => "movie",
                                cv_domain::SourceKind::Tv => "tv",
                            },
                            "path": source.path,
                            "label": source.label,
                            "position": u32::try_from(source.position).unwrap_or(u32::MAX),
                        })
                    })
                    .collect::<Vec<_>>(),
            )
        }
        "get_settings" => {
            let snapshot = state.services.settings().snapshot();
            to_value(json!({
                "values": snapshot
                    .values
                    .into_iter()
                    .map(|(key, value)| (key, value.to_string()))
                    .collect::<std::collections::BTreeMap<_, _>>(),
                "notice": snapshot.notice,
            }))
        }
        "lifecycle_counts" => {
            let kind: LibraryKindDto = arg(args, "kind")?;
            let item_kind = ItemKind::from(kind);
            let service =
                lifecycle_service(Some(state.services.lifecycle())).map_err(command_error)?;
            tokio::task::spawn_blocking(move || service.counts(item_kind))
                .await
                .map_err(|error| json!({ "code": "storage", "message": error.to_string() }))?
                .map(LifecycleCountsDto::from)
                .map_err(|error| command_error(cv_ipc::library::LibraryCommandError::from(error)))
                .and_then(to_value)
        }
        "list_source_health" => {
            let scan = Arc::clone(state.services.scan());
            let health = tokio::task::spawn_blocking(move || scan.source_health())
                .await
                .map_err(|error| json!({ "code": "storage", "message": error.to_string() }))?
                .map_err(|error| json!({ "code": "storage", "message": error.to_string() }))?;
            to_value(
                health
                    .into_iter()
                    .map(|health| {
                        json!({
                            "id": u32::try_from(health.source.id).unwrap_or(u32::MAX),
                            "kind": match health.source.kind {
                                cv_domain::SourceKind::Movie => "movie",
                                cv_domain::SourceKind::Tv => "tv",
                            },
                            "path": health.source.path,
                            "online": health.state == SourceState::Online,
                            "titles": u32::try_from(health.counts.titles).unwrap_or(u32::MAX),
                            "missing": u32::try_from(health.counts.missing).unwrap_or(u32::MAX),
                            "pending_media_info": u32::try_from(health.counts.pending_media_info).unwrap_or(u32::MAX),
                        })
                    })
                    .collect::<Vec<_>>(),
            )
        }
        "count_review_items" => {
            let service =
                lifecycle_service(Some(state.services.lifecycle())).map_err(command_error)?;
            tokio::task::spawn_blocking(move || {
                let of = |kind| service.counts(kind).map(|counts| counts.needs_review);
                let movies = of(ItemKind::Movie)?;
                let shows = of(ItemKind::Show)?;
                Ok::<_, cv_db::DbError>(movies.saturating_add(shows))
            })
            .await
            .map_err(|error| json!({ "code": "storage", "message": error.to_string() }))?
            .map_err(|error| json!({ "code": "storage", "message": error.to_string() }))
            .map(Value::from)
        }
        "get_movie_detail" => {
            let id: String = arg(args, "id")?;
            let id = parse_id(&id).map_err(command_error)?;
            get_movie_detail(detail_reader(state), id)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "get_show_detail" => {
            let id: String = arg(args, "id")?;
            let id = parse_id(&id).map_err(command_error)?;
            get_show_detail(tv_reader(state), id)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "get_episode_detail" => {
            let id: String = arg(args, "id")?;
            let id = parse_id(&id).map_err(command_error)?;
            get_episode_detail(tv_reader(state), id)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "history_for_title" => {
            let id: String = arg(args, "id")?;
            history_for_title(history(state), id)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "history_list" => {
            let filter: HistoryFilterDto = arg(args, "filter")?;
            let offset: u32 = arg(args, "offset")?;
            let limit: u32 = arg(args, "limit")?;
            history_list(history(state), filter, offset, limit)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "drives_overview" => drives_read(drives(state))
            .await
            .map_err(command_error)
            .and_then(to_value),
        "get_update_status" => Ok(json!({
            "current_version": env!("CARGO_PKG_VERSION"),
            "channel_set": false,
            "enabled": false,
            "offer": null
        })),
        "subscribe_job_progress" | "subscribe_notices" => Ok(Value::Null),
        "set_setting" | "clear_new_on_open" => Ok(Value::Null),
        "plugin:event|listen" | "plugin:event|unlisten" => Ok(Value::Null),
        "plugin:window|is_maximized" => Ok(Value::Bool(false)),
        "plugin:webview|set_webview_zoom" | "plugin:window|set_title" => Ok(Value::Null),
        other => {
            eprintln!("cinevault-server: unhandled command {other}");
            Err(json!({
                "code": "not_implemented",
                "message": format!("{other} is not served yet")
            }))
        }
    }
}

async fn stream(State(state): State<WebState>, headers: HeaderMap) -> Response {
    if session_of(&state, &headers).is_none() {
        return signed_out();
    }
    // The stream is a placeholder until task 3.5: an open SSE connection with keep-alives, so the
    // shim does not retry in a loop.
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(": cinevault stream\n\n"))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
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
