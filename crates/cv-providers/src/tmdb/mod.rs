//! The TMDB provider (the most complete implementation of the provider traits).
//!
//! * Search follows the ID-first cascade: a TMDB ID, then an IMDb or TVDB ID through `find`,
//!   then a text search (see [`TmdbProvider::search_with_ids`]).
//! * Details cover movies, shows, seasons, episodes and collections, with external IDs,
//!   cast and crew, ratings, certifications and release dates for the configured country.
//! * Titles, plots and taglines use the preferred language and fall back to the fallback
//!   language one field at a time.
//! * Artwork comes in every size TMDB serves, trailers from the `videos` lists.
//!
//! Requests go through the shared [`HttpClient`] under the lane `tmdb`, and the API key comes
//! from the [`CredentialResolver`] on every request.

mod api;
mod client;
mod convert;
mod details;
mod language;
mod media;
mod search;

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use cv_domain::{EpisodeGroup, EpisodeOrdering, Item, MediaFileType, ProviderIds, Ratings, id_key};

use crate::catalogue;
use crate::credentials::{CredentialResolver, CredentialSchema, ResolvedCredentials};
use crate::error::ProviderError;
use crate::http::HttpClient;
use crate::model::{
    ArtworkCandidate, Capabilities, MediaKind, ProviderCollection, RequestOptions, SearchHit,
    SearchQuery,
};
use crate::traits::{ArtworkProvider, MetadataProvider, Provider, RatingProvider};

pub use convert::RATING_ID;
pub(crate) use convert::certification_token;

/// The provider key and HTTP lane id.
pub const PROVIDER_ID: &str = catalogue::key::TMDB;
/// The API server.
pub const DEFAULT_BASE_URL: &str = "https://api.themoviedb.org/3";
/// The alternate API server, for networks that block the main one.
pub const ALTERNATE_BASE_URL: &str = "https://api.tmdb.org/3";
/// Used for image URLs until (or unless) `/configuration` answers.
pub const DEFAULT_IMAGE_BASE_URL: &str = "https://image.tmdb.org/t/p/";

const DISPLAY_NAME: &str = "TMDB";

/// A season with its episodes.
#[derive(Debug, Clone, PartialEq)]
pub struct TmdbSeason {
    pub season: Item,
    pub episodes: Vec<Item>,
}

/// A collection (movie set): the set item and the movies TMDB lists in it.
pub type TmdbCollection = crate::model::ProviderCollection;

struct Inner {
    http: HttpClient,
    resolver: CredentialResolver,
    base_url: String,
    image_base: Mutex<Option<String>>,
}

/// The TMDB provider. Cloning is cheap and shares the HTTP client and the cached image base.
#[derive(Clone)]
pub struct TmdbProvider {
    inner: Arc<Inner>,
}

impl TmdbProvider {
    /// A provider for the main API server.
    pub fn new(http: HttpClient, resolver: CredentialResolver) -> Self {
        Self::with_base_url(http, resolver, DEFAULT_BASE_URL)
    }

    /// A provider for another API server: [`ALTERNATE_BASE_URL`], or a local one in tests. The
    /// URL includes the version segment (`.../3`).
    pub fn with_base_url(http: HttpClient, resolver: CredentialResolver, base_url: &str) -> Self {
        Self {
            inner: Arc::new(Inner {
                http,
                resolver,
                base_url: base_url.trim_end_matches('/').to_owned(),
                image_base: Mutex::new(None),
            }),
        }
    }

    fn schema() -> Option<&'static CredentialSchema> {
        catalogue::CREDENTIAL_SCHEMAS
            .iter()
            .find(|s| s.provider == PROVIDER_ID)
    }

    // --- details beyond the traits -------------------------------------------------------

    /// A season and its episodes in aired order.
    pub async fn season_details(
        &self,
        show: &ProviderIds,
        season: u32,
        options: &RequestOptions,
    ) -> Result<TmdbSeason, ProviderError> {
        let id = self.resolve_id(MediaKind::Show, show, options).await?;
        self.fetch_season(id, season, options).await
    }

    /// One episode by its aired season and episode number.
    pub async fn episode_details(
        &self,
        show: &ProviderIds,
        season: u32,
        episode: u32,
        options: &RequestOptions,
    ) -> Result<Item, ProviderError> {
        let id = self.resolve_id(MediaKind::Show, show, options).await?;
        self.fetch_episode(id, season, episode, options).await
    }

    /// Collections matching a title. Hits carry the collection ID as `tmdbSet`.
    pub async fn search_collections(
        &self,
        title: &str,
        options: &RequestOptions,
    ) -> Result<Vec<SearchHit>, ProviderError> {
        self.collection_search(title, options).await
    }

    /// A collection by its `tmdbSet` ID.
    pub async fn collection(
        &self,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<TmdbCollection, ProviderError> {
        let id = ids
            .get_u64(id_key::TMDB_SET)
            .ok_or_else(|| ProviderError::NotFound(DISPLAY_NAME.to_owned()))?;
        self.fetch_collection(id, options).await
    }

    /// Poster and fanart candidates of a collection.
    pub async fn collection_artwork(
        &self,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Vec<ArtworkCandidate>, ProviderError> {
        let id = ids
            .get_u64(id_key::TMDB_SET)
            .ok_or_else(|| ProviderError::NotFound(DISPLAY_NAME.to_owned()))?;
        self.images_for(
            &format!("collection/{id}/images"),
            options,
            &[
                media::Slot::Posters(MediaFileType::Poster),
                media::Slot::Backdrops(MediaFileType::Fanart),
            ],
        )
        .await
    }

    /// Season poster candidates.
    pub async fn season_artwork(
        &self,
        show: &ProviderIds,
        season: u32,
        options: &RequestOptions,
    ) -> Result<Vec<ArtworkCandidate>, ProviderError> {
        let id = self.resolve_id(MediaKind::Show, show, options).await?;
        self.images_for(
            &format!("tv/{id}/season/{season}/images"),
            options,
            &[media::Slot::Posters(MediaFileType::SeasonPoster)],
        )
        .await
    }

    /// Episode still (thumb) candidates.
    pub async fn episode_artwork(
        &self,
        show: &ProviderIds,
        season: u32,
        episode: u32,
        options: &RequestOptions,
    ) -> Result<Vec<ArtworkCandidate>, ProviderError> {
        let id = self.resolve_id(MediaKind::Show, show, options).await?;
        self.images_for(
            &format!("tv/{id}/season/{season}/episode/{episode}/images"),
            options,
            &[media::Slot::Stills(MediaFileType::Thumb)],
        )
        .await
    }
}

#[async_trait]
impl Provider for TmdbProvider {
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
impl MetadataProvider for TmdbProvider {
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
            lookup_ids: vec![id_key::TMDB, id_key::IMDB, id_key::TVDB],
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
        match kind {
            MediaKind::Movie => self.fetch_movie(id, options).await,
            MediaKind::Show => self.fetch_show(id, options).await,
        }
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

    async fn person_ids(
        &self,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<ProviderIds, ProviderError> {
        let mut found = ProviderIds::new();
        let Some(person) = ids
            .get(id_key::TMDB)
            .map(str::trim)
            .filter(|id| !id.is_empty())
        else {
            return Ok(found);
        };
        let external: api::PersonExternalIds = self
            .get(
                &format!("person/{person}/external_ids"),
                Vec::new(),
                options,
            )
            .await?;
        if let Some(imdb) = external.imdb_id.as_deref().map(str::trim)
            && imdb.starts_with("nm")
        {
            found.set(id_key::IMDB, imdb);
        }
        Ok(found)
    }

    fn offers_collections(&self) -> bool {
        true
    }

    async fn search_collections(
        &self,
        title: &str,
        options: &RequestOptions,
    ) -> Result<Vec<SearchHit>, ProviderError> {
        self.collection_search(title, options).await
    }

    async fn collection(
        &self,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<ProviderCollection, ProviderError> {
        TmdbProvider::collection(self, ids, options).await
    }
}

#[async_trait]
impl ArtworkProvider for TmdbProvider {
    fn offers(&self, _kind: MediaKind, slot: cv_domain::MediaFileType) -> bool {
        matches!(
            slot,
            cv_domain::MediaFileType::Poster
                | cv_domain::MediaFileType::Fanart
                | cv_domain::MediaFileType::Clearlogo
        )
    }

    async fn artwork(
        &self,
        kind: MediaKind,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Vec<ArtworkCandidate>, ProviderError> {
        let id = self.resolve_id(kind, ids, options).await?;
        match kind {
            MediaKind::Movie => {
                self.images_for(
                    &format!("movie/{id}/images"),
                    options,
                    &[
                        media::Slot::Posters(MediaFileType::Poster),
                        media::Slot::Backdrops(MediaFileType::Fanart),
                        media::Slot::Logos(MediaFileType::Clearlogo),
                    ],
                )
                .await
            }
            MediaKind::Show => {
                self.images_for(
                    &format!("tv/{id}/images"),
                    options,
                    &[
                        media::Slot::Posters(MediaFileType::Poster),
                        media::Slot::Backdrops(MediaFileType::Fanart),
                        media::Slot::Logos(MediaFileType::Clearlogo),
                    ],
                )
                .await
            }
        }
    }

    fn offers_season(&self, slot: cv_domain::MediaFileType) -> bool {
        slot == cv_domain::MediaFileType::SeasonPoster
    }

    fn offers_collection(&self, slot: cv_domain::MediaFileType) -> bool {
        matches!(
            slot,
            cv_domain::MediaFileType::Poster | cv_domain::MediaFileType::Fanart
        )
    }

    async fn collection_artwork(
        &self,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Vec<ArtworkCandidate>, ProviderError> {
        TmdbProvider::collection_artwork(self, ids, options).await
    }

    async fn season_artwork(
        &self,
        show: &ProviderIds,
        season: u32,
        options: &RequestOptions,
    ) -> Result<Vec<ArtworkCandidate>, ProviderError> {
        TmdbProvider::season_artwork(self, show, season, options).await
    }
}

#[async_trait]
impl RatingProvider for TmdbProvider {
    async fn ratings(
        &self,
        kind: MediaKind,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Ratings, ProviderError> {
        let id = self.resolve_id(kind, ids, options).await?;
        self.fetch_ratings(kind, id, options).await
    }
}
