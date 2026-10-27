//! The MovieMeter provider: Dutch movie data from the MovieMeter API, which needs
//! a personal API key (`api_key`).
//!
//! * [`MetadataProvider::search`] uses `film/?q=`.
//! * [`MetadataProvider::lookup`] and [`MetadataProvider::details`] read `film/{id}` by the
//!   MovieMeter ID (`moviemeter`), or `film/{tt}` by the IMDb ID.
//! * Titles, years, plots, runtimes, genres (Dutch names mapped to the standard ones),
//!   countries, directors, actors, the poster and the MovieMeter rating (one to five stars)
//!   come from the film answer. MovieMeter is movies only here.
//!
//! The key comes from the [`CredentialResolver`] on every request, and requests go through the
//! shared [`HttpClient`] under the lane `moviemeter`.

mod convert;

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use cv_domain::{Item, ProviderIds, id_key};
use serde_json::Value;

use crate::catalogue;
use crate::credentials::{CredentialResolver, CredentialSchema, ResolvedCredentials};
use crate::error::ProviderError;
use crate::http::{HttpClient, HttpRequest, HttpResponse, ProviderLimits};
use crate::model::{Capabilities, MediaKind, RequestOptions, SearchHit, SearchQuery};
use crate::shared::imdb_id_of;
use crate::traits::{MetadataProvider, Provider};

pub use convert::RATING_ID;

use convert::{Film, search_films};

/// The provider key and HTTP lane id.
pub const PROVIDER_ID: &str = catalogue::key::MOVIEMETER;
/// The provider's ID key in [`ProviderIds`].
pub const ID_KEY: &str = "moviemeter";
/// The API server.
pub const DEFAULT_BASE_URL: &str = "https://www.moviemeter.nl/api";

const DISPLAY_NAME: &str = "MovieMeter";

/// How fast requests are sent. MovieMeter publishes no limit, so this is a polite one.
pub fn default_limits() -> ProviderLimits {
    ProviderLimits::new(2, 3, Duration::from_secs(1)).named(DISPLAY_NAME)
}

struct Inner {
    http: HttpClient,
    resolver: CredentialResolver,
    base_url: String,
}

/// The MovieMeter provider. Cloning is cheap.
#[derive(Clone)]
pub struct MovieMeterProvider {
    inner: Arc<Inner>,
}

impl MovieMeterProvider {
    /// A provider for the real API server. It sets the [`default_limits`] on the `moviemeter`
    /// lane.
    pub fn new(http: HttpClient, resolver: CredentialResolver) -> Self {
        http.set_provider_limits(PROVIDER_ID, default_limits());
        Self::with_base_url(http, resolver, DEFAULT_BASE_URL)
    }

    /// A provider for another server, such as a local one in tests. Lane limits are left as
    /// configured.
    pub fn with_base_url(http: HttpClient, resolver: CredentialResolver, base_url: &str) -> Self {
        Self {
            inner: Arc::new(Inner {
                http,
                resolver,
                base_url: base_url.trim_end_matches('/').to_owned(),
            }),
        }
    }

    fn schema() -> Option<&'static CredentialSchema> {
        catalogue::CREDENTIAL_SCHEMAS
            .iter()
            .find(|s| s.provider == PROVIDER_ID)
    }

    /// GET `path` with the key resolved now, so a key entered in Settings applies to the next
    /// call, and parse the JSON answer.
    async fn get(
        &self,
        path: &str,
        params: &[(&str, &str)],
        options: &RequestOptions,
    ) -> Result<Value, ProviderError> {
        let credentials = self.inner.resolver.require(PROVIDER_ID)?;
        let response = self
            .send(&credentials, path, params, options.bypass_cache, true)
            .await?;
        response.json().map_err(|e| {
            tracing::debug!(error = %e, "MovieMeter answered with an unexpected body");
            request_error("the response was not in the expected format")
        })
    }

    async fn send(
        &self,
        credentials: &ResolvedCredentials,
        path: &str,
        params: &[(&str, &str)],
        bypass_cache: bool,
        cached: bool,
    ) -> Result<HttpResponse, ProviderError> {
        self.send_with(credentials, path, params, bypass_cache, cached)
            .await
            .map_err(|e| e.with_key_hint(credentials, PROVIDER_ID))
    }

    async fn send_with(
        &self,
        credentials: &ResolvedCredentials,
        path: &str,
        params: &[(&str, &str)],
        bypass_cache: bool,
        cached: bool,
    ) -> Result<HttpResponse, ProviderError> {
        let key = credentials
            .get(catalogue::API_KEY)
            .map(|v| v.expose().trim().to_owned())
            .filter(|k| !k.is_empty())
            .ok_or_else(missing_key)?;
        // The path keeps its trailing slash (`film/`) when it has one.
        let url = format!("{}/{}", self.inner.base_url, path.trim_start_matches('/'));
        let mut request = HttpRequest::get(PROVIDER_ID, &url)
            .credential_query("api_key", &key)
            .bypass_cache(bypass_cache);
        for (name, value) in params {
            request = request.query(name, value);
        }
        if !cached {
            request = request.uncached();
        }
        let response = self.inner.http.send(request).await?;
        match response.status() {
            s if (200..300).contains(&s) => Ok(response),
            401 | 403 => Err(ProviderError::Auth {
                provider: DISPLAY_NAME.to_owned(),
                message: "the API key was not accepted".to_owned(),
            }),
            404 => Err(ProviderError::NotFound(DISPLAY_NAME.to_owned())),
            s => Err(request_error(format!("HTTP {s}"))),
        }
    }

    /// The film for an ID, preferring the MovieMeter ID over the IMDb ID.
    async fn film(
        &self,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Film, ProviderError> {
        let id = ids
            .get_u64(ID_KEY)
            .map(|id| id.to_string())
            .or_else(|| imdb_id_of(ids))
            .ok_or_else(|| ProviderError::NotFound(DISPLAY_NAME.to_owned()))?;
        let value = self.get(&format!("film/{id}"), &[], options).await?;
        Film::from_value(&value).ok_or_else(|| ProviderError::NotFound(DISPLAY_NAME.to_owned()))
    }
}

fn request_error(message: impl Into<String>) -> ProviderError {
    ProviderError::Request {
        provider: DISPLAY_NAME.to_owned(),
        message: message.into(),
    }
}

fn missing_key() -> ProviderError {
    let (url, fields) = MovieMeterProvider::schema()
        .map(|s| {
            (
                s.registration_url.to_owned(),
                vec![catalogue::API_KEY.to_owned()],
            )
        })
        .unwrap_or_default();
    ProviderError::MissingCredentials {
        provider: DISPLAY_NAME.to_owned(),
        fields,
        registration_url: url,
    }
}

#[async_trait]
impl Provider for MovieMeterProvider {
    fn id(&self) -> &'static str {
        PROVIDER_ID
    }

    fn name(&self) -> &'static str {
        DISPLAY_NAME
    }

    fn credentials(&self) -> Option<&'static CredentialSchema> {
        Self::schema()
    }

    async fn check_credentials(
        &self,
        credentials: &ResolvedCredentials,
    ) -> Result<(), ProviderError> {
        // One cheap search that is never served from the cache, so a changed key is judged on
        // its own.
        self.send(credentials, "film/", &[("q", "heat")], true, false)
            .await
            .map(|_| ())
    }
}

#[async_trait]
impl MetadataProvider for MovieMeterProvider {
    fn languages(&self) -> &'static [&'static str] {
        &["nl"]
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            movies: true,
            shows: false,
            orderings: Vec::new(),
            lookup_ids: vec![ID_KEY, id_key::IMDB],
            best_effort: false,
        }
    }

    async fn search(
        &self,
        query: &SearchQuery,
        options: &RequestOptions,
    ) -> Result<Vec<SearchHit>, ProviderError> {
        let title = query.title.trim();
        if query.kind != MediaKind::Movie || title.is_empty() {
            return Ok(Vec::new());
        }
        match self.get("film/", &[("q", title)], options).await {
            Ok(value) => Ok(search_films(&value)
                .iter()
                .map(|f| f.build_hit(false))
                .collect()),
            Err(ProviderError::NotFound(_)) => Ok(Vec::new()),
            Err(other) => Err(other),
        }
    }

    async fn lookup(
        &self,
        kind: MediaKind,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Option<SearchHit>, ProviderError> {
        if kind != MediaKind::Movie {
            return Ok(None);
        }
        match self.film(ids, options).await {
            Ok(film) => Ok(Some(film.build_hit(true))),
            Err(ProviderError::NotFound(_)) => Ok(None),
            Err(other) => Err(other),
        }
    }

    async fn details(
        &self,
        kind: MediaKind,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Item, ProviderError> {
        if kind != MediaKind::Movie {
            return Err(ProviderError::Unsupported {
                provider: DISPLAY_NAME.to_owned(),
                operation: "shows",
            });
        }
        Ok(self.film(ids, options).await?.build_item())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn movies_only() {
        let provider = MovieMeterProvider::with_base_url(
            HttpClient::builder().build().unwrap(),
            CredentialResolver::new(Arc::new(crate::MemoryUserCredentials::default())),
            "http://127.0.0.1:1",
        );
        let capabilities = provider.capabilities();
        assert!(capabilities.movies && !capabilities.shows);
        assert_eq!(provider.id(), "moviemeter");
    }

    #[test]
    fn limits_are_polite() {
        assert_eq!(default_limits().max_concurrent, 2);
    }
}
