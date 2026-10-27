//! The Trakt provider for metadata.
//!
//! This is the public-API side of Trakt: titles, people, translations, episodes and the Trakt
//! rating, all read with the app's **client ID** alone (the `trakt-api-key` header). Signing a
//! user in and syncing their watch history is the OAuth side, which belongs to the sync
//! service and needs the client secret too; nothing here uses the secret or a user token.
//!
//! * [`MetadataProvider::search`] uses `/search/{movie|show}` with the `years` filter.
//! * [`MetadataProvider::lookup`] tries the Trakt, IMDb, TMDB and TVDB IDs in turn through
//!   `/search/{id type}/{id}`; a hit found that way is the requested title.
//! * [`MetadataProvider::details`] reads the summary, the people and, when the preferred
//!   language is not English, the translation for it. Trakt's own title and text are English.
//! * [`MetadataProvider::episodes`] reads every season with its episodes in one request. Trakt
//!   offers the aired order, and the absolute order where it has absolute numbers.
//! * [`RatingProvider::ratings`] returns the Trakt community rating.
//!
//! Requests go through the shared [`HttpClient`] under the lane `trakt`.

mod api;
mod convert;

use std::sync::Arc;

use async_trait::async_trait;
use cv_domain::{EpisodeGroup, EpisodeNumber, EpisodeOrdering, Item, ItemDetail, ProviderIds};
use cv_domain::{Ratings, id_key};
use serde::de::DeserializeOwned;

use crate::catalogue;
use crate::credentials::{CredentialResolver, CredentialSchema, ResolvedCredentials};
use crate::error::ProviderError;
use crate::http::{HttpClient, HttpRequest, HttpResponse, ProviderLimits};
use crate::model::{Capabilities, MediaKind, RequestOptions, SearchHit, SearchQuery};
use crate::shared::imdb_title_id;
use crate::traits::{MetadataProvider, Provider, RatingProvider};

use api::{ApiError, People, SearchResult, Season, Summary, Translation};

pub use convert::RATING_ID;

/// The provider key and HTTP lane id.
pub const PROVIDER_ID: &str = catalogue::key::TRAKT;
/// The API server.
pub const DEFAULT_BASE_URL: &str = "https://api.trakt.tv";

const DISPLAY_NAME: &str = "Trakt";

/// The credential field metadata requests need.
const CLIENT_ID: &str = "client_id";

/// How fast requests are sent: Trakt allows 1,000 reads per five minutes.
pub fn default_limits() -> ProviderLimits {
    ProviderLimits::new(4, 3, std::time::Duration::from_secs(1)).named(DISPLAY_NAME)
}

/// Query parameters of a request.
type Params<'a> = &'a [(&'a str, &'a str)];

struct Inner {
    http: HttpClient,
    resolver: CredentialResolver,
    base_url: String,
}

/// The Trakt metadata provider. Cloning is cheap.
#[derive(Clone)]
pub struct TraktProvider {
    inner: Arc<Inner>,
}

impl TraktProvider {
    /// A provider for the real API server. It sets the [`default_limits`] on the `trakt` lane.
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

    /// The resolved credentials. Metadata needs the client ID only.
    fn credentials(&self) -> Result<ResolvedCredentials, ProviderError> {
        let resolved = self
            .inner
            .resolver
            .resolve(PROVIDER_ID)
            .map_err(|e| ProviderError::Other(e.to_string()))?;
        if resolved.get(CLIENT_ID).is_none() {
            return Err(missing_client_id());
        }
        Ok(resolved)
    }

    /// GET `path` and parse the JSON answer. The client ID is resolved now, so one entered or
    /// reset in Settings applies to the next call.
    async fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        params: Params<'_>,
        options: &RequestOptions,
    ) -> Result<T, ProviderError> {
        let credentials = self.credentials()?;
        let response = self
            .send(&credentials, path, params, options.bypass_cache, true)
            .await?;
        response.json().map_err(|e| {
            tracing::debug!(error = %e, "Trakt answered with an unexpected body");
            request_error("the response was not in the expected format")
        })
    }

    /// Like [`Self::get`], but a missing resource is `Ok(None)`.
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

    async fn send(
        &self,
        credentials: &ResolvedCredentials,
        path: &str,
        params: Params<'_>,
        bypass_cache: bool,
        cached: bool,
    ) -> Result<HttpResponse, ProviderError> {
        // A rejected or rate-limited request is reported as a problem with the user's own Trakt
        // client ID, so the message can point to its card on the Providers page.
        self.send_with(credentials, path, params, bypass_cache, cached)
            .await
            .map_err(|e| e.with_key_hint(credentials, PROVIDER_ID))
    }

    async fn send_with(
        &self,
        credentials: &ResolvedCredentials,
        path: &str,
        params: Params<'_>,
        bypass_cache: bool,
        cached: bool,
    ) -> Result<HttpResponse, ProviderError> {
        let client_id = credentials
            .get(CLIENT_ID)
            .map(|v| v.expose().trim().to_owned())
            .filter(|k| !k.is_empty())
            .ok_or_else(missing_client_id)?;
        let url = format!("{}/{}", self.inner.base_url, path.trim_start_matches('/'));
        let mut request = HttpRequest::get(PROVIDER_ID, &url)
            .credential_header("trakt-api-key", &client_id)
            .header("trakt-api-version", "2")
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .bypass_cache(bypass_cache);
        for (name, value) in params {
            request = request.query(name, value);
        }
        if !cached {
            request = request.uncached();
        }
        let response = self.inner.http.send(request).await?;
        check_status(&response)?;
        Ok(response)
    }

    // --- IDs ---------------------------------------------------------------------------

    /// The first match for one external ID, as a summary of the wanted kind.
    async fn find_by_id(
        &self,
        kind: MediaKind,
        id_type: &str,
        id: &str,
        options: &RequestOptions,
    ) -> Result<Option<Summary>, ProviderError> {
        let results: Option<Vec<SearchResult>> = self
            .get_optional(
                &format!("search/{id_type}/{id}"),
                &[("type", search_type(kind)), ("extended", "full")],
                options,
            )
            .await?;
        Ok(results
            .unwrap_or_default()
            .into_iter()
            .find_map(|r| r.into_summary(kind == MediaKind::Show)))
    }

    /// The summary of a title by any ID it has, trying them in order (see
    /// [`Self::lookup`]).
    async fn find_summary(
        &self,
        kind: MediaKind,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Option<Summary>, ProviderError> {
        for (key, id_type) in lookup_order(kind) {
            let Some(id) = ids.get(key).map(str::trim).filter(|v| !v.is_empty()) else {
                continue;
            };
            let id = if *key == id_key::IMDB {
                match imdb_title_id(id) {
                    Some(id) => id,
                    None => continue,
                }
            } else {
                id.to_owned()
            };
            if let Some(summary) = self.find_by_id(kind, id_type, &id, options).await? {
                return Ok(Some(summary));
            }
        }
        Ok(None)
    }

    /// The ID Trakt's own paths take: the Trakt ID, else the IMDb ID, else one found through the
    /// TMDB or TVDB ID.
    async fn path_id(
        &self,
        kind: MediaKind,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<String, ProviderError> {
        if let Some(id) = ids.get_u64(id_key::TRAKT) {
            return Ok(id.to_string());
        }
        if let Some(imdb) = ids.get(id_key::IMDB).and_then(imdb_title_id) {
            return Ok(imdb);
        }
        self.find_summary(kind, ids, options)
            .await?
            .and_then(|s| s.ids.trakt)
            .map(|id| id.to_string())
            .ok_or_else(|| ProviderError::NotFound(DISPLAY_NAME.to_owned()))
    }

    // --- details -----------------------------------------------------------------------

    async fn summary(
        &self,
        kind: MediaKind,
        id: &str,
        options: &RequestOptions,
    ) -> Result<Summary, ProviderError> {
        self.get(
            &format!("{}/{id}", collection(kind)),
            &[("extended", "full")],
            options,
        )
        .await
    }

    /// The translation for the preferred language, `None` for English (Trakt's own text) or when
    /// Trakt has none.
    async fn translation(
        &self,
        kind: MediaKind,
        id: &str,
        options: &RequestOptions,
    ) -> Result<Option<Translation>, ProviderError> {
        let language: String = options
            .language
            .trim()
            .split(['-', '_'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        if language.len() != 2 || language == "en" {
            return Ok(None);
        }
        let translations: Vec<Translation> = self
            .get_optional(
                &format!("{}/{id}/translations/{language}", collection(kind)),
                &[],
                options,
            )
            .await?
            .unwrap_or_default();
        Ok(convert::pick_translation(&translations, options).cloned())
    }
}

/// Order in which a title's IDs are tried: Trakt's own, then the ones other services share.
fn lookup_order(kind: MediaKind) -> &'static [(&'static str, &'static str)] {
    match kind {
        MediaKind::Movie => &[
            (id_key::TRAKT, "trakt"),
            (id_key::IMDB, "imdb"),
            (id_key::TMDB, "tmdb"),
        ],
        MediaKind::Show => &[
            (id_key::TRAKT, "trakt"),
            (id_key::IMDB, "imdb"),
            (id_key::TMDB, "tmdb"),
            (id_key::TVDB, "tvdb"),
        ],
    }
}

fn collection(kind: MediaKind) -> &'static str {
    match kind {
        MediaKind::Movie => "movies",
        MediaKind::Show => "shows",
    }
}

fn search_type(kind: MediaKind) -> &'static str {
    match kind {
        MediaKind::Movie => "movie",
        MediaKind::Show => "show",
    }
}

fn request_error(message: impl Into<String>) -> ProviderError {
    ProviderError::Request {
        provider: DISPLAY_NAME.to_owned(),
        message: message.into(),
    }
}

fn missing_client_id() -> ProviderError {
    let url = TraktProvider::schema()
        .map(|s| s.registration_url.to_owned())
        .unwrap_or_default();
    ProviderError::MissingCredentials {
        provider: DISPLAY_NAME.to_owned(),
        fields: vec![CLIENT_ID.to_owned()],
        registration_url: url,
    }
}

/// Maps an HTTP status to the error a caller can act on. 2xx passes.
fn check_status(response: &HttpResponse) -> Result<(), ProviderError> {
    let status = response.status();
    if response.is_success() {
        return Ok(());
    }
    let message = response
        .json::<ApiError>()
        .ok()
        .and_then(|e| e.error_description.or(e.error))
        .map(|m| m.trim().to_owned())
        .filter(|m| !m.is_empty());
    match status {
        401 | 403 => Err(ProviderError::Auth {
            provider: DISPLAY_NAME.to_owned(),
            message: message.unwrap_or_else(|| "the client ID was not accepted".to_owned()),
        }),
        404 => Err(ProviderError::NotFound(DISPLAY_NAME.to_owned())),
        _ => Err(request_error(match message {
            Some(m) => format!("HTTP {status}: {m}"),
            None => format!("HTTP {status}"),
        })),
    }
}

#[async_trait]
impl Provider for TraktProvider {
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
        // One cheap call that is never served from the cache, so a changed ID is judged on its
        // own. Only the client ID matters here.
        self.send(credentials, "genres/movies", &[], true, false)
            .await
            .map(|_| ())
    }
}

#[async_trait]
impl MetadataProvider for TraktProvider {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            movies: true,
            shows: true,
            orderings: vec![EpisodeOrdering::Aired, EpisodeOrdering::Absolute],
            lookup_ids: vec![id_key::TRAKT, id_key::IMDB, id_key::TMDB, id_key::TVDB],
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
        let year = query.year.map(|y| y.to_string());
        let mut params = vec![("query", title), ("extended", "full"), ("limit", "20")];
        if let Some(year) = year.as_deref() {
            params.push(("years", year));
        }
        let results: Vec<SearchResult> = self
            .get_optional(
                &format!("search/{}", search_type(query.kind)),
                &params,
                options,
            )
            .await?
            .unwrap_or_default();
        Ok(results
            .into_iter()
            .filter_map(|r| r.into_summary(query.kind == MediaKind::Show))
            .filter_map(|s| convert::build_hit(&s, false))
            .collect())
    }

    async fn lookup(
        &self,
        kind: MediaKind,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Option<SearchHit>, ProviderError> {
        Ok(self
            .find_summary(kind, ids, options)
            .await?
            .and_then(|s| convert::build_hit(&s, true)))
    }

    async fn details(
        &self,
        kind: MediaKind,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Item, ProviderError> {
        let id = self.path_id(kind, ids, options).await?;
        let summary = self.summary(kind, &id, options).await?;
        let people: Option<People> = self
            .get_optional(&format!("{}/{id}/people", collection(kind)), &[], options)
            .await?;
        let translation = self.translation(kind, &id, options).await?;
        Ok(convert::build_item(
            kind,
            &summary,
            people.as_ref(),
            translation.as_ref(),
        ))
    }

    async fn episodes(
        &self,
        show: &ProviderIds,
        ordering: &EpisodeGroup,
        options: &RequestOptions,
    ) -> Result<Vec<Item>, ProviderError> {
        let absolute = match ordering.ordering {
            EpisodeOrdering::Aired => false,
            EpisodeOrdering::Absolute => true,
            // Trakt has no other ordering; the caller shows "ordering not available".
            _ => return Err(ProviderError::NotFound(DISPLAY_NAME.to_owned())),
        };
        let id = self.path_id(MediaKind::Show, show, options).await?;
        let seasons: Vec<Season> = self
            .get(
                &format!("shows/{id}/seasons"),
                &[("extended", "episodes,full")],
                options,
            )
            .await?;
        let mut rows: Vec<(u32, u32, Item)> = Vec::new();
        for season in &seasons {
            let number = season.number.unwrap_or(0);
            for episode in &season.episodes {
                let Some(item) = convert::episode_item(episode, number) else {
                    continue;
                };
                let key = if absolute {
                    let Some(n) = number_in(&item, &EpisodeGroup::absolute()) else {
                        continue;
                    };
                    n
                } else {
                    let Some(n) = number_in(&item, &EpisodeGroup::aired()) else {
                        continue;
                    };
                    n
                };
                rows.push((key.0, key.1, item));
            }
        }
        if rows.is_empty() {
            return Err(ProviderError::NotFound(DISPLAY_NAME.to_owned()));
        }
        rows.sort_by_key(|(season, number, _)| (*season, *number));
        Ok(rows.into_iter().map(|(_, _, item)| item).collect())
    }
}

/// `(season, episode)` of an episode item in one ordering.
fn number_in(item: &Item, group: &EpisodeGroup) -> Option<(u32, u32)> {
    match &item.detail {
        ItemDetail::Episode(fields) => fields
            .numbers
            .get(group)
            .map(|n: &EpisodeNumber| (n.season, n.episode)),
        _ => None,
    }
}

#[async_trait]
impl RatingProvider for TraktProvider {
    async fn ratings(
        &self,
        kind: MediaKind,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Ratings, ProviderError> {
        let id = self.path_id(kind, ids, options).await?;
        let summary = self.summary(kind, &id, options).await?;
        let mut ratings = Ratings::new();
        if let Some(rating) = convert::rating_of(&summary) {
            ratings.insert(rating);
        }
        Ok(ratings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shows_try_the_tvdb_id_and_movies_do_not() {
        assert!(
            lookup_order(MediaKind::Show)
                .iter()
                .any(|(k, _)| *k == id_key::TVDB)
        );
        assert!(
            !lookup_order(MediaKind::Movie)
                .iter()
                .any(|(k, _)| *k == id_key::TVDB)
        );
    }

    #[test]
    fn default_limits_name_the_provider() {
        assert_eq!(default_limits().name.as_deref(), Some("Trakt"));
    }
}
