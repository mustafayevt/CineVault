//! Golden-file test runner over `tests/compat/`.
//!
//! The golden files pin CineVault's output byte for byte: the NFO files it writes in the Kodi,
//! Jellyfin, Emby and MediaPortal formats, the names the renamer builds, and what the name parsers
//! read from file and folder names. This crate loads every fixture case together with its
//! expected output, hands each case to the [`Handler`] registered for its [`Family`], and reports
//! one outcome per fixture:
//!
//! - **pass**: the handler's output equals the expected output;
//! - **fail**: the output differs, the handler reported an error, or it panicked;
//! - **pending**: no handler is registered for the family.
//!
//! Run the suite with `cargo test -p cv-compat` or print the per-fixture report with
//! `cargo run -p cv-compat --bin compat-report -- --all`. After an intended change in behaviour,
//! `CINEVAULT_UPDATE_COMPAT=1 cargo test -p cv-compat --test suite` (or `compat-report --update`)
//! writes the current output over the expected files; review that diff before committing.

mod entity;
mod family;
mod fixture;
mod handlers;
mod nfo;
mod nfo_tv;
pub mod presets;
mod rename;
mod run;
mod update;

pub use family::{Family, Layout};
pub use fixture::{Case, Expected, LoadError, Suite, default_root, load};
pub use handlers::registered;
pub use run::{Actual, Entry, Handler, HandlerError, Outcome, Report, run_suite};
pub use update::{UPDATE_ENV, UpdateError, update_expected, update_requested};

/// The API key value the TV NFO handler writes into `episodeguide` URLs, so the golden files never
/// hold a real key.
pub const API_KEY_PLACEHOLDER: &str = "cv-compat-api-key";
