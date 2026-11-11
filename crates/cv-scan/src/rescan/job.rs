//! Rescans as persistent jobs.
//!
//! One job item is one data source (plus a final item for library items whose data source is
//! gone). Items run on the **disk** lane of the job manager, a few sources at a time, and each is
//! saved in the job store, so after a crash or a quit [`resume_rescans`] finishes exactly the
//! sources that had not completed. A source's rescan is one write transaction, so a cancel never
//! leaves half a source behind. An item can also be one title's folder inside a source
//! ([`RescanTarget::Folder`], "Scan again" on a title page), read alone with
//! [`super::rescan_folder`]'s rules.
//!
//! The job tells its progress in titles ([`super::progress`]): it first counts the titles of the
//! folders it will read, then reports each title as the walks read it.

use std::collections::HashSet;
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, mpsc};
use std::time::Instant;

use cv_db::Database;
use cv_db::repo::data_sources::{get_source, list_sources};
use cv_domain::{ItemId, SourceKind};
use cv_jobs::{
    ItemControl, ItemError, ItemWork, JobHandle, JobManager, JobSpec, LaneId, StageError,
    StoreError,
};
use serde::{Deserialize, Serialize};

use super::progress::{COUNT_DEADLINE, ScanProgress};
use super::{
    RescanError, RescanOptions, RescanReport, RescanScope, count_source_titles, refresh_detached,
    rescan_held_source_in,
};

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
    /// One folder inside a data source, a title's folder, read alone: its files, and its titles
    /// found again or gone. Nothing else in the source is read or changed.
    Folder {
        /// The data source, by id; read again when the item runs.
        id: i64,
        kind: SourceKind,
        /// The folder, a full path inside the source.
        folder: String,
        /// What Activity calls it: the title's name.
        name: String,
    },
}

impl RescanTarget {
    /// The key of the target's titles in the job's progress.
    fn key(&self) -> String {
        match self {
            Self::Source { id, .. } => format!("source:{id}"),
            Self::Folder { id, folder, .. } => format!("folder:{id}:{folder}"),
            Self::Detached => "detached".to_owned(),
        }
    }
}

impl fmt::Display for RescanTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // Activity shows this as the item being worked on: the folder's own name ("Movies 4K"),
            // never the internal kind or the whole path.
            Self::Source { path, .. } => f.write_str(folder_name(path)),
            Self::Detached => f.write_str("titles whose folder was removed"),
            Self::Folder { name, .. } => f.write_str(name),
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
    progress: Arc<ScanProgress>,
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
            progress: Arc::default(),
        }
    }

    /// The job's progress in titles.
    pub fn progress(&self) -> &ScanProgress {
        &self.progress
    }

    fn connect(&self, control: &ItemControl) {
        let control = control.clone();
        self.progress
            .connect(Arc::new(move |units| control.set_units(units)));
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

    /// Counts the titles of every folder the job will read, side by side, for the first phase of
    /// its progress; a folder that has not answered by [`COUNT_DEADLINE`] is counted by its walk.
    async fn prepare(&self, items: &[&RescanTarget], control: &ItemControl) {
        self.connect(control);
        let (sender, receiver) = mpsc::channel::<(String, Option<usize>)>();
        // The folders still being listed, by key, with the name the progress shows.
        let mut listing: Vec<(String, String)> = Vec::new();
        for target in items {
            match target {
                RescanTarget::Source { id, path, .. } => {
                    let (db, options, id, key) = (
                        Arc::clone(&self.db),
                        Arc::clone(&self.options),
                        *id,
                        target.key(),
                    );
                    let sender = sender.clone();
                    let entry = (key.clone(), folder_name(path).to_owned());
                    // A listing of a dead network share can block for long; it is left behind.
                    let spawned = std::thread::Builder::new()
                        .name("cv-scan-count".to_owned())
                        .spawn(move || {
                            let titles = db
                                .read(|conn| get_source(conn, id))
                                .ok()
                                .flatten()
                                .and_then(|source| {
                                    count_source_titles(&db, &source, &options).ok().flatten()
                                });
                            let _ = sender.send((key, titles));
                        });
                    if spawned.is_ok() {
                        listing.push(entry);
                    }
                }
                RescanTarget::Folder { .. } => self.progress.counted(&target.key(), 1),
                RescanTarget::Detached => {}
            }
        }
        drop(sender);
        let progress = Arc::clone(&self.progress);
        let control = control.clone();
        let counting = tokio::task::spawn_blocking(move || {
            let deadline = Instant::now() + COUNT_DEADLINE;
            while let Some((_, name)) = listing.first() {
                progress.listing(name);
                if control.is_cancelled() {
                    return;
                }
                let left = deadline.saturating_duration_since(Instant::now());
                let Ok((key, titles)) = receiver.recv_timeout(left) else {
                    return;
                };
                listing.retain(|(waiting, _)| *waiting != key);
                if let Some(titles) = titles {
                    progress.counted(&key, titles);
                }
            }
        });
        let _ = counting.await;
        self.progress.start_working();
    }

    async fn process(&self, item: &RescanTarget, control: &ItemControl) -> Result<(), ItemError> {
        let db = Arc::clone(&self.db);
        let options = Arc::clone(&self.options);
        let progress = Arc::clone(&self.progress);
        let control = control.clone();
        let target = item.clone();
        let key = item.key();
        let outcome =
            tokio::task::spawn_blocking(move || -> Result<Option<RescanReport>, RescanError> {
                let cancelled = || control.is_cancelled();
                let outcome = match &target {
                    RescanTarget::Source { id, .. } => {
                        // A Rescan button press and a file-watcher batch can queue two jobs for
                        // the same source; without the lock both would plan from the same stored
                        // state and add the same new titles twice. The second one waits and then
                        // finds the first one's work already stored.
                        let _one_at_a_time = crate::lock::hold_sources([*id]);
                        let Some(source) = db.read(|conn| get_source(conn, *id))? else {
                            progress.finished(&key);
                            return Ok(None);
                        };
                        let observer = progress.observer(&key);
                        let scope = RescanScope {
                            focus: None,
                            observer: Some(&observer),
                        };
                        rescan_held_source_in(&db, &source, &options, &cancelled, scope).map(Some)
                    }
                    RescanTarget::Folder {
                        id, folder, name, ..
                    } => {
                        let _one_at_a_time = crate::lock::hold_sources([*id]);
                        let Some(source) = db.read(|conn| get_source(conn, *id))? else {
                            progress.finished(&key);
                            return Ok(None);
                        };
                        progress.reading_title(&key, name);
                        let scope = RescanScope {
                            focus: Some(folder),
                            observer: None,
                        };
                        rescan_held_source_in(&db, &source, &options, &cancelled, scope).map(Some)
                    }
                    RescanTarget::Detached => refresh_detached(&db).map(|_| None),
                };
                if outcome.is_ok() {
                    progress.finished(&key);
                }
                outcome
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

/// The name Activity shows for a scan of one place: `Scanning Movies 4K`, `Scanning Example
/// Movie`.
#[must_use]
pub fn job_name_of(place: &str) -> String {
    format!("Scanning {place}")
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
    // Named by the folders it reads; the pass over detached titles is not a folder.
    let folders = targets
        .iter()
        .filter(|target| !matches!(target, RescanTarget::Detached))
        .count();
    submit_rescan_named(
        manager,
        db,
        options,
        targets,
        parallel_sources,
        job_name(folders),
    )
}

/// [`submit_rescan`] under a name of the caller's ([`job_name_of`]).
pub fn submit_rescan_named(
    manager: &JobManager,
    db: Arc<Database>,
    options: RescanOptions,
    targets: Vec<RescanTarget>,
    parallel_sources: usize,
    name: String,
) -> Result<RescanHandle, StoreError> {
    let work = RescanWork::new(db, options);
    let spec = JobSpec::new(name, LaneId::Disk).concurrency(parallel_sources);
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
