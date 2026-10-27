//! Matching as the app runs it: the providers, the batch commands, the Needs review
//! inbox and the search-and-scrape panel, over the app's shared database, job manager and
//! settings.
//!
//! The rules live in [`crate::matching`] (decide, queue, scrape, search), `cv-match` (scoring) and
//! `cv-providers`. [`MatchService`] connects them so the desktop shell and the CLI run
//! the same matching:
//!
//! ```text
//! Scrape (a batch command of the app or the CLI) ──► batch jobs (one per provider lane)
//!                                                  ├─ confident ─► scrape (locks honoured)
//!                                                  └─ unsure ───► review queue ─► inbox
//! inbox: accept / bulk accept / ignore / reopen (Undo) / manual search
//! panel: providers / search / preview / scrape with a field selection
//! ```
//!
//! # Settings
//!
//! | Key | Default | Meaning |
//! |---|---|---|
//! | `matching.auto-accept-threshold` | 75 % | confidence at which the top candidate is accepted without asking |
//! | `metadata.language`, `metadata.fallback-language` | `en` | provider request languages |
//!
//! plus the scrape keys of [`crate::scrape_settings::ScrapeSettings`] and the provider choices of
//! [`crate::provider_choice`] (`providers.metadata-movie`, `providers.metadata-show`,
//! `providers.fallback-enabled`, `providers.fallback-order`, `providers.universal`).
//!
//! # Which providers match
//!
//! Only providers that are ready to be asked take part: one whose required credentials have no
//! value is left out until the user adds a key, so a missing key makes the next provider the
//! default instead of failing every title. The order is the registration order of
//! [`crate::provider_setup::register_providers`], then the Kodi XML scraper add-ons found at
//! startup, then the universal provider (built over the ready ones with the configured field
//! sources).
//!
//! The default for movies and for TV shows is the provider the user chose for that kind when it
//! is ready and handles the kind, else the first ready provider that does. When the provider a
//! title runs with finds nothing and fallback is on, the fallback providers are asked in the
//! user's order (`cv_providers::FallbackChain`); the one that answers scrapes the title and is
//! recorded as its last scraper. The Needs review search uses the same chain.
//!
//! # Anime
//!
//! ID completion consults the anime ID cross-reference ([`crate::anime_crosswalk`]) for a title
//! that has an anime-site ID. It is loaded on the first such scrape and refreshed weekly.
//!
//! # Undo of an accept
//!
//! Accepting a candidate scrapes into the item. Before it does, the service keeps a copy of the
//! item (fields, IDs, locks, ratings, cast) for the last [`UNDO_KEEP`] accepted rows. Reopening
//! a row puts that copy back, in the same transaction that reopens the row, so the Undo gives
//! back the library record and not only the inbox entry. The file list is kept as it is now,
//! and files already written to disk (NFO, artwork) stay. A restart forgets the copies; the
//! row then reopens as before and the item keeps what the scrape wrote.
//!
//! # Without a TMDB key
//!
//! CineVault ships no keys. While the required TMDB key is missing, online identification is
//! paused: batches and automatic matching do not start (the titles keep their local NFO and
//! artwork data), and the missing-key banner asks for the key. Manual scraping with another
//! provider still works.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use cv_db::repo::review::{self, ReviewQueueState, ReviewRow};
use cv_db::repo::{get_item, items_by_provider_id, update_item};
use cv_db::{Database, DbError};
use cv_domain::{
    CreditKind, FieldSet, Item, ItemDetail, ItemId, ItemKind, MediaFile, MediaFileType,
    ProviderIds, ScrapeBlock,
};
use cv_jobs::JobManager;
use cv_match::{
    Candidate, EvidenceKind, LocalEvidence, NormalizedQuery, ReviewCandidate, ScoreBreakdown,
    ScoredCandidate, SimilaritySource, Verdict, normalize_query,
};
use cv_providers::model::{MediaKind, RequestOptions, SearchQuery};
use cv_providers::{MetadataProvider, ProviderError, UNIVERSAL_ID, UniversalProvider};

use crate::anime_crosswalk::AnimeCrosswalk;
use crate::app_settings::SettingsService;
use crate::episode_scrape::scrape_after_show_blocking;
use crate::matching::batch::{
    AfterMatch, BatchError, BatchJob, BatchMode, MatchBatch, MatchContext, MatchProviders,
    resume_match_jobs, submit_batch,
};
use crate::matching::search::{Detailed, ranked_search};
use crate::matching::{
    Accepted, MatchError, MatchSettings, ScrapedHook, accept_candidate, apply_scraped_in,
    candidates_of, ignore_item, known_ids, store_completed_ids,
};
use crate::notices::NoticeHub;
use crate::provider_choice::{
    MetadataProviderInfo, ProviderCatalogue, ProviderChoices, UnsupportedAddon,
};
use crate::provider_credentials::ProviderCredentialsService;
use crate::scrape_settings::ScrapeSettings;

mod confident;
mod history;
mod preview;
mod stream;

pub use confident::{ConfidentAccepted, ConfidentOutcome, SearchOnly};
pub use preview::{
    DetailsAnswer, DiffGroup, FieldDiff, PreviewCandidate, ScrapePreset, ScrapePreviewStream,
    ScrapeSelection, scrape_selection,
};

pub use stream::{
    HitProvider, MAX_HITS_PER_SOURCE, MergedHit, NothingFound, ScrapeAnswer, ScrapeCandidateStream,
    ScrapeSource, ScrapeSummary,
};

/// How many review items one listing returns.
pub const REVIEW_LIST_LIMIT: u32 = 1000;

/// Items accepted at the same time by a bulk accept. Each provider's rate limit still applies.
const BULK_PARALLELISM: usize = 4;

/// Why a matching request failed.
#[derive(Debug, thiserror::Error)]
pub enum MatchServiceError {
    #[error("The library database could not be read: {0}")]
    Db(#[from] DbError),
    #[error("That title is not in the library any more.")]
    ItemNotFound,
    #[error("That entry is not waiting for review any more.")]
    NotPending,
    #[error("No metadata provider is ready. Add a key in Settings, Scraping and providers.")]
    NoProvider,
    #[error(
        "Online scraping is paused until you add a TMDB key in Settings, Scraping and providers."
    )]
    IdentificationPaused,
    #[error("{0}")]
    Provider(ProviderError),
    #[error("This title is locked, so it was not changed.")]
    Locked,
    #[error("Only movies and shows can be matched online.")]
    Unsupported,
    #[error("{0}")]
    Invalid(String),
    #[error("The matching job could not be queued: {0}")]
    Queue(String),
}

impl MatchServiceError {
    /// A stable code for the UI: `storage`, `not_found`, `not_pending`, `no_provider`,
    /// `identification_paused`, `provider`, `locked`, `unsupported`, `invalid` or `queue`.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Db(_) => "storage",
            Self::ItemNotFound => "not_found",
            Self::NotPending => "not_pending",
            Self::NoProvider => "no_provider",
            Self::IdentificationPaused => "identification_paused",
            Self::Provider(_) => "provider",
            Self::Locked => "locked",
            Self::Unsupported => "unsupported",
            Self::Invalid(_) => "invalid",
            Self::Queue(_) => "queue",
        }
    }
}

impl From<MatchError> for MatchServiceError {
    fn from(error: MatchError) -> Self {
        match error {
            MatchError::Db(e) => Self::Db(e),
            MatchError::Provider(e) => Self::Provider(e),
            MatchError::ItemNotFound(_) => Self::ItemNotFound,
            MatchError::UnsupportedKind(_) => Self::Unsupported,
            MatchError::NoProvider(_) => Self::NoProvider,
        }
    }
}

impl From<BatchError> for MatchServiceError {
    fn from(error: BatchError) -> Self {
        match error {
            BatchError::Db(e) => Self::Db(e),
            BatchError::NoProviders => Self::NoProvider,
            other => Self::Queue(other.to_string()),
        }
    }
}

type ServiceResult<T> = Result<T, MatchServiceError>;

// ---- what the service returns ----------------------------------------------------------------

/// Whether a candidate agrees with what is known locally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickVerdict {
    Agree,
    Conflict,
    Unknown,
}

/// What an evidence check is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickKind {
    Title,
    Runtime,
    Language,
    Year,
    Id,
}

/// One evidence check on a candidate card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceTick {
    pub kind: TickKind,
    pub verdict: TickVerdict,
    pub text: String,
}

/// How a confidence was reached; all values are fractions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreakdownView {
    pub similarity: f64,
    pub year_penalty: f64,
    pub poster_penalty: f64,
    /// Net adjustment from runtime, language and ID evidence.
    pub evidence: f64,
}

/// A title the matcher found.
#[derive(Debug, Clone, PartialEq)]
pub struct CandidateView {
    pub provider: String,
    /// The provider's own ID for it.
    pub provider_id: String,
    pub title: String,
    /// The title in its original language, when the provider gave one that differs from `title`.
    pub original_title: Option<String>,
    pub year: Option<u16>,
    pub director: Option<String>,
    pub runtime_minutes: Option<u32>,
    pub language: Option<String>,
    pub overview: Option<String>,
    /// Remote address of its poster, when the provider gave one. The app layer downloads it into
    /// the thumbnail cache; it never reaches the interface.
    pub poster_url: Option<String>,
    /// Confidence, 0 to 1.
    pub confidence: f64,
    /// What the list is ordered by (best first): the score plus how well known the title is
    /// (`cv_match::rank_key`). Two titles of one name and one confidence differ here.
    pub rank: f64,
    pub evidence: Vec<EvidenceTick>,
    pub breakdown: BreakdownView,
}

/// What the scan found about an item on disk.
#[derive(Debug, Clone, PartialEq)]
pub struct LocalView {
    pub title: String,
    pub year: Option<u16>,
    pub runtime_seconds: Option<u32>,
    pub tech_summary: Option<String>,
    pub audio_language: Option<String>,
    /// IDs the item already has, such as `tmdb:693134`.
    pub ids: Vec<String>,
    pub has_nfo: bool,
}

/// One entry of the Needs review inbox.
#[derive(Debug, Clone, PartialEq)]
pub struct ReviewEntry {
    pub review_id: i64,
    pub item: ItemId,
    pub kind: ItemKind,
    pub name: String,
    pub path: String,
    /// The path as the user names it: the library folder's label, then the folders below it
    /// (`Movies › Arrival (2016)`). The full path is for the Technical level and the tooltip.
    pub short_path: String,
    pub reason: String,
    pub attention: bool,
    pub note: Option<String>,
    /// The best candidates score the same: Accept all leaves the title for a person to decide.
    pub tied: bool,
    pub local: LocalView,
    pub candidates: Vec<CandidateView>,
}

/// A candidate picked by the user: provider and the provider's ID, so a search result works too.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub provider: String,
    pub provider_id: String,
}

/// A provider that can search and scrape a kind of title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderInfo {
    pub key: String,
    pub name: String,
}

/// A library item that is already linked to a title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryLink {
    pub item: ItemId,
    /// How the library lists it, for example `Solaris (2002)`.
    pub name: String,
}

/// One result of the search-and-scrape panel.
#[derive(Debug, Clone, PartialEq)]
pub struct ScrapeHitView {
    pub candidate: CandidateView,
    pub in_library: Option<LibraryLink>,
}

/// A cast member in a preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CastView {
    pub name: String,
    pub role: Option<String>,
}

/// What a provider would give for a result, shown before anything is written.
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewView {
    pub title: String,
    pub original_title: Option<String>,
    pub tagline: Option<String>,
    pub overview: Option<String>,
    pub year: Option<u16>,
    pub release_date: Option<String>,
    pub runtime_minutes: Option<u32>,
    pub genres: Vec<String>,
    /// Rating on a 0 to 10 scale.
    pub rating: Option<f32>,
    pub votes: Option<u32>,
    pub certification: Option<String>,
    pub cast: Vec<CastView>,
    /// IDs the provider knows, such as `imdb:tt0069293`.
    pub ids: Vec<String>,
}

/// What to scrape and how.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScrapeApply {
    pub provider: String,
    pub provider_id: String,
    /// Language of the scrape; empty uses the metadata language setting.
    pub language: String,
    /// The field keys to write (`cv_domain::MetaField::key`).
    pub fields: Vec<String>,
    pub do_not_overwrite: bool,
    /// The title is already in the library under another item.
    pub second_version: bool,
}

// ---- the service -----------------------------------------------------------------------------

/// How many accepted rows can be undone down to the item's fields.
pub const UNDO_KEEP: usize = 64;

/// An item as it was before a candidate was accepted into it.
struct AcceptSnapshot {
    review_id: i64,
    item: Item,
    /// What the after-scrape steps had said for it (artwork, ratings, collection): an undo puts
    /// these back too, so the Progress lines do not keep the undone scrape's.
    outcomes: Vec<cv_db::repo::step_outcome::StepOutcome>,
}

/// An item and its step outcomes before a scrape.
type BeforeScrape = (Item, Vec<cv_db::repo::step_outcome::StepOutcome>);

/// Runs matching over the app's shared services.
pub struct MatchService {
    db: Arc<Database>,
    jobs: JobManager,
    providers: Arc<ProviderCredentialsService>,
    settings: Arc<SettingsService>,
    notices: Arc<NoticeHub>,
    after_scrape: Mutex<Option<ScrapedHook>>,
    after_match: Mutex<Option<AfterMatch>>,
    after_undo: Mutex<Option<AfterMatch>>,
    anime: Mutex<Option<AnimeCrosswalk>>,
    accept_snapshots: Mutex<VecDeque<AcceptSnapshot>>,
    /// Counts the scrapes started from the Scrape workspace; their undo tokens are negative.
    scrape_undo: AtomicI64,
    /// The ranked candidates of the last search-only pass of each title (Accept all confident).
    searched: Mutex<HashMap<ItemId, Vec<ScoredCandidate>>>,
    /// Where the NFO and artwork files a different match replaces go (see `history.rs`).
    bin: Mutex<Arc<dyn crate::library_ops::SystemBin>>,
}

impl MatchService {
    #[must_use]
    pub fn new(
        db: Arc<Database>,
        jobs: JobManager,
        providers: Arc<ProviderCredentialsService>,
        settings: Arc<SettingsService>,
        notices: Arc<NoticeHub>,
    ) -> Arc<Self> {
        Arc::new(Self {
            db,
            jobs,
            providers,
            settings,
            notices,
            after_scrape: Mutex::new(None),
            after_match: Mutex::new(None),
            after_undo: Mutex::new(None),
            anime: Mutex::new(None),
            accept_snapshots: Mutex::new(VecDeque::new()),
            scrape_undo: AtomicI64::new(0),
            searched: Mutex::new(HashMap::new()),
            bin: Mutex::new(Arc::new(crate::library_ops::FolderOnly)),
        })
    }

    /// Sets the anime ID cross-reference that ID completion uses for anime titles. It is loaded
    /// on the first anime scrape, never at startup.
    pub fn set_anime(&self, anime: AnimeCrosswalk) {
        *self.anime.lock().unwrap_or_else(PoisonError::into_inner) = Some(anime);
    }

    fn anime(&self) -> Option<AnimeCrosswalk> {
        self.anime
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Sets what is told about every item a scrape was written to (single scrapes, accepted
    /// candidates and batches alike), after the item's provider IDs were completed.
    pub fn set_after_scrape(&self, hook: ScrapedHook) {
        *self
            .after_scrape
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(hook);
    }

    /// Sets what happens with an item once a scrape was written into it, whichever way the match
    /// came about (a batch, the inbox, the search-and-scrape panel): the app downloads its
    /// missing artwork. The hook must return at once.
    pub fn set_after_match(&self, hook: AfterMatch) {
        *self
            .after_match
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(hook);
    }

    /// Sets what happens once an undone scrape put a title back: the app writes its NFO again,
    /// so the file next to the video describes the title the library holds. The hook must
    /// return at once.
    pub fn set_after_undo(&self, hook: AfterMatch) {
        *self
            .after_undo
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(hook);
    }

    fn after_match_hook(&self) -> Option<AfterMatch> {
        self.after_match
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The keys of the providers that are not ready to be asked: their required credentials have
    /// no value.
    fn inactive_providers(&self) -> Vec<&'static str> {
        self.providers
            .list()
            .map(|statuses| {
                statuses
                    .into_iter()
                    .filter(|status| !status.active)
                    .map(|status| status.provider)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The provider choices stored in the settings.
    #[must_use]
    pub fn provider_choices(&self) -> ProviderChoices {
        ProviderChoices::from_lookup(|key| self.settings.get(key))
    }

    /// The providers that are ready to be asked, in order of preference, with the user's choices
    /// applied. A provider whose required credentials have no value is left out. The universal
    /// provider, built over the ready ones with the configured sources, comes last; the provider
    /// chosen for each kind is the default for it when it is ready, and the fallback chain
    /// follows the fallback settings.
    #[must_use]
    pub fn match_providers(&self) -> MatchProviders {
        let inactive = self.inactive_providers();
        let mut ready: Vec<Arc<dyn MetadataProvider>> = self
            .providers
            .registry()
            .metadata_providers()
            .iter()
            .filter(|provider| !inactive.contains(&provider.id()))
            .cloned()
            .collect();
        let choices = self.provider_choices();
        let kodi = &self.providers.kodi().provider_ids;
        let chain = choices.chain(&ready, |id| !kodi.contains(&id));
        if !ready.is_empty() {
            let mut universal = UniversalProvider::new(choices.universal.clone(), ready.clone());
            if let Some(crosswalk) = self.anime().and_then(|anime| anime.loaded()) {
                universal = universal.with_anime(crosswalk);
            }
            ready.push(Arc::new(universal));
        }
        MatchProviders::new(ready)
            .with_preferred(choices.movie.as_deref(), choices.show.as_deref())
            .with_fallback(chain)
    }

    /// The IMDb ID of a person known by their TMDB ID, asked of TMDB when the Cast & crew panel
    /// opens, and stored with the person so it is asked once. `None` when TMDB has none for them,
    /// or cannot be asked now (no key, offline): the panel then falls back to a name search.
    ///
    /// # Errors
    /// Fails when the library database cannot be written.
    pub async fn person_imdb_id(&self, tmdb_person: &str) -> Result<Option<String>, DbError> {
        let tmdb_person = tmdb_person.trim();
        if tmdb_person.is_empty() {
            return Ok(None);
        }
        let Some(provider) = self
            .providers
            .registry()
            .metadata_providers()
            .iter()
            .find(|provider| provider.id() == "tmdb")
            .cloned()
        else {
            return Ok(None);
        };
        let mut ids = ProviderIds::new();
        ids.set("tmdb", tmdb_person);
        let found = match provider.person_ids(&ids, &RequestOptions::default()).await {
            Ok(found) => found,
            Err(error) => {
                tracing::debug!(%error, "person IDs not read");
                return Ok(None);
            }
        };
        let Some(imdb) = found.get("imdb").map(str::to_owned) else {
            return Ok(None);
        };
        let (person, value) = (tmdb_person.to_owned(), imdb.clone());
        self.db
            .write(move |tx| cv_db::repo::add_person_id(tx, &person, "imdb", &value))?;
        Ok(Some(imdb))
    }

    /// Every metadata provider as the Providers page lists it, ready or not, with the Kodi
    /// add-ons that cannot be used and why.
    #[must_use]
    pub fn provider_catalogue(&self) -> ProviderCatalogue {
        let inactive = self.inactive_providers();
        let kodi = self.providers.kodi();
        let mut providers: Vec<MetadataProviderInfo> = self
            .providers
            .registry()
            .metadata_providers()
            .iter()
            .map(|provider| {
                let caps = provider.capabilities();
                let is_kodi = kodi.provider_ids.contains(&provider.id());
                MetadataProviderInfo {
                    id: provider.id().to_owned(),
                    name: provider.name().to_owned(),
                    movies: caps.movies,
                    shows: caps.shows,
                    ready: !inactive.contains(&provider.id()),
                    kodi: is_kodi,
                    can_fallback: !is_kodi,
                }
            })
            .collect();
        if !providers.is_empty() {
            let universal = MetadataProviderInfo {
                id: UNIVERSAL_ID.to_owned(),
                name: "Universal".to_owned(),
                movies: providers.iter().any(|p| p.ready && p.movies),
                shows: providers.iter().any(|p| p.ready && p.shows),
                ready: providers.iter().any(|p| p.ready),
                kodi: false,
                can_fallback: false,
            };
            providers.push(universal);
        }
        ProviderCatalogue {
            providers,
            unsupported_addons: kodi
                .unsupported
                .iter()
                .map(|scraper| UnsupportedAddon {
                    addon_id: scraper.addon_id.clone(),
                    addon_name: scraper.addon_name.clone(),
                    content: match scraper.content {
                        cv_providers::kodi::KodiContent::Movies => "movies",
                        cv_providers::kodi::KodiContent::TvShows => "tvshows",
                    }
                    .to_owned(),
                    reason: scraper.reason.clone(),
                })
                .collect(),
            universal_fields: cv_domain::all_fields()
                .into_iter()
                .filter(|field| {
                    field.is_scrapable() && !matches!(field, cv_domain::MetaField::Artwork(_))
                })
                .map(|field| field.key().to_owned())
                .collect(),
        }
    }

    /// The context batches run with, from the ready providers and the current settings.
    #[must_use]
    pub fn context(&self) -> MatchContext {
        MatchContext {
            db: Arc::clone(&self.db),
            providers: self.match_providers(),
            settings: MatchSettings {
                on_scraped: self
                    .after_scrape
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .clone(),
                anime: self.anime(),
                ..MatchSettings::from_values(&self.settings.snapshot().values)
            },
            after_match: self.after_match_hook(),
        }
    }

    fn fail(&self, provider: &str, error: &ProviderError) {
        self.notices.provider_failure(provider, error);
    }

    // ---- batches -----------------------------------------------------------------------

    /// Plans and queues a batch. Must be called inside a tokio runtime; planning reads the
    /// database on the calling thread, so call it from a blocking task for a large library.
    ///
    /// # Errors
    /// No provider is ready, a selection is missing, or the jobs cannot be saved.
    pub fn submit(
        &self,
        mode: BatchMode,
        selection: Option<&[ItemId]>,
    ) -> ServiceResult<MatchBatch> {
        if self.identification_paused() {
            return Err(MatchServiceError::IdentificationPaused);
        }
        let context = self.context();
        let batch = submit_batch(
            &self.jobs,
            &context,
            mode,
            selection,
            crate::matching::batch::DEFAULT_PARALLELISM,
        )?;
        Ok(batch)
    }

    /// The Match selection action: identifies the selected titles in the background. The
    /// uncertain ones arrive in the inbox. Returns how many were queued.
    ///
    /// # Errors
    /// See [`Self::submit`].
    pub fn match_items(&self, items: &[ItemId]) -> ServiceResult<usize> {
        Ok(self.submit(BatchMode::MatchSelected, Some(items))?.planned)
    }

    /// Starts again the match batches that a quit cut short.
    ///
    /// # Errors
    /// The stored jobs cannot be read.
    pub fn resume(&self) -> Result<Vec<BatchJob>, cv_jobs::StoreError> {
        if self.identification_paused() {
            // The stored batches wait for the key.
            return Ok(Vec::new());
        }
        let jobs = resume_match_jobs(&self.jobs, &self.context())?;
        Ok(jobs)
    }

    /// Whether online identification is paused for lack of the required TMDB key.
    #[must_use]
    pub fn identification_paused(&self) -> bool {
        self.providers.identification_paused()
    }

    /// Called after a credential was saved, with whether identification was paused before the
    /// save. When the save lifted the pause, the batches the pause held back (ones the user
    /// started) run again. Nothing new is queued: Scrape is always the user's choice. Returns
    /// whether the pause was lifted. Must be called inside a tokio runtime, from a blocking task
    /// (it reads the database).
    pub fn key_saved(&self, was_paused: bool) -> bool {
        if !was_paused || self.identification_paused() {
            return false;
        }
        if let Err(error) = self.resume() {
            tracing::warn!(%error, "matching batches could not be resumed");
        }
        true
    }

    /// Whether any provider is ready.
    #[must_use]
    pub fn has_providers(&self) -> bool {
        !self.match_providers().is_empty()
    }

    // ---- the review inbox --------------------------------------------------------------

    /// How many items wait for review (the sidebar count).
    ///
    /// # Errors
    /// The database cannot be read.
    pub fn pending_count(&self) -> ServiceResult<u32> {
        let count = self.db.read(review::count_pending)?;
        Ok(u32::try_from(count).unwrap_or(u32::MAX))
    }

    /// The pending items in review order (most confident first), at most `limit`.
    ///
    /// # Errors
    /// The database cannot be read.
    pub fn pending(&self, limit: u32) -> ServiceResult<Vec<ReviewEntry>> {
        let rows = self.db.read(|c| review::list_pending(c, limit, 0))?;
        // Asking which providers are ready reads the credential store, so ask once for the list.
        let providers = self.match_providers();
        let sources = self
            .db
            .read(|c| cv_db::repo::data_sources::list_sources(c, None))?;
        let mut entries = Vec::with_capacity(rows.len());
        for row in rows {
            let Some(item) = self.db.read(|c| get_item(c, row.item_id))? else {
                continue;
            };
            let mut entry = entry_of(&row, &item, &providers)?;
            entry.short_path = short_path(&item, &sources);
            entries.push(entry);
        }
        Ok(entries)
    }

    fn pending_row(&self, review_id: i64) -> ServiceResult<ReviewRow> {
        let row = self
            .db
            .read(|c| review::get_review(c, review_id))?
            .ok_or(MatchServiceError::NotPending)?;
        if row.state == ReviewQueueState::Pending {
            Ok(row)
        } else {
            Err(MatchServiceError::NotPending)
        }
    }

    /// Matches the item of a review row to `choice` and scrapes it. The row leaves the queue.
    ///
    /// # Errors
    /// The row is not pending, the provider cannot be reached, or the item is locked.
    pub async fn accept(&self, review_id: i64, choice: &Choice) -> ServiceResult<Accepted> {
        let row = self.pending_row(review_id)?;
        let context = self.context();
        let scored = chosen_candidate(&row, choice, &context.providers)?;
        let before = self.before_scrape(row.item_id);
        let result = accept_one(&context, row.item_id, &scored).await;
        match &result {
            Err(MatchServiceError::Provider(error)) => self.fail(&scored.candidate.provider, error),
            Ok(accepted) if accepted.scrape.blocked.is_some() => {
                return Err(MatchServiceError::Locked);
            }
            Ok(_) => self.keep_snapshot(review_id, before),
            _ => {}
        }
        result
    }

    /// Accepts the best candidate of every given review row. Returns the rows that were
    /// accepted; the ones that failed stay in the queue.
    ///
    /// # Errors
    /// Fails only when nothing could be accepted, with the first error.
    pub async fn accept_top(&self, review_ids: &[i64]) -> ServiceResult<Vec<i64>> {
        let context = self.context();
        let mut work = Vec::new();
        for id in review_ids {
            let Ok(row) = self.pending_row(*id) else {
                continue;
            };
            let candidates = candidates_of(&row)?;
            // Tied candidates need a person: Accept all never picks one of them.
            if top_two_tie(&candidates) || row.reason.contains("strong candidates") {
                continue;
            }
            let Some(top) = candidates.into_iter().next() else {
                continue;
            };
            let before = self.before_scrape(row.item_id);
            work.push((*id, row.item_id, ScoredCandidate::from(top), before));
        }
        let mut accepted = Vec::new();
        let mut first_error = None;
        let mut queue = work.into_iter();
        let mut running = tokio::task::JoinSet::new();
        loop {
            while running.len() < BULK_PARALLELISM {
                let Some((id, item, scored, before)) = queue.next() else {
                    break;
                };
                let context = context.clone();
                running.spawn(async move {
                    let outcome = accept_one(&context, item, &scored).await;
                    (id, scored.candidate.provider, outcome, before)
                });
            }
            let Some(joined) = running.join_next().await else {
                break;
            };
            let Ok((id, provider, outcome, before)) = joined else {
                continue;
            };
            match outcome {
                Ok(done) if done.scrape.blocked.is_none() => {
                    self.keep_snapshot(id, before);
                    accepted.push(id);
                }
                Ok(_) => {
                    first_error.get_or_insert(MatchServiceError::Locked);
                }
                Err(error) => {
                    if let MatchServiceError::Provider(inner) = &error {
                        self.fail(&provider, inner);
                    }
                    first_error.get_or_insert(error);
                }
            }
        }
        match (accepted.is_empty(), first_error) {
            (true, Some(error)) => Err(error),
            _ => Ok(accepted),
        }
    }

    /// Ignores the item of a review row: "not a catalogued title". Scans never queue it again.
    ///
    /// # Errors
    /// The row is not pending or the database fails.
    pub fn ignore(&self, review_id: i64) -> ServiceResult<()> {
        let row = self.pending_row(review_id)?;
        ignore_item(&self.db, row.item_id)?;
        Ok(())
    }

    /// Puts accepted or ignored rows back in the queue (the Undo of both). For a row accepted in
    /// this session the item is first put back as it was before the scrape (fields, IDs, locks),
    /// in the same transaction; its file list stays as it is and files already written stay. A
    /// row ignored, or accepted before a restart, only reopens: the item keeps what was written.
    ///
    /// # Errors
    /// A row is unknown, already pending, or its item has another pending row. Nothing changes
    /// then, the item included.
    pub fn reopen(&self, review_ids: &[i64]) -> ServiceResult<()> {
        let ids = review_ids.to_vec();
        let snapshots = self.snapshots_for(&ids);
        self.db.write(move |tx| {
            for id in ids {
                if let Some(snapshot) = snapshots.iter().find(|s| s.review_id == id)
                    && let Some(item_id) = snapshot.item.id
                    && let Some(current) = get_item(tx, item_id)?
                {
                    let mut restored = snapshot.item.clone();
                    // The files on disk did not go back in time.
                    restored.core.media_files = current.core.media_files;
                    update_item(tx, &restored)?;
                    restore_outcomes(tx, item_id, &snapshot.outcomes)?;
                }
                review::reopen(tx, id)?;
            }
            Ok(())
        })?;
        self.forget_snapshots(review_ids);
        Ok(())
    }

    /// Keeps a copy of the item before a scrape from the Scrape workspace, which has no review
    /// row. The returned token is negative, so it never meets a review id; pass it to
    /// [`Self::undo_scrape`], or to [`Self::forget_scrape`] when the scrape failed. `None` when
    /// the item cannot be read.
    pub fn keep_scrape_snapshot(&self, item_id: ItemId) -> Option<i64> {
        let before = self.before_scrape(item_id)?;
        let token = -(self.scrape_undo.fetch_add(1, Ordering::Relaxed) + 1);
        self.keep_snapshot(token, Some(before));
        Some(token)
    }

    /// Drops the copy kept for a scrape that did not happen.
    pub fn forget_scrape(&self, token: i64) {
        self.forget_snapshots(&[token]);
    }

    /// Puts an item back as it was before a scrape from the workspace (fields, IDs, locks). The
    /// file list stays as it is and files already written (NFO, artwork) stay, as with the
    /// Undo of an accept.
    ///
    /// # Errors
    /// The copy is gone (a restart forgets them, and only the last [`UNDO_KEEP`] are kept) or
    /// the database fails.
    pub fn undo_scrape(&self, token: i64) -> ServiceResult<()> {
        let snapshot = self
            .snapshots_for(&[token])
            .into_iter()
            .next()
            .ok_or_else(|| {
                MatchServiceError::Invalid(
                    "This scrape can no longer be undone: CineVault was restarted since."
                        .to_owned(),
                )
            })?;
        let restored_id = snapshot.item.id;
        self.db.write(move |tx| {
            if let Some(item_id) = snapshot.item.id
                && let Some(current) = get_item(tx, item_id)?
            {
                let mut restored = snapshot.item.clone();
                // The files on disk did not go back in time.
                restored.core.media_files = current.core.media_files;
                update_item(tx, &restored)?;
                restore_outcomes(tx, item_id, &snapshot.outcomes)?;
            }
            Ok(())
        })?;
        self.forget_snapshots(&[token]);
        let hook = self
            .after_undo
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let (Some(hook), Some(item_id)) = (hook, restored_id) {
            hook(item_id);
        }
        Ok(())
    }

    /// The item and its step outcomes as they are before a scrape, for its Undo. Read before the
    /// scrape runs: read after it, the outcomes would be the scrape's own (the Undo of
    /// an accept kept "scraped" instead of the "not matched" it had).
    fn before_scrape(&self, item_id: ItemId) -> Option<BeforeScrape> {
        let item = self.item(item_id).ok()?;
        let outcomes = self
            .db
            .read(|c| cv_db::repo::step_outcome::step_outcomes(c, item_id))
            .unwrap_or_default();
        Some((item, outcomes))
    }

    fn keep_snapshot(&self, review_id: i64, before: Option<BeforeScrape>) {
        let Some((item, outcomes)) = before else {
            return;
        };
        let mut snapshots = self
            .accept_snapshots
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        snapshots.retain(|s| s.review_id != review_id);
        snapshots.push_back(AcceptSnapshot {
            review_id,
            item,
            outcomes,
        });
        while snapshots.len() > UNDO_KEEP {
            snapshots.pop_front();
        }
    }

    fn snapshots_for(&self, review_ids: &[i64]) -> Vec<AcceptSnapshot> {
        self.accept_snapshots
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|s| review_ids.contains(&s.review_id))
            .map(|s| AcceptSnapshot {
                review_id: s.review_id,
                item: s.item.clone(),
                outcomes: s.outcomes.clone(),
            })
            .collect()
    }

    fn forget_snapshots(&self, review_ids: &[i64]) {
        self.accept_snapshots
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|s| !review_ids.contains(&s.review_id));
    }

    /// Searches by title or ID shortcut for the item of a review row, with the usual scoring.
    ///
    /// # Errors
    /// The row is not pending, no provider is ready, or the provider fails.
    pub async fn search_review(
        &self,
        review_id: i64,
        query: &str,
    ) -> ServiceResult<Vec<CandidateView>> {
        let row = self.pending_row(review_id)?;
        let item = self.item(row.item_id)?;
        let media = media_kind(item.kind())?;
        let context = self.context();
        let provider = context
            .providers
            .default_for(media)
            .cloned()
            .ok_or(MatchServiceError::NoProvider)?;
        let request = &context.settings.request;
        let mut found = self
            .search_item(&item, provider.as_ref(), media, query, request)
            .await?;
        // The provider has nothing for the text: the fallback providers, in the user's order,
        // may. Their candidates say which provider they came from.
        if found.is_empty()
            && let Some((fallback, _)) = context
                .providers
                .fallback()
                .search_after(
                    provider.id(),
                    &SearchQuery {
                        kind: media,
                        title: normalize_query(query).text,
                        year: None,
                    },
                    request,
                )
                .await
        {
            found = self
                .search_item(&item, fallback.as_ref(), media, query, request)
                .await?;
        }
        let evidence = EvidenceContext::of(&item);
        Ok(found
            .iter()
            .map(|d| detailed_view(d, &evidence, &context.providers))
            .collect())
    }

    // ---- the search-and-scrape panel ----------------------------------------------------

    /// The providers that can search and scrape `kind`, the default first.
    #[must_use]
    pub fn scrape_providers(&self, kind: ItemKind) -> Vec<ProviderInfo> {
        let Ok(media) = media_kind(kind) else {
            return Vec::new();
        };
        self.match_providers()
            .supporting(media)
            .into_iter()
            .map(|p| ProviderInfo {
                key: p.id().to_owned(),
                name: p.name().to_owned(),
            })
            .collect()
    }

    /// Searches `provider` for the item, with the usual scoring. Each result says which library
    /// item, if any, is already linked to it.
    ///
    /// # Errors
    /// The provider is unknown, or fails.
    pub async fn scrape_search(
        &self,
        item_id: ItemId,
        provider: &str,
        language: &str,
        query: &str,
    ) -> ServiceResult<Vec<ScrapeHitView>> {
        let item = self.item(item_id)?;
        let media = media_kind(item.kind())?;
        let context = self.context();
        let provider = context
            .providers
            .get(provider)
            .cloned()
            .ok_or(MatchServiceError::NoProvider)?;
        let request = request_for(&context.settings, language);
        let found = self
            .search_item(&item, provider.as_ref(), media, query, &request)
            .await?;
        let evidence = EvidenceContext::of(&item);
        let mut hits = Vec::with_capacity(found.len());
        for detailed in &found {
            let candidate = detailed_view(detailed, &evidence, &context.providers);
            let in_library = self.linked_item(
                provider.as_ref(),
                &detailed.scored.candidate.ids,
                &candidate.provider_id,
                item_id,
            )?;
            hits.push(ScrapeHitView {
                candidate,
                in_library,
            });
        }
        Ok(hits)
    }

    /// What `provider` would give for the title, without writing anything.
    ///
    /// # Errors
    /// The provider is unknown, or fails.
    pub async fn scrape_preview(
        &self,
        kind: ItemKind,
        provider: &str,
        provider_id: &str,
        language: &str,
    ) -> ServiceResult<PreviewView> {
        let media = media_kind(kind)?;
        let context = self.context();
        let provider = context
            .providers
            .get(provider)
            .cloned()
            .ok_or(MatchServiceError::NoProvider)?;
        let ids = ids_of_choice(provider.as_ref(), provider_id)?;
        let request = request_for(&context.settings, language);
        match provider.details(media, &ids, &request).await {
            Ok(item) => Ok(preview_of(&item)),
            Err(error) => {
                self.fail(provider.id(), &error);
                Err(MatchServiceError::Provider(error))
            }
        }
    }

    /// Links the item to the title and writes the chosen fields. Locks are honoured; when the
    /// item was linked to another title, its old provider IDs are replaced.
    ///
    /// # Errors
    /// The provider is unknown or fails, or the item is locked.
    pub async fn scrape_apply(
        &self,
        item_id: ItemId,
        request: &ScrapeApply,
    ) -> ServiceResult<cv_domain::ScrapeOutcome> {
        self.scrape_apply_recorded(item_id, request)
            .await
            .map(|(outcome, _)| outcome)
    }

    /// [`Self::scrape_apply`] that also says which History entry records the change. A different
    /// match than the title had is an entry: the title is copied and its NFO and artwork files go
    /// to the bin before the new match is written, so Undo can bring it all back. A title never
    /// scraped, or scraped with the same match, is not (nothing is lost but fetched data).
    ///
    /// # Errors
    /// See [`Self::scrape_apply`]; also when the old NFO and artwork files cannot be sent to the
    /// bin (the title is left as it was).
    pub async fn scrape_apply_recorded(
        &self,
        item_id: ItemId,
        request: &ScrapeApply,
    ) -> ServiceResult<(cv_domain::ScrapeOutcome, Option<i64>)> {
        let item = self.item(item_id)?;
        let kind = item.kind();
        let media = media_kind(kind)?;
        let context = self.context();
        let provider = context
            .providers
            .get(&request.provider)
            .cloned()
            .ok_or(MatchServiceError::NoProvider)?;
        let ids = ids_of_choice(provider.as_ref(), &request.provider_id)?;
        let options = request_for(&context.settings, &request.language);
        let scraped = match provider.details(media, &ids, &options).await {
            Ok(scraped) => scraped,
            Err(error) => {
                self.fail(provider.id(), &error);
                return Err(MatchServiceError::Provider(error));
            }
        };

        let mut settings = context.settings.clone();
        let fields: FieldSet = FieldSet::from_keys(&request.fields.join(","))
            .iter()
            .filter(|f| f.applies_to(kind))
            .collect();
        settings.scrape = ScrapeSettings {
            movie: fields.clone(),
            show: fields,
            episode: settings.scrape.episode.clone(),
            do_not_overwrite: request.do_not_overwrite,
            auto_group_collections: settings.scrape.auto_group_collections,
            episode_order: settings.scrape.episode_order,
        };
        settings.request = options.clone();
        let scrape_settings = settings.clone();

        // A different title than the one the item is linked to replaces its provider IDs (the
        // scrape only ever adds IDs), and nothing of the old title survives where the new one has
        // no value: its other ratings, its collection, its artwork addresses.
        // One transaction: a write that is refused (a locked title) or fails leaves the item as
        // it was, IDs included, and nothing is lost without the Undo copy (which is dropped on an
        // error).
        let relink = relinks(&item, provider.as_ref(), &scraped);
        // A different match is a History entry: the title is copied and its NFO and artwork
        // files go to the bin before anything is written.
        let mut kept = None;
        let new_label = history::item_label(&scraped);
        if relink && !item.core.locked && item.kind() == scraped.kind() {
            kept = self
                .snapshot_for_match(item_id, Some(&scraped.core.ids), None)
                .map_err(MatchServiceError::Invalid)?;
        }
        let written = {
            let db = Arc::clone(&self.db);
            let provider_id = provider.id().to_owned();
            tokio::task::spawn_blocking(move || {
                db.write(move |tx| {
                    if relink {
                        clear_old_match(tx, item_id, &scraped, &settings.scrape.options_for(kind))?;
                    }
                    apply_scraped_in(tx, item_id, kind, &scraped, &provider_id, &settings)
                })
            })
            .await
            .map_err(|e| MatchServiceError::Queue(e.to_string()))?
        };
        let outcome = match written {
            Ok(outcome) => outcome,
            Err(error) => {
                if let Some(kept) = &kept {
                    self.unbin_snapshot(kept);
                }
                return Err(error.into());
            }
        };
        match outcome.blocked {
            Some(ScrapeBlock::ItemLocked) => {
                if let Some(kept) = &kept {
                    self.unbin_snapshot(kept);
                }
                return Err(MatchServiceError::Locked);
            }
            Some(ScrapeBlock::KindMismatch) => {
                if let Some(kept) = &kept {
                    self.unbin_snapshot(kept);
                }
                return Err(MatchServiceError::Unsupported);
            }
            None => {}
        }
        let entry = kept.and_then(|snapshot| {
            let old = history::item_label(&snapshot.item);
            self.record_match(
                format!("Changed match · {old}"),
                format!("{old} → {new_label}"),
                vec![(snapshot, new_label.clone())],
            )
        });

        if crate::collections::wanted_after_scrape(&scrape_settings, kind) {
            let all = context.providers.refs();
            crate::collections::assign_scraped_movie(&self.db, &all, item_id, &options).await;
        }

        let known = known_ids(&self.db, item_id)?;
        let others: Vec<&dyn MetadataProvider> = context
            .providers
            .refs()
            .into_iter()
            .filter(|p| p.id() != provider.id())
            .collect();
        let anime = context.settings.anime_for(&known).await;
        let done =
            cv_providers::complete_ids(media, &known, &others, anime.as_deref(), &options).await;
        if !done.added.is_empty() {
            store_completed_ids(&self.db, item_id, done.ids)?;
        }
        if kind == ItemKind::Show {
            scrape_after_show_blocking(&self.db, item_id, provider.as_ref(), &scrape_settings)
                .await;
        }
        context.settings.notify_scraped(item_id);
        if let Some(hook) = self.after_match_hook() {
            hook(item_id);
        }
        Ok((outcome, entry))
    }

    // ---- shared pieces -----------------------------------------------------------------

    fn item(&self, id: ItemId) -> ServiceResult<Item> {
        self.db
            .read(|c| get_item(c, id))?
            .ok_or(MatchServiceError::ItemNotFound)
    }

    /// A manual search for `item`: the typed text or ID shortcut, scored against the item's own
    /// evidence. The item's stored IDs are not used, so a correction can differ from them.
    async fn search_item(
        &self,
        item: &Item,
        provider: &dyn MetadataProvider,
        media: MediaKind,
        text: &str,
        request: &RequestOptions,
    ) -> ServiceResult<Vec<Detailed>> {
        let query: NormalizedQuery = normalize_query(text);
        if query.id.is_none() && query.text.is_empty() {
            return Err(MatchServiceError::Invalid(
                "Type a title or an ID to search for.".to_owned(),
            ));
        }
        let evidence = local_evidence(item);
        match ranked_search(
            provider,
            media,
            &query,
            &ProviderIds::new(),
            &evidence,
            request,
        )
        .await
        {
            Ok(found) => Ok(found),
            Err(error) => {
                self.fail(provider.id(), &error);
                Err(MatchServiceError::Provider(error))
            }
        }
    }

    /// The library item, other than `except`, already linked to this title.
    fn linked_item(
        &self,
        provider: &dyn MetadataProvider,
        ids: &ProviderIds,
        provider_id: &str,
        except: ItemId,
    ) -> ServiceResult<Option<LibraryLink>> {
        let key = id_key_of(provider, ids).unwrap_or_else(|| provider.id().to_owned());
        let linked = self
            .db
            .read(|c| items_by_provider_id(c, &key, provider_id))?;
        let Some(other) = linked.into_iter().find(|id| *id != except) else {
            return Ok(None);
        };
        let Some(item) = self.db.read(|c| get_item(c, other))? else {
            return Ok(None);
        };
        let name = if item.core.year > 0 {
            format!("{} ({})", item.core.title, item.core.year)
        } else {
            item.core.title.clone()
        };
        Ok(Some(LibraryLink { item: other, name }))
    }
}

/// Puts the step outcomes of `item` back as they were: steps the undone scrape recorded that had
/// no outcome before are cleared.
fn restore_outcomes(
    tx: &rusqlite::Connection,
    item: ItemId,
    outcomes: &[cv_db::repo::step_outcome::StepOutcome],
) -> Result<(), cv_db::DbError> {
    use cv_db::repo::step_outcome::{clear_step_outcome, record_step_outcome, step_outcomes};
    for current in step_outcomes(tx, item)? {
        if !outcomes.iter().any(|o| o.step == current.step) {
            clear_step_outcome(tx, item, &current.step)?;
        }
    }
    for outcome in outcomes {
        record_step_outcome(
            tx,
            item,
            &outcome.step,
            outcome.status,
            &outcome.reason,
            outcome.at_ms,
        )?;
    }
    Ok(())
}

/// The item's folder as the user names it: the label of its library folder (else the folder's
/// own name), then the folders below it, joined by ` › `. The full path when it is in no known
/// library folder.
fn short_path(item: &Item, sources: &[cv_domain::DataSource]) -> String {
    let path = item.core.path.trim_end_matches(['/', '\\']);
    let parts = |text: &str| -> Vec<String> {
        text.split(['/', '\\'])
            .filter(|part| !part.is_empty())
            .map(str::to_owned)
            .collect()
    };
    let names = parts(path);
    // The deepest library folder that holds it: a folder inside another one names its titles.
    let found = sources
        .iter()
        .filter_map(|source| {
            let root = parts(&source.path);
            let inside = !root.is_empty()
                && names.len() >= root.len()
                && root
                    .iter()
                    .zip(&names)
                    .all(|(a, b)| a.eq_ignore_ascii_case(b));
            inside.then_some((source, root))
        })
        .max_by_key(|(_, root)| root.len());
    let Some((source, root)) = found else {
        return path.to_owned();
    };
    let label = if source.label.trim().is_empty() {
        root.last().cloned().unwrap_or_default()
    } else {
        source.label.trim().to_owned()
    };
    std::iter::once(label)
        .chain(names[root.len()..].iter().cloned())
        .collect::<Vec<_>>()
        .join(" \u{203a} ")
}

/// Whether the best two stored candidates score the same: only a person can tell them apart, so
/// Accept all leaves the title alone.
fn top_two_tie(candidates: &[cv_match::ReviewCandidate]) -> bool {
    match candidates {
        [first, second, ..] => {
            (first.breakdown.raw_score - second.breakdown.raw_score).abs() < 1e-9
        }
        _ => false,
    }
}

fn entry_of(
    row: &ReviewRow,
    item: &Item,
    providers: &MatchProviders,
) -> ServiceResult<ReviewEntry> {
    let local = local_view(item);
    let context = EvidenceContext::of(item);
    let stored = candidates_of(row)?;
    let tied = top_two_tie(&stored);
    let candidates = stored
        .iter()
        .map(|stored| candidate_view(stored, None, None, &context, providers))
        .collect();
    let tie = tied || row.reason.contains("strong candidates");
    Ok(ReviewEntry {
        tied: tie,
        review_id: row.id,
        item: row.item_id,
        kind: item.kind(),
        name: queue_name(item),
        path: item.core.path.clone(),
        short_path: String::new(),
        reason: row.reason.clone(),
        attention: !row.reason.starts_with("No candidates"),
        note: tie.then(|| {
            "Several titles scored the same. The runtime, year and audio language below \
             can tell them apart."
                .to_owned()
        }),
        local,
        candidates,
    })
}

fn media_kind(kind: ItemKind) -> ServiceResult<MediaKind> {
    match kind {
        ItemKind::Movie => Ok(MediaKind::Movie),
        ItemKind::Show => Ok(MediaKind::Show),
        _ => Err(MatchServiceError::Unsupported),
    }
}

fn request_for(settings: &MatchSettings, language: &str) -> RequestOptions {
    let mut request = settings.request.clone();
    if !language.trim().is_empty() {
        language.trim().clone_into(&mut request.language);
    }
    request
}

async fn accept_one(
    context: &MatchContext,
    item: ItemId,
    scored: &ScoredCandidate,
) -> ServiceResult<Accepted> {
    let providers: Vec<&dyn MetadataProvider> =
        context.providers.all().iter().map(|p| p.as_ref()).collect();
    let accepted =
        accept_candidate(&context.db, item, scored, &providers, &context.settings).await?;
    if accepted.scrape.blocked.is_none()
        && let Some(hook) = &context.after_match
    {
        hook(item);
    }
    Ok(accepted)
}

// ---- IDs ---------------------------------------------------------------------------------------

/// The key under which `provider` stores its own ID: the first ID it can look up.
fn own_key(provider: &dyn MetadataProvider) -> Option<&'static str> {
    provider.capabilities().lookup_ids.first().copied()
}

/// The provider's own ID in `ids`, with its key.
fn id_key_of(provider: &dyn MetadataProvider, ids: &ProviderIds) -> Option<String> {
    if ids.contains(provider.id()) {
        return Some(provider.id().to_owned());
    }
    provider
        .capabilities()
        .lookup_ids
        .iter()
        .find(|key| ids.contains(key))
        .map(|key| (*key).to_owned())
}

fn provider_id_of(provider: &dyn MetadataProvider, ids: &ProviderIds) -> String {
    id_key_of(provider, ids)
        .and_then(|key| ids.get(&key).map(str::to_owned))
        .unwrap_or_default()
}

fn ids_of_choice(provider: &dyn MetadataProvider, provider_id: &str) -> ServiceResult<ProviderIds> {
    let key = own_key(provider).ok_or(MatchServiceError::NoProvider)?;
    let mut ids = ProviderIds::new();
    if !ids.set(key, provider_id) {
        return Err(MatchServiceError::Invalid(
            "That result has no ID to look up.".to_owned(),
        ));
    }
    Ok(ids)
}

/// The stored candidate the user picked, or one built from a search result.
fn chosen_candidate(
    row: &ReviewRow,
    choice: &Choice,
    providers: &MatchProviders,
) -> ServiceResult<ScoredCandidate> {
    for stored in candidates_of(row)? {
        let Some(provider) = providers.get(&stored.candidate.provider) else {
            continue;
        };
        if stored.candidate.provider == choice.provider
            && provider_id_of(provider.as_ref(), &stored.candidate.ids) == choice.provider_id
        {
            return Ok(ScoredCandidate::from(stored));
        }
    }
    let provider = providers
        .get(&choice.provider)
        .ok_or(MatchServiceError::NoProvider)?;
    let ids = ids_of_choice(provider.as_ref(), &choice.provider_id)?;
    Ok(ScoredCandidate {
        candidate: Candidate {
            provider: choice.provider.clone(),
            ids,
            exact_id: true,
            ..Candidate::default()
        },
        breakdown: ScoreBreakdown {
            similarity: 1.0,
            similarity_source: SimilaritySource::ExactId,
            year_penalty: 0.0,
            poster_penalty: 0.0,
            base: 1.0,
            evidence: Vec::new(),
            raw_score: 1.0,
            score: 1.0,
        },
    })
}

/// Whether scraping `scraped` into `item` links it to another title than before: the item has an
/// ID of the provider and it differs.
/// Whether the scraped title is another title than the one the item is linked to: the
/// provider's own ID, or any other ID both have (an IMDb ID, say), differs. The collection ID
/// does not count: it names the collection, not the title.
fn relinks(item: &Item, provider: &dyn MetadataProvider, scraped: &Item) -> bool {
    let differs = |key: &str| match (item.core.ids.get(key), scraped.core.ids.get(key)) {
        (Some(old), Some(new)) => !old.trim().eq_ignore_ascii_case(new.trim()),
        _ => false,
    };
    own_key(provider).is_some_and(differs)
        || scraped
            .core
            .ids
            .iter()
            .any(|(key, _)| key != cv_domain::id_key::TMDB_SET && differs(key))
}

/// Empties what the old title leaves behind before a change of match is written: its provider
/// IDs (the scrape only ever adds IDs) and every selected, unlocked field the new title has no
/// value for (see [`cv_domain::clear_for_relink`]). Locked fields stay. A locked title, or one of
/// another kind, is left alone: the write that follows refuses it.
fn clear_old_match(
    tx: &rusqlite::Connection,
    item_id: ItemId,
    scraped: &Item,
    options: &cv_domain::ScrapeOptions,
) -> Result<(), DbError> {
    let Some(mut item) = get_item(tx, item_id)? else {
        return Err(DbError::ItemNotFound(item_id.0));
    };
    if item.core.locked || item.kind() != scraped.kind() {
        return Ok(());
    }
    let locks = cv_db::repo::field_locks(tx, item_id)?;
    item.core.ids = ProviderIds::new();
    cv_domain::clear_for_relink(&mut item, scraped, options, &locks);
    update_item(tx, &item)
}

// ---- local evidence ----------------------------------------------------------------------------

/// The video the item is judged by: the first with a known length, else the first video.
fn main_video(item: &Item) -> Option<&MediaFile> {
    let videos = || {
        item.core
            .media_files
            .iter()
            .filter(|f| f.kind == MediaFileType::Video)
    };
    videos()
        .find(|f| f.duration_secs > 0)
        .or_else(|| videos().next())
}

fn local_evidence(item: &Item) -> LocalEvidence {
    let mut evidence = match main_video(item).filter(|f| f.duration_secs > 0) {
        Some(video) => LocalEvidence::from_media(
            video.duration_secs,
            &video.audio_streams,
            item.core.ids.clone(),
        ),
        None => LocalEvidence {
            ids: item.core.ids.clone(),
            ..LocalEvidence::default()
        },
    };
    // The year of the file name or the NFO tells same-named titles apart when the search has none.
    evidence.year = (item.core.year > 0).then_some(item.core.year);
    evidence
}

/// What the evidence ticks compare a candidate with.
struct EvidenceContext {
    year: Option<u16>,
    runtime_minutes: Option<u32>,
    audio_language: Option<String>,
}

impl EvidenceContext {
    fn of(item: &Item) -> Self {
        let evidence = local_evidence(item);
        Self {
            year: (item.core.year > 0).then_some(item.core.year),
            runtime_minutes: evidence.runtime_minutes,
            audio_language: evidence.audio_language,
        }
    }
}

fn folder_name(path: &str) -> String {
    path.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .to_owned()
}

/// The label in the queue: the folder and the video file, for example `Solaris\Solaris.mkv`.
fn queue_name(item: &Item) -> String {
    let folder = folder_name(&item.core.path);
    match main_video(item) {
        Some(video) if !video.filename.is_empty() => {
            format!("{folder}{}{}", std::path::MAIN_SEPARATOR, video.filename)
        }
        _ if !folder.is_empty() => folder,
        _ => item.core.title.clone(),
    }
}

fn resolution_label(height: u32) -> Option<String> {
    match height {
        0 => None,
        2000.. => Some("2160p".to_owned()),
        1000..=1999 => Some("1080p".to_owned()),
        700..=999 => Some("720p".to_owned()),
        500..=699 => Some("576p".to_owned()),
        400..=499 => Some("480p".to_owned()),
        other => Some(format!("{other}p")),
    }
}

fn channel_layout(channels: u32) -> String {
    match channels {
        0 => String::new(),
        1 => " 1.0".to_owned(),
        2 => " 2.0".to_owned(),
        3 => " 2.1".to_owned(),
        6 => " 5.1".to_owned(),
        7 => " 6.1".to_owned(),
        8 => " 7.1".to_owned(),
        n => format!(" {n}ch"),
    }
}

fn tech_summary(video: &MediaFile) -> Option<String> {
    // No container format yet: the media info has not been read.
    if video.container_format.is_empty() {
        return None;
    }
    let mut parts = Vec::new();
    parts.extend(resolution_label(video.height));
    if !video.video_codec.is_empty() {
        parts.push(video.video_codec.clone());
    }
    if let Some(audio) = video.audio_streams.first()
        && !audio.codec.is_empty()
    {
        parts.push(format!("{}{}", audio.codec, channel_layout(audio.channels)));
    }
    (!parts.is_empty()).then(|| parts.join(" \u{b7} "))
}

fn local_view(item: &Item) -> LocalView {
    let video = main_video(item);
    let evidence = local_evidence(item);
    LocalView {
        title: if item.core.title.trim().is_empty() {
            folder_name(&item.core.path)
        } else {
            item.core.title.trim().to_owned()
        },
        year: (item.core.year > 0).then_some(item.core.year),
        runtime_seconds: video.map(|v| v.duration_secs).filter(|s| *s > 0),
        tech_summary: video.and_then(tech_summary),
        audio_language: evidence.audio_language,
        ids: item
            .core
            .ids
            .iter()
            .map(|(key, value)| format!("{key}:{value}"))
            .collect(),
        has_nfo: item
            .core
            .media_files
            .iter()
            .any(|f| f.kind == MediaFileType::Nfo),
    }
}

// ---- candidate views ---------------------------------------------------------------------------

fn tick(kind: TickKind, verdict: TickVerdict, text: impl Into<String>) -> EvidenceTick {
    EvidenceTick {
        kind,
        verdict,
        text: text.into(),
    }
}

fn evidence_ticks(
    candidate: &Candidate,
    breakdown: &ScoreBreakdown,
    local: &EvidenceContext,
) -> Vec<EvidenceTick> {
    let mut ticks = Vec::new();
    let title_verdict = if breakdown.similarity_source == SimilaritySource::ExactId
        || breakdown.similarity >= 0.85
    {
        TickVerdict::Agree
    } else if breakdown.similarity < 0.6 {
        TickVerdict::Conflict
    } else {
        TickVerdict::Unknown
    };
    ticks.push(tick(
        TickKind::Title,
        title_verdict,
        format!("title {:.2}", breakdown.similarity),
    ));
    for term in &breakdown.evidence {
        let agree = term.verdict == Verdict::Agree;
        let verdict = if agree {
            TickVerdict::Agree
        } else {
            TickVerdict::Conflict
        };
        let relation = if agree { "=" } else { "\u{2260}" };
        match term.kind {
            EvidenceKind::Runtime => {
                if let (Some(c), Some(l)) = (candidate.runtime_minutes, local.runtime_minutes) {
                    ticks.push(tick(
                        TickKind::Runtime,
                        verdict,
                        format!("runtime {c} {relation} {l} min"),
                    ));
                }
            }
            EvidenceKind::Language => {
                if let (Some(c), Some(l)) = (&candidate.original_language, &local.audio_language) {
                    ticks.push(tick(
                        TickKind::Language,
                        verdict,
                        format!("language {c} {relation} {l}"),
                    ));
                }
            }
            EvidenceKind::ProviderId => {
                let text = if agree {
                    "ID matches the one on file"
                } else {
                    "ID differs from the one on file"
                };
                ticks.push(tick(TickKind::Id, verdict, text));
            }
        }
    }
    ticks.push(match (local.year, candidate.year) {
        (_, 0) => tick(TickKind::Year, TickVerdict::Unknown, "year unknown"),
        (None, _) => tick(TickKind::Year, TickVerdict::Unknown, "year not in filename"),
        (Some(l), c) if l == c => tick(TickKind::Year, TickVerdict::Agree, format!("year {c}")),
        (Some(l), c) => tick(
            TickKind::Year,
            TickVerdict::Conflict,
            format!("year {c} \u{2260} {l}"),
        ),
    });
    ticks
}

fn candidate_view(
    stored: &ReviewCandidate,
    director: Option<String>,
    overview: Option<String>,
    local: &EvidenceContext,
    providers: &MatchProviders,
) -> CandidateView {
    let candidate = &stored.candidate;
    let breakdown = &stored.breakdown;
    let provider_id = providers
        .get(&candidate.provider)
        .map(|p| provider_id_of(p.as_ref(), &candidate.ids))
        .filter(|id| !id.is_empty())
        .or_else(|| candidate.ids.iter().next().map(|(_, v)| v.to_owned()))
        .unwrap_or_default();
    CandidateView {
        provider: candidate.provider.clone(),
        provider_id,
        title: candidate.title.clone(),
        original_title: non_empty(&candidate.original_title)
            .filter(|original| !original.eq_ignore_ascii_case(candidate.title.trim())),
        year: (candidate.year > 0).then_some(candidate.year),
        director: director.or_else(|| candidate.director.clone()),
        runtime_minutes: candidate.runtime_minutes,
        language: candidate.original_language.clone(),
        overview,
        poster_url: candidate.poster_url.clone().filter(|url| !url.is_empty()),
        confidence: breakdown.score,
        rank: cv_match::rank_key(&ScoredCandidate {
            candidate: candidate.clone(),
            breakdown: breakdown.clone(),
        }),
        evidence: evidence_ticks(candidate, breakdown, local),
        breakdown: BreakdownView {
            similarity: breakdown.similarity,
            year_penalty: breakdown.year_penalty,
            poster_penalty: breakdown.poster_penalty,
            evidence: breakdown.evidence.iter().map(|t| t.delta).sum(),
        },
    }
}

fn detailed_view(
    detailed: &Detailed,
    local: &EvidenceContext,
    providers: &MatchProviders,
) -> CandidateView {
    candidate_view(
        &ReviewCandidate::from(&detailed.scored),
        detailed.director.clone(),
        detailed.overview.clone(),
        local,
        providers,
    )
}

// ---- previews ----------------------------------------------------------------------------------

fn non_empty(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

fn preview_of(item: &Item) -> PreviewView {
    let (tagline, runtime, release, genres, certification) = match &item.detail {
        ItemDetail::Movie(m) => (
            &m.tagline,
            m.runtime_minutes,
            m.release_date.map(|d| d.to_string()),
            m.genres.iter().map(|g| g.name().to_owned()).collect(),
            m.certification.0.clone(),
        ),
        ItemDetail::Show(s) => (
            &s.tagline,
            s.runtime_minutes,
            s.first_aired.map(|d| d.to_string()),
            s.genres.iter().map(|g| g.name().to_owned()).collect(),
            s.certification.0.clone(),
        ),
        _ => (&item.core.outline, 0, None, Vec::new(), String::new()),
    };
    // The rating with the most votes speaks for the title.
    let rating = item
        .core
        .ratings
        .iter()
        .filter(|r| r.id != cv_domain::rating_key::USER)
        .max_by_key(|r| r.votes);
    PreviewView {
        title: item.core.title.clone(),
        original_title: non_empty(&item.core.original_title)
            .filter(|original| *original != item.core.title),
        tagline: non_empty(tagline),
        overview: non_empty(&item.core.plot),
        year: (item.core.year > 0).then_some(item.core.year),
        release_date: release,
        runtime_minutes: (runtime > 0).then_some(runtime),
        genres,
        rating: rating.map(|r| r.on_ten_scale()),
        votes: rating.map(|r| r.votes).filter(|v| *v > 0),
        certification: non_empty(&certification),
        cast: item
            .core
            .credits
            .iter()
            .filter(|c| c.kind == CreditKind::Actor)
            .take(12)
            .map(|c| CastView {
                name: c.person.name.clone(),
                role: non_empty(&c.role),
            })
            .collect(),
        ids: item
            .core
            .ids
            .iter()
            .map(|(key, value)| format!("{key}:{value}"))
            .collect(),
    }
}

#[cfg(test)]
mod short_path_tests {
    use super::*;

    fn source(path: &str, label: &str) -> cv_domain::DataSource {
        cv_domain::DataSource {
            id: 1,
            kind: cv_domain::SourceKind::Movie,
            path: path.to_owned(),
            label: label.to_owned(),
            position: 0,
            created_ms: 0,
        }
    }

    /// Needs review names a folder by its library folder's label.
    #[test]
    fn a_folder_reads_by_its_library_folders_label() {
        let mut item = Item::movie("Arrival");
        item.core.path = r"C:\Users\me\media\Movies\Arrival (2016)".to_owned();
        let sources = [
            source(r"C:\Users\me\media", "Media"),
            source(r"C:\Users\me\media\movies", "Movies"),
        ];
        assert_eq!(
            short_path(&item, &sources),
            "Movies \u{203a} Arrival (2016)"
        );
        assert_eq!(
            short_path(&item, &[source(r"C:\Users\me\media\Movies", "")]),
            "Movies \u{203a} Arrival (2016)"
        );
        assert_eq!(short_path(&item, &[]), item.core.path);
    }
}
