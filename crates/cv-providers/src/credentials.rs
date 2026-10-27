//! Provider credentials: the schema each provider declares and the resolver that picks a value.
//!
//! Every credential is the user's own, kept in the OS credential store; CineVault ships none.
//! Resolution, per field: the user's value, otherwise none. A provider whose required fields
//! all resolve is active; otherwise it is inactive and says which fields are missing.
//!
//! What the outside world may learn is whether each field has a value and the provider's
//! [`CredentialState`] ([`CredentialStatus`]). Values only ever leave the resolver inside
//! [`ResolvedCredentials`], whose `Debug` output is masked, and only provider code that is about
//! to build a request should call [`ResolvedCredentials::get`]. Nothing here serialises a value,
//! so no settings file, export, IPC reply, HTTP response or CLI output built from these types
//! can contain one.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use serde::Serialize;

use crate::error::ProviderError;

/// Shown instead of a credential value in `Debug` and `Display` output.
const MASK: &str = "********";

/// How much a provider matters to a fresh install. Settings groups provider cards by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Importance {
    /// CineVault cannot identify titles online without it.
    Required,
    /// Adds a lot (episode data, artwork, subtitles); worth setting up.
    Recommended,
    /// Adds something extra for people who want it.
    Optional,
}

/// Where a provider's credentials stand, as Settings shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialState {
    /// The user entered the credentials and the provider works with them.
    Active,
    /// The user has not entered the credentials the provider needs.
    Missing,
    /// The provider turned the user's key down.
    Rejected,
}

/// What a credential field holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldKind {
    /// A key, token or password: entered masked, never shown again.
    Secret,
    /// A plain value such as an account name.
    Text,
}

/// One value a provider needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CredentialField {
    /// Stable key, for example `api_key`. Also the last part of the credential-store account.
    pub key: &'static str,
    pub kind: FieldKind,
    /// The provider cannot work without it.
    pub required: bool,
}

/// The credentials one provider declares, and where to get them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CredentialSchema {
    /// Provider key, for example `tmdb`.
    pub provider: &'static str,
    /// Name as the provider writes it.
    pub display_name: &'static str,
    /// How much the provider matters; Settings groups by it.
    pub importance: Importance,
    /// The provider's place in the catalogue, from 1 (TMDB): the order Settings lists them in.
    pub rank: u8,
    pub fields: &'static [CredentialField],
    /// The page where a user registers for their own key.
    pub registration_url: &'static str,
}

impl CredentialSchema {
    pub fn field(&self, key: &str) -> Option<&'static CredentialField> {
        self.fields.iter().find(|field| field.key == key)
    }
}

/// A credential value. `Debug` and `Display` never show the content.
#[derive(Clone, PartialEq, Eq)]
pub struct CredentialValue(String);

impl CredentialValue {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The text. Call it only at the point of use, for example while building a request.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for CredentialValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CredentialValue({MASK})")
    }
}

impl fmt::Display for CredentialValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(MASK)
    }
}

/// Why the credential store could not be read or changed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CredentialError {
    /// No provider has that key.
    #[error("unknown provider `{0}`")]
    UnknownProvider(String),
    /// The provider has no such field.
    #[error("{provider} has no credential called `{field}`")]
    UnknownField { provider: String, field: String },
    /// The value cannot be stored (empty, or longer than the store allows).
    #[error("{0}")]
    Invalid(String),
    /// The OS credential store failed.
    #[error("{0}")]
    Store(String),
}

/// The user's own credentials, kept in the OS credential store. The app implements this over its
/// secret store; tests use [`MemoryUserCredentials`].
pub trait UserCredentials: Send + Sync {
    fn get(&self, account: &str) -> Result<Option<CredentialValue>, CredentialError>;
    fn set(&self, account: &str, value: &str) -> Result<(), CredentialError>;
    /// Removes the account. Returns whether anything was removed.
    fn delete(&self, account: &str) -> Result<bool, CredentialError>;
}

/// A process-local user store for tests and for sessions without an OS credential store.
#[derive(Default)]
pub struct MemoryUserCredentials {
    items: std::sync::Mutex<BTreeMap<String, String>>,
}

impl UserCredentials for MemoryUserCredentials {
    fn get(&self, account: &str) -> Result<Option<CredentialValue>, CredentialError> {
        let items = self.items.lock().unwrap_or_else(|e| e.into_inner());
        Ok(items.get(account).cloned().map(CredentialValue))
    }

    fn set(&self, account: &str, value: &str) -> Result<(), CredentialError> {
        let mut items = self.items.lock().unwrap_or_else(|e| e.into_inner());
        items.insert(account.to_owned(), value.to_owned());
        Ok(())
    }

    fn delete(&self, account: &str) -> Result<bool, CredentialError> {
        let mut items = self.items.lock().unwrap_or_else(|e| e.into_inner());
        Ok(items.remove(account).is_some())
    }
}

/// The credential-store account of a field: `<provider>.<field>`, for example `tmdb.api_key`.
pub fn credential_account(provider: &str, field: &str) -> String {
    format!("{provider}.{field}")
}

/// Providers whose key was turned down since the user last saved, removed or tested it. Cloned
/// handles share one set. It holds provider keys only, never values.
#[derive(Clone, Default)]
pub struct RejectionLog(Arc<Mutex<BTreeSet<String>>>);

impl RejectionLog {
    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeSet<String>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn mark(&self, provider: &str) {
        self.lock().insert(provider.to_owned());
    }

    pub fn clear(&self, provider: &str) {
        self.lock().remove(provider);
    }

    pub fn contains(&self, provider: &str) -> bool {
        self.lock().contains(provider)
    }
}

/// The values a provider uses for its requests.
///
/// `Debug` shows keys but never values.
#[derive(Clone, Default)]
pub struct ResolvedCredentials {
    values: BTreeMap<&'static str, CredentialValue>,
    /// Where a rejected key is noted, when these credentials came from a resolver.
    rejections: Option<RejectionLog>,
}

impl ResolvedCredentials {
    /// The value of a field, or `None` when it did not resolve. Provider code only.
    pub fn get(&self, field: &str) -> Option<&CredentialValue> {
        self.values.get(field)
    }

    /// Whether the field has a value.
    pub fn has(&self, field: &str) -> bool {
        self.values.contains_key(field)
    }

    /// Credentials made from values the caller already holds, for example a key a user just
    /// typed and wants to test.
    pub fn from_user_values<'a>(
        schema: &CredentialSchema,
        values: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Result<Self, CredentialError> {
        let mut out = Self::default();
        for (key, value) in values {
            let field = schema
                .field(key)
                .ok_or_else(|| CredentialError::UnknownField {
                    provider: schema.provider.to_owned(),
                    field: key.to_owned(),
                })?;
            let value = value.trim();
            if !value.is_empty() {
                out.values.insert(field.key, CredentialValue::new(value));
            }
        }
        Ok(out)
    }

    /// Layers `self` over `fallback`: fields present here win, the rest come from `fallback`.
    /// The result keeps `self`'s rejection log.
    pub fn over(mut self, fallback: &Self) -> Self {
        for (key, value) in &fallback.values {
            self.values.entry(key).or_insert_with(|| value.clone());
        }
        self
    }

    /// The same values, but a rejection is no longer noted: for keys the user has typed and not
    /// saved, which say nothing about the saved key.
    #[must_use]
    pub fn detached(mut self) -> Self {
        self.rejections = None;
        self
    }

    /// Notes that the provider turned the user's key down.
    pub fn note_rejected(&self, provider: &str) {
        if let Some(log) = &self.rejections {
            log.mark(provider);
        }
    }

    /// The required fields of `schema` that did not resolve.
    pub fn missing(&self, schema: &CredentialSchema) -> Vec<&'static str> {
        schema
            .fields
            .iter()
            .filter(|field| field.required && !self.values.contains_key(field.key))
            .map(|field| field.key)
            .collect()
    }
}

impl fmt::Debug for ResolvedCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut map = f.debug_map();
        for key in self.values.keys() {
            map.entry(key, &MASK);
        }
        map.finish()
    }
}

/// The state of one credential field as Settings may see it. It carries no value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FieldStatus {
    pub key: &'static str,
    pub kind: FieldKind,
    pub required: bool,
    /// The user has entered a value. The value itself never leaves the resolver.
    pub present: bool,
}

/// The state of one provider's credentials as Settings may see it. It carries no value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CredentialStatus {
    pub provider: &'static str,
    pub display_name: &'static str,
    pub registration_url: &'static str,
    pub importance: Importance,
    /// The provider's place in the catalogue, from 1; Settings sorts by it.
    pub rank: u8,
    pub fields: Vec<FieldStatus>,
    /// Every required field resolves, so the provider can be used.
    pub active: bool,
    /// The user has entered at least one value for this provider.
    pub configured: bool,
    /// Active, Missing or Rejected.
    pub state: CredentialState,
    /// Required fields with no value, when inactive.
    pub missing: Vec<&'static str>,
}

/// Picks credential values: the user's, or none.
#[derive(Clone)]
pub struct CredentialResolver {
    schemas: &'static [CredentialSchema],
    user: Arc<dyn UserCredentials>,
    rejections: RejectionLog,
}

impl fmt::Debug for CredentialResolver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CredentialResolver").finish_non_exhaustive()
    }
}

impl CredentialResolver {
    /// A resolver over the catalogue's schemas.
    pub fn new(user: Arc<dyn UserCredentials>) -> Self {
        Self::with_schemas(crate::catalogue::CREDENTIAL_SCHEMAS, user)
    }

    /// A resolver over a custom list of schemas (tests use their own providers).
    pub fn with_schemas(
        schemas: &'static [CredentialSchema],
        user: Arc<dyn UserCredentials>,
    ) -> Self {
        Self {
            schemas,
            user,
            rejections: RejectionLog::default(),
        }
    }

    pub fn schemas(&self) -> &'static [CredentialSchema] {
        self.schemas
    }

    pub fn schema(&self, provider: &str) -> Result<&'static CredentialSchema, CredentialError> {
        self.schemas
            .iter()
            .find(|schema| schema.provider == provider)
            .ok_or_else(|| CredentialError::UnknownProvider(provider.to_owned()))
    }

    fn field(
        &self,
        provider: &str,
        field: &str,
    ) -> Result<(&'static CredentialSchema, &'static CredentialField), CredentialError> {
        let schema = self.schema(provider)?;
        let field = schema
            .field(field)
            .ok_or_else(|| CredentialError::UnknownField {
                provider: provider.to_owned(),
                field: field.to_owned(),
            })?;
        Ok((schema, field))
    }

    fn user_value(
        &self,
        schema: &CredentialSchema,
        field: &CredentialField,
    ) -> Result<Option<CredentialValue>, CredentialError> {
        Ok(self
            .user
            .get(&credential_account(schema.provider, field.key))?
            .filter(|value| !value.expose().is_empty()))
    }

    /// The values to use for `provider`'s requests. Fields that do not resolve are absent; use
    /// [`Self::require`] to turn a missing required field into an error.
    pub fn resolve(&self, provider: &str) -> Result<ResolvedCredentials, CredentialError> {
        let schema = self.schema(provider)?;
        let mut out = ResolvedCredentials {
            values: BTreeMap::new(),
            rejections: Some(self.rejections.clone()),
        };
        for field in schema.fields {
            if let Some(value) = self.user_value(schema, field)? {
                out.values.insert(field.key, value);
            }
        }
        Ok(out)
    }

    /// Like [`Self::resolve`], but fails with [`ProviderError::MissingCredentials`] when a
    /// required field has no value. This is the call a provider makes before a request.
    pub fn require(&self, provider: &str) -> Result<ResolvedCredentials, ProviderError> {
        let schema = self
            .schema(provider)
            .map_err(|e| ProviderError::Other(e.to_string()))?;
        let resolved = self
            .resolve(provider)
            .map_err(|e| ProviderError::Other(e.to_string()))?;
        let missing = resolved.missing(schema);
        if missing.is_empty() {
            Ok(resolved)
        } else {
            Err(ProviderError::MissingCredentials {
                provider: schema.display_name.to_owned(),
                fields: missing.into_iter().map(str::to_owned).collect(),
                registration_url: schema.registration_url.to_owned(),
            })
        }
    }

    /// Whether the provider's required fields all have a value.
    pub fn is_active(&self, provider: &str) -> Result<bool, CredentialError> {
        Ok(self.status(provider)?.active)
    }

    /// What Settings may show about one provider's credentials.
    pub fn status(&self, provider: &str) -> Result<CredentialStatus, CredentialError> {
        let schema = self.schema(provider)?;
        let mut fields = Vec::with_capacity(schema.fields.len());
        let mut missing = Vec::new();
        let mut configured = false;
        for field in schema.fields {
            let present = self.user_value(schema, field)?.is_some();
            if field.required && !present {
                missing.push(field.key);
            }
            configured |= present;
            fields.push(FieldStatus {
                key: field.key,
                kind: field.kind,
                required: field.required,
                present,
            });
        }
        let active = missing.is_empty();
        let state = if configured && self.rejections.contains(schema.provider) {
            CredentialState::Rejected
        } else if active && configured {
            CredentialState::Active
        } else {
            CredentialState::Missing
        };
        Ok(CredentialStatus {
            provider: schema.provider,
            display_name: schema.display_name,
            registration_url: schema.registration_url,
            importance: schema.importance,
            rank: schema.rank,
            fields,
            active,
            configured,
            state,
            missing,
        })
    }

    /// The status of every provider that has credentials, in catalogue order.
    pub fn statuses(&self) -> Result<Vec<CredentialStatus>, CredentialError> {
        self.schemas
            .iter()
            .map(|schema| self.status(schema.provider))
            .collect()
    }

    /// The providers of `importance` whose key is missing or rejected, in catalogue order. The
    /// banner lists the required and recommended ones.
    pub fn needing_attention(
        &self,
        importance: Importance,
    ) -> Result<Vec<&'static CredentialSchema>, CredentialError> {
        let mut out = Vec::new();
        for schema in self.schemas.iter().filter(|s| s.importance == importance) {
            if self.status(schema.provider)?.state != CredentialState::Active {
                out.push(schema);
            }
        }
        Ok(out)
    }

    /// Saves the user's own value for a field. A saved key gets a fresh start: an earlier
    /// rejection is forgotten until the provider turns it down again.
    pub fn set_user(
        &self,
        provider: &str,
        field: &str,
        value: &str,
    ) -> Result<(), CredentialError> {
        let (schema, field) = self.field(provider, field)?;
        let value = value.trim();
        if value.is_empty() {
            return Err(CredentialError::Invalid(
                "the value is empty; remove the credential instead".to_owned(),
            ));
        }
        self.user
            .set(&credential_account(schema.provider, field.key), value)?;
        self.rejections.clear(schema.provider);
        Ok(())
    }

    /// Removes the user's own value for a field. Returns whether one existed.
    pub fn clear_user(&self, provider: &str, field: &str) -> Result<bool, CredentialError> {
        let (schema, field) = self.field(provider, field)?;
        let removed = self
            .user
            .delete(&credential_account(schema.provider, field.key))?;
        self.rejections.clear(schema.provider);
        Ok(removed)
    }

    /// Removes every credential the user saved for `provider`. Returns whether any existed.
    pub fn remove_all(&self, provider: &str) -> Result<bool, CredentialError> {
        let schema = self.schema(provider)?;
        let mut removed = false;
        for field in schema.fields {
            removed |= self
                .user
                .delete(&credential_account(schema.provider, field.key))?;
        }
        self.rejections.clear(schema.provider);
        Ok(removed)
    }

    /// Notes that the provider turned the saved key down (a failed Test of the saved key).
    pub fn note_rejected(&self, provider: &str) {
        self.rejections.mark(provider);
    }

    /// Forgets an earlier rejection (a Test of the saved key succeeded).
    pub fn note_accepted(&self, provider: &str) {
        self.rejections.clear(provider);
    }
}

#[cfg(test)]
mod tests;
