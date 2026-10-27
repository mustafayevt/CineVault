//! Starting work from a thread that is not inside a tokio runtime.
//!
//! The desktop app runs its synchronous commands and event callbacks on the window thread, which
//! has no runtime. Submitting a job, spawning a lane task or shrinking a lane from there must
//! still work instead of panicking (a panic there aborts the whole app).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

use cv_jobs::{
    ItemControl, ItemError, ItemWork, JobManager, JobSpec, JobState, LaneConfig, LaneId, Lanes,
    ManagerConfig,
};
use tokio::runtime::{Builder, Runtime};

const LIMIT: Duration = Duration::from_secs(10);

fn waiter() -> Runtime {
    Builder::new_current_thread().enable_all().build().unwrap()
}

/// Runs `f` on a plain thread (no runtime) and returns what it gave back.
fn off_runtime<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    thread::spawn(move || {
        assert!(tokio::runtime::Handle::try_current().is_err());
        f()
    })
    .join()
    .expect("starting work off a runtime must not panic")
}

#[test]
fn a_lane_task_spawned_off_a_runtime_runs() {
    let lanes = Lanes::new(LaneConfig::default());
    let task = off_runtime(move || lanes.spawn(&LaneId::provider("imdb"), async { 7 }));
    let value = waiter().block_on(async { tokio::time::timeout(LIMIT, task).await });
    assert_eq!(value.expect("the task never ran").unwrap(), 7);
}

#[test]
fn lanes_made_inside_a_runtime_keep_using_it_from_other_threads() {
    let runtime = Builder::new_multi_thread()
        .worker_threads(1)
        .thread_name("captured-runtime")
        .enable_all()
        .build()
        .unwrap();
    let lanes = {
        let _inside = runtime.enter();
        Lanes::new(LaneConfig::default())
    };
    let task = off_runtime(move || {
        lanes.spawn(&LaneId::Disk, async {
            thread::current().name().map(str::to_owned)
        })
    });
    let name = runtime.block_on(async { tokio::time::timeout(LIMIT, task).await });
    assert_eq!(
        name.expect("the task never ran").unwrap().as_deref(),
        Some("captured-runtime")
    );
}

#[test]
fn shrinking_a_busy_lane_off_a_runtime_does_not_panic() {
    let lanes = Lanes::new(LaneConfig {
        disk: 2,
        ..LaneConfig::default()
    });
    let runtime = waiter();
    let (release, released) = tokio::sync::watch::channel(false);
    // Both workers are busy, so the shrink has to wait for one of them to come back.
    let busy: Vec<_> = (0..2)
        .map(|_| {
            let _inside = runtime.enter();
            let mut released = released.clone();
            lanes.spawn(&LaneId::Disk, async move {
                let _ = released.wait_for(|done| *done).await;
            })
        })
        .collect();
    runtime.block_on(async {
        let running = || {
            lanes
                .snapshot()
                .iter()
                .find(|lane| lane.lane == LaneId::Disk)
                .map_or(0, |lane| lane.running)
        };
        while running() < 2 {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    });
    let shrunk = lanes.clone();
    off_runtime(move || shrunk.set_width(&LaneId::Disk, 1));
    release.send(true).unwrap();
    for task in busy {
        runtime.block_on(task).unwrap();
    }
}

struct Count(Arc<AtomicUsize>);

impl ItemWork for Count {
    type Item = u32;

    async fn process(&self, _item: &u32, _control: &ItemControl) -> Result<(), ItemError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[test]
fn a_job_submitted_off_a_runtime_runs_to_the_end() {
    let manager = JobManager::new(Lanes::new(LaneConfig::default()), ManagerConfig::default());
    let done = Arc::new(AtomicUsize::new(0));
    let work = Count(Arc::clone(&done));
    let job = off_runtime(move || {
        manager.submit(
            JobSpec::new("off the runtime", LaneId::Cpu),
            work,
            (0..5).collect(),
        )
    });
    let report = waiter().block_on(async { tokio::time::timeout(LIMIT, job.wait()).await });
    let report = report.expect("the job never finished");
    assert_eq!(report.snapshot.state, JobState::Completed);
    assert_eq!(done.load(Ordering::SeqCst), 5);
}
