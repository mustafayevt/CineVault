//! Where background tasks start when the caller is not inside a tokio runtime.
//!
//! `tokio::spawn` panics on a thread without a runtime. The desktop app runs its synchronous
//! commands and event callbacks on the window thread, which has none, and a panic there aborts
//! the whole app. So everything in this crate that starts a task goes through [`spawn_on`]: the
//! caller's runtime when there is one, else the runtime the [`Lanes`](crate::Lanes) were made in,
//! else the one the app registered with [`set_fallback_runtime`], else a small runtime of this
//! crate's own, built on first use.

use std::future::Future;
use std::sync::OnceLock;

use tokio::runtime::{Builder, Handle, Runtime};
use tokio::task::JoinHandle;

static FALLBACK: OnceLock<Handle> = OnceLock::new();
static OWN: OnceLock<Runtime> = OnceLock::new();

/// Makes `handle` the runtime that tasks started off a runtime run on. The app calls this once at
/// start with its main runtime. The first call wins; returns false when one was already set.
pub fn set_fallback_runtime(handle: Handle) -> bool {
    FALLBACK.set(handle).is_ok()
}

/// The runtime a task should start on, from a caller that may not be inside one.
pub fn runtime_handle() -> Handle {
    resolve(None)
}

/// Starts `future` as a task, inside or outside a tokio runtime. See the module notes for where
/// it runs.
pub fn spawn<F>(future: F) -> JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    resolve(None).spawn(future)
}

/// Like [`spawn`], preferring `captured` (the runtime an object was made in) over the fallbacks.
pub(crate) fn spawn_on<F>(captured: Option<&Handle>, future: F) -> JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    resolve(captured).spawn(future)
}

fn resolve(captured: Option<&Handle>) -> Handle {
    if let Ok(current) = Handle::try_current() {
        return current;
    }
    if let Some(captured) = captured {
        return captured.clone();
    }
    FALLBACK.get_or_init(own_runtime).clone()
}

fn own_runtime() -> Handle {
    OWN.get_or_init(|| {
        tracing::debug!("starting the background runtime for work begun outside one");
        Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("cv-jobs-runtime")
            .enable_all()
            .build()
            // Only fails when the OS refuses threads or timers; nothing can run then anyway.
            .expect("the background task runtime could not start")
    })
    .handle()
    .clone()
}
