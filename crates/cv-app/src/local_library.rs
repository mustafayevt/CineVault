//! The instant library: titles whose folders already hold Kodi, Jellyfin or Emby NFO
//! files show their real titles, years, IDs and plots as soon as they are scanned, before any
//! online lookup.
//!
//! A rescan creates an item from what the file names say and records the artwork files next to
//! the video (the grid shows a local `poster.jpg` at once). [`submit_local_import`] then reads
//! the NFO of each new title into the library, as one job on the disk lane so the Activity
//! drawer shows it and the library views refresh when it ends:
//!
//! ```text
//! rescan job -> local import job ("Reading NFO files")
//!            -> media info -> automatic matching (ID-first: the IDs the NFO carries are looked up
//!               directly, so a title that has its TMDB or IMDb ID is not searched for by name)
//! ```
//!
//! # Rules
//!
//! * Only NFO files that parse with a title are read (`cv_nfo::ownership`): release notes and
//!   other foreign files are ignored, and a pointer file that holds only an ID gives the ID.
//! * The import reads and never writes: no NFO or image file is created, changed or moved, and
//!   nothing is renamed ([`NfoService::import_local`]).
//! * Field locks are honoured and no undo record is kept; the items are new, so there is
//!   nothing to restore.
//! * A title without an NFO keeps the title and year parsed from its names.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use cv_domain::ItemId;
use cv_jobs::{
    ItemControl, ItemError, ItemWork, JobHandle, JobManager, JobSpec, LaneId, Priority, StageError,
};

use crate::app_settings::SettingsService;
use crate::nfo::{NfoService, NfoSettings};

/// The name of the import job in the Activity drawer.
pub const LOCAL_IMPORT_JOB: &str = crate::wording::job::READING_NFO_FILES;

/// Titles read by one job item.
pub const CHUNK: usize = 25;

/// A run of new titles, one job item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportChunk {
    pub items: Vec<ItemId>,
    /// Shown as the current item, for example `Titles 26-50`.
    pub label: String,
}

impl fmt::Display for ImportChunk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

/// Splits `found` into the job's items.
pub fn chunks_of(found: &[ItemId]) -> Vec<ImportChunk> {
    found
        .chunks(CHUNK)
        .enumerate()
        .map(|(index, items)| {
            let first = index * CHUNK + 1;
            ImportChunk {
                items: items.to_vec(),
                label: format!("Titles {first}-{}", first + items.len() - 1),
            }
        })
        .collect()
}

/// What an import has done so far.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImportStats {
    /// Titles whose library values came from their NFO.
    pub imported: u64,
    /// Titles with no usable NFO, which keep what their names say.
    pub without_nfo: u64,
}

#[derive(Default)]
struct Counters {
    imported: AtomicU64,
    without_nfo: AtomicU64,
}

/// The worker of an import job.
#[derive(Clone)]
pub struct LocalImportWork {
    nfo: Arc<NfoService>,
    settings: Arc<SettingsService>,
    counters: Arc<Counters>,
}

impl LocalImportWork {
    pub fn new(nfo: Arc<NfoService>, settings: Arc<SettingsService>) -> Self {
        Self {
            nfo,
            settings,
            counters: Arc::default(),
        }
    }

    /// Counts of what finished so far.
    pub fn stats(&self) -> ImportStats {
        ImportStats {
            imported: self.counters.imported.load(Ordering::Relaxed),
            without_nfo: self.counters.without_nfo.load(Ordering::Relaxed),
        }
    }
}

impl ItemWork for LocalImportWork {
    type Item = ImportChunk;

    async fn process(&self, chunk: &ImportChunk, control: &ItemControl) -> Result<(), ItemError> {
        if control.is_cancelled() {
            return Err(ItemError::Cancelled);
        }
        let nfo = Arc::clone(&self.nfo);
        let settings = Arc::clone(&self.settings);
        let items = chunk.items.clone();
        let outcome = tokio::task::spawn_blocking(move || {
            let settings = NfoSettings::from_values(&settings.snapshot().values);
            nfo.import_local(&items, &settings)
        })
        .await;
        match outcome {
            Ok(Ok(report)) => {
                let imported = u64::from(report.totals.changed + report.totals.unchanged);
                let total = chunk.items.len() as u64;
                self.counters
                    .imported
                    .fetch_add(imported, Ordering::Relaxed);
                self.counters
                    .without_nfo
                    .fetch_add(total.saturating_sub(imported), Ordering::Relaxed);
                Ok(())
            }
            Ok(Err(error)) => {
                tracing::warn!(%error, "local NFO files could not be read");
                Err(StageError::new("NFO files could not be read").into())
            }
            Err(error) => {
                tracing::error!(%error, "local import worker stopped");
                Err(StageError::new("import worker stopped").into())
            }
        }
    }
}

/// A submitted import: the job and what it has done.
#[derive(Clone)]
pub struct LocalImport {
    pub job: JobHandle,
    work: LocalImportWork,
}

impl LocalImport {
    /// Counts of what finished so far.
    pub fn stats(&self) -> ImportStats {
        self.work.stats()
    }
}

/// Queues the import of the NFO files of `found` on the disk lane. Returns `None` when there is
/// nothing to read. Must be called inside a tokio runtime. The job lives in memory: a title
/// that was not read before a quit is read by Read NFO, and matching does not depend on it.
pub fn submit_local_import(
    manager: &JobManager,
    nfo: Arc<NfoService>,
    settings: Arc<SettingsService>,
    found: &[ItemId],
) -> Option<LocalImport> {
    if found.is_empty() {
        return None;
    }
    let work = LocalImportWork::new(nfo, settings);
    let spec = JobSpec::new(LOCAL_IMPORT_JOB, LaneId::Disk).priority(Priority::Normal);
    let job = manager.submit(spec, work.clone(), chunks_of(found));
    Some(LocalImport { job, work })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_are_split_into_labelled_chunks() {
        let found: Vec<ItemId> = (1..=60).map(ItemId).collect();
        let chunks = chunks_of(&found);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].label, "Titles 1-25");
        assert_eq!(chunks[1].label, "Titles 26-50");
        assert_eq!(chunks[2].label, "Titles 51-60");
        assert_eq!(chunks[2].items.len(), 10);
        assert!(chunks_of(&[]).is_empty());
    }
}
