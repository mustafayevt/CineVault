//! External tools: MediaInfo, FFmpeg and the indexer service.
//!
//! CineVault never installs anything system-wide and never touches `PATH`. A tool comes from one
//! of two places:
//!
//! 1. a **path the user chose** (it always wins), or
//! 2. a **verified download** in the app's own `tools` folder, fetched on demand.
//!
//! Downloads are pinned in an embedded manifest ([`Manifest`]): each has an exact URL, size and
//! SHA-256. A download is hashed while it is written to a staging folder; if the hash differs the
//! file is deleted and nothing is installed ([`ToolError::ChecksumMismatch`]). Only a verified file
//! is unpacked, and the finished folder replaces the old install in one rename.
//!
//! [`ToolsManager`] is the entry point: [`ToolsManager::resolve`] finds a tool,
//! [`ToolsManager::install`] downloads one, [`ToolsManager::status`] feeds the Settings page.

mod archive;
mod download;
mod install;
mod manager;
mod manifest;
mod version;

use std::path::PathBuf;

pub use download::Cancel;
pub use install::{InstallPhase, InstallProgress};
pub use manager::{ResolvedTool, ToolSource, ToolState, ToolStatus, ToolsManager, dev_cache_dir};
pub use manifest::{
    ArchiveFormat, Artifact, ArtifactMode, FileSpec, Manifest, Package, PackageSpec, Platform,
    PlatformSpec, ProgramSpec, TREE_MAX_BYTES, ToolId, is_sha256_hex,
};
pub use version::parse_version;

/// Why a tool could not be found, downloaded or installed. Every message is ready to show.
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("There are no downloads of {0} for this computer.")]
    UnsupportedPlatform(Package),
    #[error("{0} is not installed.")]
    NotInstalled(ToolId),
    #[error("The path you set for {tool} no longer exists: {}", .path.display())]
    OverrideMissing { tool: ToolId, path: PathBuf },
    #[error("{} cannot be used as {tool}: {reason}", .path.display())]
    OverrideInvalid {
        tool: ToolId,
        path: PathBuf,
        reason: String,
    },
    #[error("The downloaded {url} does not match its expected checksum, so it was discarded.")]
    ChecksumMismatch {
        url: String,
        expected: String,
        actual: String,
    },
    #[error("The download {url} is larger than the {limit} bytes expected, so it was stopped.")]
    TooLarge { url: String, limit: u64 },
    #[error("The address {url} is not https, so nothing was downloaded from it.")]
    InsecureUrl { url: String },
    #[error("Could not download {url}: {message}")]
    Download { url: String, message: String },
    #[error("The download could not be unpacked: {0}")]
    Archive(String),
    #[error("The download was cancelled.")]
    Cancelled,
    #[error("The tool manifest is not valid: {0}")]
    Manifest(String),
    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: std::io::Error,
    },
}

impl ToolError {
    /// A short stable name for the kind of failure, for the UI to branch on.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::UnsupportedPlatform(_) => "unsupported",
            Self::NotInstalled(_) => "not_installed",
            Self::OverrideMissing { .. } => "override_missing",
            Self::OverrideInvalid { .. } => "override_invalid",
            Self::ChecksumMismatch { .. } => "checksum",
            Self::TooLarge { .. } => "too_large",
            Self::InsecureUrl { .. } => "insecure",
            Self::Download { .. } => "download",
            Self::Archive(_) => "archive",
            Self::Cancelled => "cancelled",
            Self::Manifest(_) => "manifest",
            Self::Io { .. } => "storage",
        }
    }

    pub(crate) fn io(context: impl Into<String>, source: std::io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }
}
