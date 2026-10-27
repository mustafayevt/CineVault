//! The managed indexer service on the Torrent sources settings page.
//!
//! * `torrent_indexer_service_status`: how the service stands (not installed, downloading,
//!   starting, running, ...), its version, a newer version if one was found, and how many indexers
//!   work.
//! * `torrent_install_indexer_service` switches the service on and makes sure it is downloaded and
//!   running (what the page's "Download" does after the person skipped it).
//! * `torrent_update_indexer_service` installs the newest release when it publishes a checksum.
//! * `torrent_check_sources` checks the indexers now as the Activity job "Checking torrent
//!   sources".
//! * `torrent_open_indexer_manager` opens the service's own page in the browser and returns the
//!   admin password so the page can offer a Copy button for it.
//! * `torrent_indexers` and `torrent_set_indexer` list the indexers with their health and switch
//!   one off or on for good.
//!
//! Every command refuses with `notice_not_accepted` until the Torrent search notice is accepted.

use cv_app::torrents::indexer::{IndexerError, IndexerView, ServiceState, ServiceStatus};
use cv_torrent::indexer_service::provision::IndexerState;
use serde::Serialize;
use specta::Type;
use tauri::State;

use crate::torrent_ipc::{Handoff, OpenerHandoff};
use crate::torrent_stream_ipc::{CommandResult, TorrentCommandError, TorrentState};

/// How the indexer service stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IndexerServiceStatusDto {
    /// `notInstalled`, `downloading`, `installed`, `starting`, `running`, `stopped` or `failed`.
    pub state: String,
    pub version: Option<String>,
    /// The newest version, when it is newer than the installed one.
    pub latest_version: Option<String>,
    pub port: Option<u32>,
    /// Indexers that passed the last check.
    pub working: u32,
    /// Indexers that failed the last check.
    pub failed: u32,
    /// A check of the indexers is running.
    pub checking: bool,
    /// Plain words for `failed`.
    pub reason: Option<String>,
    /// The tracker languages when the person has chosen none: English and their other languages.
    pub default_languages: Vec<String>,
    /// How many sites each language that is not a tracker language would add, most first.
    pub other_languages: Vec<LanguageCountDto>,
}

/// How many sites a language would add.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LanguageCountDto {
    /// `ru`
    pub code: String,
    pub count: u32,
}

impl From<ServiceStatus> for IndexerServiceStatusDto {
    fn from(status: ServiceStatus) -> Self {
        Self {
            state: match status.state {
                ServiceState::NotInstalled => "notInstalled",
                ServiceState::Downloading => "downloading",
                ServiceState::Installed => "installed",
                ServiceState::Starting => "starting",
                ServiceState::Running => "running",
                ServiceState::Stopped => "stopped",
                ServiceState::Failed => "failed",
            }
            .to_owned(),
            version: status.version,
            latest_version: status.latest_version,
            port: status.port.map(u32::from),
            working: u32::try_from(status.working).unwrap_or(u32::MAX),
            failed: u32::try_from(status.failed).unwrap_or(u32::MAX),
            checking: status.checking,
            reason: status.reason,
            default_languages: status.default_languages,
            other_languages: status
                .other_languages
                .into_iter()
                .map(|(code, count)| LanguageCountDto {
                    code,
                    count: u32::try_from(count).unwrap_or(u32::MAX),
                })
                .collect(),
        }
    }
}

/// One indexer of the service.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IndexerDto {
    pub id: String,
    pub name: String,
    /// `working`, `needsSolver`, `unreachable`, `error` or `off`.
    pub state: String,
    /// Plain words for a state that is not `working`.
    pub reason: Option<String>,
    /// When it was last checked, epoch milliseconds.
    pub checked_at: Option<f64>,
    /// Switched off by the person.
    pub user_off: bool,
}

impl From<IndexerView> for IndexerDto {
    fn from(view: IndexerView) -> Self {
        #[allow(clippy::cast_precision_loss)]
        let checked_at = (view.checked_at_ms > 0).then_some(view.checked_at_ms as f64);
        Self {
            id: view.id,
            name: view.name,
            state: match view.state {
                IndexerState::Working => "working",
                IndexerState::NeedsSolver => "needsSolver",
                IndexerState::Unreachable => "unreachable",
                IndexerState::Error => "error",
                IndexerState::Off => "off",
            }
            .to_owned(),
            reason: view.reason,
            checked_at,
            user_off: view.user_off,
        }
    }
}

/// The service's own page and the password to sign in to it with.
#[derive(Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IndexerManagerAccessDto {
    pub url: String,
    /// The admin password for the Copy button. Never logged.
    pub password: Option<String>,
}

impl std::fmt::Debug for IndexerManagerAccessDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IndexerManagerAccessDto")
            .field("url", &self.url)
            .field("password", &self.password.as_ref().map(|_| "<set>"))
            .finish()
    }
}

impl From<IndexerError> for TorrentCommandError {
    fn from(error: IndexerError) -> Self {
        let code = match &error {
            IndexerError::Torrent(cv_torrent::TorrentError::NoticeNotAccepted) => {
                "notice_not_accepted"
            }
            IndexerError::Torrent(_) => "invalid",
            IndexerError::NotRunning => "not_running",
            IndexerError::Refused(_) => "refused",
            IndexerError::UpToDate => "up_to_date",
            IndexerError::Failed(_) => "failed",
        };
        Self::new(code, error.to_string())
    }
}

/// How the service stands now.
#[tauri::command]
#[specta::specta]
pub fn torrent_indexer_service_status(
    state: State<'_, TorrentState>,
) -> CommandResult<IndexerServiceStatusDto> {
    state.get()?.ensure_accepted()?;
    Ok(state.indexer()?.status().into())
}

/// Switches the indexer service on and makes sure it is downloaded and running.
#[tauri::command]
#[specta::specta]
pub fn torrent_install_indexer_service(
    state: State<'_, TorrentState>,
) -> CommandResult<IndexerServiceStatusDto> {
    let service = state.get()?;
    service.set_source("indexer-service", true)?;
    let manager = state.indexer()?;
    // A change of the switch starts it; asking again also retries a service that failed.
    manager.request_apply();
    Ok(manager.status().into())
}

/// Installs the newest release of the indexer service when it publishes a checksum for it.
#[tauri::command]
#[specta::specta]
pub async fn torrent_update_indexer_service(
    state: State<'_, TorrentState>,
) -> CommandResult<IndexerServiceStatusDto> {
    state.get()?.ensure_accepted()?;
    let manager = state.indexer()?;
    manager.update().await?;
    Ok(manager.status().into())
}

/// Checks the indexers now.
#[tauri::command]
#[specta::specta]
pub fn torrent_check_sources(state: State<'_, TorrentState>) -> CommandResult<()> {
    state.get()?.ensure_accepted()?;
    state.indexer()?.check_sources()?;
    Ok(())
}

/// Opens the service's own page in the browser and returns the admin password for the Copy
/// button.
#[tauri::command]
#[specta::specta]
pub fn torrent_open_indexer_manager(
    app: tauri::AppHandle,
    state: State<'_, TorrentState>,
) -> CommandResult<IndexerManagerAccessDto> {
    state.get()?.ensure_accepted()?;
    let (url, password) = state.indexer()?.manager_access()?;
    OpenerHandoff(app).open_url(&url).map_err(|message| {
        tracing::warn!(%message, "the indexer service page could not be opened");
        TorrentCommandError::new("failed", "The browser could not be opened.")
    })?;
    Ok(IndexerManagerAccessDto { url, password })
}

/// The indexers of the service with their health, by name.
#[tauri::command]
#[specta::specta]
pub fn torrent_indexers(state: State<'_, TorrentState>) -> CommandResult<Vec<IndexerDto>> {
    state.get()?.ensure_accepted()?;
    Ok(state
        .indexer()?
        .indexers()
        .into_iter()
        .map(Into::into)
        .collect())
}

/// Switches one indexer on or off for good and returns the list as it is now.
#[tauri::command]
#[specta::specta]
pub fn torrent_set_indexer(
    state: State<'_, TorrentState>,
    id: String,
    on: bool,
) -> CommandResult<Vec<IndexerDto>> {
    state.get()?.ensure_accepted()?;
    let manager = state.indexer()?;
    manager.set_indexer_on(&id, on)?;
    Ok(manager.indexers().into_iter().map(Into::into).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(state: ServiceState) -> ServiceStatus {
        ServiceStatus {
            state,
            version: Some("0.24.2903".into()),
            latest_version: Some("0.24.3000".into()),
            port: Some(9117),
            working: 12,
            failed: 3,
            checking: true,
            reason: None,
            default_languages: vec!["en".into(), "de".into()],
            other_languages: vec![("ru".into(), 7), ("zh".into(), 4)],
        }
    }

    #[test]
    fn every_state_has_the_word_the_page_reads() {
        let words: Vec<String> = [
            ServiceState::NotInstalled,
            ServiceState::Downloading,
            ServiceState::Installed,
            ServiceState::Starting,
            ServiceState::Running,
            ServiceState::Stopped,
            ServiceState::Failed,
        ]
        .into_iter()
        .map(|state| IndexerServiceStatusDto::from(status(state)).state)
        .collect();
        assert_eq!(
            words,
            [
                "notInstalled",
                "downloading",
                "installed",
                "starting",
                "running",
                "stopped",
                "failed"
            ]
        );
    }

    #[test]
    fn a_status_carries_the_figures() {
        let dto = IndexerServiceStatusDto::from(status(ServiceState::Running));
        assert_eq!(dto.version.as_deref(), Some("0.24.2903"));
        assert_eq!(dto.latest_version.as_deref(), Some("0.24.3000"));
        assert_eq!((dto.port, dto.working, dto.failed), (Some(9117), 12, 3));
        assert!(dto.checking);
        assert_eq!(dto.default_languages, ["en", "de"]);
        assert_eq!(
            dto.other_languages,
            [
                LanguageCountDto {
                    code: "ru".into(),
                    count: 7
                },
                LanguageCountDto {
                    code: "zh".into(),
                    count: 4
                }
            ]
        );
    }

    #[test]
    fn an_indexer_row_converts_with_its_health_word_and_time() {
        let dto = IndexerDto::from(IndexerView {
            id: "yts".into(),
            name: "YTS".into(),
            state: IndexerState::NeedsSolver,
            reason: Some("A browser challenge blocks it.".into()),
            checked_at_ms: 1_700_000_000_000,
            user_off: false,
        });
        assert_eq!(dto.state, "needsSolver");
        assert_eq!(dto.checked_at, Some(1_700_000_000_000.0));
        let never = IndexerDto::from(IndexerView {
            id: "x".into(),
            name: "X".into(),
            state: IndexerState::Off,
            reason: None,
            checked_at_ms: 0,
            user_off: true,
        });
        assert_eq!(never.checked_at, None);
        assert!(never.user_off);
    }

    #[test]
    fn errors_carry_a_code_the_page_can_branch_on() {
        let code = |error: IndexerError| TorrentCommandError::from(error).code;
        assert_eq!(
            code(IndexerError::Torrent(
                cv_torrent::TorrentError::NoticeNotAccepted
            )),
            "notice_not_accepted"
        );
        assert_eq!(code(IndexerError::NotRunning), "not_running");
        assert_eq!(code(IndexerError::UpToDate), "up_to_date");
        assert_eq!(code(IndexerError::Failed("x".into())), "failed");
        assert_eq!(
            code(IndexerError::Refused(
                cv_app::torrents::indexer_update::UpdateRefused::NoDigest
            )),
            "refused"
        );
    }

    #[test]
    fn the_password_never_shows_in_a_debug_line() {
        let access = IndexerManagerAccessDto {
            url: "http://127.0.0.1:9117/UI/Dashboard".into(),
            password: Some("super-secret-password".into()),
        };
        assert!(!format!("{access:?}").contains("super-secret"));
    }
}
