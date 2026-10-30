//! Data-source scanning: parallel walker, file classification and name parsers.
//!
//! - [`classify`]: file-type classification by path (video, trailer, extras, artwork, disc files).
//! - [`companion`]: which video a file belongs to by its name in a folder several titles share.
//! - [`episode`]: the episode and season parser for TV files.
//! - [`lock`]: the per-data-source lock rescans and file moves share.
//! - [`parse`]: movie name parsers (title, year, edition, media source, provider IDs).
//! - [`rescan`]: incremental parallel rescans against the library: fingerprints, change and
//!   replace detection, the Missing-files state, and the persistent rescan job.
//! - [`skip`]: the skip rules (marker files, system folders, hidden folders, user patterns).
//! - [`stacking`]: the stacking parser for multi-part files and folders (`cd1`, `part2`, `1of2`).
//! - [`sources`]: data-source path normalisation and the nested-folder and system-folder guards.
//! - [`unmanaged`]: "Find missing", the videos of a data source that belong to no item.
//! - [`watch`]: optional file watching with a debounce that queues rescans.
//! - [`walk`]: the movie and TV show walkers that group files into movies, shows, seasons and
//!   episodes.

pub mod classify;
pub mod companion;
pub mod episode;
mod gate;
pub mod lock;
mod par;
pub mod parse;
pub mod rescan;
pub mod skip;
pub mod sources;
pub mod stacking;
pub mod unmanaged;
pub mod walk;
pub mod watch;
