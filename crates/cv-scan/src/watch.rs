//! Optional file watching of the data sources.
//!
//! A [`FolderWatcher`] watches data-source folders and reports a [`Settled`] batch once a source
//! has been **quiet** for the debounce period. Copying a movie folder in produces a stream of
//! events for as long as the copy runs; every event pushes the deadline back, so the batch
//! arrives once, after the copy has been idle for the whole debounce, and the scan then sees
//! complete files.
//!
//! - The debounce is at least [`MIN_DEBOUNCE`] (5 s) and is 5 s by default
//!   ([`WatchOptions::default`]).
//! - Each source is debounced on its own, so a busy source does not hold back a quiet one.
//! - Reads are not changes: access events and events for files the walker ignores
//!   (`Thumbs.db`, `desktop.ini`, `.DS_Store`, dot-files) are dropped, so a rescan never wakes
//!   the watcher that started it.
//! - A root that cannot be watched (a disconnected drive) is reported in
//!   [`FolderWatcher::unwatched`]; the others keep working.
//!
//! The watcher only reports; it does not scan. [`rescan_settled`] runs an incremental
//! [`rescan_source`](crate::rescan::rescan_source) for the sources of a batch (unchanged files
//! cost nothing, see [`crate::rescan`]); the application may instead queue a rescan job for
//! them. [`Debouncer`] is the pure timing core, driven with explicit instants, so its behaviour
//! is tested without sleeping.
//!
//! Scanning every data source on startup is not file watching: it is
//! [`rescan_all`](crate::rescan::rescan_all), switched by a setting.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use cv_db::Database;
use cv_db::repo::data_sources::get_source;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::rescan::{RescanError, RescanOptions, RescanReport, rescan_source};
use crate::skip::{LibraryKind, SkipRules};

/// The shortest debounce the application accepts.
pub const MIN_DEBOUNCE: Duration = Duration::from_secs(5);

/// How watching behaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WatchOptions {
    debounce: Duration,
}

impl Default for WatchOptions {
    fn default() -> Self {
        Self {
            debounce: MIN_DEBOUNCE,
        }
    }
}

impl WatchOptions {
    /// Sets the quiet period, raised to [`MIN_DEBOUNCE`] when shorter.
    pub fn with_debounce(mut self, debounce: Duration) -> Self {
        self.debounce = debounce.max(MIN_DEBOUNCE);
        self
    }

    /// Sets any quiet period, including one below [`MIN_DEBOUNCE`]. For tests that cannot wait
    /// seconds; the application uses [`with_debounce`](Self::with_debounce).
    #[doc(hidden)]
    pub fn with_debounce_unchecked(mut self, debounce: Duration) -> Self {
        self.debounce = debounce;
        self
    }

    pub fn debounce(&self) -> Duration {
        self.debounce
    }
}

/// A data-source folder to watch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchedRoot {
    pub source_id: i64,
    pub path: PathBuf,
    /// Selects the ignore rules for file names.
    pub kind: LibraryKind,
}

impl WatchedRoot {
    pub fn new(source_id: i64, path: impl Into<PathBuf>, kind: LibraryKind) -> Self {
        Self {
            source_id,
            path: path.into(),
            kind,
        }
    }
}

/// What changed in one data source since it last settled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settled {
    pub source_id: i64,
    /// The top-level entries of the source that had changes, sorted: normally a movie or show
    /// folder, or a loose file in the source folder. An empty path means the source folder
    /// itself changed (renamed or removed).
    pub folders: Vec<PathBuf>,
}

#[derive(Debug, Default)]
struct Pending {
    last_event: Option<Instant>,
    folders: BTreeSet<PathBuf>,
}

/// The timing core: collects changes per source and releases a source once it has been quiet for
/// the debounce. It reads no clock; callers pass the instant.
#[derive(Debug)]
pub struct Debouncer {
    quiet: Duration,
    pending: HashMap<i64, Pending>,
}

impl Debouncer {
    pub fn new(quiet: Duration) -> Self {
        Self {
            quiet,
            pending: HashMap::new(),
        }
    }

    /// Records a change in `folder` (relative to the source) of `source_id` at `now`. Every call
    /// restarts the source's quiet period.
    pub fn record(&mut self, source_id: i64, folder: PathBuf, now: Instant) {
        let entry = self.pending.entry(source_id).or_default();
        entry.last_event = Some(now);
        entry.folders.insert(folder);
    }

    /// True when nothing is waiting.
    pub fn is_idle(&self) -> bool {
        self.pending.is_empty()
    }

    /// The earliest instant at which some source settles, if any is waiting.
    pub fn next_deadline(&self) -> Option<Instant> {
        self.pending
            .values()
            .filter_map(|p| p.last_event)
            .map(|t| t + self.quiet)
            .min()
    }

    /// Removes and returns the sources that have been quiet for the whole debounce at `now`,
    /// ordered by source id.
    pub fn take_settled(&mut self, now: Instant) -> Vec<Settled> {
        let quiet = self.quiet;
        let mut ids: Vec<i64> = self
            .pending
            .iter()
            .filter(|(_, p)| p.last_event.is_some_and(|t| now >= t + quiet))
            .map(|(id, _)| *id)
            .collect();
        ids.sort_unstable();
        ids.into_iter()
            .filter_map(|id| {
                self.pending.remove(&id).map(|p| Settled {
                    source_id: id,
                    folders: p.folders.into_iter().collect(),
                })
            })
            .collect()
    }
}

/// Why watching could not start.
#[derive(Debug, thiserror::Error)]
pub enum WatchError {
    #[error("the file watcher could not be created: {0}")]
    Create(#[source] notify::Error),
}

enum Message {
    Event(notify::Result<Event>),
    Stop,
}

/// Watches data-source folders until dropped.
pub struct FolderWatcher {
    // Held so the OS watches stay registered.
    _watcher: RecommendedWatcher,
    tx: Sender<Message>,
    thread: Option<JoinHandle<()>>,
    unwatched: Vec<(i64, notify::Error)>,
}

impl std::fmt::Debug for FolderWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FolderWatcher")
            .field("unwatched", &self.unwatched.len())
            .finish_non_exhaustive()
    }
}

impl FolderWatcher {
    /// Starts watching `roots` recursively. `on_settled` runs on the watcher's own thread, once
    /// per source and quiet period; keep it short or hand the batch to a job queue.
    pub fn start<F>(
        roots: &[WatchedRoot],
        options: &WatchOptions,
        mut on_settled: F,
    ) -> Result<Self, WatchError>
    where
        F: FnMut(Settled) + Send + 'static,
    {
        let (tx, rx) = mpsc::channel::<Message>();
        let event_tx = tx.clone();
        let mut watcher = notify::recommended_watcher(move |event| {
            // A closed channel means the watcher is being dropped.
            let _ = event_tx.send(Message::Event(event));
        })
        .map_err(WatchError::Create)?;

        let mut mapped: Vec<MappedRoot> = Vec::new();
        let mut unwatched = Vec::new();
        for root in roots {
            match watcher.watch(&root.path, RecursiveMode::Recursive) {
                Ok(()) => mapped.push(MappedRoot::new(root)),
                Err(error) => {
                    tracing::warn!(path = %root.path.display(), %error, "folder cannot be watched");
                    unwatched.push((root.source_id, error));
                }
            }
        }

        let quiet = options.debounce;
        let thread = thread::Builder::new()
            .name("cv-folder-watch".into())
            .spawn(move || {
                let mut debouncer = Debouncer::new(quiet);
                loop {
                    let message = match debouncer.next_deadline() {
                        None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
                        Some(deadline) => {
                            rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
                        }
                    };
                    match message {
                        Ok(Message::Event(Ok(event))) => {
                            let now = Instant::now();
                            for (source_id, folder) in changes_of(&event, &mapped) {
                                debouncer.record(source_id, folder, now);
                            }
                        }
                        Ok(Message::Event(Err(error))) => {
                            tracing::warn!(%error, "file watcher error");
                        }
                        Ok(Message::Stop) | Err(RecvTimeoutError::Disconnected) => break,
                        Err(RecvTimeoutError::Timeout) => {}
                    }
                    for settled in debouncer.take_settled(Instant::now()) {
                        on_settled(settled);
                    }
                }
            })
            .map_err(|e| WatchError::Create(notify::Error::io(e)))?;

        Ok(Self {
            _watcher: watcher,
            tx,
            thread: Some(thread),
            unwatched,
        })
    }

    /// The sources whose folder could not be watched, with the reason.
    pub fn unwatched(&self) -> &[(i64, notify::Error)] {
        &self.unwatched
    }
}

impl Drop for FolderWatcher {
    fn drop(&mut self) {
        let _ = self.tx.send(Message::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// A watched root with the spellings an event path may use (the folder as given and its
/// canonical form, which differ on macOS and through links).
struct MappedRoot {
    id: i64,
    prefixes: Vec<PathBuf>,
    rules: SkipRules,
}

impl MappedRoot {
    fn new(root: &WatchedRoot) -> Self {
        let mut prefixes = vec![root.path.clone()];
        if let Ok(canonical) = std::fs::canonicalize(&root.path)
            && canonical != root.path
        {
            prefixes.push(canonical);
        }
        Self {
            id: root.source_id,
            prefixes,
            rules: SkipRules::with_defaults(root.kind),
        }
    }

    /// The path below the root, if `path` lies inside it.
    fn relative<'a>(&self, path: &'a Path) -> Option<&'a Path> {
        self.prefixes
            .iter()
            .find_map(|prefix| path.strip_prefix(prefix).ok())
    }
}

/// True for kinds that mean the content or the set of names changed.
fn is_change(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) | EventKind::Any
    )
}

/// Content and metadata modifications, as opposed to renames.
fn is_dir_touch(kind: &EventKind) -> bool {
    matches!(kind, EventKind::Modify(m) if !matches!(m, notify::event::ModifyKind::Name(_)))
}

/// Maps an event to `(source id, top-level entry)` pairs, dropping reads and ignored names.
fn changes_of(event: &Event, roots: &[MappedRoot]) -> Vec<(i64, PathBuf)> {
    if !is_change(&event.kind) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for path in &event.paths {
        for root in roots {
            let Some(rel) = root.relative(path) else {
                continue;
            };
            let Some(first) = rel.components().next() else {
                // The root itself changed (renamed or removed).
                out.push((root.id, PathBuf::new()));
                break;
            };
            // Ignore noise anywhere in the path: dot-files, hidden and system folders and files.
            let ignored = rel.components().any(|c| {
                let name = c.as_os_str().to_string_lossy();
                root.rules.is_builtin_skipped(&name) || is_noise(&name)
            });
            if ignored {
                break;
            }
            // Directories report a modification whenever a child is added or removed; the
            // child's own event says what happened, so the parent's is noise.
            if is_dir_touch(&event.kind) && path.is_dir() {
                break;
            }
            out.push((root.id, PathBuf::from(first.as_os_str())));
            break;
        }
    }
    out
}

/// File names the operating system writes on its own.
fn is_noise(name: &str) -> bool {
    ["thumbs.db", "desktop.ini", ".ds_store"]
        .iter()
        .any(|n| name.eq_ignore_ascii_case(n))
}

/// Runs an incremental rescan for the source of each settled batch, in order, and returns the
/// reports. A source that was removed from the library since the batch was queued is skipped.
pub fn rescan_settled(
    db: &Database,
    batches: &[Settled],
    options: &RescanOptions,
) -> Result<Vec<RescanReport>, RescanError> {
    let mut reports = Vec::with_capacity(batches.len());
    for batch in batches {
        let Some(source) = db.read(|conn| get_source(conn, batch.source_id))? else {
            continue;
        };
        reports.push(rescan_source(db, &source, options)?);
    }
    Ok(reports)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t0() -> Instant {
        Instant::now()
    }

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn a_source_settles_only_after_the_full_quiet_period() {
        let start = t0();
        let mut d = Debouncer::new(secs(5));
        d.record(1, PathBuf::from("Heat (1995)"), start);
        assert!(d.take_settled(start + secs(4)).is_empty());
        let settled = d.take_settled(start + secs(5));
        assert_eq!(settled.len(), 1);
        assert_eq!(settled[0].source_id, 1);
        assert_eq!(settled[0].folders, vec![PathBuf::from("Heat (1995)")]);
        assert!(d.is_idle());
    }

    #[test]
    fn a_continuing_copy_keeps_pushing_the_deadline_and_settles_once() {
        let start = t0();
        let mut d = Debouncer::new(secs(5));
        // A copy that writes a file every 3 seconds for half a minute.
        let mut settled = Vec::new();
        for i in 0..10 {
            let now = start + secs(i * 3);
            d.record(1, PathBuf::from("Big Movie"), now);
            settled.extend(d.take_settled(now));
            settled.extend(d.take_settled(now + secs(2)));
        }
        assert!(settled.is_empty(), "no batch while the copy is running");
        // The last write was at 27 s; the batch is due at 32 s.
        assert_eq!(d.next_deadline(), Some(start + secs(32)));
        assert!(d.take_settled(start + secs(31)).is_empty());
        let batch = d.take_settled(start + secs(32));
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0].folders, vec![PathBuf::from("Big Movie")]);
        // Nothing is released twice.
        assert!(d.take_settled(start + secs(60)).is_empty());
        assert_eq!(d.next_deadline(), None);
    }

    #[test]
    fn sources_are_debounced_independently_and_folders_are_merged() {
        let start = t0();
        let mut d = Debouncer::new(secs(5));
        d.record(2, PathBuf::from("B"), start);
        d.record(1, PathBuf::from("A"), start + secs(1));
        d.record(1, PathBuf::from("A2"), start + secs(3));
        d.record(1, PathBuf::from("A"), start + secs(4));
        let first = d.take_settled(start + secs(5));
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].source_id, 2);
        assert_eq!(d.next_deadline(), Some(start + secs(9)));
        let second = d.take_settled(start + secs(9));
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].source_id, 1);
        assert_eq!(
            second[0].folders,
            vec![PathBuf::from("A"), PathBuf::from("A2")]
        );
    }

    #[test]
    fn debounce_is_never_shorter_than_five_seconds() {
        let o = WatchOptions::default().with_debounce(Duration::from_millis(200));
        assert_eq!(o.debounce(), MIN_DEBOUNCE);
        let o = WatchOptions::default().with_debounce(secs(30));
        assert_eq!(o.debounce(), secs(30));
        assert_eq!(WatchOptions::default().debounce(), secs(5));
    }

    fn root(id: i64, path: &str) -> MappedRoot {
        MappedRoot::new(&WatchedRoot::new(id, path, LibraryKind::Movies))
    }

    fn event(kind: EventKind, path: &str) -> Event {
        Event::new(kind).add_path(PathBuf::from(path))
    }

    #[test]
    fn events_map_to_the_top_level_folder_and_drop_reads_and_noise() {
        use notify::event::{AccessKind, CreateKind, ModifyKind};
        let roots = [root(7, "/lib")];
        let create = EventKind::Create(CreateKind::File);

        let a = changes_of(&event(create, "/lib/Heat (1995)/Heat.mkv"), &roots);
        assert_eq!(a, vec![(7, PathBuf::from("Heat (1995)"))]);

        let b = changes_of(&event(create, "/lib/loose.mkv"), &roots);
        assert_eq!(b, vec![(7, PathBuf::from("loose.mkv"))]);

        let read = EventKind::Access(AccessKind::Read);
        assert!(changes_of(&event(read, "/lib/Heat (1995)/Heat.mkv"), &roots).is_empty());
        assert!(changes_of(&event(create, "/lib/Heat (1995)/Thumbs.db"), &roots).is_empty());
        assert!(changes_of(&event(create, "/lib/.hidden/x.mkv"), &roots).is_empty());
        assert!(changes_of(&event(create, "/elsewhere/x.mkv"), &roots).is_empty());

        let modify = EventKind::Modify(ModifyKind::Any);
        assert_eq!(
            changes_of(&event(modify, "/lib/A/B/c.srt"), &roots),
            vec![(7, PathBuf::from("A"))]
        );
    }
}
