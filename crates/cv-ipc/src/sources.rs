//! The library folder types the read pages share: which kind a folder is and the label it shows.
//!
//! The rules live in `cv_db::repo::data_sources`; this module keeps the DTO and the one lookup the
//! title pages use, so the movie and the show page read a folder's label the same way.

use std::sync::Arc;

use cv_app::scanning::{ScanService, SourceHealth, SourceState};
use cv_db::Database;
use cv_domain::SourceKind;
use serde::{Deserialize, Serialize};
use specta::Type;

/// Which library a folder feeds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SourceKindDto {
    Movie,
    Tv,
}

impl From<SourceKindDto> for SourceKind {
    fn from(kind: SourceKindDto) -> Self {
        match kind {
            SourceKindDto::Movie => Self::Movie,
            SourceKindDto::Tv => Self::Tv,
        }
    }
}

impl From<SourceKind> for SourceKindDto {
    fn from(kind: SourceKind) -> Self {
        match kind {
            SourceKind::Movie => Self::Movie,
            SourceKind::Tv => Self::Tv,
        }
    }
}

/// The label of the library folder at `root` ("NAS 4K"), or empty when the folder is gone.
pub fn source_label_of(db: &Database, kind: SourceKind, root: &str) -> String {
    if root.is_empty() {
        return String::new();
    }
    db.read(|conn| cv_db::repo::data_sources::label_of_root(conn, kind, root))
        .ok()
        .flatten()
        .unwrap_or_default()
}

/// One library folder with its live state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct SourceHealthDto {
    pub id: u32,
    pub kind: SourceKindDto,
    pub path: String,
    /// False while the drive is disconnected or the share is down; its titles stay in the library.
    pub online: bool,
    /// Movies, or shows for a TV folder.
    pub titles: u32,
    /// Titles whose folder or video is gone while the library folder itself is reachable.
    pub missing: u32,
    /// Titles still waiting for their media info.
    pub pending_media_info: u32,
}

fn small(n: u64) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

impl From<SourceHealth> for SourceHealthDto {
    fn from(health: SourceHealth) -> Self {
        Self {
            id: u32::try_from(health.source.id).unwrap_or(u32::MAX),
            kind: health.source.kind.into(),
            path: health.source.path,
            online: health.state == SourceState::Online,
            titles: small(health.counts.titles),
            missing: small(health.counts.missing),
            pending_media_info: small(health.counts.pending_media_info),
        }
    }
}

/// Every library folder with whether it is reachable and what it holds, for the sidebar.
pub async fn source_health(service: Arc<ScanService>) -> Result<Vec<SourceHealthDto>, String> {
    tokio::task::spawn_blocking(move || service.source_health())
        .await
        .map_err(|error| error.to_string())?
        .map(|rows| rows.into_iter().map(Into::into).collect())
        .map_err(|error| error.to_string())
}
