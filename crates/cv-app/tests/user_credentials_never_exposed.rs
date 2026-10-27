//! A user's provider key stays inside the resolver and the credential store. The user saves dummy
//! keys through the service, the app does its credential and settings work, and then everything
//! it can emit is searched for the dummy bytes: the status data behind IPC and the CLI,
//! settings files and log output.

use std::io;
use std::path::Path;
use std::sync::{Arc, Mutex};

use cv_app::app_settings::SettingsService;
use cv_app::provider_credentials::ProviderCredentialsService;
use cv_app::redact::{RedactingMakeWriter, redact};
use cv_app::secrets::SecretStore;
use cv_app::settings::SettingsStore;
use cv_providers::catalogue::{API_KEY, key};
use cv_providers::{CredentialState, ProviderRegistry};
use serde_json::json;

const TMDB_KEY: &str = "dummy-user-tmdb-3f6a92c1d0b7";
const TRAKT_ID: &str = "dummy-user-trakt-id-55e0a1b9c8";
const MDBLIST_KEY: &str = "dummy-user-mdblist-key-7d21f4ae";
const ALL_KEYS: [&str; 3] = [TMDB_KEY, TRAKT_ID, MDBLIST_KEY];

/// A service whose user has saved a TMDB key and a Trakt app.
fn service() -> ProviderCredentialsService {
    let service = ProviderCredentialsService::new(
        Arc::new(SecretStore::in_memory()),
        ProviderRegistry::new(),
    );
    service.set(key::TMDB, API_KEY, TMDB_KEY).unwrap();
    service.set(key::TRAKT, "client_id", TRAKT_ID).unwrap();
    service.set(key::MDBLIST, API_KEY, MDBLIST_KEY).unwrap();
    service
}

#[derive(Clone, Default)]
struct LogBuffer(Arc<Mutex<Vec<u8>>>);

impl io::Write for LogBuffer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn read_all(dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            read_all(&path, out);
        } else {
            out.push((path.display().to_string(), std::fs::read(&path).unwrap()));
        }
    }
}

fn assert_clean(what: &str, text: &str, secrets: &[&str]) {
    for secret in secrets {
        assert!(!text.contains(secret), "{what} contains {secret}: {text}");
    }
}

#[test]
fn status_data_for_ipc_http_and_cli_names_the_state_but_carries_no_key() {
    let service = service();
    let statuses = service.list().unwrap();

    let tmdb = statuses.iter().find(|s| s.provider == key::TMDB).unwrap();
    assert_eq!(tmdb.state, CredentialState::Active);
    assert!(tmdb.active && tmdb.configured);
    let omdb = statuses.iter().find(|s| s.provider == key::OMDB).unwrap();
    assert_eq!(omdb.state, CredentialState::Missing);

    // The shape every transport serialises.
    let json = serde_json::to_string(&statuses).unwrap();
    let pretty = serde_json::to_string_pretty(&statuses).unwrap();
    let debug = format!("{statuses:?} {service:?}", service = service.resolver());
    assert!(json.contains("\"active\""));
    assert!(!json.contains("builtin") && !json.contains("built-in"));
    for text in [&json, &pretty, &debug] {
        assert_clean("status data", text, &ALL_KEYS);
    }

    // Replacing and removing keeps it that way.
    service
        .set(key::TMDB, API_KEY, "another-dummy-key-44")
        .unwrap();
    let after_set = serde_json::to_string(&service.list().unwrap()).unwrap();
    assert_clean("status after set", &after_set, &ALL_KEYS);
    service.remove(key::TMDB).unwrap();
    let after_remove = serde_json::to_string(&service.list().unwrap()).unwrap();
    assert_clean("status after remove", &after_remove, &ALL_KEYS);
    assert!(after_remove.contains("\"missing\""));
}

#[test]
fn settings_files_hold_no_key() {
    let dir = tempfile::tempdir().unwrap();
    let service = service();
    let settings = SettingsService::open(SettingsStore::new(dir.path().join("settings.json")));

    // What the app does around credentials: the user picks sources and changes their keys.
    settings
        .set("providers.movies", json!(["tmdb", "omdb"]))
        .unwrap();
    service
        .set(key::TMDB, API_KEY, "replacement-dummy-key-88")
        .unwrap();
    settings
        .set("providers.tmdb.language", json!("en"))
        .unwrap();
    // A status snapshot saved by mistake would still carry no key.
    settings
        .set(
            "providers.status",
            serde_json::to_value(service.list().unwrap()).unwrap(),
        )
        .unwrap();

    let mut files = Vec::new();
    read_all(dir.path(), &mut files);
    assert!(!files.is_empty());
    for (name, bytes) in files {
        // A key is never written to settings: it lives in the credential store.
        assert_clean(
            &name,
            &String::from_utf8_lossy(&bytes),
            &[TMDB_KEY, TRAKT_ID, MDBLIST_KEY, "replacement-dummy-key-88"],
        );
    }
}

#[test]
fn log_output_masks_saved_keys_even_when_code_prints_one() {
    // Saving a key registers it with the redactor.
    let _service = service();
    let buffer = LogBuffer::default();
    let writer = buffer.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .with_writer(RedactingMakeWriter::new(move || writer.clone()))
        .finish();
    tracing::subscriber::with_default(subscriber, || {
        tracing::warn!("request failed: https://example.test/3/{TMDB_KEY}/movie");
        tracing::debug!(client = TRAKT_ID, "trakt device flow started");
        tracing::error!("{}", format!("secret was {MDBLIST_KEY}"));
    });
    let logs = String::from_utf8_lossy(&buffer.0.lock().unwrap()).into_owned();
    assert!(logs.contains("[REDACTED]"), "{logs}");
    assert_clean("logs", &logs, &ALL_KEYS);
}

#[test]
fn export_text_masks_saved_keys() {
    let _service = service();
    // A diagnostics export is plain text that passes through the same redactor.
    let export = format!(
        "tmdb state=active\nlast url https://example.test/3/{TMDB_KEY}/movie?x=1\n\
         trakt={TRAKT_ID}/{MDBLIST_KEY}\n"
    );
    let exported = redact(&export);
    assert!(exported.contains("state=active"));
    assert_clean("export", &exported, &ALL_KEYS);
}
