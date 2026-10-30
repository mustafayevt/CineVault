//! Typed commands for NFO files: Read NFO, Rewrite NFO and undoing either.
//!
//! The rules live in `cv_app::nfo`; this module opens the service on first use (it shares the
//! library database with the folder commands), reads the NFO settings, runs the work off the UI
//! thread and converts the report into DTOs. A run on the whole library can touch thousands of
//! files, so the DTO carries totals and a capped list of notes about items that were skipped,
//! kept as they were or failed, not one row per item.
//!
//! The UI registers the two actions as selection commands in the command palette
//! (`ui/src/features/nfo`), shows an Undo toast with the `token`, and calls
//! [`undo_nfo_action`] from it.

use std::sync::{Arc, OnceLock};

use cv_app::nfo::{
    ActionKind, ActionReport, NfoError, NfoPreview, NfoScope, NfoService, NfoSettings, NfoSync,
    SkipReason, UndoSummary, UnitStatus,
};
use cv_domain::ItemId;
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::settings::SettingsState;
use crate::sources::LibraryState;

/// The most notes one report carries.
const MAX_NOTES: usize = 100;

/// Held in Tauri state. The service is built by the first command that needs it.
pub struct NfoState {
    service: OnceLock<Result<Arc<NfoService>, String>>,
}

impl NfoState {
    pub const fn new() -> Self {
        Self {
            service: OnceLock::new(),
        }
    }

    pub fn service(&self, library: &LibraryState) -> Result<Arc<NfoService>, NfoCommandError> {
        self.service
            .get_or_init(|| {
                let db = library.database()?;
                let location = library.location().ok_or_else(|| {
                    "CineVault could not find a folder for its data, so NFO actions cannot be \
                     undone."
                        .to_owned()
                })?;
                Ok(Arc::new(NfoService::new(db, location.nfo_undo_dir())))
            })
            .as_ref()
            .map(Arc::clone)
            .map_err(|message| NfoCommandError {
                code: "storage".to_owned(),
                message: message.clone(),
            })
    }
}

impl Default for NfoState {
    fn default() -> Self {
        Self::new()
    }
}

/// What an action runs on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NfoScopeDto {
    /// The selected library items (ids as the library views hold them). A show covers its
    /// seasons and episodes.
    Selection { ids: Vec<String> },
    /// Every movie, set and show.
    Everything,
}

impl NfoScopeDto {
    fn to_scope(&self) -> Result<NfoScope, NfoCommandError> {
        match self {
            Self::Everything => Ok(NfoScope::All),
            Self::Selection { ids } => ids
                .iter()
                .map(|id| {
                    id.trim()
                        .parse::<i64>()
                        .map(ItemId)
                        .map_err(|_| NfoCommandError {
                            code: "invalid_id".to_owned(),
                            message: format!("{id:?} is not a library item id"),
                        })
                })
                .collect::<Result<Vec<_>, _>>()
                .map(NfoScope::Items),
        }
    }
}

/// Which action a report is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum NfoActionKindDto {
    Read,
    Rewrite,
}

/// One item that did not simply change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct NfoNoteDto {
    /// Library id of the item.
    pub item: String,
    /// Title, with season and episode for TV.
    pub label: String,
    /// `skipped_not_found`, `skipped_unsupported`, `skipped_no_files`, `skipped_folder_missing`,
    /// `skipped_no_collection_folder`, `skipped_no_nfo`, `skipped_not_managed`, `kept_foreign` or
    /// `failed`.
    pub code: String,
    /// The error text for `failed`, otherwise empty.
    pub detail: String,
}

/// The result of Read NFO or Rewrite NFO.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct NfoActionDto {
    pub kind: NfoActionKindDto,
    /// Pass it to `undo_nfo_action`. `None` when nothing changed.
    pub token: Option<String>,
    /// Items (or files of episodes) whose files or library values changed.
    pub changed: u32,
    pub unchanged: u32,
    /// Items whose target names held files that are not CineVault's; those were left alone.
    pub kept_foreign: u32,
    pub skipped: u32,
    pub failed: u32,
    pub files_written: u32,
    /// Files moved to the undo folder because the naming settings dropped them.
    pub files_trashed: u32,
    /// Up to 100 items that were skipped, kept as they were or failed.
    pub notes: Vec<NfoNoteDto>,
}

fn skip_code(reason: SkipReason) -> &'static str {
    match reason {
        SkipReason::NotFound => "skipped_not_found",
        SkipReason::Unsupported => "skipped_unsupported",
        SkipReason::NoFiles => "skipped_no_files",
        SkipReason::FolderMissing => "skipped_folder_missing",
        SkipReason::NoCollectionFolder => "skipped_no_collection_folder",
        SkipReason::NoNfo => "skipped_no_nfo",
        SkipReason::NotManaged => "skipped_not_managed",
    }
}

impl From<ActionReport> for NfoActionDto {
    fn from(report: ActionReport) -> Self {
        let notes = report
            .units
            .iter()
            .filter_map(|unit| {
                let (code, detail) = match &unit.status {
                    UnitStatus::Changed | UnitStatus::Unchanged => return None,
                    UnitStatus::KeptForeign => ("kept_foreign".to_owned(), String::new()),
                    UnitStatus::Skipped { reason } => {
                        (skip_code(*reason).to_owned(), String::new())
                    }
                    UnitStatus::Failed { message } => ("failed".to_owned(), message.clone()),
                };
                Some(NfoNoteDto {
                    item: unit.item.0.to_string(),
                    label: unit.label.clone(),
                    code,
                    detail,
                })
            })
            .take(MAX_NOTES)
            .collect();
        let t = report.totals;
        Self {
            kind: match report.kind {
                ActionKind::Read => NfoActionKindDto::Read,
                ActionKind::Rewrite => NfoActionKindDto::Rewrite,
            },
            token: report.token,
            changed: t.changed,
            unchanged: t.unchanged,
            kept_foreign: t.kept_foreign,
            skipped: t.skipped,
            failed: t.failed,
            files_written: t.files_written,
            files_trashed: t.files_trashed,
            notes,
        }
    }
}

/// What an undo did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct NfoUndoDto {
    /// Files and library values put back.
    pub restored: u32,
    /// What was left as it is because it changed since the action, each with its reason.
    pub skipped: Vec<String>,
}

impl From<UndoSummary> for NfoUndoDto {
    fn from(summary: UndoSummary) -> Self {
        Self {
            restored: summary.restored,
            skipped: summary.skipped,
        }
    }
}

/// Why a command did not run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct NfoCommandError {
    /// `storage`, `invalid_id`, `not_found`, `unknown_token`, `already_undone` or `record`.
    pub code: String,
    /// Plain sentence for the user.
    pub message: String,
}

impl From<NfoError> for NfoCommandError {
    fn from(error: NfoError) -> Self {
        let code = match &error {
            NfoError::Db(_) | NfoError::Io(_) => "storage",
            NfoError::Record(_) => "record",
            NfoError::UnknownToken(_) => "unknown_token",
            NfoError::AlreadyUndone => "already_undone",
            NfoError::Busy(_) => "busy",
        };
        Self {
            code: code.to_owned(),
            message: error.to_string(),
        }
    }
}

pub fn nfo_settings(state: &SettingsState) -> NfoSettings {
    NfoSettings::from_values(&state.service().snapshot().values)
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, NfoCommandError> + Send + 'static,
) -> Result<T, NfoCommandError> {
    cv_jobs::client::spawn_blocking(work)
        .await
        .map_err(|error| NfoCommandError {
            code: "storage".to_owned(),
            message: format!("the NFO action stopped unexpectedly: {error}"),
        })?
}

/// Reads the NFO files of the selection (or the whole library) into the library, replacing the
/// values the files hold. Returns a token that `undo_nfo_action` takes.
pub async fn read_nfo(
    library: &LibraryState,
    nfo: &NfoState,
    settings: &SettingsState,
    scope: NfoScopeDto,
) -> Result<NfoActionDto, NfoCommandError> {
    let service = nfo.service(library)?;
    let settings = nfo_settings(settings);
    let scope = scope.to_scope()?;
    blocking(move || Ok(service.read(&scope, &settings)?.into())).await
}

/// Writes the NFO files of the selection (or the whole library) from the library, following the
/// NFO settings. Files that are not CineVault's are never touched.
pub async fn rewrite_nfo(
    library: &LibraryState,
    nfo: &NfoState,
    settings: &SettingsState,
    scope: NfoScopeDto,
) -> Result<NfoActionDto, NfoCommandError> {
    let service = nfo.service(library)?;
    let settings = nfo_settings(settings);
    let scope = scope.to_scope()?;
    blocking(move || Ok(service.rewrite(&scope, &settings)?.into())).await
}

/// Undoes one Read NFO or Rewrite NFO by its token. Files and items that changed since are left
/// alone and listed in the reply.
pub async fn undo_nfo_action(
    library: &LibraryState,
    nfo: &NfoState,
    token: String,
) -> Result<NfoUndoDto, NfoCommandError> {
    let service = nfo.service(library)?;
    blocking(move || Ok(service.undo(&token)?.into())).await
}

/// How the NFO file on disk compares with the preview.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum NfoSyncDto {
    /// The file holds the same content.
    InSync,
    /// The file is CineVault's but says something else.
    Differs,
    /// There is no NFO file yet.
    Missing,
    /// A file with that name exists but is not CineVault's; a rewrite leaves it alone.
    NotManaged,
    /// The movie has no place for an NFO.
    NoTarget,
}

impl From<NfoSync> for NfoSyncDto {
    fn from(sync: NfoSync) -> Self {
        match sync {
            NfoSync::InSync => Self::InSync,
            NfoSync::Differs => Self::Differs,
            NfoSync::Missing => Self::Missing,
            NfoSync::NotManaged => Self::NotManaged,
            NfoSync::NoTarget => Self::NoTarget,
        }
    }
}

/// The NFO a movie would get from a rewrite.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct NfoPreviewDto {
    /// The rendered NFO.
    pub text: String,
    /// `Kodi`, `XBMC`, `Emby`, `Jellyfin` or `MediaPortal`.
    pub format: String,
    /// The file a rewrite writes first, when there is one.
    pub path: Option<String>,
    pub sync: NfoSyncDto,
    /// What the NFO file on disk says now, when it is CineVault's and could be read; the page
    /// shows its difference from `text`.
    pub file_text: Option<String>,
}

impl From<NfoPreview> for NfoPreviewDto {
    fn from(preview: NfoPreview) -> Self {
        Self {
            text: preview.text,
            format: preview.format.to_owned(),
            path: preview.path.map(|p| p.to_string_lossy().into_owned()),
            sync: preview.sync.into(),
            file_text: preview.file_text,
        }
    }
}

fn parse_item(id: &str) -> Result<ItemId, NfoCommandError> {
    id.trim()
        .parse::<i64>()
        .map(ItemId)
        .map_err(|_| NfoCommandError {
            code: "invalid_id".to_owned(),
            message: format!("{id:?} is not a library item id"),
        })
}

fn gone(what: &str) -> NfoCommandError {
    NfoCommandError {
        code: "not_found".to_owned(),
        message: format!("That {what} is not in the library any more."),
    }
}

/// Renders the NFO of a movie as Rewrite NFO would, without writing it, and says whether the NFO
/// on disk already holds the same.
pub async fn preview_movie_nfo(
    library: &LibraryState,
    nfo: &NfoState,
    settings: &SettingsState,
    id: String,
) -> Result<NfoPreviewDto, NfoCommandError> {
    let service = nfo.service(library)?;
    let settings = nfo_settings(settings);
    let item = parse_item(&id)?;
    blocking(move || {
        service
            .preview_movie(item, &settings)?
            .map(NfoPreviewDto::from)
            .ok_or_else(|| gone("movie"))
    })
    .await
}

/// Renders the `tvshow.nfo` of a show as Rewrite NFO would, without writing it.
pub async fn preview_show_nfo(
    library: &LibraryState,
    nfo: &NfoState,
    settings: &SettingsState,
    id: String,
) -> Result<NfoPreviewDto, NfoCommandError> {
    let service = nfo.service(library)?;
    let settings = nfo_settings(settings);
    let item = parse_item(&id)?;
    blocking(move || {
        service
            .preview_show(item, &settings)?
            .map(NfoPreviewDto::from)
            .ok_or_else(|| gone("show"))
    })
    .await
}

/// Renders the NFO of the video file an episode sits in as Rewrite NFO would, without writing it.
pub async fn preview_episode_nfo(
    library: &LibraryState,
    nfo: &NfoState,
    settings: &SettingsState,
    id: String,
) -> Result<NfoPreviewDto, NfoCommandError> {
    let service = nfo.service(library)?;
    let settings = nfo_settings(settings);
    let item = parse_item(&id)?;
    blocking(move || {
        service
            .preview_episode(item, &settings)?
            .map(NfoPreviewDto::from)
            .ok_or_else(|| gone("episode"))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use cv_db::Database;
    use cv_db::repo::insert_item;
    use cv_domain::{Item, MediaFile, MediaFileType};

    fn state_with_movie() -> (
        tempfile::TempDir,
        NfoState,
        Arc<NfoService>,
        ItemId,
        std::path::PathBuf,
    ) {
        let dir = tempfile::tempdir().expect("temp dir");
        let db = Arc::new(Database::open(dir.path().join("library.db")).expect("library"));
        let folder = dir.path().join("Heat (1995)");
        std::fs::create_dir_all(&folder).expect("folder");
        std::fs::write(folder.join("Heat (1995).mkv"), b"video").expect("video");
        let mut movie = Item::movie("Heat");
        movie.core.path = folder.to_string_lossy().into_owned();
        movie.core.media_files.push(MediaFile::new(
            MediaFileType::Video,
            folder.to_string_lossy(),
            "Heat (1995).mkv",
        ));
        let id = db.write(move |tx| insert_item(tx, &movie)).expect("insert");
        let service = Arc::new(NfoService::new(db, dir.path().join("undo")));
        let state = NfoState::new();
        state
            .service
            .set(Ok(Arc::clone(&service)))
            .expect("fresh state");
        (dir, state, service, id, folder)
    }

    #[test]
    fn scopes_parse_ids_and_refuse_nonsense() {
        let ok = NfoScopeDto::Selection {
            ids: vec!["4".to_owned(), " 7 ".to_owned()],
        }
        .to_scope()
        .expect("ids");
        assert_eq!(ok, NfoScope::Items(vec![ItemId(4), ItemId(7)]));
        assert_eq!(
            NfoScopeDto::Everything.to_scope().expect("all"),
            NfoScope::All
        );
        let bad = NfoScopeDto::Selection {
            ids: vec!["abc".to_owned()],
        }
        .to_scope()
        .expect_err("not an id");
        assert_eq!(bad.code, "invalid_id");
    }

    #[test]
    fn a_report_becomes_totals_and_notes_and_an_undo_round_trips() {
        let (_dir, _state, service, id, folder) = state_with_movie();
        let settings = NfoSettings::default();

        // One movie is written; an unknown id is a note.
        let report = service
            .rewrite(&NfoScope::Items(vec![id, ItemId(999)]), &settings)
            .expect("rewrite");
        let dto = NfoActionDto::from(report);
        assert_eq!(dto.kind, NfoActionKindDto::Rewrite);
        assert_eq!((dto.changed, dto.skipped, dto.files_written), (1, 1, 1));
        assert_eq!(dto.notes.len(), 1);
        assert_eq!(dto.notes[0].code, "skipped_not_found");
        let token = dto.token.expect("a token for the change");
        assert!(folder.join("Heat (1995).nfo").exists());

        let undo = NfoUndoDto::from(service.undo(&token).expect("undo"));
        assert_eq!(undo.restored, 1);
        assert!(undo.skipped.is_empty());
        assert!(!folder.join("Heat (1995).nfo").exists());

        let again = NfoCommandError::from(service.undo(&token).expect_err("used"));
        assert_eq!(again.code, "already_undone");
        let unknown = NfoCommandError::from(service.undo("nfo-1-1").expect_err("unknown"));
        assert_eq!(unknown.code, "unknown_token");
    }

    #[test]
    fn the_state_hands_out_its_service() {
        let (_dir, state, service, _, _) = state_with_movie();
        let library = LibraryState::unavailable("not needed for this test");
        let got = state.service(&library).expect("service");
        assert!(Arc::ptr_eq(&got, &service));
    }
}
