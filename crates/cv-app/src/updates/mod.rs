//! Application updates: look for a newer release at a configurable interval, show
//! its release notes, and install it only after the user confirms.
//!
//! ```text
//! settings ─► checks on? ─no─► Disabled (nothing is requested)
//!                  │ yes
//!                  ▼
//!   channel: the setting, else the built-in channel (the public releases repository)
//!                  ▼
//!   GET manifest (https redirects followed, never to plain http) ─► envelope { payload, signature } ─► ed25519 verify ─► parse payload
//!                  │ bad signature / shape: refused, nothing offered
//!                  ▼
//!   newer than this build? ─► offer { version, notes, size } ──(user confirms)──►
//!   download the build for this platform ─► SHA-256 and size from the signed payload ─►
//!   Windows setup install: run the setup passively over this copy, quit, the setup restarts
//!   the app; otherwise (portable, MSI, macOS): hand the file to the operating system
//! ```
//!
//! * The update checker is **on by default** and reads [`BUILT_IN_CHANNEL`] unless Settings name
//!   another channel; an empty setting means the built-in one.
//! * **Install on close.** With `updates.install-on-close` on (default off), an update that was
//!   downloaded and verified installs when the app closes, without asking: the window's close
//!   request calls [`install_on_close`], which starts the same verified setup the confirm button
//!   starts and lets the close go on (the setup waits for the app to exit). Only a copy the
//!   CineVault setup installed updates itself this way; a portable copy or a Mac keeps waiting for
//!   the confirm button, as opening an installer while quitting would only surprise.
//! * [`manifest`] holds the signed format and the signature check; it is pure.
//! * [`UpdateService`] reads the settings, talks to the channel and downloads the installer.
//!   The offer it hands out is remembered, so installing names a version and never a URL: what
//!   is downloaded can only be what a verified manifest listed.
//!
//! Releases are signed by the maintainers with the private key (`bun run release:publish`).

pub mod install;
pub mod manifest;
mod service;

pub use install::{
    CloseUpdate, InstallMethod, OpenReason, VerifiedInstaller, install_method, install_on_close,
    silent_setup_arguments, start_silent_setup,
};
pub use manifest::{
    BUILT_IN_PUBLIC_KEY, Manifest, ManifestArtifact, parse_public_key, platform_id, verify_manifest,
};
pub use service::{
    BUILT_IN_CHANNEL, CHANNEL_KEY, CHECK_KEY, CheckOutcome, INSTALL_ON_CLOSE_KEY, INTERVAL_KEY,
    LAST_CHECK_KEY, PUBLIC_KEY_KEY, UpdateConfig, UpdateError, UpdateInterval, UpdateOffer,
    UpdateService,
};

#[cfg(test)]
mod tests;
