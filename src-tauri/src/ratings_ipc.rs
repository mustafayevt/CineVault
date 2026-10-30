//! The Tauri surface of the ratings commands.
//!
//! The DTOs and the command bodies live in [`cv_ipc::ratings`]; opening a rating source's page is
//! desktop-only and stays here.

use tauri::State;
use tauri::ipc::Channel;
use tauri_plugin_opener::OpenerExt;

use crate::boot::Boot;
use crate::sources_ipc::LibraryState;

pub use cv_ipc::ratings::*;

/// Fetches the IMDb Top 250 chart and stores each title's position.
#[tauri::command]
#[specta::specta]
pub async fn fetch_imdb_top250(
    library: State<'_, LibraryState>,
    list: Top250ListDto,
) -> Result<Top250ReportDto, RatingsCommandError> {
    cv_ipc::ratings::fetch_imdb_top250(&library, list).await
}

/// How many titles the ratings sweep of `scope` would fetch. With `provider` (`omdb` or
/// `mdblist`) only the titles that lack that source's ratings count: what a new key unlocks.
#[tauri::command]
#[specta::specta]
pub async fn count_rating_sweep(
    boot: State<'_, Boot>,
    scope: RatingSweepScopeDto,
    provider: Option<String>,
) -> Result<u32, RatingsCommandError> {
    cv_ipc::ratings::count_rating_sweep(boot.services(), scope, provider).await
}

/// Fetches the ratings of the sweep's titles as one job in Activity, one title at a time,
/// stopping calmly at a source's request limit. `on_done` receives the summary once, when the job
/// is over. Answers `not_found` when there is nothing to fetch.
#[tauri::command]
#[specta::specta]
pub async fn start_rating_sweep(
    boot: State<'_, Boot>,
    scope: RatingSweepScopeDto,
    provider: Option<String>,
    on_done: Channel<RatingSweepSummaryDto>,
) -> Result<RatingSweepStartedDto, RatingsCommandError> {
    cv_ipc::ratings::start_rating_sweep(
        boot.services(),
        scope,
        provider,
        crate::ipc_sink::sink(on_done),
    )
    .await
}

/// Opens the title's page on a rating source's site (a click on a rating of the ratings strip)
/// in the default browser. Only `http` and `https` addresses are opened.
#[tauri::command]
#[specta::specta]
pub async fn open_rating_page(
    app: tauri::AppHandle,
    address: String,
) -> Result<(), RatingsCommandError> {
    cv_ipc::ratings::open_web_page(&address, &|url| {
        app.opener()
            .open_url(url, None::<&str>)
            .map_err(|error| error.to_string())
    })
}
