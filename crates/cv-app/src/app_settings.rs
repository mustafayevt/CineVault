//! The user's preferences as the settings page edits them.
//!
//! Settings are a flat map from a dotted key (`appearance.theme`) to a JSON value. Only values
//! that differ from the default are stored: resetting a setting removes its key, so "modified"
//! simply means "has a key". The defaults and the meaning of each key live with the code that
//! reads the setting (the settings page for the UI), which keeps this layer free of a schema that
//! every new feature would have to change.
//!
//! The map is persisted through the atomic [`SettingsStore`], so a crash never leaves a
//! half-written file and a corrupt file is quarantined instead of overwritten.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::settings::{SettingsError, SettingsStore};

/// Longest key accepted.
pub const MAX_KEY_LEN: usize = 96;
/// Largest serialized value accepted, in bytes. Keeps a runaway caller from bloating the file.
pub const MAX_VALUE_BYTES: usize = 64 * 1024;

/// What is written to the settings file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    /// Settings that differ from their defaults, by key.
    pub values: BTreeMap<String, Value>,
}

/// Why a change was refused.
#[derive(Debug, thiserror::Error)]
pub enum AppSettingsError {
    #[error(
        "{0:?} is not a valid setting key; use lowercase letters, digits, dots and hyphens, starting with a letter"
    )]
    InvalidKey(String),
    #[error("the value for {key:?} is larger than {MAX_VALUE_BYTES} bytes")]
    ValueTooLarge { key: String },
    /// A list of artwork file names was emptied: every artwork type is written.
    #[error("{EMPTY_ARTWORK_NAMES_REASON}")]
    EmptyArtworkNames { key: String },
    #[error("{0}")]
    Save(#[from] SettingsError),
}

/// A copy of the settings for the UI.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SettingsSnapshot {
    pub values: BTreeMap<String, Value>,
    /// A message the shell must show once, such as "your settings file was set aside".
    pub notice: Option<String>,
}

/// The reason a write that empties the file names of an artwork type is refused.
pub const EMPTY_ARTWORK_NAMES_REASON: &str =
    "Every artwork type is written; keep at least one name";

/// The settings holding the file names of one artwork type (`artwork.names.movie.poster`).
const ARTWORK_NAMES_PREFIX: &str = "artwork.names.";

/// Whether this write would leave an artwork type without a file name: an empty list, or text
/// with no name in it.
fn empties_artwork_names(key: &str, value: &Value) -> bool {
    key.starts_with(ARTWORK_NAMES_PREFIX)
        && match value {
            Value::Array(list) => list
                .iter()
                .all(|name| name.as_str().is_none_or(|text| text.trim().is_empty())),
            Value::String(text) => text.split(',').all(|name| name.trim().is_empty()),
            _ => false,
        }
}

/// Whether `key` is a well-formed setting key.
#[must_use]
pub fn is_valid_key(key: &str) -> bool {
    key.len() <= MAX_KEY_LEN
        && key.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && key
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-')
        && !key.ends_with('.')
        && !key.contains("..")
}

/// Called after a change was saved, with the keys whose value changed (added, replaced or
/// removed). It runs on the thread that made the change, after the settings lock was released,
/// so it may read the settings; it should not block.
pub type ChangeListener = Arc<dyn Fn(&[String]) + Send + Sync>;

/// The loaded settings and the file they are saved to. Every change is written before it is
/// kept in memory, so the two never disagree.
pub struct SettingsService {
    store: Option<SettingsStore>,
    state: Mutex<AppSettings>,
    notice: Mutex<Option<String>>,
    /// The settings file could not be used at start-up, so defaults are in use.
    load_failed: bool,
    listeners: Mutex<Vec<ChangeListener>>,
}

impl std::fmt::Debug for SettingsService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingsService")
            .field("store", &self.store)
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

impl SettingsService {
    /// Loads the settings from `store`. An unusable file is set aside and defaults are used; the
    /// notice for that is handed out once by [`Self::snapshot`].
    #[must_use]
    pub fn open(store: SettingsStore) -> Self {
        let loaded = store.load::<AppSettings>();
        Self {
            store: Some(store),
            state: Mutex::new(loaded.settings),
            load_failed: loaded.notice.is_some(),
            notice: Mutex::new(loaded.notice.map(|notice| notice.message())),
            listeners: Mutex::new(Vec::new()),
        }
    }

    /// Settings that live for this session only, for when no data folder can be found.
    #[must_use]
    pub fn in_memory(notice: impl Into<String>) -> Self {
        Self {
            store: None,
            state: Mutex::new(AppSettings::default()),
            notice: Mutex::new(Some(notice.into())),
            // Nothing on disk to protect: these values are never saved.
            load_failed: false,
            listeners: Mutex::new(Vec::new()),
        }
    }

    /// Whether the settings file could not be used when the app started (unreadable or
    /// damaged), so the values are defaults rather than the person's own. The
    /// start-up migrations skip such a session: they would otherwise mark themselves done on
    /// defaults and save them.
    #[must_use]
    pub fn load_failed(&self) -> bool {
        self.load_failed
    }

    fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
        mutex.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Calls `listener` with the changed keys after every saved change, until the app exits.
    pub fn on_change(&self, listener: ChangeListener) {
        Self::lock(&self.listeners).push(listener);
    }

    /// The current values. The load notice is included the first time only.
    pub fn snapshot(&self) -> SettingsSnapshot {
        SettingsSnapshot {
            values: Self::lock(&self.state).values.clone(),
            notice: Self::lock(&self.notice).take(),
        }
    }

    /// The current values, without the one-time load notice that [`Self::snapshot`] hands out.
    #[must_use]
    pub fn values(&self) -> BTreeMap<String, Value> {
        Self::lock(&self.state).values.clone()
    }

    /// The stored value of `key`, if it differs from its default.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<Value> {
        Self::lock(&self.state).values.get(key).cloned()
    }

    /// Stores `value` under `key`.
    ///
    /// # Errors
    /// Fails for a malformed key, an oversized value, or when the file cannot be written; the
    /// stored settings are unchanged then.
    pub fn set(&self, key: &str, value: Value) -> Result<(), AppSettingsError> {
        if !is_valid_key(key) {
            return Err(AppSettingsError::InvalidKey(key.to_owned()));
        }
        let size = serde_json::to_vec(&value).map_or(usize::MAX, |bytes| bytes.len());
        if size > MAX_VALUE_BYTES {
            return Err(AppSettingsError::ValueTooLarge {
                key: key.to_owned(),
            });
        }
        if empties_artwork_names(key, &value) {
            return Err(AppSettingsError::EmptyArtworkNames {
                key: key.to_owned(),
            });
        }
        self.change(|values| {
            values.insert(key.to_owned(), value);
        })
    }

    /// Stores several settings and removes others in one change: one atomic write of the file,
    /// and one announcement to the change listeners. Either all of it is kept or none of it.
    ///
    /// # Errors
    /// Fails for a malformed key, an oversized value, or when the file cannot be written; the
    /// stored settings are unchanged then.
    pub fn apply(
        &self,
        set: Vec<(String, Value)>,
        remove: &[&str],
    ) -> Result<(), AppSettingsError> {
        for (key, value) in &set {
            if !is_valid_key(key) {
                return Err(AppSettingsError::InvalidKey(key.clone()));
            }
            let size = serde_json::to_vec(value).map_or(usize::MAX, |bytes| bytes.len());
            if size > MAX_VALUE_BYTES {
                return Err(AppSettingsError::ValueTooLarge { key: key.clone() });
            }
            if empties_artwork_names(key, value) {
                return Err(AppSettingsError::EmptyArtworkNames { key: key.clone() });
            }
        }
        self.change(|values| {
            for key in remove {
                values.remove(*key);
            }
            for (key, value) in set {
                values.insert(key, value);
            }
        })
    }

    /// Returns the given settings to their defaults. Unknown keys are ignored.
    ///
    /// # Errors
    /// Fails when the file cannot be written; the stored settings are unchanged then.
    pub fn reset<S: AsRef<str>>(&self, keys: &[S]) -> Result<(), AppSettingsError> {
        self.change(|values| {
            for key in keys {
                values.remove(key.as_ref());
            }
        })
    }

    fn change(
        &self,
        apply: impl FnOnce(&mut BTreeMap<String, Value>),
    ) -> Result<(), AppSettingsError> {
        let changed = {
            let mut state = Self::lock(&self.state);
            let mut next = state.clone();
            apply(&mut next.values);
            if next == *state {
                return Ok(());
            }
            if let Some(store) = &self.store {
                store.save(&next)?;
            }
            let changed = changed_keys(&state.values, &next.values);
            *state = next;
            changed
        };
        let listeners: Vec<ChangeListener> = Self::lock(&self.listeners).clone();
        // A failing listener must neither stop the others nor fail a change already saved.
        for listener in listeners {
            crate::guard::contain("settings change listener", || listener(&changed));
        }
        Ok(())
    }
}

/// The keys whose value differs between two maps, in key order.
fn changed_keys(before: &BTreeMap<String, Value>, after: &BTreeMap<String, Value>) -> Vec<String> {
    before
        .keys()
        .chain(after.keys())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .filter(|key| before.get(*key) != after.get(*key))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn service(dir: &std::path::Path) -> SettingsService {
        SettingsService::open(SettingsStore::new(dir.join("settings.json")))
    }

    #[test]
    fn keys_must_be_dotted_lowercase_words() {
        for good in [
            "appearance.theme",
            "metadata.nfo-format",
            "a",
            "shell.sidebar",
        ] {
            assert!(is_valid_key(good), "{good}");
        }
        for bad in [
            "",
            "Theme",
            ".theme",
            "theme.",
            "a..b",
            "1a",
            "has space",
            "a/b",
        ] {
            assert!(!is_valid_key(bad), "{bad}");
        }
        assert!(!is_valid_key(&format!("a{}", "b".repeat(MAX_KEY_LEN))));
    }

    #[test]
    fn a_fresh_install_has_no_values_and_no_notice() {
        let dir = tempfile::tempdir().unwrap();
        let snapshot = service(dir.path()).snapshot();
        assert!(snapshot.values.is_empty());
        assert!(snapshot.notice.is_none());
    }

    #[test]
    fn set_values_survive_a_restart_and_reset_removes_them() {
        let dir = tempfile::tempdir().unwrap();
        let first = service(dir.path());
        first.set("appearance.theme", json!("light")).unwrap();
        first.set("artwork.extra-fanart", json!(8)).unwrap();

        let second = service(dir.path());
        let values = second.snapshot().values;
        assert_eq!(values["appearance.theme"], json!("light"));
        assert_eq!(values["artwork.extra-fanart"], json!(8));

        second.reset(&["appearance.theme", "no.such-key"]).unwrap();
        let third = service(dir.path());
        let values = third.snapshot().values;
        assert!(!values.contains_key("appearance.theme"));
        assert_eq!(values["artwork.extra-fanart"], json!(8));
    }

    #[test]
    fn the_file_is_human_readable_json() {
        let dir = tempfile::tempdir().unwrap();
        service(dir.path())
            .set("appearance.theme", json!("light"))
            .unwrap();
        let text = std::fs::read_to_string(dir.path().join("settings.json")).unwrap();
        let doc: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            doc["settings"]["values"]["appearance.theme"],
            json!("light")
        );
    }

    #[test]
    fn a_refused_change_leaves_everything_as_it_was() {
        let dir = tempfile::tempdir().unwrap();
        let svc = service(dir.path());
        svc.set("appearance.theme", json!("dark")).unwrap();
        assert!(matches!(
            svc.set("Bad Key", json!(1)),
            Err(AppSettingsError::InvalidKey(_))
        ));
        let huge = json!("x".repeat(MAX_VALUE_BYTES + 1));
        assert!(matches!(
            svc.set("appearance.theme", huge),
            Err(AppSettingsError::ValueTooLarge { .. })
        ));
        assert_eq!(svc.snapshot().values["appearance.theme"], json!("dark"));
    }

    #[test]
    fn an_artwork_type_cannot_be_left_without_a_file_name() {
        let dir = tempfile::tempdir().unwrap();
        let svc = service(dir.path());
        svc.set("artwork.names.movie.poster", json!(["poster", "folder"]))
            .unwrap();
        for empty in [json!([]), json!(""), json!(" , "), json!(["", "  "])] {
            let error = svc
                .set("artwork.names.movie.poster", empty.clone())
                .unwrap_err();
            assert!(matches!(error, AppSettingsError::EmptyArtworkNames { .. }));
            assert_eq!(
                error.to_string(),
                "Every artwork type is written; keep at least one name"
            );
            // The same through the multi-key write; nothing changes.
            let batch = svc.apply(
                vec![
                    ("appearance.theme".to_owned(), json!("dark")),
                    ("artwork.names.tv.fanart".to_owned(), empty),
                ],
                &[],
            );
            assert!(matches!(
                batch,
                Err(AppSettingsError::EmptyArtworkNames { .. })
            ));
        }
        assert_eq!(
            svc.get("artwork.names.movie.poster"),
            Some(json!(["poster", "folder"]))
        );
        assert_eq!(svc.get("appearance.theme"), None);
    }

    #[test]
    fn listeners_hear_the_changed_keys_after_a_saved_change() {
        let dir = tempfile::tempdir().unwrap();
        let svc = service(dir.path());
        let heard = Arc::new(Mutex::new(Vec::<Vec<String>>::new()));
        let sink = Arc::clone(&heard);
        svc.on_change(Arc::new(move |keys| {
            sink.lock().unwrap().push(keys.to_vec());
        }));
        svc.set("appearance.theme", json!("light")).unwrap();
        svc.set("appearance.theme", json!("light")).unwrap();
        svc.set("a.b", json!(1)).unwrap();
        svc.reset(&["appearance.theme"]).unwrap();
        assert_eq!(
            *heard.lock().unwrap(),
            vec![
                vec!["appearance.theme".to_owned()],
                vec!["a.b".to_owned()],
                vec!["appearance.theme".to_owned()],
            ],
            "an unchanged value is not a change"
        );
    }

    #[test]
    fn a_panicking_listener_neither_fails_the_change_nor_stops_the_others() {
        let dir = tempfile::tempdir().unwrap();
        let svc = service(dir.path());
        let heard = Arc::new(Mutex::new(0));
        svc.on_change(Arc::new(|_| panic!("listener bug")));
        let sink = Arc::clone(&heard);
        svc.on_change(Arc::new(move |_| *sink.lock().unwrap() += 1));
        svc.set("appearance.theme", json!("light")).unwrap();
        assert_eq!(*heard.lock().unwrap(), 1);
        assert_eq!(svc.snapshot().values["appearance.theme"], json!("light"));
    }

    #[test]
    fn a_failed_write_is_not_kept_in_memory() {
        let dir = tempfile::tempdir().unwrap();
        // The settings path is a directory, so the atomic rename over it fails.
        std::fs::create_dir(dir.path().join("settings.json")).unwrap();
        let svc = SettingsService {
            store: Some(SettingsStore::new(dir.path().join("settings.json"))),
            state: Mutex::new(AppSettings::default()),
            notice: Mutex::new(None),
            load_failed: false,
            listeners: Mutex::new(Vec::new()),
        };
        assert!(matches!(
            svc.set("appearance.theme", json!("light")),
            Err(AppSettingsError::Save(_))
        ));
        assert!(svc.snapshot().values.is_empty());
    }

    #[test]
    fn an_unreadable_file_is_set_aside_before_the_first_save() {
        let dir = tempfile::tempdir().unwrap();
        // A directory where the file should be cannot be read as a file.
        let path = dir.path().join("settings.json");
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("marker"), "the person's settings").unwrap();
        let svc = service(dir.path());
        assert!(svc.load_failed());
        let first = svc.snapshot();
        assert!(first.values.is_empty());
        assert!(first.notice.unwrap().contains("could not be opened"));
        // Nothing was touched by loading.
        assert!(path.join("marker").exists());

        svc.set("appearance.theme", json!("light")).unwrap();
        let aside: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains(".unreadable-"))
            .collect();
        assert_eq!(aside.len(), 1);
        assert!(aside[0].path().join("marker").exists());
        assert!(path.is_file());
        let reopened = service(dir.path());
        assert!(!reopened.load_failed());
        assert_eq!(reopened.values()["appearance.theme"], json!("light"));
    }

    #[test]
    fn a_corrupt_file_starts_with_defaults_keeps_the_file_and_reports_once() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("settings.json"), "{ not json").unwrap();
        let svc = service(dir.path());
        let first = svc.snapshot();
        assert!(first.values.is_empty());
        assert!(first.notice.is_some());
        assert!(svc.snapshot().notice.is_none());
        let kept = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .any(|e| e.file_name().to_string_lossy().contains(".corrupt-"));
        assert!(kept);
    }

    #[test]
    fn settings_without_a_data_folder_work_for_the_session() {
        let svc = SettingsService::in_memory("Settings will not be saved.");
        assert_eq!(
            svc.snapshot().notice.as_deref(),
            Some("Settings will not be saved.")
        );
        svc.set("appearance.theme", json!("light")).unwrap();
        assert_eq!(svc.snapshot().values["appearance.theme"], json!("light"));
    }
}
