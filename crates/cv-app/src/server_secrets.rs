//! Provider keys for the browser server, encrypted at rest.
//!
//! The desktop keeps its keys in the operating system credential store. A headless server has no
//! session bus to unlock a keyring, so it keeps `secrets.enc` in the data folder instead: a JSON
//! document of account → secret, encrypted with ChaCha20-Poly1305 under a 32-byte master key. The
//! master key comes from `CINEVAULT_SECRET_KEY` (base64) or `CINEVAULT_SECRET_KEY_FILE` (a file
//! holding the base64 key), a systemd credential or a Docker secret. It never lives in the data
//! folder, the image or `settings.json`.
//!
//! When the key is missing the server still starts: [`crate::services::AppServices`] can boot with
//! an in-memory store, health says `"keys":"locked"`, and Settings explains how to supply the key.
//! Nothing is written in plain text, and a wrong key never overwrites the existing file.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};

use crate::secrets::{SecretBackend, SecretError, SecretStore};

/// The file the encrypted keys live in, inside the data folder.
pub const SECRETS_FILE: &str = "secrets.enc";
const VERSION: u32 = 1;
const KEY_BYTES: usize = 32;
const NONCE_BYTES: usize = 12;

#[derive(Debug, Serialize, Deserialize)]
struct Envelope {
    version: u32,
    nonce: String,
    ciphertext: String,
}

/// A store of provider keys encrypted with a master key held outside the data folder.
pub struct EncryptedFileBackend {
    path: PathBuf,
    key: [u8; KEY_BYTES],
    values: Mutex<BTreeMap<String, String>>,
}

impl EncryptedFileBackend {
    /// Opens `secrets.enc` under `data_root`, decrypting it with `key`.
    ///
    /// # Errors
    /// The file exists but cannot be read, is not a recognised envelope, or the key cannot
    /// decrypt it (a wrong key is refused rather than overwriting the file).
    pub fn open(data_root: &Path, key: [u8; KEY_BYTES]) -> Result<Self, SecretError> {
        let path = data_root.join(SECRETS_FILE);
        let values = match std::fs::read(&path) {
            Ok(bytes) => decrypt(&key, &bytes)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(error) => return Err(SecretError::Store(error.to_string())),
        };
        Ok(Self {
            path,
            key,
            values: Mutex::new(values),
        })
    }

    fn lock(&self) -> MutexGuard<'_, BTreeMap<String, String>> {
        self.values.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn write(&self, values: &BTreeMap<String, String>) -> Result<(), SecretError> {
        let plain =
            serde_json::to_vec(values).map_err(|error| SecretError::Store(error.to_string()))?;
        let mut nonce = [0u8; NONCE_BYTES];
        OsRng.fill_bytes(&mut nonce);
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&self.key));
        let ciphertext = cipher
            .encrypt(Nonce::from_slice(&nonce), plain.as_ref())
            .map_err(|_| {
                SecretError::Store("the secrets file could not be encrypted".to_owned())
            })?;
        let envelope = Envelope {
            version: VERSION,
            nonce: STANDARD.encode(nonce),
            ciphertext: STANDARD.encode(ciphertext),
        };
        let bytes =
            serde_json::to_vec(&envelope).map_err(|error| SecretError::Store(error.to_string()))?;
        cv_fs::write_atomic_with_mode(&self.path, &bytes, 0o600)
            .map_err(|error| SecretError::Store(error.to_string()))
    }
}

impl SecretBackend for EncryptedFileBackend {
    fn set(&self, account: &str, value: &str) -> Result<(), SecretError> {
        if account.trim().is_empty() {
            return Err(SecretError::Invalid("account name is empty".into()));
        }
        let next = {
            let values = self.lock();
            let mut next = values.clone();
            next.insert(account.to_owned(), value.to_owned());
            next
        };
        self.write(&next)?;
        *self.lock() = next;
        Ok(())
    }

    fn get(&self, account: &str) -> Result<Option<String>, SecretError> {
        Ok(self.lock().get(account).cloned())
    }

    fn delete(&self, account: &str) -> Result<bool, SecretError> {
        let (next, removed) = {
            let values = self.lock();
            let mut next = values.clone();
            let removed = next.remove(account).is_some();
            (next, removed)
        };
        if removed {
            self.write(&next)?;
            *self.lock() = next;
        }
        Ok(removed)
    }
}

fn decrypt(key: &[u8; KEY_BYTES], bytes: &[u8]) -> Result<BTreeMap<String, String>, SecretError> {
    let envelope: Envelope = serde_json::from_slice(bytes)
        .map_err(|_| SecretError::Store("the secrets file is damaged".to_owned()))?;
    if envelope.version != VERSION {
        return Err(SecretError::Store(
            "the secrets file has a newer format".to_owned(),
        ));
    }
    let nonce = STANDARD
        .decode(&envelope.nonce)
        .map_err(|_| SecretError::Store("the secrets file is damaged".to_owned()))?;
    let ciphertext = STANDARD
        .decode(&envelope.ciphertext)
        .map_err(|_| SecretError::Store("the secrets file is damaged".to_owned()))?;
    if nonce.len() != NONCE_BYTES {
        return Err(SecretError::Store("the secrets file is damaged".to_owned()));
    }
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let plain = cipher
        .decrypt(Nonce::from_slice(&nonce), ciphertext.as_ref())
        .map_err(|_| {
            SecretError::Store(
                "the master key cannot decrypt the secrets file; check CINEVAULT_SECRET_KEY_FILE"
                    .to_owned(),
            )
        })?;
    serde_json::from_slice(&plain)
        .map_err(|_| SecretError::Store("the secrets file is damaged".to_owned()))
}

/// The master key from the environment: `CINEVAULT_SECRET_KEY_FILE` first, then
/// `CINEVAULT_SECRET_KEY`, both base64 of exactly 32 bytes. `Ok(None)` means no key is configured.
///
/// # Errors
/// A path or value is set but cannot be read or is not a 32-byte base64 key.
pub fn master_key_from_environment() -> Result<Option<[u8; KEY_BYTES]>, String> {
    let encoded = if let Some(path) = std::env::var_os("CINEVAULT_SECRET_KEY_FILE") {
        let path = PathBuf::from(path);
        Some(
            std::fs::read_to_string(&path).map_err(|error| {
                format!("CINEVAULT_SECRET_KEY_FILE {}: {error}", path.display())
            })?,
        )
    } else {
        std::env::var_os("CINEVAULT_SECRET_KEY").map(|value| value.to_string_lossy().into_owned())
    };
    let Some(encoded) = encoded else {
        return Ok(None);
    };
    let bytes = STANDARD
        .decode(encoded.trim())
        .map_err(|_| "The master key must be base64 of 32 bytes.".to_owned())?;
    let key: [u8; KEY_BYTES] = bytes
        .try_into()
        .map_err(|_| "The master key must be base64 of 32 bytes.".to_owned())?;
    Ok(Some(key))
}

/// The server's key store, or an in-memory one when no master key is configured.
///
/// # Errors
/// The key is configured but unreadable or the secrets file cannot be decrypted with it.
pub fn server_store(data_root: &Path) -> Result<(SecretStore, bool), String> {
    match master_key_from_environment()? {
        Some(key) => {
            let backend =
                EncryptedFileBackend::open(data_root, key).map_err(|error| error.to_string())?;
            Ok((SecretStore::with_backend(backend), false))
        }
        None => Ok((SecretStore::in_memory(), true)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> [u8; KEY_BYTES] {
        [7u8; KEY_BYTES]
    }

    #[test]
    fn a_key_survives_a_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let backend = EncryptedFileBackend::open(dir.path(), key()).unwrap();
        backend.set("tmdb.api_key", "secret-value").unwrap();
        drop(backend);
        let again = EncryptedFileBackend::open(dir.path(), key()).unwrap();
        assert_eq!(
            again.get("tmdb.api_key").unwrap().as_deref(),
            Some("secret-value")
        );
    }

    #[test]
    fn a_wrong_key_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let backend = EncryptedFileBackend::open(dir.path(), key()).unwrap();
        backend.set("tmdb.api_key", "secret-value").unwrap();
        let wrong = EncryptedFileBackend::open(dir.path(), [9u8; KEY_BYTES]);
        assert!(wrong.is_err());
    }

    #[test]
    fn delete_removes_and_persists() {
        let dir = tempfile::tempdir().unwrap();
        let backend = EncryptedFileBackend::open(dir.path(), key()).unwrap();
        backend.set("tmdb.api_key", "secret-value").unwrap();
        assert!(backend.delete("tmdb.api_key").unwrap());
        assert!(!backend.delete("tmdb.api_key").unwrap());
        drop(backend);
        let again = EncryptedFileBackend::open(dir.path(), key()).unwrap();
        assert!(again.get("tmdb.api_key").unwrap().is_none());
    }

    #[test]
    fn the_file_holds_no_plain_text() {
        let dir = tempfile::tempdir().unwrap();
        let backend = EncryptedFileBackend::open(dir.path(), key()).unwrap();
        backend.set("tmdb.api_key", "secret-value").unwrap();
        let bytes = std::fs::read_to_string(dir.path().join(SECRETS_FILE)).unwrap();
        assert!(!bytes.contains("secret-value"));
        assert!(bytes.contains("ciphertext"));
    }

    #[cfg(unix)]
    #[test]
    fn the_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let backend = EncryptedFileBackend::open(dir.path(), key()).unwrap();
        backend.set("tmdb.api_key", "secret-value").unwrap();
        let mode = std::fs::metadata(dir.path().join(SECRETS_FILE))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }
}
