//! A bounded, replaying fan-out.
//!
//! Job progress wants a live feed with a snapshot on connect; notices want the last few messages
//! replayed to every new subscriber, so a second tab does not miss what happened while the first
//! one was open. [`Hub`] supports both: [`Hub::subscribe`] replays the ring, [`Hub::subscribe_live`]
//! starts at the next message. A subscriber that returns `false` is forgotten.

use std::collections::VecDeque;
use std::sync::{Mutex, MutexGuard, PoisonError};

type Sink<T> = Box<dyn FnMut(&T) -> bool + Send>;

struct Inner<T> {
    ring: VecDeque<T>,
    sinks: Vec<Sink<T>>,
}

/// Fans values out to subscribers and keeps the newest `capacity` for replay.
pub struct Hub<T> {
    inner: Mutex<Inner<T>>,
    capacity: usize,
}

impl<T> Hub<T> {
    /// A hub that keeps at most `capacity` values (at least one).
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Mutex::new(Inner {
                ring: VecDeque::new(),
                sinks: Vec::new(),
            }),
            capacity: capacity.max(1),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner<T>> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl<T: Clone + Send + 'static> Hub<T> {
    /// Sends the value to every live subscriber and keeps it for replay.
    pub fn publish(&self, value: T) {
        let mut inner = self.lock();
        inner.sinks.retain_mut(|sink| sink(&value));
        if inner.ring.len() == self.capacity {
            inner.ring.pop_front();
        }
        inner.ring.push_back(value);
    }

    /// Adds a subscriber and first replays the kept values, oldest first.
    pub fn subscribe(&self, mut sink: impl FnMut(&T) -> bool + Send + 'static) {
        let mut inner = self.lock();
        for value in &inner.ring {
            if !sink(value) {
                return;
            }
        }
        inner.sinks.push(Box::new(sink));
    }

    /// Adds a subscriber that starts at the next value (a fresh page gets a snapshot instead).
    pub fn subscribe_live(&self, sink: impl FnMut(&T) -> bool + Send + 'static) {
        self.lock().sinks.push(Box::new(sink));
    }

    /// The kept values, oldest first.
    #[must_use]
    pub fn replay(&self) -> Vec<T> {
        self.lock().ring.iter().cloned().collect()
    }

    /// How many subscribers are live.
    #[must_use]
    pub fn subscribers(&self) -> usize {
        self.lock().sinks.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn a_new_subscriber_replays_the_ring() {
        let hub = Hub::new(4);
        hub.publish(1);
        hub.publish(2);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        hub.subscribe(move |value| {
            sink.lock().unwrap().push(*value);
            true
        });
        assert_eq!(*seen.lock().unwrap(), vec![1, 2]);
    }

    #[test]
    fn a_live_subscriber_starts_at_the_next_value() {
        let hub = Hub::new(4);
        hub.publish(1);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        hub.subscribe_live(move |value| {
            sink.lock().unwrap().push(*value);
            true
        });
        hub.publish(2);
        assert_eq!(*seen.lock().unwrap(), vec![2]);
    }

    #[test]
    fn the_ring_drops_the_oldest_value() {
        let hub = Hub::new(2);
        hub.publish(1);
        hub.publish(2);
        hub.publish(3);
        assert_eq!(hub.replay(), vec![2, 3]);
    }

    #[test]
    fn a_subscriber_that_returns_false_is_forgotten() {
        let hub = Hub::new(1);
        let calls = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&calls);
        hub.subscribe_live(move |_| {
            count.fetch_add(1, Ordering::SeqCst);
            false
        });
        hub.publish(1);
        hub.publish(2);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(hub.subscribers(), 0);
    }
}
