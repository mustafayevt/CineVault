//! Process-wide logging with secret redaction.
//!
//! The log detail follows the `advanced.log-level` setting (`error`, `info` or `debug`). It is
//! applied as soon as the settings are loaded and again whenever the setting changes, without a
//! restart. `CINEVAULT_LOG` and `RUST_LOG` stay in charge when they are set: they are the
//! developer's override and the setting does not touch them.

use std::sync::OnceLock;

use serde_json::Value;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Registry, reload};

use crate::app_settings::SettingsService;
use crate::redact::RedactingMakeWriter;

/// Environment variable that sets the log filter, for example `cinevault=debug,info`.
pub const LOG_ENV: &str = "CINEVAULT_LOG";

/// The setting that chooses the log detail: `error`, `info` (default) or `debug`.
pub const LEVEL_KEY: &str = "advanced.log-level";

/// What a running logger can be changed through.
struct Live {
    handle: reload::Handle<EnvFilter, Registry>,
    /// The filter the logger started with: used when the setting returns to its default.
    initial: String,
    /// `CINEVAULT_LOG` or `RUST_LOG` chose the filter, so the setting leaves it alone.
    from_env: bool,
}

static LIVE: OnceLock<Live> = OnceLock::new();

/// Starts logging to standard error through the redacting writer, so a key or token that ends up
/// in a log line is masked before it is written. Filter: `CINEVAULT_LOG`, then `RUST_LOG`, then
/// `info`. Returns `false` when a logger was already installed (a second call is harmless).
pub fn init() -> bool {
    init_with_default("info")
}

/// As [`init`], with `default_filter` when neither environment variable is set. The command line
/// uses `warn`, so a batch prints its own report and not the services' progress notes.
pub fn init_with_default(default_filter: &str) -> bool {
    let from_env =
        EnvFilter::try_from_env(LOG_ENV).is_ok() || EnvFilter::try_from_default_env().is_ok();
    let filter = EnvFilter::try_from_env(LOG_ENV)
        .or_else(|_| EnvFilter::try_from_default_env())
        .unwrap_or_else(|_| EnvFilter::new(default_filter));
    let (filter, handle) = reload::Layer::new(filter);
    let installed = tracing_subscriber::registry()
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer().with_writer(RedactingMakeWriter::new(std::io::stderr)),
        )
        .try_init()
        .is_ok();
    if installed {
        let _ = LIVE.set(Live {
            handle,
            initial: default_filter.to_owned(),
            from_env,
        });
    }
    installed
}

/// The filter directives of a `advanced.log-level` value. An unknown value reads as `info`.
#[must_use]
pub fn directives_for(level: &str) -> &'static str {
    match level {
        "error" => "error",
        // Detailed logs of CineVault's own crates without the HTTP and TLS libraries' chatter.
        "debug" => {
            "debug,hyper=info,hyper_util=info,h2=info,reqwest=info,rustls=info,tungstenite=info,tokio_util=info"
        }
        _ => "info",
    }
}

/// Applies the stored value of `advanced.log-level` (`None` is the default, back to the filter
/// the logger started with). Returns `true` when the filter changed; `false` when no logger runs
/// or the environment chose the filter.
pub fn apply_level(stored: Option<&str>) -> bool {
    let Some(live) = LIVE.get() else {
        return false;
    };
    if live.from_env {
        return false;
    }
    let directives = stored.map_or(live.initial.as_str(), directives_for);
    live.handle.reload(EnvFilter::new(directives)).is_ok()
}

/// Applies the log level held by `settings`.
pub fn apply_from(settings: &SettingsService) -> bool {
    let stored = settings.get(LEVEL_KEY);
    apply_level(stored.as_ref().and_then(Value::as_str))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_level_maps_to_a_filter() {
        assert_eq!(directives_for("error"), "error");
        assert_eq!(directives_for("info"), "info");
        assert!(directives_for("debug").starts_with("debug,"));
        assert_eq!(
            directives_for("loud"),
            "info",
            "unknown values read as normal"
        );
    }

    #[test]
    fn every_directive_parses() {
        for level in ["error", "info", "debug"] {
            EnvFilter::try_new(directives_for(level)).expect(level);
        }
    }
}
