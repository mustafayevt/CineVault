//! Windows: a connection without a drive letter (`WNetAddConnection2W`), through a fixed
//! PowerShell script.
//!
//! The script is a constant of this file, sent as `-EncodedCommand` so no quoting can change it.
//! The share's network path and the user go in the environment (`CINEVAULT_SHARE_REMOTE`,
//! `CINEVAULT_SHARE_USER`), already checked and never pasted into script text; the password is
//! the one line the script reads from standard input as UTF-8. The script declares
//! `WNetAddConnection2W` and `WNetCancelConnection2W` with `Add-Type` (C# source that is part of
//! the script) and hands the password to it as a .NET method argument: no cmdlet ever gets it as
//! a parameter, so PowerShell's module logging, which records cmdlet parameters, never sees it.
//! The script prints one JSON line `{ok, code, message, connected}` with the Win32 error code of
//! a failure.
//!
//! The connection is not persistent: it lasts until the user signs out, Windows stores no
//! credential for it, and the folder keeps its network path (`\\nas\movies\Films`).
//!
//! Only connections without a drive letter count as CineVault's: a drive letter the user mapped
//! to the same share is never reported as connected here and never disconnected
//! (`WNetCancelConnection2W` on a network path cancels only the connections without a device).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use serde::Deserialize;

use crate::address::ShareAddress;
use crate::connector::{Cancel, Connected, Login, ShareConnector, Support};
use crate::error::{ShareError, ShareErrorKind};
use crate::process::{self, Spawner, ToolCommand};

/// How long PowerShell may take to connect: its own start plus the server's answer.
const CONNECT_LIMIT: Duration = Duration::from_secs(60);

/// How long a check or a disconnect may take.
const CHECK_LIMIT: Duration = Duration::from_secs(30);

const REMOTE_VAR: &str = "CINEVAULT_SHARE_REMOTE";
const USER_VAR: &str = "CINEVAULT_SHARE_USER";

/// Shared start of every script: stop on errors, no progress records, UTF-8 output, the result
/// line, and the connections to the share that have no drive letter.
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
function Get-Win32Message($code) {
    return (New-Object System.ComponentModel.Win32Exception ([int]$code)).Message
}
function Get-DevicelessMapping {
    Get-SmbMapping -RemotePath $env:CINEVAULT_SHARE_REMOTE -ErrorAction SilentlyContinue |
        Where-Object { -not $_.LocalPath }
}
function Test-Mapped {
    return [bool](Get-DevicelessMapping | Where-Object { $_.Status -eq 'OK' })
}
";

/// The two calls of the Windows networking API the connect and disconnect scripts use, declared
/// with `Add-Type`. No drive letter (`LocalName` is null), not remembered in the profile (flags
/// 0), and a disconnect never forced while files are open.
const NATIVE: &str = r#"
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class CineVaultShare {
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private class NetResource {
        public int Scope;
        public int Type;
        public int DisplayType;
        public int Usage;
        public string LocalName;
        public string RemoteName;
        public string Comment;
        public string Provider;
    }
    [DllImport("mpr.dll", CharSet = CharSet.Unicode)]
    private static extern int WNetAddConnection2W(NetResource resource, string password, string user, int flags);
    [DllImport("mpr.dll", CharSet = CharSet.Unicode)]
    private static extern int WNetCancelConnection2W(string name, int flags, bool force);
    [DllImport("mpr.dll", CharSet = CharSet.Unicode)]
    private static extern int WNetGetLastErrorW(out int error, StringBuilder description, int descriptionSize, StringBuilder provider, int providerSize);
    public static int Connect(string remote, string user, string password) {
        NetResource resource = new NetResource();
        resource.Type = 1;
        resource.RemoteName = remote;
        int code = WNetAddConnection2W(resource, password, user, 0);
        if (code == 1208) {
            // ERROR_EXTENDED_ERROR: the network's own code says what happened.
            int extended;
            StringBuilder description = new StringBuilder(256);
            StringBuilder provider = new StringBuilder(256);
            if (WNetGetLastErrorW(out extended, description, description.Capacity, provider, provider.Capacity) == 0 && extended != 0) {
                return extended;
            }
        }
        return code;
    }
    public static int Disconnect(string remote) {
        return WNetCancelConnection2W(remote, 0, false);
    }
}
'@
"#;

/// Windows answers "the network name cannot be found" (67) also when the server does not answer
/// at all; the server's SMB port tells the two apart, so an unreachable server reads as such (53).
const CONNECT: &str = r"
function Test-Server {
    $server = $env:CINEVAULT_SHARE_REMOTE.TrimStart('\').Split('\')[0]
    $client = New-Object System.Net.Sockets.TcpClient
    try { return $client.ConnectAsync($server, 445).Wait(3000) } catch { return $false } finally { $client.Dispose() }
}
try {
    $reader = New-Object System.IO.StreamReader ([Console]::OpenStandardInput()), (New-Object System.Text.UTF8Encoding $false)
    $password = $reader.ReadLine()
    if ($null -eq $password) { $password = '' }
    $code = [CineVaultShare]::Connect($env:CINEVAULT_SHARE_REMOTE, $env:CINEVAULT_SHARE_USER, $password)
    $password = $null
    if ($code -eq 67 -and -not (Test-Server)) { $code = 53 }
    if ($code -eq 0) {
        Write-Result $true 0 '' $true
    } else {
        Write-Result $false $code (Get-Win32Message $code) $false
    }
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
    $code = 0
    if (Get-DevicelessMapping) {
        $code = [CineVaultShare]::Disconnect($env:CINEVAULT_SHARE_REMOTE)
    }
    if ($code -eq 0) {
        Write-Result $true 0 '' $false
    } else {
        Write-Result $false $code (Get-Win32Message $code) (Test-Mapped)
    }
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

/// Connects shares with `WNetAddConnection2W`.
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

    /// The command that runs the `parts` of a script for `share`, with `user` when connecting.
    fn command(&self, parts: &[&str], share: &ShareAddress, user: Option<&str>) -> ToolCommand {
        let mut command = ToolCommand::new(&self.powershell)
            .arg("-NoProfile")
            .arg("-NonInteractive")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-EncodedCommand")
            .arg(encode_script(parts))
            .env(REMOTE_VAR, share.unc());
        if let Some(user) = user {
            command = command.env(USER_VAR, user);
        }
        command
    }

    /// Runs the connect script for `share`: the password goes to its standard input only.
    fn map_connection(
        &self,
        share: &ShareAddress,
        login: &Login,
        cancel: &Cancel,
    ) -> Result<ScriptResult, ShareError> {
        let command = self.command(&[NATIVE, CONNECT], share, Some(login.user()));
        let password = login.password();
        self.run(
            &command,
            Some(password.expose()),
            cancel,
            CONNECT_LIMIT,
            |text| password.scrub(text),
        )
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
    ) -> Result<Connected, ShareError> {
        let root = PathBuf::from(share.unc());
        if self.is_connected(share, cancel)? {
            return Ok(Connected { root, made: false });
        }
        let result = self.map_connection(share, login, cancel)?;
        if result.ok {
            Ok(Connected { root, made: true })
        } else {
            let message = login
                .password()
                .scrub(result.message.as_deref().unwrap_or_default());
            Err(error_for(result.code, message))
        }
    }

    fn is_connected(&self, share: &ShareAddress, cancel: &Cancel) -> Result<bool, ShareError> {
        let result = self.run(
            &self.command(&[CHECK], share, None),
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
            &self.command(&[NATIVE, DISCONNECT], share, None),
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

/// The script as `-EncodedCommand` takes it: the prelude and `parts`, UTF-16LE, base64.
fn encode_script(parts: &[&str]) -> String {
    let text = format!("{PRELUDE}{}", parts.concat());
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

        let connected = connector
            .connect(&share(), &login(), &Cancel::new())
            .unwrap();

        assert_eq!(connected.root, PathBuf::from(r"\\nas\movies"));
        assert!(connected.made);
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
        let script = decoded(connect);
        assert!(script.contains("WNetAddConnection2W"));
        assert!(script.contains("[CineVaultShare]::Connect("));
        assert!(
            script.contains("WNetAddConnection2W(resource, password, user, 0)"),
            "not remembered in the profile"
        );
        // The password is a .NET method argument, never a cmdlet parameter that module logging
        // would record.
        assert!(!script.contains("-Password"), "{script}");
        assert!(!script.contains("New-SmbMapping"));
        assert!(!script.contains("LocalName ="), "no drive letter");
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
        let connected = connector
            .connect(&share(), &login(), &Cancel::new())
            .unwrap();
        assert_eq!(connected.root, PathBuf::from(r"\\nas\movies"));
        assert!(!connected.made, "connected before: not CineVault's");
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
        assert!(script.contains("WNetCancelConnection2W(remote, 0, false)"));
        assert!(!script.contains("Remove-SmbMapping"));
        assert!(script.contains("if (Get-DevicelessMapping)"));
    }

    #[test]
    fn only_connections_without_a_drive_letter_count() {
        // A drive letter the user mapped to the same share is neither "connected" for CineVault
        // nor ever disconnected by it.
        assert!(PRELUDE.contains("Where-Object { -not $_.LocalPath }"));
        assert!(PRELUDE.contains("Get-DevicelessMapping | Where-Object"));
        assert!(!CHECK.contains("Get-SmbMapping") && !DISCONNECT.contains("Get-SmbMapping"));
        assert!(DISCONNECT.contains("Get-DevicelessMapping"));
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
            let text = format!("{PRELUDE}{NATIVE}{script}");
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
        let refused = match connector.connect(&own, &stranger, &cancel) {
            Ok(connected) => {
                // Never leave a connection behind, even if this computer accepted the login.
                if connected.made {
                    connector.disconnect(&own, &cancel).unwrap();
                }
                panic!("a made-up user was accepted: {connected:?}");
            }
            Err(refused) => refused,
        };
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
