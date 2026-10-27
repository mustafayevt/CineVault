//! The streaming scrape search through the generic engine, with mock providers: events arrive
//! provider by provider, hits that share an IMDb ID reach the page as one entry, and an empty
//! search says why just before Done. No network.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use cv_app::app_settings::SettingsService;
use cv_app::match_service::MatchService;
use cv_app::notices::NoticeHub;
use cv_app::provider_credentials::ProviderCredentialsService;
use cv_db::repo::insert_item;
use cv_db::{Database, DbOptions};
use cv_domain::{Item, ItemId, ItemKind, ProviderIds};
use cv_jobs::{JobManager, JobStore, LaneConfig, Lanes, ManagerConfig};
use cv_providers::model::{Capabilities, MediaKind, RequestOptions, SearchHit, SearchQuery};
use cv_providers::{
    CredentialResolver, MemoryUserCredentials, MetadataProvider, Provider, ProviderError,
    ProviderRegistry,
};

use super::*;
use crate::provider_stream::{SearchRegistry, Sink, finishing};

/// A provider that answers after a delay with one title, or cannot be reached.
struct Mock {
    id: &'static str,
    name: &'static str,
    lookup: Vec<&'static str>,
    /// (the provider's own ID, IMDb ID, title); `None` is a provider that cannot be reached.
    answer: Option<Vec<(&'static str, &'static str, &'static str)>>,
    delay: Duration,
}

#[async_trait]
impl Provider for Mock {
    fn id(&self) -> &'static str {
        self.id
    }
    fn name(&self) -> &'static str {
        self.name
    }
}

#[async_trait]
impl MetadataProvider for Mock {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            movies: true,
            shows: false,
            orderings: Vec::new(),
            lookup_ids: self.lookup.clone(),
            best_effort: false,
        }
    }

    async fn search(
        &self,
        _query: &SearchQuery,
        _options: &RequestOptions,
    ) -> Result<Vec<SearchHit>, ProviderError> {
        tokio::time::sleep(self.delay).await;
        let Some(titles) = &self.answer else {
            return Err(ProviderError::Request {
                provider: self.name.to_owned(),
                message: "connection reset".to_owned(),
            });
        };
        Ok(titles
            .iter()
            .map(|(own, imdb, title)| {
                let mut ids = ProviderIds::new();
                ids.set(self.lookup[0], own);
                ids.set("imdb", imdb);
                SearchHit {
                    provider: self.id.to_owned(),
                    ids,
                    title: (*title).to_owned(),
                    year: 1999,
                    ..SearchHit::default()
                }
            })
            .collect())
    }

    async fn lookup(
        &self,
        _kind: MediaKind,
        _ids: &ProviderIds,
        _options: &RequestOptions,
    ) -> Result<Option<SearchHit>, ProviderError> {
        Ok(None)
    }

    async fn details(
        &self,
        _kind: MediaKind,
        _ids: &ProviderIds,
        _options: &RequestOptions,
    ) -> Result<Item, ProviderError> {
        Err(ProviderError::NotFound(self.name.to_owned()))
    }
}

struct Rig {
    _dir: tempfile::TempDir,
    db: Arc<Database>,
    service: Arc<MatchService>,
}

fn rig(providers: Vec<Mock>, keys: &[&str]) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(
        Database::open_with(dir.path().join("library.db"), &DbOptions { readers: 2 }).unwrap(),
    );
    let manager = JobManager::with_store(
        Lanes::new(LaneConfig::default()),
        ManagerConfig::default(),
        JobStore::new(Arc::clone(&db)),
    );
    let mut registry = ProviderRegistry::new();
    for provider in providers {
        registry.register_metadata(Arc::new(provider));
    }
    let credentials = Arc::new(ProviderCredentialsService::with_resolver(
        CredentialResolver::new(Arc::new(MemoryUserCredentials::default())),
        registry,
    ));
    for key in keys {
        credentials.set(key, "api_key", "a-key").unwrap();
    }
    let service = MatchService::new(
        Arc::clone(&db),
        manager,
        credentials,
        Arc::new(SettingsService::in_memory("")),
        Arc::new(NoticeHub::new()),
    );
    Rig {
        _dir: dir,
        db,
        service,
    }
}

impl Rig {
    fn movie(&self) -> ItemId {
        let mut item = Item::movie("The Matrix");
        item.core.year = 1999;
        self.db
            .write(move |tx| insert_item(tx, &item))
            .expect("insert item")
    }
}

type Events = Arc<Mutex<Vec<ProviderEvent<ScrapeStreamHitDto>>>>;

fn collector() -> (Events, Sink<ScrapeStreamHitDto>) {
    let events: Events = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&events);
    (
        events,
        Arc::new(move |event| sink.lock().unwrap().push(event)),
    )
}

async fn until(what: &str, mut check: impl FnMut() -> bool) {
    for _ in 0..500 {
        if check() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("timed out waiting for {what}");
}

fn describe(events: &[ProviderEvent<ScrapeStreamHitDto>]) -> Vec<String> {
    events
        .iter()
        .map(|event| match event {
            ProviderEvent::Started { provider } => format!("started {provider}"),
            ProviderEvent::Results { provider, items } => {
                format!("results {provider} {}", items.len())
            }
            ProviderEvent::Partial { provider, items } => {
                format!("partial {provider} {}", items.len())
            }
            ProviderEvent::Update { provider, items } => {
                format!("update {provider} {}", items.len())
            }
            ProviderEvent::Failed {
                provider,
                retryable,
                ..
            } => format!("failed {provider} retryable={retryable}"),
            ProviderEvent::NeedsKey { provider } => format!("needs-key {provider}"),
            ProviderEvent::Done => "done".to_owned(),
        })
        .collect()
}

fn run(rig: &Rig, item: ItemId, id: &str) -> (Events, SearchRegistry, ScrapeCandidateStream) {
    let stream = rig
        .service
        .scrape_candidate_stream(item, "The Matrix", ItemKind::Movie, "")
        .unwrap();
    let registry = SearchRegistry::default();
    let (events, sink) = collector();
    registry.start(
        &Lanes::new(LaneConfig::default()),
        id,
        searches(&stream, None),
        finishing(sink, Some(nothing_found(&stream))),
    );
    (events, registry, stream)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn providers_answer_one_by_one_and_a_shared_imdb_id_arrives_as_one_entry() {
    let rig = rig(
        vec![
            Mock {
                id: "tmdb",
                name: "TMDB",
                lookup: vec!["tmdb", "imdb"],
                answer: Some(vec![("603", "tt0133093", "The Matrix")]),
                delay: Duration::from_millis(250),
            },
            Mock {
                id: "omdb",
                name: "OMDb",
                lookup: vec!["imdb"],
                answer: Some(vec![("tt0133093", "tt0133093", "The Matrix")]),
                delay: Duration::ZERO,
            },
            Mock {
                id: "tvdb",
                name: "TheTVDB",
                lookup: vec!["tvdb", "imdb"],
                answer: None,
                delay: Duration::ZERO,
            },
        ],
        &["tmdb", "omdb", "tvdb"],
    );
    let item = rig.movie();
    let (events, registry, stream) = run(&rig, item, "s1");
    until("done", || {
        events.lock().unwrap().last() == Some(&ProviderEvent::Done)
    })
    .await;

    let seen = describe(&events.lock().unwrap());
    // The quick provider answers first; the slow one's entry is the same entry, sent again.
    assert_eq!(seen[..3], ["started tmdb", "started omdb", "started tvdb"]);
    assert!(
        seen.iter().position(|e| e == "results omdb 1")
            < seen.iter().position(|e| e == "results tmdb 1"),
        "{seen:?}"
    );
    assert_eq!(seen.last().unwrap(), "done");
    assert!(
        !seen.iter().any(|e| e.starts_with("failed search")),
        "{seen:?}"
    );

    let all = events.lock().unwrap().clone();
    let entries: Vec<&ScrapeStreamHitDto> = all
        .iter()
        .filter_map(|event| match event {
            ProviderEvent::Partial { items, .. } | ProviderEvent::Results { items, .. } => {
                Some(items)
            }
            _ => None,
        })
        .flatten()
        .collect();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].id, entries[1].id, "one entry, sent again");
    assert_eq!(entries[1].providers.len(), 2);
    assert!(entries[1].ids.contains(&"imdb:tt0133093".to_owned()));
    assert_eq!(stream.summary().found(), 1);

    // The provider that failed can be asked again, and only that one.
    events.lock().unwrap().clear();
    assert_eq!(
        registry.retry("s1", "tvdb"),
        crate::provider_stream::RetryOutcome::Started
    );
    until("second done", || {
        events.lock().unwrap().last() == Some(&ProviderEvent::Done)
    })
    .await;
    assert_eq!(
        describe(&events.lock().unwrap()),
        ["started tvdb", "failed tvdb retryable=true", "done"]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_search_that_finds_nothing_says_why_just_before_done() {
    let rig = rig(
        vec![Mock {
            id: "omdb",
            name: "OMDb",
            lookup: vec!["imdb"],
            answer: Some(Vec::new()),
            delay: Duration::ZERO,
        }],
        &["omdb"],
    );
    let item = rig.movie();
    let (events, _registry, _stream) = run(&rig, item, "s2");
    until("done", || {
        events.lock().unwrap().last() == Some(&ProviderEvent::Done)
    })
    .await;

    let all = events.lock().unwrap().clone();
    assert_eq!(
        describe(&all),
        [
            "started omdb",
            "results omdb 0",
            "failed search retryable=false",
            "done"
        ]
    );
    let ProviderEvent::Failed { reason, .. } = &all[2] else {
        panic!()
    };
    assert!(reason.contains("The Matrix"), "{reason}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_provider_without_a_key_is_listed_last_and_says_so() {
    let rig = rig(
        vec![
            Mock {
                id: "tmdb",
                name: "TMDB",
                lookup: vec!["tmdb", "imdb"],
                answer: Some(vec![("603", "tt0133093", "The Matrix")]),
                delay: Duration::ZERO,
            },
            Mock {
                id: "omdb",
                name: "OMDb",
                lookup: vec!["imdb"],
                answer: Some(Vec::new()),
                delay: Duration::ZERO,
            },
        ],
        &["omdb"],
    );
    let item = rig.movie();
    let (events, _registry, stream) = run(&rig, item, "s3");
    until("done", || {
        events.lock().unwrap().last() == Some(&ProviderEvent::Done)
    })
    .await;

    assert_eq!(
        stream
            .sources
            .iter()
            .map(|s| s.key.as_str())
            .collect::<Vec<_>>(),
        ["omdb", "tmdb"]
    );
    let seen = describe(&events.lock().unwrap());
    assert!(seen.contains(&"needs-key tmdb".to_owned()), "{seen:?}");
}
