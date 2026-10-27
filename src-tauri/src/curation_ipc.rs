//! Typed commands for watched state and the NEW flag.
//!
//! The rules live in `cv_app::curation`. [`set_watched`] marks movies, episodes, seasons or shows
//! watched or not watched and returns the previous state of every title that changed, which
//! [`restore_watched`] puts back (so an Undo keeps play counts). The older per-page commands
//! (`set_movie_watched`, `set_episodes_watched`) go through the same service, so every way of
//! marking something watched rewrites the NFO files.
//!
//! The services are built by the first command that needs them and share the library database and
//! the NFO service with the other edit commands. Work runs on a blocking thread.

use std::sync::{Arc, OnceLock};

use cv_app::curation::{NewFlagReport, NewFlagService, WatchReport, WatchService};
use cv_db::repo::WatchState;
use cv_domain::{ItemId, ItemKind};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use crate::edit_ipc::{EditCommandError, blocking, id_of};
use crate::library_ipc::LibraryCommandError;
use crate::nfo_ipc::{NfoState, nfo_settings};
use crate::settings_ipc::SettingsState;
use crate::sources_ipc::LibraryState;

/// Held in Tauri state. The services are built by the first command that needs them.
#[derive(Default)]
pub struct CurationState {
    watch: OnceLock<Arc<WatchService>>,
    flags: OnceLock<Arc<NewFlagService>>,
}

impl CurationState {
    fn storage(message: String) -> EditCommandError {
        EditCommandError {
            code: "storage".to_owned(),
            message,
            field: None,
        }
    }

    pub(crate) fn watch(
        &self,
        library: &LibraryState,
        nfo: &NfoState,
    ) -> Result<Arc<WatchService>, EditCommandError> {
        if let Some(service) = self.watch.get() {
            return Ok(Arc::clone(service));
        }
        let db = library.database().map_err(Self::storage)?;
        let nfo = nfo.service(library).map_err(|error| EditCommandError {
            code: error.code,
            message: error.message,
            field: None,
        })?;
        Ok(Arc::clone(
            self.watch
                .get_or_init(|| Arc::new(WatchService::new(db, nfo))),
        ))
    }

    fn flags(&self, library: &LibraryState) -> Result<Arc<NewFlagService>, EditCommandError> {
        if let Some(service) = self.flags.get() {
            return Ok(Arc::clone(service));
        }
        let db = library.database().map_err(Self::storage)?;
        Ok(Arc::clone(
            self.flags.get_or_init(|| Arc::new(NewFlagService::new(db))),
        ))
    }
}

fn ids_of(ids: &[String]) -> Result<Vec<ItemId>, EditCommandError> {
    ids.iter().map(|id| id_of(id)).collect()
}

fn library_error(error: EditCommandError) -> LibraryCommandError {
    LibraryCommandError {
        code: error.code,
        message: error.message,
    }
}

// ---- watched ---------------------------------------------------------------------------------

/// The watched state of one movie or episode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct WatchStateDto {
    pub id: String,
    pub watched: bool,
    pub play_count: u32,
    /// Unix milliseconds of the last time it was watched.
    pub last_watched_ms: Option<f64>,
}

impl From<WatchState> for WatchStateDto {
    fn from(state: WatchState) -> Self {
        Self {
            id: state.id.0.to_string(),
            watched: state.watched,
            play_count: state.play_count,
            #[allow(clippy::cast_precision_loss)]
            last_watched_ms: state.last_watched_ms.map(|ms| ms as f64),
        }
    }
}

impl WatchStateDto {
    fn into_state(self) -> Result<WatchState, EditCommandError> {
        Ok(WatchState {
            id: id_of(&self.id)?,
            watched: self.watched,
            play_count: self.play_count,
            #[allow(clippy::cast_possible_truncation)]
            last_watched_ms: self.last_watched_ms.map(|ms| ms as i64),
        })
    }
}

/// What marking titles watched did.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct WatchReportDto {
    /// Movies and episodes the selection covers.
    pub covered: u32,
    /// How many of them changed.
    pub changed: u32,
    /// The state each changed title had before. Pass it to `restore_watched` to undo.
    pub previous: Vec<WatchStateDto>,
    pub nfo_files_written: u32,
    pub nfo_problem: Option<String>,
}

impl From<WatchReport> for WatchReportDto {
    fn from(report: WatchReport) -> Self {
        Self {
            covered: report.covered,
            changed: report.changed(),
            previous: report.previous.into_iter().map(Into::into).collect(),
            nfo_files_written: report.nfo_files_written,
            nfo_problem: report.nfo_problem,
        }
    }
}

/// Marks movies, episodes, seasons or shows (a season or show is its episodes) watched or not
/// watched. Marking watched sets the play count to at least 1 and the last-watched date to now;
/// marking not watched resets the play count to 0.
#[tauri::command]
#[specta::specta]
pub async fn set_watched(
    library: State<'_, LibraryState>,
    nfo: State<'_, NfoState>,
    curation: State<'_, CurationState>,
    settings: State<'_, SettingsState>,
    ids: Vec<String>,
    watched: bool,
) -> Result<WatchReportDto, EditCommandError> {
    let ids = ids_of(&ids)?;
    let service = curation.watch(&library, &nfo)?;
    let nfo_settings = nfo_settings(&settings);
    blocking(move || Ok(service.set_watched(&ids, watched, &nfo_settings)?.into())).await
}

/// What flipping the watched state did.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct ToggleWatchedDto {
    /// The state the titles were set to.
    pub watched: bool,
    pub report: WatchReportDto,
}

/// Flips the watched state of movies, episodes, seasons or shows: when everything they cover is
/// watched it is marked not watched, otherwise all of it is marked watched.
#[tauri::command]
#[specta::specta]
pub async fn toggle_watched(
    library: State<'_, LibraryState>,
    nfo: State<'_, NfoState>,
    curation: State<'_, CurationState>,
    settings: State<'_, SettingsState>,
    ids: Vec<String>,
) -> Result<ToggleWatchedDto, EditCommandError> {
    let ids = ids_of(&ids)?;
    let service = curation.watch(&library, &nfo)?;
    let nfo_settings = nfo_settings(&settings);
    blocking(move || {
        let (watched, report) = service.toggle(&ids, &nfo_settings)?;
        Ok(ToggleWatchedDto {
            watched,
            report: report.into(),
        })
    })
    .await
}

/// Puts back the states a `set_watched` reported, play counts included.
#[tauri::command]
#[specta::specta]
pub async fn restore_watched(
    library: State<'_, LibraryState>,
    nfo: State<'_, NfoState>,
    curation: State<'_, CurationState>,
    settings: State<'_, SettingsState>,
    previous: Vec<WatchStateDto>,
) -> Result<WatchReportDto, EditCommandError> {
    let states = previous
        .into_iter()
        .map(WatchStateDto::into_state)
        .collect::<Result<Vec<_>, _>>()?;
    let service = curation.watch(&library, &nfo)?;
    let nfo_settings = nfo_settings(&settings);
    blocking(move || Ok(service.restore(&states, &nfo_settings)?.into())).await
}

/// Marks titles watched for a page that shows one Undo toast. The report carries the state each
/// changed title had before, so `restore_watched` puts back play counts and dates exactly.
pub(crate) async fn mark_watched(
    library: &State<'_, LibraryState>,
    nfo: &State<'_, NfoState>,
    curation: &State<'_, CurationState>,
    settings: &State<'_, SettingsState>,
    ids: Vec<ItemId>,
    watched: bool,
) -> Result<WatchReportDto, LibraryCommandError> {
    let service = curation.watch(library, nfo).map_err(library_error)?;
    let nfo_settings = nfo_settings(settings);
    blocking(move || Ok(service.set_watched(&ids, watched, &nfo_settings)?.into()))
        .await
        .map_err(library_error)
}

// ---- the NEW flag ----------------------------------------------------------------------------

/// Which titles a flag change touched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct NewFlagReportDto {
    /// The items whose flag changed (episodes included). Pass them to `restore_new_flags` to
    /// undo.
    pub changed: Vec<String>,
    /// How many titles that is: movies and shows, an episode counted as its show.
    pub titles: u32,
}

impl From<NewFlagReport> for NewFlagReportDto {
    fn from(report: NewFlagReport) -> Self {
        Self {
            changed: report.changed.iter().map(|id| id.0.to_string()).collect(),
            titles: u32::try_from(report.titles).unwrap_or(u32::MAX),
        }
    }
}

fn kind_of(text: Option<&str>) -> Result<Option<ItemKind>, EditCommandError> {
    Ok(match text {
        None => None,
        Some("movie") => Some(ItemKind::Movie),
        Some("show") => Some(ItemKind::Show),
        Some("episode") => Some(ItemKind::Episode),
        Some(other) => {
            return Err(EditCommandError {
                code: "invalid".to_owned(),
                message: format!("{other:?} is not a kind of title"),
                field: None,
            });
        }
    })
}

/// Clears the NEW flag of the given titles.
#[tauri::command]
#[specta::specta]
pub async fn clear_new_flags(
    library: State<'_, LibraryState>,
    curation: State<'_, CurationState>,
    ids: Vec<String>,
) -> Result<NewFlagReportDto, EditCommandError> {
    let ids = ids_of(&ids)?;
    let service = curation.flags(&library)?;
    blocking(move || Ok(service.clear(&ids)?.into())).await
}

/// Clears the NEW flag of a title whose page was opened: only its own flag, so an opened show
/// keeps its NEW episodes (and the marker they give it). Returns the ids that changed.
#[tauri::command]
#[specta::specta]
pub async fn clear_new_on_open(
    library: State<'_, LibraryState>,
    curation: State<'_, CurationState>,
    id: String,
) -> Result<NewFlagReportDto, EditCommandError> {
    let id = id_of(&id)?;
    let service = curation.flags(&library)?;
    blocking(move || Ok(service.opened(id)?.into())).await
}

/// Clears the NEW flag of every title, or of one kind (`movie`, `show` or `episode`).
#[tauri::command]
#[specta::specta]
pub async fn clear_all_new_flags(
    library: State<'_, LibraryState>,
    curation: State<'_, CurationState>,
    kind: Option<String>,
) -> Result<NewFlagReportDto, EditCommandError> {
    let kind = kind_of(kind.as_deref())?;
    let service = curation.flags(&library)?;
    blocking(move || Ok(service.clear_all(kind)?.into())).await
}

/// Flags the given titles NEW again (Undo of a clear).
#[tauri::command]
#[specta::specta]
pub async fn restore_new_flags(
    library: State<'_, LibraryState>,
    curation: State<'_, CurationState>,
    ids: Vec<String>,
) -> Result<NewFlagReportDto, EditCommandError> {
    let ids = ids_of(&ids)?;
    let service = curation.flags(&library)?;
    blocking(move || Ok(service.restore(&ids)?.into())).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_watch_state_survives_the_wire_type() {
        let state = WatchState {
            id: ItemId(42),
            watched: true,
            play_count: 3,
            last_watched_ms: Some(1_714_564_800_000),
        };
        let dto = WatchStateDto::from(state);
        assert_eq!(dto.id, "42");
        assert_eq!(dto.into_state().unwrap(), state);
        let none = WatchState {
            last_watched_ms: None,
            ..state
        };
        assert_eq!(WatchStateDto::from(none).into_state().unwrap(), none);
    }

    #[test]
    fn only_the_three_playable_kinds_are_accepted_for_clearing() {
        assert_eq!(kind_of(None).unwrap(), None);
        assert_eq!(kind_of(Some("show")).unwrap(), Some(ItemKind::Show));
        assert_eq!(kind_of(Some("other")).unwrap_err().code, "invalid");
    }
}
