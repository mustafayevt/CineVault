//! Lane behaviour: isolation between lanes, configurable widths, FIFO order and accounting.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use cv_jobs::{LaneConfig, LaneId, Lanes};
use tokio::sync::{Notify, oneshot};
use tokio::time::{sleep, timeout};

const LIMIT: Duration = Duration::from_secs(5);

fn config(disk: usize, cpu: usize, provider: usize) -> LaneConfig {
    LaneConfig {
        disk,
        cpu,
        provider_default: provider,
        ..LaneConfig::default()
    }
}

async fn until(mut condition: impl FnMut() -> bool) {
    timeout(LIMIT, async {
        while !condition() {
            sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .expect("condition not reached in time");
}

#[tokio::test]
async fn blocked_provider_lane_does_not_delay_disk_jobs() {
    let lanes = Lanes::new(config(2, 2, 1));
    let tmdb = LaneId::provider("tmdb");

    // Occupy the only TMDB worker with a job that never finishes until released, and queue more
    // TMDB jobs behind it.
    let (release, released) = oneshot::channel::<()>();
    let blocked = lanes.spawn(&tmdb, async move {
        let _ = released.await;
    });
    let queued_behind: Vec<_> = (0..5).map(|_| lanes.spawn(&tmdb, async {})).collect();
    until(|| {
        lanes
            .snapshot()
            .iter()
            .any(|s| s.lane == tmdb && s.running == 1 && s.queued == 5)
    })
    .await;

    // Disk and CPU jobs still run to completion while the provider lane is stuck.
    let disk_done = Arc::new(AtomicUsize::new(0));
    let disk_jobs: Vec<_> = (0..20)
        .map(|_| {
            let done = Arc::clone(&disk_done);
            lanes.spawn(&LaneId::Disk, async move {
                done.fetch_add(1, Ordering::SeqCst);
            })
        })
        .collect();
    let cpu_job = lanes.spawn(&LaneId::Cpu, async { 7 });
    // Another provider is unaffected as well.
    let other = lanes.spawn(&LaneId::provider("fanart"), async { "ok" });

    timeout(LIMIT, async {
        for job in disk_jobs {
            job.await.expect("disk job");
        }
        assert_eq!(cpu_job.await.expect("cpu job"), 7);
        assert_eq!(other.await.expect("other provider job"), "ok");
    })
    .await
    .expect("disk, CPU and other-provider jobs were delayed by the blocked provider lane");
    assert_eq!(disk_done.load(Ordering::SeqCst), 20);

    // The blocked lane is still blocked: nothing queued behind it has run.
    assert!(queued_behind.iter().all(|h| !h.is_finished()));
    assert!(!blocked.is_finished());

    // Releasing it lets the queue drain.
    release.send(()).expect("release");
    timeout(LIMIT, async {
        blocked.await.expect("blocked job");
        for job in queued_behind {
            job.await.expect("queued provider job");
        }
    })
    .await
    .expect("provider lane did not drain after release");
}

#[tokio::test]
async fn lane_width_bounds_concurrency() {
    let lanes = Lanes::new(config(3, 1, 1));
    let current = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));

    let jobs: Vec<_> = (0..12)
        .map(|_| {
            let (current, peak) = (Arc::clone(&current), Arc::clone(&peak));
            lanes.spawn(&LaneId::Disk, async move {
                let now = current.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(now, Ordering::SeqCst);
                sleep(Duration::from_millis(15)).await;
                current.fetch_sub(1, Ordering::SeqCst);
            })
        })
        .collect();
    for job in jobs {
        timeout(LIMIT, job).await.expect("in time").expect("job");
    }
    assert_eq!(peak.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn provider_widths_are_independent_and_configurable() {
    let mut cfg = config(1, 1, 1);
    cfg.providers.insert("tmdb".into(), 4);
    let lanes = Lanes::new(cfg);

    let width = |id: &LaneId| {
        lanes
            .snapshot()
            .into_iter()
            .find(|s| &s.lane == id)
            .map(|s| s.width)
    };
    let tmdb = LaneId::provider("tmdb");
    let tvdb = LaneId::provider("tvdb");

    // Provider lanes are created on first use, with the explicit or default width.
    assert_eq!(width(&tmdb), None);
    lanes.run(&tmdb, async {}).await;
    lanes.run(&tvdb, async {}).await;
    assert_eq!(width(&tmdb), Some(4));
    assert_eq!(width(&tvdb), Some(1));
    assert_eq!(width(&LaneId::Disk), Some(1));
}

#[tokio::test]
async fn jobs_in_one_lane_start_in_submission_order() {
    let lanes = Lanes::new(config(1, 1, 1));
    let order = Arc::new(std::sync::Mutex::new(Vec::new()));
    let jobs: Vec<_> = (0..10)
        .map(|i| {
            let order = Arc::clone(&order);
            lanes.spawn(&LaneId::Disk, async move {
                order.lock().expect("lock").push(i);
                tokio::task::yield_now().await;
            })
        })
        .collect();
    for job in jobs {
        timeout(LIMIT, job).await.expect("in time").expect("job");
    }
    assert_eq!(*order.lock().expect("lock"), (0..10).collect::<Vec<_>>());
}

#[tokio::test]
async fn width_can_grow_and_shrink_at_runtime() {
    let lanes = Lanes::new(config(1, 1, 1));
    let gate = Arc::new(Notify::new());
    let started = Arc::new(AtomicUsize::new(0));
    let spawn_gated = |lanes: &Lanes| {
        let (gate, started) = (Arc::clone(&gate), Arc::clone(&started));
        lanes.spawn(&LaneId::Cpu, async move {
            started.fetch_add(1, Ordering::SeqCst);
            gate.notified().await;
        })
    };

    let first: Vec<_> = (0..4).map(|_| spawn_gated(&lanes)).collect();
    until(|| started.load(Ordering::SeqCst) == 1).await;
    sleep(Duration::from_millis(30)).await;
    assert_eq!(started.load(Ordering::SeqCst), 1);

    // Growing admits waiting jobs immediately.
    lanes.set_width(&LaneId::Cpu, 3);
    until(|| started.load(Ordering::SeqCst) == 3).await;
    assert_eq!(lanes.config().cpu, 3);

    // Shrinking never interrupts running jobs; surplus workers retire as they finish.
    lanes.set_width(&LaneId::Cpu, 1);
    for _ in 0..3 {
        gate.notify_one();
        sleep(Duration::from_millis(10)).await;
    }
    until(|| started.load(Ordering::SeqCst) == 4).await;
    gate.notify_one();
    for job in first {
        timeout(LIMIT, job).await.expect("in time").expect("job");
    }

    // A zero width is raised to one.
    lanes.set_width(&LaneId::Cpu, 0);
    assert_eq!(lanes.config().cpu, 1);
    timeout(LIMIT, lanes.run(&LaneId::Cpu, async {}))
        .await
        .expect("lane still usable");
}

#[tokio::test]
async fn aborting_a_queued_job_frees_its_queue_slot() {
    let lanes = Lanes::new(config(1, 1, 1));
    let gate = Arc::new(Notify::new());
    let g = Arc::clone(&gate);
    let running = lanes.spawn(&LaneId::Disk, async move { g.notified().await });
    let waiting = lanes.spawn(&LaneId::Disk, async {});
    until(|| {
        lanes
            .snapshot()
            .iter()
            .any(|s| s.lane == LaneId::Disk && s.running == 1 && s.queued == 1)
    })
    .await;

    waiting.abort();
    assert!(waiting.await.expect_err("aborted").is_cancelled());
    until(|| {
        lanes
            .snapshot()
            .iter()
            .any(|s| s.lane == LaneId::Disk && s.running == 1 && s.queued == 0)
    })
    .await;

    gate.notify_one();
    timeout(LIMIT, running)
        .await
        .expect("in time")
        .expect("job");
    until(|| {
        lanes
            .snapshot()
            .iter()
            .any(|s| s.lane == LaneId::Disk && s.running == 0 && s.queued == 0)
    })
    .await;
}

#[tokio::test]
async fn a_panicking_job_releases_its_worker() {
    let lanes = Lanes::new(config(1, 1, 1));
    let bad = lanes.spawn(&LaneId::Disk, async { panic!("boom") });
    assert!(bad.await.expect_err("panicked").is_panic());
    let value = timeout(LIMIT, lanes.run(&LaneId::Disk, async { 1 }))
        .await
        .expect("worker was released");
    assert_eq!(value, 1);
}
