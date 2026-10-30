//! The one-writer owner file.
//!
//! SQLite tolerates one writer per library. The desktop app and the server cannot both open the
//! same data folder, and a command-line batch must not open it behind a running server either.
//! [`acquire`] takes an exclusive lock on `instance.lock` and writes `library-owner.json` with the
//! role, process, host and a heartbeat. A clean exit removes the file; a crashed process leaves it
//! behind until the heartbeat is older than [`STALE_AFTER`], after which the next process takes
//! over.
//!
//! The lock is a local filesystem lock. The docs say the data folder must not be an SMB or NFS
//! share, where advisory locks are not dependable.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// The lock file, in the data folder.
pub const LOCK_FILE: &str = "instance.lock";
/// The owner record, in the data folder.
pub const OWNER_FILE: &str = "library-owner.json";
/// The protocol version of the owner record.
pub const PROTOCOL: u32 = 1;
/// How often the owner rewrites its heartbeat.
pub const HEARTBEAT: Duration = Duration::from_secs(5);
/// How old a heartbeat may be before the owner is considered gone.
pub const STALE_AFTER: Duration = Duration::from_secs(30);

/// What a role is called in a sentence.
#[must_use]
pub fn label(owner: &Owner) -> &'static str {
    match owner.role.as_str() {
        "server" => "The CineVault server",
        "app" => "The CineVault app",
        "cli" => "A CineVault command-line batch",
        _ => "Another CineVault process",
    }
}

/// One process's claim on a data folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Owner {
    /// `app`, `server` or `cli`.
    pub role: String,
    pub pid: u32,
    /// The host name, for a message when two machines share a folder.
    pub host: String,
    /// Unix milliseconds when the process started.
    pub started_ms: u64,
    /// Unix milliseconds of the last heartbeat.
    pub heartbeat_ms: u64,
    pub protocol: u32,
}

impl Owner {
    /// The sentence a second process shows.
    #[must_use]
    pub fn sentence(&self) -> String {
        format!(
            "{} is using this library (process {} on {}).",
            label(self),
            self.pid,
            self.host
        )
    }
}

/// Why the owner file could not be taken.
#[derive(Debug, thiserror::Error)]
pub enum OwnerError {
    #[error("could not create {path}: {source}")]
    Create {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not lock {path}: {source}")]
    Lock {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not write {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// A held claim. Keep it alive for as long as this process writes the library; dropping it removes
/// the owner file and releases the lock.
pub struct OwnerGuard {
    path: PathBuf,
    stop: Arc<AtomicBool>,
    heartbeat: Option<JoinHandle<()>>,
    _lock: File,
}

impl Drop for OwnerGuard {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.heartbeat.take() {
            let _ = handle.join();
        }
        let _ = std::fs::remove_file(&self.path);
        // `_lock` closes after this, releasing the operating-system lock.
    }
}

/// Takes the data folder, or returns `None` when another live process holds it.
///
/// # Errors
/// The folder or the lock file cannot be created, or the owner record cannot be written.
pub fn acquire(data_root: &Path, role: &str) -> Result<Option<OwnerGuard>, OwnerError> {
    std::fs::create_dir_all(data_root).map_err(|source| OwnerError::Create {
        path: data_root.to_path_buf(),
        source,
    })?;
    let lock_path = data_root.join(LOCK_FILE);
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|source| OwnerError::Create {
            path: lock_path.clone(),
            source,
        })?;
    match fs4::FileExt::try_lock(&file) {
        Ok(()) => {}
        Err(fs4::TryLockError::WouldBlock) => return Ok(None),
        Err(fs4::TryLockError::Error(source)) => {
            return Err(OwnerError::Lock {
                path: lock_path,
                source,
            });
        }
    }
    let path = data_root.join(OWNER_FILE);
    let owner = Owner {
        role: role.to_owned(),
        pid: std::process::id(),
        host: host_name(),
        started_ms: now_ms(),
        heartbeat_ms: now_ms(),
        protocol: PROTOCOL,
    };
    write(&path, &owner)?;

    let stop = Arc::new(AtomicBool::new(false));
    let heartbeat_stop = Arc::clone(&stop);
    let heartbeat_path = path.clone();
    let heartbeat = thread::spawn(move || {
        let mut waited = Duration::ZERO;
        while !heartbeat_stop.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(500));
            waited += Duration::from_millis(500);
            if waited < HEARTBEAT {
                continue;
            }
            waited = Duration::ZERO;
            let live = Owner {
                heartbeat_ms: now_ms(),
                ..owner.clone()
            };
            if write(&heartbeat_path, &live).is_err() {
                // A failed heartbeat is not fatal: the library is still held by the lock, and the
                // next successful write catches up.
            }
        }
    });
    Ok(Some(OwnerGuard {
        path,
        stop,
        heartbeat: Some(heartbeat),
        _lock: file,
    }))
}

/// Reads the owner record, if there is one.
#[must_use]
pub fn read_owner(data_root: &Path) -> Option<Owner> {
    let bytes = std::fs::read(data_root.join(OWNER_FILE)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Whether the owner's heartbeat is recent enough to trust.
#[must_use]
pub fn is_fresh(owner: &Owner) -> bool {
    now_ms().saturating_sub(owner.heartbeat_ms) <= STALE_AFTER.as_millis() as u64
}

fn write(path: &Path, owner: &Owner) -> Result<(), OwnerError> {
    let bytes = serde_json::to_vec_pretty(owner).map_err(|source| OwnerError::Write {
        path: path.to_path_buf(),
        source: io::Error::other(source),
    })?;
    cv_fs::write_atomic_with_mode(path, &bytes, 0o600).map_err(|source| OwnerError::Write {
        path: path.to_path_buf(),
        source,
    })
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

fn host_name() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_else(|_| "unknown".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_first_owner_takes_the_folder_and_writes_the_record() {
        let dir = tempfile::tempdir().unwrap();
        let guard = acquire(dir.path(), "server").unwrap();
        assert!(guard.is_some());
        let owner = read_owner(dir.path()).unwrap();
        assert_eq!(owner.role, "server");
        assert_eq!(owner.pid, std::process::id());
        assert!(is_fresh(&owner));
    }

    #[test]
    fn a_second_owner_is_refused_until_the_first_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let first = acquire(dir.path(), "server").unwrap();
        assert!(first.is_some());
        assert!(acquire(dir.path(), "app").unwrap().is_none());
        drop(first);
        assert!(!dir.path().join(OWNER_FILE).exists());
        assert!(acquire(dir.path(), "app").unwrap().is_some());
    }

    #[test]
    fn an_old_heartbeat_is_stale() {
        let owner = Owner {
            role: "server".into(),
            pid: 1,
            host: "test".into(),
            started_ms: 0,
            heartbeat_ms: now_ms().saturating_sub(60_000),
            protocol: PROTOCOL,
        };
        assert!(!is_fresh(&owner));
    }

    #[test]
    fn the_sentence_names_the_role_and_process() {
        let owner = Owner {
            role: "server".into(),
            pid: 42,
            host: "nas".into(),
            started_ms: 0,
            heartbeat_ms: now_ms(),
            protocol: PROTOCOL,
        };
        assert!(owner.sentence().contains("server"));
        assert!(owner.sentence().contains("42"));
        assert!(owner.sentence().contains("nas"));
    }

    #[cfg(unix)]
    #[test]
    fn the_owner_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let _guard = acquire(dir.path(), "server").unwrap();
        let mode = std::fs::metadata(dir.path().join(OWNER_FILE))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }
}
