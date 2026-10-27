//! The TheSportsDB provider: sports leagues as TV shows.
//!
//! The data model follows the way sports libraries are organised: a **league** is a show, a
//! **season** is a season and an **event** (a match, a race, a fight card) is an episode.
//!
//! * [`MetadataProvider::search`] lists the leagues (`all_leagues.php`) and filters them by name
//!   on this side; the service has no league search. The free key lists only a few leagues, a
//!   supporter's key lists them all.
//! * [`MetadataProvider::lookup`] and [`MetadataProvider::details`] read `lookupleague.php` and
//!   `search_all_seasons.php` by the league ID (`thesportsdb`).
//! * [`MetadataProvider::episodes`] reads `eventsseason.php` for every season. A season is
//!   numbered by the year it starts (`2014-2015` is season 2014) and its events are numbered in
//!   the order they were played.
//!
//! **Key.** The key is a path segment of every request. With no key of the user's, the public
//! test key is used, which is enough to try the provider; a supporter's key (Settings,
//! Providers) replaces it. A key the service rejects fails with an authentication error.
//!
//! Requests go through the shared [`HttpClient`] under the lane `thesportsdb`.

mod convert;

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use cv_domain::{EpisodeGroup, EpisodeOrdering, Item, ProviderIds};
use serde_json::Value;

use crate::catalogue;
use crate::credentials::{CredentialResolver, CredentialSchema, ResolvedCredentials};
use crate::error::ProviderError;
use crate::http::{HttpClient, HttpRequest, ProviderLimits};
use crate::model::{Capabilities, MediaKind, RequestOptions, SearchHit, SearchQuery};
use crate::traits::{MetadataProvider, Provider};

/// The provider key and HTTP lane id.
pub const PROVIDER_ID: &str = catalogue::key::THESPORTSDB;
/// The provider's ID key in [`ProviderIds`]: the league ID, or the event ID on an episode.
pub const ID_KEY: &str = "thesportsdb";
/// The API server.
pub const DEFAULT_BASE_URL: &str = "https://www.thesportsdb.com/api/v1/json";
/// The service's public test key, used when the user has none.
pub const FREE_KEY: &str = "123";

const DISPLAY_NAME: &str = "TheSportsDB";

/// How fast requests are sent. The free key allows 30 requests a minute.
pub fn default_limits() -> ProviderLimits {
    ProviderLimits::new(2, 30, Duration::from_secs(60)).named(DISPLAY_NAME)
}

struct Inner {
    http: HttpClient,
    resolver: CredentialResolver,
    base_url: String,
}

/// The TheSportsDB provider. Cloning is cheap.
#[derive(Clone)]
pub struct TheSportsDbProvider {
    inner: Arc<Inner>,
}

impl TheSportsDbProvider {
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

    /// The key to use now: the user's, else the public test key.
    fn key_of(credentials: &ResolvedCredentials) -> String {
        credentials
            .get(catalogue::API_KEY)
            .map(|v| v.expose().trim().to_owned())
            .filter(|k| !k.is_empty())
            .unwrap_or_else(|| FREE_KEY.to_owned())
    }

    /// GET `endpoint` (for example `lookupleague.php`) with the key resolved now and parse the
    /// JSON answer. `stable` data (the league list, the seasons of a league) is cached for a
    /// day, the rest for the API default.
    async fn get(
        &self,
        endpoint: &str,
        params: &[(&str, &str)],
        options: &RequestOptions,
        stable: bool,
    ) -> Result<Value, ProviderError> {
        let credentials = self
            .inner
            .resolver
            .resolve(PROVIDER_ID)
            .map_err(|e| ProviderError::Other(e.to_string()))?;
        self.send(&credentials, endpoint, params, options.bypass_cache, stable)
            .await
    }

    async fn send(
        &self,
        credentials: &ResolvedCredentials,
        endpoint: &str,
        params: &[(&str, &str)],
        bypass_cache: bool,
        stable: bool,
    ) -> Result<Value, ProviderError> {
        let key = Self::key_of(credentials);
        let url = format!("{}/{key}/{endpoint}", self.inner.base_url);
        let mut request = HttpRequest::get(PROVIDER_ID, &url)
            .secret(&key)
            .bypass_cache(bypass_cache);
        if stable {
            request = request.html();
        }
        for (name, value) in params {
            request = request.query(name, value);
        }
        let response = self.inner.http.send(request).await?;
        match response.status() {
            s if (200..300).contains(&s) => response.json().map_err(|e| {
                tracing::debug!(error = %e, "TheSportsDB answered with an unexpected body");
                request_error("the response was not in the expected format")
            }),
            401 | 403 => Err(ProviderError::Auth {
                provider: DISPLAY_NAME.to_owned(),
                message: "the API key was not accepted".to_owned(),
            }),
            404 => Err(ProviderError::NotFound(DISPLAY_NAME.to_owned())),
            s => Err(request_error(format!("HTTP {s}"))),
        }
    }

    /// The league for an ID, `None` when the service has none.
    async fn league(
        &self,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Option<Value>, ProviderError> {
        let Some(id) = ids.get_u64(ID_KEY) else {
            return Ok(None);
        };
        let answer = self
            .get(
                "lookupleague.php",
                &[("id", &id.to_string())],
                options,
                false,
            )
            .await?;
        Ok(convert::first(&answer, "leagues").cloned())
    }

    async fn seasons(
        &self,
        league: &str,
        options: &RequestOptions,
    ) -> Result<Vec<String>, ProviderError> {
        let answer = self
            .get("search_all_seasons.php", &[("id", league)], options, true)
            .await?;
        Ok(convert::season_names(&answer))
    }
}

fn request_error(message: impl Into<String>) -> ProviderError {
    ProviderError::Request {
        provider: DISPLAY_NAME.to_owned(),
        message: message.into(),
    }
}

fn movies_unsupported() -> ProviderError {
    ProviderError::Unsupported {
        provider: DISPLAY_NAME.to_owned(),
        operation: "movies",
    }
}

#[async_trait]
impl Provider for TheSportsDbProvider {
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
        // One lookup that is never served from the cache, so a changed key is judged on its own.
        self.send(
            credentials,
            "lookupleague.php",
            &[("id", "4328")],
            true,
            false,
        )
        .await
        .map(|_| ())
    }
}

#[async_trait]
impl MetadataProvider for TheSportsDbProvider {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            movies: false,
            shows: true,
            orderings: vec![EpisodeOrdering::Aired],
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
        if query.kind != MediaKind::Show || title.is_empty() {
            return Ok(Vec::new());
        }
        let answer = self.get("all_leagues.php", &[], options, true).await?;
        Ok(convert::search_leagues(&answer, title))
    }

    async fn lookup(
        &self,
        kind: MediaKind,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Option<SearchHit>, ProviderError> {
        if kind != MediaKind::Show {
            return Ok(None);
        }
        match self.league(ids, options).await {
            Ok(league) => Ok(league.and_then(|l| convert::build_hit(&l, true, options))),
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
        if kind != MediaKind::Show {
            return Err(movies_unsupported());
        }
        let league = self
            .league(ids, options)
            .await?
            .ok_or_else(|| ProviderError::NotFound(DISPLAY_NAME.to_owned()))?;
        let id = convert::text(&league, "idLeague").unwrap_or_default();
        // The season list only adds season names; a league without one still has details.
        let seasons = match self.seasons(id, options).await {
            Ok(seasons) => seasons,
            Err(ProviderError::NotFound(_)) => Vec::new(),
            Err(other) => return Err(other),
        };
        convert::build_show(&league, &seasons, options)
            .ok_or_else(|| ProviderError::NotFound(DISPLAY_NAME.to_owned()))
    }

    async fn episodes(
        &self,
        show: &ProviderIds,
        ordering: &EpisodeGroup,
        options: &RequestOptions,
    ) -> Result<Vec<Item>, ProviderError> {
        if ordering.ordering != EpisodeOrdering::Aired {
            return Err(ProviderError::NotFound(DISPLAY_NAME.to_owned()));
        }
        let id = show
            .get_u64(ID_KEY)
            .ok_or_else(|| ProviderError::NotFound(DISPLAY_NAME.to_owned()))?
            .to_string();
        let mut items: Vec<Item> = Vec::new();
        for season in self.seasons(&id, options).await? {
            // A season that is not a year (a name such as `Playoffs`) cannot be numbered.
            if convert::season_number(&season).is_none() {
                continue;
            }
            let answer = match self
                .get(
                    "eventsseason.php",
                    &[("id", &id), ("s", &season)],
                    options,
                    false,
                )
                .await
            {
                Ok(answer) => answer,
                // A season without events is empty, not a failure of the whole list.
                Err(ProviderError::NotFound(_)) => continue,
                Err(other) => return Err(other),
            };
            items.extend(convert::season_episodes(&answer, &season));
        }
        if items.is_empty() {
            return Err(ProviderError::NotFound(DISPLAY_NAME.to_owned()));
        }
        Ok(items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_follow_the_free_key() {
        let limits = default_limits();
        assert_eq!(limits.requests, 30);
        assert_eq!(limits.period, Duration::from_secs(60));
    }

    #[test]
    fn the_free_key_is_used_when_the_user_has_none() {
        assert_eq!(
            TheSportsDbProvider::key_of(&ResolvedCredentials::default()),
            FREE_KEY
        );
        let schema = TheSportsDbProvider::schema().unwrap();
        let own = ResolvedCredentials::from_user_values(schema, [("api_key", " my-key ")]).unwrap();
        assert_eq!(TheSportsDbProvider::key_of(&own), "my-key");
    }

    #[test]
    fn movies_are_not_offered() {
        assert!(matches!(
            movies_unsupported(),
            ProviderError::Unsupported { .. }
        ));
    }
}
