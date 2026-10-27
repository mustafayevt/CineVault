//! Organize: previewing, applying and undoing folder and file renames from naming templates.
//!
//! This is the service behind the Organize screen. The rules live in `cv-rename`;
//! this module joins them to the library and the disk:
//!
//! 1. **Load** ([`load`]): the movies and shows in scope come out of the library with their files
//!    as [`cv_rename::PlanFile`]s (paths relative to the data source root).
//! 2. **Plan** ([`OrganizeService::preview`]): every item is planned under the profile, batch
//!    conflicts are detected against the real file system and the library
//!    ([`cv_rename::conflict::detect`]), the user's resolutions (keep as versions, skip, edit) are
//!    applied and detection runs again. The result is a [`Preview`]: a diff row per item that
//!    changes, the conflicts with the resolutions on offer, the counts (will change, already tidy,
//!    conflicts, locked) and three live examples. Nothing touches the disk.
//! 3. **Apply** ([`OrganizeService::apply`]): the same plan is made again from the same request (so
//!    a disk that changed after the preview is noticed), the checked items that are free of
//!    conflicts run through the [`cv_rename::exec::Executor`] and one journal batch is written.
//! 4. **Journal and undo** ([`OrganizeService::journal`], [`OrganizeService::undo`]): the batches
//!    newest first, and the reverse replay with the partial-undo report.
//! 5. **Start-up** ([`recover_interrupted_batches`]): the composition root calls it once, before
//!    the first scan, to settle what a crash left half done.
//!
//! The template editor reads [`check_pattern`] (diagnostics with positions and suggestions) and
//! [`token_catalogue`] (the `$` autocomplete list), both from the `cv-template` alias maps and
//! validator.

mod check;
mod examples;
mod file_example;
mod load;
mod model;
mod numbered;
mod patterns;
mod service;
mod startup;
mod state;

#[cfg(test)]
mod tests;

pub use check::{Finding, FindingSeverity, TokenInfo, check_pattern, token_catalogue};
pub use file_example::{FileExample, SAMPLE_FILE, example_for_file, tag_count};
pub use model::{
    Applied, AppliedItem, AppliedStatus, ApplyRequest, BusyTitle, Choice, CompanionCounts,
    ConflictCode, ConflictRow, Counts, Example, ExampleKind, FileChange, JournalBatch,
    NotRestorable, OrganizeError, PatternField, Preview, PreviewRequest, PreviewRow,
    ResolutionOption, RowKind, RowStatus, Scope, TemplateProblem, UndoOutcome,
};
pub use numbered::{numbered_names, numbered_names_on};
pub use patterns::PatternSet;
pub(crate) use service::record_rename_from_journal;
pub use service::{ChangedHook, OrganizeService};
pub use startup::{
    DEFERRED_RECOVERY_INTERVAL, recover_interrupted_batches, retry_deferred_recovery,
};
pub use state::{JOB_NAME as STATE_JOB_NAME, OrganizeStateService, current_profile, profile_key};
