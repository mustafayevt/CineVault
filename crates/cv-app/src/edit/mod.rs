//! In-place editing of an item's fields, and the locks that protect them from scraping.
//!
//! Spec: `metadata-editing` ("In-place editing of all fields", "Field-level locks"). The detail
//! pages of movies, sets, shows and episodes edit through one service:
//!
//! * [`EditService::load`] returns every editable field of the item with its lock key, the set
//!   of locked fields and the item lock;
//! * [`EditService::save`] checks all changed values first, writes them in one transaction and
//!   then rewrites the item's NFO files with the NFO settings. A refused value changes nothing.
//!   The reply carries the previous values so the page can offer Undo (a save of those values);
//! * [`EditService::set_field_lock`] and [`EditService::set_item_locked`] flip the locks.
//!
//! A movie's collection is the field `collection`: the name of the collection (movie set) it
//! belongs to, empty for none. It can only name a collection that exists; the snapshot lists them.
//!
//! A lock protects a field from machines (scrape, Read NFO), not from its owner:
//! saving never checks a lock. Fields that no scrape writes (sort title, edition, media source,
//! note, IDs and so on) have no lock key; they are protected by that fact.
//!
//! The NFO is a copy of the library values, so a failed NFO write does not undo the save: it is
//! reported in [`SaveReport::nfo_problem`] and a later Rewrite NFO fixes it.

mod fields;
#[cfg(test)]
mod tests;

use std::collections::BTreeSet;
use std::sync::Arc;

use cv_db::repo::{
    field_locks, find_set_by_title, get_item, set_field_lock, set_item_locked, set_titles,
    set_user_rating, update_item,
};
use cv_db::{Database, DbError};
use cv_domain::{FieldSet, Item, ItemId, ItemKind, MetaField};
use rusqlite::Connection;

pub use fields::{
    FieldEntry, FieldError, FieldValue, NumberValue, Pair, PersonValue, RatingValue, TrailerValue,
    lock_key, read_fields,
};

use crate::nfo::{NfoScope, NfoService, NfoSettings};

/// Why an edit could not be made.
#[derive(Debug, thiserror::Error)]
pub enum EditError {
    #[error("this item is no longer in the library")]
    NotFound,
    #[error("the library could not be read or written: {0}")]
    Db(#[from] DbError),
    /// A value was refused. Nothing was saved.
    #[error("{0}")]
    Invalid(#[from] FieldError),
    #[error("{0:?} is not a field that can be locked")]
    NotLockable(String),
}

/// An item as the editor sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct EditSnapshot {
    pub kind: ItemKind,
    pub fields: Vec<FieldEntry>,
    /// Lock keys of the fields that are locked.
    pub locked_fields: Vec<String>,
    /// The whole item is locked.
    pub item_locked: bool,
    /// The names of the collections a movie can join, sorted; empty for other kinds.
    pub collections: Vec<String>,
}

/// The key of a movie's collection field.
pub const COLLECTION_KEY: &str = "collection";

/// The name of the collection the movie belongs to; empty for none.
fn collection_of(conn: &Connection, item: &Item) -> Result<String, DbError> {
    let Some(parent) = item.parent_id else {
        return Ok(String::new());
    };
    Ok(get_item(conn, parent)?
        .filter(|set| set.kind() == ItemKind::Set)
        .map(|set| set.core.title)
        .unwrap_or_default())
}

/// The collection field of a movie, with its lock key.
fn collection_entry(conn: &Connection, item: &Item) -> Result<FieldEntry, DbError> {
    Ok(FieldEntry {
        key: COLLECTION_KEY.to_owned(),
        value: FieldValue::Text(collection_of(conn, item)?),
        lock: Some(MetaField::Collection.key().to_owned()),
    })
}

/// Puts the movie into the named collection, or takes it out of any for an empty name. Returns
/// the field as it was before when it changed.
fn apply_collection(
    conn: &Connection,
    item: &mut Item,
    value: &FieldValue,
) -> Result<Result<Option<FieldEntry>, FieldError>, DbError> {
    if item.kind() != ItemKind::Movie {
        return Ok(Err(FieldError::new(
            COLLECTION_KEY,
            "this field cannot be edited here",
        )));
    }
    let FieldValue::Text(name) = value else {
        return Ok(Err(FieldError::new(COLLECTION_KEY, "expected text")));
    };
    let name = name.trim();
    let target = if name.is_empty() {
        None
    } else {
        match find_set_by_title(conn, name)? {
            Some(id) => Some(id),
            None => {
                return Ok(Err(FieldError::new(
                    COLLECTION_KEY,
                    format!("there is no collection called \"{name}\""),
                )));
            }
        }
    };
    if item.parent_id == target {
        return Ok(Ok(None));
    }
    let before = collection_entry(conn, item)?;
    item.parent_id = target;
    Ok(Ok(Some(before)))
}

/// What a save did.
#[derive(Debug, Clone, PartialEq)]
pub struct SaveReport {
    /// Keys of the fields whose value changed.
    pub changed: Vec<String>,
    /// The values those fields had before, for Undo.
    pub previous: Vec<FieldEntry>,
    /// NFO files written (or updated) by the save.
    pub nfo_files_written: u32,
    /// Why the NFO could not be written, when the library was saved but the files were not.
    pub nfo_problem: Option<String>,
}

/// Edits items and their locks.
pub struct EditService {
    db: Arc<Database>,
    nfo: Arc<NfoService>,
}

impl EditService {
    pub fn new(db: Arc<Database>, nfo: Arc<NfoService>) -> Self {
        Self { db, nfo }
    }

    /// The editable fields, locks and item lock of an item.
    ///
    /// # Errors
    /// Fails when the item is gone or the library cannot be read.
    pub fn load(&self, id: ItemId) -> Result<EditSnapshot, EditError> {
        self.db
            .read(|conn| {
                let item = get_item(conn, id)?.ok_or(DbError::ItemNotFound(id.0))?;
                let kind = item.kind();
                let locked: FieldSet = field_locks(conn, id)?;
                let mut fields = read_fields(&item);
                let mut collections = Vec::new();
                if kind == ItemKind::Movie {
                    fields.push(collection_entry(conn, &item)?);
                    collections = set_titles(conn)?;
                }
                Ok(EditSnapshot {
                    kind,
                    fields,
                    locked_fields: all_lock_keys(kind)
                        .into_iter()
                        .filter(|key| MetaField::from_key(key).is_some_and(|f| locked.contains(f)))
                        .collect(),
                    item_locked: item.core.locked,
                    collections,
                })
            })
            .map_err(not_found)
    }

    /// Saves the changed fields, then rewrites the item's NFO files.
    ///
    /// # Errors
    /// Fails, saving nothing, when a value is refused ([`EditError::Invalid`]), the item is gone
    /// or the library cannot be written.
    pub fn save(
        &self,
        id: ItemId,
        changes: Vec<FieldEntry>,
        nfo_settings: &NfoSettings,
    ) -> Result<SaveReport, EditError> {
        let outcome = self
            .db
            .write(move |tx| {
                let mut item = get_item(tx, id)?.ok_or(DbError::ItemNotFound(id.0))?;
                let mut previous = Vec::new();
                let mut changed = Vec::new();
                for change in &changes {
                    if change.key == COLLECTION_KEY {
                        match apply_collection(tx, &mut item, &change.value)? {
                            Err(error) => return Ok(Err(error)),
                            Ok(None) => {}
                            Ok(Some(before)) => {
                                previous.push(before);
                                changed.push(change.key.clone());
                            }
                        }
                        continue;
                    }
                    let Some(before) = fields::read_field(&item, &change.key) else {
                        // A season field that does not exist yet reads as empty text.
                        if let Err(error) =
                            fields::apply_change(&mut item, &change.key, &change.value)
                        {
                            return Ok(Err(error));
                        }
                        changed.push(change.key.clone());
                        previous.push(FieldEntry {
                            key: change.key.clone(),
                            value: FieldValue::Text(String::new()),
                            lock: None,
                        });
                        continue;
                    };
                    if before == change.value {
                        continue;
                    }
                    if let Err(error) = fields::apply_change(&mut item, &change.key, &change.value)
                    {
                        return Ok(Err(error));
                    }
                    // A value can normalise to what is stored already (trimmed text, a repeated tag).
                    if fields::read_field(&item, &change.key).as_ref() == Some(&before) {
                        continue;
                    }
                    previous.push(FieldEntry {
                        key: change.key.clone(),
                        value: before,
                        lock: lock_key(item.kind(), &change.key).map(str::to_owned),
                    });
                    changed.push(change.key.clone());
                }
                if changed.is_empty() {
                    return Ok(Ok((changed, previous)));
                }
                update_item(tx, &item)?;
                if changed.iter().any(|key| key == "user_rating") {
                    set_user_rating(tx, id, item.core.ratings.user_rating())?;
                }
                Ok(Ok((changed, previous)))
            })
            .map_err(not_found)?;
        let (changed, previous) = outcome?;
        if changed.is_empty() {
            return Ok(SaveReport {
                changed,
                previous,
                nfo_files_written: 0,
                nfo_problem: None,
            });
        }
        let (nfo_files_written, nfo_problem) = self.rewrite_nfo(id, nfo_settings);
        Ok(SaveReport {
            changed,
            previous,
            nfo_files_written,
            nfo_problem,
        })
    }

    fn rewrite_nfo(&self, id: ItemId, settings: &NfoSettings) -> (u32, Option<String>) {
        match self.nfo.rewrite(&NfoScope::Items(vec![id]), settings) {
            Ok(report) => {
                let problem = report.units.iter().find_map(|unit| match &unit.status {
                    crate::nfo::UnitStatus::Failed { message } => Some(message.clone()),
                    _ => None,
                });
                (report.totals.files_written, problem)
            }
            Err(error) => (0, Some(error.to_string())),
        }
    }

    /// Locks or unlocks one field against scraping and NFO re-reading. `key` is a lock key from
    /// [`FieldEntry::lock`].
    ///
    /// # Errors
    /// Fails for a key that is no lock key of the item's kind, when the item is gone or the
    /// library cannot be written.
    pub fn set_field_lock(&self, id: ItemId, key: &str, locked: bool) -> Result<(), EditError> {
        let field =
            MetaField::from_key(key).ok_or_else(|| EditError::NotLockable(key.to_owned()))?;
        let key = key.to_owned();
        self.db
            .write(move |tx| {
                let item = get_item(tx, id)?.ok_or(DbError::ItemNotFound(id.0))?;
                if !field.applies_to(item.kind()) {
                    return Ok(Err(EditError::NotLockable(key)));
                }
                set_field_lock(tx, id, field, locked)?;
                Ok(Ok(()))
            })
            .map_err(not_found)?
    }

    /// Locks or unlocks the whole item.
    ///
    /// # Errors
    /// Fails when the item is gone or the library cannot be written.
    pub fn set_item_locked(&self, id: ItemId, locked: bool) -> Result<(), EditError> {
        self.db
            .write(move |tx| {
                get_item(tx, id)?.ok_or(DbError::ItemNotFound(id.0))?;
                set_item_locked(tx, &[id], locked)?;
                Ok(())
            })
            .map_err(not_found)
    }
}

fn not_found(error: DbError) -> EditError {
    match error {
        DbError::ItemNotFound(_) => EditError::NotFound,
        other => EditError::Db(other),
    }
}

/// The lock keys an item of `kind` can have, without repeats.
fn all_lock_keys(kind: ItemKind) -> Vec<String> {
    cv_domain::all_fields()
        .into_iter()
        .filter(|f| f.applies_to(kind))
        .map(|f| f.key().to_owned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}
