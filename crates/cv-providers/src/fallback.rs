//! The ordered fallback-provider chain.
//!
//! When the primary provider has nothing for a search, the app may try other providers in an order
//! the user chose and stop at the first that returns results.
//!
//! * "Nothing" is an empty result, a provider that has no match, or a primary that is inactive
//!   because its key is missing. A real failure of the primary (outage, rejected key) is an error
//!   for that job and does not start the chain.
//! * Hits without a title or without any ID are dropped before deciding, so a provider that only
//!   answers with unusable rows counts as empty.
//! * A chain provider that is inactive, does not handle the kind, or fails is skipped and
//!   reported in [`FallbackSearch::skipped`]; the next one is tried.
//! * The universal provider and Kodi add-on providers never join the chain.
//!
//! An empty chain is the "fallback off" setting: [`FallbackChain::search`] then behaves like the
//! primary alone.

use std::sync::Arc;

use cv_domain::Item;

use crate::error::ProviderError;
use crate::model::{MediaKind, RequestOptions, SearchHit, SearchQuery};
use crate::traits::MetadataProvider;
use crate::universal::UNIVERSAL_ID;

type Shared = Arc<dyn MetadataProvider>;

/// The result of a search with fallback.
#[derive(Debug, Clone, Default)]
pub struct FallbackSearch {
    /// Key of the provider whose hits these are (the primary when nothing was found).
    pub provider: String,
    pub hits: Vec<SearchHit>,
    /// A chain provider answered instead of the primary.
    pub fell_back: bool,
    /// Providers that were tried and skipped (key and error).
    pub skipped: Vec<(String, ProviderError)>,
}

/// A search with fallback and the details of its first hit.
#[derive(Debug, Clone)]
pub struct FallbackScrape {
    /// The details; `last_scraper_id` is the provider that supplied them.
    pub item: Item,
    pub search: FallbackSearch,
}

/// The providers to try after the primary, in order.
#[derive(Clone, Default)]
pub struct FallbackChain {
    providers: Vec<Shared>,
}

impl FallbackChain {
    /// The chain in the given order. Universal and Kodi add-on providers, and a second provider
    /// with a key already in the chain, are left out.
    pub fn new(providers: impl IntoIterator<Item = Shared>) -> Self {
        let mut chain: Vec<Shared> = Vec::new();
        for provider in providers {
            let id = provider.id();
            if id != UNIVERSAL_ID && !id.starts_with("kodi") && !chain.iter().any(|p| p.id() == id)
            {
                chain.push(provider);
            }
        }
        Self { providers: chain }
    }

    /// No fallback.
    pub fn disabled() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }

    /// Provider keys in order.
    pub fn ids(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.providers.iter().map(|p| p.id())
    }

    /// Searches with `primary`; when that finds nothing, with each chain provider in order.
    ///
    /// # Errors
    /// Fails when the primary fails for a reason other than having no match or being inactive.
    pub async fn search(
        &self,
        primary: &Shared,
        query: &SearchQuery,
        options: &RequestOptions,
    ) -> Result<FallbackSearch, ProviderError> {
        self.search_inner(primary, query, options)
            .await
            .map(|(_, found)| found)
    }

    /// Searches as [`Self::search`], then fetches the details of the first hit from the provider
    /// that found it. Hits are in the provider's own rank order; callers that score candidates
    /// use [`Self::search`] and choose themselves.
    ///
    /// # Errors
    /// Fails like [`Self::search`], when nothing was found anywhere (`NotFound`) or when the
    /// details request fails.
    pub async fn scrape_first(
        &self,
        primary: &Shared,
        query: &SearchQuery,
        options: &RequestOptions,
    ) -> Result<FallbackScrape, ProviderError> {
        let (provider, search) = self.search_inner(primary, query, options).await?;
        let Some(hit) = search.hits.first() else {
            return Err(ProviderError::NotFound(primary.name().to_owned()));
        };
        let mut item = provider.details(query.kind, &hit.ids, options).await?;
        provider.id().clone_into(&mut item.core.last_scraper_id);
        Ok(FallbackScrape { item, search })
    }

    /// Tries the chain providers (not `primary_id`, which has just found nothing) in order and
    /// returns the first that has usable hits, with the providers that were skipped on the way.
    /// `None` when the chain is empty or nobody found anything. Callers that run their own
    /// primary search, such as batch matching, use this for the second half of
    /// [`Self::search`].
    pub async fn search_after(
        &self,
        primary_id: &str,
        query: &SearchQuery,
        options: &RequestOptions,
    ) -> Option<(Shared, FallbackSearch)> {
        let mut found = FallbackSearch {
            provider: primary_id.to_owned(),
            ..FallbackSearch::default()
        };
        let (provider, hits) = self
            .run_chain(primary_id, query, options, &mut found.skipped)
            .await?;
        found.provider = provider.id().to_owned();
        found.hits = hits;
        found.fell_back = true;
        Some((provider, found))
    }

    /// The providers of the chain that can be tried after `primary_id` for a `kind` of title.
    pub fn after<'a>(
        &'a self,
        primary_id: &'a str,
        kind: MediaKind,
    ) -> impl Iterator<Item = &'a Shared> + 'a {
        self.providers
            .iter()
            .filter(move |p| p.id() != primary_id && handles(p.as_ref(), kind))
    }

    async fn run_chain(
        &self,
        primary_id: &str,
        query: &SearchQuery,
        options: &RequestOptions,
        skipped: &mut Vec<(String, ProviderError)>,
    ) -> Option<(Shared, Vec<SearchHit>)> {
        for provider in self.after(primary_id, query.kind) {
            match provider.search(query, options).await {
                Ok(hits) => {
                    let hits = usable(hits);
                    if !hits.is_empty() {
                        return Some((provider.clone(), hits));
                    }
                }
                Err(error) => {
                    if !is_nothing(&error) {
                        tracing::warn!(provider = provider.id(), %error, "fallback provider failed");
                    }
                    skipped.push((provider.id().to_owned(), error));
                }
            }
        }
        None
    }

    async fn search_inner(
        &self,
        primary: &Shared,
        query: &SearchQuery,
        options: &RequestOptions,
    ) -> Result<(Shared, FallbackSearch), ProviderError> {
        let mut found = FallbackSearch {
            provider: primary.id().to_owned(),
            ..FallbackSearch::default()
        };
        match primary.search(query, options).await {
            Ok(hits) => found.hits = usable(hits),
            Err(error) if is_nothing(&error) => {
                found.skipped.push((primary.id().to_owned(), error))
            }
            Err(error) => return Err(error),
        }
        if !found.hits.is_empty() {
            return Ok((primary.clone(), found));
        }

        if let Some((provider, hits)) = self
            .run_chain(primary.id(), query, options, &mut found.skipped)
            .await
        {
            found.provider = provider.id().to_owned();
            found.hits = hits;
            found.fell_back = true;
            return Ok((provider, found));
        }
        Ok((primary.clone(), found))
    }
}

/// The error means "no result" rather than "broken": no match, or no key.
fn is_nothing(error: &ProviderError) -> bool {
    matches!(
        error.root(),
        ProviderError::NotFound(_) | ProviderError::MissingCredentials { .. }
    )
}

fn usable(hits: Vec<SearchHit>) -> Vec<SearchHit> {
    hits.into_iter()
        .filter(|hit| !hit.title.trim().is_empty() && !hit.ids.is_empty())
        .collect()
}

fn handles(provider: &dyn MetadataProvider, kind: MediaKind) -> bool {
    let capabilities = provider.capabilities();
    match kind {
        MediaKind::Movie => capabilities.movies,
        MediaKind::Show => capabilities.shows,
    }
}
