//! The composition root: every long-lived service of the app, built once.
//!
//! [`AppServices::boot`] opens the one shared library [`Database`], loads the settings (a damaged
//! file is set aside and its notice goes to the notification centre), opens the thumbnail cache
//! in the data location's cache folder, and builds the [`JobManager`] over a [`JobStore`] in the
//! same library. [`AppServices::start`] then begins the background work: the job failure watcher
//! and the resume of every persisted job kind. [`AppServices::shutdown`] pauses the jobs so the
//! next launch picks them up.
//!
//! The GUI and the CLI both borrow from this struct instead of opening their own copies.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use cv_artwork::thumb::{DEFAULT_MAX_BYTES, ThumbCache};
use cv_artwork::thumb_job::resume_thumb_jobs;
use cv_db::Database;
use cv_jobs::{JobManager, JobStore, LaneConfig, Lanes, ManagerConfig};
use cv_providers::{CredentialResolver, MemoryUserCredentials, ProviderRegistry};
use tokio::task::JoinHandle;

use crate::actions::{ActionsService, RatingSources};
use crate::activity::ActivityService;
use crate::app_settings::SettingsService;
use crate::artwork::ArtworkService;
use crate::caches::CacheService;
use crate::changes::{ChangeHub, ChangePart};
use crate::data_location::DataLocation;
use crate::lifecycle::{LifecycleService, Working};
use crate::local_library::submit_local_import;
use crate::match_service::MatchService;
use crate::matching::ScrapedHook;
use crate::matching::batch::MatchContext;
use crate::nfo::NfoService;
use crate::notices::{AppNotice, NoticeHub, NoticeLevel};
use crate::organize::OrganizeStateService;
use crate::provider_credentials::ProviderCredentialsService;
use crate::provider_setup::{HTTP_CACHE_DIR, build_service_with_kodi, kodi_roots};
use crate::rating_status::RatingStatusService;
use crate::scanning::ScanService;
use crate::scrape_follow_up::ScrapeFollowUp;
use crate::secrets::SecretStore;
use crate::settings::SettingsStore;
use crate::tools::ToolsService;
use crate::torrents::TorrentService;
use crate::torrents::indexer::{IndexerParts, IndexerServiceManager};

/// Why the services could not start.
#[derive(Debug, thiserror::Error)]
pub enum BootError {
    #[error("CineVault could not prepare its data folder {path}: {source}")]
    DataFolder {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("CineVault could not open the library: {0}")]
    Library(#[from] cv_db::DbError),
    #[error("{0}")]
    FreshStart(#[from] crate::fresh_start::FreshStartError),
    #[error("CineVault could not open its image cache: {0}")]
    Cache(cv_artwork::thumb::ThumbError),
}

/// What the services are built from.
pub struct BootOptions {
    /// Where the data lives.
    pub location: DataLocation,
    /// The OS credential store (or an in-memory one in tests).
    pub secrets: Arc<SecretStore>,
    /// Widths of the job lanes.
    pub lanes: LaneConfig,
    /// Replaces the providers of this build. For tests, which register mock providers.
    pub providers: Option<ProviderCredentialsService>,
    /// How long a burst of changes is collected before open pages are told (see
    /// [`crate::changes`]).
    pub change_window: Duration,
    /// Whether this process may make the fresh start of this version (set the library and the
    /// settings of an earlier version aside). Only the app does; the command line refuses a
    /// data folder that still needs it.
    pub fresh_start: bool,
    /// Whether the NFOs of changed titles are written again in the background (the follower and
    /// the write after media info is read). On unless a test needs the files to stay as it left
    /// them.
    pub background_nfo: bool,
}

impl BootOptions {
    pub fn new(location: DataLocation, secrets: Arc<SecretStore>) -> Self {
        Self {
            location,
            secrets,
            lanes: LaneConfig::default(),
            providers: None,
            change_window: crate::changes::DEFAULT_WINDOW,
            fresh_start: true,
            background_nfo: true,
        }
    }

    /// Never makes the fresh start: a data folder whose library or settings come from an earlier
    /// version is refused with [`crate::fresh_start::FreshStartError::NeedsApp`]. The command
    /// line boots this way.
    #[must_use]
    pub fn without_fresh_start(mut self) -> Self {
        self.fresh_start = false;
        self
    }

    /// Writes no NFO behind the caller's back: nothing rewrites a title's NFO after a change or
    /// after media info was read, only the actions that ask for it. For tests that check what a
    /// step writes.
    #[must_use]
    pub fn without_background_nfo(mut self) -> Self {
        self.background_nfo = false;
        self
    }

    /// Uses `providers` instead of the providers of this build.
    #[must_use]
    pub fn with_providers(mut self, providers: ProviderCredentialsService) -> Self {
        self.providers = Some(providers);
        self
    }

    /// Registers no provider at all, so nothing is ever sent to the network. For tests that scan
    /// real folders: the titles a scan finds would otherwise be matched online.
    #[must_use]
    pub fn without_providers(self) -> Self {
        let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
        self.with_providers(ProviderCredentialsService::with_resolver(
            resolver,
            ProviderRegistry::new(),
        ))
    }
}

/// What [`AppServices::start`] picked up from the previous session.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResumeReport {
    /// Rescans started again.
    pub rescans: usize,
    /// Thumbnail builds started again.
    pub thumbnails: usize,
    /// Match batches started again.
    pub matches: usize,
    /// Stored match batches were left alone because no metadata provider is registered.
    pub matches_deferred: bool,
}

impl ResumeReport {
    /// Jobs started again, of every kind.
    pub fn total(&self) -> usize {
        self.rescans + self.thumbnails + self.matches
    }
}

/// The app's services. Cheap parts are `Arc`s, so state objects can clone what they need.
pub struct AppServices {
    location: DataLocation,
    db: Arc<Database>,
    settings: Arc<SettingsService>,
    notices: Arc<NoticeHub>,
    thumbs: Arc<ThumbCache>,
    caches: Arc<CacheService>,
    jobs: JobManager,
    providers: Arc<ProviderCredentialsService>,
    tools: Arc<ToolsService>,
    scan: Arc<ScanService>,
    matching: Arc<MatchService>,
    artwork: Arc<ArtworkService>,
    actions: Arc<ActionsService>,
    organize_state: Arc<OrganizeStateService>,
    scrape_follow_up: Arc<ScrapeFollowUp>,
    lifecycle: Arc<LifecycleService>,
    rating_status: Arc<RatingStatusService>,
    changes: Arc<ChangeHub>,
    activity: Arc<ActivityService>,
    torrents: Arc<TorrentService>,
    indexer: Arc<IndexerServiceManager>,
    tasks: Mutex<Vec<JoinHandle<()>>>,
}

impl AppServices {
    /// Builds every service. Nothing runs in the background yet; see [`Self::start`].
    ///
    /// # Errors
    /// Fails when the data folder cannot be created or the library cannot be opened.
    pub fn boot(options: BootOptions) -> Result<Arc<Self>, BootError> {
        let BootOptions {
            location,
            secrets,
            lanes,
            providers,
            change_window,
            fresh_start,
            background_nfo,
        } = options;
        // What changed for which titles: services announce it, the shell relays it to open pages.
        let changes = ChangeHub::new(change_window);
        location
            .ensure_dirs()
            .map_err(|source| BootError::DataFolder {
                path: location.root().to_path_buf(),
                source,
            })?;
        let notices = Arc::new(NoticeHub::new());
        if let Some(warning) = location.warning() {
            notices.publish(
                AppNotice::new(
                    NoticeLevel::Warning,
                    "The data folder choice could not be used",
                )
                .detail(warning)
                .source("Data folder"),
            );
        }

        // The fresh start of schema version 100: the library of an earlier build is set aside
        // before any SQLite connection is made, then the settings of an earlier version.
        if fresh_start {
            crate::fresh_start::run(&location, &secrets)?;
        } else {
            crate::fresh_start::refuse_pending(&location)?;
        }
        let db = Arc::new(Database::open(location.library_db())?);

        // A damaged settings file is set aside and defaults are used; say so once. An unreadable
        // one is left alone and set aside before the first save.
        let settings = Arc::new(SettingsService::open(SettingsStore::new(
            location.settings_file(),
        )));
        // The log detail the person chose applies from the first line the services write.
        crate::logging::apply_from(&settings);
        if let Some(message) = settings.snapshot().notice {
            notices.publish(
                AppNotice::new(NoticeLevel::Warning, "Your settings could not be loaded")
                    .detail(message)
                    .source("Settings"),
            );
        }

        let thumbs = Arc::new(open_thumb_cache(&location)?);
        let jobs = JobManager::with_store(
            Lanes::new(lanes),
            ManagerConfig::default(),
            JobStore::new(Arc::clone(&db)),
        );
        let providers = Arc::new(providers.unwrap_or_else(|| {
            build_service_with_kodi(
                Arc::clone(&secrets),
                Some(location.cache_dir().join(HTTP_CACHE_DIR)),
                &kodi_roots(location.root()),
            )
        }));

        // The caches and the Activity lanes share the providers' HTTP client (limits, cache,
        // redaction) when there is one.
        let http = providers
            .http()
            .cloned()
            .or_else(|| crate::provider_setup::http_client(None).ok());
        let caches = Arc::new(
            CacheService::new(Arc::clone(&thumbs), http.clone(), Arc::clone(&settings))
                .with_frames_dir(location.cache_dir().join("frames")),
        );
        // Trimming the image cache and ageing out grabbed frames walk their folders (tens of
        // thousands of files in a large library): off the start-up path.
        {
            let caches = Arc::clone(&caches);
            let _ = std::thread::Builder::new()
                .name("image-cache-trim".into())
                .spawn(move || caches.apply_settings());
        }
        let activity = Arc::new(ActivityService::new(
            jobs.clone(),
            http,
            providers.resolver().clone(),
        ));

        let tools = Arc::new(ToolsService::open(location.tools_dir(), &settings));
        let scan = ScanService::new(
            Arc::clone(&db),
            jobs.clone(),
            Arc::clone(&thumbs),
            Arc::clone(&settings),
            Arc::clone(&notices),
            tools.manager(),
        );
        let matching = MatchService::new(
            Arc::clone(&db),
            jobs.clone(),
            Arc::clone(&providers),
            Arc::clone(&settings),
            Arc::clone(&notices),
        );
        // A different match sends the title's old NFO and artwork files to the Recycle Bin /
        // Trash, so History can bring them back.
        matching.set_bin(crate::library_ops::PolicyBin::os(
            Arc::clone(&settings) as Arc<dyn crate::deleted_files::DeletedFilesSource>
        ));
        // The anime ID cross-reference: downloaded on the first anime scrape, kept in the cache
        // folder and refreshed weekly.
        match crate::anime_crosswalk::AnimeCrosswalk::in_dir(
            location.cache_dir().join(crate::anime_crosswalk::CACHE_DIR),
        ) {
            Ok(anime) => matching.set_anime(anime),
            Err(error) => tracing::warn!(%error, "the anime ID database is not available"),
        }
        // Every scrape (single, accepted candidate or batch) announces what it changed.
        {
            let changes = Arc::clone(&changes);
            matching.set_after_scrape(ScrapedHook(Arc::new(move |item| {
                // A scrape wrote the fields, the ratings the provider gives and the stage.
                changes.publish(
                    &[item],
                    &[ChangePart::Details, ChangePart::Ratings, ChangePart::Stage],
                );
            })));
        }
        // Once a title is matched and scraped, its empty artwork types are filled (when the
        // setting is on).
        let artwork = ArtworkService::new(
            Arc::clone(&db),
            Arc::clone(&providers),
            Arc::clone(&settings),
            Arc::clone(&thumbs),
        );
        // The artwork picker offers frames from the video next to the online images.
        artwork.set_frame_source(
            Arc::new(crate::artwork::frames::FfmpegFrames::new(tools.manager())),
            location.cache_dir().join("frames"),
        );
        // The automation actions (ratings, aspect ratio, checksums,
        // artwork clean-up) and the after-scrape steps that run them.
        let actions = ActionsService::new(
            Arc::clone(&db),
            Arc::clone(&settings),
            tools.manager(),
            Arc::clone(&notices),
            Arc::clone(&artwork),
            RatingSources::for_app(
                providers.http(),
                providers.resolver(),
                &location.cache_dir(),
            ),
        );
        // Which titles already have the names of the naming profile is worked out in the
        // background (a dry run of the renamer) and kept after a scrape, a rename, a scan and a
        // change of profile.
        let organize_state =
            OrganizeStateService::new(Arc::clone(&db), Arc::clone(&settings), jobs.clone());
        actions.set_changes(Arc::clone(&changes));
        // Torrent search: title lookup and the source list. Nothing is contacted until the
        // responsibility notice was accepted.
        let torrents = Arc::new(TorrentService::new(
            Arc::clone(&settings),
            Arc::clone(&secrets),
            Arc::clone(&db),
            Arc::clone(&providers),
        ));
        // The managed indexer service: downloaded, started and stopped as the switches ask, and
        // never before the Torrent search notice was accepted.
        let indexer = IndexerServiceManager::new(IndexerParts {
            tools: Arc::clone(&tools),
            settings: Arc::clone(&settings),
            secrets: Arc::clone(&secrets),
            db: Arc::clone(&db),
            jobs: jobs.clone(),
            torrents: Arc::clone(&torrents),
            notices: Arc::clone(&notices),
            data_dir: location.indexer_service_dir(),
        });
        {
            let manager = Arc::clone(&indexer);
            settings.on_change(Arc::new(move |keys| {
                if keys.iter().any(|key| {
                    key == crate::torrent_settings::INDEXER_ENABLED_KEY
                        || key == crate::torrent_settings::SOLVER_URL_KEY
                        || key == crate::torrent_settings::NOTICE_ACCEPTED_KEY
                }) {
                    manager.request_apply();
                }
            }));
        }
        artwork.set_changes(Arc::clone(&changes));
        scan.set_changes(Arc::clone(&changes));
        organize_state.set_changes(Arc::clone(&changes));
        // The stage and flags of every title are derived from the library, the step outcomes, the
        // cached organize state and the titles a job is touching now.
        let working = Arc::new(Working::default());
        actions.set_working(Arc::clone(&working));
        let lifecycle = LifecycleService::new(
            Arc::clone(&db),
            Arc::clone(&settings),
            Arc::clone(&organize_state),
            working,
        );
        // The chips of the ratings strip: a value, or why there is none.
        let rating_status = RatingStatusService::new(
            Arc::clone(&db),
            Arc::clone(&settings),
            Arc::new(providers.resolver().clone()),
            Arc::clone(lifecycle.working()),
        );
        {
            let state = Arc::clone(&organize_state);
            settings.on_change(Arc::new(move |_| state.settings_changed()));
        }
        // The NFO service shared by the scrape follow-up and the actions.
        let nfo = Arc::new(NfoService::new(Arc::clone(&db), location.nfo_undo_dir()));
        // Old undo records are pruned off the start-up path.
        {
            let nfo = Arc::clone(&nfo);
            let _ = std::thread::Builder::new()
                .name("nfo-undo-prune".into())
                .spawn(move || {
                    nfo.prune_undo_records();
                });
        }
        // Every change of a scraped title's data is written to its NFO, whatever made it.
        if background_nfo {
            crate::nfo::follow::NfoFollower::start(
                Arc::clone(&db),
                Arc::clone(&nfo),
                Arc::clone(&settings),
                &changes,
            );
        }
        // Stored ratings are written to the NFO as well.
        actions.set_nfo(Arc::clone(&nfo));
        // A chosen image's address goes into the NFO in place of the old one.
        artwork.set_nfo(Arc::clone(&nfo));
        if background_nfo {
            // Media info that was read is written to the NFO of a title CineVault scraped (so
            // the NFO is its own), so its <fileinfo> matches the file. An NFO that
            // only came with the folder is left as the user had it.
            let nfo = Arc::clone(&nfo);
            let db = Arc::clone(&db);
            let settings = Arc::clone(&settings);
            scan.set_after_media_info(Arc::new(move |items| {
                let nfo_settings =
                    crate::nfo::settings::NfoSettings::from_values(&settings.snapshot().values);
                let Ok(runtime) = tokio::runtime::Handle::try_current() else {
                    return;
                };
                let nfo = Arc::clone(&nfo);
                let db = Arc::clone(&db);
                drop(runtime.spawn_blocking(move || {
                    let with_nfo = db
                        .read(|c| {
                            let scraped = cv_db::repo::lifecycle::scraped_items(c, &items)?;
                            let mut out = Vec::new();
                            for id in scraped {
                                if cv_db::repo::get_item(c, id)?.is_some_and(|item| {
                                    !item.core.last_scraper_id.is_empty()
                                        && item
                                            .core
                                            .media_files
                                            .iter()
                                            .any(|f| f.kind == cv_domain::MediaFileType::Nfo)
                                }) {
                                    out.push(id);
                                }
                            }
                            Ok(out)
                        })
                        .unwrap_or_default();
                    if with_nfo.is_empty() {
                        return;
                    }
                    if let Err(error) =
                        nfo.rewrite(&crate::nfo::NfoScope::Items(with_nfo), &nfo_settings)
                    {
                        tracing::warn!(%error, "the NFO was not written after reading media info");
                    }
                }));
            }));
        }
        // Whichever provider scraped the title: the NFO is written, then the empty artwork
        // types are filled by the IDs the title has now.
        let scrape_follow_up = ScrapeFollowUp::new(
            Arc::clone(&nfo),
            Arc::clone(&artwork),
            Arc::clone(&settings),
        );
        scrape_follow_up.set_changes(Arc::clone(&changes));
        scrape_follow_up.set_notices(Arc::clone(&notices));
        {
            // An undone scrape writes the NFO of the title the library holds again.
            let follow_up = Arc::clone(&scrape_follow_up);
            matching.set_after_undo(Arc::new(move |item| follow_up.after_undo(item)));
        }
        // The artwork step of the scraped titles is one named job per scrape in Activity.
        scrape_follow_up.set_jobs(jobs.clone());
        {
            let follow_up = Arc::clone(&scrape_follow_up);
            let actions = Arc::clone(&actions);
            let state = Arc::clone(&organize_state);
            matching.set_after_match(Arc::new(move |item| {
                let download = follow_up.after_match_task(item);
                // The NFO and the artwork the follow-up writes are files the name check looks
                // at: it runs again once they are there.
                let again = Arc::clone(&state);
                let download = cv_jobs::spawn(async move {
                    let _ = download.await;
                    again.note_changed(&[item]);
                });
                actions.after_match_following(item, Some(download));
                state.note_changed(&[item]);
            }));
        }
        // The NFO files next to the titles a rescan found are read, so a folder that already
        // holds Kodi, Jellyfin or Emby files shows its real titles at once (the instant library).
        // Nothing is scraped and nothing is renamed: a title stays New until the user scrapes it,
        // or Scraped when its NFO already carries a provider ID.
        {
            let nfo = Arc::clone(&nfo);
            let jobs = jobs.clone();
            let settings = Arc::clone(&settings);
            let state = Arc::clone(&organize_state);
            scan.set_after_scan(Arc::new(move |found| {
                // Titles the scan added get their name check, after their NFO files are read:
                // the IDs an NFO carries decide whether the title is planned at all.
                match submit_local_import(&jobs, Arc::clone(&nfo), Arc::clone(&settings), &found) {
                    Some(import) => {
                        // The import was submitted on this runtime, so it is there.
                        let state = Arc::clone(&state);
                        tokio::spawn(async move {
                            import.job.wait().await;
                            state.refresh_missing();
                        });
                    }
                    None => {
                        state.refresh_missing();
                    }
                }
            }));
        }

        Ok(Arc::new(Self {
            location,
            db,
            settings,
            notices,
            thumbs,
            caches,
            jobs,
            providers,
            tools,
            scan,
            matching,
            artwork,
            actions,
            organize_state,
            scrape_follow_up,
            lifecycle,
            rating_status,
            changes,
            activity,
            torrents,
            indexer,
            tasks: Mutex::new(Vec::new()),
        }))
    }

    pub fn location(&self) -> &DataLocation {
        &self.location
    }

    /// The one library database every service shares.
    pub fn db(&self) -> &Arc<Database> {
        &self.db
    }

    pub fn settings(&self) -> &Arc<SettingsService> {
        &self.settings
    }

    pub fn notices(&self) -> &Arc<NoticeHub> {
        &self.notices
    }

    /// The thumbnail cache, in the data location's cache folder.
    pub fn thumbs(&self) -> &Arc<ThumbCache> {
        &self.thumbs
    }

    /// Usage and cleanup of the thumbnail cache and the provider response cache.
    pub fn caches(&self) -> &Arc<CacheService> {
        &self.caches
    }

    /// The job manager, backed by the library's job store.
    pub fn jobs(&self) -> &JobManager {
        &self.jobs
    }

    pub fn providers(&self) -> &Arc<ProviderCredentialsService> {
        &self.providers
    }

    /// The external tools (MediaInfo, FFmpeg): where they are and how to install them.
    pub fn tools(&self) -> &Arc<ToolsService> {
        &self.tools
    }

    /// The scan pipeline: Rescan, the watcher, Find missing, media info and source health.
    pub fn scan(&self) -> &Arc<ScanService> {
        &self.scan
    }

    /// Matching: batches, the Needs review inbox and the search-and-scrape panel.
    pub fn matching(&self) -> &Arc<MatchService> {
        &self.matching
    }

    /// Artwork: Download missing artwork, Re-download, a chosen image, and the step after a scrape.
    pub fn artwork(&self) -> &Arc<ArtworkService> {
        &self.artwork
    }

    /// The automation actions and the after-scrape steps.
    pub fn actions(&self) -> &Arc<ActionsService> {
        &self.actions
    }

    /// Which titles already have the names of the naming profile, kept current in the
    /// background.
    pub fn organize_state(&self) -> &Arc<OrganizeStateService> {
        &self.organize_state
    }

    /// The NFO and artwork steps that follow every scrape.
    pub fn scrape_follow_up(&self) -> &Arc<ScrapeFollowUp> {
        &self.scrape_follow_up
    }

    /// What changed for which titles, announced to open pages. Subscribe to relay it.
    pub fn changes(&self) -> &Arc<ChangeHub> {
        &self.changes
    }

    /// The stage, flags and explanation lines of titles, and the pipeline counts.
    pub fn lifecycle(&self) -> &Arc<LifecycleService> {
        &self.lifecycle
    }

    /// The chips of the ratings strip.
    pub fn rating_status(&self) -> &Arc<RatingStatusService> {
        &self.rating_status
    }

    /// Torrent search: titles, seasons and episodes from TMDB, and the sources to search.
    pub fn torrents(&self) -> &Arc<TorrentService> {
        &self.torrents
    }

    /// The managed indexer service of Torrent search.
    pub fn indexer(&self) -> &Arc<IndexerServiceManager> {
        &self.indexer
    }

    /// What the Activity drawer shows besides job progress: lanes and failures.
    pub fn activity(&self) -> &Arc<ActivityService> {
        &self.activity
    }

    /// The context batch matching runs with, from the providers that are ready and the current
    /// settings.
    pub fn match_context(&self) -> MatchContext {
        self.matching.context()
    }

    /// Starts the background work and resumes the jobs of the previous session. Must be called
    /// inside a tokio runtime. Calling it twice repeats the resume (harmless) but starts the
    /// watchers again, so call it once.
    pub fn start(self: &Arc<Self>) -> ResumeReport {
        // The failure watcher goes first so a job that ends at once is still reported.
        let watcher = tokio::spawn(Arc::clone(&self.notices).watch_jobs(self.jobs.clone()));
        self.tasks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend([watcher]);
        // The indexer service starts in the background when the Torrent search notice was
        // accepted and the service is wanted; before that this does nothing at all.
        self.indexer.request_apply();
        let report = self.resume_jobs();
        // Titles with no name check for the active profile (a new install, an update, a profile
        // changed while the app was closed) get one.
        self.organize_state.refresh_missing();
        // The scan on startup, the watcher and the pending media info come after the resume, so
        // a scan the last session left unfinished is not started a second time.
        self.scan.startup(report.rescans);
        report
    }

    /// Starts again every persisted job kind. A kind that cannot be read is logged and reported
    /// as a notice; the others still resume.
    pub fn resume_jobs(&self) -> ResumeReport {
        let mut report = ResumeReport::default();

        match self.scan.resume() {
            Ok(count) => report.rescans = count,
            Err(error) => self.resume_failed("library scans", &error),
        }
        match resume_thumb_jobs(&self.jobs, &self.thumbs) {
            Ok(jobs) => report.thumbnails = jobs.len(),
            Err(error) => self.resume_failed("image cache builds", &error),
        }

        if self.matching.has_providers() {
            match self.matching.resume() {
                Ok(jobs) => report.matches = jobs.len(),
                Err(error) => self.resume_failed("matching", &error),
            }
        } else {
            // The stored batches stay as they are until a provider is ready (a key is added).
            tracing::info!("no metadata provider is ready; match batches stay stored");
            report.matches_deferred = true;
        }

        if report.total() > 0 {
            tracing::info!(?report, "resumed jobs from the previous session");
        }
        report
    }

    fn resume_failed(&self, what: &str, error: &dyn std::fmt::Display) {
        tracing::warn!(%error, "cannot resume {what}");
        self.notices.publish(
            AppNotice::new(
                NoticeLevel::Error,
                format!("Unfinished {what} could not be resumed"),
            )
            .detail(error.to_string())
            .source("Jobs"),
        );
    }

    /// Pauses every job, so the unfinished ones are picked up by the next launch, and stops the
    /// background tasks. Safe to call more than once.
    pub fn shutdown(&self) {
        self.indexer.shutdown();
        self.scan.shutdown();
        self.jobs.pause_all();
        for task in self
            .tasks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain(..)
        {
            task.abort();
        }
    }
}

/// Opens the thumbnail cache in the data location's cache folder. When that folder cannot be
/// used, a folder under the temp directory serves for this session.
fn open_thumb_cache(location: &DataLocation) -> Result<ThumbCache, BootError> {
    match ThumbCache::open(&location.cache_dir(), Some(DEFAULT_MAX_BYTES)) {
        Ok(cache) => Ok(cache),
        Err(error) => {
            tracing::warn!(%error, "cannot open the thumbnail cache; using a temporary one");
            ThumbCache::open(
                &std::env::temp_dir().join("cinevault-cache"),
                Some(DEFAULT_MAX_BYTES),
            )
            .map_err(BootError::Cache)
        }
    }
}
