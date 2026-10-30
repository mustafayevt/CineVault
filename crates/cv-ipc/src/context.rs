//! The one set of services and state objects both surfaces use.
//!
//! `AppServices` already builds the library, settings, jobs, providers and the rest. [`AppContext`]
//! adds the per-surface state objects that used to be `.manage()`d one by one in the Tauri setup,
//! so the server cannot silently miss one. Modules add their state here as they move into this
//! crate; until then the context is the shared services plus whatever has moved.

use std::sync::Arc;

use cv_app::services::AppServices;

/// The services and state a command needs.
#[derive(Clone)]
pub struct AppContext {
    services: Arc<AppServices>,
}

impl AppContext {
    #[must_use]
    pub fn new(services: Arc<AppServices>) -> Self {
        Self { services }
    }

    #[must_use]
    pub fn services(&self) -> &Arc<AppServices> {
        &self.services
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_context_hands_back_the_services_it_was_built_with() {
        let dir = tempfile::tempdir().unwrap();
        let inputs = cv_app::data_location::LocationInputs {
            exe_dir: dir.path().join("exe"),
            user_data_base: dir.path().join("data"),
            user_config_base: dir.path().join("cfg"),
        };
        let folder = dir.path().join("cinevault");
        cv_app::data_location::DataLocation::set_custom_folder(&inputs, &folder).unwrap();
        let location = cv_app::data_location::DataLocation::resolve(&inputs);
        let services = AppServices::boot(
            cv_app::services::BootOptions::new(
                location,
                Arc::new(cv_app::secrets::SecretStore::in_memory()),
            )
            .without_providers()
            .without_background_nfo(),
        )
        .unwrap();
        let context = AppContext::new(Arc::clone(&services));
        assert!(Arc::ptr_eq(context.services(), &services));
    }
}
