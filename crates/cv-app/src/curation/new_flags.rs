//! The NEW flag.

use std::collections::BTreeSet;
use std::sync::Arc;

use cv_db::repo::flags::count_titles_of;
use cv_db::repo::{count_new, get_flags, new_ids, set_new};
use cv_db::{Database, DbError};
use cv_domain::{ItemId, ItemKind};

use crate::edit::EditError;

/// Which titles a flag change touched.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NewFlagReport {
    /// The items whose flag changed, for Undo (episodes included).
    pub changed: Vec<ItemId>,
    /// How many titles that is: movies and shows, an episode counted as its show.
    pub titles: u64,
}

/// Clears and restores the NEW flag.
pub struct NewFlagService {
    db: Arc<Database>,
}

impl NewFlagService {
    #[must_use]
    pub const fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// Clears the flag of the given titles. A show also loses the flag of its episodes: a show
    /// carries the NEW marker while one of them is NEW. Titles that are not flagged are not
    /// listed in the reply; unknown ids are ignored.
    ///
    /// # Errors
    /// Fails when the library cannot be written.
    pub fn clear(&self, ids: &[ItemId]) -> Result<NewFlagReport, EditError> {
        let mut wanted = ids.to_vec();
        let shows = ids.to_vec();
        let episodes = self.db.read(move |conn| new_episodes_of(conn, &shows))?;
        wanted.extend(episodes);
        self.change(&wanted, true)
    }

    /// Clears the flag of a title that was opened: only its own flag. An opened show keeps the
    /// flags of its episodes, which were not opened, so the show still counts new episodes.
    ///
    /// # Errors
    /// Fails when the library cannot be written.
    pub fn opened(&self, id: ItemId) -> Result<NewFlagReport, EditError> {
        self.change(&[id], true)
    }

    /// Flags the given titles again (Undo of a clear).
    ///
    /// # Errors
    /// Fails when the library cannot be written.
    pub fn restore(&self, ids: &[ItemId]) -> Result<NewFlagReport, EditError> {
        self.change(ids, false)
    }

    /// Clears the flag of every flagged title, or of one kind only ("Clear NEW" for all).
    ///
    /// # Errors
    /// Fails when the library cannot be written.
    pub fn clear_all(&self, kind: Option<ItemKind>) -> Result<NewFlagReport, EditError> {
        let changed = self.db.write(move |tx| {
            let mut flagged = new_ids(tx, kind)?;
            // Clearing the shows clears their episodes too, or they would keep the marker.
            if kind == Some(ItemKind::Show) {
                flagged.extend(new_ids(tx, Some(ItemKind::Episode))?);
            }
            set_new(tx, &flagged, false)?;
            Ok(flagged)
        })?;
        self.report(changed)
    }

    /// How many titles are flagged, optionally of one kind.
    ///
    /// # Errors
    /// Fails when the library cannot be read.
    pub fn count(&self, kind: Option<ItemKind>) -> Result<u64, EditError> {
        Ok(self.db.read(|conn| count_new(conn, kind))?)
    }

    /// `clearing` removes the flag from the flagged titles among `ids`; otherwise it sets the
    /// flag on the unflagged ones.
    fn change(&self, ids: &[ItemId], clearing: bool) -> Result<NewFlagReport, EditError> {
        let mut seen = BTreeSet::new();
        let ids: Vec<ItemId> = ids.iter().copied().filter(|id| seen.insert(id.0)).collect();
        let changed = self.db.write(move |tx| {
            let mut touched = Vec::new();
            for id in ids {
                match get_flags(tx, id) {
                    Ok(flags) if flags.is_new == clearing => touched.push(id),
                    Ok(_) | Err(DbError::ItemNotFound(_)) => {}
                    Err(other) => return Err(other),
                }
            }
            set_new(tx, &touched, !clearing)?;
            Ok(touched)
        })?;
        self.report(changed)
    }

    fn report(&self, changed: Vec<ItemId>) -> Result<NewFlagReport, EditError> {
        let ids = changed.clone();
        let titles = self.db.read(move |conn| count_titles_of(conn, &ids))?;
        Ok(NewFlagReport { changed, titles })
    }
}

/// The NEW episodes of the shows among `ids`, in id order.
fn new_episodes_of(conn: &rusqlite::Connection, ids: &[ItemId]) -> Result<Vec<ItemId>, DbError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let list = serde_json::to_string(&ids.iter().map(|id| id.0).collect::<Vec<_>>())?;
    let mut stmt = conn.prepare_cached(
        "SELECT e.id FROM item e JOIN item s ON s.id = e.parent_id \
         WHERE s.kind = 'show' AND s.id IN (SELECT value FROM json_each(?1)) \
           AND e.kind = 'episode' AND e.is_new = 1 ORDER BY e.id",
    )?;
    let rows = stmt
        .query_map([list], |r| r.get::<_, i64>(0).map(ItemId))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}
