//! The AniDB provider: anime from AniDB's HTTP API, with a local title search.
//!
//! * **Search is local.** AniDB has no search request. The provider downloads the title dump
//!   (`anime-titles.dat.gz`, cached for a day by the HTTP cache and kept parsed in memory for two
//!   days, because AniDB allows one download a day) and fuzzy-matches the query against every
//!   title in every language (see [`titles`]).
//! * **Details** come from `httpapi?request=anime&aid=…`: titles in many languages, creators and
//!   studios, the description (AniDB markup removed), ratings, external IDs (MyAnimeList, IMDb,
//!   TMDB), weighty tags as genres, voice actors as cast and every episode. The answer is cached
//!   for a day.
//! * **Episodes** are the regular episodes in AniDB's own numbering, offered as the aired and the
//!   absolute ordering (season 1), and the specials as season 0.
//!
//! **Client registration.** AniDB identifies an app by a registered client name and version,
//! sent as `client` and `clientver` with every request. The name is the provider's `api_key`
//! credential (a build can ship one; a user can register their own at the page in the credential
//! schema), the version is the optional `client_version` field and defaults to `1`.
//!
//! **Rate limit.** AniDB bans clients that send more than one request every two seconds. The
//! provider's lane `anidb` is limited to exactly that ([`default_limits`]) and carries both the
//! API requests and the dump download. An answer that says the client is banned is reported as
//! rate limiting, linked to the provider's card in Settings, and is never cached.

mod convert;
mod titles;
pub(crate) mod xml;

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use cv_domain::{EpisodeGroup, EpisodeOrdering, Item, ProviderIds, id_key};
use tokio::sync::Mutex;

use crate::catalogue;
use crate::credentials::{CredentialResolver, CredentialSchema, ResolvedCredentials};
use crate::error::ProviderError;
use crate::http::{HttpClient, HttpRequest, HttpResponse, ProviderLimits};
use crate::model::{Capabilities, MediaKind, RequestOptions, SearchHit, SearchQuery};
use crate::traits::{MetadataProvider, Provider};

pub use convert::RATING_ID;

use convert::{Anime, Answer, parse_answer};
use titles::TitleIndex;

/// The provider key and HTTP lane id.
pub const PROVIDER_ID: &str = catalogue::key::ANIDB;
/// The provider's ID key in [`ProviderIds`].
pub const ID_KEY: &str = id_key::ANIDB;
/// The HTTP API.
pub const DEFAULT_API_URL: &str = "http://api.anidb.net:9001/httpapi";
/// The title dump.
pub const DEFAULT_TITLES_URL: &str = "https://anidb.net/api/anime-titles.dat.gz";
/// Where AniDB's pictures are.
pub(crate) const IMAGE_BASE: &str = "http://img7.anidb.net/pics/anime/";

const DISPLAY_NAME: &str = "AniDB";

/// The client version sent when the user has not set one.
const DEFAULT_CLIENT_VERSION: &str = "1";

/// How long the parsed title dump is reused: AniDB allows one download a day.
const TITLES_TTL: Duration = Duration::from_secs(2 * 24 * 60 * 60);

/// The most hits a search returns.
const MAX_HITS: usize = 20;

/// AniDB's limit: one request every two seconds, one at a time.
pub fn default_limits() -> ProviderLimits {
    ProviderLimits::one_per(Duration::from_secs(2)).named(DISPLAY_NAME)
}

struct Inner {
    http: HttpClient,
    resolver: CredentialResolver,
    api_url: String,
    titles_url: String,
    /// The parsed dump and when it was read. The lock also keeps two searches from downloading
    /// the dump at once.
    titles: Mutex<Option<(Instant, Arc<TitleIndex>)>>,
}

/// The AniDB provider. Cloning is cheap and shares the parsed title dump.
#[derive(Clone)]
pub struct AnidbProvider {
    inner: Arc<Inner>,
}

impl AnidbProvider {
    /// A provider for the real service. It sets the one-request-per-two-seconds
    /// [`default_limits`] on the `anidb` lane.
    pub fn new(http: HttpClient, resolver: CredentialResolver) -> Self {
        http.set_provider_limits(PROVIDER_ID, default_limits());
        Self::with_urls(http, resolver, DEFAULT_API_URL, DEFAULT_TITLES_URL)
    }

    /// A provider for other servers, such as a local one in tests. Lane limits are left as
    /// configured.
    pub fn with_urls(
        http: HttpClient,
        resolver: CredentialResolver,
        api_url: &str,
        titles_url: &str,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                http,
                resolver,
                api_url: api_url.to_owned(),
                titles_url: titles_url.to_owned(),
                titles: Mutex::new(None),
            }),
        }
    }

    fn schema() -> Option<&'static CredentialSchema> {
        catalogue::CREDENTIAL_SCHEMAS
            .iter()
            .find(|s| s.provider == PROVIDER_ID)
    }

    /// The client name and version to send.
    fn client_of(credentials: &ResolvedCredentials) -> Result<(String, String), ProviderError> {
        let get = |field: &str| {
            credentials
                .get(field)
                .map(|v| v.expose().trim().to_owned())
                .filter(|v| !v.is_empty())
        };
        let Some(name) = get(catalogue::API_KEY) else {
            let schema = Self::schema();
            return Err(ProviderError::MissingCredentials {
                provider: DISPLAY_NAME.to_owned(),
                fields: vec![catalogue::API_KEY.to_owned()],
                registration_url: schema.map_or("", |s| s.registration_url).to_owned(),
            });
        };
        let version = get(catalogue::ANIDB_CLIENT_VERSION)
            .filter(|v| v.bytes().all(|b| b.is_ascii_digit()))
            .unwrap_or_else(|| DEFAULT_CLIENT_VERSION.to_owned());
        Ok((name, version))
    }

    /// Requests an anime and reads the answer.
    async fn fetch_anime(
        &self,
        credentials: &ResolvedCredentials,
        aid: u64,
        bypass_cache: bool,
        cached: bool,
    ) -> Result<Anime, ProviderError> {
        self.fetch_anime_inner(credentials, aid, bypass_cache, cached)
            .await
            .map_err(|e| e.with_key_hint(credentials, PROVIDER_ID))
    }

    async fn fetch_anime_inner(
        &self,
        credentials: &ResolvedCredentials,
        aid: u64,
        bypass_cache: bool,
        cached: bool,
    ) -> Result<Anime, ProviderError> {
        let (client, version) = Self::client_of(credentials)?;
        let mut request = HttpRequest::get(PROVIDER_ID, &self.inner.api_url)
            .query("request", "anime")
            .query("clientver", &version)
            .query("protover", "1")
            .query("aid", &aid.to_string())
            .credential_query("client", &client)
            .bypass_cache(bypass_cache);
        // A day, like the other pages and lists the provider reads.
        request = if cached {
            request.html()
        } else {
            request.uncached()
        };
        let response = self.inner.http.send(request.clone()).await?;
        if !response.is_success() {
            return Err(request_error(format!("HTTP {}", response.status())));
        }
        let text = body_text(&response).map_err(request_error)?;
        match parse_answer(&text) {
            Ok(Answer::Anime(anime)) => Ok(*anime),
            Ok(Answer::Error { code, message }) => {
                let error = answer_error(&code, &message);
                // Only "no such anime" is a stable answer; the rest must not stick in the cache.
                if !matches!(error, ProviderError::NotFound(_)) {
                    self.inner.http.forget(&request).await;
                }
                Err(error)
            }
            Err(reason) => {
                tracing::warn!(%reason, "AniDB answered with an unexpected document");
                self.inner.http.forget(&request).await;
                Err(request_error("the response was not in the expected format"))
            }
        }
    }

    /// The anime for the AniDB ID in `ids`.
    async fn anime(
        &self,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Anime, ProviderError> {
        let aid = ids
            .get_u64(ID_KEY)
            .ok_or_else(|| ProviderError::NotFound(DISPLAY_NAME.to_owned()))?;
        let credentials = self.inner.resolver.require(PROVIDER_ID)?;
        self.fetch_anime(&credentials, aid, options.bypass_cache, true)
            .await
    }

    /// The title index, downloading the dump when the one in memory is older than two days. When
    /// the download fails and an older index exists, the older one is used.
    async fn index(&self, options: &RequestOptions) -> Result<Arc<TitleIndex>, ProviderError> {
        let mut slot = self.inner.titles.lock().await;
        if let Some((read_at, index)) = slot.as_ref()
            && read_at.elapsed() < TITLES_TTL
            && !options.bypass_cache
        {
            return Ok(Arc::clone(index));
        }
        match self.download_titles(options.bypass_cache).await {
            Ok(index) => {
                let index = Arc::new(index);
                *slot = Some((Instant::now(), Arc::clone(&index)));
                Ok(index)
            }
            Err(error) => match slot.as_ref() {
                Some((_, stale)) => {
                    tracing::warn!(%error, "AniDB title dump not refreshed; using the older one");
                    Ok(Arc::clone(stale))
                }
                None => Err(error),
            },
        }
    }

    async fn download_titles(&self, bypass_cache: bool) -> Result<TitleIndex, ProviderError> {
        let request = HttpRequest::get(PROVIDER_ID, &self.inner.titles_url)
            .html()
            .bypass_cache(bypass_cache);
        let response = self.inner.http.send(request.clone()).await?;
        if !response.is_success() {
            return Err(request_error(format!(
                "the title dump could not be downloaded (HTTP {})",
                response.status()
            )));
        }
        let parsed = titles::dump_text(response.body()).and_then(|text| TitleIndex::parse(&text));
        match parsed {
            Ok(index) => {
                tracing::debug!(anime = index.len(), "AniDB title dump read");
                Ok(index)
            }
            Err(reason) => {
                tracing::warn!(%reason, "AniDB title dump could not be read");
                self.inner.http.forget(&request).await;
                Err(request_error(
                    "the title dump was not in the expected format",
                ))
            }
        }
    }
}

/// The body as text: unpacked when a server sent it gzipped without saying so.
fn body_text(response: &HttpResponse) -> Result<String, String> {
    titles::dump_text(response.body())
}

fn request_error(message: impl Into<String>) -> ProviderError {
    ProviderError::Request {
        provider: DISPLAY_NAME.to_owned(),
        message: message.into(),
    }
}

/// AniDB's `<error>` answers as errors: no such anime, a banned client, a client it does not
/// know, anything else.
fn answer_error(code: &str, message: &str) -> ProviderError {
    let lower = message.to_ascii_lowercase();
    if code == "330" || lower.contains("not found") {
        ProviderError::NotFound(DISPLAY_NAME.to_owned())
    } else if lower.contains("banned") || matches!(code, "500" | "555") {
        // A ban lasts about a day.
        ProviderError::RateLimited {
            provider: DISPLAY_NAME.to_owned(),
            retry_after_secs: 86_400,
        }
    } else if lower.contains("client") || matches!(code, "302" | "303" | "309") {
        ProviderError::Auth {
            provider: DISPLAY_NAME.to_owned(),
            message: "the client name or version was not accepted".to_owned(),
        }
    } else {
        request_error(format!(
            "AniDB answered with an error{}",
            if code.is_empty() {
                String::new()
            } else {
                format!(" ({code})")
            }
        ))
    }
}

#[async_trait]
impl Provider for AnidbProvider {
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
        // One request for a well-known anime, never from the cache, so the client is judged on
        // its own. It counts toward the two-second limit like any other.
        match self.fetch_anime(credentials, 1, true, false).await {
            Ok(_) => Ok(()),
            // The client was accepted; this anime just is not there.
            Err(ProviderError::NotFound(_)) => Ok(()),
            Err(other) => Err(other),
        }
    }
}

#[async_trait]
impl MetadataProvider for AnidbProvider {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            movies: true,
            shows: true,
            orderings: vec![EpisodeOrdering::Aired, EpisodeOrdering::Absolute],
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
        if title.is_empty() {
            return Ok(Vec::new());
        }
        let index = self.index(options).await?;
        Ok(index
            .search(title, MAX_HITS)
            .into_iter()
            .map(|found| {
                let names = found.names;
                let mut ids = ProviderIds::new();
                ids.set(ID_KEY, names.aid.to_string());
                SearchHit {
                    provider: PROVIDER_ID.to_owned(),
                    ids,
                    title: names.english.clone().unwrap_or_else(|| names.main.clone()),
                    original_title: names.japanese.unwrap_or(names.main),
                    year: 0,
                    overview: String::new(),
                    poster_url: None,
                    exact_id: false,
                    popularity: 0.0,
                    votes: 0,
                }
            })
            .collect())
    }

    async fn lookup(
        &self,
        _kind: MediaKind,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Option<SearchHit>, ProviderError> {
        if ids.get_u64(ID_KEY).is_none() {
            return Ok(None);
        }
        match self.anime(ids, options).await {
            Ok(anime) => Ok(Some(anime.build_hit(options, true))),
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
        let anime = self.anime(ids, options).await?;
        Ok(anime.build_item(kind == MediaKind::Movie, options))
    }

    async fn episodes(
        &self,
        show: &ProviderIds,
        ordering: &EpisodeGroup,
        options: &RequestOptions,
    ) -> Result<Vec<Item>, ProviderError> {
        if !matches!(
            ordering.ordering,
            EpisodeOrdering::Aired | EpisodeOrdering::Absolute
        ) {
            return Err(ProviderError::NotFound(DISPLAY_NAME.to_owned()));
        }
        let anime = self.anime(show, options).await?;
        let mut items = anime.episodes(options);
        if ordering.ordering == EpisodeOrdering::Absolute {
            // Specials have no absolute number.
            items.retain(|item| match &item.detail {
                cv_domain::ItemDetail::Episode(e) => e.numbers.get(ordering).is_some(),
                _ => false,
            });
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
    fn the_limit_is_one_request_every_two_seconds() {
        let limits = default_limits();
        assert_eq!(limits.max_concurrent, 1);
        assert_eq!(limits.requests, 1);
        assert_eq!(limits.period, Duration::from_secs(2));
        assert_eq!(limits.name.as_deref(), Some("AniDB"));
    }

    #[test]
    fn error_answers_become_typed_errors() {
        assert_eq!(
            answer_error("330", "anime not found"),
            ProviderError::NotFound("AniDB".to_owned())
        );
        assert!(matches!(
            answer_error("500", "banned"),
            ProviderError::RateLimited {
                retry_after_secs: 86_400,
                ..
            }
        ));
        assert!(matches!(
            answer_error("", "Banned"),
            ProviderError::RateLimited { .. }
        ));
        assert!(matches!(
            answer_error("302", "client version missing or invalid"),
            ProviderError::Auth { .. }
        ));
        assert!(matches!(
            answer_error("999", "something odd"),
            ProviderError::Request { .. }
        ));
    }

    #[test]
    fn the_client_version_defaults_to_one() {
        let schema = AnidbProvider::schema().unwrap();
        let credentials = |values: &[(&str, &str)]| {
            ResolvedCredentials::from_user_values(schema, values.iter().copied()).unwrap()
        };
        assert_eq!(
            AnidbProvider::client_of(&credentials(&[("api_key", "cvtest")])).unwrap(),
            ("cvtest".to_owned(), "1".to_owned())
        );
        assert_eq!(
            AnidbProvider::client_of(&credentials(&[
                ("api_key", "cvtest"),
                ("client_version", "3")
            ]))
            .unwrap()
            .1,
            "3"
        );
        // A version that is not a number is not sent.
        assert_eq!(
            AnidbProvider::client_of(&credentials(&[
                ("api_key", "cvtest"),
                ("client_version", "v2")
            ]))
            .unwrap()
            .1,
            "1"
        );
        assert!(matches!(
            AnidbProvider::client_of(&credentials(&[])),
            Err(ProviderError::MissingCredentials { .. })
        ));
    }
}
