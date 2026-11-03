//! The Tauri surface of the torrent Wishlist. The DTOs and the command bodies live in
//! [`cv_ipc::torrent_wishlist`]; these are the thin wrappers.

use tauri::State;

use cv_ipc::torrent_stream::{CommandResult, TorrentState};
use cv_ipc::torrent_wishlist::{
    WishAddDto, WishCheckDto, WishCriteriaDto, WishEntryDto, WishScopeDto,
};

/// Every entry of the Wishlist, the newest first, with its matches and poster. Entries whose
/// criteria the library now meets are marked done. Only the library is read.
#[tauri::command]
#[specta::specta]
pub async fn torrent_wishlist(state: State<'_, TorrentState>) -> CommandResult<Vec<WishEntryDto>> {
    cv_ipc::torrent_wishlist::torrent_wishlist(&state).await
}

/// Adds a title to the Wishlist, or replaces the criteria of the entry with this title and
/// scope. Returns the entry's id.
#[tauri::command]
#[specta::specta]
pub async fn torrent_wishlist_add(
    state: State<'_, TorrentState>,
    request: WishAddDto,
) -> CommandResult<u32> {
    cv_ipc::torrent_wishlist::torrent_wishlist_add(&state, request).await
}

/// Changes an entry's scope and criteria; the entry is judged and checked again.
#[tauri::command]
#[specta::specta]
pub fn torrent_wishlist_update(
    state: State<'_, TorrentState>,
    id: u32,
    scope: WishScopeDto,
    criteria: WishCriteriaDto,
) -> CommandResult<()> {
    cv_ipc::torrent_wishlist::torrent_wishlist_update(&state, id, scope, criteria)
}

/// Removes an entry and its matches.
#[tauri::command]
#[specta::specta]
pub fn torrent_wishlist_remove(state: State<'_, TorrentState>, id: u32) -> CommandResult<()> {
    cv_ipc::torrent_wishlist::torrent_wishlist_remove(&state, id)
}

/// Checks the entries that are due (`due_only`) or every entry that is not done. One check runs
/// at a time; a second answers at once with `busy`.
#[tauri::command]
#[specta::specta]
pub async fn torrent_wishlist_check(
    state: State<'_, TorrentState>,
    due_only: bool,
) -> CommandResult<WishCheckDto> {
    cv_ipc::torrent_wishlist::torrent_wishlist_check(&state, due_only).await
}

/// Marks a match `sent` or `dismissed` (or `seen` again).
#[tauri::command]
#[specta::specta]
pub fn torrent_wishlist_match_state(
    state: State<'_, TorrentState>,
    id: u32,
    key: String,
    match_state: String,
) -> CommandResult<()> {
    cv_ipc::torrent_wishlist::torrent_wishlist_match_state(&state, id, key, match_state)
}

/// Marks every new match seen (the Wishlist was opened). Returns how many there were.
#[tauri::command]
#[specta::specta]
pub fn torrent_wishlist_seen(state: State<'_, TorrentState>) -> CommandResult<u32> {
    cv_ipc::torrent_wishlist::torrent_wishlist_seen(&state)
}

/// How many matches have not been looked at yet, for the sidebar.
#[tauri::command]
#[specta::specta]
pub fn torrent_wishlist_new_count(state: State<'_, TorrentState>) -> CommandResult<u32> {
    cv_ipc::torrent_wishlist::torrent_wishlist_new_count(&state)
}
