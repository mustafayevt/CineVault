//! Windows: `New-SmbMapping` without a drive letter, through a fixed PowerShell script.
//!
//! The script is a constant of this file, sent as `-EncodedCommand` so no quoting can change it.
//! The share's network path and the user go in the environment (`CINEVAULT_SHARE_REMOTE`,
//! `CINEVAULT_SHARE_USER`), already checked and never pasted into script text; the password is
//! the one line the script reads from standard input as UTF-8. The script prints one JSON line
//! `{ok, code, message, connected}` with the Win32 error code of a failure.
//!
//! The mapping is not persistent: it lasts until the user signs out, Windows stores no credential
//! for it, and the folder keeps its network path (`\\nas\movies\Films`).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use serde::Deserialize;

use crate::address::ShareAddress;
use crate::connector::{Cancel, Login, ShareConnector, Support};
use crate::error::{ShareError, ShareErrorKind};
use crate::process::{self, Spawner, ToolCommand};

/// How long PowerShell may take to connect: its own start plus the server's answer.
const CONNECT_LIMIT: Duration = Duration::from_secs(60);

/// How long a check or a disconnect may take.
const CHECK_LIMIT: Duration = Duration::from_secs(30);

const REMOTE_VAR: &str = "CINEVAULT_SHARE_REMOTE";
const USER_VAR: &str = "CINEVAULT_SHARE_USER";

/// Shared start of every script: stop on errors, no progress records, UTF-8 output, and the
/// result line.
const PRELUDE: &str = r"
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
try { [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false } catch { }
function Write-Result($ok, $code, $message, $connected) {
    [pscustomobject]@{ ok = $ok; code = $code; message = $message; connected = $connected } |
        ConvertTo-Json -Compress
}
function Get-Win32Code($record) {
    if ($record.FullyQualifiedErrorId -match 'Windows System Error (\d+)') { return [int]$Matches[1] }
    $data = $record.Exception.ErrorData
    if ($data -and $data.CimInstanceProperties['error_Code']) {
        return [int]$data.CimInstanceProperties['error_Code'].Value
    }
    return 0
}
function Test-Mapped {
    $mapping = Get-SmbMapping -RemotePath $env:CINEVAULT_SHARE_REMOTE -ErrorAction SilentlyContinue
    return [bool]($mapping | Where-Object { $_.Status -eq 'OK' })
}
";

const CONNECT: &str = r"
try {
    $reader = New-Object System.IO.StreamReader ([Console]::OpenStandardInput()), (New-Object System.Text.UTF8Encoding $false)
    $password = $reader.ReadLine()
    if ($null -eq $password) { $password = '' }
    New-SmbMapping -RemotePath $env:CINEVAULT_SHARE_REMOTE -UserName $env:CINEVAULT_SHARE_USER -Password $password -Persistent $false | Out-Null
    $password = $null
    Write-Result $true 0 '' $true
} catch {
    $password = $null
    Write-Result $false (Get-Win32Code $_) $_.Exception.Message $false
}
";

const CHECK: &str = r"
try {
    Write-Result $true 0 '' (Test-Mapped)
} catch {
    Write-Result $false (Get-Win32Code $_) $_.Exception.Message $false
}
";

const DISCONNECT: &str = r"
try {
    $mapping = Get-SmbMapping -RemotePath $env:CINEVAULT_SHARE_REMOTE -ErrorAction SilentlyContinue
    if ($mapping) {
        Remove-SmbMapping -RemotePath $env:CINEVAULT_SHARE_REMOTE -Force:$false -UpdateProfile:$false -Confirm:$false
    }
    Write-Result $true 0 '' $false
} catch {
    Write-Result $false (Get-Win32Code $_) $_.Exception.Message (Test-Mapped)
}
";

/// The JSON line a script prints.
#[derive(Debug, Deserialize)]
struct ScriptResult {
    ok: bool,
    #[serde(default)]
    code: i64,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    connected: bool,
}

/// Connects shares with `New-SmbMapping`.
pub struct WindowsConnector {
    spawner: Arc<dyn Spawner>,
    powershell: PathBuf,
}

impl WindowsConnector {
    /// The connector of this computer: Windows PowerShell from the system folder, never a
    /// `powershell.exe` found on `PATH`.
    #[cfg(windows)]
    #[must_use]
    pub fn new() -> Self {
        let system_root = std::env::var_os("SystemRoot")
            .map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
        Self::with_parts(
            Arc::new(crate::process::SystemSpawner),
            system_root.join(r"System32\WindowsPowerShell\v1.0\powershell.exe"),
        )
    }

    pub(crate) fn with_parts(spawner: Arc<dyn Spawner>, powershell: PathBuf) -> Self {
        Self {
            spawner,
            powershell,
        }
    }

    /// The command that runs `script` for `share`, with `user` when connecting.
    fn command(&self, script: &str, share: &ShareAddress, user: Option<&str>) -> ToolCommand {
        let mut command = ToolCommand::new(&self.powershell)
            .arg("-NoProfile")
            .arg("-NonInteractive")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-EncodedCommand")
            .arg(encode_script(script))
            .env(REMOTE_VAR, share.unc());
        if let Some(user) = user {
            command = command.env(USER_VAR, user);
        }
        command
    }

    fn run(
        &self,
        command: &ToolCommand,
        input: Option<&str>,
        cancel: &Cancel,
        limit: Duration,
        scrub: impl Fn(&str) -> String,
    ) -> Result<ScriptResult, ShareError> {
        let finished = process::run(self.spawner.as_ref(), command, input, cancel, limit)?;
        let line = finished
            .stdout
            .lines()
            .map(str::trim)
            .rfind(|line| line.starts_with('{'))
            .unwrap_or_default();
        serde_json::from_str(line).map_err(|_| {
            let shown = process::last_line(&format!("{}\n{}", finished.stdout, finished.stderr));
            ShareError::new(
                ShareErrorKind::Other,
                format!("PowerShell gave no result: {}", scrub(&shown)),
            )
            .with_code(i64::from(finished.code))
        })
    }
}

impl ShareConnector for WindowsConnector {
    fn support(&self) -> Support {
        Support::Supported
    }

    fn connect(
        &self,
        share: &ShareAddress,
        login: &Login,
        cancel: &Cancel,
    ) -> Result<PathBuf, ShareError> {
        if self.is_connected(share, cancel)? {
            return Ok(PathBuf::from(share.unc()));
        }
        let command = self.command(CONNECT, share, Some(login.user()));
        let password = login.password();
        let result = self.run(
            &command,
            Some(password.expose()),
            cancel,
            CONNECT_LIMIT,
            |text| password.scrub(text),
        )?;
        if result.ok {
            Ok(PathBuf::from(share.unc()))
        } else {
            let message = password.scrub(result.message.as_deref().unwrap_or_default());
            Err(error_for(result.code, message))
        }
    }

    fn is_connected(&self, share: &ShareAddress, cancel: &Cancel) -> Result<bool, ShareError> {
        let result = self.run(
            &self.command(CHECK, share, None),
            None,
            cancel,
            CHECK_LIMIT,
            str::to_owned,
        )?;
        if result.ok {
            Ok(result.connected)
        } else {
            Err(error_for(result.code, result.message.unwrap_or_default()))
        }
    }

    fn disconnect(&self, share: &ShareAddress, cancel: &Cancel) -> Result<(), ShareError> {
        let result = self.run(
            &self.command(DISCONNECT, share, None),
            None,
            cancel,
            CHECK_LIMIT,
            str::to_owned,
        )?;
        // "The network connection does not exist": it is gone already.
        if result.ok || result.code == 2250 {
            Ok(())
        } else {
            Err(error_for(result.code, result.message.unwrap_or_default()))
        }
    }

    fn local_root(
        &self,
        share: &ShareAddress,
        _cancel: &Cancel,
    ) -> Result<Option<PathBuf>, ShareError> {
        Ok(Some(PathBuf::from(share.unc())))
    }
}

/// The script as `-EncodedCommand` takes it: the prelude and `script`, UTF-16LE, base64.
fn encode_script(script: &str) -> String {
    let text = format!("{PRELUDE}{script}");
    let bytes: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// The error kind of a Win32 error code.
fn kind_for(code: i64) -> ShareErrorKind {
    match code {
        // ERROR_LOGON_FAILURE, ERROR_INVALID_PASSWORD, NERR_BadUsername
        1326 | 86 | 2202 => ShareErrorKind::WrongLogin,
        // ERROR_BAD_NETPATH, ERROR_NETWORK_UNREACHABLE
        53 | 1231 => ShareErrorKind::Unreachable,
        // ERROR_BAD_NET_NAME
        67 => ShareErrorKind::ShareNotFound,
        // ERROR_SESSION_CREDENTIAL_CONFLICT
        1219 => ShareErrorKind::OtherUserConnected,
        // ERROR_ACCOUNT_LOCKED_OUT
        1909 => ShareErrorKind::AccountLocked,
        // ERROR_OPEN_FILES, ERROR_DEVICE_IN_USE
        2401 | 2404 => ShareErrorKind::InUse,
        _ => ShareErrorKind::Other,
    }
}

fn error_for(code: i64, message: String) -> ShareError {
    let error = ShareError::new(kind_for(code), message.trim());
    if code == 0 {
        error
    } else {
        error.with_code(code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connector::Password;
    use crate::process::fake::{FakeSpawner, Step};

    const SECRET: &str = "Example-Pass-42";

    fn connector(scripts: Vec<Vec<Step>>) -> (WindowsConnector, Arc<FakeSpawner>) {
        let spawner = Arc::new(FakeSpawner::new(scripts));
        let connector = WindowsConnector::with_parts(
            Arc::clone(&spawner) as Arc<dyn Spawner>,
            PathBuf::from("powershell.exe"),
        );
        (connector, spawner)
    }

    fn share() -> ShareAddress {
        ShareAddress::new("nas", "movies").unwrap()
    }

    fn login() -> Login {
        Login::new(r"HOME\alex", Password::new(SECRET).unwrap()).unwrap()
    }

    fn decoded(command: &ToolCommand) -> String {
        let encoded = command.args.last().unwrap().to_string_lossy().into_owned();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap();
        let units: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect();
        String::from_utf16(&units).unwrap()
    }

    const NOT_MAPPED: &str = "{\"ok\":true,\"code\":0,\"message\":\"\",\"connected\":false}\n";

    #[test]
    fn connecting_sends_the_password_on_standard_input_only() {
        let (connector, spawner) = connector(vec![
            vec![Step::Out(NOT_MAPPED), Step::Exit(0)],
            vec![
                Step::Read,
                Step::Out("{\"ok\":true,\"code\":0,\"message\":\"\",\"connected\":true}\r\n"),
                Step::Exit(0),
            ],
        ]);

        let root = connector
            .connect(&share(), &login(), &Cancel::new())
            .unwrap();

        assert_eq!(root, PathBuf::from(r"\\nas\movies"));
        assert_eq!(spawner.input(), format!("{SECRET}\n"));
        let commands = spawner.commands();
        assert_eq!(commands.len(), 2);
        for command in &commands {
            assert!(!command.mentions(SECRET), "{command:?}");
            assert!(!decoded(command).contains(SECRET));
        }
        let connect = &commands[1];
        assert_eq!(
            connect.args[..5],
            [
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-EncodedCommand"
            ]
            .map(std::ffi::OsString::from)
        );
        assert!(decoded(connect).contains("New-SmbMapping"));
        assert!(decoded(connect).contains("-Persistent $false"));
        assert!(!decoded(connect).contains("-LocalPath"), "no drive letter");
        assert!(
            connect
                .env
                .contains(&(REMOTE_VAR.into(), r"\\nas\movies".into()))
        );
        assert!(
            connect
                .env
                .contains(&(USER_VAR.into(), r"HOME\alex".into()))
        );
    }

    #[test]
    fn a_share_that_is_already_mapped_is_not_connected_again() {
        let (connector, spawner) = connector(vec![vec![
            Step::Out("{\"ok\":true,\"code\":0,\"message\":\"\",\"connected\":true}"),
            Step::Exit(0),
        ]]);
        let root = connector
            .connect(&share(), &login(), &Cancel::new())
            .unwrap();
        assert_eq!(root, PathBuf::from(r"\\nas\movies"));
        assert_eq!(spawner.commands().len(), 1);
        assert!(decoded(&spawner.commands()[0]).contains("Get-SmbMapping"));
        assert_eq!(spawner.input(), "", "no password was sent");
    }

    #[test]
    fn a_failed_connect_maps_its_win32_code_and_hides_the_password() {
        let (connector, _) = connector(vec![
            vec![Step::Out(NOT_MAPPED), Step::Exit(0)],
            vec![
                Step::Read,
                Step::Out(
                    "{\"ok\":false,\"code\":1326,\"message\":\"The user name or password is incorrect. Example-Pass-42\",\"connected\":false}\n",
                ),
                Step::Exit(0),
            ],
        ]);
        let error = connector
            .connect(&share(), &login(), &Cancel::new())
            .unwrap_err();
        assert_eq!(error.kind(), ShareErrorKind::WrongLogin);
        assert_eq!(error.code(), Some(1326));
        assert!(!error.message().contains(SECRET));
        assert!(!format!("{error} {error:?}").contains(SECRET));
    }

    #[test]
    fn every_listed_win32_code_maps_to_its_kind() {
        for (code, kind) in [
            (1326, ShareErrorKind::WrongLogin),
            (86, ShareErrorKind::WrongLogin),
            (2202, ShareErrorKind::WrongLogin),
            (53, ShareErrorKind::Unreachable),
            (1231, ShareErrorKind::Unreachable),
            (67, ShareErrorKind::ShareNotFound),
            (1219, ShareErrorKind::OtherUserConnected),
            (1909, ShareErrorKind::AccountLocked),
            (2401, ShareErrorKind::InUse),
            (2404, ShareErrorKind::InUse),
            (5, ShareErrorKind::Other),
            (0, ShareErrorKind::Other),
        ] {
            assert_eq!(kind_for(code), kind, "{code}");
        }
        let other = error_for(5, "Access is denied. ".into());
        assert_eq!(other.to_string(), "Access is denied. (error 5)");
    }

    #[test]
    fn powershell_without_a_result_line_gives_its_last_words() {
        let (connector, _) = connector(vec![vec![
            Step::Err("powershell.exe is blocked by your administrator\n"),
            Step::Exit(1),
        ]]);
        let error = connector
            .is_connected(&share(), &Cancel::new())
            .unwrap_err();
        assert_eq!(error.kind(), ShareErrorKind::Other);
        assert!(error.message().contains("blocked by your administrator"));
    }

    #[test]
    fn disconnecting_removes_the_mapping_and_accepts_a_missing_one() {
        let (connector, spawner) = connector(vec![
            vec![
                Step::Out("{\"ok\":true,\"code\":0,\"message\":\"\",\"connected\":false}"),
                Step::Exit(0),
            ],
            vec![
                Step::Out("{\"ok\":false,\"code\":2250,\"message\":\"gone\",\"connected\":false}"),
                Step::Exit(0),
            ],
            vec![
                Step::Out(
                    "{\"ok\":false,\"code\":2401,\"message\":\"open files\",\"connected\":true}",
                ),
                Step::Exit(0),
            ],
        ]);
        connector.disconnect(&share(), &Cancel::new()).unwrap();
        connector.disconnect(&share(), &Cancel::new()).unwrap();
        let busy = connector.disconnect(&share(), &Cancel::new()).unwrap_err();
        assert_eq!(busy.kind(), ShareErrorKind::InUse);
        let script = decoded(&spawner.commands()[0]);
        assert!(script.contains("Remove-SmbMapping"));
        assert!(script.contains("-Force:$false"));
    }

    #[test]
    fn an_ipv6_server_uses_the_literal_host_name() {
        let (connector, spawner) = connector(vec![vec![Step::Out(NOT_MAPPED), Step::Exit(0)]]);
        let share = ShareAddress::new("[fe80::1]", "movies").unwrap();
        assert!(!connector.is_connected(&share, &Cancel::new()).unwrap());
        assert!(spawner.commands()[0].env.contains(&(
            REMOTE_VAR.into(),
            r"\\fe80--1.ipv6-literal.net\movies".into()
        )));
    }

    #[test]
    fn the_scripts_read_their_inputs_from_the_environment_only() {
        for script in [CONNECT, CHECK, DISCONNECT] {
            let text = format!("{PRELUDE}{script}");
            assert!(!text.contains("{0}") && !text.contains("$args"), "{script}");
        }
        assert!(CONNECT.contains("$env:CINEVAULT_SHARE_USER"));
        assert!(PRELUDE.contains("$env:CINEVAULT_SHARE_REMOTE"));
    }

    /// Runs the real scripts against this computer and an address nobody answers:
    /// `CINEVAULT_LIVE_SHARES=1 cargo test -p cv-shares live_`. No share is left connected.
    #[cfg(windows)]
    #[test]
    fn live_scripts_report_their_results() {
        if std::env::var_os("CINEVAULT_LIVE_SHARES").is_none() {
            return;
        }
        let connector = WindowsConnector::new();
        let cancel = Cancel::new();
        let nowhere = ShareAddress::new("192.0.2.1", "movies").unwrap();
        assert!(!connector.is_connected(&nowhere, &cancel).unwrap());
        connector.disconnect(&nowhere, &cancel).unwrap();

        let stranger = Login::new(
            "cinevault-no-such-user",
            Password::new("not-the-password").unwrap(),
        )
        .unwrap();
        let own = ShareAddress::new("127.0.0.1", "C$").unwrap();
        let refused = connector.connect(&own, &stranger, &cancel).unwrap_err();
        eprintln!("{refused:?}");
        // Loopback refuses another login in its own way (1326 or 1208, by Windows version); the
        // point is that the script ran and reported a Win32 code.
        assert!(refused.code().is_some(), "{refused:?}");

        let unreachable = connector
            .connect(
                &nowhere,
                &stranger,
                &Cancel::with_limit(Duration::from_secs(90)),
            )
            .unwrap_err();
        eprintln!("{unreachable:?}");
        assert_eq!(
            unreachable.kind(),
            ShareErrorKind::Unreachable,
            "{unreachable:?}"
        );
    }
}
