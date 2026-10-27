//! Tests of the persistent job rows.

use rusqlite::Connection;

use super::jobs::*;

fn memory_db() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.pragma_update(None, "foreign_keys", true).unwrap();
    crate::schema::migrate(&mut conn).unwrap();
    conn
}

fn new_items(n: usize) -> Vec<NewJobItem> {
    (0..n)
        .map(|i| NewJobItem {
            label: format!("item {i}"),
            payload: i.to_string(),
        })
        .collect()
}

fn job(kind: &str) -> NewJob<'_> {
    NewJob {
        kind,
        name: "a job",
        lane: "disk",
        priority: 0,
        payload: r#"{"concurrency":2}"#,
    }
}

fn running(done: i64) -> JobUpdate<'static> {
    JobUpdate {
        state: "running",
        priority: 0,
        done,
        error: "",
    }
}

#[test]
fn a_job_is_stored_queued_with_all_its_items_pending() {
    let conn = memory_db();
    let id = insert_job(&conn, &job("scan"), &new_items(3)).unwrap();

    let stored = get_job(&conn, id).unwrap().unwrap();
    assert_eq!(
        (stored.kind.as_str(), stored.state.as_str()),
        ("scan", "queued")
    );
    assert_eq!((stored.done, stored.total), (0, 3));
    assert_eq!(stored.payload, r#"{"concurrency":2}"#);
    assert_eq!(stored.started_ms, None);
    let counts = job_item_counts(&conn, id).unwrap();
    assert_eq!(
        counts,
        JobItemCounts {
            pending: 3,
            ..JobItemCounts::default()
        }
    );
    let items = job_items_in_states(&conn, id, REMAINING_ITEM_STATES).unwrap();
    assert_eq!(items.iter().map(|i| i.idx).collect::<Vec<_>>(), [0, 1, 2]);
    assert_eq!(items[1].label, "item 1");
    assert_eq!(items[1].payload, "1");
}

#[test]
fn item_outcomes_decide_what_remains() {
    let conn = memory_db();
    let id = insert_job(&conn, &job("scan"), &new_items(5)).unwrap();
    set_job_item_state(&conn, id, 0, "done", "").unwrap();
    set_job_item_state(&conn, id, 1, "failed", "rate limit").unwrap();
    set_job_item_state(&conn, id, 2, "rolled_back", "").unwrap();

    let counts = job_item_counts(&conn, id).unwrap();
    assert_eq!(
        counts,
        JobItemCounts {
            pending: 2,
            done: 1,
            failed: 1,
            rolled_back: 1
        }
    );
    let remaining = job_items_in_states(&conn, id, REMAINING_ITEM_STATES).unwrap();
    assert_eq!(
        remaining.iter().map(|i| i.idx).collect::<Vec<_>>(),
        [2, 3, 4]
    );
    let finished = job_items_in_states(&conn, id, FINISHED_ITEM_STATES).unwrap();
    assert_eq!(finished[1].reason, "rate limit");

    // Putting an item back to pending clears its reason.
    set_job_item_state(&conn, id, 1, "pending", "").unwrap();
    assert_eq!(job_item_counts(&conn, id).unwrap().failed, 0);
}

#[test]
fn only_cut_short_jobs_are_resumable_and_kinds_are_separate() {
    let conn = memory_db();
    let queued = insert_job(&conn, &job("a"), &new_items(1)).unwrap();
    let running_id = insert_job(&conn, &job("a"), &new_items(1)).unwrap();
    let paused = insert_job(&conn, &job("b"), &new_items(1)).unwrap();
    let done = insert_job(&conn, &job("a"), &new_items(1)).unwrap();
    let cancelled = insert_job(&conn, &job("a"), &new_items(1)).unwrap();
    let state = |state| JobUpdate {
        state,
        ..running(0)
    };
    update_job(&conn, running_id, &state("running")).unwrap();
    update_job(&conn, paused, &state("paused")).unwrap();
    update_job(&conn, done, &state("succeeded")).unwrap();
    update_job(&conn, cancelled, &state("canceled")).unwrap();

    let ids = |kind| -> Vec<i64> {
        resumable_jobs(&conn, kind)
            .unwrap()
            .iter()
            .map(|j| j.id)
            .collect()
    };
    assert_eq!(ids(Some("a")), [queued, running_id]);
    assert_eq!(ids(Some("b")), [paused]);
    assert_eq!(ids(None), [queued, running_id, paused]);
}

#[test]
fn updates_stamp_start_and_finish_times_and_attempts_count() {
    let conn = memory_db();
    let id = insert_job(&conn, &job("a"), &new_items(2)).unwrap();

    update_job(&conn, id, &running(1)).unwrap();
    let first = get_job(&conn, id).unwrap().unwrap();
    assert_eq!((first.state.as_str(), first.done), ("running", 1));
    let started = first.started_ms.expect("start time");
    assert_eq!(first.finished_ms, None);

    // Pausing and running again keeps the first start time.
    update_job(
        &conn,
        id,
        &JobUpdate {
            state: "paused",
            ..running(1)
        },
    )
    .unwrap();
    update_job(&conn, id, &running(1)).unwrap();
    assert_eq!(
        get_job(&conn, id).unwrap().unwrap().started_ms,
        Some(started)
    );

    bump_attempts(&conn, id).unwrap();
    update_job(
        &conn,
        id,
        &JobUpdate {
            state: "failed",
            error: "boom",
            ..running(2)
        },
    )
    .unwrap();
    let last = get_job(&conn, id).unwrap().unwrap();
    assert_eq!((last.attempts, last.error.as_str()), (1, "boom"));
    assert!(last.finished_ms.is_some());
}

#[test]
fn history_lists_newest_first_and_only_finished_jobs_can_be_deleted() {
    let conn = memory_db();
    let old = insert_job(&conn, &job("a"), &new_items(2)).unwrap();
    let new = insert_job(&conn, &job("a"), &new_items(1)).unwrap();
    assert_eq!(
        list_jobs(&conn, 10)
            .unwrap()
            .iter()
            .map(|j| j.id)
            .collect::<Vec<_>>(),
        [new, old]
    );
    assert_eq!(list_jobs(&conn, 1).unwrap().len(), 1);

    assert!(
        !delete_finished_job(&conn, old).unwrap(),
        "a queued job is kept"
    );
    update_job(
        &conn,
        old,
        &JobUpdate {
            state: "succeeded",
            ..running(2)
        },
    )
    .unwrap();
    assert!(delete_finished_job(&conn, old).unwrap());
    assert!(get_job(&conn, old).unwrap().is_none());
    // Its items went with it.
    assert_eq!(
        job_item_counts(&conn, old).unwrap(),
        JobItemCounts::default()
    );
}

#[test]
fn deleting_the_job_row_cascades_to_its_items_and_bad_payloads_are_rejected() {
    let conn = memory_db();
    let id = insert_job(&conn, &job("a"), &new_items(2)).unwrap();
    conn.execute("DELETE FROM job WHERE id = ?1", [id]).unwrap();
    let left: i64 = conn
        .query_row("SELECT COUNT(*) FROM job_item", [], |r| r.get(0))
        .unwrap();
    assert_eq!(left, 0);

    let bad = vec![NewJobItem {
        label: "x".into(),
        payload: "{not json".into(),
    }];
    assert!(insert_job(&conn, &job("a"), &bad).is_err());
}
