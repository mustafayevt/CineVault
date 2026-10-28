//! Repositories: item CRUD, keyset-paged queries and the full-text index.
//!
//! Every function takes a plain [`rusqlite::Connection`]. Reads run inside
//! [`Database::read`](crate::Database::read) and writes inside
//! [`Database::write`](crate::Database::write), whose transaction derefs to a connection, so a
//! caller composes several repository calls into one atomic change.
//!
//! - [`items`]: insert, update, get, list children and delete of whole [`Item`](cv_domain::Item)s
//!   with all their related rows (files, streams, credits, ratings, IDs, tags, genres, artwork
//!   URLs, trailers and episode numbers).
//! - [`filter`] and [`query`]: composable filters and sort keys, and keyset paging over them.
//! - [`facet`]: the filter catalogue (movie, show and set filters) as SQL-backed facets.
//! - [`availability`]: online, offline, missing and invalid states computed from the file system;
//!   nothing is ever deleted.
//! - [`data_sources`]: the movie and TV folders the library is built from, with reorder and a
//!   remap that re-points stored paths without deleting rows.
//! - [`fingerprint`]: the scan fingerprint table and the stored view of a data source that an
//!   incremental rescan compares against.
//! - [`flags`]: the persisted NEW flag and review state.
//! - [`artwork_refs`]: the remembered source URLs of extra fanart and extra thumbs, so artwork can be
//!   downloaded again.
//! - [`graphics`]: the image files recorded for items (and their descendants), for the thumbnail job.
//! - [`locks`]: item locks, field locks and the scrape write path that honours them.
//! - [`jobs`]: persistent background jobs and their items, so jobs resume after a restart.
//! - [`media_info`]: which videos still wait for media info, and the title counts of a data source.
//! - [`match_targets`]: which movies and shows a batch match job works on (new, unmatched, a selection).
//! - [`review`]: the review queue: matches that need a human decision, with reasons, candidates and
//!   the score breakdown, and the persisted ignore.
//! - [`organize_state`]: the cache of whether each title's names match the active naming profile
//!   (tidy, pending, conflict, locked), filled by a background dry run of the renamer planner.
//! - [`size_totals`]: the one rule for Total size (distinct online files) behind the row figure, the
//!   sort and the library total.
//! - [`step_outcome`]: what each after-scrape step (artwork, ratings, collections) did
//!   for each item, with the reason, so skipped and not-found results are visible.
//! - [`rename_journal`]: the write-ahead journal of an organize batch, and the path repointing that
//!   finishes a rename without a rescan.
//! - [`lifecycle`]: the facts and grouped counts behind the title lifecycle (stage and flags).
//! - [`library_ops`]: the reads behind change data source, remove from library and delete with files:
//!   a title's tree of items, its files, and whether its folder is shared.
//! - [`library_review`]: the missing/duplicate review: which rows are missing, offline or duplicates,
//!   the "keep" decision, and re-pointing a title whose files were moved by hand.
//! - [`ratings`]: ratings per source with their scales, and the user rating, which no update or
//!   scrape overwrites.
//! - [`people`]: the actors and directors of the library found by name, for the command palette.
//! - [`top250`]: the IMDb Top 250 position of movies and shows, stored from a fetched chart.
//! - [`torrent_indexers`]: the indexers of the managed indexer service and how the last check left them.
//! - [`torrent_landing`]: the library movies whose copy could be better and the newest additions,
//!   for the Torrent search landing.
//! - [`watched`]: watched state and play counts of movies and episodes, with show and season expansion.
//! - [`history`]: the lasting record of renames, moves, deletes, artwork and match changes, with
//!   their titles, for the History page and its Undo.
//! - [`fts`]: the `item_fts` index, kept in step by every insert and update, and search.
//! - [`search`]: the folded title keys and the forgiving quick search over them (punctuation,
//!   spacing, diacritics, numerals, small typos) with its ranking.

pub mod artwork_refs;
pub mod availability;
mod children;
mod codec;
pub mod data_sources;
pub mod date_added;
pub mod drive_facts;
pub mod facet;
pub mod filter;
pub mod fingerprint;
pub mod flags;
pub mod fts;
pub mod graphics;
pub mod history;
pub mod items;
pub mod jobs;
pub mod library_ops;
pub mod library_review;
pub mod lifecycle;
pub mod locks;
pub mod match_targets;
pub mod media_info;
pub mod organize_state;
pub(crate) mod path_case;
pub mod people;
pub mod query;
pub mod ratings;
pub mod rename_journal;
pub mod review;
pub mod search;
pub mod size_totals;
pub mod step_outcome;
pub mod table_facts;
pub mod top250;
pub mod torrent_indexers;
pub mod torrent_landing;
pub mod watched;

#[cfg(test)]
mod data_sources_tests;
#[cfg(test)]
mod fingerprint_tests;
#[cfg(test)]
mod jobs_tests;
#[cfg(test)]
mod library_window_tests;
#[cfg(test)]
mod rating_preference_tests;
#[cfg(test)]
mod search_tests;
#[cfg(test)]
mod source_label_tests;
#[cfg(test)]
mod stage_filter_tests;
#[cfg(test)]
mod tests;

pub use availability::{
    AvailabilityChange, AvailabilityPlan, AvailabilityReport, FileProbe, PathStyle, StdFileProbe,
    apply_availability, get_availability, plan_availability, set_availability,
};
pub use facet::{
    FacetCondition, FacetValue, FilterDef, FilterId, FilterKind, NumOp, catalogue, facet_values,
};
pub use filter::{Filter, IntRange, ItemFlag, MatchMode, TextRange};
pub use flags::{
    ItemFlags, clear_all_new, clear_new_children, count_new, count_new_children, get_flags,
    new_ids, set_new, set_review_state, set_review_states,
};
pub use items::{
    add_person_id, children_of, delete_item, delete_tree, find_set_by_title, get_item, get_summary,
    insert_item, item_exists, items_by_provider_id, prune_unused_lookups, set_titles, update_item,
};
pub use locks::{
    apply_scrape, field_locks, field_locks_of, set_field_lock, set_field_locks, set_item_locked,
};
pub use query::{
    Cursor, CursorValue, Direction, ItemQuery, ItemSummary, Page, SortField, SortKey, SourceGroup,
    count_items, list_items, list_items_at, source_groups,
};
pub use ratings::{
    get_ratings, get_user_rating, rating_fetch_candidates, remove_rating, set_user_rating,
    upsert_ratings,
};
pub use top250::{Top250Report, apply_top250};
pub use watched::{WatchState, get_watch_state, playable_in, set_watch_state};
