//! Where the app keeps its data.
//!
//! Three modes, in this order of precedence:
//!
//! 1. **Portable**: a file named `portable` sits next to the executable. All data lives in a
//!    `data` folder beside the executable.
//! 2. **Custom folder**: the user picked a data folder. The choice is stored in a small pointer
//!    file (`data-location.json`) in the per-user *config* folder, because it cannot live inside
//!    the data folder it points to.
//! 3. **Per-user** (default): the per-user application-data folder (`%APPDATA%\CineVault` on
//!    Windows, `~/Library/Application Support/CineVault` on macOS).
//!
//! Resolution never creates or moves anything; call [`DataLocation::ensure_dirs`] to create the
//! folders. A pointer file that cannot be used falls back to the per-user folder and the result
//! carries a warning, so a bad pointer never blocks startup and is never overwritten silently.

use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Name of the marker file that switches on portable mode.
pub const PORTABLE_MARKER: &str = "portable";
/// Data folder name used in portable mode, beside the executable.
pub const PORTABLE_DATA_DIR: &str = "data";
/// Folder name under the per-user application-data folder.
pub const APP_DIR_NAME: &str = "CineVault";
/// Name of the pointer file holding the custom data folder choice.
pub const POINTER_FILE: &str = "data-location.json";

/// Which rule picked the data folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocationMode {
    /// `data` folder beside the executable (marker file present).
    Portable,
    /// A user-chosen folder.
    Custom,
    /// The per-user application-data folder.
    UserDefault,
    /// `CINEVAULT_DATA_DIR`, for a container or service.
    Environment,
}

/// Errors from saving the custom folder choice.
#[derive(Debug, thiserror::Error)]
pub enum LocationError {
    #[error("the data folder must be an absolute path: {0}")]
    NotAbsolute(PathBuf),
    #[error("could not write {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// The folders the resolution looks at. Tests build this by hand; [`Self::from_environment`]
/// fills it from the running process.
#[derive(Debug, Clone)]
pub struct LocationInputs {
    /// Folder that contains the executable.
    pub exe_dir: PathBuf,
    /// Per-user application-data folder of the OS (the app folder is added below it).
    pub user_data_base: PathBuf,
    /// Per-user config folder holding the pointer file (the app folder is added below it).
    pub user_config_base: PathBuf,
}

impl LocationInputs {
    /// Inputs for the running process. Returns `None` if the executable path or the OS folders
    /// cannot be determined.
    #[must_use]
    pub fn from_environment() -> Option<Self> {
        let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
        Some(Self {
            exe_dir,
            user_data_base: dirs::data_dir()?,
            user_config_base: dirs::config_dir()?,
        })
    }

    fn pointer_path(&self) -> PathBuf {
        self.user_config_base.join(APP_DIR_NAME).join(POINTER_FILE)
    }
}

#[derive(Serialize, Deserialize)]
struct Pointer {
    data_folder: PathBuf,
}

/// The resolved data folder and how it was chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataLocation {
    root: PathBuf,
    mode: LocationMode,
    warning: Option<String>,
}

impl DataLocation {
    /// Resolve the data folder: portable marker, then custom pointer, then per-user default.
    #[must_use]
    pub fn resolve(inputs: &LocationInputs) -> Self {
        Self::resolve_with_env(inputs, None)
    }

    /// Resolve the data folder with an optional `CINEVAULT_DATA_DIR` value.
    ///
    /// An absolute environment value wins over every other rule. A relative or empty one is
    /// refused with a warning and the next rule decides, so a bad variable never stops the app.
    #[must_use]
    pub fn resolve_with_env(inputs: &LocationInputs, data_dir: Option<&OsStr>) -> Self {
        if let Some(value) = data_dir {
            let path = PathBuf::from(value);
            if path.is_absolute() {
                return Self {
                    root: path,
                    mode: LocationMode::Environment,
                    warning: None,
                };
            }
            let warning = if value.is_empty() {
                "CINEVAULT_DATA_DIR is empty; using the default data folder".to_owned()
            } else {
                format!(
                    "CINEVAULT_DATA_DIR={} is not an absolute path; using the default data folder",
                    path.display()
                )
            };
            let mut location = Self::resolve_fallback(inputs);
            location.warning = Some(match location.warning.take() {
                Some(other) => format!("{warning}; {other}"),
                None => warning,
            });
            return location;
        }
        Self::resolve_fallback(inputs)
    }

    fn resolve_fallback(inputs: &LocationInputs) -> Self {
        if inputs.exe_dir.join(PORTABLE_MARKER).is_file() {
            return Self {
                root: inputs.exe_dir.join(PORTABLE_DATA_DIR),
                mode: LocationMode::Portable,
                warning: None,
            };
        }
        let default = inputs.user_data_base.join(APP_DIR_NAME);
        let pointer_path = inputs.pointer_path();
        match fs::read_to_string(&pointer_path) {
            Ok(text) => match serde_json::from_str::<Pointer>(&text) {
                Ok(p) if p.data_folder.is_absolute() => Self {
                    root: p.data_folder,
                    mode: LocationMode::Custom,
                    warning: None,
                },
                Ok(p) => Self::fallback(
                    default,
                    format!(
                        "the custom data folder {} is not an absolute path; using the default",
                        p.data_folder.display()
                    ),
                ),
                Err(e) => Self::fallback(
                    default,
                    format!(
                        "{} is not valid ({e}); using the default data folder",
                        pointer_path.display()
                    ),
                ),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self {
                root: default,
                mode: LocationMode::UserDefault,
                warning: None,
            },
            Err(e) => Self::fallback(
                default,
                format!(
                    "could not read {} ({e}); using the default data folder",
                    pointer_path.display()
                ),
            ),
        }
    }

    fn fallback(root: PathBuf, warning: String) -> Self {
        Self {
            root,
            mode: LocationMode::UserDefault,
            warning: Some(warning),
        }
    }

    /// Resolve for the running process, or `None` if the OS folders are unknown.
    ///
    /// `CINEVAULT_DATA_DIR` is read first, so a container with no `HOME` still finds its data.
    #[must_use]
    pub fn resolve_for_process() -> Option<Self> {
        let data_dir = std::env::var_os("CINEVAULT_DATA_DIR");
        if let Some(value) = data_dir.as_deref() {
            let path = PathBuf::from(value);
            if path.is_absolute() {
                return Some(Self {
                    root: path,
                    mode: LocationMode::Environment,
                    warning: None,
                });
            }
        }
        LocationInputs::from_environment().map(|i| Self::resolve_with_env(&i, data_dir.as_deref()))
    }

    /// Remember a user-chosen data folder. Takes effect on the next [`Self::resolve`] (the app
    /// restarts, or the caller moves the data first). Portable mode still wins over it.
    ///
    /// # Errors
    /// Fails if the folder is not absolute or the pointer file cannot be written.
    pub fn set_custom_folder(inputs: &LocationInputs, folder: &Path) -> Result<(), LocationError> {
        if !folder.is_absolute() {
            return Err(LocationError::NotAbsolute(folder.to_path_buf()));
        }
        let path = inputs.pointer_path();
        let io = |source| LocationError::Io {
            path: path.clone(),
            source,
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(io)?;
        }
        let json = serde_json::to_vec_pretty(&Pointer {
            data_folder: folder.to_path_buf(),
        })
        .map_err(|e| io(std::io::Error::other(e)))?;
        let tmp = path.with_extension("json.tmp");
        let mut f = fs::File::create(&tmp).map_err(io)?;
        f.write_all(&json).map_err(io)?;
        f.sync_all().map_err(io)?;
        drop(f);
        fs::rename(&tmp, &path).map_err(io)
    }

    /// Forget the custom folder and go back to the per-user default.
    ///
    /// # Errors
    /// Fails if the pointer file exists but cannot be removed.
    pub fn clear_custom_folder(inputs: &LocationInputs) -> Result<(), LocationError> {
        let path = inputs.pointer_path();
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(LocationError::Io { path, source }),
        }
    }

    /// The data folder.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Which rule chose [`Self::root`].
    #[must_use]
    pub fn mode(&self) -> LocationMode {
        self.mode
    }

    /// Set when a custom folder choice had to be ignored.
    #[must_use]
    pub fn warning(&self) -> Option<&str> {
        self.warning.as_deref()
    }

    /// The library database file.
    #[must_use]
    pub fn library_db(&self) -> PathBuf {
        self.root.join("library.db")
    }

    /// The settings file.
    #[must_use]
    pub fn settings_file(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    /// Cache folder (artwork thumbnails, provider responses).
    #[must_use]
    pub fn cache_dir(&self) -> PathBuf {
        self.root.join("cache")
    }

    /// Downloaded external tools (MediaInfo, FFmpeg, the indexer service). Created when the first one is
    /// installed.
    #[must_use]
    pub fn tools_dir(&self) -> PathBuf {
        self.root.join("tools")
    }

    /// The data folder of the managed indexer service (its configuration, cache and indexer
    /// settings). Created when the service is first started, which never happens while Torrent
    /// search is off.
    #[must_use]
    pub fn indexer_service_dir(&self) -> PathBuf {
        self.root.join("indexer-service")
    }

    /// The user's export templates: one folder per template, each with a `template.conf`. Not
    /// created by the app; the user adds folders to it.
    #[must_use]
    pub fn export_templates_dir(&self) -> PathBuf {
        self.root.join("export-templates")
    }

    /// Scratch space for work in progress, such as the update download (`tmp/updates`). Nothing
    /// in it is kept. Created when first used.
    #[must_use]
    pub fn tmp_dir(&self) -> PathBuf {
        self.root.join("tmp")
    }

    /// Where Read NFO and Rewrite NFO keep what they need to undo an action (replaced files, files
    /// moved out of the way, replaced library values). Created when the first action needs it;
    /// nothing in it is deleted by the app.
    #[must_use]
    pub fn nfo_undo_dir(&self) -> PathBuf {
        self.root.join("nfo-undo")
    }

    /// Create the data and cache folders if missing.
    ///
    /// # Errors
    /// Fails if a folder cannot be created.
    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        fs::create_dir_all(&self.root)?;
        fs::create_dir_all(self.cache_dir())
    }
}
