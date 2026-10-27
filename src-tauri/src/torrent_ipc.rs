//! Handing a release to the torrent app, and the Torrent sources settings.
//!
//! * `torrent_open_magnet` validates a magnet link in Rust (`cv_torrent::magnet`) and gives it to
//!   the operating system's magnet handler through the Tauri opener. The web view never opens a
//!   magnet itself, so the capability file stays as it is.
//! * `torrent_open_torrent_file` downloads a `.torrent` file the person asked for (at most
//!   8 MiB) into `<data>/torrents/tmp/`, checks that it is one, and opens it. The folder is
//!   emptied when the app starts.
//! * `torrent_sources_status`, `torrent_set_source`, `torrent_test_torznab` and
//!   `torrent_set_torznab` back the Settings page: the source list with its health, the switches,
//!   and the user's own Torznab instance. The key goes to the credential store and is never
//!   returned, logged or written to the settings file.
//!
//! * `torrent_detect_jackett` looks for a Jackett the person runs themselves (their saved
//!   instance's address, then `127.0.0.1:9117`), never the managed service, and reads no file of
//!   it: the page then asks for its API key.
//!
//! Every command refuses with `notice_not_accepted` until the Torrent search notice is accepted.

use std::path::{Path, PathBuf};

use cv_app::torrents::{
    Health, KeyChange, OwnJackett, SourceStatus, TorrentDownload, TorrentService,
};
use cv_torrent::magnet::validate_magnet;
use cv_torrent::model::{SourceError, SourceKind, web_page};
use serde::Serialize;
use specta::Type;
use tauri::State;
use tauri_plugin_opener::OpenerExt;

use crate::torrent_stream_ipc::{CommandResult, TorrentCommandError, TorrentState};

/// The folder, under the data folder, downloaded `.torrent` files wait in.
pub const TMP_DIR: &[&str] = &["torrents", "tmp"];

/// The largest `.torrent` file taken.
pub const MAX_TORRENT_BYTES: usize = 8 * 1024 * 1024;

/// Hands a link or a file to the system.
pub trait Handoff: Send + Sync {
    /// Opens an address with the program registered for it (a magnet link, a web page).
    ///
    /// # Errors
    /// The operating system's message.
    fn open_url(&self, url: &str) -> Result<(), String>;

    /// Opens a file with the program registered for its type.
    ///
    /// # Errors
    /// The operating system's message.
    fn open_path(&self, path: &Path) -> Result<(), String>;
}

/// The Tauri opener as a [`Handoff`].
pub struct OpenerHandoff<R: tauri::Runtime>(pub tauri::AppHandle<R>);

impl<R: tauri::Runtime> Handoff for OpenerHandoff<R> {
    fn open_url(&self, url: &str) -> Result<(), String> {
        self.0
            .opener()
            .open_url(url, None::<&str>)
            .map_err(|e| e.to_string())
    }

    fn open_path(&self, path: &Path) -> Result<(), String> {
        self.0
            .opener()
            .open_path(path.to_string_lossy().into_owned(), None::<&str>)
            .map_err(|e| e.to_string())
    }
}

/// Whether an operating-system message says no program is registered for what was opened:
/// Windows `ERROR_NO_ASSOCIATION` (1155), the macOS Launch Services "application not found"
/// (-10814) and the wording around them.
#[must_use]
pub fn is_no_handler(message: &str) -> bool {
    let lower = message.to_lowercase();
    [
        "1155",
        "no application is associated",
        "no association",
        "-10814",
        "klsapplicationnotfounderr",
        "no application knows how to open",
        "no application set to open",
        "unable to find an application",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn handoff_error(message: &str, what: &str) -> TorrentCommandError {
    if is_no_handler(message) {
        TorrentCommandError::new(
            "no_handler",
            format!("No torrent app is set up for {what}."),
        )
    } else {
        tracing::warn!(%message, "the system could not open a torrent hand-over");
        TorrentCommandError::new("failed", "The torrent app could not be opened.")
    }
}

/// Gives a magnet link to the system's handler.
///
/// # Errors
/// `notice_not_accepted`; `invalid_magnet` for a link that fails the validation (nothing is opened);
/// `no_handler` when no program is registered for magnet links; `failed` for anything else.
pub fn open_magnet(
    service: &TorrentService,
    handoff: &dyn Handoff,
    magnet: &str,
) -> CommandResult<()> {
    service.ensure_accepted()?;
    validate_magnet(magnet).map_err(|error| {
        TorrentCommandError::new("invalid_magnet", format!("This link is unusable: {error}."))
    })?;
    handoff
        .open_url(magnet)
        .map_err(|message| handoff_error(&message, "magnet links"))
}

/// Opens a release's page on its source's site in the browser.
///
/// # Errors
/// `notice_not_accepted`; `invalid_address` for anything that is not an `http` or `https` address (nothing
/// is opened); `failed` when the system could not open it.
pub fn open_page(
    service: &TorrentService,
    handoff: &dyn Handoff,
    address: &str,
) -> CommandResult<()> {
    service.ensure_accepted()?;
    let Some(address) = web_page(address) else {
        return Err(TorrentCommandError::new(
            "invalid_address",
            "That is not a web page address.",
        ));
    };
    handoff.open_url(&address).map_err(|message| {
        tracing::warn!(%message, "the system could not open a release page");
        TorrentCommandError::new("failed", "The page could not be opened.")
    })
}

/// Whether bytes are a `.torrent` file: a bencoded dictionary that starts with one of the usual
/// top-level keys and holds an `info` dictionary.
#[must_use]
pub fn is_torrent_file(bytes: &[u8]) -> bool {
    const KEYS: [&str; 10] = [
        "announce",
        "announce-list",
        "info",
        "comment",
        "created by",
        "creation date",
        "encoding",
        "url-list",
        "nodes",
        "httpseeds",
    ];
    if bytes.first() != Some(&b'd') {
        return false;
    }
    let rest = &bytes[1..];
    let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 || digits > 3 || rest.get(digits) != Some(&b':') {
        return false;
    }
    let Some(length) = std::str::from_utf8(&rest[..digits])
        .ok()
        .and_then(|d| d.parse::<usize>().ok())
    else {
        return false;
    };
    let start = digits + 1;
    let Some(key) = rest.get(start..start + length) else {
        return false;
    };
    let first_key_ok = std::str::from_utf8(key).is_ok_and(|k| KEYS.contains(&k));
    first_key_ok && bytes.windows(7).any(|w| w == b"4:infod")
}

/// A file name from a release name: letters, digits, dots, dashes and spaces, at most 80
/// characters, with the `.torrent` extension.
#[must_use]
pub fn torrent_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '.' | '-' | ' ') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let cleaned = cleaned.trim_matches(|c: char| c == '.' || c == ' ' || c == '_');
    let cleaned: String = cleaned.chars().take(80).collect();
    let cleaned = cleaned.trim_end_matches(['.', ' ']);
    if cleaned.is_empty() {
        "release.torrent".to_owned()
    } else {
        format!("{cleaned}.torrent")
    }
}

/// Empties the folder downloaded `.torrent` files wait in. Called when the app starts.
pub fn clear_tmp(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_type().is_ok_and(|t| t.is_file()) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Downloads the `.torrent` file of a release and opens it.
///
/// # Errors
/// `notice_not_accepted`; `failed` with a plain message when the download fails, the answer is not a
/// torrent file, or the file cannot be written; `no_handler` when no program opens `.torrent`
/// files.
pub async fn open_torrent_file(
    service: &TorrentService,
    handoff: &dyn Handoff,
    tmp_dir: &Path,
    url: &str,
    name: &str,
) -> CommandResult<()> {
    service.ensure_accepted()?;
    let download = service
        .fetch_torrent_file(url)
        .await
        .map_err(|error| download_error(&error))?;
    let bytes = match download {
        TorrentDownload::File(bytes) => bytes,
        // A site with magnet links only: the link goes to the torrent app like any other.
        TorrentDownload::Magnet(magnet) => return open_magnet(service, handoff, &magnet),
    };
    if bytes.len() > MAX_TORRENT_BYTES || !is_torrent_file(&bytes) {
        return Err(TorrentCommandError::new(
            "failed",
            "That address did not give a torrent file.",
        ));
    }
    std::fs::create_dir_all(tmp_dir).map_err(|error| {
        TorrentCommandError::new("failed", format!("The file could not be saved: {error}"))
    })?;
    let path = tmp_dir.join(torrent_file_name(name));
    std::fs::write(&path, &bytes).map_err(|error| {
        TorrentCommandError::new("failed", format!("The file could not be saved: {error}"))
    })?;
    handoff
        .open_path(&path)
        .map_err(|message| handoff_error(&message, "torrent files"))
}

fn download_error(error: &SourceError) -> TorrentCommandError {
    let message = match error {
        SourceError::Unreachable(_) | SourceError::Timeout => {
            "The torrent file could not be downloaded: nothing answered."
        }
        SourceError::RateLimited => "The site is limiting requests. Try again in a moment.",
        SourceError::Blocked => "The site asked for a browser check instead of the file.",
        SourceError::BadAnswer(_) | SourceError::KeyRejected => {
            "That address did not give a torrent file."
        }
    };
    TorrentCommandError::new("failed", message)
}

// ------------------------------------------------------------------ settings

/// A source as the Torrent sources settings list it.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TorrentSourceStatusDto {
    pub id: String,
    pub name: String,
    /// `built-in`, `indexer` or `torznab`.
    pub kind: String,
    pub on: bool,
    /// `working`, `needsSolver`, `unreachable`, `error`, `untested`, `off` or `checking`.
    pub health: String,
    /// Plain words for a source that is not working.
    pub reason: Option<String>,
    /// When it was last asked, epoch milliseconds.
    pub checked_at: Option<f64>,
}

impl From<SourceStatus> for TorrentSourceStatusDto {
    fn from(status: SourceStatus) -> Self {
        #[allow(clippy::cast_precision_loss)]
        let checked_at = status.checked_at_ms.map(|ms| ms as f64);
        Self {
            id: status.id,
            name: status.name,
            kind: match status.kind {
                SourceKind::BuiltIn => "built-in",
                SourceKind::Indexer => "indexer",
                SourceKind::Torznab => "torznab",
            }
            .to_owned(),
            on: status.on,
            health: match status.health {
                Health::Working => "working",
                Health::NeedsSolver => "needsSolver",
                Health::Unreachable => "unreachable",
                Health::Error => "error",
                Health::Untested => "untested",
                Health::Off => "off",
                Health::Checking => "checking",
            }
            .to_owned(),
            reason: status.reason,
            checked_at,
        }
    }
}

/// What a Torznab instance can do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TorznabCapsDto {
    pub search: bool,
    pub movie_search: bool,
    pub tv_search: bool,
    /// Whether movies and shows can be searched by IMDb ID.
    pub imdb_ids: bool,
    /// How many indexers the instance has, when it says.
    pub indexers: Option<u32>,
}

/// The outcome of a connection test.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TorznabTestDto {
    pub ok: bool,
    /// What to tell the person: `Connected · 12 indexers`, `The key was rejected`, ...
    pub message: String,
    pub caps: Option<TorznabCapsDto>,
}

/// The sentence for a failed connection test.
#[must_use]
pub fn torznab_failure_message(error: &SourceError) -> String {
    match error {
        SourceError::KeyRejected => "The key was rejected".to_owned(),
        SourceError::Unreachable(_) | SourceError::Timeout => {
            "Nothing answers at this address".to_owned()
        }
        SourceError::RateLimited => {
            "The instance is limiting requests. Try again in a moment.".to_owned()
        }
        SourceError::Blocked => "This address shows a web page, not a Torznab feed".to_owned(),
        SourceError::BadAnswer(message) => {
            if message.starts_with("This is not a Torznab feed") || message.starts_with("Enter") {
                message.clone()
            } else {
                "This is not a Torznab feed".to_owned()
            }
        }
    }
}

fn statuses(service: &TorrentService) -> CommandResult<Vec<TorrentSourceStatusDto>> {
    Ok(service
        .source_statuses()?
        .into_iter()
        .map(Into::into)
        .collect())
}

fn tmp_dir(state: &TorrentState) -> PathBuf {
    let mut dir = state.data_dir().unwrap_or_default();
    for part in TMP_DIR {
        dir.push(part);
    }
    dir
}

/// Gives a magnet link to the system's magnet handler (the person's torrent app).
#[tauri::command]
#[specta::specta]
pub async fn torrent_open_magnet(
    app: tauri::AppHandle,
    state: State<'_, TorrentState>,
    magnet: String,
) -> CommandResult<()> {
    let service = state.get()?;
    open_magnet(&service, &OpenerHandoff(app), &magnet)
}

/// Opens a release's page on its source's site in the browser (`http` and `https` only).
#[tauri::command]
#[specta::specta]
pub async fn torrent_open_page(
    app: tauri::AppHandle,
    state: State<'_, TorrentState>,
    address: String,
) -> CommandResult<()> {
    let service = state.get()?;
    open_page(&service, &OpenerHandoff(app), &address)
}

/// Downloads a release's `.torrent` file and opens it with the registered program.
#[tauri::command]
#[specta::specta]
pub async fn torrent_open_torrent_file(
    app: tauri::AppHandle,
    state: State<'_, TorrentState>,
    url: String,
    name: String,
) -> CommandResult<()> {
    let service = state.get()?;
    let dir = tmp_dir(&state);
    open_torrent_file(&service, &OpenerHandoff(app), &dir, &url, &name).await
}

/// Every source with its switch, health and when it was last asked.
#[tauri::command]
#[specta::specta]
pub fn torrent_sources_status(
    state: State<'_, TorrentState>,
) -> CommandResult<Vec<TorrentSourceStatusDto>> {
    let service = state.get()?;
    statuses(&service)
}

/// Switches a source on or off and returns the list as it is now. `indexer-service` switches the
/// managed indexer service.
#[tauri::command]
#[specta::specta]
pub fn torrent_set_source(
    state: State<'_, TorrentState>,
    id: String,
    on: bool,
) -> CommandResult<Vec<TorrentSourceStatusDto>> {
    let service = state.get()?;
    service.set_source(&id, on)?;
    statuses(&service)
}

/// Tests the user's own Torznab instance. `api_key` `None` uses the saved key. A failed test is
/// an answer (`ok: false` with the message), not an error.
#[tauri::command]
#[specta::specta]
pub async fn torrent_test_torznab(
    state: State<'_, TorrentState>,
    url: String,
    api_key: Option<String>,
) -> CommandResult<TorznabTestDto> {
    let service = state.get()?;
    service.ensure_accepted()?;
    Ok(match service.test_torznab(&url, api_key.as_deref()).await {
        Ok(summary) => {
            let indexers = summary.indexers.and_then(|n| u32::try_from(n).ok());
            TorznabTestDto {
                ok: true,
                message: match indexers {
                    Some(1) => "Connected · 1 indexer".to_owned(),
                    Some(n) => format!("Connected · {n} indexers"),
                    None => "Connected".to_owned(),
                },
                caps: Some(TorznabCapsDto {
                    search: summary.search,
                    movie_search: summary.movie_search,
                    tv_search: summary.tv_search,
                    imdb_ids: summary.imdb_ids,
                    indexers,
                }),
            }
        }
        Err(error) => TorznabTestDto {
            ok: false,
            message: torznab_failure_message(&error),
            caps: None,
        },
    })
}

/// Saves the user's own Torznab instance. `api_key`: `None` keeps the saved key, an empty text
/// removes it, other text replaces it. An empty address removes the instance and its key.
#[tauri::command]
#[specta::specta]
pub fn torrent_set_torznab(
    state: State<'_, TorrentState>,
    url: String,
    api_key: Option<String>,
) -> CommandResult<Vec<TorrentSourceStatusDto>> {
    let service = state.get()?;
    let change = match api_key {
        None => KeyChange::Keep,
        Some(key) if key.trim().is_empty() => KeyChange::Clear,
        Some(key) => KeyChange::Set(key),
    };
    service.set_torznab(&url, change)?;
    statuses(&service)
}

/// A Jackett the person runs themselves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OwnJackettDto {
    /// `http://127.0.0.1:9117`, where its dashboard (and the API key) is.
    pub base_url: String,
    /// Its all-indexers Torznab feed.
    pub feed_url: String,
    /// The own-instance setting already uses it.
    pub in_use: bool,
}

impl From<OwnJackett> for OwnJackettDto {
    fn from(found: OwnJackett) -> Self {
        Self {
            base_url: found.base_url,
            feed_url: found.feed_url,
            in_use: found.in_use,
        }
    }
}

/// Looks for a Jackett the person runs themselves; `None` when none answers.
#[tauri::command]
#[specta::specta]
pub async fn torrent_detect_jackett(
    state: State<'_, TorrentState>,
) -> CommandResult<Option<OwnJackettDto>> {
    let service = state.get()?;
    let managed_port = state
        .indexer()
        .ok()
        .and_then(|manager| manager.status().port);
    Ok(service
        .detect_jackett(managed_port)
        .await?
        .map(OwnJackettDto::from))
}

#[cfg(test)]
mod tests;
