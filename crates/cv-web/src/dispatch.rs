//! The server's command dispatcher: one arm per command in `ui/src/bindings.ts`.
//!
//! Every arm calls the `cv-ipc` function the desktop app calls, with the same arguments, so a
//! fix or a feature is written once. What is left here is transport: reading the arguments out of
//! the request, handing a command's progress to the page's event stream, and the answers a
//! browser has no way to perform (starting a program on the client, the desktop installer).
//!
//! `every_command_in_the_bindings_has_an_arm` fails when a command the interface can invoke has
//! no arm, so a command added to the desktop registry cannot be forgotten here.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use cv_ipc::detail::{DetailReader, get_movie_detail, parse_id};
use cv_ipc::history::HistoryFilterDto;
use cv_ipc::library::{
    LibraryKindDto, LibraryQueryDto, LibraryReader, library_counts, library_facet_values,
    library_filter_catalogue, library_size_total, library_source_groups, list_library_rows,
    list_library_window, search_library_people,
};
use cv_ipc::tv::{TvReader, get_episode_detail, get_show_detail};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::state::{DeleteGuard, WebState};

/// What a command answers: its value, or the typed `{code, message}` error.
pub(crate) type DispatchResult = Result<Value, Value>;

pub(crate) fn bad_request(error: impl std::fmt::Display) -> Value {
    json!({ "code": "bad_request", "message": format!("bad argument: {error}") })
}

/// The browser has no equivalent of this command. The interface hides its control.
pub(crate) fn not_applicable(message: impl Into<String>) -> Value {
    json!({ "code": "not_applicable", "message": message.into() })
}

fn storage(error: impl std::fmt::Display) -> Value {
    json!({ "code": "storage", "message": error.to_string() })
}

fn arg<T: DeserializeOwned>(args: &Value, key: &str) -> Result<T, Value> {
    serde_json::from_value(args.get(key).cloned().unwrap_or(Value::Null))
        .map_err(|error| bad_request(format!("{key}: {error}")))
}

fn to_value<T: Serialize>(value: T) -> DispatchResult {
    serde_json::to_value(value).map_err(bad_request)
}

fn command_error(error: impl Serialize) -> Value {
    serde_json::to_value(error).unwrap_or_else(
        |_| json!({ "code": "storage", "message": "the library could not be read" }),
    )
}

/// A channel argument: the interface sends `__CHANNEL__:<callback id>`.
pub(crate) fn channel_id(value: &Value) -> Option<u64> {
    value.as_str()?.strip_prefix("__CHANNEL__:")?.parse().ok()
}

fn channel_arg(args: &Value, key: &str) -> Result<u64, Value> {
    args.get(key)
        .and_then(channel_id)
        .ok_or_else(|| bad_request(format!("{key} is not a channel")))
}

/// A value sink that sends every value to the page's stream as one channel message.
fn channel_sink<T: Serialize + Send + Sync + 'static>(
    state: &WebState,
    client: &str,
    stream: u64,
) -> cv_ipc::sink::Sink<T> {
    let streams = Arc::clone(&state.streams);
    let client = client.to_owned();
    let index = Arc::new(AtomicU64::new(0));
    Arc::new(move |value: T| {
        // A page that is gone simply drops the value; the work goes on.
        streams.channel(&client, stream, &index, &value);
    })
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
    let fallback =
        cv_app::tv_info::season_art_fallback(&state.services.settings().snapshot().values);
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
    .with_season_art_fallback(fallback)
}

/// Reads work off the async workers: the commands in `cv-ipc` that are not `async` still read
/// the library or the settings file.
async fn on_blocking<T, F>(work: F) -> Result<T, Value>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    // A blocking thread has no task-local; this keeps the command's client there.
    cv_jobs::client::spawn_blocking(work)
        .await
        .map_err(|error| storage(error.to_string()))
}

/// The browser's side of a torrent hand-off: the server never starts a program on the client, so
/// nothing is opened here. The command still runs the shared checks (the notice, the magnet's
/// shape, a page's address) and then answers `not_applicable`, and the page opens the link with
/// `window.open` or offers "Copy magnet link".
pub(crate) struct BrowserHandoff;

impl cv_ipc::torrent::Handoff for BrowserHandoff {
    fn open_url(&self, _url: &str) -> Result<(), String> {
        // The page opens the address; the server only validated it.
        Ok(())
    }

    fn open_path(&self, _path: &Path) -> Result<(), String> {
        // A browser downloads the file itself; the server writes nothing.
        Ok(())
    }
}

/// The server's side of the update commands: the server updates by pulling a new image or
/// installing a package, so nothing is opened, installed or quit here. Busy jobs still come from
/// the one job manager both surfaces share.
pub(crate) struct ServerUpdatePlatform {
    pub(crate) jobs: cv_jobs::JobManager,
}

impl cv_ipc::updates::UpdatePlatform for ServerUpdatePlatform {
    fn open_url(&self, _url: &str) -> Result<(), String> {
        Err("The server opens nothing: the release notes are a link in the page.".to_owned())
    }

    fn open_path(&self, _path: &Path) -> Result<(), String> {
        Err("The server is updated by pulling a new image or installing a package.".to_owned())
    }

    fn quit(&self) {}

    fn busy_jobs(&self) -> u32 {
        let busy = self
            .jobs
            .list()
            .iter()
            .filter(|job| {
                matches!(
                    job.state,
                    cv_jobs::JobState::Running | cv_jobs::JobState::Queued
                )
            })
            .count();
        u32::try_from(busy).unwrap_or(u32::MAX)
    }
}

/// Refuses a path outside the folders this server may browse. The library folder commands go
/// through the same guard as the folder browser, so a command cannot bypass the roots.
fn inside_roots(state: &WebState, path: &str) -> Result<(), Value> {
    let folder = std::path::PathBuf::from(path.trim());
    if !folder.is_absolute() {
        return Err(json!({
            "code": "bad_path",
            "message": "The folder must be an absolute path."
        }));
    }
    let roots = crate::allowed_roots(state);
    let resolved = std::fs::canonicalize(&folder).unwrap_or(folder);
    if crate::path_inside_roots(&resolved, &roots) {
        Ok(())
    } else {
        Err(json!({
            "code": "outside_roots",
            "message": "That folder is outside the folders this server may browse."
        }))
    }
}

/// Where a browser may take an artwork image from: a web address, or a file it uploaded to the
/// server's staging folder (`<data>/tmp/artwork-*`). Anything else, a `file:` address, another
/// server path, a quoted or `..` path that only looks staged, is refused, so a command cannot
/// copy a private server file into the media folders or delete one afterwards. Returns the staged
/// file to remove once it was placed.
fn artwork_source(uploads: &Path, source: &str) -> Result<Option<std::path::PathBuf>, Value> {
    let refused = || {
        json!({
            "code": "not_allowed",
            "message": "Upload the image from this browser instead of naming a file on the server."
        })
    };
    // The same trimming the artwork service applies before it reads the source.
    let text = source.trim().trim_matches(['"', '\'']).trim();
    let lower = text.to_ascii_lowercase();
    if lower.starts_with("https://") || lower.starts_with("http://") {
        return Ok(None);
    }
    let path = Path::new(text);
    let plain = path.components().all(|component| {
        matches!(
            component,
            std::path::Component::Normal(_)
                | std::path::Component::RootDir
                | std::path::Component::Prefix(_)
        )
    });
    if !path.is_absolute() || !plain {
        return Err(refused());
    }
    let named = path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("artwork-"));
    let parent = path
        .parent()
        .and_then(|parent| std::fs::canonicalize(parent).ok());
    let staging = std::fs::canonicalize(uploads).ok();
    if !named || parent.is_none() || parent != staging {
        return Err(refused());
    }
    Ok(Some(path.to_path_buf()))
}

/// Runs one command with the page's client id as the client of every job it starts, so the other
/// sessions' progress cards can say "Started from another session".
pub(crate) async fn dispatch(
    state: &WebState,
    cmd: &str,
    args: &Value,
    client: &str,
) -> DispatchResult {
    cv_jobs::client::scope(client.to_owned(), run_command(state, cmd, args, client)).await
}

async fn run_command(state: &WebState, cmd: &str, args: &Value, client: &str) -> DispatchResult {
    if state.keys_locked && cmd == "set_provider_credential" {
        return Err(json!({
            "code": "keys_locked",
            "message": "Provider keys are locked. Set CINEVAULT_SECRET_KEY_FILE and restart the server."
        }));
    }
    let commands = &state.commands;
    let library = &commands.library;
    match cmd {
        // ---- development aids ------------------------------------------------------------
        "ping" => {
            let message: String = arg(args, "message")?;
            to_value(json!({
                "echo": message,
                "appVersion": env!("CARGO_PKG_VERSION"),
                "sequence": 1,
            }))
        }
        "register_image_source" => Err(not_applicable(
            "The browser's images come from the library queries, which register them.",
        )),
        "start_sample_job" => Err(not_applicable(
            "The sample job is a development aid of the desktop app.",
        )),

        // ---- jobs ------------------------------------------------------------------------
        "subscribe_job_progress" => {
            let stream = channel_arg(args, "onProgress")?;
            let streams = Arc::clone(&state.streams);
            let client = client.to_owned();
            let index = Arc::new(AtomicU64::new(0));
            cv_ipc::jobs::subscribe_job_progress(
                &state.progress,
                state.services.jobs(),
                move |update| streams.channel(&client, stream, &index, &update),
            );
            Ok(Value::Null)
        }
        "cancel_job" => {
            let id: u32 = arg(args, "id")?;
            to_value(state.services.jobs().cancel(cv_jobs::JobId(u64::from(id))))
        }
        "set_job_paused" => {
            let id: u32 = arg(args, "id")?;
            let paused: bool = arg(args, "paused")?;
            to_value(
                state
                    .services
                    .activity()
                    .set_job_paused(cv_jobs::JobId(u64::from(id)), paused),
            )
        }
        "subscribe_notices" => {
            let stream = channel_arg(args, "onNotice")?;
            let streams = Arc::clone(&state.streams);
            let client = client.to_owned();
            let index = Arc::new(AtomicU64::new(0));
            state
                .notices
                .subscribe(move |notice| streams.channel(&client, stream, &index, &notice));
            Ok(Value::Null)
        }

        // ---- settings --------------------------------------------------------------------
        "get_settings" => {
            let settings = Arc::clone(&state.commands);
            on_blocking(move || to_value(cv_ipc::settings::get_settings(&settings.settings)))
                .await?
        }
        "set_setting" => {
            let key: String = arg(args, "key")?;
            let value_json: String = arg(args, "valueJson")?;
            // The interface throws this as a sentence, so the answer is a plain message.
            cv_ipc::settings::set_setting(&commands.settings, key, value_json)
                .await
                .map_err(Value::String)
                .and_then(to_value)
        }
        "reset_settings" => {
            let keys: Vec<String> = arg(args, "keys")?;
            cv_ipc::settings::reset_settings(&commands.settings, keys)
                .await
                .map_err(Value::String)
                .and_then(to_value)
        }

        // ---- library folders -------------------------------------------------------------
        "list_data_sources" => {
            let commands = Arc::clone(&state.commands);
            on_blocking(move || {
                cv_ipc::sources::list_data_sources(&commands.library).map_err(command_error)
            })
            .await?
            .and_then(to_value)
        }
        "add_data_source" => {
            let kind: cv_ipc::sources::SourceKindDto = arg(args, "kind")?;
            let path: String = arg(args, "path")?;
            inside_roots(state, &path)?;
            cv_ipc::sources::add_data_source(library, kind, path)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "remove_data_source" => {
            let id: u32 = arg(args, "id")?;
            cv_ipc::sources::remove_data_source(library, id)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "reorder_data_sources" => {
            let kind: cv_ipc::sources::SourceKindDto = arg(args, "kind")?;
            let ids: Vec<u32> = arg(args, "ids")?;
            let commands = Arc::clone(&state.commands);
            on_blocking(move || {
                cv_ipc::sources::reorder_data_sources(&commands.library, kind, ids)
                    .map_err(command_error)
            })
            .await?
            .and_then(to_value)
        }
        "remap_data_source" => {
            let id: u32 = arg(args, "id")?;
            let path: String = arg(args, "path")?;
            inside_roots(state, &path)?;
            cv_ipc::sources::remap_data_source(library, id, path)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "change_data_source_kind" => {
            let id: u32 = arg(args, "id")?;
            let kind: cv_ipc::sources::SourceKindDto = arg(args, "kind")?;
            cv_ipc::sources::change_data_source_kind(library, id, kind)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "rename_data_source_label" => {
            let id: u32 = arg(args, "id")?;
            let label: String = arg(args, "label")?;
            let commands = Arc::clone(&state.commands);
            on_blocking(move || {
                cv_ipc::sources::rename_data_source_label(&commands.library, id, label)
                    .map_err(command_error)
            })
            .await?
            .and_then(to_value)
        }
        "analyze_library_folder" => {
            let path: String = arg(args, "path")?;
            inside_roots(state, &path)?;
            cv_ipc::onboarding::analyze_library_folder(library, path)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }

        // ---- library reads ----------------------------------------------------------------
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
        "library_filter_catalogue" => {
            let kind: LibraryKindDto = arg(args, "kind")?;
            to_value(library_filter_catalogue(kind))
        }
        "search_library_people" => {
            let term: String = arg(args, "term")?;
            let limit: u32 = arg(args, "limit")?;
            search_library_people(reader(state), term, limit)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "lifecycle_counts" => {
            let kind: LibraryKindDto = arg(args, "kind")?;
            cv_ipc::lifecycle::lifecycle_counts(library, kind)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "get_lifecycles" => {
            let ids: Vec<String> = arg(args, "ids")?;
            cv_ipc::lifecycle::get_lifecycles(library, ids)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "count_review_items" => cv_ipc::matching::count_review_items(&commands.matching, library)
            .await
            .map_err(command_error)
            .and_then(to_value),

        // ---- source health and the scan ---------------------------------------------------
        "list_source_health" => cv_ipc::scan::list_source_health(&commands.scan)
            .await
            .map_err(command_error)
            .and_then(to_value),
        "rescan_library" => cv_ipc::scan::rescan_library(&commands.scan)
            .await
            .map_err(command_error)
            .and_then(to_value),
        "find_missing_files" => cv_ipc::scan::find_missing_files(&commands.scan)
            .await
            .map_err(command_error)
            .and_then(to_value),
        "read_pending_media_info" => cv_ipc::scan::read_pending_media_info(&commands.scan)
            .await
            .map_err(command_error)
            .and_then(to_value),

        // ---- drives ----------------------------------------------------------------------
        "drives_overview" => cv_ipc::drives::drives_overview(commands.drives.clone())
            .await
            .map_err(command_error)
            .and_then(to_value),
        "drives_trash_contents" => {
            let drive_id: Option<String> = arg(args, "driveId")?;
            cv_ipc::drives::drives_trash_contents(commands.drives.clone(), drive_id)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "empty_drives_trash" => {
            let drive_id: Option<String> = arg(args, "driveId")?;
            let confirm: String = arg(args, "confirm")?;
            cv_ipc::drives::empty_drives_trash(commands.drives.clone(), drive_id, confirm)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }

        // ---- detail pages -----------------------------------------------------------------
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
        "get_cast_portraits" => {
            let id: String = arg(args, "id")?;
            let id = parse_id(&id).map_err(command_error)?;
            let cache = state
                .images
                .service()
                .map(|service| Arc::clone(service.cache()));
            cv_ipc::people::get_cast_portraits(
                Arc::clone(state.services.db()),
                cache,
                commands.matching.artwork.clone(),
                id,
            )
            .await
            .map_err(command_error)
            .and_then(to_value)
        }
        "get_person_imdb_id" => {
            let tmdb_id: String = arg(args, "tmdbId")?;
            let service = commands.matching.get().map_err(command_error)?;
            cv_ipc::people::get_person_imdb_id(service, tmdb_id)
                .await
                .map_err(|message| json!({ "code": "storage", "message": message }))
                .and_then(to_value)
        }
        "get_collection" => {
            let id: String = arg(args, "id")?;
            let id = cv_ipc::edit::id_of(&id).map_err(command_error)?;
            let Some(service) = commands.collections.clone() else {
                return Err(storage(
                    "CineVault could not open the library, so collections are not available.",
                ));
            };
            let cache = state
                .images
                .service()
                .map(|service| Arc::clone(service.cache()));
            cv_ipc::collections::get_collection(
                service,
                Arc::clone(state.services.db()),
                Arc::clone(state.images.sources()),
                cache,
                id,
            )
            .await
            .map_err(command_error)
            .and_then(to_value)
        }

        // ---- watched state and the NEW flag -----------------------------------------------
        "set_movie_watched" => {
            let id: String = arg(args, "id")?;
            let watched: bool = arg(args, "watched")?;
            let id = parse_id(&id).map_err(command_error)?;
            cv_ipc::curation::mark_watched(
                library,
                &commands.nfo,
                &commands.curation,
                &commands.settings,
                vec![id],
                watched,
            )
            .await
            .map_err(command_error)
            .and_then(to_value)
        }
        "set_episodes_watched" => {
            let ids: Vec<String> = arg(args, "ids")?;
            let watched: bool = arg(args, "watched")?;
            let ids = ids
                .iter()
                .map(|id| parse_id(id))
                .collect::<Result<Vec<_>, _>>()
                .map_err(command_error)?;
            cv_ipc::curation::mark_watched(
                library,
                &commands.nfo,
                &commands.curation,
                &commands.settings,
                ids,
                watched,
            )
            .await
            .map_err(command_error)
            .and_then(to_value)
        }
        "set_watched" => {
            let ids: Vec<String> = arg(args, "ids")?;
            let watched: bool = arg(args, "watched")?;
            cv_ipc::curation::set_watched(
                library,
                &commands.nfo,
                &commands.curation,
                &commands.settings,
                ids,
                watched,
            )
            .await
            .map_err(command_error)
            .and_then(to_value)
        }
        "toggle_watched" => {
            let ids: Vec<String> = arg(args, "ids")?;
            cv_ipc::curation::toggle_watched(
                library,
                &commands.nfo,
                &commands.curation,
                &commands.settings,
                ids,
            )
            .await
            .map_err(command_error)
            .and_then(to_value)
        }
        "restore_watched" => {
            let previous: Vec<cv_ipc::curation::WatchStateDto> = arg(args, "previous")?;
            cv_ipc::curation::restore_watched(
                library,
                &commands.nfo,
                &commands.curation,
                &commands.settings,
                previous,
            )
            .await
            .map_err(command_error)
            .and_then(to_value)
        }
        "clear_new_flags" => {
            let ids: Vec<String> = arg(args, "ids")?;
            cv_ipc::curation::clear_new_flags(library, &commands.curation, ids)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "clear_new_on_open" => {
            let id: String = arg(args, "id")?;
            cv_ipc::curation::clear_new_on_open(library, &commands.curation, id)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "clear_all_new_flags" => {
            let kind: Option<String> = arg(args, "kind")?;
            cv_ipc::curation::clear_all_new_flags(library, &commands.curation, kind)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "restore_new_flags" => {
            let ids: Vec<String> = arg(args, "ids")?;
            cv_ipc::curation::restore_new_flags(library, &commands.curation, ids)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }

        // ---- edit and locks ---------------------------------------------------------------
        "get_item_edit" => {
            let id: String = arg(args, "id")?;
            cv_ipc::edit::get_item_edit(library, &commands.nfo, &commands.edit, id)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "save_item_edit" => {
            let id: String = arg(args, "id")?;
            let changes: Vec<cv_ipc::edit::EditFieldDto> = arg(args, "changes")?;
            cv_ipc::edit::save_item_edit(
                library,
                &commands.nfo,
                &commands.edit,
                &commands.settings,
                id,
                changes,
            )
            .await
            .map_err(command_error)
            .and_then(to_value)
        }
        "set_field_lock" => {
            let id: String = arg(args, "id")?;
            let field: String = arg(args, "field")?;
            let locked: bool = arg(args, "locked")?;
            cv_ipc::edit::set_field_lock(library, &commands.nfo, &commands.edit, id, field, locked)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "set_item_lock" => {
            let id: String = arg(args, "id")?;
            let locked: bool = arg(args, "locked")?;
            cv_ipc::edit::set_item_lock(library, &commands.nfo, &commands.edit, id, locked)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }

        // ---- NFO ---------------------------------------------------------------------------
        "read_nfo" => {
            let scope: cv_ipc::nfo::NfoScopeDto = arg(args, "scope")?;
            cv_ipc::nfo::read_nfo(library, &commands.nfo, &commands.settings, scope)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "rewrite_nfo" => {
            let scope: cv_ipc::nfo::NfoScopeDto = arg(args, "scope")?;
            cv_ipc::nfo::rewrite_nfo(library, &commands.nfo, &commands.settings, scope)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "undo_nfo_action" => {
            let token: String = arg(args, "token")?;
            cv_ipc::nfo::undo_nfo_action(library, &commands.nfo, token)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "preview_movie_nfo" => {
            let id: String = arg(args, "id")?;
            cv_ipc::nfo::preview_movie_nfo(library, &commands.nfo, &commands.settings, id)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "preview_show_nfo" => {
            let id: String = arg(args, "id")?;
            cv_ipc::nfo::preview_show_nfo(library, &commands.nfo, &commands.settings, id)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "preview_episode_nfo" => {
            let id: String = arg(args, "id")?;
            cv_ipc::nfo::preview_episode_nfo(library, &commands.nfo, &commands.settings, id)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }

        // ---- selection actions -------------------------------------------------------------
        "run_item_action" => {
            let ids: Vec<String> = arg(args, "ids")?;
            let action: String = arg(args, "action")?;
            cv_ipc::actions::run_item_action(
                Some(&state.services),
                Some(Arc::clone(&commands.actions)),
                ids,
                action,
            )
            .await
            .map_err(command_error)
            .and_then(to_value)
        }

        // ---- matching and the scrape workspace --------------------------------------------
        "start_match_batch" => {
            let mode: cv_ipc::matching::MatchModeDto = arg(args, "mode")?;
            let item_ids: Option<Vec<String>> = arg(args, "itemIds")?;
            cv_ipc::matching::start_match_batch(&commands.matching, mode, item_ids)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "match_library_items" => {
            let item_ids: Vec<String> = arg(args, "itemIds")?;
            cv_ipc::matching::match_library_items(&commands.matching, item_ids)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "list_review_items" => cv_ipc::matching::list_review_items(&commands.matching)
            .await
            .map_err(command_error)
            .and_then(to_value),
        "accept_review_candidate" => {
            let review_id: u32 = arg(args, "reviewId")?;
            let provider: String = arg(args, "provider")?;
            let provider_id: String = arg(args, "providerId")?;
            cv_ipc::matching::accept_review_candidate(
                &commands.matching,
                review_id,
                provider,
                provider_id,
            )
            .await
            .map_err(command_error)
            .and_then(to_value)
        }
        "accept_top_review_candidates" => {
            let review_ids: Vec<u32> = arg(args, "reviewIds")?;
            cv_ipc::matching::accept_top_review_candidates(&commands.matching, review_ids)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "ignore_review_item" => {
            let review_id: u32 = arg(args, "reviewId")?;
            cv_ipc::matching::ignore_review_item(&commands.matching, review_id)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "reopen_review_items" => {
            let review_ids: Vec<u32> = arg(args, "reviewIds")?;
            cv_ipc::matching::reopen_review_items(&commands.matching, review_ids)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "search_review_candidates" => {
            let review_id: u32 = arg(args, "reviewId")?;
            let query: String = arg(args, "query")?;
            cv_ipc::matching::search_review_candidates(&commands.matching, review_id, query)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "list_metadata_providers" => cv_ipc::matching::list_metadata_providers(&commands.matching)
            .await
            .map_err(command_error)
            .and_then(to_value),
        "apply_scrape" => {
            let item_id: String = arg(args, "itemId")?;
            let request: cv_ipc::matching::ScrapeRequestDto = arg(args, "request")?;
            cv_ipc::matching::apply_scrape(&commands.matching, item_id, request)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "undo_scrape" => {
            let token: String = arg(args, "token")?;
            cv_ipc::matching::undo_scrape(&commands.matching, token)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "search_new_title" => {
            let item_id: String = arg(args, "itemId")?;
            let query: String = arg(args, "query")?;
            let language: String = arg(args, "language")?;
            cv_ipc::matching::search_new_title(&commands.matching, item_id, query, language)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "current_match_candidate" => {
            let item_id: String = arg(args, "itemId")?;
            let language: String = arg(args, "language")?;
            cv_ipc::matching::current_match_candidate(&commands.matching, item_id, language)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "accept_confident_titles" => {
            let item_ids: Vec<String> = arg(args, "itemIds")?;
            cv_ipc::matching::accept_confident_titles(&commands.matching, item_ids)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "scrape_field_selection" => {
            let kind: cv_ipc::matching::MatchKindDto = arg(args, "kind")?;
            let preset: cv_ipc::scrape_preview::ScrapePresetDto = arg(args, "preset")?;
            let custom: Vec<String> = arg(args, "custom")?;
            to_value(cv_ipc::scrape_preview::scrape_field_selection(
                kind, preset, custom,
            ))
        }
        "cancel_search" => {
            let search_id: String = arg(args, "searchId")?;
            to_value(cv_ipc::provider_stream::cancel_search(
                &commands.search,
                &search_id,
            ))
        }
        "retry_provider" => {
            let search_id: String = arg(args, "searchId")?;
            let provider: String = arg(args, "provider")?;
            cv_ipc::provider_stream::retry_provider(&commands.search, &search_id, &provider)
                .map_err(command_error)
                .and_then(to_value)
        }
        "stream_scrape_candidates" => {
            let item_id: String = arg(args, "itemId")?;
            let query: String = arg(args, "query")?;
            let kind: cv_ipc::matching::MatchKindDto = arg(args, "kind")?;
            let language: String = arg(args, "language")?;
            let search_id: String = arg(args, "searchId")?;
            let stream = channel_arg(args, "onEvent")?;
            let sink = channel_sink::<
                cv_ipc::provider_stream::ProviderEvent<cv_ipc::scrape_stream::ScrapeStreamHitDto>,
            >(state, client, stream);
            cv_ipc::scrape_stream::stream_scrape_candidates(
                &commands.matching,
                &commands.search,
                item_id,
                query,
                kind,
                language,
                search_id,
                sink,
            )
            .await
            .map_err(command_error)
            .and_then(to_value)
        }
        "stream_scrape_preview" => {
            let item_id: String = arg(args, "itemId")?;
            let candidate: cv_ipc::scrape_preview::PreviewCandidateDto = arg(args, "candidate")?;
            let language: String = arg(args, "language")?;
            let search_id: String = arg(args, "searchId")?;
            let stream = channel_arg(args, "onEvent")?;
            let sink = channel_sink::<
                cv_ipc::provider_stream::ProviderEvent<cv_ipc::scrape_preview::FieldDiffDto>,
            >(state, client, stream);
            cv_ipc::scrape_preview::stream_scrape_preview(
                &commands.matching,
                &commands.search,
                item_id,
                candidate,
                language,
                search_id,
                sink,
            )
            .await
            .map_err(command_error)
            .and_then(to_value)
        }

        // ---- artwork ----------------------------------------------------------------------
        "stream_artwork_candidates" => {
            let item_id: String = arg(args, "itemId")?;
            let slot: String = arg(args, "slot")?;
            let search_id: String = arg(args, "searchId")?;
            let stream = channel_arg(args, "onEvent")?;
            let sink = channel_sink::<
                cv_ipc::provider_stream::ProviderEvent<cv_ipc::artwork::ArtworkCandidateDto>,
            >(state, client, stream);
            cv_ipc::artwork_stream::stream_artwork_candidates(
                &commands.artwork,
                &commands.search,
                item_id,
                slot,
                search_id,
                sink,
            )
            .await
            .map_err(command_error)
            .and_then(to_value)
        }
        "artwork_candidate_counts" => {
            let item_id: String = arg(args, "itemId")?;
            cv_ipc::artwork_stream::artwork_candidate_counts(&commands.artwork, item_id)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "artwork_full_image" => {
            let candidate_id: String = arg(args, "candidateId")?;
            cv_ipc::artwork::artwork_full_image(
                &commands.artwork,
                state.images.sources(),
                candidate_id,
            )
            .await
            .map_err(command_error)
            .and_then(to_value)
        }
        "place_artwork" => {
            let item_id: String = arg(args, "itemId")?;
            let slot: String = arg(args, "slot")?;
            let source: String = arg(args, "source")?;
            // A browser may only place an image it uploaded to the server's staging folder. A
            // pasted `file:` address or any other server path would copy a private file into the
            // media folders.
            let staged = artwork_source(&state.data_root.join("tmp"), &source)?;
            let placed = cv_ipc::artwork::place_artwork(&commands.artwork, item_id, slot, source)
                .await
                .map_err(command_error)
                .and_then(to_value);
            if let Some(path) = staged {
                let _ = tokio::fs::remove_file(&path).await;
            }
            placed
        }
        "place_extra_artwork" => {
            let item_id: String = arg(args, "itemId")?;
            let slot: String = arg(args, "slot")?;
            let sources: Vec<String> = arg(args, "sources")?;
            let uploads = state.data_root.join("tmp");
            let staged = sources
                .iter()
                .map(|source| artwork_source(&uploads, source))
                .collect::<Result<Vec<_>, _>>()?;
            let placed =
                cv_ipc::artwork::place_extra_artwork(&commands.artwork, item_id, slot, sources)
                    .await
                    .map_err(command_error)
                    .and_then(to_value);
            for path in staged.into_iter().flatten() {
                let _ = tokio::fs::remove_file(&path).await;
            }
            placed
        }
        "download_missing_artwork" => {
            let item_ids: Vec<String> = arg(args, "itemIds")?;
            cv_ipc::artwork::download_missing_artwork(&commands.artwork, item_ids)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "fill_season_artwork" => {
            let item_id: String = arg(args, "itemId")?;
            let season: Option<u32> = arg(args, "season")?;
            cv_ipc::artwork::fill_season_artwork(&commands.artwork, item_id, season)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }

        // ---- provider credentials and ratings ---------------------------------------------
        "list_provider_credentials" => {
            let commands = Arc::clone(&state.commands);
            on_blocking(move || {
                cv_ipc::providers::list_provider_credentials(&commands.providers)
                    .map_err(command_error)
            })
            .await?
            .and_then(to_value)
        }
        "set_provider_credential" => {
            let provider: String = arg(args, "provider")?;
            let field: String = arg(args, "field")?;
            let value: String = arg(args, "value")?;
            cv_ipc::providers::set_provider_credential(&commands.providers, provider, field, value)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "remove_provider_credentials" => {
            let provider: String = arg(args, "provider")?;
            let commands = Arc::clone(&state.commands);
            on_blocking(move || {
                cv_ipc::providers::remove_provider_credentials(&commands.providers, provider)
                    .map_err(command_error)
            })
            .await?
            .and_then(to_value)
        }
        "restore_provider_credentials" => {
            let provider: String = arg(args, "provider")?;
            cv_ipc::providers::restore_provider_credentials(&commands.providers, provider)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "test_provider_credentials" => {
            let provider: String = arg(args, "provider")?;
            let drafts: Vec<cv_ipc::providers::CredentialDraft> = arg(args, "drafts")?;
            cv_ipc::providers::test_provider_credentials(&commands.providers, provider, drafts)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "get_key_attention" => {
            let commands = Arc::clone(&state.commands);
            on_blocking(move || {
                cv_ipc::providers::get_key_attention(&commands.providers).map_err(command_error)
            })
            .await?
            .and_then(to_value)
        }
        "list_provider_attributions" => {
            let commands = Arc::clone(&state.commands);
            on_blocking(move || {
                cv_ipc::providers::list_provider_attributions(&commands.providers)
                    .map_err(command_error)
            })
            .await?
            .and_then(to_value)
        }
        "fetch_imdb_top250" => {
            let list: cv_ipc::ratings::Top250ListDto = arg(args, "list")?;
            cv_ipc::ratings::fetch_imdb_top250(library, list)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "count_rating_sweep" => {
            let scope: cv_ipc::ratings::RatingSweepScopeDto = arg(args, "scope")?;
            let provider: Option<String> = arg(args, "provider")?;
            cv_ipc::ratings::count_rating_sweep(Some(&state.services), scope, provider)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "start_rating_sweep" => {
            let scope: cv_ipc::ratings::RatingSweepScopeDto = arg(args, "scope")?;
            let provider: Option<String> = arg(args, "provider")?;
            let stream = channel_arg(args, "onDone")?;
            let sink = channel_sink(state, client, stream);
            cv_ipc::ratings::start_rating_sweep(Some(&state.services), scope, provider, sink)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }

        // ---- History, delete and restore ---------------------------------------------------
        "history_list" => {
            let filter: HistoryFilterDto = arg(args, "filter")?;
            let offset: u32 = arg(args, "offset")?;
            let limit: u32 = arg(args, "limit")?;
            cv_ipc::history::history_list(
                Arc::clone(&commands.history_reads),
                filter,
                offset,
                limit,
            )
            .await
            .map_err(command_error)
            .and_then(to_value)
        }
        "history_titles" => {
            let entry_id: String = arg(args, "entryId")?;
            cv_ipc::history::history_titles(Arc::clone(&commands.history_reads), entry_id)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "history_for_title" => {
            let id: String = arg(args, "id")?;
            cv_ipc::history::history_for_title(Arc::clone(&commands.history_reads), id)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "history_entry_for_batch" => {
            let batch_id: String = arg(args, "batchId")?;
            cv_ipc::history::history_entry_for_batch(Arc::clone(&commands.history_reads), batch_id)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "history_bin_total" => {
            cv_ipc::history::history_bin_total(Arc::clone(&commands.history_reads))
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "history_undo" => {
            let entry_id: String = arg(args, "entryId")?;
            cv_ipc::history::history_undo(
                library,
                &commands.history,
                &commands.artwork,
                &commands.matching,
                entry_id,
            )
            .await
            .map_err(command_error)
            .and_then(to_value)
        }
        "history_undo_title" => {
            let entry_id: String = arg(args, "entryId")?;
            let id: String = arg(args, "id")?;
            cv_ipc::history::history_undo_title(
                library,
                &commands.history,
                &commands.artwork,
                &commands.matching,
                entry_id,
                id,
            )
            .await
            .map_err(command_error)
            .and_then(to_value)
        }
        "delete_titles" => {
            let ids: Vec<String> = arg(args, "ids")?;
            cv_ipc::history::delete_titles(library, &commands.history, ids)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "erase_titles" => {
            let ids: Vec<String> = arg(args, "ids")?;
            let confirm: String = arg(args, "confirm")?;
            let intent: String = arg(args, "intent")?;
            let password: String = arg(args, "password")?;
            if !DeleteGuard::enabled(&state.services)
                || !state.auth.check_password(&password)
                || !state.delete_guard.consume(&intent, &ids)
            {
                return Err(json!({
                    "code": "not_allowed",
                    "message": "Permanent delete needs the current password and a fresh confirmation."
                }));
            }
            cv_ipc::history::erase_titles(library, &commands.history, ids, confirm)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }

        // ---- moving, removing and the review inbox ----------------------------------------
        "plan_change_source" => {
            let ids: Vec<String> = arg(args, "ids")?;
            let target: u32 = arg(args, "target")?;
            cv_ipc::library_ops::plan_change_source(library, &commands.library_ops, ids, target)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "preview_change_source_title" => {
            let id: String = arg(args, "id")?;
            let target: u32 = arg(args, "target")?;
            cv_ipc::library_ops::preview_change_source_title(
                library,
                &commands.library_ops,
                id,
                target,
            )
            .await
            .map_err(command_error)
            .and_then(to_value)
        }
        "apply_change_source" => {
            let ids: Vec<String> = arg(args, "ids")?;
            let target: u32 = arg(args, "target")?;
            let stream = channel_arg(args, "onProgress")?;
            let sink = channel_sink(state, client, stream);
            cv_ipc::library_ops::apply_change_source(
                library,
                &commands.library_ops,
                ids,
                target,
                sink,
            )
            .await
            .map_err(command_error)
            .and_then(to_value)
        }
        "control_move" => {
            let action: cv_ipc::library_ops::MoveControlDto = arg(args, "action")?;
            let commands = Arc::clone(&state.commands);
            on_blocking(move || {
                cv_ipc::library_ops::control_move(&commands.library_ops, action);
                Ok::<Value, Value>(Value::Null)
            })
            .await?
        }
        "list_unfinished_batches" => {
            cv_ipc::library_ops::list_unfinished_batches(library, &commands.library_ops)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "resume_unfinished_batch" => {
            let batch_id: String = arg(args, "batchId")?;
            let stream = channel_arg(args, "onProgress")?;
            let sink = channel_sink(state, client, stream);
            cv_ipc::library_ops::resume_unfinished_batch(
                library,
                &commands.library_ops,
                batch_id,
                sink,
            )
            .await
            .map_err(command_error)
            .and_then(to_value)
        }
        "roll_back_unfinished_batch" => {
            let batch_id: String = arg(args, "batchId")?;
            cv_ipc::library_ops::roll_back_unfinished_batch(
                library,
                &commands.library_ops,
                batch_id,
            )
            .await
            .map_err(command_error)
            .and_then(to_value)
        }
        "plan_removal" => {
            let ids: Vec<String> = arg(args, "ids")?;
            cv_ipc::library_ops::plan_removal(library, &commands.library_ops, ids)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "remove_from_library" => {
            let ids: Vec<String> = arg(args, "ids")?;
            let ignore_folders: bool = arg(args, "ignoreFolders")?;
            cv_ipc::library_ops::remove_from_library(
                library,
                &commands.library_ops,
                ids,
                ignore_folders,
            )
            .await
            .map_err(command_error)
            .and_then(to_value)
        }
        "delete_with_files" => {
            let ids: Vec<String> = arg(args, "ids")?;
            cv_ipc::library_ops::delete_with_files(library, &commands.library_ops, ids)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "list_library_review" => {
            cv_ipc::library_ops::list_library_review(library, &commands.library_ops)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "remove_review_rows" => {
            let ids: Vec<String> = arg(args, "ids")?;
            cv_ipc::library_ops::remove_review_rows(library, &commands.library_ops, ids)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "undo_review_removal" => {
            let token: String = arg(args, "token")?;
            cv_ipc::library_ops::undo_review_removal(library, &commands.library_ops, token)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "keep_review_rows" => {
            let ids: Vec<String> = arg(args, "ids")?;
            cv_ipc::library_ops::keep_review_rows(library, &commands.library_ops, ids)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "unkeep_review_rows" => {
            let ids: Vec<String> = arg(args, "ids")?;
            cv_ipc::library_ops::unkeep_review_rows(library, &commands.library_ops, ids)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "relocate_review_row" => {
            let id: String = arg(args, "id")?;
            let folder: String = arg(args, "folder")?;
            inside_roots(state, &folder)?;
            cv_ipc::library_ops::relocate_review_row(library, &commands.library_ops, id, folder)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }

        // ---- rename (organize) -------------------------------------------------------------
        "organize_profiles" => {
            let commands = Arc::clone(&state.commands);
            on_blocking(move || to_value(cv_ipc::organize::organize_profiles(&commands.settings)))
                .await?
        }
        "organize_select_profile" => {
            let id: String = arg(args, "id")?;
            let commands = Arc::clone(&state.commands);
            on_blocking(move || {
                cv_ipc::organize::organize_select_profile(&commands.settings, id)
                    .map_err(command_error)
            })
            .await?
            .and_then(to_value)
        }
        "organize_save_profile" => {
            let name: String = arg(args, "name")?;
            let from: String = arg(args, "from")?;
            let patterns: cv_ipc::organize::PatternsDto = arg(args, "patterns")?;
            let commands = Arc::clone(&state.commands);
            on_blocking(move || {
                cv_ipc::organize::organize_save_profile(&commands.settings, name, from, patterns)
                    .map_err(command_error)
            })
            .await?
            .and_then(to_value)
        }
        "organize_update_profile" => {
            let id: String = arg(args, "id")?;
            let patterns: cv_ipc::organize::PatternsDto = arg(args, "patterns")?;
            let commands = Arc::clone(&state.commands);
            on_blocking(move || {
                cv_ipc::organize::organize_update_profile(&commands.settings, id, patterns)
                    .map_err(command_error)
            })
            .await?
            .and_then(to_value)
        }
        "organize_delete_profile" => {
            let id: String = arg(args, "id")?;
            let commands = Arc::clone(&state.commands);
            on_blocking(move || {
                cv_ipc::organize::organize_delete_profile(&commands.settings, id)
                    .map_err(command_error)
            })
            .await?
            .and_then(to_value)
        }
        "organize_check_template" => {
            let field: cv_ipc::organize::PatternFieldDto = arg(args, "field")?;
            let template: String = arg(args, "template")?;
            to_value(cv_ipc::organize::organize_check_template(field, template))
        }
        "organize_example_for_file" => {
            let profile_id: String = arg(args, "profileId")?;
            let file_name: Option<String> = arg(args, "fileName")?;
            let commands = Arc::clone(&state.commands);
            on_blocking(move || {
                cv_ipc::organize::organize_example_for_file(
                    &commands.settings,
                    profile_id,
                    file_name,
                )
                .map_err(command_error)
            })
            .await?
            .and_then(to_value)
        }
        "organize_tokens" => {
            let tv: bool = arg(args, "tv")?;
            to_value(cv_ipc::organize::organize_tokens(tv))
        }
        "organize_preview" => {
            let request: cv_ipc::organize::OrganizeRequestDto = arg(args, "request")?;
            cv_ipc::organize::organize_preview(
                library,
                &commands.organize,
                &commands.settings,
                request,
            )
            .await
            .map_err(command_error)
            .and_then(to_value)
        }
        "organize_apply" => {
            let request: cv_ipc::organize::OrganizeRequestDto = arg(args, "request")?;
            let include: Vec<String> = arg(args, "include")?;
            cv_ipc::organize::organize_apply(
                library,
                &commands.organize,
                &commands.settings,
                request,
                include,
            )
            .await
            .map_err(command_error)
            .and_then(to_value)
        }
        "organize_journal" => cv_ipc::organize::organize_journal(library, &commands.organize)
            .await
            .map_err(command_error)
            .and_then(to_value),
        "organize_undo" => {
            let batch_id: String = arg(args, "batchId")?;
            cv_ipc::organize::organize_undo(library, &commands.organize, batch_id)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }

        // ---- episodes and their order ------------------------------------------------------
        "get_show_orderings" => {
            let show_id: String = arg(args, "showId")?;
            cv_ipc::episode_order::get_show_orderings(&commands.episode_order, show_id)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "set_show_ordering" => {
            let show_id: String = arg(args, "showId")?;
            let ordering: cv_ipc::episode_order::EpisodeOrderingDto = arg(args, "ordering")?;
            cv_ipc::episode_order::set_show_ordering(&commands.episode_order, show_id, ordering)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "search_show_episodes" => {
            let show_id: String = arg(args, "showId")?;
            let query: String = arg(args, "query")?;
            cv_ipc::episode_scrape::search_show_episodes(&commands.episode_scrape, show_id, query)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "assign_episode" => {
            let episode_id: String = arg(args, "episodeId")?;
            let pick: cv_ipc::episode_scrape::EpisodePickDto = arg(args, "pick")?;
            let fields: Option<Vec<String>> = arg(args, "fields")?;
            let do_not_overwrite: Option<bool> = arg(args, "doNotOverwrite")?;
            let episodes = &commands.episode_scrape;
            cv_ipc::episode_scrape::assign_episode(
                episodes,
                episode_id,
                pick,
                fields,
                do_not_overwrite,
            )
            .await
            .map_err(command_error)
            .and_then(to_value)
        }
        "scrape_show_episodes" => {
            let show_id: String = arg(args, "showId")?;
            let fields: Option<Vec<String>> = arg(args, "fields")?;
            let do_not_overwrite: Option<bool> = arg(args, "doNotOverwrite")?;
            let episodes = &commands.episode_scrape;
            cv_ipc::episode_scrape::scrape_show_episodes(
                episodes,
                show_id,
                fields,
                do_not_overwrite,
            )
            .await
            .map_err(command_error)
            .and_then(to_value)
        }
        "scrape_episode" => {
            let episode_id: String = arg(args, "episodeId")?;
            let fields: Option<Vec<String>> = arg(args, "fields")?;
            let do_not_overwrite: Option<bool> = arg(args, "doNotOverwrite")?;
            let episodes = &commands.episode_scrape;
            cv_ipc::episode_scrape::scrape_episode(episodes, episode_id, fields, do_not_overwrite)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }

        // ---- tools ------------------------------------------------------------------------
        "list_tools" => cv_ipc::tools::list_tools(&commands.tools)
            .await
            .map_err(command_error)
            .and_then(to_value),
        "set_tool_path" => {
            let tool: String = arg(args, "tool")?;
            let path: Option<String> = arg(args, "path")?;
            cv_ipc::tools::set_tool_path(&commands.tools, &commands.settings, tool, path)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "install_tool" => {
            let tool: String = arg(args, "tool")?;
            let stream = channel_arg(args, "onProgress")?;
            let sink = channel_sink(state, client, stream);
            cv_ipc::tools::install_tool(&commands.tools, &commands.scan, tool, sink)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "cancel_tool_install" => {
            let commands = Arc::clone(&state.commands);
            on_blocking(move || to_value(cv_ipc::tools::cancel_tool_install(&commands.tools)))
                .await?
        }
        "expect_tools" => {
            let tools: Vec<String> = arg(args, "tools")?;
            let commands = Arc::clone(&state.commands);
            on_blocking(move || {
                cv_ipc::tools::expect_tools(&commands.tools, tools);
                Ok::<Value, Value>(Value::Null)
            })
            .await?
        }

        // ---- updates ----------------------------------------------------------------------
        "get_update_status" => {
            let commands = Arc::clone(&state.commands);
            on_blocking(move || to_value(cv_ipc::updates::get_update_status(&commands.updates)))
                .await?
        }
        "check_for_update" => {
            let manual: bool = arg(args, "manual")?;
            cv_ipc::updates::check_for_update(&commands.updates, manual)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "update_blocking_jobs" => {
            let platform = ServerUpdatePlatform {
                jobs: state.services.jobs().clone(),
            };
            on_blocking(move || to_value(cv_ipc::updates::update_blocking_jobs(&platform))).await?
        }
        "cancel_update_download" => {
            let commands = Arc::clone(&state.commands);
            on_blocking(move || {
                to_value(cv_ipc::updates::cancel_update_download(&commands.updates))
            })
            .await?
        }
        "install_update" => Err(not_applicable(
            "The server is updated by pulling a new image or installing a new package; \
             the installer is desktop-only.",
        )),

        // ---- torrent search and sources ---------------------------------------------------
        "torrent_title_search" => {
            let request: cv_ipc::torrent_stream::TorrentTitleSearchRequestDto =
                arg(args, "request")?;
            let stream = channel_arg(args, "onEvent")?;
            let sink = channel_sink::<
                cv_ipc::provider_stream::ProviderEvent<cv_ipc::torrent_stream::TorrentTitleHitDto>,
            >(state, client, stream);
            cv_ipc::torrent_stream::torrent_title_search(
                &commands.torrent,
                &commands.search,
                request,
                sink,
            )
            .await
            .map_err(command_error)
            .and_then(to_value)
        }
        "torrent_title_details" => {
            let kind: String = arg(args, "kind")?;
            let tmdb_id: u32 = arg(args, "tmdbId")?;
            cv_ipc::torrent_stream::torrent_title_details(&commands.torrent, kind, tmdb_id)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "torrent_title_seasons" => {
            let tmdb_id: u32 = arg(args, "tmdbId")?;
            cv_ipc::torrent_stream::torrent_title_seasons(&commands.torrent, tmdb_id)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "torrent_title_episodes" => {
            let tmdb_id: u32 = arg(args, "tmdbId")?;
            let season: u16 = arg(args, "season")?;
            cv_ipc::torrent_stream::torrent_title_episodes(&commands.torrent, tmdb_id, season)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "torrent_landing" => {
            let cache = state
                .images
                .service()
                .map(|service| Arc::clone(service.cache()));
            cv_ipc::torrent_stream::torrent_landing(
                &commands.torrent,
                Arc::clone(state.images.sources()),
                cache,
            )
            .await
            .map_err(command_error)
            .and_then(to_value)
        }
        "torrent_release_search" => {
            let request: cv_ipc::torrent_stream::TorrentSearchRequestDto = arg(args, "request")?;
            let stream = channel_arg(args, "onEvent")?;
            let sink = channel_sink::<
                cv_ipc::provider_stream::ProviderEvent<cv_ipc::torrent_stream::TorrentReleaseDto>,
            >(state, client, stream);
            cv_ipc::torrent_stream::torrent_release_search(
                &commands.torrent,
                &commands.search,
                request,
                sink,
            )
            .await
            .map_err(command_error)
            .and_then(to_value)
        }
        "torrent_sources_status" => {
            let commands = Arc::clone(&state.commands);
            on_blocking(move || {
                cv_ipc::torrent::torrent_sources_status(&commands.torrent).map_err(command_error)
            })
            .await?
            .and_then(to_value)
        }
        "torrent_set_source" => {
            let id: String = arg(args, "id")?;
            let on: bool = arg(args, "on")?;
            let commands = Arc::clone(&state.commands);
            on_blocking(move || {
                cv_ipc::torrent::torrent_set_source(&commands.torrent, id, on)
                    .map_err(command_error)
            })
            .await?
            .and_then(to_value)
        }
        "torrent_test_torznab" => {
            let url: String = arg(args, "url")?;
            let api_key: Option<String> = arg(args, "apiKey")?;
            cv_ipc::torrent::torrent_test_torznab(&commands.torrent, url, api_key)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "torrent_set_torznab" => {
            let url: String = arg(args, "url")?;
            let api_key: Option<String> = arg(args, "apiKey")?;
            let commands = Arc::clone(&state.commands);
            on_blocking(move || {
                cv_ipc::torrent::torrent_set_torznab(&commands.torrent, url, api_key)
                    .map_err(command_error)
            })
            .await?
            .and_then(to_value)
        }
        "torrent_detect_jackett" => cv_ipc::torrent::torrent_detect_jackett(&commands.torrent)
            .await
            .map_err(command_error)
            .and_then(to_value),
        "torrent_indexer_service_status" => {
            let commands = Arc::clone(&state.commands);
            on_blocking(move || {
                cv_ipc::torrent_indexer::torrent_indexer_service_status(&commands.torrent)
                    .map_err(command_error)
            })
            .await?
            .and_then(to_value)
        }
        "torrent_install_indexer_service" => {
            let commands = Arc::clone(&state.commands);
            on_blocking(move || {
                cv_ipc::torrent_indexer::torrent_install_indexer_service(&commands.torrent)
                    .map_err(command_error)
            })
            .await?
            .and_then(to_value)
        }
        "torrent_update_indexer_service" => {
            cv_ipc::torrent_indexer::torrent_update_indexer_service(&commands.torrent)
                .await
                .map_err(command_error)
                .and_then(to_value)
        }
        "torrent_check_sources" => {
            let commands = Arc::clone(&state.commands);
            on_blocking(move || {
                cv_ipc::torrent_indexer::torrent_check_sources(&commands.torrent)
                    .map_err(command_error)
            })
            .await?
            .and_then(to_value)
        }
        "torrent_indexers" => {
            let commands = Arc::clone(&state.commands);
            on_blocking(move || {
                cv_ipc::torrent_indexer::torrent_indexers(&commands.torrent).map_err(command_error)
            })
            .await?
            .and_then(to_value)
        }
        "torrent_set_indexer" => {
            let id: String = arg(args, "id")?;
            let on: bool = arg(args, "on")?;
            let commands = Arc::clone(&state.commands);
            on_blocking(move || {
                cv_ipc::torrent_indexer::torrent_set_indexer(&commands.torrent, id, on)
                    .map_err(command_error)
            })
            .await?
            .and_then(to_value)
        }

        // ---- desktop-only: no equivalent in a browser -------------------------------------
        // The shim answers these itself; the arms keep an older client's calls harmless.
        "plugin:event|listen" | "plugin:event|unlisten" => Ok(Value::Null),
        "plugin:window|is_maximized" => Ok(Value::Bool(false)),
        "plugin:webview|set_webview_zoom" | "plugin:window|set_title" => Ok(Value::Null),
        "pick_folder" => Err(not_applicable(
            "A browser cannot open the server's folder picker; use the folder browser.",
        )),
        "pick_files" => Err(not_applicable(
            "A browser cannot open the server's file picker; choose the file in the page.",
        )),
        "pick_library_folders" => Err(not_applicable(
            "A browser cannot open the server's folder picker; use the folder browser.",
        )),
        "open_folder" => Err(not_applicable("The folder is on the server.")),
        "play_item" => Err(not_applicable(
            "Play in browser hands the file to this browser's own player.",
        )),
        "play_external" => Err(not_applicable(
            "No player starts on the server; use Play in browser.",
        )),
        "open_rating_page" => Err(not_applicable(
            "Your browser opens the rating page: open the link in the page.",
        )),
        "open_logs" => Err(not_applicable(
            "The server's log is on the machine that runs it.",
        )),
        "torrent_open_magnet" => {
            let magnet: String = arg(args, "magnet")?;
            // The shared checks run first: the notice, and a link that must be a magnet.
            cv_ipc::torrent::torrent_open_magnet(&commands.torrent, &BrowserHandoff, magnet)
                .await
                .map_err(command_error)?;
            Err(not_applicable(
                "Your browser opens magnet links: use Copy magnet link.",
            ))
        }
        "torrent_open_page" => {
            let address: String = arg(args, "address")?;
            cv_ipc::torrent::torrent_open_page(&commands.torrent, &BrowserHandoff, address)
                .await
                .map_err(command_error)?;
            Err(not_applicable(
                "Your browser opens the release page: open the link in the page.",
            ))
        }
        "torrent_open_torrent_file" => Err(not_applicable(
            "Download the torrent file from its page in your browser.",
        )),
        "torrent_open_indexer_manager" => Err(not_applicable(
            "The indexer manager runs on the server; open its address in your browser.",
        )),

        other => {
            // The shim answers the window and webview calls a browser has no use for; anything
            // else that arrives here is a command the server does not know.
            if other.starts_with("plugin:") {
                return Err(not_applicable(
                    "The desktop window and taskbar controls have no browser equivalent.",
                ));
            }
            eprintln!("cinevault-server: unhandled command {other}");
            Err(json!({
                "code": "unknown_command",
                "message": format!("{other} is not a command this server knows")
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// The dispatcher's source, read as the table it is.
    fn source() -> &'static str {
        include_str!("dispatch.rs")
    }

    /// One arm's body: from its `"name" =>` to the next arm.
    fn arm(name: &str) -> String {
        let needle = format!("\n        \"{name}\" =>");
        let Some(at) = source().find(&needle) else {
            return String::new();
        };
        let rest = &source()[at + needle.len()..];
        let end = rest.find("\n        \"").unwrap_or(rest.len());
        rest[..end].to_owned()
    }

    /// Every command the UI can invoke, by the name it sends.
    fn binding_commands() -> BTreeSet<String> {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../ui/src/bindings.ts");
        let text = std::fs::read_to_string(path).expect("ui/src/bindings.ts must exist");
        let mut names = BTreeSet::new();
        let mut rest = text.as_str();
        while let Some(at) = rest.find("__TAURI_INVOKE") {
            rest = &rest[at + "__TAURI_INVOKE".len()..];
            // The call is `__TAURI_INVOKE("name"` or `__TAURI_INVOKE<Type>("name"`.
            let Some(quote) = rest.find('"') else { break };
            let after = &rest[quote + 1..];
            let Some(end) = after.find('"') else { break };
            let name = &after[..end];
            // The import line names a package, not a command.
            if !name.is_empty() && !name.contains('/') {
                names.insert(name.to_owned());
            }
            rest = &after[end + 1..];
        }
        names
    }

    /// The commands the server shares with the desktop registry, with their argument names.
    fn registry() -> &'static [cv_ipc::commands::CommandInfo] {
        cv_ipc::commands::COMMANDS
    }

    /// Fails when the registry lists a command the dispatcher has no arm for, or when an arm
    /// reads an argument under another name than the interface sends.
    #[test]
    fn every_registry_command_has_an_arm_with_its_arguments() {
        for command in registry() {
            let body = arm(command.name);
            assert!(!body.is_empty(), "{} has no arm", command.name);
            for name in command.args {
                assert!(
                    body.contains(&format!("args, \"{name}\"")),
                    "{} does not read its argument {name}",
                    command.name
                );
            }
        }
    }

    /// Every command the interface can invoke is either in the shared registry or one of the
    /// commands a browser has no equivalent for. A command added to the interface alone fails
    /// here, so it cannot be forgotten.
    #[test]
    fn every_binding_command_is_dispatched() {
        let known: BTreeSet<&str> = registry().iter().map(|command| command.name).collect();
        for name in binding_commands() {
            assert!(
                known.contains(name.as_str()) || !arm(&name).is_empty(),
                "{name} is in neither the registry nor the dispatcher"
            );
        }
    }

    /// The commands a browser cannot perform answer `not_applicable` with a sentence; the
    /// interface hides their controls. `fetch_imdb_top250` is served but not yet in the shared
    /// table, so it is not desktop-only.
    #[test]
    fn the_desktop_only_commands_answer_not_applicable() {
        let registry: BTreeSet<&str> = registry().iter().map(|command| command.name).collect();
        let dev = ["ping", "register_image_source", "start_sample_job"];
        let served_elsewhere = ["fetch_imdb_top250"];
        for name in binding_commands() {
            if registry.contains(name.as_str())
                || dev.contains(&name.as_str())
                || served_elsewhere.contains(&name.as_str())
            {
                continue;
            }
            let body = arm(&name);
            assert!(!body.is_empty(), "{name} has no arm");
            assert!(
                body.contains("not_applicable(") || body.contains("BrowserHandoff"),
                "{name} must answer not_applicable, not {body}"
            );
        }
    }

    /// The development aids answer plainly instead of pretending to work.
    #[test]
    fn the_development_aids_answer_not_applicable() {
        for name in ["register_image_source", "start_sample_job"] {
            assert!(arm(name).contains("not_applicable("), "{name}");
        }
    }

    /// Only a web address or a file in the staging folder may become artwork. A quoted path, a
    /// `file:` address, a `..` walk out of the staging folder and another server file are refused.
    #[test]
    fn artwork_comes_only_from_the_web_or_the_staging_folder() {
        let data = tempfile::tempdir().unwrap();
        let uploads = data.path().join("tmp");
        std::fs::create_dir_all(&uploads).unwrap();
        let staged = uploads.join("artwork-abc.png");
        std::fs::write(&staged, b"x").unwrap();
        let secret = data.path().join("library.db");
        std::fs::write(&secret, b"x").unwrap();

        assert_eq!(
            artwork_source(&uploads, "https://image.tmdb.org/a.jpg"),
            Ok(None)
        );
        assert_eq!(
            artwork_source(&uploads, staged.to_str().unwrap()),
            Ok(Some(staged.clone()))
        );
        let walk = format!("{}/../library.db", uploads.display());
        let quoted = format!("\"{}\"", secret.display());
        let file_url = format!("FILE:///{}", secret.display());
        let walk_named = format!("{}/../artwork-abc.png", uploads.join("x").display());
        for source in [
            secret.to_str().unwrap(),
            walk.as_str(),
            quoted.as_str(),
            file_url.as_str(),
            walk_named.as_str(),
            "relative/artwork-a.png",
            "",
        ] {
            assert!(
                artwork_source(&uploads, source).is_err(),
                "{source} must be refused"
            );
        }
    }

    #[test]
    fn a_channel_argument_is_parsed() {
        assert_eq!(channel_id(&json!("__CHANNEL__:7")), Some(7));
        assert_eq!(channel_id(&json!("7")), None);
        assert_eq!(channel_id(&json!(null)), None);
    }

    #[test]
    fn the_browser_handoff_starts_nothing() {
        use cv_ipc::torrent::Handoff;
        assert!(BrowserHandoff.open_url("magnet:?xt=urn:btih:x").is_ok());
        assert!(
            BrowserHandoff
                .open_path(Path::new("/tmp/a.torrent"))
                .is_ok()
        );
    }

    #[test]
    fn the_server_platform_counts_busy_jobs() {
        use cv_ipc::updates::UpdatePlatform;
        let manager = cv_jobs::JobManager::new(
            cv_jobs::Lanes::new(cv_jobs::LaneConfig::default()),
            cv_jobs::ManagerConfig::default(),
        );
        let platform = ServerUpdatePlatform {
            jobs: manager.clone(),
        };
        assert_eq!(platform.busy_jobs(), 0);
        assert!(platform.open_url("https://example.com").is_err());
    }
}
