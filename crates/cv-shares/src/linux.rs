//! Linux desktops: the desktop's user-level share mounting, through `gio`.
//!
//! `gio mount smb://server/share/` asks for the user, the domain and the password on its terminal
//! or standard input, one prompt at a time. CineVault reads each prompt before it answers, in the
//! order User, Domain (when asked), Password: a prompt it does not expect, or one out of that
//! order, ends the attempt before the password is sent. A User prompt after the password means the
//! server refused it. `gio info` gives the local folder of a mounted share
//! (`/run/user/1000/gvfs/smb-share:server=nas,share=movies`), and `gio mount -u` unmounts it.
//! `gio` saves no password itself.
//!
//! Without `gio`, a session bus or a working `gio mount -l`, the desktop offers no user-level share
//! mounting, and the page points to the system's own way of mounting a share.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::address::ShareAddress;
use crate::connector::{Cancel, Connected, Login, ShareConnector, Support};
use crate::error::{ShareError, ShareErrorKind};
use crate::process::{self, Session, Spawner, ToolCommand};

const CONNECT_LIMIT: Duration = Duration::from_secs(60);
const CHECK_LIMIT: Duration = Duration::from_secs(15);

/// Connects shares with `gio mount`.
pub struct LinuxConnector {
    spawner: Arc<dyn Spawner>,
    /// `gio` on `PATH`, when there is one.
    gio: Option<PathBuf>,
    /// Whether the desktop session has a session bus (the user-level mounts live behind it).
    session_bus: bool,
}

/// The prompts `gio mount` asks, in the C locale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Prompt {
    User,
    Domain,
    Password,
}

impl LinuxConnector {
    /// The connector of this desktop session.
    #[cfg(target_os = "linux")]
    #[must_use]
    pub fn new() -> Self {
        let bus = std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some_and(|v| !v.is_empty())
            || std::env::var_os("XDG_RUNTIME_DIR")
                .is_some_and(|dir| std::path::Path::new(&dir).join("bus").exists());
        Self::with_parts(
            Arc::new(crate::process::SystemSpawner),
            find_on_path("gio"),
            bus,
        )
    }

    pub(crate) fn with_parts(
        spawner: Arc<dyn Spawner>,
        gio: Option<PathBuf>,
        session_bus: bool,
    ) -> Self {
        Self {
            spawner,
            gio,
            session_bus,
        }
    }

    /// `gio` with `args`, in the C locale so the prompts are the ones this connector knows.
    fn gio(&self, args: &[&str]) -> Result<ToolCommand, ShareError> {
        let gio = self
            .gio
            .as_ref()
            .filter(|_| self.session_bus)
            .ok_or_else(ShareError::no_user_mounting)?;
        Ok(args
            .iter()
            .fold(ToolCommand::new(gio), |command, arg| command.arg(*arg))
            .env("LC_ALL", "C"))
    }

    /// Runs `gio mount` for `share`. Returns whether it mounted the share (`false`: it was
    /// mounted by someone else meanwhile).
    fn mount(
        &self,
        share: &ShareAddress,
        login: &Login,
        cancel: &Cancel,
    ) -> Result<bool, ShareError> {
        let url = share.smb_url();
        let command = self.gio(&["mount", &url])?;
        let mut session = Session::start(self.spawner.as_ref(), &command, cancel, CONNECT_LIMIT)?;
        Self::answer_prompts(&mut session, login)?;
        let finished = session.finish()?;
        let output = login
            .password()
            .scrub(&format!("{}\n{}", finished.stdout, finished.stderr));
        if finished.code == 0 {
            return Ok(true);
        }
        if output.to_ascii_lowercase().contains("already mounted") {
            return Ok(false);
        }
        Err(mount_error(&output).with_code(i64::from(finished.code)))
    }

    fn answer_prompts(session: &mut Session, login: &Login) -> Result<(), ShareError> {
        let password = login.password();
        let (mut user, mut domain, mut sent) = (false, false, false);
        while let Some(text) = session.prompt()? {
            let answer = match prompt_kind(&text) {
                Some(Prompt::User) if !user && !sent => {
                    user = true;
                    login.name()
                }
                Some(Prompt::Domain) if user && !domain && !sent => {
                    domain = true;
                    // An empty answer takes the default gio offers.
                    login.domain().unwrap_or("")
                }
                Some(Prompt::Password) if user && !sent => {
                    sent = true;
                    password.expose()
                }
                _ => {
                    session.stop();
                    return Err(if sent {
                        ShareError::new(ShareErrorKind::WrongLogin, "the login was asked again")
                    } else {
                        ShareError::new(
                            ShareErrorKind::Other,
                            format!("gio asked: {}", password.scrub(&text)),
                        )
                    });
                }
            };
            session.answer(answer)?;
        }
        Ok(())
    }
}

impl ShareConnector for LinuxConnector {
    fn support(&self) -> Support {
        let Ok(command) = self.gio(&["mount", "-l"]) else {
            return Support::NoUserMounting;
        };
        match process::run(
            self.spawner.as_ref(),
            &command,
            None,
            &Cancel::new(),
            CHECK_LIMIT,
        ) {
            Ok(finished) if finished.code == 0 => Support::Supported,
            _ => Support::NoUserMounting,
        }
    }

    fn connect(
        &self,
        share: &ShareAddress,
        login: &Login,
        cancel: &Cancel,
    ) -> Result<Connected, ShareError> {
        if let Some(root) = self.local_root(share, cancel)? {
            return Ok(Connected { root, made: false });
        }
        let made = self.mount(share, login, cancel)?;
        let root = self.local_root(share, cancel)?.ok_or_else(|| {
            ShareError::new(
                ShareErrorKind::NoUserMounting,
                "the share is mounted, but the desktop gives it no local folder",
            )
        })?;
        Ok(Connected { root, made })
    }

    fn is_connected(&self, share: &ShareAddress, cancel: &Cancel) -> Result<bool, ShareError> {
        Ok(self.local_root(share, cancel)?.is_some())
    }

    fn disconnect(&self, share: &ShareAddress, cancel: &Cancel) -> Result<(), ShareError> {
        if !self.is_connected(share, cancel)? {
            return Ok(());
        }
        let url = share.smb_url();
        let command = self.gio(&["mount", "-u", &url])?;
        let finished = process::run(self.spawner.as_ref(), &command, None, cancel, CHECK_LIMIT)?;
        if finished.code == 0 {
            return Ok(());
        }
        let output = format!("{}\n{}", finished.stdout, finished.stderr);
        let lower = output.to_ascii_lowercase();
        if lower.contains("not mounted") {
            return Ok(());
        }
        let kind = if lower.contains("busy") {
            ShareErrorKind::InUse
        } else {
            ShareErrorKind::Other
        };
        Err(ShareError::new(kind, process::last_line(&output)).with_code(i64::from(finished.code)))
    }

    fn local_root(
        &self,
        share: &ShareAddress,
        cancel: &Cancel,
    ) -> Result<Option<PathBuf>, ShareError> {
        let url = share.smb_url();
        let command = self.gio(&["info", &url])?;
        let finished = process::run(self.spawner.as_ref(), &command, None, cancel, CHECK_LIMIT)?;
        if finished.code != 0 {
            return Ok(None);
        }
        Ok(local_path(&finished.stdout))
    }
}

/// Which prompt `text` (`User [alex]:`, `Domain [WORKGROUP]:`, `Password:`) is.
fn prompt_kind(text: &str) -> Option<Prompt> {
    let text = text.trim().trim_end_matches(':').trim_end();
    let name = match text.find('[') {
        Some(at) if text.ends_with(']') => text[..at].trim_end(),
        _ => text,
    };
    [
        ("User", Prompt::User),
        ("Domain", Prompt::Domain),
        ("Password", Prompt::Password),
    ]
    .into_iter()
    .find(|(word, _)| name.eq_ignore_ascii_case(word))
    .map(|(_, prompt)| prompt)
}

/// The `local path:` that `gio info` printed.
fn local_path(info: &str) -> Option<PathBuf> {
    info.lines()
        .filter_map(|line| line.trim().strip_prefix("local path:"))
        .map(str::trim)
        .find(|path| path.starts_with('/'))
        .map(PathBuf::from)
}

/// The error a failed `gio mount` reported.
fn mount_error(output: &str) -> ShareError {
    let line = process::last_line(output);
    let lower = output.to_ascii_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| lower.contains(w));
    let kind = if has(&[
        "permission denied",
        "login failed",
        "access denied",
        "logon failure",
    ]) {
        ShareErrorKind::WrongLogin
    } else if has(&[
        "failed to retrieve share list",
        "connection refused",
        "connection timed out",
        "timed out",
        "no route to host",
        "host is down",
        "network is unreachable",
        "name or service not known",
        "could not resolve",
    ]) {
        ShareErrorKind::Unreachable
    } else if has(&[
        "no such file or directory",
        "bad network name",
        "share not found",
    ]) {
        ShareErrorKind::ShareNotFound
    } else {
        ShareErrorKind::Other
    };
    ShareError::new(kind, line)
}

/// The first `name` file in a `PATH` folder.
#[cfg(target_os = "linux")]
fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connector::Password;
    use crate::process::fake::{FakeSpawner, Step};

    const SECRET: &str = "Example-Pass-42";
    const ROOT: &str = "/run/user/1000/gvfs/smb-share:server=nas,share=movies";
    const MOUNTED: &str = "display name: movies on nas\nlocal path: /run/user/1000/gvfs/smb-share:server=nas,share=movies\nuri: smb://nas/movies/\n";
    const NOT_MOUNTED: &str = "gio: smb://nas/movies/: The specified location is not mounted\n";

    fn connector(scripts: Vec<Vec<Step>>) -> (LinuxConnector, Arc<FakeSpawner>) {
        let spawner = Arc::new(FakeSpawner::new(scripts));
        let connector = LinuxConnector::with_parts(
            Arc::clone(&spawner) as Arc<dyn Spawner>,
            Some(PathBuf::from("/usr/bin/gio")),
            true,
        );
        (connector, spawner)
    }

    fn share() -> ShareAddress {
        ShareAddress::new("nas", "movies").unwrap()
    }

    fn login(user: &str) -> Login {
        Login::new(user, Password::new(SECRET).unwrap()).unwrap()
    }

    fn not_mounted() -> Vec<Step> {
        vec![Step::Err(NOT_MOUNTED), Step::Exit(2)]
    }

    fn mounted() -> Vec<Step> {
        vec![Step::Out(MOUNTED), Step::Exit(0)]
    }

    fn args(command: &ToolCommand) -> Vec<String> {
        command
            .args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn the_prompts_are_answered_in_order_and_the_password_stays_off_the_arguments() {
        let (connector, spawner) = connector(vec![
            not_mounted(),
            vec![
                Step::Out(
                    "Authentication Required\nEnter user and password for share \"movies\" on \"nas\":\n",
                ),
                Step::Out("User [me]: "),
                Step::Read,
                Step::Out("Domain [WORKGROUP]: "),
                Step::Read,
                Step::Out("Password: "),
                Step::Read,
                Step::Exit(0),
            ],
            mounted(),
        ]);

        let connected = connector
            .connect(&share(), &login(r"HOME\alex"), &Cancel::new())
            .unwrap();

        assert_eq!(connected.root, PathBuf::from(ROOT));
        assert!(connected.made);
        assert_eq!(spawner.input(), format!("alex\nHOME\n{SECRET}\n"));
        let commands = spawner.commands();
        assert_eq!(
            commands.iter().map(args).collect::<Vec<_>>(),
            [
                vec!["info", "smb://nas/movies/"],
                vec!["mount", "smb://nas/movies/"],
                vec!["info", "smb://nas/movies/"],
            ]
        );
        for command in &commands {
            assert!(!command.mentions(SECRET), "{command:?}");
            assert!(command.env.contains(&("LC_ALL".into(), "C".into())));
        }
    }

    #[test]
    fn the_default_domain_is_taken_and_a_missing_domain_prompt_is_fine() {
        let (connector, spawner) = connector(vec![
            not_mounted(),
            vec![
                Step::Out("User [me]: "),
                Step::Read,
                Step::Out("Domain [WORKGROUP]: "),
                Step::Read,
                Step::Out("Password: "),
                Step::Read,
                Step::Exit(0),
            ],
            mounted(),
            not_mounted(),
            vec![
                Step::Out("User: "),
                Step::Read,
                Step::Out("Password: "),
                Step::Read,
                Step::Exit(0),
            ],
            mounted(),
        ]);
        connector
            .connect(&share(), &login("alex"), &Cancel::new())
            .unwrap();
        connector
            .connect(&share(), &login("alex"), &Cancel::new())
            .unwrap();
        assert_eq!(
            spawner.input(),
            format!("alex\n\n{SECRET}\nalex\n{SECRET}\n")
        );
    }

    #[test]
    fn a_changed_prompt_order_fails_without_sending_the_password() {
        for script in [
            vec![Step::Out("Password: "), Step::Read, Step::Exit(0)],
            vec![
                Step::Out("Domain [WORKGROUP]: "),
                Step::Read,
                Step::Out("Password: "),
                Step::Read,
                Step::Exit(0),
            ],
            vec![
                Step::Out("User [me]: "),
                Step::Read,
                Step::Out("Login anonymously? [y/N]: "),
                Step::Read,
                Step::Exit(0),
            ],
        ] {
            let (connector, spawner) = connector(vec![not_mounted(), script]);
            let error = connector
                .connect(&share(), &login("alex"), &Cancel::new())
                .unwrap_err();
            assert_eq!(error.kind(), ShareErrorKind::Other);
            assert!(!spawner.input().contains(SECRET), "{}", spawner.input());
            assert!(spawner.killed());
        }
    }

    #[test]
    fn the_login_asked_again_is_a_wrong_login() {
        let (connector, spawner) = connector(vec![
            not_mounted(),
            vec![
                Step::Out("User [me]: "),
                Step::Read,
                Step::Out("Password: "),
                Step::Read,
                Step::Out("User [alex]: "),
                Step::Read,
                Step::Exit(0),
            ],
        ]);
        let error = connector
            .connect(&share(), &login("alex"), &Cancel::new())
            .unwrap_err();
        assert_eq!(error.kind(), ShareErrorKind::WrongLogin);
        assert_eq!(spawner.input().matches(SECRET).count(), 1);
    }

    #[test]
    fn recorded_failures_map_to_their_kinds() {
        for (stderr, kind) in [
            (
                "gio: smb://nas/movies/: Failed to mount Windows share: Permission denied\n",
                ShareErrorKind::WrongLogin,
            ),
            (
                "gio: smb://nas/movies/: Failed to mount Windows share: Connection timed out\n",
                ShareErrorKind::Unreachable,
            ),
            (
                "gio: smb://nas/movies/: Failed to retrieve share list from server: No route to host\n",
                ShareErrorKind::Unreachable,
            ),
            (
                "gio: smb://nas/movies/: Failed to mount Windows share: No such file or directory\n",
                ShareErrorKind::ShareNotFound,
            ),
            (
                "gio: smb://nas/movies/: Failed to mount Windows share: Software caused connection abort\n",
                ShareErrorKind::Other,
            ),
        ] {
            let (connector, _) = connector(vec![
                not_mounted(),
                vec![
                    Step::Out("User [me]: "),
                    Step::Read,
                    Step::Out("Password: "),
                    Step::Read,
                    Step::Err(stderr),
                    Step::Exit(2),
                ],
            ]);
            let error = connector
                .connect(&share(), &login("alex"), &Cancel::new())
                .unwrap_err();
            assert_eq!(error.kind(), kind, "{stderr}");
            assert_eq!(error.code(), Some(2));
            assert!(error.message().starts_with("gio: smb://nas/movies/"));
        }
    }

    #[test]
    fn a_mounted_share_is_found_and_unmounted() {
        let (connector, spawner) = connector(vec![mounted(), mounted(), vec![Step::Exit(0)]]);
        let connected = connector
            .connect(&share(), &login("alex"), &Cancel::new())
            .unwrap();
        assert_eq!(connected.root, PathBuf::from(ROOT));
        assert!(!connected.made, "mounted by someone else: not ours");
        assert_eq!(spawner.commands().len(), 1, "no gio mount ran");
        connector.disconnect(&share(), &Cancel::new()).unwrap();
        assert_eq!(
            args(&spawner.commands()[2]),
            ["mount", "-u", "smb://nas/movies/"]
        );
        assert_eq!(spawner.input(), "");
    }

    #[test]
    fn a_share_mounted_by_someone_else_meanwhile_is_not_ours() {
        let (connector, _) = connector(vec![
            not_mounted(),
            vec![
                Step::Out("User [me]: "),
                Step::Read,
                Step::Out("Password: "),
                Step::Read,
                Step::Err("gio: smb://nas/movies/: Location is already mounted\n"),
                Step::Exit(2),
            ],
            mounted(),
        ]);
        let connected = connector
            .connect(&share(), &login("alex"), &Cancel::new())
            .unwrap();
        assert!(!connected.made);
    }

    #[test]
    fn a_busy_share_is_in_use() {
        let (connector, _) = connector(vec![
            mounted(),
            vec![
                Step::Err("gio: smb://nas/movies/: Device or resource busy\n"),
                Step::Exit(2),
            ],
        ]);
        let error = connector.disconnect(&share(), &Cancel::new()).unwrap_err();
        assert_eq!(error.kind(), ShareErrorKind::InUse);
    }

    #[test]
    fn without_gio_or_a_session_bus_there_is_no_user_mounting() {
        let spawner: Arc<dyn Spawner> = Arc::new(FakeSpawner::new(Vec::new()));
        let no_gio = LinuxConnector::with_parts(Arc::clone(&spawner), None, true);
        let no_bus =
            LinuxConnector::with_parts(spawner, Some(PathBuf::from("/usr/bin/gio")), false);
        for connector in [no_gio, no_bus] {
            assert_eq!(connector.support(), Support::NoUserMounting);
            let error = connector
                .connect(&share(), &login("alex"), &Cancel::new())
                .unwrap_err();
            assert_eq!(error.kind(), ShareErrorKind::NoUserMounting);
        }
    }

    #[test]
    fn support_needs_a_working_mount_list() {
        let (working, _) = connector(vec![vec![Step::Out("Drive(0): x\n"), Step::Exit(0)]]);
        assert_eq!(working.support(), Support::Supported);
        let (broken, _) = connector(vec![vec![
            Step::Err("gio: Operation not supported\n"),
            Step::Exit(1),
        ]]);
        assert_eq!(broken.support(), Support::NoUserMounting);
    }

    #[test]
    fn prompts_and_local_paths_are_read() {
        assert_eq!(prompt_kind("User [alex]:"), Some(Prompt::User));
        assert_eq!(prompt_kind("Domain [WORKGROUP]: "), Some(Prompt::Domain));
        assert_eq!(prompt_kind("Password:"), Some(Prompt::Password));
        assert_eq!(prompt_kind("Login anonymously? [y/N]:"), None);
        assert_eq!(prompt_kind("Enter user and password:"), None);
        assert_eq!(local_path(MOUNTED), Some(PathBuf::from(ROOT)));
        assert_eq!(local_path("uri: smb://nas/movies/\n"), None);
    }
}
