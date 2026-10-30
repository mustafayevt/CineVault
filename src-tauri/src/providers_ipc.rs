//! The Tauri surface of the provider credential commands.
//!
//! The state, the DTOs and the command bodies live in [`cv_ipc::providers`].

use tauri::State;

pub use cv_ipc::providers::*;

/// Lists every provider that has credentials, with its importance and state.
#[tauri::command]
#[specta::specta]
pub fn list_provider_credentials(
    state: State<'_, ProvidersState>,
) -> Result<Vec<ProviderCredentialsDto>, ProviderCommandError> {
    cv_ipc::providers::list_provider_credentials(&state)
}

/// Saves the user's own value for a field. Returns the updated list.
#[tauri::command]
#[specta::specta]
pub async fn set_provider_credential(
    state: State<'_, ProvidersState>,
    provider: String,
    field: String,
    value: String,
) -> Result<Vec<ProviderCredentialsDto>, ProviderCommandError> {
    cv_ipc::providers::set_provider_credential(&state, provider, field, value).await
}

/// Removes everything the user saved for a provider (the card's Remove button). Returns the
/// updated list.
#[tauri::command]
#[specta::specta]
pub fn remove_provider_credentials(
    state: State<'_, ProvidersState>,
    provider: String,
) -> Result<Vec<ProviderCredentialsDto>, ProviderCommandError> {
    cv_ipc::providers::remove_provider_credentials(&state, provider)
}

/// Puts back what the last Remove of a provider took away (the Undo of Remove). Returns the
/// updated list.
#[tauri::command]
#[specta::specta]
pub async fn restore_provider_credentials(
    state: State<'_, ProvidersState>,
    provider: String,
) -> Result<Vec<ProviderCredentialsDto>, ProviderCommandError> {
    cv_ipc::providers::restore_provider_credentials(&state, provider).await
}

/// Tests a provider's credentials with the provider. `drafts` are typed values that are tried
/// instead of the saved ones and not stored.
#[tauri::command]
#[specta::specta]
pub async fn test_provider_credentials(
    state: State<'_, ProvidersState>,
    provider: String,
    drafts: Vec<CredentialDraft>,
) -> Result<CredentialTestResult, ProviderCommandError> {
    cv_ipc::providers::test_provider_credentials(&state, provider, drafts).await
}

/// The providers whose key is missing or rejected, for the banner. Optional providers never
/// appear.
#[tauri::command]
#[specta::specta]
pub fn get_key_attention(
    state: State<'_, ProvidersState>,
) -> Result<KeyAttentionDto, ProviderCommandError> {
    cv_ipc::providers::get_key_attention(&state)
}

/// The credits About shows: every provider's required notice, marked when the provider is in use.
#[tauri::command]
#[specta::specta]
pub fn list_provider_attributions(
    state: State<'_, ProvidersState>,
) -> Result<Vec<ProviderAttributionDto>, ProviderCommandError> {
    cv_ipc::providers::list_provider_attributions(&state)
}
