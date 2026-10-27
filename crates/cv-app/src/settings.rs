//! Settings storage that never resets silently.
//!
//! The settings live in one human-readable JSON file wrapped in a small envelope:
//!
//! ```json
//! { "version": 2, "settings": { ... } }
//! ```
//!
//! * The envelope version of this build is [`SETTINGS_VERSION`]. A file with a lower version was
//!   written before the fresh start of this version (schema 100): it is never read or migrated.
//!   [`SettingsStore::set_aside_earlier`] renames it to `settings-old.json` (or
//!   `settings-old-2.json`, ...) and the app starts from defaults; [`SettingsStore::load`] does
//!   the same if it still meets one.
//!
//! * Writes are atomic: the new content goes to a temp file in the same folder, is flushed to
//!   disk, and then replaces the real file with a rename. A crash leaves either the old or the
//!   new file, never a half-written one.
//! * Older versions are upgraded in memory by the registered migration steps and written back on
//!   the next save.
//! * A file that cannot be used (invalid JSON, wrong shape, written by a newer app) is never
//!   overwritten. It is renamed to `<name>.corrupt-<UTC timestamp>`, the app starts with
//!   defaults, and the load result carries a [`SettingsNotice`] for the shell to show.
//! * A file that exists but cannot be read (another program holds it, permissions) is read again
//!   a few times with short pauses. When it still cannot be read the app starts with defaults, the
//!   file is left alone, and before the first save of the session it is renamed to
//!   `<name>.unreadable-<UTC timestamp>`, so a save never replaces settings that were never read.
//!   When it cannot be set aside, the save fails instead.

use std::ffi::OsString;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

/// The settings envelope version this build writes when no migration is registered. Version 1
/// was written by the builds before the fresh start; such a file is set aside, never read.
pub const SETTINGS_VERSION: u32 = 2;

/// One upgrade step. Step `i` (0-based) turns a version `SETTINGS_VERSION + i` document into
/// version `SETTINGS_VERSION + i + 1`.
/// It receives the content of the `settings` object and returns the upgraded content.
pub type Migration = fn(Value) -> Result<Value, String>;

/// Errors from saving settings. Loading never fails: it falls back to defaults with a notice.
#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("could not write settings at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not serialize settings: {0}")]
    Serialize(#[from] serde_json::Error),
}

/// Why a settings file was set aside.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuarantineReason {
    /// The file is not valid JSON or lacks the `version`/`settings` envelope.
    Unparseable(String),
    /// The JSON is valid but does not fit the settings type.
    InvalidContent(String),
    /// A migration step failed.
    MigrationFailed { from_version: u32, message: String },
    /// The file was written by a newer app version than this one understands.
    NewerVersion { found: u32, supported: u32 },
    /// The file was written before the fresh start of this version and could not be set aside.
    EarlierVersion { found: u32 },
}

impl std::fmt::Display for QuarantineReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unparseable(e) => write!(f, "the file is not valid settings JSON ({e})"),
            Self::InvalidContent(e) => write!(f, "the settings content is invalid ({e})"),
            Self::MigrationFailed {
                from_version,
                message,
            } => write!(
                f,
                "upgrading settings from version {from_version} failed ({message})"
            ),
            Self::NewerVersion { found, supported } => write!(
                f,
                "the file has version {found} but this app supports up to {supported}"
            ),
            Self::EarlierVersion { found } => write!(
                f,
                "the file has version {found}, from an earlier CineVault, and could not be \
                 renamed to settings-old.json"
            ),
        }
    }
}

/// Something the user must be told about after loading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsNotice {
    /// The settings file was unusable. It was renamed (not deleted) and defaults are in use.
    Quarantined {
        /// Where the settings file is expected to be.
        original: PathBuf,
        /// Where the broken file was moved to, or `None` if it could not be moved.
        quarantined: Option<PathBuf>,
        reason: QuarantineReason,
    },
    /// The settings file exists but could not be read. It was left in place; the first save of
    /// the session sets it aside as `<name>.unreadable-<timestamp>` before writing.
    Unreadable {
        /// Where the settings file is.
        original: PathBuf,
        /// The read error.
        error: String,
    },
}

impl SettingsNotice {
    /// A plain-language message for the notification.
    pub fn message(&self) -> String {
        match self {
            Self::Quarantined {
                original,
                quarantined,
                reason,
            } => {
                let kept = match quarantined {
                    Some(p) => format!("The old file was kept as {}.", p.display()),
                    None => format!(
                        "The old file at {} could not be moved aside and was left untouched.",
                        original.display()
                    ),
                };
                format!("Your settings could not be read, so defaults are in use: {reason}. {kept}")
            }
            Self::Unreadable { original, error } => format!(
                "Your settings file at {} could not be opened ({error}), so defaults are in use \
                 for now. The file was left untouched: restart CineVault to try again. If you \
                 change a setting first, the file is kept as {}.unreadable-<time> next to it.",
                original.display(),
                original
                    .file_name()
                    .map_or_else(|| "settings.json".into(), |n| n.to_string_lossy())
            ),
        }
    }
}

/// The result of [`SettingsStore::load`].
#[derive(Debug, Clone, PartialEq)]
pub struct Loaded<T> {
    pub settings: T,
    pub notice: Option<SettingsNotice>,
}

/// A versioned, atomically written settings file.
#[derive(Debug, Clone)]
pub struct SettingsStore {
    path: PathBuf,
    migrations: Vec<Migration>,
    /// Set when the file exists but could not be read: the next save sets it aside first.
    unread: Arc<AtomicBool>,
}

impl SettingsStore {
    /// A store at `path` with no migrations (current version [`SETTINGS_VERSION`]).
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            migrations: Vec::new(),
            unread: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Registers the upgrade steps, oldest first. The current version is
    /// `SETTINGS_VERSION + steps.len()`.
    #[must_use]
    pub fn with_migrations(mut self, steps: Vec<Migration>) -> Self {
        self.migrations = steps;
        self
    }

    /// The version this build writes.
    pub fn current_version(&self) -> u32 {
        u32::try_from(self.migrations.len())
            .map_or(u32::MAX, |n| n.saturating_add(SETTINGS_VERSION))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Loads the settings. A missing file gives defaults silently (first run). An unusable file
    /// is quarantined and defaults are returned together with a notice. A file that cannot be
    /// read is retried briefly, then left in place with a notice, and set aside before the next
    /// save.
    pub fn load<T: DeserializeOwned + Default>(&self) -> Loaded<T> {
        self.remove_stale_temp_files();
        self.unread.store(false, Ordering::SeqCst);
        let bytes = match read_with_retries(&self.path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Loaded {
                    settings: T::default(),
                    notice: None,
                };
            }
            Err(e) => {
                // Unreadable (permissions, held by another program): leave it alone, say so, and
                // set it aside before anything is saved over it.
                tracing::error!(path = %self.path.display(), error = %e, "settings file unreadable");
                self.unread.store(true, Ordering::SeqCst);
                return Loaded {
                    settings: T::default(),
                    notice: Some(SettingsNotice::Unreadable {
                        original: self.path.clone(),
                        error: e.to_string(),
                    }),
                };
            }
        };
        match self.parse::<T>(&bytes) {
            Ok(settings) => Loaded {
                settings,
                notice: None,
            },
            // Normally set aside by the fresh start before the first load; never read.
            Err(QuarantineReason::EarlierVersion { found }) => match self.move_to_old() {
                Ok(old) => {
                    tracing::info!(to = %old.display(), "settings of an earlier version set aside");
                    Loaded {
                        settings: T::default(),
                        notice: None,
                    }
                }
                Err(e) => {
                    tracing::error!(error = %e, "the earlier settings file could not be set aside");
                    // Not saved over: the first save sets it aside or fails.
                    self.unread.store(true, Ordering::SeqCst);
                    Loaded {
                        settings: T::default(),
                        notice: Some(SettingsNotice::Quarantined {
                            original: self.path.clone(),
                            quarantined: None,
                            reason: QuarantineReason::EarlierVersion { found },
                        }),
                    }
                }
            },
            Err(reason) => Loaded {
                settings: T::default(),
                notice: Some(self.quarantine(reason)),
            },
        }
    }

    /// Writes the settings atomically: temp file in the same folder, flush to disk, rename over
    /// the real file.
    pub fn save<T: Serialize>(&self, settings: &T) -> Result<(), SettingsError> {
        let mut envelope = Map::new();
        envelope.insert("version".into(), Value::from(self.current_version()));
        envelope.insert("settings".into(), serde_json::to_value(settings)?);
        let mut text = serde_json::to_string_pretty(&Value::Object(envelope))?;
        text.push('\n');
        self.set_aside_unread()?;
        self.write_atomic(text.as_bytes())
    }

    /// The envelope version of the file when it was written before the fresh start of this
    /// version (lower than [`SETTINGS_VERSION`]); `None` when there is no file, it is of this
    /// version or later, or it is not a settings envelope at all (loading handles those).
    ///
    /// # Errors
    /// The file exists but cannot be read.
    pub fn earlier_version(&self) -> std::io::Result<Option<u32>> {
        let bytes = match read_with_retries(&self.path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        Ok(serde_json::from_slice::<Value>(&bytes)
            .ok()
            .and_then(|doc| doc.get("version").and_then(Value::as_u64))
            .and_then(|v| u32::try_from(v).ok())
            .filter(|v| (1..SETTINGS_VERSION).contains(v)))
    }

    /// The settings step of the fresh start: renames a file of an earlier version to
    /// `settings-old.json` (or the first free `settings-old-N.json`) in the same folder, by a
    /// retrying rename that never replaces a file. Returns where it went, or `None` when there
    /// was nothing to set aside. The file is never read again.
    ///
    /// # Errors
    /// The file cannot be read or renamed; it is then where it was.
    pub fn set_aside_earlier(&self) -> std::io::Result<Option<PathBuf>> {
        if self.earlier_version()?.is_none() {
            return Ok(None);
        }
        self.move_to_old().map(Some)
    }

    fn move_to_old(&self) -> std::io::Result<PathBuf> {
        let target = self.old_path();
        cv_fs::retry("set aside the earlier settings", &self.path, || {
            if fs::symlink_metadata(&target).is_ok() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    format!("{} already exists", target.display()),
                ));
            }
            fs::rename(&self.path, &target)
        })?;
        sync_parent_dir(&self.path);
        Ok(target)
    }

    /// `settings-old.json`, or `settings-old-2.json`, `settings-old-3.json`, ... when taken.
    fn old_path(&self) -> PathBuf {
        let stem = self
            .path
            .file_stem()
            .map_or_else(|| "settings".into(), |s| s.to_string_lossy().into_owned());
        let ext = self
            .path
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        (1u32..)
            .map(|n| {
                if n == 1 {
                    self.sibling(format!("{stem}-old{ext}").into())
                } else {
                    self.sibling(format!("{stem}-old-{n}{ext}").into())
                }
            })
            .find(|p| fs::symlink_metadata(p).is_err())
            .expect("an unbounded sequence has a free name")
    }

    /// Whether the last load found a file it could not read, which is not set aside yet.
    #[must_use]
    pub fn has_unread_file(&self) -> bool {
        self.unread.load(Ordering::SeqCst)
    }

    /// Renames a file the last load could not read to `<name>.unreadable-<timestamp>`, so the
    /// coming save cannot replace settings that were never read. A save fails when it cannot.
    fn set_aside_unread(&self) -> Result<(), SettingsError> {
        if !self.unread.load(Ordering::SeqCst) {
            return Ok(());
        }
        let target = self.unique_sibling(".unreadable-");
        match cv_fs::retry("set aside the settings file", &self.path, || {
            fs::rename(&self.path, &target)
        }) {
            Ok(()) => {
                tracing::warn!(
                    from = %self.path.display(), to = %target.display(),
                    "unread settings file set aside before saving"
                );
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                tracing::error!(
                    path = %self.path.display(), error = %source,
                    "the unread settings file could not be set aside; not saving over it"
                );
                return Err(SettingsError::Io {
                    path: self.path.clone(),
                    source,
                });
            }
        }
        self.unread.store(false, Ordering::SeqCst);
        Ok(())
    }

    fn parse<T: DeserializeOwned>(&self, bytes: &[u8]) -> Result<T, QuarantineReason> {
        let doc: Value = serde_json::from_slice(bytes)
            .map_err(|e| QuarantineReason::Unparseable(e.to_string()))?;
        let Value::Object(mut obj) = doc else {
            return Err(QuarantineReason::Unparseable(
                "top level is not an object".into(),
            ));
        };
        let version = obj
            .get("version")
            .and_then(Value::as_u64)
            .and_then(|v| u32::try_from(v).ok())
            .filter(|v| *v >= 1)
            .ok_or_else(|| QuarantineReason::Unparseable("missing or invalid version".into()))?;
        if version < SETTINGS_VERSION {
            return Err(QuarantineReason::EarlierVersion { found: version });
        }
        let mut content = obj
            .remove("settings")
            .ok_or_else(|| QuarantineReason::Unparseable("missing settings object".into()))?;
        let current = self.current_version();
        if version > current {
            return Err(QuarantineReason::NewerVersion {
                found: version,
                supported: current,
            });
        }
        for from in version..current {
            let step = self.migrations[(from - SETTINGS_VERSION) as usize];
            content = step(content).map_err(|message| QuarantineReason::MigrationFailed {
                from_version: from,
                message,
            })?;
        }
        serde_json::from_value(content).map_err(|e| QuarantineReason::InvalidContent(e.to_string()))
    }

    /// Renames the broken file to `<name>.corrupt-<timestamp>` and builds the notice.
    fn quarantine(&self, reason: QuarantineReason) -> SettingsNotice {
        let target = self.quarantine_path();
        let quarantined = match fs::rename(&self.path, &target) {
            Ok(()) => {
                tracing::warn!(
                    from = %self.path.display(), to = %target.display(), %reason,
                    "settings file quarantined, starting with defaults"
                );
                Some(target)
            }
            Err(e) => {
                tracing::error!(
                    path = %self.path.display(), error = %e,
                    "could not quarantine settings file; leaving it in place"
                );
                None
            }
        };
        SettingsNotice::Quarantined {
            original: self.path.clone(),
            quarantined,
            reason,
        }
    }

    fn quarantine_path(&self) -> PathBuf {
        self.unique_sibling(".corrupt-")
    }

    /// `<name><infix><timestamp>`, with `-1`, `-2`, ... added when that name is taken.
    fn unique_sibling(&self, infix: &str) -> PathBuf {
        let stamp = utc_stamp(SystemTime::now());
        let base = self.file_name_with(&format!("{infix}{stamp}"));
        let mut candidate = self.sibling(base.clone());
        let mut n = 1;
        while candidate.exists() {
            let mut name = base.clone();
            name.push(format!("-{n}"));
            candidate = self.sibling(name);
            n += 1;
        }
        candidate
    }

    fn file_name_with(&self, suffix: &str) -> OsString {
        let mut name = self
            .path
            .file_name()
            .map_or_else(|| OsString::from("settings.json"), OsString::from);
        name.push(suffix);
        name
    }

    fn sibling(&self, name: OsString) -> PathBuf {
        self.path.with_file_name(name)
    }

    fn write_atomic(&self, bytes: &[u8]) -> Result<(), SettingsError> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let io_err = |source| SettingsError::Io {
            path: self.path.clone(),
            source,
        };
        if let Some(dir) = self.path.parent().filter(|d| !d.as_os_str().is_empty()) {
            fs::create_dir_all(dir).map_err(io_err)?;
        }
        let tmp = self.sibling(self.file_name_with(&format!(
            ".tmp-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        )));
        let result = (|| {
            let mut file = File::create(&tmp)?;
            file.write_all(bytes)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&tmp, &self.path)?;
            sync_parent_dir(&self.path);
            Ok(())
        })();
        if let Err(source) = result {
            let _ = fs::remove_file(&tmp);
            return Err(io_err(source));
        }
        Ok(())
    }

    /// Removes temp files left behind by a crash between write and rename.
    fn remove_stale_temp_files(&self) {
        let Some(dir) = self.path.parent().filter(|d| !d.as_os_str().is_empty()) else {
            return;
        };
        let prefix = self.file_name_with(".tmp-").to_string_lossy().into_owned();
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with(&prefix) {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}

/// Reads `path`, trying again for a moment while another program holds it (antivirus, a sync
/// client, a backup tool).
fn read_with_retries(path: &Path) -> std::io::Result<Vec<u8>> {
    cv_fs::retry("read the settings", path, || fs::read(path))
}

/// Flushes the directory entry after the rename where the platform allows it.
fn sync_parent_dir(path: &Path) {
    #[cfg(unix)]
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty())
        && let Ok(d) = File::open(dir)
    {
        let _ = d.sync_all();
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// `YYYYMMDDTHHMMSSZ` in UTC.
fn utc_stamp(t: SystemTime) -> String {
    let secs = t
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Civil date from days since 1970-01-01 (proleptic Gregorian).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}{month:02}{day:02}T{:02}{:02}{:02}Z",
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests;
