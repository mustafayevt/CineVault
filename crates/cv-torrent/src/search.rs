//! Running the sources at once and merging what they find.
//!
//! [`Orchestrator::run`] starts every source that applies on its own task. Each source has its
//! own deadline ([`SOURCE_DEADLINE`], 25 seconds; the user's own Torznab instance gets
//! [`AGGREGATE_DEADLINE_FACTOR`] times that) and a failure, a rate limit, a challenge page or
//! a timeout in one never stops another. Indexer sources are held to
//! [`INDEXER_IN_FLIGHT`] (16) at a time. Everything found goes through one shared
//! [`MergeState`], so a release returned by three sources is one row naming all three, and each
//! row is judged (relevance, flags, upgrade marker) before it reaches the [`Sink`].
//!
//! The sink is called while the merge is locked, so it sees the rows of a release in the order
//! they changed; it must therefore return quickly (a channel send).
//!
//! The pieces are public so the app can run a single source again after a failure
//! ([`SearchSession::run_source`]) without disturbing the rows already merged.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::cancel::CancelToken;
use crate::merge::{MergeState, MergedRow};
use crate::model::{
    Release, SkipReason, SourceError, SourceId, SourceKind, SourceOutcome, Support, TorrentQuery,
};
use crate::rank::{Assessment, assess};
use crate::source::{PartialSink, SearchContext, TorrentSource};

/// How long one source may take.
pub const SOURCE_DEADLINE: Duration = Duration::from_secs(25);
/// How many indexer sources search at the same time.
pub const INDEXER_IN_FLIGHT: usize = 16;
/// How many times the deadline the user's own Torznab instance gets: its all-indexers feed asks
/// every site it holds before each answer (about 22 seconds for 88 sites in a real instance), and
/// it is asked by ID and by title.
pub const AGGREGATE_DEADLINE_FACTOR: u32 = 4;

/// A merged release with its judgement, as the sink receives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultRow {
    /// The merge key; a consumer replaces rows by it.
    pub id: String,
    /// Keys of rows that were folded into this one and are to be dropped.
    pub absorbs: Vec<String>,
    pub release: Release,
    pub assessment: Assessment,
}

/// What a finished search found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    /// Distinct releases.
    pub total: usize,
    /// Those that are the title searched for.
    pub matching: usize,
    /// The rest, hidden unless asked for.
    pub hidden: usize,
    pub outcomes: Vec<(SourceId, SourceOutcome)>,
}

impl Summary {
    /// How many sources answered (with or without releases).
    #[must_use]
    pub fn answered(&self) -> usize {
        self.outcomes
            .iter()
            .filter(|(_, outcome)| matches!(outcome, SourceOutcome::Done { .. }))
            .count()
    }

    /// How many sources failed.
    #[must_use]
    pub fn failed(&self) -> usize {
        self.outcomes
            .iter()
            .filter(|(_, outcome)| matches!(outcome, SourceOutcome::Failed(_)))
            .count()
    }
}

/// How a run ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Finished(Summary),
    /// The search was cancelled; the sink heard nothing more after the cancel.
    Cancelled,
}

/// Where the progress of a search goes. Calls come from several tasks; they must not block.
pub trait Sink: Send + Sync {
    /// A source that applies has begun.
    fn started(&self, source: &SourceId);
    /// Rows a source has found so far (a text source sends one batch per spelling).
    fn partial(&self, source: &SourceId, rows: Vec<ResultRow>);
    /// A source finished; every row it touched, as merged with the other sources.
    fn results(&self, source: &SourceId, rows: Vec<ResultRow>);
    /// A source failed.
    fn failed(&self, source: &SourceId, error: &SourceError);
    /// A source does not apply to this title.
    fn skipped(&self, source: &SourceId, reason: SkipReason);
    /// Every source has finished.
    fn done(&self, summary: &Summary);
}

/// The state of one search: its query and its merge. Sources are run into it one at a time with
/// the sink of the caller.
pub struct SearchSession {
    query: Arc<TorrentQuery>,
    merge: Mutex<MergeState>,
    deadline: Duration,
}

impl SearchSession {
    /// A session for `query` with the standard per-source deadline.
    #[must_use]
    pub fn new(query: TorrentQuery) -> Arc<Self> {
        Self::with_deadline(query, SOURCE_DEADLINE)
    }

    /// Like [`Self::new`] with another per-source deadline (tests use a short one).
    #[must_use]
    pub fn with_deadline(query: TorrentQuery, deadline: Duration) -> Arc<Self> {
        Arc::new(Self {
            query: Arc::new(query),
            merge: Mutex::new(MergeState::new()),
            deadline,
        })
    }

    /// The query being searched.
    #[must_use]
    pub fn query(&self) -> &TorrentQuery {
        &self.query
    }

    fn rows_of(&self, merged: Vec<MergedRow>) -> Vec<ResultRow> {
        merged
            .into_iter()
            .map(|row| {
                let assessment = assess(&row.release, &self.query);
                ResultRow {
                    id: row.id,
                    absorbs: row.absorbs,
                    release: row.release,
                    assessment,
                }
            })
            .collect()
    }

    /// Merges `raws` found by `source` and hands the touched rows to `deliver`, with the merge
    /// locked so rows reach the sink in the order they changed.
    fn merge_and_deliver(
        &self,
        sink: &dyn Sink,
        source: &SourceId,
        mut raws: Vec<crate::model::RawRelease>,
        deliver: impl FnOnce(&dyn Sink, &SourceId, Vec<ResultRow>),
    ) {
        for raw in &mut raws {
            raw.source.get_or_insert_with(|| source.clone());
        }
        let mut merge = self.merge.lock().unwrap_or_else(PoisonError::into_inner);
        let rows = self.rows_of(merge.add_all(raws));
        deliver(sink, source, rows);
    }

    /// Runs one source to the end: skipped, failed, timed out or done. A source that applies
    /// is reported as started first. Returns what happened, or `None` when cancelled.
    pub async fn run_source(
        self: &Arc<Self>,
        source: Arc<dyn TorrentSource>,
        cancel: &CancelToken,
        sink: &Arc<dyn Sink>,
    ) -> Option<SourceOutcome> {
        let id = source.id().clone();
        if let Support::Skip(reason) = source.supports(&self.query) {
            sink.skipped(&id, reason);
            return Some(SourceOutcome::Skipped(reason));
        }
        if cancel.is_cancelled() {
            return None;
        }
        sink.started(&id);

        let session = Arc::clone(self);
        let partial_id = id.clone();
        let partial_sink = Arc::clone(sink);
        let partial: PartialSink = Arc::new(move |rows| {
            session.merge_and_deliver(
                partial_sink.as_ref(),
                &partial_id,
                rows,
                |sink, id, rows| sink.partial(id, rows),
            );
        });
        let context = SearchContext::with_partial(cancel.clone(), partial);

        let deadline = if source.kind() == SourceKind::Torznab {
            self.deadline * AGGREGATE_DEADLINE_FACTOR
        } else {
            self.deadline
        };
        let search = tokio::time::timeout(deadline, source.search(&self.query, &context));
        let result = tokio::select! {
            () = cancel.cancelled() => return None,
            result = search => result,
        };
        if cancel.is_cancelled() {
            return None;
        }
        match result {
            Ok(Ok(raws)) => {
                let count = raws.len();
                self.merge_and_deliver(sink.as_ref(), &id, raws, |sink, id, rows| {
                    sink.results(id, rows);
                });
                Some(SourceOutcome::Done { releases: count })
            }
            Ok(Err(error)) => {
                sink.failed(&id, &error);
                Some(SourceOutcome::Failed(error))
            }
            Err(_elapsed) => {
                sink.failed(&id, &SourceError::Timeout);
                Some(SourceOutcome::Failed(SourceError::Timeout))
            }
        }
    }

    /// The counts of everything merged so far.
    #[must_use]
    pub fn summary(&self, outcomes: Vec<(SourceId, SourceOutcome)>) -> Summary {
        let merge = self.merge.lock().unwrap_or_else(PoisonError::into_inner);
        let rows = self.rows_of(merge.rows());
        let matching = rows
            .iter()
            .filter(|row| row.assessment.relevance.is_match())
            .count();
        Summary {
            total: rows.len(),
            matching,
            hidden: rows.len() - matching,
            outcomes,
        }
    }
}

/// Runs a set of sources for one query.
pub struct Orchestrator {
    sources: Vec<Arc<dyn TorrentSource>>,
    deadline: Duration,
    indexer_in_flight: usize,
}

impl Orchestrator {
    /// An orchestrator over `sources` with the standard deadline and indexer limit.
    #[must_use]
    pub fn new(sources: Vec<Arc<dyn TorrentSource>>) -> Self {
        Self {
            sources,
            deadline: SOURCE_DEADLINE,
            indexer_in_flight: INDEXER_IN_FLIGHT,
        }
    }

    /// Changes the per-source deadline.
    #[must_use]
    pub fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }

    /// Changes how many indexer sources run at once.
    #[must_use]
    pub fn with_indexer_in_flight(mut self, limit: usize) -> Self {
        self.indexer_in_flight = limit.max(1);
        self
    }

    /// The sources.
    #[must_use]
    pub fn sources(&self) -> &[Arc<dyn TorrentSource>] {
        &self.sources
    }

    /// Searches every source at once and reports to `sink`. Returns when all have finished, or
    /// as soon as `cancel` is cancelled, in which case every running request is dropped.
    pub async fn run(
        &self,
        query: TorrentQuery,
        sink: Arc<dyn Sink>,
        cancel: &CancelToken,
    ) -> Outcome {
        let session = SearchSession::with_deadline(query, self.deadline);
        let indexers = Arc::new(Semaphore::new(self.indexer_in_flight));
        let mut tasks: JoinSet<(usize, Option<SourceOutcome>)> = JoinSet::new();

        for (index, source) in self.sources.iter().enumerate() {
            let session = Arc::clone(&session);
            let sink = Arc::clone(&sink);
            let source = Arc::clone(source);
            let cancel = cancel.clone();
            let indexers = Arc::clone(&indexers);
            tasks.spawn(async move {
                // An indexer waits for a free slot (the service is searched indexer by indexer);
                // the slot is held until the source is done.
                let _slot = if source.kind() == SourceKind::Indexer {
                    tokio::select! {
                        () = cancel.cancelled() => return (index, None),
                        permit = indexers.acquire_owned() => permit.ok(),
                    }
                } else {
                    None
                };
                (index, session.run_source(source, &cancel, &sink).await)
            });
        }

        let mut outcomes: Vec<Option<SourceOutcome>> = vec![None; self.sources.len()];
        loop {
            tokio::select! {
                () = cancel.cancelled() => {
                    tasks.abort_all();
                    while tasks.join_next().await.is_some() {}
                    return Outcome::Cancelled;
                }
                joined = tasks.join_next() => match joined {
                    Some(Ok((index, outcome))) => outcomes[index] = outcome,
                    Some(Err(error)) => {
                        tracing::warn!(%error, "a torrent source task ended abnormally");
                    }
                    None => break,
                },
            }
        }
        if cancel.is_cancelled() {
            return Outcome::Cancelled;
        }

        let outcomes: Vec<(SourceId, SourceOutcome)> = self
            .sources
            .iter()
            .zip(outcomes)
            .filter_map(|(source, outcome)| Some((source.id().clone(), outcome?)))
            .collect();
        let summary = session.summary(outcomes);
        sink.done(&summary);
        Outcome::Finished(summary)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;

    use super::*;
    use crate::model::{MediaKind, RawRelease, Scope, TitleIds};

    /// What a sink heard, in order.
    #[derive(Default)]
    struct Recorder {
        events: Mutex<Vec<String>>,
        rows: Mutex<Vec<ResultRow>>,
    }

    impl Recorder {
        fn events(&self) -> Vec<String> {
            self.events.lock().unwrap().clone()
        }
    }

    impl Sink for Recorder {
        fn started(&self, source: &SourceId) {
            self.events
                .lock()
                .unwrap()
                .push(format!("started {}", source.0));
        }
        fn partial(&self, source: &SourceId, rows: Vec<ResultRow>) {
            self.events
                .lock()
                .unwrap()
                .push(format!("partial {} {}", source.0, rows.len()));
            self.rows.lock().unwrap().extend(rows);
        }
        fn results(&self, source: &SourceId, rows: Vec<ResultRow>) {
            self.events
                .lock()
                .unwrap()
                .push(format!("results {} {}", source.0, rows.len()));
            self.rows.lock().unwrap().extend(rows);
        }
        fn failed(&self, source: &SourceId, error: &SourceError) {
            self.events
                .lock()
                .unwrap()
                .push(format!("failed {} {error}", source.0));
        }
        fn skipped(&self, source: &SourceId, reason: SkipReason) {
            self.events
                .lock()
                .unwrap()
                .push(format!("skipped {} {reason:?}", source.0));
        }
        fn done(&self, summary: &Summary) {
            self.events.lock().unwrap().push(format!(
                "done {} {} {}",
                summary.total, summary.matching, summary.hidden
            ));
        }
    }

    enum Behaviour {
        Rows(Vec<RawRelease>),
        Fail(SourceError),
        Sleep(Duration),
        Partials(Vec<Vec<RawRelease>>, Vec<RawRelease>),
        Skip(SkipReason),
    }

    struct Fake {
        id: SourceId,
        kind: SourceKind,
        behaviour: Behaviour,
        running: Arc<AtomicUsize>,
        peak: Arc<AtomicUsize>,
    }

    fn fake(id: &str, kind: SourceKind, behaviour: Behaviour) -> Arc<Fake> {
        Arc::new(Fake {
            id: SourceId(id.into()),
            kind,
            behaviour,
            running: Arc::new(AtomicUsize::new(0)),
            peak: Arc::new(AtomicUsize::new(0)),
        })
    }

    #[async_trait]
    impl TorrentSource for Fake {
        fn id(&self) -> &SourceId {
            &self.id
        }
        fn name(&self) -> &str {
            &self.id.0
        }
        fn kind(&self) -> SourceKind {
            self.kind
        }
        fn supports(&self, _query: &TorrentQuery) -> Support {
            match &self.behaviour {
                Behaviour::Skip(reason) => Support::Skip(*reason),
                _ => Support::Yes,
            }
        }
        async fn search(
            &self,
            _query: &TorrentQuery,
            context: &SearchContext,
        ) -> Result<Vec<RawRelease>, SourceError> {
            let now = self.running.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak.fetch_max(now, Ordering::SeqCst);
            let result = match &self.behaviour {
                Behaviour::Rows(rows) => Ok(rows.clone()),
                Behaviour::Fail(error) => Err(error.clone()),
                Behaviour::Sleep(duration) => {
                    tokio::time::sleep(*duration).await;
                    Ok(Vec::new())
                }
                Behaviour::Partials(batches, last) => {
                    for batch in batches {
                        context.partial(batch.clone());
                    }
                    Ok(last.clone())
                }
                Behaviour::Skip(_) => Ok(Vec::new()),
            };
            self.running.fetch_sub(1, Ordering::SeqCst);
            result
        }
    }

    fn release(name: &str, hash_digit: char, seeders: u32) -> RawRelease {
        RawRelease {
            name: name.into(),
            info_hash: Some(hash_digit.to_string().repeat(40)),
            size: Some(2_000_000_000),
            seeders: Some(seeders),
            ..RawRelease::default()
        }
    }

    fn query() -> TorrentQuery {
        TorrentQuery {
            kind: MediaKind::Movie,
            ids: TitleIds::default(),
            title: "Dune".into(),
            original_title: None,
            alternative_titles: Vec::new(),
            year: Some(2021),
            scope: Scope::Movie,
            runtime_minutes: Some(155),
            anime: false,
            library_copy: None,
        }
    }

    fn sources(list: Vec<Arc<Fake>>) -> Vec<Arc<dyn TorrentSource>> {
        list.into_iter()
            .map(|s| s as Arc<dyn TorrentSource>)
            .collect()
    }

    async fn run(list: Vec<Arc<Fake>>) -> (Outcome, Arc<Recorder>) {
        let recorder = Arc::new(Recorder::default());
        let outcome = Orchestrator::new(sources(list))
            .with_deadline(Duration::from_millis(300))
            .run(query(), recorder.clone(), &CancelToken::new())
            .await;
        (outcome, recorder)
    }

    #[tokio::test]
    async fn rows_from_three_sources_are_one_row_naming_all_three() {
        let same = release("Dune.2021.2160p.WEB-DL", 'a', 10);
        let (outcome, recorder) = run(vec![
            fake(
                "a",
                SourceKind::BuiltIn,
                Behaviour::Rows(vec![same.clone()]),
            ),
            fake(
                "b",
                SourceKind::BuiltIn,
                Behaviour::Rows(vec![same.clone()]),
            ),
            fake("c", SourceKind::Torznab, Behaviour::Rows(vec![same])),
        ])
        .await;
        let Outcome::Finished(summary) = outcome else {
            panic!("finished")
        };
        assert_eq!((summary.total, summary.matching, summary.hidden), (1, 1, 0));
        assert_eq!(summary.answered(), 3);
        let last = recorder.rows.lock().unwrap().last().cloned().unwrap();
        assert_eq!(last.release.sources.len(), 3);
        assert!(recorder.events().last().unwrap().starts_with("done 1 1 0"));
    }

    #[tokio::test]
    async fn a_failing_source_does_not_stop_the_others_and_each_reason_is_reported() {
        let (outcome, recorder) = run(vec![
            fake(
                "good",
                SourceKind::BuiltIn,
                Behaviour::Rows(vec![release("Dune.2021.1080p", 'a', 3)]),
            ),
            fake(
                "limited",
                SourceKind::BuiltIn,
                Behaviour::Fail(SourceError::RateLimited),
            ),
            fake(
                "blocked",
                SourceKind::BuiltIn,
                Behaviour::Fail(SourceError::Blocked),
            ),
            fake(
                "slow",
                SourceKind::BuiltIn,
                Behaviour::Sleep(Duration::from_secs(30)),
            ),
        ])
        .await;
        let Outcome::Finished(summary) = outcome else {
            panic!("finished")
        };
        assert_eq!((summary.answered(), summary.failed()), (1, 3));
        let events = recorder.events();
        assert!(events.contains(&"results good 1".to_owned()));
        assert!(events.contains(&"failed limited rate limited".to_owned()));
        assert!(events.contains(&"failed blocked blocked by a challenge page".to_owned()));
        assert!(events.contains(&"failed slow timed out".to_owned()));
    }

    #[tokio::test]
    async fn skipped_sources_are_reported_and_not_started() {
        let (_, recorder) = run(vec![
            fake(
                "tv",
                SourceKind::BuiltIn,
                Behaviour::Skip(SkipReason::TvOnly),
            ),
            fake("ok", SourceKind::BuiltIn, Behaviour::Rows(Vec::new())),
        ])
        .await;
        let events = recorder.events();
        assert!(events.contains(&"skipped tv TvOnly".to_owned()));
        assert!(!events.iter().any(|e| e == "started tv"));
        assert!(events.contains(&"started ok".to_owned()));
    }

    #[tokio::test]
    async fn hidden_releases_are_counted() {
        let (outcome, _) = run(vec![fake(
            "a",
            SourceKind::BuiltIn,
            Behaviour::Rows(vec![
                release("Dune.2021.1080p", 'a', 1),
                release("Dune.1984.1080p", 'b', 1),
                release("Dune.Part.Two.2024.1080p", 'c', 1),
            ]),
        )])
        .await;
        let Outcome::Finished(summary) = outcome else {
            panic!("finished")
        };
        assert_eq!((summary.total, summary.matching, summary.hidden), (3, 1, 2));
    }

    #[tokio::test]
    async fn partial_rows_arrive_before_the_final_results() {
        let (_, recorder) = run(vec![fake(
            "text",
            SourceKind::BuiltIn,
            Behaviour::Partials(
                vec![
                    vec![release("Dune.2021.1080p", 'a', 1)],
                    vec![release("Dune.2021.720p", 'b', 1)],
                ],
                vec![
                    release("Dune.2021.1080p", 'a', 1),
                    release("Dune.2021.720p", 'b', 1),
                ],
            ),
        )])
        .await;
        let events = recorder.events();
        let at = |needle: &str| {
            events
                .iter()
                .position(|e| e == needle)
                .unwrap_or(usize::MAX)
        };
        assert!(at("partial text 1") < at("results text 2"), "{events:?}");
        assert_eq!(
            events
                .iter()
                .filter(|e| e.starts_with("partial text"))
                .count(),
            2
        );
    }

    #[tokio::test]
    async fn rows_carry_their_judgement() {
        let (_, recorder) = run(vec![fake(
            "a",
            SourceKind::BuiltIn,
            Behaviour::Rows(vec![
                release("Dune.2021.2160p.WEB-DL", 'a', 0),
                release("Dune.1984.1080p", 'b', 4),
            ]),
        )])
        .await;
        let rows = recorder.rows.lock().unwrap().clone();
        let matching = rows
            .iter()
            .find(|r| r.release.name.contains("2021"))
            .unwrap();
        assert!(matching.assessment.relevance.is_match());
        assert!(
            matching
                .assessment
                .flags
                .iter()
                .any(|f| f.text == "No seeders")
        );
        let other = rows
            .iter()
            .find(|r| r.release.name.contains("1984"))
            .unwrap();
        assert!(!other.assessment.relevance.is_match());
    }

    #[tokio::test]
    async fn at_most_sixteen_indexers_search_at_once() {
        let running = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let list: Vec<Arc<Fake>> = (0..40)
            .map(|i| {
                Arc::new(Fake {
                    id: SourceId(format!("indexer:{i}")),
                    kind: SourceKind::Indexer,
                    behaviour: Behaviour::Sleep(Duration::from_millis(20)),
                    running: Arc::clone(&running),
                    peak: Arc::clone(&peak),
                })
            })
            .collect();
        let recorder = Arc::new(Recorder::default());
        let outcome = Orchestrator::new(sources(list))
            .run(query(), recorder.clone(), &CancelToken::new())
            .await;
        let Outcome::Finished(summary) = outcome else {
            panic!("finished")
        };
        assert_eq!(summary.answered(), 40);
        assert_eq!(peak.load(Ordering::SeqCst), INDEXER_IN_FLIGHT);
    }

    #[tokio::test]
    async fn built_in_sources_are_not_held_by_the_indexer_limit() {
        let running = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let list: Vec<Arc<Fake>> = (0..20)
            .map(|i| {
                Arc::new(Fake {
                    id: SourceId(format!("s{i}")),
                    kind: SourceKind::BuiltIn,
                    behaviour: Behaviour::Sleep(Duration::from_millis(50)),
                    running: Arc::clone(&running),
                    peak: Arc::clone(&peak),
                })
            })
            .collect();
        let recorder = Arc::new(Recorder::default());
        Orchestrator::new(sources(list))
            .run(query(), recorder, &CancelToken::new())
            .await;
        assert_eq!(peak.load(Ordering::SeqCst), 20);
    }

    #[tokio::test]
    async fn cancelling_returns_at_once_and_the_sink_hears_nothing_more() {
        let recorder = Arc::new(Recorder::default());
        let cancel = CancelToken::new();
        let orchestrator = Orchestrator::new(sources(vec![
            fake(
                "slow1",
                SourceKind::BuiltIn,
                Behaviour::Sleep(Duration::from_secs(30)),
            ),
            fake(
                "slow2",
                SourceKind::Indexer,
                Behaviour::Sleep(Duration::from_secs(30)),
            ),
        ]));
        let started = std::time::Instant::now();
        let handle = {
            let recorder = recorder.clone();
            let cancel = cancel.clone();
            tokio::spawn(async move { orchestrator.run(query(), recorder, &cancel).await })
        };
        tokio::time::sleep(Duration::from_millis(50)).await;
        cancel.cancel();
        let outcome = handle.await.unwrap();
        assert_eq!(outcome, Outcome::Cancelled);
        assert!(started.elapsed() < Duration::from_secs(1));
        let events = recorder.events();
        assert!(
            !events
                .iter()
                .any(|e| e.starts_with("done") || e.starts_with("failed")),
            "{events:?}"
        );
    }

    #[tokio::test]
    async fn no_sources_finishes_with_nothing() {
        let (outcome, recorder) = run(Vec::new()).await;
        let Outcome::Finished(summary) = outcome else {
            panic!("finished")
        };
        assert_eq!(summary.total, 0);
        assert_eq!(recorder.events(), vec!["done 0 0 0".to_owned()]);
    }

    #[tokio::test]
    async fn one_source_can_be_run_again_into_the_same_merge() {
        let recorder = Arc::new(Recorder::default());
        let session = SearchSession::new(query());
        let sink: Arc<dyn Sink> = recorder.clone();
        let first = fake(
            "a",
            SourceKind::BuiltIn,
            Behaviour::Rows(vec![release("Dune.2021.1080p", 'a', 1)]),
        );
        let second = fake(
            "b",
            SourceKind::BuiltIn,
            Behaviour::Rows(vec![release("Dune.2021.1080p", 'a', 9)]),
        );
        let cancel = CancelToken::new();
        session.run_source(first, &cancel, &sink).await;
        session.run_source(second, &cancel, &sink).await;
        let summary = session.summary(Vec::new());
        assert_eq!(summary.total, 1);
        let last = recorder.rows.lock().unwrap().last().cloned().unwrap();
        assert_eq!(last.release.sources.len(), 2);
        assert_eq!(last.release.seeders, Some(9));
    }

    #[test]
    fn the_standard_limits_are_the_documented_ones() {
        assert_eq!(SOURCE_DEADLINE, Duration::from_secs(25));
        assert_eq!(INDEXER_IN_FLIGHT, 16);
    }
}
