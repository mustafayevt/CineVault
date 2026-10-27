//! Name parsers for movies: title, year, edition, media source and provider IDs
//!.
//!
//! - [`title`]: [`detect_clean_title_and_year`] turns a file or folder name such as
//!   `The.Matrix.1999.1080p.BluRay.x264-GRP.mkv` into the title `The Matrix` and the year 1999. It
//!   applies user bad words, hard and soft stop words, edition names and bracket groups.
//! - [`ids`]: [`detect_ids`] finds IMDb, TMDB and TVDB IDs in a name, a URL or NFO text.
//! - [`source`]: [`detect_media_source`] finds the media source (`WEB-DL`, `BluRay`, ...).
//!
//! All parsers work on text only. Nothing is read from disk, so they behave the same on every
//! operating system. The byte-exact fixtures are the `title-parsing` and `id-parsing` families in
//! `tests/compat`, run through `cv-compat`; the tests next to this module pin the individual
//! rules and the edge cases the fixtures do not show.

pub mod ids;
pub mod source;
pub mod title;

pub use ids::{ParsedIds, detect_ids};
pub use source::detect_media_source;
pub use title::{
    CleanTitle, HARD_STOPWORDS, SOFT_STOPWORDS, detect_clean_title_and_year,
    detect_clean_title_and_year_at, detect_clean_title_and_year_of_folder, detect_edition,
    remove_stop_and_bad_words_tv,
};

#[cfg(test)]
mod tests;
