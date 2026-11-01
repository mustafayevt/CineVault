//! Copies of a movie: the same film in the library more than once.
//!
//! The groups come from [`cv_db::repo::copies`], computed on every read. [`rank`] suggests the
//! copy of best quality.

pub mod rank;
