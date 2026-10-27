//! Failure aggregation by reason and retry of failed items.
//!
//! The headline test, `three_forced_failures_are_grouped_and_retrying_runs_only_those_three`,
//! forces 3 rate-limit failures in a 20-item job and checks that the drawer line reads
//! "3 failed · rate limit" and that retrying runs exactly those 3 items and no others.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cv_db::Database;
use cv_jobs::{
    ItemControl, ItemError, ItemStatus, ItemWork, JobManager, JobSpec, JobState, JobStore,
    LaneConfig, LaneId, Lanes, ManagerConfig, Priority, StageError, group_failures,
};
use tokio::time::sleep;

/// Fails the items in `failing` with the given reason until `healed`, and counts how often each
/// item was processed.
struct Flaky {
    failing: BTreeMap<u32, &'static str>,
    healed: Arc<Mutex<bool>>,
    runs: Arc<Mutex<BTreeMap<u32, u32>>>,
}

impl Flaky {
    fn new(failing: &[(u32, &'static str)]) -> Self {
        Self {
            failing: failing.iter().copied().collect(),
            healed: Arc::default(),
            runs: Arc::default(),
        }
    }
}

impl ItemWork for Flaky {
    type Item = u32;

    async fn process(&self, item: &u32, _control: &ItemControl) -> Result<(), ItemError> {
        sleep(Duration::from_millis(5)).await;
        *self.runs.lock().unwrap().entry(*item).or_default() += 1;
        match self.failing.get(item) {
            Some(reason) if !*self.healed.lock().unwrap() => Err(StageError::new(*reason).into()),
            _ => Ok(()),
        }
    }
}

fn manager() -> JobManager {
    JobManager::new(Lanes::new(LaneConfig::default()), ManagerConfig::default())
}

fn spec() -> JobSpec {
    JobSpec::new("thumbnails", LaneId::provider("subs")).concurrency(4)
}

#[tokio::test(start_paused = true)]
async fn three_forced_failures_are_grouped_and_retrying_runs_only_those_three() {
    let work = Flaky::new(&[(3, "rate limit"), (8, "rate limit"), (15, "rate limit")]);
    let healed = Arc::clone(&work.healed);
    let runs = Arc::clone(&work.runs);

    let job = manager().submit(spec(), work, (0..20).collect());
    let report = job.wait().await;

    // The failures did not stop the job.
    assert_eq!(report.snapshot.state, JobState::Completed);
    assert_eq!((report.snapshot.done, report.snapshot.failed), (20, 3));

    // They are grouped by reason, and the drawer line reads as the spec says.
    let groups = report.failure_groups();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].reason, "rate limit");
    assert_eq!(groups[0].count(), 3);
    assert_eq!(groups[0].summary(), "3 failed · rate limit");
    assert_eq!(
        groups[0].items.iter().map(|i| i.index).collect::<Vec<_>>(),
        [3, 8, 15]
    );
    assert_eq!(job.failure_groups(), groups);
    assert!(runs.lock().unwrap().values().all(|&n| n == 1));

    // The provider recovers; retrying re-queues only those 3.
    *healed.lock().unwrap() = true;
    let retry = job.retry_failed().expect("there are failures to retry");
    let retried = retry.wait().await;

    let runs = runs.lock().unwrap();
    for item in 0..20u32 {
        let expected = if [3, 8, 15].contains(&item) { 2 } else { 1 };
        assert_eq!(runs[&item], expected, "item {item}");
    }
    assert_eq!(retried.snapshot.name, "thumbnails (retry)");
    assert_eq!(retried.snapshot.state, JobState::Completed);
    assert_eq!((retried.snapshot.total, retried.snapshot.done), (3, 3));
    assert_eq!(retried.snapshot.failed, 0);
    assert_eq!(retried.snapshot.lane, LaneId::provider("subs"));
    assert_eq!(
        retried
            .items
            .iter()
            .map(|r| r.label.as_str())
            .collect::<Vec<_>>(),
        ["3", "8", "15"]
    );
    assert!(retried.failure_groups().is_empty());
}

#[tokio::test(start_paused = true)]
async fn failures_are_grouped_by_reason_biggest_group_first() {
    let work = Flaky::new(&[
        (0, "no match"),
        (1, "rate limit"),
        (2, "rate limit"),
        (5, "timeout"),
        (6, "rate limit"),
        (7, "no match"),
    ]);
    let job = manager().submit(spec(), work, (0..10).collect());
    let report = job.wait().await;

    let groups = report.failure_groups();
    let lines: Vec<String> = groups.iter().map(|g| g.summary()).collect();
    assert_eq!(
        lines,
        [
            "3 failed · rate limit",
            "2 failed · no match",
            "1 failed · timeout"
        ]
    );
    assert_eq!(
        groups[0].items.iter().map(|i| i.index).collect::<Vec<_>>(),
        [1, 2, 6]
    );
    // Succeeded items never show up as failures.
    let everything: usize = groups.iter().map(|g| g.count()).sum();
    assert_eq!(everything, report.snapshot.failed);
    assert_eq!(group_failures(&report.items), groups);
}

#[tokio::test(start_paused = true)]
async fn retrying_a_job_with_no_failures_or_one_still_running_does_nothing() {
    let job = manager().submit(spec(), Flaky::new(&[]), (0..5).collect());
    let report = job.wait().await;
    assert!(report.failure_groups().is_empty());
    assert!(job.retry_failed().is_none());

    let running = manager().submit(spec(), Flaky::new(&[(0, "rate limit")]), (0..5).collect());
    assert!(running.retry_failed().is_none(), "job is not over yet");
    running.wait().await;
    assert!(running.retry_failed().is_some());
}

#[tokio::test(start_paused = true)]
async fn items_that_fail_again_are_reported_again_and_can_be_retried_once_more() {
    let work = Flaky::new(&[(2, "rate limit"), (4, "rate limit")]);
    let healed = Arc::clone(&work.healed);
    let runs = Arc::clone(&work.runs);
    let job = manager().submit(spec(), work, (0..6).collect());
    job.wait().await;

    // Still failing: the retry fails the same two items, and nothing else is touched.
    let first = job.retry_failed().unwrap();
    let first_report = first.wait().await;
    assert_eq!(first_report.snapshot.state, JobState::Failed); // every retried item failed
    assert_eq!(
        first_report.failure_groups()[0].summary(),
        "2 failed · rate limit"
    );

    *healed.lock().unwrap() = true;
    let second = first.retry_failed().unwrap();
    let second_report = second.wait().await;
    assert_eq!(second_report.snapshot.state, JobState::Completed);
    assert_eq!(second_report.snapshot.name, "thumbnails (retry) (retry)");

    let runs = runs.lock().unwrap();
    assert_eq!(runs[&2], 3);
    assert_eq!(runs[&4], 3);
    assert_eq!(runs[&0], 1);
}

#[tokio::test(start_paused = true)]
async fn a_retry_keeps_the_priority_of_the_job() {
    let work = Flaky::new(&[(1, "rate limit")]);
    let job = manager().submit(spec().priority(Priority::High), work, (0..4).collect());
    job.wait().await;
    let retry = job.retry_failed().unwrap();
    assert_eq!(retry.snapshot().priority, Priority::High);
    retry.wait().await;
}

// ---- stored jobs -----------------------------------------------------------------------------

fn stored_manager(db: &Arc<Database>) -> JobManager {
    JobManager::with_store(
        Lanes::new(LaneConfig::default()),
        ManagerConfig::default(),
        JobStore::new(Arc::clone(db)),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_persistent_job_retries_only_its_failed_items_as_a_persistent_job() {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(Database::open(dir.path().join("library.db")).unwrap());
    let store = JobStore::new(Arc::clone(&db));
    let manager = stored_manager(&db);

    let work = Flaky::new(&[(3, "rate limit"), (8, "rate limit"), (15, "rate limit")]);
    let healed = Arc::clone(&work.healed);
    let runs = Arc::clone(&work.runs);
    let job = manager
        .submit_persistent("subs", spec(), work, (0..20u32).collect())
        .unwrap();
    job.wait().await;
    let row = job.stored_id().unwrap();

    // The stored job reports the same groups, so the history can show them.
    let groups = store.failure_groups(row).unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].summary(), "3 failed · rate limit");

    *healed.lock().unwrap() = true;
    let retry = job.retry_failed().unwrap();
    let retry_row = retry.stored_id().expect("the retry is persistent too");
    assert_ne!(retry_row, row);
    let report = retry.wait().await;
    assert_eq!(report.snapshot.state, JobState::Completed);

    let counts = store.item_counts(retry_row).unwrap();
    assert_eq!((counts.done, counts.failed, counts.pending), (3, 0, 0));
    assert_eq!(store.job(retry_row).unwrap().unwrap().state, "succeeded");
    assert_eq!(
        store.job(retry_row).unwrap().unwrap().name,
        "thumbnails (retry)"
    );
    let total_runs: u32 = runs.lock().unwrap().values().sum();
    assert_eq!(total_runs, 23);
    // The original job's record is unchanged.
    assert_eq!(store.item_counts(row).unwrap().failed, 3);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failures_from_before_a_restart_are_retried_from_the_database() {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(Database::open(dir.path().join("library.db")).unwrap());
    let store = JobStore::new(Arc::clone(&db));

    let row = {
        let work = Flaky::new(&[(1, "rate limit"), (4, "no match"), (6, "rate limit")]);
        let job = stored_manager(&db)
            .submit_persistent("subs", spec(), work, (0..10u32).collect())
            .unwrap();
        job.wait().await;
        job.stored_id().unwrap()
    };

    // A new manager, as after a restart: it knows the job only from the database.
    let manager = stored_manager(&db);
    let work = Flaky::new(&[]);
    let runs = Arc::clone(&work.runs);
    let retry = manager
        .retry_failed_persisted(row, |_| work)
        .unwrap()
        .expect("the job has failed items");
    let report = retry.wait().await;
    assert_eq!(report.snapshot.state, JobState::Completed);
    assert_eq!(report.snapshot.total, 3);
    let ran: Vec<u32> = runs.lock().unwrap().keys().copied().collect();
    assert_eq!(ran, [1, 4, 6]);

    // A job without failures, or an unknown one, yields nothing.
    let retry_row = store
        .history(10)
        .unwrap()
        .iter()
        .map(|j| j.id)
        .max()
        .unwrap();
    assert!(
        manager
            .retry_failed_persisted(retry_row, |_| Flaky::new(&[]))
            .unwrap()
            .is_none()
    );
    assert!(
        manager
            .retry_failed_persisted(9999, |_| Flaky::new(&[]))
            .unwrap()
            .is_none()
    );
    assert!(matches!(report.items[0].status, ItemStatus::Done));
}
