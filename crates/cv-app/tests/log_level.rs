//! `advanced.log-level`: the running logger follows the setting without a restart, and a setting
//! that returns to its default goes back to the filter the logger started with. Its own test
//! binary, because the logger is process-wide.

use cv_app::app_settings::SettingsService;
use cv_app::logging;
use cv_app::settings::SettingsStore;
use serde_json::json;
use tracing::Level;

#[test]
fn the_log_level_setting_changes_the_running_logger() {
    if std::env::var_os(logging::LOG_ENV).is_some() || std::env::var_os("RUST_LOG").is_some() {
        // The environment owns the filter then; the setting is documented to leave it alone.
        return;
    }
    assert!(logging::init());
    assert!(tracing::enabled!(Level::INFO));
    assert!(!tracing::enabled!(Level::DEBUG));

    let dir = tempfile::tempdir().unwrap();
    let settings = SettingsService::open(SettingsStore::new(dir.path().join("settings.json")));

    settings.set(logging::LEVEL_KEY, json!("debug")).unwrap();
    assert!(logging::apply_from(&settings));
    assert!(
        tracing::enabled!(Level::DEBUG),
        "detailed shows debug lines"
    );

    settings.set(logging::LEVEL_KEY, json!("error")).unwrap();
    assert!(logging::apply_from(&settings));
    assert!(!tracing::enabled!(Level::INFO), "errors only hides notes");
    assert!(tracing::enabled!(Level::ERROR));

    settings.reset(&[logging::LEVEL_KEY]).unwrap();
    assert!(logging::apply_from(&settings));
    assert!(
        tracing::enabled!(Level::INFO),
        "the default is normal detail"
    );
    assert!(!tracing::enabled!(Level::DEBUG));
}
