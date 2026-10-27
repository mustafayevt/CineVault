//! Streaming scrape candidates: every enabled metadata provider is asked for the
//! title, hits that share an IMDb, TMDB or TVDB ID are one entry, titles already in the library
//! are marked, and an empty result says why. Mock providers, no network.

mod support;

use std::sync::Arc;

use async_trait::async_trait;
use cv_app::app_settings::SettingsService;
use cv_app::match_service::{MatchService, NothingFound, ScrapeAnswer};
use cv_app::notices::NoticeHub;
use cv_app::provider_credentials::ProviderCredentialsService;
use cv_domain::{Item, ItemId, ItemKind, ProviderIds};
use cv_providers::model::{Capabilities, MediaKind, RequestOptions, SearchHit, SearchQuery};
use cv_providers::{
    CredentialResolver, MemoryUserCredentials, MetadataProvider, Provider, ProviderError,
    ProviderRegistry,
};
use support::Lab;

type Titles = Vec<(
    &'static str,
    Option<&'static str>,
    Option<&'static str>,
    &'static str,
    u16,
)>;

/// How a mock provider answers a search.
#[derive(Clone)]
enum Reply {
    /// These titles: (provider's own ID, IMDb ID in the hit, IMDb ID only in the details, title,
    /// year).
    Titles(Titles),
    Unreachable,
}

struct Mock {
    id: &'static str,
    name: &'static str,
    lookup: Vec<&'static str>,
    reply: Reply,
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

impl Mock {
    fn hit(&self, entry: &(&str, Option<&str>, Option<&str>, &str, u16)) -> SearchHit {
        let mut ids = ProviderIds::new();
        // The provider's own ID is stored under its first lookup key.
        ids.set(self.lookup[0], entry.0);
        if let Some(imdb) = entry.1 {
            ids.set("imdb", imdb);
        }
        SearchHit {
            provider: self.id.to_owned(),
            ids,
            title: entry.3.to_owned(),
            // "The Matrix" is known in its original language as "Matrix"; the others by their
            // own title.
            original_title: if entry.3 == "The Matrix" {
                "Matrix".to_owned()
            } else {
                entry.3.to_owned()
            },
            year: entry.4,
            ..SearchHit::default()
        }
    }
}

#[async_trait]
impl MetadataProvider for Mock {
    fn languages(&self) -> &'static [&'static str] {
        // The mock OFDb answers in German, as the real one does.
        if self.id == "ofdb" { &["de"] } else { &[] }
    }

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
        match &self.reply {
            Reply::Titles(titles) => Ok(titles.iter().map(|t| self.hit(t)).collect()),
            Reply::Unreachable => Err(ProviderError::Request {
                provider: self.name.to_owned(),
                message: "connection reset".to_owned(),
            }),
        }
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
        ids: &ProviderIds,
        _options: &RequestOptions,
    ) -> Result<Item, ProviderError> {
        let Reply::Titles(titles) = &self.reply else {
            return Err(ProviderError::NotFound(self.name.to_owned()));
        };
        let own = ids.get(self.lookup[0]).unwrap_or_default();
        let entry = titles
            .iter()
            .find(|t| t.0 == own)
            .ok_or_else(|| ProviderError::NotFound(self.name.to_owned()))?;
        let mut item = Item::movie(entry.3);
        item.core.year = entry.4;
        item.core.ids.set(self.lookup[0], entry.0);
        if let Some(imdb) = entry.1.or(entry.2) {
            item.core.ids.set("imdb", imdb);
        }
        Ok(item)
    }
}

struct Rig {
    lab: Lab,
    service: Arc<MatchService>,
}

fn rig(providers: Vec<Mock>, keys: &[&str]) -> Rig {
    let lab = Lab::new();
    let mut registry = ProviderRegistry::new();
    for provider in providers {
        registry.register_metadata(Arc::new(provider));
    }
    let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
    let credentials = Arc::new(ProviderCredentialsService::with_resolver(
        resolver, registry,
    ));
    for key in keys {
        credentials.set(key, "api_key", "a-key").unwrap();
    }
    let service = MatchService::new(
        Arc::clone(&lab.db),
        lab.manager.clone(),
        credentials,
        Arc::new(SettingsService::in_memory("")),
        Arc::new(NoticeHub::new()),
    );
    Rig { lab, service }
}

fn matrix_tmdb() -> Mock {
    // A TMDB search result has no IMDb ID; its details do.
    let titles: Titles = vec![
        ("603", None, Some("tt0133093"), "The Matrix", 1999),
        ("604", None, Some("tt0234215"), "The Matrix Reloaded", 2003),
    ];
    Mock {
        id: "tmdb",
        name: "TMDB",
        lookup: vec!["tmdb", "imdb"],
        reply: Reply::Titles(titles),
    }
}

fn matrix_omdb() -> Mock {
    let titles: Titles = vec![
        ("tt0133093", Some("tt0133093"), None, "The Matrix", 1999),
        (
            "tt0295432",
            Some("tt0295432"),
            None,
            "The Matrix Revisited",
            2001,
        ),
    ];
    Mock {
        id: "omdb",
        name: "OMDb",
        lookup: vec!["imdb"],
        reply: Reply::Titles(titles),
    }
}

fn movie(rig: &Rig) -> ItemId {
    rig.lab.movie("The Matrix", 1999)
}

async fn answer(rig: &Rig, item: ItemId, key: &str) -> ScrapeAnswer {
    let stream = rig
        .service
        .scrape_candidate_stream(item, "The Matrix", ItemKind::Movie, "")
        .unwrap();
    stream
        .sources
        .iter()
        .find(|s| s.key == key)
        .unwrap_or_else(|| panic!("no source {key}"))
        .fetch()
        .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hits_with_the_same_imdb_id_merge_into_one_entry_and_scores_keep_sorting() {
    let rig = rig(vec![matrix_tmdb(), matrix_omdb()], &["tmdb", "omdb"]);
    let item = movie(&rig);
    let stream = rig
        .service
        .scrape_candidate_stream(item, "The Matrix", ItemKind::Movie, "")
        .unwrap();
    assert_eq!(
        stream
            .sources
            .iter()
            .map(|s| s.key.as_str())
            .collect::<Vec<_>>(),
        ["tmdb", "omdb"]
    );

    // TMDB answers first: two entries, the better match first.
    let ScrapeAnswer::Hits(first) = stream.sources[0].fetch().await else {
        panic!("tmdb did not answer");
    };
    assert_eq!(first.len(), 2);
    assert_eq!(first[0].candidate.title, "The Matrix");
    // The original title comes along when it differs from the title, and only then.
    assert_eq!(first[0].candidate.original_title.as_deref(), Some("Matrix"));
    assert_eq!(first[1].candidate.original_title, None);
    assert!(first[0].confidence() >= first[1].confidence());
    assert_eq!(first[0].providers.len(), 1);
    let matrix_id = first[0].id.clone();

    // OMDb knows the same film by the same IMDb ID: it joins the entry, and adds its own.
    let ScrapeAnswer::Hits(second) = stream.sources[1].fetch().await else {
        panic!("omdb did not answer");
    };
    let joined = second.iter().find(|h| h.id == matrix_id).expect("joined");
    let providers: Vec<&str> = joined
        .providers
        .iter()
        .map(|p| p.provider.as_str())
        .collect();
    assert_eq!(providers.len(), 2, "{providers:?}");
    assert!(providers.contains(&"tmdb") && providers.contains(&"omdb"));
    assert!(joined.ids.contains(&"imdb:tt0133093".to_owned()));
    assert!(joined.ids.contains(&"tmdb:603".to_owned()));
    let own_ids: Vec<(&str, &str)> = joined
        .providers
        .iter()
        .map(|p| (p.provider.as_str(), p.provider_id.as_str()))
        .collect();
    assert!(own_ids.contains(&("tmdb", "603")) && own_ids.contains(&("omdb", "tt0133093")));
    // The highest confidence of the two is kept.
    let best = joined
        .providers
        .iter()
        .map(|p| p.confidence)
        .fold(0.0_f64, f64::max);
    assert!((joined.confidence() - best).abs() < f64::EPSILON);
    // The other OMDb title is a new entry; everything sent is sorted by confidence.
    assert!(
        second
            .iter()
            .any(|h| h.candidate.title == "The Matrix Revisited")
    );
    assert!(
        second
            .windows(2)
            .all(|w| w[0].confidence() >= w[1].confidence())
    );
    assert_eq!(stream.summary().found(), 3);
    assert!(stream.summary().nothing_found().is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_later_hit_that_bridges_two_entries_absorbs_one() {
    // TMDB knows the film by its TMDB ID only, OMDb by its IMDb ID only ...
    let tmdb = Mock {
        id: "tmdb",
        name: "TMDB",
        lookup: vec!["tmdb", "imdb"],
        reply: Reply::Titles(vec![("603", None, None, "The Matrix", 1999)]),
    };
    let omdb = Mock {
        id: "omdb",
        name: "OMDb",
        lookup: vec!["imdb"],
        reply: Reply::Titles(vec![(
            "tt0133093",
            Some("tt0133093"),
            None,
            "The Matrix",
            1999,
        )]),
    };
    // ... and a third provider's hit carries both IDs.
    let bridge = Mock {
        id: "bridge",
        name: "Bridge",
        lookup: vec!["tmdb", "imdb"],
        reply: Reply::Titles(vec![("603", Some("tt0133093"), None, "The Matrix", 1999)]),
    };
    let rig = rig(vec![tmdb, omdb, bridge], &["tmdb", "omdb"]);
    let item = movie(&rig);
    let stream = rig
        .service
        .scrape_candidate_stream(item, "The Matrix", ItemKind::Movie, "")
        .unwrap();

    let ScrapeAnswer::Hits(a) = stream.sources[0].fetch().await else {
        panic!()
    };
    let ScrapeAnswer::Hits(b) = stream.sources[1].fetch().await else {
        panic!()
    };
    // Nothing is shared yet: two entries.
    assert_ne!(a[0].id, b[0].id);
    assert!(b[0].absorbs.is_empty());
    assert_eq!(stream.summary().found(), 2);

    let ScrapeAnswer::Hits(c) = stream.sources[2].fetch().await else {
        panic!()
    };
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].providers.len(), 3);
    assert_eq!(c[0].id, a[0].id, "the older entry stays");
    assert_eq!(
        c[0].absorbs,
        [b[0].id.clone()],
        "the page drops the other one"
    );
    assert_eq!(stream.summary().found(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_entry_already_linked_to_another_item_says_which() {
    let rig = rig(vec![matrix_tmdb()], &["tmdb"]);
    let mut linked = Item::movie("The Matrix");
    linked.core.year = 1999;
    linked.core.ids.set("tmdb", "603");
    rig.lab.add(linked);
    let item = movie(&rig);

    let ScrapeAnswer::Hits(hits) = answer(&rig, item, "tmdb").await else {
        panic!()
    };

    let matrix = hits
        .iter()
        .find(|h| h.candidate.title == "The Matrix")
        .unwrap();
    assert_eq!(
        matrix.in_library.as_ref().unwrap().name,
        "The Matrix (1999)"
    );
    let reloaded = hits
        .iter()
        .find(|h| h.candidate.title.ends_with("Reloaded"))
        .unwrap();
    assert!(reloaded.in_library.is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_provider_without_its_key_is_listed_and_answers_needs_key() {
    // OMDb has a key; TMDB does not.
    let rig = rig(vec![matrix_tmdb(), matrix_omdb()], &["omdb"]);
    let item = movie(&rig);
    let stream = rig
        .service
        .scrape_candidate_stream(item, "", ItemKind::Movie, "")
        .unwrap();

    assert_eq!(
        stream
            .sources
            .iter()
            .map(|s| s.key.as_str())
            .collect::<Vec<_>>(),
        ["omdb", "tmdb"],
        "ready providers first, then the ones that wait for a key"
    );
    assert_eq!(
        stream.query, "The Matrix",
        "a blank search uses the item's title"
    );
    assert_eq!(stream.sources[1].fetch().await, ScrapeAnswer::NeedsKey);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nothing_found_says_why() {
    // No key anywhere.
    let rig_keys = rig(vec![matrix_tmdb()], &[]);
    let item = movie(&rig_keys);
    let stream = rig_keys
        .service
        .scrape_candidate_stream(item, "Heat", ItemKind::Movie, "")
        .unwrap();
    for source in &stream.sources {
        let _ = source.fetch().await;
    }
    let (reason, message) = stream.summary().nothing_found().unwrap();
    assert_eq!(reason, NothingFound::NoKey);
    assert!(
        message.contains("Settings, Scraping and providers"),
        "{message}"
    );

    // Answered, but knows no such title.
    let empty = Mock {
        id: "omdb",
        name: "OMDb",
        lookup: vec!["imdb"],
        reply: Reply::Titles(Vec::new()),
    };
    let rig_empty = rig(vec![empty], &["omdb"]);
    let item = movie(&rig_empty);
    let stream = rig_empty
        .service
        .scrape_candidate_stream(item, "Zzyzx Road", ItemKind::Movie, "")
        .unwrap();
    for source in &stream.sources {
        assert_eq!(source.fetch().await, ScrapeAnswer::Hits(Vec::new()));
    }
    let (reason, message) = stream.summary().nothing_found().unwrap();
    assert_eq!(reason, NothingFound::NotFound);
    assert!(message.contains("Zzyzx Road"), "{message}");

    // Nobody could be reached.
    let down = Mock {
        id: "omdb",
        name: "OMDb",
        lookup: vec!["imdb"],
        reply: Reply::Unreachable,
    };
    let rig_down = rig(vec![down], &["omdb"]);
    let item = movie(&rig_down);
    let stream = rig_down
        .service
        .scrape_candidate_stream(item, "Heat", ItemKind::Movie, "")
        .unwrap();
    let ScrapeAnswer::Failed { retryable, .. } = stream.sources[0].fetch().await else {
        panic!("expected a failure");
    };
    assert!(retryable);
    assert_eq!(
        stream.summary().nothing_found().unwrap().0,
        NothingFound::Offline
    );

    // No provider handles shows at all.
    let stream = rig_down
        .service
        .scrape_candidate_stream(item, "Heat", ItemKind::Show, "")
        .unwrap();
    assert!(stream.sources.is_empty());
    assert_eq!(
        stream.summary().nothing_found().unwrap().0,
        NothingFound::NoProvider
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_search_needs_something_to_search_for_and_an_item_that_can_be_scraped() {
    let rig = rig(vec![matrix_tmdb()], &["tmdb"]);
    let show = rig.lab.show("Dark", 2017);
    let season = rig.lab.add(Item::season(show, 1));
    let error = rig
        .service
        .scrape_candidate_stream(season, "Dark", ItemKind::Show, "")
        .unwrap_err();
    assert_eq!(error.code(), "unsupported");

    let blank = rig.lab.add(Item::movie("   "));
    let error = rig
        .service
        .scrape_candidate_stream(blank, "", ItemKind::Movie, "")
        .unwrap_err();
    assert_eq!(error.code(), "invalid");
}

/// A German-only provider that is the most confident of an entry does not lead it in
/// an English scrape, so choosing the entry scrapes the English provider; the entry keeps the
/// highest confidence.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_provider_in_another_language_never_leads_a_merged_entry() {
    let tmdb = Mock {
        id: "tmdb",
        name: "TMDB",
        lookup: vec!["tmdb", "imdb"],
        reply: Reply::Titles(vec![(
            "603",
            Some("tt0133093"),
            None,
            "The Matrix Film",
            1999,
        )]),
    };
    let ofdb = Mock {
        id: "ofdb",
        name: "OFDb",
        lookup: vec!["ofdb", "imdb"],
        reply: Reply::Titles(vec![("1234", Some("tt0133093"), None, "The Matrix", 1999)]),
    };
    let rig = rig(vec![ofdb, tmdb], &["tmdb"]);
    let item = movie(&rig);

    for language in ["en", "de"] {
        let stream = rig
            .service
            .scrape_candidate_stream(item, "The Matrix", ItemKind::Movie, language)
            .unwrap();
        let mut last = Vec::new();
        for source in &stream.sources {
            if let ScrapeAnswer::Hits(hits) = source.fetch().await {
                last = hits;
            }
        }
        let entry = &last[0];
        assert_eq!(entry.providers.len(), 2, "{entry:?}");
        let best = entry
            .providers
            .iter()
            .map(|p| p.confidence)
            .fold(0.0_f64, f64::max);
        let ofdb_confidence = entry
            .providers
            .iter()
            .find(|p| p.provider == "ofdb")
            .unwrap()
            .confidence;
        assert!((ofdb_confidence - best).abs() < f64::EPSILON, "{entry:?}");
        assert!((entry.confidence() - best).abs() < f64::EPSILON);
        let lead = if language == "en" { "tmdb" } else { "ofdb" };
        assert_eq!(entry.candidate.provider, lead, "{language}");
        assert_eq!(entry.providers[0].provider, lead, "{language}");
    }
}

/// One provider that lists hundreds of loose hits does not flood the list.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_provider_lists_its_best_hits_only() {
    let many: Titles = (0..200)
        .map(|n| {
            let id: &'static str = Box::leak(format!("{n}").into_boxed_str());
            let title: &'static str = Box::leak(format!("The Matrix {n}").into_boxed_str());
            (id, None, None, title, 1999)
        })
        .collect();
    let flood = Mock {
        id: "omdb",
        name: "OMDb",
        lookup: vec!["imdb"],
        reply: Reply::Titles(many),
    };
    let rig = rig(vec![flood], &["omdb"]);
    let item = movie(&rig);
    let ScrapeAnswer::Hits(hits) = answer(&rig, item, "omdb").await else {
        panic!("omdb did not answer");
    };
    assert_eq!(hits.len(), cv_app::match_service::MAX_HITS_PER_SOURCE);
}
