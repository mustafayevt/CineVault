//! Which browser session started a job.
//!
//! The desktop has one window; the server can have several browsers at once. The server runs each
//! command inside [`scope`], so the job specs the command submits carry its client. A job
//! submitted outside a command (a resumed job, a background sweep) has no client.
//!
//! The client is a tokio task-local, not a thread-local: a command is async and resumes on any
//! worker thread after an `.await`, so a thread-local set at its start is gone (or belongs to
//! another session's command) by the time the job is submitted. Work a command hands to a
//! blocking thread keeps the client through [`enter`], which [`current`] reads when no task-local
//! is set.

use std::cell::RefCell;
use std::future::Future;

tokio::task_local! {
    static TASK: String;
}

thread_local! {
    static CURRENT: RefCell<String> = const { RefCell::new(String::new()) };
}

/// The client of the command running in this task (or on this blocking thread), or empty.
#[must_use]
pub fn current() -> String {
    TASK.try_with(Clone::clone)
        .unwrap_or_else(|_| CURRENT.with(|current| current.borrow().clone()))
}

/// Runs `work` with `client` as the client of every job it submits, across `.await` points and
/// worker threads.
pub async fn scope<F: Future>(client: String, work: F) -> F::Output {
    TASK.scope(client, work).await
}

/// Sets the client on this thread for the life of the returned guard, and clears it on drop.
/// For synchronous work on a blocking thread; async code uses [`scope`].
pub fn enter(client: &str) -> Scope {
    CURRENT.with(|current| *current.borrow_mut() = client.to_owned());
    Scope
}

/// `tokio::task::spawn_blocking` that keeps the caller's client on the blocking thread, so a job
/// submitted from there is still attributed to the session that asked for it.
pub fn spawn_blocking<F, R>(work: F) -> tokio::task::JoinHandle<R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    let client = current();
    tokio::task::spawn_blocking(move || {
        let _client = enter(&client);
        work()
    })
}

/// Clears the client when the work ends, including on an early return.
pub struct Scope;

impl Drop for Scope {
    fn drop(&mut self) {
        CURRENT.with(|current| current.borrow_mut().clear());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn the_client_survives_awaits_on_other_threads() {
        let seen = scope("tab-a".to_owned(), async {
            let mut seen = Vec::new();
            for _ in 0..20 {
                tokio::task::yield_now().await;
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                seen.push(current());
            }
            seen
        })
        .await;
        assert!(seen.iter().all(|client| client == "tab-a"), "{seen:?}");
        assert_eq!(current(), "");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn two_commands_at_once_keep_their_own_clients() {
        let a = tokio::spawn(scope("tab-a".to_owned(), async {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            current()
        }));
        let b = tokio::spawn(scope("tab-b".to_owned(), async {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            current()
        }));
        assert_eq!(a.await.unwrap(), "tab-a");
        assert_eq!(b.await.unwrap(), "tab-b");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn blocking_work_keeps_the_client() {
        let seen = scope("tab-d".to_owned(), async {
            spawn_blocking(current).await.unwrap()
        })
        .await;
        assert_eq!(seen, "tab-d");
    }

    #[test]
    fn a_blocking_thread_uses_the_entered_client() {
        let _scope = enter("tab-c");
        assert_eq!(current(), "tab-c");
    }
}
