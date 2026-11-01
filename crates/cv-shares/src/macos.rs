//! macOS: `mount_smbfs` into a folder CineVault owns, `~/CineVault Shares/<server>/<share>`.
//!
//! `mount_smbfs` reads the password from its terminal, not from a pipe, and a URL with
//! `user:password@` would show it in the process list. So the tool runs under the system's
//! `/usr/bin/script`, which gives it a pseudo-terminal of its own and relays CineVault's pipe to
//! it; CineVault waits for the `Password:` prompt (the tool has turned echo off by then) and only
//! then writes the password. No terminal code of CineVault's own, and no `unsafe`.
//!
//! The mount folder is made with mode `0700`, outside `~/Library` and the data folder (which the
//! folder rules refuse). A connect counts only when the folder is a mount point afterwards.
//! Disconnecting is `umount`; an empty mount folder left behind is removed with `rmdir`, never
//! recursively, and only once it is not a mount point.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::address::{ShareAddress, percent_encode};
use crate::connector::{Cancel, Connected, Login, ShareConnector, Support, undo_interrupted};
use crate::error::{ShareError, ShareErrorKind};
use crate::process::{self, Session, Spawner, ToolCommand};

use crate::MAC_MOUNT_FOLDER as MOUNT_FOLDER;

const SCRIPT: &str = "/usr/bin/script";
const MOUNT_SMBFS: &str = "/sbin/mount_smbfs";
const UMOUNT: &str = "/sbin/umount";

const CONNECT_LIMIT: Duration = Duration::from_secs(60);
const UNMOUNT_LIMIT: Duration = Duration::from_secs(30);

/// Tells whether a folder is a mount point.
pub(crate) type MountCheck = Arc<dyn Fn(&Path) -> bool + Send + Sync>;

/// Mounts shares with `mount_smbfs`.
pub struct MacConnector {
    spawner: Arc<dyn Spawner>,
    base: PathBuf,
    is_mount_point: MountCheck,
}

impl MacConnector {
    /// The connector of this computer, mounting below `~/CineVault Shares`.
    #[cfg(target_os = "macos")]
    #[must_use]
    pub fn new() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
        Self::with_parts(
            Arc::new(crate::process::SystemSpawner),
            home.join(MOUNT_FOLDER),
            Arc::new(cv_fs::paths::is_mount_point),
        )
    }

    pub(crate) fn with_parts(
        spawner: Arc<dyn Spawner>,
        base: PathBuf,
        is_mount_point: MountCheck,
    ) -> Self {
        Self {
            spawner,
            base,
            is_mount_point,
        }
    }

    /// `~/CineVault Shares/<server>/<share>`. The names were checked, so neither is `.`, `..` or
    /// holds a separator.
    fn mount_folder(&self, share: &ShareAddress) -> PathBuf {
        self.base.join(share.server()).join(share.share())
    }

    /// The command that mounts `share` at `folder` on a pseudo-terminal.
    fn mount_command(share: &ShareAddress, login: &Login, folder: &Path) -> ToolCommand {
        let user = match login.domain() {
            Some(domain) => format!(
                "{};{}",
                percent_encode(domain),
                percent_encode(login.name())
            ),
            None => percent_encode(login.name()),
        };
        let url = format!(
            "//{user}@{}/{}",
            share.server(),
            percent_encode(share.share())
        );
        ToolCommand::new(SCRIPT)
            .arg("-q")
            .arg("/dev/null")
            .arg(MOUNT_SMBFS)
            .arg(url)
            .arg(folder.as_os_str())
            .env("LC_ALL", "C")
    }

    /// Mounts `share` at `folder` with `mount_smbfs`, answering its password prompt.
    fn mount(
        &self,
        share: &ShareAddress,
        login: &Login,
        folder: &Path,
        cancel: &Cancel,
    ) -> Result<PathBuf, ShareError> {
        make_private_folder(folder).map_err(|error| {
            ShareError::new(
                ShareErrorKind::Other,
                format!("{} could not be made: {error}", folder.display()),
            )
        })?;
        let password = login.password();
        let command = Self::mount_command(share, login, folder);
        let mut session = Session::start(self.spawner.as_ref(), &command, cancel, CONNECT_LIMIT)?;
        let mut answered = false;
        while let Some(prompt) = session.prompt()? {
            if answered || !prompt.to_ascii_lowercase().contains("password") {
                // A second question means the password was refused; anything else is a question
                // this connector does not know, which never gets the password.
                session.stop();
                let error = if answered {
                    ShareError::new(ShareErrorKind::WrongLogin, "the password was asked again")
                } else {
                    ShareError::new(
                        ShareErrorKind::Other,
                        format!("mount_smbfs asked: {}", password.scrub(&prompt)),
                    )
                };
                drop(session);
                self.remove_empty_folder(share);
                return Err(error);
            }
            session.answer(password.expose())?;
            answered = true;
        }
        let finished = session.finish()?;
        if (self.is_mount_point)(folder) {
            return Ok(folder.to_path_buf());
        }
        self.remove_empty_folder(share);
        let output = password.scrub(&format!("{}\n{}", finished.stdout, finished.stderr));
        Err(mount_error(&output).with_code(i64::from(finished.code)))
    }

    /// Removes the empty mount folder of `share` and then its empty server folder, once the share
    /// is not mounted there. Never recursive: a folder with anything in it stays.
    fn remove_empty_folder(&self, share: &ShareAddress) {
        let folder = self.mount_folder(share);
        if !(self.is_mount_point)(&folder)
            && std::fs::remove_dir(&folder).is_ok()
            && let Some(server) = folder.parent()
        {
            let _ = std::fs::remove_dir(server);
        }
    }
}

impl ShareConnector for MacConnector {
    fn support(&self) -> Support {
        Support::Supported
    }

    fn connect(
        &self,
        share: &ShareAddress,
        login: &Login,
        cancel: &Cancel,
    ) -> Result<Connected, ShareError> {
        let folder = self.mount_folder(share);
        if (self.is_mount_point)(&folder) {
            return Ok(Connected {
                root: folder,
                made: false,
            });
        }
        let root = self
            .mount(share, login, &folder, cancel)
            .map_err(|error| undo_interrupted(self, share, error))?;
        Ok(Connected { root, made: true })
    }

    fn is_connected(&self, share: &ShareAddress, _cancel: &Cancel) -> Result<bool, ShareError> {
        Ok((self.is_mount_point)(&self.mount_folder(share)))
    }

    fn disconnect(&self, share: &ShareAddress, cancel: &Cancel) -> Result<(), ShareError> {
        let folder = self.mount_folder(share);
        if (self.is_mount_point)(&folder) {
            let command = ToolCommand::new(UMOUNT)
                .arg(folder.as_os_str())
                .env("LC_ALL", "C");
            let finished =
                process::run(self.spawner.as_ref(), &command, None, cancel, UNMOUNT_LIMIT)?;
            if (self.is_mount_point)(&folder) {
                let output = format!("{}\n{}", finished.stdout, finished.stderr);
                let kind = if output.to_ascii_lowercase().contains("busy") {
                    ShareErrorKind::InUse
                } else {
                    ShareErrorKind::Other
                };
                return Err(ShareError::new(kind, process::last_line(&output))
                    .with_code(i64::from(finished.code)));
            }
        }
        self.remove_empty_folder(share);
        Ok(())
    }

    fn local_root(
        &self,
        share: &ShareAddress,
        _cancel: &Cancel,
    ) -> Result<Option<PathBuf>, ShareError> {
        Ok(Some(self.mount_folder(share)))
    }
}

/// Makes `folder` and the folders above it that are missing, readable by the user only.
fn make_private_folder(folder: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(folder)
}

/// The error a failed `mount_smbfs` reported, from its last words.
fn mount_error(output: &str) -> ShareError {
    let line = output
        .lines()
        .map(str::trim)
        .rfind(|line| line.starts_with("mount_smbfs:"))
        .map_or_else(|| process::last_line(output), str::to_owned);
    let lower = line.to_ascii_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| lower.contains(w));
    let kind = if has(&[
        "authentication error",
        "authentication failed",
        "logon failure",
    ]) {
        ShareErrorKind::WrongLogin
    } else if has(&[
        "no such file or directory",
        "share not found",
        "bad network name",
    ]) {
        ShareErrorKind::ShareNotFound
    } else if has(&[
        "server connection failed",
        "no route to host",
        "host is down",
        "connection refused",
        "operation timed out",
        "network is unreachable",
        "unknown host",
        "could not resolve",
    ]) {
        ShareErrorKind::Unreachable
    } else {
        ShareErrorKind::Other
    };
    ShareError::new(kind, line)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;
    use crate::connector::Password;
    use crate::process::fake::{FakeSpawner, Step};

    const SECRET: &str = "Example-Pass-42";

    struct Rig {
        connector: MacConnector,
        spawner: Arc<FakeSpawner>,
        mounted: Arc<AtomicBool>,
        _dir: tempfile::TempDir,
        base: PathBuf,
    }

    fn rig(scripts: Vec<Vec<Step>>, mounted: Arc<AtomicBool>) -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join(MOUNT_FOLDER);
        let spawner = Arc::new(FakeSpawner::new(scripts));
        let flag = Arc::clone(&mounted);
        let connector = MacConnector::with_parts(
            Arc::clone(&spawner) as Arc<dyn Spawner>,
            base.clone(),
            Arc::new(move |_: &Path| flag.load(Ordering::SeqCst)),
        );
        Rig {
            connector,
            spawner,
            mounted,
            _dir: dir,
            base,
        }
    }

    fn share() -> ShareAddress {
        ShareAddress::new("nas", "My Movies").unwrap()
    }

    fn login(user: &str) -> Login {
        Login::new(user, Password::new(SECRET).unwrap()).unwrap()
    }

    fn mount_on(flag: &Arc<AtomicBool>) -> Step {
        let flag = Arc::clone(flag);
        Step::Then(Box::new(move || flag.store(true, Ordering::SeqCst)))
    }

    #[test]
    fn the_password_goes_to_the_prompt_and_never_into_the_arguments() {
        let mounted = Arc::new(AtomicBool::new(false));
        let scripts = vec![vec![
            Step::Out("Password for nas:"),
            Step::Read,
            Step::Out("\r\n"),
            mount_on(&mounted),
            Step::Exit(0),
        ]];
        let rig = rig(scripts, mounted);

        let connected = rig
            .connector
            .connect(&share(), &login(r"HOME\al ex"), &Cancel::new())
            .unwrap();
        assert!(connected.made);
        let root = connected.root;

        assert_eq!(root, rig.base.join("nas").join("My Movies"));
        assert!(root.is_dir(), "the mount folder was made");
        assert_eq!(rig.spawner.input(), format!("{SECRET}\n"));
        let command = &rig.spawner.commands()[0];
        assert!(!command.mentions(SECRET), "{command:?}");
        assert_eq!(command.program, PathBuf::from(SCRIPT));
        let args: Vec<String> = command
            .args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            [
                "-q".to_owned(),
                "/dev/null".to_owned(),
                MOUNT_SMBFS.to_owned(),
                "//HOME;al%20ex@nas/My%20Movies".to_owned(),
                root.to_string_lossy().into_owned(),
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_mount_folder_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let mounted = Arc::new(AtomicBool::new(false));
        let scripts = vec![vec![
            Step::Out("Password:"),
            Step::Read,
            mount_on(&mounted),
            Step::Exit(0),
        ]];
        let rig = rig(scripts, mounted);
        let root = rig
            .connector
            .connect(&share(), &login("alex"), &Cancel::new())
            .unwrap()
            .root;
        let mode = std::fs::metadata(&root).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
    }

    #[test]
    fn a_wrong_password_is_reported_and_the_empty_folder_removed() {
        let rig = rig(
            vec![vec![
                Step::Out("Password:"),
                Step::Read,
                Step::Err("mount_smbfs: server rejected the connection: Authentication error\n"),
                Step::Exit(77),
            ]],
            Arc::new(AtomicBool::new(false)),
        );
        let error = rig
            .connector
            .connect(&share(), &login("alex"), &Cancel::new())
            .unwrap_err();
        assert_eq!(error.kind(), ShareErrorKind::WrongLogin);
        assert_eq!(error.code(), Some(77));
        assert!(!rig.base.join("nas").exists(), "empty folders removed");
    }

    #[test]
    fn a_password_asked_again_is_a_wrong_login() {
        let rig = rig(
            vec![vec![
                Step::Out("Password:"),
                Step::Read,
                Step::Out("\r\nPassword:"),
                Step::Read,
                Step::Exit(0),
            ]],
            Arc::new(AtomicBool::new(false)),
        );
        let error = rig
            .connector
            .connect(&share(), &login("alex"), &Cancel::new())
            .unwrap_err();
        assert_eq!(error.kind(), ShareErrorKind::WrongLogin);
        assert_eq!(rig.spawner.input(), format!("{SECRET}\n"), "sent once");
        assert!(rig.spawner.killed());
    }

    #[test]
    fn an_unknown_question_never_gets_the_password() {
        let rig = rig(
            vec![vec![Step::Out("Username:"), Step::Read, Step::Exit(0)]],
            Arc::new(AtomicBool::new(false)),
        );
        let error = rig
            .connector
            .connect(&share(), &login("alex"), &Cancel::new())
            .unwrap_err();
        assert_eq!(error.kind(), ShareErrorKind::Other);
        assert_eq!(rig.spawner.input(), "");
    }

    #[test]
    fn recorded_failures_map_to_their_kinds() {
        for (output, kind) in [
            (
                "mount_smbfs: server connection failed: No route to host",
                ShareErrorKind::Unreachable,
            ),
            (
                "mount_smbfs: server connection failed: Operation timed out",
                ShareErrorKind::Unreachable,
            ),
            (
                "mount_smbfs: mount error: /x: No such file or directory",
                ShareErrorKind::ShareNotFound,
            ),
            (
                "Password:\r\nmount_smbfs: server rejected the connection: Authentication error",
                ShareErrorKind::WrongLogin,
            ),
            (
                "mount_smbfs: mount error: /x: Permission denied",
                ShareErrorKind::Other,
            ),
        ] {
            assert_eq!(mount_error(output).kind(), kind, "{output}");
        }
        assert_eq!(
            mount_error("noise\nmount_smbfs: server connection failed: Host is down\n").message(),
            "mount_smbfs: server connection failed: Host is down"
        );
    }

    #[test]
    fn a_mounted_share_is_not_mounted_again_and_unmounts_with_umount() {
        let mounted = Arc::new(AtomicBool::new(true));
        let flag = Arc::clone(&mounted);
        let rig = rig(
            vec![vec![
                Step::Then(Box::new(move || flag.store(false, Ordering::SeqCst))),
                Step::Exit(0),
            ]],
            mounted,
        );
        let connected = rig
            .connector
            .connect(&share(), &login("alex"), &Cancel::new())
            .unwrap();
        assert!(!connected.made, "mounted before: not ours");
        let root = connected.root;
        assert!(rig.spawner.commands().is_empty());
        std::fs::create_dir_all(&root).unwrap();
        assert!(
            rig.connector
                .is_connected(&share(), &Cancel::new())
                .unwrap()
        );

        rig.connector.disconnect(&share(), &Cancel::new()).unwrap();

        let command = &rig.spawner.commands()[0];
        assert_eq!(command.program, PathBuf::from(UMOUNT));
        assert_eq!(command.args, [root.as_os_str().to_owned()]);
        assert!(!rig.mounted.load(Ordering::SeqCst));
        assert!(!root.exists(), "the empty mount folder is removed");
    }

    #[test]
    fn a_stopped_mount_is_unmounted_again() {
        let mounted = Arc::new(AtomicBool::new(false));
        let rig = rig(
            vec![
                vec![
                    Step::Out("Password:"),
                    Step::Read,
                    mount_on(&mounted),
                    Step::Hang,
                ],
                vec![Step::Exit(0)],
            ],
            Arc::clone(&mounted),
        );
        let cancel = Cancel::new();
        let stopper = cancel.clone();
        let flag = Arc::clone(&mounted);
        let watcher = std::thread::spawn(move || {
            while !flag.load(Ordering::SeqCst) {
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            stopper.cancel();
        });
        let error = rig
            .connector
            .connect(&share(), &login("alex"), &cancel)
            .unwrap_err();
        watcher.join().unwrap();
        assert_eq!(error.kind(), ShareErrorKind::Cancelled);
        let commands = rig.spawner.commands();
        assert_eq!(commands.len(), 2, "{commands:?}");
        assert_eq!(commands[1].program, PathBuf::from(UMOUNT));
    }

    #[test]
    fn a_busy_share_stays_mounted_and_its_folder_is_kept() {
        let rig = rig(
            vec![vec![
                Step::Err("umount(/x): Resource busy -- try 'diskutil unmount'\n"),
                Step::Exit(1),
            ]],
            Arc::new(AtomicBool::new(true)),
        );
        let root = rig.base.join("nas").join("My Movies");
        std::fs::create_dir_all(&root).unwrap();
        let error = rig
            .connector
            .disconnect(&share(), &Cancel::new())
            .unwrap_err();
        assert_eq!(error.kind(), ShareErrorKind::InUse);
        assert!(root.is_dir());
    }

    #[test]
    fn a_folder_with_files_is_never_removed() {
        let rig = rig(Vec::new(), Arc::new(AtomicBool::new(false)));
        let root = rig.base.join("nas").join("My Movies");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("left.txt"), b"x").unwrap();
        rig.connector.disconnect(&share(), &Cancel::new()).unwrap();
        assert!(root.join("left.txt").is_file());
    }
}
