//! Rename after Scrape: the titles a scrape was written into, gathered and renamed together.
//!
//! With `renamer.after-scrape` on (and "Metadata only" off), every title whose after-scrape steps
//! have finished joins a queue. Once no title has joined for [`QUIET`], the queue is handed to the
//! runner as one list, so a scrape of fifty titles is one rename, one journal batch and one Undo,
//! not fifty. The runner is the batch engine's rename step (set by the app when its batch runner
//! starts): it renames only the titles whose plan is a plain change and holds back conflicts,
//! locked titles and titles whose video details are not read yet. Without a runner (the command
//! line), nothing is renamed after a scrape.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use cv_domain::ItemId;

/// How long the queue waits for more titles before it is renamed.
pub const QUIET: Duration = Duration::from_secs(3);

/// Renames a list of titles. Returns at once; the work runs in the background.
pub type RenameRunner = Arc<dyn Fn(Vec<ItemId>) + Send + Sync>;

#[derive(Default)]
struct State {
    pending: BTreeSet<ItemId>,
    /// Bumped by every title that joins, so the waiting task sees whether the queue was quiet.
    generation: u64,
    /// Whether a task is waiting to hand the queue over.
    scheduled: bool,
}

/// The titles waiting to be renamed after their scrape.
pub struct RenameQueue {
    state: Mutex<State>,
    runner: Mutex<Option<RenameRunner>>,
    quiet: Duration,
}

impl Default for RenameQueue {
    fn default() -> Self {
        Self::with_quiet(QUIET)
    }
}

impl RenameQueue {
    /// A queue that waits `quiet` for more titles (tests use a short wait).
    #[must_use]
    pub fn with_quiet(quiet: Duration) -> Self {
        Self {
            state: Mutex::new(State::default()),
            runner: Mutex::new(None),
            quiet,
        }
    }

    /// Sets who renames the titles. A later runner replaces an earlier one.
    pub fn set_runner(&self, runner: RenameRunner) {
        *self.runner.lock().unwrap_or_else(PoisonError::into_inner) = Some(runner);
    }

    /// Adds a title. The queue is handed to the runner once no title has joined for the quiet
    /// time.
    pub fn push(self: &Arc<Self>, item: ItemId) {
        let start = {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            state.pending.insert(item);
            state.generation += 1;
            !std::mem::replace(&mut state.scheduled, true)
        };
        if start {
            let queue = Arc::clone(self);
            cv_jobs::spawn(async move { queue.wait_and_run().await });
        }
    }

    async fn wait_and_run(&self) {
        let mut seen = self.generation();
        loop {
            tokio::time::sleep(self.quiet).await;
            let now = self.generation();
            if now == seen {
                break;
            }
            seen = now;
        }
        let ids: Vec<ItemId> = {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            state.scheduled = false;
            std::mem::take(&mut state.pending).into_iter().collect()
        };
        let runner = self
            .runner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Some(runner) = runner
            && !ids.is_empty()
        {
            runner(ids);
        }
    }

    fn generation(&self) -> u64 {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .generation
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recording() -> (RenameRunner, Arc<Mutex<Vec<Vec<ItemId>>>>) {
        let calls: Arc<Mutex<Vec<Vec<ItemId>>>> = Arc::default();
        let sink = Arc::clone(&calls);
        let runner: RenameRunner = Arc::new(move |ids| sink.lock().unwrap().push(ids));
        (runner, calls)
    }

    #[tokio::test(start_paused = true)]
    async fn titles_scraped_close_together_are_renamed_as_one_list() {
        let queue = Arc::new(RenameQueue::with_quiet(Duration::from_secs(3)));
        let (runner, calls) = recording();
        queue.set_runner(runner);
        queue.push(ItemId(1));
        tokio::time::sleep(Duration::from_secs(2)).await;
        queue.push(ItemId(2));
        queue.push(ItemId(1));
        tokio::time::sleep(Duration::from_secs(2)).await;
        assert!(calls.lock().unwrap().is_empty(), "still waiting for quiet");
        tokio::time::sleep(Duration::from_secs(3)).await;
        assert_eq!(*calls.lock().unwrap(), vec![vec![ItemId(1), ItemId(2)]]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_title_after_the_hand_over_starts_a_new_list() {
        let queue = Arc::new(RenameQueue::with_quiet(Duration::from_secs(1)));
        let (runner, calls) = recording();
        queue.set_runner(runner);
        queue.push(ItemId(1));
        tokio::time::sleep(Duration::from_secs(5)).await;
        queue.push(ItemId(7));
        tokio::time::sleep(Duration::from_secs(5)).await;
        assert_eq!(
            *calls.lock().unwrap(),
            vec![vec![ItemId(1)], vec![ItemId(7)]]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn without_a_runner_the_titles_are_dropped() {
        let queue = Arc::new(RenameQueue::with_quiet(Duration::from_secs(1)));
        queue.push(ItemId(1));
        tokio::time::sleep(Duration::from_secs(5)).await;
        let (runner, calls) = recording();
        queue.set_runner(runner);
        tokio::time::sleep(Duration::from_secs(5)).await;
        assert!(calls.lock().unwrap().is_empty());
    }
}
