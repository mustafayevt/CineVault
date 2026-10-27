//! The streaming scrape preview through the generic engine: the diff of the search result
//! arrives first, the details fill in the rest, and the details can be asked again. A mock
//! provider, no network.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use cv_app::app_settings::SettingsService;
use cv_app::match_service::MatchService;
use cv_app::notices::NoticeHub;
use cv_app::provider_credentials::ProviderCredentialsService;
use cv_db::repo::insert_item;
use cv_db::{Database, DbOptions};
use cv_domain::{Item, ItemDetail, ItemId, ProviderIds};
use cv_jobs::{JobManager, JobStore, LaneConfig, Lanes, ManagerConfig};
use cv_providers::model::{Capabilities, MediaKind, RequestOptions, SearchHit, SearchQuery};
use cv_providers::{
    CredentialResolver, MemoryUserCredentials, MetadataProvider, Provider, ProviderError,
    ProviderRegistry,
};

use super::*;
use crate::provider_stream::{RetryOutcome, SearchRegistry, Sink};

/// Knows "Heat" and answers its details after a delay; the first details request fails.
struct Heat {
    asked: Mutex<u32>,
}

#[async_trait]
impl Provider for Heat {
    fn id(&self) -> &'static str {
        "omdb"
    }
    fn name(&self) -> &'static str {
        "OMDb"
    }
}

#[async_trait]
impl MetadataProvider for Heat {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            movies: true,
            shows: false,
            orderings: Vec::new(),
            lookup_ids: vec!["imdb"],
            best_effort: false,
        }
    }

    async fn search(
        &self,
        _query: &SearchQuery,
        _options: &RequestOptions,
    ) -> Result<Vec<SearchHit>, ProviderError> {
        Ok(Vec::new())
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
        tokio::time::sleep(Duration::from_millis(150)).await;
        let attempt = {
            let mut asked = self.asked.lock().unwrap();
            *asked += 1;
            *asked
        };
        if attempt == 1 {
            return Err(ProviderError::Request {
                provider: "OMDb".to_owned(),
                message: "timed out".to_owned(),
            });
        }
        let mut item = Item::movie("Heat");
        item.core.year = 1995;
        item.core.plot = "A group of professional bank robbers.".to_owned();
        item.core.artwork_urls.insert(
            cv_domain::MediaFileType::Poster,
            "https://img.test/p.jpg".to_owned(),
        );
        if let ItemDetail::Movie(movie) = &mut item.detail {
            movie.runtime_minutes = 170;
        }
        Ok(item)
    }
}

type Events = Arc<Mutex<Vec<ProviderEvent<FieldDiffDto>>>>;

fn collector() -> (Events, Sink<FieldDiffDto>) {
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

fn rows(event: &ProviderEvent<FieldDiffDto>) -> Vec<&str> {
    match event {
        ProviderEvent::Results { items, .. } => items.iter().map(|d| d.field.as_str()).collect(),
        _ => Vec::new(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_hit_diff_comes_first_the_details_follow_and_a_failure_can_be_retried() {
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
    registry.register_metadata(Arc::new(Heat {
        asked: Mutex::new(0),
    }));
    let credentials = Arc::new(ProviderCredentialsService::with_resolver(
        CredentialResolver::new(Arc::new(MemoryUserCredentials::default())),
        registry,
    ));
    credentials.set("omdb", "api_key", "a-key").unwrap();
    let service = MatchService::new(
        Arc::clone(&db),
        manager,
        credentials,
        Arc::new(SettingsService::in_memory("")),
        Arc::new(NoticeHub::new()),
    );
    let item: ItemId = db
        .write(|tx| insert_item(tx, &Item::movie("heat dvdrip")))
        .unwrap();
    let candidate = PreviewCandidate {
        provider: "omdb".to_owned(),
        provider_id: "tt0113277".to_owned(),
        title: "Heat".to_owned(),
        year: Some(1995),
        runtime_minutes: Some(170),
        overview: None,
    };
    let stream = Arc::new(service.scrape_preview_stream(item, &candidate, "").unwrap());
    let phases: Vec<Arc<dyn SearchProvider<FieldDiffDto>>> = vec![
        Arc::new(HitPhase {
            stream: Arc::clone(&stream),
        }),
        Arc::new(DetailsPhase {
            stream,
            artwork: None,
        }),
    ];
    let searches = SearchRegistry::default();
    let (events, sink) = collector();
    searches.start(&Lanes::new(LaneConfig::default()), "p1", phases, sink);
    until("done", || {
        events.lock().unwrap().last() == Some(&ProviderEvent::Done)
    })
    .await;

    // Both are asked at once; the diff of the search result is there before the details fail.
    let seen = events.lock().unwrap().clone();
    assert_eq!(
        seen[0],
        ProviderEvent::Started {
            provider: "hit".to_owned()
        }
    );
    let hit = seen
        .iter()
        .position(|e| matches!(e, ProviderEvent::Results { provider, .. } if provider == "hit"))
        .expect("the hit diff");
    let failed = seen
        .iter()
        .position(|e| matches!(e, ProviderEvent::Failed { provider, .. } if provider == "details"))
        .expect("the first details request fails");
    assert!(hit < failed);
    assert_eq!(rows(&seen[hit]), ["title", "year", "runtime"]);
    let ProviderEvent::Failed { retryable, .. } = &seen[failed] else {
        panic!()
    };
    assert!(retryable);

    // Retrying the details sends the full diff, with the hit's fields again, and nothing else.
    events.lock().unwrap().clear();
    assert_eq!(searches.retry("p1", "details"), RetryOutcome::Started);
    until("second done", || {
        events.lock().unwrap().last() == Some(&ProviderEvent::Done)
    })
    .await;
    let again = events.lock().unwrap().clone();
    assert_eq!(again.len(), 3, "{again:?}");
    let all = rows(&again[1]);
    for field in ["title", "year", "runtime", "plot", "poster"] {
        assert!(all.contains(&field), "{field} in {all:?}");
    }
    let ProviderEvent::Results { items, .. } = &again[1] else {
        panic!()
    };
    let title = items.iter().find(|d| d.field == "title").unwrap();
    assert_eq!(title.change, FieldChangeDto::Changed);
    assert_eq!(title.group, "text");
    let poster = items.iter().find(|d| d.field == "poster").unwrap();
    assert_eq!(poster.group, "artwork");
    assert_eq!(poster.change, FieldChangeDto::Added);
    assert!(!poster.locked);
}

#[test]
fn a_preset_becomes_the_fields_apply_takes() {
    let only_empty =
        scrape_field_selection(MatchKindDto::Movie, ScrapePresetDto::OnlyEmpty, vec![]);
    assert!(only_empty.do_not_overwrite);
    assert!(only_empty.fields.contains(&"plot".to_owned()));

    let custom = scrape_field_selection(
        MatchKindDto::Show,
        ScrapePresetDto::Custom,
        vec![
            "rating".to_owned(),
            "fanart".to_owned(),
            "collection".to_owned(),
        ],
    );
    assert_eq!(
        custom.fields,
        ["rating", "fanart"],
        "a show has no collection"
    );
    assert!(!custom.do_not_overwrite);

    let artwork = scrape_field_selection(MatchKindDto::Movie, ScrapePresetDto::ArtworkOnly, vec![]);
    assert!(
        artwork
            .fields
            .iter()
            .all(|f| !["plot", "title", "rating"].contains(&f.as_str()))
    );
    assert!(artwork.fields.contains(&"poster".to_owned()));
}
