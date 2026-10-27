//! TV show extras: season artwork names and the show-artwork fallback, the air
//! information of a show, and duplicate episode detection.
//!
//! * [`season_art`] names the files a season's image is written to (`season01-poster.jpg`,
//!   `season-specials-poster.jpg`, `Season 1/folder.jpg`) from the choice lists of the artwork
//!   settings, and decides when a season without its own image shows the show's.
//! * [`air`] derives first, last and next air dates and the end date of a stopped show from the
//!   known episode list (owned and missing). The NFO writers apply the same rule to `<enddate>`.
//! * [`duplicates`] finds episode numbers that more than one episode claims.
//!
//! Everything here is pure: callers (the show page reader, the artwork writer) pass in what they
//! have loaded.

pub mod air;
pub mod duplicates;
pub mod season_art;

pub use air::{AirInfo, EpisodeAir, air_info};
pub use duplicates::duplicate_numbers;
pub use season_art::{
    SEASON_ART_FALLBACK_KEY, SeasonImage, season_art_fallback, season_targets, with_show_fallback,
};

#[cfg(test)]
mod tests;
