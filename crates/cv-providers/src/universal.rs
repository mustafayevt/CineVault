//! The universal provider: one title assembled from several providers, field by field.
//!
//! The user picks, per [`MetaField`], which provider supplies the value (title from TMDB, plot
//! from IMDb, ratings from OMDb, collection from TMDB, ...). A field with no choice comes from
//! the *search* provider. When the chosen provider has no value for a field, the ordered
//! [`UniversalConfig::fallbacks`] are tried and the first one that has a value wins.
//!
//! [`UniversalProvider::scrape`] works in rounds:
//!
//! 1. Every provider that is needed (the search provider, the field sources and the fallbacks)
//!    and that understands one of the IDs known so far is asked for its details, in parallel.
//!    The IDs each answer reports are added to the known ones, so TMDB's external IDs let an
//!    IMDb-only provider run in the next round.
//! 2. When providers are still waiting and none can run, [`complete_ids`] bridges the gap once,
//!    asking the other providers (TMDB, then TheTVDB, then the rest) for the IDs that are
//!    missing, and the rounds go on.
//! 3. A provider that has no such title, is inactive (no key) or fails is skipped and reported
//!    in [`UniversalScrape::failures`]; the other fields still come from the providers that
//!    answered. Only when nobody answered is the scrape an error.
//!
//! The values are merged with the same rules every scrape uses ([`cv_domain::apply_scrape`]), one
//! field at a time. The provider IDs of all answers are united. The result is a plain item: the
//! caller applies it to the stored one with the user's field selection, *do not overwrite* mode
//! and the field locks (`cv_db::repo::apply_scrape`), so locked items and fields stay untouched.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use cv_domain::{
    EpisodeGroup, FieldSet, Item, ItemKind, MetaField, ProviderIds, ScrapeOptions, all_fields,
    apply_scrape, id_key,
};
use serde::{Deserialize, Serialize};
use tokio::task::JoinSet;

use crate::completion::{AnimeCrossReference, can_resolve, complete_ids};
use crate::error::ProviderError;
use crate::model::{Capabilities, MediaKind, RequestOptions, SearchHit, SearchQuery};
use crate::traits::{MetadataProvider, Provider};

/// Provider key of the universal provider.
pub const UNIVERSAL_ID: &str = "universal";

/// Which provider supplies what.
///
/// Provider keys are the providers' [`Provider::id`] values (`tmdb`, `imdb`, `omdb`, ...). A key
/// that names no available provider is ignored, so a setting that outlives a provider does not
/// break scraping; [`UniversalProvider::unknown_sources`] lists such keys for Settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct UniversalConfig {
    /// The provider that searches, and that supplies every field with no choice of its own.
    /// Empty means the first available provider.
    pub search: String,
    /// The provider chosen for each field.
    pub fields: BTreeMap<MetaField, String>,
    /// Tried in order for a field the chosen provider has no value for.
    pub fallbacks: Vec<String>,
    /// The provider of episode lists. Empty means the search provider.
    pub episodes: String,
}

/// What a universal scrape produced.
#[derive(Debug, Clone)]
pub struct UniversalScrape {
    /// The combined title; `last_scraper_id` is `universal`.
    pub item: Item,
    /// The provider each filled field came from.
    pub sources: BTreeMap<MetaField, String>,
    /// Providers that were needed but gave nothing (key and error).
    pub failures: Vec<(String, ProviderError)>,
}

type Shared = Arc<dyn MetadataProvider>;

/// Combines the fields of several metadata providers.
pub struct UniversalProvider {
    config: UniversalConfig,
    providers: Vec<Shared>,
    anime: Option<Arc<AnimeCrossReference>>,
}

impl UniversalProvider {
    /// `providers` are the metadata providers it may use; a provider with the universal key is
    /// left out, and so is a second provider with the same key.
    pub fn new(config: UniversalConfig, providers: impl IntoIterator<Item = Shared>) -> Self {
        let mut unique: Vec<Shared> = Vec::new();
        for provider in providers {
            if provider.id() != UNIVERSAL_ID && !unique.iter().any(|p| p.id() == provider.id()) {
                unique.push(provider);
            }
        }
        Self {
            config,
            providers: unique,
            anime: None,
        }
    }

    /// Uses the offline anime ID cross-reference when bridging IDs.
    #[must_use]
    pub fn with_anime(mut self, anime: Arc<AnimeCrossReference>) -> Self {
        self.anime = Some(anime);
        self
    }

    pub fn config(&self) -> &UniversalConfig {
        &self.config
    }

    /// Provider keys the configuration names that are not available.
    pub fn unknown_sources(&self) -> Vec<String> {
        let mut unknown: Vec<String> = Vec::new();
        let named = std::iter::once(&self.config.search)
            .chain(self.config.fields.values())
            .chain(&self.config.fallbacks)
            .chain(std::iter::once(&self.config.episodes));
        for key in named {
            if !key.is_empty() && self.get(key).is_none() && !unknown.contains(key) {
                unknown.push(key.clone());
            }
        }
        unknown
    }

    fn get(&self, id: &str) -> Option<&Shared> {
        self.providers.iter().find(|p| p.id() == id)
    }

    /// The search provider: the configured one, else the first available.
    fn search_provider(&self) -> Option<&Shared> {
        self.get(&self.config.search).or(self.providers.first())
    }

    fn episode_provider(&self) -> Option<&Shared> {
        self.get(&self.config.episodes)
            .or_else(|| self.search_provider())
    }

    /// Provider keys to try for `field`, best first: the chosen one (the search provider when
    /// there is no choice), then the fallbacks.
    fn source_order(&self, field: MetaField) -> Vec<&'static str> {
        let chosen = self
            .config
            .fields
            .get(&field)
            .and_then(|id| self.get(id))
            .or_else(|| self.search_provider())
            .map(|p| p.id());
        let mut order: Vec<&'static str> = chosen.into_iter().collect();
        for fallback in &self.config.fallbacks {
            if let Some(p) = self.get(fallback)
                && !order.contains(&p.id())
            {
                order.push(p.id());
            }
        }
        order
    }

    /// The providers a scrape of `kind` needs, search provider first. Providers that do not
    /// handle the kind are left out.
    fn needed(&self, kind: MediaKind) -> Vec<Shared> {
        let item_kind = item_kind(kind);
        let mut keys: Vec<&'static str> = Vec::new();
        for field in all_fields()
            .into_iter()
            .filter(|f| f.applies_to(item_kind) && f.is_scrapable())
        {
            for key in self.source_order(field) {
                if !keys.contains(&key) {
                    keys.push(key);
                }
            }
        }
        keys.into_iter()
            .filter_map(|key| self.get(key))
            .filter(|p| handles(p.as_ref(), kind))
            .cloned()
            .collect()
    }

    /// Providers that may be asked for IDs: TMDB first, then TheTVDB, then the rest.
    fn bridge_order(&self, kind: MediaKind, skip: &[String]) -> Vec<Shared> {
        let rank = |id: &str| match id {
            "tmdb" => 0,
            "tvdb" => 1,
            _ => 2,
        };
        let mut bridge: Vec<Shared> = self
            .providers
            .iter()
            .filter(|p| handles(p.as_ref(), kind) && !skip.iter().any(|s| s == p.id()))
            .cloned()
            .collect();
        bridge.sort_by_key(|p| rank(p.id()));
        bridge
    }

    /// Scrapes a title from the configured providers and combines the answers.
    ///
    /// # Errors
    /// Fails only when no provider produced a title; the error is the search provider's if it
    /// failed, else the first failure.
    pub async fn scrape(
        &self,
        kind: MediaKind,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<UniversalScrape, ProviderError> {
        let mut pending = self.needed(kind);
        if pending.is_empty() {
            return Err(ProviderError::Other(
                "The universal provider has no provider for this kind of title.".to_owned(),
            ));
        }
        let order: Vec<&'static str> = pending.iter().map(|p| p.id()).collect();
        let mut known = ids.clone();
        let mut answers: BTreeMap<&'static str, Item> = BTreeMap::new();
        let mut failures: Vec<(String, ProviderError)> = Vec::new();
        let mut bridged = false;

        loop {
            let (ready, waiting): (Vec<Shared>, Vec<Shared>) = pending
                .into_iter()
                .partition(|p| can_resolve(p.as_ref(), kind, &known));
            pending = waiting;
            if ready.is_empty() {
                if pending.is_empty() || bridged {
                    break;
                }
                bridged = true;
                let tried: Vec<String> = order
                    .iter()
                    .filter(|id| answers.contains_key(*id))
                    .map(|id| (*id).to_owned())
                    .chain(failures.iter().map(|(id, _)| id.clone()))
                    // Waiting providers are fetched in the next round; asking them for IDs now
                    // would fetch the same details twice.
                    .chain(pending.iter().map(|p| p.id().to_owned()))
                    .collect();
                let bridge = self.bridge_order(kind, &tried);
                let refs: Vec<&dyn MetadataProvider> = bridge.iter().map(|p| p.as_ref()).collect();
                let done = complete_ids(kind, &known, &refs, self.anime.as_deref(), options).await;
                known = done.ids;
                failures.extend(done.failures);
                continue;
            }
            for (id, result) in fetch_all(ready, kind, &known, options).await {
                match result {
                    Ok(item) => {
                        add_ids(&mut known, &item.core.ids);
                        answers.insert(id, item);
                    }
                    Err(error) => failures.push((id.to_owned(), error)),
                }
            }
        }
        // Providers that never found an ID to work with did not answer either.
        for provider in &pending {
            failures.push((
                provider.id().to_owned(),
                ProviderError::NotFound(provider.name().to_owned()),
            ));
        }

        if answers.is_empty() {
            let primary = self.search_provider().map(|p| p.id());
            let error = failures
                .iter()
                .find(|(id, _)| Some(id.as_str()) == primary)
                .or(failures.first())
                .map(|(_, e)| e.clone())
                .unwrap_or_else(|| ProviderError::NotFound("Universal provider".to_owned()));
            return Err(error);
        }
        Ok(self.combine(kind, ids, &known, &order, &answers, failures))
    }

    /// Merges the answers, field by field.
    fn combine(
        &self,
        kind: MediaKind,
        requested: &ProviderIds,
        known: &ProviderIds,
        order: &[&'static str],
        answers: &BTreeMap<&'static str, Item>,
        failures: Vec<(String, ProviderError)>,
    ) -> UniversalScrape {
        let mut item = match kind {
            MediaKind::Movie => Item::movie(""),
            MediaKind::Show => Item::show(""),
        };
        item.core.ids = requested.clone();
        let no_locks = FieldSet::new();
        let mut sources = BTreeMap::new();

        let item_kind = item_kind(kind);
        for field in all_fields()
            .into_iter()
            .filter(|f| f.applies_to(item_kind) && f.is_scrapable())
        {
            let only = ScrapeOptions::only([field]);
            for source in self.source_order(field) {
                let Some(answer) = answers.get(source) else {
                    continue;
                };
                // The merge rules decide what "has a value" means for each field.
                let outcome = apply_scrape(&mut item, answer, &only, &no_locks);
                if outcome.written.contains(&field) {
                    sources.insert(field, source.to_owned());
                    break;
                }
            }
        }

        // The union of every answer's IDs; the collection ID belongs to the collection field.
        let none = ScrapeOptions::only([]);
        for id in order {
            if let Some(answer) = answers.get(id) {
                apply_scrape(&mut item, answer, &none, &no_locks);
            }
        }
        add_ids(&mut item.core.ids, known);
        item.core.last_scraper_id = UNIVERSAL_ID.to_owned();
        UniversalScrape {
            item,
            sources,
            failures,
        }
    }
}

fn item_kind(kind: MediaKind) -> ItemKind {
    match kind {
        MediaKind::Movie => ItemKind::Movie,
        MediaKind::Show => ItemKind::Show,
    }
}

fn handles(provider: &dyn MetadataProvider, kind: MediaKind) -> bool {
    let capabilities = provider.capabilities();
    match kind {
        MediaKind::Movie => capabilities.movies,
        MediaKind::Show => capabilities.shows,
    }
}

/// Adds the IDs of `from` that `to` lacks; the collection ID is not a plain ID.
fn add_ids(to: &mut ProviderIds, from: &ProviderIds) {
    for (key, value) in from.iter() {
        if key != id_key::TMDB_SET && !to.contains(key) {
            to.set(key, value);
        }
    }
}

/// Asks the providers for their details at the same time. A provider that stops unexpectedly
/// counts as failed. The answers come back in the order of `providers`.
async fn fetch_all(
    providers: Vec<Shared>,
    kind: MediaKind,
    ids: &ProviderIds,
    options: &RequestOptions,
) -> Vec<(&'static str, Result<Item, ProviderError>)> {
    let keys: Vec<&'static str> = providers.iter().map(|p| p.id()).collect();
    let mut tasks = JoinSet::new();
    for (index, provider) in providers.into_iter().enumerate() {
        let ids = ids.clone();
        let options = options.clone();
        tasks.spawn(async move { (index, provider.details(kind, &ids, &options).await) });
    }
    let mut results: Vec<Option<Result<Item, ProviderError>>> = keys.iter().map(|_| None).collect();
    while let Some(joined) = tasks.join_next().await {
        if let Ok((index, result)) = joined {
            results[index] = Some(result);
        }
    }
    keys.into_iter()
        .zip(results)
        .map(|(key, result)| {
            let result = result.unwrap_or_else(|| {
                Err(ProviderError::Other(format!("{key} stopped unexpectedly")))
            });
            (key, result)
        })
        .collect()
}

#[async_trait]
impl Provider for UniversalProvider {
    fn id(&self) -> &'static str {
        UNIVERSAL_ID
    }

    fn name(&self) -> &'static str {
        "Universal"
    }
}

#[async_trait]
impl MetadataProvider for UniversalProvider {
    fn capabilities(&self) -> Capabilities {
        let mut lookup_ids: Vec<&'static str> = Vec::new();
        for provider in &self.providers {
            for key in provider.capabilities().lookup_ids {
                if !lookup_ids.contains(&key) {
                    lookup_ids.push(key);
                }
            }
        }
        let search = self.search_provider().map(|p| p.capabilities());
        Capabilities {
            movies: self.providers.iter().any(|p| p.capabilities().movies),
            shows: self.providers.iter().any(|p| p.capabilities().shows),
            orderings: self
                .episode_provider()
                .map(|p| p.capabilities().orderings)
                .unwrap_or_default(),
            lookup_ids,
            best_effort: search.is_some_and(|c| c.best_effort),
        }
    }

    /// Searches with the search provider; the hits are labelled `universal`, their IDs stay.
    async fn search(
        &self,
        query: &SearchQuery,
        options: &RequestOptions,
    ) -> Result<Vec<SearchHit>, ProviderError> {
        let provider = self.search_provider().ok_or_else(no_providers)?;
        let mut hits = provider.search(query, options).await?;
        for hit in &mut hits {
            UNIVERSAL_ID.clone_into(&mut hit.provider);
        }
        Ok(hits)
    }

    async fn lookup(
        &self,
        kind: MediaKind,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Option<SearchHit>, ProviderError> {
        let provider = self.search_provider().ok_or_else(no_providers)?;
        let mut known = ids.clone();
        if !can_resolve(provider.as_ref(), kind, &known) {
            let bridge = self.bridge_order(kind, &[provider.id().to_owned()]);
            let refs: Vec<&dyn MetadataProvider> = bridge.iter().map(|p| p.as_ref()).collect();
            known = complete_ids(kind, &known, &refs, self.anime.as_deref(), options)
                .await
                .ids;
        }
        let hit = provider.lookup(kind, &known, options).await?;
        Ok(hit.map(|mut hit| {
            UNIVERSAL_ID.clone_into(&mut hit.provider);
            hit
        }))
    }

    async fn details(
        &self,
        kind: MediaKind,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Item, ProviderError> {
        Ok(self.scrape(kind, ids, options).await?.item)
    }

    /// The episode list from the episode provider, with its ID resolved first.
    async fn episodes(
        &self,
        show: &ProviderIds,
        ordering: &EpisodeGroup,
        options: &RequestOptions,
    ) -> Result<Vec<Item>, ProviderError> {
        let provider = self.episode_provider().ok_or_else(no_providers)?;
        let mut known = show.clone();
        if !can_resolve(provider.as_ref(), MediaKind::Show, &known) {
            let bridge = self.bridge_order(MediaKind::Show, &[provider.id().to_owned()]);
            let refs: Vec<&dyn MetadataProvider> = bridge.iter().map(|p| p.as_ref()).collect();
            known = complete_ids(
                MediaKind::Show,
                &known,
                &refs,
                self.anime.as_deref(),
                options,
            )
            .await
            .ids;
        }
        provider.episodes(&known, ordering, options).await
    }
}

fn no_providers() -> ProviderError {
    ProviderError::Other("The universal provider has no providers to use.".to_owned())
}
