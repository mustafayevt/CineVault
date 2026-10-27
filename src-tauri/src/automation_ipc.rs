//! The channel by which the `cinevault` command line asks the running app to execute a batch.
//!
//! The library has one writer: when the app is open, a command-line batch runs here, on the
//! app's own services, instead of in a second process. The rules (the protocol, the batch steps,
//! the selection) live in `cv_app::automation`; this module starts the listener when the app
//! starts and stops it when the app exits. The renames of a forwarded batch tell the poster index
//! behind `cv://img` about every move, exactly as the Organize page does.
//!
//! If the channel cannot be opened (no free pipe name, a read-only data folder), the app goes on
//! without it and the command line runs its batches in its own process.

use std::sync::{Arc, Mutex, PoisonError};

use cv_app::automation::ipc::IpcServer;
use cv_app::automation::{BatchEngine, CommandBackend, EngineOptions};
use cv_artwork::library_source::LibrarySources;
use cv_db::repo::rename_journal::PathObserver;

use crate::boot::Boot;

/// Held in Tauri state.
#[derive(Default)]
pub struct AutomationState {
    server: Mutex<Option<IpcServer>>,
    /// Runs the plans of the selection actions.
    commands: Mutex<Option<Arc<CommandBackend>>>,
}

impl AutomationState {
    /// Opens the channel. Needs the async runtime.
    pub fn start(&self, boot: &Boot, sources: &Arc<LibrarySources>) {
        let Some(services) = boot.services() else {
            return;
        };
        let observer: Arc<dyn PathObserver> = sources.clone();
        let engine = BatchEngine::new(
            Arc::clone(services),
            EngineOptions {
                path_observer: Some(observer),
                ..EngineOptions::default()
            },
        );
        let commands = CommandBackend::start(Arc::clone(&engine));
        *self.commands.lock().unwrap_or_else(PoisonError::into_inner) = Some(commands);
        match IpcServer::start(engine, services.location()) {
            Ok(server) => {
                *self.server.lock().unwrap_or_else(PoisonError::into_inner) = Some(server);
            }
            Err(error) => {
                tracing::warn!(%error, "the command-line channel could not be opened");
            }
        }
    }

    /// The runner of batches, once the app has started it. The selection actions queue their
    /// plans here.
    pub fn backend(&self) -> Option<Arc<CommandBackend>> {
        self.commands
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Closes the channel and removes the endpoint file.
    pub fn stop(&self) {
        self.server
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        self.commands
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
    }

    /// Whether the channel is open.
    #[cfg(test)]
    pub fn is_open(&self) -> bool {
        self.server
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some()
    }
}

#[cfg(test)]
mod tests {
    use cv_app::automation::ipc::{ForwardError, forward};
    use cv_app::automation::{BatchRequest, EntityKind};
    use cv_app::data_location::{DataLocation, LocationInputs};
    use cv_app::secrets::SecretStore;
    use cv_app::services::{AppServices, BootOptions};

    use super::*;

    fn services(root: &std::path::Path) -> (DataLocation, Arc<AppServices>) {
        let inputs = LocationInputs {
            exe_dir: root.join("app"),
            user_data_base: root.join("data"),
            user_config_base: root.join("config"),
        };
        std::fs::create_dir_all(&inputs.exe_dir).unwrap();
        let location = DataLocation::resolve(&inputs);
        let options = BootOptions::new(location.clone(), Arc::new(SecretStore::in_memory()))
            .without_providers();
        (location, AppServices::boot(options).expect("boot"))
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_running_app_executes_a_batch_sent_from_the_command_line() {
        let dir = tempfile::tempdir().unwrap();
        let (location, services) = services(dir.path());
        let boot = Boot::for_services(Arc::clone(&services));
        let sources = Arc::new(LibrarySources::new());
        let state = AutomationState::default();
        state.start(&boot, &sources);
        assert!(state.is_open());

        let mut request = BatchRequest::new(EntityKind::Movie);
        request.steps.nfo = true;
        request.dry_run = true;
        let report = forward(&location, &request, |_| {})
            .await
            .expect("forwarded");
        assert!(report.forwarded && report.error.is_none());

        state.stop();
        assert!(matches!(
            forward(&location, &request, |_| {}).await,
            Err(ForwardError::NoRunningApp)
        ));
    }
}
