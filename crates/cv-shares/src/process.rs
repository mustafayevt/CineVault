//! Running the system's tools: a fixed program and argument list, output read as it comes, and
//! answers written to standard input only after the tool asked for them.
//!
//! The connectors talk to a [`ToolProcess`] made by a [`Spawner`], so their dialogs are tested on
//! every platform against a scripted fake (`fake` in tests) that records the arguments and the
//! input it got.

use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use crate::connector::Cancel;
use crate::error::{ShareError, ShareErrorKind};

/// How long a prompt must stay unanswered with no more output before it counts as a question.
// The prompt dialog is used by the macOS and Linux connectors only.
#[cfg_attr(windows, allow(dead_code))]
const SETTLE: Duration = Duration::from_millis(150);

/// How long one wait for output lasts before the stop signal is checked again.
const POLL: Duration = Duration::from_millis(50);

/// Output kept per stream; a tool that prints more is not one of ours behaving normally.
const MAX_OUTPUT: usize = 64 * 1024;

/// A program to start: its path, its arguments and extra environment. Never holds a password.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCommand {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub env: Vec<(OsString, OsString)>,
}

impl ToolCommand {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            env: Vec::new(),
        }
    }

    #[must_use]
    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }

    #[must_use]
    pub fn env(mut self, name: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.env.push((name.into(), value.into()));
        self
    }

    /// Whether `text` appears in the program, an argument or the environment.
    #[cfg(test)]
    pub(crate) fn mentions(&self, text: &str) -> bool {
        std::iter::once(self.program.as_os_str())
            .chain(self.args.iter().map(OsString::as_os_str))
            .chain(
                self.env
                    .iter()
                    .flat_map(|(k, v)| [k.as_os_str(), v.as_os_str()]),
            )
            .any(|part| part.to_string_lossy().contains(text))
    }
}

/// What a running tool did since the last look.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Stdout(String),
    Stderr(String),
    /// Nothing within the wait.
    Idle,
    /// The tool ended with this exit code (-1 when it was killed by a signal) and all its output
    /// has been read.
    Ended(i32),
}

/// A running tool.
pub trait ToolProcess: Send {
    /// The next event, waiting up to `wait` for one.
    fn read(&mut self, wait: Duration) -> Event;
    /// Writes `text` to the tool's standard input.
    ///
    /// # Errors
    /// When the tool no longer reads.
    fn write(&mut self, text: &str) -> io::Result<()>;
    /// Closes the tool's standard input.
    fn close_input(&mut self);
    /// Stops the tool.
    fn kill(&mut self);
}

/// Starts tools.
pub trait Spawner: Send + Sync {
    /// Starts `command` with piped standard input, output and error.
    ///
    /// # Errors
    /// When the program cannot be started.
    fn spawn(&self, command: &ToolCommand) -> io::Result<Box<dyn ToolProcess>>;
}

/// Starts real processes, without a console window on Windows.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemSpawner;

impl Spawner for SystemSpawner {
    fn spawn(&self, command: &ToolCommand) -> io::Result<Box<dyn ToolProcess>> {
        let mut cmd = Command::new(&command.program);
        cmd.args(&command.args)
            .envs(command.env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW: no console window flashes up.
            cmd.creation_flags(0x0800_0000);
        }
        let mut child = cmd.spawn()?;
        let (tx, rx) = mpsc::channel();
        let mut streams = 0;
        if let Some(out) = child.stdout.take() {
            spawn_reader(Box::new(out), false, tx.clone());
            streams += 1;
        }
        if let Some(err) = child.stderr.take() {
            spawn_reader(Box::new(err), true, tx);
            streams += 1;
        }
        Ok(Box::new(SystemProcess {
            stdin: child.stdin.take(),
            child,
            rx,
            open_streams: streams,
            ended: None,
        }))
    }
}

enum Message {
    Text { error: bool, text: String },
    Closed,
}

fn spawn_reader(mut pipe: Box<dyn Read + Send>, error: bool, tx: Sender<Message>) {
    std::thread::spawn(move || {
        let mut buffer = [0u8; 4096];
        let mut pending: Vec<u8> = Vec::new();
        while let Ok(n) = pipe.read(&mut buffer) {
            if n == 0 {
                break;
            }
            pending.extend_from_slice(&buffer[..n]);
            let text = take_text(&mut pending);
            if !text.is_empty() && tx.send(Message::Text { error, text }).is_err() {
                return;
            }
        }
        if !pending.is_empty() {
            let text = String::from_utf8_lossy(&pending).into_owned();
            let _ = tx.send(Message::Text { error, text });
        }
        let _ = tx.send(Message::Closed);
    });
}

/// The complete UTF-8 text at the start of `pending`, leaving a character cut in half for the
/// next read. Bytes that are not UTF-8 at all are replaced.
fn take_text(pending: &mut Vec<u8>) -> String {
    match std::str::from_utf8(pending) {
        Ok(text) => {
            let text = text.to_owned();
            pending.clear();
            text
        }
        Err(e) if e.error_len().is_none() => {
            let tail = pending.split_off(e.valid_up_to());
            let text = String::from_utf8_lossy(pending).into_owned();
            *pending = tail;
            text
        }
        Err(_) => {
            let text = String::from_utf8_lossy(pending).into_owned();
            pending.clear();
            text
        }
    }
}

struct SystemProcess {
    child: Child,
    stdin: Option<ChildStdin>,
    rx: Receiver<Message>,
    open_streams: usize,
    ended: Option<i32>,
}

impl ToolProcess for SystemProcess {
    fn read(&mut self, wait: Duration) -> Event {
        if let Some(code) = self.ended {
            return Event::Ended(code);
        }
        if self.open_streams == 0 {
            return match self.child.try_wait() {
                Ok(Some(status)) => {
                    let code = status.code().unwrap_or(-1);
                    self.ended = Some(code);
                    Event::Ended(code)
                }
                Ok(None) => {
                    std::thread::sleep(wait);
                    Event::Idle
                }
                Err(_) => {
                    self.ended = Some(-1);
                    Event::Ended(-1)
                }
            };
        }
        match self.rx.recv_timeout(wait) {
            Ok(Message::Text { error: false, text }) => Event::Stdout(text),
            Ok(Message::Text { error: true, text }) => Event::Stderr(text),
            Ok(Message::Closed) => {
                self.open_streams -= 1;
                Event::Idle
            }
            Err(RecvTimeoutError::Timeout) => Event::Idle,
            Err(RecvTimeoutError::Disconnected) => {
                self.open_streams = 0;
                Event::Idle
            }
        }
    }

    fn write(&mut self, text: &str) -> io::Result<()> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "input closed"))?;
        stdin.write_all(text.as_bytes())?;
        stdin.flush()
    }

    fn close_input(&mut self) {
        self.stdin = None;
    }

    fn kill(&mut self) {
        self.stdin = None;
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for SystemProcess {
    fn drop(&mut self) {
        if self.ended.is_none() && matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// What a finished tool left.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Finished {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// A conversation with one tool: wait for its prompts, answer them, collect its output, and stop
/// it when the attempt is cancelled or runs out of time.
pub(crate) struct Session {
    process: Box<dyn ToolProcess>,
    stdout: String,
    stderr: String,
    /// How much of `stdout` earlier prompts used up.
    #[cfg_attr(windows, allow(dead_code))]
    consumed: usize,
    cancel: Cancel,
    deadline: Instant,
    ended: Option<i32>,
}

impl Session {
    /// Starts `command`. It is stopped when `cancel` fires or after `limit`.
    pub(crate) fn start(
        spawner: &dyn Spawner,
        command: &ToolCommand,
        cancel: &Cancel,
        limit: Duration,
    ) -> Result<Self, ShareError> {
        if cancel.is_cancelled() {
            return Err(ShareError::cancelled());
        }
        let program = command.program.display().to_string();
        tracing::debug!(%program, "starting a share tool");
        let process = spawner.spawn(command).map_err(|error| {
            ShareError::new(
                ShareErrorKind::Other,
                format!("{program} could not be started: {error}"),
            )
        })?;
        let own = Instant::now() + limit;
        let deadline = cancel.deadline().map_or(own, |at| at.min(own));
        Ok(Self {
            process,
            stdout: String::new(),
            stderr: String::new(),
            consumed: 0,
            cancel: cancel.clone(),
            deadline,
            ended: None,
        })
    }

    fn check_stop(&mut self) -> Result<(), ShareError> {
        if self.cancel.is_cancelled() {
            self.process.kill();
            self.ended = Some(-1);
            return Err(ShareError::cancelled());
        }
        if Instant::now() >= self.deadline {
            self.process.kill();
            self.ended = Some(-1);
            return Err(ShareError::timed_out());
        }
        Ok(())
    }

    /// Reads one event into the collected output; the exit code once the tool has ended.
    fn pump(&mut self) -> Option<i32> {
        if let Some(code) = self.ended {
            return Some(code);
        }
        match self.process.read(POLL) {
            Event::Stdout(text) => push_capped(&mut self.stdout, &text),
            Event::Stderr(text) => push_capped(&mut self.stderr, &text),
            Event::Idle => {}
            Event::Ended(code) => self.ended = Some(code),
        }
        self.ended
    }

    /// The last line of the output no prompt used up yet, when it ends with `:` (a question).
    #[cfg_attr(windows, allow(dead_code))]
    fn pending_prompt(&self) -> Option<String> {
        let fresh = &self.stdout[self.consumed..];
        let line = fresh.rsplit(['\n', '\r']).next().unwrap_or(fresh).trim();
        line.ends_with(':').then(|| line.to_owned())
    }

    /// Waits until the tool asks something and returns the question (the last line of output,
    /// ending with `:`), or `None` when the tool ended instead.
    #[cfg_attr(windows, allow(dead_code))]
    pub(crate) fn prompt(&mut self) -> Result<Option<String>, ShareError> {
        let mut quiet_since: Option<Instant> = None;
        loop {
            self.check_stop()?;
            let before = self.stdout.len();
            if self.pump().is_some() {
                return Ok(None);
            }
            if self.stdout.len() != before {
                quiet_since = None;
                continue;
            }
            if let Some(prompt) = self.pending_prompt() {
                let since = *quiet_since.get_or_insert_with(Instant::now);
                if since.elapsed() >= SETTLE {
                    self.consumed = self.stdout.len();
                    return Ok(Some(prompt));
                }
            }
        }
    }

    /// Writes `line` and a line break to the tool's input.
    pub(crate) fn answer(&mut self, line: &str) -> Result<(), ShareError> {
        self.process
            .write(&format!("{line}\n"))
            .map_err(|error| ShareError::new(ShareErrorKind::Other, error.to_string()))
    }

    /// Stops the tool at once (an unexpected prompt).
    pub(crate) fn stop(&mut self) {
        if self.ended.is_none() {
            self.process.kill();
            self.ended = Some(-1);
        }
    }

    /// Closes the tool's input and waits for it to end.
    pub(crate) fn finish(mut self) -> Result<Finished, ShareError> {
        self.process.close_input();
        let code = loop {
            self.check_stop()?;
            if let Some(code) = self.pump() {
                break code;
            }
        };
        Ok(Finished {
            code,
            stdout: std::mem::take(&mut self.stdout),
            stderr: std::mem::take(&mut self.stderr),
        })
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stop();
    }
}

fn push_capped(buffer: &mut String, text: &str) {
    if buffer.len() < MAX_OUTPUT {
        buffer.push_str(text);
    }
}

/// Runs `command`, writes `input` (when given) once it started, and waits for the end.
pub(crate) fn run(
    spawner: &dyn Spawner,
    command: &ToolCommand,
    input: Option<&str>,
    cancel: &Cancel,
    limit: Duration,
) -> Result<Finished, ShareError> {
    let mut session = Session::start(spawner, command, cancel, limit)?;
    if let Some(input) = input {
        session.answer(input)?;
    }
    session.finish()
}

/// The last line of `text` that says something, for an error message.
pub(crate) fn last_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .unwrap_or_default()
        .to_owned()
}

/// A scripted stand-in for a tool, for tests on every platform.
#[cfg(test)]
pub(crate) mod fake {
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    use super::*;

    /// One step of a fake tool's script.
    pub(crate) enum Step {
        Out(&'static str),
        Err(&'static str),
        /// Reads one line of input; ends the tool with code 1 when the input was closed.
        Read,
        /// Runs a side effect (a mount appearing).
        Then(Box<dyn FnOnce() + Send>),
        /// Never ends by itself.
        Hang,
        Exit(i32),
    }

    /// What the fake tools saw.
    #[derive(Debug, Default)]
    pub(crate) struct Log {
        pub commands: Vec<ToolCommand>,
        pub input: String,
        pub killed: bool,
    }

    /// Hands out one script per started tool, in order.
    pub(crate) struct FakeSpawner {
        scripts: Mutex<VecDeque<Vec<Step>>>,
        pub log: Arc<Mutex<Log>>,
    }

    impl FakeSpawner {
        pub(crate) fn new(scripts: Vec<Vec<Step>>) -> Self {
            Self {
                scripts: Mutex::new(scripts.into()),
                log: Arc::default(),
            }
        }

        pub(crate) fn commands(&self) -> Vec<ToolCommand> {
            self.log.lock().unwrap().commands.clone()
        }

        pub(crate) fn input(&self) -> String {
            self.log.lock().unwrap().input.clone()
        }

        pub(crate) fn killed(&self) -> bool {
            self.log.lock().unwrap().killed
        }
    }

    impl Spawner for FakeSpawner {
        fn spawn(&self, command: &ToolCommand) -> io::Result<Box<dyn ToolProcess>> {
            self.log.lock().unwrap().commands.push(command.clone());
            let steps = self
                .scripts
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no fake tool left"))?;
            Ok(Box::new(FakeProcess {
                steps: steps.into(),
                lines: VecDeque::new(),
                partial: String::new(),
                input_closed: false,
                ended: None,
                log: Arc::clone(&self.log),
            }))
        }
    }

    struct FakeProcess {
        steps: VecDeque<Step>,
        lines: VecDeque<String>,
        partial: String,
        input_closed: bool,
        ended: Option<i32>,
        log: Arc<Mutex<Log>>,
    }

    impl ToolProcess for FakeProcess {
        fn read(&mut self, wait: Duration) -> Event {
            loop {
                if let Some(code) = self.ended {
                    return Event::Ended(code);
                }
                match self.steps.front() {
                    None => self.ended = Some(0),
                    Some(Step::Out(text)) => {
                        let text = (*text).to_owned();
                        self.steps.pop_front();
                        return Event::Stdout(text);
                    }
                    Some(Step::Err(text)) => {
                        let text = (*text).to_owned();
                        self.steps.pop_front();
                        return Event::Stderr(text);
                    }
                    Some(Step::Read) => {
                        if self.lines.pop_front().is_some() {
                            self.steps.pop_front();
                        } else if self.input_closed {
                            self.ended = Some(1);
                        } else {
                            std::thread::sleep(wait.min(Duration::from_millis(2)));
                            return Event::Idle;
                        }
                    }
                    Some(Step::Then(_)) => {
                        if let Some(Step::Then(effect)) = self.steps.pop_front() {
                            effect();
                        }
                    }
                    Some(Step::Hang) => {
                        std::thread::sleep(wait.min(Duration::from_millis(2)));
                        return Event::Idle;
                    }
                    Some(Step::Exit(code)) => self.ended = Some(*code),
                }
            }
        }

        fn write(&mut self, text: &str) -> io::Result<()> {
            if self.input_closed || self.ended.is_some() {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed"));
            }
            self.log.lock().unwrap().input.push_str(text);
            self.partial.push_str(text);
            while let Some(at) = self.partial.find('\n') {
                let line: String = self.partial.drain(..=at).collect();
                self.lines.push_back(line);
            }
            Ok(())
        }

        fn close_input(&mut self) {
            self.input_closed = true;
        }

        fn kill(&mut self) {
            self.log.lock().unwrap().killed = true;
            self.ended = Some(-1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::{FakeSpawner, Step};
    use super::*;

    fn command() -> ToolCommand {
        ToolCommand::new("tool").arg("--x")
    }

    #[test]
    fn a_prompt_is_answered_and_the_output_collected() {
        let spawner = FakeSpawner::new(vec![vec![
            Step::Out("Hello\nName: "),
            Step::Read,
            Step::Out("done\n"),
            Step::Err("warning\n"),
            Step::Exit(3),
        ]]);
        let mut session =
            Session::start(&spawner, &command(), &Cancel::new(), Duration::from_secs(5)).unwrap();
        assert_eq!(session.prompt().unwrap().as_deref(), Some("Name:"));
        session.answer("alex").unwrap();
        assert_eq!(session.prompt().unwrap(), None);
        let finished = session.finish().unwrap();
        assert_eq!(finished.code, 3);
        assert_eq!(finished.stdout, "Hello\nName: done\n");
        assert_eq!(finished.stderr, "warning\n");
        assert_eq!(spawner.input(), "alex\n");
    }

    #[test]
    fn a_line_ending_with_a_colon_before_more_output_is_no_prompt() {
        let spawner = FakeSpawner::new(vec![vec![
            Step::Out("Enter user and password for share:"),
            Step::Out("\nUser [me]: "),
            Step::Read,
            Step::Exit(0),
        ]]);
        let mut session =
            Session::start(&spawner, &command(), &Cancel::new(), Duration::from_secs(5)).unwrap();
        assert_eq!(session.prompt().unwrap().as_deref(), Some("User [me]:"));
    }

    #[test]
    fn a_cancel_stops_the_tool() {
        let spawner = FakeSpawner::new(vec![vec![Step::Hang]]);
        let cancel = Cancel::new();
        let session =
            Session::start(&spawner, &command(), &cancel, Duration::from_secs(5)).unwrap();
        cancel.cancel();
        let error = session.finish().unwrap_err();
        assert_eq!(error.kind(), ShareErrorKind::Cancelled);
        assert!(error.is_interrupted());
        assert!(spawner.killed());
    }

    #[test]
    fn a_tool_that_does_not_end_in_time_counts_as_unreachable() {
        let spawner = FakeSpawner::new(vec![vec![Step::Hang]]);
        let cancel = Cancel::with_limit(Duration::from_millis(30));
        let error = run(&spawner, &command(), None, &cancel, Duration::from_secs(5)).unwrap_err();
        assert_eq!(error.kind(), ShareErrorKind::Unreachable);
        assert!(error.is_interrupted());
        assert!(spawner.killed());
    }

    #[test]
    fn utf8_cut_between_reads_is_kept_whole() {
        let mut pending = "Grü".as_bytes().to_vec();
        let last = pending.pop().unwrap();
        assert_eq!(take_text(&mut pending), "Gr");
        pending.push(last);
        assert_eq!(take_text(&mut pending), "ü");
        assert!(pending.is_empty());
    }

    #[test]
    fn the_last_line_is_the_message() {
        assert_eq!(last_line("a\n  b failed  \n\n"), "b failed");
        assert_eq!(last_line(""), "");
    }

    #[test]
    fn a_real_tool_runs_with_its_input() {
        // A system program that prints what it read, on every platform CI runs.
        let command = if cfg!(windows) {
            ToolCommand::new("findstr").arg("hel")
        } else {
            ToolCommand::new("/bin/sh")
                .arg("-c")
                .arg("read x; echo got $x")
        };
        let finished = run(
            &SystemSpawner,
            &command,
            Some("hello"),
            &Cancel::new(),
            Duration::from_secs(20),
        )
        .unwrap();
        assert_eq!(finished.code, 0);
        assert!(finished.stdout.contains("hello"), "{}", finished.stdout);
    }
}
