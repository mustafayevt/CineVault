//! Resource lanes: a width-limited, FIFO worker pool per resource.

use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use tokio::runtime::Handle;
use tokio::sync::Semaphore;
use tokio::task::JoinHandle;

use crate::runtime;

/// Identifies one lane.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum LaneId {
    /// Scanning and file moves.
    Disk,
    /// Media info, FFmpeg and image processing.
    Cpu,
    /// Network work for one provider, named by its provider id (for example `tmdb`).
    Provider(String),
}

impl LaneId {
    /// Lane for the provider with the given id.
    pub fn provider(id: impl Into<String>) -> Self {
        Self::Provider(id.into())
    }
}

impl fmt::Display for LaneId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Disk => f.write_str("disk"),
            Self::Cpu => f.write_str("cpu"),
            Self::Provider(id) => write!(f, "provider:{id}"),
        }
    }
}

/// Worker counts per lane. A width of 0 is raised to 1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LaneConfig {
    /// Workers in the disk lane.
    pub disk: usize,
    /// Workers in the CPU lane.
    pub cpu: usize,
    /// Workers for a provider lane with no explicit width.
    pub provider_default: usize,
    /// Explicit widths by provider id.
    pub providers: HashMap<String, usize>,
}

impl Default for LaneConfig {
    fn default() -> Self {
        let cores = std::thread::available_parallelism().map_or(4, usize::from);
        Self {
            disk: 2,
            cpu: cores.saturating_sub(1).max(1),
            provider_default: 4,
            providers: HashMap::new(),
        }
    }
}

impl LaneConfig {
    /// Configured width for a lane (at least 1).
    pub fn width_of(&self, lane: &LaneId) -> usize {
        let width = match lane {
            LaneId::Disk => self.disk,
            LaneId::Cpu => self.cpu,
            LaneId::Provider(id) => self
                .providers
                .get(id)
                .copied()
                .unwrap_or(self.provider_default),
        };
        width.max(1)
    }
}

/// Point-in-time state of a lane, for the Activity drawer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaneSnapshot {
    /// The lane.
    pub lane: LaneId,
    /// Configured worker count.
    pub width: usize,
    /// Tasks currently holding a worker.
    pub running: usize,
    /// Tasks waiting for a worker.
    pub queued: usize,
}

#[derive(Debug)]
struct Lane {
    semaphore: Arc<Semaphore>,
    width: Mutex<usize>,
    running: AtomicUsize,
    queued: AtomicUsize,
}

impl Lane {
    fn new(width: usize) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(width)),
            width: Mutex::new(width),
            running: AtomicUsize::new(0),
            queued: AtomicUsize::new(0),
        }
    }

    /// Waits for a worker. The returned guard holds the worker and the running count; the
    /// queued count covers the wait and is released even if the waiting future is dropped.
    async fn acquire(self: Arc<Self>) -> Worker {
        let queued = Counted::new(Arc::clone(&self), |l| &l.queued);
        let permit = Arc::clone(&self.semaphore).acquire_owned().await.ok();
        drop(queued);
        Worker {
            _running: Counted::new(Arc::clone(&self), |l| &l.running),
            _permit: permit,
        }
    }
}

/// Holds one of a lane's counters incremented for as long as it lives.
struct Counted {
    lane: Arc<Lane>,
    pick: fn(&Lane) -> &AtomicUsize,
}

impl Counted {
    fn new(lane: Arc<Lane>, pick: fn(&Lane) -> &AtomicUsize) -> Self {
        pick(&lane).fetch_add(1, Ordering::SeqCst);
        Self { lane, pick }
    }
}

impl Drop for Counted {
    fn drop(&mut self) {
        (self.pick)(&self.lane).fetch_sub(1, Ordering::SeqCst);
    }
}

/// A claimed worker slot.
struct Worker {
    _running: Counted,
    _permit: Option<tokio::sync::OwnedSemaphorePermit>,
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Debug)]
struct Inner {
    config: Mutex<LaneConfig>,
    lanes: Mutex<HashMap<LaneId, Arc<Lane>>>,
    /// The runtime the lanes were made in, if any: tasks started from a thread outside every
    /// runtime run there.
    runtime: Option<Handle>,
}

/// The set of lanes. Cheap to clone; clones share the same lanes.
///
/// Submitting never blocks: [`Lanes::spawn`] returns at once and the task waits for a free
/// worker in its own lane only. Tasks in a lane start in submission order. Provider lanes are
/// created on first use.
#[derive(Debug, Clone)]
pub struct Lanes {
    inner: Arc<Inner>,
}

impl Default for Lanes {
    fn default() -> Self {
        Self::new(LaneConfig::default())
    }
}

impl Lanes {
    /// Creates the lanes with the given widths. The disk and CPU lanes exist immediately.
    pub fn new(config: LaneConfig) -> Self {
        let mut lanes = HashMap::new();
        for id in [LaneId::Disk, LaneId::Cpu] {
            let width = config.width_of(&id);
            lanes.insert(id, Arc::new(Lane::new(width)));
        }
        Self {
            inner: Arc::new(Inner {
                config: Mutex::new(config),
                lanes: Mutex::new(lanes),
                runtime: Handle::try_current().ok(),
            }),
        }
    }

    fn lane(&self, id: &LaneId) -> Arc<Lane> {
        let mut lanes = locked(&self.inner.lanes);
        if let Some(lane) = lanes.get(id) {
            return Arc::clone(lane);
        }
        let width = locked(&self.inner.config).width_of(id);
        let lane = Arc::new(Lane::new(width));
        lanes.insert(id.clone(), Arc::clone(&lane));
        lane
    }

    /// Runs `work` on a worker of `lane` and returns its handle. Aborting the handle while the
    /// task is queued removes it from the queue.
    ///
    /// Works from any thread: outside a tokio runtime the task runs on the runtime the lanes were
    /// made in, else on the app's fallback runtime (see [`crate::set_fallback_runtime`]).
    pub fn spawn<F, T>(&self, lane: &LaneId, work: F) -> JoinHandle<T>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let lane = self.lane(lane);
        self.spawn_task(async move {
            let _worker = lane.acquire().await;
            work.await
        })
    }

    /// Starts `future` as a task without a lane, on the same runtime [`Lanes::spawn`] would use.
    pub(crate) fn spawn_task<F>(&self, future: F) -> JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        runtime::spawn_on(self.inner.runtime.as_ref(), future)
    }

    /// Runs `work` on a worker of `lane` and waits for it. If the returned future is dropped,
    /// the work is dropped with it.
    pub async fn run<F, T>(&self, lane: &LaneId, work: F) -> T
    where
        F: Future<Output = T>,
    {
        let _worker = self.lane(lane).acquire().await;
        work.await
    }

    /// Changes a lane's worker count (at least 1). Tasks already running are never interrupted:
    /// when shrinking, surplus workers retire as their tasks finish.
    pub fn set_width(&self, lane: &LaneId, width: usize) {
        let width = width.max(1);
        {
            let mut config = locked(&self.inner.config);
            match lane {
                LaneId::Disk => config.disk = width,
                LaneId::Cpu => config.cpu = width,
                LaneId::Provider(id) => {
                    config.providers.insert(id.clone(), width);
                }
            }
        }
        let lane = self.lane(lane);
        let mut current = locked(&lane.width);
        if width > *current {
            lane.semaphore.add_permits(width - *current);
        } else if width < *current {
            let surplus = *current - width;
            let forgotten = lane.semaphore.forget_permits(surplus);
            let remaining = surplus - forgotten;
            if remaining > 0 {
                // The rest are held by running tasks; retire them as they come back.
                let semaphore = Arc::clone(&lane.semaphore);
                let count = u32::try_from(remaining).unwrap_or(u32::MAX);
                self.spawn_task(async move {
                    if let Ok(permits) = semaphore.acquire_many_owned(count).await {
                        permits.forget();
                    }
                });
            }
        }
        *current = width;
    }

    /// Current configuration, including widths changed through [`Lanes::set_width`].
    pub fn config(&self) -> LaneConfig {
        locked(&self.inner.config).clone()
    }

    /// State of every lane that exists, ordered by lane id.
    pub fn snapshot(&self) -> Vec<LaneSnapshot> {
        let lanes = locked(&self.inner.lanes);
        let mut out: Vec<LaneSnapshot> = lanes
            .iter()
            .map(|(id, lane)| LaneSnapshot {
                lane: id.clone(),
                width: *locked(&lane.width),
                running: lane.running.load(Ordering::SeqCst),
                queued: lane.queued.load(Ordering::SeqCst),
            })
            .collect();
        out.sort_by(|a, b| a.lane.cmp(&b.lane));
        out
    }
}
