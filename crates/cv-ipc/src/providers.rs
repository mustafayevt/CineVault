//! Typed commands for provider credentials in Settings.
//!
//! The page learns, per provider, its importance (required, recommended or optional), its state
//! (active, missing or rejected) and which fields have a value. It never receives a key: keys
//! travel one way only, from the page to the credential store.

use std::collections::BTreeMap;
use std::sync::Arc;

use cv_app::data_location::DataLocation;
use cv_app::match_service::MatchService;
use cv_app::provider_credentials::{AttentionEntry, ProviderCredentialsService, TestError};
use cv_app::provider_setup::build_service;
use cv_app::secrets::SecretStore;
use cv_app::services::AppServices;
use cv_providers::{
    ATTRIBUTIONS, CredentialError, CredentialState, CredentialStatus, FieldKind, Importance,
    ProviderError,
};
use serde::{Deserialize, Serialize};
use specta::Type;

/// Held in Tauri state.
pub struct ProvidersState {
    service: Arc<ProviderCredentialsService>,
    /// Told when a saved key lifts the pause on identification. Absent when the services could
    /// not start.
    matching: Option<Arc<MatchService>>,
    /// The app settings, where a newly working OMDb or MDBList key turns on its rating source.
    settings: Option<Arc<cv_app::app_settings::SettingsService>>,
}

impl ProvidersState {
    /// The credential service the app services built: the user's keys in the OS credential
    /// store, one shared HTTP client. When the services could not start, a service without a
    /// response cache is built so Settings still works.
    pub fn from_services(services: Option<&Arc<AppServices>>) -> Self {
        let service = services.map_or_else(
            || {
                // The same credential store the services would have used (a portable copy has
                // its own).
                let secrets = DataLocation::resolve_for_process()
                    .map_or_else(SecretStore::system, |l| SecretStore::for_location(&l));
                Arc::new(build_service(Arc::new(secrets), None))
            },
            |services| Arc::clone(services.providers()),
        );
        let matching = services.map(|services| Arc::clone(services.matching()));
        let settings = services.map(|services| Arc::clone(services.settings()));
        Self {
            service,
            matching,
            settings,
        }
    }

    /// Saves a value, and when that lifted the pause on identification starts matching the
    /// waiting titles. Must run inside the async runtime.
    pub async fn save(
        &self,
        provider: &str,
        field: &str,
        value: &str,
    ) -> Result<Vec<ProviderCredentialsDto>, ProviderCommandError> {
        self.changing(|service| service.set(provider, field, value))
            .await
    }

    /// Puts back what the last Remove of `provider` took away, and resumes matching as a save
    /// does. Must run inside the async runtime.
    pub async fn restore(
        &self,
        provider: &str,
    ) -> Result<Vec<ProviderCredentialsDto>, ProviderCommandError> {
        self.changing(|service| service.restore(provider).map(|_| ()))
            .await
    }

    /// The providers whose credentials are all in place now.
    fn active_providers(&self) -> Vec<String> {
        self.service
            .list()
            .map(|statuses| {
                statuses
                    .into_iter()
                    .filter(|status| status.active)
                    .map(|status| status.provider.to_owned())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Runs a change of the saved credentials; when it lifted the pause on identification,
    /// resumes the scrapes the user started that the pause held back (nothing new is queued).
    /// Returns the updated list.
    async fn changing(
        &self,
        change: impl FnOnce(&ProviderCredentialsService) -> Result<(), CredentialError>,
    ) -> Result<Vec<ProviderCredentialsDto>, ProviderCommandError> {
        let was_paused = self.service.identification_paused();
        let was_active = self.active_providers();
        change(&self.service)?;
        // A key that works now for OMDb or MDBList turns its rating source on.
        if let Some(settings) = &self.settings {
            for provider in self.active_providers() {
                if was_active.contains(&provider) {
                    continue;
                }
                if let Err(error) = cv_app::ratings::choose_source_for_key(settings, &provider) {
                    tracing::warn!(%error, provider, "the rating source of a new key was not turned on");
                }
            }
        }
        if was_paused && let Some(matching) = self.matching.clone() {
            let handle = tokio::runtime::Handle::current();
            // Planning reads the database and the job manager spawns on the current runtime.
            let started = cv_jobs::client::spawn_blocking(move || {
                let _entered = handle.enter();
                matching.key_saved(was_paused)
            })
            .await;
            if let Err(error) = started {
                tracing::warn!(%error, "held-back scrapes could not be resumed after saving a key");
            }
        }
        list(&self.service)
    }
}

/// How much a provider matters; Settings groups the cards by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ImportanceDto {
    Required,
    Recommended,
    Optional,
}

impl From<Importance> for ImportanceDto {
    fn from(importance: Importance) -> Self {
        match importance {
            Importance::Required => Self::Required,
            Importance::Recommended => Self::Recommended,
            Importance::Optional => Self::Optional,
        }
    }
}

/// Where a provider's credentials stand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CredentialStateDto {
    /// The user entered the credentials and the provider works with them.
    Active,
    /// The user has not entered what the provider needs.
    Missing,
    /// The provider turned the user's key down.
    Rejected,
}

impl From<CredentialState> for CredentialStateDto {
    fn from(state: CredentialState) -> Self {
        match state {
            CredentialState::Active => Self::Active,
            CredentialState::Missing => Self::Missing,
            CredentialState::Rejected => Self::Rejected,
        }
    }
}

/// How a field is entered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CredentialFieldKindDto {
    /// A key, token or password: entered masked.
    Secret,
    /// A plain value such as an account name.
    Text,
}

/// One credential field of a provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct CredentialFieldDto {
    /// `api_key`, `app_key`, `username`, ...
    pub key: String,
    pub kind: CredentialFieldKindDto,
    pub required: bool,
    /// The user has entered a value. The value itself is never sent.
    pub present: bool,
}

/// One provider's credentials as Settings shows them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct ProviderCredentialsDto {
    /// `tmdb`, `omdb`, ...
    pub id: String,
    pub name: String,
    pub registration_url: String,
    pub importance: ImportanceDto,
    /// The provider's place in the catalogue, from 1 (TMDB): the order the page lists them in.
    pub rank: u32,
    pub fields: Vec<CredentialFieldDto>,
    /// Every required field has a value.
    pub active: bool,
    /// The user has entered at least one value.
    pub configured: bool,
    pub state: CredentialStateDto,
    /// Keys of required fields with no value.
    pub missing: Vec<String>,
    /// This build can ask the provider whether it accepts the credentials (the Test button).
    pub testable: bool,
}

impl From<CredentialStatus> for ProviderCredentialsDto {
    fn from(status: CredentialStatus) -> Self {
        Self {
            id: status.provider.to_owned(),
            name: status.display_name.to_owned(),
            registration_url: status.registration_url.to_owned(),
            importance: status.importance.into(),
            rank: u32::from(status.rank),
            fields: status
                .fields
                .into_iter()
                .map(|field| CredentialFieldDto {
                    key: field.key.to_owned(),
                    kind: match field.kind {
                        FieldKind::Secret => CredentialFieldKindDto::Secret,
                        FieldKind::Text => CredentialFieldKindDto::Text,
                    },
                    required: field.required,
                    present: field.present,
                })
                .collect(),
            active: status.active,
            configured: status.configured,
            state: status.state.into(),
            missing: status.missing.into_iter().map(str::to_owned).collect(),
            testable: false,
        }
    }
}

/// Why a command did not do what was asked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct ProviderCommandError {
    /// Stable code: `unknown_provider`, `unknown_field`, `invalid` or `store`.
    pub code: String,
    /// Plain sentence for the user.
    pub message: String,
}

impl From<CredentialError> for ProviderCommandError {
    fn from(error: CredentialError) -> Self {
        let code = match &error {
            CredentialError::UnknownProvider(_) => "unknown_provider",
            CredentialError::UnknownField { .. } => "unknown_field",
            CredentialError::Invalid(_) => "invalid",
            CredentialError::Store(_) => "store",
        };
        Self {
            code: code.to_owned(),
            message: error.to_string(),
        }
    }
}

/// A value the user typed and has not saved yet.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Type)]
pub struct CredentialDraft {
    pub field: String,
    pub value: String,
}

/// The answer to the Test button.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct CredentialTestResult {
    pub ok: bool,
    /// `rejected`, `rate_limited`, `missing`, `unsupported`, `request` or `other` when not ok.
    pub code: Option<String>,
    /// The provider's own message, or a sentence from the app.
    pub message: Option<String>,
}

fn failed(code: &str, message: String) -> CredentialTestResult {
    CredentialTestResult {
        ok: false,
        code: Some(code.to_owned()),
        message: Some(message),
    }
}

fn test_result(outcome: Result<(), TestError>) -> CredentialTestResult {
    match outcome {
        Ok(()) => CredentialTestResult {
            ok: true,
            code: None,
            message: None,
        },
        Err(TestError::Credential(error)) => failed("other", error.to_string()),
        // The card shows the provider's own words: its link target is the card itself.
        Err(TestError::Provider(error)) => match error.root() {
            ProviderError::Auth { message, .. } => failed("rejected", message.clone()),
            root @ ProviderError::RateLimited { .. } => failed("rate_limited", root.to_string()),
            root @ ProviderError::MissingCredentials { .. } => failed("missing", root.to_string()),
            ProviderError::Unsupported { provider, .. } => failed(
                "unsupported",
                format!("Testing {provider} keys is not available in this build yet."),
            ),
            root @ ProviderError::Request { .. } => failed("request", root.to_string()),
            root => failed("other", root.to_string()),
        },
    }
}

fn list(
    service: &ProviderCredentialsService,
) -> Result<Vec<ProviderCredentialsDto>, ProviderCommandError> {
    Ok(service
        .list()?
        .into_iter()
        .map(|status| {
            let testable = service.can_test(status.provider);
            ProviderCredentialsDto {
                testable,
                ..ProviderCredentialsDto::from(status)
            }
        })
        .collect())
}

/// Lists every provider that has credentials, with its importance and state.
pub fn list_provider_credentials(
    state: &ProvidersState,
) -> Result<Vec<ProviderCredentialsDto>, ProviderCommandError> {
    list(&state.service)
}

/// Saves the user's own value for a field. Returns the updated list.
pub async fn set_provider_credential(
    state: &ProvidersState,
    provider: String,
    field: String,
    value: String,
) -> Result<Vec<ProviderCredentialsDto>, ProviderCommandError> {
    state.save(&provider, &field, &value).await
}

/// Removes everything the user saved for a provider (the card's Remove button). Returns the
/// updated list.
pub fn remove_provider_credentials(
    state: &ProvidersState,
    provider: String,
) -> Result<Vec<ProviderCredentialsDto>, ProviderCommandError> {
    state.service.remove(&provider)?;
    list(&state.service)
}

/// Puts back what the last Remove of a provider took away (the Undo of Remove). Returns the
/// updated list.
pub async fn restore_provider_credentials(
    state: &ProvidersState,
    provider: String,
) -> Result<Vec<ProviderCredentialsDto>, ProviderCommandError> {
    state.restore(&provider).await
}

/// Tests a provider's credentials with the provider. `drafts` are typed values that are tried
/// instead of the saved ones and not stored.
pub async fn test_provider_credentials(
    state: &ProvidersState,
    provider: String,
    drafts: Vec<CredentialDraft>,
) -> Result<CredentialTestResult, ProviderCommandError> {
    let drafts: BTreeMap<String, String> = drafts
        .into_iter()
        .map(|draft| (draft.field, draft.value))
        .collect();
    Ok(test_result(state.service.test(&provider, &drafts).await))
}

/// A provider the missing-key banner lists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct KeyAttentionEntryDto {
    /// `tmdb`, `tvdb`, ...: also the id of the provider's card on the Providers page.
    pub id: String,
    pub name: String,
    /// `missing` or `rejected`.
    pub state: CredentialStateDto,
}

impl From<AttentionEntry> for KeyAttentionEntryDto {
    fn from(entry: AttentionEntry) -> Self {
        Self {
            id: entry.provider.to_owned(),
            name: entry.name.to_owned(),
            state: entry.state.into(),
        }
    }
}

/// What the missing-key banner shows on the library and Home pages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct KeyAttentionDto {
    /// Required providers that are missing or rejected (TMDB).
    pub required: Vec<KeyAttentionEntryDto>,
    /// Recommended providers that are missing or rejected.
    pub recommended: Vec<KeyAttentionEntryDto>,
    /// Online identification is paused until the required key is added.
    pub identification_paused: bool,
}

fn attention(
    service: &ProviderCredentialsService,
) -> Result<KeyAttentionDto, ProviderCommandError> {
    let attention = service.attention()?;
    Ok(KeyAttentionDto {
        required: attention.required.into_iter().map(Into::into).collect(),
        recommended: attention.recommended.into_iter().map(Into::into).collect(),
        identification_paused: service.identification_paused(),
    })
}

/// The providers whose key is missing or rejected, for the banner. Optional providers never
/// appear.
pub fn get_key_attention(state: &ProvidersState) -> Result<KeyAttentionDto, ProviderCommandError> {
    attention(&state.service)
}

/// One provider's credit for the About screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct ProviderAttributionDto {
    /// `tmdb`, `tvdb`, ...
    pub id: String,
    pub name: String,
    /// The notice the provider's terms ask for.
    pub notice: String,
    pub url: String,
    /// The terms require the notice wherever the provider's data is shown.
    pub required: bool,
    /// The provider has the user's credentials, so the app can use it.
    pub in_use: bool,
}

fn attributions(
    service: &ProviderCredentialsService,
) -> Result<Vec<ProviderAttributionDto>, ProviderCommandError> {
    let statuses = service.list()?;
    Ok(ATTRIBUTIONS
        .iter()
        .map(|credit| ProviderAttributionDto {
            id: credit.provider.to_owned(),
            name: credit.name.to_owned(),
            notice: credit.notice.to_owned(),
            url: credit.url.to_owned(),
            required: credit.required,
            in_use: statuses
                .iter()
                .any(|status| status.provider == credit.provider && status.active),
        })
        .collect())
}

/// The credits About shows: every provider's required notice, marked when the provider is in use.
pub fn list_provider_attributions(
    state: &ProvidersState,
) -> Result<Vec<ProviderAttributionDto>, ProviderCommandError> {
    attributions(&state.service)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cv_providers::ProviderRegistry;

    const KEY: &str = "typed-by-the-user-77";

    fn service() -> ProviderCredentialsService {
        ProviderCredentialsService::new(Arc::new(SecretStore::in_memory()), ProviderRegistry::new())
    }

    #[test]
    fn the_list_reports_importance_and_state_and_never_a_value() {
        let service = service();
        service.set("omdb", "api_key", KEY).unwrap();
        service.set("tmdb", "api_key", "tmdb-secret-0042").unwrap();
        let dtos = list(&service).unwrap();

        let tmdb = dtos.iter().find(|d| d.id == "tmdb").unwrap();
        assert_eq!(tmdb.importance, ImportanceDto::Required);
        assert_eq!(tmdb.state, CredentialStateDto::Active);
        assert!(tmdb.active && tmdb.configured);
        let omdb = dtos.iter().find(|d| d.id == "omdb").unwrap();
        assert_eq!(omdb.importance, ImportanceDto::Recommended);
        assert_eq!(omdb.state, CredentialStateDto::Active);
        assert!(omdb.fields.iter().all(|f| f.present));
        let tvdb = dtos.iter().find(|d| d.id == "tvdb").unwrap();
        assert_eq!(tvdb.importance, ImportanceDto::Recommended);
        assert_eq!(tvdb.state, CredentialStateDto::Missing);
        assert!(!tvdb.active && !tvdb.configured);
        assert_eq!(tvdb.missing, vec!["api_key".to_owned()]);

        let json = serde_json::to_string(&dtos).unwrap();
        assert!(!json.contains(KEY));
        assert!(!json.contains("tmdb-secret-0042"));
        assert!(!json.contains("builtin"));
    }

    #[test]
    fn the_providers_group_as_the_providers_page_shows_them() {
        let dtos = list(&service()).unwrap();
        let of = |importance| {
            dtos.iter()
                .filter(|d| d.importance == importance)
                .map(|d| d.id.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(of(ImportanceDto::Required), ["tmdb"]);
        assert_eq!(of(ImportanceDto::Recommended), ["omdb", "tvdb", "fanart"]);
        assert!(of(ImportanceDto::Optional).contains(&"trakt"));
        assert!(dtos.iter().all(|d| d.state == CredentialStateDto::Missing));
    }

    #[test]
    fn the_rank_is_in_the_dto_and_the_list_comes_in_rank_order() {
        let dtos = list(&service()).unwrap();
        let ranks: Vec<u32> = dtos.iter().map(|d| d.rank).collect();
        let expected: Vec<u32> = (1..=u32::try_from(dtos.len()).unwrap()).collect();
        assert_eq!(ranks, expected);
        let first_four: Vec<&str> = dtos.iter().take(4).map(|d| d.id.as_str()).collect();
        assert_eq!(first_four, ["tmdb", "omdb", "tvdb", "fanart"]);
        let rank_of = |id: &str| dtos.iter().find(|d| d.id == id).unwrap().rank;
        assert!(rank_of("trakt") > rank_of("fanart"));
    }

    #[test]
    fn removing_a_provider_deletes_its_key_and_the_card_shows_missing_again() {
        let service = service();
        service.set("tmdb", "api_key", KEY).unwrap();
        service.remove("tmdb").unwrap();
        let dtos = list(&service).unwrap();
        let tmdb = dtos.iter().find(|d| d.id == "tmdb").unwrap();
        assert_eq!(tmdb.state, CredentialStateDto::Missing);
        assert!(tmdb.fields.iter().all(|f| !f.present));
    }

    #[test]
    fn the_banner_lists_tmdb_and_the_recommended_providers_and_never_a_value() {
        let service = service();
        let banner = attention(&service).unwrap();
        let ids = |entries: &[KeyAttentionEntryDto]| -> Vec<String> {
            entries.iter().map(|e| e.id.clone()).collect()
        };
        assert_eq!(ids(&banner.required), ["tmdb"]);
        assert_eq!(ids(&banner.recommended), ["omdb", "tvdb", "fanart"]);
        service.set("tmdb", "api_key", KEY).unwrap();
        let banner = attention(&service).unwrap();
        assert!(banner.required.is_empty());
        assert!(!serde_json::to_string(&banner).unwrap().contains(KEY));
    }

    #[test]
    fn ipc_replies_as_json_carry_no_key() {
        let service = service();
        service.set("tmdb", "api_key", KEY).unwrap();
        service.set("omdb", "api_key", KEY).unwrap();
        let replies = [
            serde_json::to_string(&list(&service).unwrap()).unwrap(),
            serde_json::to_string(&attributions(&service).unwrap()).unwrap(),
            serde_json::to_string(&attention(&service).unwrap()).unwrap(),
            serde_json::to_string(&test_result(Err(TestError::Provider(
                ProviderError::Auth {
                    provider: "TMDB".to_owned(),
                    message: "Invalid API key".to_owned(),
                }
                .with_key_hint(&cv_providers::ResolvedCredentials::default(), "tmdb"),
            ))))
            .unwrap(),
        ];
        for reply in &replies {
            assert!(!reply.contains(KEY), "{reply}");
        }
    }

    #[test]
    fn a_rejected_or_rate_limited_key_maps_to_its_code_with_the_providers_words() {
        let none = cv_providers::ResolvedCredentials::default();
        let rejected = test_result(Err(TestError::Provider(
            ProviderError::Auth {
                provider: "TMDB".to_owned(),
                message: "Invalid API key".to_owned(),
            }
            .with_key_hint(&none, "tmdb"),
        )));
        assert_eq!(rejected.code.as_deref(), Some("rejected"));
        assert_eq!(rejected.message.as_deref(), Some("Invalid API key"));
        let limited = test_result(Err(TestError::Provider(
            ProviderError::RateLimited {
                provider: "TMDB".to_owned(),
                retry_after_secs: 5,
            }
            .with_key_hint(&none, "tmdb"),
        )));
        assert_eq!(limited.code.as_deref(), Some("rate_limited"));
        assert!(limited.message.unwrap().contains("5 seconds"));
    }

    #[test]
    fn tmdb_is_registered_so_the_settings_test_button_reaches_it() {
        let service = build_service(Arc::new(SecretStore::in_memory()), None);
        assert!(service.registry().get("tmdb").is_some());
        // The provider shares the service's resolver: a key saved now is what it reads.
        assert!(
            service
                .resolver()
                .resolve("tmdb")
                .unwrap()
                .get("api_key")
                .is_none()
        );
        service
            .set("tmdb", "api_key", "saved-after-start-31")
            .unwrap();
        assert!(
            service
                .resolver()
                .resolve("tmdb")
                .unwrap()
                .get("api_key")
                .is_some()
        );
    }

    #[test]
    fn tvdb_is_registered_and_offers_an_optional_subscriber_pin() {
        let service = build_service(Arc::new(SecretStore::in_memory()), None);
        assert!(service.registry().get("tvdb").is_some());

        let dtos = list(&service).unwrap();
        let tvdb = dtos.iter().find(|d| d.id == "tvdb").unwrap();
        let pin = tvdb
            .fields
            .iter()
            .find(|f| f.key == "pin")
            .expect("pin field");
        assert!(!pin.required);
        assert_eq!(pin.kind, CredentialFieldKindDto::Secret);
        // Without a key the provider is inactive, whatever the PIN says.
        service.set("tvdb", "pin", "1234-5678").unwrap();
        let dtos = list(&service).unwrap();
        let tvdb = dtos.iter().find(|d| d.id == "tvdb").unwrap();
        assert!(!tvdb.active);
        assert_eq!(tvdb.missing, vec!["api_key".to_owned()]);
        assert!(!format!("{dtos:?}").contains("1234-5678"));
    }

    #[test]
    fn attributions_credit_every_provider_and_mark_the_ones_in_use() {
        let service = service();
        service.set("tmdb", "api_key", KEY).unwrap();
        let credits = attributions(&service).unwrap();

        let tmdb = credits.iter().find(|c| c.id == "tmdb").unwrap();
        assert!(tmdb.required && tmdb.in_use);
        assert!(tmdb.notice.contains("TMDB API"));
        let tvdb = credits.iter().find(|c| c.id == "tvdb").unwrap();
        assert!(tvdb.required && !tvdb.in_use);
        assert!(credits.iter().any(|c| c.name == "Fanart.tv" && c.required));
        // Credits carry no key.
        assert!(!format!("{credits:?}").contains(KEY));
    }

    #[test]
    fn errors_carry_a_stable_code() {
        let service = service();
        let error: ProviderCommandError =
            service.set("nowhere", "api_key", "x").unwrap_err().into();
        assert_eq!(error.code, "unknown_provider");
        let error: ProviderCommandError = service.set("tmdb", "api_key", " ").unwrap_err().into();
        assert_eq!(error.code, "invalid");
    }

    #[test]
    fn test_outcomes_map_to_codes() {
        assert!(test_result(Ok(())).ok);
        let unsupported = test_result(Err(TestError::Provider(ProviderError::Unsupported {
            provider: "OMDb".to_owned(),
            operation: "x",
        })));
        assert_eq!(unsupported.code.as_deref(), Some("unsupported"));
    }
}
