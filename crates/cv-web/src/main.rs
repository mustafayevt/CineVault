//! CineVault's browser server.
//!
//! Boots the same `AppServices` the desktop app and the command line use, serves the built
//! interface and answers the typed command surface over HTTP through the shared `cv-ipc` layer.
//! This is the read-path skeleton: sign-in, the write commands and the SSE stream land next.

mod auth;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex as StdMutex};

use axum::body::Body;
use axum::extract::{ConnectInfo, Path as UrlPath, Query, State};
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::sse::{Event, KeepAlive, Sse};
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
use cv_ipc::drives::{drives_overview as drives_read, drives_trash_contents};
use cv_ipc::history::{HistoryFilterDto, history_for_title, history_list};
use cv_ipc::image::ImageContext;
use cv_ipc::library::{
    LibraryKindDto, LibraryQueryDto, LibraryReader, library_counts, library_facet_values,
    library_filter_catalogue, library_size_total, library_source_groups, list_library_rows,
    list_library_window, search_library_people,
};
use cv_ipc::lifecycle::{LifecycleCountsDto, service as lifecycle_service};
use cv_ipc::scan::{ScanState, find_missing_files, read_pending_media_info, rescan_library};
use cv_ipc::tv::{TvReader, get_episode_detail, get_show_detail};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncSeekExt, SeekFrom};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::io::ReaderStream;

type SseSender = mpsc::Sender<Result<Event, std::convert::Infallible>>;

#[derive(Default)]
struct Streams {
    clients: StdMutex<HashMap<String, SseSender>>,
}

impl Streams {
    fn connect(&self, client: &str) -> mpsc::Receiver<Result<Event, std::convert::Infallible>> {
        let (sender, receiver) = mpsc::channel(256);
        if let Ok(mut clients) = self.clients.lock() {
            clients.insert(client.to_owned(), sender);
        }
        receiver
    }

    fn send(&self, client: &str, event: Event) -> bool {
        let Ok(clients) = self.clients.lock() else {
            return false;
        };
        let Some(sender) = clients.get(client) else {
            return false;
        };
        sender.try_send(Ok(event)).is_ok()
    }

    fn channel<T: serde::Serialize>(
        &self,
        client: &str,
        stream: u64,
        index: &AtomicU64,
        message: &T,
    ) -> bool {
        let index = index.fetch_add(1, AtomicOrdering::Relaxed);
        let data = json!({
            "kind": "channel",
            "stream": stream,
            "index": index,
            "message": message,
        });
        self.send(client, Event::default().data(data.to_string()))
    }

    #[expect(dead_code)] // Used by the items-changed relay next.
    fn event<T: serde::Serialize>(&self, client: &str, name: &str, payload: &T) -> bool {
        let data = json!({ "kind": "event", "event": name, "payload": payload });
        self.send(client, Event::default().data(data.to_string()))
    }
}

#[derive(Clone)]
struct WebState {
    services: Arc<AppServices>,
    images: Arc<ImageContext>,
    auth: Arc<auth::Auth>,
    progress: Arc<cv_ipc::jobs::ProgressHub>,
    notices: Arc<cv_ipc::notices::NoticeHub>,
    streams: Arc<Streams>,
    data_root: PathBuf,
    bind: String,
    port: u16,
    ui_dir: PathBuf,
}

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
    let services = AppServices::boot(
        BootOptions::new(location, Arc::new(SecretStore::in_memory()))
            .without_providers()
            .without_background_nfo(),
    )?;
    services.start();
    let images = Arc::new(ImageContext::from_services(&services));
    let progress = Arc::new(cv_ipc::jobs::ProgressHub::default());
    let notices = cv_ipc::notices::NoticeHub::from_app(Arc::clone(services.notices()));
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
        streams: Arc::new(Streams::default()),
        data_root: data_root.clone(),
        bind: bind.clone(),
        port,
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
        .route("/api/media/{item}/{part}", get(media))
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

async fn server_info(State(state): State<WebState>, headers: HeaderMap) -> Response {
    let Some((_, _)) = session_of(&state, &headers) else {
        return signed_out();
    };
    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "bind": state.bind,
        "port": state.port,
        "sessions": state.auth.live_sessions(),
        "keys": "unknown",
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
    headers: HeaderMap,
    Json(body): Json<PasswordChangeBody>,
) -> Response {
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
    match dispatch(&state, &body.cmd, &body.args, &body.client).await {
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

async fn dispatch(state: &WebState, cmd: &str, args: &Value, client: &str) -> DispatchResult {
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
        "rescan_library" => {
            let scan = ScanState::from_services(Some(&state.services));
            rescan_library(&scan)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "find_missing_files" => {
            let scan = ScanState::from_services(Some(&state.services));
            find_missing_files(&scan)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "read_pending_media_info" => {
            let scan = ScanState::from_services(Some(&state.services));
            read_pending_media_info(&scan)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "drives_trash_contents" => {
            let drive_id: Option<String> = args
                .get("driveId")
                .and_then(|value| value.as_str())
                .map(str::to_owned);
            drives_trash_contents(drives(state), drive_id)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "get_update_status" => Ok(json!({
            "current_version": env!("CARGO_PKG_VERSION"),
            "channel_set": false,
            "enabled": false,
            "offer": null
        })),
        "subscribe_job_progress" => {
            let Some(stream) = args.get("onProgress").and_then(channel_id) else {
                return Err(bad_request("onProgress is not a channel"));
            };
            let index = Arc::new(AtomicU64::new(0));
            let streams = Arc::clone(&state.streams);
            let client = client.to_owned();
            cv_ipc::jobs::subscribe_job_progress(
                &state.progress,
                state.services.jobs(),
                move |update| streams.channel(&client, stream, &index, &update),
            );
            Ok(Value::Null)
        }
        "subscribe_notices" => {
            let Some(stream) = args.get("onNotice").and_then(channel_id) else {
                return Err(bad_request("onNotice is not a channel"));
            };
            let index = Arc::new(AtomicU64::new(0));
            let streams = Arc::clone(&state.streams);
            let client = client.to_owned();
            state
                .notices
                .subscribe(move |notice| streams.channel(&client, stream, &index, &notice));
            Ok(Value::Null)
        }
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

#[derive(Debug, Deserialize)]
struct ClientQuery {
    #[serde(default)]
    client: String,
}

fn channel_id(value: &Value) -> Option<u64> {
    value.as_str()?.strip_prefix("__CHANNEL__:")?.parse().ok()
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

fn allowed_roots(state: &WebState) -> Vec<PathBuf> {
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
    let extra: &[&str] = if cfg!(target_os = "linux") {
        &["/media", "/mnt", "/srv"]
    } else {
        &[]
    };
    roots.extend(extra.iter().map(PathBuf::from));
    roots
}

fn inside_roots(path: &Path, roots: &[PathBuf]) -> bool {
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
    if !inside_roots(&folder, &roots) {
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
        .filter(|parent| inside_roots(parent, &roots))
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
    headers: HeaderMap,
    Json(body): Json<MkdirBody>,
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
    if !inside_roots(&parent, &roots) {
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

async fn media(
    State(state): State<WebState>,
    UrlPath((item, part)): UrlPath<(String, u32)>,
    headers: HeaderMap,
) -> Response {
    if session_of(&state, &headers).is_none() {
        return signed_out();
    }
    let Ok(id) = parse_id(&item) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let files = match video_files(&state, id).await {
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
    if !inside_library(&state, &canonical).await {
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
