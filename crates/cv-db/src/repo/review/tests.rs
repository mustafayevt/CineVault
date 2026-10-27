//! Tests of the review queue.

use cv_domain::{Item, ItemId, ReviewState};
use rusqlite::Connection;
use serde_json::json;

use super::*;
use crate::repo::{get_flags, insert_item};

fn memory_db() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.pragma_update(None, "foreign_keys", true).unwrap();
    crate::schema::migrate(&mut conn).unwrap();
    conn
}

fn movie(conn: &Connection, title: &str) -> ItemId {
    insert_item(conn, &Item::movie(title)).unwrap()
}

fn review(item_id: ItemId, reason: &str, score: Option<f64>) -> NewReview {
    NewReview {
        item_id,
        reason: reason.to_owned(),
        best_score: score,
        candidates: json!([{ "title": "Solaris", "year": 1972, "breakdown": { "score": 0.9 } }]),
        score_breakdown: Some(json!({ "score": 0.9, "evidence": [] })),
    }
}

fn state_of(conn: &Connection, id: ItemId) -> ReviewState {
    get_flags(conn, id).unwrap().review_state
}

#[test]
fn enqueue_stores_reason_candidates_and_breakdown_and_flags_the_item() {
    let conn = memory_db();
    let id = movie(&conn, "Solaris");
    let outcome = enqueue(&conn, &review(id, "2 strong candidates", Some(0.9))).unwrap();
    let row_id = outcome.review_id().unwrap();
    assert!(matches!(outcome, EnqueueOutcome::Queued(_)));

    let row = get_review(&conn, row_id).unwrap().unwrap();
    assert_eq!(row.item_id, id);
    assert_eq!(row.reason, "2 strong candidates");
    assert_eq!(row.state, ReviewQueueState::Pending);
    assert_eq!(row.best_score, Some(0.9));
    assert_eq!(row.candidates[0]["title"], "Solaris");
    assert_eq!(row.score_breakdown.unwrap()["score"], 0.9);
    assert!(row.created_ms > 0);
    assert_eq!(row.resolved_ms, None);
    assert_eq!(state_of(&conn, id), ReviewState::NeedsReview);
    assert_eq!(count_pending(&conn).unwrap(), 1);
}

#[test]
fn an_item_without_candidates_is_queued_with_no_score() {
    let conn = memory_db();
    let id = movie(&conn, "Home video");
    let mut r = review(id, "No candidates found", None);
    r.candidates = json!([]);
    r.score_breakdown = None;
    enqueue(&conn, &r).unwrap();
    let row = pending_for_item(&conn, id).unwrap().unwrap();
    assert_eq!(row.best_score, None);
    assert_eq!(row.candidates, json!([]));
    assert_eq!(row.score_breakdown, None);
}

#[test]
fn queueing_a_pending_item_again_refreshes_its_single_row() {
    let conn = memory_db();
    let id = movie(&conn, "Solaris");
    let first = enqueue(&conn, &review(id, "2 strong candidates", Some(0.9)))
        .unwrap()
        .review_id()
        .unwrap();
    let second = enqueue(&conn, &review(id, "Best match 60% is below 75%", Some(0.6))).unwrap();
    assert_eq!(second, EnqueueOutcome::Refreshed(first));
    assert_eq!(count_pending(&conn).unwrap(), 1);
    let row = get_review(&conn, first).unwrap().unwrap();
    assert_eq!(row.reason, "Best match 60% is below 75%");
    assert_eq!(row.best_score, Some(0.6));
}

#[test]
fn enqueue_rejects_an_unknown_item() {
    let conn = memory_db();
    let err = enqueue(&conn, &review(ItemId(9999), "x", None)).unwrap_err();
    assert!(matches!(err, DbError::ItemNotFound(9999)));
}

#[test]
fn the_inbox_lists_pending_rows_most_confident_first() {
    let conn = memory_db();
    let a = movie(&conn, "A");
    let b = movie(&conn, "B");
    let c = movie(&conn, "C");
    let d = movie(&conn, "D");
    enqueue(&conn, &review(a, "r", Some(0.6))).unwrap();
    enqueue(&conn, &review(b, "r", Some(0.9))).unwrap();
    enqueue(&conn, &review(c, "r", None)).unwrap();
    enqueue(&conn, &review(d, "r", Some(0.9))).unwrap();

    let order: Vec<_> = list_pending(&conn, 10, 0)
        .unwrap()
        .into_iter()
        .map(|r| r.item_id)
        .collect();
    assert_eq!(order, vec![b, d, a, c]);
    let page: Vec<_> = list_pending(&conn, 2, 1)
        .unwrap()
        .into_iter()
        .map(|r| r.item_id)
        .collect();
    assert_eq!(page, vec![d, a]);
}

#[test]
fn bulk_accept_set_is_every_pending_row_at_or_above_the_chosen_score() {
    let conn = memory_db();
    let ids: Vec<_> = ["A", "B", "C", "D"].map(|t| movie(&conn, t)).to_vec();
    for (id, score) in ids.iter().zip([0.95, 0.85, 0.849, 0.2]) {
        enqueue(&conn, &review(*id, "r", Some(score))).unwrap();
    }
    let picked: Vec<_> = pending_at_or_above(&conn, 0.85)
        .unwrap()
        .into_iter()
        .map(|r| r.item_id)
        .collect();
    assert_eq!(picked, vec![ids[0], ids[1]]);
}

#[test]
fn accepting_closes_the_row_and_resolves_the_item() {
    let conn = memory_db();
    let id = movie(&conn, "Solaris");
    let row_id = enqueue(&conn, &review(id, "r", Some(0.9)))
        .unwrap()
        .review_id()
        .unwrap();
    assert_eq!(
        resolve_pending(&conn, id, ReviewQueueState::Accepted).unwrap(),
        Some(row_id)
    );
    let row = get_review(&conn, row_id).unwrap().unwrap();
    assert_eq!(row.state, ReviewQueueState::Accepted);
    assert!(row.resolved_ms.is_some());
    assert_eq!(state_of(&conn, id), ReviewState::Resolved);
    assert_eq!(count_pending(&conn).unwrap(), 0);
    assert_eq!(pending_for_item(&conn, id).unwrap(), None);
    // Nothing pending any more: a second resolve changes nothing.
    assert_eq!(
        resolve_pending(&conn, id, ReviewQueueState::Accepted).unwrap(),
        None
    );
}

#[test]
fn skipping_keeps_the_item_in_needs_review_and_allows_a_new_row() {
    let conn = memory_db();
    let id = movie(&conn, "Solaris");
    enqueue(&conn, &review(id, "r", Some(0.5))).unwrap();
    resolve_pending(&conn, id, ReviewQueueState::Skipped).unwrap();
    assert_eq!(state_of(&conn, id), ReviewState::NeedsReview);
    assert_eq!(count_pending(&conn).unwrap(), 0);
    // A later matching run may queue it again.
    assert!(matches!(
        enqueue(&conn, &review(id, "r", Some(0.5))).unwrap(),
        EnqueueOutcome::Queued(_)
    ));
}

#[test]
fn an_ignored_item_leaves_the_inbox_and_is_never_queued_again() {
    let conn = memory_db();
    let id = movie(&conn, "Birthday 2019");
    enqueue(&conn, &review(id, "r", Some(0.3))).unwrap();
    resolve_pending(&conn, id, ReviewQueueState::Dismissed).unwrap();

    assert!(is_dismissed(&conn, id).unwrap());
    assert_eq!(dismissed_item_ids(&conn).unwrap(), vec![id]);
    assert_eq!(count_pending(&conn).unwrap(), 0);
    assert_eq!(state_of(&conn, id), ReviewState::Resolved);

    // A future scan or matching run tries to queue it: nothing is written.
    assert_eq!(
        enqueue(&conn, &review(id, "r", Some(0.3))).unwrap(),
        EnqueueOutcome::Dismissed
    );
    assert_eq!(count_pending(&conn).unwrap(), 0);
    assert_eq!(state_of(&conn, id), ReviewState::Resolved);
}

#[test]
fn ignoring_a_library_item_with_no_pending_row_records_a_closed_row() {
    let conn = memory_db();
    let id = movie(&conn, "Birthday 2019");
    let row_id = dismiss(&conn, id).unwrap();
    let row = get_review(&conn, row_id).unwrap().unwrap();
    assert_eq!(row.state, ReviewQueueState::Dismissed);
    assert!(is_dismissed(&conn, id).unwrap());
    assert_eq!(state_of(&conn, id), ReviewState::Resolved);
    // Ignoring twice keeps the one row.
    assert_eq!(dismiss(&conn, id).unwrap(), row_id);
    assert!(matches!(
        dismiss(&conn, ItemId(777)),
        Err(DbError::ItemNotFound(777))
    ));
    assert_eq!(
        enqueue(&conn, &review(id, "r", Some(0.4))).unwrap(),
        EnqueueOutcome::Dismissed
    );
}

#[test]
fn reopen_is_the_undo_of_an_ignore() {
    let conn = memory_db();
    let id = movie(&conn, "Birthday 2019");
    let row_id = enqueue(&conn, &review(id, "r", Some(0.3)))
        .unwrap()
        .review_id()
        .unwrap();
    resolve_pending(&conn, id, ReviewQueueState::Dismissed).unwrap();
    reopen(&conn, row_id).unwrap();

    assert!(!is_dismissed(&conn, id).unwrap());
    assert_eq!(count_pending(&conn).unwrap(), 1);
    assert_eq!(state_of(&conn, id), ReviewState::NeedsReview);
    let row = get_review(&conn, row_id).unwrap().unwrap();
    assert_eq!(row.state, ReviewQueueState::Pending);
    assert_eq!(row.resolved_ms, None);
    // Not closed any more, so it cannot be reopened twice.
    assert!(matches!(
        reopen(&conn, row_id),
        Err(DbError::InvalidData(_))
    ));
    assert!(matches!(reopen(&conn, 4242), Err(DbError::InvalidData(_))));
}

#[test]
fn a_row_cannot_be_resolved_to_pending() {
    let conn = memory_db();
    let id = movie(&conn, "Solaris");
    enqueue(&conn, &review(id, "r", Some(0.5))).unwrap();
    assert!(matches!(
        resolve_pending(&conn, id, ReviewQueueState::Pending),
        Err(DbError::InvalidData(_))
    ));
    assert_eq!(count_pending(&conn).unwrap(), 1);
}

#[test]
fn deleting_an_item_removes_its_review_rows() {
    // The foreign key cascades; the user-facing delete of library rows is explicit only.
    let conn = memory_db();
    let id = movie(&conn, "Solaris");
    enqueue(&conn, &review(id, "r", Some(0.5))).unwrap();
    crate::repo::delete_item(&conn, id).unwrap();
    assert_eq!(count_pending(&conn).unwrap(), 0);
}

#[test]
fn state_text_round_trips() {
    for s in [
        ReviewQueueState::Pending,
        ReviewQueueState::Accepted,
        ReviewQueueState::Skipped,
        ReviewQueueState::Dismissed,
    ] {
        assert_eq!(ReviewQueueState::parse(s.as_str()), Some(s));
    }
    assert_eq!(ReviewQueueState::parse("later"), None);
}
