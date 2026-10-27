//! A response from a provider.

use std::fmt;
use std::sync::Arc;

use serde::de::DeserializeOwned;

/// The answer to an [`crate::http::HttpRequest`]: any status except a 429 that ran out of
/// retries (that is a [`crate::ProviderError::RateLimited`]). Providers decide what a 401 or a
/// 404 means for them.
#[derive(Clone)]
pub struct HttpResponse {
    pub(crate) status: u16,
    pub(crate) content_type: Option<String>,
    pub(crate) body: Arc<[u8]>,
    pub(crate) from_cache: bool,
    pub(crate) etag: Option<String>,
    pub(crate) location: Option<String>,
}

impl HttpResponse {
    /// The HTTP status code.
    #[must_use]
    pub fn status(&self) -> u16 {
        self.status
    }

    /// Whether the status is 2xx.
    #[must_use]
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// The `Content-Type` header, if the server sent one.
    #[must_use]
    pub fn content_type(&self) -> Option<&str> {
        self.content_type.as_deref()
    }

    /// The raw body.
    #[must_use]
    pub fn body(&self) -> &[u8] {
        &self.body
    }

    /// The body as text; invalid UTF-8 is replaced.
    #[must_use]
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// Parses the body as JSON.
    pub fn json<T: DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_slice(&self.body)
    }

    /// The `ETag` header of a network response, for conditional requests. Cached responses carry
    /// none.
    #[must_use]
    pub fn etag(&self) -> Option<&str> {
        self.etag.as_deref()
    }

    /// The `Location` header of a redirect the client did not follow (a redirect to an address that
    /// is not a web page, such as a magnet link). Cached responses carry none.
    #[must_use]
    pub fn location(&self) -> Option<&str> {
        self.location.as_deref()
    }

    /// Whether this response came from the disk cache rather than the network.
    #[must_use]
    pub fn from_cache(&self) -> bool {
        self.from_cache
    }
}

impl fmt::Debug for HttpResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpResponse")
            .field("status", &self.status)
            .field("content_type", &self.content_type)
            .field("body_len", &self.body.len())
            .field("from_cache", &self.from_cache)
            .finish()
    }
}
