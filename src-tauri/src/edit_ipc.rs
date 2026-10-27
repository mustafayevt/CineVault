//! Typed commands for in-place editing on the detail pages.
//!
//! [`get_item_edit`] returns every editable field of a movie, set, show or episode with its
//! current value, the lock key that protects it from scraping, and the locked fields.
//! [`save_item_edit`] saves changed fields (all or nothing), rewrites the NFO files and returns
//! the previous values so the page can offer Undo by saving them back. [`set_field_lock`] and
//! [`set_item_lock`] flip the locks. The rules live in `cv_app::edit`.
//!
//! The service is built by the first command; it shares the library database and the NFO service
//! with the NFO actions. Work runs on a blocking thread.

use std::sync::{Arc, OnceLock};

use cv_app::changes::ChangePart;
use cv_app::edit::{
    EditError, EditService, EditSnapshot, FieldEntry, FieldValue, NumberValue, Pair, PersonValue,
    RatingValue, SaveReport, TrailerValue,
};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use crate::detail_ipc::{enum_name, parse_id};
use crate::nfo_ipc::{NfoState, nfo_settings};
use crate::settings_ipc::SettingsState;
use crate::sources_ipc::LibraryState;

/// Held in Tauri state. The service is built by the first command that needs it.
pub struct EditState {
    service: OnceLock<Arc<EditService>>,
}

impl EditState {
    pub const fn new() -> Self {
        Self {
            service: OnceLock::new(),
        }
    }

    fn service(
        &self,
        library: &LibraryState,
        nfo: &NfoState,
    ) -> Result<Arc<EditService>, EditCommandError> {
        if let Some(service) = self.service.get() {
            return Ok(Arc::clone(service));
        }
        let db = library.database().map_err(|message| EditCommandError {
            code: "storage".to_owned(),
            message,
            field: None,
        })?;
        let nfo = nfo.service(library).map_err(|error| EditCommandError {
            code: error.code,
            message: error.message,
            field: None,
        })?;
        Ok(Arc::clone(
            self.service
                .get_or_init(|| Arc::new(EditService::new(db, nfo))),
        ))
    }
}

impl Default for EditState {
    fn default() -> Self {
        Self::new()
    }
}

/// A key and a value: a provider ID, or an artwork address by type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct EditPairDto {
    pub key: String,
    pub value: String,
}

/// One credit in an editable list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct EditPersonDto {
    /// `actor`, `guest`, `director`, `writer`, `producer`, `composer`, `editor`, `camera` or
    /// `other`.
    pub kind: String,
    pub name: String,
    pub role: String,
}

/// One trailer in an editable list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct EditTrailerDto {
    pub name: String,
    pub url: String,
    pub quality: String,
    pub language: String,
    pub in_nfo: bool,
}

/// An episode number in one ordering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct EditNumberDto {
    /// `aired`, `dvd`, `absolute`, `alternate` or `display`.
    pub ordering: String,
    /// The provider's name for a custom ordering; empty for the standard ones.
    pub name: String,
    pub season: u32,
    pub episode: u32,
}

/// One source rating in an editable list; the user's own rating is the `user_rating` field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct EditRatingDto {
    /// `imdb`, `tmdb`, `metacritic` and so on.
    pub source: String,
    /// The rating on the scale of the source.
    pub value: f64,
    /// The top of the scale: 10, 5, 4 or 100.
    pub max: u32,
    pub votes: u32,
}

/// The value of one field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EditValueDto {
    Text {
        value: String,
    },
    /// `None` clears the field.
    Number {
        value: Option<f64>,
    },
    Flag {
        value: bool,
    },
    /// `YYYY-MM-DD`; `None` clears the field.
    Date {
        value: Option<String>,
    },
    List {
        values: Vec<String>,
    },
    Pairs {
        pairs: Vec<EditPairDto>,
    },
    People {
        people: Vec<EditPersonDto>,
    },
    Trailers {
        trailers: Vec<EditTrailerDto>,
    },
    Numbers {
        numbers: Vec<EditNumberDto>,
    },
    Ratings {
        ratings: Vec<EditRatingDto>,
    },
}

impl From<FieldValue> for EditValueDto {
    fn from(value: FieldValue) -> Self {
        match value {
            FieldValue::Text(value) => Self::Text { value },
            FieldValue::Number(value) => Self::Number { value },
            FieldValue::Flag(value) => Self::Flag { value },
            FieldValue::Date(value) => Self::Date { value },
            FieldValue::List(values) => Self::List { values },
            FieldValue::Pairs(pairs) => Self::Pairs {
                pairs: pairs
                    .into_iter()
                    .map(|p| EditPairDto {
                        key: p.key,
                        value: p.value,
                    })
                    .collect(),
            },
            FieldValue::People(people) => Self::People {
                people: people
                    .into_iter()
                    .map(|p| EditPersonDto {
                        kind: p.kind,
                        name: p.name,
                        role: p.role,
                    })
                    .collect(),
            },
            FieldValue::Trailers(trailers) => Self::Trailers {
                trailers: trailers
                    .into_iter()
                    .map(|t| EditTrailerDto {
                        name: t.name,
                        url: t.url,
                        quality: t.quality,
                        language: t.language,
                        in_nfo: t.in_nfo,
                    })
                    .collect(),
            },
            FieldValue::Numbers(numbers) => Self::Numbers {
                numbers: numbers
                    .into_iter()
                    .map(|n| EditNumberDto {
                        ordering: n.ordering,
                        name: n.name,
                        season: n.season,
                        episode: n.episode,
                    })
                    .collect(),
            },
            FieldValue::Ratings(ratings) => Self::Ratings {
                ratings: ratings
                    .into_iter()
                    .map(|r| EditRatingDto {
                        source: r.source,
                        value: r.value,
                        max: r.max,
                        votes: r.votes,
                    })
                    .collect(),
            },
        }
    }
}

impl From<EditValueDto> for FieldValue {
    fn from(value: EditValueDto) -> Self {
        match value {
            EditValueDto::Text { value } => Self::Text(value),
            EditValueDto::Number { value } => Self::Number(value),
            EditValueDto::Flag { value } => Self::Flag(value),
            EditValueDto::Date { value } => Self::Date(value),
            EditValueDto::List { values } => Self::List(values),
            EditValueDto::Pairs { pairs } => Self::Pairs(
                pairs
                    .into_iter()
                    .map(|p| Pair {
                        key: p.key,
                        value: p.value,
                    })
                    .collect(),
            ),
            EditValueDto::People { people } => Self::People(
                people
                    .into_iter()
                    .map(|p| PersonValue {
                        kind: p.kind,
                        name: p.name,
                        role: p.role,
                    })
                    .collect(),
            ),
            EditValueDto::Trailers { trailers } => Self::Trailers(
                trailers
                    .into_iter()
                    .map(|t| TrailerValue {
                        name: t.name,
                        url: t.url,
                        quality: t.quality,
                        language: t.language,
                        in_nfo: t.in_nfo,
                    })
                    .collect(),
            ),
            EditValueDto::Numbers { numbers } => Self::Numbers(
                numbers
                    .into_iter()
                    .map(|n| NumberValue {
                        ordering: n.ordering,
                        name: n.name,
                        season: n.season,
                        episode: n.episode,
                    })
                    .collect(),
            ),
            EditValueDto::Ratings { ratings } => Self::Ratings(
                ratings
                    .into_iter()
                    .map(|r| RatingValue {
                        source: r.source,
                        value: r.value,
                        max: r.max,
                        votes: r.votes,
                    })
                    .collect(),
            ),
        }
    }
}

/// One editable field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct EditFieldDto {
    pub key: String,
    pub value: EditValueDto,
    /// The lock key that protects the field from scraping; `None` for a field no scrape writes.
    pub lock: Option<String>,
}

impl From<FieldEntry> for EditFieldDto {
    fn from(entry: FieldEntry) -> Self {
        Self {
            key: entry.key,
            value: entry.value.into(),
            lock: entry.lock,
        }
    }
}

impl From<EditFieldDto> for FieldEntry {
    fn from(dto: EditFieldDto) -> Self {
        Self {
            key: dto.key,
            value: dto.value.into(),
            lock: dto.lock,
        }
    }
}

/// An item as the editor sees it.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct ItemEditDto {
    /// `movie`, `set`, `show`, `season` or `episode`.
    pub kind: String,
    pub fields: Vec<EditFieldDto>,
    /// Lock keys of the locked fields.
    pub locked_fields: Vec<String>,
    /// The whole item is locked.
    pub item_locked: bool,
    /// The collections a movie can join, by name; empty for other kinds.
    pub collections: Vec<String>,
}

impl From<EditSnapshot> for ItemEditDto {
    fn from(snapshot: EditSnapshot) -> Self {
        Self {
            kind: enum_name(&snapshot.kind),
            fields: snapshot.fields.into_iter().map(Into::into).collect(),
            locked_fields: snapshot.locked_fields,
            item_locked: snapshot.item_locked,
            collections: snapshot.collections,
        }
    }
}

/// What a save did.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct EditSavedDto {
    /// Keys of the fields whose value changed.
    pub changed: Vec<String>,
    /// The values those fields had before; save them back to undo.
    pub previous: Vec<EditFieldDto>,
    /// NFO files written by the save.
    pub nfo_files_written: u32,
    /// Why the NFO files could not be written, when the library was saved but the files were not.
    pub nfo_problem: Option<String>,
}

impl From<SaveReport> for EditSavedDto {
    fn from(report: SaveReport) -> Self {
        Self {
            changed: report.changed,
            previous: report.previous.into_iter().map(Into::into).collect(),
            nfo_files_written: report.nfo_files_written,
            nfo_problem: report.nfo_problem,
        }
    }
}

/// Why an edit command did not run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct EditCommandError {
    /// `storage`, `bad_id`, `not_found`, `invalid`, `not_lockable` or `nothing_to_undo`.
    pub code: String,
    /// Plain sentence for the user.
    pub message: String,
    /// The field that was refused, for `invalid`.
    pub field: Option<String>,
}

impl From<EditError> for EditCommandError {
    fn from(error: EditError) -> Self {
        match error {
            EditError::NotFound => Self {
                code: "not_found".to_owned(),
                message: error.to_string(),
                field: None,
            },
            EditError::Invalid(refusal) => Self {
                code: "invalid".to_owned(),
                message: refusal.message,
                field: Some(refusal.key),
            },
            EditError::NotLockable(_) => Self {
                code: "not_lockable".to_owned(),
                message: error.to_string(),
                field: None,
            },
            EditError::Db(_) => Self {
                code: "storage".to_owned(),
                message: error.to_string(),
                field: None,
            },
        }
    }
}

pub(crate) async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, EditCommandError> + Send + 'static,
) -> Result<T, EditCommandError> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| EditCommandError {
            code: "storage".to_owned(),
            message: format!("the edit stopped unexpectedly: {error}"),
            field: None,
        })?
}

pub(crate) fn id_of(text: &str) -> Result<cv_domain::ItemId, EditCommandError> {
    parse_id(text).map_err(|error| EditCommandError {
        code: error.code,
        message: error.message,
        field: None,
    })
}

/// Every editable field of an item with its lock key, the locked fields and the item lock.
#[tauri::command]
#[specta::specta]
pub async fn get_item_edit(
    library: State<'_, LibraryState>,
    nfo: State<'_, NfoState>,
    edit: State<'_, EditState>,
    id: String,
) -> Result<ItemEditDto, EditCommandError> {
    let id = id_of(&id)?;
    let service = edit.service(&library, &nfo)?;
    blocking(move || Ok(service.load(id)?.into())).await
}

/// Saves the given fields of an item and rewrites its NFO files. Nothing is saved when any value
/// is refused. The reply holds the previous values for Undo.
#[tauri::command]
#[specta::specta]
pub async fn save_item_edit(
    library: State<'_, LibraryState>,
    nfo: State<'_, NfoState>,
    edit: State<'_, EditState>,
    settings: State<'_, SettingsState>,
    id: String,
    changes: Vec<EditFieldDto>,
) -> Result<EditSavedDto, EditCommandError> {
    let id = id_of(&id)?;
    let service = edit.service(&library, &nfo)?;
    let nfo_settings = nfo_settings(&settings);
    let changes: Vec<FieldEntry> = changes.into_iter().map(Into::into).collect();
    let (hub, names) = (
        library.changes().cloned(),
        library.organize_state().cloned(),
    );
    blocking(move || {
        let saved = service.save(id, changes, &nfo_settings)?;
        if !saved.changed.is_empty() {
            // The fields changed, and a new title or year can change the names it should have.
            if let Some(hub) = &hub {
                hub.publish(&[id], &[ChangePart::Details, ChangePart::Stage]);
            }
            if let Some(names) = &names {
                names.note_changed(&[id]);
            }
        }
        Ok(saved.into())
    })
    .await
}

/// Locks or unlocks one field against scraping and NFO re-reading. `field` is a lock key.
#[tauri::command]
#[specta::specta]
pub async fn set_field_lock(
    library: State<'_, LibraryState>,
    nfo: State<'_, NfoState>,
    edit: State<'_, EditState>,
    id: String,
    field: String,
    locked: bool,
) -> Result<(), EditCommandError> {
    let id = id_of(&id)?;
    let service = edit.service(&library, &nfo)?;
    let hub = library.changes().cloned();
    blocking(move || {
        service.set_field_lock(id, &field, locked)?;
        // The NFO lists the locked fields: it is written again (`NfoFollower`).
        if let Some(hub) = &hub {
            hub.publish(&[id], &[ChangePart::Details]);
        }
        Ok(())
    })
    .await
}

/// Locks or unlocks the whole item.
#[tauri::command]
#[specta::specta]
pub async fn set_item_lock(
    library: State<'_, LibraryState>,
    nfo: State<'_, NfoState>,
    edit: State<'_, EditState>,
    id: String,
    locked: bool,
) -> Result<(), EditCommandError> {
    let id = id_of(&id)?;
    let service = edit.service(&library, &nfo)?;
    blocking(move || Ok(service.set_item_locked(id, locked)?)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_survive_the_round_trip_through_the_wire_type() {
        let values = vec![
            FieldValue::Text("a".into()),
            FieldValue::Number(Some(3.0)),
            FieldValue::Number(None),
            FieldValue::Flag(true),
            FieldValue::Date(Some("2024-03-01".into())),
            FieldValue::List(vec!["x".into()]),
            FieldValue::Pairs(vec![Pair {
                key: "tmdb".into(),
                value: "1".into(),
            }]),
            FieldValue::People(vec![PersonValue {
                kind: "actor".into(),
                name: "N".into(),
                role: "R".into(),
            }]),
            FieldValue::Trailers(vec![TrailerValue {
                name: "T".into(),
                url: "https://example.test".into(),
                quality: "1080p".into(),
                language: "en".into(),
                in_nfo: true,
            }]),
            FieldValue::Numbers(vec![NumberValue {
                ordering: "aired".into(),
                name: String::new(),
                season: 1,
                episode: 2,
            }]),
            FieldValue::Ratings(vec![RatingValue {
                source: "imdb".into(),
                value: 7.8,
                max: 10,
                votes: 1200,
            }]),
        ];
        for value in values {
            let wire: EditValueDto = value.clone().into();
            let json = serde_json::to_string(&wire).unwrap();
            let back: EditValueDto = serde_json::from_str(&json).unwrap();
            assert_eq!(FieldValue::from(back), value, "{json}");
        }
    }

    #[test]
    fn a_refused_value_names_its_field() {
        let error = EditCommandError::from(EditError::Invalid(cv_app::edit::FieldError {
            key: "release_date".into(),
            message: "enter the date as year-month-day".into(),
        }));
        assert_eq!(error.code, "invalid");
        assert_eq!(error.field.as_deref(), Some("release_date"));
    }
}
