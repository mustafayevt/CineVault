//! The Tauri surface of the organize (rename) commands.
//!
//! The state, the DTOs and the command bodies live in [`cv_ipc::organize`]. The start-up recovery
//! hook stays here because it runs from the desktop composition root.

use std::sync::Arc;

use cv_app::organize;

use tauri::State;

use crate::settings_ipc::SettingsState;
use crate::sources_ipc::LibraryState;

pub use cv_ipc::organize::*;

/// Settles what a crash left half done. **Call once at start-up, before the first scan**, off
/// the window's thread: it looks at the library folders, and a share that does not answer can
/// take many seconds.
///
/// Rows whose disk state recovery will not guess at are logged; the Organize screen lists them
/// in the journal. Rows on a drive that is not there are settled when it is back. A library that
/// cannot be opened is reported by the commands that need it.
pub fn recover_at_startup(library: &LibraryState, boot: &crate::boot::Boot) {
    let Ok(db) = library.database() else {
        return;
    };
    let settled = organize::recover_interrupted_batches(&db);
    if let Ok(report) = &settled {
        if !report.deferred.is_empty() {
            tracing::info!(
                rows = report.deferred.len(),
                "interrupted renames on a drive that is not there wait for it"
            );
        }
        organize::retry_deferred_recovery(Arc::clone(&db), report);
    }
    // Moves and renames that did not finish are listed before any new file work and offered in
    // Activity's Needs attention: Resume or Roll back. The folders they touched are not scanned
    // behind the user's back: a half-copied title lives in a hidden staging folder.
    let recovery = cv_app::library_ops::RecoveryService::new(Arc::clone(&db));
    // A batch that finished but has no History entry (the app stopped in between) gets one, so
    // History can undo it.
    if let Err(error) = recovery.record_unrecorded() {
        tracing::warn!(%error, "finished batches could not be checked for History entries");
    }
    if let Some(services) = boot.services() {
        match recovery.find() {
            Ok(found) => {
                for batch in found {
                    services.notices().publish(batch.notice());
                }
            }
            Err(error) => tracing::warn!(%error, "unfinished moves could not be listed"),
        }
    }
    match settled {
        Ok(report) => {
            let attention = report.needs_attention().count();
            if attention > 0 {
                tracing::warn!(
                    rows = attention,
                    "an interrupted organize left files that need a look"
                );
            }
        }
        Err(error) => tracing::warn!(%error, "organize crash recovery could not run"),
    }
}

/// The naming profiles (the presets and the user's own) and the active one.
#[tauri::command]
#[specta::specta]
pub fn organize_profiles(settings: State<'_, SettingsState>) -> OrganizeProfilesDto {
    cv_ipc::organize::organize_profiles(&settings)
}

/// Makes a profile the active one (the one rename-after-scrape uses too).
#[tauri::command]
#[specta::specta]
pub fn organize_select_profile(
    settings: State<'_, SettingsState>,
    id: String,
) -> Result<OrganizeProfilesDto, OrganizeCommandError> {
    cv_ipc::organize::organize_select_profile(&settings, id)
}

/// Saves the editor's templates as a new custom profile named `name`, copied from `from` for
/// everything but the templates, and makes it the active one.
#[tauri::command]
#[specta::specta]
pub fn organize_save_profile(
    settings: State<'_, SettingsState>,
    name: String,
    from: String,
    patterns: PatternsDto,
) -> Result<OrganizeProfilesDto, OrganizeCommandError> {
    cv_ipc::organize::organize_save_profile(&settings, name, from, patterns)
}

/// Replaces the templates of a custom profile.
#[tauri::command]
#[specta::specta]
pub fn organize_update_profile(
    settings: State<'_, SettingsState>,
    id: String,
    patterns: PatternsDto,
) -> Result<OrganizeProfilesDto, OrganizeCommandError> {
    cv_ipc::organize::organize_update_profile(&settings, id, patterns)
}

/// Deletes a custom profile. When it was the active one the default becomes active.
#[tauri::command]
#[specta::specta]
pub fn organize_delete_profile(
    settings: State<'_, SettingsState>,
    id: String,
) -> Result<OrganizeProfilesDto, OrganizeCommandError> {
    cv_ipc::organize::organize_delete_profile(&settings, id)
}

/// Checks a template as typed: unknown tokens with suggestions, syntax errors with positions and
/// the TV rules for the episode file template.
#[tauri::command]
#[specta::specta]
pub fn organize_check_template(
    field: PatternFieldDto,
    template: String,
) -> Vec<TemplateFindingDto> {
    cv_ipc::organize::organize_check_template(field, template)
}

/// Renders a profile's movie templates on one file name, or on a built-in sample when no name is
/// given. Nothing is read from disk and nothing is saved: the first run uses it before any
/// library exists.
#[tauri::command]
#[specta::specta]
pub fn organize_example_for_file(
    settings: State<'_, SettingsState>,
    profile_id: String,
    file_name: Option<String>,
) -> Result<FileExampleDto, OrganizeCommandError> {
    cv_ipc::organize::organize_example_for_file(&settings, profile_id, file_name)
}

/// The tokens of the movie templates (`tv` false) or the TV templates, for the `$` autocomplete.
#[tauri::command]
#[specta::specta]
pub fn organize_tokens(tv: bool) -> Vec<TemplateTokenDto> {
    cv_ipc::organize::organize_tokens(tv)
}

/// Plans the request and describes it: the diff rows, conflicts, counts and live examples.
/// Nothing touches the disk.
#[tauri::command]
#[specta::specta]
pub async fn organize_preview(
    library: State<'_, LibraryState>,
    organize: State<'_, OrganizeState>,
    settings: State<'_, SettingsState>,
    request: OrganizeRequestDto,
) -> Result<OrganizePreviewDto, OrganizeCommandError> {
    cv_ipc::organize::organize_preview(&library, &organize, &settings, request).await
}

/// Runs the checked changes of the request and records one journal batch. The request is
/// planned again first; a checked item that is not a clean change any more is not run and is
/// listed in `not_run`.
#[tauri::command]
#[specta::specta]
pub async fn organize_apply(
    library: State<'_, LibraryState>,
    organize: State<'_, OrganizeState>,
    settings: State<'_, SettingsState>,
    request: OrganizeRequestDto,
    include: Vec<String>,
) -> Result<AppliedDto, OrganizeCommandError> {
    cv_ipc::organize::organize_apply(&library, &organize, &settings, request, include).await
}

/// The organize batches, newest first.
#[tauri::command]
#[specta::specta]
pub async fn organize_journal(
    library: State<'_, LibraryState>,
    organize: State<'_, OrganizeState>,
) -> Result<Vec<JournalBatchDto>, OrganizeCommandError> {
    cv_ipc::organize::organize_journal(&library, &organize).await
}

/// Takes a batch back in reverse. Items changed since are left alone and reported.
#[tauri::command]
#[specta::specta]
pub async fn organize_undo(
    library: State<'_, LibraryState>,
    organize: State<'_, OrganizeState>,
    batch_id: String,
) -> Result<UndoOutcomeDto, OrganizeCommandError> {
    cv_ipc::organize::organize_undo(&library, &organize, batch_id).await
}
