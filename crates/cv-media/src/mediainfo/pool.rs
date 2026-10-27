//! The worker pool: parallel reads, a limit on how many run at once and on how many run on one
//! drive, and a time limit per file that grows with its size.
//!
//! A hard disk serves one read well and several badly: MediaInfo seeks through a large remux to
//! find the first frames of every track, and a few such reads at once on one disk make each one
//! many times slower (a 96 GB file that takes 50 seconds alone did not finish in 120 seconds
//! with a read per core on the same disk). So reads are also limited per volume
//! ([`PoolOptions::per_volume`], one by default), while files on other drives are read in
//! parallel. The time limit starts at [`DEFAULT_TIMEOUT`] and adds [`DEFAULT_PER_GB`] for every
//! gigabyte of the file, up to [`MAX_TIMEOUT`].

use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use super::MediaInfoError;
use super::snapshot::{BatchItem, MediaSnapshot};

/// How long a small file may take before it is given up on; larger files get more
/// ([`PoolOptions::timeout_for`]).
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

/// How much longer a file may take for each gigabyte (10^9 bytes) of its size.
pub const DEFAULT_PER_GB: Duration = Duration::from_secs(3);

/// The longest a file may take, whatever its size.
pub const MAX_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// How often a waiting read looks whether it was cancelled.
const CANCEL_POLL: Duration = Duration::from_millis(100);

/// Reads one file. [`super::MediaInfoLibrary`] is the real implementation; tests use fakes to
/// exercise the pool's limits without a library.
pub trait Analyzer: Send + Sync + 'static {
    /// # Errors
    /// Whatever stops the file from being read.
    fn analyze(&self, path: &Path) -> Result<MediaSnapshot, MediaInfoError>;
}

/// Pool settings.
#[derive(Clone, Copy, Debug)]
pub struct PoolOptions {
    /// The most files read at the same time. Never below 1.
    pub limit: usize,
    /// The most files read at the same time from one volume (see [`volume_key`]). Never below 1.
    pub per_volume: usize,
    /// How long a small file may take.
    pub timeout: Duration,
    /// How much longer a file may take for each gigabyte of its size.
    pub per_gb: Duration,
    /// The longest any file may take; never below `timeout`.
    pub max_timeout: Duration,
}

impl Default for PoolOptions {
    /// One reader per CPU core, one per volume, and 120 seconds plus 3 seconds per gigabyte per
    /// file, at most 15 minutes.
    fn default() -> Self {
        Self {
            limit: std::thread::available_parallelism().map_or(1, usize::from),
            per_volume: 1,
            timeout: DEFAULT_TIMEOUT,
            per_gb: DEFAULT_PER_GB,
            max_timeout: MAX_TIMEOUT,
        }
    }
}

impl PoolOptions {
    /// The time limit of a file of `size_bytes`: the base limit, plus [`Self::per_gb`] for every
    /// started gigabyte, capped at [`Self::max_timeout`].
    #[must_use]
    pub fn timeout_for(&self, size_bytes: u64) -> Duration {
        let gigabytes = u32::try_from(size_bytes.div_ceil(1_000_000_000)).unwrap_or(u32::MAX);
        let extra = self.per_gb.saturating_mul(gigabytes);
        self.timeout
            .saturating_add(extra)
            .min(self.max_timeout.max(self.timeout))
    }
}

/// Which volume `path` is on, as the per-volume limit counts it: the drive (`D:`) or network
/// share (`\\server\share`) on Windows, the device on macOS and Linux. Paths on one volume give
/// the same key; a path whose volume cannot be told gives an empty key.
#[must_use]
pub fn volume_key(path: &Path) -> String {
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        match path.components().next() {
            Some(Component::Prefix(prefix)) => match prefix.kind() {
                Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
                    format!("{}:", char::from(letter).to_ascii_uppercase())
                }
                Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => format!(
                    r"\\{}\{}",
                    server.to_string_lossy().to_lowercase(),
                    share.to_string_lossy().to_lowercase()
                ),
                _ => prefix.as_os_str().to_string_lossy().to_lowercase(),
            },
            _ => String::new(),
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        path.ancestors()
            .find_map(|dir| std::fs::metadata(dir).ok())
            .map(|meta| meta.dev().to_string())
            .unwrap_or_default()
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = path;
        String::new()
    }
}

/// A counting semaphore: at most `limit` reads hold a permit at once.
struct Gate {
    free: Mutex<usize>,
    released: Condvar,
}

struct Permit<'a>(&'a Gate);

impl Gate {
    fn new(limit: usize) -> Self {
        Self {
            free: Mutex::new(limit),
            released: Condvar::new(),
        }
    }

    /// Waits for a permit, giving up (`None`) as soon as `cancelled` says so.
    fn acquire_unless(&self, cancelled: &dyn Fn() -> bool) -> Option<Permit<'_>> {
        let mut free = lock(&self.free);
        while *free == 0 {
            if cancelled() {
                return None;
            }
            free = self
                .released
                .wait_timeout(free, CANCEL_POLL)
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .0;
        }
        *free -= 1;
        Some(Permit(self))
    }
}

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        *lock(&self.0.free) += 1;
        self.0.released.notify_one();
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

const RUNNING: u8 = 0;
const FINISHED: u8 = 1;
const ABANDONED: u8 = 2;

#[derive(Default)]
struct Stats {
    active: AtomicUsize,
    peak: AtomicUsize,
    abandoned: AtomicUsize,
}

/// A pool of MediaInfo readers.
///
/// Every read happens on a thread of its own with its own library instance, so there is no global
/// lock. A permit gate keeps at most [`PoolOptions::limit`] reads running, and a gate per volume
/// at most [`PoolOptions::per_volume`] on one drive (the volume's permit is taken first, so a read
/// waiting for its drive never holds a slot another drive could use). A read that takes longer
/// than its time limit ([`PoolOptions::timeout_for`]) is reported as [`MediaInfoError::Timeout`]
/// and the pool moves on; the stuck thread cannot be killed, so it is left to finish by itself (it
/// is counted by [`Self::abandoned`] and frees its instance when it does). It keeps its volume's
/// permit until then, so a stuck disk is not given a second read.
pub struct MediaInfoPool<A: Analyzer = super::MediaInfoLibrary> {
    analyzer: Arc<A>,
    gate: Gate,
    limit: usize,
    options: PoolOptions,
    volumes: Mutex<HashMap<String, Arc<Gate>>>,
    stats: Arc<Stats>,
}

/// The permit of a volume, owned so a read's thread can keep it after the caller gave up.
struct VolumePermit(Arc<Gate>);

impl Drop for VolumePermit {
    fn drop(&mut self) {
        *lock(&self.0.free) += 1;
        self.0.released.notify_one();
    }
}

impl<A: Analyzer> MediaInfoPool<A> {
    #[must_use]
    pub fn new(analyzer: A, options: PoolOptions) -> Self {
        let limit = options.limit.max(1);
        Self {
            analyzer: Arc::new(analyzer),
            gate: Gate::new(limit),
            limit,
            options: PoolOptions {
                limit,
                per_volume: options.per_volume.max(1),
                ..options
            },
            volumes: Mutex::default(),
            stats: Arc::default(),
        }
    }

    /// The settings in use.
    #[must_use]
    pub const fn options(&self) -> &PoolOptions {
        &self.options
    }

    /// Waits for a read slot on the volume of `path`; `None` when cancelled while waiting.
    fn volume_permit(&self, path: &Path, cancelled: &dyn Fn() -> bool) -> Option<VolumePermit> {
        let gate = Arc::clone(
            lock(&self.volumes)
                .entry(volume_key(path))
                .or_insert_with(|| Arc::new(Gate::new(self.options.per_volume))),
        );
        let permit = gate.acquire_unless(cancelled)?;
        // The count stays taken; the owned permit gives it back.
        std::mem::forget(permit);
        Some(VolumePermit(gate))
    }

    /// The most files read at once.
    #[must_use]
    pub const fn limit(&self) -> usize {
        self.limit
    }

    /// The most reads that actually ran at the same moment so far.
    #[must_use]
    pub fn peak_concurrency(&self) -> usize {
        self.stats.peak.load(Ordering::Relaxed)
    }

    /// Reads that timed out and are still running in the background.
    #[must_use]
    pub fn abandoned(&self) -> usize {
        self.stats.abandoned.load(Ordering::Relaxed)
    }

    /// Reads one file, waiting for a free slot first, and gives up after the time limit.
    ///
    /// # Errors
    /// [`MediaInfoError::Timeout`] after the limit, [`MediaInfoError::Crashed`] if the reader
    /// panicked, otherwise whatever the analyzer reports.
    pub fn extract(&self, path: &Path) -> Result<MediaSnapshot, MediaInfoError> {
        self.extract_unless(path, &|| false)
    }

    /// [`Self::extract`] that stops waiting as soon as `cancelled` says so, whether it waits for
    /// a slot or for the read: the read itself cannot be stopped and finishes in the background.
    ///
    /// # Errors
    /// As [`Self::extract`], and [`MediaInfoError::Cancelled`].
    pub fn extract_unless(
        &self,
        path: &Path,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<MediaSnapshot, MediaInfoError> {
        let cancel = || MediaInfoError::Cancelled {
            path: path.to_path_buf(),
        };
        // The drive first, then a slot: waiting for a busy drive holds no slot.
        let volume = self.volume_permit(path, cancelled).ok_or_else(cancel)?;
        let _permit = self.gate.acquire_unless(cancelled).ok_or_else(cancel)?;
        let size = std::fs::metadata(path)
            .ok()
            .filter(std::fs::Metadata::is_file)
            .map_or(0, |meta| meta.len());
        let timeout = self.options.timeout_for(size);
        let (sender, receiver) = mpsc::channel();
        let state = Arc::new(AtomicU8::new(RUNNING));
        let job = {
            let (analyzer, stats, state) =
                (self.analyzer.clone(), self.stats.clone(), state.clone());
            let path = path.to_path_buf();
            move || {
                // Held until the read really ends, even when the caller gave up on it.
                let _volume = volume;
                // So is the file: a rename of its folder waits for the read (`cv_fs::reading`).
                let _reading = cv_fs::reading(&path);
                let now = stats.active.fetch_add(1, Ordering::SeqCst) + 1;
                stats.peak.fetch_max(now, Ordering::SeqCst);
                let outcome = catch_unwind(AssertUnwindSafe(|| analyzer.analyze(&path)));
                stats.active.fetch_sub(1, Ordering::SeqCst);
                let outcome = match outcome {
                    Ok(outcome) => outcome,
                    Err(_) => Err(MediaInfoError::Crashed { path }),
                };
                // If the caller already gave up on this read, record that it ended.
                if state
                    .compare_exchange(RUNNING, FINISHED, Ordering::SeqCst, Ordering::SeqCst)
                    .is_err()
                {
                    stats.abandoned.fetch_sub(1, Ordering::SeqCst);
                }
                // The receiver is gone after a timeout; nobody wants the result then.
                let _ = sender.send(outcome);
            }
        };
        std::thread::Builder::new()
            .name("mediainfo-read".to_owned())
            .spawn(job)
            .map_err(MediaInfoError::Thread)?;

        let deadline = Instant::now() + timeout;
        let waited = loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match receiver.recv_timeout(left.min(CANCEL_POLL)) {
                Err(RecvTimeoutError::Timeout) if left > CANCEL_POLL && !cancelled() => {}
                other => break other,
            }
        };
        let stopped = cancelled();
        match waited {
            Ok(outcome) => outcome,
            Err(RecvTimeoutError::Timeout) => {
                // Count first, so the thread's decrement can never run ahead of it.
                self.stats.abandoned.fetch_add(1, Ordering::SeqCst);
                if state
                    .compare_exchange(RUNNING, ABANDONED, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
                {
                    if stopped {
                        Err(cancel())
                    } else {
                        Err(MediaInfoError::Timeout {
                            path: path.to_path_buf(),
                            seconds: timeout.as_secs().max(1),
                        })
                    }
                } else {
                    // It finished in the instant between the timeout and now: use its result.
                    self.stats.abandoned.fetch_sub(1, Ordering::SeqCst);
                    receiver.recv().unwrap_or_else(|_| {
                        Err(MediaInfoError::Crashed {
                            path: path.to_path_buf(),
                        })
                    })
                }
            }
            Err(RecvTimeoutError::Disconnected) => Err(MediaInfoError::Crashed {
                path: path.to_path_buf(),
            }),
        }
    }

    /// Reads `paths` in parallel and calls `done(index, path, result)` as each finishes, from the
    /// worker threads, in completion order. One failing file never stops the others.
    ///
    /// `keep_going` is asked before each file starts; return false to stop starting new files
    /// (files already running finish). Returns how many files were started.
    pub fn extract_each<F, K>(&self, paths: &[PathBuf], keep_going: K, done: F) -> usize
    where
        F: Fn(usize, &Path, Result<MediaSnapshot, MediaInfoError>) + Sync,
        K: Fn() -> bool + Sync,
    {
        let next = AtomicUsize::new(0);
        let started = AtomicUsize::new(0);
        let workers = self.limit.min(paths.len());
        std::thread::scope(|scope| {
            for _ in 0..workers {
                scope.spawn(|| {
                    loop {
                        if !keep_going() {
                            return;
                        }
                        let index = next.fetch_add(1, Ordering::SeqCst);
                        let Some(path) = paths.get(index) else {
                            return;
                        };
                        started.fetch_add(1, Ordering::SeqCst);
                        done(index, path, self.extract(path));
                    }
                });
            }
        });
        started.load(Ordering::SeqCst)
    }

    /// Reads `paths` in parallel; the results are in the same order as `paths`.
    #[must_use]
    pub fn extract_all(&self, paths: &[PathBuf]) -> Vec<BatchItem> {
        let slots: Mutex<Vec<Option<Result<MediaSnapshot, MediaInfoError>>>> =
            Mutex::new((0..paths.len()).map(|_| None).collect());
        self.extract_each(
            paths,
            || true,
            |index, _, result| lock(&slots)[index] = Some(result),
        );
        let results = slots
            .into_inner()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        paths
            .iter()
            .zip(results)
            .map(|(path, result)| BatchItem {
                path: path.clone(),
                result: result
                    .unwrap_or_else(|| Err(MediaInfoError::Crashed { path: path.clone() })),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;
    use std::time::Instant;

    use super::*;
    use crate::mediainfo::{StreamInfo, StreamKind};

    /// A fake reader: sleeps, then answers according to the file name.
    struct Fake {
        delay: Duration,
    }

    impl Analyzer for Fake {
        fn analyze(&self, path: &Path) -> Result<MediaSnapshot, MediaInfoError> {
            std::thread::sleep(self.delay);
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with("bad") {
                return Err(MediaInfoError::Open {
                    path: path.to_path_buf(),
                });
            }
            assert!(!name.starts_with("panic"), "the reader blew up");
            if name.starts_with("hang") {
                std::thread::sleep(Duration::from_millis(600));
            }
            let mut general = StreamInfo::new();
            general.push("CompleteName", name);
            let mut snapshot = MediaSnapshot::new();
            snapshot.push(StreamKind::General, general);
            Ok(snapshot)
        }
    }

    fn pool(limit: usize, delay_ms: u64, timeout_ms: u64) -> MediaInfoPool<Fake> {
        MediaInfoPool::new(
            Fake {
                delay: Duration::from_millis(delay_ms),
            },
            PoolOptions {
                limit,
                // The fake paths are all on one (unknown) volume; these tests are about the
                // overall limit.
                per_volume: usize::MAX,
                timeout: Duration::from_millis(timeout_ms),
                per_gb: Duration::ZERO,
                max_timeout: Duration::from_millis(timeout_ms),
            },
        )
    }

    /// A fake reader that records how many reads run at once on each volume.
    struct PerVolume {
        delay: Duration,
        active: Mutex<HashMap<String, usize>>,
        peak: Mutex<HashMap<String, usize>>,
    }

    impl Analyzer for PerVolume {
        fn analyze(&self, path: &Path) -> Result<MediaSnapshot, MediaInfoError> {
            let key = volume_key(path);
            {
                let mut active = lock(&self.active);
                let now = active.entry(key.clone()).or_default();
                *now += 1;
                let now = *now;
                let mut peak = lock(&self.peak);
                let top = peak.entry(key.clone()).or_default();
                *top = (*top).max(now);
            }
            std::thread::sleep(self.delay);
            *lock(&self.active).entry(key).or_default() -= 1;
            Ok(MediaSnapshot::new())
        }
    }

    fn per_volume_pool(limit: usize, per_volume: usize) -> MediaInfoPool<PerVolume> {
        MediaInfoPool::new(
            PerVolume {
                delay: Duration::from_millis(30),
                active: Mutex::default(),
                peak: Mutex::default(),
            },
            PoolOptions {
                limit,
                per_volume,
                ..PoolOptions::default()
            },
        )
    }

    fn files(prefix: &str, count: usize) -> Vec<PathBuf> {
        (0..count)
            .map(|i| PathBuf::from(format!("{prefix}{i:03}.mkv")))
            .collect()
    }

    fn name_of(item: &BatchItem) -> String {
        item.result
            .as_ref()
            .ok()
            .and_then(|s| s.general())
            .and_then(|g| g.get("CompleteName"))
            .unwrap_or("")
            .to_owned()
    }

    #[test]
    fn a_file_counts_as_read_until_its_read_really_ends_even_after_a_timeout() {
        // A rename of the folder waits for CineVault's own reads; a read the pool gave up on
        // still has the file open, so it must still count.
        let folder = std::env::temp_dir()
            .join(format!("cv-media-pool-{}", std::process::id()))
            .join("Bridge of Spies (2015)");
        let stream = folder.join("BDMV").join("STREAM").join("hang-00800.m2ts");
        let pool = pool(1, 0, 100);
        assert!(matches!(
            pool.extract(&stream),
            Err(MediaInfoError::Timeout { .. })
        ));
        assert_eq!(cv_fs::read_inside(&folder), Some(stream));
        assert_eq!(
            cv_fs::wait_until_unread(&folder, Duration::from_secs(10)),
            None,
            "the read ended and let go of the file"
        );
    }

    #[test]
    fn fifty_files_run_in_parallel_up_to_the_limit_and_keep_their_order() {
        let pool = pool(4, 40, 10_000);
        let paths = files("movie", 50);
        let items = pool.extract_all(&paths);
        assert_eq!(items.len(), 50);
        for (item, path) in items.iter().zip(&paths) {
            assert_eq!(item.path(), path);
            assert_eq!(name_of(item), path.to_string_lossy());
        }
        // The limit is used (the reads overlap) but never exceeded. This counts the overlap
        // instead of timing the batch, which a loaded machine would make flaky.
        assert_eq!(
            pool.peak_concurrency(),
            4,
            "the limit is used but never exceeded"
        );
    }

    #[test]
    fn a_limit_of_one_reads_one_file_at_a_time() {
        let pool = pool(1, 5, 10_000);
        let items = pool.extract_all(&files("movie", 8));
        assert!(items.iter().all(|i| i.result.is_ok()));
        assert_eq!(pool.peak_concurrency(), 1);
    }

    #[test]
    fn the_limit_is_at_least_one() {
        let pool = pool(0, 1, 10_000);
        assert_eq!(pool.limit(), 1);
        assert!(pool.extract(Path::new("movie.mkv")).is_ok());
    }

    #[test]
    fn one_bad_file_is_reported_and_the_other_199_are_processed() {
        let pool = pool(8, 1, 10_000);
        let mut paths = files("movie", 200);
        paths[77] = PathBuf::from("bad-file.mkv");
        let items = pool.extract_all(&paths);
        let failed: Vec<_> = items.iter().filter(|i| i.result.is_err()).collect();
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].path(), Path::new("bad-file.mkv"));
        assert_eq!(items.iter().filter(|i| i.result.is_ok()).count(), 199);
    }

    #[test]
    fn a_panicking_reader_is_reported_as_crashed_and_the_batch_continues() {
        let pool = pool(2, 1, 10_000);
        let paths = vec![
            PathBuf::from("movie1.mkv"),
            PathBuf::from("panic.mkv"),
            PathBuf::from("movie2.mkv"),
        ];
        let items = pool.extract_all(&paths);
        assert!(items[0].result.is_ok());
        assert!(matches!(
            items[1].result,
            Err(MediaInfoError::Crashed { .. })
        ));
        assert!(items[2].result.is_ok());
    }

    #[test]
    fn a_read_over_the_time_limit_times_out_and_frees_its_slot() {
        let pool = pool(1, 0, 150);
        let hung = pool.extract(Path::new("hang.mkv"));
        assert!(
            matches!(hung, Err(MediaInfoError::Timeout { .. })),
            "{hung:?}"
        );
        // The caller was released while the stuck read is still running: it is abandoned, not
        // waited for.
        assert_eq!(pool.abandoned(), 1);
        // The slot is free again, so the next file is read at once.
        assert!(pool.extract(Path::new("movie.mkv")).is_ok());
        // The stuck read ends by itself and is no longer counted.
        let deadline = Instant::now() + Duration::from_secs(5);
        while pool.abandoned() > 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(pool.abandoned(), 0);
    }

    #[test]
    fn the_timeout_message_names_the_file_and_the_limit() {
        let error = MediaInfoError::Timeout {
            path: PathBuf::from("a.mkv"),
            seconds: DEFAULT_TIMEOUT.as_secs(),
        };
        assert_eq!(
            error.to_string(),
            "Reading a.mkv took longer than 120 seconds, so it was skipped."
        );
        assert_eq!(PoolOptions::default().timeout, Duration::from_secs(120));
        assert!(PoolOptions::default().limit >= 1);
    }

    #[test]
    fn files_on_one_drive_are_read_one_at_a_time() {
        let dir = tempfile::tempdir().unwrap();
        let paths: Vec<PathBuf> = (0..8)
            .map(|i| dir.path().join(format!("movie{i}.mkv")))
            .collect();
        let pool = per_volume_pool(8, 1);
        let items = pool.extract_all(&paths);
        assert!(items.iter().all(|i| i.result.is_ok()));
        let key = volume_key(dir.path());
        assert_eq!(
            lock(&pool.analyzer.peak).get(&key).copied(),
            Some(1),
            "a drive never serves two reads at once"
        );
        assert_eq!(pool.peak_concurrency(), 1);

        // Allowing two per drive lets two run.
        let pool = per_volume_pool(8, 2);
        let _ = pool.extract_all(&paths);
        assert_eq!(lock(&pool.analyzer.peak).get(&key).copied(), Some(2));
    }

    #[cfg(windows)]
    #[test]
    fn files_on_different_drives_are_read_in_parallel() {
        let pool = per_volume_pool(8, 1);
        let mut paths = Vec::new();
        for i in 0..6 {
            paths.push(PathBuf::from(format!(r"X:\Movies\a{i}.mkv")));
            paths.push(PathBuf::from(format!(r"Y:\Movies\b{i}.mkv")));
            paths.push(PathBuf::from(format!(r"\\nas\media\c{i}.mkv")));
        }
        let _ = pool.extract_all(&paths);
        let peak = lock(&pool.analyzer.peak).clone();
        assert_eq!(peak.get("X:"), Some(&1));
        assert_eq!(peak.get("Y:"), Some(&1));
        assert_eq!(peak.get(r"\\nas\media"), Some(&1));
        assert!(
            pool.peak_concurrency() > 1,
            "the volumes are read in parallel, one read each"
        );
    }

    #[cfg(windows)]
    #[test]
    fn the_volume_key_names_the_drive_or_share() {
        assert_eq!(volume_key(Path::new(r"d:\Media\a.mkv")), "D:");
        assert_eq!(volume_key(Path::new(r"\\?\D:\Media\a.mkv")), "D:");
        assert_eq!(volume_key(Path::new(r"\\NAS\Media\a.mkv")), r"\\nas\media");
        assert_eq!(volume_key(Path::new("relative.mkv")), "");
    }

    #[test]
    fn the_time_limit_grows_with_the_file_size() {
        let options = PoolOptions::default();
        assert_eq!(options.timeout_for(0), DEFAULT_TIMEOUT);
        assert_eq!(options.timeout_for(700_000_000), Duration::from_secs(123));
        // The 96 GB remux of a real library: 120 s + 97 x 3 s.
        assert_eq!(
            options.timeout_for(96_517_266_560),
            Duration::from_secs(411)
        );
        assert_eq!(options.timeout_for(u64::MAX), MAX_TIMEOUT);
    }

    #[test]
    fn a_large_file_is_given_its_larger_time_limit() {
        // A read of 600 ms against a base limit of 150 ms: an empty file times out, the same
        // read of a 2 GB file, which adds two seconds, does not.
        let dir = tempfile::tempdir().unwrap();
        let small = dir.path().join("hang-small.mkv");
        std::fs::write(&small, b"").unwrap();
        let large = dir.path().join("hang-large.mkv");
        std::fs::File::create(&large)
            .unwrap()
            .set_len(2_000_000_000)
            .unwrap();
        let pool = MediaInfoPool::new(
            Fake {
                delay: Duration::ZERO,
            },
            PoolOptions {
                limit: 2,
                per_volume: 2,
                timeout: Duration::from_millis(150),
                per_gb: Duration::from_millis(1000),
                max_timeout: Duration::from_secs(60),
            },
        );
        let timed_out = pool.extract(&small);
        assert!(
            matches!(timed_out, Err(MediaInfoError::Timeout { .. })),
            "{timed_out:?}"
        );
        assert!(pool.extract(&large).is_ok());
    }

    #[test]
    fn a_cancelled_read_stops_waiting_for_its_drive_and_for_its_result() {
        let pool = pool(1, 0, 10_000);
        let stop = AtomicBool::new(true);
        let started = Instant::now();
        let result = pool.extract_unless(Path::new("hang.mkv"), &|| stop.load(Ordering::SeqCst));
        assert!(
            matches!(result, Err(MediaInfoError::Cancelled { .. })),
            "{result:?}"
        );
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn stopping_early_starts_no_new_files() {
        let pool = pool(1, 20, 10_000);
        let paths = files("movie", 20);
        let seen = AtomicUsize::new(0);
        let stop = AtomicBool::new(false);
        let started = pool.extract_each(
            &paths,
            || !stop.load(Ordering::SeqCst),
            |_, _, _| {
                if seen.fetch_add(1, Ordering::SeqCst) + 1 == 3 {
                    stop.store(true, Ordering::SeqCst);
                }
            },
        );
        assert_eq!(started, 3);
        assert_eq!(seen.load(Ordering::SeqCst), 3);
    }
}
