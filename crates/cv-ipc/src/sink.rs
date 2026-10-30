//! Where a stream's values go, independent of the transport.
//!
//! A command hands its progress to a sink; the desktop adapter sends it over a Tauri channel and
//! the server over its event stream. A sink that has nowhere left to send simply drops the value:
//! leaving a page must not stop the work.

use std::sync::Arc;

/// A transport-neutral sink for a stream of values.
pub type Sink<T> = Arc<dyn Fn(T) + Send + Sync>;
