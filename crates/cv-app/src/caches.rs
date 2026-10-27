//! The two caches Settings, Advanced manages: the thumbnail cache and the cache of provider
//! responses.
//!
//! * The thumbnail cache is capped (`advanced.image-cache-size`, in megabytes). A new cap is
//!   applied at once, and a cache that is over it is trimmed, least recently used first. Clearing
//!   removes every thumbnail; they are made again when an image is shown.
//! * The response cache keeps provider answers for a short, fixed time. Clearing removes them all,
//!   so the next lookup asks the provider again.
//!
//! * The video frames the artwork picker grabs (`cache/frames/<item>/`) count as images: they are
//!   in the image cache's size and go when it is cleared. Frames older than [`FRAME_MAX_AGE`] are
//!   removed when the app starts; a frame placed as artwork has been copied next to the video.
//!
//! Neither cache holds anything that cannot be rebuilt: artwork files in the library are never
//! touched.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use cv_artwork::thumb::{DEFAULT_MAX_BYTES, ThumbCache, ThumbError};
use cv_providers::HttpClient;
use serde_json::Value;

use crate::app_settings::SettingsService;

/// The setting that caps the thumbnail cache, in megabytes.
pub const IMAGE_CACHE_SIZE_KEY: &str = "advanced.image-cache-size";
/// The cap when the setting is not set, in megabytes.
pub const DEFAULT_IMAGE_CACHE_MB: u64 = DEFAULT_MAX_BYTES / BYTES_PER_MB;
/// The smallest and largest cap accepted, in megabytes.
pub const MIN_IMAGE_CACHE_MB: u64 = 100;
pub const MAX_IMAGE_CACHE_MB: u64 = 51_200;

const BYTES_PER_MB: u64 = 1024 * 1024;

/// Grabbed video frames older than this are removed when the app starts.
pub const FRAME_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);

/// Which cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheKind {
    /// Thumbnails of posters and other artwork.
    Images,
    /// Answers of the metadata providers.
    Responses,
}

/// How much a cache holds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheSize {
    pub entries: u64,
    pub bytes: u64,
}

/// Why a cache could not be read or cleared.
#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error("The image cache could not be used: {0}")]
    Thumbs(#[from] ThumbError),
    #[error("The response cache could not be cleared: {0}")]
    Responses(#[from] std::io::Error),
    #[error("The cache task stopped: {0}")]
    Task(String),
}

/// The cap in bytes that the settings ask for: the stored megabytes, kept within the accepted
/// range, or the default.
#[must_use]
pub fn image_cache_cap(settings: &SettingsService) -> u64 {
    let megabytes = settings
        .get(IMAGE_CACHE_SIZE_KEY)
        .as_ref()
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        // The range check makes the cast exact.
        .map_or(DEFAULT_IMAGE_CACHE_MB, |value| {
            value.clamp(MIN_IMAGE_CACHE_MB as f64, MAX_IMAGE_CACHE_MB as f64) as u64
        });
    megabytes * BYTES_PER_MB
}

/// Usage and cleanup of the caches.
pub struct CacheService {
    thumbs: Arc<ThumbCache>,
    http: Option<HttpClient>,
    settings: Arc<SettingsService>,
    /// Where the artwork picker keeps grabbed video frames.
    frames: Option<PathBuf>,
}

impl CacheService {
    #[must_use]
    pub fn new(
        thumbs: Arc<ThumbCache>,
        http: Option<HttpClient>,
        settings: Arc<SettingsService>,
    ) -> Self {
        Self {
            thumbs,
            http,
            settings,
            frames: None,
        }
    }

    /// Also counts, clears and ages out the grabbed video frames in `dir`.
    #[must_use]
    pub fn with_frames_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.frames = Some(dir.into());
        self
    }

    /// Takes the cap from the settings and trims the thumbnail cache when it is over the cap, and
    /// removes grabbed frames older than [`FRAME_MAX_AGE`]. Blocking: it walks the cache folders.
    /// A failure is logged.
    pub fn apply_settings(&self) {
        self.thumbs
            .set_max_bytes(Some(image_cache_cap(&self.settings)));
        if let Err(error) = self.thumbs.enforce_cap() {
            tracing::warn!(%error, "the image cache could not be trimmed to its cap");
        }
        self.prune_frames(SystemTime::now());
    }

    /// Removes the frame folders of titles whose newest frame is older than [`FRAME_MAX_AGE`] at
    /// `now`; returns what was removed.
    pub fn prune_frames(&self, now: SystemTime) -> CacheSize {
        let Some(dir) = &self.frames else {
            return CacheSize::default();
        };
        let mut removed = CacheSize::default();
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            let (size, newest) = tree_usage(&path);
            let old = newest
                .and_then(|newest| now.duration_since(newest).ok())
                .is_some_and(|age| age > FRAME_MAX_AGE);
            if old && remove_entry(&path) {
                removed.entries += size.entries;
                removed.bytes += size.bytes;
            }
        }
        removed
    }

    /// How much `kind` holds.
    ///
    /// # Errors
    /// The image cache folder cannot be read.
    pub async fn usage(&self, kind: CacheKind) -> Result<CacheSize, CacheError> {
        match kind {
            CacheKind::Images => {
                let thumbs = Arc::clone(&self.thumbs);
                let frames = self.frames.clone();
                let (usage, frames) = tokio::task::spawn_blocking(move || {
                    (
                        thumbs.usage(),
                        frames.map(|dir| tree_usage(&dir).0).unwrap_or_default(),
                    )
                })
                .await
                .map_err(|e| CacheError::Task(e.to_string()))?;
                let usage = usage?;
                Ok(CacheSize {
                    entries: usage.entries + frames.entries,
                    bytes: usage.bytes + frames.bytes,
                })
            }
            CacheKind::Responses => Ok(match self.http.as_ref().and_then(HttpClient::cache) {
                Some(cache) => {
                    let usage = cache.usage().await;
                    CacheSize {
                        entries: usage.entries,
                        bytes: usage.bytes,
                    }
                }
                None => CacheSize::default(),
            }),
        }
    }

    /// Removes everything in `kind`; returns what was removed.
    ///
    /// # Errors
    /// The cache folder cannot be read or removed.
    pub async fn clear(&self, kind: CacheKind) -> Result<CacheSize, CacheError> {
        match kind {
            CacheKind::Images => {
                let thumbs = Arc::clone(&self.thumbs);
                let frames = self.frames.clone();
                let (report, frames) = tokio::task::spawn_blocking(move || {
                    (
                        thumbs.clear(),
                        frames.map(|dir| clear_dir(&dir)).unwrap_or_default(),
                    )
                })
                .await
                .map_err(|e| CacheError::Task(e.to_string()))?;
                let report = report?;
                Ok(CacheSize {
                    entries: report.removed_entries + frames.entries,
                    bytes: report.freed_bytes + frames.bytes,
                })
            }
            CacheKind::Responses => {
                let Some(cache) = self.http.as_ref().and_then(HttpClient::cache) else {
                    return Ok(CacheSize::default());
                };
                let before = cache.usage().await;
                cache.clear(None).await?;
                Ok(CacheSize {
                    entries: before.entries,
                    bytes: before.bytes,
                })
            }
        }
    }
}

/// The files below `path` (or `path` itself) and the newest modification time among them.
fn tree_usage(path: &Path) -> (CacheSize, Option<SystemTime>) {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return (CacheSize::default(), None);
    };
    if !meta.is_dir() {
        let size = CacheSize {
            entries: 1,
            bytes: meta.len(),
        };
        return (size, meta.modified().ok());
    }
    let mut total = CacheSize::default();
    let mut newest = meta.modified().ok();
    for entry in std::fs::read_dir(path).into_iter().flatten().flatten() {
        let (size, time) = tree_usage(&entry.path());
        total.entries += size.entries;
        total.bytes += size.bytes;
        newest = newest.max(time);
    }
    (total, newest)
}

/// Removes a file or a folder with everything below it; false when it could not be removed.
fn remove_entry(path: &Path) -> bool {
    let removed = if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    };
    match removed {
        Ok(()) => true,
        Err(error) => {
            tracing::warn!(%error, path = %path.display(), "a cached frame could not be removed");
            false
        }
    }
}

/// Removes everything inside `dir` and returns what was removed.
fn clear_dir(dir: &Path) -> CacheSize {
    let mut removed = CacheSize::default();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        let (size, _) = tree_usage(&path);
        if remove_entry(&path) {
            removed.entries += size.entries;
            removed.bytes += size.bytes;
        }
    }
    removed
}

#[cfg(test)]
mod tests;
