//! Typed commands for the external tools (MediaInfo and FFmpeg): list their state and
//! versions for Settings, set a path, and download one with progress over a channel.
//!
//! The rules live in `cv_app::tools` and `cv_media::tools`; this module converts to DTOs and runs
//! the slow work (starting programs to read versions, downloading) off the UI thread.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use cv_app::data_location::DataLocation;
use cv_app::tools::{ToolsError, ToolsService};
use cv_media::tools::{InstallPhase, InstallProgress, ToolId, ToolSource, ToolState, ToolStatus};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tauri::ipc::Channel;

use crate::boot::Boot;
use crate::scan_ipc::ScanState;
use crate::settings_ipc::SettingsState;

/// Held in Tauri state.
pub struct ToolsState {
    service: Arc<ToolsService>,
}

impl ToolsState {
    /// The tools service the app services built (the same one the scan pipeline reads media info
    /// with). When the services could not start, opens the tools folder under the data folder on
    /// its own and restores the paths saved in settings; without a data folder, downloads go to
    /// a folder beside the temp files for this session.
    pub fn from_boot(boot: &Boot, settings: &SettingsState) -> Self {
        if let Some(services) = boot.services() {
            return Self {
                service: Arc::clone(services.tools()),
            };
        }
        let dir = DataLocation::resolve_for_process().map_or_else(
            || std::env::temp_dir().join("cinevault-tools"),
            |location| location.tools_dir(),
        );
        Self {
            service: Arc::new(ToolsService::open(dir, settings.service())),
        }
    }
}

/// Why a tool is or is not usable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ToolStateDto {
    Ready,
    NotInstalled,
    OverrideMissing,
    Damaged,
    Unsupported,
}

impl From<ToolState> for ToolStateDto {
    fn from(state: ToolState) -> Self {
        match state {
            ToolState::Ready => Self::Ready,
            ToolState::NotInstalled => Self::NotInstalled,
            ToolState::OverrideMissing => Self::OverrideMissing,
            ToolState::Damaged => Self::Damaged,
            ToolState::Unsupported => Self::Unsupported,
        }
    }
}

/// Where the tool in use came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ToolSourceDto {
    /// A path the user set.
    Custom,
    /// A verified download in the app's data folder.
    Downloaded,
}

impl From<ToolSource> for ToolSourceDto {
    fn from(source: ToolSource) -> Self {
        match source {
            ToolSource::Override => Self::Custom,
            ToolSource::Managed => Self::Downloaded,
        }
    }
}

/// One tool as Settings shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct ToolStatusDto {
    /// `mediainfo` or `ffmpeg`.
    pub id: String,
    /// Name of the program that provides it.
    pub name: String,
    pub state: ToolStateDto,
    pub source: Option<ToolSourceDto>,
    /// The file in use.
    pub path: Option<String>,
    /// The version in use, when known.
    pub version: Option<String>,
    /// The path the user set, even when it is broken.
    pub custom_path: Option<String>,
    /// The version a download would install.
    pub download_version: Option<String>,
    /// Size of that download in bytes.
    pub download_bytes: Option<u32>,
    /// A downloaded copy is older than the version this release pins.
    pub update_available: bool,
    /// A verified downloaded copy is there to go back to when the path is cleared.
    pub downloaded: bool,
    pub license: String,
    pub homepage: String,
}

fn small(n: u64) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

impl From<ToolStatus> for ToolStatusDto {
    fn from(status: ToolStatus) -> Self {
        Self {
            id: status.id.as_str().to_owned(),
            name: status.name,
            state: status.state.into(),
            source: status.source.map(Into::into),
            path: status.path.as_deref().map(text),
            version: status.version,
            custom_path: status.override_path.as_deref().map(text),
            download_version: status.pinned_version,
            download_bytes: status.download_size.map(small),
            update_available: status.update_available,
            downloaded: status.downloaded,
            license: status.license,
            homepage: status.homepage,
        }
    }
}

/// Why a command did not do what was asked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct ToolCommandError {
    /// Stable code: `checksum`, `download`, `too_large`, `insecure`, `archive`, `cancelled`,
    /// `unsupported`, `override_invalid`, `unknown_tool`, `busy` or `storage`.
    pub code: String,
    /// Plain sentence for the user.
    pub message: String,
}

impl From<ToolsError> for ToolCommandError {
    fn from(error: ToolsError) -> Self {
        Self {
            code: error.code().to_owned(),
            message: error.to_string(),
        }
    }
}

/// The step an install is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ToolInstallPhaseDto {
    Downloading,
    Extracting,
    Finishing,
}

/// Progress of a download, sent over a channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ToolInstallEvent {
    pub phase: ToolInstallPhaseDto,
    pub downloaded: u32,
    pub total: u32,
}

impl From<&InstallProgress> for ToolInstallEvent {
    fn from(progress: &InstallProgress) -> Self {
        Self {
            phase: match progress.phase {
                InstallPhase::Downloading => ToolInstallPhaseDto::Downloading,
                InstallPhase::Extracting => ToolInstallPhaseDto::Extracting,
                InstallPhase::Finishing => ToolInstallPhaseDto::Finishing,
            },
            downloaded: small(progress.downloaded),
            total: small(progress.total),
        }
    }
}

/// At most this often during a download; phase changes always go out.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

async fn statuses(service: Arc<ToolsService>) -> Result<Vec<ToolStatusDto>, ToolCommandError> {
    // Reading a version starts the program, so keep it off the UI thread.
    tauri::async_runtime::spawn_blocking(move || {
        service.status().into_iter().map(Into::into).collect()
    })
    .await
    .map_err(|error| ToolCommandError {
        code: "storage".to_owned(),
        message: format!("CineVault could not read the tools: {error}"),
    })
}

/// Lists every tool with its state and version.
#[tauri::command]
#[specta::specta]
pub async fn list_tools(
    state: State<'_, ToolsState>,
) -> Result<Vec<ToolStatusDto>, ToolCommandError> {
    statuses(Arc::clone(&state.service)).await
}

/// Sets the path to use for a tool (a file, or the folder that holds it), or clears it with
/// `None` so the downloaded copy is used again. Returns the updated list.
#[tauri::command]
#[specta::specta]
pub async fn set_tool_path(
    state: State<'_, ToolsState>,
    settings: State<'_, SettingsState>,
    tool: String,
    path: Option<String>,
) -> Result<Vec<ToolStatusDto>, ToolCommandError> {
    let id = ToolsService::parse_tool(&tool)?;
    let trimmed = path.as_deref().map(str::trim).filter(|p| !p.is_empty());
    state
        .service
        .set_path(settings.service(), id, trimmed.map(Path::new))?;
    statuses(Arc::clone(&state.service)).await
}

/// Downloads the program that provides `tool`, checks its SHA-256 and installs it. Progress goes
/// to `on_progress`. Returns the updated list.
#[tauri::command]
#[specta::specta]
pub async fn install_tool(
    state: State<'_, ToolsState>,
    scan: State<'_, ScanState>,
    tool: String,
    on_progress: Channel<ToolInstallEvent>,
) -> Result<Vec<ToolStatusDto>, ToolCommandError> {
    let id = ToolsService::parse_tool(&tool)?;
    let on_progress = crate::ipc_sink::sink(on_progress);
    let service = Arc::clone(&state.service);
    let worker = Arc::clone(&service);
    let joined = tauri::async_runtime::spawn_blocking(move || {
        let mut last = Instant::now().checked_sub(PROGRESS_INTERVAL);
        let mut last_phase = None;
        worker.install(id, &mut |progress| {
            let changed = last_phase != Some(progress.phase);
            let due = last.is_none_or(|at| at.elapsed() >= PROGRESS_INTERVAL);
            if changed || due {
                last = Some(Instant::now());
                last_phase = Some(progress.phase);
                // A closed channel (the page was left) must not stop the download.
                on_progress(ToolInstallEvent::from(&progress));
            }
        })
    })
    .await;
    match joined {
        Ok(result) => result?,
        Err(error) => {
            return Err(ToolCommandError {
                code: "storage".to_owned(),
                message: format!("The download stopped unexpectedly: {error}"),
            });
        }
    }
    // Titles scanned before MediaInfo was there have been waiting for their details.
    if id == ToolId::MediaInfo
        && let Err(error) = scan.read_pending_media_info()
    {
        tracing::warn!(error = %error.message, "pending media info was not started");
    }
    statuses(service).await
}

/// Announces the tools the first-run setup is about to download, one after the other, so the
/// work that needs them (the media info of the first scan) waits for them instead of
/// reporting them missing. Unknown ids and ready tools are ignored.
#[tauri::command]
#[specta::specta]
pub fn expect_tools(state: State<'_, ToolsState>, tools: Vec<String>) {
    state.service.expect(&tools);
}

/// Stops the running download. Returns false when none is running.
#[tauri::command]
#[specta::specta]
pub fn cancel_tool_install(state: State<'_, ToolsState>) -> bool {
    state.service.cancel_install()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn status(state: ToolState, source: Option<ToolSource>) -> ToolStatus {
        ToolStatus {
            id: ToolId::Ffmpeg,
            name: "FFmpeg".to_owned(),
            state,
            source,
            path: Some(PathBuf::from("C:/tools/ffmpeg.exe")),
            version: Some("8.1.2".to_owned()),
            override_path: None,
            pinned_version: Some("8.1.2".to_owned()),
            download_size: Some(109_728_040),
            update_available: false,
            downloaded: false,
            license: "GPL".to_owned(),
            homepage: "https://ffmpeg.org".to_owned(),
        }
    }

    #[test]
    fn a_status_converts_with_its_version_and_source() {
        let dto = ToolStatusDto::from(status(ToolState::Ready, Some(ToolSource::Managed)));
        assert_eq!(dto.id, "ffmpeg");
        assert_eq!(dto.state, ToolStateDto::Ready);
        assert_eq!(dto.source, Some(ToolSourceDto::Downloaded));
        assert_eq!(dto.version.as_deref(), Some("8.1.2"));
        assert_eq!(dto.download_bytes, Some(109_728_040));
    }

    #[test]
    fn huge_sizes_saturate_instead_of_wrapping() {
        assert_eq!(small(u64::MAX), u32::MAX);
    }

    #[test]
    fn errors_carry_a_code_and_a_sentence() {
        let error = ToolCommandError::from(ToolsError::UnknownTool("zip".to_owned()));
        assert_eq!(error.code, "unknown_tool");
        assert!(error.message.contains("zip"));
    }

    #[test]
    fn progress_converts_phase_and_sizes() {
        let progress = InstallProgress {
            package: cv_media::tools::Package::Ffmpeg,
            phase: InstallPhase::Extracting,
            downloaded: 10,
            total: 20,
        };
        let event = ToolInstallEvent::from(&progress);
        assert_eq!(event.phase, ToolInstallPhaseDto::Extracting);
        assert_eq!((event.downloaded, event.total), (10, 20));
    }
}
