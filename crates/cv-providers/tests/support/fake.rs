//! An in-memory metadata provider for tests of the universal provider and the fallback chain.
//! It knows a few titles, finds them by the ID keys it declares, and records every call.

#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use cv_domain::{Item, ProviderIds};
use cv_providers::model::{Capabilities, MediaKind, RequestOptions, SearchHit, SearchQuery};
use cv_providers::{MetadataProvider, Provider, ProviderError};

pub struct Fake {
    id: &'static str,
    lookup_ids: Vec<&'static str>,
    movies: bool,
    shows: bool,
    titles: Vec<Item>,
    error: Option<ProviderError>,
    calls: Mutex<Vec<String>>,
}

impl Fake {
    /// A provider that understands `lookup_ids`, handles movies and shows, and knows `titles`.
    pub fn new(id: &'static str, lookup_ids: &[&'static str], titles: Vec<Item>) -> Self {
        Self {
            id,
            lookup_ids: lookup_ids.to_vec(),
            movies: true,
            shows: true,
            titles,
            error: None,
            calls: Mutex::new(Vec::new()),
        }
    }

    pub fn movies_only(mut self) -> Self {
        self.shows = false;
        self
    }

    /// Every call answers with this error.
    pub fn failing(mut self, error: ProviderError) -> Self {
        self.error = Some(error);
        self
    }

    pub fn shared(self) -> Arc<Self> {
        Arc::new(self)
    }

    /// The calls so far, for example `details tmdb=603`.
    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    fn record(&self, what: &str) {
        self.calls.lock().unwrap().push(what.to_owned());
    }

    fn find(&self, kind: MediaKind, ids: &ProviderIds) -> Option<&Item> {
        self.titles.iter().find(|item| {
            let right_kind = match kind {
                MediaKind::Movie => item.kind() == cv_domain::ItemKind::Movie,
                MediaKind::Show => item.kind() == cv_domain::ItemKind::Show,
            };
            right_kind
                && self
                    .lookup_ids
                    .iter()
                    .any(|key| ids.get(key).is_some() && ids.get(key) == item.core.ids.get(key))
        })
    }

    fn hit(&self, item: &Item) -> SearchHit {
        SearchHit {
            provider: self.id.to_owned(),
            ids: item.core.ids.clone(),
            title: item.core.title.clone(),
            year: item.core.year,
            overview: item.core.plot.clone(),
            ..SearchHit::default()
        }
    }
}

pub fn ids(pairs: &[(&str, &str)]) -> ProviderIds {
    pairs.iter().copied().collect()
}

#[async_trait]
impl Provider for Fake {
    fn id(&self) -> &'static str {
        self.id
    }

    fn name(&self) -> &'static str {
        self.id
    }
}

#[async_trait]
impl MetadataProvider for Fake {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            movies: self.movies,
            shows: self.shows,
            orderings: Vec::new(),
            lookup_ids: self.lookup_ids.clone(),
            best_effort: false,
        }
    }

    async fn search(
        &self,
        query: &SearchQuery,
        _options: &RequestOptions,
    ) -> Result<Vec<SearchHit>, ProviderError> {
        self.record(&format!("search {}", query.title));
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        let wanted = query.title.to_lowercase();
        Ok(self
            .titles
            .iter()
            .filter(|item| item.core.title.to_lowercase().contains(&wanted))
            .map(|item| self.hit(item))
            .collect())
    }

    async fn lookup(
        &self,
        kind: MediaKind,
        ids: &ProviderIds,
        _options: &RequestOptions,
    ) -> Result<Option<SearchHit>, ProviderError> {
        self.record("lookup");
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        Ok(self.find(kind, ids).map(|item| self.hit(item)))
    }

    async fn details(
        &self,
        kind: MediaKind,
        ids: &ProviderIds,
        _options: &RequestOptions,
    ) -> Result<Item, ProviderError> {
        let given: Vec<String> = ids.iter().map(|(k, v)| format!("{k}={v}")).collect();
        self.record(&format!("details {}", given.join(",")));
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        self.find(kind, ids)
            .cloned()
            .ok_or_else(|| ProviderError::NotFound(self.id.to_owned()))
    }
}
