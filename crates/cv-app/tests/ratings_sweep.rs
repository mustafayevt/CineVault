//! "Fetch missing ratings": the library-wide ratings sweep runs as one job over the scraped titles
//! that lack a usable source's ratings, and stops calmly at a source's request limit. The rating
//! sources are in-memory fakes; nothing here touches the network.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use cv_app::actions::RatingSources;
use cv_app::data_location::{DataLocation, LocationInputs};
use cv_app::rating_status::SweepScope;
use cv_app::ratings_sweep::submit_sweep;
use cv_app::secrets::SecretStore;
use cv_app::services::{AppServices, BootOptions};
use cv_db::repo::{get_item, insert_item};
use cv_domain::{Item, ItemId, ProviderIds, Rating, Ratings};
use cv_providers::model::{MediaKind, RequestOptions};
use cv_providers::{
    CredentialResolver, MemoryUserCredentials, Provider, ProviderError, RatingFetcher,
    RatingProvider,
};
use serde_json::json;
use tempfile::TempDir;

/// A rating source that gives the same ratings to every title, or answers with its request
/// limit, counting how often it was asked.
struct Source {
    id: &'static str,
    ratings: Vec<Rating>,
    limited: bool,
    asked: AtomicUsize,
}

#[async_trait]
impl Provider for Source {
    fn id(&self) -> &'static str {
        self.id
    }
    fn name(&self) -> &'static str {
        self.id
    }
}

#[async_trait]
impl RatingProvider for Source {
    async fn ratings(
        &self,
        _kind: MediaKind,
        _ids: &ProviderIds,
        _options: &RequestOptions,
    ) -> Result<Ratings, ProviderError> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        if self.limited {
            return Err(ProviderError::RateLimited {
                provider: "OMDb".to_owned(),
                retry_after_secs: 3600,
            });
        }
        Ok(self.ratings.iter().cloned().collect())
    }
}

fn source(id: &'static str, ratings: &[Rating], limited: bool) -> Arc<Source> {
    Arc::new(Source {
        id,
        ratings: ratings.to_vec(),
        limited,
        asked: AtomicUsize::new(0),
    })
}

fn location(root: &Path) -> DataLocation {
    let inputs = LocationInputs {
        exe_dir: root.join("app"),
        user_data_base: root.join("userdata"),
        user_config_base: root.join("userconfig"),
    };
    std::fs::create_dir_all(&inputs.exe_dir).unwrap();
    DataLocation::resolve(&inputs)
}

struct Rig {
    _temp: TempDir,
    services: Arc<AppServices>,
    resolver: CredentialResolver,
}

impl Rig {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let mut options =
            BootOptions::new(location(temp.path()), Arc::new(SecretStore::in_memory()));
        options.change_window = std::time::Duration::ZERO;
        let services = AppServices::boot(options).expect("boot");
        let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
        Self {
            _temp: temp,
            services,
            resolver,
        }
    }

    fn use_sources(&self, fetcher: RatingFetcher, chosen: &str) {
        self.services.actions().set_rating_sources(RatingSources {
            fetcher,
            dataset: None,
            scrapers: Vec::new(),
        });
        self.services
            .settings()
            .set("ratings.sources-movie", json!(chosen))
            .unwrap();
    }

    /// A scraped movie (it has an IMDb ID).
    fn movie(&self, title: &str, imdb: &str) -> ItemId {
        let mut item = Item::movie(title);
        item.core.ids.set("imdb", imdb);
        self.services
            .db()
            .write(move |tx| insert_item(tx, &item))
            .unwrap()
    }

    fn unscraped(&self, title: &str) -> ItemId {
        let item = Item::movie(title);
        self.services
            .db()
            .write(move |tx| insert_item(tx, &item))
            .unwrap()
    }

    fn missing(&self) -> Vec<ItemId> {
        self.services
            .rating_status()
            .sweep_titles(SweepScope::Missing, None)
            .unwrap()
    }

    async fn sweep(&self, scope: SweepScope) -> cv_app::ratings_sweep::SweepSummary {
        let (job, work, _) = submit_sweep(
            self.services.jobs(),
            self.services.actions(),
            self.services.rating_status(),
            self.services.db(),
            scope,
            None,
        )
        .unwrap()
        .expect("titles to fetch");
        job.wait().await;
        work.summary()
    }
}

#[tokio::test]
async fn the_sweep_fetches_the_titles_that_lack_ratings_and_then_has_nothing_left() {
    let rig = Rig::new();
    let dataset = source("dataset", &[Rating::new("imdb", 8.3, 700_000, 10)], false);
    rig.use_sources(
        RatingFetcher::new(rig.resolver.clone()).with_dataset(Arc::clone(&dataset) as _),
        "imdb-dataset",
    );
    let heat = rig.movie("Heat", "tt0113277");
    let alien = rig.movie("Alien", "tt0078748");
    // A title never scraped has nothing a source could look it up by.
    rig.unscraped("Home video");

    assert_eq!(rig.missing(), vec![heat, alien]);
    let summary = rig.sweep(SweepScope::Missing).await;
    assert_eq!((summary.updated, summary.failed, summary.left), (2, 0, 0));
    assert_eq!(dataset.asked.load(Ordering::SeqCst), 2);
    let item = rig
        .services
        .db()
        .read(|c| get_item(c, heat))
        .unwrap()
        .unwrap();
    assert_eq!(item.core.ratings.get("imdb").unwrap().value, 8.3);

    // Every source answered: nothing is missing any more, so nothing is asked again.
    assert!(rig.missing().is_empty());
    assert!(
        submit_sweep(
            rig.services.jobs(),
            rig.services.actions(),
            rig.services.rating_status(),
            rig.services.db(),
            SweepScope::Missing,
            None,
        )
        .unwrap()
        .is_none()
    );
    // "All titles" refreshes them all the same.
    let again = rig.sweep(SweepScope::All).await;
    assert_eq!(again.updated + again.unchanged, 2);
}

#[tokio::test]
async fn a_request_limit_stops_the_sweep_calmly_and_says_what_is_left() {
    let rig = Rig::new();
    let omdb = source("omdb", &[], true);
    rig.use_sources(
        RatingFetcher::new(rig.resolver.clone()).with_omdb(Arc::clone(&omdb) as _),
        "omdb",
    );
    for (title, imdb) in [
        ("Heat", "tt0113277"),
        ("Alien", "tt0078748"),
        ("Ran", "tt0089881"),
    ] {
        rig.movie(title, imdb);
    }

    let summary = rig.sweep(SweepScope::All).await;
    assert_eq!(summary.limited_by.as_deref(), Some("OMDb"));
    assert_eq!(summary.left, 2, "{summary:?}");
    assert!(!summary.cancelled);
    // Only the first title was asked; the rest waits for another run.
    assert_eq!(omdb.asked.load(Ordering::SeqCst), 1);
    assert!(
        summary
            .sentence()
            .contains("Stopped at the OMDb request limit: 2 titles are left"),
        "{}",
        summary.sentence()
    );
}
