//! Rename planning, execution, conflict detection and the undo journal.
//!
//! This crate starts with the **planner**: pure functions from a movie or show, its
//! files and a naming profile to a plan of what moves where. Nothing touches the disk.
//!
//! - [`MovieRenamer`] renders movie folder and file names and plans the rename of one movie:
//!   [`MovieRenamer::plan`] gives a [`MoviePlan`].
//! - [`TvRenamer`] does the same for a show with its seasons and episodes:
//!   [`TvRenamer::plan`] gives a [`TvPlan`].
//! - [`conflict::detect`] finds the conflicts of a batch of plans (duplicate and foreign
//!   destinations, path length, case-only renames) and [`conflict::resolve()`] applies the user's
//!   choice (keep as versions, skip, edit).
//! - [`exec::Executor`] applies a batch to the disk: write-ahead journal, same-volume rename or
//!   cross-volume copy-verify-delete, a `.deletedByCineVault` trash, empty-folder and
//!   unwanted-file cleanup; [`exec::recover`] settles a crash.
//! - [`exec::undo_batch`] takes a journaled batch back in reverse, verifying every destination
//!   and reporting what it could not restore.
//! - [`preset::Preset`] and [`profile::ProfileBook`] hold the naming presets (Clean, Detailed,
//!   Media-server safe), custom profiles and the active one.
//! - [`media_guard`] holds back a title whose names use media-info tokens while its video details
//!   are not read, so a rename never strips the quality tags from a name.
//! - [`clean::create_destination`] is the clean-up every rendered name goes through.
//! - [`MovieSettings`] and [`TvSettings`] hold the naming profile; their `Default` is a fresh
//!   install.
//!
//! The behaviour is pinned by the compatibility fixtures `tests/compat/fixtures/rename` (run
//! by `cargo test -p cv-compat`).
//!
//! ```
//! use cv_domain::Item;
//! use cv_rename::{MovieRenamer, MovieSettings};
//!
//! let mut movie = Item::movie("Dune: Part Two");
//! movie.core.year = 2024;
//! let renamer = MovieRenamer::new(MovieSettings::default());
//! assert_eq!(renamer.folder_name(&movie, None).unwrap(), "Dune - Part Two (2024)");
//! ```

pub mod clean;
pub mod conflict;
pub mod error;
pub mod exec;
pub mod lang;
pub mod media_guard;
pub mod movie;
pub mod multi;
pub mod naming;
pub mod options;
pub mod pattern;
pub mod plan;
pub mod preset;
pub mod profile;
pub mod render;
pub mod tv;

pub use error::RenameError;
pub use lang::SubtitleLanguageStyle;
pub use media_guard::{MEDIA_HELD_REASON, MediaNeeds};
pub use movie::{MovieInput, MoviePlan, MovieRenamer};
pub use options::{MovieSettings, MultiEpisodeStyle, NameOptions, SpaceSubstitution, TvSettings};
pub use plan::{
    Fate, FolderChange, Op, OpKind, Owner, PlanFile, PlannedFile, Problem, SubtitleTag,
};
pub use preset::Preset;
pub use profile::{ProfileBook, ProfileError, RenameProfile};
pub use tv::{EpisodeInput, SeasonInput, TvInput, TvPlan, TvRenamer};
