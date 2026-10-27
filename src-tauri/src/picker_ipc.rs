//! System pickers for the places that used to need a typed path: the library folders
//! page and the artwork picker's own file.
//!
//! The dialogs open from Rust (`DialogExt`), so the web view needs no dialog permission. Each
//! command returns what the user chose, or nothing when the dialog was cancelled. The first-run
//! setup keeps its own multi-folder command (`onboarding_ipc::pick_library_folders`).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_plugin_dialog::{DialogExt, FilePath};

/// Which files a file picker offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PickFileKind {
    /// A poster, fanart or other picture.
    Image,
}

/// The extensions a kind offers, without the dot. The artwork picker accepts the same image types.
fn extensions(kind: PickFileKind) -> &'static [&'static str] {
    match kind {
        PickFileKind::Image => &["jpg", "jpeg", "png", "webp", "bmp"],
    }
}

fn filter_name(kind: PickFileKind) -> &'static str {
    match kind {
        PickFileKind::Image => "Images",
    }
}

fn path_text(path: FilePath) -> Option<String> {
    path.into_path()
        .ok()
        .map(|path: PathBuf| path.to_string_lossy().into_owned())
}

/// Opens the system folder picker for a single folder. `None` when cancelled.
#[tauri::command]
#[specta::specta]
pub async fn pick_folder(app: tauri::AppHandle, title: Option<String>) -> Option<String> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let mut dialog = app.dialog().file();
    if let Some(title) = title {
        dialog = dialog.set_title(title);
    }
    dialog.pick_folder(move |chosen| {
        // A closed receiver means the window went away; there is nobody to tell.
        let _ = sender.send(chosen);
    });
    receiver.await.ok().flatten().and_then(path_text)
}

/// Opens the system file picker for files of one kind. Empty when cancelled; one file at most
/// unless `multiple` is set.
#[tauri::command]
#[specta::specta]
pub async fn pick_files(
    app: tauri::AppHandle,
    kind: PickFileKind,
    multiple: bool,
    title: Option<String>,
) -> Vec<String> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let mut dialog = app
        .dialog()
        .file()
        .add_filter(filter_name(kind), extensions(kind));
    if let Some(title) = title {
        dialog = dialog.set_title(title);
    }
    if multiple {
        dialog.pick_files(move |chosen| {
            let _ = sender.send(chosen.unwrap_or_default());
        });
    } else {
        dialog.pick_file(move |chosen| {
            let _ = sender.send(chosen.into_iter().collect::<Vec<_>>());
        });
    }
    receiver
        .await
        .unwrap_or_default()
        .into_iter()
        .filter_map(path_text)
        .collect()
}

/// Why a folder could not be opened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct OpenFolderError {
    /// `not_openable` (not an existing folder given as a full path) or `open_failed`.
    pub code: String,
    /// Plain sentence for the user.
    pub message: String,
}

/// Whether `path` is a folder that may be handed to the file manager: a full path to an existing
/// folder, never a file or a program.
fn openable(path: &std::path::Path) -> bool {
    path.is_absolute() && path.is_dir()
}

/// Opens a folder in the file manager. Only an existing folder given as a full path is opened.
#[tauri::command]
#[specta::specta]
pub async fn open_folder(app: tauri::AppHandle, path: String) -> Result<(), OpenFolderError> {
    use tauri_plugin_opener::OpenerExt;
    let target = PathBuf::from(path.trim());
    if !openable(&target) {
        return Err(OpenFolderError {
            code: "not_openable".to_owned(),
            message: format!("{} cannot be opened from here.", target.display()),
        });
    }
    app.opener()
        .open_path(target.to_string_lossy().into_owned(), None::<&str>)
        .map_err(|error| OpenFolderError {
            code: "open_failed".to_owned(),
            message: error.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_takes_existing_folders_but_never_a_file_or_a_program() {
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("setup.exe");
        std::fs::write(&program, b"MZ").unwrap();
        assert!(openable(dir.path()));
        assert!(!openable(&program));
        assert!(!openable(&dir.path().join("missing")));
        assert!(!openable(std::path::Path::new("relative/folder")));
    }

    #[test]
    fn the_image_filter_matches_what_the_artwork_picker_accepts() {
        let images = extensions(PickFileKind::Image);
        for wanted in ["jpg", "jpeg", "png", "webp", "bmp"] {
            assert!(images.contains(&wanted), "{wanted} missing");
        }
        assert!(images.iter().all(|e| !e.starts_with('.')));
    }
}
