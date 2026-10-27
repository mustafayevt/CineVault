//! What changed for which titles, so open pages refresh in place.
//!
//! The writes a page shows (ratings stored, artwork placed, an edit saved, a scrape applied, a
//! media info read, an organize applied or undone) publish an
//! [`ItemsChanged`] on the [`ChangeHub`]. The desktop shell turns each one into the typed
//! `ItemsChanged` event; the UI invalidates only the queries of the parts that changed.
//!
//! A burst of changes (a batch scrape, a media-info pass) is coalesced: publications of the same
//! parts inside a short window leave as one event with the union of their titles.
//!
//! The parts:
//!
//! | Part | Meaning |
//! |---|---|
//! | `Ratings` | the stored ratings of the titles |
//! | `Artwork` | artwork files or the artwork the title uses |
//! | `Details` | the fields of the title (an edit or a scrape) |
//! | `Files` | the files of the title: paths, media info, extras |
//! | `Stage` | the lifecycle: stage, flags and the explanation (also "fetching…") |

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use cv_domain::ItemId;

/// How long a burst of changes is collected before it is announced.
pub const DEFAULT_WINDOW: Duration = Duration::from_millis(150);

/// The most titles one event carries; a bigger burst is split.
const MAX_ITEMS_PER_EVENT: usize = 2_000;

/// Which part of a title changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ChangePart {
    Ratings,
    Artwork,
    Details,
    Files,
    Stage,
}

impl ChangePart {
    /// The stable text used in logs.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ratings => "ratings",
            Self::Artwork => "artwork",
            Self::Details => "details",
            Self::Files => "files",
            Self::Stage => "stage",
        }
    }
}

/// Titles whose parts changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemsChanged {
    pub items: Vec<ItemId>,
    pub parts: Vec<ChangePart>,
}

/// Called with every announced change, on the thread that flushes it.
pub type ChangeListener = Arc<dyn Fn(&ItemsChanged) + Send + Sync>;

/// Collects changes and announces them to the listeners.
pub struct ChangeHub {
    window: Duration,
    listeners: Mutex<Vec<ChangeListener>>,
    pending: Mutex<BTreeMap<Vec<ChangePart>, BTreeSet<ItemId>>>,
    scheduled: AtomicBool,
}

impl ChangeHub {
    /// A hub that collects changes for `window` before announcing them. A zero window announces
    /// each change at once, on the publishing thread.
    #[must_use]
    pub fn new(window: Duration) -> Arc<Self> {
        Arc::new(Self {
            window,
            listeners: Mutex::new(Vec::new()),
            pending: Mutex::new(BTreeMap::new()),
            scheduled: AtomicBool::new(false),
        })
    }

    /// Calls `listener` with every change that is announced from now on.
    pub fn subscribe(&self, listener: ChangeListener) {
        self.listeners
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(listener);
    }

    /// Notes that `parts` of `items` changed. Must be called inside a tokio runtime for the
    /// collecting window to apply; without one the change is announced at once.
    pub fn publish(self: &Arc<Self>, items: &[ItemId], parts: &[ChangePart]) {
        if items.is_empty() || parts.is_empty() {
            return;
        }
        let mut parts = parts.to_vec();
        parts.sort_unstable();
        parts.dedup();
        self.pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(parts)
            .or_default()
            .extend(items.iter().copied());
        let runtime = tokio::runtime::Handle::try_current();
        match runtime {
            Ok(runtime) if !self.window.is_zero() => {
                if self.scheduled.swap(true, Ordering::SeqCst) {
                    return;
                }
                let hub = Arc::clone(self);
                runtime.spawn(async move {
                    tokio::time::sleep(hub.window).await;
                    hub.scheduled.store(false, Ordering::SeqCst);
                    hub.flush();
                });
            }
            _ => self.flush(),
        }
    }

    /// Announces what is collected now.
    pub fn flush(&self) {
        let batch =
            std::mem::take(&mut *self.pending.lock().unwrap_or_else(PoisonError::into_inner));
        if batch.is_empty() {
            return;
        }
        let listeners: Vec<ChangeListener> = self
            .listeners
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        for (parts, items) in batch {
            let items: Vec<ItemId> = items.into_iter().collect();
            for chunk in items.chunks(MAX_ITEMS_PER_EVENT) {
                let change = ItemsChanged {
                    items: chunk.to_vec(),
                    parts: parts.clone(),
                };
                // A failing listener must not stop the others.
                for listener in &listeners {
                    crate::guard::contain("change listener", || listener(&change));
                }
            }
        }
    }
}

/// A service's way to announce changes: a handle that does nothing until the hub is set, so a
/// service built without one (a test, the command line) works unchanged.
#[derive(Clone, Default)]
pub struct Changes(Arc<OnceLock<Arc<ChangeHub>>>);

impl Changes {
    /// Connects the service to the hub. Only the first call counts.
    pub fn set(&self, hub: Arc<ChangeHub>) {
        let _ = self.0.set(hub);
    }

    /// Announces that `parts` of `items` changed.
    pub fn publish(&self, items: &[ItemId], parts: &[ChangePart]) {
        if let Some(hub) = self.0.get() {
            hub.publish(items, parts);
        }
    }

    /// Announces that `parts` of one title changed.
    pub fn publish_one(&self, item: ItemId, parts: &[ChangePart]) {
        self.publish(&[item], parts);
    }
}

impl std::fmt::Debug for Changes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Changes")
            .field("connected", &self.0.get().is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use super::*;

    fn collect(hub: &ChangeHub) -> Arc<Mutex<Vec<ItemsChanged>>> {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        hub.subscribe(Arc::new(move |change| {
            sink.lock().unwrap().push(change.clone());
        }));
        seen
    }

    #[test]
    fn a_zero_window_announces_each_change_at_once() {
        let hub = ChangeHub::new(Duration::ZERO);
        let seen = collect(&hub);
        hub.publish(&[ItemId(2), ItemId(1)], &[ChangePart::Ratings]);
        hub.publish(&[ItemId(1)], &[ChangePart::Stage, ChangePart::Ratings]);
        let seen = seen.lock().unwrap();
        assert_eq!(
            *seen,
            vec![
                ItemsChanged {
                    items: vec![ItemId(1), ItemId(2)],
                    parts: vec![ChangePart::Ratings],
                },
                ItemsChanged {
                    items: vec![ItemId(1)],
                    parts: vec![ChangePart::Ratings, ChangePart::Stage],
                },
            ]
        );
    }

    #[test]
    fn nothing_is_announced_for_no_titles_or_no_parts() {
        let hub = ChangeHub::new(Duration::ZERO);
        let seen = collect(&hub);
        hub.publish(&[], &[ChangePart::Details]);
        hub.publish(&[ItemId(1)], &[]);
        assert!(seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_burst_inside_the_window_leaves_as_one_event_per_set_of_parts() {
        let hub = ChangeHub::new(Duration::from_millis(40));
        let seen = collect(&hub);
        for id in 1..=50 {
            hub.publish(&[ItemId(id)], &[ChangePart::Details, ChangePart::Stage]);
        }
        hub.publish(&[ItemId(7)], &[ChangePart::Ratings]);
        assert!(seen.lock().unwrap().is_empty(), "nothing leaves early");
        tokio::time::sleep(Duration::from_millis(150)).await;
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        let details = seen
            .iter()
            .find(|c| c.parts == [ChangePart::Details, ChangePart::Stage])
            .expect("details event");
        assert_eq!(details.items.len(), 50);
    }

    #[test]
    fn an_unconnected_handle_does_nothing_and_a_connected_one_publishes() {
        let changes = Changes::default();
        changes.publish_one(ItemId(1), &[ChangePart::Files]);
        let hub = ChangeHub::new(Duration::ZERO);
        let count = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&count);
        hub.subscribe(Arc::new(move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
        }));
        let copy = changes.clone();
        changes.set(Arc::clone(&hub));
        copy.publish_one(ItemId(1), &[ChangePart::Files]);
        assert_eq!(count.load(Ordering::SeqCst), 1, "a clone shares the hub");
    }
}
