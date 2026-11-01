//! The Tauri surface of the copies commands.
//!
//! The groups, the DTOs and the command bodies live in [`cv_ipc::copies`].

use tauri::State;

use crate::library_ipc::LibraryCommandError;
use crate::sources_ipc::LibraryState;

pub use cv_ipc::copies::*;

/// The copies of a movie, kept or not; `None` when it has no copy.
#[tauri::command]
#[specta::specta]
pub async fn copies_of(
    library: State<'_, LibraryState>,
    id: String,
) -> Result<Option<CopiesOfDto>, LibraryCommandError> {
    cv_ipc::copies::copies_of(&library, id).await
}

/// The Compare copies page of a movie: one card per copy, the best quality marked; `None` when
/// the movie has no copy left.
#[tauri::command]
#[specta::specta]
pub async fn compare_copies(
    library: State<'_, LibraryState>,
    id: String,
) -> Result<Option<ComparisonDto>, LibraryCommandError> {
    cv_ipc::copies::compare_copies(&library, id).await
}

/// "Keep both" / "Keep all": the user keeps every one of `ids` as copies. No file changes.
#[tauri::command]
#[specta::specta]
pub async fn keep_all_copies(
    library: State<'_, LibraryState>,
    ids: Vec<String>,
) -> Result<(), LibraryCommandError> {
    cv_ipc::copies::keep_all_copies(&library, ids).await
}

/// "Ask again": the movie's group counts as copies again. Also the Undo of "Keep both".
#[tauri::command]
#[specta::specta]
pub async fn ask_again_copies(
    library: State<'_, LibraryState>,
    id: String,
) -> Result<(), LibraryCommandError> {
    cv_ipc::copies::ask_again_copies(&library, id).await
}
