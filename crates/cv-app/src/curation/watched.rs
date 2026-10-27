//! Watched state and play counts.

use std::sync::Arc;

use cv_db::Database;
use cv_db::repo::{WatchState, get_watch_state, playable_in, set_watch_state};
use cv_domain::ItemId;

use super::{not_found, now_millis, rewrite_nfo};
use crate::edit::EditError;
use crate::nfo::{NfoService, NfoSettings};

/// What a watched change did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WatchReport {
    /// The movies and episodes the selection covers (a show is its episodes).
    pub covered: u32,
    /// The state each changed title had before, for Undo. Titles already in the wanted state are
    /// not listed.
    pub previous: Vec<WatchState>,
    pub nfo_files_written: u32,
    pub nfo_problem: Option<String>,
}

impl WatchReport {
    /// How many titles changed.
    #[must_use]
    pub fn changed(&self) -> u32 {
        u32::try_from(self.previous.len()).unwrap_or(u32::MAX)
    }
}

/// Marks titles watched or not watched.
pub struct WatchService {
    db: Arc<Database>,
    nfo: Arc<NfoService>,
}

impl WatchService {
    #[must_use]
    pub fn new(db: Arc<Database>, nfo: Arc<NfoService>) -> Self {
        Self { db, nfo }
    }

    /// Marks the titles watched (play count at least 1, last watched now) or not watched (play
    /// count 0). Seasons, shows and collections stand for the movies and episodes below them;
    /// unknown ids are ignored.
    ///
    /// # Errors
    /// Fails when the library cannot be read or written.
    pub fn set_watched(
        &self,
        ids: &[ItemId],
        watched: bool,
        nfo_settings: &NfoSettings,
    ) -> Result<WatchReport, EditError> {
        let ids = ids.to_vec();
        let now_ms = now_millis();
        let (covered, previous) = self
            .db
            .write(move |tx| {
                let playable = playable_in(tx, &ids)?;
                let mut previous = Vec::new();
                for id in &playable {
                    let Some(state) = get_watch_state(tx, *id)? else {
                        continue;
                    };
                    let next = if watched {
                        state.marked_watched(now_ms)
                    } else {
                        state.marked_unwatched()
                    };
                    if let Some(next) = next {
                        set_watch_state(tx, &next)?;
                        previous.push(state);
                    }
                }
                Ok((playable.len(), previous))
            })
            .map_err(not_found)?;
        Ok(self.finish(covered, previous, nfo_settings))
    }

    /// Flips the watched state of the titles: when every movie and episode they cover is
    /// watched they are marked not watched, otherwise all of them are marked watched (so a
    /// season with one unwatched episode becomes fully watched). Returns the state the titles
    /// were set to.
    ///
    /// # Errors
    /// Fails when the library cannot be read or written.
    pub fn toggle(
        &self,
        ids: &[ItemId],
        nfo_settings: &NfoSettings,
    ) -> Result<(bool, WatchReport), EditError> {
        let wanted = ids.to_vec();
        let all_watched = self
            .db
            .read(move |conn| {
                let playable = playable_in(conn, &wanted)?;
                let mut all = !playable.is_empty();
                for id in playable {
                    all &= get_watch_state(conn, id)?.is_some_and(|state| state.watched);
                }
                Ok(all)
            })
            .map_err(not_found)?;
        let target = !all_watched;
        Ok((target, self.set_watched(ids, target, nfo_settings)?))
    }

    /// Puts back the states a change reported, play counts included. Titles that are gone are
    /// skipped.
    ///
    /// # Errors
    /// Fails when the library cannot be written.
    pub fn restore(
        &self,
        previous: &[WatchState],
        nfo_settings: &NfoSettings,
    ) -> Result<WatchReport, EditError> {
        let states = previous.to_vec();
        let (covered, restored) = self
            .db
            .write(move |tx| {
                let mut before = Vec::new();
                for state in &states {
                    if let Some(current) = get_watch_state(tx, state.id)? {
                        set_watch_state(tx, state)?;
                        before.push(current);
                    }
                }
                Ok((states.len(), before))
            })
            .map_err(not_found)?;
        Ok(self.finish(covered, restored, nfo_settings))
    }

    fn finish(
        &self,
        covered: usize,
        previous: Vec<WatchState>,
        nfo_settings: &NfoSettings,
    ) -> WatchReport {
        let ids: Vec<ItemId> = previous.iter().map(|s| s.id).collect();
        let (nfo_files_written, nfo_problem) = rewrite_nfo(&self.nfo, &ids, nfo_settings);
        WatchReport {
            covered: u32::try_from(covered).unwrap_or(u32::MAX),
            previous,
            nfo_files_written,
            nfo_problem,
        }
    }
}
