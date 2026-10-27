//! One process-wide lock per data source, shared by rescans and by the work that moves files in a
//! source (organize, its undo, a change of data source).
//!
//! A rescan reads the disk first and the library after. Work that moves a show folder and points
//! the library at the new place in the meantime makes the two disagree: the walk found the folder
//! under its old name, the library has the new one (or the other way round), and the show is
//! added a second time. Holding the source while the files move keeps a rescan wholly before or
//! wholly after them.
//!
//! [`hold_sources`] takes every source it is given at once, so two holders that want overlapping
//! sources never wait on each other in a circle.

use std::collections::HashSet;
use std::sync::{Condvar, LazyLock, Mutex, MutexGuard};

struct Held {
    ids: Mutex<HashSet<i64>>,
    released: Condvar,
}

static HELD: LazyLock<Held> = LazyLock::new(|| Held {
    ids: Mutex::new(HashSet::new()),
    released: Condvar::new(),
});

fn held() -> MutexGuard<'static, HashSet<i64>> {
    HELD.ids
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The data sources held by one caller; they are released when it is dropped.
#[derive(Debug)]
#[must_use = "the sources are released as soon as the guard is dropped"]
pub struct SourceGuard {
    ids: Vec<i64>,
}

impl SourceGuard {
    /// The data source ids this guard holds.
    pub fn ids(&self) -> &[i64] {
        &self.ids
    }
}

impl Drop for SourceGuard {
    fn drop(&mut self) {
        let mut ids = held();
        for id in &self.ids {
            ids.remove(id);
        }
        drop(ids);
        HELD.released.notify_all();
    }
}

/// Waits until none of `ids` is held by anyone else, then holds them all. Not re-entrant: a
/// caller that holds a source must not ask for it again.
pub fn hold_sources(ids: impl IntoIterator<Item = i64>) -> SourceGuard {
    let mut wanted: Vec<i64> = ids.into_iter().collect();
    wanted.sort_unstable();
    wanted.dedup();
    let mut current = held();
    while wanted.iter().any(|id| current.contains(id)) {
        current = HELD
            .released
            .wait(current)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
    }
    current.extend(wanted.iter().copied());
    SourceGuard { ids: wanted }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    use super::*;

    // Ids far from the ones other tests of this process use.
    const A: i64 = 9_000_001;
    const B: i64 = 9_000_002;

    #[test]
    fn a_held_source_waits_until_released() {
        let guard = hold_sources([A, B]);
        let entered = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&entered);
        let waiter = std::thread::spawn(move || {
            let _again = hold_sources([B]);
            flag.store(true, Ordering::SeqCst);
        });
        std::thread::sleep(Duration::from_millis(100));
        assert!(!entered.load(Ordering::SeqCst), "B is still held");
        drop(guard);
        waiter.join().unwrap();
        assert!(entered.load(Ordering::SeqCst));
    }

    #[test]
    fn other_sources_are_not_held_back() {
        let _a = hold_sources([A + 10]);
        let b = hold_sources([B + 10]);
        assert_eq!(b.ids(), &[B + 10]);
    }
}
