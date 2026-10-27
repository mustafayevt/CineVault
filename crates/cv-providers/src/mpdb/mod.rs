//! The MPDb.tv provider: French movie data and artwork from a private service.
//!
//! MPDb.tv works for subscribers only. Every request carries the app key, the account name
//! (base64url) and a subscription key derived from them, all in the URL path:
//!
//! ```text
//! GET {base}/search/{api key}/{username, base64url}/{signature}/movies/{query}
//! GET {base}/movies/{api key}/{username, base64url}/{signature}/{id}
//! ```
//!
//! where the signature is `sha1(username + api key + subscription key)` in hex. The user supplies
//! the three values in Settings (none of them can ship with a build), and they are read from the
//! [`CredentialResolver`] on every request, so a change applies to the next call. They are masked
//! wherever the HTTP client prints a request.
//!
//! * [`MetadataProvider::search`] reads the search list; [`MetadataProvider::lookup`] and
//!   [`MetadataProvider::details`] read one movie by the MPDb.tv ID (`mpdb`).
//! * Titles, plots, runtimes, genres and countries (French names mapped to the standard ones),
//!   cast and crew, ratings and the artwork lists come from the movie answer; the artwork lists
//!   (posters, fanart, discs, clear art, clear logos, banners) are also offered through
//!   [`ArtworkProvider`].
//!
//! MPDb.tv publishes no schema, so the answers are read leniently (see [`convert`]) and the
//! fixtures are hand-built; the opt-in live test confirms the names with a subscription.

mod convert;
mod digest;

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use cv_domain::{Item, ProviderIds};
use serde_json::Value;

use crate::catalogue;
use crate::credentials::{CredentialResolver, CredentialSchema, ResolvedCredentials};
use crate::error::ProviderError;
use crate::http::{HttpClient, HttpRequest, HttpResponse, ProviderLimits};
use crate::model::{
    ArtworkCandidate, Capabilities, MediaKind, RequestOptions, SearchHit, SearchQuery,
};
use crate::traits::{ArtworkProvider, MetadataProvider, Provider};

pub use convert::RATING_ID;

use convert::{Movie, detail_movie, search_movies};

/// The provider key and HTTP lane id.
pub const PROVIDER_ID: &str = catalogue::key::MPDB;
/// The provider's ID key in [`ProviderIds`].
pub const ID_KEY: &str = "mpdb";
/// The API server.
pub const DEFAULT_BASE_URL: &str = "https://mpdb.tv/api/v1";

const DISPLAY_NAME: &str = "MPDb.tv";

/// How fast requests are sent. MPDb.tv publishes no limit, so this is a polite one.
pub fn default_limits() -> ProviderLimits {
    ProviderLimits::new(2, 2, Duration::from_secs(1)).named(DISPLAY_NAME)
}

/// The path segments that identify an account: the app key, the account name as base64url and
/// the signature.
struct Account {
    api_key: String,
    user: String,
    signature: String,
}

impl Account {
    fn of(credentials: &ResolvedCredentials) -> Result<Self, ProviderError> {
        let get = |field: &str| {
            credentials
                .get(field)
                .map(|v| v.expose().trim().to_owned())
                .filter(|v| !v.is_empty())
        };
        let (api_key, username, subscription) = (
            get(catalogue::API_KEY),
            get(catalogue::MPDB_USERNAME),
            get(catalogue::MPDB_SUBSCRIPTION_KEY),
        );
        let (Some(api_key), Some(username), Some(subscription)) = (api_key, username, subscription)
        else {
            return Err(missing(credentials));
        };
        Ok(Self {
            signature: digest::sha1_hex(format!("{username}{api_key}{subscription}").as_bytes()),
            user: digest::base64url(username.as_bytes()),
            api_key,
        })
    }
}

fn missing(credentials: &ResolvedCredentials) -> ProviderError {
    let schema = MpdbProvider::schema();
    ProviderError::MissingCredentials {
        provider: DISPLAY_NAME.to_owned(),
        fields: schema
            .map(|s| credentials.missing(s))
            .unwrap_or_default()
            .into_iter()
            .map(str::to_owned)
            .collect(),
        registration_url: schema.map_or("", |s| s.registration_url).to_owned(),
    }
}

struct Inner {
    http: HttpClient,
    resolver: CredentialResolver,
    base_url: String,
}

/// The MPDb.tv provider. Cloning is cheap.
#[derive(Clone)]
pub struct MpdbProvider {
    inner: Arc<Inner>,
}

impl MpdbProvider {
    /// A provider for the real API server. It sets the [`default_limits`] on its lane.
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

    /// GET the path below the account segments, with the credentials resolved now, and parse the
    /// JSON answer.
    async fn get(
        &self,
        kind: &str,
        tail: &[&str],
        options: &RequestOptions,
    ) -> Result<Value, ProviderError> {
        let credentials = self.inner.resolver.require(PROVIDER_ID)?;
        let response = self
            .send(&credentials, kind, tail, options.bypass_cache, true)
            .await?;
        response.json().map_err(|e| {
            tracing::debug!(error = %e, "MPDb.tv answered with an unexpected body");
            request_error("the response was not in the expected format")
        })
    }

    /// Builds the URL `{base}/{kind}/{api key}/{user}/{signature}/{tail...}`, with every segment
    /// escaped, and sends it. The segments that carry the account are registered as secrets so
    /// they are masked wherever the request is printed.
    async fn send(
        &self,
        credentials: &ResolvedCredentials,
        kind: &str,
        tail: &[&str],
        bypass_cache: bool,
        cached: bool,
    ) -> Result<HttpResponse, ProviderError> {
        let account = Account::of(credentials)?;
        let mut url = reqwest::Url::parse(&format!("{}/", self.inner.base_url))
            .map_err(|_| request_error("invalid base address"))?;
        url.path_segments_mut()
            .map_err(|()| request_error("invalid base address"))?
            .pop_if_empty()
            .extend([kind, &account.api_key, &account.user, &account.signature])
            .extend(tail.iter().copied());
        let mut request = HttpRequest::get(PROVIDER_ID, url.as_str())
            .secret(&account.api_key)
            .secret(&account.user)
            .secret(&account.signature)
            .bypass_cache(bypass_cache);
        if !cached {
            request = request.uncached();
        }
        let response = self.inner.http.send(request).await?;
        match response.status() {
            s if (200..300).contains(&s) => Ok(response),
            401 | 403 => Err(ProviderError::Auth {
                provider: DISPLAY_NAME.to_owned(),
                message: "the API key, account name or subscription key was not accepted"
                    .to_owned(),
            }),
            404 => Err(ProviderError::NotFound(DISPLAY_NAME.to_owned())),
            s => Err(request_error(format!("HTTP {s}"))),
        }
    }

    /// The movie for an ID, `None` when the ID is not an MPDb.tv one.
    async fn movie(
        &self,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Movie, ProviderError> {
        let id = ids
            .get(ID_KEY)
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .ok_or_else(|| ProviderError::NotFound(DISPLAY_NAME.to_owned()))?;
        let value = self.get("movies", &[id], options).await?;
        detail_movie(&value).ok_or_else(|| ProviderError::NotFound(DISPLAY_NAME.to_owned()))
    }
}

fn request_error(message: impl Into<String>) -> ProviderError {
    ProviderError::Request {
        provider: DISPLAY_NAME.to_owned(),
        message: message.into(),
    }
}

#[async_trait]
impl Provider for MpdbProvider {
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
        // One cheap search that is never served from the cache, so changed credentials are
        // judged on their own.
        self.send(credentials, "search", &["movies", "heat"], true, false)
            .await
            .map(|_| ())
    }
}

#[async_trait]
impl MetadataProvider for MpdbProvider {
    fn languages(&self) -> &'static [&'static str] {
        &["fr"]
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            movies: true,
            shows: false,
            orderings: Vec::new(),
            lookup_ids: vec![ID_KEY],
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
        match self.get("search", &["movies", title], options).await {
            Ok(value) => Ok(search_movies(&value)
                .iter()
                .map(|m| m.build_hit(false))
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
        if kind != MediaKind::Movie || !ids.contains(ID_KEY) {
            return Ok(None);
        }
        match self.movie(ids, options).await {
            Ok(movie) => Ok(Some(movie.build_hit(true))),
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
        Ok(self.movie(ids, options).await?.build_item())
    }
}

#[async_trait]
impl ArtworkProvider for MpdbProvider {
    fn offers(&self, kind: MediaKind, slot: cv_domain::MediaFileType) -> bool {
        kind == MediaKind::Movie
            && matches!(
                slot,
                cv_domain::MediaFileType::Poster
                    | cv_domain::MediaFileType::Fanart
                    | cv_domain::MediaFileType::Disc
                    | cv_domain::MediaFileType::Clearart
                    | cv_domain::MediaFileType::Clearlogo
                    | cv_domain::MediaFileType::Banner
            )
    }

    async fn artwork(
        &self,
        kind: MediaKind,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Vec<ArtworkCandidate>, ProviderError> {
        if kind != MediaKind::Movie || !ids.contains(ID_KEY) {
            return Ok(Vec::new());
        }
        match self.movie(ids, options).await {
            Ok(movie) => Ok(movie.artwork()),
            Err(ProviderError::NotFound(_)) => Ok(Vec::new()),
            Err(other) => Err(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credentials(values: &[(&str, &str)]) -> ResolvedCredentials {
        ResolvedCredentials::from_user_values(
            MpdbProvider::schema().unwrap(),
            values.iter().copied(),
        )
        .unwrap()
    }

    #[test]
    fn the_account_segments_follow_the_documented_derivation() {
        let account = Account::of(&credentials(&[
            ("api_key", "KEY"),
            ("username", "test"),
            ("subscription_key", "ABO"),
        ]))
        .unwrap();
        assert_eq!(account.user, "dGVzdA==");
        assert_eq!(account.signature, digest::sha1_hex(b"testKEYABO"));
        assert_eq!(account.api_key, "KEY");
    }

    #[test]
    fn a_missing_field_names_what_is_missing() {
        let Err(ProviderError::MissingCredentials { fields, .. }) =
            Account::of(&credentials(&[("api_key", "KEY"), ("username", "test")]))
        else {
            panic!("should be missing credentials");
        };
        assert_eq!(fields, ["subscription_key"]);
    }

    #[test]
    fn movies_only() {
        let provider = MpdbProvider::with_base_url(
            HttpClient::builder().build().unwrap(),
            CredentialResolver::new(Arc::new(crate::MemoryUserCredentials::default())),
            "http://127.0.0.1:1",
        );
        let capabilities = provider.capabilities();
        assert!(capabilities.movies && !capabilities.shows);
        assert_eq!(provider.id(), "mpdb");
    }
}
