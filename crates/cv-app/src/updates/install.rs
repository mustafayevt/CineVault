//! Starting a verified installer.
//!
//! On Windows, a copy installed by the CineVault setup (an `uninstall.exe` sits beside the
//! executable) is updated in place: the downloaded setup runs in its passive mode with fixed
//! arguments, waits for the app to quit (the `NSIS_HOOK_PREINSTALL` hook), installs over the
//! running copy without the wizard pages or the old uninstaller, keeps the shortcuts and pins,
//! and starts the new version. Every other case (a portable copy, an MSI install, a build run
//! from a folder, macOS) hands the installer to the operating system as before, and says why.
//!
//! Only a [`VerifiedInstaller`] can be started, and only [`super::UpdateService::download`]
//! makes one: a file that matched the size and SHA-256 of a signed manifest, inside the update
//! service's own download folder. It is checked once more, under a lock that keeps it from
//! being changed, right before it starts.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::UpdateError;
use crate::data_location::PORTABLE_MARKER;

/// The uninstaller the CineVault setup writes beside the executable. Its presence marks a copy
/// the setup installed and can update in place.
pub const SETUP_UNINSTALLER: &str = "uninstall.exe";

/// The setup's own switches: `/P` passive (a progress window, no pages), `/R` start the app
/// again when the install finished, `/UPDATE` update mode (no uninstall of the old version, no
/// new shortcuts, so pins and shortcuts stay). `/D=` (added last) names the folder to update.
pub const SILENT_SETUP_SWITCHES: [&str; 3] = ["/P", "/R", "/UPDATE"];

/// An installer that matched the signed size and SHA-256. Made only by the update service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedInstaller {
    path: PathBuf,
    /// The update service's download folder, which the file must be inside.
    root: PathBuf,
    version: String,
    size: u64,
    sha256: String,
}

impl VerifiedInstaller {
    pub(super) fn new(
        path: PathBuf,
        root: PathBuf,
        version: String,
        size: u64,
        sha256: String,
    ) -> Self {
        Self {
            path,
            root,
            version,
            size,
            sha256: sha256.trim().to_ascii_lowercase(),
        }
    }

    /// Where the installer was saved.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The version the signed manifest named.
    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Checks the file again and returns it open. On Windows the handle allows others to read
    /// the file but not to write, rename or delete it, so what was checked is what starts while
    /// the handle is held.
    ///
    /// # Errors
    /// [`UpdateError::Io`] when the file is outside the download folder or cannot be read,
    /// [`UpdateError::WrongSize`] or [`UpdateError::ChecksumMismatch`] when it changed.
    pub fn reverify(&self) -> Result<File, UpdateError> {
        if !self.path.starts_with(&self.root) || !self.path.is_file() {
            return Err(UpdateError::Io(format!(
                "the installer {} is not in the update download folder",
                self.path.display()
            )));
        }
        let mut file = open_locked(&self.path)
            .map_err(|error| UpdateError::Io(format!("{}: {error}", self.path.display())))?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0_u8; 64 * 1024];
        let mut total = 0_u64;
        loop {
            let read = file
                .read(&mut buffer)
                .map_err(|error| UpdateError::Io(error.to_string()))?;
            if read == 0 {
                break;
            }
            total += read as u64;
            hasher.update(&buffer[..read]);
        }
        if total != self.size {
            return Err(UpdateError::WrongSize);
        }
        if super::service::hex(hasher.finalize().as_slice()) != self.sha256 {
            return Err(UpdateError::ChecksumMismatch);
        }
        Ok(file)
    }
}

#[cfg(windows)]
fn open_locked(path: &Path) -> std::io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    /// `FILE_SHARE_READ`: others may read (and run) the file, but not write or delete it.
    const FILE_SHARE_READ: u32 = 0x1;
    std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(path)
}

#[cfg(not(windows))]
fn open_locked(path: &Path) -> std::io::Result<File> {
    File::open(path)
}

/// Why an update is handed to the operating system instead of being installed in place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenReason {
    /// A portable copy (the `portable` marker): the setup would install a second, per-user copy.
    Portable,
    /// A copy the CineVault setup did not install (an MSI install, or a build run from a folder).
    NotSetupInstall,
    /// The download is not the Windows setup program.
    NotSetupFile,
    /// macOS: the disk image opens and the app is dragged to Applications.
    NotWindows,
}

impl OpenReason {
    /// A plain sentence for the user.
    #[must_use]
    pub fn message(self) -> &'static str {
        match self {
            Self::Portable => {
                "This is a portable copy, so CineVault does not install over it. The new setup was opened; to keep the copy portable, download the portable zip from the release page instead."
            }
            Self::NotSetupInstall => {
                "This copy was not installed with the CineVault setup (for example with the MSI package), so it cannot update itself. The installer was opened; follow its steps."
            }
            Self::NotSetupFile => {
                "This update is not the CineVault setup program, so it cannot install by itself. The installer was opened; follow its steps."
            }
            Self::NotWindows => {
                "The disk image was opened. Drag CineVault to Applications to replace this version."
            }
        }
    }

    /// A stable code for logs.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::Portable => "portable",
            Self::NotSetupInstall => "not_setup_install",
            Self::NotSetupFile => "not_setup_file",
            Self::NotWindows => "not_windows",
        }
    }
}

/// How an update is installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallMethod {
    /// Run the setup passively over the copy in `install_dir`, then quit; the setup starts the
    /// new version.
    SilentSetup { install_dir: PathBuf },
    /// Hand the installer to the operating system, which shows its own steps.
    OpenInstaller(OpenReason),
}

/// Picks how to install `installer` for the copy whose executable sits in `exe_dir`. `windows`
/// is whether this is Windows (a parameter, so both sides are tested everywhere).
#[must_use]
pub fn install_method(exe_dir: &Path, installer: &Path, windows: bool) -> InstallMethod {
    if !windows {
        return InstallMethod::OpenInstaller(OpenReason::NotWindows);
    }
    // Portable mode wins, as it does for the data folder.
    if exe_dir.join(PORTABLE_MARKER).is_file() {
        return InstallMethod::OpenInstaller(OpenReason::Portable);
    }
    let is_exe = installer
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"));
    if !is_exe {
        return InstallMethod::OpenInstaller(OpenReason::NotSetupFile);
    }
    if !exe_dir.join(SETUP_UNINSTALLER).is_file() {
        return InstallMethod::OpenInstaller(OpenReason::NotSetupInstall);
    }
    InstallMethod::SilentSetup {
        install_dir: exe_dir.to_path_buf(),
    }
}

/// What the close of the app did about an update.
#[derive(Debug)]
pub enum CloseUpdate {
    /// Nothing to do: the setting is off, no update was downloaded, or this copy cannot install
    /// itself (a portable copy, an MSI install, a Mac).
    Nothing,
    /// The verified setup of this version was started; the app closes and the setup takes over.
    Started { version: String },
    /// The setup could not be started or the file changed: the app closes as usual and the
    /// update stays on offer.
    Failed(UpdateError),
}

/// Starts the downloaded update when the app closes (spec `app-shell` "Install updates when the
/// app closes"): only with the setting on, only a verified installer, and only for a copy the
/// setup installed. `exe_dir` is the folder of the running executable; `start` runs the setup
/// (`start_silent_setup` in the app; a fake in tests).
pub fn install_on_close(
    service: &super::UpdateService,
    exe_dir: &Path,
    windows: bool,
    start: &dyn Fn(&VerifiedInstaller, &Path) -> Result<(), UpdateError>,
) -> CloseUpdate {
    let Some(installer) = service.installer_for_close() else {
        return CloseUpdate::Nothing;
    };
    match install_method(exe_dir, installer.path(), windows) {
        InstallMethod::SilentSetup { install_dir } => match start(&installer, &install_dir) {
            Ok(()) => CloseUpdate::Started {
                version: installer.version().to_owned(),
            },
            Err(error) => CloseUpdate::Failed(error),
        },
        InstallMethod::OpenInstaller(_) => CloseUpdate::Nothing,
    }
}

/// The setup's command line after the program: the fixed switches, then `/D=<folder>`, which
/// the setup wants last and without quotes (it takes the rest of the line, spaces included).
/// Nothing in it comes from the manifest.
#[must_use]
pub fn silent_setup_arguments(install_dir: &Path) -> Vec<String> {
    let mut arguments: Vec<String> = SILENT_SETUP_SWITCHES
        .iter()
        .map(|switch| (*switch).to_owned())
        .collect();
    arguments.push(format!("/D={}", install_dir.display()));
    arguments
}

/// Starts the verified setup passively over `install_dir`. The caller quits the app next; the
/// setup waits for it to exit, installs, and starts the new version.
///
/// # Errors
/// The file changed since it was verified, or the program could not be started.
pub fn start_silent_setup(
    installer: &VerifiedInstaller,
    install_dir: &Path,
) -> Result<(), UpdateError> {
    // Held until the setup has started, so the file cannot be swapped in between.
    let _locked = installer.reverify()?;
    let arguments = silent_setup_arguments(install_dir);
    tracing::info!(
        installer = %installer.path().display(),
        version = installer.version(),
        arguments = ?arguments,
        "starting the update setup"
    );
    spawn(installer.path(), &arguments).map_err(|error| {
        tracing::warn!(%error, "the update setup could not be started");
        UpdateError::Launch(error.to_string())
    })
}

#[cfg(windows)]
fn spawn(program: &Path, arguments: &[String]) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    let mut command = std::process::Command::new(program);
    // Raw, so `/D=` reaches the setup unquoted even when the folder has spaces.
    command.raw_arg(arguments.join(" "));
    command.spawn().map(drop)
}

#[cfg(not(windows))]
fn spawn(program: &Path, arguments: &[String]) -> std::io::Result<()> {
    std::process::Command::new(program)
        .args(arguments)
        .spawn()
        .map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verified(dir: &Path, bytes: &[u8]) -> VerifiedInstaller {
        let path = dir.join("1.2.0").join("CineVault-setup.exe");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
        std::fs::write(&path, bytes).expect("write");
        let sha = super::super::service::hex(Sha256::digest(bytes).as_slice());
        VerifiedInstaller::new(
            path,
            dir.to_path_buf(),
            "1.2.0".to_owned(),
            bytes.len() as u64,
            sha.to_ascii_uppercase(),
        )
    }

    #[test]
    fn the_arguments_are_fixed_and_name_the_running_copy_last() {
        let arguments = silent_setup_arguments(Path::new(r"C:\Users\A B\AppData\Local\CineVault"));
        assert_eq!(
            arguments,
            [
                "/P",
                "/R",
                "/UPDATE",
                r"/D=C:\Users\A B\AppData\Local\CineVault"
            ]
        );
    }

    #[test]
    fn a_setup_install_updates_in_place() {
        let dir = tempfile::tempdir().expect("dir");
        std::fs::write(dir.path().join(SETUP_UNINSTALLER), b"").expect("uninstaller");
        let installer = Path::new("CineVault-1.2.0-setup.exe");
        assert_eq!(
            install_method(dir.path(), installer, true),
            InstallMethod::SilentSetup {
                install_dir: dir.path().to_path_buf()
            }
        );
        // An upper-case extension is the same program.
        assert!(matches!(
            install_method(dir.path(), Path::new("SETUP.EXE"), true),
            InstallMethod::SilentSetup { .. }
        ));
    }

    #[test]
    fn a_portable_copy_never_installs_itself() {
        let dir = tempfile::tempdir().expect("dir");
        std::fs::write(dir.path().join(SETUP_UNINSTALLER), b"").expect("uninstaller");
        std::fs::write(dir.path().join(PORTABLE_MARKER), b"").expect("marker");
        assert_eq!(
            install_method(dir.path(), Path::new("setup.exe"), true),
            InstallMethod::OpenInstaller(OpenReason::Portable)
        );
        assert!(OpenReason::Portable.message().contains("portable copy"));
    }

    #[test]
    fn other_copies_and_files_open_the_installer() {
        let dir = tempfile::tempdir().expect("dir");
        // No uninstaller beside the executable: an MSI install or a build run from a folder.
        assert_eq!(
            install_method(dir.path(), Path::new("setup.exe"), true),
            InstallMethod::OpenInstaller(OpenReason::NotSetupInstall)
        );
        std::fs::write(dir.path().join(SETUP_UNINSTALLER), b"").expect("uninstaller");
        assert_eq!(
            install_method(dir.path(), Path::new("CineVault.msi"), true),
            InstallMethod::OpenInstaller(OpenReason::NotSetupFile)
        );
        assert_eq!(
            install_method(dir.path(), Path::new("CineVault.dmg"), false),
            InstallMethod::OpenInstaller(OpenReason::NotWindows)
        );
    }

    #[test]
    fn a_verified_installer_is_checked_again_before_it_starts() {
        let dir = tempfile::tempdir().expect("dir");
        let installer = verified(dir.path(), b"the real installer");
        drop(installer.reverify().expect("still good"));

        // Same size, other bytes.
        std::fs::write(installer.path(), b"the evil installer").expect("swap");
        assert!(matches!(
            installer.reverify(),
            Err(UpdateError::ChecksumMismatch)
        ));
        assert!(matches!(
            start_silent_setup(&installer, dir.path()),
            Err(UpdateError::ChecksumMismatch)
        ));
        std::fs::write(installer.path(), b"short").expect("swap");
        assert!(matches!(installer.reverify(), Err(UpdateError::WrongSize)));
        std::fs::remove_file(installer.path()).expect("remove");
        assert!(matches!(installer.reverify(), Err(UpdateError::Io(_))));
    }

    #[test]
    fn an_installer_outside_the_download_folder_is_refused() {
        let dir = tempfile::tempdir().expect("dir");
        let other = tempfile::tempdir().expect("other");
        let good = verified(dir.path(), b"bytes");
        let moved = VerifiedInstaller::new(
            good.path().to_path_buf(),
            other.path().to_path_buf(),
            "1.2.0".to_owned(),
            5,
            good.sha256.clone(),
        );
        assert!(matches!(moved.reverify(), Err(UpdateError::Io(_))));
    }

    #[cfg(windows)]
    #[test]
    fn the_checked_file_cannot_be_changed_while_it_is_held() {
        let dir = tempfile::tempdir().expect("dir");
        let installer = verified(dir.path(), b"the real installer");
        let held = installer.reverify().expect("good");
        assert!(std::fs::write(installer.path(), b"the evil installer").is_err());
        assert!(std::fs::remove_file(installer.path()).is_err());
        // Others may still read it, as the system does to start it.
        assert_eq!(
            std::fs::read(installer.path()).expect("read"),
            b"the real installer"
        );
        drop(held);
    }
}
