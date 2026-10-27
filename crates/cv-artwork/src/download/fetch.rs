//! Getting the bytes of an image from its source URL.
//!
//! [`ImageFetcher`] is what the downloads ask; [`HttpImageFetcher`] is the real one. It sends the
//! request through the shared provider HTTP client, so image downloads get a lane of
//! their own with a concurrency and rate limit, retry after a `429` and redacted logs. Images are
//! never put in the client's response cache: they are written to the library folder, which is the
//! cache that matters.
//!
//! `file:` URLs (video frame grabs, see [`crate::frame_source`]) are read from disk.

use async_trait::async_trait;
use cv_providers::http::{HttpClient, HttpRequest, ProviderLimits};

use crate::download::engine::MAX_IMAGE_BYTES;
use crate::frame_source::path_from_file_url;

/// The lane image downloads use in the HTTP client.
pub const IMAGE_LANE: &str = "images";

/// Why an image could not be fetched.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FetchError {
    /// The server answered with an error status.
    #[error("the server answered {0}")]
    Status(u16),
    /// The request did not complete.
    #[error("{0}")]
    Network(String),
    /// A local image could not be read.
    #[error("the image file could not be read: {0}")]
    File(String),
    /// The image is larger than any artwork should be.
    #[error("the image is larger than {} MB", MAX_IMAGE_BYTES / (1024 * 1024))]
    TooLarge,
    /// The answer had no content.
    #[error("the answer was empty")]
    Empty,
    /// Not an `http`, `https` or `file` address.
    #[error("only http, https and file addresses can be downloaded")]
    Scheme,
}

/// Fetches the bytes behind an image URL.
#[async_trait]
pub trait ImageFetcher: Send + Sync {
    /// The bytes of the image at `url`.
    async fn fetch(&self, url: &str) -> Result<Vec<u8>, FetchError>;
}

/// The fetcher of the app: the shared HTTP client for web addresses, the disk for `file:` ones.
#[derive(Clone)]
pub struct HttpImageFetcher {
    http: HttpClient,
}

impl HttpImageFetcher {
    /// Gives the image lane of `http` its limits (four at a time, thirty a second) and fetches
    /// through it.
    pub fn new(http: HttpClient) -> Self {
        http.set_provider_limits(
            IMAGE_LANE,
            ProviderLimits::new(4, 30, std::time::Duration::from_secs(1)).named("Image downloads"),
        );
        Self { http }
    }
}

#[async_trait]
impl ImageFetcher for HttpImageFetcher {
    async fn fetch(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        if let Some(path) = path_from_file_url(url) {
            let size = tokio::fs::metadata(&path)
                .await
                .map_err(|e| FetchError::File(e.to_string()))?
                .len();
            if size > MAX_IMAGE_BYTES as u64 {
                return Err(FetchError::TooLarge);
            }
            return tokio::fs::read(&path)
                .await
                .map_err(|e| FetchError::File(e.to_string()));
        }
        let lower = url.trim().to_ascii_lowercase();
        if !(lower.starts_with("https://") || lower.starts_with("http://")) {
            return Err(FetchError::Scheme);
        }
        // The client stops reading once the body passes the cap (after decompression), and
        // rejects a larger Content-Length before reading anything.
        let response = self
            .http
            .send(
                HttpRequest::get(IMAGE_LANE, url.trim())
                    .uncached()
                    .max_body_bytes(MAX_IMAGE_BYTES),
            )
            .await
            .map_err(|e| FetchError::Network(e.to_string()))?;
        if !response.is_success() {
            return Err(FetchError::Status(response.status()));
        }
        if response.body().is_empty() {
            return Err(FetchError::Empty);
        }
        Ok(response.body().to_vec())
    }
}

/// A fetcher that answers from memory: the images it was given, by URL, and `404` for any other.
/// For tests of code that downloads artwork, and for offline runs.
#[derive(Debug, Default)]
pub struct StaticFetcher {
    images: std::sync::Mutex<std::collections::BTreeMap<String, Vec<u8>>>,
    requests: std::sync::Mutex<Vec<String>>,
}

impl StaticFetcher {
    pub fn new() -> Self {
        Self::default()
    }

    /// Serves `bytes` at `url`.
    pub fn insert(&self, url: &str, bytes: Vec<u8>) {
        if let Ok(mut images) = self.images.lock() {
            images.insert(url.to_owned(), bytes);
        }
    }

    /// Every URL that was asked for, in order.
    pub fn requests(&self) -> Vec<String> {
        self.requests.lock().map(|r| r.clone()).unwrap_or_default()
    }
}

#[async_trait]
impl ImageFetcher for StaticFetcher {
    async fn fetch(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        if let Ok(mut requests) = self.requests.lock() {
            requests.push(url.to_owned());
        }
        let found = self.images.lock().ok().and_then(|i| i.get(url).cloned());
        found.ok_or(FetchError::Status(404))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn file_addresses_are_read_from_disk_and_other_schemes_are_refused() {
        let http = HttpClient::builder().build().expect("client");
        let fetcher = HttpImageFetcher::new(http);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("frame.jpg");
        std::fs::write(&path, b"pixels").unwrap();

        let url = crate::frame_source::file_url(&path);
        assert_eq!(fetcher.fetch(&url).await.unwrap(), b"pixels");
        assert!(matches!(
            fetcher
                .fetch(&crate::frame_source::file_url(&dir.path().join("gone.jpg")))
                .await,
            Err(FetchError::File(_))
        ));
        assert_eq!(
            fetcher.fetch("ftp://x/y.jpg").await,
            Err(FetchError::Scheme)
        );
    }

    #[tokio::test]
    async fn a_local_image_past_the_size_cap_is_not_read() {
        let fetcher = HttpImageFetcher::new(HttpClient::builder().build().expect("client"));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("huge.png");
        let file = std::fs::File::create(&path).unwrap();
        // A sparse file: the size is what counts, nothing is written.
        file.set_len(MAX_IMAGE_BYTES as u64 + 1).unwrap();
        drop(file);
        assert_eq!(
            fetcher.fetch(&crate::frame_source::file_url(&path)).await,
            Err(FetchError::TooLarge)
        );
    }
}
