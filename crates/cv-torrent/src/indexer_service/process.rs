//! Starting and stopping the indexer service program.
//!
//! The service listens on a free loopback port and runs only while CineVault runs:
//!
//! * Windows: the child is put in a job object that kills its whole process tree when the last
//!   handle to the job closes. The app holds the only handle, so the operating system ends the
//!   service when the app exits for any reason, a crash or `taskkill` included.
//! * macOS: the child has its own process group and a small `/bin/sh` watchdog polls the app's
//!   process ID every two seconds and kills the child's group once the app is gone.
//! * Every platform: [`Running::stop`] (also run on drop) ends it at once, and the next start
//!   ends a leftover from an earlier run, found from our own PID record and the service's PID file.
//!   A leftover is only ever ended when its program file is inside our install folder, and never by
//!   name.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use cv_providers::http::{HttpClient, HttpRequest};
use serde::{Deserialize, Serialize};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

use super::ServiceError;
use super::config::{ServerConfigFile, free_loopback_port, generate_api_key};
use crate::cancel::CancelToken;
use crate::source::INDEXER_LANE;

/// How long a started service gets to answer.
pub const READY_TIMEOUT: Duration = Duration::from_secs(60);

/// Our record of the process we started: its ID and program file.
const OWN_PID_FILE: &str = "cv-indexer.pid";
/// The service's own PID file (`--PIDFile`).
const SERVICE_PID_FILE: &str = "jackett.pid";

const POLL_EVERY: Duration = Duration::from_millis(250);

/// What a start needs.
#[derive(Debug, Clone, Copy)]
pub struct StartParams<'a> {
    /// The program file (`JackettConsole.exe`, `jackett`).
    pub program: &'a Path,
    /// The folder of the install the program is in. A leftover process is ended only when its
    /// program file is inside it.
    pub install_dir: &'a Path,
    /// The service's data folder (`<app data>/indexer-service`).
    pub data_dir: &'a Path,
    /// The challenge solver address, if the person set one.
    pub solver_url: Option<&'a str>,
    /// How long the service gets to answer; [`READY_TIMEOUT`] in the app.
    pub ready_timeout: Duration,
}

/// Starts the indexer service.
#[derive(Debug, Clone, Copy)]
pub struct IndexerProcess;

impl IndexerProcess {
    /// Ends any leftover from an earlier run, writes the configuration, starts the program with no
    /// window and waits until it answers on its loopback port.
    ///
    /// # Errors
    /// [`ServiceError`] when the program cannot be started, exits at once, does not answer in
    /// `params.ready_timeout`, or the start is cancelled. The program is stopped on any failure.
    pub async fn start(
        params: &StartParams<'_>,
        http: &HttpClient,
        cancel: &CancelToken,
    ) -> Result<Running, ServiceError> {
        fs::create_dir_all(params.data_dir).map_err(|error| {
            ServiceError::io(
                format!("Could not create {}", params.data_dir.display()),
                error,
            )
        })?;
        let ended = end_leftovers(params.data_dir, params.install_dir);
        if !ended.is_empty() {
            tracing::info!(pids = ?ended, "ended a leftover indexer service");
        }

        let config_dir = ServerConfigFile::config_dir(params.data_dir);
        let api_key = match ServerConfigFile::read_api_key(&config_dir) {
            Some(key) => key,
            None => generate_api_key()?,
        };
        let port = free_loopback_port()?;
        ServerConfigFile::write_or_patch(&config_dir, port, &api_key, params.solver_url)?;

        let pid_file = params.data_dir.join(SERVICE_PID_FILE);
        let _ = fs::remove_file(&pid_file);
        let mut command = service_command(params, &pid_file);
        let child = command.spawn().map_err(|error| {
            ServiceError::io(
                format!("Could not start {}", params.program.display()),
                error,
            )
        })?;
        let mut running = Running::new(child, port, api_key, params);
        tracing::info!(port, pid = running.pid, "started the indexer service");

        running
            .wait_until_ready(http, cancel, params.ready_timeout)
            .await?;
        Ok(running)
    }
}

/// The command line of the service. `-d` (the data folder) exists on Windows only; elsewhere the
/// service keeps its files under `XDG_CONFIG_HOME`.
fn service_command(params: &StartParams<'_>, pid_file: &Path) -> Command {
    let mut command = Command::new(params.program);
    command
        .arg("--NoUpdates")
        .arg("--PIDFile")
        .arg(pid_file)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(folder) = params.program.parent() {
        command.current_dir(folder);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.arg("-d").arg(params.data_dir);
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.env("XDG_CONFIG_HOME", params.data_dir);
        if std::env::var_os("HOME").is_none() {
            command.env("HOME", params.data_dir);
        }
        // Its own process group, so the whole tree can be ended together.
        command.process_group(0);
    }
    command
}

/// What ties the child to the app.
#[derive(Default)]
struct Guard {
    #[cfg(windows)]
    job: Option<win32job::Job>,
    #[cfg(unix)]
    watchdog: Option<Child>,
}

impl Guard {
    #[cfg(windows)]
    fn attach(child: &Child) -> Self {
        use std::os::windows::io::AsRawHandle;
        let attached = (|| {
            let job = win32job::Job::create().ok()?;
            let mut info = job.query_extended_limit_info().ok()?;
            info.limit_kill_on_job_close();
            job.set_extended_limit_info(&info).ok()?;
            job.assign_process(child.as_raw_handle() as isize).ok()?;
            Some(job)
        })();
        if attached.is_none() {
            tracing::warn!("the indexer service could not be tied to the app with a job object");
        }
        Self { job: attached }
    }

    #[cfg(unix)]
    fn attach(child: &Child) -> Self {
        let watchdog = Command::new("/bin/sh")
            .arg("-c")
            .arg(WATCHDOG_SCRIPT)
            .arg("cv-watchdog")
            .arg(std::process::id().to_string())
            .arg(child.id().to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| tracing::warn!(%error, "the indexer service watchdog did not start"))
            .ok();
        Self { watchdog }
    }

    #[cfg(not(any(windows, unix)))]
    fn attach(_child: &Child) -> Self {
        Self::default()
    }

    /// Lets go of what ties the child to the app (after the child was ended).
    fn release(&mut self) {
        #[cfg(windows)]
        {
            // Dropping the job closes its last handle, which ends anything the child started.
            self.job = None;
        }
        #[cfg(unix)]
        if let Some(mut watchdog) = self.watchdog.take() {
            let _ = watchdog.kill();
            let _ = watchdog.wait();
        }
    }
}

/// Polls the app's process; when it is gone, ends the child's process group (`$2`).
#[cfg(unix)]
const WATCHDOG_SCRIPT: &str = r#"while kill -0 "$1" 2>/dev/null; do sleep 2; done
kill -TERM -- "-$2" 2>/dev/null
sleep 3
kill -KILL -- "-$2" 2>/dev/null"#;

/// What the app needs of a started service, so the code that manages it can be tested without a
/// process.
pub trait ServiceHandle: Send {
    /// The loopback port it listens on.
    fn port(&self) -> u16;
    /// The API key it accepts. Never log it.
    fn api_key(&self) -> String;
    /// Whether it is still running.
    fn is_running(&mut self) -> bool;
    /// Ends it and waits. Safe to call twice.
    fn stop(&mut self);
}

impl ServiceHandle for Running {
    fn port(&self) -> u16 {
        self.port
    }

    fn api_key(&self) -> String {
        self.api_key.clone()
    }

    fn is_running(&mut self) -> bool {
        Running::is_running(self)
    }

    fn stop(&mut self) {
        Running::stop(self);
    }
}

/// Starts the service. The app uses [`ProcessLauncher`]; tests use one that starts nothing.
#[async_trait::async_trait]
pub trait Launcher: Send + Sync {
    /// Starts the service and returns it once it answers.
    ///
    /// # Errors
    /// As [`IndexerProcess::start`].
    async fn start(
        &self,
        params: &StartParams<'_>,
        http: &HttpClient,
        cancel: &CancelToken,
    ) -> Result<Box<dyn ServiceHandle>, ServiceError>;
}

/// Starts the real program.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessLauncher;

#[async_trait::async_trait]
impl Launcher for ProcessLauncher {
    async fn start(
        &self,
        params: &StartParams<'_>,
        http: &HttpClient,
        cancel: &CancelToken,
    ) -> Result<Box<dyn ServiceHandle>, ServiceError> {
        let running = IndexerProcess::start(params, http, cancel).await?;
        Ok(Box::new(running))
    }
}

/// A started indexer service. It is ended when this value is dropped.
pub struct Running {
    port: u16,
    api_key: String,
    pid: u32,
    child: Option<Child>,
    guard: Guard,
    data_dir: PathBuf,
}

impl std::fmt::Debug for Running {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Running")
            .field("port", &self.port)
            .field("pid", &self.pid)
            .finish_non_exhaustive()
    }
}

#[derive(Serialize, Deserialize)]
struct OwnRecord {
    pid: u32,
    exe: PathBuf,
}

impl Running {
    fn new(child: Child, port: u16, api_key: String, params: &StartParams<'_>) -> Self {
        let pid = child.id();
        let guard = Guard::attach(&child);
        let record = OwnRecord {
            pid,
            exe: params.program.to_path_buf(),
        };
        if let Ok(text) = serde_json::to_string(&record) {
            let _ = fs::write(params.data_dir.join(OWN_PID_FILE), text);
        }
        Self {
            port,
            api_key,
            pid,
            child: Some(child),
            guard,
            data_dir: params.data_dir.to_path_buf(),
        }
    }

    /// The loopback port the service listens on.
    #[must_use]
    pub const fn port(&self) -> u16 {
        self.port
    }

    /// The API key the service accepts. Never log it.
    #[must_use]
    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    /// The process ID of the service.
    #[must_use]
    pub const fn pid(&self) -> u32 {
        self.pid
    }

    /// `http://127.0.0.1:<port>`.
    #[must_use]
    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// Whether the program is still running.
    pub fn is_running(&mut self) -> bool {
        self.child
            .as_mut()
            .is_some_and(|child| matches!(child.try_wait(), Ok(None)))
    }

    async fn wait_until_ready(
        &mut self,
        http: &HttpClient,
        cancel: &CancelToken,
        limit: Duration,
    ) -> Result<(), ServiceError> {
        let started = Instant::now();
        // The admin API wants a session; the Torznab feed answers to the API key alone, and says
        // `<error>` (with status 200) for a wrong key, so the capabilities page proves both that
        // the service is up and that it took our key.
        let url = format!(
            "{}/api/v2.0/indexers/all/results/torznab/api?t=caps",
            self.base_url()
        );
        loop {
            if cancel.is_cancelled() {
                self.stop();
                return Err(ServiceError::Cancelled);
            }
            if let Some(child) = self.child.as_mut()
                && let Ok(Some(status)) = child.try_wait()
            {
                self.stop();
                return Err(ServiceError::Exited(status.to_string()));
            }
            let request = HttpRequest::get(INDEXER_LANE, &url)
                .credential_query("apikey", &self.api_key)
                .uncached();
            if let Ok(response) = http.send(request).await
                && response.is_success()
                && response.text().contains("<caps")
            {
                return Ok(());
            }
            if started.elapsed() >= limit {
                self.stop();
                return Err(ServiceError::NotReady(limit.as_secs()));
            }
            tokio::select! {
                () = tokio::time::sleep(POLL_EVERY) => {}
                () = cancel.cancelled() => {}
            }
        }
    }

    /// Ends the service and anything it started, and waits for it. Safe to call twice.
    pub fn stop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        #[cfg(unix)]
        kill_group(child.id());
        let _ = child.kill();
        let _ = child.wait();
        self.guard.release();
        let _ = fs::remove_file(self.data_dir.join(OWN_PID_FILE));
        let _ = fs::remove_file(self.data_dir.join(SERVICE_PID_FILE));
        tracing::info!(pid = self.pid, "stopped the indexer service");
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Ends the process group `pgid` (the service and anything it started).
#[cfg(unix)]
fn kill_group(pgid: u32) {
    let _ = Command::new("kill")
        .args(["-KILL", "--"])
        .arg(format!("-{pgid}"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

// ---- leftovers -----------------------------------------------------------------------------

/// Ends the processes an earlier run left behind and returns their IDs. Two records are read: our
/// own (`cv-indexer.pid`) and the service's (`jackett.pid`). A process is ended only when it is
/// alive and its program file is inside `install_dir`; a process ID that now belongs to something
/// else is left alone. Both records are removed.
pub fn end_leftovers(data_dir: &Path, install_dir: &Path) -> Vec<u32> {
    let mut candidates = Vec::new();
    let own = data_dir.join(OWN_PID_FILE);
    if let Ok(text) = fs::read_to_string(&own)
        && let Ok(record) = serde_json::from_str::<OwnRecord>(&text)
    {
        candidates.push(record.pid);
    }
    let service = data_dir.join(SERVICE_PID_FILE);
    if let Ok(text) = fs::read_to_string(&service)
        && let Ok(pid) = text.trim().parse::<u32>()
    {
        candidates.push(pid);
    }
    candidates.sort_unstable();
    candidates.dedup();
    let _ = fs::remove_file(&own);
    let _ = fs::remove_file(&service);

    let mut ended = Vec::new();
    for pid in candidates {
        if pid == std::process::id() {
            continue;
        }
        if end_if_ours(pid, install_dir) {
            ended.push(pid);
        }
    }
    ended
}

/// Ends `pid` when it runs a program inside `install_dir`. Returns whether it did.
fn end_if_ours(pid: u32, install_dir: &Path) -> bool {
    let mut system = System::new();
    let target = Pid::from_u32(pid);
    let kind = ProcessRefreshKind::nothing().with_exe(UpdateKind::Always);
    system.refresh_processes_specifics(ProcessesToUpdate::Some(&[target]), true, kind);
    let Some(process) = system.process(target) else {
        return false;
    };
    let Some(exe) = process.exe() else {
        return false;
    };
    if !is_inside(exe, install_dir) {
        tracing::info!(
            pid,
            "left a process alone: it is not from the indexer service install"
        );
        return false;
    }
    let killed = process.kill();
    // Wait for it to go, so the port and the files it held are free.
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[target]),
            true,
            ProcessRefreshKind::nothing(),
        );
        if system.process(target).is_none() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    killed
}

/// Whether `file` is inside `folder`, comparing real paths (and ignoring case on Windows).
fn is_inside(file: &Path, folder: &Path) -> bool {
    let file = comparable(file);
    let folder = comparable(folder);
    file.starts_with(&folder) && file != folder
}

fn comparable(path: &Path) -> PathBuf {
    let real = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let text = real.to_string_lossy();
    let text = text.strip_prefix(r"\\?\").unwrap_or(&text);
    if cfg!(windows) {
        PathBuf::from(text.to_lowercase().replace('/', "\\"))
    } else {
        PathBuf::from(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_is_inside_its_folder_but_a_sibling_with_the_same_prefix_is_not() {
        let temp = tempfile::tempdir().expect("temp dir");
        let install = temp.path().join("indexer-service").join("1.0");
        let sibling = temp.path().join("indexer-service").join("1.0-other");
        fs::create_dir_all(&install).expect("install");
        fs::create_dir_all(&sibling).expect("sibling");
        let program = install.join("run.exe");
        let stranger = sibling.join("run.exe");
        fs::write(&program, b"x").expect("program");
        fs::write(&stranger, b"x").expect("stranger");
        assert!(is_inside(&program, &install));
        assert!(!is_inside(&stranger, &install));
        assert!(!is_inside(&install, &install));
    }

    #[cfg(windows)]
    #[test]
    fn windows_paths_compare_without_case_or_verbatim_prefix() {
        assert_eq!(
            comparable(Path::new(r"\\?\C:\Tools\Run.exe")),
            comparable(Path::new(r"c:\tools\run.exe"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_watchdog_waits_for_the_app_then_ends_the_group() {
        assert!(WATCHDOG_SCRIPT.contains(r#"kill -0 "$1""#));
        assert!(WATCHDOG_SCRIPT.contains(r#"kill -TERM -- "-$2""#));
        assert!(WATCHDOG_SCRIPT.contains(r#"kill -KILL -- "-$2""#));
    }
}
