//! Persistent jobs: jobs and items are stored in SQLite and resume after a restart.
//!
//! The headline test, `kill_and_restart_resumes_the_remaining_items`, runs a job in a child
//! process, kills that process mid-job, and resumes the job in this one: only the items that
//! had not finished run again. The other tests cover the details in-process, "restarting" by
//! dropping the runtime and database and opening them again.

use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use cv_db::Database;
use cv_jobs::{
    ItemControl, ItemError, ItemStatus, ItemWork, JobManager, JobSpec, JobState, JobStore,
    LaneConfig, LaneId, Lanes, ManagerConfig, Priority, StageError, StoreError,
};
use tokio::runtime::Runtime;

const KIND: &str = "test-work";
const CHILD_ENV: &str = "CV_JOBS_KILL_TEST_DIR";

/// Appends each processed item to a log file, so a test can see what ran in which process.
struct Logged {
    log: PathBuf,
    delay: Duration,
    fail: fn(u32) -> Option<&'static str>,
}

impl Logged {
    fn new(log: &Path, delay_ms: u64) -> Self {
        Self {
            log: log.to_path_buf(),
            delay: Duration::from_millis(delay_ms),
            fail: |_| None,
        }
    }
}

impl ItemWork for Logged {
    type Item = u32;

    async fn process(&self, item: &u32, _control: &ItemControl) -> Result<(), ItemError> {
        tokio::time::sleep(self.delay).await;
        append(&self.log, *item);
        match (self.fail)(*item) {
            Some(reason) => Err(StageError::new(reason).into()),
            None => Ok(()),
        }
    }
}

fn append(log: &Path, item: u32) {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
        .unwrap();
    writeln!(file, "{item}").unwrap();
}

fn read_log(log: &Path) -> Vec<u32> {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.trim().parse().ok())
        .collect()
}

fn manager(db: &Arc<Database>) -> JobManager {
    JobManager::with_store(
        Lanes::new(LaneConfig::default()),
        ManagerConfig::default(),
        JobStore::new(Arc::clone(db)),
    )
}

fn spec(name: &str) -> JobSpec {
    JobSpec::new(name, LaneId::Disk)
}

fn open(dir: &Path) -> Arc<Database> {
    Arc::new(Database::open(dir.join("library.db")).unwrap())
}

/// Polls until `check` holds; panics after a minute.
fn wait_for(what: &str, mut check: impl FnMut() -> bool) {
    let start = Instant::now();
    while !check() {
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "timed out waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn job_state(store: &JobStore, id: i64) -> String {
    store.job(id).unwrap().unwrap().state
}

// ---- the kill-and-restart test ---------------------------------------------------------------

/// Entry point of the child process of `kill_and_restart_resumes_the_remaining_items`. Does
/// nothing in a normal test run.
#[test]
fn child_job_runner() {
    let Ok(dir) = std::env::var(CHILD_ENV) else {
        return;
    };
    let dir = PathBuf::from(dir);
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let db = open(&dir);
        let manager = manager(&db);
        let job = manager
            .submit_persistent(
                KIND,
                spec("kill me").concurrency(1),
                Logged::new(&dir.join("first.log"), 100),
                (0..40).collect(),
            )
            .unwrap();
        job.wait().await;
    });
}

#[test]
fn kill_and_restart_resumes_the_remaining_items() {
    let dir = tempfile::tempdir().unwrap();
    let first_log = dir.path().join("first.log");

    // Run the job in another process and kill it part-way through.
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "child_job_runner",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD_ENV, dir.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    wait_for("the child to process 8 items", || {
        read_log(&first_log).len() >= 8
    });
    child.kill().unwrap();
    child.wait().unwrap();
    let processed_before_kill = read_log(&first_log);
    assert!(
        processed_before_kill.len() < 40,
        "the child finished before it was killed"
    );

    // What the database says survived the kill.
    let db = open(dir.path());
    let store = JobStore::new(Arc::clone(&db));
    let stored = store.history(10).unwrap();
    assert_eq!(stored.len(), 1);
    let job_row = stored[0].id;
    assert!(
        matches!(stored[0].state.as_str(), "queued" | "running"),
        "state after kill: {}",
        stored[0].state
    );
    let counts = store.item_counts(job_row).unwrap();
    assert_eq!(counts.done + counts.pending, 40);
    assert!(counts.pending > 0 && counts.done >= 7, "{counts:?}");
    // An item is committed after it ran, so at most the one in flight is logged but not saved.
    assert!(
        (processed_before_kill.len() as i64 - counts.done).abs() <= 1,
        "log {} vs committed {}",
        processed_before_kill.len(),
        counts.done
    );
    let remaining: Vec<u32> = store
        .items(job_row, &["pending"])
        .unwrap()
        .iter()
        .map(|i| i.idx as u32)
        .collect();
    assert_eq!(remaining.len() as i64, counts.pending);

    // Restart: resume with a fresh worker that logs somewhere else.
    let second_log = dir.path().join("second.log");
    let rt = Runtime::new().unwrap();
    let report = rt.block_on(async {
        let manager = manager(&db);
        let jobs = manager
            .resume_persisted(KIND, |_| Logged::new(&second_log, 1))
            .unwrap();
        assert_eq!(jobs.len(), 1, "exactly the cut-short job resumes");
        assert_eq!(jobs[0].stored_id(), Some(job_row));
        assert_eq!(jobs[0].snapshot().total, 40);
        assert_eq!(jobs[0].snapshot().done as i64, counts.done);
        jobs[0].wait().await
    });

    // Only the remaining items ran again, once each, in order.
    assert_eq!(read_log(&second_log), remaining);
    // Together the two runs covered every item.
    let mut all: Vec<u32> = read_log(&first_log);
    all.extend(read_log(&second_log));
    all.sort_unstable();
    all.dedup();
    assert_eq!(all, (0..40).collect::<Vec<_>>());

    assert_eq!(report.snapshot.state, JobState::Completed);
    assert_eq!((report.snapshot.done, report.snapshot.total), (40, 40));
    assert_eq!(
        report.items.len(),
        40,
        "finished-before items are in the report"
    );
    assert!(report.items.iter().all(|r| r.status == ItemStatus::Done));
    let counts = store.item_counts(job_row).unwrap();
    assert_eq!((counts.done, counts.pending), (40, 0));
    let row = store.job(job_row).unwrap().unwrap();
    assert_eq!(row.state, "succeeded");
    assert_eq!(row.attempts, 1);
    assert_eq!(row.done, 40);
    assert!(row.finished_ms.is_some());

    // A finished job is not resumed again.
    let again = rt.block_on(async {
        manager(&db)
            .resume_persisted(KIND, |_| Logged::new(&second_log, 1))
            .unwrap()
            .len()
    });
    assert_eq!(again, 0);
}

// ---- in-process details ----------------------------------------------------------------------

#[test]
fn a_persistent_job_is_stored_with_its_items_and_finishes_as_succeeded() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("run.log");
    let db = open(dir.path());
    let store = JobStore::new(Arc::clone(&db));

    let rt = Runtime::new().unwrap();
    let (row, report) = rt.block_on(async {
        let job = manager(&db)
            .submit_persistent(
                KIND,
                spec("scan").priority(Priority::High).concurrency(3),
                Logged::new(&log, 1),
                vec![10, 20, 30],
            )
            .unwrap();
        // Stored before it returns: the job and its items are already readable.
        let row = job.stored_id().unwrap();
        let queued = store.items(row, &["pending", "done"]).unwrap();
        assert_eq!(queued.len(), 3);
        (row, job.wait().await)
    });

    assert_eq!(report.snapshot.state, JobState::Completed);
    let stored = store.job(row).unwrap().unwrap();
    assert_eq!(stored.kind, KIND);
    assert_eq!(stored.name, "scan");
    assert_eq!(stored.lane, "disk");
    assert_eq!(stored.priority, 1);
    assert_eq!(stored.state, "succeeded");
    assert_eq!((stored.done, stored.total), (3, 3));
    assert!(stored.started_ms.is_some() && stored.finished_ms.is_some());
    let items = store.items(row, &["done"]).unwrap();
    assert_eq!(
        items
            .iter()
            .map(|i| (i.idx, i.label.as_str(), i.payload.as_str()))
            .collect::<Vec<_>>(),
        [(0, "10", "10"), (1, "20", "20"), (2, "30", "30")]
    );
}

#[test]
fn quitting_with_pause_all_resumes_the_rest_next_launch() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("first.log");
    let second = dir.path().join("second.log");

    // First launch: start a job, pause everything after a few items, then quit.
    let row = {
        let db = open(dir.path());
        let rt = Runtime::new().unwrap();
        let row = rt.block_on(async {
            let manager = manager(&db);
            let job = manager
                .submit_persistent(
                    KIND,
                    spec("artwork"),
                    Logged::new(&first, 20),
                    (0..30).collect(),
                )
                .unwrap();
            let mut rx = job.subscribe();
            rx.wait_for(|s| s.done >= 5).await.unwrap();
            manager.pause_all();
            // Items already running finish; wait for them and for the paused state to be saved.
            rx.wait_for(|s| s.state == JobState::Paused && s.current.is_empty())
                .await
                .unwrap();
            job.stored_id().unwrap()
        });
        let store = JobStore::new(Arc::clone(&db));
        wait_for("the paused state to be saved", || {
            job_state(&store, row) == "paused"
        });
        drop(rt);
        row
    };
    let done_first = read_log(&first);
    assert!(done_first.len() >= 5 && done_first.len() < 30);

    // Second launch: the paused job resumes from the remaining items.
    let db = open(dir.path());
    let rt = Runtime::new().unwrap();
    let report = rt.block_on(async {
        let jobs = manager(&db)
            .resume_persisted(KIND, |_| Logged::new(&second, 1))
            .unwrap();
        assert_eq!(jobs.len(), 1);
        jobs[0].wait().await
    });
    let mut ran_second = read_log(&second);
    assert_eq!(ran_second.len(), 30 - done_first.len());
    ran_second.retain(|i| done_first.contains(i));
    assert!(ran_second.is_empty(), "finished items ran again");
    assert_eq!(report.snapshot.state, JobState::Completed);
    assert_eq!(
        JobStore::new(db).job(row).unwrap().unwrap().state,
        "succeeded"
    );
}

#[test]
fn failed_items_stay_failed_and_are_not_run_again_on_resume() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("first.log");
    let second = dir.path().join("second.log");

    // Stop the "process" after the first six items by never letting the rest finish: the
    // worker for items >= 6 blocks, and the runtime is dropped.
    struct Stuck {
        inner: Logged,
    }
    impl ItemWork for Stuck {
        type Item = u32;
        async fn process(&self, item: &u32, control: &ItemControl) -> Result<(), ItemError> {
            if *item >= 6 {
                std::future::pending::<()>().await;
            }
            self.inner.process(item, control).await
        }
    }

    let row = {
        let db = open(dir.path());
        let rt = Runtime::new().unwrap();
        let row = rt.block_on(async {
            let mut inner = Logged::new(&first, 1);
            inner.fail = |i| (i == 1 || i == 4).then_some("no match");
            let job = manager(&db)
                .submit_persistent(KIND, spec("match"), Stuck { inner }, (0..10).collect())
                .unwrap();
            job.subscribe().wait_for(|s| s.done >= 6).await.unwrap();
            job.stored_id().unwrap()
        });
        drop(rt);
        row
    };

    let db = open(dir.path());
    let store = JobStore::new(Arc::clone(&db));
    let counts = store.item_counts(row).unwrap();
    assert_eq!((counts.done, counts.failed, counts.pending), (4, 2, 4));
    let failed = store.items(row, &["failed"]).unwrap();
    assert_eq!(failed.iter().map(|i| i.idx).collect::<Vec<_>>(), [1, 4]);
    assert!(failed.iter().all(|i| i.reason == "no match"));

    let rt = Runtime::new().unwrap();
    let report = rt.block_on(async {
        let jobs = manager(&db)
            .resume_persisted(KIND, |_| Logged::new(&second, 1))
            .unwrap();
        let snapshot = jobs[0].snapshot();
        assert_eq!((snapshot.done, snapshot.failed, snapshot.total), (6, 2, 10));
        jobs[0].wait().await
    });
    assert_eq!(read_log(&second), [6, 7, 8, 9]);
    assert_eq!(report.snapshot.state, JobState::Completed);
    assert_eq!((report.snapshot.done, report.snapshot.failed), (10, 2));
    assert_eq!(report.items.len(), 10);
    let reasons: Vec<_> = report
        .items
        .iter()
        .filter_map(|r| match &r.status {
            ItemStatus::Failed { reason } => Some((r.index, reason.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(reasons, [(1, "no match"), (4, "no match")]);
    assert_eq!(store.job(row).unwrap().unwrap().state, "succeeded");
}

#[test]
fn a_cancelled_job_is_not_resumed() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("run.log");
    let db = open(dir.path());
    let rt = Runtime::new().unwrap();
    let row = rt.block_on(async {
        let job = manager(&db)
            .submit_persistent(
                KIND,
                spec("thumbnails"),
                Logged::new(&log, 50),
                (0..20).collect(),
            )
            .unwrap();
        job.subscribe().wait_for(|s| s.done >= 2).await.unwrap();
        job.cancel();
        let report = job.wait().await;
        assert_eq!(report.snapshot.state, JobState::Cancelled);
        job.stored_id().unwrap()
    });
    let store = JobStore::new(Arc::clone(&db));
    assert_eq!(store.job(row).unwrap().unwrap().state, "canceled");

    let resumed = rt.block_on(async {
        manager(&db)
            .resume_persisted(KIND, |_| Logged::new(&log, 1))
            .unwrap()
            .len()
    });
    assert_eq!(resumed, 0);
}

#[test]
fn a_job_whose_items_all_finished_before_the_kill_completes_on_resume() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("run.log");
    let db = open(dir.path());
    let store = JobStore::new(Arc::clone(&db));
    let rt = Runtime::new().unwrap();
    let row = rt.block_on(async {
        let job = manager(&db)
            .submit_persistent(KIND, spec("nfo"), Logged::new(&log, 1), (0..5).collect())
            .unwrap();
        job.wait().await;
        job.stored_id().unwrap()
    });
    // Simulate a kill after the last item was saved but before the job row was.
    db.write(move |tx| {
        tx.execute(
            "UPDATE job SET state = 'running', finished_ms = NULL WHERE id = ?1",
            [row],
        )?;
        Ok(())
    })
    .unwrap();

    let report = rt.block_on(async {
        let jobs = manager(&db)
            .resume_persisted(KIND, |_| Logged::new(&log, 1))
            .unwrap();
        assert_eq!(jobs.len(), 1);
        jobs[0].wait().await
    });
    assert_eq!(report.snapshot.state, JobState::Completed);
    assert_eq!(read_log(&log).len(), 5, "nothing ran twice");
    assert_eq!(store.job(row).unwrap().unwrap().state, "succeeded");
}

#[test]
fn resuming_twice_does_not_start_a_job_twice_and_other_kinds_are_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("run.log");
    let db = open(dir.path());
    let store = JobStore::new(Arc::clone(&db));

    // Two cut-short jobs of different kinds, stored without running them.
    {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let m = manager(&db);
            m.pause_all();
            m.submit_persistent("alpha", spec("a"), Logged::new(&log, 1), vec![1, 2])
                .unwrap();
            m.submit_persistent("beta", spec("b"), Logged::new(&log, 1), vec![3, 4])
                .unwrap();
        });
        wait_for("both jobs to be saved as paused", || {
            store
                .history(10)
                .unwrap()
                .iter()
                .all(|j| j.state == "paused")
        });
    }

    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let m = manager(&db);
        let first = m
            .resume_persisted("alpha", |_| Logged::new(&log, 1))
            .unwrap();
        let second = m
            .resume_persisted("alpha", |_| Logged::new(&log, 1))
            .unwrap();
        assert_eq!((first.len(), second.len()), (1, 0));
        first[0].wait().await;
    });
    assert_eq!(read_log(&log), [1, 2], "beta did not run");
    let states: Vec<_> = store
        .history(10)
        .unwrap()
        .into_iter()
        .map(|j| (j.kind, j.state))
        .collect();
    assert!(states.contains(&("alpha".into(), "succeeded".into())));
    assert!(states.contains(&("beta".into(), "paused".into())));
}

#[test]
fn a_job_with_unreadable_items_is_marked_failed_and_the_others_still_resume() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("run.log");
    let db = open(dir.path());
    let store = JobStore::new(Arc::clone(&db));
    {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let m = manager(&db);
            m.pause_all();
            m.submit_persistent(KIND, spec("good"), Logged::new(&log, 1), vec![7, 8])
                .unwrap();
            m.submit_persistent(KIND, spec("bad"), Logged::new(&log, 1), vec![1, 2])
                .unwrap();
        });
        wait_for("both jobs to be saved", || {
            store
                .history(10)
                .unwrap()
                .iter()
                .all(|j| j.state == "paused")
        });
    }
    // Damage the second job: its items no longer decode as numbers.
    db.write(|tx| {
        tx.execute(
            "UPDATE job_item SET payload = '\"not a number\"' WHERE job_id = (SELECT id FROM job WHERE name = 'bad')",
            [],
        )?;
        Ok(())
    })
    .unwrap();

    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let jobs = manager(&db)
            .resume_persisted(KIND, |_| Logged::new(&log, 1))
            .unwrap();
        assert_eq!(jobs.len(), 1);
        jobs[0].wait().await;
    });
    assert_eq!(read_log(&log), [7, 8]);
    let history = store.history(10).unwrap();
    let bad = history.iter().find(|j| j.name == "bad").unwrap();
    assert_eq!(bad.state, "failed");
    assert!(!bad.error.is_empty());
}

#[test]
fn a_manager_without_a_store_refuses_persistent_jobs() {
    let manager = JobManager::new(Lanes::new(LaneConfig::default()), ManagerConfig::default());
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let dir = tempfile::tempdir().unwrap();
        let err = manager
            .submit_persistent(
                KIND,
                spec("x"),
                Logged::new(&dir.path().join("l"), 1),
                vec![1],
            )
            .err()
            .expect("refused");
        assert!(matches!(err, StoreError::NoStore));
        assert!(matches!(
            manager.resume_persisted(KIND, |_| Logged::new(&dir.path().join("l"), 1)),
            Err(StoreError::NoStore)
        ));
    });
}

#[test]
fn jobs_that_are_not_persistent_leave_no_rows() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("run.log");
    let db = open(dir.path());
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let job = manager(&db).submit(spec("temp"), Logged::new(&log, 1), vec![1, 2, 3]);
        assert_eq!(job.stored_id(), None);
        job.wait().await;
    });
    assert!(JobStore::new(db).history(10).unwrap().is_empty());
}
