//! The Tauri surface of the episode ordering commands.
//!
//! The state, the DTOs and the command bodies live in [`cv_ipc::episode_order`].

use tauri::State;

pub use cv_ipc::episode_order::*;

/// The orderings a show can use and the one it uses now.
#[tauri::command]
#[specta::specta]
pub async fn get_show_orderings(
    state: State<'_, EpisodeOrderState>,
    show_id: String,
) -> CommandResult<ShowOrderingsDto> {
    cv_ipc::episode_order::get_show_orderings(&state, show_id).await
}

/// Switches the show to an ordering and rematches its episodes. Answers `ordering_not_available`
/// when the provider has no episodes for the show in that ordering; the show is then unchanged.
#[tauri::command]
#[specta::specta]
pub async fn set_show_ordering(
    state: State<'_, EpisodeOrderState>,
    show_id: String,
    ordering: EpisodeOrderingDto,
) -> CommandResult<OrderingReportDto> {
    cv_ipc::episode_order::set_show_ordering(&state, show_id, ordering).await
}
