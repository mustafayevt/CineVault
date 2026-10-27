//! What happens to deleted and replaced files (Settings › Deleted files, spec `title-deletion`
//! "Where deleted files go").
//!
//! | Key | Values | Meaning |
//! |---|---|---|
//! | `files.deleted-mode` | `bin` (default), `folder`, `custom`, `erase` | where Delete, a move's originals, rename leftovers and replaced artwork or NFO files go |
//! | `files.trash-folder` | a folder path (default none) | the custom trash folder of `custom` |
//! | `files.auto-empty-days` | `off` (default), `7`, `30`, `90` | erase what has been in CineVault's own trash folders longer than this |
//!
//! * `bin`: the Recycle Bin / Trash for Delete and a move's originals, the drive's
//!   `.deletedByCineVault` where the drive has no bin; rename leftovers and replaced artwork or NFO
//!   files go to the drive's trash folder (as before this setting).
//! * `folder`: always the drive's `.deletedByCineVault`, never the system bin.
//! * `custom`: `<folder>/.deletedByCineVault/<batch>/<drive>/<path>` for files on the folder's
//!   own volume; files on another volume go to their drive's trash folder (a move there would be
//!   a copy). Only that `.deletedByCineVault` is ever emptied, never the chosen folder's top.
//! * `erase`: erased at once. Chosen only after the typed confirmation in Settings. A delete is
//!   then a permanent delete; a move erases an original only after its copy is verified.
//!
//! An unknown or unreadable value is `bin`: the default never erases anything.

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::Value;

use crate::app_settings::SettingsService;

/// The key of the choice.
pub const MODE_KEY: &str = "files.deleted-mode";
/// The key of the custom trash folder.
pub const FOLDER_KEY: &str = "files.trash-folder";
/// The key of the automatic emptying.
pub const AUTO_EMPTY_KEY: &str = "files.auto-empty-days";

/// Where deleted and replaced files go.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum DeletedMode {
    /// The Recycle Bin / Trash, the drive's trash folder where there is none.
    #[default]
    Bin,
    /// Always the drive's trash folder.
    Folder,
    /// The custom trash folder for files on its volume, the drive's trash folder for others.
    Custom(PathBuf),
    /// Erased at once.
    Erase,
}

/// The whole choice.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeletedFiles {
    pub mode: DeletedMode,
    /// Erase what has been in CineVault's own trash folders longer than this many days.
    pub auto_empty_days: Option<u32>,
}

impl DeletedFiles {
    /// The choice in a settings map.
    #[must_use]
    pub fn from_values(get: impl Fn(&str) -> Option<Value>) -> Self {
        let text = |key: &str| {
            get(key)
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default()
        };
        let mode = match text(MODE_KEY).as_str() {
            "folder" => DeletedMode::Folder,
            "erase" => DeletedMode::Erase,
            "custom" => {
                let folder = PathBuf::from(text(FOLDER_KEY).trim());
                if folder.is_absolute() {
                    DeletedMode::Custom(folder)
                } else {
                    // A custom choice without a usable folder is the drive's trash folder:
                    // never the bin the person turned away from, never erasing.
                    DeletedMode::Folder
                }
            }
            _ => DeletedMode::Bin,
        };
        let auto_empty_days = match get(AUTO_EMPTY_KEY) {
            Some(Value::String(s)) => s.trim().parse::<u32>().ok(),
            Some(Value::Number(n)) => n.as_u64().and_then(|n| u32::try_from(n).ok()),
            _ => None,
        }
        .filter(|days| [7, 30, 90].contains(days));
        Self {
            mode,
            auto_empty_days,
        }
    }

    /// The custom trash folder, when that is the choice.
    #[must_use]
    pub fn custom_folder(&self) -> Option<&std::path::Path> {
        match &self.mode {
            DeletedMode::Custom(folder) => Some(folder),
            _ => None,
        }
    }

    /// Whether deleted and replaced files are erased at once.
    #[must_use]
    pub fn erases(&self) -> bool {
        self.mode == DeletedMode::Erase
    }

    /// Whether the system bin is used at all.
    #[must_use]
    pub fn uses_bin(&self) -> bool {
        self.mode == DeletedMode::Bin
    }
}

/// Where the services read the choice from, at the moment they need it.
pub trait DeletedFilesSource: Send + Sync {
    fn deleted_files(&self) -> DeletedFiles;
}

impl DeletedFilesSource for SettingsService {
    fn deleted_files(&self) -> DeletedFiles {
        DeletedFiles::from_values(|key| self.get(key))
    }
}

/// A fixed choice (tests, tools, a service without settings).
#[derive(Debug, Clone, Default)]
pub struct FixedDeletedFiles(pub DeletedFiles);

impl DeletedFilesSource for FixedDeletedFiles {
    fn deleted_files(&self) -> DeletedFiles {
        self.0.clone()
    }
}

/// The default choice, for services that were given no settings.
#[must_use]
pub fn default_source() -> Arc<dyn DeletedFilesSource> {
    Arc::new(FixedDeletedFiles::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn read(pairs: &[(&str, Value)]) -> DeletedFiles {
        DeletedFiles::from_values(|key| {
            pairs
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| v.clone())
        })
    }

    #[test]
    fn nothing_stored_is_the_recycle_bin_without_emptying() {
        let choice = read(&[]);
        assert_eq!(choice.mode, DeletedMode::Bin);
        assert_eq!(choice.auto_empty_days, None);
        assert!(!choice.erases());
    }

    #[test]
    fn every_choice_is_read_and_a_custom_one_needs_a_full_path() {
        assert_eq!(
            read(&[(MODE_KEY, json!("folder"))]).mode,
            DeletedMode::Folder
        );
        assert_eq!(read(&[(MODE_KEY, json!("erase"))]).mode, DeletedMode::Erase);
        assert_eq!(
            read(&[(MODE_KEY, json!("custom"))]).mode,
            DeletedMode::Folder
        );
        let dir = std::env::temp_dir();
        let custom = read(&[
            (MODE_KEY, json!("custom")),
            (FOLDER_KEY, json!(dir.to_string_lossy())),
        ]);
        assert_eq!(custom.mode, DeletedMode::Custom(dir));
        assert_eq!(
            read(&[(MODE_KEY, json!("nonsense"))]).mode,
            DeletedMode::Bin
        );
    }

    #[test]
    fn only_the_offered_day_counts_empty_automatically() {
        assert_eq!(
            read(&[(AUTO_EMPTY_KEY, json!("30"))]).auto_empty_days,
            Some(30)
        );
        assert_eq!(read(&[(AUTO_EMPTY_KEY, json!(7))]).auto_empty_days, Some(7));
        assert_eq!(
            read(&[(AUTO_EMPTY_KEY, json!("off"))]).auto_empty_days,
            None
        );
        assert_eq!(read(&[(AUTO_EMPTY_KEY, json!("1"))]).auto_empty_days, None);
    }
}
