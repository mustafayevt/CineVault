//! The Tauri surface of the episode scrape commands.
//!
//! The state, the DTOs and the command bodies live in [`cv_ipc::episode_scrape`].

use tauri::State;

pub use cv_ipc::episode_scrape::*;

/// Searches the episode list of a show by title. Fails with `not_available` when the provider has
/// no list for the show, and `no_provider` when no provider is ready.
#[tauri::command]
#[specta::specta]
pub async fn search_show_episodes(
    state: State<'_, EpisodeScrapeState>,
    show_id: String,
    query: String,
) -> CommandResult<Vec<EpisodeHitDto>> {
    cv_ipc::episode_scrape::search_show_episodes(&state, show_id, query).await
}

/// Gives an episode the numbers and IDs of a searched list entry and scrapes the selected fields
/// into it. `fields` (field keys) and `do_not_overwrite` override the settings for this call.
#[tauri::command]
#[specta::specta]
pub async fn assign_episode(
    state: State<'_, EpisodeScrapeState>,
    episode_id: String,
    pick: EpisodePickDto,
    fields: Option<Vec<String>>,
    do_not_overwrite: Option<bool>,
) -> CommandResult<AssignEpisodeDto> {
    cv_ipc::episode_scrape::assign_episode(&state, episode_id, pick, fields, do_not_overwrite).await
}

/// Scrapes the episodes of a show by number in the show's ordering.
#[tauri::command]
#[specta::specta]
pub async fn scrape_show_episodes(
    state: State<'_, EpisodeScrapeState>,
    show_id: String,
    fields: Option<Vec<String>>,
    do_not_overwrite: Option<bool>,
) -> CommandResult<EpisodeScrapeReportDto> {
    cv_ipc::episode_scrape::scrape_show_episodes(&state, show_id, fields, do_not_overwrite).await
}

/// Scrapes one episode. Fails with `no_match` when the list has nothing for its number, which is
/// the cue to search it by title.
#[tauri::command]
#[specta::specta]
pub async fn scrape_episode(
    state: State<'_, EpisodeScrapeState>,
    episode_id: String,
    fields: Option<Vec<String>>,
    do_not_overwrite: Option<bool>,
) -> CommandResult<EpisodeWriteDto> {
    cv_ipc::episode_scrape::scrape_episode(&state, episode_id, fields, do_not_overwrite).await
}
