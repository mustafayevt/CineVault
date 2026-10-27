//! Typed commands for collections (movie sets): the collection page, "Find collections" and the
//! undo of both. Collections are automatic, from the movies' TMDB collection; there is no way to
//! create, edit, fill or link one by hand.
//!
//! The rules live in `cv_app::collections`. Every change that can be undone returns a token that
//! `undo_collection_change` takes back. Posters of owned movies reach the web view as `cv://img`
//! keys; the poster of a movie the library lacks is the address of the provider's image. Work runs
//! on a blocking thread.

use std::sync::{Arc, OnceLock};

use cv_app::collections::{CollectionEntry, CollectionPage, CollectionService};
use cv_artwork::library_source::LibrarySources;
use cv_db::Database;
use cv_domain::ItemId;
use serde::Serialize;
use specta::Type;
use tauri::State;

use crate::boot::Boot;
use crate::edit_ipc::{EditCommandError, blocking, id_of};
use crate::img_ipc::ImageState;
use crate::library_ipc::{PosterDto, poster_dto};
use crate::nfo_ipc::NfoState;
use crate::sources_ipc::LibraryState;

/// Held in Tauri state. The service is built by the first command that needs it.
pub struct CollectionsState {
    service: OnceLock<Arc<CollectionService>>,
}

impl CollectionsState {
    pub fn from_boot(_boot: &Boot) -> Self {
        Self {
            service: OnceLock::new(),
        }
    }

    fn service(
        &self,
        library: &LibraryState,
        nfo: &NfoState,
    ) -> Result<Arc<CollectionService>, EditCommandError> {
        if let Some(service) = self.service.get() {
            return Ok(Arc::clone(service));
        }
        let db = library.database().map_err(|message| EditCommandError {
            code: "storage".to_owned(),
            message,
            field: None,
        })?;
        let nfo = nfo.service(library).map_err(|error| EditCommandError {
            code: error.code,
            message: error.message,
            field: None,
        })?;
        Ok(Arc::clone(
            self.service
                .get_or_init(|| Arc::new(CollectionService::new(db, nfo))),
        ))
    }
}

fn error_of(error: cv_app::collections::CollectionError) -> EditCommandError {
    EditCommandError {
        code: error.code().to_owned(),
        message: error.to_string(),
        field: None,
    }
}

// ---- wire types ------------------------------------------------------------------------------

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

/// The page of a collection with the posters of its movies.
fn page_dto(
    page: CollectionPage,
    db: &Database,
    sources: &LibrarySources,
    cache: Option<&cv_artwork::thumb::ThumbCache>,
) -> Result<CollectionPageDto, EditCommandError> {
    let mut wanted: Vec<ItemId> = page.entries.iter().filter_map(|e| e.id).collect();
    wanted.push(page.id);
    let keys = sources
        .posters_for(db, &wanted)
        .map_err(|error| EditCommandError {
            code: "storage".to_owned(),
            message: error.to_string(),
            field: None,
        })?;
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

// ---- commands --------------------------------------------------------------------------------

/// A collection with its movies in release order, owned and missing.
#[tauri::command]
#[specta::specta]
pub async fn get_collection(
    library: State<'_, LibraryState>,
    nfo: State<'_, NfoState>,
    images: State<'_, ImageState>,
    collections: State<'_, CollectionsState>,
    id: String,
) -> Result<CollectionPageDto, EditCommandError> {
    let id = id_of(&id)?;
    let service = collections.service(&library, &nfo)?;
    let db = library.database().map_err(|message| EditCommandError {
        code: "storage".to_owned(),
        message,
        field: None,
    })?;
    let sources = Arc::clone(images.sources());
    let cache = images.service().map(|service| service.cache().clone());
    blocking(move || {
        let page = service.page(id).map_err(error_of)?;
        page_dto(page, &db, &sources, cache.as_deref())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collection_errors_keep_their_codes() {
        let error = error_of(cv_app::collections::CollectionError::NoProvider);
        assert_eq!(error.code, "no_provider");
        let error = error_of(cv_app::collections::CollectionError::Invalid("x".into()));
        assert_eq!(
            (error.code.as_str(), error.message.as_str()),
            ("invalid", "x")
        );
    }
}
