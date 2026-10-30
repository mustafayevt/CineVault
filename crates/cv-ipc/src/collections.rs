//! The collection (movie set) page: one read of a collection with its owned and missing movies.
//!
//! The rules live in `cv_app::collections`; this module converts the page to the DTOs the UI sees
//! and gives every owned movie its `cv://img` poster key. The collection service itself is built
//! by the surface that holds the app's state.

use std::sync::Arc;

use cv_app::collections::{CollectionEntry, CollectionError, CollectionPage, CollectionService};
use cv_artwork::library_source::LibrarySources;
use cv_artwork::thumb::ThumbCache;
use cv_db::Database;
use cv_domain::ItemId;
use serde::Serialize;
use specta::Type;

use crate::error::CommandError;
use crate::library::{PosterDto, poster_dto};

/// One movie on a collection page, owned or missing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct CollectionEntryDto {
    /// The library item; `None` for a movie the library lacks (shown ghosted).
    pub id: Option<String>,
    pub title: String,
    /// `0` when unknown.
    pub year: u16,
    /// `yyyy-MM-dd`.
    pub release_date: Option<String>,
    pub watched: bool,
    /// The poster file of an owned movie, for `cv://img`.
    pub poster: Option<PosterDto>,
    /// The provider's poster address for a missing movie.
    pub poster_url: Option<String>,
    pub tmdb_id: Option<String>,
}

/// A collection with its movies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct CollectionPageDto {
    pub id: String,
    pub title: String,
    pub sort_title: String,
    pub plot: String,
    pub tmdb_id: Option<String>,
    /// The collection's own poster, else the poster of its first movie.
    pub poster: Option<PosterDto>,
    /// Owned and missing movies together, in release order.
    pub entries: Vec<CollectionEntryDto>,
    pub owned: u32,
    /// Owned movies plus released movies the library lacks.
    pub total: u32,
}

/// A collection failure in the command shape, keeping the service's code.
pub fn error_of(error: CollectionError) -> CommandError {
    CommandError::new(error.code(), error.to_string())
}

/// The page of a collection with the posters of its movies.
pub fn page_dto(
    page: CollectionPage,
    db: &Database,
    sources: &LibrarySources,
    cache: Option<&ThumbCache>,
) -> Result<CollectionPageDto, CommandError> {
    let mut wanted: Vec<ItemId> = page.entries.iter().filter_map(|e| e.id).collect();
    wanted.push(page.id);
    let keys = sources
        .posters_for(db, &wanted)
        .map_err(|error| CommandError::new("storage", error.to_string()))?;
    let entry = |entry: &CollectionEntry| CollectionEntryDto {
        id: entry.id.map(|id| id.0.to_string()),
        title: entry.title.clone(),
        year: entry.year,
        release_date: entry.release_date.map(|d| d.to_string()),
        watched: entry.watched,
        poster: entry
            .id
            .and_then(|id| keys.get(&id))
            .map(|key| poster_dto(cache, key)),
        poster_url: Some(entry.poster_url.clone()).filter(|url| !url.is_empty()),
        tmdb_id: entry.tmdb_id.clone(),
    };
    let entries: Vec<CollectionEntryDto> = page.entries.iter().map(entry).collect();
    let poster = keys
        .get(&page.id)
        .map(|key| poster_dto(cache, key))
        .or_else(|| entries.iter().find_map(|e| e.poster.clone()));
    Ok(CollectionPageDto {
        id: page.id.0.to_string(),
        title: page.title,
        sort_title: page.sort_title,
        plot: page.plot,
        tmdb_id: page.tmdb_id,
        poster,
        entries,
        owned: page.owned,
        total: page.total,
    })
}

/// A collection with its movies in release order, owned and missing. Blocking work runs off the
/// async workers.
pub async fn get_collection(
    service: Arc<CollectionService>,
    db: Arc<Database>,
    sources: Arc<LibrarySources>,
    cache: Option<Arc<ThumbCache>>,
    id: ItemId,
) -> Result<CollectionPageDto, CommandError> {
    cv_jobs::client::spawn_blocking(move || {
        let page = service.page(id).map_err(error_of)?;
        page_dto(page, &db, &sources, cache.as_deref())
    })
    .await
    .map_err(|error| {
        CommandError::new("storage", format!("the edit stopped unexpectedly: {error}"))
    })?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collection_errors_keep_their_codes() {
        let error = error_of(CollectionError::NoProvider);
        assert_eq!(error.code, "no_provider");
        let error = error_of(CollectionError::Invalid("x".into()));
        assert_eq!(
            (error.code.as_str(), error.message.as_str()),
            ("invalid", "x")
        );
    }
}
