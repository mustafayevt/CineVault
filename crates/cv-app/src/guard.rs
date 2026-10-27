//! Keeping one failing callback from taking the whole app down.
//!
//! The desktop app runs synchronous commands and window callbacks on the window thread, inside
//! system callbacks that cannot unwind: a panic there aborts the process. Change listeners run on
//! whatever thread made the change, so they can end up there too. [`contain`] runs such a
//! callback, and when it panics logs the panic as an error and carries on. The panic itself is
//! still printed by the panic hook; nothing is silently swallowed.

use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};

/// Runs `f`; when it panics, logs the panic under `what` and returns `None`.
pub fn contain<T>(what: &str, f: impl FnOnce() -> T) -> Option<T> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(value) => Some(value),
        Err(panic) => {
            tracing::error!(
                callback = what,
                panic = %panic_message(panic.as_ref()),
                "a callback panicked; the app carries on"
            );
            None
        }
    }
}

/// The message a panic was raised with, or a placeholder when it carried something else.
pub fn panic_message(panic: &(dyn Any + Send)) -> String {
    if let Some(text) = panic.downcast_ref::<&str>() {
        (*text).to_owned()
    } else if let Some(text) = panic.downcast_ref::<String>() {
        text.clone()
    } else {
        "(no message)".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_callback_that_returns_hands_back_its_value() {
        assert_eq!(contain("test", || 7), Some(7));
    }

    #[test]
    fn a_panicking_callback_is_contained_and_its_message_kept() {
        assert_eq!(contain("test", || -> u8 { panic!("boom {}", 1) }), None);
        let panic = catch_unwind(|| panic!("plain")).unwrap_err();
        assert_eq!(panic_message(panic.as_ref()), "plain");
        let panic = catch_unwind(|| panic!("formatted {}", 2)).unwrap_err();
        assert_eq!(panic_message(panic.as_ref()), "formatted 2");
    }
}
