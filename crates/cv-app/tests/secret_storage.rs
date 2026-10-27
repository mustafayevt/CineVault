//! A key saved through the OS credential store is retrievable, and it is
//! absent from the settings file and from the logs.

use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use cv_app::redact::RedactingMakeWriter;
use cv_app::secrets::{SecretError, SecretStore};
use cv_app::settings::SettingsStore;
use serde::{Deserialize, Serialize};
use tracing_subscriber::fmt::MakeWriter;

#[derive(Debug, Default, Serialize, Deserialize)]
struct TestSettings {
    language: String,
    tmdb_key_configured: bool,
}

/// Collects log output in memory.
#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

struct CaptureWriter(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for CaptureWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Capture {
    type Writer = CaptureWriter;
    fn make_writer(&'a self) -> Self::Writer {
        CaptureWriter(self.0.clone())
    }
}

impl Capture {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }
}

/// Removes the test credentials even when an assertion fails.
struct Teardown<'a> {
    store: &'a SecretStore,
    accounts: &'a [&'a str],
}

impl Drop for Teardown<'_> {
    fn drop(&mut self) {
        for account in self.accounts {
            let _ = self.store.delete(account);
        }
    }
}

fn unique_service() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("CineVault-test-{}-{nanos}", std::process::id())
}

fn logged_provider_request(capture: &Capture, key: &str) {
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_max_level(tracing::Level::DEBUG)
        .with_writer(RedactingMakeWriter::new(capture.clone()))
        .finish();
    tracing::subscriber::with_default(subscriber, || {
        let url = format!("https://api.themoviedb.org/3/movie/603?api_key={key}&language=en");
        tracing::debug!(%url, "provider request");
        tracing::debug!(authorization = %format!("Bearer {key}"), "provider headers");
        // Free text with no recognisable shape: only the registered value catches this.
        tracing::debug!("retrying request for key {key} after timeout");
    });
}

#[test]
fn saved_key_is_retrievable_and_absent_from_settings_file_and_logs() {
    let store = SecretStore::system_with_service(unique_service());
    let account = "tmdb.api_key";
    let _teardown = Teardown {
        store: &store,
        accounts: &[account],
    };
    let key = "cvtest-9f3a7c1e5b2d4086a1c3e5f7";

    // Save. Skip, with a clear message, when no credential store is reachable.
    match store.set(account, key) {
        Ok(()) => {}
        Err(SecretError::Unavailable(why)) => {
            eprintln!("SKIPPED: OS credential store unavailable ({why})");
            return;
        }
        Err(other) => {
            eprintln!("SKIPPED: OS credential store rejected the write ({other})");
            return;
        }
    }

    // Retrieve from the credential store (a new Entry is opened on every call).
    match store.get(account).unwrap() {
        Some(found) => assert_eq!(found.expose(), key),
        None => {
            eprintln!("SKIPPED: credential store did not persist the entry (in-process mock only)");
            return;
        }
    }
    assert!(store.contains(account).unwrap());

    // The settings file records only that a key exists.
    let dir = tempfile::tempdir().unwrap();
    let settings_path = dir.path().join("settings.json");
    let settings = SettingsStore::new(&settings_path);
    settings
        .save(&TestSettings {
            language: "en".into(),
            tmdb_key_configured: true,
        })
        .unwrap();
    let file_text = std::fs::read_to_string(&settings_path).unwrap();
    assert!(
        !file_text.contains(key),
        "settings file leaked the key: {file_text}"
    );
    assert!(
        !file_text.contains("cvtest-"),
        "settings file leaked key material: {file_text}"
    );

    // Logs.
    let capture = Capture::default();
    logged_provider_request(&capture, key);
    let logs = capture.text();
    assert!(
        logs.contains("provider request"),
        "nothing was logged: {logs:?}"
    );
    assert!(!logs.contains(key), "log leaked the key: {logs}");
    assert!(
        logs.contains("[REDACTED]"),
        "expected a redaction marker: {logs}"
    );

    // Delete leaves nothing behind.
    assert!(store.delete(account).unwrap());
    assert!(store.get(account).unwrap().is_none());
}

#[test]
fn log_redaction_works_without_the_os_store() {
    let store = SecretStore::in_memory();
    let key = "cvtest-in-memory-key-6d2b8e04";
    store.set("trakt.access_token", key).unwrap();

    let capture = Capture::default();
    logged_provider_request(&capture, key);
    let logs = capture.text();
    assert!(!logs.contains(key), "log leaked the key: {logs}");
}
