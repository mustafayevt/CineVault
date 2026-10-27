//! The TVmaze provider: TV shows from the public TVmaze API, which needs no key.
//!
//! * [`MetadataProvider::search`] uses `/search/shows`.
//! * [`MetadataProvider::lookup`] tries the TVmaze ID (`/shows/{id}`), then the IMDb ID
//!   (`/lookup/shows?imdb=`), then the TVDB ID (`/lookup/shows?thetvdb=`). The lookup endpoints
//!   answer with a redirect to the show, which the HTTP client follows.
//! * [`MetadataProvider::details`] reads the show with its cast, crew, images and seasons
//!   embedded, plus its alternate episode lists, which become the show's episode groups.
//! * [`MetadataProvider::episodes`] returns the aired order (`/shows/{id}/episodes?specials=1`)
//!   or one alternate list. The first alternate list marked as a DVD release is the DVD
//!   ordering; the others are alternate orderings named as TVmaze names them.
//!
//! TVmaze lists TV only, so movie requests find nothing. Requests go through the shared
//! [`HttpClient`] under the lane `tvmaze` (20 calls per 10 seconds).

mod api;
mod convert;

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use cv_domain::{EpisodeGroup, EpisodeOrdering, Item, ProviderIds, id_key};
use serde::de::DeserializeOwned;

use crate::error::ProviderError;
use crate::http::{HttpClient, HttpRequest, HttpResponse, ProviderLimits};
use crate::model::{Capabilities, MediaKind, RequestOptions, SearchHit, SearchQuery};
use crate::shared::imdb_title_id;
use crate::traits::{MetadataProvider, Provider};

use api::{AlternateEpisode, AlternateList, Episode, SearchEntry, Show};

pub use convert::RATING_ID;

/// The provider key and HTTP lane id.
pub const PROVIDER_ID: &str = "tvmaze";
/// The API server.
pub const DEFAULT_BASE_URL: &str = "https://api.tvmaze.com";

const DISPLAY_NAME: &str = "TVmaze";

/// How fast requests are sent: TVmaze allows 20 calls per 10 seconds.
pub fn default_limits() -> ProviderLimits {
    ProviderLimits::new(2, 20, Duration::from_secs(10)).named(DISPLAY_NAME)
}

type Params<'a> = &'a [(&'a str, &'a str)];

struct Inner {
    http: HttpClient,
    base_url: String,
}

/// The TVmaze provider. Cloning is cheap.
#[derive(Clone)]
pub struct TvmazeProvider {
    inner: Arc<Inner>,
}

impl TvmazeProvider {
    /// A provider for the real API server. It sets the [`default_limits`] on the `tvmaze` lane.
    pub fn new(http: HttpClient) -> Self {
        http.set_provider_limits(PROVIDER_ID, default_limits());
        Self::with_base_url(http, DEFAULT_BASE_URL)
    }

    /// A provider for another server, such as a local one in tests. Lane limits are left as
    /// configured.
    pub fn with_base_url(http: HttpClient, base_url: &str) -> Self {
        Self {
            inner: Arc::new(Inner {
                http,
                base_url: base_url.trim_end_matches('/').to_owned(),
            }),
        }
    }

    async fn send(
        &self,
        path: &str,
        params: Params<'_>,
        options: &RequestOptions,
    ) -> Result<HttpResponse, ProviderError> {
        let url = format!("{}/{}", self.inner.base_url, path.trim_start_matches('/'));
        let mut request = HttpRequest::get(PROVIDER_ID, &url)
            .header("Accept", "application/json")
            .bypass_cache(options.bypass_cache);
        for (name, value) in params {
            request = request.query(name, value);
        }
        let response = self.inner.http.send(request).await?;
        match response.status() {
            s if (200..300).contains(&s) => Ok(response),
            404 => Err(ProviderError::NotFound(DISPLAY_NAME.to_owned())),
            s => Err(request_error(format!("HTTP {s}"))),
        }
    }

    async fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        params: Params<'_>,
        options: &RequestOptions,
    ) -> Result<T, ProviderError> {
        self.send(path, params, options).await?.json().map_err(|e| {
            tracing::debug!(error = %e, "TVmaze answered with an unexpected body");
            request_error("the response was not in the expected format")
        })
    }

    async fn get_optional<T: DeserializeOwned>(
        &self,
        path: &str,
        params: Params<'_>,
        options: &RequestOptions,
    ) -> Result<Option<T>, ProviderError> {
        match self.get(path, params, options).await {
            Ok(value) => Ok(Some(value)),
            Err(ProviderError::NotFound(_)) => Ok(None),
            Err(other) => Err(other),
        }
    }

    /// The show found by one of its IDs.
    async fn find_show(
        &self,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Option<Show>, ProviderError> {
        if let Some(id) = ids.get_u64(id_key::TVMAZE)
            && let Some(show) = self
                .get_optional::<Show>(&format!("shows/{id}"), &[], options)
                .await?
        {
            return Ok(Some(show));
        }
        if let Some(imdb) = ids.get(id_key::IMDB).and_then(imdb_title_id)
            && let Some(show) = self
                .get_optional::<Show>("lookup/shows", &[("imdb", imdb.as_str())], options)
                .await?
        {
            return Ok(Some(show));
        }
        if let Some(tvdb) = ids.get_u64(id_key::TVDB) {
            let tvdb = tvdb.to_string();
            return self
                .get_optional::<Show>("lookup/shows", &[("thetvdb", tvdb.as_str())], options)
                .await;
        }
        Ok(None)
    }

    /// The TVmaze ID of a show: the one in `ids`, else found through the IMDb or TVDB ID.
    async fn show_id(
        &self,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<u64, ProviderError> {
        if let Some(id) = ids.get_u64(id_key::TVMAZE) {
            return Ok(id);
        }
        self.find_show(ids, options)
            .await?
            .and_then(|s| s.id)
            .ok_or_else(|| ProviderError::NotFound(DISPLAY_NAME.to_owned()))
    }

    /// The show's alternate lists as episode groups with their list IDs.
    async fn groups(
        &self,
        id: u64,
        options: &RequestOptions,
    ) -> Result<Vec<(EpisodeGroup, u64)>, ProviderError> {
        let lists: Vec<AlternateList> = self
            .get_optional(&format!("shows/{id}/alternatelists"), &[], options)
            .await?
            .unwrap_or_default();
        Ok(convert::groups_of(&lists))
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
impl Provider for TvmazeProvider {
    fn id(&self) -> &'static str {
        PROVIDER_ID
    }

    fn name(&self) -> &'static str {
        DISPLAY_NAME
    }
}

#[async_trait]
impl MetadataProvider for TvmazeProvider {
    fn languages(&self) -> &'static [&'static str] {
        &["en"]
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            movies: false,
            shows: true,
            orderings: vec![
                EpisodeOrdering::Aired,
                EpisodeOrdering::Dvd,
                EpisodeOrdering::Alternate,
            ],
            lookup_ids: vec![id_key::TVMAZE, id_key::IMDB, id_key::TVDB],
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
        let entries: Vec<SearchEntry> = self
            .get_optional("search/shows", &[("q", title)], options)
            .await?
            .unwrap_or_default();
        Ok(entries
            .iter()
            .filter_map(|e| convert::build_hit(&e.show, false))
            .collect())
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
        Ok(self
            .find_show(ids, options)
            .await?
            .and_then(|s| convert::build_hit(&s, true)))
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
        let id = self.show_id(ids, options).await?;
        let show: Show = self
            .get(
                &format!("shows/{id}"),
                &[
                    ("embed[]", "cast"),
                    ("embed[]", "crew"),
                    ("embed[]", "images"),
                    ("embed[]", "seasons"),
                ],
                options,
            )
            .await?;
        let groups: Vec<EpisodeGroup> = self
            .groups(id, options)
            .await?
            .into_iter()
            .map(|(group, _)| group)
            .collect();
        Ok(convert::build_show(&show, &groups))
    }

    async fn episodes(
        &self,
        show: &ProviderIds,
        ordering: &EpisodeGroup,
        options: &RequestOptions,
    ) -> Result<Vec<Item>, ProviderError> {
        let id = self.show_id(show, options).await?;
        let items = if ordering.ordering == EpisodeOrdering::Aired {
            let episodes: Vec<Episode> = self
                .get(
                    &format!("shows/{id}/episodes"),
                    &[("specials", "1")],
                    options,
                )
                .await?;
            convert::aired_episodes(&episodes)
        } else {
            let list = self
                .groups(id, options)
                .await?
                .into_iter()
                .find(|(group, _)| group == ordering)
                .map(|(_, list)| list)
                // The show lacks this ordering; the caller shows "ordering not available".
                .ok_or_else(|| ProviderError::NotFound(DISPLAY_NAME.to_owned()))?;
            let entries: Vec<AlternateEpisode> = self
                .get(
                    &format!("alternatelists/{list}/alternateepisodes"),
                    &[("embed", "episodes")],
                    options,
                )
                .await?;
            convert::alternate_episodes(&entries, ordering)
        };
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
    fn limits_follow_the_documented_rate() {
        let limits = default_limits();
        assert_eq!(limits.requests, 20);
        assert_eq!(limits.period, Duration::from_secs(10));
        assert_eq!(limits.name.as_deref(), Some("TVmaze"));
    }

    #[test]
    fn movies_are_not_offered() {
        assert!(matches!(
            movies_unsupported(),
            ProviderError::Unsupported { .. }
        ));
    }
}
