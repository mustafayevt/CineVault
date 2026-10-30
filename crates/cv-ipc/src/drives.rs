//! The drives of the library: the overview the Drives page and the sidebar read, and what an
//! Empty of one drive would erase.
//!
//! The rules live in `cv_app::drives`; this module converts to the DTOs the UI sees. The service
//! itself is built and kept alive by the surface that holds the app state, because it also
//! announces changes and empties trash folders on its own timetable.

use std::sync::Arc;

use cv_app::drives::{Drive, DrivesOverview, DrivesService};
use serde::Serialize;
use specta::Type;

use crate::sources::SourceKindDto;

/// Why the overview could not be read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct DrivesCommandError {
    pub code: String,
    pub message: String,
}

/// The kind of device a drive is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DriveKindDto {
    Hdd,
    Ssd,
    Network,
    Removable,
}

impl From<cv_app::drives::DriveKind> for DriveKindDto {
    fn from(kind: cv_app::drives::DriveKind) -> Self {
        match kind {
            cv_app::drives::DriveKind::Hdd => Self::Hdd,
            cv_app::drives::DriveKind::Ssd => Self::Ssd,
            cv_app::drives::DriveKind::Network => Self::Network,
            cv_app::drives::DriveKind::Removable => Self::Removable,
        }
    }
}

/// One library folder on a drive.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DriveFolderDto {
    /// The data source id; `plan_change_source` takes it as the move target.
    pub source_id: u32,
    pub path: String,
    pub label: String,
    #[serde(rename = "type")]
    pub kind: SourceKindDto,
    /// Movies, or shows for a TV folder.
    pub titles: u32,
    pub bytes: f64,
}

/// How many titles of a drive fall in each resolution class.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct QualityCountsDto {
    pub p2160: u32,
    pub p1080: u32,
    pub p720: u32,
    pub sd: u32,
}

/// One drive that holds a library folder.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DriveDto {
    pub id: String,
    /// `D: Media`.
    pub name: String,
    pub kind: DriveKindDto,
    pub total_bytes: f64,
    pub free_bytes: f64,
    pub folders: Vec<DriveFolderDto>,
    /// Used space that is not library titles; `None` under 1 GB, offline or unknown.
    pub not_in_library_bytes: Option<f64>,
    pub quality: QualityCountsDto,
    pub online: bool,
    /// When an offline drive was last reachable, epoch milliseconds.
    pub last_seen_ms: Option<f64>,
    /// Under 5 percent or under 50 GB free.
    pub low: bool,
    /// Bytes in the trash folders (`.deletedByCineVault`) of its library folders.
    pub trash_bytes: f64,
    /// Bytes CineVault itself sent to the Recycle Bin / Trash from this drive, still there.
    pub bin_bytes: f64,
    /// The custom trash folder of Settings › Deleted files when it is on this drive.
    pub custom_trash: Option<CustomTrashDto>,
}

/// The custom trash folder on a drive.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CustomTrashDto {
    pub path: String,
    pub bytes: f64,
}

/// One entry of what an Empty would erase.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TrashEntryDto {
    /// `folder` (a batch of a trash folder) or `bin` (a path CineVault sent to the bin).
    pub kind: String,
    /// The batch folder, or the path the file had before it went to the bin.
    pub path: String,
    pub bytes: f64,
}

/// What an Empty of one drive (or of all of them) would erase.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TrashContentsDto {
    pub entries: Vec<TrashEntryDto>,
    pub bytes: f64,
}

/// The drives and when the numbers were read.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DrivesOverviewDto {
    pub drives: Vec<DriveDto>,
    pub updated_ms: f64,
}

/// A byte count as the number the UI reads.
#[allow(clippy::cast_precision_loss)]
pub fn exact(n: u64) -> f64 {
    n as f64
}

fn small(n: u64) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

impl From<Drive> for DriveDto {
    fn from(drive: Drive) -> Self {
        Self {
            id: drive.id,
            name: drive.name,
            kind: drive.kind.into(),
            total_bytes: exact(drive.total_bytes),
            free_bytes: exact(drive.free_bytes),
            folders: drive
                .folders
                .into_iter()
                .map(|folder| DriveFolderDto {
                    source_id: u32::try_from(folder.source_id).unwrap_or(u32::MAX),
                    path: folder.path,
                    label: folder.label,
                    kind: match folder.kind {
                        cv_domain::SourceKind::Movie => SourceKindDto::Movie,
                        cv_domain::SourceKind::Tv => SourceKindDto::Tv,
                    },
                    titles: small(folder.titles),
                    bytes: exact(folder.bytes),
                })
                .collect(),
            not_in_library_bytes: drive.not_in_library_bytes.map(exact),
            quality: QualityCountsDto {
                p2160: small(drive.quality.p2160),
                p1080: small(drive.quality.p1080),
                p720: small(drive.quality.p720),
                sd: small(drive.quality.sd),
            },
            online: drive.online,
            #[allow(clippy::cast_precision_loss)]
            last_seen_ms: drive.last_seen_ms.map(|ms| ms as f64),
            low: drive.low,
            trash_bytes: exact(drive.trash_bytes),
            bin_bytes: exact(drive.bin_bytes),
            custom_trash: drive.custom_trash.map(|(path, bytes)| CustomTrashDto {
                path,
                bytes: exact(bytes),
            }),
        }
    }
}

impl From<DrivesOverview> for DrivesOverviewDto {
    fn from(overview: DrivesOverview) -> Self {
        Self {
            drives: overview.drives.into_iter().map(Into::into).collect(),
            #[allow(clippy::cast_precision_loss)]
            updated_ms: overview.updated_ms as f64,
        }
    }
}

/// The drives that hold a library folder: name, kind, size and free space, the folders on each
/// with their titles and bytes, the space the library does not use, the resolution mix, whether
/// the drive answers and when it last did. Reads the disks.
pub async fn drives_overview(
    service: Option<Arc<DrivesService>>,
) -> Result<DrivesOverviewDto, DrivesCommandError> {
    let Some(service) = service else {
        return Ok(DrivesOverview::default().into());
    };
    tokio::task::spawn_blocking(move || service.overview())
        .await
        .map_err(|error| DrivesCommandError {
            code: "storage".to_owned(),
            message: error.to_string(),
        })?
        .map(Into::into)
        .map_err(|error| DrivesCommandError {
            code: "storage".to_owned(),
            message: format!("The library could not be read: {error}"),
        })
}

/// A drives failure in the command shape.
pub fn storage(message: String) -> DrivesCommandError {
    DrivesCommandError {
        code: "storage".to_owned(),
        message,
    }
}

/// What Empty… would erase on the drive `drive_id` (every drive when `None`): the batches of its
/// trash folders and what CineVault sent to the Recycle Bin from it, biggest first.
pub async fn drives_trash_contents(
    service: Option<Arc<DrivesService>>,
    drive_id: Option<String>,
) -> Result<TrashContentsDto, DrivesCommandError> {
    let Some(service) = service else {
        return Ok(TrashContentsDto {
            entries: Vec::new(),
            bytes: 0.0,
        });
    };
    let contents = tokio::task::spawn_blocking(move || service.trash_contents(drive_id.as_deref()))
        .await
        .map_err(|error| storage(error.to_string()))?
        .map_err(|error| storage(format!("The library could not be read: {error}")))?;
    let bytes = exact(contents.bytes());
    let mut entries: Vec<TrashEntryDto> = contents
        .batches
        .iter()
        .map(|batch| TrashEntryDto {
            kind: "folder".to_owned(),
            path: batch.path().to_string_lossy().into_owned(),
            bytes: exact(batch.bytes),
        })
        .chain(contents.bin.iter().map(|entry| TrashEntryDto {
            kind: "bin".to_owned(),
            path: entry.item.original.clone(),
            bytes: exact(entry.item.size),
        }))
        .collect();
    entries.sort_by(|a, b| b.bytes.total_cmp(&a.bytes));
    Ok(TrashContentsDto { entries, bytes })
}

/// What an Empty did.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EmptyReportDto {
    pub bytes: f64,
    pub batches: u32,
    pub bin_items: u32,
    /// Paths that could not be erased, each with the reason.
    pub problems: Vec<String>,
    /// History entries whose Undo was switched off.
    pub entries: u32,
}

/// Empties the trash of the drive `drive_id` (every drive when `None`): erases its trash folders'
/// batches and what CineVault sent to the Recycle Bin from it. `confirm` must be the literal
/// `DELETE` the person typed. History entries whose files were erased lose their Undo.
pub async fn empty_drives_trash(
    service: Option<Arc<DrivesService>>,
    drive_id: Option<String>,
    confirm: String,
) -> Result<EmptyReportDto, DrivesCommandError> {
    let Some(service) = service else {
        return Err(storage("The library is not open.".to_owned()));
    };
    let report =
        tokio::task::spawn_blocking(move || service.empty_trash(drive_id.as_deref(), &confirm))
            .await
            .map_err(|error| storage(error.to_string()))?
            .map_err(|error| DrivesCommandError {
                code: match error {
                    cv_app::drives::EmptyError::Confirmation => "confirmation",
                    cv_app::drives::EmptyError::Db(_) => "storage",
                }
                .to_owned(),
                message: error.to_string(),
            })?;
    Ok(EmptyReportDto {
        bytes: exact(report.bytes),
        batches: u32::try_from(report.batches).unwrap_or(u32::MAX),
        bin_items: u32::try_from(report.bin_items).unwrap_or(u32::MAX),
        problems: report
            .problems
            .into_iter()
            .map(|(path, why)| {
                if path.is_empty() {
                    why
                } else {
                    format!("{path}: {why}")
                }
            })
            .collect(),
        entries: u32::try_from(report.entries.len()).unwrap_or(u32::MAX),
    })
}

#[cfg(test)]
mod tests {
    use cv_app::drives::{DriveFolder, QualityCounts};
    use cv_domain::SourceKind;

    use super::*;

    #[test]
    fn the_wire_shape_is_the_one_the_drives_page_reads() {
        let overview = DrivesOverview {
            drives: vec![Drive {
                id: "D:".to_owned(),
                name: "D: Media".to_owned(),
                kind: cv_app::drives::DriveKind::Ssd,
                total_bytes: 1_000,
                free_bytes: 400,
                folders: vec![DriveFolder {
                    source_id: 3,
                    path: "D:/Movies".to_owned(),
                    label: "Movies".to_owned(),
                    kind: SourceKind::Movie,
                    titles: 39,
                    bytes: 600,
                }],
                not_in_library_bytes: None,
                quality: QualityCounts {
                    p2160: 1,
                    p1080: 2,
                    p720: 3,
                    sd: 4,
                },
                online: true,
                last_seen_ms: None,
                low: true,
                trash_bytes: 2_400,
                bin_bytes: 0,
                custom_trash: None,
            }],
            updated_ms: 5,
        };
        let json = serde_json::to_value(DrivesOverviewDto::from(overview)).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "drives": [{
                    "id": "D:", "name": "D: Media", "kind": "ssd",
                    "totalBytes": 1000.0, "freeBytes": 400.0,
                    "folders": [{
                        "sourceId": 3, "path": "D:/Movies", "label": "Movies",
                        "type": "movie", "titles": 39, "bytes": 600.0
                    }],
                    "notInLibraryBytes": null,
                    "quality": {"p2160": 1, "p1080": 2, "p720": 3, "sd": 4},
                    "online": true, "lastSeenMs": null, "low": true,
                    "trashBytes": 2400.0, "binBytes": 0.0, "customTrash": null
                }],
                "updatedMs": 5.0
            })
        );
    }
}
