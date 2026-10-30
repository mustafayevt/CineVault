//! Typed events without a transport.
//!
//! The desktop app implements [`EventSink`] with `tauri::Emitter`; the server implements it with
//! its stream. The command layer only names the event and hands over the payload, so the three
//! Tauri event structs can stay in the desktop crate (the orphan rule would not let this crate
//! implement their trait anyway).

use serde_json::Value;

/// Where a typed event goes.
pub trait EventSink: Send + Sync {
    /// Sends one event, named as the UI's `events` map names it, with its JSON payload.
    fn emit(&self, event: &str, payload: Value);
}

/// An event sink for surfaces that have none (tests, the command line, a quiet import).
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopEventSink;

impl EventSink for NoopEventSink {
    fn emit(&self, _event: &str, _payload: Value) {}
}
