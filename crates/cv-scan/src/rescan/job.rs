//! Rescans as persistent jobs.
//!
//! One job item is one data source (plus a final item for library items whose data source is
//! gone). Items run on the **disk** lane of the job manager, a few sources at a time, and each is
//! saved in the job store, so after a crash or a quit [`resume_rescans`] finishes exactly the
//! sources that had not completed. A source's rescan is one write transaction, so a cancel never
//! leaves half a source behind.

use std::collections::HashSet;
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};

use cv_db::Database;
use cv_db::repo::data_sources::{get_source, list_sources};
use cv_domain::{ItemId, SourceKind};
use cv_jobs::{
    ItemControl, ItemError, ItemWork, JobHandle, JobManager, JobSpec, LaneId, StageError,
    StoreError,
};
use serde::{Deserialize, Serialize};

use super::{RescanError, RescanOptions, RescanReport, refresh_detached, rescan_held_source};

/// The job kind a rescan is stored and resumed under.
pub const RESCAN_JOB_KIND: &str = "library.rescan";

/// One unit of a rescan job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "target", rename_all = "snake_case")]
pub enum RescanTarget {
    /// One data source, by id. The path is only for display: the source is read again when the
    /// item runs, so a remap between submit and run is honoured.
    Source {
        id: i64,
        kind: SourceKind,
        path: String,
    },
    /// Items whose data source was removed or remapped away.
    Detached,
}

impl fmt::Display for RescanTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // Activity shows this as the item being worked on: the folder's own name ("Movies 4K"),
            // never the internal kind or the whole path.
            Self::Source { path, .. } => f.write_str(folder_name(path)),
            Self::Detached => f.write_str("titles whose folder was removed"),
        }
    }
}

/// The last segment of a folder path: `D:\Media\Movies 4K\` gives `Movies 4K`, a drive root its
/// letter.
fn folder_name(path: &str) -> &str {
    let trimmed = path.trim_end_matches(['/', '\\']);
    let name = trimmed.rsplit(['/', '\\']).next().unwrap_or(trimmed);
    if name.is_empty() { path } else { name }
}

/// The targets of a full rescan: every data source in library order, then the detached items
/// when there are any.
pub fn rescan_targets(db: &Database) -> Result<Vec<RescanTarget>, cv_db::DbError> {
    let mut targets: Vec<RescanTarget> = db
        .read(|conn| list_sources(conn, None))?
        .into_iter()
        .map(|s| RescanTarget::Source {
            id: s.id,
            kind: s.kind,
            path: s.path,
        })
        .collect();
    // The pass over titles whose folder was removed runs only when there are some, so a scan of
    // six folders is six items of work, as its name says.
    if super::has_detached(db)? {
        targets.push(RescanTarget::Detached);
    }
    Ok(targets)
}

/// The worker of a rescan job. Collects one report per source that ran.
#[derive(Clone)]
pub struct RescanWork {
    db: Arc<Database>,
    options: Arc<RescanOptions>,
    reports: Arc<Mutex<Vec<RescanReport>>>,
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl RescanWork {
    pub fn new(db: Arc<Database>, options: RescanOptions) -> Self {
        Self {
            db,
            options: Arc::new(options),
            reports: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// The reports of the sources that finished so far.
    pub fn reports(&self) -> Vec<RescanReport> {
        locked(&self.reports).clone()
    }
}

impl ItemWork for RescanWork {
    type Item = RescanTarget;

    /// `Scan of 4 folders → 212 new`: the folders that were read and the titles they added.
    fn result(&self) -> Option<String> {
        let reports = locked(&self.reports);
        if reports.is_empty() {
            return None;
        }
        let folders = reports.len();
        // Titles, as the library counts them: a new show with its 40 episodes is 1 new
        //. New episodes of shows already in the library are said apart.
        let titles: usize = reports.iter().map(|r| r.new_titles).sum();
        let episodes: usize = reports.iter().map(|r| r.new_episodes).sum();
        let mut found = match (titles, episodes) {
            (0, 0) => "no new titles".to_owned(),
            (0, 1) => "1 new episode".to_owned(),
            (0, episodes) => format!("{episodes} new episodes"),
            (titles, _) => format!("{titles} new"),
        };
        let relinked: usize = reports.iter().map(|r| r.relinked.len()).sum();
        match relinked {
            0 => {}
            1 => found.push_str(", 1 moved title found again"),
            n => found.push_str(&format!(", {n} moved titles found again")),
        }
        Some(format!(
            "Scan of {folders} {} \u{2192} {found}",
            if folders == 1 { "folder" } else { "folders" }
        ))
    }

    async fn process(&self, item: &RescanTarget, control: &ItemControl) -> Result<(), ItemError> {
        let db = Arc::clone(&self.db);
        let options = Arc::clone(&self.options);
        let control = control.clone();
        let target = item.clone();
        let outcome =
            tokio::task::spawn_blocking(move || -> Result<Option<RescanReport>, RescanError> {
                match target {
                    RescanTarget::Source { id, .. } => {
                        // A Rescan button press and a file-watcher batch can queue two jobs for
                        // the same source; without the lock both would plan from the same stored
                        // state and add the same new titles twice. The second one waits and then
                        // finds the first one's work already stored.
                        let _one_at_a_time = crate::lock::hold_sources([id]);
                        let Some(source) = db.read(|conn| get_source(conn, id))? else {
                            return Ok(None);
                        };
                        rescan_held_source(&db, &source, &options, &|| control.is_cancelled())
                            .map(Some)
                    }
                    RescanTarget::Detached => {
                        refresh_detached(&db)?;
                        Ok(None)
                    }
                }
            })
            .await;
        match outcome {
            Ok(Ok(Some(report))) => {
                locked(&self.reports).push(report);
                Ok(())
            }
            Ok(Ok(None)) => Ok(()),
            Ok(Err(RescanError::Cancelled)) => Err(ItemError::Cancelled),
            Ok(Err(RescanError::Db(error))) => {
                tracing::warn!(%error, "rescan failed");
                Err(StageError::new("database error").into())
            }
            Err(error) => {
                tracing::error!(%error, "rescan worker stopped");
                Err(StageError::new("scan worker stopped").into())
            }
        }
    }
}

/// A submitted rescan: the job and the reports its sources produce.
#[derive(Clone)]
pub struct RescanHandle {
    pub job: JobHandle,
    work: RescanWork,
}

impl RescanHandle {
    /// The reports of the sources that finished so far, in completion order.
    pub fn reports(&self) -> Vec<RescanReport> {
        self.work.reports()
    }

    /// The items the finished sources created or changed (new items first, then updated ones,
    /// each once). The input of the thumbnail job that follows a rescan.
    pub fn changed_items(&self) -> Vec<ItemId> {
        let mut seen = HashSet::new();
        let mut changed = Vec::new();
        for report in self.work.reports() {
            for id in report.new_items.iter().chain(&report.updated) {
                if seen.insert(*id) {
                    changed.push(*id);
                }
            }
        }
        changed
    }
}

/// The name Activity shows for a scan of `sources` folders: `Scanning 4 folders`.
#[must_use]
pub fn job_name(sources: usize) -> String {
    format!(
        "Scanning {sources} {}",
        if sources == 1 { "folder" } else { "folders" }
    )
}

/// Queues a persistent rescan of `targets` on the disk lane, at most `parallel_sources` sources
/// at a time. The manager needs a job store.
pub fn submit_rescan(
    manager: &JobManager,
    db: Arc<Database>,
    options: RescanOptions,
    targets: Vec<RescanTarget>,
    parallel_sources: usize,
) -> Result<RescanHandle, StoreError> {
    let work = RescanWork::new(db, options);
    // Named by the folders it reads; the pass over detached titles is not a folder.
    let folders = targets
        .iter()
        .filter(|target| matches!(target, RescanTarget::Source { .. }))
        .count();
    let spec = JobSpec::new(job_name(folders), LaneId::Disk).concurrency(parallel_sources);
    let job = manager.submit_persistent(RESCAN_JOB_KIND, spec, work.clone(), targets)?;
    Ok(RescanHandle { job, work })
}

/// Starts again the rescans that a crash or quit cut short, with only their unfinished sources.
pub fn resume_rescans(
    manager: &JobManager,
    db: &Arc<Database>,
    options: &RescanOptions,
) -> Result<Vec<RescanHandle>, StoreError> {
    let mut works: Vec<RescanWork> = Vec::new();
    let jobs = manager.resume_persisted(RESCAN_JOB_KIND, |_stored| {
        let work = RescanWork::new(Arc::clone(db), options.clone());
        works.push(work.clone());
        work
    })?;
    Ok(jobs
        .into_iter()
        .zip(works)
        .map(|(job, work)| RescanHandle { job, work })
        .collect())
}
