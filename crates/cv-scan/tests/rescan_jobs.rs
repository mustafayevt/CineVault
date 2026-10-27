//! Rescans as persistent jobs on the disk lane.

mod support;

use std::sync::Arc;

use cv_db::repo::data_sources::{insert_source, remove_source};
use cv_domain::{ItemKind, SourceKind};
use cv_jobs::{JobManager, JobState, JobStore, LaneConfig, Lanes, ManagerConfig};
use cv_scan::rescan::job::{
    RESCAN_JOB_KIND, RescanTarget, rescan_targets, resume_rescans, submit_rescan,
};
use support::Lib;

fn manager(lib: &Lib) -> JobManager {
    JobManager::with_store(
        Lanes::new(LaneConfig::default()),
        ManagerConfig::default(),
        JobStore::new(Arc::clone(&lib.db)),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_rescan_job_runs_every_source_and_stores_its_progress() {
    let lib = Lib::movies();
    lib.put("Alien (1979)/Alien (1979).mkv", "a");
    lib.put("Heat (1995)/Heat (1995).mkv", "h");
    let tv_root = lib.dir.path().join("shows");
    support::put(
        &tv_root.join("Andor (2022)/Season 01/Andor.S01E01.mkv"),
        "e",
    );
    let path = tv_root.to_string_lossy().into_owned();
    lib.db
        .write(move |tx| insert_source(tx, SourceKind::Tv, &path))
        .unwrap();

    let targets = rescan_targets(&lib.db).unwrap();
    // Two sources, and no pass over detached titles: there are none.
    assert_eq!(targets.len(), 2);
    assert!(!targets.contains(&RescanTarget::Detached));
    // The current item Activity shows is the folder's own name, not "tv: <whole path>".
    assert_eq!(targets[1].to_string(), "shows");

    let manager = manager(&lib);
    let handle = submit_rescan(&manager, Arc::clone(&lib.db), lib.options(), targets, 2).unwrap();
    assert_eq!(handle.job.snapshot().name, "Scanning 2 folders");
    let report = handle.job.wait().await;

    assert_eq!(report.snapshot.state, JobState::Completed);
    // Named by its folders only, and its history step says what the scan found.
    assert_eq!(report.snapshot.name, "Scanning 2 folders");
    // Titles as the library counts them (Alien, Heat and Andor), not its episode too.
    assert_eq!(
        report.snapshot.result.as_deref(),
        Some("Scan of 2 folders \u{2192} 3 new")
    );
    let reports = handle.reports();
    assert_eq!(reports.len(), 2);
    assert_eq!(
        reports.iter().map(|r| r.new_items.len()).sum::<usize>(),
        2 + 2
    );
    assert_eq!(lib.items(ItemKind::Movie).len(), 2);
    assert_eq!(lib.items(ItemKind::Episode).len(), 1);

    // The job and its items are in the library database.
    let stored = JobStore::new(Arc::clone(&lib.db));
    let row = handle.job.stored_id().expect("persistent");
    let counts = stored.item_counts(row).unwrap();
    assert_eq!((counts.done, counts.pending, counts.failed), (2, 0, 0));

    // A second run finds nothing to do.
    let again = submit_rescan(
        &manager,
        Arc::clone(&lib.db),
        lib.options(),
        rescan_targets(&lib.db).unwrap(),
        2,
    )
    .unwrap();
    again.job.wait().await;
    assert!(again.reports().iter().all(|r| r.is_unchanged()));
}

/// A removed folder leaves titles behind: the next scan also looks at them, and is still named by
/// the folders it reads.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn titles_of_a_removed_folder_join_the_scan_without_counting_as_a_folder() {
    let lib = Lib::movies();
    lib.put("Alien (1979)/Alien (1979).mkv", "a");
    let other = lib.dir.path().join("more");
    support::put(&other.join("Heat (1995)/Heat (1995).mkv"), "h");
    let path = other.to_string_lossy().into_owned();
    let id = lib
        .db
        .write(move |tx| insert_source(tx, SourceKind::Movie, &path))
        .unwrap()
        .id;
    let manager = manager(&lib);
    let first = submit_rescan(
        &manager,
        Arc::clone(&lib.db),
        lib.options(),
        rescan_targets(&lib.db).unwrap(),
        2,
    )
    .unwrap();
    first.job.wait().await;
    assert_eq!(lib.items(ItemKind::Movie).len(), 2);

    lib.db.write(move |tx| remove_source(tx, id)).unwrap();
    let targets = rescan_targets(&lib.db).unwrap();
    assert_eq!(targets.len(), 2);
    assert_eq!(targets[1], RescanTarget::Detached);
    let again = submit_rescan(&manager, Arc::clone(&lib.db), lib.options(), targets, 2).unwrap();
    assert_eq!(again.job.snapshot().name, "Scanning 1 folder");
    assert_eq!(again.job.wait().await.snapshot.state, JobState::Completed);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unfinished_rescan_resumes_with_its_remaining_sources() {
    let lib = Lib::movies();
    lib.put("Alien (1979)/Alien (1979).mkv", "a");

    // Nothing to resume on a fresh library.
    let manager_a = manager(&lib);
    assert!(
        resume_rescans(&manager_a, &lib.db, &lib.options())
            .unwrap()
            .is_empty()
    );

    // Store a job whose items never ran, as a quit would leave it.
    let store = JobStore::new(Arc::clone(&lib.db));
    let targets = rescan_targets(&lib.db).unwrap();
    let row = {
        use cv_db::repo::jobs::{NewJob, NewJobItem, insert_job};
        let items: Vec<NewJobItem> = targets
            .iter()
            .map(|t| NewJobItem {
                label: t.to_string(),
                payload: serde_json::to_string(t).unwrap(),
            })
            .collect();
        let payload = serde_json::json!({ "concurrency": 1 }).to_string();
        lib.db
            .write(move |tx| {
                let job = NewJob {
                    kind: RESCAN_JOB_KIND,
                    name: "Scanning 1 folder",
                    lane: "disk",
                    priority: 0,
                    payload: &payload,
                };
                insert_job(tx, &job, &items)
            })
            .unwrap()
    };
    assert!(store.job(row).unwrap().is_some());

    let manager_b = manager(&lib);
    let resumed = resume_rescans(&manager_b, &lib.db, &lib.options()).unwrap();
    assert_eq!(resumed.len(), 1);
    let report = resumed[0].job.wait().await;
    assert_eq!(report.snapshot.state, JobState::Completed);
    assert_eq!(lib.items(ItemKind::Movie).len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_rescans_of_one_source_never_add_a_title_twice() {
    let lib = Lib::movies();
    for n in 0..30 {
        lib.put(&format!("Film {n} (20{n:02})/Film {n} (20{n:02}).mkv"), "x");
    }
    let manager = manager(&lib);
    // A Rescan press and a watcher batch arriving together.
    let first = submit_rescan(
        &manager,
        Arc::clone(&lib.db),
        lib.options(),
        rescan_targets(&lib.db).unwrap(),
        2,
    )
    .unwrap();
    let second = submit_rescan(
        &manager,
        Arc::clone(&lib.db),
        lib.options(),
        rescan_targets(&lib.db).unwrap(),
        2,
    )
    .unwrap();
    first.job.wait().await;
    second.job.wait().await;
    assert_eq!(lib.items(ItemKind::Movie).len(), 30);
}
