//! The Tauri surface of the provider stream engine.
//!
//! The engine, its state and the plain `cancel_search`/`retry_provider` calls live in
//! [`cv_ipc::provider_stream`]. This module adds the channel adapters the streaming commands use
//! and keeps the two commands as thin wrappers.

use std::sync::Arc;

use serde::Serialize;
use tauri::State;
use tauri::ipc::Channel;

pub use cv_ipc::provider_stream::*;

/// Starts a search that sends its events over a Tauri channel.
pub fn start_on_channel<T: Serialize + Send + Sync + 'static>(
    state: &SearchState,
    search_id: &str,
    providers: Vec<Arc<dyn SearchProvider<T>>>,
    channel: Channel<ProviderEvent<T>>,
) -> Result<(), SearchCommandError> {
    state.start_on_sink(search_id, providers, crate::ipc_sink::sink(channel))
}

/// As [`start_on_channel`]; `finish` can add one event just before `Done`.
pub fn start_on_channel_finishing<T: Serialize + Send + Sync + 'static>(
    state: &SearchState,
    search_id: &str,
    providers: Vec<Arc<dyn SearchProvider<T>>>,
    channel: Channel<ProviderEvent<T>>,
    finish: Option<Finish<T>>,
) -> Result<(), SearchCommandError> {
    state.start_on_sink_finishing(search_id, providers, crate::ipc_sink::sink(channel), finish)
}

/// Stops a search. Providers that have not started their request never do, and a request that is
/// waiting for its answer is dropped. Cancelling a search that is gone is fine.
#[tauri::command]
#[specta::specta]
pub fn cancel_search(state: State<'_, SearchState>, search_id: String) -> bool {
    cv_ipc::provider_stream::cancel_search(&state, &search_id)
}

/// Asks one provider of a search again; the other providers are left alone. The answer comes on
/// the search's channel: `Started`, the result and `Done`.
#[tauri::command]
#[specta::specta]
pub fn retry_provider(
    state: State<'_, SearchState>,
    search_id: String,
    provider: String,
) -> Result<(), SearchCommandError> {
    cv_ipc::provider_stream::retry_provider(&state, &search_id, &provider)
}
