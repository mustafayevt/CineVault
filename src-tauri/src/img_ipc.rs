//! The `cv://img/<key>/<size>` protocol and the command that tells it where an image lives.
//!
//! The protocol handler is a thin adapter: the rules (URL parsing, cache headers, lazy
//! generation) live in `cv_artwork::img`. Images never cross IPC. The UI puts the URL in
//! `<img src>`, and the web view fetches and caches it. The only command here moves a key.

#[cfg(debug_assertions)]
use std::path::PathBuf;
use std::sync::Arc;

use cv_artwork::img::{ImageRequest, ImageResponse, ImageService};
use cv_artwork::library_source::LibrarySources;
use cv_artwork::thumb::{DEFAULT_MAX_BYTES, ThumbCache};
#[cfg(debug_assertions)]
use serde::Serialize;
#[cfg(debug_assertions)]
use specta::Type;
#[cfg(debug_assertions)]
use tauri::State;
use tauri::http::{self, header};
use tauri::{Manager, Runtime, UriSchemeContext, UriSchemeResponder};

use crate::boot::Boot;

/// Held in Tauri state: the library's key-to-file index and the protocol service over the
/// thumbnail cache.
pub struct ImageState {
    sources: Arc<LibrarySources>,
    service: Option<ImageService>,
}

impl ImageState {
    /// The thumbnail cache the app services opened in the data folder's cache folder. When the
    /// services could not start, a cache under the temp folder serves for this session.
    pub fn from_boot(boot: &Boot) -> Self {
        if let Some(services) = boot.services() {
            return Self::with_cache(Some(Arc::clone(services.thumbs())));
        }
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

    fn with_cache(cache: Option<Arc<ThumbCache>>) -> Self {
        // The path-to-key map is saved beside the thumbnails, so a poster whose folder is offline
        // still shows its cached thumbnail after a restart.
        let sources = Arc::new(match &cache {
            Some(cache) => LibrarySources::with_store(cache.root().join("poster-keys.json")),
            None => LibrarySources::new(),
        });
        let service = cache.map(|cache| ImageService::new(cache, sources.clone()));
        Self { sources, service }
    }

    /// The index the library queries fill with each poster's key and file.
    pub fn sources(&self) -> &Arc<LibrarySources> {
        &self.sources
    }

    pub fn service(&self) -> Option<&ImageService> {
        self.service.as_ref()
    }
}

/// Turn the protocol's answer into an HTTP response.
fn to_http(response: ImageResponse) -> http::Response<Vec<u8>> {
    let mut builder = http::Response::builder().status(response.status);
    for (name, value) in &response.headers {
        builder = builder.header(*name, value.as_str());
    }
    builder.body(response.body).unwrap_or_else(|_| {
        http::Response::builder()
            .status(500)
            .body(Vec::new())
            .unwrap_or_default()
    })
}

/// Answer one request. Blocking: lazy generation decodes and encodes images.
fn serve(service: &ImageService, request: &http::Request<Vec<u8>>) -> http::Response<Vec<u8>> {
    let if_none_match = request
        .headers()
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok());
    let answer = service.handle(&ImageRequest {
        method: request.method().as_str(),
        path: request.uri().path(),
        if_none_match,
    });
    to_http(answer)
}

/// The handler registered for the `cv` scheme.
pub fn handle_protocol<R: Runtime>(
    context: UriSchemeContext<'_, R>,
    request: http::Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    // A request that comes before the app has set up its state gets "not ready", not a panic in
    // the web view's callback.
    let service = context
        .app_handle()
        .try_state::<ImageState>()
        .and_then(|state| state.service().cloned());
    let Some(service) = service else {
        responder.respond(
            http::Response::builder()
                .status(503)
                .header(header::CACHE_CONTROL, "no-store")
                .body(Vec::new())
                .unwrap_or_default(),
        );
        return;
    };
    tauri::async_runtime::spawn_blocking(move || {
        responder.respond(serve(&service, &request));
    });
}

/// Where the UI finds an image: the key to put in a `cv://img/<key>/<size>` URL.
#[cfg(debug_assertions)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct ImageRefDto {
    pub key: String,
}

/// Tells the image protocol where the image file at `path` lives and returns its content key.
/// The thumbnail is generated when the web view first requests the URL. Only a key comes back.
///
/// This is a test surface for the component gallery; the
/// library's own posters are registered by the library queries (see `library_ipc`). It is
/// compiled and registered in debug builds only, so a release build cannot serve arbitrary files.
#[cfg(debug_assertions)]
#[tauri::command]
#[specta::specta]
pub async fn register_image_source(
    state: State<'_, ImageState>,
    path: String,
) -> Result<ImageRefDto, String> {
    let sources = state.sources().clone();
    let path = PathBuf::from(path);
    tauri::async_runtime::spawn_blocking(move || sources.manual().register_file(&path))
        .await
        .map_err(|e| e.to_string())?
        .map(|key| ImageRefDto {
            key: key.to_string(),
        })
        .map_err(|_| "That file cannot be read.".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};
    use std::io::Cursor;

    fn png() -> Vec<u8> {
        let img = RgbaImage::from_fn(600, 900, |x, y| {
            Rgba([(x % 255) as u8, (y % 255) as u8, 9, 255])
        });
        let mut out = Vec::new();
        img.write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    fn state(dir: &std::path::Path) -> ImageState {
        let cache = ThumbCache::open(&dir.join("cache"), None).unwrap();
        ImageState::with_cache(Some(Arc::new(cache)))
    }

    fn request(uri: &str, etag: Option<&str>) -> http::Request<Vec<u8>> {
        let mut builder = http::Request::builder().method("GET").uri(uri);
        if let Some(etag) = etag {
            builder = builder.header(header::IF_NONE_MATCH, etag);
        }
        builder.body(Vec::new()).unwrap()
    }

    #[test]
    fn serves_through_both_platform_url_forms_with_cache_headers() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("poster.png");
        std::fs::write(&file, png()).unwrap();
        let state = state(tmp.path());
        let key = state.sources().manual().register_file(&file).unwrap();
        let service = state.service().unwrap().clone();

        // Windows and Android: http://cv.localhost/<encoded path>; others: cv://localhost/<path>.
        let urls = [
            format!("http://cv.localhost/img%2F{key}%2Fgrid"),
            format!("cv://localhost/img/{key}/grid"),
        ];
        for url in urls {
            let response = serve(&service, &request(&url, None));
            assert_eq!(response.status(), 200, "{url}");
            let headers = response.headers();
            assert_eq!(headers[header::CONTENT_TYPE], "image/webp");
            assert_eq!(
                headers[header::CACHE_CONTROL],
                "public, max-age=31536000, immutable"
            );
            assert_eq!(headers[header::ETAG], format!("\"{key}-grid\"").as_str());
            assert!(response.body().starts_with(b"RIFF"));
        }
        assert_eq!(service.cache().generations(), 1);
    }

    #[test]
    fn answers_304_and_404_and_forwards_if_none_match() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("poster.png");
        std::fs::write(&file, png()).unwrap();
        let state = state(tmp.path());
        let key = state.sources().manual().register_file(&file).unwrap();
        let service = state.service().unwrap().clone();
        let url = format!("http://cv.localhost/img%2F{key}%2Flist");

        let first = serve(&service, &request(&url, None));
        let tag = first.headers()[header::ETAG].to_str().unwrap().to_owned();
        let again = serve(&service, &request(&url, Some(&tag)));
        assert_eq!(again.status(), 304);
        assert!(again.body().is_empty());

        let missing = format!("http://cv.localhost/img%2F{}%2Flist", "0".repeat(32));
        assert_eq!(serve(&service, &request(&missing, None)).status(), 404);
    }

    #[test]
    fn a_state_without_a_cache_has_no_service() {
        assert!(ImageState::with_cache(None).service().is_none());
    }
}
