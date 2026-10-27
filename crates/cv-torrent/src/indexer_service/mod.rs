//! The managed indexer service: process, configuration, admin API and provisioning.
//!
//! * [`admin`]: the service's admin API on its loopback port;
//! * [`config`]: `ServerConfig.json` (the keys CineVault owns, a free port, an API key);
//! * [`process`]: starting the program on a loopback port, waiting until it answers, stopping it,
//!   and making sure it never outlives the app;
//! * [`provision`]: choosing, setting up, testing and keeping the public indexers.

pub mod admin;
pub mod config;
pub mod process;
pub mod provision;

use std::io;

pub use admin::{AdminClient, AdminError, IndexerInfo};
pub use config::{ServerConfigFile, free_loopback_port, generate_api_key};
pub use process::{
    IndexerProcess, Launcher, ProcessLauncher, READY_TIMEOUT, Running, ServiceHandle, StartParams,
};

/// Why the indexer service could not be started or kept running. Every message is ready to show.
#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: io::Error,
    },
    #[error("The indexer service settings could not be written: {0}")]
    Config(String),
    #[error("The indexer service stopped right after it started ({0}).")]
    Exited(String),
    #[error("The indexer service did not answer within {0} seconds.")]
    NotReady(u64),
    #[error("Starting the indexer service was cancelled.")]
    Cancelled,
}

impl ServiceError {
    pub(crate) fn io(context: impl Into<String>, source: io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }
}
