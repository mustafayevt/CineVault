//! Streaming download with SHA-256 verification.
//!
//! The body is hashed while it is written, so a download is never read twice. The file is
//! deleted when the hash differs, when it grows past its pinned size, or when the user cancels.

use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use sha2::{Digest, Sha256};

use super::ToolError;

/// A flag the user flips to stop a download. Clones share the flag.
#[derive(Debug, Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Asks the download to stop at its next chunk.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

const CHUNK: usize = 64 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
/// A whole download, however slow the line. The biggest pinned file is about 110 MB.
const TOTAL_TIMEOUT: Duration = Duration::from_secs(60 * 60);

/// Lower-case hex of `bytes`.
pub(crate) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, b| {
            let _ = write!(out, "{b:02x}");
            out
        })
}

/// Whether `url` may be fetched: `https`, or plain `http` to this computer (the test server).
fn check_url(url: &str) -> Result<bool, ToolError> {
    if url.starts_with("https://") {
        return Ok(true);
    }
    if let Some(rest) = url.strip_prefix("http://") {
        let host = rest.split(['/', ':', '?', '#']).next().unwrap_or_default();
        if host == "127.0.0.1" || host == "localhost" || rest.starts_with("[::1]") {
            return Ok(false);
        }
    }
    Err(ToolError::InsecureUrl {
        url: url.to_owned(),
    })
}

fn agent(https_only: bool) -> ureq::Agent {
    ureq::Agent::config_builder()
        .https_only(https_only)
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_global(Some(TOTAL_TIMEOUT))
        .user_agent(concat!("CineVault/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

/// Downloads `url` into `dest`, which must not exist yet, and checks it against `sha256`.
///
/// `on_bytes` receives the running total after every chunk. On any failure `dest` is removed.
///
/// # Errors
/// [`ToolError::ChecksumMismatch`] when the hash differs, [`ToolError::TooLarge`] when the body
/// passes `max_bytes`, [`ToolError::Cancelled`], [`ToolError::InsecureUrl`], or a network and
/// file error.
pub(crate) fn download_verified(
    url: &str,
    sha256: &str,
    max_bytes: u64,
    dest: &Path,
    cancel: &Cancel,
    on_bytes: &mut dyn FnMut(u64),
) -> Result<(), ToolError> {
    let https_only = check_url(url)?;
    let result = fetch(url, sha256, max_bytes, dest, cancel, on_bytes, https_only);
    if result.is_err() {
        let _ = std::fs::remove_file(dest);
    }
    result
}

fn fetch(
    url: &str,
    sha256: &str,
    max_bytes: u64,
    dest: &Path,
    cancel: &Cancel,
    on_bytes: &mut dyn FnMut(u64),
    https_only: bool,
) -> Result<(), ToolError> {
    let network = |error: ureq::Error| ToolError::Download {
        url: url.to_owned(),
        message: describe(&error),
    };
    let mut response = agent(https_only).get(url).call().map_err(network)?;
    let body = response.body_mut();
    if body
        .content_length()
        .is_some_and(|length| length > max_bytes)
    {
        return Err(ToolError::TooLarge {
            url: url.to_owned(),
            limit: max_bytes,
        });
    }
    // One byte past the limit is enough to tell that the body is too long.
    let mut reader = body.with_config().limit(max_bytes + 1).reader();
    let mut file = File::create_new(dest)
        .map_err(|error| ToolError::io(format!("Could not create {}", dest.display()), error))?;
    let write_error = |error| ToolError::io(format!("Could not write {}", dest.display()), error);

    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; CHUNK];
    let mut total = 0_u64;
    loop {
        if cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        let read = match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) => {
                return Err(ToolError::Download {
                    url: url.to_owned(),
                    message: error.to_string(),
                });
            }
        };
        total += read as u64;
        if total > max_bytes {
            return Err(ToolError::TooLarge {
                url: url.to_owned(),
                limit: max_bytes,
            });
        }
        hasher.update(&buffer[..read]);
        file.write_all(&buffer[..read]).map_err(write_error)?;
        on_bytes(total);
    }
    file.flush().map_err(write_error)?;
    file.sync_all().map_err(write_error)?;

    let actual = hex(hasher.finalize().as_slice());
    if actual != sha256 {
        tracing::warn!(url, expected = sha256, actual = %actual, "download failed its checksum");
        return Err(ToolError::ChecksumMismatch {
            url: url.to_owned(),
            expected: sha256.to_owned(),
            actual,
        });
    }
    Ok(())
}

fn describe(error: &ureq::Error) -> String {
    match error {
        ureq::Error::StatusCode(code) => format!("the server answered {code}"),
        ureq::Error::HostNotFound => "the server could not be found".to_owned(),
        ureq::Error::ConnectionFailed => "the connection failed".to_owned(),
        ureq::Error::Timeout(_) => "the server took too long to answer".to_owned(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_https_and_loopback_addresses_are_allowed() {
        assert_eq!(check_url("https://example.com/a").ok(), Some(true));
        assert_eq!(check_url("http://127.0.0.1:8080/a").ok(), Some(false));
        assert_eq!(check_url("http://localhost/a").ok(), Some(false));
        assert!(check_url("http://example.com/a").is_err());
        assert!(check_url("http://127.0.0.1.example.com/a").is_err());
        assert!(check_url("ftp://127.0.0.1/a").is_err());
    }

    #[test]
    fn hex_is_lower_case_and_padded() {
        assert_eq!(hex(&[0x00, 0x0a, 0xff]), "000aff");
    }
}
