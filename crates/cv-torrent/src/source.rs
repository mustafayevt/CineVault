//! The source trait and the HTTP plumbing every source shares.
//!
//! A source is one small client: it knows its site's address and answer format and nothing
//! else. [`SourceHttp`] sends its requests through the shared provider HTTP client, one lane per
//! source (`torrent:<id>`: one request per second, two in flight), with a 20 second deadline, no
//! automatic retry, the CineVault User-Agent and an 8 MiB limit on the answer, and turns every
//! way a request can go wrong into a [`SourceError`] so the page can say why a source is empty.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use cv_providers::ProviderError;
use cv_providers::http::{HttpClient, HttpClientBuilder, HttpRequest, ProviderLimits, RetryPolicy};
use serde::de::DeserializeOwned;

use crate::cancel::CancelToken;
use crate::model::{RawRelease, SourceError, SourceId, SourceKind, Support, TorrentQuery};

/// The longest an answer may be.
pub const MAX_ANSWER_BYTES: usize = 8 * 1024 * 1024;
/// How long one request may take.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
/// How long one request to the user's own Torznab instance may take. Its all-indexers feed asks
/// every site it holds before it answers (dozens of sites took 22 seconds in a real instance).
pub const AGGREGATE_REQUEST_TIMEOUT: Duration = Duration::from_secs(75);
/// The lane name of the loopback indexer service.
pub const INDEXER_LANE: &str = "torrent:indexer";

/// The User-Agent every torrent request carries.
#[must_use]
pub fn user_agent() -> String {
    format!(
        "CineVault/{} (+https://github.com/mustafayevt/CineVault)",
        env!("CARGO_PKG_VERSION")
    )
}

/// The limits of a direct source's lane: one request per second, two in flight.
#[must_use]
pub fn source_limits() -> ProviderLimits {
    ProviderLimits::new(2, 1, Duration::from_secs(1))
}

/// The limits of the indexer service's lane: it is on this computer, so many at once.
#[must_use]
pub fn indexer_limits() -> ProviderLimits {
    ProviderLimits {
        name: None,
        max_concurrent: 16,
        requests: 32,
        period: Duration::from_secs(1),
        burst: 16,
    }
}

/// A client builder set up for torrent sources: the User-Agent, a 20 second timeout, no retry on
/// HTTP 429, the 8 MiB answer limit, source limits for every lane and indexer limits for the
/// indexer lane. The caller adds the cache directory, redactor and anything else it needs.
#[must_use]
pub fn client_builder() -> HttpClientBuilder {
    HttpClient::builder()
        .user_agent(&user_agent())
        .timeout(REQUEST_TIMEOUT)
        .max_body_bytes(MAX_ANSWER_BYTES)
        .retry_policy(RetryPolicy {
            max_retries: 0,
            max_wait: Duration::ZERO,
            base_backoff: Duration::ZERO,
        })
        .default_limits(source_limits())
        .provider_limits(INDEXER_LANE, indexer_limits())
}

/// Receives rows a source has found before it has finished (the text sources send one batch per
/// spelling of the title).
pub type PartialSink = Arc<dyn Fn(Vec<RawRelease>) + Send + Sync>;

/// What a search hands to a source besides the query.
#[derive(Clone)]
pub struct SearchContext {
    cancel: CancelToken,
    partial: Option<PartialSink>,
}

impl SearchContext {
    /// A context that only carries the cancellation token.
    #[must_use]
    pub fn quiet(cancel: CancelToken) -> Self {
        Self {
            cancel,
            partial: None,
        }
    }

    /// A context that also passes partial rows on.
    #[must_use]
    pub fn with_partial(cancel: CancelToken, partial: PartialSink) -> Self {
        Self {
            cancel,
            partial: Some(partial),
        }
    }

    /// Whether the search was cancelled; a source stops between requests when it was.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// The cancellation token.
    #[must_use]
    pub fn cancel_token(&self) -> &CancelToken {
        &self.cancel
    }

    /// Offers rows found so far. Nothing happens when the search does not want them.
    pub fn partial(&self, rows: Vec<RawRelease>) {
        if let Some(sink) = &self.partial
            && !rows.is_empty()
        {
            sink(rows);
        }
    }
}

/// One place releases can be found.
#[async_trait]
pub trait TorrentSource: Send + Sync {
    /// `torrentio`, `yts`, `eztv`, `apibay`, `knaben`, `nyaa`, `indexer:<id>` or `torznab`.
    fn id(&self) -> &SourceId;
    /// The name the page shows.
    fn name(&self) -> &str;
    fn kind(&self) -> SourceKind;
    /// Whether the source applies to this query. A source that does not is skipped, with the
    /// reason shown, and never contacted.
    fn supports(&self, query: &TorrentQuery) -> Support;
    /// Searches. Every release carries this source's id.
    async fn search(
        &self,
        query: &TorrentQuery,
        context: &SearchContext,
    ) -> Result<Vec<RawRelease>, SourceError>;
}

/// An answer that arrived: what status it had and what it held.
#[derive(Debug, Clone)]
pub struct SourceResponse {
    pub status: u16,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
    /// Where a redirect that was not followed points (for example to a magnet link).
    pub location: Option<String>,
}

impl SourceResponse {
    /// The body as text; invalid UTF-8 is replaced.
    #[must_use]
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// Whether a body is a web page instead of data: the sign of a challenge or block page.
#[must_use]
pub fn looks_like_html(body: &[u8]) -> bool {
    let head: String = String::from_utf8_lossy(&body[..body.len().min(512)])
        .trim_start_matches('\u{feff}')
        .trim_start()
        .to_ascii_lowercase();
    head.starts_with("<!doctype html")
        || head.starts_with("<html")
        || head.starts_with("<head")
        || head.starts_with("<body")
        || (head.starts_with("<!--") && head.contains("<html"))
}

/// Sends the requests of one source.
#[derive(Clone)]
pub struct SourceHttp {
    client: HttpClient,
    lane: String,
    timeout: Option<Duration>,
}

impl std::fmt::Debug for SourceHttp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SourceHttp")
            .field("lane", &self.lane)
            .finish_non_exhaustive()
    }
}

impl SourceHttp {
    /// A sender for the source `source_id` (lane `torrent:<id>`).
    #[must_use]
    pub fn new(client: HttpClient, source_id: &str) -> Self {
        Self {
            client,
            lane: format!("torrent:{source_id}"),
            timeout: None,
        }
    }

    /// A sender on an explicit lane (the loopback indexer service uses [`INDEXER_LANE`]).
    #[must_use]
    pub fn on_lane(client: HttpClient, lane: &str) -> Self {
        Self {
            client,
            lane: lane.to_owned(),
            timeout: None,
        }
    }

    /// Gives every request its own whole-request timeout instead of [`REQUEST_TIMEOUT`].
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// The lane requests are sent on.
    #[must_use]
    pub fn lane(&self) -> &str {
        &self.lane
    }

    fn build(&self, url: &str, query: &[(&str, &str)]) -> HttpRequest {
        let mut request = HttpRequest::get(&self.lane, url).uncached();
        if let Some(timeout) = self.timeout {
            request = request.timeout(timeout);
        }
        for (name, value) in query {
            request = request.query(name, value);
        }
        request
    }

    /// Sends a GET and returns whatever answered, whatever its status. Only transport problems
    /// are errors here.
    ///
    /// # Errors
    /// [`SourceError::RateLimited`], [`SourceError::Timeout`], [`SourceError::Unreachable`] or
    /// [`SourceError::BadAnswer`] (an answer past the size limit, or an unusable address).
    pub async fn get_raw(
        &self,
        url: &str,
        query: &[(&str, &str)],
    ) -> Result<SourceResponse, SourceError> {
        self.send(self.build(url, query)).await
    }

    /// Like [`Self::get_raw`] with request headers (credentials go in `query` of the request
    /// builder by the caller when needed; here only plain headers).
    pub async fn get_raw_with_headers(
        &self,
        url: &str,
        query: &[(&str, &str)],
        headers: &[(&str, &str)],
    ) -> Result<SourceResponse, SourceError> {
        let mut request = self.build(url, query);
        for (name, value) in headers {
            request = request.header(name, value);
        }
        self.send(request).await
    }

    /// Sends a GET whose `credentials` go in the query but never into a cache key, a log line or
    /// an error message (an API key).
    pub async fn get_credentialed(
        &self,
        url: &str,
        query: &[(&str, &str)],
        credentials: &[(&str, &str)],
    ) -> Result<SourceResponse, SourceError> {
        let mut request = self.build(url, query);
        for (name, value) in credentials {
            request = request.credential_query(name, value);
        }
        self.send(request).await
    }

    /// Sends a prepared request (a caller that has credentials to keep out of logs builds the
    /// request itself with `credential_query`).
    pub async fn send(&self, request: HttpRequest) -> Result<SourceResponse, SourceError> {
        match self.client.send(request).await {
            Ok(response) => Ok(SourceResponse {
                status: response.status(),
                content_type: response.content_type().map(str::to_owned),
                body: response.body().to_vec(),
                location: response.location().map(str::to_owned),
            }),
            Err(error) => Err(map_transport_error(&error)),
        }
    }

    /// Fetches text. A non-success status or a web page instead of data is an error.
    ///
    /// # Errors
    /// See [`Self::get_raw`] and [`check_status`].
    pub async fn get_text(&self, url: &str, query: &[(&str, &str)]) -> Result<String, SourceError> {
        let response = self.get_raw(url, query).await?;
        check_status(&response)?;
        if looks_like_html(&response.body) {
            return Err(SourceError::Blocked);
        }
        Ok(response.text())
    }

    /// Fetches JSON. A web page where JSON was expected is [`SourceError::Blocked`]; broken or
    /// truncated JSON is [`SourceError::BadAnswer`].
    ///
    /// # Errors
    /// See [`Self::get_raw`] and [`check_status`].
    pub async fn get_json<T: DeserializeOwned>(
        &self,
        url: &str,
        query: &[(&str, &str)],
    ) -> Result<T, SourceError> {
        let response = self.get_raw(url, query).await?;
        parse_json(&response)
    }
}

/// Reads a response as JSON, with the status and web-page checks of [`SourceHttp::get_json`].
///
/// # Errors
/// [`SourceError`] as for [`SourceHttp::get_json`].
pub fn parse_json<T: DeserializeOwned>(response: &SourceResponse) -> Result<T, SourceError> {
    check_status(response)?;
    if looks_like_html(&response.body) {
        return Err(SourceError::Blocked);
    }
    serde_json::from_slice(&response.body)
        .map_err(|error| SourceError::BadAnswer(format!("the answer is not valid JSON ({error})")))
}

/// Maps a status to an error: 429 rate limited; 401 and 403 blocked when the body is a web page
/// and bad answer otherwise; 5xx unreachable (a site that is down); anything else outside 2xx a
/// bad answer.
///
/// # Errors
/// The mapped [`SourceError`] for a non-success status.
pub fn check_status(response: &SourceResponse) -> Result<(), SourceError> {
    match response.status {
        200..=299 => Ok(()),
        429 => Err(SourceError::RateLimited),
        401 | 403 | 503 if looks_like_html(&response.body) => Err(SourceError::Blocked),
        500..=599 => Err(SourceError::Unreachable(format!(
            "the site answered HTTP {}",
            response.status
        ))),
        status => Err(SourceError::BadAnswer(format!(
            "the site answered HTTP {status}"
        ))),
    }
}

/// Maps a failed request of the shared client. The client's messages are plain sentences
/// ("the request timed out", "could not connect: ...", "the answer is too large") and never
/// carry the address or a credential, so they can be shown.
fn map_transport_error(error: &ProviderError) -> SourceError {
    match error {
        ProviderError::RateLimited { .. } => SourceError::RateLimited,
        ProviderError::Request { message, .. } => {
            let lower = message.to_lowercase();
            if lower.contains("timed out") {
                SourceError::Timeout
            } else if lower.contains("too large") {
                SourceError::BadAnswer("the answer is too large".to_owned())
            } else if lower.contains("invalid request url") {
                SourceError::BadAnswer("the address is not usable".to_owned())
            } else {
                SourceError::Unreachable(message.clone())
            }
        }
        other => SourceError::Unreachable(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(status: u16, body: &str) -> SourceResponse {
        SourceResponse {
            status,
            content_type: None,
            body: body.as_bytes().to_vec(),
            location: None,
        }
    }

    #[test]
    fn html_pages_are_recognised() {
        assert!(looks_like_html(b"<!DOCTYPE html><html>"));
        assert!(looks_like_html(b"  \n<html lang=en>"));
        assert!(looks_like_html("\u{feff}<HTML>".as_bytes()));
        assert!(!looks_like_html(b"{\"a\":1}"));
        assert!(!looks_like_html(b"<?xml version=\"1.0\"?><rss/>"));
        assert!(!looks_like_html(b"[]"));
        assert!(!looks_like_html(b""));
    }

    #[test]
    fn statuses_map_to_the_errors_the_page_shows() {
        assert_eq!(check_status(&response(200, "")), Ok(()));
        assert_eq!(check_status(&response(204, "")), Ok(()));
        assert_eq!(
            check_status(&response(429, "")),
            Err(SourceError::RateLimited)
        );
        assert_eq!(
            check_status(&response(403, "<html>Just a moment...</html>")),
            Err(SourceError::Blocked)
        );
        assert_eq!(
            check_status(&response(503, "<!doctype html>")),
            Err(SourceError::Blocked)
        );
        assert!(matches!(
            check_status(&response(403, "denied")),
            Err(SourceError::BadAnswer(_))
        ));
        assert!(matches!(
            check_status(&response(500, "")),
            Err(SourceError::Unreachable(_))
        ));
        assert!(matches!(
            check_status(&response(503, "maintenance")),
            Err(SourceError::Unreachable(_))
        ));
        assert!(matches!(
            check_status(&response(404, "")),
            Err(SourceError::BadAnswer(_))
        ));
    }

    #[test]
    fn json_errors_do_not_echo_the_body() {
        let error =
            parse_json::<serde_json::Value>(&response(200, "{\"secret\": tru")).unwrap_err();
        let SourceError::BadAnswer(message) = error else {
            panic!("expected a bad answer");
        };
        assert!(!message.contains("secret"), "{message}");
    }

    #[test]
    fn the_user_agent_names_the_app_and_its_home() {
        let agent = user_agent();
        assert!(agent.starts_with("CineVault/"));
        assert!(agent.contains("github.com/mustafayevt/CineVault"));
    }

    #[test]
    fn transport_errors_map_by_what_went_wrong() {
        let request = |message: &str| ProviderError::Request {
            provider: "torrent:x".into(),
            message: message.into(),
        };
        assert_eq!(
            map_transport_error(&request("the request timed out")),
            SourceError::Timeout
        );
        assert!(matches!(
            map_transport_error(&request("could not connect: dns error")),
            SourceError::Unreachable(_)
        ));
        assert!(matches!(
            map_transport_error(&request("the answer is too large")),
            SourceError::BadAnswer(_)
        ));
        assert_eq!(
            map_transport_error(&ProviderError::RateLimited {
                provider: "x".into(),
                retry_after_secs: 3
            }),
            SourceError::RateLimited
        );
    }
}
