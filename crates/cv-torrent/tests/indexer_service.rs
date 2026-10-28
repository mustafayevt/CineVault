//! The indexer service process against a stub program (`tests/bin/stub_indexer.rs`): it starts on
//! a loopback port with the configuration CineVault owns, is waited for until it answers, is
//! stopped completely, ends with the app, and a leftover from an earlier run is ended only when it
//! is ours. No test touches the network.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use cv_providers::http::{HttpClient, HttpRequest};
use cv_torrent::cancel::CancelToken;
use cv_torrent::indexer_service::process::end_leftovers;
use cv_torrent::indexer_service::{
    IndexerProcess, Running, ServerConfigFile, ServiceError, StartParams,
};
use cv_torrent::source::{INDEXER_LANE, client_builder};
use sysinfo::{Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System};

const STUB: &str = env!("CARGO_BIN_EXE_stub_indexer");
const HARNESS: &str = env!("CARGO_BIN_EXE_indexer_parent_harness");

fn program_name() -> &'static str {
    if cfg!(windows) {
        "JackettConsole.exe"
    } else {
        "jackett"
    }
}

/// An install folder holding a copy of the stub, and an empty data folder.
struct Fixture {
    _temp: tempfile::TempDir,
    install: PathBuf,
    program: PathBuf,
    data: PathBuf,
    other: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("temp dir");
        let install = temp
            .path()
            .join("tools")
            .join("indexer-service")
            .join("1.0");
        let other = temp.path().join("somewhere-else");
        let data = temp.path().join("data").join("indexer-service");
        fs::create_dir_all(&install).expect("install");
        fs::create_dir_all(&other).expect("other");
        fs::create_dir_all(&data).expect("data");
        let program = install.join(program_name());
        fs::copy(STUB, &program).expect("copy the stub");
        Self {
            _temp: temp,
            install,
            program,
            data,
            other,
        }
    }

    fn params(&self) -> StartParams<'_> {
        StartParams {
            program: &self.program,
            install_dir: &self.install,
            data_dir: &self.data,
            solver_url: None,
            ready_timeout: Duration::from_secs(20),
        }
    }

    /// Tells the stub how to behave on its next start.
    fn behave(&self, behavior: &str) {
        fs::write(self.data.join("stub-behavior.txt"), behavior).expect("behavior");
    }

    /// Starts an idle copy of the stub, from the install folder or from somewhere else, whose
    /// process ID it writes into `pid_file`.
    fn idle(&self, inside: bool, pid_file: &Path) -> Ended {
        let folder = if inside { &self.install } else { &self.other };
        let copy = folder.join(format!("idle-{program}", program = program_name()));
        fs::copy(STUB, &copy).expect("copy the stub");
        let child = spawn_when_not_busy(|| {
            Command::new(&copy)
                .arg("--idle")
                .arg("--PIDFile")
                .arg(pid_file)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
        })
        .map(Ended)
        .expect("start an idle stub");
        // The stub writes its PID file as its first act.
        let deadline = Instant::now() + Duration::from_secs(10);
        while !pid_file.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(pid_file.exists(), "the idle stub did not start");
        child
    }
}

/// Starts a program that was copied a moment ago. On Linux the start can fail with "Text file
/// busy" while another test, running in parallel, still holds the copy's write handle open in a
/// process it is forking; that ends within milliseconds, so the start is tried again.
fn spawn_when_not_busy(
    mut start: impl FnMut() -> std::io::Result<Child>,
) -> std::io::Result<Child> {
    let mut attempts = 0;
    loop {
        match start() {
            Err(error)
                if error.kind() == std::io::ErrorKind::ExecutableFileBusy && attempts < 100 =>
            {
                attempts += 1;
                std::thread::sleep(Duration::from_millis(20));
            }
            other => return other,
        }
    }
}

/// A child process that is ended when the test is over, also when it fails or panics, so no
/// stand-in program outlives the test run.
struct Ended(Child);

impl Drop for Ended {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl std::ops::Deref for Ended {
    type Target = Child;
    fn deref(&self) -> &Child {
        &self.0
    }
}

impl std::ops::DerefMut for Ended {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.0
    }
}

/// Ends the stub process `pid` (a service a killed harness started) when dropped, if it is
/// still running and is a program inside `install`: never anything else.
struct EndedStub<'a> {
    pid: u32,
    install: &'a Path,
}

impl Drop for EndedStub<'_> {
    fn drop(&mut self) {
        let mut system = System::new();
        let target = Pid::from_u32(self.pid);
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[target]),
            true,
            ProcessRefreshKind::nothing().with_exe(sysinfo::UpdateKind::Always),
        );
        if let Some(process) = system.process(target)
            && process.exe().is_some_and(|exe| {
                let install = fs::canonicalize(self.install).unwrap_or_default();
                fs::canonicalize(exe).is_ok_and(|exe| exe.starts_with(&install))
            })
        {
            process.kill();
        }
    }
}

fn client() -> HttpClient {
    client_builder().build().expect("http client")
}

async fn start(fixture: &Fixture) -> Result<Running, ServiceError> {
    IndexerProcess::start(&fixture.params(), &client(), &CancelToken::new()).await
}

fn alive(pid: u32) -> bool {
    let mut system = System::new();
    let target = Pid::from_u32(pid);
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[target]),
        true,
        ProcessRefreshKind::nothing(),
    );
    // A program that has ended but whose parent has not collected it yet (a zombie on Unix)
    // still has an entry; it is not running.
    system
        .process(target)
        .is_some_and(|process| process.status() != ProcessStatus::Zombie)
}

/// Waits up to `seconds` for `pid` to be gone.
fn gone_within(pid: u32, seconds: u64) -> bool {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < deadline {
        if !alive(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    !alive(pid)
}

fn read_config(fixture: &Fixture) -> serde_json::Value {
    let dir = ServerConfigFile::config_dir(&fixture.data);
    serde_json::from_str(&fs::read_to_string(dir.join("ServerConfig.json")).expect("config"))
        .expect("json")
}

#[tokio::test]
async fn it_starts_on_a_loopback_port_with_the_configuration_we_own() {
    let fixture = Fixture::new();
    let mut running = start(&fixture).await.expect("start");
    assert!(running.is_running());
    assert!(running.port() > 0);
    assert_eq!(running.api_key().len(), 32);

    let config = read_config(&fixture);
    assert_eq!(config["Port"], running.port());
    assert_eq!(config["APIKey"], running.api_key());
    assert_eq!(config["LocalBindAddress"], "127.0.0.1");
    assert_eq!(config["AllowExternal"], false);
    assert_eq!(config["UpdateDisabled"], true);
    assert_eq!(config["CacheEnabled"], true);
    assert!(config["FlareSolverrUrl"].is_null());
    assert!(running.base_url().starts_with("http://127.0.0.1:"));

    // It answers with the key and refuses without it.
    let http = client();
    let url = format!(
        "{}/api/v2.0/indexers/all/results/torznab/api?t=caps",
        running.base_url()
    );
    let good = http
        .send(
            HttpRequest::get(INDEXER_LANE, &url)
                .credential_query("apikey", running.api_key())
                .uncached(),
        )
        .await
        .expect("answer");
    assert!(good.is_success() && good.text().contains("<caps"));
    let bad = http
        .send(
            HttpRequest::get(INDEXER_LANE, &url)
                .credential_query("apikey", "wrong")
                .uncached(),
        )
        .await;
    assert!(bad.map_or(true, |response| !response.text().contains("<caps")));

    // The program got the command line the service needs, with the data folder where it expects it.
    let report = fs::read_to_string(fixture.data.join("stub-args.txt"))
        .expect("the stub reported its command line");
    assert!(report.contains("--NoUpdates"), "{report}");
    assert!(report.contains("--PIDFile"), "{report}");
    assert!(report.contains("jackett.pid"), "{report}");
    if cfg!(windows) {
        assert!(report.contains("-d "), "{report}");
    } else {
        let xdg = report
            .lines()
            .find(|l| l.starts_with("xdg="))
            .unwrap_or_default();
        assert!(xdg.contains("indexer-service"), "{report}");
    }

    // Both PID records name the running program.
    let service_pid: u32 = fs::read_to_string(fixture.data.join("jackett.pid"))
        .expect("the service PID file")
        .trim()
        .parse()
        .expect("a PID");
    assert_eq!(service_pid, running.pid());
    let own = fs::read_to_string(fixture.data.join("cv-indexer.pid")).expect("our record");
    assert!(own.contains(&running.pid().to_string()), "{own}");
    running.stop();
}

#[tokio::test]
async fn the_challenge_solver_address_reaches_the_configuration() {
    let fixture = Fixture::new();
    let params = StartParams {
        solver_url: Some("http://localhost:8191"),
        ..fixture.params()
    };
    let mut running = IndexerProcess::start(&params, &client(), &CancelToken::new())
        .await
        .expect("start");
    assert_eq!(
        read_config(&fixture)["FlareSolverrUrl"],
        "http://localhost:8191"
    );
    running.stop();
}

#[tokio::test]
async fn stopping_ends_the_program_and_clears_the_records() {
    let fixture = Fixture::new();
    let mut running = start(&fixture).await.expect("start");
    let pid = running.pid();
    let port = running.port();
    assert!(alive(pid));
    running.stop();
    assert!(!running.is_running());
    assert!(gone_within(pid, 5), "the program is still running");
    assert!(!fixture.data.join("jackett.pid").exists());
    assert!(!fixture.data.join("cv-indexer.pid").exists());
    assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_err());
    // Stopping again is fine.
    running.stop();
}

#[tokio::test]
async fn dropping_it_ends_the_program_too() {
    let fixture = Fixture::new();
    let running = start(&fixture).await.expect("start");
    let pid = running.pid();
    drop(running);
    assert!(gone_within(pid, 5));
}

#[tokio::test]
async fn a_second_start_keeps_the_api_key_and_ends_the_first() {
    let fixture = Fixture::new();
    let first = start(&fixture).await.expect("first");
    let (pid, key) = (first.pid(), first.api_key().to_owned());
    // The first handle is forgotten, as after a crash: only the records are left.
    std::mem::forget(first);
    let mut second = start(&fixture).await.expect("second");
    assert_eq!(second.api_key(), key);
    assert_ne!(second.pid(), pid);
    assert!(gone_within(pid, 5), "the first program was not ended");
    second.stop();
}

#[tokio::test]
async fn a_program_that_exits_at_once_is_reported_not_waited_for() {
    let fixture = Fixture::new();
    fixture.behave("exit_immediately");
    let started = Instant::now();
    let error = start(&fixture).await.expect_err("it exits");
    assert!(matches!(error, ServiceError::Exited(_)), "{error:?}");
    assert!(started.elapsed() < Duration::from_secs(15));
    assert!(!fixture.data.join("cv-indexer.pid").exists());
}

#[tokio::test]
async fn a_program_that_never_answers_is_stopped_after_the_limit() {
    let fixture = Fixture::new();
    fixture.behave("never_ready");
    let params = StartParams {
        ready_timeout: Duration::from_secs(1),
        ..fixture.params()
    };
    let error = IndexerProcess::start(&params, &client(), &CancelToken::new())
        .await
        .expect_err("it never answers");
    assert!(matches!(error, ServiceError::NotReady(1)), "{error:?}");
    assert!(!fixture.data.join("cv-indexer.pid").exists());
    // Nothing is left running: a new start finds no leftover to end.
    assert!(end_leftovers(&fixture.data, &fixture.install).is_empty());
}

#[tokio::test]
async fn a_slow_program_is_waited_for() {
    let fixture = Fixture::new();
    fixture.behave("delay_ms=800");
    let started = Instant::now();
    let mut running = start(&fixture).await.expect("it answers in the end");
    assert!(started.elapsed() >= Duration::from_millis(700));
    running.stop();
}

#[tokio::test]
async fn a_cancelled_start_stops_the_program() {
    let fixture = Fixture::new();
    fixture.behave("never_ready");
    let cancel = CancelToken::new();
    let later = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(600)).await;
        later.cancel();
    });
    let error = IndexerProcess::start(&fixture.params(), &client(), &cancel)
        .await
        .expect_err("cancelled");
    assert!(matches!(error, ServiceError::Cancelled), "{error:?}");
    assert!(!fixture.data.join("cv-indexer.pid").exists());
}

#[tokio::test]
async fn a_leftover_from_the_install_folder_is_ended_before_the_start() {
    let fixture = Fixture::new();
    // Found through the service's own PID file.
    let mut leftover = fixture.idle(true, &fixture.data.join("jackett.pid"));
    let pid = leftover.id();
    let mut running = start(&fixture).await.expect("start");
    assert!(
        leftover.try_wait().expect("wait").is_some(),
        "the leftover is still running"
    );
    assert_ne!(running.pid(), pid);
    running.stop();
}

#[tokio::test]
async fn a_leftover_found_through_our_own_record_is_ended_too() {
    let fixture = Fixture::new();
    let scratch = fixture.data.join("scratch.pid");
    let mut leftover = fixture.idle(true, &scratch);
    let record = serde_json::json!({ "pid": leftover.id(), "exe": fixture.program });
    fs::write(fixture.data.join("cv-indexer.pid"), record.to_string()).expect("record");
    let mut running = start(&fixture).await.expect("start");
    assert!(
        leftover.try_wait().expect("wait").is_some(),
        "the leftover is still running"
    );
    running.stop();
}

#[tokio::test]
async fn a_process_with_a_recorded_id_that_is_not_ours_is_left_alone() {
    let fixture = Fixture::new();
    // The ID in the PID file now belongs to a program from somewhere else.
    let mut stranger = fixture.idle(false, &fixture.data.join("jackett.pid"));
    let mut running = start(&fixture).await.expect("start");
    assert!(
        stranger.try_wait().expect("wait").is_none(),
        "a stranger was ended"
    );
    assert!(alive(stranger.id()));
    running.stop();
}

#[test]
fn a_record_of_a_process_that_is_gone_is_cleared_quietly() {
    let fixture = Fixture::new();
    fs::write(fixture.data.join("jackett.pid"), "999999").expect("record");
    fs::write(fixture.data.join("cv-indexer.pid"), "not json").expect("record");
    assert!(end_leftovers(&fixture.data, &fixture.install).is_empty());
    assert!(!fixture.data.join("jackett.pid").exists());
    assert!(!fixture.data.join("cv-indexer.pid").exists());
}

#[test]
fn the_app_itself_is_never_ended_by_a_record() {
    let fixture = Fixture::new();
    fs::write(
        fixture.data.join("jackett.pid"),
        std::process::id().to_string(),
    )
    .expect("record");
    assert!(end_leftovers(&fixture.data, &fixture.install).is_empty());
    assert!(alive(std::process::id()));
}

/// The service must end with the app, however the app ends: a harness that starts it the way the
/// app does is killed, and the service has to follow.
#[test]
fn the_program_ends_when_the_app_is_killed() {
    let fixture = Fixture::new();
    let mut harness = Command::new(HARNESS)
        .arg(&fixture.program)
        .arg(&fixture.install)
        .arg(&fixture.data)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map(Ended)
        .expect("start the harness");
    let mut line = String::new();
    BufReader::new(harness.stdout.take().expect("stdout"))
        .read_line(&mut line)
        .expect("the harness reports the service");
    let pid: u32 = line
        .trim()
        .strip_prefix("pid=")
        .and_then(|n| n.parse().ok())
        .unwrap_or_else(|| panic!("unexpected harness output {line:?}"));
    let _service = EndedStub {
        pid,
        install: &fixture.install,
    };
    assert!(alive(pid), "the service is not running");

    harness.kill().expect("kill the harness");
    let _ = harness.wait();
    // Windows ends it at once; elsewhere the watchdog notices within a few seconds.
    assert!(gone_within(pid, 20), "the service outlived the app");
}
