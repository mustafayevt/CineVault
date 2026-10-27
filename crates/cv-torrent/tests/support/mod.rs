//! Helpers shared by the integration tests of cv-torrent.

#![allow(dead_code)]

pub mod fake_server;
pub mod queries;

use std::time::Duration;

use cv_providers::http::{HttpClient, ProviderLimits};

/// A torrent HTTP client as the app builds it, but with limits loose enough that tests do not
/// wait a second between requests.
pub fn fast_client() -> HttpClient {
    cv_torrent::source::client_builder()
        .default_limits(ProviderLimits::new(8, 1000, Duration::from_secs(1)))
        .build()
        .expect("the HTTP client builds")
}
