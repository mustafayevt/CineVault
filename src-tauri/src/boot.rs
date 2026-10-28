//! Builds the app services once, at launch, and keeps them for the state objects to borrow.
//!
//! The rules live in `cv_app::services`. This module only supplies what the desktop app adds:
//! the OS credential store (the user's own provider keys), and logging. When the
//! services cannot start (no data folder, a library file that cannot be opened), the window
//! still opens: every state object falls back to what it can do without the library, and the
//! reason reaches the notification centre.

use std::fs::File;
use std::sync::Arc;

use cv_app::data_location::DataLocation;
use cv_app::logging;
use cv_app::notices::{AppNotice, NoticeHub, NoticeLevel};
use cv_app::secrets::SecretStore;
use cv_app::services::{AppServices, BootOptions};

use crate::instance_lock;

/// Held in Tauri state.
pub struct Boot {
    services: Option<Arc<AppServices>>,
    notices: Arc<NoticeHub>,
    /// The Linux instance lock. It is kept only so that it stays held until the process ends.
    _instance_lock: Option<File>,
    /// Another copy holds the instance lock, so this copy opens nothing and only reports that.
    already_running: bool,
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
        // On Linux a second copy can slip past the single-instance plugin (no session bus), so
        // the data folder's lock is taken before the library opens. Without the lock the copy
        // still starts; the plugin remains the first check.
        let instance_lock = if cfg!(target_os = "linux") {
            match instance_lock::acquire(location.root()) {
                Ok(Some(file)) => Some(file),
                Ok(None) => return Self::held_elsewhere(),
                Err(error) => {
                    tracing::warn!(
                        %error,
                        "the instance lock could not be taken; starting without it"
                    );
                    None
                }
            }
        } else {
            None
        };
        let options = BootOptions::new(
            location.clone(),
            Arc::new(SecretStore::for_location(&location)),
        );
        match AppServices::boot(options) {
            Ok(services) => Self {
                notices: Arc::clone(services.notices()),
                services: Some(services),
                _instance_lock: instance_lock,
                already_running: false,
            },
            Err(error) => {
                tracing::error!(%error, "the app services could not start");
                Self::failed(error.to_string(), instance_lock)
            }
        }
    }

    /// A boot around services built by a test.
    #[cfg(test)]
    pub fn for_services(services: Arc<AppServices>) -> Self {
        Self {
            notices: Arc::clone(services.notices()),
            services: Some(services),
            _instance_lock: None,
            already_running: false,
        }
    }

    /// The state of a copy that found the data folder's lock held by another copy. It opens
    /// nothing, because the library and settings belong to the running copy.
    fn held_elsewhere() -> Self {
        Self {
            services: None,
            notices: Arc::new(NoticeHub::new()),
            _instance_lock: None,
            already_running: true,
        }
    }

    /// True when another copy of the app holds the data folder's lock. The shell reports that
    /// and leaves this copy's services empty.
    pub fn already_running(&self) -> bool {
        self.already_running
    }

    fn failed(message: String, instance_lock: Option<File>) -> Self {
        let notices = Arc::new(NoticeHub::new());
        notices.publish(
            AppNotice::new(NoticeLevel::Error, "CineVault could not open your library")
                .detail(message)
                .source("Startup"),
        );
        Self {
            services: None,
            notices,
            _instance_lock: instance_lock,
            already_running: false,
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
