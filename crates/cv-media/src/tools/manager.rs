//! Finds tools, installs them on demand and reports their state for the Settings page.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError, RwLock, RwLockWriteGuard};
use std::time::{Duration, Instant, SystemTime};

use super::ToolError;
use super::download::Cancel;
use super::install::{self, InstallProgress, InstallRecord};
use super::manifest::{Manifest, Package, Platform, PlatformSpec, ToolId};
use super::version::ask_version;

/// Where a tool came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolSource {
    /// A path the user set.
    Override,
    /// A verified download in the app's tools folder.
    Managed,
}

/// A tool that can be started or loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTool {
    pub id: ToolId,
    pub path: PathBuf,
    pub source: ToolSource,
}

/// How a tool stands, for the Settings page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolState {
    Ready,
    /// Nothing downloaded and no path set; the app offers to download it.
    NotInstalled,
    /// The user's path no longer points at a file.
    OverrideMissing,
    /// A download exists but files are missing or have the wrong size; download it again.
    Damaged,
    /// No downloads exist for this computer; only a path can be set.
    Unsupported,
}

/// Everything the Settings page shows about one tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolStatus {
    pub id: ToolId,
    /// The name of the download that provides it.
    pub name: String,
    pub state: ToolState,
    pub source: Option<ToolSource>,
    /// The file in use.
    pub path: Option<PathBuf>,
    /// The version in use, when it is known.
    pub version: Option<String>,
    /// The path the user set, even when it is broken.
    pub override_path: Option<PathBuf>,
    /// The version a download would install.
    pub pinned_version: Option<String>,
    /// The size of that download in bytes.
    pub download_size: Option<u64>,
    /// True when a managed install is older than the pinned version.
    pub update_available: bool,
    /// A verified download is in the tools folder: what the tool falls back to when the path the
    /// user set is cleared.
    pub downloaded: bool,
    pub license: String,
    pub homepage: String,
}

enum Managed {
    Found { path: PathBuf, version: String },
    Damaged,
    Absent,
}

type FileKey = (u64, Option<SystemTime>);

/// Locates and installs the external tools.
#[derive(Debug)]
pub struct ToolsManager {
    root: PathBuf,
    manifest: Manifest,
    platform: Option<Platform>,
    overrides: RwLock<BTreeMap<ToolId, PathBuf>>,
    installing: Mutex<()>,
    asked: Mutex<HashMap<PathBuf, (FileKey, Option<String>)>>,
    loaded: Mutex<BTreeMap<ToolId, String>>,
    /// Packages whose download is running or was announced (the first-run setup downloads its
    /// tools one after the other), with the moment the promise runs out.
    coming: Mutex<BTreeMap<Package, Instant>>,
}

/// How long an announced download counts as coming before it starts.
const EXPECTED_FOR: Duration = Duration::from_secs(30 * 60);
/// How long a running download counts as coming at most.
const INSTALLING_FOR: Duration = Duration::from_secs(6 * 60 * 60);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl ToolsManager {
    /// A manager keeping downloads under `root`, using this build's manifest and this computer's
    /// platform.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self::with_manifest(root, Manifest::embedded(), Platform::current())
    }

    /// A manager with its own manifest and platform (tests and tooling).
    #[must_use]
    pub fn with_manifest(
        root: impl Into<PathBuf>,
        manifest: Manifest,
        platform: Option<Platform>,
    ) -> Self {
        Self {
            root: root.into(),
            manifest,
            platform,
            overrides: RwLock::new(BTreeMap::new()),
            installing: Mutex::new(()),
            asked: Mutex::new(HashMap::new()),
            loaded: Mutex::new(BTreeMap::new()),
            coming: Mutex::new(BTreeMap::new()),
        }
    }

    /// The folder downloads live in.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    // ---- user paths -------------------------------------------------------------------------

    /// Sets, or with `None` clears, the path the user chose for `tool`. A folder is accepted and
    /// means the tool's usual file name inside it. Returns the file that will be used.
    ///
    /// # Errors
    /// [`ToolError::OverrideInvalid`] when the path is not absolute or holds no such file.
    pub fn set_override(
        &self,
        tool: ToolId,
        path: Option<&Path>,
    ) -> Result<Option<PathBuf>, ToolError> {
        let Some(path) = path else {
            self.write_overrides().remove(&tool);
            return Ok(None);
        };
        let file = normalize_override(tool, path)?;
        self.write_overrides().insert(tool, file.clone());
        Ok(Some(file))
    }

    /// Checks that `path` (a file, or the folder that holds it) really is `tool` before the user's
    /// choice is saved, and returns the file. Its name must be the tool's, and a program must
    /// answer its version flag the way the tool does; the answer is remembered for the status.
    /// A library is checked when it is loaded instead, since it cannot be run.
    ///
    /// # Errors
    /// [`ToolError::OverrideInvalid`] with the reason.
    pub fn check_override(&self, tool: ToolId, path: &Path) -> Result<PathBuf, ToolError> {
        let file = normalize_override(tool, path)?;
        if tool.is_library() {
            return Ok(file);
        }
        let version = ask_version(tool, &file);
        if let Ok(metadata) = fs::metadata(&file) {
            let key = (metadata.len(), metadata.modified().ok());
            lock(&self.asked).insert(file.clone(), (key, version.clone()));
        }
        if version.is_none() {
            return Err(ToolError::OverrideInvalid {
                tool,
                path: file,
                reason: format!("it did not answer the way {tool} does when asked its version"),
            });
        }
        Ok(file)
    }

    /// Restores a stored path without checking that it still exists (it is checked when used, so
    /// a drive that is not mounted yet does not lose the setting).
    pub fn restore_override(&self, tool: ToolId, path: impl Into<PathBuf>) {
        self.write_overrides().insert(tool, path.into());
    }

    /// The path the user set for `tool`.
    #[must_use]
    pub fn override_for(&self, tool: ToolId) -> Option<PathBuf> {
        self.overrides
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&tool)
            .cloned()
    }

    fn write_overrides(&self) -> RwLockWriteGuard<'_, BTreeMap<ToolId, PathBuf>> {
        self.overrides
            .write()
            .unwrap_or_else(PoisonError::into_inner)
    }

    // ---- finding ----------------------------------------------------------------------------

    /// Finds `tool`: the user's path first, then the verified download.
    ///
    /// # Errors
    /// [`ToolError::OverrideMissing`] when the user's path is gone (it is never silently replaced
    /// by a download), [`ToolError::NotInstalled`] when there is nothing yet, so the caller can
    /// offer [`Self::install`].
    pub fn resolve(&self, tool: ToolId) -> Result<ResolvedTool, ToolError> {
        if let Some(path) = self.override_for(tool) {
            return if path.is_file() {
                Ok(ResolvedTool {
                    id: tool,
                    path,
                    source: ToolSource::Override,
                })
            } else {
                Err(ToolError::OverrideMissing { tool, path })
            };
        }
        match self.find_managed(tool) {
            Managed::Found { path, .. } => Ok(ResolvedTool {
                id: tool,
                path,
                source: ToolSource::Managed,
            }),
            Managed::Damaged | Managed::Absent => Err(ToolError::NotInstalled(tool)),
        }
    }

    fn find_managed(&self, tool: ToolId) -> Managed {
        let package = tool.package();
        let pinned = self
            .platform
            .and_then(|platform| self.manifest.platform_spec(package, platform))
            .map(|spec| spec.version.clone());
        let Ok(entries) = fs::read_dir(self.root.join(package.as_str())) else {
            return Managed::Absent;
        };
        let mut versions: Vec<String> = entries
            .flatten()
            .filter(|entry| entry.path().is_dir())
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect();
        // The pinned version first, then the others from newest-looking to oldest.
        versions.sort_by(|a, b| b.cmp(a));
        if let Some(pinned) = &pinned
            && let Some(at) = versions.iter().position(|v| v == pinned)
        {
            let version = versions.remove(at);
            versions.insert(0, version);
        }
        let mut damaged = false;
        for version in versions {
            let dir = install::install_dir(&self.root, package, &version);
            match check_install(&dir, tool) {
                Some(path) => return Managed::Found { path, version },
                None => damaged = true,
            }
        }
        if damaged {
            Managed::Damaged
        } else {
            Managed::Absent
        }
    }

    // ---- versions ---------------------------------------------------------------------------

    /// Records the version a loaded library reported about itself, for libraries whose version
    /// cannot be read from outside (a user-chosen MediaInfo library).
    pub fn report_loaded_version(&self, tool: ToolId, version: impl Into<String>) {
        lock(&self.loaded).insert(tool, version.into());
    }

    /// The version of a resolved tool: known from the install record for downloads, asked of the
    /// program for a user's path (and remembered until the file changes).
    #[must_use]
    pub fn version_of(&self, tool: &ResolvedTool) -> Option<String> {
        match tool.source {
            ToolSource::Managed => match self.find_managed(tool.id) {
                Managed::Found { version, .. } => Some(version),
                _ => None,
            },
            ToolSource::Override => {
                if tool.id.is_library() {
                    return lock(&self.loaded).get(&tool.id).cloned();
                }
                let metadata = fs::metadata(&tool.path).ok()?;
                let key = (metadata.len(), metadata.modified().ok());
                if let Some((cached, version)) = lock(&self.asked).get(&tool.path)
                    && *cached == key
                {
                    return version.clone();
                }
                let version = ask_version(tool.id, &tool.path);
                lock(&self.asked).insert(tool.path.clone(), (key, version.clone()));
                version
            }
        }
    }

    // ---- state for Settings -----------------------------------------------------------------

    /// The state of every tool the Tools page lists. Asking a user's program for its version starts it briefly, so
    /// call this off the UI thread.
    #[must_use]
    pub fn status(&self) -> Vec<ToolStatus> {
        ToolId::LISTED
            .into_iter()
            .map(|tool| self.status_of(tool))
            .collect()
    }

    /// The state of one tool, including one the Tools page does not list (the indexer service,
    /// whose card reads this to show a damaged install).
    #[must_use]
    pub fn status_of(&self, tool: ToolId) -> ToolStatus {
        let package = tool.package();
        let package_spec = self.manifest.package(package);
        let platform_spec = self
            .platform
            .and_then(|platform| self.manifest.platform_spec(package, platform));
        let pinned_version = platform_spec.map(|spec| spec.version.clone());
        let mut status = ToolStatus {
            id: tool,
            name: package_spec.map_or_else(|| package.to_string(), |p| p.display.clone()),
            state: ToolState::NotInstalled,
            source: None,
            path: None,
            version: None,
            override_path: self.override_for(tool),
            pinned_version: pinned_version.clone(),
            download_size: platform_spec.map(|spec| spec.download_size()),
            update_available: false,
            downloaded: matches!(self.find_managed(tool), Managed::Found { .. }),
            license: package_spec.map(|p| p.license.clone()).unwrap_or_default(),
            homepage: package_spec.map(|p| p.homepage.clone()).unwrap_or_default(),
        };
        if platform_spec.is_none() {
            status.state = ToolState::Unsupported;
        }
        match self.resolve(tool) {
            Ok(resolved) => {
                status.state = ToolState::Ready;
                status.source = Some(resolved.source);
                status.version = self.version_of(&resolved);
                status.update_available = resolved.source == ToolSource::Managed
                    && status.version.is_some()
                    && status.version != pinned_version;
                status.path = Some(resolved.path);
            }
            Err(ToolError::OverrideMissing { path, .. }) => {
                status.state = ToolState::OverrideMissing;
                status.source = Some(ToolSource::Override);
                status.path = Some(path);
            }
            Err(_) => {
                if matches!(self.find_managed(tool), Managed::Damaged) {
                    status.state = ToolState::Damaged;
                }
            }
        }
        status
    }

    // ---- installing -------------------------------------------------------------------------

    /// Notes that `package` is about to be downloaded (a queue of downloads that runs one after
    /// the other), so work that needs it waits for it instead of reporting it missing. The note
    /// ends when its install finishes, or after half an hour if it never starts.
    pub fn expect(&self, package: Package) {
        let until = Instant::now() + EXPECTED_FOR;
        let mut coming = lock(&self.coming);
        let entry = coming.entry(package).or_insert(until);
        *entry = (*entry).max(until);
    }

    /// Whether `package` is being downloaded, or a download of it was announced with
    /// [`Self::expect`] and has not finished yet.
    #[must_use]
    pub fn is_coming(&self, package: Package) -> bool {
        let mut coming = lock(&self.coming);
        let now = Instant::now();
        coming.retain(|_, until| *until > now);
        coming.contains_key(&package)
    }

    /// Marks `package` as coming while `work` runs, and as settled afterwards, however it went.
    fn while_installing<T>(&self, package: Package, work: impl FnOnce() -> T) -> T {
        lock(&self.coming).insert(package, Instant::now() + INSTALLING_FOR);
        let result = work();
        lock(&self.coming).remove(&package);
        result
    }

    /// Downloads `package` into the tools folder after checking it against the pinned SHA-256.
    /// Installing a package that is already current downloads it again, which repairs a damaged
    /// install. Only one install runs at a time; a second call waits.
    ///
    /// # Errors
    /// [`ToolError::UnsupportedPlatform`], [`ToolError::ChecksumMismatch`] for a corrupted
    /// download, [`ToolError::Cancelled`], or a network or file error. The previous install is
    /// kept on any failure.
    pub fn install(
        &self,
        package: Package,
        cancel: &Cancel,
        progress: &mut dyn FnMut(InstallProgress),
    ) -> Result<(), ToolError> {
        let platform = self
            .platform
            .ok_or(ToolError::UnsupportedPlatform(package))?;
        let spec = self
            .manifest
            .platform_spec(package, platform)
            .ok_or(ToolError::UnsupportedPlatform(package))?;
        self.while_installing(package, || {
            let _guard = lock(&self.installing);
            fs::create_dir_all(&self.root)
                .map_err(|error| ToolError::io("Could not create the tools folder", error))?;
            install::install_package(&self.root, package, platform, spec, cancel, progress)
        })
    }

    /// What this build pins for `package` on this computer, if anything.
    #[must_use]
    pub fn pinned_spec(&self, package: Package) -> Option<PlatformSpec> {
        self.platform
            .and_then(|platform| self.manifest.platform_spec(package, platform))
            .cloned()
    }

    /// Like [`Self::install`] for a download that is not the one this build pins: the same staging,
    /// checksum and unpack steps, with `spec` describing the download. The caller is responsible
    /// for where `spec` comes from; the only use is a newer indexer service whose published digest
    /// was read from its release. The spec is checked by the same rules as a manifest.
    ///
    /// # Errors
    /// [`ToolError::Manifest`] when `spec` breaks a manifest rule, otherwise as [`Self::install`].
    pub fn install_spec(
        &self,
        package: Package,
        spec: &PlatformSpec,
        cancel: &Cancel,
        progress: &mut dyn FnMut(InstallProgress),
    ) -> Result<(), ToolError> {
        let platform = self
            .platform
            .ok_or(ToolError::UnsupportedPlatform(package))?;
        spec.validate(package)?;
        self.while_installing(package, || {
            let _guard = lock(&self.installing);
            fs::create_dir_all(&self.root)
                .map_err(|error| ToolError::io("Could not create the tools folder", error))?;
            install::install_package(&self.root, package, platform, spec, cancel, progress)
        })
    }
}

/// The file for `tool` inside a finished install, if every file the install lists is present
/// with its recorded size.
fn check_install(dir: &Path, tool: ToolId) -> Option<PathBuf> {
    let record: InstallRecord = install::read_record(dir)?;
    for file in &record.files {
        let size = fs::metadata(dir.join(&file.name)).ok()?.len();
        if size != file.size {
            return None;
        }
    }
    let file = record
        .files
        .iter()
        .find(|file| file.tool.as_deref() == Some(tool.as_str()))?;
    Some(dir.join(&file.name))
}

fn normalize_override(tool: ToolId, path: &Path) -> Result<PathBuf, ToolError> {
    let invalid = |reason: &str| ToolError::OverrideInvalid {
        tool,
        path: path.to_path_buf(),
        reason: reason.to_owned(),
    };
    if !path.is_absolute() {
        return Err(invalid(
            "give the full path, starting from the drive or root folder",
        ));
    }
    let file = if path.is_dir() {
        path.join(tool.file_name())
    } else {
        path.to_path_buf()
    };
    if file.is_file() {
        let named = file
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.to_lowercase().contains(tool.program_name()));
        if named {
            Ok(file)
        } else {
            Err(ToolError::OverrideInvalid {
                tool,
                path: file,
                reason: format!(
                    "this is not {tool}. Choose the file {}, or the folder that holds it",
                    tool.file_name()
                ),
            })
        }
    } else if path.is_dir() {
        Err(invalid(&format!("the folder has no {}", tool.file_name())))
    } else {
        Err(invalid("there is no file at that path"))
    }
}

/// A folder for tools fetched while developing, outside the app's data folder and ignored by git:
/// `<repo>/.dev-tools`, or the folder in the `CINEVAULT_TOOLS_DIR` environment variable.
///
/// Fill it with `bun run tools:fetch`; media tests then use [`ToolsManager::new`] on it.
#[must_use]
pub fn dev_cache_dir() -> PathBuf {
    std::env::var_os("CINEVAULT_TOOLS_DIR").map_or_else(
        || {
            let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
            // The repository root is two levels above `crates/cv-media`.
            crate_dir
                .ancestors()
                .nth(2)
                .unwrap_or(crate_dir)
                .join(".dev-tools")
        },
        PathBuf::from,
    )
}
