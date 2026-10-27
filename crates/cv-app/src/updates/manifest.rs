//! The signed update manifest.
//!
//! The channel serves a small JSON envelope:
//!
//! ```json
//! { "payload": "<base64 of the payload JSON bytes>", "signature": "<base64 ed25519 signature>" }
//! ```
//!
//! The signature covers the exact payload bytes, so nothing has to be put into a canonical form.
//! The payload is JSON as well:
//!
//! ```json
//! {
//!   "version": "1.2.0",
//!   "pub_date": "2026-11-01T10:00:00Z",
//!   "notes": "What is new, as plain text or Markdown.",
//!   "platforms": {
//!     "windows-x86_64": { "url": "https://.../CineVault-1.2.0-setup.exe", "sha256": "<hex>", "size": 123456 },
//!     "darwin-aarch64": { "url": "...", "sha256": "...", "size": 1 }
//!   }
//! }
//! ```
//!
//! Platform ids are `<os>-<arch>` with `windows` or `darwin` and `x86_64` or `aarch64`.

use std::collections::BTreeMap;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use semver::Version;
use serde::Deserialize;

use super::UpdateError;

/// The public key (base64, 32 bytes) releases are signed with (`bun run release:keys`). A key in Settings (`general.update-public-key`) wins over it,
/// which is how the local test channel works.
pub const BUILT_IN_PUBLIC_KEY: Option<&str> = Some("53G71tWBhUldHsBBE+aasXwgAOehYaPDLRb8HZNFUNs=");

/// The biggest envelope read from a channel.
pub const MAX_MANIFEST_BYTES: u64 = 512 * 1024;

#[derive(Deserialize)]
struct Envelope {
    payload: String,
    signature: String,
}

#[derive(Deserialize)]
struct Payload {
    version: String,
    #[serde(default)]
    pub_date: Option<String>,
    #[serde(default)]
    notes: String,
    #[serde(default)]
    platforms: BTreeMap<String, ManifestArtifact>,
}

/// One downloadable build.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ManifestArtifact {
    pub url: String,
    /// Lower-case or upper-case hex SHA-256 of the file.
    pub sha256: String,
    /// Exact size in bytes.
    pub size: u64,
}

/// A manifest whose signature was verified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub version: Version,
    pub published: Option<String>,
    pub notes: String,
    pub platforms: BTreeMap<String, ManifestArtifact>,
}

/// The platform id of this build, for example `windows-x86_64`.
#[must_use]
pub fn platform_id() -> String {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    };
    format!("{os}-{}", std::env::consts::ARCH)
}

/// Reads a public key written as base64 of its 32 bytes.
///
/// # Errors
/// [`UpdateError::BadKey`] when the text is not such a key.
pub fn parse_public_key(text: &str) -> Result<VerifyingKey, UpdateError> {
    let bytes = STANDARD
        .decode(text.trim())
        .map_err(|_| UpdateError::BadKey)?;
    let bytes: [u8; 32] = bytes.try_into().map_err(|_| UpdateError::BadKey)?;
    VerifyingKey::from_bytes(&bytes).map_err(|_| UpdateError::BadKey)
}

/// Checks the signature of `body` (the envelope) against `key` and reads the payload. Nothing
/// inside the payload is looked at before the signature has been verified.
///
/// # Errors
/// [`UpdateError::BadSignature`] when the signature is missing, malformed or wrong, and
/// [`UpdateError::BadManifest`] when the signed content is not a manifest.
pub fn verify_manifest(body: &[u8], key: &VerifyingKey) -> Result<Manifest, UpdateError> {
    let envelope: Envelope = serde_json::from_slice(body)
        .map_err(|error| UpdateError::BadManifest(format!("it is not valid JSON ({error})")))?;
    let payload = STANDARD
        .decode(envelope.payload.trim())
        .map_err(|_| UpdateError::BadManifest("its payload is not base64".to_owned()))?;
    let signature = STANDARD
        .decode(envelope.signature.trim())
        .ok()
        .and_then(|bytes| Signature::from_slice(&bytes).ok())
        .ok_or(UpdateError::BadSignature)?;
    key.verify(&payload, &signature)
        .map_err(|_| UpdateError::BadSignature)?;

    let payload: Payload = serde_json::from_slice(&payload)
        .map_err(|error| UpdateError::BadManifest(format!("its content is invalid ({error})")))?;
    let version = Version::parse(payload.version.trim().trim_start_matches('v')).map_err(|_| {
        UpdateError::BadManifest(format!("{:?} is not a version number", payload.version))
    })?;
    Ok(Manifest {
        version,
        published: payload.pub_date,
        notes: payload.notes,
        platforms: payload.platforms,
    })
}
