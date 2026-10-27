//! The install pipeline: download and verify into a staging folder, unpack, then move the
//! finished folder into place in one step.
//!
//! Layout under the tools folder:
//!
//! ```text
//! <tools>/.staging/<package>-<n>/   work in progress, removed on failure and at the next install
//! <tools>/<package>/<version>/      a finished install: the files and install.json
//! ```
//!
//! Nothing is unpacked before its download matched the pinned SHA-256, and a failure at any step
//! leaves the previous install untouched.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::ToolError;
use super::archive;
use super::download::{Cancel, download_verified};
use super::manifest::{Package, Platform, PlatformSpec};

/// Name of the record written into every finished install.
pub(crate) const RECORD_FILE: &str = "install.json";
const STAGING_DIR: &str = ".staging";

/// Which step an install is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallPhase {
    Downloading,
    Extracting,
    Finishing,
}

/// A progress report during [`super::ToolsManager::install`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallProgress {
    pub package: Package,
    pub phase: InstallPhase,
    /// Bytes downloaded so far across all files of the package.
    pub downloaded: u64,
    /// Bytes to download in all.
    pub total: u64,
}

/// What an install left behind; read back to check the files are still there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct InstallRecord {
    pub package: String,
    pub version: String,
    pub platform: String,
    pub files: Vec<InstalledFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct InstalledFile {
    /// The tool this file is, or `None` for a licence text.
    pub tool: Option<String>,
    pub name: String,
    pub size: u64,
}

pub(crate) fn install_dir(root: &Path, package: Package, version: &str) -> PathBuf {
    root.join(package.as_str()).join(version)
}

/// Reads the record of the install in `dir`, if it holds a readable one.
pub(crate) fn read_record(dir: &Path) -> Option<InstallRecord> {
    let text = fs::read_to_string(dir.join(RECORD_FILE)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Removes a folder when dropped, unless it was kept.
struct Cleanup(Option<PathBuf>);

impl Cleanup {
    fn keep(&mut self) {
        self.0 = None;
    }
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = fs::remove_dir_all(path);
        }
    }
}

/// Downloads, verifies and installs `package` as described by `spec`.
///
/// # Errors
/// Any [`ToolError`] from the steps; the previous install is never changed on failure.
pub(crate) fn install_package(
    root: &Path,
    package: Package,
    platform: Platform,
    spec: &PlatformSpec,
    cancel: &Cancel,
    progress: &mut dyn FnMut(InstallProgress),
) -> Result<(), ToolError> {
    let staging_root = root.join(STAGING_DIR);
    // A crashed earlier install may have left work behind; installs are serialised by the caller.
    let _ = fs::remove_dir_all(&staging_root);
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let staging = staging_root.join(format!("{package}-{nonce}"));
    let mut cleanup = Cleanup(Some(staging_root.clone()));
    let unpacked = staging.join("files");
    fs::create_dir_all(&unpacked)
        .map_err(|error| ToolError::io("Could not prepare the tools folder", error))?;

    let total = spec.download_size();
    let report = |phase, downloaded| InstallProgress {
        package,
        phase,
        downloaded,
        total,
    };
    progress(report(InstallPhase::Downloading, 0));

    let mut done = 0_u64;
    let mut written: Vec<archive::Extracted> = Vec::new();
    for (index, artifact) in spec.artifacts.iter().enumerate() {
        let part = staging.join(format!("download-{index}.part"));
        let base = done;
        download_verified(
            &artifact.url,
            &artifact.sha256,
            artifact.size,
            &part,
            cancel,
            &mut |bytes| progress(report(InstallPhase::Downloading, base + bytes)),
        )?;
        done += artifact.size;
        progress(report(InstallPhase::Extracting, done));
        let extracted = archive::extract(artifact, &part, &unpacked);
        let _ = fs::remove_file(&part);
        written.extend(extracted?);
    }
    if cancel.is_cancelled() {
        return Err(ToolError::Cancelled);
    }
    progress(report(InstallPhase::Finishing, done));

    let record = write_record(&unpacked, package, platform, spec, &written)?;
    set_executable(&unpacked, &record);

    let version_dir = install_dir(root, package, &spec.version);
    if let Some(parent) = version_dir.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| ToolError::io("Could not prepare the tools folder", error))?;
    }
    // Keep the old install until the new one is in place, so a failed move loses nothing.
    let previous = staging.join("previous");
    let moved_aside = version_dir.exists() && fs::rename(&version_dir, &previous).is_ok();
    if let Err(error) = rename_with_retry(&unpacked, &version_dir) {
        if moved_aside {
            let _ = fs::rename(&previous, &version_dir);
        }
        return Err(error);
    }
    cleanup.keep();
    let _ = fs::remove_dir_all(&staging_root);
    prune_other_versions(root, package, &spec.version);
    tracing::info!(%package, version = %spec.version, "installed external tool");
    Ok(())
}

fn write_record(
    dir: &Path,
    package: Package,
    platform: Platform,
    spec: &PlatformSpec,
    written: &[archive::Extracted],
) -> Result<InstallRecord, ToolError> {
    let mut files = Vec::new();
    for file in written {
        let size = fs::metadata(dir.join(&file.name))
            .map_err(|error| ToolError::io(format!("Could not read {}", file.name), error))?
            .len();
        files.push(InstalledFile {
            tool: file.tool.clone(),
            name: file.name.clone(),
            size,
        });
    }
    let record = InstallRecord {
        package: package.as_str().to_owned(),
        version: spec.version.clone(),
        platform: platform.key().to_owned(),
        files,
    };
    let json = serde_json::to_string_pretty(&record)
        .map_err(|error| ToolError::Manifest(error.to_string()))?;
    fs::write(dir.join(RECORD_FILE), json)
        .map_err(|error| ToolError::io("Could not write the install record", error))?;
    Ok(record)
}

#[cfg(unix)]
fn set_executable(dir: &Path, record: &InstallRecord) {
    use std::os::unix::fs::PermissionsExt;
    for file in record.files.iter().filter(|file| file.tool.is_some()) {
        let _ = fs::set_permissions(dir.join(&file.name), fs::Permissions::from_mode(0o755));
    }
}

#[cfg(not(unix))]
fn set_executable(_dir: &Path, _record: &InstallRecord) {}

/// Windows virus scanners briefly hold freshly written files, which makes a rename fail.
fn rename_with_retry(from: &Path, to: &Path) -> Result<(), ToolError> {
    let mut attempt = 0;
    loop {
        match fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(error) if attempt >= 20 => {
                return Err(ToolError::io(
                    "Could not move the finished install into place",
                    error,
                ));
            }
            Err(_) => {
                attempt += 1;
                std::thread::sleep(Duration::from_millis(150));
            }
        }
    }
}

/// Deletes the older versions of a package. They are the app's own downloads, not user data.
fn prune_other_versions(root: &Path, package: Package, keep: &str) {
    let Ok(entries) = fs::read_dir(root.join(package.as_str())) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name() != keep {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}
