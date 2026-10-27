//! The provider side of the app: one shared HTTP client and the registry of every provider this
//! build contains.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use cv_providers::http::ProviderLimits;
use cv_providers::kodi::UnsupportedScraper;
use cv_providers::{
    AnidbProvider, CredentialResolver, FanartProvider, FernsehserienProvider, HttpClient,
    ImdbProvider, KodiCatalogue, MdbListProvider, MovieMeterProvider, MpdbProvider, OfdbProvider,
    OmdbProvider, Provider, ProviderError, ProviderRegistry, TheSportsDbProvider, TmdbProvider,
    TraktProvider, TvdbProvider, TvmazeProvider,
};

use crate::provider_credentials::{ProviderCredentialsService, SecretStoreCredentials};
use crate::secrets::SecretStore;

/// Folder name, under the data location's cache folder, of the provider response cache.
pub const HTTP_CACHE_DIR: &str = "http";

/// The shared HTTP client: one per app, so every provider shares its limits, cache and redaction.
/// Log lines and error text that mention a request pass through the app redactor. A cache folder
/// that cannot be used costs the cache, not the providers.
///
/// # Errors
/// Fails when the HTTP client itself cannot start.
pub fn http_client(cache_dir: Option<PathBuf>) -> Result<HttpClient, ProviderError> {
    let builder = || {
        HttpClient::builder().redactor(Arc::new(|text: &str| {
            crate::redact::redact(text).into_owned()
        }))
    };
    if let Some(dir) = cache_dir {
        match builder().cache_dir(dir).build() {
            Ok(client) => return Ok(client),
            Err(error) => tracing::warn!(%error, "cannot open the provider response cache"),
        }
    }
    builder().build()
}

/// Rate limits of the providers whose own constructors do not set any: TMDB allows roughly fifty
/// requests a second, TheTVDB and OMDb are used gently. Every other provider sets its documented
/// limits (AniDB: one request per two seconds) when it is created.
fn base_limits() -> [(&'static str, ProviderLimits); 3] {
    [
        (
            "tmdb",
            ProviderLimits::new(4, 40, Duration::from_secs(1)).named("TMDB"),
        ),
        (
            "tvdb",
            ProviderLimits::new(4, 10, Duration::from_secs(1)).named("TheTVDB"),
        ),
        (
            "omdb",
            ProviderLimits::new(2, 5, Duration::from_secs(1)).named("OMDb"),
        ),
    ]
}

/// Every provider this build contains, each registered with the shared HTTP client and the
/// credential resolver and with its rate limits applied.
///
/// All of them but Fanart.tv search and scrape titles, so those are registered as metadata providers
/// ([`ProviderRegistry::register_metadata`]): batch matching, the review inbox and the search and
/// scrape panel take their providers from there. The order is the order of preference: the first
/// provider that supports a kind (and has its credentials) is the default for it.
pub fn register_providers(http: &HttpClient, resolver: &CredentialResolver) -> ProviderRegistry {
    for (provider, limits) in base_limits() {
        http.set_provider_limits(provider, limits);
    }
    let mut registry = ProviderRegistry::new();
    let tmdb = TmdbProvider::new(http.clone(), resolver.clone());
    let tvdb = TvdbProvider::new(http.clone(), resolver.clone());
    let imdb = ImdbProvider::new(http.clone());
    let mpdb = MpdbProvider::new(http.clone(), resolver.clone());
    registry.register_metadata(Arc::new(tmdb.clone()));
    registry.register_metadata(Arc::new(tvdb.clone()));
    registry.register_metadata(Arc::new(imdb.clone()));
    registry.register_metadata(Arc::new(TvmazeProvider::new(http.clone())));
    registry.register_metadata(Arc::new(OmdbProvider::new(http.clone(), resolver.clone())));
    registry.register_metadata(Arc::new(TraktProvider::new(http.clone(), resolver.clone())));
    registry.register_metadata(Arc::new(MovieMeterProvider::new(
        http.clone(),
        resolver.clone(),
    )));
    registry.register_metadata(Arc::new(mpdb.clone()));
    registry.register_metadata(Arc::new(OfdbProvider::new(http.clone())));
    registry.register_metadata(Arc::new(FernsehserienProvider::new(http.clone())));
    registry.register_metadata(Arc::new(AnidbProvider::new(http.clone(), resolver.clone())));
    registry.register_metadata(Arc::new(TheSportsDbProvider::new(
        http.clone(),
        resolver.clone(),
    )));
    // The artwork providers: the metadata providers that also offer images, plus Fanart.tv, which
    // offers nothing else. The order they are asked in is the default order of each library type
    // (`ProviderRegistry::artwork_providers_for`); frame grabs are not a provider (cv-artwork).
    registry.register_artwork(Arc::new(tmdb));
    registry.register_artwork(Arc::new(FanartProvider::new(
        http.clone(),
        resolver.clone(),
    )));
    registry.register_artwork(Arc::new(imdb));
    registry.register_artwork(Arc::new(mpdb));
    registry.register_artwork(Arc::new(tvdb));
    // MDBList is a rating source, not a title provider; it is registered so its key can be
    // tested from Settings.
    registry.register(Arc::new(MdbListProvider::new(
        http.clone(),
        resolver.clone(),
    )));
    registry
}

/// The Kodi XML scraper add-ons found at startup.
#[derive(Default)]
pub struct KodiAddons {
    /// Provider keys of the scrapers that were registered, in registration order.
    pub provider_ids: Vec<&'static str>,
    /// Scrapers that were found and cannot run (a Python scraper, a missing file), with the
    /// reason, for the Providers page.
    pub unsupported: Vec<UnsupportedScraper>,
}

/// Registers the Kodi XML scrapers found under `roots` as metadata providers, after the
/// built-in ones, and lists the ones that cannot be used. The folders are only read: nothing is
/// written to a Kodi installation and no add-on code is run beyond its XML scraper rules.
pub fn register_kodi_addons(
    registry: &mut ProviderRegistry,
    http: &HttpClient,
    roots: &[PathBuf],
) -> KodiAddons {
    let catalogue = KodiCatalogue::load(http, roots, &HashMap::new());
    let mut loaded = KodiAddons {
        unsupported: catalogue.unsupported,
        ..KodiAddons::default()
    };
    for provider in catalogue.providers {
        loaded.provider_ids.push(provider.id());
        registry.register_metadata(Arc::new(provider));
    }
    loaded
}

/// The credential service with every provider this build contains registered. Providers hold
/// clones of the service's resolver, so a key saved in Settings applies to the next request.
pub fn build_service(
    store: Arc<SecretStore>,
    cache_dir: Option<PathBuf>,
) -> ProviderCredentialsService {
    build_service_with_kodi(store, cache_dir, &[])
}

/// [`build_service`], plus the Kodi XML scraper add-ons found under `kodi_roots` (see
/// `cv_providers::kodi::default_roots`).
pub fn build_service_with_kodi(
    store: Arc<SecretStore>,
    cache_dir: Option<PathBuf>,
    kodi_roots: &[PathBuf],
) -> ProviderCredentialsService {
    let resolver = CredentialResolver::new(Arc::new(SecretStoreCredentials::new(store)));
    match http_client(cache_dir) {
        Ok(http) => {
            let mut registry = register_providers(&http, &resolver);
            let kodi = register_kodi_addons(&mut registry, &http, kodi_roots);
            ProviderCredentialsService::with_resolver(resolver, registry)
                .with_http(http)
                .with_kodi(kodi)
        }
        Err(error) => {
            tracing::error!(%error, "cannot start the provider HTTP client");
            ProviderCredentialsService::with_resolver(resolver, ProviderRegistry::new())
        }
    }
}

/// The Kodi add-on folders to scan: CineVault's own `kodi_scraper` folder under `app_dir`, then
/// the folders of an installed Kodi.
#[must_use]
pub fn kodi_roots(app_dir: &Path) -> Vec<PathBuf> {
    cv_providers::kodi::default_roots(app_dir)
}

#[cfg(test)]
mod tests {
    use cv_providers::MemoryUserCredentials;
    use cv_providers::model::MediaKind;

    use super::*;

    fn registry() -> (HttpClient, ProviderRegistry) {
        let http = HttpClient::builder().build().expect("http client");
        let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
        let registry = register_providers(&http, &resolver);
        (http, registry)
    }

    #[test]
    fn every_implemented_provider_is_registered_for_matching() {
        let (_, registry) = registry();
        let metadata: Vec<_> = registry
            .metadata_providers()
            .iter()
            .map(|p| p.id())
            .collect();
        assert_eq!(
            metadata,
            [
                "tmdb",
                "tvdb",
                "imdb",
                "tvmaze",
                "omdb",
                "trakt",
                "moviemeter",
                "mpdb",
                "ofdb",
                "fernsehserien",
                "anidb",
                "thesportsdb"
            ]
        );
        // Credential tests find every one of them too.
        for id in metadata {
            assert!(registry.get(id).is_some(), "{id}");
        }
    }

    #[test]
    fn artwork_providers_are_registered_in_each_library_types_default_order() {
        let (_, registry) = registry();
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
        // Fanart.tv offers artwork only, so it is not a metadata provider, but credential tests
        // find it.
        assert!(registry.get("fanart").is_some());
        assert!(
            registry
                .metadata_providers()
                .iter()
                .all(|p| p.id() != "fanart")
        );
    }

    #[test]
    fn every_provider_with_credentials_can_test_them() {
        let store = Arc::new(SecretStore::in_memory());
        let service = build_service(store, None);
        let untestable: Vec<_> = cv_providers::catalogue::CREDENTIAL_SCHEMAS
            .iter()
            .map(|schema| schema.provider)
            .filter(|id| !service.can_test(id))
            .collect();
        assert!(untestable.is_empty(), "{untestable:?}");
    }

    #[test]
    fn every_provider_has_its_own_rate_limits() {
        let (http, registry) = registry();
        let width = |id: &str| http.lane_status(id).max_concurrent;
        // AniDB: one request at a time, one per two seconds.
        assert_eq!(width("anidb"), 1);
        assert_eq!(anidb_interval(), Duration::from_secs(2));
        assert_eq!(width("tmdb"), 4);
        assert_eq!(width("omdb"), 2);
        assert_eq!(width("imdb"), 2);
        assert_eq!(width("thesportsdb"), 2);
        assert_eq!(width("fanart"), 2);
        for provider in registry.metadata_providers() {
            assert!(width(provider.id()) >= 1, "{}", provider.id());
        }
    }

    fn anidb_interval() -> Duration {
        cv_providers::anidb::default_limits().period
    }
}
