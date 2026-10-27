//! The background job that builds the thumbnail cache for newly scanned items.
//!
//! After a rescan finds new or changed items, [`build_after`] waits for that job to end, reads
//! the poster-like image files recorded for those items (and for their seasons and episodes) and
//! queues one persistent job, `artwork.thumbs`, that fills the [`ThumbCache`] with the grid and
//! list sizes. The grid then finds its thumbnails ready instead of generating them on first view.
//!
//! # Staying out of the way
//!
//! - Every image is one job item on the **CPU lane** at [`Priority::Low`], so scanning, media
//!   info and anything the user starts run first.
//! - The job runs [`default_parallelism`] images at a time: half of the cores, at most four, so
//!   it never uses every core. The CPU lane width bounds it further.
//! - Decoding, resizing and encoding run on blocking threads, never on the async runtime that
//!   serves the UI's commands and `cv://img` requests. The responsiveness test
//!   (`tests/thumb_job_5k.rs`) states the latency bound that is held while 5,000 posters build.
//! - The job is persistent, so a quit or crash resumes with only the unfinished images
//!   ([`resume_thumb_jobs`]); building an image already in the cache decodes nothing.
//!
//! # What is built
//!
//! Only [`THUMB_KINDS`] (posters, season posters, key art, square art) at [`BUILD_SIZES`] (grid
//! and list). Fan art, banners, thumbs and the hero size are generated on first request by the
//! `cv://img` handler, because they are rarely viewed and cost the most to decode.
//!
//! A missing or damaged image fails only its own item; the failures are grouped by reason in the
//! Activity drawer ("image file missing", "unsupported or damaged image").

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use cv_db::repo::graphics::graphic_files_of;
use cv_db::{Database, DbError};
use cv_domain::{ItemId, MediaFileType};
use cv_jobs::{
    ItemControl, ItemError, ItemWork, JobHandle, JobManager, JobSpec, LaneId, Priority, StageError,
    StoreError,
};
use serde::{Deserialize, Serialize};

use crate::thumb::{ThumbCache, ThumbError, ThumbSize};

/// The job kind a thumbnail build is stored and resumed under.
pub const THUMB_JOB_KIND: &str = "artwork.thumbs";

/// The sizes the job builds. Hero is generated on demand.
pub const BUILD_SIZES: [ThumbSize; 2] = [ThumbSize::Grid, ThumbSize::List];

/// The image types the job builds: the ones shown as posters in the grid and the lists.
pub const THUMB_KINDS: [MediaFileType; 4] = [
    MediaFileType::Poster,
    MediaFileType::SeasonPoster,
    MediaFileType::Keyart,
    MediaFileType::Squareart,
];

/// Images built at the same time when the caller does not choose: half of the cores, between one
/// and four.
pub fn default_parallelism() -> usize {
    let cores = std::thread::available_parallelism().map_or(2, usize::from);
    (cores / 2).clamp(1, 4)
}

/// One image to build, as saved in the job store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThumbTarget {
    /// The library item the image belongs to.
    pub item: i64,
    /// The image file.
    pub path: String,
}

impl std::fmt::Display for ThumbTarget {
    /// The file name, shown as the current item.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = Path::new(&self.path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&self.path);
        f.write_str(name)
    }
}

/// The thumbnail targets of `items` and their descendants: each poster-like file once, in item
/// order. Reads the database only; files are not opened.
pub fn thumb_targets(db: &Database, items: &[ItemId]) -> Result<Vec<ThumbTarget>, DbError> {
    let files = db.read(|conn| graphic_files_of(conn, items, &THUMB_KINDS))?;
    let mut seen = HashSet::new();
    let mut targets = Vec::with_capacity(files.len());
    for file in files {
        let path = PathBuf::from(&file.directory).join(&file.filename);
        let path = path.to_string_lossy().into_owned();
        if seen.insert(path.clone()) {
            targets.push(ThumbTarget {
                item: file.item_id.0,
                path,
            });
        }
    }
    Ok(targets)
}

/// What a thumbnail job did so far.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ThumbBuildStats {
    /// Images decoded and written to the cache.
    pub generated: u64,
    /// Images whose thumbnails were already cached (nothing decoded).
    pub reused: u64,
}

#[derive(Default)]
struct Counters {
    generated: AtomicU64,
    reused: AtomicU64,
}

/// The worker of a thumbnail job.
#[derive(Clone)]
pub struct ThumbWork {
    cache: Arc<ThumbCache>,
    counters: Arc<Counters>,
}

impl ThumbWork {
    pub fn new(cache: Arc<ThumbCache>) -> Self {
        Self {
            cache,
            counters: Arc::default(),
        }
    }

    /// Counts of the images that finished so far.
    pub fn stats(&self) -> ThumbBuildStats {
        ThumbBuildStats {
            generated: self.counters.generated.load(Ordering::Relaxed),
            reused: self.counters.reused.load(Ordering::Relaxed),
        }
    }
}

fn failure(error: &ThumbError) -> StageError {
    let reason = match error {
        ThumbError::Io { source, .. } if source.kind() == std::io::ErrorKind::NotFound => {
            "image file missing"
        }
        ThumbError::Io { .. } => "image file unreadable",
        ThumbError::Decode(_) => "unsupported or damaged image",
        ThumbError::Encode(_) => "thumbnail encoding failed",
        ThumbError::InvalidKey(_) => "invalid thumbnail key",
    };
    StageError::new(reason)
}

impl ItemWork for ThumbWork {
    type Item = ThumbTarget;

    async fn process(&self, item: &ThumbTarget, control: &ItemControl) -> Result<(), ItemError> {
        if control.is_cancelled() {
            return Err(ItemError::Cancelled);
        }
        let cache = Arc::clone(&self.cache);
        let path = PathBuf::from(&item.path);
        // The image work is CPU-bound: keep it off the async workers.
        let outcome = tokio::task::spawn_blocking(move || cache.ensure(&path, &BUILD_SIZES)).await;
        match outcome {
            Ok(Ok(ensured)) => {
                let counter = if ensured.reused() {
                    &self.counters.reused
                } else {
                    &self.counters.generated
                };
                counter.fetch_add(1, Ordering::Relaxed);
                Ok(())
            }
            Ok(Err(error)) => {
                tracing::debug!(%error, path = %item.path, "thumbnail not built");
                Err(failure(&error).into())
            }
            Err(error) => {
                tracing::error!(%error, "thumbnail worker stopped");
                Err(StageError::new("thumbnail worker stopped").into())
            }
        }
    }
}

/// A submitted thumbnail build: the job and what it has done.
#[derive(Clone)]
pub struct ThumbJob {
    pub job: JobHandle,
    work: ThumbWork,
}

impl ThumbJob {
    /// Counts of the images that finished so far.
    pub fn stats(&self) -> ThumbBuildStats {
        self.work.stats()
    }
}

/// Queues a persistent thumbnail build of `targets` on the CPU lane at low priority, at most
/// `parallelism` images at a time (see [`default_parallelism`]). Returns `None` when there is
/// nothing to build. The manager needs a job store.
pub fn submit_thumb_job(
    manager: &JobManager,
    cache: Arc<ThumbCache>,
    targets: Vec<ThumbTarget>,
    parallelism: usize,
) -> Result<Option<ThumbJob>, StoreError> {
    if targets.is_empty() {
        return Ok(None);
    }
    let work = ThumbWork::new(cache);
    let spec = JobSpec::new(job_name(targets.len()), LaneId::Cpu)
        .priority(Priority::Low)
        .concurrency(parallelism);
    let job = manager.submit_persistent(THUMB_JOB_KIND, spec, work.clone(), targets)?;
    Ok(Some(ThumbJob { job, work }))
}

/// The name Activity shows for building `images` thumbnails: `Preparing 40 thumbnails`.
#[must_use]
pub fn job_name(images: usize) -> String {
    format!(
        "Preparing {images} {}",
        if images == 1 {
            "thumbnail"
        } else {
            "thumbnails"
        }
    )
}

/// Starts again the thumbnail builds that a crash or quit cut short, with only their unfinished
/// images.
pub fn resume_thumb_jobs(
    manager: &JobManager,
    cache: &Arc<ThumbCache>,
) -> Result<Vec<ThumbJob>, StoreError> {
    let mut works: Vec<ThumbWork> = Vec::new();
    let jobs = manager.resume_persisted(THUMB_JOB_KIND, |_stored| {
        let work = ThumbWork::new(Arc::clone(cache));
        works.push(work.clone());
        work
    })?;
    Ok(jobs
        .into_iter()
        .zip(works)
        .map(|(job, work)| ThumbJob { job, work })
        .collect())
}

/// Hooks the thumbnail build after another job, normally a rescan.
///
/// When `after` ends (for any reason), `changed` is called to list the items it created or
/// changed, and a thumbnail job is queued for their images. A cancelled rescan still builds what
/// it had added. Must be called inside a tokio runtime. The task's value is the queued job, or
/// `None` when there was nothing to build or the job could not be queued (logged).
pub fn build_after<F>(
    manager: &JobManager,
    db: &Arc<Database>,
    cache: &Arc<ThumbCache>,
    after: JobHandle,
    changed: F,
    parallelism: usize,
) -> tokio::task::JoinHandle<Option<ThumbJob>>
where
    F: FnOnce() -> Vec<ItemId> + Send + 'static,
{
    let manager = manager.clone();
    let db = Arc::clone(db);
    let cache = Arc::clone(cache);
    cv_jobs::spawn(async move {
        after.wait().await;
        let items = changed();
        if items.is_empty() {
            return None;
        }
        let targets = tokio::task::spawn_blocking(move || thumb_targets(&db, &items))
            .await
            .ok()?
            .inspect_err(|error| tracing::warn!(%error, "could not list images to cache"))
            .ok()?;
        submit_thumb_job(&manager, cache, targets, parallelism)
            .inspect_err(|error| tracing::warn!(%error, "could not queue the image cache job"))
            .ok()
            .flatten()
    })
}
