//! The scan pipeline's read commands: Rescan, Find missing, media info for pending titles and the
//! live health of each library folder.
//!
//! The pipeline itself lives in `cv_app::scanning`. This module holds its state, runs the slow disk
//! work off the async workers and converts to the DTOs the UI sees. A surface builds the state from
//! the app services and wraps each function in its own transport.

use std::sync::Arc;

use cv_app::scanning::{MediaInfoStatus, ScanError, ScanService};
use cv_app::services::AppServices;
use cv_scan::unmanaged::{UnmanagedReason, UnmanagedReport};
use serde::Serialize;
use specta::Type;

use crate::sources::{SourceHealthDto, SourceKindDto, source_health};

/// The scan pipeline over the app services, or nothing when they could not start.
pub struct ScanState {
    service: Option<Arc<ScanService>>,
}

impl ScanState {
    /// The scan service of the app services.
    pub fn from_services(services: Option<&Arc<AppServices>>) -> Self {
        Self {
            service: services.map(|services| Arc::clone(services.scan())),
        }
    }

    fn get(&self) -> Result<Arc<ScanService>, ScanCommandError> {
        self.service.clone().ok_or_else(|| ScanCommandError {
            code: "storage".to_owned(),
            message: "CineVault could not open the library, so it cannot be scanned.".to_owned(),
        })
    }

    /// Queues a rescan of every library folder. Must run inside the async runtime.
    pub fn rescan(&self) -> Result<RescanStartedDto, ScanCommandError> {
        let service = self.get()?;
        let run = service.rescan_all()?;
        Ok(RescanStartedDto {
            job_id: u32::try_from(run.handle.job.id().0).unwrap_or(u32::MAX),
            sources: u32::try_from(run.sources).unwrap_or(u32::MAX),
            media_info: service.media_info_status().into(),
        })
    }

    /// Reads the media info that is pending. Must run inside the async runtime.
    pub fn read_pending_media_info(&self) -> Result<MediaInfoStatusDto, ScanCommandError> {
        Ok(self.get()?.request_media_info().into())
    }
}

/// Whether the stream details of the scanned titles can be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum MediaInfoStatusDto {
    /// MediaInfo is installed; the details are read in the background.
    Ready,
    /// MediaInfo is not installed. The titles are in the library with their details pending;
    /// offer the download in Settings, Tools.
    Missing,
}

impl From<MediaInfoStatus> for MediaInfoStatusDto {
    fn from(status: MediaInfoStatus) -> Self {
        match status {
            MediaInfoStatus::Ready => Self::Ready,
            MediaInfoStatus::Missing => Self::Missing,
        }
    }
}

/// A rescan that was queued.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct RescanStartedDto {
    /// The job in the Activity drawer.
    pub job_id: u32,
    /// Library folders it covers.
    pub sources: u32,
    pub media_info: MediaInfoStatusDto,
}

/// Why a command did not do what was asked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct ScanCommandError {
    /// Stable code: `storage`.
    pub code: String,
    /// Plain sentence for the user.
    pub message: String,
}

impl From<ScanError> for ScanCommandError {
    fn from(error: ScanError) -> Self {
        Self {
            code: "storage".to_owned(),
            message: error.to_string(),
        }
    }
}

impl From<cv_db::DbError> for ScanCommandError {
    fn from(error: cv_db::DbError) -> Self {
        Self {
            code: "storage".to_owned(),
            message: format!("The library database could not be read: {error}"),
        }
    }
}

fn small(n: u64) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// Why a video belongs to no title.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum MissingReasonDto {
    /// The scan reads this folder but did not group the file into a title.
    NotGrouped,
    /// A skip rule or marker file hides the folder.
    SkippedLocation,
}

/// One video that belongs to no title.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct MissingVideoDto {
    /// The file, or for a disc the disc folder.
    pub path: String,
    /// The folder to scan again to add it.
    pub folder: String,
    /// `path` relative to its library folder, `/` separated.
    pub relative: String,
    /// Size in whole megabytes (0 for a disc folder).
    pub size_mb: u32,
    pub reason: MissingReasonDto,
    pub is_disc: bool,
}

/// The unmanaged videos of one library folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct MissingInSourceDto {
    pub source_id: u32,
    pub kind: SourceKindDto,
    pub path: String,
    /// The folder could not be read, so nothing was compared.
    pub unreadable: bool,
    /// Some sub-folders could not be read.
    pub partial: bool,
    pub videos: Vec<MissingVideoDto>,
}

impl From<UnmanagedReport> for MissingInSourceDto {
    fn from(report: UnmanagedReport) -> Self {
        Self {
            source_id: u32::try_from(report.source.id).unwrap_or(u32::MAX),
            kind: report.source.kind.into(),
            path: report.source.path,
            unreadable: report.unreadable,
            partial: report.partial,
            videos: report
                .videos
                .into_iter()
                .map(|video| MissingVideoDto {
                    path: video.path.to_string_lossy().into_owned(),
                    folder: video.folder.to_string_lossy().into_owned(),
                    relative: video.relative,
                    size_mb: small(video.size / (1024 * 1024)),
                    reason: match video.reason {
                        UnmanagedReason::NotGrouped => MissingReasonDto::NotGrouped,
                        UnmanagedReason::SkippedLocation => MissingReasonDto::SkippedLocation,
                    },
                    is_disc: video.is_disc,
                })
                .collect(),
        }
    }
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, ScanCommandError> + Send + 'static,
) -> Result<T, ScanCommandError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| ScanCommandError {
            code: "storage".to_owned(),
            message: error.to_string(),
        })?
}

/// Rescans every library folder: new titles appear, changed and removed files are noticed, and
/// the thumbnails and media info of what was found follow in the background. Titles whose files
/// are gone become Missing; nothing is deleted.
pub async fn rescan_library(state: &ScanState) -> Result<RescanStartedDto, ScanCommandError> {
    state.rescan()
}

/// Lists the video files inside the library folders that belong to no title.
pub async fn find_missing_files(
    state: &ScanState,
) -> Result<Vec<MissingInSourceDto>, ScanCommandError> {
    let service = state.get()?;
    blocking(move || {
        Ok(service
            .find_missing()?
            .into_iter()
            .map(Into::into)
            .collect())
    })
    .await
}

/// Every library folder with whether it is reachable and what it holds, for the sidebar.
pub async fn list_source_health(
    state: &ScanState,
) -> Result<Vec<SourceHealthDto>, ScanCommandError> {
    let service = state.get()?;
    source_health(service)
        .await
        .map_err(|message| ScanCommandError {
            code: "storage".to_owned(),
            message,
        })
}

/// Reads the media info of the titles that are still waiting for it, for example after MediaInfo
/// was installed. `missing` means it is still not installed.
pub async fn read_pending_media_info(
    state: &ScanState,
) -> Result<MediaInfoStatusDto, ScanCommandError> {
    state.read_pending_media_info()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_services_every_command_says_why() {
        let state = ScanState::from_services(None);
        let error = state.rescan().expect_err("no services");
        assert_eq!(error.code, "storage");
        assert!(error.message.contains("could not open the library"));
    }
}
