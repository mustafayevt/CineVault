//! CineVault's browser server as a library.
//!
//! The headless `cinevault-server` ([`headless`]) and the desktop app's Browser access
//! ([`embed`]) serve the same routes ([`server`]) over the same command layer (`cv-ipc`). The
//! sign-in rules live in [`auth`], the home-network rule in [`network`].

pub mod auth;
mod dispatch;
pub mod embed;
pub mod headless;
pub mod network;
mod server;
pub mod state;

#[cfg(test)]
mod tests;
