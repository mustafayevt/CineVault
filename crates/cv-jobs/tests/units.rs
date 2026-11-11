//! A job's progress in its own units, and the `prepare` step that counts them.

use std::sync::{Arc, Mutex};

use cv_jobs::{
    ItemControl, ItemError, ItemWork, JobManager, JobSpec, JobState, LaneConfig, LaneId, Lanes,
    ManagerConfig, UnitPhase, UnitProgress,
};

fn manager() -> JobManager {
    let lanes = Lanes::new(LaneConfig::default());
    JobManager::new(lanes, ManagerConfig::default())
}

fn units(phase: UnitPhase, done: u64, total: u64, current: &str) -> UnitProgress {
    UnitProgress {
        phase,
        done,
        total,
        current: Some(current.to_owned()),
        eta_secs: Some(12.0),
    }
}

/// Counts its items in `prepare`, then reports one unit per item as it works.
#[derive(Default)]
struct Counting {
    prepared: Mutex<Vec<Vec<u32>>>,
    done: Mutex<u64>,
    total: Mutex<u64>,
}

struct Work(Arc<Counting>);

impl ItemWork for Work {
    type Item = u32;

    async fn prepare(&self, items: &[&u32], control: &ItemControl) {
        let list: Vec<u32> = items.iter().map(|item| **item).collect();
        *self.0.total.lock().unwrap() = list.len() as u64;
        self.0.prepared.lock().unwrap().push(list);
        control.set_units(units(UnitPhase::Counting, 0, items.len() as u64, "root"));
    }

    async fn process(&self, item: &u32, control: &ItemControl) -> Result<(), ItemError> {
        let done = {
            let mut done = self.0.done.lock().unwrap();
            *done += 1;
            *done
        };
        let total = *self.0.total.lock().unwrap();
        control.set_units(units(UnitPhase::Working, done, total, &item.to_string()));
        Ok(())
    }
}

#[tokio::test]
async fn prepare_runs_once_with_the_items_before_the_first_item() {
    let state = Arc::new(Counting::default());
    let job = manager().submit(
        JobSpec::new("Counting", LaneId::Disk),
        Work(Arc::clone(&state)),
        vec![4, 5, 6],
    );
    let report = job.wait().await;
    assert_eq!(report.snapshot.state, JobState::Completed);
    assert_eq!(*state.prepared.lock().unwrap(), vec![vec![4, 5, 6]]);
}

#[tokio::test]
async fn the_units_are_in_the_snapshot_and_lose_their_estimate_when_the_job_ends() {
    let state = Arc::new(Counting::default());
    let job = manager().submit(
        JobSpec::new("Counting", LaneId::Disk),
        Work(Arc::clone(&state)),
        vec![1, 2],
    );
    let report = job.wait().await;
    let last = report.snapshot.units.expect("the job reported units");
    assert_eq!(last.phase, UnitPhase::Working);
    assert_eq!((last.done, last.total), (2, 2));
    // Over: nothing in hand and no time left to estimate.
    assert_eq!(last.current, None);
    assert_eq!(last.eta_secs, None);
}

#[tokio::test]
async fn a_job_that_reports_no_units_has_none() {
    struct Plain;
    impl ItemWork for Plain {
        type Item = u32;
        async fn process(&self, _item: &u32, _control: &ItemControl) -> Result<(), ItemError> {
            Ok(())
        }
    }
    let job = manager().submit(JobSpec::new("Plain", LaneId::Disk), Plain, vec![1]);
    assert_eq!(job.wait().await.snapshot.units, None);
}

#[tokio::test]
async fn a_job_cancelled_before_it_starts_is_not_prepared() {
    let state = Arc::new(Counting::default());
    let manager = manager();
    manager.pause_all();
    let job = manager.submit(
        JobSpec::new("Counting", LaneId::Disk),
        Work(Arc::clone(&state)),
        vec![1],
    );
    job.cancel();
    manager.resume_all();
    let report = job.wait().await;
    assert_eq!(report.snapshot.state, JobState::Cancelled);
    assert!(state.prepared.lock().unwrap().is_empty());
}
