//! The library's image index and the image service over the thumbnail cache.
//!
//! `cv_artwork::img` does the work (URL parsing, cache headers, lazy generation and stand-ins);
//! this module holds the two pieces the surfaces pass it: the key-to-file index the library
//! queries fill, and the service over the shared thumbnail cache. The desktop protocol handler and
//! the server's image route both answer from [`ImageContext::handle`], so an image behaves the
//! same on both.

use std::sync::Arc;

use cv_app::services::AppServices;
use cv_artwork::img::{ImageRequest, ImageResponse, ImageService};
use cv_artwork::library_source::LibrarySources;
use cv_artwork::thumb::{DEFAULT_MAX_BYTES, ThumbCache};

/// The library's key-to-file index and the image service over the thumbnail cache.
#[derive(Clone)]
pub struct ImageContext {
    sources: Arc<LibrarySources>,
    service: Option<Arc<ImageService>>,
}

impl ImageContext {
    /// The poster index and the thumbnail cache the app services opened in the data folder's cache
    /// folder.
    pub fn from_services(services: &AppServices) -> Self {
        Self::with_cache(Some(Arc::clone(services.thumbs())))
    }

    /// A cache under the temp folder, for a start that could not open the app services: it serves
    /// thumbnails for this session, so the window still shows pictures.
    pub fn with_temp_cache() -> Self {
        let fallback = std::env::temp_dir().join("cinevault-cache");
        let cache = match ThumbCache::open(&fallback, Some(DEFAULT_MAX_BYTES)) {
            Ok(cache) => Some(Arc::new(cache)),
            Err(error) => {
                tracing::warn!(%error, "cannot open the thumbnail cache");
                None
            }
        };
        Self::with_cache(cache)
    }

    /// The index and service over `cache`. The path-to-key map is saved beside the thumbnails, so
    /// a poster whose folder is offline still shows its cached thumbnail after a restart.
    pub fn with_cache(cache: Option<Arc<ThumbCache>>) -> Self {
        let sources = Arc::new(match &cache {
            Some(cache) => LibrarySources::with_store(cache.root().join("poster-keys.json")),
            None => LibrarySources::new(),
        });
        let service = cache.map(|cache| Arc::new(ImageService::new(cache, sources.clone())));
        Self { sources, service }
    }

    /// The index the library queries fill with each poster's key and file.
    pub fn sources(&self) -> &Arc<LibrarySources> {
        &self.sources
    }

    /// The image service, when a thumbnail cache could be opened.
    pub fn service(&self) -> Option<&ImageService> {
        self.service.as_deref()
    }

    /// Answer one image request. Blocking: lazy generation decodes and encodes images.
    ///
    /// Without a cache the answer is the protocol's "not ready": 503 and `no-store`.
    pub fn handle(&self, request: &ImageRequest<'_>) -> ImageResponse {
        match &self.service {
            Some(service) => service.handle(request),
            None => ImageResponse {
                status: 503,
                headers: vec![("cache-control", "no-store".to_owned())],
                body: Vec::new(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_context_without_a_cache_has_no_service_and_answers_not_ready() {
        let images = ImageContext::with_cache(None);
        assert!(images.service().is_none());
        let answer = images.handle(&ImageRequest {
            method: "GET",
            path: "/img/00000000000000000000000000000000/grid",
            if_none_match: None,
        });
        assert_eq!(answer.status, 503);
        assert_eq!(answer.header("cache-control"), Some("no-store"));
        assert!(answer.body.is_empty());
    }
}
