//! Network shares against a real SMB server, with the system's own connector: a wrong password,
//! a connect with the password remembered and a scan, a restart that connects the share again, a
//! remembered password that has become wrong, forgetting the share once its folder is removed,
//! and a share the user connected first, which CineVault uses but never disconnects.
//!
//! Opt-in, Windows only: set `CINEVAULT_LIVE_SMB_ADDRESS` (`\\server\share`, a throwaway server),
//! `CINEVAULT_LIVE_SMB_USER` and `CINEVAULT_LIVE_SMB_PASSWORD`. The share must hold
//! `Films\Heat (1995)\Heat (1995).mkv` and `Films\Alien (1979)\Alien (1979).mkv` (tiny files
//! do). The test connects only to that address, never with a drive letter, keeps the password
//! under a credential-store service of its own and removes it, its connections and its data
//! folder when it ends. Without the variables the test is skipped.
#![cfg(windows)]

use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use cv_app::data_location::{DataLocation, LocationInputs};
use cv_app::data_sources::DataSourceService;
use cv_app::network_shares::{ConnectRequest, OfflineReason, secret_account};
use cv_app::scanning::SourceState;
use cv_app::secrets::SecretStore;
use cv_app::services::{AppServices, BootOptions};
use cv_scan::sources::SourceGuard;
use cv_shares::{Address, Cancel, Login, Password, ShareAddress};
use tempfile::TempDir;

struct Live {
    address: Address,
    user: String,
    password: String,
}

fn live() -> Option<Live> {
    let var = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
    let (address, user, password) = (
        var("CINEVAULT_LIVE_SMB_ADDRESS")?,
        var("CINEVAULT_LIVE_SMB_USER")?,
        var("CINEVAULT_LIVE_SMB_PASSWORD")?,
    );
    let mut address = Address::parse(&address).expect("CINEVAULT_LIVE_SMB_ADDRESS");
    address.folder = vec!["Films".to_owned()];
    Some(Live {
        address,
        user,
        password,
    })
}

/// Runs a PowerShell command with the share's network path in `CV_REMOTE` (and the login in
/// `CV_USER` and `CV_PASSWORD` when given), never in the command text.
fn powershell(script: &str, remote: &str, login: Option<(&str, &str)>) -> String {
    let mut command = Command::new("powershell");
    command
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("CV_REMOTE", remote);
    if let Some((user, password)) = login {
        command.env("CV_USER", user).env("CV_PASSWORD", password);
    }
    let output = command.output().expect("powershell");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// The connections to the share as `LocalPath|Status`, one per line.
fn mappings(remote: &str) -> Vec<String> {
    powershell(
        "Get-SmbMapping -RemotePath $env:CV_REMOTE -ErrorAction SilentlyContinue | \
         ForEach-Object { \"$($_.LocalPath)|$($_.Status)\" }",
        remote,
        None,
    )
    .lines()
    .map(str::to_owned)
    .collect()
}

/// Removes every connection to the test share and the passwords the test may have stored.
struct Cleanup {
    remote: String,
    secrets: Arc<SecretStore>,
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        powershell(
            "Get-SmbMapping -RemotePath $env:CV_REMOTE -ErrorAction SilentlyContinue | \
             Remove-SmbMapping -Force -ErrorAction SilentlyContinue",
            &self.remote,
            None,
        );
        for id in 1..=20 {
            let _ = self.secrets.delete(&secret_account(id));
        }
    }
}

/// Ends the session Windows keeps to the server after its last connection ended, as a restart of
/// the computer does. While it lasts, the next connect with another password is not checked but
/// refused with error 1219 (another user), and waiting does not end it; a failed login through
/// `New-SmbMapping` does. The probe's password cannot be right, so it connects nothing.
fn end_windows_session(remote: &str, user: &str) {
    let answer = powershell(
        "try { New-SmbMapping -RemotePath $env:CV_REMOTE -UserName $env:CV_USER \
         -Password $env:CV_PASSWORD -ErrorAction Stop | Out-Null; 'connected' } \
         catch { $_.FullyQualifiedErrorId }",
        remote,
        Some((user, "cv-live-probe-never-right")),
    );
    eprintln!("   ended Windows' session to the server ({answer})");
    assert!(answer.contains("1326"), "the probe answered {answer}");
}

fn location(root: &Path) -> DataLocation {
    let inputs = LocationInputs {
        exe_dir: root.join("app"),
        user_data_base: root.join("userdata"),
        user_config_base: root.join("userconfig"),
    };
    std::fs::create_dir_all(&inputs.exe_dir).unwrap();
    DataLocation::resolve(&inputs)
}

fn boot(root: &Path, secrets: &Arc<SecretStore>) -> Arc<AppServices> {
    let options = BootOptions::new(location(root), Arc::clone(secrets)).without_providers();
    AppServices::boot(options).expect("boot")
}

fn availability(services: &AppServices, folder: &str) -> Option<String> {
    let pattern = format!("%{folder}%");
    services
        .db()
        .read(move |c| {
            Ok(c.query_row(
                "SELECT availability FROM item WHERE kind = 'movie' AND path LIKE ?1",
                [pattern],
                |row| row.get::<_, String>(0),
            )
            .ok())
        })
        .unwrap()
}

fn idle(services: &AppServices) -> bool {
    services
        .jobs()
        .list()
        .iter()
        .all(|job| job.state.is_terminal())
}

fn until(what: &str, mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(90);
    while !check() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Waits until both titles read `state`, noting every state seen on the way.
fn titles_become(services: &AppServices, state: &str, seen: &mut Vec<Option<String>>) {
    until(&format!("the titles {state}"), || {
        let now = [
            availability(services, "Heat"),
            availability(services, "Alien"),
        ];
        seen.extend(now.iter().cloned());
        idle(services) && now.iter().all(|s| s.as_deref() == Some(state))
    });
}

fn login(live: &Live, password: &str) -> Login {
    Login::new(&live.user, Password::new(password).unwrap()).unwrap()
}

fn request(live: &Live, password: &str, remember: bool) -> ConnectRequest {
    ConnectRequest {
        address: live.address.clone(),
        kind: None,
        login: login(live, password),
        remember,
    }
}

fn remove_folders(services: &AppServices, root: &Path) {
    let sources = DataSourceService::new(
        Arc::clone(services.db()),
        SourceGuard::for_current_os(&[root.to_string_lossy().into_owned()]),
    );
    for source in sources.list().unwrap() {
        sources.remove(source.id).unwrap();
    }
}

fn forget_unused(services: &AppServices) {
    until("the jobs to end", || idle(services));
    services.shares().sweep(Duration::ZERO);
}

#[test]
fn a_real_share_connects_reconnects_fails_and_is_forgotten() {
    let Some(live) = live() else {
        eprintln!(
            "skipped: set CINEVAULT_LIVE_SMB_ADDRESS, CINEVAULT_LIVE_SMB_USER and \
             CINEVAULT_LIVE_SMB_PASSWORD"
        );
        return;
    };
    let share: ShareAddress = live.address.share.clone();
    let remote = share.unc();
    let mut nonce = [0u8; 8];
    getrandom::fill(&mut nonce).unwrap();
    let service = format!(
        "CineVault live test {}",
        nonce.iter().map(|b| format!("{b:02x}")).collect::<String>()
    );
    let secrets = Arc::new(SecretStore::system_with_service(service));
    let _cleanup = Cleanup {
        remote: remote.clone(),
        secrets: Arc::clone(&secrets),
    };
    assert!(
        mappings(&remote).is_empty(),
        "the test share is connected already"
    );
    let connector = cv_shares::system_connector();
    let temp = TempDir::new().unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();

    // 1. A wrong password: a plain message, nothing connected, nothing kept.
    let services = boot(temp.path(), &secrets);
    runtime.block_on(async { services.start() });
    let error = services
        .shares()
        .connect_and_add(&request(&live, "not-the-password", true), &Cancel::new())
        .expect_err("a wrong password connects");
    eprintln!("1. wrong password: {} ({})", error, error.code());
    assert_eq!(error.code(), "wrong_login");
    assert_eq!(error.to_string(), "Wrong user name or password.");
    assert!(mappings(&remote).is_empty(), "{:?}", mappings(&remote));
    assert!(services.shares().list().unwrap().is_empty());

    // 2. The right password, remembered: connected without a drive letter, the folder added and
    //    scanned, its titles online.
    let added = services
        .shares()
        .connect_and_add(&request(&live, &live.password, true), &Cancel::new())
        .expect("the share connects");
    let id = added.share.id;
    let account = secret_account(id);
    eprintln!(
        "2. added {} on {}",
        added.source.path, added.share.local_root
    );
    assert_eq!(mappings(&remote), vec!["|OK".to_owned()]);
    assert!(added.share.connected_by_us && added.share.remember);
    assert!(secrets.contains(&account).unwrap());
    runtime
        .block_on(async { services.scan().rescan_source(added.source.id) })
        .unwrap();
    let mut seen = Vec::new();
    titles_become(&services, "online", &mut seen);
    services.shutdown();
    drop(services);

    // 3. A restart of the computer: the connection is gone; at start the share connects again
    //    with the remembered password and its titles stay online, never missing.
    connector.disconnect(&share, &Cancel::new()).unwrap();
    end_windows_session(&remote, &live.user);
    assert!(mappings(&remote).is_empty());
    let services = boot(temp.path(), &secrets);
    runtime.block_on(async { services.start() });
    let mut seen = Vec::new();
    until("the share to connect at start", || {
        seen.push(availability(&services, "Heat"));
        let rows = services.shares().list().unwrap();
        services.shares().state_of(&rows[0]).is_none()
            && rows[0].last_connected_ms > added.share.last_connected_ms
    });
    titles_become(&services, "online", &mut seen);
    eprintln!("3. after the restart: {:?}", mappings(&remote));
    assert_eq!(mappings(&remote), vec!["|OK".to_owned()]);
    assert!(
        seen.iter().all(|s| s.as_deref() == Some("online")),
        "{seen:?}"
    );
    services.shutdown();
    drop(services);

    // 4. The remembered password became wrong: the folder is offline with its reason, the titles
    //    offline, never missing.
    connector.disconnect(&share, &Cancel::new()).unwrap();
    end_windows_session(&remote, &live.user);
    secrets.set(&account, "not-the-password").unwrap();
    let services = boot(temp.path(), &secrets);
    runtime.block_on(async { services.start() });
    until("the connect at start to fail", || {
        services.shares().list().unwrap()[0].last_error.is_some()
    });
    let row = services.shares().list().unwrap()[0].clone();
    eprintln!("4. the connect at start: {:?}", row.last_error);
    assert_eq!(row.last_error.as_deref(), Some("wrong_login"));
    let mut seen = Vec::new();
    titles_become(&services, "offline", &mut seen);
    let health = services.scan().source_health().unwrap();
    let films = &health[0];
    eprintln!(
        "4. wrong stored password: {:?} {:?}",
        films.state,
        films.reason.map(OfflineReason::label)
    );
    assert_eq!(films.state, SourceState::Offline);
    assert_eq!(
        films.reason.map(OfflineReason::label),
        Some("Wrong password")
    );
    assert!(
        seen.iter()
            .all(|s| matches!(s.as_deref(), Some("online" | "offline"))),
        "{seen:?}"
    );
    assert!(mappings(&remote).is_empty());

    // Connect… with the right password brings it back.
    services
        .shares()
        .connect_again(id, &login(&live, &live.password), true, &Cancel::new())
        .expect("connects again");
    let mut seen = Vec::new();
    titles_become(&services, "online", &mut seen);
    assert_eq!(mappings(&remote), vec!["|OK".to_owned()]);

    // 5. The folder removed: the sweep forgets the share, ends CineVault's connection and deletes
    //    the password.
    remove_folders(&services, temp.path());
    forget_unused(&services);
    eprintln!("5. after the sweep: {:?}", mappings(&remote));
    assert!(services.shares().list().unwrap().is_empty());
    assert!(mappings(&remote).is_empty(), "{:?}", mappings(&remote));
    assert!(!secrets.contains(&account).unwrap());

    // 6. The user connected the share first: CineVault uses that connection, keeps no password
    //    for it, and leaves it alone when the share is forgotten.
    powershell(
        "New-SmbMapping -RemotePath $env:CV_REMOTE -UserName $env:CV_USER \
         -Password $env:CV_PASSWORD | Out-Null",
        &remote,
        Some((&live.user, &live.password)),
    );
    let own = mappings(&remote);
    eprintln!("6. the user's own connection: {own:?}");
    assert_eq!(
        own,
        vec!["|OK".to_owned()],
        "the user's connection was not made"
    );
    let added = services
        .shares()
        .connect_and_add(&request(&live, &live.password, true), &Cancel::new())
        .expect("adds on the user's connection");
    assert!(!added.share.connected_by_us && !added.share.remember);
    assert!(!secrets.contains(&secret_account(added.share.id)).unwrap());
    remove_folders(&services, temp.path());
    forget_unused(&services);
    assert!(services.shares().list().unwrap().is_empty());
    eprintln!("6. after the sweep: {:?}", mappings(&remote));
    assert_eq!(
        mappings(&remote),
        own,
        "CineVault ended the user's connection"
    );
    services.shutdown();
    drop(services);
    powershell(
        "Remove-SmbMapping -RemotePath $env:CV_REMOTE -Force",
        &remote,
        None,
    );
    assert!(mappings(&remote).is_empty());
}
