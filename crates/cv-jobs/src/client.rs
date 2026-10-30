//! Which browser session started a job.
//!
//! The desktop has one window; the server can have several browsers at once. The server sets the
//! client for the duration of a command, so the job specs the command submits carry it. A job
//! submitted outside a command (a resumed job, a background sweep) has no client.

use std::cell::RefCell;

thread_local! {
    static CURRENT: RefCell<String> = const { RefCell::new(String::new()) };
}

/// The client of the command running on this thread, or empty.
#[must_use]
pub fn current() -> String {
    CURRENT.with(|current| current.borrow().clone())
}

/// Sets the client for the life of the returned guard, and clears it on drop.
pub fn enter(client: &str) -> Scope {
    CURRENT.with(|current| *current.borrow_mut() = client.to_owned());
    Scope
}

/// Clears the client when the command ends, including on an early return.
pub struct Scope;

impl Drop for Scope {
    fn drop(&mut self) {
        CURRENT.with(|current| current.borrow_mut().clear());
    }
}
