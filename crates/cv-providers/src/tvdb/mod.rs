//! The TheTVDB v4 provider.
//!
//! * Credentials are the API key and, optionally, the subscriber PIN of the user's own key. The
//!   provider signs in with `POST login` and sends the token it gets as a bearer token; it
//!   signs in again when the credentials change or the token expires.
//! * Search follows the ID-first cascade: a TVDB ID, then an IMDb ID through `search/remoteid`,
//!   then a text search (see [`TvdbProvider::search_with_ids`]).
//! * Shows and movies come from the `extended` records, with their translations (preferred
//!   language, then the fallback language, then English), remote IDs, cast, genres,
//!   certifications and trailers.
//! * Every episode ordering TheTVDB has is available through
//!   [`MetadataProvider::episodes`]: aired (`official` or `default`), DVD, absolute and the
//!   alternate orders (`alternate`, `regional`, `altdvd`, `alttwo`). Each episode carries its
//!   aired number and its number in the requested ordering.
//! * Artwork is read from the record's artworks with the type names from `artwork/types`.
//!
//! Requests go through the shared [`HttpClient`] under the lane `tvdb`.

mod api;
mod client;
mod convert;
mod details;
mod language;
mod search;

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use cv_domain::{EpisodeGroup, EpisodeOrdering, Item, ProviderIds, id_key};

use crate::catalogue;
use crate::credentials::{CredentialResolver, CredentialSchema, ResolvedCredentials};
use crate::error::ProviderError;
use crate::http::HttpClient;
use crate::model::{
    ArtworkCandidate, Capabilities, MediaKind, RequestOptions, SearchHit, SearchQuery,
};
use crate::traits::{ArtworkProvider, MetadataProvider, Provider};

/// The provider key and HTTP lane id.
pub const PROVIDER_ID: &str = catalogue::key::TVDB;
/// The API server, including the version segment.
pub const DEFAULT_BASE_URL: &str = "https://api4.thetvdb.com/v4";

const DISPLAY_NAME: &str = "TheTVDB";

/// A season with its episodes.
#[derive(Debug, Clone, PartialEq)]
pub struct TvdbSeason {
    pub season: Item,
    pub episodes: Vec<Item>,
}

/// The sign-in a token belongs to. Kept in memory only and never printed.
struct Session {
    /// The API key and PIN the token was issued for.
    credentials: String,
    token: String,
}

struct Inner {
    http: HttpClient,
    resolver: CredentialResolver,
    base_url: String,
    session: tokio::sync::Mutex<Option<Session>>,
    artwork_types: Mutex<Option<Arc<Vec<api::ArtworkType>>>>,
}

/// The TheTVDB provider. Cloning is cheap and shares the HTTP client, the token and the
/// artwork types.
#[derive(Clone)]
pub struct TvdbProvider {
    inner: Arc<Inner>,
}

impl TvdbProvider {
    /// A provider for the main API server.
    pub fn new(http: HttpClient, resolver: CredentialResolver) -> Self {
        Self::with_base_url(http, resolver, DEFAULT_BASE_URL)
    }

    /// A provider for another API server, such as a local one in tests. The URL includes the
    /// version segment (`.../v4`).
    pub fn with_base_url(http: HttpClient, resolver: CredentialResolver, base_url: &str) -> Self {
        Self {
            inner: Arc::new(Inner {
                http,
                resolver,
                base_url: base_url.trim_end_matches('/').to_owned(),
                session: tokio::sync::Mutex::new(None),
                artwork_types: Mutex::new(None),
            }),
        }
    }

    fn schema() -> Option<&'static CredentialSchema> {
        catalogue::CREDENTIAL_SCHEMAS
            .iter()
            .find(|s| s.provider == PROVIDER_ID)
    }

    // --- details beyond the traits -------------------------------------------------------

    /// An aired season of a show with its episodes: the season's name, plot and artwork, and
    /// its episodes with their aired numbers.
    pub async fn season_details(
        &self,
        show: &ProviderIds,
        season: u32,
        options: &RequestOptions,
    ) -> Result<TvdbSeason, ProviderError> {
        let id = self.resolve_id(MediaKind::Show, show, options).await?;
        self.fetch_season(id, season, options).await
    }

    /// Season poster, fanart and banner candidates of an aired season.
    pub async fn season_artwork(
        &self,
        show: &ProviderIds,
        season: u32,
        options: &RequestOptions,
    ) -> Result<Vec<ArtworkCandidate>, ProviderError> {
        let id = self.resolve_id(MediaKind::Show, show, options).await?;
        self.fetch_season_artwork(id, season, options).await
    }
}

#[async_trait]
impl Provider for TvdbProvider {
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
        self.verify(credentials).await
    }
}

#[async_trait]
impl MetadataProvider for TvdbProvider {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            movies: true,
            shows: true,
            orderings: vec![
                EpisodeOrdering::Aired,
                EpisodeOrdering::Dvd,
                EpisodeOrdering::Absolute,
                EpisodeOrdering::Alternate,
            ],
            lookup_ids: vec![id_key::TVDB, id_key::IMDB],
            best_effort: false,
        }
    }

    async fn search(
        &self,
        query: &SearchQuery,
        options: &RequestOptions,
    ) -> Result<Vec<SearchHit>, ProviderError> {
        self.search_with_ids(query, &ProviderIds::new(), options)
            .await
    }

    async fn lookup(
        &self,
        kind: MediaKind,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Option<SearchHit>, ProviderError> {
        self.lookup_by_ids(kind, ids, options).await
    }

    async fn details(
        &self,
        kind: MediaKind,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Item, ProviderError> {
        let id = self.resolve_id(kind, ids, options).await?;
        self.fetch_details(kind, id, options).await
    }

    async fn episodes(
        &self,
        show: &ProviderIds,
        ordering: &EpisodeGroup,
        options: &RequestOptions,
    ) -> Result<Vec<Item>, ProviderError> {
        let id = self.resolve_id(MediaKind::Show, show, options).await?;
        self.fetch_episode_list(id, ordering, options).await
    }
}

#[async_trait]
impl ArtworkProvider for TvdbProvider {
    fn offers(&self, _kind: MediaKind, slot: cv_domain::MediaFileType) -> bool {
        matches!(
            slot,
            cv_domain::MediaFileType::Poster
                | cv_domain::MediaFileType::Fanart
                | cv_domain::MediaFileType::Banner
                | cv_domain::MediaFileType::Squareart
                | cv_domain::MediaFileType::Clearart
                | cv_domain::MediaFileType::Clearlogo
                | cv_domain::MediaFileType::Thumb
        )
    }

    async fn artwork(
        &self,
        kind: MediaKind,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Vec<ArtworkCandidate>, ProviderError> {
        let id = self.resolve_id(kind, ids, options).await?;
        self.fetch_artwork(kind, id, options).await
    }

    fn offers_season(&self, slot: cv_domain::MediaFileType) -> bool {
        matches!(
            slot,
            cv_domain::MediaFileType::SeasonPoster
                | cv_domain::MediaFileType::SeasonFanart
                | cv_domain::MediaFileType::SeasonBanner
        )
    }

    async fn season_artwork(
        &self,
        show: &ProviderIds,
        season: u32,
        options: &RequestOptions,
    ) -> Result<Vec<ArtworkCandidate>, ProviderError> {
        TvdbProvider::season_artwork(self, show, season, options).await
    }
}
