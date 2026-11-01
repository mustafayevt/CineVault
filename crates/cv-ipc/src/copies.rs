//! Copies of a movie: the commands behind the Copies filter's Compare page, the Movie page line
//! and the "Keep both" decision.
//!
//! The groups, the best-quality mark and the decisions live in `cv_app::copies`; this module
//! converts to the DTOs the screens read. "Keep this one" first asks [`keep_copy`], which checks
//! the group as it is now, then sends only the copies it confirmed to the existing delete, which
//! follows Settings › Deleted files.

use cv_app::copies::{CopiesService, CopyMedia, CopyRow, KeepCopyError};
use cv_app::organize::OrganizeService;
use cv_db::repo::copies::CopyGroup;
use cv_domain::ItemId;
use serde::Serialize;
use specta::Type;

use crate::detail::parse_id;
use crate::library::{LibraryCommandError, blocking, storage};
use crate::sources::LibraryState;

/// The copies of one movie, kept or not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct CopiesOfDto {
    /// The ids of every copy of the film (the movie itself included), in id order.
    pub ids: Vec<String>,
    /// The user chose to keep these copies: they are not counted and show no badge.
    pub kept: bool,
}

impl From<&CopyGroup> for CopiesOfDto {
    fn from(group: &CopyGroup) -> Self {
        Self {
            ids: group.members.iter().map(|id| id.0.to_string()).collect(),
            kept: group.kept.is_some(),
        }
    }
}

/// What the media info of one copy says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct CopyMediaDto {
    /// `2160p`, `1080p`, ...; empty when the frame is below every bucket.
    pub resolution: String,
    /// `Dolby Vision`, `HDR10`, ...; empty for SDR.
    pub hdr_formats: Vec<String>,
    /// The lead audio track, such as `TrueHD Atmos 7.1`; empty without audio.
    pub audio: String,
    pub video_bitrate_kbps: u32,
    /// The running time of the video in minutes; `0` when the file does not say.
    pub duration_minutes: u32,
}

impl From<CopyMedia> for CopyMediaDto {
    fn from(media: CopyMedia) -> Self {
        Self {
            resolution: media.resolution,
            hdr_formats: media.hdr_formats,
            audio: media.audio,
            video_bitrate_kbps: media.video_bitrate_kbps,
            duration_minutes: media.duration_minutes,
        }
    }
}

/// One card of the Compare copies page.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct CopyRowDto {
    /// Item id as text.
    pub id: String,
    pub title: String,
    /// `0` when unknown.
    pub year: u32,
    /// `None` while the media info has not been read ("Not read yet").
    pub media: Option<CopyMediaDto>,
    /// `BluRay`, `WEB-DL`, ...; empty when unknown.
    pub source: String,
    /// Bytes by the library's size rule. A plain number: exact up to 2^53.
    pub size_bytes: f64,
    pub folder: String,
    /// The name of the main video file.
    pub file_name: String,
    /// The label of the library folder the copy lives on; empty when it was removed.
    pub source_label: String,
    /// The suggested copy ("Best quality").
    pub best: bool,
    /// The video may be the same file as another copy's.
    pub same_file: bool,
}

#[allow(clippy::cast_precision_loss)]
fn exact(bytes: u64) -> f64 {
    bytes as f64
}

impl From<CopyRow> for CopyRowDto {
    fn from(row: CopyRow) -> Self {
        Self {
            id: row.id.0.to_string(),
            title: row.title,
            year: u32::from(row.year),
            media: row.media.map(Into::into),
            source: row.source,
            size_bytes: exact(row.size_bytes),
            folder: row.folder,
            file_name: row.file_name,
            source_label: row.source_label,
            best: row.best,
            same_file: row.same_file,
        }
    }
}

/// The Compare copies page of one movie.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct ComparisonDto {
    /// The movie's own copy first, then the others in id order.
    pub copies: Vec<CopyRowDto>,
    /// The user chose to keep these copies ("You chose to keep these copies").
    pub kept: bool,
    /// Every copy shows the same Resolution, HDR, Audio and Source.
    pub look_same: bool,
    /// Two copies may be one file reached through two library folders: the page offers no
    /// "Keep this one".
    pub same_file: bool,
}

fn service(library: &LibraryState) -> Result<std::sync::Arc<CopiesService>, LibraryCommandError> {
    library.database().map(CopiesService::new).map_err(storage)
}

fn ids_of(texts: &[String]) -> Result<Vec<ItemId>, LibraryCommandError> {
    texts.iter().map(|text| parse_id(text)).collect()
}

/// The copies of a movie, kept or not; `None` when it has no copy. Feeds the Movie page's
/// "You have 2 copies" line.
pub async fn copies_of(
    library: &LibraryState,
    id: String,
) -> Result<Option<CopiesOfDto>, LibraryCommandError> {
    let id = parse_id(&id)?;
    let copies = service(library)?;
    blocking(move || Ok(copies.group_of(id)?.as_ref().map(CopiesOfDto::from))).await
}

/// The Compare copies page of a movie: one card per copy with the best quality marked; `None`
/// when the movie has no copy left.
pub async fn compare_copies(
    library: &LibraryState,
    id: String,
) -> Result<Option<ComparisonDto>, LibraryCommandError> {
    let id = parse_id(&id)?;
    let copies = service(library)?;
    blocking(move || {
        Ok(copies.compare(id)?.map(|page| ComparisonDto {
            copies: page.copies.into_iter().map(Into::into).collect(),
            kept: page.kept.is_some(),
            look_same: page.look_same,
            same_file: page.same_file,
        }))
    })
    .await
}

/// "Keep both" / "Keep all": records that the user keeps every one of `ids` as copies. No file
/// changes.
pub async fn keep_all_copies(
    library: &LibraryState,
    ids: Vec<String>,
) -> Result<(), LibraryCommandError> {
    let ids = ids_of(&ids)?;
    let copies = service(library)?;
    blocking(move || {
        copies.keep_all(&ids)?;
        Ok(())
    })
    .await
}

/// "Keep this one", checked before anything is deleted: the copies of `others` the delete may
/// take, or a refusal with a plain message when `keep` is gone or offline, one of `others` is not
/// a copy in its group, or two copies may be one file. Deletes nothing itself.
pub async fn keep_copy(
    library: &LibraryState,
    keep: String,
    others: Vec<String>,
) -> Result<Vec<String>, LibraryCommandError> {
    let keep = parse_id(&keep)?;
    let others = ids_of(&others)?;
    let copies = service(library)?;
    blocking(move || {
        copies
            .keep_copy(keep, &others)
            .map(|ids| ids.iter().map(|id| id.0.to_string()).collect())
            .map_err(|error| match error {
                KeepCopyError::Db(error) => error.into(),
                refused => LibraryCommandError {
                    code: refused.code().to_owned(),
                    message: refused.to_string(),
                },
            })
    })
    .await
}

/// "Ask again": removes the decision that keeps the movie's group, so it counts again. Also the
/// Undo of "Keep both".
pub async fn ask_again_copies(
    library: &LibraryState,
    id: String,
) -> Result<(), LibraryCommandError> {
    let id = parse_id(&id)?;
    let copies = service(library)?;
    blocking(move || {
        copies.ask_again(id)?;
        Ok(())
    })
    .await
}

/// The copy label (`2160p`, `copy 2`) a kept movie carries in its folder or video name that its
/// naming profile no longer needs, once the other copies are gone; `None` when its names are
/// plain.
pub fn label_to_drop(
    organize: &OrganizeService,
    profile: &cv_rename::RenameProfile,
    id: ItemId,
) -> Result<Option<String>, LibraryCommandError> {
    let plain = organize
        .plan_plain(profile, id)
        .map_err(|error| storage(error.to_string()))?;
    Ok(plain.and_then(|names| names.label_to_drop()))
}
