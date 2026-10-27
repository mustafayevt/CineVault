//! The shared HTTP client.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant, SystemTime};

use tokio::sync::OnceCell;

use crate::error::ProviderError;

use super::cache::{CachePolicy, DiskCache, NewEntry};
use super::lane::{Lane, LaneStatus, ProviderLimits};
use super::redact::Redactor;
use super::request::{HttpMethod, HttpRequest, Prepared};
use super::response::HttpResponse;
use super::retry::{Pause, RetryPolicy, TokioPause, parse_retry_after};

const DEFAULT_USER_AGENT: &str = concat!("CineVault/", env!("CARGO_PKG_VERSION"));

/// The largest answer body a client reads unless it or the request sets another limit: 16 MiB,
/// counted after decompression, so a huge or compressed-bomb answer cannot fill the memory.
pub const DEFAULT_MAX_BODY_BYTES: usize = 16 * 1024 * 1024;

/// Reported each time a lane is paused by a 429, so the Activity drawer can show it as throttled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThrottleEvent {
    pub provider: String,
    /// How long the lane is paused.
    pub wait: Duration,
    /// Which retry this wait precedes (1 for the first).
    pub retry: u32,
}

type ThrottleObserver = Arc<dyn Fn(&ThrottleEvent) + Send + Sync>;
type Shared = Arc<OnceCell<Result<HttpResponse, ProviderError>>>;

/// Collects the settings for an [`HttpClient`].
pub struct HttpClientBuilder {
    user_agent: String,
    timeout: Duration,
    connect_timeout: Duration,
    cache_dir: Option<std::path::PathBuf>,
    cache_policy: CachePolicy,
    max_body: Option<usize>,
    retry: RetryPolicy,
    default_limits: ProviderLimits,
    limits: HashMap<String, ProviderLimits>,
    redactor: Option<Redactor>,
    pause: Arc<dyn Pause>,
    observer: Option<ThrottleObserver>,
}

impl HttpClientBuilder {
    fn new() -> Self {
        Self {
            user_agent: DEFAULT_USER_AGENT.to_owned(),
            timeout: Duration::from_secs(30),
            connect_timeout: Duration::from_secs(10),
            cache_dir: None,
            cache_policy: CachePolicy::default(),
            max_body: Some(DEFAULT_MAX_BODY_BYTES),
            retry: RetryPolicy::default(),
            default_limits: ProviderLimits::default(),
            limits: HashMap::new(),
            redactor: None,
            pause: Arc::new(TokioPause),
            observer: None,
        }
    }

    /// The `User-Agent` header. Defaults to `CineVault/<version>`.
    #[must_use]
    pub fn user_agent(mut self, user_agent: &str) -> Self {
        self.user_agent = user_agent.to_owned();
        self
    }

    /// The whole-request timeout (30 seconds by default).
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Stores responses under `dir` (for example a folder inside the data location's cache
    /// folder). Without it nothing is cached.
    #[must_use]
    pub fn cache_dir(mut self, dir: impl Into<std::path::PathBuf>) -> Self {
        self.cache_dir = Some(dir.into());
        self
    }

    /// How long API and HTML responses stay cached.
    #[must_use]
    pub fn cache_policy(mut self, policy: CachePolicy) -> Self {
        self.cache_policy = policy;
        self
    }

    /// The largest answer body the client reads, in bytes, counted after decompression. A longer
    /// answer fails with a request error ("the answer is too large") after at most that much was
    /// read. [`DEFAULT_MAX_BODY_BYTES`] by default; a request can set its own
    /// ([`HttpRequest::max_body_bytes`]).
    #[must_use]
    pub fn max_body_bytes(mut self, max: usize) -> Self {
        self.max_body = Some(max);
        self
    }

    /// 429 retry limits. The default is 3 retries, each waiting at most 30 seconds.
    #[must_use]
    pub fn retry_policy(mut self, retry: RetryPolicy) -> Self {
        self.retry = retry;
        self
    }

    /// The limits for a provider that has none of its own.
    #[must_use]
    pub fn default_limits(mut self, limits: ProviderLimits) -> Self {
        self.default_limits = limits;
        self
    }

    /// The limits for one provider lane.
    #[must_use]
    pub fn provider_limits(mut self, provider: &str, limits: ProviderLimits) -> Self {
        self.limits.insert(provider.to_owned(), limits);
        self
    }

    /// An application-wide redactor applied to every message and URL the client prints, on top
    /// of its own request-scoped masking. The app passes its secret-storage redaction here.
    #[must_use]
    pub fn redactor(mut self, redactor: Redactor) -> Self {
        self.redactor = Some(redactor);
        self
    }

    /// Replaces how the client waits out a 429 (tests record the wait instead of sleeping).
    #[must_use]
    pub fn pause(mut self, pause: Arc<dyn Pause>) -> Self {
        self.pause = pause;
        self
    }

    /// Calls `observer` whenever a lane is paused by a 429.
    #[must_use]
    pub fn on_throttle(
        mut self,
        observer: impl Fn(&ThrottleEvent) + Send + Sync + 'static,
    ) -> Self {
        self.observer = Some(Arc::new(observer));
        self
    }

    /// Builds the client.
    pub fn build(self) -> Result<HttpClient, ProviderError> {
        let http = reqwest::Client::builder()
            .user_agent(self.user_agent)
            .timeout(self.timeout)
            .connect_timeout(self.connect_timeout)
            .build()
            .map_err(|err| {
                ProviderError::Other(format!("could not start the HTTP client: {err}"))
            })?;
        Ok(HttpClient {
            inner: Arc::new(Inner {
                http,
                cache: self.cache_dir.map(DiskCache::new),
                policy: RwLock::new(self.cache_policy),
                max_body: self.max_body,
                retry: self.retry,
                default_limits: self.default_limits,
                limits: RwLock::new(self.limits),
                lanes: Mutex::new(HashMap::new()),
                inflight: Mutex::new(HashMap::new()),
                human_checks: Mutex::new(HashMap::new()),
                redactor: self.redactor,
                pause: self.pause,
                observer: self.observer,
            }),
        })
    }
}

struct Inner {
    http: reqwest::Client,
    cache: Option<DiskCache>,
    policy: RwLock<CachePolicy>,
    max_body: Option<usize>,
    retry: RetryPolicy,
    default_limits: ProviderLimits,
    limits: RwLock<HashMap<String, ProviderLimits>>,
    lanes: Mutex<HashMap<String, Arc<Lane>>>,
    inflight: Mutex<HashMap<String, Shared>>,
    /// When each site that asked for a human check may be asked again.
    human_checks: Mutex<HashMap<String, Instant>>,
    redactor: Option<Redactor>,
    pause: Arc<dyn Pause>,
    observer: Option<ThrottleObserver>,
}

/// The one HTTP client every provider uses. Cloning is cheap and shares everything.
///
/// For each request it:
///
/// 1. answers from the disk cache when a fresh entry exists (unless the request bypasses it);
/// 2. otherwise joins an identical request already in flight, so concurrent duplicates cost one
///    network call;
/// 3. waits for the provider's lane (concurrency slot, then request rate) and sends it;
/// 4. on HTTP 429, pauses the lane for `Retry-After` (at most 30 seconds) and retries, up to 3
///    times, then fails with [`ProviderError::RateLimited`];
/// 5. stores a 2xx answer in the cache for its kind's forced TTL.
///
/// Credentials never leave the request: they are not hashed, cached, logged or included in
/// error messages.
#[derive(Clone)]
pub struct HttpClient {
    inner: Arc<Inner>,
}

/// How long a site that asked for a human check is left alone: nothing a program does passes the
/// check, so asking on every search only fills the interface with the same failure.
pub const HUMAN_CHECK_PAUSE: Duration = Duration::from_secs(600);

impl HttpClient {
    /// Remembers that `provider`'s site asked for a human check, so [`Self::human_check_pause`]
    /// says to leave it alone for [`HUMAN_CHECK_PAUSE`].
    pub fn note_human_check(&self, provider: &str) {
        self.inner
            .human_checks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(provider.to_owned(), Instant::now() + HUMAN_CHECK_PAUSE);
    }

    /// Whether `provider`'s site asked for a human check recently, so it should not be asked yet.
    #[must_use]
    pub fn human_check_pause(&self, provider: &str) -> bool {
        let mut checks = self
            .inner
            .human_checks
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        match checks.get(provider) {
            Some(until) if *until > Instant::now() => true,
            Some(_) => {
                checks.remove(provider);
                false
            }
            None => false,
        }
    }

    /// Starts a builder.
    #[must_use]
    pub fn builder() -> HttpClientBuilder {
        HttpClientBuilder::new()
    }

    /// Sends a request. See the type documentation for the steps.
    pub async fn send(&self, request: HttpRequest) -> Result<HttpResponse, ProviderError> {
        let prepared = request.prepare(self.inner.redactor.clone())?;

        if request.cacheable()
            && !request.bypass_cache
            && let Some(hit) = self.cache_lookup(&request, &prepared).await
        {
            tracing::debug!(
                provider = %request.provider,
                url = %prepared.display_url,
                "provider response served from cache"
            );
            return Ok(hit);
        }

        if request.method != HttpMethod::Get {
            return self.fetch(&request, &prepared).await;
        }

        // Identical concurrent requests share one fetch. If the caller doing the work is
        // cancelled, another waiter takes over.
        let cell = {
            let mut inflight = self
                .inner
                .inflight
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            Arc::clone(inflight.entry(prepared.key.clone()).or_default())
        };
        let result = cell
            .get_or_init(|| self.fetch(&request, &prepared))
            .await
            .clone();
        {
            let mut inflight = self
                .inner
                .inflight
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if inflight
                .get(&prepared.key)
                .is_some_and(|current| Arc::ptr_eq(current, &cell))
            {
                inflight.remove(&prepared.key);
            }
        }
        result
    }

    /// What a provider's lane is doing right now.
    #[must_use]
    pub fn lane_status(&self, provider: &str) -> LaneStatus {
        self.lane(provider).status()
    }

    /// Replaces the limits of one provider. Requests already running finish under the old ones.
    pub fn set_provider_limits(&self, provider: &str, limits: ProviderLimits) {
        self.inner
            .limits
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(provider.to_owned(), limits);
        self.inner
            .lanes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(provider);
    }

    /// Changes how long cached responses live (a Settings change). It applies to entries already
    /// on disk too.
    pub fn set_cache_policy(&self, policy: CachePolicy) {
        *self.inner.policy.write().unwrap_or_else(|e| e.into_inner()) = policy;
    }

    /// The current cache policy.
    #[must_use]
    pub fn cache_policy(&self) -> CachePolicy {
        *self.inner.policy.read().unwrap_or_else(|e| e.into_inner())
    }

    /// The on-disk cache, if the client has one (for clearing it from Settings).
    #[must_use]
    pub fn cache(&self) -> Option<&DiskCache> {
        self.inner.cache.as_ref()
    }

    /// Drops the cached response of `request`, if there is one. A provider calls this when a
    /// response turned out to be unusable (for example a page it could not read), so the bad
    /// answer is not served again until it expires.
    pub async fn forget(&self, request: &HttpRequest) {
        let (Some(cache), Ok(prepared)) = (
            &self.inner.cache,
            request.prepare(self.inner.redactor.clone()),
        ) else {
            return;
        };
        if let Err(err) = cache.remove(&request.provider, &prepared.key).await {
            tracing::warn!(
                provider = %request.provider,
                error = %prepared.masker.text(&err.to_string()),
                "could not remove a cached response"
            );
        }
    }

    /// Removes expired cache entries; returns how many were removed.
    pub async fn purge_expired_cache(&self) -> usize {
        match &self.inner.cache {
            Some(cache) => {
                cache
                    .purge_expired(&self.cache_policy(), SystemTime::now())
                    .await
            }
            None => 0,
        }
    }

    fn lane(&self, provider: &str) -> Arc<Lane> {
        let mut lanes = self.inner.lanes.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(lane) = lanes.get(provider) {
            return Arc::clone(lane);
        }
        let limits = self
            .inner
            .limits
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(provider)
            .cloned()
            .unwrap_or_else(|| self.inner.default_limits.clone());
        let lane = Arc::new(Lane::new(provider, &limits));
        lanes.insert(provider.to_owned(), Arc::clone(&lane));
        lane
    }

    async fn cache_lookup(
        &self,
        request: &HttpRequest,
        prepared: &Prepared,
    ) -> Option<HttpResponse> {
        let cache = self.inner.cache.as_ref()?;
        let policy = self.cache_policy();
        let hit = cache
            .get(&request.provider, &prepared.key, &policy, SystemTime::now())
            .await?;
        Some(HttpResponse {
            status: hit.status,
            content_type: hit.content_type,
            body: hit.body.into(),
            from_cache: true,
            etag: None,
            location: None,
        })
    }

    /// The network path: lane admission, the send, 429 retries and the cache write.
    async fn fetch(
        &self,
        request: &HttpRequest,
        prepared: &Prepared,
    ) -> Result<HttpResponse, ProviderError> {
        let lane = self.lane(&request.provider);
        let retry = self.inner.retry;
        let mut retries = 0u32;
        let mut already_waited = false;

        loop {
            if !std::mem::take(&mut already_waited)
                && let Some(remaining) = lane.remaining_throttle()
            {
                self.inner.pause.pause(remaining).await;
            }

            let raw = {
                let _slot = lane.admit().await;
                self.send_once(request, prepared, &lane.display_name)
                    .await?
            };
            tracing::debug!(
                provider = %request.provider,
                url = %prepared.display_url,
                status = raw.status,
                "provider request finished"
            );

            if raw.status != 429 {
                let response = HttpResponse {
                    status: raw.status,
                    content_type: raw.content_type,
                    body: raw.body.into(),
                    from_cache: false,
                    etag: raw.etag,
                    location: raw.location,
                };
                if response.is_success() && request.cacheable() {
                    self.cache_store(request, prepared, &response).await;
                }
                return Ok(response);
            }

            let asked = raw
                .retry_after
                .as_deref()
                .and_then(|value| parse_retry_after(value, SystemTime::now()));
            let wait = retry.wait_for(retries, asked);
            if retries >= retry.max_retries {
                let shown = asked.unwrap_or(wait);
                tracing::warn!(
                    provider = %request.provider,
                    url = %prepared.display_url,
                    retries,
                    "provider still rate limiting after the last retry"
                );
                return Err(ProviderError::RateLimited {
                    provider: lane.display_name.clone(),
                    retry_after_secs: u32::try_from(shown.as_secs()).unwrap_or(u32::MAX),
                });
            }

            retries += 1;
            lane.throttle(wait);
            let event = ThrottleEvent {
                provider: request.provider.clone(),
                wait,
                retry: retries,
            };
            if let Some(observer) = &self.inner.observer {
                observer(&event);
            }
            tracing::warn!(
                provider = %request.provider,
                url = %prepared.display_url,
                wait_secs = wait.as_secs_f32(),
                retry = retries,
                "provider rate limited the request; retrying after the wait"
            );
            self.inner.pause.pause(wait).await;
            already_waited = true;
        }
    }

    async fn send_once(
        &self,
        request: &HttpRequest,
        prepared: &Prepared,
        provider_name: &str,
    ) -> Result<RawResponse, ProviderError> {
        let fail = |message: String| ProviderError::Request {
            provider: provider_name.to_owned(),
            message: prepared.masker.text(&message),
        };

        let mut builder = match request.method {
            HttpMethod::Get => self.inner.http.get(prepared.send_url.clone()),
            HttpMethod::Post => self.inner.http.post(prepared.send_url.clone()),
        };
        for (name, value) in &prepared.headers {
            builder = builder.header(name.clone(), value.clone());
        }
        if let Some(body) = &request.body {
            builder = builder.body(body.clone());
        }
        if let Some(timeout) = request.timeout {
            builder = builder.timeout(timeout);
        }

        let response = builder.send().await.map_err(|err| fail(describe(err)))?;
        let status = response.status().as_u16();
        let header = |name: &str| {
            response
                .headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned)
        };
        let content_type = header("content-type");
        let retry_after = header("retry-after");
        let etag = header("etag");
        let location = header("location");
        let body = self
            .read_body(response, request.max_body.or(self.inner.max_body), &fail)
            .await?;
        Ok(RawResponse {
            status,
            content_type,
            retry_after,
            etag,
            location,
            body,
        })
    }

    /// Reads the body, stopping with an error once it passes the size limit.
    async fn read_body(
        &self,
        mut response: reqwest::Response,
        max: Option<usize>,
        fail: &(dyn Fn(String) -> ProviderError + Sync),
    ) -> Result<Vec<u8>, ProviderError> {
        let Some(max) = max else {
            return response
                .bytes()
                .await
                .map(|bytes| bytes.to_vec())
                .map_err(|err| fail(describe(err)));
        };
        let too_large = || fail("the answer is too large".to_owned());
        if response
            .content_length()
            .is_some_and(|length| length > max as u64)
        {
            return Err(too_large());
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|err| fail(describe(err)))? {
            if body.len() + chunk.len() > max {
                return Err(too_large());
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }

    async fn cache_store(
        &self,
        request: &HttpRequest,
        prepared: &Prepared,
        response: &HttpResponse,
    ) {
        let Some(cache) = &self.inner.cache else {
            return;
        };
        if self.cache_policy().ttl(request.cache).is_none() {
            return;
        }
        let entry = NewEntry {
            kind: request.cache,
            status: response.status,
            content_type: response.content_type.as_deref(),
            redacted_url: &prepared.display_url,
            body: &response.body,
        };
        if let Err(err) = cache
            .put(&request.provider, &prepared.key, &entry, SystemTime::now())
            .await
        {
            tracing::warn!(
                provider = %request.provider,
                error = %prepared.masker.text(&err.to_string()),
                "could not write the response cache"
            );
        }
    }
}

struct RawResponse {
    status: u16,
    content_type: Option<String>,
    retry_after: Option<String>,
    etag: Option<String>,
    location: Option<String>,
    body: Vec<u8>,
}

/// A plain-language description of a transport error, without the URL `reqwest` would add.
fn describe(err: reqwest::Error) -> String {
    let lead = if err.is_timeout() {
        "the request timed out"
    } else if err.is_connect() {
        "could not connect"
    } else {
        "the request failed"
    };
    let err = err.without_url();
    let mut message = lead.to_owned();
    let mut source = std::error::Error::source(&err);
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    message
}

impl std::fmt::Debug for HttpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpClient")
            .field(
                "cache",
                &self.inner.cache.as_ref().map(|c| c.root().to_owned()),
            )
            .field("retry", &self.inner.retry)
            .finish_non_exhaustive()
    }
}
