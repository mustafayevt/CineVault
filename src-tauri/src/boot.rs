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
use cv_app::secrets::SecretStore;
use cv_app::services::{AppServices, BootOptions};

/// Held in Tauri state.
pub struct Boot {
    services: Option<Arc<AppServices>>,
    notices: Arc<NoticeHub>,
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
            );
        };
        let options = BootOptions::new(
            location.clone(),
            Arc::new(SecretStore::for_location(&location)),
        );
        match AppServices::boot(options) {
            Ok(services) => Self {
                notices: Arc::clone(services.notices()),
                services: Some(services),
            },
            Err(error) => {
                tracing::error!(%error, "the app services could not start");
                Self::failed(error.to_string())
            }
        }
    }

    /// A boot around services built by a test.
    #[cfg(test)]
    pub fn for_services(services: Arc<AppServices>) -> Self {
        Self {
            notices: Arc::clone(services.notices()),
            services: Some(services),
        }
    }

    fn failed(message: String) -> Self {
        let notices = Arc::new(NoticeHub::new());
        notices.publish(
            AppNotice::new(NoticeLevel::Error, "CineVault could not open your library")
                .detail(message)
                .source("Startup"),
        );
        Self {
            services: None,
            notices,
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
