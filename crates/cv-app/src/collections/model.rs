//! The values the collection service takes and returns.

use cv_domain::{CalendarDate, ItemId};

/// A collection in a list: enough to pick one, link it or draw its card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionSummary {
    pub id: ItemId,
    pub title: String,
    /// The TMDB collection it is linked to, if any.
    pub tmdb_id: Option<String>,
    /// Movies of the collection that are in the library.
    pub owned: u32,
    /// Released movies of the collection that are not in the library.
    pub missing: u32,
}

/// One movie on a collection page: owned (`id` is set) or missing (ghosted).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionEntry {
    /// The library item, `None` for a movie the library does not have.
    pub id: Option<ItemId>,
    pub title: String,
    /// `0` when unknown.
    pub year: u16,
    pub release_date: Option<CalendarDate>,
    pub watched: bool,
    /// The provider's poster for a missing movie; empty for an owned one.
    pub poster_url: String,
    /// The movie's TMDB ID, if known.
    pub tmdb_id: Option<String>,
}

impl CollectionEntry {
    #[must_use]
    pub fn is_owned(&self) -> bool {
        self.id.is_some()
    }
}

/// A collection with its movies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionPage {
    pub id: ItemId,
    pub title: String,
    pub sort_title: String,
    pub plot: String,
    pub tmdb_id: Option<String>,
    /// Owned and missing movies together, in release order.
    pub entries: Vec<CollectionEntry>,
    /// Movies in the library.
    pub owned: u32,
    /// Movies in the collection: the owned ones and the released ones that are missing.
    pub total: u32,
}

/// One movie that was put into a collection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assigned {
    pub movie: ItemId,
    pub set: ItemId,
    /// The collection the movie was in before, if any.
    pub previous: Option<ItemId>,
}

/// A movie a collection search could not place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub movie: ItemId,
    pub message: String,
}

/// What a "Find collections" run did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FindReport {
    /// Movies that were put into a collection.
    pub assigned: Vec<Assigned>,
    /// Collections the run created.
    pub created: Vec<ItemId>,
    /// Movies that already were in the right collection.
    pub unchanged: u32,
    /// Movies the provider says belong to no collection.
    pub no_collection: u32,
    /// Movies whose item lock or collection field lock kept them as they are.
    pub locked: u32,
    pub failed: Vec<Failure>,
}

/// What deleting a collection did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeleteReport {
    /// Movies that were in it and stay in the library without a collection.
    pub movies_kept: u32,
    pub undo: Option<u64>,
    pub nfo_files_written: u32,
    pub nfo_problem: Option<String>,
}

/// What a find run did, with the way back.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FindOutcome {
    pub report: FindReport,
    pub undo: Option<u64>,
    pub nfo_files_written: u32,
    pub nfo_problem: Option<String>,
}

/// What an undo put back.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UndoReport {
    /// The collection that was restored by undoing a delete.
    pub restored_set: Option<ItemId>,
    /// Movies put back where they were.
    pub movies: u32,
    pub nfo_files_written: u32,
    pub nfo_problem: Option<String>,
}
