//! The automation actions behind the batch engine and the after-scrape steps.
//!
//! One [`ActionsService`] holds what the actions need and runs each of them over a list of titles:
//!
//! | Action | Does | Needs |
//! |---|---|---|
//! | [`ActionsService::fetch_ratings`] | fetches and stores the ratings of the chosen sources | a key for the sources that want one |
//! | [`ActionsService::detect_aspect`] | measures the picture without its black bars | FFmpeg, the media info read |
//! | [`ActionsService::checksums`] | calculates CRC-32 and the perceptual hash | FFmpeg for the hash |
//! | [`ActionsService::download_artwork`] | fills the empty artwork types | an artwork provider |
//! | [`ActionsService::redownload_artwork`] | fetches artwork that is gone from disk again, from the sources it remembers | an artwork provider |
//! | [`ActionsService::cleanup_artwork`] | moves artwork the naming no longer uses to the trash | nothing |
//! | [`ActionsService::cleanup_unwanted`] | moves the profile's unwanted files (`.txt`, `.url` ...) to the trash, with a dry run | the clean-up switched on in the naming profile |
//!
//! Every action answers with an [`ActionRun`]: one [`ItemOutcome`] per title, plus a [`Blocker`]
//! when the whole action cannot run (a tool is not installed, a key or an account is missing, no
//! program is set). A title that fails does not stop the others. Nothing is ever deleted: files
//! an action replaces or no longer needs go to the trash folder of the data source, and an
//! existing artwork file is never overwritten.
//!
//! The batch engine (`automation::engine`) turns an [`ActionRun`] into a step report. The
//! scrape pipeline calls [`ActionsService::after_match`] for every title a scrape was written to;
//! that runs the after-scrape steps the settings switch on (see [`AfterScrape`]).

mod after_scrape;
mod cleanup;
mod downloads;
mod media;
mod ratings;
mod settings;

use std::collections::HashSet;
use std::sync::{Arc, Mutex, PoisonError};

use cv_db::Database;
use cv_db::repo::get_item;
use cv_domain::{Item, ItemId};
use cv_media::tools::ToolsManager;
use tokio::sync::Semaphore;

use crate::app_settings::SettingsService;
use crate::artwork::ArtworkService;
use crate::changes::{ChangeHub, ChangePart, Changes};
use crate::lifecycle::{RatingsGuard, Working, WorkingGuard};
use crate::notices::NoticeHub;
use crate::step_outcomes::StepStatus;

pub use ratings::{DatasetRetry, RatingSources};
pub use settings::{AfterScrape, CHECKSUMS_AFTER_SCRAPE_KEY};

/// How an action ended for one title.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutcomeKind {
    /// The action changed something for the title.
    Done,
    /// There was nothing to do: the title has it already.
    Unchanged,
    /// The action did not apply or could not be asked, and says why.
    Skipped,
    /// The action tried and failed.
    Failed,
}

/// What an action did for one title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemOutcome {
    pub id: ItemId,
    pub title: String,
    /// `0` when the title has no year.
    pub year: u16,
    pub kind: OutcomeKind,
    /// One plain sentence.
    pub message: String,
    /// The finer status recorded for the title's step outcome (needs a key, not found), when the
    /// kind alone does not say it. `None` takes the status of [`Self::kind`].
    pub status: Option<StepStatus>,
}

impl ItemOutcome {
    #[must_use]
    pub fn of(item: &Item, kind: OutcomeKind, message: impl Into<String>) -> Self {
        Self {
            id: item.id.unwrap_or(ItemId(0)),
            title: item.core.title.clone(),
            year: item.core.year,
            kind,
            message: message.into(),
            status: None,
        }
    }

    /// The same outcome with a finer step status than its kind says.
    #[must_use]
    pub fn with_status(mut self, status: StepStatus) -> Self {
        self.status = Some(status);
        self
    }

    /// The status stored for the title's step: the finer one when set, else the kind's.
    #[must_use]
    pub fn step_status(&self) -> StepStatus {
        self.status.unwrap_or(match self.kind {
            OutcomeKind::Done => StepStatus::Done,
            OutcomeKind::Unchanged => StepStatus::Unchanged,
            OutcomeKind::Skipped => StepStatus::Skipped,
            OutcomeKind::Failed => StepStatus::Failed,
        })
    }

    /// An outcome for a title that could not be read.
    #[must_use]
    pub fn unreadable(id: ItemId, message: impl Into<String>) -> Self {
        Self {
            id,
            title: format!("#{}", id.0),
            year: 0,
            kind: OutcomeKind::Failed,
            message: message.into(),
            status: None,
        }
    }
}

/// Why a whole action cannot run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Blocker {
    /// A tool the action needs (FFmpeg) is not installed or its path is broken. The
    /// message says where to fix it.
    Tool(String),
    /// A key, an account, a provider or a program is not set up. The message says where to set it
    /// up. The batch carries on with the next step.
    Setup(String),
}

impl Blocker {
    #[must_use]
    pub fn message(&self) -> &str {
        match self {
            Self::Tool(message) | Self::Setup(message) => message,
        }
    }
}

/// What an action did over a list of titles.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActionRun {
    pub items: Vec<ItemOutcome>,
    /// Set when the action stopped because it cannot run at all.
    pub blocker: Option<Blocker>,
}

impl ActionRun {
    /// The number of titles with this outcome.
    #[must_use]
    pub fn count(&self, kind: OutcomeKind) -> usize {
        self.items.iter().filter(|i| i.kind == kind).count()
    }
}

/// The most after-scrape runs that work at the same time.
const AFTER_SCRAPE_PARALLEL: usize = 2;

/// Runs the automation actions. Cheap to share: it is held in an `Arc` by the app services.
pub struct ActionsService {
    db: Arc<Database>,
    settings: Arc<SettingsService>,
    tools: Arc<ToolsManager>,
    notices: Arc<NoticeHub>,
    artwork: Arc<ArtworkService>,
    ratings: Mutex<RatingSources>,
    /// Writes the NFO of a title whose ratings were stored, when the NFO settings want it.
    nfo: Mutex<Option<Arc<crate::nfo::NfoService>>>,
    /// Notices already shown this session, by step, so a missing tool is told once.
    announced: Mutex<HashSet<&'static str>>,
    gate: Semaphore,
    /// The titles whose after-scrape steps are running, shown as "working" in their lifecycle.
    working: Mutex<Option<Arc<Working>>>,
    /// Announces the ratings that were stored, and the start and end of a fetch.
    changes: Changes,
}

impl ActionsService {
    /// Builds the service over the app's shared parts. No rating source is set yet; see
    /// [`Self::set_rating_sources`].
    #[must_use]
    pub fn new(
        db: Arc<Database>,
        settings: Arc<SettingsService>,
        tools: Arc<ToolsManager>,
        notices: Arc<NoticeHub>,
        artwork: Arc<ArtworkService>,
        ratings: RatingSources,
    ) -> Arc<Self> {
        Arc::new(Self {
            settings,
            tools,
            notices,
            artwork,
            ratings: Mutex::new(ratings),
            nfo: Mutex::new(None),
            announced: Mutex::new(HashSet::new()),
            gate: Semaphore::new(AFTER_SCRAPE_PARALLEL),
            working: Mutex::new(None),
            changes: Changes::default(),
            db,
        })
    }

    /// Connects the service to the hub that announces what changed to open pages.
    pub fn set_changes(&self, hub: Arc<ChangeHub>) {
        self.changes.set(hub);
    }

    /// Marks the titles whose after-scrape steps run in `working`, so their lifecycle says so.
    pub fn set_working(&self, working: Arc<Working>) {
        *self.working.lock().unwrap_or_else(PoisonError::into_inner) = Some(working);
    }

    /// Holds `item` as working until the guard is dropped (nothing when no registry is set).
    fn hold_working(&self, item: ItemId) -> Option<WorkingGuard> {
        self.working
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .map(|working| working.hold(item))
    }

    /// Marks the ratings of `item` as being fetched until the guard is dropped.
    fn hold_ratings(&self, item: ItemId) -> Option<RatingsGuard> {
        self.working
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .map(|working| working.hold_ratings(item))
    }

    /// Sets the NFO service: stored ratings are written to the title's NFO by the NFO settings.
    pub fn set_nfo(&self, nfo: Arc<crate::nfo::NfoService>) {
        *self.nfo.lock().unwrap_or_else(PoisonError::into_inner) = Some(nfo);
    }

    /// Writes the NFO of `id` again when the NFO settings want NFOs written. A problem is logged.
    async fn rewrite_nfo(&self, id: ItemId) {
        let Some(nfo) = self
            .nfo
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
        else {
            return;
        };
        let settings = crate::nfo::settings::NfoSettings::from_values(&self.values());
        let written = tokio::task::spawn_blocking(move || {
            nfo.rewrite(&crate::nfo::NfoScope::Items(vec![id]), &settings)
        })
        .await;
        match written {
            Ok(Ok(report)) => {
                if report.totals.changed > 0 {
                    self.changes.publish_one(id, &[ChangePart::Files]);
                }
            }
            Ok(Err(error)) => {
                tracing::warn!(item = id.0, %error, "the NFO was not written after a ratings fetch")
            }
            Err(error) => tracing::warn!(item = id.0, %error, "the NFO task stopped"),
        }
    }

    /// Replaces the rating sources. For the composition root and for tests with mock sources.
    pub fn set_rating_sources(&self, sources: RatingSources) {
        *self.ratings.lock().unwrap_or_else(PoisonError::into_inner) = sources;
    }

    fn rating_sources(&self) -> RatingSources {
        self.ratings
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn values(&self) -> std::collections::BTreeMap<String, serde_json::Value> {
        self.settings.snapshot().values
    }

    /// Reads a title off the async threads.
    async fn load(&self, id: ItemId) -> Result<Item, ItemOutcome> {
        let db = Arc::clone(&self.db);
        match tokio::task::spawn_blocking(move || db.read(|conn| get_item(conn, id))).await {
            Ok(Ok(Some(item))) => Ok(item),
            Ok(Ok(None)) => Err(ItemOutcome::unreadable(
                id,
                "The title is no longer in the library.",
            )),
            Ok(Err(error)) => Err(ItemOutcome::unreadable(
                id,
                format!("The library could not be read: {error}"),
            )),
            Err(_) => Err(ItemOutcome::unreadable(
                id,
                "The work stopped unexpectedly.",
            )),
        }
    }
}
