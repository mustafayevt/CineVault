//! The Tauri surface of the collection page.
//!
//! The page conversion and the DTOs live in [`cv_ipc::collections`]; this module keeps the service
//! the app state builds once and wraps the one read.

use std::sync::{Arc, OnceLock};

use cv_app::collections::CollectionService;
use tauri::State;

use crate::boot::Boot;
use crate::edit_ipc::{EditCommandError, id_of};
use crate::img_ipc::ImageState;
use crate::nfo_ipc::NfoState;
use crate::sources_ipc::LibraryState;

pub use cv_ipc::collections::*;

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
    cv_ipc::collections::get_collection(service, db, sources, cache, id)
        .await
        .map_err(|error| EditCommandError {
            code: error.code,
            message: error.message,
            field: None,
        })
}
