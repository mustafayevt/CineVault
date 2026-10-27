//! Secret storage in the operating system credential store.
//!
//! API keys, passwords and OAuth tokens live in Windows Credential Manager, the macOS Keychain or
//! the Secret Service on Linux (GNOME Keyring or KWallet), through the `keyring` crate, never in
//! the settings file. Settings only hold non-secret facts such as "a TMDB key is configured",
//! which the UI learns from [`SecretStore::contains`].
//!
//! Every value that passes through a [`SecretStore`] is registered with [`crate::redact`], so it
//! is masked in logs even when it shows up somewhere no pattern rule would catch it.
//!
//! Notes:
//! * Secrets are addressed by an account name such as `tmdb.api_key` or `trakt.access_token`
//!   under one service name per app (see [`SERVICE_NAME`]). A portable copy has a service of its
//!   own ([`service_name_for`]), so it starts with no keys and never changes an installed copy's.
//! * Windows limits one credential to about 1,200 characters. Provider keys and tokens fit.
//! * Linux has no plain-text fallback. When no Secret Service answers (none running, or locked),
//!   saving fails with [`SecretError::NoKeyring`] and the Settings page says so; no key is written
//!   to a file.
//! * [`SecretStore::in_memory`] exists for tests and for sessions where no credential store is
//!   reachable; its contents vanish when the process ends.

use std::collections::HashMap;
use std::fmt;
use std::path::Path;
use std::sync::Mutex;

use sha2::{Digest, Sha256};

use crate::data_location::{DataLocation, LocationMode};
use crate::redact;

/// Service name under which CineVault's secrets are filed in the credential store.
pub const SERVICE_NAME: &str = "CineVault";

/// The file in a portable data folder that names its own credential-store service.
pub const PORTABLE_SECRETS_ID_FILE: &str = "credential-store-id";

/// The credential-store service a data location files its secrets under.
///
/// An installed copy (per-user or custom data folder) uses [`SERVICE_NAME`]. A portable copy uses
/// a service of its own, so it never reads or overwrites the keys of an installed copy on the
/// same machine, nor those of another portable copy: `CineVault portable <id>`, where the id is
/// kept in [`PORTABLE_SECRETS_ID_FILE`] in its data folder. The id is random and made once, so
/// the copy keeps its keys when its drive letter or folder changes. When the file cannot be
/// written (read-only media), the id is derived from the data folder's path instead.
pub fn service_name_for(location: &DataLocation) -> String {
    if location.mode() != LocationMode::Portable {
        return SERVICE_NAME.to_owned();
    }
    let id = portable_id(location.root()).unwrap_or_else(|error| {
        tracing::warn!(%error, "the portable credential id could not be saved; using the path");
        path_id(location.root())
    });
    format!("{SERVICE_NAME} portable {id}")
}

fn is_id(text: &str) -> bool {
    text.len() == 32 && text.bytes().all(|b| b.is_ascii_hexdigit())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The id stored in the data folder, made and saved (atomically) on first use.
fn portable_id(root: &Path) -> std::io::Result<String> {
    let path = root.join(PORTABLE_SECRETS_ID_FILE);
    if let Ok(text) = std::fs::read_to_string(&path) {
        let text = text.trim().to_ascii_lowercase();
        if is_id(&text) {
            return Ok(text);
        }
    }
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|error| std::io::Error::other(error.to_string()))?;
    let id = hex(&bytes);
    std::fs::create_dir_all(root)?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, format!("{id}\n"))?;
    std::fs::rename(&tmp, &path)?;
    Ok(id)
}

/// An id from the data folder's path, for when none can be stored.
fn path_id(root: &Path) -> String {
    let folded = root.to_string_lossy().to_lowercase();
    hex(&Sha256::digest(folded.as_bytes())[..16])
}

/// Errors from the credential store.
#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    /// No credential store is reachable (no desktop session, locked or missing keychain).
    #[error("the system credential store is not available: {0}")]
    Unavailable(String),
    /// On Linux, no Secret Service (GNOME Keyring or KWallet) is running or unlocked.
    #[error(
        "No system keyring is running. Install or unlock GNOME Keyring or KWallet, then try again."
    )]
    NoKeyring,
    /// The store rejected the request.
    #[error("credential store error: {0}")]
    Store(String),
    /// The account name or value cannot be stored.
    #[error("invalid secret: {0}")]
    Invalid(String),
}

/// Whether a Secret Service failure says that no keyring is on the session bus: the service is
/// not provided, or there is no session bus to reach it on. Pure, so it is tested on every OS.
fn is_missing_service(text: &str) -> bool {
    [
        "org.freedesktop.secrets",
        "ServiceUnknown",
        "was not provided by any",
        "DBUS_SESSION_BUS_ADDRESS",
        "Connection refused",
    ]
    .iter()
    .any(|marker| text.contains(marker))
}

impl SecretError {
    fn from_keyring(err: keyring::Error) -> Self {
        match err {
            keyring::Error::NoStorageAccess(e) => {
                if cfg!(target_os = "linux") {
                    Self::NoKeyring
                } else {
                    Self::Unavailable(e.to_string())
                }
            }
            keyring::Error::PlatformFailure(e) => {
                let text = e.to_string();
                if cfg!(target_os = "linux") && is_missing_service(&text) {
                    Self::NoKeyring
                } else {
                    Self::Store(text)
                }
            }
            keyring::Error::TooLong(name, max) => {
                Self::Invalid(format!("{name} is longer than the store allows ({max})"))
            }
            keyring::Error::Invalid(attr, why) => Self::Invalid(format!("{attr}: {why}")),
            other => Self::Store(other.to_string()),
        }
    }
}

/// A secret value. `Debug` and `Display` never show the content, so it cannot leak through
/// `{:?}` formatting or a derived `Debug` on a struct that holds it.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The secret text. Call this only at the point of use, for example when building a request.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(")?;
        f.write_str(redact::MASK)?;
        f.write_str(")")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(redact::MASK)
    }
}

/// Where secrets are physically kept.
pub trait SecretBackend: Send + Sync {
    fn set(&self, account: &str, value: &str) -> Result<(), SecretError>;
    fn get(&self, account: &str) -> Result<Option<String>, SecretError>;
    /// Removes the account. Returns whether anything was removed.
    fn delete(&self, account: &str) -> Result<bool, SecretError>;
}

/// The OS credential store (Windows Credential Manager, macOS Keychain, Linux Secret Service).
#[derive(Debug, Clone)]
pub struct KeyringBackend {
    service: String,
}

impl KeyringBackend {
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }

    fn entry(&self, account: &str) -> Result<keyring::Entry, SecretError> {
        keyring::Entry::new(&self.service, account).map_err(SecretError::from_keyring)
    }
}

impl SecretBackend for KeyringBackend {
    fn set(&self, account: &str, value: &str) -> Result<(), SecretError> {
        self.entry(account)?
            .set_password(value)
            .map_err(SecretError::from_keyring)
    }

    fn get(&self, account: &str) -> Result<Option<String>, SecretError> {
        match self.entry(account)?.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(SecretError::from_keyring(e)),
        }
    }

    fn delete(&self, account: &str) -> Result<bool, SecretError> {
        match self.entry(account)?.delete_credential() {
            Ok(()) => Ok(true),
            Err(keyring::Error::NoEntry) => Ok(false),
            Err(e) => Err(SecretError::from_keyring(e)),
        }
    }
}

/// A process-local store. Nothing is persisted.
#[derive(Debug, Default)]
pub struct MemoryBackend {
    items: Mutex<HashMap<String, String>>,
}

impl SecretBackend for MemoryBackend {
    fn set(&self, account: &str, value: &str) -> Result<(), SecretError> {
        self.items
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(account.into(), value.into());
        Ok(())
    }

    fn get(&self, account: &str) -> Result<Option<String>, SecretError> {
        Ok(self
            .items
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(account)
            .cloned())
    }

    fn delete(&self, account: &str) -> Result<bool, SecretError> {
        Ok(self
            .items
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(account)
            .is_some())
    }
}

/// Secret storage used by the app services.
pub struct SecretStore {
    backend: Box<dyn SecretBackend>,
}

impl fmt::Debug for SecretStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecretStore").finish_non_exhaustive()
    }
}

impl SecretStore {
    /// The OS credential store under [`SERVICE_NAME`].
    pub fn system() -> Self {
        Self::system_with_service(SERVICE_NAME)
    }

    /// The OS credential store under the service of a data location ([`service_name_for`]): a
    /// portable copy keeps its secrets apart from an installed one.
    pub fn for_location(location: &DataLocation) -> Self {
        Self::system_with_service(service_name_for(location))
    }

    /// The OS credential store under a custom service name (tests use a unique one).
    pub fn system_with_service(service: impl Into<String>) -> Self {
        Self::with_backend(KeyringBackend::new(service))
    }

    /// A store that keeps everything in memory.
    pub fn in_memory() -> Self {
        Self::with_backend(MemoryBackend::default())
    }

    pub fn with_backend(backend: impl SecretBackend + 'static) -> Self {
        Self {
            backend: Box::new(backend),
        }
    }

    fn check_account(account: &str) -> Result<(), SecretError> {
        if account.trim().is_empty() {
            return Err(SecretError::Invalid("account name is empty".into()));
        }
        Ok(())
    }

    /// Saves (or replaces) a secret and registers it for log redaction.
    pub fn set(&self, account: &str, value: &str) -> Result<(), SecretError> {
        Self::check_account(account)?;
        if value.is_empty() {
            return Err(SecretError::Invalid(
                "secret value is empty; use delete to remove it".into(),
            ));
        }
        // Register first so the value is masked even if the store call logs an error.
        redact::register_secret(value);
        self.backend.set(account, value)
    }

    /// Reads a secret, or `None` when none is saved. The value is registered for log redaction.
    pub fn get(&self, account: &str) -> Result<Option<Secret>, SecretError> {
        Self::check_account(account)?;
        let value = self.backend.get(account)?;
        if let Some(v) = &value {
            redact::register_secret(v);
        }
        Ok(value.map(Secret))
    }

    /// Whether a secret is saved under this account.
    pub fn contains(&self, account: &str) -> Result<bool, SecretError> {
        Ok(self.get(account)?.is_some())
    }

    /// Removes a secret. Returns whether one existed.
    pub fn delete(&self, account: &str) -> Result<bool, SecretError> {
        Self::check_account(account)?;
        if let Some(v) = self.backend.get(account)? {
            redact::unregister_secret(&v);
        }
        self.backend.delete(account)
    }
}

#[cfg(test)]
mod tests;
