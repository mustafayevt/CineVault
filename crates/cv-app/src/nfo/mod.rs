//! NFO naming, ownership and the Read NFO / Rewrite NFO actions with undo.
//!
//! * [`settings`]: the NFO options as the settings page stores them.
//! * [`paths`]: where each item's NFO files go, and which files could be its own.
//! * [`model`]: library items to the plain models the `cv-nfo` writers print.
//! * [`merge`]: a read NFO applied to a library item.
//! * [`preview`]: the NFO a movie would get, rendered without writing.
//! * [`service`]: the actions themselves ([`NfoService`]), their reports and the undo records.
//! * [`follow`]: the NFOs of changed titles are written again ([`follow::NfoFollower`]).
//!
//! Ownership is the safety rule behind all of it: a file is CineVault's only when it parses with
//! a title (`cv_nfo::ownership`), a foreign file is never overwritten, and cleanup only ever
//! touches managed files that are candidates of the item (`paths::NfoTargets`).

pub mod follow;
pub mod merge;
pub mod model;
pub mod paths;
pub mod preview;
mod readback;
pub mod report;
pub mod retention;
mod rewrite;
pub mod service;
pub mod settings;
pub mod time;

#[cfg(test)]
mod tests;

pub use preview::{NfoPreview, NfoSync};
pub use report::{
    ActionKind, ActionReport, FileOutcome, FileReport, SkipReason, Totals, UndoSummary, UnitReport,
    UnitStatus,
};
pub use service::{NfoError, NfoScope, NfoService};
pub use settings::NfoSettings;
