//! Batch matching as background jobs.
//!
//! Four modes (spec: *Batch matching modes*):
//!
//! | Mode | Items | What happens to each |
//! |---|---|---|
//! | [`BatchMode::MatchNew`] | movies and shows flagged NEW | identify, then accept or queue for review |
//! | [`BatchMode::MatchUnmatched`] | movies and shows no provider has scraped | identify, then accept or queue for review |
//! | [`BatchMode::MatchSelected`] | the selection, matched or not | identify, then accept or queue for review |
//! | [`BatchMode::RescrapeByIds`] | the selection | refresh by the stored IDs; no search, no review |
//! | [`BatchMode::ForceBestMatch`] | the selection | search, accept the top candidate; no review |
//!
//! Identifying an item is ID-first: an ID the item already has (NFO, folder name, an earlier
//! scrape) is looked up directly and counts as an exact match at 100%. Without one, the title and
//! year are searched, candidates are scored with `cv-match`, and the acceptance rules decide.
//! Local runtime evidence is fetched (provider details of the top candidates) only when the first
//! ranking could not decide, so a clean match costs one search and one details request.
//!
//! # Jobs
//!
//! [`submit_batch`] plans the work, groups it by provider and queues one persistent job per
//! provider on that provider's lane ([`LaneId::Provider`]), so a slow provider delays only its own
//! items. Each item is one job item; a provider error fails that item (grouped by reason in the
//! Activity drawer) and the job goes on. Nothing waits for the user: an uncertain item is written
//! to the review queue and the job moves to the next one. [`resume_match_jobs`] finishes the
//! unfinished items after a restart.
//!
//! # Staying out of the UI's way
//!
//! The worker is async and holds no runtime thread while it waits for a provider. Every database
//! read and write runs on the blocking pool, never on the runtime that serves commands. The
//! responsiveness test (`tests/batch_match_1200.rs`) states the latency bound held while 1,200
//! items are matched.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use cv_db::repo::get_item;
use cv_db::repo::match_targets::{MatchScope, MatchTargetRow, match_targets, match_targets_of};
use cv_db::repo::review::{self, EnqueueOutcome};
use cv_db::repo::step_outcome::StepStatus;
use cv_db::{Database, DbError};
use cv_domain::{Item, ItemId, ItemKind, MediaFileType, ProviderIds};
use cv_jobs::{
    ItemControl, ItemError, ItemWork, JobHandle, JobManager, JobSpec, LaneId, StageError,
    StoreError,
};
use cv_match::{
    Candidate, Decision, LocalEvidence, NormalizedQuery, ReviewReason, ScoredCandidate, decide,
    normalize_query, rank_candidates,
};
use cv_providers::model::{MediaKind, RequestOptions, SearchHit, SearchQuery};
use cv_providers::{
    FallbackChain, MetadataProvider, ProviderError, ProviderRegistry, UNIVERSAL_ID, complete_ids,
};
use serde::{Deserialize, Serialize};

use crate::episode_scrape::scrape_after_show_blocking;

use super::{
    MatchError, MatchSettings, apply_scraped, candidate_from_hit, known_ids, queue_for_review,
    queue_without_key, search, store_completed_ids, with_details,
};

/// The job kind a batch match is stored and resumed under.
pub const MATCH_JOB_KIND: &str = "match.batch";

/// Items of one batch processed at the same time. The provider lane (width 4 by default) bounds
/// it further, and each provider enforces its own rate limit.
pub const DEFAULT_PARALLELISM: usize = 4;

/// How many top candidates get their runtime looked up when the first ranking could not decide.
pub const DETAIL_LOOKUPS: usize = 3;

/// What a batch does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchMode {
    /// Match the items a scan flagged NEW.
    MatchNew,
    /// Match every item no provider has scraped.
    MatchUnmatched,
    /// Identify the user's selection, whether or not it was matched before: the Match selection
    /// action. An ID the item has is looked up directly; otherwise the title is searched.
    MatchSelected,
    /// Refresh the selection by its stored IDs, without searching.
    RescrapeByIds,
    /// Accept the top candidate for the selection, with no review.
    ForceBestMatch,
}

impl BatchMode {
    /// The name shown in the Activity drawer.
    pub const fn title(self) -> &'static str {
        match self {
            Self::MatchNew => "Scraping new titles",
            Self::MatchUnmatched => "Scraping unmatched titles",
            Self::MatchSelected => "Scraping the selected titles",
            Self::RescrapeByIds => "Scraping the selected titles again by their IDs",
            Self::ForceBestMatch => "Using the best match for the selected titles",
        }
    }

    /// Whether the mode works on the user's selection (the others scan the library).
    pub const fn needs_selection(self) -> bool {
        matches!(
            self,
            Self::MatchSelected | Self::RescrapeByIds | Self::ForceBestMatch
        )
    }
}

/// One item of a batch, as saved in the job store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchTarget {
    /// The library item.
    pub item: i64,
    pub mode: BatchMode,
    /// `Movie` or `Show`.
    pub kind: ItemKind,
    /// The provider whose lane the item runs on and that identifies or scrapes it.
    pub provider: String,
    /// Shown as the current item.
    pub label: String,
}

impl std::fmt::Display for MatchTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

/// The metadata providers a batch may use.
#[derive(Clone, Default)]
pub struct MatchProviders {
    list: Vec<Arc<dyn MetadataProvider>>,
    /// The provider the user chose for movies, when it is ready.
    preferred_movie: Option<String>,
    /// The provider the user chose for TV shows, when it is ready.
    preferred_show: Option<String>,
    /// The providers tried, in order, when the one a title runs with finds nothing.
    fallback: FallbackChain,
}

impl MatchProviders {
    /// Providers in order of preference: the first that supports a kind is its default.
    pub fn new(list: Vec<Arc<dyn MetadataProvider>>) -> Self {
        Self {
            list,
            ..Self::default()
        }
    }

    /// Sets the provider chosen for each kind. A choice that is not in the list, or does not
    /// support the kind, is ignored, so the default is still the first ready provider.
    #[must_use]
    pub fn with_preferred(mut self, movie: Option<&str>, show: Option<&str>) -> Self {
        self.preferred_movie = movie.map(str::to_owned);
        self.preferred_show = show.map(str::to_owned);
        self
    }

    /// Sets the fallback chain: what is tried when the provider of a title finds nothing.
    #[must_use]
    pub fn with_fallback(mut self, chain: FallbackChain) -> Self {
        self.fallback = chain;
        self
    }

    /// The fallback chain (empty when fallback is off).
    pub fn fallback(&self) -> &FallbackChain {
        &self.fallback
    }

    /// The metadata providers of the registry (see `ProviderRegistry::register_metadata`).
    pub fn from_registry(registry: &ProviderRegistry) -> Self {
        Self::new(registry.metadata_providers().to_vec())
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn get(&self, id: &str) -> Option<&Arc<dyn MetadataProvider>> {
        self.list.iter().find(|p| p.id() == id)
    }

    /// Every provider, in order of preference.
    pub fn all(&self) -> &[Arc<dyn MetadataProvider>] {
        &self.list
    }

    /// The providers that support a kind of title, in order of preference.
    pub fn supporting(&self, kind: MediaKind) -> Vec<&Arc<dyn MetadataProvider>> {
        self.list
            .iter()
            .filter(|p| {
                let caps = p.capabilities();
                match kind {
                    MediaKind::Movie => caps.movies,
                    MediaKind::Show => caps.shows,
                }
            })
            .collect()
    }

    /// The default provider for a kind of title: the one the user chose in Providers when it is
    /// ready and supports the kind, else the first ready provider that does.
    pub fn default_for(&self, kind: MediaKind) -> Option<&Arc<dyn MetadataProvider>> {
        let supports = |p: &Arc<dyn MetadataProvider>| {
            let caps = p.capabilities();
            match kind {
                MediaKind::Movie => caps.movies,
                MediaKind::Show => caps.shows,
            }
        };
        let chosen = match kind {
            MediaKind::Movie => self.preferred_movie.as_deref(),
            MediaKind::Show => self.preferred_show.as_deref(),
        };
        chosen
            .and_then(|id| self.get(id))
            .filter(|p| supports(p))
            .or_else(|| self.list.iter().find(|p| supports(p)))
    }

    /// Every provider that can be asked for IDs, in order of preference, as plain references.
    /// The universal provider is left out: it is built from the others.
    pub fn refs(&self) -> Vec<&dyn MetadataProvider> {
        self.list
            .iter()
            .filter(|p| p.id() != UNIVERSAL_ID)
            .map(|p| p.as_ref())
            .collect()
    }

    /// Every provider except `id`, for completing the IDs of a title that `id` just scraped:
    /// asking the same provider again would repeat the request that produced the scrape.
    fn others(&self, id: &str) -> Vec<&dyn MetadataProvider> {
        self.list
            .iter()
            .filter(|p| p.id() != id && p.id() != UNIVERSAL_ID)
            .map(|p| p.as_ref())
            .collect()
    }
}

/// What happens with an item once a scrape has been written into it: the app downloads its
/// missing artwork. The hook must return at once and do its work in the background; it is called
/// from the match worker.
pub type AfterMatch = Arc<dyn Fn(ItemId) + Send + Sync>;

/// Everything a batch needs besides the items.
#[derive(Clone)]
pub struct MatchContext {
    pub db: Arc<Database>,
    pub providers: MatchProviders,
    pub settings: MatchSettings,
    /// Called for each item a scrape was written into. `None` does nothing.
    pub after_match: Option<AfterMatch>,
}

/// Why a batch could not be planned or queued.
#[derive(Debug, thiserror::Error)]
pub enum BatchError {
    #[error(transparent)]
    Db(#[from] DbError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("{} needs a selection of items", .0.title())]
    SelectionRequired(BatchMode),
    #[error("no metadata provider is available")]
    NoProviders,
}

/// The items of a batch and where they run.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BatchPlan {
    pub targets: Vec<MatchTarget>,
    /// Items no provider can serve (for example a show when only a movie provider exists).
    pub no_provider: Vec<ItemId>,
}

/// Chooses the items of a batch. `selection` is required by the modes that work on the user's
/// selection; for the other two it narrows the library-wide set when given.
///
/// # Errors
/// Fails without a selection for [`BatchMode::MatchSelected`], [`BatchMode::RescrapeByIds`] and
/// [`BatchMode::ForceBestMatch`],
/// when no provider exists, and on database errors.
pub fn plan_batch(
    db: &Database,
    providers: &MatchProviders,
    mode: BatchMode,
    selection: Option<&[ItemId]>,
) -> Result<BatchPlan, BatchError> {
    if providers.is_empty() {
        return Err(BatchError::NoProviders);
    }
    let rows = match (mode, selection) {
        (_, None) if mode.needs_selection() => {
            return Err(BatchError::SelectionRequired(mode));
        }
        (_, Some(ids)) if mode.needs_selection() => db.read(|c| match_targets_of(c, ids))?,
        (_, picked) => {
            let scope = if mode == BatchMode::MatchNew {
                MatchScope::New
            } else {
                MatchScope::Unmatched
            };
            let mut rows = db.read(|c| match_targets(c, scope))?;
            if let Some(ids) = picked {
                let keep: HashSet<ItemId> = ids.iter().copied().collect();
                rows.retain(|r| keep.contains(&r.id));
            }
            rows
        }
    };
    let mut plan = BatchPlan::default();
    for row in rows {
        match provider_for(providers, mode, &row) {
            Some(provider) => plan.targets.push(MatchTarget {
                item: row.id.0,
                mode,
                kind: row.kind,
                provider,
                label: label_of(&row),
            }),
            None => plan.no_provider.push(row.id),
        }
    }
    Ok(plan)
}

fn media_kind_of(kind: ItemKind) -> Option<MediaKind> {
    match kind {
        ItemKind::Movie => Some(MediaKind::Movie),
        ItemKind::Show => Some(MediaKind::Show),
        _ => None,
    }
}

/// The provider an item runs with. A rescrape refreshes from the provider that scraped the item,
/// when this build still has it; everything else uses the default for the kind.
fn provider_for(
    providers: &MatchProviders,
    mode: BatchMode,
    row: &MatchTargetRow,
) -> Option<String> {
    let media = media_kind_of(row.kind)?;
    if mode == BatchMode::RescrapeByIds
        && let Some(last) = providers.get(&row.last_scraper_id)
    {
        let caps = last.capabilities();
        let serves = match media {
            MediaKind::Movie => caps.movies,
            MediaKind::Show => caps.shows,
        };
        if serves {
            return Some(last.id().to_owned());
        }
    }
    providers.default_for(media).map(|p| p.id().to_owned())
}

fn label_of(row: &MatchTargetRow) -> String {
    let title = if row.title.trim().is_empty() {
        folder_name(&row.path)
    } else {
        row.title.trim().to_owned()
    };
    if row.year > 0 {
        format!("{title} ({})", row.year)
    } else {
        title
    }
}

fn folder_name(path: &str) -> String {
    path.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .to_owned()
}

/// What a batch did so far.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BatchStats {
    /// Items matched and scraped (automatic accept, forced best match, ID-first).
    pub accepted: u64,
    /// Of those, items identified by an ID they already had, with no text search.
    pub by_id: u64,
    /// Items refreshed by their stored IDs.
    pub rescraped: u64,
    /// Items written to the Needs review inbox.
    pub queued: u64,
    /// Items skipped because the user ignored them earlier.
    pub ignored: u64,
    /// Items skipped because they, or the scrape, are locked.
    pub locked: u64,
    /// Provider requests that searched by text.
    pub searches: u64,
}

impl BatchStats {
    /// `198 scraped, 2 wait in Needs review`; `None` when nothing was matched or queued.
    pub fn result(&self) -> Option<String> {
        let scraped = self.accepted + self.rescraped;
        let mut parts = Vec::new();
        if scraped > 0 {
            parts.push(format!("{scraped} scraped"));
        }
        if self.queued > 0 {
            parts.push(format!("{} wait in Needs review", self.queued));
        }
        (!parts.is_empty()).then(|| parts.join(", "))
    }

    /// `2 matched, 6 wait in Needs review`; `None` when nothing was matched or queued.
    pub fn outcome(&self) -> Option<String> {
        let matched = self.accepted + self.rescraped;
        let mut parts = Vec::new();
        if matched > 0 {
            parts.push(format!("{matched} matched"));
        }
        if self.queued > 0 {
            let titles = if self.queued == 1 {
                "title waits"
            } else {
                "titles wait"
            };
            parts.push(format!("{} {titles} in Needs review", self.queued));
        }
        (!parts.is_empty()).then(|| parts.join(", "))
    }
}

#[derive(Default)]
struct Counters {
    accepted: AtomicU64,
    by_id: AtomicU64,
    rescraped: AtomicU64,
    queued: AtomicU64,
    ignored: AtomicU64,
    locked: AtomicU64,
    searches: AtomicU64,
}

fn bump(counter: &AtomicU64) {
    counter.fetch_add(1, Ordering::Relaxed);
}

/// What happened to one item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Done {
    Accepted { by_id: bool },
    Rescraped,
    Queued,
    Ignored,
    Locked,
}

/// Why one item did not finish.
#[derive(Debug)]
enum RunError {
    Cancelled,
    /// Force best match found no candidate at all.
    NoCandidates,
    Failed(String),
}

impl From<MatchError> for RunError {
    fn from(error: MatchError) -> Self {
        match error {
            MatchError::Provider(e) => Self::from(e),
            MatchError::ItemNotFound(_) => Self::Failed("item no longer exists".to_owned()),
            MatchError::UnsupportedKind(_) => {
                Self::Failed("only movies and shows are matched".to_owned())
            }
            MatchError::NoProvider(_) => Self::Failed("provider not available".to_owned()),
            MatchError::Db(e) => Self::from(e),
        }
    }
}

impl From<DbError> for RunError {
    fn from(error: DbError) -> Self {
        tracing::warn!(%error, "database error while matching");
        Self::Failed("database error".to_owned())
    }
}

impl From<ProviderError> for RunError {
    fn from(error: ProviderError) -> Self {
        tracing::debug!(%error, "provider error while matching");
        let reason = match error.root() {
            ProviderError::RateLimited { .. } => "provider rate limit",
            ProviderError::NotFound(_) => "not found at provider",
            ProviderError::Auth { .. } | ProviderError::MissingCredentials { .. } => {
                "provider credentials"
            }
            ProviderError::Request { .. } | ProviderError::FormatChanged(_) => {
                "provider unreachable"
            }
            _ => "provider error",
        };
        Self::Failed(reason.to_owned())
    }
}

type RunResult<T> = Result<T, RunError>;

/// Runs database work on the blocking pool, so the runtime that serves the UI never waits on
/// SQLite.
async fn blocking<T, E, F>(db: &Arc<Database>, work: F) -> RunResult<T>
where
    T: Send + 'static,
    E: Into<RunError> + Send + 'static,
    F: FnOnce(&Database) -> Result<T, E> + Send + 'static,
{
    let db = Arc::clone(db);
    match tokio::task::spawn_blocking(move || work(&db)).await {
        Ok(result) => result.map_err(Into::into),
        Err(error) => {
            tracing::error!(%error, "database worker stopped");
            Err(RunError::Failed("database error".to_owned()))
        }
    }
}

/// What the matcher knows about an item before it asks a provider.
struct Subject {
    kind: ItemKind,
    title: String,
    year: u16,
    ids: ProviderIds,
    locked: bool,
    dismissed: bool,
    evidence: LocalEvidence,
}

fn load_subject(db: &Database, id: ItemId) -> Result<Subject, MatchError> {
    let item: Item = db
        .read(|c| get_item(c, id))?
        .ok_or(MatchError::ItemNotFound(id.0))?;
    let kind = item.kind();
    if !matches!(kind, ItemKind::Movie | ItemKind::Show) {
        return Err(MatchError::UnsupportedKind(kind));
    }
    let dismissed = db.read(|c| review::is_dismissed(c, id))?;
    let core = &item.core;
    let title = if core.title.trim().is_empty() {
        folder_name(&core.path)
    } else {
        core.title.trim().to_owned()
    };
    // The main video: the first one with a known duration.
    let evidence = core
        .media_files
        .iter()
        .find(|f| f.kind == MediaFileType::Video && f.duration_secs > 0)
        .map_or_else(
            || LocalEvidence {
                ids: core.ids.clone(),
                ..LocalEvidence::default()
            },
            |f| LocalEvidence::from_media(f.duration_secs, &f.audio_streams, core.ids.clone()),
        );
    Ok(Subject {
        kind,
        title,
        year: core.year,
        ids: core.ids.clone(),
        locked: core.locked,
        dismissed,
        evidence,
    })
}

/// The worker of a batch.
#[derive(Clone)]
pub struct MatchWork {
    ctx: Arc<MatchContext>,
    counters: Arc<Counters>,
}

impl MatchWork {
    pub fn new(ctx: MatchContext) -> Self {
        Self {
            ctx: Arc::new(ctx),
            counters: Arc::default(),
        }
    }

    /// Counts of what finished so far.
    pub fn stats(&self) -> BatchStats {
        let c = &self.counters;
        BatchStats {
            accepted: c.accepted.load(Ordering::Relaxed),
            by_id: c.by_id.load(Ordering::Relaxed),
            rescraped: c.rescraped.load(Ordering::Relaxed),
            queued: c.queued.load(Ordering::Relaxed),
            ignored: c.ignored.load(Ordering::Relaxed),
            locked: c.locked.load(Ordering::Relaxed),
            searches: c.searches.load(Ordering::Relaxed),
        }
    }

    fn count(&self, done: Done) {
        let c = &self.counters;
        match done {
            Done::Accepted { by_id } => {
                bump(&c.accepted);
                if by_id {
                    bump(&c.by_id);
                }
            }
            Done::Rescraped => bump(&c.rescraped),
            Done::Queued => bump(&c.queued),
            Done::Ignored => bump(&c.ignored),
            Done::Locked => bump(&c.locked),
        }
    }

    async fn run(&self, target: &MatchTarget, control: &ItemControl) -> RunResult<Done> {
        let ctx = &self.ctx;
        let item_id = ItemId(target.item);
        let subject = blocking(&ctx.db, move |db| load_subject(db, item_id)).await?;
        let Some(media) = media_kind_of(subject.kind) else {
            return Err(RunError::Failed(
                "only movies and shows are matched".to_owned(),
            ));
        };
        if subject.locked {
            return Ok(Done::Locked);
        }
        if subject.dismissed && !target.mode.needs_selection() {
            return Ok(Done::Ignored);
        }
        let provider = ctx
            .providers
            .get(&target.provider)
            .or_else(|| ctx.providers.default_for(media))
            .cloned()
            .ok_or_else(|| RunError::Failed("provider not available".to_owned()))?;
        check(control)?;

        let mut request = ctx.settings.request.clone();
        if target.mode == BatchMode::RescrapeByIds {
            // A rescrape asks for fresh data.
            request.bypass_cache = true;
        }

        match target.mode {
            BatchMode::RescrapeByIds => {
                self.rescrape(item_id, &subject, media, &provider, &request, control)
                    .await
            }
            mode => {
                self.identify(item_id, mode, &subject, media, &provider, &request, control)
                    .await
            }
        }
    }

    /// Refreshes the item by the IDs it has: no search, no review.
    async fn rescrape(
        &self,
        item_id: ItemId,
        subject: &Subject,
        media: MediaKind,
        provider: &Arc<dyn MetadataProvider>,
        request: &RequestOptions,
        control: &ItemControl,
    ) -> RunResult<Done> {
        let usable = provider
            .capabilities()
            .lookup_ids
            .iter()
            .any(|key| subject.ids.contains(key));
        if !usable {
            return Err(RunError::Failed("no stored ID".to_owned()));
        }
        let scraped = provider.details(media, &subject.ids, request).await?;
        check(control)?;
        let done = self
            .store_scrape(item_id, subject.kind, media, provider, scraped, request)
            .await?;
        Ok(match done {
            Some(()) => Done::Rescraped,
            None => Done::Locked,
        })
    }

    /// Identifies the item and either scrapes it or queues it for review.
    #[allow(clippy::too_many_arguments)]
    async fn identify(
        &self,
        item_id: ItemId,
        mode: BatchMode,
        subject: &Subject,
        media: MediaKind,
        provider: &Arc<dyn MetadataProvider>,
        request: &RequestOptions,
        control: &ItemControl,
    ) -> RunResult<Done> {
        let ctx = &self.ctx;
        let mut query = normalize_query(&subject.title);
        if query.year.is_none() && subject.year > 0 {
            query.year = Some(subject.year);
        }

        let found = self
            .find_hits(provider, media, &query, subject, request)
            .await;
        let (mut hits, by_id) = match found {
            Ok(found) => found,
            // The provider has no key: the title waits in Needs review with that reason, and is
            // scraped from there once the key is set.
            Err(error) => {
                if let ProviderError::MissingCredentials { provider: name, .. } = error.root() {
                    let name = name.clone();
                    let outcome =
                        blocking(&ctx.db, move |db| queue_without_key(db, item_id, &name)).await?;
                    return Ok(match outcome {
                        EnqueueOutcome::Dismissed => Done::Ignored,
                        EnqueueOutcome::Queued(_) | EnqueueOutcome::Refreshed(_) => Done::Queued,
                    });
                }
                return Err(error.into());
            }
        };
        check(control)?;
        // The provider found nothing: the fallback chain may know the title. The provider that
        // answers takes over the rest of the item (its details, and the last scraper recorded).
        let mut provider = Arc::clone(provider);
        if hits.is_empty()
            && query.id.is_none()
            && let Some((fallback, found)) = ctx
                .providers
                .fallback()
                .search_after(
                    provider.id(),
                    &SearchQuery {
                        kind: media,
                        title: query.text.clone(),
                        year: query.year,
                    },
                    request,
                )
                .await
        {
            self.counters.searches.fetch_add(1, Ordering::Relaxed);
            tracing::debug!(
                from = provider.id(),
                to = fallback.id(),
                "the fallback provider found the title"
            );
            provider = fallback;
            hits = found.hits;
            check(control)?;
        }
        let provider = &provider;
        let mut ranked = rank(&query, &hits, subject);

        if mode == BatchMode::ForceBestMatch {
            let Some(best) = ranked.first() else {
                return Err(RunError::NoCandidates);
            };
            return self
                .accept(item_id, subject, media, provider, best, by_id, request)
                .await;
        }

        let mut decision = decide(&ranked, &ctx.settings.rules);
        if let Decision::Review(reason) = decision
            && reason != ReviewReason::NoCandidates
            && subject.evidence.runtime_minutes.is_some()
            && !by_id
        {
            // The titles could not decide it. The runtime of the best candidates can.
            ranked = self
                .rank_with_runtimes(&query, ranked, subject, media, provider, request, control)
                .await?;
            decision = decide(&ranked, &ctx.settings.rules);
        }
        check(control)?;
        // Without a year in the name, a weak or tied match says so: the year is what is missing.
        if let Decision::Review(reason) = decision
            && query.year.is_none()
            && !by_id
        {
            let best = ranked.first().map_or(0.0, |c| c.breakdown.score);
            decision = Decision::Review(reason.without_year(best));
        }
        match decision {
            Decision::Accept { .. } => {
                self.accept(
                    item_id, subject, media, provider, &ranked[0], by_id, request,
                )
                .await
            }
            Decision::Review(reason) => {
                let outcome = blocking(&ctx.db, move |db| {
                    queue_for_review(db, item_id, reason, &ranked)
                })
                .await?;
                Ok(match outcome {
                    EnqueueOutcome::Dismissed => Done::Ignored,
                    EnqueueOutcome::Queued(_) | EnqueueOutcome::Refreshed(_) => Done::Queued,
                })
            }
        }
    }

    /// The candidates for the item. An ID the item already has is looked up directly, with no
    /// text search; the hit is an exact match. Returns whether that happened.
    async fn find_hits(
        &self,
        provider: &Arc<dyn MetadataProvider>,
        media: MediaKind,
        query: &NormalizedQuery,
        subject: &Subject,
        request: &RequestOptions,
    ) -> Result<(Vec<SearchHit>, bool), ProviderError> {
        let found =
            search::find_hits(provider.as_ref(), media, query, &subject.ids, request).await?;
        self.counters
            .searches
            .fetch_add(found.searches, Ordering::Relaxed);
        Ok((found.hits, found.by_id))
    }

    /// Re-ranks `ranked` after filling the runtime of its best candidates from provider details.
    #[allow(clippy::too_many_arguments)]
    async fn rank_with_runtimes(
        &self,
        query: &NormalizedQuery,
        ranked: Vec<ScoredCandidate>,
        subject: &Subject,
        media: MediaKind,
        provider: &Arc<dyn MetadataProvider>,
        request: &RequestOptions,
        control: &ItemControl,
    ) -> RunResult<Vec<ScoredCandidate>> {
        let mut candidates: Vec<Candidate> = ranked.into_iter().map(|s| s.candidate).collect();
        for candidate in candidates.iter_mut().take(DETAIL_LOOKUPS) {
            check(control)?;
            match provider.details(media, &candidate.ids, request).await {
                Ok(details) => *candidate = with_details(candidate.clone(), &details),
                // A candidate whose details fail keeps its score; the others still count.
                Err(error) => tracing::debug!(%error, "candidate details unavailable"),
            }
        }
        Ok(rank_candidates(query, candidates, &subject.evidence))
    }

    /// Scrapes the candidate into the item.
    #[allow(clippy::too_many_arguments)]
    async fn accept(
        &self,
        item_id: ItemId,
        subject: &Subject,
        media: MediaKind,
        provider: &Arc<dyn MetadataProvider>,
        candidate: &ScoredCandidate,
        by_id: bool,
        request: &RequestOptions,
    ) -> RunResult<Done> {
        let scraped = provider
            .details(media, &candidate.candidate.ids, request)
            .await?;
        let stored = self
            .store_scrape(item_id, subject.kind, media, provider, scraped, request)
            .await?;
        Ok(match stored {
            Some(()) => Done::Accepted { by_id },
            None => Done::Locked,
        })
    }

    /// Writes a scrape and completes the item's provider IDs. `None` when the scrape was blocked
    /// by a lock.
    async fn store_scrape(
        &self,
        item_id: ItemId,
        kind: ItemKind,
        media: MediaKind,
        provider: &Arc<dyn MetadataProvider>,
        scraped: Item,
        request: &RequestOptions,
    ) -> RunResult<Option<()>> {
        let ctx = &self.ctx;
        let mut settings = ctx.settings.clone();
        settings.request = request.clone();
        let settings_for_episodes = settings.clone();
        let provider_id = provider.id().to_owned();
        let outcome = blocking(&ctx.db, move |db| {
            apply_scraped(db, item_id, kind, scraped, &provider_id, &settings)
        })
        .await?;
        if outcome.blocked.is_some() {
            return Ok(None);
        }
        if crate::collections::wanted_after_scrape(&ctx.settings, kind) {
            crate::collections::assign_scraped_movie(
                &ctx.db,
                &ctx.providers.refs(),
                item_id,
                request,
            )
            .await;
        }
        let known = blocking(&ctx.db, move |db| known_ids(db, item_id)).await?;
        let anime = ctx.settings.anime_for(&known).await;
        let done = complete_ids(
            media,
            &known,
            &ctx.providers.others(provider.id()),
            anime.as_deref(),
            request,
        )
        .await;
        if !done.added.is_empty() {
            let ids = done.ids;
            blocking(&ctx.db, move |db| store_completed_ids(db, item_id, ids)).await?;
        }
        if kind == ItemKind::Show {
            scrape_after_show_blocking(&ctx.db, item_id, provider.as_ref(), &settings_for_episodes)
                .await;
        }
        ctx.settings.notify_scraped(item_id);
        if let Some(hook) = &ctx.after_match {
            hook(item_id);
        }
        Ok(Some(()))
    }
}

fn check(control: &ItemControl) -> RunResult<()> {
    if control.is_cancelled() {
        Err(RunError::Cancelled)
    } else {
        Ok(())
    }
}

fn rank(query: &NormalizedQuery, hits: &[SearchHit], subject: &Subject) -> Vec<ScoredCandidate> {
    let candidates = hits.iter().map(candidate_from_hit).collect();
    rank_candidates(query, candidates, &subject.evidence)
}

impl MatchWork {
    /// Stores that the item's scrape was tried and found no match (the provider failed, or force
    /// best match found nothing), so it reads as Not matched rather than not scraped yet. A
    /// title that is already matched keeps its flagless state: the lifecycle reads the miss only
    /// for a title without a provider ID. A failed write is logged.
    async fn record_miss(&self, item: &MatchTarget, status: StepStatus, reason: String) {
        let id = ItemId(item.item);
        let written = blocking(&self.ctx.db, move |db| {
            db.write(move |tx| super::record_scrape(tx, id, status, &reason))
        })
        .await;
        if written.is_err() {
            tracing::debug!(item = id.0, "the failed scrape was not stored");
        }
    }
}

impl ItemWork for MatchWork {
    type Item = MatchTarget;

    async fn process(&self, item: &MatchTarget, control: &ItemControl) -> Result<(), ItemError> {
        if control.is_cancelled() {
            return Err(ItemError::Cancelled);
        }
        match self.run(item, control).await {
            Ok(done) => {
                self.count(done);
                Ok(())
            }
            Err(RunError::Cancelled) => Err(ItemError::Cancelled),
            Err(RunError::NoCandidates) => {
                self.record_miss(item, StepStatus::NotFound, "No candidates found".to_owned())
                    .await;
                Err(StageError::new("no candidates found").into())
            }
            Err(RunError::Failed(reason)) => {
                self.record_miss(
                    item,
                    StepStatus::Failed,
                    format!("The scrape failed: {reason}"),
                )
                .await;
                Err(StageError::new(reason).into())
            }
        }
    }

    /// How many titles were matched and how many wait in Needs review, so a finished batch does
    /// not read as done when its titles went to the inbox.
    fn outcome(&self) -> Option<String> {
        self.stats().outcome()
    }

    /// `198 scraped`, and what waits in Needs review: the step of the batch in the run history.
    fn result(&self) -> Option<String> {
        self.stats().result()
    }
}

/// One queued job of a batch: the items of one provider.
#[derive(Clone)]
pub struct BatchJob {
    pub job: JobHandle,
    /// The provider whose lane the job runs on.
    pub provider: String,
    work: MatchWork,
}

impl BatchJob {
    /// Counts of what finished so far.
    pub fn stats(&self) -> BatchStats {
        self.work.stats()
    }
}

/// A queued batch: one job per provider that has items.
#[derive(Clone, Default)]
pub struct MatchBatch {
    pub jobs: Vec<BatchJob>,
    /// Items planned across all jobs.
    pub planned: usize,
    /// Items no provider can serve; they are not queued.
    pub no_provider: Vec<ItemId>,
}

impl MatchBatch {
    /// Waits for every job, then returns their summed counts.
    pub async fn wait(&self) -> BatchStats {
        let mut total = BatchStats::default();
        for job in &self.jobs {
            job.job.wait().await;
            let s = job.stats();
            total.accepted += s.accepted;
            total.by_id += s.by_id;
            total.rescraped += s.rescraped;
            total.queued += s.queued;
            total.ignored += s.ignored;
            total.locked += s.locked;
            total.searches += s.searches;
        }
        total
    }
}

/// Plans a batch and queues it as persistent background jobs, one per provider, each on its
/// provider's lane. Returns at once; the jobs run on their own. Must be called inside a tokio
/// runtime. The manager needs a job store.
///
/// The planning reads the database on the calling thread; call this from a blocking task when
/// the library is large.
///
/// # Errors
/// See [`plan_batch`]; also fails when the jobs cannot be saved.
pub fn submit_batch(
    manager: &JobManager,
    ctx: &MatchContext,
    mode: BatchMode,
    selection: Option<&[ItemId]>,
    parallelism: usize,
) -> Result<MatchBatch, BatchError> {
    let plan = plan_batch(&ctx.db, &ctx.providers, mode, selection)?;
    let mut by_provider: BTreeMap<String, Vec<MatchTarget>> = BTreeMap::new();
    for target in &plan.targets {
        by_provider
            .entry(target.provider.clone())
            .or_default()
            .push(target.clone());
    }
    let mut batch = MatchBatch {
        planned: plan.targets.len(),
        no_provider: plan.no_provider,
        jobs: Vec::new(),
    };
    for (provider, targets) in by_provider {
        let name = crate::wording::job::scraping_titles(
            targets.len(),
            ctx.providers.get(&provider).map(|p| p.name()),
        );
        let spec = JobSpec::new(name, LaneId::provider(provider.clone())).concurrency(parallelism);
        let work = MatchWork::new(ctx.clone());
        let job = manager.submit_persistent(MATCH_JOB_KIND, spec, work.clone(), targets)?;
        batch.jobs.push(BatchJob {
            job,
            provider,
            work,
        });
    }
    Ok(batch)
}

/// Starts again the batches that a crash or quit cut short, with only their unfinished items.
///
/// # Errors
/// Fails when the stored jobs cannot be read.
pub fn resume_match_jobs(
    manager: &JobManager,
    ctx: &MatchContext,
) -> Result<Vec<BatchJob>, StoreError> {
    let mut works: Vec<MatchWork> = Vec::new();
    let handles = manager.resume_persisted(MATCH_JOB_KIND, |_stored| {
        let work = MatchWork::new(ctx.clone());
        works.push(work.clone());
        work
    })?;
    Ok(handles
        .into_iter()
        .zip(works)
        .map(|(job, work)| BatchJob {
            provider: match job.snapshot().lane {
                LaneId::Provider(id) => id,
                other => other.to_string(),
            },
            job,
            work,
        })
        .collect())
}

#[cfg(test)]
mod outcome_tests {
    use super::BatchStats;

    #[test]
    fn a_batch_says_how_many_titles_were_matched_and_how_many_wait_for_review() {
        let stats = |accepted, queued| BatchStats {
            accepted,
            queued,
            ..BatchStats::default()
        };
        assert_eq!(
            stats(0, 6).outcome().as_deref(),
            Some("6 titles wait in Needs review")
        );
        assert_eq!(
            stats(2, 1).outcome().as_deref(),
            Some("2 matched, 1 title waits in Needs review")
        );
        assert_eq!(stats(3, 0).outcome().as_deref(), Some("3 matched"));
        assert_eq!(stats(0, 0).outcome(), None);
    }

    #[test]
    fn a_batch_reads_in_the_run_history_as_what_it_scraped() {
        let stats = |accepted, queued| BatchStats {
            accepted,
            queued,
            ..BatchStats::default()
        };
        assert_eq!(stats(198, 0).result().as_deref(), Some("198 scraped"));
        assert_eq!(
            stats(198, 14).result().as_deref(),
            Some("198 scraped, 14 wait in Needs review")
        );
        assert_eq!(stats(0, 0).result(), None);
    }
}
