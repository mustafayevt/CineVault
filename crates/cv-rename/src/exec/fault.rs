//! A fault-injection file system for tests.
//!
//! [`FaultFs`] wraps a real [`Fs`] (the disk, by default) and makes it fail on purpose, so a test
//! can show that a rename, a move, an undo or a recovery leaves every file at its old or its new
//! place whatever goes wrong:
//!
//! - **Stop after N operations.** [`FaultFs::stop_at`] kills the "process" when the n-th
//!   operation that changes the disk (a rename, a copy, a file created, a file or folder
//!   removed) starts, or right after it is done. The stop is a panic whose payload is [`Stop`],
//!   raised with [`std::panic::resume_unwind`] so nothing after it runs and no panic message is
//!   printed; the test catches it with [`std::panic::catch_unwind`] and then plays the restart.
//! - **Stop after N bytes.** [`FaultFs::stop_after_bytes`] kills it in the middle of a copy once
//!   that many bytes were written in all; the bytes before the stop stay on the disk, as a
//!   half-written file does after a real kill.
//! - **Fail at the N-th operation.** [`FaultFs::fail_at`] makes that operation fail with a
//!   [`Fault`] instead (the code under test goes on and has to handle it).
//! - **Disk full.** [`FaultFs::disk_full_after`] makes writes fail with the system's "disk full"
//!   error once that many bytes were written in all (the write that crosses the limit is short,
//!   like a real one).
//! - **Errors by rule.** A [`Rule`] makes chosen operations on chosen paths fail with a
//!   [`Fault`]: disk full, permission denied, a file in use (a sharing violation on Windows,
//!   `EBUSY` elsewhere), across devices, or any error kind. A rule can fire a set number of times
//!   (a handle held for a moment) or always (a handle never let go).
//!
//! The same shim serves the executor of `cv-rename` (through [`Fs`]) and the move engine of
//! `cv-app`, whose file system adds streams: it wraps its own writers with [`FaultFs::create`]
//! and its readers with [`FaultFs::open`], so byte limits and rules apply to a chunked copy too.
//!
//! The shim is compiled for this crate's tests and, with the `fault-injection` feature, for the
//! tests of other crates. It is never part of the app.

use std::any::Any;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use super::PART_SUFFIX;
use super::fs::{Fs, Meta, StdFs};

/// The payload of a simulated stop: the process was killed, nothing after it ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stop;

/// True when a caught panic is a stop of the shim.
#[must_use]
pub fn is_stop(payload: &(dyn Any + Send)) -> bool {
    payload.downcast_ref::<Stop>().is_some()
}

fn stop() -> ! {
    std::panic::resume_unwind(Box::new(Stop))
}

/// An operation of the file system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Meta,
    CreateDir,
    Rename,
    /// A whole-file copy through [`Fs::copy_file`].
    Copy,
    /// A file created for writing (the move engine's chunked copy).
    Create,
    /// A file opened for reading (the move engine's chunked copy).
    Open,
    SameContent,
    RemoveFile,
    RemoveDir,
    List,
}

impl Op {
    /// Every operation that changes the disk. These are the ones [`FaultFs::stop_at`] counts.
    pub const MUTATIONS: [Op; 5] = [
        Op::Rename,
        Op::Copy,
        Op::Create,
        Op::RemoveFile,
        Op::RemoveDir,
    ];

    /// Every operation.
    pub const ALL: [Op; 10] = [
        Op::Meta,
        Op::CreateDir,
        Op::Rename,
        Op::Copy,
        Op::Create,
        Op::Open,
        Op::SameContent,
        Op::RemoveFile,
        Op::RemoveDir,
        Op::List,
    ];

    #[must_use]
    pub fn mutates(self) -> bool {
        Self::MUTATIONS.contains(&self)
    }
}

/// What a failing operation reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// The volume is full (`ERROR_DISK_FULL`, `ENOSPC`).
    DiskFull,
    /// Not allowed (`ERROR_ACCESS_DENIED`, `EACCES`). On Windows this is also what a file held
    /// without delete sharing gives, so [`cv_fs::retry`] tries it again for a moment first.
    PermissionDenied,
    /// Held by another program (`ERROR_SHARING_VIOLATION`, `EBUSY`); tried again for a moment.
    InUse,
    /// The two paths are on different volumes.
    CrossesDevices,
    /// Any other error, never tried again.
    Kind(io::ErrorKind),
}

impl Fault {
    /// The error as the system gives it.
    #[must_use]
    pub fn error(self) -> io::Error {
        let windows = cfg!(windows);
        match self {
            Fault::DiskFull => io::Error::from_raw_os_error(if windows { 112 } else { 28 }),
            Fault::PermissionDenied => io::Error::from_raw_os_error(if windows { 5 } else { 13 }),
            Fault::InUse => io::Error::from_raw_os_error(if windows { 32 } else { 16 }),
            Fault::CrossesDevices => io::Error::from(io::ErrorKind::CrossesDevices),
            Fault::Kind(kind) => io::Error::from(kind),
        }
    }
}

type PathTest = Arc<dyn Fn(&Path, Option<&Path>) -> bool + Send + Sync>;

/// Which paths a rule applies to.
#[derive(Clone)]
enum Target {
    Any,
    /// The file name of the path (or of the source of a rename or copy) is this name, or this
    /// name with the `.cvpart` suffix of a copy that is being written.
    Named(String),
    /// The path (or the source) contains this text, with `/` as the separator.
    Within(String),
    /// The destination of a rename or copy contains this text, with `/` as the separator.
    Into(String),
    When(PathTest),
}

/// Makes some operations on some paths fail.
#[derive(Clone)]
pub struct Rule {
    ops: Vec<Op>,
    target: Target,
    fault: Fault,
    /// How many more times it fires; `None` is always.
    times: Option<usize>,
}

impl Rule {
    /// `fault` for every operation in `ops`, on any path, every time.
    #[must_use]
    pub fn on(ops: &[Op], fault: Fault) -> Self {
        Rule {
            ops: ops.to_vec(),
            target: Target::Any,
            fault,
            times: None,
        }
    }

    /// Only for a path (or the source of a rename or copy) whose file name is `name`, or `name`
    /// plus `.cvpart` (a copy of it being written).
    #[must_use]
    pub fn named(mut self, name: &str) -> Self {
        self.target = Target::Named(name.to_owned());
        self
    }

    /// Only for a path (or the source of a rename or copy) that contains `text` (`/` matches
    /// either separator).
    #[must_use]
    pub fn within(mut self, text: &str) -> Self {
        self.target = Target::Within(text.replace('\\', "/"));
        self
    }

    /// Only for a rename or copy whose destination contains `text` (`/` matches either
    /// separator).
    #[must_use]
    pub fn into(mut self, text: &str) -> Self {
        self.target = Target::Into(text.replace('\\', "/"));
        self
    }

    /// Only when `test(path, destination)` holds.
    #[must_use]
    pub fn when(
        mut self,
        test: impl Fn(&Path, Option<&Path>) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.target = Target::When(Arc::new(test));
        self
    }

    /// Fires only the next `n` times it matches, then lets the operation through.
    #[must_use]
    pub fn times(mut self, n: usize) -> Self {
        self.times = Some(n);
        self
    }

    fn matches(&self, op: Op, path: &Path, to: Option<&Path>) -> bool {
        if !self.ops.contains(&op) || self.times == Some(0) {
            return false;
        }
        let text = |p: &Path| p.to_string_lossy().replace('\\', "/");
        match &self.target {
            Target::Any => true,
            Target::Named(name) => path.file_name().is_some_and(|n| {
                let n = n.to_string_lossy();
                n == name.as_str()
                    || n.strip_suffix(PART_SUFFIX)
                        .is_some_and(|bare| bare == name.as_str())
            }),
            Target::Within(part) => text(path).contains(part.as_str()),
            Target::Into(part) => to.is_some_and(|to| text(to).contains(part.as_str())),
            Target::When(test) => test(path, to),
        }
    }
}

/// When a [`FaultFs::stop_at`] stop happens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum When {
    /// Before the operation touches the disk.
    Before,
    /// Right after it was done, before the caller hears of it.
    After,
}

#[derive(Default)]
struct Plan {
    stop_at: Option<(usize, When)>,
    fail_at: Option<(usize, Fault)>,
    stop_after_bytes: Option<u64>,
    disk_full_after: Option<u64>,
    cross_volume: bool,
    corrupt_copies: bool,
    rules: Vec<Rule>,
}

#[derive(Default)]
struct State {
    plan: Mutex<Plan>,
    ops: AtomicUsize,
    bytes: AtomicU64,
}

impl State {
    fn plan(&self) -> MutexGuard<'_, Plan> {
        self.plan.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A file system that fails on purpose. Clones share their faults and counters, so a test can
/// hand one clone to the code under test and change the faults through another.
pub struct FaultFs<F = StdFs> {
    inner: F,
    state: Arc<State>,
}

impl<F: Clone> Clone for FaultFs<F> {
    fn clone(&self) -> Self {
        FaultFs {
            inner: self.inner.clone(),
            state: Arc::clone(&self.state),
        }
    }
}

impl Default for FaultFs<StdFs> {
    fn default() -> Self {
        Self::new()
    }
}

impl FaultFs<StdFs> {
    /// The real disk, failing nowhere yet.
    #[must_use]
    pub fn new() -> Self {
        Self::over(StdFs)
    }

    /// Every rename into another folder fails as across devices, so the executor copies.
    #[must_use]
    pub fn across_volumes() -> Self {
        let fs = Self::new();
        fs.cross_volume(true);
        fs
    }

    /// Stops at mutation `at` (0 based), before it or right after it, optionally across volumes.
    #[must_use]
    pub fn crashing(at: usize, after: bool, cross_volume: bool) -> Self {
        let fs = Self::new();
        fs.stop_at(at, if after { When::After } else { When::Before });
        fs.cross_volume(cross_volume);
        fs
    }
}

impl<F> FaultFs<F> {
    /// `inner`, failing nowhere yet.
    pub fn over(inner: F) -> Self {
        FaultFs {
            inner,
            state: Arc::default(),
        }
    }

    /// The wrapped file system.
    pub fn inner(&self) -> &F {
        &self.inner
    }

    /// Stops at the `op`-th operation that changes the disk (0 based), counted from the start.
    pub fn stop_at(&self, op: usize, when: When) -> &Self {
        self.state.plan().stop_at = Some((op, when));
        self
    }

    /// The `op`-th operation that changes the disk (0 based) fails with `fault` instead of
    /// running; the ones before and after it run.
    pub fn fail_at(&self, op: usize, fault: Fault) -> &Self {
        self.state.plan().fail_at = Some((op, fault));
        self
    }

    /// Stops once `bytes` were written by copies in all; the bytes up to there are on the disk.
    pub fn stop_after_bytes(&self, bytes: u64) -> &Self {
        self.state.plan().stop_after_bytes = Some(bytes);
        self
    }

    /// Writes fail with "disk full" once `bytes` were written by copies in all.
    pub fn disk_full_after(&self, bytes: u64) -> &Self {
        self.state.plan().disk_full_after = Some(bytes);
        self
    }

    /// While on, a rename into another folder fails as across devices.
    pub fn cross_volume(&self, on: bool) -> &Self {
        self.state.plan().cross_volume = on;
        self
    }

    /// While on, every copy made through [`Fs::copy_file`] is cut short after it was written.
    pub fn corrupt_copies(&self, on: bool) -> &Self {
        self.state.plan().corrupt_copies = on;
        self
    }

    /// Adds a rule. Rules are tried in the order they were added; the first that matches fires.
    pub fn rule(&self, rule: Rule) -> &Self {
        self.state.plan().rules.push(rule);
        self
    }

    /// The next `n` operations that change the disk fail for a moment as when another program
    /// holds the path (`usize::MAX`: they never stop failing).
    pub fn busy(&self, n: usize) -> &Self {
        let rule = Rule::on(&Op::MUTATIONS, Fault::InUse);
        self.rule(if n == usize::MAX { rule } else { rule.times(n) })
    }

    /// Stops failing: room was made, the handle was let go, the permission was given. The
    /// counters keep counting.
    pub fn heal(&self) -> &Self {
        *self.state.plan() = Plan::default();
        self
    }

    /// The operations that changed the disk so far.
    #[must_use]
    pub fn ops(&self) -> usize {
        self.state.ops.load(Ordering::SeqCst)
    }

    /// The bytes copies wrote so far.
    #[must_use]
    pub fn bytes(&self) -> u64 {
        self.state.bytes.load(Ordering::SeqCst)
    }

    /// The rules and counters before an operation: the error of a rule that fires, or the index
    /// of the mutation (to stop after it), or a stop before it.
    fn gate(&self, op: Op, path: &Path, to: Option<&Path>) -> io::Result<Option<usize>> {
        let mut stop_now = false;
        let counted = {
            let mut plan = self.state.plan();
            if op == Op::Rename
                && plan.cross_volume
                && to.is_some_and(|to| path.parent() != to.parent())
            {
                return Err(Fault::CrossesDevices.error());
            }
            if let Some(rule) = plan.rules.iter_mut().find(|r| r.matches(op, path, to)) {
                if let Some(left) = rule.times.as_mut() {
                    *left -= 1;
                }
                return Err(rule.fault.error());
            }
            if op.mutates() {
                let n = self.state.ops.fetch_add(1, Ordering::SeqCst);
                if let Some((at, fault)) = plan.fail_at
                    && at == n
                {
                    return Err(fault.error());
                }
                stop_now = plan.stop_at == Some((n, When::Before));
                Some(n)
            } else {
                None
            }
        };
        if stop_now {
            stop();
        }
        Ok(counted)
    }

    /// Stops right after mutation `n` when the plan says so.
    fn after(&self, counted: Option<usize>) {
        if let Some(n) = counted
            && self.state.plan().stop_at == Some((n, When::After))
        {
            stop();
        }
    }

    fn mutate<T>(
        &self,
        op: Op,
        path: &Path,
        to: Option<&Path>,
        run: impl FnOnce() -> io::Result<T>,
    ) -> io::Result<T> {
        let counted = self.gate(op, path, to)?;
        let out = run();
        self.after(counted);
        out
    }

    /// Creates a file for writing through `open` (the caller's own writer), counted as a
    /// mutation, with the rules for [`Op::Create`] applied first. What is written to the result
    /// counts against [`Self::stop_after_bytes`] and [`Self::disk_full_after`].
    ///
    /// # Errors
    /// A rule fired, or `open` failed.
    pub fn create<W: Write>(
        &self,
        path: &Path,
        open: impl FnOnce() -> io::Result<W>,
    ) -> io::Result<FaultWriter<W>> {
        let inner = self.mutate(Op::Create, path, None, open)?;
        Ok(FaultWriter {
            inner,
            state: Arc::clone(&self.state),
        })
    }

    /// Opens a file for reading through `open`, with the rules for [`Op::Open`] applied first.
    ///
    /// # Errors
    /// A rule fired, or `open` failed.
    pub fn open<R: Read>(
        &self,
        path: &Path,
        open: impl FnOnce() -> io::Result<R>,
    ) -> io::Result<R> {
        self.gate(Op::Open, path, None)?;
        open()
    }

    fn byte_limits(&self) -> bool {
        let plan = self.state.plan();
        plan.stop_after_bytes.is_some() || plan.disk_full_after.is_some()
    }
}

/// A writer whose bytes count against the byte limits of its [`FaultFs`].
pub struct FaultWriter<W> {
    inner: W,
    state: Arc<State>,
}

impl<W> FaultWriter<W> {
    /// The caller's writer, to finish it.
    pub fn into_inner(self) -> W {
        self.inner
    }
}

enum Limit {
    Stop,
    Full,
}

impl<W: Write> Write for FaultWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let (stop_at, full_at) = {
            let plan = self.state.plan();
            (plan.stop_after_bytes, plan.disk_full_after)
        };
        let done = self.state.bytes.load(Ordering::SeqCst);
        // The nearer limit wins; at a tie, the stop.
        let mut allowed = buf.len() as u64;
        let mut limit = None;
        for (at, kind) in [(stop_at, Limit::Stop), (full_at, Limit::Full)] {
            if let Some(at) = at {
                let room = at.saturating_sub(done);
                if room < allowed {
                    allowed = room;
                    limit = Some(kind);
                }
            }
        }
        let allowed = usize::try_from(allowed).unwrap_or(usize::MAX);
        if allowed > 0 {
            let n = self.inner.write(&buf[..allowed])?;
            self.state.bytes.fetch_add(n as u64, Ordering::SeqCst);
            if n < allowed || limit.is_none() {
                return Ok(n);
            }
        }
        match limit {
            None => Ok(0),
            Some(Limit::Stop) => {
                let _ = self.inner.flush();
                stop()
            }
            Some(Limit::Full) if allowed > 0 => Ok(allowed),
            Some(Limit::Full) => Err(Fault::DiskFull.error()),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl<F: Fs> FaultFs<F> {
    /// A copy whose bytes count against the byte limits. It writes to the real disk.
    fn counted_copy(&self, from: &Path, to: &Path) -> io::Result<()> {
        let (src, dst) = (StdFs::native(from), StdFs::native(to));
        let modified = fs::metadata(&src)?.modified()?;
        let mut reader = File::open(&src)?;
        let mut writer = FaultWriter {
            inner: File::create(&dst)?,
            state: Arc::clone(&self.state),
        };
        io::copy(&mut reader, &mut writer)?;
        writer.into_inner().set_modified(modified)
    }
}

impl<F: Fs> Fs for FaultFs<F> {
    fn meta(&self, path: &Path) -> io::Result<Option<Meta>> {
        self.gate(Op::Meta, path, None)?;
        self.inner.meta(path)
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        self.gate(Op::CreateDir, path, None)?;
        self.inner.create_dir_all(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        self.mutate(Op::Rename, from, Some(to), || self.inner.rename(from, to))
    }

    fn copy_file(&self, from: &Path, to: &Path) -> io::Result<()> {
        self.mutate(Op::Copy, from, Some(to), || {
            if self.byte_limits() {
                self.counted_copy(from, to)?;
            } else {
                self.inner.copy_file(from, to)?;
            }
            if self.state.plan().corrupt_copies {
                fs::write(StdFs::native(to), b"cut")?;
            }
            Ok(())
        })
    }

    fn same_content(&self, a: &Path, b: &Path) -> io::Result<bool> {
        self.gate(Op::SameContent, a, Some(b))?;
        self.inner.same_content(a, b)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        self.mutate(Op::RemoveFile, path, None, || self.inner.remove_file(path))
    }

    fn remove_dir(&self, path: &Path) -> io::Result<()> {
        self.mutate(Op::RemoveDir, path, None, || self.inner.remove_dir(path))
    }

    fn list_dir(&self, path: &Path) -> io::Result<Vec<String>> {
        self.gate(Op::List, path, None)?;
        self.inner.list_dir(path)
    }
}

#[cfg(test)]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use super::*;

    fn dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn a_stop_before_the_nth_operation_leaves_it_undone_and_after_leaves_it_done() {
        for (when, renamed) in [(When::Before, false), (When::After, true)] {
            let d = dir();
            let (a, b, c) = (d.path().join("a"), d.path().join("b"), d.path().join("c"));
            fs::write(&a, "x").unwrap();
            fs::write(&c, "y").unwrap();
            let fs = FaultFs::new();
            fs.stop_at(1, when);
            fs.create_dir_all(&d.path().join("sub")).unwrap(); // not counted
            fs.remove_file(&c).unwrap(); // mutation 0
            let stopped = catch_unwind(AssertUnwindSafe(|| fs.rename(&a, &b)));
            assert!(is_stop(stopped.unwrap_err().as_ref()));
            assert_eq!(b.exists(), renamed, "{when:?}");
            assert_eq!(fs.ops(), 2);
        }
    }

    #[test]
    fn a_stop_after_n_bytes_leaves_the_bytes_before_it_on_the_disk() {
        let d = dir();
        let (src, dst) = (d.path().join("src"), d.path().join("dst"));
        fs::write(&src, vec![1_u8; 10_000]).unwrap();
        let fs = FaultFs::new();
        fs.stop_after_bytes(4_096);
        let stopped = catch_unwind(AssertUnwindSafe(|| fs.copy_file(&src, &dst)));
        assert!(is_stop(stopped.unwrap_err().as_ref()));
        assert_eq!(fs::metadata(&dst).unwrap().len(), 4_096);
        assert_eq!(fs.bytes(), 4_096);
    }

    #[test]
    fn a_full_disk_cuts_the_write_that_crosses_it_and_fails_the_next() {
        let d = dir();
        let fs = FaultFs::new();
        fs.disk_full_after(5);
        let path = d.path().join("f");
        let mut w = fs.create(&path, || File::create(&path)).unwrap();
        assert_eq!(w.write(b"abc").unwrap(), 3);
        assert_eq!(w.write(b"defg").unwrap(), 2);
        let full = w.write(b"h").unwrap_err();
        assert_eq!(full.kind(), io::ErrorKind::StorageFull);
        assert!(w.write_all(b"xyz").is_err());
        drop(w);
        assert_eq!(fs::read(&path).unwrap(), b"abcde");
    }

    #[test]
    fn rules_fail_the_chosen_operations_on_the_chosen_paths_and_can_run_out() {
        let d = dir();
        let (a, b) = (d.path().join("movie.mkv"), d.path().join("other.mkv"));
        fs::write(&a, "x").unwrap();
        fs::write(&b, "y").unwrap();
        let fs = FaultFs::new();
        fs.rule(
            Rule::on(&[Op::Rename], Fault::InUse)
                .named("movie.mkv")
                .times(2),
        );
        fs.rule(Rule::on(&[Op::Rename], Fault::PermissionDenied).into("trash"));
        let in_use = fs.rename(&a, &d.path().join("x")).unwrap_err();
        assert!(cv_fs::is_transient(&in_use));
        assert!(fs.rename(&a, &d.path().join("x")).is_err());
        fs.rename(&a, &d.path().join("x")).unwrap();
        let denied = fs.rename(&b, &d.path().join("trash")).unwrap_err();
        assert_eq!(denied.kind(), io::ErrorKind::PermissionDenied);
        assert!(b.exists());
        // A copy being written counts as the file it copies.
        let part = d.path().join("poster.jpg.cvpart");
        fs.rule(Rule::on(&[Op::Create], Fault::DiskFull).named("poster.jpg"));
        let full = fs.create(&part, || File::create(&part)).err().unwrap();
        assert_eq!(full.kind(), io::ErrorKind::StorageFull);
        assert!(!part.exists());
        fs.heal();
        fs.rename(&b, &d.path().join("trash")).unwrap();
    }

    #[test]
    fn busy_fails_the_next_mutations_for_a_moment_and_across_volumes_refuses_a_move() {
        let d = dir();
        let a = d.path().join("a");
        fs::write(&a, "x").unwrap();
        let fs = FaultFs::new();
        fs.busy(1);
        assert!(cv_fs::is_transient(
            &fs.rename(&a, &d.path().join("b")).unwrap_err()
        ));
        fs.rename(&a, &d.path().join("b")).unwrap();
        fs::create_dir(d.path().join("sub")).unwrap();
        fs.cross_volume(true);
        let across = fs
            .rename(&d.path().join("b"), &d.path().join("sub/b"))
            .unwrap_err();
        assert_eq!(across.kind(), io::ErrorKind::CrossesDevices);
        fs.rename(&d.path().join("b"), &d.path().join("c")).unwrap();
    }
}
