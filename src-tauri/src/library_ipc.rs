//! The Tauri surface of the library read commands: windows of cards, table rows and counts.
//!
//! The behaviour and the DTOs live in [`cv_ipc::library`]. Each function here opens a reader on
//! the app's state and calls the shared implementation, so the desktop app and the browser server
//! answer a library page the same way and a fix is written once.

use std::sync::Arc;

use tauri::State;

use crate::img_ipc::ImageState;
use crate::library_facets::{FacetValueDto, FilterDefDto};
use crate::sources_ipc::LibraryState;

pub use cv_ipc::library::*;

/// Opens a reader on the shared library, image index and thumbnail cache.
fn reader(
    library: &State<'_, LibraryState>,
    images: &State<'_, ImageState>,
) -> Result<LibraryReader, LibraryCommandError> {
    let db = library.database().map_err(storage)?;
    let cache = images.service().map(|service| service.cache().clone());
    Ok(LibraryReader::from_parts(
        db,
        Arc::clone(images.sources()),
        cache,
        library.lifecycle().cloned(),
    ))
}

/// Rows `offset..offset + limit` of a listing, with the total.
#[tauri::command]
#[specta::specta]
pub async fn list_library_window(
    library: State<'_, LibraryState>,
    images: State<'_, ImageState>,
    query: LibraryQueryDto,
    offset: u32,
    limit: u32,
) -> Result<LibraryWindowDto, LibraryCommandError> {
    let reader = reader(&library, &images)?;
    cv_ipc::library::list_library_window(reader, query, offset, limit).await
}

/// Rows `offset..offset + limit` of a listing with every table column, and the total.
#[tauri::command]
#[specta::specta]
pub async fn list_library_rows(
    library: State<'_, LibraryState>,
    images: State<'_, ImageState>,
    query: LibraryQueryDto,
    offset: u32,
    limit: u32,
) -> Result<LibraryRowsWindowDto, LibraryCommandError> {
    let reader = reader(&library, &images)?;
    cv_ipc::library::list_library_rows(reader, query, offset, limit).await
}

/// The filters of the full catalogue that exist for a library, in picker order.
#[tauri::command]
#[specta::specta]
pub fn library_filter_catalogue(kind: LibraryKindDto) -> Vec<FilterDefDto> {
    cv_ipc::library::library_filter_catalogue(kind)
}

/// The values of one list filter with their item counts, narrowed by the query's other chips.
#[tauri::command]
#[specta::specta]
pub async fn library_facet_values(
    library: State<'_, LibraryState>,
    images: State<'_, ImageState>,
    query: LibraryQueryDto,
    facet: String,
) -> Result<Vec<FacetValueDto>, LibraryCommandError> {
    let reader = reader(&library, &images)?;
    cv_ipc::library::library_facet_values(reader, query, facet).await
}

/// The matching titles grouped by library folder label, with their counts.
#[tauri::command]
#[specta::specta]
pub async fn library_source_groups(
    library: State<'_, LibraryState>,
    images: State<'_, ImageState>,
    query: LibraryQueryDto,
) -> Result<Vec<SourceGroupDto>, LibraryCommandError> {
    let reader = reader(&library, &images)?;
    cv_ipc::library::library_source_groups(reader, query).await
}

/// The actors and directors whose name contains `term` (all of them for an empty term), most
/// credited first, at most `limit`. The command palette's `@` search.
#[tauri::command]
#[specta::specta]
pub async fn search_library_people(
    library: State<'_, LibraryState>,
    images: State<'_, ImageState>,
    term: String,
    limit: u32,
) -> Result<Vec<PersonHitDto>, LibraryCommandError> {
    let reader = reader(&library, &images)?;
    cv_ipc::library::search_library_people(reader, term, limit).await
}

/// The counts the sidebar shows.
/// The size of the movie or the TV library: every file once, online titles only, plus what is
/// offline. Home shows these two numbers.
#[tauri::command]
#[specta::specta]
pub async fn library_size_total(
    library: State<'_, LibraryState>,
    images: State<'_, ImageState>,
    kind: LibraryKindDto,
) -> Result<LibrarySizeTotalDto, LibraryCommandError> {
    let reader = reader(&library, &images)?;
    cv_ipc::library::library_size_total(reader, kind).await
}

#[tauri::command]
#[specta::specta]
pub async fn library_counts(
    library: State<'_, LibraryState>,
    images: State<'_, ImageState>,
) -> Result<LibraryCountsDto, LibraryCommandError> {
    let reader = reader(&library, &images)?;
    cv_ipc::library::library_counts(reader).await
}
