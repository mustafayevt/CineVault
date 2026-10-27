//! [`CollectionService`]: the collection operations the app offers, with undo and NFO upkeep.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use cv_db::Database;
use cv_domain::ItemId;
use cv_providers::MetadataProvider;
use cv_providers::model::RequestOptions;

use super::model::{CollectionPage, CollectionSummary, DeleteReport, FindOutcome, UndoReport};
use super::store::{self, DeletedSet};
use super::{CollectionError, remote, today};
use crate::curation::rewrite_nfo;
use crate::nfo::{NfoService, NfoSettings};

/// How many operations can still be undone.
const UNDO_DEPTH: usize = 16;

enum Undo {
    /// Movies go back to the collection they were in; collections a run created are removed
    /// again when they are empty.
    Assign {
        before: Vec<(ItemId, Option<ItemId>)>,
        created: Vec<ItemId>,
    },
    /// A deleted collection comes back with its movies.
    Delete(Box<DeletedSet>),
}

#[derive(Default)]
struct UndoStore {
    records: VecDeque<(u64, Undo)>,
}

/// Manages collections (movie sets): creating, editing and deleting them, putting movies in and
/// out, linking them to a provider, and finding the collection of movies.
pub struct CollectionService {
    db: Arc<Database>,
    nfo: Arc<NfoService>,
    undo: Mutex<UndoStore>,
    next_token: AtomicU64,
}

impl CollectionService {
    #[must_use]
    pub fn new(db: Arc<Database>, nfo: Arc<NfoService>) -> Self {
        Self {
            db,
            nfo,
            undo: Mutex::new(UndoStore::default()),
            next_token: AtomicU64::new(1),
        }
    }

    // ---- reading -----------------------------------------------------------------------

    /// Every collection with its counts, by name.
    ///
    /// # Errors
    /// Fails when the library cannot be read.
    pub fn list(&self) -> Result<Vec<CollectionSummary>, CollectionError> {
        self.db.read(|c| Ok(store::summaries(c)))?
    }

    /// The movies that are in no collection and have a TMDB or IMDb ID: what "Find collections"
    /// looks at when no selection is given.
    ///
    /// # Errors
    /// Fails when the library cannot be read.
    pub fn ungrouped_movies(&self) -> Result<Vec<ItemId>, CollectionError> {
        self.db.read(|c| Ok(store::ungrouped_movies(c)))?
    }

    /// One collection with its movies in release order. Movies of the collection that the library
    /// lacks are included when they have come out.
    ///
    /// # Errors
    /// Fails with [`CollectionError::NotFound`] for an unknown collection.
    pub fn page(&self, id: ItemId) -> Result<CollectionPage, CollectionError> {
        self.page_on(id, today())
    }

    pub(crate) fn page_on(
        &self,
        id: ItemId,
        today: cv_domain::CalendarDate,
    ) -> Result<CollectionPage, CollectionError> {
        self.db.read(|c| Ok(store::page(c, id, today)))?
    }

    // ---- deleting ----------------------------------------------------------------------

    /// Deletes a collection. Its movies stay in the library without a collection.
    ///
    /// # Errors
    /// Fails with [`CollectionError::NotFound`] for an unknown collection.
    pub fn delete(&self, id: ItemId, nfo: &NfoSettings) -> Result<DeleteReport, CollectionError> {
        let deleted = store::write(&self.db, move |tx| store::delete(tx, id))?;
        let members = deleted.members.clone();
        let (nfo_files_written, nfo_problem) = rewrite_nfo(&self.nfo, &members, nfo);
        let movies_kept = u32::try_from(members.len()).unwrap_or(u32::MAX);
        let undo = Some(self.remember(Undo::Delete(Box::new(deleted))));
        Ok(DeleteReport {
            movies_kept,
            undo,
            nfo_files_written,
            nfo_problem,
        })
    }

    // ---- finding -----------------------------------------------------------------------

    /// Finds the collection of each movie and puts the movie into it, creating collections that
    /// the library lacks ("Find collections"). Movies with a lock stay as they are.
    ///
    /// # Errors
    /// Fails when no provider knows collections or the library cannot be written.
    pub async fn find(
        &self,
        providers: &[&dyn MetadataProvider],
        movies: &[ItemId],
        request: &RequestOptions,
        nfo: &NfoSettings,
    ) -> Result<FindOutcome, CollectionError> {
        let report =
            remote::find_collections(&self.db, providers, movies, request, true, true, today())
                .await?;
        let mut outcome = FindOutcome {
            report,
            ..FindOutcome::default()
        };
        if outcome.report.assigned.is_empty() {
            return Ok(outcome);
        }
        let mut touched: Vec<ItemId> = outcome.report.assigned.iter().map(|a| a.movie).collect();
        touched.extend(outcome.report.assigned.iter().filter_map(|a| a.previous));
        touched.extend(outcome.report.assigned.iter().map(|a| a.set));
        let (written, problem) = rewrite_nfo(&self.nfo, &touched, nfo);
        outcome.nfo_files_written = written;
        outcome.nfo_problem = problem;
        outcome.undo = Some(
            self.remember(Undo::Assign {
                before: outcome
                    .report
                    .assigned
                    .iter()
                    .map(|a| (a.movie, a.previous))
                    .collect(),
                created: outcome.report.created.clone(),
            }),
        );
        Ok(outcome)
    }

    // ---- undo --------------------------------------------------------------------------

    /// Puts back what the operation behind `token` changed. An operation can be undone once.
    ///
    /// # Errors
    /// Fails with [`CollectionError::NothingToUndo`] for an unknown or used token.
    pub fn undo(&self, token: u64, nfo: &NfoSettings) -> Result<UndoReport, CollectionError> {
        let record = {
            let mut store = self.undo.lock().unwrap_or_else(PoisonError::into_inner);
            let at = store
                .records
                .iter()
                .position(|(t, _)| *t == token)
                .ok_or(CollectionError::NothingToUndo)?;
            store.records.remove(at).map(|(_, r)| r)
        }
        .ok_or(CollectionError::NothingToUndo)?;

        let mut report = UndoReport::default();
        let touched: Vec<ItemId> = match record {
            Undo::Assign { before, created } => {
                let ids: Vec<ItemId> = before.iter().map(|(movie, _)| *movie).collect();
                let current: Vec<ItemId> = {
                    // Collections the movies are in now are written again too.
                    self.db.read(|c| {
                        Ok(ids
                            .iter()
                            .filter_map(|id| cv_db::repo::get_item(c, *id).ok().flatten())
                            .filter_map(|item| item.parent_id)
                            .collect())
                    })?
                };
                let previous: Vec<ItemId> = before.iter().filter_map(|(_, set)| *set).collect();
                let moved = store::write(&self.db, move |tx| {
                    let moved = store::restore_parents(tx, &before)?;
                    store::delete_empty_sets(tx, &created)?;
                    Ok(moved)
                })?;
                report.movies = moved;
                let mut touched = ids;
                touched.extend(current);
                touched.extend(previous);
                touched
            }
            Undo::Delete(deleted) => {
                let (set, back) = store::write(&self.db, move |tx| store::restore(tx, &deleted))?;
                report.restored_set = Some(set);
                report.movies = back;
                let mut touched = vec![set];
                touched.extend(
                    self.db
                        .read(|c| Ok(store::members(c, set)))??
                        .iter()
                        .filter_map(|m| m.id),
                );
                touched
            }
        };
        let (written, problem) = rewrite_nfo(&self.nfo, &touched, nfo);
        report.nfo_files_written = written;
        report.nfo_problem = problem;
        Ok(report)
    }

    // ---- shared pieces -----------------------------------------------------------------

    fn remember(&self, record: Undo) -> u64 {
        let token = self.next_token.fetch_add(1, Ordering::Relaxed);
        let mut store = self.undo.lock().unwrap_or_else(PoisonError::into_inner);
        store.records.push_back((token, record));
        while store.records.len() > UNDO_DEPTH {
            store.records.pop_front();
        }
        token
    }
}
