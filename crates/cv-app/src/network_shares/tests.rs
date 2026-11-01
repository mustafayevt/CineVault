use std::path::{Path, PathBuf};
use std::time::Duration;

use cv_scan::sources::{PathStyle, SourceGuard, SystemFolders};
use cv_shares::ShareErrorKind;
use cv_shares::fake::{FakeCall, FakeConnector};
use tempfile::TempDir;

use super::*;
use crate::secrets::{MemoryBackend, SecretBackend};

const PASSWORD: &str = "Example-Pass-42";

/// A credential store that is not there, as on a Linux desktop without a Secret Service.
struct NoKeyring;

impl SecretBackend for NoKeyring {
    fn set(&self, _account: &str, _value: &str) -> Result<(), SecretError> {
        Err(SecretError::NoKeyring)
    }
    fn get(&self, _account: &str) -> Result<Option<String>, SecretError> {
        Err(SecretError::NoKeyring)
    }
    fn delete(&self, _account: &str) -> Result<bool, SecretError> {
        Err(SecretError::NoKeyring)
    }
}

/// A credential store that keeps passwords but answers every delete with `error`.
struct FailingDelete(MemoryBackend, fn() -> SecretError);

impl SecretBackend for FailingDelete {
    fn set(&self, account: &str, value: &str) -> Result<(), SecretError> {
        self.0.set(account, value)
    }
    fn get(&self, account: &str) -> Result<Option<String>, SecretError> {
        self.0.get(account)
    }
    fn delete(&self, _account: &str) -> Result<bool, SecretError> {
        Err((self.1)())
    }
}

/// A credential store that runs a hook (set later) on every delete.
#[derive(Default)]
struct DeleteHook(
    MemoryBackend,
    Arc<std::sync::OnceLock<Box<dyn Fn() + Send + Sync>>>,
);

impl SecretBackend for DeleteHook {
    fn set(&self, account: &str, value: &str) -> Result<(), SecretError> {
        self.0.set(account, value)
    }
    fn get(&self, account: &str) -> Result<Option<String>, SecretError> {
        self.0.get(account)
    }
    fn delete(&self, account: &str) -> Result<bool, SecretError> {
        if let Some(hook) = self.1.get() {
            hook();
        }
        self.0.delete(account)
    }
}

struct Rig {
    dir: TempDir,
    db: Arc<Database>,
    fake: Arc<FakeConnector>,
    secrets: Arc<SecretStore>,
    service: NetworkShareService,
}

fn rig_with(secrets: SecretStore) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(Database::open(dir.path().join("library.db")).unwrap());
    let guard = SourceGuard::new(
        PathStyle::current(),
        SystemFolders::new(PathStyle::current()),
    );
    let sources = DataSourceService::new(Arc::clone(&db), guard);
    let fake = Arc::new(FakeConnector::new(dir.path().join("shares")));
    let secrets = Arc::new(secrets);
    let service = NetworkShareService::new(
        sources,
        Arc::clone(&fake) as Arc<dyn ShareConnector>,
        Arc::clone(&secrets),
    );
    Rig {
        dir,
        db,
        fake,
        secrets,
        service,
    }
}

fn rig() -> Rig {
    rig_with(SecretStore::in_memory())
}

impl Rig {
    /// Where the fake shows `\\nas\movies`, with `folders` already on the share.
    fn share_with(&self, folders: &[&str]) -> PathBuf {
        let root = self.fake.root_of(&movies());
        for folder in folders {
            std::fs::create_dir_all(root.join(folder)).unwrap();
        }
        root
    }

    fn sources(&self) -> Vec<String> {
        self.db
            .read(|c| cv_db::repo::data_sources::list_sources(c, None))
            .unwrap()
            .into_iter()
            .map(|s| s.path)
            .collect()
    }
}

fn movies() -> cv_shares::ShareAddress {
    cv_shares::ShareAddress::new("nas", "movies").unwrap()
}

fn request(address: &str, password: &str, remember: bool) -> ConnectRequest {
    ConnectRequest {
        address: Address::parse(address).unwrap(),
        kind: Some(SourceKind::Movie),
        login: Login::new("alex", Password::new(password).unwrap()).unwrap(),
        remember,
    }
}

fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Every file below `dir` that holds `needle`.
fn files_holding(dir: &Path, needle: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(folder) = stack.pop() {
        for entry in std::fs::read_dir(&folder).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if std::fs::read(&path)
                .unwrap_or_default()
                .windows(needle.len())
                .any(|w| w == needle.as_bytes())
            {
                found.push(path);
            }
        }
    }
    found
}

#[test]
fn a_share_folder_is_added_and_its_password_remembered() {
    let rig = rig();
    let root = rig.share_with(&["Films"]);

    let added = rig
        .service
        .connect_and_add(
            &request(r"\\nas\movies\Films", PASSWORD, true),
            &Cancel::new(),
        )
        .unwrap();

    assert_eq!(added.source.path, text(&root.join("Films")));
    assert_eq!(added.source.kind, SourceKind::Movie);
    assert_eq!(
        (added.share.server.as_str(), added.share.share.as_str()),
        ("nas", "movies")
    );
    assert_eq!(added.share.user, "alex");
    assert_eq!(added.share.local_root, text(&root));
    assert!(added.share.remember);
    assert!(added.share.last_connected_ms.is_some());
    assert_eq!(rig.service.list().unwrap(), vec![added.share.clone()]);
    assert_eq!(rig.sources(), vec![text(&root.join("Films"))]);
    assert_eq!(
        rig.secrets
            .get(&secret_account(added.share.id))
            .unwrap()
            .map(|s| s.expose().to_owned()),
        Some(PASSWORD.to_owned())
    );
    assert!(rig.service.unsaved_password(added.share.id).is_none());
    assert!(
        added.share.connected_by_us,
        "the connect made the connection"
    );
    assert!(matches!(
        &rig.fake.calls()[..],
        [FakeCall::Connect { user, .. }] if user == "alex"
    ));
}

#[test]
fn a_second_folder_on_the_same_share_uses_its_row() {
    let rig = rig();
    rig.share_with(&["Films", "Documentaries"]);
    let first = rig
        .service
        .connect_and_add(
            &request(r"\\nas\movies\Films", PASSWORD, true),
            &Cancel::new(),
        )
        .unwrap();
    let second = rig
        .service
        .connect_and_add(
            &request("smb://nas/movies/Documentaries", PASSWORD, true),
            &Cancel::new(),
        )
        .unwrap();
    assert_eq!(first.share.id, second.share.id);
    assert_eq!(rig.service.list().unwrap().len(), 1);
    assert_eq!(rig.sources().len(), 2);
}

#[test]
fn a_folder_that_is_not_on_the_share_adds_nothing_and_disconnects() {
    let rig = rig();
    rig.share_with(&[]);

    let error = rig
        .service
        .connect_and_add(
            &request(r"\\nas\movies\Films", PASSWORD, true),
            &Cancel::new(),
        )
        .unwrap_err();

    assert!(
        matches!(&error, ShareConnectError::Share(e) if e.kind() == ShareErrorKind::FolderNotFound)
    );
    assert_eq!(error.code(), "folder_not_found");
    assert!(rig.sources().is_empty());
    assert!(rig.service.list().unwrap().is_empty());
    assert!(rig.fake.calls().contains(&FakeCall::Disconnect(movies())));
    assert!(!rig.secrets.contains(&secret_account(1)).unwrap());
}

#[test]
fn a_folder_inside_a_library_folder_is_refused_with_the_usual_message() {
    let rig = rig();
    rig.share_with(&["Action"]);
    rig.service
        .connect_and_add(&request(r"\\nas\movies", PASSWORD, true), &Cancel::new())
        .unwrap();

    let error = rig
        .service
        .connect_and_add(
            &request(r"\\nas\movies\Action", PASSWORD, true),
            &Cancel::new(),
        )
        .unwrap_err();

    let ShareConnectError::Folder(rule) = &error else {
        panic!("expected the folder rules, got {error:?}");
    };
    let usual = rig
        .service
        .sources
        .add(
            SourceKind::Movie,
            &text(&rig.fake.root_of(&movies()).join("Action")),
        )
        .unwrap_err();
    assert_eq!(rule.code(), usual.code());
    assert_eq!(rule.to_string(), usual.to_string());
    assert_eq!(rig.sources().len(), 1);
    // The share stays: the first folder still uses it.
    assert_eq!(rig.service.list().unwrap().len(), 1);
    assert!(!rig.fake.calls().contains(&FakeCall::Disconnect(movies())));
}

#[test]
fn without_a_keyring_nothing_is_kept_until_the_user_connects_without_remembering() {
    let rig = rig_with(SecretStore::with_backend(NoKeyring));
    rig.share_with(&["Films"]);

    let error = rig
        .service
        .connect_and_add(
            &request(r"\\nas\movies\Films", PASSWORD, true),
            &Cancel::new(),
        )
        .unwrap_err();
    assert!(matches!(
        error,
        ShareConnectError::Credential(SecretError::NoKeyring)
    ));
    assert_eq!(error.code(), "no_keyring");
    assert!(
        rig.service.list().unwrap().is_empty(),
        "the row is rolled back"
    );
    assert!(rig.sources().is_empty());

    let added = rig
        .service
        .connect_and_add(
            &request(r"\\nas\movies\Films", PASSWORD, false),
            &Cancel::new(),
        )
        .unwrap();
    assert!(!added.share.remember);
    assert_eq!(
        rig.service
            .unsaved_password(added.share.id)
            .map(|p| p.expose().to_owned()),
        Some(PASSWORD.to_owned())
    );
    assert!(files_holding(rig.dir.path(), PASSWORD).is_empty());
}

#[test]
fn no_file_holds_the_password_whether_remembered_or_not() {
    // Passwords of this test only: the log redaction is shared by every test in the process.
    let (kept, unsaved) = ("Remembered-Pass-31", "Unsaved-Pass-77");
    let rig = rig();
    rig.share_with(&["Films"]);
    let series = cv_shares::ShareAddress::new("nas", "series").unwrap();
    std::fs::create_dir_all(rig.fake.root_of(&series)).unwrap();
    rig.service
        .connect_and_add(&request(r"\\nas\movies\Films", kept, true), &Cancel::new())
        .unwrap();
    let mut tv = request(r"\\nas\series", unsaved, false);
    tv.kind = Some(SourceKind::Tv);
    rig.service.connect_and_add(&tv, &Cancel::new()).unwrap();

    for secret in [kept, unsaved] {
        assert_eq!(files_holding(rig.dir.path(), secret), Vec::<PathBuf>::new());
        assert_eq!(
            redact::redact(&format!("log {secret} line")),
            "log [REDACTED] line"
        );
    }
}

#[test]
fn a_short_password_never_shows_in_a_formatted_error() {
    let short = "pw4";
    let rig = rig();
    rig.share_with(&[]);
    let attempt = request(r"\\nas\movies\Films", short, true);

    let mut errors = vec![
        rig.service
            .connect_and_add(&attempt, &Cancel::new())
            .unwrap_err(),
    ];
    for kind in [
        ShareErrorKind::WrongLogin,
        ShareErrorKind::Unreachable,
        ShareErrorKind::Other,
    ] {
        rig.fake
            .fail_connect(&movies(), Some(ShareError::new(kind, "system said no")));
        errors.push(
            rig.service
                .connect_and_add(&attempt, &Cancel::new())
                .unwrap_err(),
        );
    }
    for error in &errors {
        for shown in [error.to_string(), format!("{error:?}")] {
            assert!(!shown.contains(short), "{shown}");
        }
    }
    assert!(!format!("{attempt:?}").contains(short));
    assert_eq!(errors[1].code(), "wrong_login");
    assert!(rig.service.list().unwrap().is_empty());
    assert!(rig.sources().is_empty());
}

// ---- at start ------------------------------------------------------------------------------

/// The same library after a restart of the computer: a new service, and a fake on which no share
/// is connected.
fn restarted(rig: &Rig) -> (Arc<NetworkShareService>, Arc<FakeConnector>) {
    let guard = SourceGuard::new(
        PathStyle::current(),
        SystemFolders::new(PathStyle::current()),
    );
    let fake = Arc::new(FakeConnector::new(rig.dir.path().join("shares")));
    let service = NetworkShareService::new(
        DataSourceService::new(Arc::clone(&rig.db), guard),
        Arc::clone(&fake) as Arc<dyn ShareConnector>,
        Arc::clone(&rig.secrets),
    );
    (Arc::new(service), fake)
}

/// Connects `\server\share` and adds its folder `Films`.
fn add(rig: &Rig, server: &str, share: &str, remember: bool) -> ShareAdded {
    let address = cv_shares::ShareAddress::new(server, share).unwrap();
    std::fs::create_dir_all(rig.fake.root_of(&address).join("Films")).unwrap();
    rig.service
        .connect_and_add(
            &request(&format!(r"\\{server}\{share}\Films"), PASSWORD, remember),
            &Cancel::new(),
        )
        .unwrap()
}

type Events = Arc<Mutex<Vec<Vec<i64>>>>;

fn record_hand_overs(service: &NetworkShareService) -> Events {
    let events: Events = Arc::default();
    let sink = Arc::clone(&events);
    service.set_after_connect(Arc::new(move |folders| sink.lock().unwrap().push(folders)));
    events
}

fn wait_for(events: &Events, count: usize) {
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while events.lock().unwrap().len() < count {
        assert!(std::time::Instant::now() < deadline, "timed out");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn row(service: &NetworkShareService, id: i64) -> NetworkShare {
    service
        .list()
        .unwrap()
        .into_iter()
        .find(|s| s.id == id)
        .unwrap()
}

#[test]
fn remembered_shares_reconnect_per_server_and_record_why_one_did_not() {
    let rig = rig();
    let movies = add(&rig, "nas", "movies", true);
    let tv = add(&rig, "nas", "tv", true);
    let private = add(&rig, "nas", "private", false);
    let films = add(&rig, "box", "films", true);

    let (service, fake) = restarted(&rig);
    fake.set_delay(Duration::from_millis(150));
    let tv_address = cv_shares::ShareAddress::new("nas", "tv").unwrap();
    fake.fail_connect(
        &tv_address,
        Some(ShareError::new(ShareErrorKind::WrongLogin, "logon failure")),
    );
    let box_address = cv_shares::ShareAddress::new("box", "films").unwrap();
    fake.mark_connected(&box_address);
    let events = record_hand_overs(&service);

    let waiting = service.reconnect_at_start();
    assert_eq!(waiting.len(), 4);
    assert_eq!(
        service.folder_state(&movies.source.path),
        Some(ShareState::Connecting)
    );
    assert_eq!(service.folder_state(&text(rig.dir.path())), None);
    wait_for(&events, 4);

    // The share already connected on the other server is handed over first, without waiting
    // for the slow server.
    let events = events.lock().unwrap().clone();
    assert_eq!(events[0], vec![films.source.id]);
    assert!(events.contains(&vec![movies.source.id]));
    assert!(events.contains(&vec![tv.source.id]));
    assert!(events.contains(&vec![private.source.id]));

    assert_eq!(row(&service, movies.share.id).last_error, None);
    assert!(row(&service, movies.share.id).last_connected_ms >= movies.share.last_connected_ms);
    assert_eq!(
        row(&service, tv.share.id).last_error.as_deref(),
        Some("wrong_login")
    );
    assert_eq!(
        row(&service, private.share.id).last_error.as_deref(),
        Some(PASSWORD_NEEDED)
    );
    assert_eq!(row(&service, films.share.id).last_error, None);
    assert_eq!(service.folder_state(&movies.source.path), None);

    let connects: Vec<(String, String)> = fake
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            FakeCall::Connect {
                share, password, ..
            } => Some((share.share().to_owned(), password.expose().to_owned())),
            _ => None,
        })
        .collect();
    assert_eq!(connects.len(), 2, "{connects:?}");
    assert!(connects.contains(&("movies".to_owned(), PASSWORD.to_owned())));
    assert!(connects.contains(&("tv".to_owned(), PASSWORD.to_owned())));
}

#[test]
fn a_share_that_does_not_answer_in_time_is_unreachable_for_this_start() {
    let rig = rig();
    let movies = add(&rig, "nas", "movies", true);
    let (service, fake) = restarted(&rig);
    let mut service = Arc::into_inner(service).unwrap();
    service.reconnect_limit = Duration::from_millis(50);
    let service = Arc::new(service);
    fake.set_delay(Duration::from_secs(30));
    let events = record_hand_overs(&service);

    let started = std::time::Instant::now();
    service.reconnect_at_start();
    wait_for(&events, 1);

    assert!(started.elapsed() < Duration::from_secs(10));
    assert_eq!(
        row(&service, movies.share.id).last_error.as_deref(),
        Some("unreachable")
    );
}

#[test]
fn connecting_again_brings_the_folders_back_and_a_wrong_password_is_recorded() {
    let rig = rig();
    let private = add(&rig, "nas", "private", false);
    let (service, fake) = restarted(&rig);
    let events = record_hand_overs(&service);
    service.reconnect_at_start();
    wait_for(&events, 1);
    events.lock().unwrap().clear();

    let address = cv_shares::ShareAddress::new("nas", "private").unwrap();
    fake.fail_connect(
        &address,
        Some(ShareError::new(ShareErrorKind::WrongLogin, "logon failure")),
    );
    let login = Login::new("alex", Password::new("Old-Pass-1").unwrap()).unwrap();
    let error = service
        .connect_again(private.share.id, &login, true, &Cancel::new())
        .unwrap_err();
    assert_eq!(error.code(), "wrong_login");
    assert_eq!(
        row(&service, private.share.id).last_error.as_deref(),
        Some("wrong_login")
    );
    assert!(events.lock().unwrap().is_empty());

    fake.fail_connect(&address, None);
    let login = Login::new(r"HOME\alex", Password::new("New-Pass-2").unwrap()).unwrap();
    let share = service
        .connect_again(private.share.id, &login, true, &Cancel::new())
        .unwrap();
    assert_eq!(share.last_error, None);
    assert!(share.remember);
    assert_eq!(share.user, r"HOME\alex");
    assert_eq!(
        rig.secrets
            .get(&secret_account(share.id))
            .unwrap()
            .map(|s| s.expose().to_owned()),
        Some("New-Pass-2".to_owned())
    );
    assert_eq!(*events.lock().unwrap(), vec![vec![private.source.id]]);

    assert_eq!(
        service
            .connect_again(9_999, &login, true, &Cancel::new())
            .unwrap_err()
            .code(),
        "unknown_share"
    );
}

// ---- offline reasons -------------------------------------------------------------------------

#[test]
fn failure_codes_read_as_the_reasons_of_the_folder_row() {
    let reasons = [
        (
            "wrong_login",
            OfflineReason::WrongPassword,
            "Wrong password",
        ),
        (
            "unreachable",
            OfflineReason::Unreachable,
            "Server not reachable",
        ),
        (
            "share_not_found",
            OfflineReason::ShareNotFound,
            "Share not found",
        ),
        (
            PASSWORD_NEEDED,
            OfflineReason::PasswordNeeded,
            "Password needed",
        ),
        (
            "account_locked",
            OfflineReason::NotConnected,
            "Not connected",
        ),
        (
            "something new",
            OfflineReason::NotConnected,
            "Not connected",
        ),
    ];
    for (code, reason, label) in reasons {
        assert_eq!(OfflineReason::from_code(code), reason, "{code}");
        assert_eq!(reason.label(), label);
    }
}

#[test]
fn a_folder_of_a_share_that_did_not_connect_says_why() {
    let rig = rig();
    let movies = add(&rig, "nas", "movies", false);
    let (service, _fake) = restarted(&rig);
    let events = record_hand_overs(&service);
    service.reconnect_at_start();
    wait_for(&events, 1);

    assert_eq!(
        service.folder_state(&movies.source.path),
        Some(ShareState::Failed(OfflineReason::PasswordNeeded))
    );
    assert_eq!(service.folder_state(&text(rig.dir.path())), None);
}

#[test]
fn the_mount_folder_of_the_macos_connector_is_the_one_folder_health_checks() {
    assert_eq!(cv_shares::MAC_MOUNT_FOLDER, paths::MAC_SHARE_MOUNTS);
}

// ---- forgetting ------------------------------------------------------------------------------

fn remove_folder(rig: &Rig, source: i64) {
    rig.service.sources.remove(source).unwrap();
}

#[test]
fn removing_the_last_folder_marks_the_share_and_adding_one_again_clears_the_mark() {
    let rig = rig();
    rig.share_with(&["Films", "Series"]);
    let films = rig
        .service
        .connect_and_add(
            &request(r"\\nas\movies\Films", PASSWORD, true),
            &Cancel::new(),
        )
        .unwrap();
    let series = rig
        .service
        .connect_and_add(
            &request(r"\\nas\movies\Series", PASSWORD, true),
            &Cancel::new(),
        )
        .unwrap();
    let id = films.share.id;

    remove_folder(&rig, films.source.id);
    assert_eq!(
        row(&rig.service, id).unused_since_ms,
        None,
        "Series still uses it"
    );
    remove_folder(&rig, series.source.id);
    assert!(row(&rig.service, id).unused_since_ms.is_some());

    // Undo puts the folder back: the share is used again and keeps its password.
    rig.service
        .sources
        .add(SourceKind::Movie, &series.source.path)
        .unwrap();
    assert_eq!(row(&rig.service, id).unused_since_ms, None);
    assert_eq!(rig.service.sweep(Duration::ZERO), 0);
    assert!(rig.secrets.contains(&secret_account(id)).unwrap());
    assert!(!rig.fake.calls().contains(&FakeCall::Disconnect(movies())));
}

#[test]
fn an_unused_share_is_forgotten_a_minute_later_and_its_files_stay() {
    let rig = rig();
    let root = rig.share_with(&["Films"]);
    std::fs::write(root.join("Films").join("Heat (1995).mkv"), b"heat").unwrap();
    let films = rig
        .service
        .connect_and_add(
            &request(r"\\nas\movies\Films", PASSWORD, true),
            &Cancel::new(),
        )
        .unwrap();
    remove_folder(&rig, films.source.id);

    assert_eq!(rig.service.sweep(FORGET_AFTER), 0, "not within the minute");
    assert_eq!(rig.service.sweep(Duration::ZERO), 1);

    assert!(rig.service.list().unwrap().is_empty());
    assert!(
        !rig.secrets
            .contains(&secret_account(films.share.id))
            .unwrap()
    );
    assert!(rig.fake.calls().contains(&FakeCall::Disconnect(movies())));
    assert!(!rig.fake.is_connected(&movies(), &Cancel::new()).unwrap());
    assert_eq!(
        std::fs::read(root.join("Films").join("Heat (1995).mkv")).unwrap(),
        b"heat"
    );
}

#[test]
fn a_share_is_not_forgotten_while_a_job_runs() {
    let rig = rig();
    rig.share_with(&["Films"]);
    let films = rig
        .service
        .connect_and_add(
            &request(r"\\nas\movies\Films", PASSWORD, false),
            &Cancel::new(),
        )
        .unwrap();
    remove_folder(&rig, films.source.id);
    let running = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let flag = Arc::clone(&running);
    rig.service.set_busy(Arc::new(move || {
        flag.load(std::sync::atomic::Ordering::SeqCst)
    }));

    assert_eq!(rig.service.sweep(Duration::ZERO), 0);
    assert_eq!(rig.service.list().unwrap().len(), 1);
    assert!(!rig.fake.calls().contains(&FakeCall::Disconnect(movies())));

    running.store(false, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(rig.service.sweep(Duration::ZERO), 1);
    assert!(rig.service.unsaved_password(films.share.id).is_none());
}

#[test]
fn a_share_whose_files_are_open_is_forgotten_without_being_disconnected() {
    let rig = rig();
    rig.share_with(&["Films"]);
    let films = rig
        .service
        .connect_and_add(
            &request(r"\\nas\movies\Films", PASSWORD, true),
            &Cancel::new(),
        )
        .unwrap();
    remove_folder(&rig, films.source.id);
    rig.fake.fail_disconnect(
        &movies(),
        Some(ShareError::new(ShareErrorKind::InUse, "files are open")),
    );

    assert_eq!(rig.service.sweep(Duration::ZERO), 1);
    assert!(rig.service.list().unwrap().is_empty());
    assert!(
        !rig.secrets
            .contains(&secret_account(films.share.id))
            .unwrap()
    );
    assert!(rig.fake.is_connected(&movies(), &Cancel::new()).unwrap());
}

#[test]
fn an_unused_share_is_not_connected_again_at_start() {
    let rig = rig();
    let movies = add(&rig, "nas", "movies", true);
    remove_folder(&rig, movies.source.id);
    let (service, fake) = restarted(&rig);
    assert!(service.reconnect_at_start().is_empty());
    assert!(fake.calls().is_empty());
}

#[test]
fn only_empty_mount_folders_of_no_share_are_removed_and_never_while_mounted() {
    let rig = rig();
    let known = add(&rig, "nas", "movies", true);
    let (service, fake) = restarted(&rig);
    let mut service = Arc::into_inner(service).unwrap();
    // The fake shows shares below `shares`, as the macOS connector does below its mount folder.
    let base = rig.dir.path().join("shares");
    service.mount_folders = Some(base.clone());
    service.is_mounted = Arc::new(|folder: &Path| folder.ends_with("busy"));

    let nas = base.join("nas");
    let (gone, busy, full) = (nas.join("gone"), nas.join("busy"), nas.join("full"));
    for folder in [&gone, &busy, &full] {
        std::fs::create_dir_all(folder).unwrap();
    }
    std::fs::write(full.join("keep.txt"), b"mine").unwrap();
    let old_server = base.join("old");
    std::fs::create_dir_all(old_server.join("films")).unwrap();

    service.clean_mount_folders();

    assert!(!gone.exists(), "an empty folder that is not mounted goes");
    assert!(busy.exists(), "a folder that stays mounted is left alone");
    let busy_share = cv_shares::ShareAddress::new("nas", "busy").unwrap();
    assert!(fake.calls().contains(&FakeCall::Disconnect(busy_share)));
    assert_eq!(std::fs::read(full.join("keep.txt")).unwrap(), b"mine");
    assert!(Path::new(&known.share.local_root).join("Films").exists());
    assert!(!old_server.exists(), "an emptied server folder goes too");
    assert!(
        !fake
            .calls()
            .iter()
            .any(|call| matches!(call, FakeCall::Disconnect(share) if share.share() != "busy")),
        "only a mounted folder is unmounted"
    );
}

#[test]
fn a_folder_without_a_chosen_type_is_added_as_what_it_holds() {
    let rig = rig();
    let root = rig.share_with(&["Films", "Shows/Severance/Season 1"]);
    std::fs::write(
        root.join("Shows/Severance/Season 1/Severance - S01E01.mkv"),
        b"x",
    )
    .unwrap();
    let ask = |address: &str| ConnectRequest {
        kind: None,
        ..request(address, PASSWORD, true)
    };

    let shows = rig
        .service
        .connect_and_add(&ask(r"\\nas\movies\Shows"), &Cancel::new())
        .unwrap();
    let films = rig
        .service
        .connect_and_add(&ask(r"\\nas\movies\Films"), &Cancel::new())
        .unwrap();

    assert_eq!(shows.source.kind, SourceKind::Tv);
    assert_eq!(films.source.kind, SourceKind::Movie);
}

// ---- the user's own connections --------------------------------------------------------------

fn disconnects(fake: &FakeConnector) -> usize {
    fake.calls()
        .iter()
        .filter(|call| matches!(call, FakeCall::Disconnect(_)))
        .count()
}

#[test]
fn a_share_the_user_connected_is_used_as_it_is_and_never_disconnected() {
    let rig = rig();
    rig.share_with(&["Films"]);
    // The user's own connection (a drive letter, a file-manager mount) is there already.
    rig.fake.mark_connected(&movies());

    let added = rig
        .service
        .connect_and_add(
            &request(r"\\nas\movies\Films", PASSWORD, true),
            &Cancel::new(),
        )
        .unwrap();

    assert!(!added.share.connected_by_us);
    // The system did not check this login, so it is not kept.
    assert!(!added.share.remember);
    assert!(
        !rig.secrets
            .contains(&secret_account(added.share.id))
            .unwrap()
    );
    assert!(rig.service.unsaved_password(added.share.id).is_none());
    assert_eq!(rig.sources().len(), 1);

    remove_folder(&rig, added.source.id);
    assert_eq!(rig.service.sweep(Duration::ZERO), 1);
    assert!(rig.service.list().unwrap().is_empty());
    assert_eq!(disconnects(&rig.fake), 0);
    assert!(rig.fake.is_connected(&movies(), &Cancel::new()).unwrap());
}

#[test]
fn a_folder_not_added_on_the_users_own_connection_leaves_it_connected() {
    let rig = rig();
    rig.share_with(&[]);
    rig.fake.mark_connected(&movies());

    let error = rig
        .service
        .connect_and_add(
            &request(r"\\nas\movies\Films", PASSWORD, true),
            &Cancel::new(),
        )
        .unwrap_err();

    assert_eq!(error.code(), "folder_not_found");
    assert_eq!(disconnects(&rig.fake), 0);
    assert!(rig.fake.is_connected(&movies(), &Cancel::new()).unwrap());
}

#[test]
fn a_known_share_found_connected_keeps_its_login_and_owner() {
    let rig = rig();
    let films = add(&rig, "nas", "movies", true);
    std::fs::create_dir_all(rig.fake.root_of(&movies()).join("Series")).unwrap();

    // Connected already: the new, unchecked password does not replace the remembered one.
    let added = rig
        .service
        .connect_and_add(
            &request(r"\\nas\movies\Series", "Unchecked-Pass-9", false),
            &Cancel::new(),
        )
        .unwrap();

    assert_eq!(added.share.id, films.share.id);
    assert!(added.share.remember);
    assert!(added.share.connected_by_us, "made by the first connect");
    assert_eq!(
        rig.secrets
            .get(&secret_account(films.share.id))
            .unwrap()
            .map(|s| s.expose().to_owned()),
        Some(PASSWORD.to_owned())
    );
}

#[test]
fn a_check_that_fails_is_a_failure_and_never_counts_as_not_connected() {
    let rig = rig();
    rig.share_with(&["Films"]);
    rig.fake.fail_check(
        &movies(),
        Some(ShareError::new(
            ShareErrorKind::Other,
            "no answer from the system",
        )),
    );

    let error = rig
        .service
        .connect_and_add(
            &request(r"\\nas\movies\Films", PASSWORD, true),
            &Cancel::new(),
        )
        .unwrap_err();
    assert_eq!(error.code(), "other");
    assert_eq!(disconnects(&rig.fake), 0);

    // At start: the share is not connected over a check that failed, and its row stops claiming
    // the connection, so a connection that turns up later is never taken for CineVault's.
    rig.fake.fail_check(&movies(), None);
    let films = add(&rig, "nas", "movies", true);
    assert!(films.share.connected_by_us);
    let (service, fake) = restarted(&rig);
    fake.fail_check(
        &movies(),
        Some(ShareError::new(
            ShareErrorKind::Other,
            "no answer from the system",
        )),
    );
    let events = record_hand_overs(&service);
    service.reconnect_at_start();
    wait_for(&events, 1);
    let after = row(&service, films.share.id);
    assert_eq!(after.last_error.as_deref(), Some("other"));
    assert!(!after.connected_by_us);
    assert!(
        !fake
            .calls()
            .iter()
            .any(|call| matches!(call, FakeCall::Connect { .. }))
    );
}

#[test]
fn at_start_a_share_is_ours_only_when_this_start_or_an_earlier_one_connected_it() {
    let rig = rig();
    let movies_added = add(&rig, "nas", "movies", true);
    let tv = add(&rig, "nas", "tv", true);
    let found = add(&rig, "box", "films", true);
    let tv_address = cv_shares::ShareAddress::new("nas", "tv").unwrap();
    let box_address = cv_shares::ShareAddress::new("box", "films").unwrap();
    // The row of a share the user had connected before CineVault did.
    rig.db
        .write(move |tx| rows::set_connected_by_us(tx, found.share.id, false))
        .unwrap();

    let (service, fake) = restarted(&rig);
    fake.fail_connect(
        &tv_address,
        Some(ShareError::new(ShareErrorKind::WrongLogin, "logon failure")),
    );
    fake.mark_connected(&box_address);
    let events = record_hand_overs(&service);
    service.reconnect_at_start();
    wait_for(&events, 3);

    assert!(
        row(&service, movies_added.share.id).connected_by_us,
        "reconnected now"
    );
    assert!(
        !row(&service, tv.share.id).connected_by_us,
        "its connection is gone"
    );
    assert!(
        !row(&service, found.share.id).connected_by_us,
        "still the user's"
    );
}

// ---- forgetting, safely ----------------------------------------------------------------------

#[test]
fn a_share_is_forgotten_when_the_credential_store_is_not_there() {
    for error in [(|| SecretError::NoKeyring) as fn() -> SecretError, || {
        SecretError::Unavailable("locked".into())
    }] {
        let rig = rig_with(SecretStore::with_backend(FailingDelete(
            MemoryBackend::default(),
            error,
        )));
        let films = add(&rig, "nas", "movies", true);
        remove_folder(&rig, films.source.id);
        assert_eq!(rig.service.sweep(Duration::ZERO), 1);
        assert!(rig.service.list().unwrap().is_empty());
        assert_eq!(disconnects(&rig.fake), 1);
    }
}

#[test]
fn a_password_that_cannot_be_deleted_keeps_the_share_and_nothing_is_disconnected() {
    let rig = rig_with(SecretStore::with_backend(FailingDelete(
        MemoryBackend::default(),
        || SecretError::Store("denied".into()),
    )));
    let films = add(&rig, "nas", "movies", true);
    remove_folder(&rig, films.source.id);

    for _ in 0..3 {
        assert_eq!(rig.service.sweep(Duration::ZERO), 0);
    }
    assert_eq!(rig.service.list().unwrap().len(), 1);
    assert_eq!(
        disconnects(&rig.fake),
        0,
        "never disconnected while the share stays"
    );
}

#[test]
fn a_share_without_a_remembered_password_never_asks_the_credential_store() {
    let rig = rig_with(SecretStore::with_backend(FailingDelete(
        MemoryBackend::default(),
        || SecretError::Store("must not be asked".into()),
    )));
    let films = add(&rig, "nas", "movies", false);
    remove_folder(&rig, films.source.id);
    assert_eq!(rig.service.sweep(Duration::ZERO), 1);
    assert!(rig.service.list().unwrap().is_empty());
}

#[test]
fn a_forgotten_share_is_disconnected_once_whatever_the_sweeps() {
    let rig = rig();
    let films = add(&rig, "nas", "movies", true);
    remove_folder(&rig, films.source.id);
    rig.fake.fail_disconnect(
        &movies(),
        Some(ShareError::new(ShareErrorKind::InUse, "files are open")),
    );
    assert_eq!(rig.service.sweep(Duration::ZERO), 1);
    for _ in 0..3 {
        assert_eq!(rig.service.sweep(Duration::ZERO), 0);
    }
    assert_eq!(disconnects(&rig.fake), 1);
}

#[test]
fn a_folder_added_while_a_share_is_forgotten_keeps_the_share() {
    let hook = Arc::new(std::sync::OnceLock::<Box<dyn Fn() + Send + Sync>>::new());
    let rig = rig_with(SecretStore::with_backend(DeleteHook(
        MemoryBackend::default(),
        Arc::clone(&hook),
    )));
    let films = add(&rig, "nas", "movies", true);
    remove_folder(&rig, films.source.id);
    // "Add folder…" on the share lands between the first check and the removal.
    let guard = SourceGuard::new(
        PathStyle::current(),
        SystemFolders::new(PathStyle::current()),
    );
    let sources = DataSourceService::new(Arc::clone(&rig.db), guard);
    let path = films.source.path.clone();
    let _ = hook.set(Box::new(move || {
        let _ = sources.add(SourceKind::Movie, &path);
    }));

    assert_eq!(rig.service.sweep(Duration::ZERO), 0);
    let kept = row(&rig.service, films.share.id);
    assert_eq!(kept.unused_since_ms, None);
    assert!(!kept.remember, "its password is gone, so it asks for one");
    assert_eq!(disconnects(&rig.fake), 0);
}

#[test]
fn the_mount_folder_clean_up_waits_for_a_connect_in_progress() {
    let rig = rig();
    let base = rig.dir.path().join("shares");
    let mut service = NetworkShareService::new(
        DataSourceService::new(
            Arc::clone(&rig.db),
            SourceGuard::new(
                PathStyle::current(),
                SystemFolders::new(PathStyle::current()),
            ),
        ),
        Arc::clone(&rig.fake) as Arc<dyn ShareConnector>,
        Arc::clone(&rig.secrets),
    );
    service.mount_folders = Some(base);
    service.is_mounted = Arc::new(|_: &Path| false);
    let service = Arc::new(service);
    rig.fake.set_delay(Duration::from_millis(500));

    let connecting = Arc::clone(&service);
    let worker = std::thread::spawn(move || {
        connecting.connect_and_add(&request(r"\\nas\movies", PASSWORD, true), &Cancel::new())
    });
    // The connect is under way (its tool is running) when the clean-up starts.
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while !rig
        .fake
        .calls()
        .iter()
        .any(|call| matches!(call, FakeCall::Connect { .. }))
    {
        assert!(
            std::time::Instant::now() < deadline,
            "the connect did not start"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    service.clean_mount_folders();

    // The clean-up ran only once the connect was done and its share had a row, so the new
    // share's folder was never taken for a leftover.
    assert_eq!(service.list().unwrap().len(), 1);
    let added = worker.join().unwrap().unwrap();
    assert!(Path::new(&added.share.local_root).is_dir());
}
