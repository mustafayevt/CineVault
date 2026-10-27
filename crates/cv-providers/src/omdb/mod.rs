//! The OMDb provider: movie and series details and ratings from the OMDb API.
//!
//! * [`MetadataProvider::search`] uses `s=` (with the `type` and `y` filters).
//! * [`MetadataProvider::lookup`] and [`MetadataProvider::details`] read a title by IMDb ID
//!   (`i=`, full plot, Rotten Tomatoes fields on). OMDb knows no other ID, so a title without an
//!   IMDb ID is not found; the cross-provider ID completion supplies the ID first.
//! * [`MetadataProvider::episodes`] lists a series' aired episodes, one `Season=` request per
//!   season. OMDb has no other ordering.
//! * [`RatingProvider::ratings`] returns the IMDb, Metacritic and Rotten Tomatoes ratings.
//!
//! OMDb answers in English only, so the request language is ignored. Its errors come back as
//! JSON (`"Response":"False"`), often with HTTP 401, and are mapped to typed errors: a bad key
//! is [`ProviderError::Auth`], the daily limit is [`ProviderError::RateLimited`].
//!
//! Requests go through the shared [`HttpClient`] under the lane `omdb`, and the key comes from
//! the [`CredentialResolver`] on every request.

mod convert;

use std::sync::Arc;

use async_trait::async_trait;
use cv_domain::{EpisodeGroup, EpisodeOrdering, Item, ProviderIds, Ratings, id_key};
use serde_json::Value;

use crate::catalogue;
use crate::credentials::{CredentialResolver, CredentialSchema, ResolvedCredentials};
use crate::error::ProviderError;
use crate::http::{HttpClient, HttpRequest};
use crate::model::{Capabilities, MediaKind, RequestOptions, SearchHit, SearchQuery};
use crate::shared::{imdb_id_of, seconds_to_utc_midnight};
use crate::traits::{MetadataProvider, Provider, RatingProvider};

/// The rating source key OMDb's IMDb rating is stored under.
pub use convert::IMDB_RATING_ID;
pub use convert::{
    METACRITIC_RATING_ID, RT_AUDIENCE_AVERAGE_RATING_ID, RT_AUDIENCE_RATING_ID,
    RT_CRITICS_AVERAGE_RATING_ID, RT_CRITICS_RATING_ID,
};

use convert::{Title, hit_from_search, parse_episodes};

/// The provider key and HTTP lane id.
pub const PROVIDER_ID: &str = catalogue::key::OMDB;
/// The API server.
pub const DEFAULT_BASE_URL: &str = "https://www.omdbapi.com";

const DISPLAY_NAME: &str = "OMDb";

/// A series with more seasons than this is cut off (OMDb is queried once per season).
const MAX_SEASONS: u32 = 100;

struct Inner {
    http: HttpClient,
    resolver: CredentialResolver,
    base_url: String,
}

/// The OMDb provider. Cloning is cheap.
#[derive(Clone)]
pub struct OmdbProvider {
    inner: Arc<Inner>,
}

impl OmdbProvider {
    /// A provider for the real API server.
    pub fn new(http: HttpClient, resolver: CredentialResolver) -> Self {
        Self::with_base_url(http, resolver, DEFAULT_BASE_URL)
    }

    /// A provider for another server, such as a local one in tests.
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

    /// One request with the key resolved now, so a key entered in Settings applies to the next
    /// call. The answer is the parsed JSON, or the error OMDb reported.
    async fn get(
        &self,
        params: &[(&str, &str)],
        options: &RequestOptions,
    ) -> Result<Value, ProviderError> {
        let credentials = self.inner.resolver.require(PROVIDER_ID)?;
        self.send(&credentials, params, options.bypass_cache, true)
            .await
    }

    async fn send(
        &self,
        credentials: &ResolvedCredentials,
        params: &[(&str, &str)],
        bypass_cache: bool,
        cached: bool,
    ) -> Result<Value, ProviderError> {
        self.send_with(credentials, params, bypass_cache, cached)
            .await
            .map_err(|e| e.with_key_hint(credentials, PROVIDER_ID))
    }

    async fn send_with(
        &self,
        credentials: &ResolvedCredentials,
        params: &[(&str, &str)],
        bypass_cache: bool,
        cached: bool,
    ) -> Result<Value, ProviderError> {
        let key = credentials
            .get(catalogue::API_KEY)
            .map(|v| v.expose().trim().to_owned())
            .filter(|k| !k.is_empty())
            .ok_or_else(missing_key)?;
        let mut request = HttpRequest::get(PROVIDER_ID, &format!("{}/", self.inner.base_url))
            .credential_query("apikey", &key)
            .query("r", "json")
            .bypass_cache(bypass_cache);
        for (name, value) in params {
            request = request.query(name, value);
        }
        if !cached {
            request = request.uncached();
        }
        let response = self.inner.http.send(request).await?;
        let body: Option<Value> = response.json().ok();
        if let Some(message) = body.as_ref().and_then(failure_message) {
            return Err(map_failure(&message));
        }
        if !response.is_success() {
            return Err(request_error(format!("HTTP {}", response.status())));
        }
        body.ok_or_else(|| request_error("the response was not in the expected format"))
    }

    /// The title with this IMDb ID (full plot, Rotten Tomatoes fields on).
    async fn title(&self, imdb: &str, options: &RequestOptions) -> Result<Title, ProviderError> {
        let value = self
            .get(
                &[("i", imdb), ("plot", "full"), ("tomatoes", "true")],
                options,
            )
            .await?;
        Title::from_value(&value)
            .ok_or_else(|| request_error("the response was not in the expected format"))
    }

    fn imdb_id(ids: &ProviderIds) -> Result<String, ProviderError> {
        imdb_id_of(ids).ok_or_else(|| ProviderError::NotFound(DISPLAY_NAME.to_owned()))
    }

    /// A title that is a `kind`, or `NotFound`.
    async fn typed_title(
        &self,
        kind: MediaKind,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Title, ProviderError> {
        let id = Self::imdb_id(ids)?;
        let title = self.title(&id, options).await?;
        if title.fits(kind) {
            Ok(title)
        } else {
            Err(ProviderError::NotFound(DISPLAY_NAME.to_owned()))
        }
    }
}

fn request_error(message: impl Into<String>) -> ProviderError {
    ProviderError::Request {
        provider: DISPLAY_NAME.to_owned(),
        message: message.into(),
    }
}

fn missing_key() -> ProviderError {
    let (url, fields) = OmdbProvider::schema()
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

/// The `Error` text of a `"Response":"False"` answer.
fn failure_message(value: &Value) -> Option<String> {
    let failed = value
        .get("Response")
        .and_then(Value::as_str)
        .is_some_and(|r| r.eq_ignore_ascii_case("false"));
    failed.then(|| {
        value
            .get("Error")
            .and_then(Value::as_str)
            .unwrap_or("the request failed")
            .trim()
            .to_owned()
    })
}

/// Maps OMDb's error text to a typed error.
fn map_failure(message: &str) -> ProviderError {
    let lower = message.to_ascii_lowercase();
    if lower.contains("api key") {
        ProviderError::Auth {
            provider: DISPLAY_NAME.to_owned(),
            message: message.to_owned(),
        }
    } else if lower.contains("limit reached") {
        ProviderError::RateLimited {
            provider: DISPLAY_NAME.to_owned(),
            retry_after_secs: seconds_to_utc_midnight(),
        }
    } else if lower.contains("not found") || lower.starts_with("incorrect imdb id") {
        ProviderError::NotFound(DISPLAY_NAME.to_owned())
    } else if lower.contains("too many results") {
        // A search too broad to list: nothing usable, and more words would narrow it.
        ProviderError::NotFound(DISPLAY_NAME.to_owned())
    } else {
        request_error(message)
    }
}

#[async_trait]
impl Provider for OmdbProvider {
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
        // One cheap lookup that is never served from the cache, so a changed key is judged on
        // its own.
        self.send(credentials, &[("i", "tt0133093")], true, false)
            .await
            .map(|_| ())
    }
}

#[async_trait]
impl MetadataProvider for OmdbProvider {
    fn languages(&self) -> &'static [&'static str] {
        &["en"]
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            movies: true,
            shows: true,
            orderings: vec![EpisodeOrdering::Aired],
            lookup_ids: vec![id_key::IMDB],
            best_effort: false,
        }
    }

    async fn search(
        &self,
        query: &SearchQuery,
        options: &RequestOptions,
    ) -> Result<Vec<SearchHit>, ProviderError> {
        let title = query.title.trim();
        if title.is_empty() {
            return Ok(Vec::new());
        }
        let kind = match query.kind {
            MediaKind::Movie => "movie",
            MediaKind::Show => "series",
        };
        let year = query.year.map(|y| y.to_string());
        let mut params = vec![("s", title), ("type", kind)];
        if let Some(year) = year.as_deref() {
            params.push(("y", year));
        }
        match self.get(&params, options).await {
            Ok(value) => Ok(hit_from_search(&value, query.kind)),
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
        if imdb_id_of(ids).is_none() {
            return Ok(None);
        }
        match self.typed_title(kind, ids, options).await {
            Ok(title) => Ok(Some(title.build_hit())),
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
        Ok(self.typed_title(kind, ids, options).await?.build_item(kind))
    }

    async fn episodes(
        &self,
        show: &ProviderIds,
        ordering: &EpisodeGroup,
        options: &RequestOptions,
    ) -> Result<Vec<Item>, ProviderError> {
        if ordering.ordering != EpisodeOrdering::Aired {
            return Err(ProviderError::Unsupported {
                provider: DISPLAY_NAME.to_owned(),
                operation: "episode orderings other than aired",
            });
        }
        let id = Self::imdb_id(show)?;
        let title = self.title(&id, options).await?;
        if !title.fits(MediaKind::Show) {
            return Err(ProviderError::NotFound(DISPLAY_NAME.to_owned()));
        }
        let seasons = title.total_seasons().min(MAX_SEASONS);
        let mut episodes = Vec::new();
        for season in 1..=seasons {
            let number = season.to_string();
            let value = self
                .get(&[("i", id.as_str()), ("Season", number.as_str())], options)
                .await;
            match value {
                Ok(value) => episodes.extend(parse_episodes(&value, season)),
                // A season OMDb lacks leaves a gap, not a failed list.
                Err(ProviderError::NotFound(_)) => {}
                Err(other) => return Err(other),
            }
        }
        if episodes.is_empty() {
            return Err(ProviderError::NotFound(DISPLAY_NAME.to_owned()));
        }
        Ok(episodes)
    }
}

#[async_trait]
impl RatingProvider for OmdbProvider {
    async fn ratings(
        &self,
        kind: MediaKind,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Ratings, ProviderError> {
        Ok(self.typed_title(kind, ids, options).await?.ratings())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failures_map_to_typed_errors() {
        assert!(matches!(
            map_failure("Invalid API key!"),
            ProviderError::Auth { .. }
        ));
        assert!(matches!(
            map_failure("No API key provided."),
            ProviderError::Auth { .. }
        ));
        assert!(matches!(
            map_failure("Request limit reached!"),
            ProviderError::RateLimited { .. }
        ));
        assert!(matches!(
            map_failure("Movie not found!"),
            ProviderError::NotFound(_)
        ));
        assert!(matches!(
            map_failure("Incorrect IMDb ID."),
            ProviderError::NotFound(_)
        ));
        assert!(matches!(
            map_failure("Something odd"),
            ProviderError::Request { .. }
        ));
    }

    #[test]
    fn only_false_responses_are_failures() {
        let ok: Value = serde_json::from_str(r#"{"Response":"True","Title":"Heat"}"#).unwrap();
        assert_eq!(failure_message(&ok), None);
        let bad: Value =
            serde_json::from_str(r#"{"Response":"False","Error":"Movie not found!"}"#).unwrap();
        assert_eq!(failure_message(&bad).as_deref(), Some("Movie not found!"));
    }
}
