//! The settings store behind the Settings page.
//!
//! Values cross the boundary as JSON text so that one pair of commands serves every setting,
//! whatever its type; the settings page parses and validates them against its own schema. The
//! state is transport-neutral: a surface builds it from the app services (or the settings file
//! alone when the services could not start) and wraps the three commands.

use std::collections::BTreeMap;
use std::sync::Arc;

use cv_app::app_settings::SettingsService;
use cv_app::caches::{CacheKind, CacheService, IMAGE_CACHE_SIZE_KEY};
use cv_app::data_location::DataLocation;
use cv_app::logging;
use cv_app::scanning::{ScanService, WATCH_DEBOUNCE_KEY};
use cv_app::services::AppServices;
use cv_app::settings::SettingsStore;
use serde::{Deserialize, Serialize};
use specta::Type;

/// The settings and the services a saved setting may need to tell.
pub struct SettingsState {
    settings: Arc<SettingsService>,
    /// Told when a setting that changes the folder watcher is saved.
    scan: Option<Arc<ScanService>>,
    /// Told when the size cap of the image cache changes.
    caches: Option<Arc<CacheService>>,
}

impl SettingsState {
    /// The settings the app services loaded. When the services could not start, the settings
    /// file is opened on its own, or settings are kept for this session only when no data folder
    /// can be found.
    pub fn from_services(services: Option<&Arc<AppServices>>) -> Self {
        if let Some(services) = services {
            return Self {
                settings: Arc::clone(services.settings()),
                scan: Some(Arc::clone(services.scan())),
                caches: Some(Arc::clone(services.caches())),
            };
        }
        Self {
            settings: Arc::new(match DataLocation::resolve_for_process() {
                // Settings of an earlier version wait for the fresh start, which follows the
                // library's: they are not set aside or read while the library cannot open.
                Some(location)
                    if SettingsStore::new(location.settings_file())
                        .earlier_version()
                        .is_ok_and(|v| v.is_some()) =>
                {
                    SettingsService::in_memory(
                        "Your settings are kept as they are until CineVault can open your \
                         library; changes made now are not saved.",
                    )
                }
                Some(location) => {
                    SettingsService::open(SettingsStore::new(location.settings_file()))
                }
                None => SettingsService::in_memory(
                    "CineVault could not find a folder for its data, so settings will not be \
                     saved when you quit.",
                ),
            }),
            scan: None,
            caches: None,
        }
    }
}

impl SettingsState {
    /// The settings, for other commands that keep their own values here.
    pub fn service(&self) -> &SettingsService {
        &self.settings
    }

    /// The same settings, for a service that keeps them.
    pub fn shared(&self) -> Arc<SettingsService> {
        Arc::clone(&self.settings)
    }

    /// Applies the settings that take effect at once: the log detail changes, and the folder
    /// watcher restarts when its debounce changes. Blocking (it probes the library folders).
    pub fn apply(&self, keys: &[&str]) {
        if keys.contains(&logging::LEVEL_KEY) {
            logging::apply_from(&self.settings);
        }
        if let Some(caches) = &self.caches
            && keys.contains(&IMAGE_CACHE_SIZE_KEY)
        {
            caches.apply_settings();
        }
        if let Some(scan) = &self.scan
            && keys.contains(&WATCH_DEBOUNCE_KEY)
        {
            scan.refresh_watcher();
        }
    }
}

/// Every setting that differs from its default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct SettingsSnapshotDto {
    /// JSON text of each stored value, by setting key.
    pub values: BTreeMap<String, String>,
    /// A message to show once, for example when a damaged settings file was set aside.
    pub notice: Option<String>,
}

fn snapshot(service: &SettingsService) -> SettingsSnapshotDto {
    let snapshot = service.snapshot();
    SettingsSnapshotDto {
        values: snapshot
            .values
            .into_iter()
            .map(|(key, value)| (key, value.to_string()))
            .collect(),
        notice: snapshot.notice,
    }
}

fn store(service: &SettingsService, key: &str, value_json: &str) -> Result<(), String> {
    let value = serde_json::from_str(value_json)
        .map_err(|error| format!("the value for {key} is not valid JSON: {error}"))?;
    service.set(key, value).map_err(|error| error.to_string())
}

/// Returns the stored settings (those that differ from their defaults).
pub fn get_settings(state: &SettingsState) -> SettingsSnapshotDto {
    snapshot(&state.settings)
}

/// Saves one setting. `value_json` is the JSON text of the new value.
pub async fn set_setting(
    state: &SettingsState,
    key: String,
    value_json: String,
) -> Result<(), String> {
    store(&state.settings, &key, &value_json)?;
    let (settings, scan, caches) = (
        Arc::clone(&state.settings),
        state.scan.clone(),
        state.caches.clone(),
    );
    cv_jobs::client::spawn_blocking(move || {
        SettingsState {
            settings,
            scan,
            caches,
        }
        .apply(&[&key]);
    })
    .await
    .map_err(|error| error.to_string())?;
    Ok(())
}

/// Returns the given settings to their defaults.
pub async fn reset_settings(state: &SettingsState, keys: Vec<String>) -> Result<(), String> {
    state
        .settings
        .reset(&keys)
        .map_err(|error| error.to_string())?;
    let (settings, scan, caches) = (
        Arc::clone(&state.settings),
        state.scan.clone(),
        state.caches.clone(),
    );
    cv_jobs::client::spawn_blocking(move || {
        let names: Vec<&str> = keys.iter().map(String::as_str).collect();
        SettingsState {
            settings,
            scan,
            caches,
        }
        .apply(&names);
    })
    .await
    .map_err(|error| error.to_string())?;
    Ok(())
}

/// Which cache a command means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CacheKindDto {
    /// Thumbnails of posters and other artwork.
    Images,
    /// Answers of the metadata providers.
    Responses,
}

impl From<CacheKindDto> for CacheKind {
    fn from(kind: CacheKindDto) -> Self {
        match kind {
            CacheKindDto::Images => Self::Images,
            CacheKindDto::Responses => Self::Responses,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service() -> (tempfile::TempDir, SettingsService) {
        let dir = tempfile::tempdir().expect("temp dir");
        let service = SettingsService::open(SettingsStore::new(dir.path().join("settings.json")));
        (dir, service)
    }

    #[test]
    fn values_travel_as_json_text() {
        let (_dir, service) = service();
        store(&service, "appearance.theme", "\"light\"").expect("store");
        store(&service, "artwork.extra-fanart", "8").expect("store");
        let dto = snapshot(&service);
        assert_eq!(dto.values["appearance.theme"], "\"light\"");
        assert_eq!(dto.values["artwork.extra-fanart"], "8");
        assert_eq!(dto.notice, None);
    }

    #[test]
    fn invalid_json_and_bad_keys_are_refused_with_a_message() {
        let (_dir, service) = service();
        let json_error = store(&service, "appearance.theme", "{oops").expect_err("bad json");
        assert!(json_error.contains("not valid JSON"), "{json_error}");
        let key_error = store(&service, "Bad Key", "1").expect_err("bad key");
        assert!(key_error.contains("not a valid setting key"), "{key_error}");
        assert!(snapshot(&service).values.is_empty());
    }

    #[test]
    fn reset_drops_the_stored_values() {
        let (_dir, service) = service();
        store(&service, "appearance.theme", "\"light\"").expect("store");
        service
            .reset(&["appearance.theme".to_owned()])
            .expect("reset");
        assert!(snapshot(&service).values.is_empty());
    }
}
