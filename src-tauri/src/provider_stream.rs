//! The Tauri surface of the provider stream engine.
//!
//! The engine, its state and the plain `cancel_search`/`retry_provider` calls live in
//! [`cv_ipc::provider_stream`]. The streaming flows build their own channel sinks and call
//! `SearchState::start_on_sink`; this module keeps the two commands.

use tauri::State;

pub use cv_ipc::provider_stream::*;

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
