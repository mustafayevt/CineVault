//! Describing one request.
//!
//! A request keeps its credentials apart from the rest of it: `credential_query`,
//! `credential_header` and `secret` carry values that are sent but never cached, hashed into a
//! cache key, logged or printed. Everything else (`query`, `header`, the URL) is the request's
//! identity and decides cache and coalescing keys.

use std::fmt;

use reqwest::Url;
use reqwest::header::{HeaderName, HeaderValue};
use sha2::{Digest, Sha256};

use crate::error::ProviderError;

use super::cache::CacheKind;
use super::redact::{Masker, Redactor};

/// The request methods providers use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
}

impl HttpMethod {
    fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
        }
    }
}

/// One request to a provider. Build it with [`HttpRequest::get`] or [`HttpRequest::post`].
#[derive(Clone)]
pub struct HttpRequest {
    pub(crate) provider: String,
    pub(crate) method: HttpMethod,
    pub(crate) url: String,
    pub(crate) query: Vec<(String, String)>,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) credential_query: Vec<(String, String)>,
    pub(crate) credential_headers: Vec<(String, String)>,
    pub(crate) secrets: Vec<String>,
    pub(crate) body: Option<Vec<u8>>,
    pub(crate) cache: CacheKind,
    pub(crate) bypass_cache: bool,
    pub(crate) timeout: Option<std::time::Duration>,
    pub(crate) max_body: Option<usize>,
}

impl HttpRequest {
    fn new(provider: &str, method: HttpMethod, url: &str, cache: CacheKind) -> Self {
        Self {
            provider: provider.to_owned(),
            method,
            url: url.to_owned(),
            query: Vec::new(),
            headers: Vec::new(),
            credential_query: Vec::new(),
            credential_headers: Vec::new(),
            secrets: Vec::new(),
            body: None,
            cache,
            bypass_cache: false,
            timeout: None,
            max_body: None,
        }
    }

    /// A GET for `provider` (its lane id, for example `tmdb`). Cached as an API response and
    /// coalesced with identical requests by default.
    #[must_use]
    pub fn get(provider: &str, url: &str) -> Self {
        Self::new(provider, HttpMethod::Get, url, CacheKind::Api)
    }

    /// A POST with a body. Never cached or coalesced.
    #[must_use]
    pub fn post(provider: &str, url: &str, body: impl Into<Vec<u8>>) -> Self {
        let mut request = Self::new(provider, HttpMethod::Post, url, CacheKind::None);
        request.body = Some(body.into());
        request
    }

    /// Adds a query parameter that is part of the request's identity.
    #[must_use]
    pub fn query(mut self, name: &str, value: &str) -> Self {
        self.query.push((name.to_owned(), value.to_owned()));
        self
    }

    /// Adds a header that is part of the request's identity (for example `Accept-Language`).
    #[must_use]
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_owned(), value.to_owned()));
        self
    }

    /// Adds a query parameter that carries a credential (for example TMDB's `api_key`).
    #[must_use]
    pub fn credential_query(mut self, name: &str, value: &str) -> Self {
        self.credential_query
            .push((name.to_owned(), value.to_owned()));
        self
    }

    /// Adds a header that carries a credential (for example `Authorization` or `api-key`).
    #[must_use]
    pub fn credential_header(mut self, name: &str, value: &str) -> Self {
        self.credential_headers
            .push((name.to_owned(), value.to_owned()));
        self
    }

    /// Registers a secret value that appears somewhere else in the request, such as inside the
    /// URL path, so it is masked wherever the client prints text about the request.
    #[must_use]
    pub fn secret(mut self, value: &str) -> Self {
        self.secrets.push(value.to_owned());
        self
    }

    /// Marks the response as a scraped HTML page (cached for a day by default).
    #[must_use]
    pub fn html(mut self) -> Self {
        self.cache = CacheKind::Html;
        self
    }

    /// Never reads or writes the cache for this request.
    #[must_use]
    pub fn uncached(mut self) -> Self {
        self.cache = CacheKind::None;
        self
    }

    /// When `true`, skips the cache read and goes to the network (a forced rescrape). A
    /// successful response still refreshes the cache.
    #[must_use]
    pub fn bypass_cache(mut self, bypass: bool) -> Self {
        self.bypass_cache = bypass;
        self
    }

    /// A whole-request timeout for this request instead of the client's own (a search that
    /// fans out to many sites behind one address takes longer than one site).
    #[must_use]
    pub fn timeout(mut self, timeout: std::time::Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// The largest answer body this request reads, in bytes, instead of the client's own limit
    /// (an image may be larger than an API answer).
    #[must_use]
    pub fn max_body_bytes(mut self, max: usize) -> Self {
        self.max_body = Some(max);
        self
    }

    /// The provider lane id.
    #[must_use]
    pub fn provider(&self) -> &str {
        &self.provider
    }

    /// Whether this request may be served from or stored in the cache.
    pub(crate) fn cacheable(&self) -> bool {
        self.method == HttpMethod::Get && self.cache != CacheKind::None
    }

    /// Parses the URL and derives the cache key and the masking rules.
    pub(crate) fn prepare(&self, global: Option<Redactor>) -> Result<Prepared, ProviderError> {
        let mut secrets = self.secrets.clone();
        secrets.extend(self.credential_query.iter().map(|(_, v)| v.clone()));
        secrets.extend(self.credential_headers.iter().map(|(_, v)| v.clone()));
        let masker = Masker::new(&secrets, global);

        let invalid = |what: &str| ProviderError::Request {
            provider: self.provider.clone(),
            message: format!("invalid {what}"),
        };

        let mut identity = Url::parse(&self.url).map_err(|_| invalid("request URL"))?;
        if !matches!(identity.scheme(), "http" | "https") {
            return Err(invalid("request URL scheme"));
        }
        if !self.query.is_empty() {
            identity.query_pairs_mut().extend_pairs(&self.query);
        }
        let mut send_url = identity.clone();
        if !self.credential_query.is_empty() {
            send_url
                .query_pairs_mut()
                .extend_pairs(&self.credential_query);
        }

        let mut headers = Vec::new();
        for (name, value) in &self.headers {
            headers.push(parse_header(name, value, false).ok_or_else(|| invalid("header"))?);
        }
        for (name, value) in &self.credential_headers {
            headers.push(parse_header(name, value, true).ok_or_else(|| invalid("header"))?);
        }

        let key = cache_key(self.method, &identity, &self.headers);
        let display_url = masker.url(&identity);
        Ok(Prepared {
            key,
            send_url,
            display_url,
            headers,
            masker,
        })
    }
}

fn parse_header(name: &str, value: &str, sensitive: bool) -> Option<(HeaderName, HeaderValue)> {
    let name = HeaderName::from_bytes(name.as_bytes()).ok()?;
    let mut value = HeaderValue::from_str(value).ok()?;
    value.set_sensitive(sensitive);
    Some((name, value))
}

/// SHA-256 over the method, the credential-free URL and the identity headers, as hex.
fn cache_key(method: HttpMethod, identity: &Url, headers: &[(String, String)]) -> String {
    let mut sorted: Vec<(String, &str)> = headers
        .iter()
        .map(|(n, v)| (n.to_ascii_lowercase(), v.as_str()))
        .collect();
    sorted.sort();
    let mut hasher = Sha256::new();
    hasher.update(method.as_str().as_bytes());
    hasher.update(b"\n");
    hasher.update(identity.as_str().as_bytes());
    for (name, value) in sorted {
        hasher.update(b"\n");
        hasher.update(name.as_bytes());
        hasher.update(b":");
        hasher.update(value.as_bytes());
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

/// A request ready to send.
pub(crate) struct Prepared {
    pub key: String,
    pub send_url: Url,
    /// The identity URL with credentials masked: the only URL that may be logged or stored.
    pub display_url: String,
    pub headers: Vec<(HeaderName, HeaderValue)>,
    pub masker: Masker,
}

impl fmt::Debug for HttpRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names = |pairs: &[(String, String)]| -> Vec<String> {
            pairs.iter().map(|(n, _)| n.clone()).collect()
        };
        // Credential values are deliberately left out: only their names are shown, and the URL
        // is printed with credential-style parameters masked.
        let shown_url = Url::parse(&self.url).map_or_else(
            |_| "<invalid URL>".to_owned(),
            |u| Masker::new(&self.secrets, None).url(&u),
        );
        f.debug_struct("HttpRequest")
            .field("provider", &self.provider)
            .field("method", &self.method)
            .field("url", &shown_url)
            .field("query", &self.query)
            .field("headers", &self.headers)
            .field("credential_query", &names(&self.credential_query))
            .field("credential_headers", &names(&self.credential_headers))
            .field("secrets", &self.secrets.len())
            .field("cache", &self.cache)
            .field("bypass_cache", &self.bypass_cache)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_do_not_change_the_cache_key_but_query_does() {
        let a = HttpRequest::get("tmdb", "https://example.test/search")
            .query("q", "heat")
            .credential_query("api_key", "first-key-1")
            .prepare(None)
            .unwrap();
        let b = HttpRequest::get("tmdb", "https://example.test/search")
            .query("q", "heat")
            .credential_query("api_key", "second-key-2")
            .prepare(None)
            .unwrap();
        let c = HttpRequest::get("tmdb", "https://example.test/search")
            .query("q", "ronin")
            .credential_query("api_key", "first-key-1")
            .prepare(None)
            .unwrap();
        assert_eq!(a.key, b.key);
        assert_ne!(a.key, c.key);
        assert!(a.send_url.as_str().contains("api_key=first-key-1"));
        assert!(!a.display_url.contains("first-key-1"));
    }

    #[test]
    fn identity_headers_change_the_key_credential_headers_do_not() {
        let base = || HttpRequest::get("tmdb", "https://example.test/x");
        let plain = base().prepare(None).unwrap();
        let language = base()
            .header("Accept-Language", "de")
            .prepare(None)
            .unwrap();
        let auth = base()
            .credential_header("Authorization", "Bearer token-value-1")
            .prepare(None)
            .unwrap();
        assert_ne!(plain.key, language.key);
        assert_eq!(plain.key, auth.key);
    }

    #[test]
    fn debug_output_hides_credential_values() {
        let request = HttpRequest::get("tmdb", "https://example.test/x")
            .credential_query("api_key", "very-secret-value")
            .credential_header("Authorization", "Bearer another-secret")
            .secret("path-secret-1");
        let shown = format!("{request:?}");
        assert!(!shown.contains("very-secret-value"), "{shown}");
        assert!(!shown.contains("another-secret"), "{shown}");
        assert!(!shown.contains("path-secret-1"), "{shown}");
        assert!(shown.contains("api_key"), "{shown}");
    }

    #[test]
    fn rejects_bad_urls_without_echoing_them() {
        let err = HttpRequest::get("tmdb", "not a url?api_key=leaky-key-123")
            .prepare(None)
            .map(|_| ())
            .unwrap_err();
        assert!(!err.to_string().contains("leaky-key-123"));
        let err = HttpRequest::get("tmdb", "ftp://example.test/x")
            .prepare(None)
            .map(|_| ())
            .unwrap_err();
        assert!(err.to_string().contains("scheme"));
    }

    #[test]
    fn post_is_not_cacheable() {
        assert!(HttpRequest::get("p", "https://e.test/").cacheable());
        assert!(
            !HttpRequest::get("p", "https://e.test/")
                .uncached()
                .cacheable()
        );
        assert!(!HttpRequest::post("p", "https://e.test/", b"x".to_vec()).cacheable());
    }
}
