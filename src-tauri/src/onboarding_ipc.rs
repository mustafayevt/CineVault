//! The Tauri surface of the first-run setup commands.
//!
//! The analysis, its DTOs and the naming example live in [`cv_ipc::onboarding`] and
//! [`cv_ipc::organize`]. The system folder picker is desktop-only and stays here.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::State;
use tauri_plugin_dialog::DialogExt;

use crate::sources_ipc::LibraryState;

pub use cv_ipc::onboarding::*;

/// Looks inside a folder and says whether it holds movies, TV shows or both.
#[tauri::command]
#[specta::specta]
pub async fn analyze_library_folder(
    state: State<'_, LibraryState>,
    path: String,
) -> Result<FolderAnalysisDto, OnboardingCommandError> {
    cv_ipc::onboarding::analyze_library_folder(&state, path).await
}

/// Set while the first-run folder picker is open.
static PICKER_OPEN: AtomicBool = AtomicBool::new(false);

/// Holds [`PICKER_OPEN`] until dropped.
struct PickerOpen;

impl PickerOpen {
    /// `None` when a picker is open already.
    fn claim() -> Option<Self> {
        PICKER_OPEN
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| Self)
    }
}

impl Drop for PickerOpen {
    fn drop(&mut self) {
        PICKER_OPEN.store(false, Ordering::Release);
    }
}

/// Opens the system folder picker and returns the folders the user chose (none when cancelled).
/// Only one picker opens at a time; a request while it is open returns nothing.
#[tauri::command]
#[specta::specta]
pub async fn pick_library_folders(app: tauri::AppHandle) -> Vec<String> {
    let Some(_open) = PickerOpen::claim() else {
        return Vec::new();
    };
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog().file().pick_folders(move |chosen| {
        // A closed receiver means the window went away; there is nobody to tell.
        let _ = sender.send(chosen);
    });
    receiver
        .await
        .ok()
        .flatten()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|path| path.into_path().ok())
        .map(|path: PathBuf| path.to_string_lossy().into_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_one_folder_picker_is_open_at_a_time() {
        let first = super::PickerOpen::claim().expect("no picker is open");
        assert!(super::PickerOpen::claim().is_none());
        drop(first);
        assert!(super::PickerOpen::claim().is_some());
    }
}
