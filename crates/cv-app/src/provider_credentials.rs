//! Provider credentials for Settings: the user's own keys in the OS credential store, the
//! missing-key banner and the Test button. CineVault ships no keys.
//!
//! The rules live in `cv_providers::credentials`; this service connects them to the secret store
//! and the provider registry. What it hands to the GUI and the CLI is a
//! [`CredentialStatus`]: whether each field has a value, the provider's importance and its state
//! (Active, Missing or Rejected), never a key.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use cv_providers::{
    CredentialError, CredentialResolver, CredentialState, CredentialStatus, CredentialValue,
    HttpClient, Importance, ProviderError, ProviderRegistry, ResolvedCredentials, UserCredentials,
};

use crate::provider_setup::KodiAddons;
use crate::secrets::{SecretError, SecretStore};

impl From<SecretError> for CredentialError {
    fn from(error: SecretError) -> Self {
        match error {
            SecretError::Invalid(message) => Self::Invalid(message),
            other => Self::Store(other.to_string()),
        }
    }
}

/// The user's provider credentials, kept in the secret store under `<provider>.<field>`.
pub struct SecretStoreCredentials(Arc<SecretStore>);

impl SecretStoreCredentials {
    pub fn new(store: Arc<SecretStore>) -> Self {
        Self(store)
    }
}

impl UserCredentials for SecretStoreCredentials {
    fn get(&self, account: &str) -> Result<Option<CredentialValue>, CredentialError> {
        Ok(self
            .0
            .get(account)?
            .map(|secret| CredentialValue::new(secret.expose())))
    }

    fn set(&self, account: &str, value: &str) -> Result<(), CredentialError> {
        Ok(self.0.set(account, value)?)
    }

    fn delete(&self, account: &str) -> Result<bool, CredentialError> {
        Ok(self.0.delete(account)?)
    }
}

/// Settings operations over provider credentials.
pub struct ProviderCredentialsService {
    resolver: CredentialResolver,
    registry: ProviderRegistry,
    /// The HTTP client the providers share, when the service was built with one.
    http: Option<HttpClient>,
    /// The Kodi XML scraper add-ons found at startup.
    kodi: KodiAddons,
    /// What the last Remove of each provider took away, so its Undo can put it back. Held in
    /// memory only, for as long as the app runs, and replaced by the next Remove.
    removed: Mutex<BTreeMap<String, Vec<(&'static str, CredentialValue)>>>,
}

impl ProviderCredentialsService {
    pub fn new(store: Arc<SecretStore>, registry: ProviderRegistry) -> Self {
        Self::with_resolver(
            CredentialResolver::new(Arc::new(SecretStoreCredentials::new(store))),
            registry,
        )
    }

    /// A service over a resolver that already exists. Providers hold clones of the resolver, so
    /// the registry that tests their credentials can be built from the same one.
    pub fn with_resolver(resolver: CredentialResolver, registry: ProviderRegistry) -> Self {
        Self {
            resolver,
            registry,
            http: None,
            kodi: KodiAddons::default(),
            removed: Mutex::new(BTreeMap::new()),
        }
    }

    /// Remembers the Kodi add-ons that were registered in the registry and the ones that could
    /// not be.
    #[must_use]
    pub fn with_kodi(mut self, kodi: KodiAddons) -> Self {
        self.kodi = kodi;
        self
    }

    /// The Kodi XML scraper add-ons found at startup.
    pub fn kodi(&self) -> &KodiAddons {
        &self.kodi
    }

    /// Remembers the HTTP client the registered providers use, so downloads that are not provider
    /// calls (artwork images) share its limits, retry and redaction.
    #[must_use]
    pub fn with_http(mut self, http: HttpClient) -> Self {
        self.http = Some(http);
        self
    }

    /// The shared HTTP client, or `None` when the service was built without one (tests).
    pub fn http(&self) -> Option<&HttpClient> {
        self.http.as_ref()
    }

    /// The resolver that providers use for their requests.
    pub fn resolver(&self) -> &CredentialResolver {
        &self.resolver
    }

    /// The provider implementations this service can test credentials with.
    pub fn registry(&self) -> &ProviderRegistry {
        &self.registry
    }

    /// Whether [`Self::test`] can ask `provider` about its credentials: this build has an
    /// implementation of it that checks them.
    #[must_use]
    pub fn can_test(&self, provider: &str) -> bool {
        self.registry.get(provider).is_some()
    }

    /// Every provider with credentials: importance, state and missing fields.
    pub fn list(&self) -> Result<Vec<CredentialStatus>, CredentialError> {
        self.resolver.statuses()
    }

    /// Saves the user's own value. It applies from the next request, and an earlier rejection
    /// of the provider's key is forgotten.
    pub fn set(&self, provider: &str, field: &str, value: &str) -> Result<(), CredentialError> {
        self.resolver.set_user(provider, field, value)?;
        // Any literal occurrence of the value in a log line or export is masked from now on,
        // even if some code path were to print one.
        crate::redact::register_secret(value.trim());
        Ok(())
    }

    /// Removes the user's own value for one field.
    pub fn clear(&self, provider: &str, field: &str) -> Result<(), CredentialError> {
        self.resolver.clear_user(provider, field).map(|_| ())
    }

    /// Removes everything the user saved for a provider: the Remove button of its card. What
    /// was removed is kept in memory so [`Self::restore`] can undo it.
    pub fn remove(&self, provider: &str) -> Result<(), CredentialError> {
        let schema = self.resolver.schema(provider)?;
        let saved = self.resolver.resolve(provider)?;
        let values: Vec<_> = schema
            .fields
            .iter()
            .filter_map(|field| saved.get(field.key).map(|value| (field.key, value.clone())))
            .collect();
        self.resolver.remove_all(provider)?;
        let mut removed = self.removed.lock().unwrap_or_else(PoisonError::into_inner);
        if values.is_empty() {
            removed.remove(provider);
        } else {
            removed.insert(provider.to_owned(), values);
        }
        Ok(())
    }

    /// Puts back what the last [`Self::remove`] of `provider` took away: the Undo of Remove.
    /// Returns false when there is nothing to put back.
    pub fn restore(&self, provider: &str) -> Result<bool, CredentialError> {
        self.resolver.schema(provider)?;
        let values = self
            .removed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(provider);
        let Some(values) = values else {
            return Ok(false);
        };
        for (field, value) in &values {
            self.set(provider, field, value.expose())?;
        }
        Ok(true)
    }

    /// The providers whose key is missing or rejected, split by importance, for the banner.
    /// Optional providers never appear.
    pub fn attention(&self) -> Result<KeyAttention, CredentialError> {
        let collect = |importance| -> Result<Vec<AttentionEntry>, CredentialError> {
            self.resolver
                .needing_attention(importance)?
                .into_iter()
                .map(|schema| {
                    Ok(AttentionEntry {
                        provider: schema.provider,
                        name: schema.display_name,
                        state: self.resolver.status(schema.provider)?.state,
                    })
                })
                .collect()
        };
        Ok(KeyAttention {
            required: collect(Importance::Required)?,
            recommended: collect(Importance::Recommended)?,
        })
    }

    /// Whether online identification is paused: a required provider that this build registers
    /// (TMDB) has no working key. Local NFO and artwork data still load; matching does not
    /// start until the key is added.
    #[must_use]
    pub fn identification_paused(&self) -> bool {
        self.resolver
            .schemas()
            .iter()
            .filter(|schema| schema.importance == Importance::Required)
            .filter(|schema| self.registry.get(schema.provider).is_some())
            .any(|schema| {
                self.resolver
                    .status(schema.provider)
                    .is_ok_and(|status| !status.active)
            })
    }

    /// Checks a provider's credentials with the provider itself. `drafts` are values the user
    /// typed and has not saved; they are tried instead of the saved ones and are not stored.
    pub async fn test(
        &self,
        provider: &str,
        drafts: &BTreeMap<String, String>,
    ) -> Result<(), TestError> {
        let schema = self
            .resolver
            .schema(provider)
            .map_err(TestError::Credential)?;
        let typed = ResolvedCredentials::from_user_values(
            schema,
            drafts.iter().map(|(k, v)| (k.as_str(), v.as_str())),
        )
        .map_err(TestError::Credential)?;
        let saved = self
            .resolver
            .resolve(provider)
            .map_err(TestError::Credential)?;
        let typed_any = drafts.values().any(|value| !value.trim().is_empty());
        let credentials = if typed_any {
            // Typed values say nothing about the saved key, so a rejection is not noted.
            typed.over(&saved).detached()
        } else {
            saved
        };
        let missing = credentials.missing(schema);
        if !missing.is_empty() {
            return Err(TestError::Provider(ProviderError::MissingCredentials {
                provider: schema.display_name.to_owned(),
                fields: missing.into_iter().map(str::to_owned).collect(),
                registration_url: schema.registration_url.to_owned(),
            }));
        }
        let outcome = self
            .registry
            .check_credentials(schema.provider, schema.display_name, &credentials)
            .await;
        if !typed_any {
            match &outcome {
                Ok(()) => self.resolver.note_accepted(schema.provider),
                Err(error) if matches!(error.root(), ProviderError::Auth { .. }) => {
                    self.resolver.note_rejected(schema.provider);
                }
                Err(_) => {}
            }
        }
        outcome.map_err(TestError::Provider)
    }
}

/// A provider the missing-key banner lists. It carries no value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttentionEntry {
    /// Provider key, for example `tmdb`: the id of its card on the Providers page.
    pub provider: &'static str,
    pub name: &'static str,
    /// `Missing` or `Rejected`.
    pub state: CredentialState,
}

/// What the missing-key banner lists.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeyAttention {
    pub required: Vec<AttentionEntry>,
    pub recommended: Vec<AttentionEntry>,
}

impl KeyAttention {
    /// Nothing to ask the user for.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.required.is_empty() && self.recommended.is_empty()
    }
}

/// Why a credential test did not succeed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TestError {
    #[error(transparent)]
    Credential(CredentialError),
    #[error(transparent)]
    Provider(ProviderError),
}

#[cfg(test)]
mod tests;
