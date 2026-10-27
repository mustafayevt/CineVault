//! The provider traits.
//!
//! A provider takes its credentials from a [`crate::CredentialResolver`] at request time, so a
//! key the user enters or resets applies to the next call with no restart.

use async_trait::async_trait;
use cv_domain::{EpisodeGroup, Item, MediaFileType, ProviderIds, Ratings};

use crate::credentials::{CredentialSchema, ResolvedCredentials};
use crate::error::ProviderError;
use crate::model::{
    ArtworkCandidate, Capabilities, MediaKind, ProviderCollection, RequestOptions, SearchHit,
    SearchQuery,
};

/// What every provider has in common.
#[async_trait]
pub trait Provider: Send + Sync {
    /// Stable key, for example `tmdb`. Matches the credential schema's provider key.
    fn id(&self) -> &'static str;

    /// Name as the provider writes it.
    fn name(&self) -> &'static str;

    /// The credentials it needs. `None` for providers that work without any.
    fn credentials(&self) -> Option<&'static CredentialSchema> {
        None
    }

    /// Makes one cheap authenticated request with `credentials` and reports whether the provider
    /// accepts them. The Settings Test button calls this, with the value the user just typed or
    /// with the ones in use.
    async fn check_credentials(
        &self,
        credentials: &ResolvedCredentials,
    ) -> Result<(), ProviderError> {
        let _ = credentials;
        Err(ProviderError::Unsupported {
            provider: self.name().to_owned(),
            operation: "testing credentials",
        })
    }
}

/// A source of titles and their details (movies, shows, seasons, episodes).
#[async_trait]
pub trait MetadataProvider: Provider {
    fn capabilities(&self) -> Capabilities;

    /// The languages its plots, titles and genres come in (two-letter codes). Empty, the
    /// default, for a provider that answers in the requested language.
    fn languages(&self) -> &'static [&'static str] {
        &[]
    }

    /// Whether it answers in `language` (`en`, `de-DE`): it follows the request, or the language
    /// is one of its own.
    fn serves_language(&self, language: &str) -> bool {
        let own = self.languages();
        let wanted = language.split(['-', '_']).next().unwrap_or_default().trim();
        own.is_empty() || wanted.is_empty() || own.iter().any(|l| l.eq_ignore_ascii_case(wanted))
    }

    /// Search by title and optional year.
    async fn search(
        &self,
        query: &SearchQuery,
        options: &RequestOptions,
    ) -> Result<Vec<SearchHit>, ProviderError>;

    /// Direct lookup by any ID the provider understands (see [`Capabilities::lookup_ids`]).
    /// `Ok(None)` means the provider has no title with that ID.
    async fn lookup(
        &self,
        kind: MediaKind,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Option<SearchHit>, ProviderError>;

    /// Full details for a title. The item is not stored yet (`id` is `None`).
    async fn details(
        &self,
        kind: MediaKind,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Item, ProviderError>;

    /// The full episode list of a show in one ordering. The default says the provider has none,
    /// which is right for movie-only providers.
    async fn episodes(
        &self,
        show: &ProviderIds,
        ordering: &EpisodeGroup,
        options: &RequestOptions,
    ) -> Result<Vec<Item>, ProviderError> {
        let _ = (show, ordering, options);
        Err(ProviderError::Unsupported {
            provider: self.name().to_owned(),
            operation: "episode lists",
        })
    }

    /// The other IDs the provider knows for a person, from the person's own ID in `ids` (TMDB:
    /// the IMDb `nm` ID). The default knows none.
    async fn person_ids(
        &self,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<ProviderIds, ProviderError> {
        let _ = (ids, options);
        Ok(ProviderIds::new())
    }

    /// Whether the provider knows collections (movie sets). The default is no.
    fn offers_collections(&self) -> bool {
        false
    }

    /// Collections matching a title. Hits carry the collection ID as `tmdbSet`. The default says
    /// the provider has none.
    async fn search_collections(
        &self,
        title: &str,
        options: &RequestOptions,
    ) -> Result<Vec<SearchHit>, ProviderError> {
        let _ = (title, options);
        Err(ProviderError::Unsupported {
            provider: self.name().to_owned(),
            operation: "collections",
        })
    }

    /// A collection by its `tmdbSet` ID, with every movie it lists.
    async fn collection(
        &self,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<ProviderCollection, ProviderError> {
        let _ = (ids, options);
        Err(ProviderError::Unsupported {
            provider: self.name().to_owned(),
            operation: "collections",
        })
    }
}

/// A source of posters, fanart and other images.
#[async_trait]
pub trait ArtworkProvider: Provider {
    /// Whether the provider has images of `slot` (an artwork type) for a kind of title. The
    /// artwork picker leaves a provider that does not offer the type out of its search instead
    /// of asking it for nothing. The default says yes, which is right for a provider whose
    /// types are not known up front.
    fn offers(&self, kind: MediaKind, slot: MediaFileType) -> bool {
        let _ = (kind, slot);
        true
    }

    async fn artwork(
        &self,
        kind: MediaKind,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Vec<ArtworkCandidate>, ProviderError>;

    /// Whether [`Self::season_artwork`] can offer images of the season type `slot`
    /// (`SeasonPoster`, `SeasonFanart`, `SeasonBanner` or `SeasonThumb`). The picker asks only
    /// the providers that do. The default says no, as the default `season_artwork` has none.
    fn offers_season(&self, slot: MediaFileType) -> bool {
        let _ = slot;
        false
    }

    /// Whether [`Self::collection_artwork`] can offer images of the type `slot` (`Poster` or
    /// `Fanart`) of a movie collection. The picker asks only the providers that do. The default
    /// says no.
    fn offers_collection(&self, slot: MediaFileType) -> bool {
        let _ = slot;
        false
    }

    /// The posters and fanart of a movie collection. `ids` holds the collection's ID under the
    /// `tmdbSet` key. The default says the provider has none.
    async fn collection_artwork(
        &self,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Vec<ArtworkCandidate>, ProviderError> {
        let _ = (ids, options);
        Ok(Vec::new())
    }

    /// The images of one season of a show (`0` is the specials), as season posters, fanart,
    /// banners and thumbs. `show` holds the show's IDs. The default says the provider has none,
    /// which is right for movie-only providers and for providers without season images.
    async fn season_artwork(
        &self,
        show: &ProviderIds,
        season: u32,
        options: &RequestOptions,
    ) -> Result<Vec<ArtworkCandidate>, ProviderError> {
        let _ = (show, season, options);
        Ok(Vec::new())
    }
}

/// A source of ratings.
#[async_trait]
pub trait RatingProvider: Provider {
    async fn ratings(
        &self,
        kind: MediaKind,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> Result<Ratings, ProviderError>;
}
