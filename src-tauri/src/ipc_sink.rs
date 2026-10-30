//! The Tauri side of the transport-neutral sinks.
//!
//! A command's progress goes to a [`cv_ipc::sink::Sink`]; the desktop adapter sends each value over
//! the channel the page opened. A closed channel simply drops the value: leaving a page must not
//! stop the work.

use serde::Serialize;
use tauri::ipc::Channel;

/// A sink that sends every value over `channel`.
pub fn sink<T: Serialize + Send + Sync + 'static>(channel: Channel<T>) -> cv_ipc::sink::Sink<T> {
    std::sync::Arc::new(move |value| {
        // A closed channel means the page is gone. The value is dropped; the work goes on.
        if let Err(error) = channel.send(value) {
            tracing::debug!(%error, "a stream event had nowhere to go");
        }
    })
}
