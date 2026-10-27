//! A tiny ordered parallel map on scoped threads, used to read folder trees and group them
//! without a thread-pool dependency.

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

/// The number of worker threads to use when the caller does not say: the available cores, at
/// most 16 (a spinning disk or a network share gains nothing from more).
pub(crate) fn default_threads() -> usize {
    thread::available_parallelism()
        .map_or(4, |n| n.get())
        .min(16)
}

/// Runs `f` over `items` on up to `workers` threads and returns the results in item order.
///
/// With one worker or one item nothing is spawned. A panic in `f` propagates to the caller after
/// the other workers finish.
pub(crate) fn par_map<T, R, F>(items: &[T], workers: usize, f: F) -> Vec<R>
where
    T: Sync,
    R: Send,
    F: Fn(&T) -> R + Sync,
{
    let workers = workers.min(items.len());
    if workers <= 1 {
        return items.iter().map(f).collect();
    }
    let next = AtomicUsize::new(0);
    let slots: Vec<Mutex<Option<R>>> = items.iter().map(|_| Mutex::new(None)).collect();
    thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(item) = items.get(i) else { break };
                    let value = f(item);
                    *slots[i]
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(value);
                }
            });
        }
    });
    slots
        .into_iter()
        .map(|slot| {
            slot.into_inner()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .expect("every slot is filled once the workers have finished")
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn results_keep_the_item_order() {
        let items: Vec<u32> = (0..200).collect();
        let doubled = par_map(&items, 8, |n| n * 2);
        assert_eq!(doubled, items.iter().map(|n| n * 2).collect::<Vec<_>>());
    }

    #[test]
    fn one_worker_and_empty_input_work() {
        assert_eq!(par_map(&[1, 2, 3], 1, |n| n + 1), vec![2, 3, 4]);
        assert!(par_map(&[] as &[u8], 4, |n| *n).is_empty());
    }
}
