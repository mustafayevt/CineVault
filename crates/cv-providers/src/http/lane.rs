//! One lane per provider: a concurrency limit, a request-rate limit and a throttle window.

use std::num::NonZeroU32;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use governor::{DefaultDirectRateLimiter, Quota};
use tokio::sync::{Semaphore, SemaphorePermit};

/// Limits for one provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderLimits {
    /// Name shown in messages; the provider id when `None`.
    pub name: Option<String>,
    /// Requests in flight at once.
    pub max_concurrent: usize,
    /// Requests allowed per `period`, sustained.
    pub requests: u32,
    /// The period `requests` is measured over.
    pub period: Duration,
    /// Requests allowed back to back before the sustained rate applies.
    pub burst: u32,
}

impl Default for ProviderLimits {
    /// Four at once, ten per second.
    fn default() -> Self {
        Self {
            name: None,
            max_concurrent: 4,
            requests: 10,
            period: Duration::from_secs(1),
            burst: 10,
        }
    }
}

impl ProviderLimits {
    /// One request at a time and no more than one per `interval` (for example AniDB: two
    /// seconds).
    #[must_use]
    pub fn one_per(interval: Duration) -> Self {
        Self {
            name: None,
            max_concurrent: 1,
            requests: 1,
            period: interval,
            burst: 1,
        }
    }

    /// `requests` per `period` with up to `max_concurrent` in flight and a burst of `requests`.
    #[must_use]
    pub fn new(max_concurrent: usize, requests: u32, period: Duration) -> Self {
        Self {
            name: None,
            max_concurrent,
            requests,
            period,
            burst: requests,
        }
    }

    /// Sets the name used in messages.
    #[must_use]
    pub fn named(mut self, name: &str) -> Self {
        self.name = Some(name.to_owned());
        self
    }

    fn quota(&self) -> Quota {
        let requests = self.requests.max(1);
        let burst = NonZeroU32::new(self.burst.max(1)).unwrap_or(NonZeroU32::MIN);
        let spacing = self.period / requests;
        Quota::with_period(spacing)
            .map(|q| q.allow_burst(burst))
            .unwrap_or_else(|| Quota::per_second(NonZeroU32::MAX))
    }
}

/// What a lane is doing, for the Activity drawer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneStatus {
    pub provider: String,
    pub in_flight: usize,
    pub max_concurrent: usize,
    /// How much longer the lane is paused after a 429, if it is.
    pub throttled_for: Option<Duration>,
}

pub(crate) struct Lane {
    pub provider: String,
    pub display_name: String,
    max_concurrent: usize,
    semaphore: Semaphore,
    limiter: DefaultDirectRateLimiter,
    throttled_until: Mutex<Option<Instant>>,
    in_flight: AtomicUsize,
}

/// Holds a concurrency slot; releases it on drop.
pub(crate) struct Slot<'a> {
    lane: &'a Lane,
    _permit: SemaphorePermit<'a>,
}

impl Drop for Slot<'_> {
    fn drop(&mut self) {
        self.lane.in_flight.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Lane {
    pub(crate) fn new(provider: &str, limits: &ProviderLimits) -> Self {
        let max_concurrent = limits.max_concurrent.max(1);
        Self {
            provider: provider.to_owned(),
            display_name: limits.name.clone().unwrap_or_else(|| provider.to_owned()),
            max_concurrent,
            semaphore: Semaphore::new(max_concurrent),
            limiter: DefaultDirectRateLimiter::direct(limits.quota()),
            throttled_until: Mutex::new(None),
            in_flight: AtomicUsize::new(0),
        }
    }

    /// Waits for a concurrency slot and then for the rate limiter.
    pub(crate) async fn admit(&self) -> Slot<'_> {
        // The semaphore is never closed, so acquiring cannot fail.
        let permit = self
            .semaphore
            .acquire()
            .await
            .expect("lane semaphore is never closed");
        self.in_flight.fetch_add(1, Ordering::SeqCst);
        let slot = Slot {
            lane: self,
            _permit: permit,
        };
        self.limiter.until_ready().await;
        slot
    }

    /// Pauses the lane for `wait` from now (never shortening an existing pause).
    pub(crate) fn throttle(&self, wait: Duration) {
        let until = Instant::now() + wait;
        let mut guard = self
            .throttled_until
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if guard.is_none_or(|current| current < until) {
            *guard = Some(until);
        }
    }

    /// How long the lane is still paused.
    pub(crate) fn remaining_throttle(&self) -> Option<Duration> {
        let guard = self
            .throttled_until
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        guard
            .map(|until| until.saturating_duration_since(Instant::now()))
            .filter(|d| !d.is_zero())
    }

    pub(crate) fn status(&self) -> LaneStatus {
        LaneStatus {
            provider: self.provider.clone(),
            in_flight: self.in_flight.load(Ordering::SeqCst),
            max_concurrent: self.max_concurrent,
            throttled_for: self.remaining_throttle(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn spaces_requests_by_the_sustained_rate() {
        let lane = Lane::new(
            "anidb",
            &ProviderLimits::one_per(Duration::from_millis(150)),
        );
        let start = Instant::now();
        for _ in 0..3 {
            drop(lane.admit().await);
        }
        assert!(
            start.elapsed() >= Duration::from_millis(280),
            "three requests at one per 150 ms took {:?}",
            start.elapsed()
        );
    }

    #[tokio::test]
    async fn counts_in_flight_and_reports_throttle() {
        let lane = Lane::new("tmdb", &ProviderLimits::default());
        assert_eq!(lane.status().in_flight, 0);
        let slot = lane.admit().await;
        assert_eq!(lane.status().in_flight, 1);
        drop(slot);
        assert_eq!(lane.status().in_flight, 0);

        assert_eq!(lane.status().throttled_for, None);
        lane.throttle(Duration::from_secs(5));
        let left = lane.status().throttled_for.unwrap();
        assert!(left > Duration::from_secs(4) && left <= Duration::from_secs(5));
        lane.throttle(Duration::from_secs(1));
        assert!(lane.status().throttled_for.unwrap() > Duration::from_secs(4));
    }

    #[test]
    fn display_name_falls_back_to_the_id() {
        let plain = Lane::new("fanart", &ProviderLimits::default());
        assert_eq!(plain.display_name, "fanart");
        let named = Lane::new("fanart", &ProviderLimits::default().named("Fanart.tv"));
        assert_eq!(named.display_name, "Fanart.tv");
    }
}
