//! Copies of a movie: the same film in the library more than once.
//!
//! The groups come from [`cv_db::repo::copies`], computed on every read. [`rank`] suggests the
//! copy of best quality; [`CopiesService`] gives the screens the counts, the compare rows and the
//! "Keep both" and "Ask again" decisions; [`label_attributes`] gives Rename what a copy's label
//! is made of.

mod labels;
pub mod rank;
mod service;

pub use labels::label_attributes;
pub use service::{
    Comparison, CopiesService, CopyMedia, CopyRow, KeepCopyError, audio_label, open_count,
    source_label,
};
