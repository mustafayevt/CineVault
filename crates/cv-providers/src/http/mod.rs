//! The shared HTTP client for all providers.
//!
//! * [`HttpClient`] sends [`HttpRequest`]s and returns [`HttpResponse`]s.
//! * [`ProviderLimits`] sets a provider's concurrency and request rate; each provider gets its own
//!   lane, so a slow or throttled provider never holds up another.
//! * [`RetryPolicy`] covers HTTP 429: up to 3 retries, each waiting `Retry-After` but at most 30
//!   seconds.
//! * [`DiskCache`] and [`CachePolicy`] keep responses for a forced time: 15 minutes for API
//!   responses and one day for scraped HTML by default.
//! * Identical concurrent GETs are coalesced into one network call.
//! * Credentials are passed apart from the rest of a request and never reach cache files, cache
//!   keys, logs or error messages. See [`redact`].

mod cache;
mod client;
mod lane;
pub mod redact;
mod request;
mod response;
mod retry;

pub use cache::{CacheKind, CachePolicy, DiskCache, DiskUsage};
pub use client::{DEFAULT_MAX_BODY_BYTES, HttpClient, HttpClientBuilder, ThrottleEvent};
pub use lane::{LaneStatus, ProviderLimits};
pub use redact::{MASK, Redactor};
pub use request::{HttpMethod, HttpRequest};
pub use response::HttpResponse;
pub use retry::{Pause, RetryPolicy, TokioPause, parse_retry_after};
