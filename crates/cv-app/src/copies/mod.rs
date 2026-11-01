//! Copies of a movie: the same film in the library more than once.
//!
//! The groups come from [`cv_db::repo::copies`], computed on every read. [`rank`] suggests the
//! copy of best quality; [`CopiesService`] gives the screens the counts, the compare rows and the
//! "Keep both" and "Ask again" decisions.

pub mod rank;
mod service;

pub use service::{
    Comparison, CopiesService, CopyMedia, CopyRow, audio_label, open_count, source_label,
};
