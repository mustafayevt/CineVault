//! The Fanart.tv artwork provider.
//!
//! Fanart.tv offers community artwork only: posters, backgrounds, HD and standard clearlogos and
//! clearart, banners, thumbs, discs and, for shows, character art and season images. It has no
//! metadata, so this is an [`ArtworkProvider`] and not a [`crate::MetadataProvider`].
//!
//! * **Lookup.** Movies are asked for by IMDb ID first, then by TMDB ID; shows by TVDB ID.
//!   A title Fanart.tv does not know (HTTP 404) or one without a usable ID offers no artwork; it
//!   is not an error.
//! * **Credential.** The API key (`api_key`) is sent in the `api-key` header. It is the user's own
//!   key; a rejected or rate-limited key links to the provider's card in Settings.
//! * **Types.** `lang` `00` (or none) means no text: such a poster is key art, not a poster.
//!   `likes` is the score. The answers carry no sizes, so every image gets the size Fanart.tv asks
//!   uploaders to deliver for its type.
//! * **Seasons.** [`FanartProvider::season_artwork`] and [`FanartProvider::all_seasons_artwork`]
//!   return the season poster, thumb, banner and fanart candidates.
//!
//! Requests go through the shared [`HttpClient`] under the lane `fanart`.

mod convert;

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use cv_domain::{ProviderIds, id_key};
use serde_json::Value;

use crate::catalogue;
use crate::credentials::{CredentialResolver, CredentialSchema, ResolvedCredentials};
use crate::error::ProviderError;
use crate::http::{HttpClient, HttpRequest, ProviderLimits};
use crate::model::{ArtworkCandidate, MediaKind, RequestOptions};
use crate::shared::imdb_id_of;
use crate::traits::{ArtworkProvider, Provider};

use convert::SeasonFilter;

/// The provider key and HTTP lane id.
pub const PROVIDER_ID: &str = catalogue::key::FANART;
/// The API server, including the version segment.
pub const DEFAULT_BASE_URL: &str = "https://webservice.fanart.tv/v3.2";

const DISPLAY_NAME: &str = "Fanart.tv";

/// How fast requests are sent: two at a time and no more than five per second.
pub fn default_limits() -> ProviderLimits {
    ProviderLimits::new(2, 5, Duration::from_secs(1)).named(DISPLAY_NAME)
}

struct Inner {
    http: HttpClient,
    resolver: CredentialResolver,
    base_url: String,
}

/// The Fanart.tv provider. Cloning is cheap.
#[derive(Clone)]
pub struct FanartProvider {
    inner: Arc<Inner>,
}

impl FanartProvider {
    /// A provider for the real API server. It sets the polite [`default_limits`] on the `fanart`
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

    /// The endpoints that can answer for a title, best first: movies by IMDb then TMDB ID, shows
    /// by TVDB ID.
    fn endpoints(kind: MediaKind, ids: &ProviderIds) -> Vec<String> {
        match kind {
            MediaKind::Movie => {
                let mut out = Vec::new();
                if let Some(imdb) = imdb_id_of(ids) {
                    out.push(format!("movies/{imdb}"));
                }
                if let Some(tmdb) = ids.get_u64(id_key::TMDB) {
                    out.push(format!("movies/{tmdb}"));
                }
                out
            }
            MediaKind::Show => ids
                .get_u64(id_key::TVDB)
                .map(|tvdb| vec![format!("tv/{tvdb}")])
                .unwrap_or_default(),
        }
    }

    /// The answer for a title, or `None` when Fanart.tv has nothing under any of its IDs.
    async fn answer(
        &self,
        kind: MediaKind,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Option<Value>, ProviderError> {
        let credentials = self.inner.resolver.require(PROVIDER_ID)?;
        for endpoint in Self::endpoints(kind, ids) {
            match self
                .send(&credentials, &endpoint, options.bypass_cache, true)
                .await
            {
                Ok(value) if value.is_object() => return Ok(Some(value)),
                Ok(_) => return Err(request_error("the response was not in the expected format")),
                Err(ProviderError::NotFound(_)) => {}
                Err(other) => return Err(other),
            }
        }
        Ok(None)
    }

    /// Like [`Self::answer`], then converts it with `filter`.
    async fn candidates(
        &self,
        kind: MediaKind,
        ids: &ProviderIds,
        filter: SeasonFilter,
        options: &RequestOptions,
    ) -> Result<Vec<ArtworkCandidate>, ProviderError> {
        Ok(self
            .answer(kind, ids, options)
            .await?
            .map(|answer| convert::candidates(&answer, kind, filter, options))
            .unwrap_or_default())
    }

    /// The poster, thumb, banner and fanart candidates of one season of a show (`0` is the
    /// specials), by the show's TVDB ID.
    ///
    /// # Errors
    /// As [`ArtworkProvider::artwork`].
    pub async fn season_artwork(
        &self,
        show: &ProviderIds,
        season: u32,
        options: &RequestOptions,
    ) -> Result<Vec<ArtworkCandidate>, ProviderError> {
        self.candidates(MediaKind::Show, show, SeasonFilter::Number(season), options)
            .await
    }

    /// The season images Fanart.tv marks as belonging to every season of a show.
    ///
    /// # Errors
    /// As [`ArtworkProvider::artwork`].
    pub async fn all_seasons_artwork(
        &self,
        show: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Vec<ArtworkCandidate>, ProviderError> {
        self.candidates(MediaKind::Show, show, SeasonFilter::All, options)
            .await
    }

    async fn send(
        &self,
        credentials: &ResolvedCredentials,
        endpoint: &str,
        bypass_cache: bool,
        cached: bool,
    ) -> Result<Value, ProviderError> {
        self.send_with(credentials, endpoint, bypass_cache, cached)
            .await
            .map_err(|e| e.with_key_hint(credentials, PROVIDER_ID))
    }

    async fn send_with(
        &self,
        credentials: &ResolvedCredentials,
        endpoint: &str,
        bypass_cache: bool,
        cached: bool,
    ) -> Result<Value, ProviderError> {
        let key = credentials
            .get(catalogue::API_KEY)
            .map(|v| v.expose().trim().to_owned())
            .filter(|k| !k.is_empty())
            .ok_or_else(missing_key)?;
        let url = format!("{}/{endpoint}", self.inner.base_url);
        let mut request = HttpRequest::get(PROVIDER_ID, &url)
            .credential_header("api-key", &key)
            .bypass_cache(bypass_cache);
        if !cached {
            request = request.uncached();
        }
        let response = self.inner.http.send(request).await?;
        if response.is_success() {
            return response
                .json::<Value>()
                .ok()
                .ok_or_else(|| request_error("the response was not in the expected format"));
        }
        let message = response
            .json::<Value>()
            .ok()
            .and_then(|body| {
                ["error message", "error", "message"]
                    .iter()
                    .find_map(|k| body.get(*k).and_then(Value::as_str).map(str::to_owned))
            })
            .map(|m| m.trim().to_owned())
            .filter(|m| !m.is_empty());
        match response.status() {
            401 | 403 => Err(ProviderError::Auth {
                provider: DISPLAY_NAME.to_owned(),
                message: message.unwrap_or_else(|| "Invalid API key".to_owned()),
            }),
            404 => Err(ProviderError::NotFound(DISPLAY_NAME.to_owned())),
            status => Err(request_error(match message {
                Some(m) => format!("HTTP {status}: {m}"),
                None => format!("HTTP {status}"),
            })),
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
    let (url, fields) = FanartProvider::schema()
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
impl Provider for FanartProvider {
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
        // The list of the latest movies is one cheap authenticated request; it is never served
        // from the cache, so a changed key is judged on its own.
        self.send(credentials, "movies/latest", true, false)
            .await
            .map(|_| ())
    }
}

#[async_trait]
impl ArtworkProvider for FanartProvider {
    fn offers(&self, kind: MediaKind, slot: cv_domain::MediaFileType) -> bool {
        use cv_domain::MediaFileType::{
            Banner, Characterart, Clearart, Clearlogo, Disc, Fanart, Poster, Thumb,
        };
        match kind {
            MediaKind::Movie => matches!(
                slot,
                Poster | Fanart | Clearlogo | Clearart | Disc | Banner | Thumb
            ),
            MediaKind::Show => matches!(
                slot,
                Poster | Fanart | Clearlogo | Clearart | Banner | Thumb | Characterart
            ),
        }
    }

    async fn artwork(
        &self,
        kind: MediaKind,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Vec<ArtworkCandidate>, ProviderError> {
        self.candidates(kind, ids, SeasonFilter::Title, options)
            .await
    }

    fn offers_season(&self, slot: cv_domain::MediaFileType) -> bool {
        matches!(
            slot,
            cv_domain::MediaFileType::SeasonPoster
                | cv_domain::MediaFileType::SeasonFanart
                | cv_domain::MediaFileType::SeasonBanner
                | cv_domain::MediaFileType::SeasonThumb
        )
    }

    async fn season_artwork(
        &self,
        show: &ProviderIds,
        season: u32,
        options: &RequestOptions,
    ) -> Result<Vec<ArtworkCandidate>, ProviderError> {
        FanartProvider::season_artwork(self, show, season, options).await
    }
}
