//! The `cinevault` command line.
//!
//! `cinevault movie` and `cinevault tvshow` run the library's batch steps (update, scrape, media
//! info, rename, NFO) in a fixed order, for scripts and schedulers. The selection can be
//! narrowed by data source, folder or quick-search query; `--dry-run` plans without
//! changing anything and `--json` prints per-title results. When the CineVault app is open, it
//! runs the batch itself over a local channel, so the library has one writer; otherwise the
//! command opens the library and runs the batch in its own process.

pub mod args;
pub mod output;
pub mod run;

pub use args::Cli;
pub use run::{Env, execute, run_batch};
