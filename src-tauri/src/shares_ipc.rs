//! The Tauri surface of the network share commands.
//!
//! The rules, the state and the DTOs live in [`cv_ipc::shares`]. Every command here is
//! desktop-only: the server's dispatcher answers them `not_applicable`, because a browser's
//! server connects its shares with its installer.

use tauri::State;

pub use cv_ipc::shares::*;

/// What this computer can do with network shares: whether it can connect them at all, which
/// system it is and where a connected share appears.
#[tauri::command]
#[specta::specta]
pub async fn network_share_support(
    state: State<'_, SharesState>,
) -> Result<ShareSupportDto, ShareCommandError> {
    cv_ipc::shares::network_share_support(&state).await
}

/// Connects the share with the login and adds the folder inside it as a library folder.
#[tauri::command]
#[specta::specta]
pub async fn connect_network_share(
    state: State<'_, SharesState>,
    input: ConnectShareInput,
) -> Result<ShareAddedDto, ShareCommandError> {
    cv_ipc::shares::connect_network_share(&state, input).await
}

/// Connects a known share again with a new login ("Connect…"); its folders come online.
#[tauri::command]
#[specta::specta]
pub async fn connect_network_share_again(
    state: State<'_, SharesState>,
    input: ReconnectShareInput,
) -> Result<NetworkShareDto, ShareCommandError> {
    cv_ipc::shares::connect_network_share_again(&state, input).await
}

/// Stops the connect that is running. Returns false when none is.
#[tauri::command]
#[specta::specta]
pub fn cancel_network_share_connect(state: State<'_, SharesState>) -> bool {
    cv_ipc::shares::cancel_network_share_connect(&state)
}

/// Every share CineVault connected.
#[tauri::command]
#[specta::specta]
pub async fn list_network_shares(
    state: State<'_, SharesState>,
) -> Result<Vec<NetworkShareDto>, ShareCommandError> {
    cv_ipc::shares::list_network_shares(&state).await
}
