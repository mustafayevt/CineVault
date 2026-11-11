//! A scan's progress in titles, not library folders.
//!
//! A rescan job has one item per library folder, which says little when eight folders hold 700
//! titles on a slow network share. [`ScanProgress`] follows the job in titles instead:
//!
//! 1. **Counting.** Before the first folder is walked, the job lists every library folder it will
//!    read ([`crate::walk::count_titles`]) and reports "312 found" with the folder being listed.
//!    The listings run side by side and the phase ends after [`COUNT_DEADLINE`] at the latest;
//!    a folder that has not answered by then is counted when its walk lists it.
//! 2. **Working.** Each walk tells which title it reads and when it is done
//!    ([`crate::walk::WalkObserver`]): "312 of 707 titles · Example Movie (2020)". Once enough
//!    titles were timed ([`MIN_TIMED_TITLES`] over [`MIN_TIMED`]) an estimate of the time left
//!    is added; it is left out while it would be a guess, and when it is longer than a day.
//!
//! The total never drops while the job runs: a folder's titles are the larger of what the count
//! found and what its walk found, and a finished folder counts all of them as done. Reports go to
//! the job ([`cv_jobs::ItemControl::set_units`]) at most every [`REPORT_INTERVAL`], so at most
//! four a second reach the UI however fast the titles go; a change of phase and the end of a
//! folder are reported at once.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use cv_jobs::{UnitPhase, UnitProgress};

use crate::walk::WalkObserver;

/// The shortest time between two reports: at most four a second.
pub const REPORT_INTERVAL: Duration = Duration::from_millis(250);
/// The longest the counting phase waits for the library folders to be listed.
pub const COUNT_DEADLINE: Duration = Duration::from_secs(15);
/// Titles that must be done before the time left is estimated.
pub const MIN_TIMED_TITLES: u64 = 10;
/// Time the work must have run before the time left is estimated.
pub const MIN_TIMED: Duration = Duration::from_secs(5);
/// Estimates longer than this are not shown: they would be noise.
pub const MAX_ESTIMATE: Duration = Duration::from_secs(24 * 3600);

/// Where the reports go: the job's [`cv_jobs::ItemControl::set_units`] in the app.
pub type UnitSink = Arc<dyn Fn(UnitProgress) + Send + Sync>;

/// The time left at the pace so far: `done` titles in `elapsed`, `total` in all. `None` until
/// enough titles were timed, when nothing is left, and when the result would be absurd.
#[must_use]
pub fn estimate(done: u64, total: u64, elapsed: Duration) -> Option<f64> {
    if done < MIN_TIMED_TITLES || elapsed < MIN_TIMED || done >= total {
        return None;
    }
    let left = (total - done) as f64 * elapsed.as_secs_f64() / done as f64;
    (left.is_finite() && left > 0.0 && left <= MAX_ESTIMATE.as_secs_f64()).then_some(left)
}

/// The titles of one library folder (or one title folder) of the job.
#[derive(Debug, Default, Clone, Copy)]
struct Folder {
    /// What the counting phase found.
    counted: u64,
    /// What the walk found so far.
    seen: u64,
    done: u64,
    finished: bool,
}

impl Folder {
    fn total(&self) -> u64 {
        self.counted.max(self.seen).max(self.done)
    }
}

struct State {
    phase: UnitPhase,
    folders: BTreeMap<String, Folder>,
    current: Option<String>,
    /// When the working phase began, and the titles done by then (a resumed job's are not
    /// timed).
    working_since: Option<(Instant, u64)>,
    last_report: Option<Instant>,
    sink: Option<UnitSink>,
}

/// The progress of one rescan job in titles. Shared by the job's items and the walks' threads.
pub struct ScanProgress {
    state: Mutex<State>,
}

impl Default for ScanProgress {
    fn default() -> Self {
        Self {
            state: Mutex::new(State {
                phase: UnitPhase::Counting,
                folders: BTreeMap::new(),
                current: None,
                working_since: None,
                last_report: None,
                sink: None,
            }),
        }
    }
}

impl std::fmt::Debug for ScanProgress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScanProgress").finish_non_exhaustive()
    }
}

impl ScanProgress {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Sends the reports to `sink` from now on.
    pub fn connect(&self, sink: UnitSink) {
        self.lock().sink = Some(sink);
    }

    /// The counting phase is listing `name`.
    pub fn listing(&self, name: &str) {
        self.listing_at(name, Instant::now());
    }

    fn listing_at(&self, name: &str, now: Instant) {
        let mut state = self.lock();
        state.current = Some(name.to_owned());
        Self::report(&mut state, now, false);
    }

    /// The counting phase found `titles` in the folder `key` (one title folder: 1).
    pub fn counted(&self, key: &str, titles: usize) {
        self.counted_at(key, titles, Instant::now());
    }

    fn counted_at(&self, key: &str, titles: usize, now: Instant) {
        let mut state = self.lock();
        state.folders.entry(key.to_owned()).or_default().counted = titles as u64;
        Self::report(&mut state, now, false);
    }

    /// Counting is over; the titles are read now.
    pub fn start_working(&self) {
        self.start_working_at(Instant::now());
    }

    fn start_working_at(&self, now: Instant) {
        let mut state = self.lock();
        if state.phase == UnitPhase::Working {
            return;
        }
        state.phase = UnitPhase::Working;
        state.current = None;
        let done = state.folders.values().map(|f| f.done).sum();
        state.working_since = Some((now, done));
        Self::report(&mut state, now, true);
    }

    /// The title folder `key`, named `name`, is being read whole (a "Scan again" of one title).
    pub fn reading_title(&self, key: &str, name: &str) {
        let now = Instant::now();
        let mut state = self.lock();
        let folder = state.folders.entry(key.to_owned()).or_default();
        folder.seen = folder.seen.max(1);
        state.current = Some(name.to_owned());
        Self::report(&mut state, now, false);
    }

    /// The folder `key` is done: every title it counted is.
    pub fn finished(&self, key: &str) {
        self.finished_at(key, Instant::now());
    }

    fn finished_at(&self, key: &str, now: Instant) {
        let mut state = self.lock();
        let folder = state.folders.entry(key.to_owned()).or_default();
        folder.done = folder.total();
        folder.finished = true;
        Self::report(&mut state, now, true);
    }

    /// What the walk of the folder `key` tells goes into this progress.
    #[must_use]
    pub fn observer<'a>(&'a self, key: &'a str) -> FolderObserver<'a> {
        FolderObserver {
            progress: self,
            key,
        }
    }

    /// The progress as of `now`.
    #[must_use]
    pub fn snapshot(&self) -> UnitProgress {
        Self::view(&self.lock(), Instant::now())
    }

    fn view(state: &State, now: Instant) -> UnitProgress {
        let total: u64 = state.folders.values().map(Folder::total).sum();
        let done: u64 = state.folders.values().map(|f| f.done).sum();
        let eta_secs = match (state.phase, state.working_since) {
            (UnitPhase::Working, Some((since, before))) => estimate(
                done.saturating_sub(before),
                total.saturating_sub(before),
                now.saturating_duration_since(since),
            ),
            _ => None,
        };
        UnitProgress {
            phase: state.phase,
            done: if state.phase == UnitPhase::Counting {
                0
            } else {
                done
            },
            total,
            current: state.current.clone(),
            eta_secs,
        }
    }

    /// Sends the progress, unless the last report was under [`REPORT_INTERVAL`] ago and this one
    /// is not `forced`.
    fn report(state: &mut State, now: Instant, forced: bool) {
        let Some(sink) = state.sink.clone() else {
            return;
        };
        let due = state
            .last_report
            .is_none_or(|last| now.saturating_duration_since(last) >= REPORT_INTERVAL);
        if !(forced || due) {
            return;
        }
        state.last_report = Some(now);
        sink(Self::view(state, now));
    }

    fn found_at(&self, key: &str, count: usize, now: Instant) {
        let mut state = self.lock();
        let folder = state.folders.entry(key.to_owned()).or_default();
        folder.seen += count as u64;
        Self::report(&mut state, now, false);
    }

    fn reading_at(&self, name: &str, now: Instant) {
        let mut state = self.lock();
        state.current = Some(name.to_owned());
        Self::report(&mut state, now, false);
    }

    fn read_at(&self, key: &str, count: usize, now: Instant) {
        if count == 0 {
            return;
        }
        let mut state = self.lock();
        let folder = state.folders.entry(key.to_owned()).or_default();
        if !folder.finished {
            folder.done += count as u64;
        }
        Self::report(&mut state, now, false);
    }
}

/// The walk of one folder telling a [`ScanProgress`].
pub struct FolderObserver<'a> {
    progress: &'a ScanProgress,
    key: &'a str,
}

impl WalkObserver for FolderObserver<'_> {
    fn found(&self, count: usize) {
        self.progress.found_at(self.key, count, Instant::now());
    }

    fn reading(&self, name: &str) {
        self.progress.reading_at(name, Instant::now());
    }

    fn read(&self, count: usize) {
        self.progress.read_at(self.key, count, Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recorder() -> (UnitSink, Arc<Mutex<Vec<UnitProgress>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let into = Arc::clone(&seen);
        let sink: UnitSink = Arc::new(move |units| into.lock().unwrap().push(units));
        (sink, seen)
    }

    #[test]
    fn the_estimate_waits_for_enough_titles_and_time_and_is_never_absurd() {
        let minute = Duration::from_secs(60);
        assert_eq!(estimate(5, 100, minute), None, "too few titles");
        assert_eq!(estimate(50, 100, Duration::from_secs(2)), None, "too short");
        assert_eq!(estimate(100, 100, minute), None, "nothing left");
        // 10 titles a minute, 90 left: 9 minutes.
        assert_eq!(estimate(10, 100, minute), Some(540.0));
        // A crawl of one title a day would say weeks: not shown.
        assert_eq!(estimate(10, 1_000, Duration::from_secs(10 * 86_400)), None);
    }

    #[test]
    fn counting_then_working_reports_titles_not_folders() {
        let (sink, seen) = recorder();
        let progress = ScanProgress::default();
        progress.connect(sink);
        let t0 = Instant::now();
        progress.listing_at("Movies", t0);
        progress.counted_at("s1", 400, t0 + Duration::from_secs(1));
        progress.counted_at("s2", 307, t0 + Duration::from_secs(2));
        let counting = progress.snapshot();
        assert_eq!(counting.phase, UnitPhase::Counting);
        assert_eq!((counting.done, counting.total), (0, 707));

        progress.start_working_at(t0 + Duration::from_secs(2));
        let walk = progress.observer("s1");
        walk.found(400);
        for n in 0..312 {
            walk.reading(&format!("Title {n}"));
            walk.read(1);
        }
        let working = progress.snapshot();
        assert_eq!(working.phase, UnitPhase::Working);
        assert_eq!((working.done, working.total), (312, 707));
        assert_eq!(working.current.as_deref(), Some("Title 311"));
        let last = seen.lock().unwrap().last().cloned().unwrap();
        assert_eq!(last.phase, UnitPhase::Working);
    }

    #[test]
    fn reports_are_throttled_but_a_phase_change_and_a_finished_folder_go_at_once() {
        let (sink, seen) = recorder();
        let progress = ScanProgress::default();
        progress.connect(sink);
        let t0 = Instant::now();
        progress.counted_at("s1", 1_000, t0);
        progress.start_working_at(t0);
        // A thousand titles in one second: at most four reports a second, plus the forced ones.
        for n in 0..1_000u64 {
            progress.read_at("s1", 1, t0 + Duration::from_millis(n));
        }
        let sent = seen.lock().unwrap().len();
        assert!(sent <= 2 + 4 + 1, "{sent} reports in a second");
        progress.finished_at("s1", t0 + Duration::from_millis(1_001));
        let last = seen.lock().unwrap().last().cloned().unwrap();
        assert_eq!((last.done, last.total), (1_000, 1_000));
    }

    #[test]
    fn the_total_never_drops_and_a_finished_folder_is_whole() {
        let progress = ScanProgress::default();
        let t0 = Instant::now();
        progress.counted_at("s1", 10, t0);
        progress.counted_at("s2", 5, t0);
        progress.start_working_at(t0);
        // The walk finds fewer than counted (a folder went meanwhile), then more in another.
        progress.found_at("s1", 8, t0);
        progress.read_at("s1", 8, t0);
        assert_eq!(progress.snapshot().total, 15);
        progress.finished_at("s1", t0);
        progress.found_at("s2", 7, t0);
        let now = progress.snapshot();
        assert_eq!((now.done, now.total), (10, 17));
        progress.finished_at("s2", t0);
        let end = progress.snapshot();
        assert_eq!((end.done, end.total), (17, 17));
    }

    #[test]
    fn the_estimate_appears_once_enough_titles_were_timed() {
        let progress = ScanProgress::default();
        let t0 = Instant::now();
        progress.counted_at("s1", 100, t0);
        progress.start_working_at(t0);
        progress.read_at("s1", 5, t0 + Duration::from_secs(10));
        assert_eq!(
            ScanProgress::view(&progress.lock(), t0 + Duration::from_secs(10)).eta_secs,
            None
        );
        progress.read_at("s1", 15, t0 + Duration::from_secs(20));
        let eta = ScanProgress::view(&progress.lock(), t0 + Duration::from_secs(20)).eta_secs;
        // 20 titles in 20 s, 80 left.
        assert_eq!(eta, Some(80.0));
    }
}
