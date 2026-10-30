//! Builds the app services once, at launch, and keeps them for the state objects to borrow.
//!
//! The rules live in `cv_app::services`. This module only supplies what the desktop app adds:
//! the OS credential store (the user's own provider keys), and logging. When the
//! services cannot start (no data folder, a library file that cannot be opened), the window
//! still opens: every state object falls back to what it can do without the library, and the
//! reason reaches the notification centre.

use std::sync::Arc;

use cv_app::data_location::DataLocation;
use cv_app::logging;
use cv_app::notices::{AppNotice, NoticeHub, NoticeLevel};
use cv_app::owner::{self, OwnerGuard};
use cv_app::secrets::SecretStore;
use cv_app::services::{AppServices, BootOptions};

/// Held in Tauri state.
pub struct Boot {
    services: Option<Arc<AppServices>>,
    notices: Arc<NoticeHub>,
    /// The data folder's owner file, kept until the process ends.
    _owner: Option<OwnerGuard>,
    /// Another process holds the data folder, so this copy opens nothing and only reports that.
    already_running: bool,
    already_running_message: String,
}

impl Boot {
    /// Starts logging and builds the services.
    pub fn start() -> Self {
        logging::init();
        let Some(location) = DataLocation::resolve_for_process() else {
            return Self::failed(
                "CineVault could not find a folder for its data, so the library and settings \
                 cannot be saved."
                    .to_owned(),
                None,
            );
        };
        // The owner file is the portable one-writer rule: the desktop app and the server check
        // it on every system, so neither opens a library the other holds.
        let owner = match owner::acquire(location.root(), "app") {
            Ok(Some(guard)) => Some(guard),
            Ok(None) => {
                let message = owner::read_owner(location.root()).map_or_else(
                    || "CineVault is already running.".to_owned(),
                    |owner| format!("{} Stop it, or open another data folder.", owner.sentence()),
                );
                return Self::held_elsewhere(message);
            }
            Err(error) => {
                tracing::warn!(%error, "the owner file could not be taken; starting without it");
                None
            }
        };
        let options = BootOptions::new(
            location.clone(),
            Arc::new(SecretStore::for_location(&location)),
        );
        match AppServices::boot(options) {
            Ok(services) => Self {
                notices: Arc::clone(services.notices()),
                services: Some(services),
                _owner: owner,
                already_running: false,
                already_running_message: String::new(),
            },
            Err(error) => {
                tracing::error!(%error, "the app services could not start");
                Self::failed(error.to_string(), owner)
            }
        }
    }

    /// A boot around services built by a test.
    #[cfg(test)]
    pub fn for_services(services: Arc<AppServices>) -> Self {
        Self {
            notices: Arc::clone(services.notices()),
            services: Some(services),
            _owner: None,
            already_running: false,
            already_running_message: String::new(),
        }
    }

    /// The state of a copy that found the data folder held by another process. It opens nothing,
    /// because the library and settings belong to the running copy.
    fn held_elsewhere(message: String) -> Self {
        Self {
            services: None,
            notices: Arc::new(NoticeHub::new()),
            _owner: None,
            already_running: true,
            already_running_message: message,
        }
    }

    /// True when another copy of the app holds the data folder's lock. The shell reports that
    /// and leaves this copy's services empty.
    pub fn already_running(&self) -> bool {
        self.already_running
    }

    /// The sentence a copy shows when another process holds the library.
    pub fn already_running_message(&self) -> &str {
        &self.already_running_message
    }

    fn failed(message: String, owner: Option<OwnerGuard>) -> Self {
        let notices = Arc::new(NoticeHub::new());
        notices.publish(
            AppNotice::new(NoticeLevel::Error, "CineVault could not open your library")
                .detail(message)
                .source("Startup"),
        );
        Self {
            services: None,
            notices,
            _owner: owner,
            already_running: false,
            already_running_message: String::new(),
        }
    }

    /// The services, or `None` when they could not start.
    pub fn services(&self) -> Option<&Arc<AppServices>> {
        self.services.as_ref()
    }

    /// Where notices for the notification centre go.
    pub fn notices(&self) -> &Arc<NoticeHub> {
        &self.notices
    }

    /// Begins the background work and resumes the jobs of the previous session. Needs the async
    /// runtime, so the shell calls it from a spawned task.
    pub fn start_background(&self) {
        if let Some(services) = &self.services {
            services.start();
        }
    }

    /// Pauses the jobs so the next launch resumes them. Called when the app exits.
    pub fn shutdown(&self) {
        if let Some(services) = &self.services {
            services.shutdown();
        }
    }
}
