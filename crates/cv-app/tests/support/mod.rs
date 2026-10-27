//! Shared helpers for the batch matching tests: a scriptable provider with no network, a
//! temporary library, and a job manager with a job store.

#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use cv_app::matching::MatchSettings;
use cv_app::matching::batch::{MatchContext, MatchProviders};
use cv_db::repo::{get_item, insert_item};
use cv_db::{Database, DbOptions};
use cv_domain::{
    AudioStream, Item, ItemDetail, ItemId, ItemKind, MediaFile, MediaFileType, ProviderIds,
    StreamFlag,
};
use cv_jobs::{JobManager, JobStore, LaneConfig, Lanes, ManagerConfig};
use cv_providers::model::{
    Capabilities, MediaKind, ProviderCollection, RequestOptions, SearchHit, SearchQuery,
};
use cv_providers::{MetadataProvider, Provider, ProviderError};

/// One title the scripted provider knows.
#[derive(Debug, Clone)]
pub struct Entry {
    pub tmdb: String,
    pub imdb: String,
    pub title: String,
    pub year: u16,
    pub runtime: u32,
    /// Lower-case query texts that find this entry.
    pub found_by: Vec<String>,
}

impl Entry {
    pub fn new(tmdb: &str, title: &str, year: u16) -> Self {
        Self {
            tmdb: tmdb.to_owned(),
            imdb: format!("tt{tmdb:0>7}"),
            title: title.to_owned(),
            year,
            runtime: 100,
            found_by: vec![title.to_lowercase()],
        }
    }

    pub fn runtime(mut self, minutes: u32) -> Self {
        self.runtime = minutes;
        self
    }

    /// Replaces the queries that find the entry.
    pub fn found_by(mut self, query: &str) -> Self {
        self.found_by = vec![query.to_lowercase()];
        self
    }

    /// Adds a query that finds the entry as well.
    pub fn also(mut self, query: &str) -> Self {
        self.found_by.push(query.to_lowercase());
        self
    }
}

/// A provider that answers from a fixed catalogue and counts what it was asked.
pub struct Scripted {
    id: &'static str,
    movies: bool,
    shows: bool,
    entries: Vec<Entry>,
    by_query: HashMap<String, Vec<usize>>,
    by_tmdb: HashMap<String, usize>,
    pub latency: Duration,
    /// Block the calling thread this long in a search (the negative control).
    pub block: Duration,
    pub searches: AtomicU64,
    pub lookups: AtomicU64,
    pub details: AtomicU64,
    pub bypass_requests: AtomicU64,
    /// Queries (lower case) whose search fails with a request error.
    pub failing: Mutex<HashSet<String>>,
    /// Queries (lower case) whose search fails because the provider has no key.
    pub keyless: Mutex<HashSet<String>>,
    /// The one collection the provider knows (`tmdbSet` ID and name), when it knows collections.
    collection: Option<(String, String)>,
}

impl Scripted {
    pub fn new(id: &'static str, entries: Vec<Entry>) -> Self {
        let mut by_query: HashMap<String, Vec<usize>> = HashMap::new();
        let mut by_tmdb = HashMap::new();
        for (i, e) in entries.iter().enumerate() {
            by_tmdb.insert(e.tmdb.clone(), i);
            for q in &e.found_by {
                by_query.entry(q.clone()).or_default().push(i);
            }
        }
        Self {
            id,
            movies: true,
            shows: true,
            entries,
            by_query,
            by_tmdb,
            latency: Duration::ZERO,
            block: Duration::ZERO,
            searches: AtomicU64::default(),
            lookups: AtomicU64::default(),
            details: AtomicU64::default(),
            bypass_requests: AtomicU64::default(),
            failing: Mutex::default(),
            keyless: Mutex::default(),
            collection: None,
        }
    }

    /// Makes the provider know one collection, by its `tmdbSet` ID.
    pub fn with_collection(mut self, tmdb_set: &str, name: &str) -> Self {
        self.collection = Some((tmdb_set.to_owned(), name.to_owned()));
        self
    }

    pub fn kinds(mut self, movies: bool, shows: bool) -> Self {
        self.movies = movies;
        self.shows = shows;
        self
    }

    pub fn latency(mut self, latency: Duration) -> Self {
        self.latency = latency;
        self
    }

    pub fn blocking(mut self, block: Duration) -> Self {
        self.block = block;
        self
    }

    pub fn fail_search(&self, query: &str) {
        self.failing.lock().unwrap().insert(query.to_lowercase());
    }

    /// Makes the search for `query` fail as a provider without its key does.
    pub fn need_key_for(&self, query: &str) {
        self.keyless.lock().unwrap().insert(query.to_lowercase());
    }

    pub fn count(counter: &AtomicU64) -> u64 {
        counter.load(Ordering::Relaxed)
    }

    async fn pause(&self) {
        if !self.latency.is_zero() {
            tokio::time::sleep(self.latency).await;
        }
    }

    fn hit(&self, e: &Entry) -> SearchHit {
        let mut ids = ProviderIds::new();
        ids.set("tmdb", &e.tmdb);
        SearchHit {
            provider: self.id.to_owned(),
            ids,
            title: e.title.clone(),
            year: e.year,
            poster_url: Some(format!("https://img.invalid/{}.jpg", e.tmdb)),
            ..SearchHit::default()
        }
    }

    fn entry_of(&self, ids: &ProviderIds) -> Option<&Entry> {
        if let Some(i) = ids.get("tmdb").and_then(|t| self.by_tmdb.get(t)) {
            return Some(&self.entries[*i]);
        }
        let imdb = ids.get("imdb")?;
        self.entries.iter().find(|e| e.imdb == imdb)
    }
}

#[async_trait]
impl Provider for Scripted {
    fn id(&self) -> &'static str {
        self.id
    }
    fn name(&self) -> &'static str {
        "Scripted"
    }
}

#[async_trait]
impl MetadataProvider for Scripted {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            movies: self.movies,
            shows: self.shows,
            orderings: Vec::new(),
            lookup_ids: vec!["tmdb", "imdb"],
            best_effort: false,
        }
    }

    async fn search(
        &self,
        query: &SearchQuery,
        _options: &RequestOptions,
    ) -> Result<Vec<SearchHit>, ProviderError> {
        self.searches.fetch_add(1, Ordering::Relaxed);
        if !self.block.is_zero() {
            std::thread::sleep(self.block);
        }
        self.pause().await;
        let text = query.title.to_lowercase();
        if self.failing.lock().unwrap().contains(&text) {
            return Err(ProviderError::Request {
                provider: "Scripted".to_owned(),
                message: "connection reset".to_owned(),
            });
        }
        if self.keyless.lock().unwrap().contains(&text) {
            return Err(ProviderError::MissingCredentials {
                provider: "TMDB".to_owned(),
                fields: vec!["api_key".to_owned()],
                registration_url: String::new(),
            });
        }
        Ok(self
            .by_query
            .get(&text)
            .map(|indexes| {
                indexes
                    .iter()
                    .map(|i| self.hit(&self.entries[*i]))
                    .collect()
            })
            .unwrap_or_default())
    }

    async fn lookup(
        &self,
        _kind: MediaKind,
        ids: &ProviderIds,
        _options: &RequestOptions,
    ) -> Result<Option<SearchHit>, ProviderError> {
        self.lookups.fetch_add(1, Ordering::Relaxed);
        self.pause().await;
        Ok(self.entry_of(ids).map(|e| self.hit(e)))
    }

    async fn details(
        &self,
        kind: MediaKind,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Item, ProviderError> {
        self.details.fetch_add(1, Ordering::Relaxed);
        if options.bypass_cache {
            self.bypass_requests.fetch_add(1, Ordering::Relaxed);
        }
        self.pause().await;
        let Some(e) = self.entry_of(ids) else {
            return Err(ProviderError::NotFound("Scripted".to_owned()));
        };
        let mut item = match kind {
            MediaKind::Movie => Item::movie(&e.title),
            MediaKind::Show => Item::show(&e.title),
        };
        item.core.year = e.year;
        item.core.plot = format!("Plot of {} ({})", e.title, e.year);
        item.core.ids.set("tmdb", &e.tmdb);
        item.core.ids.set("imdb", &e.imdb);
        match &mut item.detail {
            ItemDetail::Movie(m) => m.runtime_minutes = e.runtime,
            ItemDetail::Show(s) => s.runtime_minutes = e.runtime,
            _ => {}
        }
        Ok(item)
    }

    fn offers_collections(&self) -> bool {
        self.collection.is_some()
    }

    async fn collection(
        &self,
        ids: &ProviderIds,
        _options: &RequestOptions,
    ) -> Result<ProviderCollection, ProviderError> {
        let wanted = ids.get(cv_domain::id_key::TMDB_SET).unwrap_or_default();
        let Some((id, name)) = self.collection.as_ref().filter(|(id, _)| id == wanted) else {
            return Err(ProviderError::NotFound("Scripted".to_owned()));
        };
        let mut item = Item::set(name);
        item.core.ids.set(cv_domain::id_key::TMDB_SET, id);
        Ok(ProviderCollection {
            item,
            movies: Vec::new(),
        })
    }
}

/// A temporary library with a job manager that has a job store.
pub struct Lab {
    pub dir: tempfile::TempDir,
    pub db: Arc<Database>,
    pub manager: JobManager,
}

impl Lab {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().expect("temp dir");
        let db = Arc::new(
            Database::open_with(dir.path().join("library.db"), &DbOptions { readers: 4 })
                .expect("open database"),
        );
        let manager = JobManager::with_store(
            Lanes::new(LaneConfig::default()),
            ManagerConfig::default(),
            JobStore::new(Arc::clone(&db)),
        );
        Self { dir, db, manager }
    }

    pub fn context(&self, providers: Vec<Arc<dyn MetadataProvider>>) -> MatchContext {
        MatchContext {
            db: Arc::clone(&self.db),
            providers: MatchProviders::new(providers),
            settings: MatchSettings::default(),
            after_match: None,
        }
    }

    pub fn add(&self, item: Item) -> ItemId {
        self.db
            .write(move |tx| insert_item(tx, &item))
            .expect("insert item")
    }

    pub fn movie(&self, title: &str, year: u16) -> ItemId {
        let mut item = Item::movie(title);
        item.core.year = year;
        self.add(item)
    }

    pub fn show(&self, title: &str, year: u16) -> ItemId {
        let mut item = Item::show(title);
        item.core.year = year;
        self.add(item)
    }

    /// A movie whose main video lasts `minutes` and whose primary audio is `language`.
    pub fn movie_with_video(&self, title: &str, year: u16, minutes: u32, language: &str) -> ItemId {
        let mut item = Item::movie(title);
        item.core.year = year;
        let mut file = MediaFile::new(MediaFileType::Video, "/lib", "film.mkv");
        file.duration_secs = minutes * 60;
        file.audio_streams.push(AudioStream {
            language: language.to_owned(),
            flags: [StreamFlag::Default].into_iter().collect(),
            ..AudioStream::default()
        });
        item.core.media_files.push(file);
        self.add(item)
    }

    pub fn item(&self, id: ItemId) -> Item {
        self.db.read(|c| get_item(c, id)).unwrap().unwrap()
    }

    pub fn kind_of(&self, id: ItemId) -> ItemKind {
        self.item(id).kind()
    }
}

pub fn arc(provider: Scripted) -> Arc<Scripted> {
    Arc::new(provider)
}

pub fn dynamic(provider: &Arc<Scripted>) -> Arc<dyn MetadataProvider> {
    let p: Arc<dyn MetadataProvider> = provider.clone();
    p
}
