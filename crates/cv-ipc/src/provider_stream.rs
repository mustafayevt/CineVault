//! Generic provider streaming: every remote search streams its results provider by
//! provider, and nothing waits for the slowest one.
//!
//! A flow (artwork candidates, scrape candidates) supplies
//! one [`SearchProvider`] per provider and a [`Sink`]. The
//! [`SearchRegistry`] runs each provider as its own task on its network lane, so lanes and rate
//! limits stay in charge, and keeps the search under its `search_id` so it can be cancelled or
//! one provider can be asked again:
//!
//! * `Started{provider}` for every provider when the search begins;
//! * then, as each provider answers, `Results{provider, items}`, `Failed{provider, reason,
//!   retryable}` or `NeedsKey{provider}`;
//! * a provider with a lot to hand over may send some of it earlier: `Partial{provider, items}`
//!   is more of its answer while it is still working, and `Update{provider, items}` sends items
//!   that were sent before again, improved (a poster that has arrived). Its last `Results`
//!   carries the rest and ends its part of the search, as before;
//! * `Done` once every provider has answered.
//!
//! [`cancel_search`] stops a search: providers that have not started their request never do, and
//! one that is waiting for its answer is dropped. A cancel can arrive before its search starts
//! (the start command plans first, the cancel command does not): the id is remembered, and the
//! start that comes later asks no provider at all. [`retry_provider`] asks one provider again and
//! leaves the others alone: it sends `Started`, the answer and `Done` again.
//!
//! A rejected key is a `Failed` event whose reason is [`KEY_REJECTED`] and that is not retryable.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use async_trait::async_trait;
use cv_app::services::AppServices;
use cv_jobs::{LaneId, Lanes};
use serde::Serialize;
use specta::Type;
use tokio::sync::Notify;

/// The `reason` of a failed event that means the provider rejected the saved key.
pub const KEY_REJECTED: &str = "key-rejected";

/// Searches kept for retry; the oldest are forgotten beyond this many.
const KEEP: usize = 64;

/// Ids of searches cancelled before they started, kept so their start asks nobody; the oldest are
/// forgotten beyond this many.
const KEEP_CANCELLED: usize = 256;

/// What a search sends over its Channel, one event at a time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ProviderEvent<T> {
    /// The provider was asked.
    Started { provider: String },
    /// The provider answered with these items (possibly none). Ends its part of the search.
    Results { provider: String, items: Vec<T> },
    /// The provider has these items so far and is still working; more follow.
    Partial { provider: String, items: Vec<T> },
    /// Items sent before, now improved (an image that has arrived). Adds nothing to the count.
    Update { provider: String, items: Vec<T> },
    /// The provider could not answer. `retryable` says whether asking again can help.
    Failed {
        provider: String,
        reason: String,
        retryable: bool,
    },
    /// The provider needs a key the user has not set.
    NeedsKey { provider: String },
    /// Every provider has answered.
    Done,
}

/// What one provider's request came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderOutcome<T> {
    Results(Vec<T>),
    Failed { reason: String, retryable: bool },
    NeedsKey,
}

impl<T> ProviderOutcome<T> {
    /// A provider that answered with these items.
    pub fn results(items: Vec<T>) -> Self {
        Self::Results(items)
    }

    /// A provider that failed for this reason; a timeout or a rate limit is retryable.
    pub fn failed(reason: impl Into<String>, retryable: bool) -> Self {
        Self::Failed {
            reason: reason.into(),
            retryable,
        }
    }

    /// A provider whose key the server rejected.
    pub fn key_rejected() -> Self {
        Self::failed(KEY_REJECTED, false)
    }
}

/// Tells a running request to stop. Cheap to clone; all clones share the state.
#[derive(Debug, Clone, Default)]
pub struct CancelToken {
    inner: Arc<CancelState>,
}

#[derive(Debug, Default)]
struct CancelState {
    cancelled: AtomicBool,
    wake: Notify,
}

impl CancelToken {
    pub fn cancel(&self) {
        self.inner.cancelled.store(true, Ordering::SeqCst);
        self.inner.wake.notify_waiters();
    }

    /// Whether the search was cancelled. A provider that makes several requests checks this
    /// between them.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::SeqCst)
    }

    /// Completes when the search is cancelled.
    pub async fn cancelled(&self) {
        loop {
            let notified = self.inner.wake.notified();
            if self.is_cancelled() {
                return;
            }
            notified.await;
        }
    }
}

/// One provider of a search.
#[async_trait]
pub trait SearchProvider<T: Send + 'static>: Send + Sync + 'static {
    /// The provider's id; it names the network lane the request runs on.
    fn id(&self) -> &str;

    /// The lane the request runs on: the provider's own network lane unless the work is CPU work
    /// (frame grabs), which runs on the CPU lane.
    fn lane(&self) -> LaneId {
        LaneId::provider(self.id())
    }

    /// Asks the provider. Called once per start and once per retry, on the provider's lane. A
    /// request that makes several calls checks `cancel` between them.
    async fn search(&self, cancel: &CancelToken) -> ProviderOutcome<T>;

    /// Like [`search`](Self::search), for a request that can hand over part of its answer early
    /// through `progress`; whatever it returns is the rest. The default hands over nothing early.
    async fn search_progressive(
        &self,
        cancel: &CancelToken,
        progress: &Progress<T>,
    ) -> ProviderOutcome<T> {
        let _ = progress;
        self.search(cancel).await
    }
}

/// How a provider hands over part of its answer before it has the whole of it. Nothing is sent
/// once the search is cancelled.
pub struct Progress<T: Send + 'static> {
    search: Arc<Search<T>>,
    provider: String,
}

impl<T: Send + 'static> Progress<T> {
    /// Sends these items now; the provider goes on working.
    pub fn partial(&self, items: Vec<T>) {
        self.search.emit(ProviderEvent::Partial {
            provider: self.provider.clone(),
            items,
        });
    }

    /// Sends items that were sent before again, improved.
    pub fn update(&self, items: Vec<T>) {
        self.search.emit(ProviderEvent::Update {
            provider: self.provider.clone(),
            items,
        });
    }
}

/// Where a search's events go.
pub type Sink<T> = Arc<dyn Fn(ProviderEvent<T>) + Send + Sync>;

/// How a retry request was answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryOutcome {
    /// The provider is being asked again.
    Started,
    /// No search has this id (it was cancelled, or never started).
    UnknownSearch,
    /// The search has no such provider.
    UnknownProvider,
    /// The provider is still answering its earlier request.
    StillRunning,
    /// The search was cancelled.
    Cancelled,
}

/// What the registry can do to any search, whatever its item type.
trait Control: Send + Sync {
    fn cancel(&self);
    fn retry(self: Arc<Self>, provider: &str) -> RetryOutcome;
}

struct Search<T> {
    lanes: Lanes,
    providers: Vec<Arc<dyn SearchProvider<T>>>,
    sink: Sink<T>,
    token: CancelToken,
    /// Providers asked and not yet answered, per start or retry batch.
    pending: AtomicUsize,
    running: Mutex<Vec<String>>,
}

impl<T: Send + 'static> Search<T> {
    fn emit(&self, event: ProviderEvent<T>) {
        if !self.token.is_cancelled() {
            (self.sink)(event);
        }
    }

    /// Asks `provider` on its lane. Sends `Started` now and its answer when it comes; the last
    /// provider of the batch to answer sends `Done`.
    fn ask(self: &Arc<Self>, provider: &Arc<dyn SearchProvider<T>>) {
        let id = provider.id().to_owned();
        self.running
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(id.clone());
        self.emit(ProviderEvent::Started {
            provider: id.clone(),
        });
        let (search, provider) = (Arc::clone(self), Arc::clone(provider));
        let lane = provider.lane();
        // The task waits for a worker of the provider's lane; a cancel before it starts means no
        // request is ever made.
        drop(self.lanes.spawn(&lane, async move {
            // Counts the provider as answered even when its request panics, so `Done` still comes.
            let _answered = Answered {
                search: Arc::clone(&search),
                id: id.clone(),
            };
            if !search.token.is_cancelled() {
                let progress = Progress {
                    search: Arc::clone(&search),
                    provider: id.clone(),
                };
                let answer = tokio::select! {
                    answer = provider.search_progressive(&search.token, &progress) => Some(answer),
                    () = search.token.cancelled() => None,
                };
                if let Some(answer) = answer {
                    search.emit(match answer {
                        ProviderOutcome::Results(items) => ProviderEvent::Results {
                            provider: id.clone(),
                            items,
                        },
                        ProviderOutcome::Failed { reason, retryable } => ProviderEvent::Failed {
                            provider: id.clone(),
                            reason,
                            retryable,
                        },
                        ProviderOutcome::NeedsKey => ProviderEvent::NeedsKey {
                            provider: id.clone(),
                        },
                    });
                }
            }
        }));
    }
}

/// Marks one provider of a search as answered when it is dropped: at the end of its task, or when
/// the task unwinds from a panic. The last provider of a batch to answer sends `Done`.
struct Answered<T: Send + 'static> {
    search: Arc<Search<T>>,
    id: String,
}

impl<T: Send + 'static> Drop for Answered<T> {
    fn drop(&mut self) {
        self.search
            .running
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|running| *running != self.id);
        if self.search.pending.fetch_sub(1, Ordering::SeqCst) == 1 {
            self.search.emit(ProviderEvent::Done);
        }
    }
}

impl<T: Send + 'static> Control for Search<T> {
    fn cancel(&self) {
        self.token.cancel();
    }

    fn retry(self: Arc<Self>, provider: &str) -> RetryOutcome {
        if self.token.is_cancelled() {
            return RetryOutcome::Cancelled;
        }
        let Some(found) = self.providers.iter().find(|p| p.id() == provider).cloned() else {
            return RetryOutcome::UnknownProvider;
        };
        if self
            .running
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .any(|running| running == provider)
        {
            return RetryOutcome::StillRunning;
        }
        self.pending.fetch_add(1, Ordering::SeqCst);
        self.ask(&found);
        RetryOutcome::Started
    }
}

/// The running and recent searches by `search_id`.
#[derive(Default)]
pub struct SearchRegistry {
    searches: Mutex<Vec<(String, Arc<dyn Control>)>>,
    /// Ids cancelled before their search started (see [`Self::cancel`]).
    cancelled: Mutex<VecDeque<String>>,
}

impl SearchRegistry {
    /// Starts a search: every provider is asked at once on its own lane, and `sink` receives the
    /// events. A search with the same id is cancelled and replaced. A search whose id was
    /// cancelled before this start asks no provider and sends nothing: nobody listens any more.
    /// Must be called inside a tokio runtime.
    pub fn start<T: Send + 'static>(
        &self,
        lanes: &Lanes,
        search_id: &str,
        providers: Vec<Arc<dyn SearchProvider<T>>>,
        sink: Sink<T>,
    ) {
        let search = Arc::new(Search {
            lanes: lanes.clone(),
            sink,
            token: CancelToken::default(),
            pending: AtomicUsize::new(providers.len()),
            running: Mutex::new(Vec::new()),
            providers,
        });
        {
            let mut searches = self.searches.lock().unwrap_or_else(PoisonError::into_inner);
            // Checked under the searches lock, like `cancel` adds it, so the two cannot cross.
            {
                let mut cancelled = self
                    .cancelled
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                if let Some(at) = cancelled.iter().position(|id| id == search_id) {
                    cancelled.remove(at);
                    tracing::debug!(search_id, "a search cancelled before it started is not run");
                    return;
                }
            }
            if let Some(at) = searches.iter().position(|(id, _)| id == search_id) {
                searches.remove(at).1.cancel();
            }
            searches.push((
                search_id.to_owned(),
                Arc::clone(&search) as Arc<dyn Control>,
            ));
            while searches.len() > KEEP {
                searches.remove(0).1.cancel();
            }
        }
        if search.providers.is_empty() {
            search.emit(ProviderEvent::Done);
            return;
        }
        for provider in search.providers.clone() {
            search.ask(&provider);
        }
    }

    /// Stops a search and forgets it. Returns whether there was one. An id with no search yet is
    /// remembered, so a start that arrives after its cancel asks nobody.
    pub fn cancel(&self, search_id: &str) -> bool {
        let found = {
            let mut searches = self.searches.lock().unwrap_or_else(PoisonError::into_inner);
            let found = searches
                .iter()
                .position(|(id, _)| id == search_id)
                .map(|at| searches.remove(at).1);
            if found.is_none() {
                // Under the searches lock, so a start cannot slip in between the check and this.
                let mut cancelled = self
                    .cancelled
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                if !cancelled.iter().any(|id| id == search_id) {
                    cancelled.push_back(search_id.to_owned());
                    while cancelled.len() > KEEP_CANCELLED {
                        cancelled.pop_front();
                    }
                }
            }
            found
        };
        found.map(|search| search.cancel()).is_some()
    }

    /// Asks one provider of a search again; the others are left alone.
    pub fn retry(&self, search_id: &str, provider: &str) -> RetryOutcome {
        let found = self
            .searches
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .find(|(id, _)| id == search_id)
            .map(|(_, search)| Arc::clone(search));
        match found {
            Some(search) => search.retry(provider),
            None => RetryOutcome::UnknownSearch,
        }
    }

    /// How many searches are kept.
    #[cfg(test)]
    #[must_use]
    pub fn len(&self) -> usize {
        self.searches
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    #[cfg(test)]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The searches of the app. The streaming commands of each flow start their searches here.
pub struct SearchState {
    registry: SearchRegistry,
    lanes: Option<Lanes>,
}

impl SearchState {
    /// Searches on the lanes of the app's job manager (the network lanes of its providers).
    pub fn from_services(services: Option<&Arc<AppServices>>) -> Self {
        Self {
            registry: SearchRegistry::default(),
            lanes: services.map(|services| services.jobs().lanes().clone()),
        }
    }

    /// Searches on these lanes (tests, and a surface that brings its own).
    pub fn with_lanes(lanes: Option<Lanes>) -> Self {
        Self {
            registry: SearchRegistry::default(),
            lanes,
        }
    }

    /// Starts a search that sends its events to `sink`. Fails when the library could not be
    /// opened and so no lanes exist.
    pub fn start_on_sink<T: Send + 'static>(
        &self,
        search_id: &str,
        providers: Vec<Arc<dyn SearchProvider<T>>>,
        sink: Sink<T>,
    ) -> Result<(), SearchCommandError> {
        self.start_on_sink_finishing(search_id, providers, sink, None)
    }

    /// As [`Self::start_on_sink`]; `finish` is asked each time every provider has answered
    /// (after the first start and after a retry), and an event it returns is sent just before
    /// `Done`. The scrape search uses it to say why nothing was found.
    pub fn start_on_sink_finishing<T: Send + 'static>(
        &self,
        search_id: &str,
        providers: Vec<Arc<dyn SearchProvider<T>>>,
        sink: Sink<T>,
        finish: Option<Finish<T>>,
    ) -> Result<(), SearchCommandError> {
        let lanes = self.lanes.as_ref().ok_or_else(|| SearchCommandError {
            code: "unavailable".to_owned(),
            message: "CineVault could not open the library, so nothing can be searched.".to_owned(),
        })?;
        self.registry
            .start(lanes, search_id, providers, finishing(sink, finish));
        Ok(())
    }
}

/// What a search may add just before its `Done`.
pub type Finish<T> = Arc<dyn Fn() -> Option<ProviderEvent<T>> + Send + Sync>;

/// A sink that sends the event `finish` returns just before every `Done`.
pub fn finishing<T: Send + 'static>(sink: Sink<T>, finish: Option<Finish<T>>) -> Sink<T> {
    let Some(finish) = finish else {
        return sink;
    };
    Arc::new(move |event| {
        if matches!(event, ProviderEvent::Done)
            && let Some(extra) = finish()
        {
            sink(extra);
        }
        sink(event);
    })
}

/// Why a search command did not run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct SearchCommandError {
    /// Stable code: `unavailable`, `unknown_search`, `unknown_provider`, `still_running`.
    pub code: String,
    /// Plain sentence for the user.
    pub message: String,
}

/// Stops a search. Providers that have not started their request never do, and a request that is
/// waiting for its answer is dropped. Cancelling a search that is gone is fine.
pub fn cancel_search(state: &SearchState, search_id: &str) -> bool {
    state.registry.cancel(search_id)
}

/// Asks one provider of a search again; the other providers are left alone. The answer comes on
/// the search's sink: `Started`, the result and `Done`.
pub fn retry_provider(
    state: &SearchState,
    search_id: &str,
    provider: &str,
) -> Result<(), SearchCommandError> {
    match state.registry.retry(search_id, provider) {
        RetryOutcome::Started => Ok(()),
        RetryOutcome::UnknownSearch | RetryOutcome::Cancelled => Err(SearchCommandError {
            code: "unknown_search".to_owned(),
            message: "That search is over. Search again.".to_owned(),
        }),
        RetryOutcome::UnknownProvider => Err(SearchCommandError {
            code: "unknown_provider".to_owned(),
            message: "That search does not ask this provider.".to_owned(),
        }),
        RetryOutcome::StillRunning => Err(SearchCommandError {
            code: "still_running".to_owned(),
            message: "This provider is still answering.".to_owned(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use cv_jobs::LaneConfig;

    use super::*;

    /// How a mock provider answers.
    #[derive(Clone)]
    enum Script {
        /// Answers with these items after `delay`.
        Items(Vec<u32>, Duration),
        /// Fails: the first `fails` requests fail with `reason`, later ones return `then`.
        Flaky {
            fails: usize,
            reason: &'static str,
            then: Vec<u32>,
        },
        Keyless,
        /// Makes `pages` requests of 20ms each, checking the token between them.
        Paged {
            pages: usize,
        },
        /// Hands over `early` batches at once through `progress`, then waits `wait` and answers
        /// with `last`.
        Progressive {
            early: Vec<Vec<u32>>,
            wait: Duration,
            last: Vec<u32>,
        },
    }

    struct Mock {
        id: &'static str,
        script: Script,
        requests: AtomicUsize,
        /// Set when a request in flight was dropped by a cancel.
        dropped: Arc<AtomicBool>,
    }

    impl Mock {
        fn new(id: &'static str, script: Script) -> Arc<Self> {
            Arc::new(Self {
                id,
                script,
                requests: AtomicUsize::new(0),
                dropped: Arc::new(AtomicBool::new(false)),
            })
        }

        fn requests(&self) -> usize {
            self.requests.load(Ordering::SeqCst)
        }
    }

    struct DropFlag(Arc<AtomicBool>, bool);

    impl Drop for DropFlag {
        fn drop(&mut self) {
            if !self.1 {
                self.0.store(true, Ordering::SeqCst);
            }
        }
    }

    #[async_trait]
    impl SearchProvider<u32> for Mock {
        fn id(&self) -> &str {
            self.id
        }

        async fn search(&self, cancel: &CancelToken) -> ProviderOutcome<u32> {
            let attempt = self.requests.fetch_add(1, Ordering::SeqCst);
            let mut finished = DropFlag(Arc::clone(&self.dropped), false);
            let outcome = match &self.script {
                Script::Items(items, delay) => {
                    tokio::time::sleep(*delay).await;
                    ProviderOutcome::results(items.clone())
                }
                Script::Flaky {
                    fails,
                    reason,
                    then,
                } => {
                    if attempt < *fails {
                        ProviderOutcome::failed(*reason, true)
                    } else {
                        ProviderOutcome::results(then.clone())
                    }
                }
                Script::Keyless => ProviderOutcome::NeedsKey,
                Script::Paged { pages } => {
                    for _ in 0..*pages {
                        if cancel.is_cancelled() {
                            break;
                        }
                        self.requests.fetch_add(1, Ordering::SeqCst);
                        tokio::time::sleep(Duration::from_millis(20)).await;
                    }
                    ProviderOutcome::results(Vec::new())
                }
                Script::Progressive { .. } => self.search_progressive_script(cancel, None).await,
            };
            finished.1 = true;
            outcome
        }

        async fn search_progressive(
            &self,
            cancel: &CancelToken,
            progress: &Progress<u32>,
        ) -> ProviderOutcome<u32> {
            match &self.script {
                Script::Progressive { .. } => {
                    self.search_progressive_script(cancel, Some(progress)).await
                }
                _ => self.search(cancel).await,
            }
        }
    }

    impl Mock {
        async fn search_progressive_script(
            &self,
            _cancel: &CancelToken,
            progress: Option<&Progress<u32>>,
        ) -> ProviderOutcome<u32> {
            self.requests.fetch_add(1, Ordering::SeqCst);
            let Script::Progressive { early, wait, last } = &self.script else {
                unreachable!()
            };
            let mut finished = DropFlag(Arc::clone(&self.dropped), false);
            for batch in early {
                if let Some(progress) = progress {
                    progress.partial(batch.clone());
                }
            }
            tokio::time::sleep(*wait).await;
            finished.1 = true;
            ProviderOutcome::results(last.clone())
        }
    }

    type Events = Arc<Mutex<Vec<ProviderEvent<u32>>>>;

    fn collector() -> (Events, Sink<u32>) {
        let events: Events = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&events);
        (
            events,
            Arc::new(move |event| sink.lock().unwrap().push(event)),
        )
    }

    fn lanes() -> Lanes {
        Lanes::new(LaneConfig::default())
    }

    async fn until(what: &str, mut check: impl FnMut() -> bool) {
        for _ in 0..400 {
            if check() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("timed out waiting for {what}");
    }

    fn started(provider: &str) -> ProviderEvent<u32> {
        ProviderEvent::Started {
            provider: provider.to_owned(),
        }
    }

    fn providers(list: &[&Arc<Mock>]) -> Vec<Arc<dyn SearchProvider<u32>>> {
        list.iter()
            .map(|p| Arc::clone(*p) as Arc<dyn SearchProvider<u32>>)
            .collect()
    }

    #[tokio::test]
    async fn every_provider_starts_first_and_the_fast_ones_do_not_wait_for_the_slow_one() {
        let slow = Mock::new(
            "slow",
            Script::Items(vec![1, 2], Duration::from_millis(120)),
        );
        let failing = Mock::new(
            "failing",
            Script::Flaky {
                fails: 99,
                reason: "timed out",
                then: vec![],
            },
        );
        let keyless = Mock::new("keyless", Script::Keyless);
        let registry = SearchRegistry::default();
        let (events, sink) = collector();

        registry.start(
            &lanes(),
            "s1",
            providers(&[&slow, &failing, &keyless]),
            sink,
        );
        until("done", || {
            events.lock().unwrap().last() == Some(&ProviderEvent::Done)
        })
        .await;

        let events = events.lock().unwrap().clone();
        // All three were asked before anything answered.
        assert_eq!(
            events[..3],
            [started("slow"), started("failing"), started("keyless")]
        );
        // The two quick answers came before the slow provider's results, which came last.
        let tail = &events[3..];
        assert_eq!(tail.len(), 4, "{tail:?}");
        assert!(tail.contains(&ProviderEvent::Failed {
            provider: "failing".to_owned(),
            reason: "timed out".to_owned(),
            retryable: true,
        }));
        assert!(tail.contains(&ProviderEvent::NeedsKey {
            provider: "keyless".to_owned(),
        }));
        assert_eq!(
            tail[2],
            ProviderEvent::Results {
                provider: "slow".to_owned(),
                items: vec![1, 2],
            }
        );
        assert_eq!(tail[3], ProviderEvent::Done);
        assert_eq!(
            events.iter().filter(|e| **e == ProviderEvent::Done).count(),
            1
        );
    }

    #[tokio::test]
    async fn a_provider_hands_over_its_first_items_before_it_has_the_rest() {
        let progressive = Mock::new(
            "tmdb",
            Script::Progressive {
                early: vec![vec![1, 2], vec![3]],
                wait: Duration::from_millis(60),
                last: vec![4],
            },
        );
        let registry = SearchRegistry::default();
        let (events, sink) = collector();

        registry.start(&lanes(), "s1", providers(&[&progressive]), sink);
        until("done", || {
            events.lock().unwrap().last() == Some(&ProviderEvent::Done)
        })
        .await;

        // The early batches come in order, before the last items, and `Done` still comes once.
        assert_eq!(
            *events.lock().unwrap(),
            vec![
                started("tmdb"),
                ProviderEvent::Partial {
                    provider: "tmdb".to_owned(),
                    items: vec![1, 2],
                },
                ProviderEvent::Partial {
                    provider: "tmdb".to_owned(),
                    items: vec![3],
                },
                ProviderEvent::Results {
                    provider: "tmdb".to_owned(),
                    items: vec![4],
                },
                ProviderEvent::Done,
            ]
        );
    }

    #[tokio::test]
    async fn a_cancel_drops_the_provider_that_is_still_working_after_its_early_items() {
        let progressive = Mock::new(
            "tmdb",
            Script::Progressive {
                early: vec![vec![1]],
                wait: Duration::from_secs(30),
                last: vec![2],
            },
        );
        let registry = SearchRegistry::default();
        let (events, sink) = collector();

        registry.start(&lanes(), "s1", providers(&[&progressive]), sink);
        until("the early items", || {
            events
                .lock()
                .unwrap()
                .iter()
                .any(|e| matches!(e, ProviderEvent::Partial { .. }))
        })
        .await;
        assert!(registry.cancel("s1"));
        until("the request to be dropped", || {
            progressive.dropped.load(Ordering::SeqCst)
        })
        .await;
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Nothing is sent after the cancel: no results and no `Done`.
        let seen = events.lock().unwrap().clone();
        assert!(
            !seen
                .iter()
                .any(|e| matches!(e, ProviderEvent::Results { .. } | ProviderEvent::Done)),
            "{seen:?}"
        );
    }

    #[tokio::test]
    async fn a_cancel_that_arrives_before_the_start_means_no_provider_is_asked() {
        let provider = Mock::new("tmdb", Script::Items(vec![1], Duration::ZERO));
        let registry = SearchRegistry::default();
        let (events, sink) = collector();

        // The window closed while the start command was still planning.
        assert!(!registry.cancel("early"));
        registry.start(&lanes(), "early", providers(&[&provider]), sink);
        tokio::time::sleep(Duration::from_millis(50)).await;

        assert_eq!(provider.requests(), 0);
        assert!(events.lock().unwrap().is_empty());
        assert!(registry.is_empty(), "the search is not kept either");
        assert_eq!(registry.retry("early", "tmdb"), RetryOutcome::UnknownSearch);

        // The remembered cancel is used up: another search runs as usual.
        let (events, sink) = collector();
        registry.start(&lanes(), "later", providers(&[&provider]), sink);
        until("done", || {
            events.lock().unwrap().last() == Some(&ProviderEvent::Done)
        })
        .await;
        assert_eq!(provider.requests(), 1);
    }

    /// A provider whose request panics.
    struct Panics;

    #[async_trait]
    impl SearchProvider<u32> for Panics {
        fn id(&self) -> &str {
            "panics"
        }

        async fn search(&self, _cancel: &CancelToken) -> ProviderOutcome<u32> {
            panic!("the provider broke");
        }
    }

    #[tokio::test]
    async fn a_provider_that_panics_still_lets_the_search_end_with_done() {
        let steady = Mock::new("steady", Script::Items(vec![7], Duration::ZERO));
        let registry = SearchRegistry::default();
        let (events, sink) = collector();
        let mut list = providers(&[&steady]);
        list.push(Arc::new(Panics));
        registry.start(&lanes(), "p1", list, sink);
        until("done", || {
            events.lock().unwrap().last() == Some(&ProviderEvent::Done)
        })
        .await;
        // Nothing is left running, so the broken provider can be asked again.
        assert_eq!(registry.retry("p1", "panics"), RetryOutcome::Started);
    }

    #[tokio::test]
    async fn retrying_one_provider_asks_only_that_one_and_ends_with_done_again() {
        let steady = Mock::new("steady", Script::Items(vec![7], Duration::ZERO));
        let flaky = Mock::new(
            "flaky",
            Script::Flaky {
                fails: 1,
                reason: "rate limit",
                then: vec![9],
            },
        );
        let registry = SearchRegistry::default();
        let (events, sink) = collector();
        registry.start(&lanes(), "s2", providers(&[&steady, &flaky]), sink);
        until("first done", || {
            events.lock().unwrap().contains(&ProviderEvent::Done)
        })
        .await;
        events.lock().unwrap().clear();

        assert_eq!(registry.retry("s2", "flaky"), RetryOutcome::Started);
        until("second done", || {
            events.lock().unwrap().contains(&ProviderEvent::Done)
        })
        .await;

        assert_eq!(
            *events.lock().unwrap(),
            vec![
                started("flaky"),
                ProviderEvent::Results {
                    provider: "flaky".to_owned(),
                    items: vec![9],
                },
                ProviderEvent::Done,
            ]
        );
        assert_eq!(flaky.requests(), 2);
        assert_eq!(
            steady.requests(),
            1,
            "the other provider was not asked again"
        );
    }

    /// `retry_provider` is a synchronous command, so it runs on the window thread, outside every
    /// tokio runtime. Asking a provider again from there used to panic and abort the app.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn retrying_from_a_thread_outside_the_runtime_asks_the_provider_again() {
        let flaky = Mock::new(
            "flaky",
            Script::Flaky {
                fails: 1,
                reason: "human check",
                then: vec![3],
            },
        );
        let registry = Arc::new(SearchRegistry::default());
        let (events, sink) = collector();
        registry.start(&lanes(), "s9", providers(&[&flaky]), sink);
        until("first done", || {
            events.lock().unwrap().contains(&ProviderEvent::Done)
        })
        .await;
        events.lock().unwrap().clear();

        let outside = Arc::clone(&registry);
        let outcome = std::thread::spawn(move || {
            assert!(tokio::runtime::Handle::try_current().is_err());
            outside.retry("s9", "flaky")
        })
        .join()
        .expect("asking again from outside the runtime must not panic");
        assert_eq!(outcome, RetryOutcome::Started);
        until("second done", || {
            events.lock().unwrap().contains(&ProviderEvent::Done)
        })
        .await;
        assert_eq!(flaky.requests(), 2);
    }

    #[tokio::test]
    async fn a_retry_names_what_is_wrong_with_it() {
        let slow = Mock::new("slow", Script::Items(vec![1], Duration::from_millis(150)));
        let registry = SearchRegistry::default();
        let (_events, sink) = collector();
        assert_eq!(registry.retry("nope", "slow"), RetryOutcome::UnknownSearch);
        registry.start(&lanes(), "s3", providers(&[&slow]), sink);
        assert_eq!(registry.retry("s3", "other"), RetryOutcome::UnknownProvider);
        assert_eq!(registry.retry("s3", "slow"), RetryOutcome::StillRunning);
        assert!(registry.cancel("s3"));
        assert!(!registry.cancel("s3"), "a cancelled search is forgotten");
        assert_eq!(registry.retry("s3", "slow"), RetryOutcome::UnknownSearch);
        assert!(registry.is_empty());
    }

    #[tokio::test]
    async fn cancelling_drops_the_waiting_request_and_sends_nothing_more() {
        let slow = Mock::new("slow", Script::Items(vec![1], Duration::from_millis(400)));
        let registry = SearchRegistry::default();
        let (events, sink) = collector();
        registry.start(&lanes(), "s4", providers(&[&slow]), sink);
        until("the request to begin", || slow.requests() == 1).await;

        assert!(registry.cancel("s4"));
        until("the request to be dropped", || {
            slow.dropped.load(Ordering::SeqCst)
        })
        .await;
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Only the start was sent: no results, no Done.
        assert_eq!(*events.lock().unwrap(), vec![started("slow")]);
    }

    #[tokio::test]
    async fn cancelling_stops_further_requests_of_a_provider_between_its_calls() {
        let paged = Mock::new("paged", Script::Paged { pages: 50 });
        let registry = SearchRegistry::default();
        let (_events, sink) = collector();
        registry.start(&lanes(), "s5", providers(&[&paged]), sink);
        until("a few pages", || paged.requests() >= 3).await;

        registry.cancel("s5");
        tokio::time::sleep(Duration::from_millis(60)).await;
        let after_cancel = paged.requests();
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(
            paged.requests(),
            after_cancel,
            "no request after the cancel"
        );
        assert!(after_cancel < 50);
    }

    #[tokio::test]
    async fn a_provider_still_waiting_for_its_lane_never_makes_its_request_after_a_cancel() {
        // The lane has one worker and the first search holds it.
        let lanes = Lanes::new(LaneConfig {
            provider_default: 1,
            ..LaneConfig::default()
        });
        let busy = Mock::new("shared", Script::Items(vec![1], Duration::from_millis(150)));
        let registry = SearchRegistry::default();
        let (_events, first) = collector();
        registry.start(&lanes, "holder", providers(&[&busy]), first);
        until("the lane to be taken", || busy.requests() == 1).await;

        let queued = Mock::new("shared", Script::Items(vec![2], Duration::ZERO));
        let (_events, second) = collector();
        registry.start(&lanes, "queued", providers(&[&queued]), second);
        registry.cancel("queued");
        tokio::time::sleep(Duration::from_millis(400)).await;

        assert_eq!(
            queued.requests(),
            0,
            "cancelled before its turn on the lane"
        );
    }

    #[tokio::test]
    async fn starting_a_search_again_under_the_same_id_cancels_the_old_one() {
        let first = Mock::new("slow", Script::Items(vec![1], Duration::from_millis(300)));
        let second = Mock::new("quick", Script::Items(vec![2], Duration::ZERO));
        let registry = SearchRegistry::default();
        let (old_events, old_sink) = collector();
        let (new_events, new_sink) = collector();
        registry.start(&lanes(), "same", providers(&[&first]), old_sink);
        registry.start(&lanes(), "same", providers(&[&second]), new_sink);
        until("the new search", || {
            new_events.lock().unwrap().contains(&ProviderEvent::Done)
        })
        .await;
        tokio::time::sleep(Duration::from_millis(100)).await;

        assert_eq!(registry.len(), 1);
        assert_eq!(*old_events.lock().unwrap(), vec![started("slow")]);
    }

    #[tokio::test]
    async fn a_search_without_providers_is_done_at_once() {
        let registry = SearchRegistry::default();
        let (events, sink) = collector();
        registry.start::<u32>(&lanes(), "empty", Vec::new(), sink);
        assert_eq!(*events.lock().unwrap(), vec![ProviderEvent::Done]);
    }

    #[test]
    fn the_events_have_the_wire_shape_the_ui_reads() {
        let json = |event: &ProviderEvent<u32>| serde_json::to_value(event).unwrap();
        assert_eq!(
            json(&started("tmdb")),
            serde_json::json!({"kind": "started", "provider": "tmdb"})
        );
        assert_eq!(
            json(&ProviderEvent::Results {
                provider: "tmdb".to_owned(),
                items: vec![1, 2],
            }),
            serde_json::json!({"kind": "results", "provider": "tmdb", "items": [1, 2]})
        );
        assert_eq!(
            json(&ProviderEvent::Partial {
                provider: "tmdb".to_owned(),
                items: vec![1],
            }),
            serde_json::json!({"kind": "partial", "provider": "tmdb", "items": [1]})
        );
        assert_eq!(
            json(&ProviderEvent::Update {
                provider: "tmdb".to_owned(),
                items: vec![1],
            }),
            serde_json::json!({"kind": "update", "provider": "tmdb", "items": [1]})
        );
        assert_eq!(
            json(&ProviderEvent::Failed {
                provider: "tmdb".to_owned(),
                reason: KEY_REJECTED.to_owned(),
                retryable: false,
            }),
            serde_json::json!({
                "kind": "failed", "provider": "tmdb", "reason": "key-rejected", "retryable": false
            })
        );
        assert_eq!(
            json(&ProviderEvent::NeedsKey {
                provider: "omdb".to_owned(),
            }),
            serde_json::json!({"kind": "needsKey", "provider": "omdb"})
        );
        assert_eq!(
            json(&ProviderEvent::<u32>::Done),
            serde_json::json!({"kind": "done"})
        );
    }
}
