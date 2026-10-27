//! The fresh start of schema version 100 on a temporary data folder:
//! an old library and old settings are set aside, the app starts from defaults, the keys of kept
//! providers stay and resolve, the credentials of removed features go, and the steps never
//! repeat.

use std::path::Path;
use std::sync::Arc;

use cv_app::data_location::{DataLocation, LocationInputs};
use cv_app::fresh_start::{self, DatabaseStep, RETIRED_ACCOUNTS};
use cv_app::provider_credentials::ProviderCredentialsService;
use cv_app::secrets::SecretStore;
use cv_app::services::{AppServices, BootOptions};
use cv_providers::{CredentialState, ProviderRegistry};
use tempfile::TempDir;

/// The settings of a 0.2.5 install: envelope version 1, a few changed values.
const OLD_SETTINGS: &str = r#"{
  "version": 1,
  "settings": {
    "values": {
      "onboarding.completed": true,
      "ui.theme": "light",
      "renamer.movie-folder": "{title} ({year})"
    }
  }
}
"#;

fn location(root: &Path) -> DataLocation {
    let inputs = LocationInputs {
        exe_dir: root.join("app"),
        user_data_base: root.join("userdata"),
        user_config_base: root.join("userconfig"),
    };
    std::fs::create_dir_all(&inputs.exe_dir).unwrap();
    let location = DataLocation::resolve(&inputs);
    location.ensure_dirs().unwrap();
    location
}

/// A library file of schema version 17 (header only; never opened).
fn old_library_bytes() -> Vec<u8> {
    let mut bytes = b"SQLite format 3\0".to_vec();
    bytes.resize(100, 0);
    bytes[60..64].copy_from_slice(&17u32.to_be_bytes());
    bytes.extend_from_slice(b"pages of the old library");
    bytes
}

/// A credential store holding kept provider keys and every retired account.
fn secrets() -> Arc<SecretStore> {
    let store = SecretStore::in_memory();
    store.set("tmdb.api_key", "cvtest-tmdb-key-1b7c").unwrap();
    store.set("omdb.api_key", "cvtest-omdb-key-5e2a").unwrap();
    store
        .set("trakt.client_id", "cvtest-trakt-client-9f30")
        .unwrap();
    for account in RETIRED_ACCOUNTS {
        store.set(account, "cvtest-retired-value").unwrap();
    }
    Arc::new(store)
}

fn boot(location: &DataLocation, secrets: &Arc<SecretStore>) -> Arc<AppServices> {
    let providers = ProviderCredentialsService::new(Arc::clone(secrets), ProviderRegistry::new());
    AppServices::boot(
        BootOptions::new(location.clone(), Arc::clone(secrets)).with_providers(providers),
    )
    .expect("boot")
}

fn active(services: &AppServices, provider: &str) -> bool {
    services
        .providers()
        .list()
        .unwrap()
        .into_iter()
        .any(|s| s.provider == provider && s.active && s.state == CredentialState::Active)
}

#[test]
fn an_update_sets_the_old_library_and_settings_aside_and_keeps_the_provider_keys() {
    let temp = TempDir::new().unwrap();
    let loc = location(temp.path());
    std::fs::write(loc.library_db(), old_library_bytes()).unwrap();
    std::fs::write(loc.settings_file(), OLD_SETTINGS).unwrap();
    let secrets = secrets();

    let services = boot(&loc, &secrets);

    let root = loc.root();
    // The old files, byte for byte, beside the new ones.
    assert_eq!(
        std::fs::read(root.join("library-old.db")).unwrap(),
        old_library_bytes()
    );
    assert_eq!(
        std::fs::read_to_string(root.join("settings-old.json")).unwrap(),
        OLD_SETTINGS
    );
    // A new, empty library and default settings: the first-run setup opens again.
    assert_eq!(
        services.db().schema_version().unwrap(),
        cv_db::SCHEMA_VERSION
    );
    assert!(services.settings().values().is_empty());
    assert!(services.settings().get("onboarding.completed").is_none());
    assert!(services.settings().snapshot().notice.is_none());
    // Kept keys resolve; retired credentials are gone.
    assert!(active(&services, "tmdb"), "the TMDB key no longer resolves");
    assert!(active(&services, "omdb"), "the OMDb key no longer resolves");
    assert!(secrets.get("trakt.client_id").unwrap().is_some());
    for account in RETIRED_ACCOUNTS {
        assert!(
            secrets.get(account).unwrap().is_none(),
            "{account} was not deleted"
        );
    }
    services.shutdown();
    drop(services);

    // A second start changes nothing: the theme set after the reset is kept.
    secrets.set("http-api.key", "cvtest-later-value").unwrap();
    {
        let services = boot(&loc, &secrets);
        services
            .settings()
            .set("ui.theme", serde_json::json!("dark"))
            .unwrap();
        services.shutdown();
    }
    let services = boot(&loc, &secrets);
    assert_eq!(
        services.settings().get("ui.theme"),
        Some(serde_json::json!("dark"))
    );
    assert!(!root.join("settings-old-2.json").exists());
    assert!(!root.join("library-old-2.db").exists());
    // No reset ran, so nothing was deleted from the credential store either.
    assert!(secrets.get("http-api.key").unwrap().is_some());
    services.shutdown();
}

#[test]
fn a_stop_after_the_database_step_finishes_the_settings_step_on_the_next_start() {
    let temp = TempDir::new().unwrap();
    let loc = location(temp.path());
    std::fs::write(loc.library_db(), old_library_bytes()).unwrap();
    std::fs::write(loc.settings_file(), OLD_SETTINGS).unwrap();
    let secrets = secrets();

    // The first start stopped right after the database step.
    let step = fresh_start::database_step(&loc.library_db()).unwrap();
    assert!(matches!(step, DatabaseStep::MovedAside { .. }), "{step:?}");
    assert_eq!(
        std::fs::read_to_string(loc.settings_file()).unwrap(),
        OLD_SETTINGS
    );

    let report = fresh_start::run(&loc, &secrets).unwrap();

    assert_eq!(report.database, DatabaseStep::Unchanged);
    assert_eq!(report.settings, Some(loc.root().join("settings-old.json")));
    assert_eq!(report.retired_credentials, RETIRED_ACCOUNTS.len());
    assert_eq!(
        std::fs::read_to_string(loc.root().join("settings-old.json")).unwrap(),
        OLD_SETTINGS
    );
    assert!(!loc.settings_file().exists());
    assert!(secrets.get("tmdb.api_key").unwrap().is_some());

    // And once more: nothing left to do.
    let again = fresh_start::run(&loc, &secrets).unwrap();
    assert_eq!(again.database, DatabaseStep::Unchanged);
    assert_eq!((again.settings, again.retired_credentials), (None, 0));
}

#[test]
fn an_existing_settings_old_is_kept_and_the_next_free_name_is_used() {
    let temp = TempDir::new().unwrap();
    let loc = location(temp.path());
    std::fs::write(loc.root().join("settings-old.json"), "earlier copy").unwrap();
    std::fs::write(loc.settings_file(), OLD_SETTINGS).unwrap();

    let report = fresh_start::run(&loc, &SecretStore::in_memory()).unwrap();

    assert_eq!(
        report.settings,
        Some(loc.root().join("settings-old-2.json"))
    );
    assert_eq!(
        std::fs::read_to_string(loc.root().join("settings-old.json")).unwrap(),
        "earlier copy"
    );
    assert_eq!(
        std::fs::read_to_string(loc.root().join("settings-old-2.json")).unwrap(),
        OLD_SETTINGS
    );
}

#[test]
fn settings_of_this_version_and_no_settings_are_left_alone() {
    let temp = TempDir::new().unwrap();
    let loc = location(temp.path());
    let secrets = secrets();

    let report = fresh_start::run(&loc, &secrets).unwrap();
    assert_eq!((report.settings, report.retired_credentials), (None, 0));

    let current = "{\"version\":2,\"settings\":{\"values\":{\"ui.theme\":\"dark\"}}}";
    std::fs::write(loc.settings_file(), current).unwrap();
    let report = fresh_start::run(&loc, &secrets).unwrap();
    assert_eq!((report.settings, report.retired_credentials), (None, 0));
    assert_eq!(
        std::fs::read_to_string(loc.settings_file()).unwrap(),
        current
    );
    assert!(secrets.get("http-api.key").unwrap().is_some());
}
