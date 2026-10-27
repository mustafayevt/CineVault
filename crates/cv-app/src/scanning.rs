//! The scan pipeline as the app runs it: Rescan, scan on startup, the folder watcher,
//! Find missing, thumbnails and media info after a scan, and the health of each data source.
//!
//! The rules live in `cv-scan` (what a rescan does), `cv-artwork` (the thumbnail job) and
//! `cv-media` (reading media info). [`ScanService`] connects them to the app's one database, job
//! manager and settings, so the desktop shell and the CLI run the same pipeline:
//!
//! ```text
//! Rescan / startup / watcher batch
//!     -> rescan job (disk lane, persistent)           resumed on launch
//!         -> thumbnail job for new and changed items  (cpu lane, low priority)
//!         -> media-info sweep for titles still pending (sidecars, runtime, aspect ratio)
//! ```
//!
//! # Settings
//!
//! | Key | Default | Meaning |
//! |---|---|---|
//! | `library.watch-debounce-seconds` | 5 | the quiet period before a changed source is rescanned; values below 5 are raised to 5 |
//! | `library.skip-patterns` | `MAKEMKV` | folder names (regular expressions, or a full path) a scan does not enter; entries are separated by `;` |
//! | `library.skip-nomedia` | on | a folder holding a `.nomedia` file is skipped (`.cinevaultignore` is always honoured) |
//! | `library.bad-words` | none | words stripped from file and folder names before the title is parsed; entries are separated by `,` |
//! | `library.vsmeta-artwork` | off | read the artwork a NAS stores in `.vsmeta` files |
//! | `media.store-mediainfo-xml` | off | write a MediaInfo XML sidecar next to each video that was read |
//! | `media.runtime-from-media` | off | use the video length as the runtime |
//! | `media.detect-aspect-ratio` | off | measure the visible picture ratio with FFmpeg |
//!
//! Scanning on start and watching the data sources are always on: they have no setting. A scan
//! only reads the folders and the NFO files in them; it never contacts a provider and never
//! renames. ([`ScanService::set_background_scans`] switches both off for tests.)
//!
//! # Media info and the tools
//!
//! Media info needs the MediaInfo library from the tools manager. When it is not installed the
//! scan still completes: the new titles keep video rows without a container format, which is what
//! "media info pending" means, and [`MediaInfoStatus::Missing`] tells the caller to offer the
//! download. A notice says so once per session. Installing the tool and calling
//! [`ScanService::request_media_info`] reads everything that is pending.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::Duration;

use cv_artwork::thumb::ThumbCache;
use cv_artwork::thumb_job::{build_after, default_parallelism as thumb_parallelism};
use cv_db::repo::clear_all_new;
use cv_db::repo::data_sources::{get_source, list_sources};
use cv_db::repo::media_info::{SourceCounts, pending_media_info, source_counts};
use cv_db::{Database, DbError};
use cv_domain::{DataSource, ItemId, SourceKind};
use cv_jobs::{JobManager, StoreError};
use cv_media::tools::ToolsManager;
use cv_scan::rescan::job::{
    RescanHandle, RescanTarget, rescan_targets, resume_rescans, submit_rescan,
};
use cv_scan::rescan::{RescanOptions, Skipped};
use cv_scan::skip::{DEFAULT_USER_PATTERNS, LibraryKind, SkipRules};
use cv_scan::unmanaged::{UnmanagedReport, find_unmanaged_all};
use cv_scan::walk::WalkOptions;
use cv_scan::watch::{FolderWatcher, MIN_DEBOUNCE, Settled, WatchOptions, WatchedRoot};

use crate::app_settings::SettingsService;
use crate::media_info::{
    MediaInfoSession, MediaInfoSettings, MediaInfoWork, MediaReader, ReaderSource, TOOLS_PAGE,
    default_parallelism as media_parallelism, reload_titles, submit_media_info, submit_named,
    tools_reader_source,
};
use crate::notices::{AppNotice, NoticeHub, NoticeLevel};

/// Setting: clear the NEW flag of every title when a data-source update starts, so only what
/// that update finds is flagged. Off by default.
pub const RESET_NEW_ON_UPDATE_KEY: &str = "library.reset-new-on-update";
/// Setting: seconds a source must stay quiet before it is rescanned.
pub const WATCH_DEBOUNCE_KEY: &str = "library.watch-debounce-seconds";
/// Setting: folder names a scan does not enter, separated by `;`.
pub const SKIP_PATTERNS_KEY: &str = "library.skip-patterns";
/// Setting: honour a `.nomedia` marker file.
pub const SKIP_NOMEDIA_KEY: &str = "library.skip-nomedia";
/// Setting: use the artwork stored in `.vsmeta` files.
pub const VSMETA_ARTWORK_KEY: &str = "library.vsmeta-artwork";
/// Setting: words removed from names before the title is parsed, separated by `,`.
pub const BAD_WORDS_KEY: &str = "library.bad-words";

/// Called with the titles a finished rescan created, so they can be matched.
pub type AfterScan = Arc<dyn Fn(Vec<ItemId>) + Send + Sync>;

/// Sources a rescan job works on at the same time.
const SOURCE_PARALLELISM: usize = 2;
/// How long the sidebar waits for a folder to answer before it shows the source as offline.
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// The scan settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanSettings {
    /// Clear every NEW flag when an update of the data sources starts.
    pub reset_new_on_update: bool,
    /// The quiet period before a changed source is rescanned (at least five seconds).
    pub debounce: Duration,
}

impl ScanSettings {
    /// Reads the settings; anything missing takes its default.
    #[must_use]
    pub fn read(settings: &SettingsService) -> Self {
        let flag = |key: &str| settings.get(key).and_then(|value| value.as_bool());
        let debounce = settings
            .get(WATCH_DEBOUNCE_KEY)
            .and_then(|value| value.as_u64())
            .map_or(MIN_DEBOUNCE, Duration::from_secs)
            .max(MIN_DEBOUNCE);
        Self {
            reset_new_on_update: flag(RESET_NEW_ON_UPDATE_KEY).unwrap_or(false),
            debounce,
        }
    }
}

/// Splits a setting that holds a list as text; blank entries go.
fn split_list(value: Option<&serde_json::Value>, separator: char) -> Option<Vec<String>> {
    let text = value?.as_str()?;
    Some(
        text.split(separator)
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .map(str::to_owned)
            .collect(),
    )
}

/// The rescan options the three scan settings stand for. A missing or mistyped value takes its
/// default: the built-in skip list, `.nomedia` honoured and no bad words.
#[must_use]
pub fn rescan_options_from(
    skip_patterns: Option<&serde_json::Value>,
    skip_nomedia: Option<&serde_json::Value>,
    bad_words: Option<&serde_json::Value>,
    vsmeta_artwork: Option<&serde_json::Value>,
) -> RescanOptions {
    let patterns = split_list(skip_patterns, ';').unwrap_or_else(|| {
        DEFAULT_USER_PATTERNS
            .iter()
            .map(|p| (*p).to_owned())
            .collect()
    });
    let nomedia = skip_nomedia
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true);
    // Bad words are kept in lower case; they match without regard to case.
    let words: Vec<String> = split_list(bad_words, ',')
        .unwrap_or_default()
        .into_iter()
        .map(|word| word.to_lowercase())
        .collect();
    let vsmeta = vsmeta_artwork
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let walk = |kind| {
        WalkOptions::new(SkipRules::new(kind, &patterns, nomedia), words.clone())
            .with_vsmeta_artwork(vsmeta)
    };
    RescanOptions {
        movies: walk(LibraryKind::Movies),
        tv: walk(LibraryKind::TvShows),
        ..RescanOptions::default()
    }
}

/// Why a scan request failed.
#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("The library database could not be read: {0}")]
    Db(#[from] DbError),
    #[error("The scan could not be queued: {0}")]
    Jobs(#[from] StoreError),
}

/// A rescan that was queued.
#[derive(Clone)]
pub struct RescanRun {
    /// The job and its reports.
    pub handle: RescanHandle,
    /// Data sources it covers.
    pub sources: usize,
}

/// Whether media info can be read right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaInfoStatus {
    /// MediaInfo is installed; pending titles are being read.
    Ready,
    /// MediaInfo is not installed. Titles stay pending; offer the download.
    Missing,
}

/// Whether a data source's folder can be reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceState {
    Online,
    /// The drive is disconnected or the share is down; its titles stay in the library.
    Offline,
}

/// One data source with its live state, for the sidebar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceHealth {
    pub source: DataSource,
    pub state: SourceState,
    pub counts: SourceCounts,
}

#[derive(Default)]
struct Sweep {
    running: bool,
    dirty: bool,
}

/// What Reload media info did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MediaInfoReload {
    /// Titles with a video that were reloaded.
    pub titles: usize,
    /// Titles whose details were stored again.
    pub read: u64,
    /// Titles that could not be read again: title and the short reason.
    pub failed: Vec<(String, String)>,
}

/// Why Reload media info did not run.
#[derive(Debug, thiserror::Error)]
pub enum ReloadError {
    /// MediaInfo is not installed or its path is broken; nothing was cleared.
    #[error("{0}")]
    NoMediaInfo(String),
    #[error("the library could not be changed: {0}")]
    Library(String),
}

/// Runs the scan pipeline over the app's shared services.
pub struct ScanService {
    db: Arc<Database>,
    jobs: JobManager,
    thumbs: Arc<ThumbCache>,
    settings: Arc<SettingsService>,
    notices: Arc<NoticeHub>,
    tools: Arc<ToolsManager>,
    reader: Mutex<ReaderSource>,
    session: Arc<MediaInfoSession>,
    watcher: Mutex<Option<FolderWatcher>>,
    runtime: Mutex<Option<tokio::runtime::Handle>>,
    sweep: Mutex<Sweep>,
    follow_ups: AtomicUsize,
    after_scan: Mutex<Option<AfterScan>>,
    /// Whether the scan on start and the folder watcher run (always, except in tests).
    background: AtomicBool,
    /// Called with the titles whose media info a pass or a reload stored.
    after_media_info: Mutex<Option<AfterScan>>,
    missing_tool_announced: Mutex<bool>,
    /// The library folders a scan skipped, by path, with the title of their warning and why.
    skips_announced: Mutex<HashMap<String, (String, Skipped)>>,
    /// What each library folder's last probe said, by path, so one slow answer (a disk busy
    /// with the start-up scan) does not turn a healthy folder offline.
    probes: Mutex<HashMap<String, ProbeMemory>>,
    /// Announces the titles whose media info was read, so open pages refresh.
    changes: crate::changes::Changes,
    me: Weak<ScanService>,
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl ScanService {
    /// Builds the service over the app's shared parts. Nothing runs until a method is called.
    #[must_use]
    pub fn new(
        db: Arc<Database>,
        jobs: JobManager,
        thumbs: Arc<ThumbCache>,
        settings: Arc<SettingsService>,
        notices: Arc<NoticeHub>,
        tools: Arc<ToolsManager>,
    ) -> Arc<Self> {
        let reader = tools_reader_source(Arc::clone(&tools));
        Arc::new_cyclic(|me| Self {
            db,
            jobs,
            thumbs,
            settings,
            notices,
            tools,
            reader: Mutex::new(reader),
            session: Arc::default(),
            watcher: Mutex::new(None),
            runtime: Mutex::new(None),
            sweep: Mutex::new(Sweep::default()),
            follow_ups: AtomicUsize::new(0),
            after_scan: Mutex::new(None),
            background: AtomicBool::new(true),
            after_media_info: Mutex::new(None),
            missing_tool_announced: Mutex::new(false),
            skips_announced: Mutex::new(HashMap::new()),
            probes: Mutex::new(HashMap::new()),
            changes: crate::changes::Changes::default(),
            me: me.clone(),
        })
    }

    /// Connects the service to the hub that announces what changed to open pages.
    pub fn set_changes(&self, hub: Arc<crate::changes::ChangeHub>) {
        self.changes.set(hub);
    }

    /// Replaces how media info is read. For tests, which have no MediaInfo library.
    #[doc(hidden)]
    pub fn set_media_reader(&self, source: ReaderSource) {
        *locked(&self.reader) = source;
    }

    /// Switches the scan on start and the folder watcher on (the default) or off. Tests that
    /// scan on their own turn them off, so nothing else walks their folders. The app never does.
    pub fn set_background_scans(&self, on: bool) {
        self.background.store(on, Ordering::SeqCst);
    }

    /// Sets what happens with the titles a finished rescan created: the app reads the NFO files
    /// next to them.
    pub fn set_after_scan(&self, hook: AfterScan) {
        *locked(&self.after_scan) = Some(hook);
    }

    /// Sets what runs for the titles whose media info was stored (their NFO carries it too).
    pub fn set_after_media_info(&self, hook: AfterScan) {
        *locked(&self.after_media_info) = Some(hook);
    }

    fn media_info_stored(&self, items: Vec<ItemId>) {
        if items.is_empty() {
            return;
        }
        let hook = locked(&self.after_media_info).clone();
        if let Some(hook) = hook {
            hook(items);
        }
    }

    /// The walk and parse options of a rescan, from the skip and bad-word settings.
    #[must_use]
    pub fn rescan_options(&self) -> RescanOptions {
        rescan_options_from(
            self.settings.get(SKIP_PATTERNS_KEY).as_ref(),
            self.settings.get(SKIP_NOMEDIA_KEY).as_ref(),
            self.settings.get(BAD_WORDS_KEY).as_ref(),
            self.settings.get(VSMETA_ARTWORK_KEY).as_ref(),
        )
    }

    // ---- rescans ---------------------------------------------------------------------------

    /// Queues a rescan of every data source (and of the titles whose source is gone). The
    /// thumbnails and the media info of what it finds follow by themselves. Must be called inside
    /// a tokio runtime.
    ///
    /// # Errors
    /// The database cannot be read or the job cannot be stored.
    pub fn rescan_all(&self) -> Result<RescanRun, ScanError> {
        let targets = rescan_targets(&self.db)?;
        self.reset_new_if_asked()?;
        self.submit(targets)
    }

    /// Dates the titles that an earlier scan stamped all at once from their files. It runs once,
    /// in the background, ahead of the next scan, so the lists that refresh when that scan ends
    /// already show the repaired dates.
    fn repair_date_added(&self) {
        let (db, settings) = (Arc::clone(&self.db), Arc::clone(&self.settings));
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .and_then(|d| i64::try_from(d.as_millis()).ok())
            .unwrap_or(i64::MAX);
        drop(tokio::task::spawn_blocking(move || {
            if let Err(error) = crate::date_added::repair(&db, &settings, now_ms) {
                tracing::warn!(%error, "the date added repair could not run");
            }
        }));
    }

    /// Queues a rescan of one data source, or returns `None` when it no longer exists.
    ///
    /// # Errors
    /// As [`Self::rescan_all`].
    pub fn rescan_source(&self, id: i64) -> Result<Option<RescanRun>, ScanError> {
        let Some(source) = self.db.read(|conn| get_source(conn, id))? else {
            return Ok(None);
        };
        let target = RescanTarget::Source {
            id: source.id,
            kind: source.kind,
            path: source.path,
        };
        self.reset_new_if_asked()?;
        self.submit(vec![target]).map(Some)
    }

    /// With the "reset NEW on update" setting on, clears every NEW flag before an update of the
    /// data sources, so the flags afterwards mark what that update found.
    fn reset_new_if_asked(&self) -> Result<(), ScanError> {
        if ScanSettings::read(&self.settings).reset_new_on_update {
            self.db.write(|tx| clear_all_new(tx, None))?;
        }
        Ok(())
    }

    fn submit(&self, targets: Vec<RescanTarget>) -> Result<RescanRun, ScanError> {
        self.repair_date_added();
        let sources = targets
            .iter()
            .filter(|target| matches!(target, RescanTarget::Source { .. }))
            .count();
        let handle = submit_rescan(
            &self.jobs,
            Arc::clone(&self.db),
            self.rescan_options(),
            targets,
            SOURCE_PARALLELISM,
        )?;
        self.follow_up(&handle);
        Ok(RescanRun { handle, sources })
    }

    /// Starts again the rescans that a quit cut short. Returns how many.
    ///
    /// # Errors
    /// The stored jobs cannot be read.
    pub fn resume(&self) -> Result<usize, StoreError> {
        let handles = resume_rescans(&self.jobs, &self.db, &self.rescan_options())?;
        for handle in &handles {
            self.follow_up(handle);
        }
        Ok(handles.len())
    }

    /// What follows a rescan, whichever way it was started: thumbnails for the items it created
    /// or changed, then a notice for sources that could not be read, then media info.
    fn follow_up(&self, handle: &RescanHandle) {
        let changed = handle.clone();
        // The cache job needs nothing more from us; it queues itself when the rescan ends.
        drop(build_after(
            &self.jobs,
            &self.db,
            &self.thumbs,
            handle.job.clone(),
            move || changed.changed_items(),
            thumb_parallelism(),
        ));
        let me = self.me.clone();
        let handle = handle.clone();
        self.follow_ups.fetch_add(1, Ordering::SeqCst);
        cv_jobs::spawn(async move {
            handle.job.wait().await;
            if let Some(service) = me.upgrade() {
                service.after_rescan(&handle);
                service.follow_ups.fetch_sub(1, Ordering::SeqCst);
            }
        });
    }

    /// A library folder got a new label: its warning, if a scan skipped it, names it so at once.
    pub fn source_relabelled(&self, source: &DataSource) {
        let mut announced = locked(&self.skips_announced);
        let Some((title, skipped)) = announced.get_mut(&source.path) else {
            return;
        };
        let notice = skip_notice(source, *skipped).topic(skip_topic(&source.path));
        if *title != notice.title {
            title.clone_from(&notice.title);
            self.notices.publish(notice);
        }
    }

    fn after_rescan(&self, handle: &RescanHandle) {
        let reports = handle.reports();
        for report in &reports {
            let path = report.source.path.clone();
            let topic = skip_topic(&path);
            // An empty folder only looks like a disconnected drive when the library holds titles
            // of it: a folder added empty on purpose is just empty.
            let skipped = report.skipped.filter(|skipped| {
                *skipped != Skipped::SourceEmpty
                    || self
                        .db
                        .read(|c| source_counts(c, &path))
                        .map_or(true, |counts| counts.titles > 0)
            });
            let mut announced = locked(&self.skips_announced);
            let Some(skipped) = skipped else {
                // The folder reads again: its warning no longer holds.
                if announced.remove(&path).is_some() {
                    self.notices.publish(AppNotice::resolved(topic));
                }
                continue;
            };
            let notice = skip_notice(&report.source, skipped).topic(topic);
            // Said once per state; a new label or another reason replaces the card.
            if announced
                .get(&path)
                .is_some_and(|(title, _)| *title == notice.title)
            {
                continue;
            }
            announced.insert(path, (notice.title.clone(), skipped));
            self.notices.publish(notice);
        }
        let new_items: usize = reports.iter().map(|r| r.new_items.len()).sum();
        let updated: usize = reports.iter().map(|r| r.updated.len()).sum();
        tracing::info!(new_items, updated, "library scan finished");
        self.request_media_info();
        let hook = locked(&self.after_scan).clone();
        if let Some(hook) = hook {
            let found: Vec<ItemId> = reports
                .iter()
                .flat_map(|report| report.new_items.iter().copied())
                .collect();
            if !found.is_empty() {
                hook(found);
            }
        }
    }

    // ---- the watcher -----------------------------------------------------------------------

    /// Starts, restarts or stops the folder watcher to match the settings and the current data
    /// sources. Call it after the watch setting or the list of sources changes.
    pub fn refresh_watcher(&self) {
        let settings = ScanSettings::read(&self.settings);
        // Dropping the old watcher stops its thread before the new one starts.
        let mut slot = locked(&self.watcher);
        *slot = None;
        if !self.background.load(Ordering::SeqCst) {
            return;
        }
        let Some(runtime) = locked(&self.runtime)
            .clone()
            .or_else(|| tokio::runtime::Handle::try_current().ok())
        else {
            tracing::warn!("folder watching needs the async runtime; it is not started");
            return;
        };
        let sources = match self.db.read(|conn| list_sources(conn, None)) {
            Ok(sources) => sources,
            Err(error) => {
                tracing::warn!(%error, "cannot list data sources to watch");
                return;
            }
        };
        let answers = probe_all(
            sources.iter().map(|source| source.path.as_str()),
            PROBE_TIMEOUT,
        );
        let roots: Vec<WatchedRoot> = sources
            .iter()
            .zip(answers)
            .filter(|(_, answer)| *answer == Some(true))
            .map(|(source, _)| {
                let kind = match source.kind {
                    SourceKind::Movie => LibraryKind::Movies,
                    SourceKind::Tv => LibraryKind::TvShows,
                };
                WatchedRoot::new(source.id, &source.path, kind)
            })
            .collect();
        if roots.is_empty() {
            return;
        }
        let me = self.me.clone();
        let options = WatchOptions::default().with_debounce(settings.debounce);
        let started = FolderWatcher::start(&roots, &options, move |settled| {
            if let Some(service) = me.upgrade() {
                // This runs on the watcher's own thread; the job manager needs the runtime.
                let _inside = runtime.enter();
                service.on_settled(&settled);
            }
        });
        match started {
            Ok(watcher) => {
                for (id, error) in watcher.unwatched() {
                    tracing::warn!(source = id, %error, "a data source cannot be watched");
                }
                *slot = Some(watcher);
            }
            Err(error) => tracing::warn!(%error, "the folder watcher could not start"),
        }
    }

    /// Whether a watcher is running.
    #[must_use]
    pub fn is_watching(&self) -> bool {
        locked(&self.watcher).is_some()
    }

    fn on_settled(&self, settled: &Settled) {
        match self.rescan_source(settled.source_id) {
            Ok(Some(_)) => {
                tracing::info!(source = settled.source_id, "queued a rescan after changes")
            }
            Ok(None) => {}
            Err(error) => tracing::warn!(%error, "could not queue a rescan after changes"),
        }
    }

    // ---- startup and shutdown --------------------------------------------------------------

    /// Starts the scan work of a launch: the scan on startup (unless the previous session's scan
    /// was resumed instead or there is no data source), the watcher, and the media info that is
    /// still pending. Must be called inside a tokio runtime.
    pub fn startup(&self, resumed_rescans: usize) {
        *locked(&self.runtime) = tokio::runtime::Handle::try_current().ok();
        let has_sources = self
            .db
            .read(|conn| list_sources(conn, None))
            .is_ok_and(|sources| !sources.is_empty());
        if self.background.load(Ordering::SeqCst)
            && resumed_rescans == 0
            && has_sources
            && let Err(error) = self.rescan_all()
        {
            tracing::warn!(%error, "the scan on startup could not be queued");
            self.notices.publish(
                AppNotice::new(NoticeLevel::Error, "The scan on startup could not start")
                    .detail(error.to_string())
                    .source("Library scan"),
            );
        }
        self.refresh_watcher();
        self.request_media_info();
    }

    /// Stops the watcher.
    pub fn shutdown(&self) {
        *locked(&self.watcher) = None;
    }

    // ---- media info ------------------------------------------------------------------------

    /// Reads the media info of every title that is still pending, in the background. Calls while
    /// a pass is running make it look again when it ends, so nothing a rescan adds meanwhile is
    /// missed. Returns [`MediaInfoStatus::Missing`] (after telling the user once) when MediaInfo
    /// is not installed. Must be called inside a tokio runtime.
    pub fn request_media_info(&self) -> MediaInfoStatus {
        if let Err(error) = self.open_reader() {
            // Say so only when something is waiting; an empty library has nothing to read.
            let waiting = self
                .db
                .read(|conn| pending_media_info(conn, 1))
                .is_ok_and(|pending| !pending.is_empty());
            if waiting {
                self.announce_media_info_missing(&error.to_string());
            }
            return MediaInfoStatus::Missing;
        }
        // A new request (a scan, a start, Read now) tries again what failed earlier in this
        // session; what stays unreadable is skipped by its mark in the library.
        self.session.forget_all();
        {
            let mut sweep = locked(&self.sweep);
            sweep.dirty = true;
            if sweep.running {
                return MediaInfoStatus::Ready;
            }
            sweep.running = true;
        }
        if let Some(service) = self.me.upgrade() {
            cv_jobs::spawn(async move { service.sweep_media_info().await });
        }
        MediaInfoStatus::Ready
    }

    /// Reload media info: forgets what is stored about the videos of these titles (a show: of
    /// its episodes), clears their "unreadable" marks, and reads them again from the files
    /// themselves, never from a sidecar. Waits for the job, which the Activity drawer shows as
    /// "Reload media info".
    ///
    /// Nothing is cleared when MediaInfo is not installed, so a title never loses its details to
    /// a reload that cannot read them back.
    ///
    /// # Errors
    /// [`ReloadError::NoMediaInfo`] without MediaInfo; [`ReloadError::Library`] when the library
    /// cannot be changed.
    pub async fn reload_media_info(&self, ids: &[ItemId]) -> Result<MediaInfoReload, ReloadError> {
        let reader = self
            .open_reader()
            .map_err(|error| ReloadError::NoMediaInfo(error.to_string()))?;
        let (db, session, wanted) = (
            Arc::clone(&self.db),
            Arc::clone(&self.session),
            ids.to_vec(),
        );
        let targets = tokio::task::spawn_blocking(move || reload_titles(&db, &session, &wanted))
            .await
            .map_err(|error| ReloadError::Library(error.to_string()))?
            .map_err(|error| ReloadError::Library(error.to_string()))?;
        let mut report = MediaInfoReload {
            titles: targets.len(),
            ..MediaInfoReload::default()
        };
        let work = MediaInfoWork::new(
            Arc::clone(&self.db),
            reader,
            Arc::clone(&self.tools),
            MediaInfoSettings::read(&self.settings),
            Arc::clone(&self.notices),
            Arc::clone(&self.session),
        )
        .with_changes(self.changes.clone())
        .forced();
        let labels: Vec<String> = targets.iter().map(|t| t.title.clone()).collect();
        let Some(job) = submit_named(&self.jobs, true, work, targets, media_parallelism()) else {
            return Ok(report);
        };
        let done = job.job.wait().await;
        report.read = job.stats().read;
        self.media_info_stored(job.stored());
        for record in &done.items {
            if let cv_jobs::ItemStatus::Failed { reason } = &record.status {
                let title = labels.get(record.index).cloned().unwrap_or_default();
                report.failed.push((title, reason.clone()));
            }
        }
        Ok(report)
    }

    /// Whether media info can be read now, without starting anything.
    #[must_use]
    pub fn media_info_status(&self) -> MediaInfoStatus {
        if self.open_reader().is_ok() {
            MediaInfoStatus::Ready
        } else {
            MediaInfoStatus::Missing
        }
    }

    /// Completes when no rescan is waiting to hand over to media info and no media-info pass is
    /// running or waiting. For tests and shutdown.
    pub async fn media_info_idle(&self) {
        loop {
            {
                let sweep = locked(&self.sweep);
                if !sweep.running && !sweep.dirty && self.follow_ups.load(Ordering::SeqCst) == 0 {
                    return;
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    fn open_reader(&self) -> Result<Arc<dyn MediaReader>, cv_media::mediainfo::MediaInfoError> {
        let source = Arc::clone(&locked(&self.reader));
        source()
    }

    fn announce_media_info_missing(&self, reason: &str) {
        let mut announced = locked(&self.missing_tool_announced);
        if std::mem::replace(&mut *announced, true) {
            return;
        }
        tracing::info!(
            reason,
            "MediaInfo is not available; media info stays pending"
        );
        self.notices.publish(
            AppNotice::new(NoticeLevel::Info, "Media details are waiting for MediaInfo")
                .detail(
                    "Your titles are in the library, but their video and audio details stay \
                     pending until MediaInfo is installed. Download it or set its path under \
                     Settings, External tools.",
                )
                .settings_link(TOOLS_PAGE)
                .source("Media info"),
        );
    }

    async fn sweep_media_info(self: Arc<Self>) {
        loop {
            {
                let mut sweep = locked(&self.sweep);
                if !sweep.dirty {
                    sweep.running = false;
                    return;
                }
                sweep.dirty = false;
            }
            let Ok(reader) = self.open_reader() else {
                // The tool went away between the request and now; the titles stay pending.
                continue;
            };
            let db = Arc::clone(&self.db);
            let pending =
                tokio::task::spawn_blocking(move || db.read(|c| pending_media_info(c, usize::MAX)))
                    .await;
            let targets: Vec<_> = match pending {
                Ok(Ok(list)) => list
                    .into_iter()
                    .filter(|p| !self.session.has_failed(p.item_id.0))
                    .map(Into::into)
                    .collect(),
                Ok(Err(error)) => {
                    tracing::warn!(%error, "cannot list titles that wait for media info");
                    continue;
                }
                Err(error) => {
                    tracing::warn!(%error, "listing pending media info stopped");
                    continue;
                }
            };
            let work = MediaInfoWork::new(
                Arc::clone(&self.db),
                reader,
                Arc::clone(&self.tools),
                MediaInfoSettings::read(&self.settings),
                Arc::clone(&self.notices),
                Arc::clone(&self.session),
            )
            .with_changes(self.changes.clone());
            let Some(job) = submit_media_info(&self.jobs, work, targets, media_parallelism())
            else {
                continue;
            };
            job.job.wait().await;
            let stats = job.stats();
            tracing::info!(?stats, "media info pass finished");
            self.media_info_stored(job.stored());
            if stats.iso_skipped > 0 {
                self.notices.publish(
                    AppNotice::new(
                        NoticeLevel::Info,
                        "Aspect ratio detection skipped some ISO images",
                    )
                    .detail(format!(
                        "ISO images are not supported for detection, so {} kept the ratio \
                         MediaInfo reported.",
                        if stats.iso_skipped == 1 {
                            "one image".to_owned()
                        } else {
                            format!("{} images", stats.iso_skipped)
                        }
                    ))
                    .source("Aspect ratio"),
                );
            }
        }
    }

    // ---- Find missing and source health ------------------------------------------------------

    /// Lists the videos of every data source that belong to no title. Reads the disk: call it
    /// off the UI thread.
    ///
    /// # Errors
    /// The library database cannot be read.
    pub fn find_missing(&self) -> Result<Vec<UnmanagedReport>, DbError> {
        let options = self.rescan_options();
        find_unmanaged_all(&self.db, &options.movies, &options.tv)
    }

    /// Every data source with whether its folder answers and what it holds. A folder that does
    /// not answer within a few seconds counts as offline. Reads the disk: call it off the UI
    /// thread.
    ///
    /// # Errors
    /// The library database cannot be read.
    pub fn source_health(&self) -> Result<Vec<SourceHealth>, DbError> {
        let sources = self.db.read(|conn| list_sources(conn, None))?;
        // Every folder is asked at once, so six folders wait for one timeout, not six.
        let answers = probe_all(
            sources.iter().map(|source| source.path.as_str()),
            PROBE_TIMEOUT,
        );
        let mut memory = locked(&self.probes);
        let health: Vec<SourceHealth> = sources
            .into_iter()
            .zip(answers)
            .map(|(source, answer)| {
                let counts = self.db.read(|conn| source_counts(conn, &source.path))?;
                let seen = memory.entry(source.path.clone()).or_default();
                let state = seen.state(answer, &counts);
                Ok(SourceHealth {
                    source,
                    state,
                    counts,
                })
            })
            .collect::<Result<_, DbError>>()?;
        drop(memory);
        self.stamp_seen(&health);
        Ok(health)
    }

    /// Remembers when each library folder that answers last did, for the Drives page's "last
    /// seen". A folder seen in the last minute is not written again.
    fn stamp_seen(&self, health: &[SourceHealth]) {
        let answering: Vec<i64> = health
            .iter()
            .filter(|h| h.state == SourceState::Online)
            .map(|h| h.source.id)
            .collect();
        if answering.is_empty() {
            return;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
        if let Err(error) = self.db.write(move |tx| {
            cv_db::repo::drive_facts::mark_sources_seen(tx, &answering, now, 60_000)
        }) {
            tracing::debug!(%error, "could not record when the library folders last answered");
        }
    }
}

/// The Needs attention item for a library folder a scan skipped. The title names the folder as
/// the sidebar does (its label), so a long path never fills the card; the whole path is in the
/// detail, for the reader who needs to find it.
/// The notice topic of a library folder a scan skipped: one card per folder, taken back once a
/// scan reads it again.
fn skip_topic(path: &str) -> String {
    format!("source-skipped:{path}")
}

fn skip_notice(source: &DataSource, skipped: Skipped) -> AppNotice {
    let path = &source.path;
    let name = if source.label.trim().is_empty() {
        path.as_str()
    } else {
        source.label.as_str()
    };
    let (title, detail) = match skipped {
        Skipped::SourceUnavailable => (
            format!("{name} could not be read"),
            format!(
                "Its titles stay in the library, marked offline, until the folder is back. The \
                 folder is {path}."
            ),
        ),
        Skipped::SourceEmpty => (
            format!("{name} is empty"),
            format!(
                "This looks like a disconnected drive, so its titles were left as they are. The \
                 folder is {path}."
            ),
        ),
    };
    AppNotice::new(NoticeLevel::Warning, title)
        .detail(detail)
        .source("Library scan")
}

/// What one folder's probes said lately.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct ProbeMemory {
    /// The last answer the folder gave in time.
    last: Option<bool>,
    /// Probes in a row that got no answer in time.
    silent: u32,
}

impl ProbeMemory {
    /// The folder's state from this probe's `answer` (`None`: no answer in time). A folder that
    /// answers is online or offline as it says. One that is silent once keeps its last answer, or,
    /// before it ever answered (just after a start, with the disk busy), what the library last
    /// knew of its titles; silent twice in a row, it is offline, as a dead share is.
    fn state(&mut self, answer: Option<bool>, counts: &SourceCounts) -> SourceState {
        let online = match answer {
            Some(online) => {
                self.last = Some(online);
                self.silent = 0;
                online
            }
            None => {
                self.silent += 1;
                if self.silent > 1 {
                    false
                } else {
                    self.last
                        .unwrap_or(counts.titles == 0 || counts.offline < counts.titles)
                }
            }
        };
        if online {
            SourceState::Online
        } else {
            SourceState::Offline
        }
    }
}

/// Whether the library folder at `path` is there: a folder, and on Linux one whose drive is
/// really mounted.
fn folder_answers(path: &Path) -> bool {
    answers(path, root_is_mounted(path))
}

/// `path` is a folder and its drive is mounted (`root_is_mounted`). An unplugged Linux drive
/// leaves its empty mount-point folder behind on the system drive; that folder is a folder, but
/// it must not pass for the library, so the drive counts as offline until it is mounted again.
fn answers(path: &Path, root_is_mounted: bool) -> bool {
    root_is_mounted && path.is_dir()
}

/// Whether the mount point that holds `path` (found by the path alone: `/media/<user>/<name>`,
/// `/run/media/<user>/<name>` or `/mnt/<name>`) has a file system mounted on it. A folder
/// anywhere else is on the system drive, which is always there.
#[cfg(target_os = "linux")]
fn root_is_mounted(path: &Path) -> bool {
    let root = cv_fs::paths::linux_mount_root(&path.to_string_lossy());
    root == "/" || cv_fs::paths::is_mount_point(Path::new(&root))
}

#[cfg(not(target_os = "linux"))]
fn root_is_mounted(_path: &Path) -> bool {
    true
}

/// Whether each of `paths` is a folder, asked all at once: `Some(answer)`, or `None` when it did
/// not answer within `timeout`. A dead network share can block a file-system call for a long
/// time, so each probe runs on its own thread and is left behind when it is late.
fn probe_all<'a>(paths: impl Iterator<Item = &'a str>, timeout: Duration) -> Vec<Option<bool>> {
    let receivers: Vec<Option<mpsc::Receiver<bool>>> = paths
        .map(|path| {
            let (sender, receiver) = mpsc::channel();
            let owned = path.to_owned();
            std::thread::Builder::new()
                .name("cv-source-probe".to_owned())
                .spawn(move || {
                    let _ = sender.send(folder_answers(Path::new(&owned)));
                })
                .ok()
                .map(|_| receiver)
        })
        .collect();
    let deadline = std::time::Instant::now() + timeout;
    receivers
        .into_iter()
        .map(|receiver| {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            receiver.and_then(|receiver| receiver.recv_timeout(left).ok())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn counts(titles: u64, offline: u64) -> SourceCounts {
        SourceCounts {
            titles,
            offline,
            ..SourceCounts::default()
        }
    }

    #[test]
    fn a_folder_that_is_late_once_keeps_its_state_and_goes_offline_when_it_stays_silent() {
        let mut memory = ProbeMemory::default();
        // Just after a start, with the disk busy: no answer yet, and the library last saw the
        // titles online, so the folder is not shown offline.
        assert_eq!(memory.state(None, &counts(28, 0)), SourceState::Online);
        assert_eq!(
            memory.state(Some(true), &counts(28, 0)),
            SourceState::Online
        );
        // One late answer keeps the last one.
        assert_eq!(memory.state(None, &counts(28, 0)), SourceState::Online);
        // Twice in a row is a folder that does not answer.
        assert_eq!(memory.state(None, &counts(28, 0)), SourceState::Offline);
        assert_eq!(
            memory.state(Some(true), &counts(28, 0)),
            SourceState::Online
        );
        // A folder that answers "not there" is offline at once.
        assert_eq!(
            memory.state(Some(false), &counts(28, 0)),
            SourceState::Offline
        );
    }

    #[test]
    fn a_silent_folder_the_last_scan_found_offline_starts_offline() {
        let mut memory = ProbeMemory::default();
        assert_eq!(memory.state(None, &counts(5, 5)), SourceState::Offline);
    }

    #[test]
    fn a_skipped_folder_is_named_by_its_label_with_the_path_in_the_detail() {
        let source = |label: &str, path: &str| DataSource {
            id: 1,
            kind: SourceKind::Tv,
            path: path.to_owned(),
            label: label.to_owned(),
            position: 0,
            created_ms: 0,
        };
        let notice = skip_notice(
            &source("Shows", r"C:\Users\Alex\media\Mixed2\Shows"),
            Skipped::SourceUnavailable,
        );
        assert_eq!(notice.title, "Shows could not be read");
        assert!(
            notice
                .detail
                .unwrap_or_default()
                .contains(r"The folder is C:\Users\Alex\media\Mixed2\Shows.")
        );
        assert_eq!(
            skip_notice(&source("", "/m"), Skipped::SourceEmpty).title,
            "/m is empty"
        );
    }

    #[test]
    fn every_folder_is_probed_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let here = dir.path().to_string_lossy().into_owned();
        let gone = dir.path().join("gone").to_string_lossy().into_owned();
        let answers = probe_all([here.as_str(), gone.as_str()].into_iter(), PROBE_TIMEOUT);
        assert_eq!(answers, [Some(true), Some(false)]);
    }

    #[test]
    fn a_folder_on_a_drive_that_is_not_mounted_does_not_answer() {
        let dir = tempfile::tempdir().unwrap();
        assert!(answers(dir.path(), true));
        // The empty folder an unplugged drive leaves behind.
        assert!(!answers(dir.path(), false));
        // A mounted drive without the folder, or with a file in its place, does not answer either.
        assert!(!answers(&dir.path().join("gone"), true));
        let file = dir.path().join("file");
        std::fs::write(&file, b"x").unwrap();
        assert!(!answers(&file, true));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_mount_shaped_path_with_nothing_mounted_is_offline_on_linux() {
        let dir = tempfile::tempdir().unwrap();
        // A folder on the system drive is online.
        assert!(folder_answers(dir.path()));
        // The root of a temp dir is `/`, so it is always mounted; a made-up drive is not.
        assert!(root_is_mounted(dir.path()));
        assert!(!root_is_mounted(Path::new(
            "/mnt/cinevault-test-drive-that-is-not-there/Movies"
        )));
        assert!(!folder_answers(Path::new(
            "/mnt/cinevault-test-drive-that-is-not-there/Movies"
        )));
    }

    #[test]
    fn a_fresh_install_uses_the_built_in_skip_list_and_honours_nomedia() {
        let options = rescan_options_from(None, None, None, None);
        assert!(
            options
                .movies
                .skip
                .is_skipped_folder("MAKEMKV", "/m/MAKEMKV")
        );
        assert!(options.tv.skip.is_skipped_folder("MAKEMKV", "/t/MAKEMKV"));
        assert!(options.movies.skip.skips_nomedia());
        assert!(options.movies.bad_words.is_empty());
        assert!(!options.movies.extract_vsmeta_artwork);
    }

    #[test]
    fn the_settings_replace_the_skip_list_the_nomedia_switch_and_the_bad_words() {
        let patterns = json!(" Samples ; ^Extras[0-9]+$ ;;");
        let off = json!(false);
        let on = json!(true);
        let words = json!("1080P, BluRay ,,");
        let options = rescan_options_from(Some(&patterns), Some(&off), Some(&words), Some(&on));
        let skip = &options.movies.skip;
        assert!(skip.is_skipped_folder("Samples", "/m/Samples"));
        assert!(skip.is_skipped_folder("Extras12", "/m/Extras12"));
        assert!(
            !skip.is_skipped_folder("MAKEMKV", "/m/MAKEMKV"),
            "the list is replaced"
        );
        assert!(!skip.skips_nomedia());
        assert_eq!(options.movies.bad_words, ["1080p", "bluray"]);
        assert_eq!(options.tv.bad_words, options.movies.bad_words);
        assert!(options.movies.extract_vsmeta_artwork && options.tv.extract_vsmeta_artwork);
    }

    #[test]
    fn an_empty_skip_list_skips_nothing_of_the_users_and_a_mistyped_value_is_ignored() {
        let empty = json!("");
        let options = rescan_options_from(Some(&empty), None, None, None);
        assert!(
            !options
                .movies
                .skip
                .is_skipped_folder("MAKEMKV", "/m/MAKEMKV")
        );

        let wrong = json!(7);
        let options = rescan_options_from(Some(&wrong), Some(&wrong), Some(&wrong), Some(&wrong));
        assert!(
            options
                .movies
                .skip
                .is_skipped_folder("MAKEMKV", "/m/MAKEMKV")
        );
        assert!(options.movies.skip.skips_nomedia());
        assert!(options.movies.bad_words.is_empty());
    }
}
