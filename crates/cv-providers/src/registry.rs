//! The provider implementations a build contains.

use std::sync::Arc;

use crate::credentials::ResolvedCredentials;
use crate::error::ProviderError;
use crate::model::MediaKind;
use crate::traits::{ArtworkProvider, MetadataProvider, Provider};

/// Holds the registered providers by key.
#[derive(Default, Clone)]
pub struct ProviderRegistry {
    providers: Vec<Arc<dyn Provider>>,
    /// The providers that can also search and scrape titles, in registration order. The batch
    /// matcher takes its providers from here.
    metadata: Vec<Arc<dyn MetadataProvider>>,
    /// The providers that offer artwork, in registration order (the default order of the
    /// artwork providers; Settings lets the user reorder and disable them).
    artwork: Vec<Arc<dyn ArtworkProvider>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a provider. A provider registered twice under one key replaces the first.
    pub fn register(&mut self, provider: Arc<dyn Provider>) {
        self.metadata.retain(|p| p.id() != provider.id());
        self.artwork.retain(|p| p.id() != provider.id());
        self.providers.retain(|p| p.id() != provider.id());
        self.providers.push(provider);
    }

    /// Adds a provider that searches and scrapes titles. It is also registered as a plain
    /// provider, so credential tests find it, and it replaces an earlier one with the same key.
    pub fn register_metadata(&mut self, provider: Arc<dyn MetadataProvider>) {
        let plain: Arc<dyn Provider> = provider.clone();
        self.register(plain);
        self.metadata.push(provider);
    }

    /// Adds a provider that offers artwork. It is also registered as a plain provider, so
    /// credential tests find it, unless that key is already registered (a metadata provider that
    /// also offers artwork keeps its place in the metadata list).
    pub fn register_artwork(&mut self, provider: Arc<dyn ArtworkProvider>) {
        self.artwork.retain(|p| p.id() != provider.id());
        if self.get(provider.id()).is_none() {
            let plain: Arc<dyn Provider> = provider.clone();
            self.register(plain);
        }
        self.artwork.push(provider);
    }

    /// The artwork providers, in registration order.
    pub fn artwork_providers(&self) -> &[Arc<dyn ArtworkProvider>] {
        &self.artwork
    }

    /// The artwork providers that serve a kind of library, in the default order for it
    /// ([`crate::artwork::default_provider_order`]). A provider that order does not name (MPDb.tv
    /// for shows) is left out.
    pub fn artwork_providers_for(&self, kind: MediaKind) -> Vec<Arc<dyn ArtworkProvider>> {
        let order = crate::artwork::default_provider_order(kind);
        let mut ranked: Vec<(usize, Arc<dyn ArtworkProvider>)> = self
            .artwork
            .iter()
            .filter_map(|p| Some((order.iter().position(|id| *id == p.id())?, Arc::clone(p))))
            .collect();
        ranked.sort_by_key(|(rank, _)| *rank);
        ranked.into_iter().map(|(_, p)| p).collect()
    }

    /// The metadata providers, in registration order (the first one that supports a kind is the
    /// default for it).
    pub fn metadata_providers(&self) -> &[Arc<dyn MetadataProvider>] {
        &self.metadata
    }

    pub fn get(&self, id: &str) -> Option<&Arc<dyn Provider>> {
        self.providers.iter().find(|p| p.id() == id)
    }

    pub fn ids(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.providers.iter().map(|p| p.id())
    }

    /// Asks the provider to check `credentials`. Providers that are not part of this build
    /// answer [`ProviderError::Unsupported`].
    pub async fn check_credentials(
        &self,
        id: &str,
        display_name: &str,
        credentials: &ResolvedCredentials,
    ) -> Result<(), ProviderError> {
        match self.get(id) {
            Some(provider) => provider.check_credentials(credentials).await,
            None => Err(ProviderError::Unsupported {
                provider: display_name.to_owned(),
                operation: "testing credentials in this build",
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;

    use super::*;
    use crate::model::{Capabilities, MediaKind, RequestOptions, SearchHit, SearchQuery};

    struct Titles(&'static str);

    #[async_trait]
    impl Provider for Titles {
        fn id(&self) -> &'static str {
            self.0
        }
        fn name(&self) -> &'static str {
            "Titles"
        }
    }

    #[async_trait]
    impl MetadataProvider for Titles {
        fn capabilities(&self) -> Capabilities {
            Capabilities {
                movies: true,
                shows: false,
                orderings: Vec::new(),
                lookup_ids: Vec::new(),
                best_effort: false,
            }
        }
        async fn search(
            &self,
            _: &SearchQuery,
            _: &RequestOptions,
        ) -> Result<Vec<SearchHit>, ProviderError> {
            Ok(Vec::new())
        }
        async fn lookup(
            &self,
            _: MediaKind,
            _: &cv_domain::ProviderIds,
            _: &RequestOptions,
        ) -> Result<Option<SearchHit>, ProviderError> {
            Ok(None)
        }
        async fn details(
            &self,
            _: MediaKind,
            _: &cv_domain::ProviderIds,
            _: &RequestOptions,
        ) -> Result<cv_domain::Item, ProviderError> {
            Err(ProviderError::NotFound("Titles".to_owned()))
        }
    }

    struct Plain;

    #[async_trait]
    impl Provider for Plain {
        fn id(&self) -> &'static str {
            "fanart"
        }
        fn name(&self) -> &'static str {
            "Plain"
        }
    }

    #[test]
    fn metadata_providers_keep_their_registration_order_and_are_also_plain_providers() {
        let mut registry = ProviderRegistry::new();
        registry.register(Arc::new(Plain));
        registry.register_metadata(Arc::new(Titles("tmdb")));
        registry.register_metadata(Arc::new(Titles("tvdb")));
        let ids: Vec<_> = registry
            .metadata_providers()
            .iter()
            .map(|p| p.id())
            .collect();
        assert_eq!(ids, ["tmdb", "tvdb"]);
        assert!(registry.get("tmdb").is_some());
        assert!(registry.get("fanart").is_some());
        assert_eq!(registry.ids().count(), 3);
    }

    #[test]
    fn registering_a_key_again_replaces_it_in_both_lists() {
        let mut registry = ProviderRegistry::new();
        registry.register_metadata(Arc::new(Titles("tmdb")));
        registry.register_metadata(Arc::new(Titles("tmdb")));
        assert_eq!(registry.metadata_providers().len(), 1);
        assert_eq!(registry.ids().count(), 1);
        // A plain provider under the same key takes the slot and is no longer a metadata one.
        struct Impostor;
        #[async_trait]
        impl Provider for Impostor {
            fn id(&self) -> &'static str {
                "tmdb"
            }
            fn name(&self) -> &'static str {
                "Impostor"
            }
        }
        registry.register(Arc::new(Impostor));
        assert!(registry.metadata_providers().is_empty());
        assert_eq!(registry.get("tmdb").map(|p| p.name()), Some("Impostor"));
    }

    struct Images(&'static str);

    #[async_trait]
    impl Provider for Images {
        fn id(&self) -> &'static str {
            self.0
        }
        fn name(&self) -> &'static str {
            "Images"
        }
    }

    #[async_trait]
    impl ArtworkProvider for Images {
        async fn artwork(
            &self,
            _: MediaKind,
            _: &cv_domain::ProviderIds,
            _: &RequestOptions,
        ) -> Result<Vec<crate::model::ArtworkCandidate>, ProviderError> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn artwork_providers_follow_the_default_order_of_each_library_type() {
        let mut registry = ProviderRegistry::new();
        registry.register_metadata(Arc::new(Titles("tmdb")));
        for id in ["tmdb", "tvdb", "fanart", "mpdb", "imdb", "unlisted"] {
            registry.register_artwork(Arc::new(Images(id)));
        }
        let ids = |kind| -> Vec<&'static str> {
            registry
                .artwork_providers_for(kind)
                .iter()
                .map(|p| p.id())
                .collect()
        };
        assert_eq!(
            ids(MediaKind::Movie),
            ["tmdb", "fanart", "imdb", "mpdb", "tvdb"]
        );
        assert_eq!(ids(MediaKind::Show), ["tvdb", "fanart", "tmdb", "imdb"]);
        // Registered in registration order, found by credential tests, and TMDB stays a
        // metadata provider too.
        assert_eq!(registry.artwork_providers().len(), 6);
        assert!(registry.get("fanart").is_some());
        assert_eq!(registry.metadata_providers().len(), 1);
        assert_eq!(registry.ids().filter(|id| *id == "tmdb").count(), 1);
    }

    #[test]
    fn registering_an_artwork_provider_again_replaces_it() {
        let mut registry = ProviderRegistry::new();
        registry.register_artwork(Arc::new(Images("fanart")));
        registry.register_artwork(Arc::new(Images("fanart")));
        assert_eq!(registry.artwork_providers().len(), 1);
        assert_eq!(registry.ids().count(), 1);
    }
}
