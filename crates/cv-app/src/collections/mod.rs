//! Collections (movie sets): grouping movies into franchises, with owned and missing titles.
//!
//! Spec: `movie-collections` ("Collections as a view of movies", "Automatic collection
//! assignment", "Missing movies in a collection", "Manual collection management"). The artwork and
//! NFO locations of collections are in [`crate::nfo`].
//!
//! A collection is a library item of kind `set`; a movie is in a collection when its `parent_id`
//! is that item, so a movie is in at most one. The library page for collections is the ordinary
//! library listing; this module adds what a single collection needs.
//!
//! * [`CollectionService`] reads a collection page (movies in release order, with the movies the
//!   library lacks), creates, edits and deletes collections, puts movies in and out, links a
//!   collection to a TMDB collection and runs "Find collections" for a selection. Every change
//!   that can be undone returns a token for [`CollectionService::undo`].
//! * Deleting a collection never deletes its movies; they stay without a collection.
//! * [`assign_scraped_movie`] is what a scrape with the collection field selected calls: the movie
//!   goes into its TMDB collection, which is created from the TMDB data when the library has none.
//!   A failure there is logged and never fails the scrape.
//!
//! # Missing movies
//!
//! Fetching a TMDB collection stores the movies of it that the library lacks in the set
//! (`missing_movies`). A movie counts as owned when a movie of the collection has its TMDB or IMDb
//! ID, or, without IDs, its title and year. Movies that have not come out yet (a release date after
//! today, or neither date nor year) are left out, so the counts read "owned of released". The page
//! checks the stored list against the movies again every time it is read, so a movie added by hand
//! leaves the missing list at once.
//!
//! The NFO is a copy of the library values, so a failed NFO write never undoes a change: it is
//! reported next to the result.

mod model;
mod remote;
mod service;
mod store;
#[cfg(test)]
mod tests;

use cv_db::DbError;
use cv_domain::CalendarDate;
use cv_providers::ProviderError;

pub use model::{
    Assigned, CollectionEntry, CollectionPage, CollectionSummary, DeleteReport, Failure,
    FindOutcome, FindReport, UndoReport,
};
pub use service::CollectionService;
pub use store::MAX_TITLE_LEN;

use cv_domain::{ItemKind, MetaField};

use crate::matching::MatchSettings;

pub(crate) use remote::assign_scraped_movie;
pub(crate) use store::{ensure_named, play_order};

/// Whether a scrape with these settings should put a movie into its collection: it is a movie and
/// the collection field is one of the fields the scrape writes and `movies.collections.auto-group`
/// is not switched off.
pub(crate) fn wanted_after_scrape(settings: &MatchSettings, kind: ItemKind) -> bool {
    kind == ItemKind::Movie
        && settings.scrape.auto_group_collections
        && settings
            .scrape
            .options_for(kind)
            .fields
            .contains(MetaField::Collection)
}

/// Why a collection operation failed.
#[derive(Debug, thiserror::Error)]
pub enum CollectionError {
    #[error("That collection is no longer in the library.")]
    NotFound,
    #[error("The library could not be read or written: {0}")]
    Db(#[from] DbError),
    /// A value was refused. Nothing was saved.
    #[error("{0}")]
    Invalid(String),
    /// The operation to undo is unknown, already undone or too old.
    #[error("This change can no longer be undone.")]
    NothingToUndo,
    #[error(
        "No metadata provider with collections is ready. Add a TMDB key in Settings, Scraping and providers."
    )]
    NoProvider,
    #[error("{0}")]
    Provider(ProviderError),
}

impl CollectionError {
    /// A stable code for the UI: `not_found`, `storage`, `invalid`, `nothing_to_undo`,
    /// `no_provider` or `provider`.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotFound => "not_found",
            Self::Db(_) => "storage",
            Self::Invalid(_) => "invalid",
            Self::NothingToUndo => "nothing_to_undo",
            Self::NoProvider => "no_provider",
            Self::Provider(_) => "provider",
        }
    }
}

/// Today's date in UTC, which decides which movies have come out.
pub(crate) fn today() -> CalendarDate {
    use chrono::Datelike;
    let now = chrono::Utc::now();
    let month = u8::try_from(now.month()).unwrap_or(1);
    let day = u8::try_from(now.day()).unwrap_or(1);
    // `new` only refuses an impossible date; the epoch is the (unreachable) fallback.
    CalendarDate::new(now.year(), month, day)
        .or_else(|| CalendarDate::new(1970, 1, 1))
        .unwrap_or_else(|| unreachable!("1970-01-01 is a date"))
}
