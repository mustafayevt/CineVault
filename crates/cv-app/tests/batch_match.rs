//! Batch matching modes as background jobs: which items each mode takes, what it does
//! with them, and how the jobs sit on provider lanes. No network: the provider is scripted.

mod support;

use std::time::Duration;

use cv_app::matching::MatchSettings;
use cv_app::matching::batch::{
    BatchError, BatchMode, MATCH_JOB_KIND, MatchTarget, plan_batch, resume_match_jobs, submit_batch,
};
use cv_db::repo::review::{self, ReviewQueueState};
use cv_db::repo::step_outcome::{STEP_SCRAPE, StepStatus, step_outcome};
use cv_db::repo::{get_flags, set_new};
use cv_domain::{ItemId, ItemKind, ReviewState};
use cv_jobs::{JobState, LaneId};
use serde_json::json;
use support::{Entry, Lab, Scripted, arc, dynamic};

fn pending(lab: &Lab) -> u64 {
    lab.db.read(review::count_pending).unwrap()
}

fn solaris_catalogue() -> Scripted {
    Scripted::new(
        "tmdb",
        vec![
            Entry::new("593", "Solaris", 1972).runtime(167),
            Entry::new("2103", "Solaris", 2002).runtime(99),
            Entry::new("693134", "Dune: Part Two", 2024)
                .also("dune part two")
                .also("zzz query"),
            Entry::new("1", "Crash", 2004),
            Entry::new("2", "Crash", 2004),
        ],
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn match_new_takes_only_the_items_a_scan_flagged() {
    let lab = Lab::new();
    let provider = arc(solaris_catalogue());
    let ctx = lab.context(vec![dynamic(&provider)]);
    let fresh = lab.movie("Dune: Part Two", 2024);
    let old = lab.movie("Solaris", 1972);
    lab.db.write(move |tx| set_new(tx, &[fresh], true)).unwrap();

    let batch = submit_batch(&lab.manager, &ctx, BatchMode::MatchNew, None, 4).unwrap();
    assert_eq!(batch.planned, 1);
    let stats = batch.wait().await;

    assert_eq!(stats.accepted, 1);
    let scraped = lab.item(fresh);
    assert_eq!(scraped.core.last_scraper_id, "tmdb");
    assert_eq!(scraped.core.ids.get("tmdb"), Some("693134"));
    assert_eq!(scraped.core.plot, "Plot of Dune: Part Two (2024)");
    assert_eq!(
        lab.item(old).core.last_scraper_id,
        "",
        "an old item is left alone"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn match_unmatched_skips_scraped_and_ignored_items() {
    let lab = Lab::new();
    let provider = arc(solaris_catalogue());
    let ctx = lab.context(vec![dynamic(&provider)]);
    let todo = lab.movie("Dune: Part Two", 2024);
    let ignored = lab.movie("Holiday 2019", 2019);
    let mut done = cv_domain::Item::movie("Solaris");
    done.core.last_scraper_id = "tmdb".to_owned();
    let done = lab.add(done);
    lab.db
        .write(move |tx| review::dismiss(tx, ignored))
        .unwrap();

    let batch = submit_batch(&lab.manager, &ctx, BatchMode::MatchUnmatched, None, 4).unwrap();
    assert_eq!(batch.planned, 1);
    let stats = batch.wait().await;

    assert_eq!((stats.accepted, stats.queued), (1, 0));
    assert_eq!(lab.item(todo).core.last_scraper_id, "tmdb");
    assert_eq!(lab.item(ignored).core.last_scraper_id, "");
    assert_eq!(
        lab.item(done).core.plot,
        "",
        "an already scraped item is not touched"
    );
    assert_eq!(
        provider.searches.load(std::sync::atomic::Ordering::Relaxed),
        1
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn uncertain_items_are_queued_for_review_and_the_batch_does_not_wait() {
    let lab = Lab::new();
    let provider = arc(solaris_catalogue());
    let ctx = lab.context(vec![dynamic(&provider)]);
    let crash = lab.movie("Crash", 2004);
    let nothing = lab.movie("Nothing Matches This", 2001);
    let dune = lab.movie("Dune: Part Two", 2024);

    let stats = submit_batch(&lab.manager, &ctx, BatchMode::MatchUnmatched, None, 4)
        .unwrap()
        .wait()
        .await;

    assert_eq!((stats.accepted, stats.queued), (1, 2));
    assert_eq!(pending(&lab), 2);
    let tie = lab
        .db
        .read(|c| review::pending_for_item(c, crash))
        .unwrap()
        .unwrap();
    assert_eq!(tie.reason, "2 strong candidates");
    let none = lab
        .db
        .read(|c| review::pending_for_item(c, nothing))
        .unwrap()
        .unwrap();
    assert_eq!(none.reason, "No candidates found");
    assert_eq!(
        lab.db.read(|c| get_flags(c, crash)).unwrap().review_state,
        ReviewState::NeedsReview
    );
    assert_eq!(lab.item(dune).core.last_scraper_id, "tmdb");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rescrape_by_ids_refreshes_fifty_movies_without_search_or_review() {
    let lab = Lab::new();
    let mut entries = Vec::new();
    let mut picked: Vec<ItemId> = Vec::new();
    for n in 0..50 {
        let id = format!("{}", 1000 + n);
        entries.push(Entry::new(
            &id,
            &format!("Film {n:02}"),
            1980 + (n % 30) as u16,
        ));
        let mut item = cv_domain::Item::movie(format!("Film {n:02}"));
        item.core.ids.set("tmdb", &id);
        item.core.last_scraper_id = "tmdb".to_owned();
        item.core.plot = "stale".to_owned();
        picked.push(lab.add(item));
    }
    let provider = arc(Scripted::new("tmdb", entries));
    let ctx = lab.context(vec![dynamic(&provider)]);

    let batch = submit_batch(
        &lab.manager,
        &ctx,
        BatchMode::RescrapeByIds,
        Some(&picked),
        4,
    )
    .unwrap();
    let stats = batch.wait().await;

    assert_eq!(stats.rescraped, 50);
    assert_eq!(Scripted::count(&provider.searches), 0, "no search");
    assert_eq!(Scripted::count(&provider.lookups), 0, "no lookup either");
    assert_eq!(pending(&lab), 0, "no review");
    assert_eq!(
        Scripted::count(&provider.bypass_requests),
        50,
        "fresh data is requested"
    );
    assert!(
        picked
            .iter()
            .all(|id| lab.item(*id).core.plot.starts_with("Plot of Film"))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rescrape_fails_an_item_without_an_id_and_goes_on() {
    let lab = Lab::new();
    let provider = arc(solaris_catalogue());
    let ctx = lab.context(vec![dynamic(&provider)]);
    let mut with_id = cv_domain::Item::movie("Dune");
    with_id.core.ids.set("tmdb", "693134");
    let with_id = lab.add(with_id);
    let without = lab.movie("Solaris", 1972);

    let batch = submit_batch(
        &lab.manager,
        &ctx,
        BatchMode::RescrapeByIds,
        Some(&[with_id, without]),
        2,
    )
    .unwrap();
    let report = batch.jobs[0].job.wait().await;

    assert_eq!(report.snapshot.state, JobState::Completed);
    assert_eq!((report.snapshot.done, report.snapshot.failed), (2, 1));
    let groups = report.failure_groups();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].reason, "no stored ID");
    assert_eq!(Scripted::count(&provider.searches), 0);
    assert_eq!(lab.item(without).core.last_scraper_id, "");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn force_best_match_accepts_the_top_candidate_with_no_review() {
    let lab = Lab::new();
    let provider = arc(solaris_catalogue());
    let ctx = lab.context(vec![dynamic(&provider)]);
    let tie = lab.movie("Crash", 2004);
    let weak = lab.movie("Zzz Query", 2024);
    let nothing = lab.movie("Nothing Matches This", 2001);
    // The two uncertain items sit in the inbox first.
    submit_batch(&lab.manager, &ctx, BatchMode::MatchUnmatched, None, 4)
        .unwrap()
        .wait()
        .await;
    let queued_before = pending(&lab);
    assert!(queued_before >= 2);

    let batch = submit_batch(
        &lab.manager,
        &ctx,
        BatchMode::ForceBestMatch,
        Some(&[tie, weak, nothing]),
        4,
    )
    .unwrap();
    let report = batch.jobs[0].job.wait().await;

    assert_eq!((report.snapshot.done, report.snapshot.failed), (3, 1));
    assert_eq!(report.failure_groups()[0].reason, "no candidates found");
    // The forced scrape that found nothing is stored as tried and not matched.
    assert_eq!(
        scrape_of(&lab, nothing),
        Some((StepStatus::NotFound, "No candidates found".to_owned()))
    );
    assert_eq!(scrape_of(&lab, tie).map(|o| o.0), Some(StepStatus::Done));
    assert_eq!(lab.item(tie).core.last_scraper_id, "tmdb");
    assert_eq!(lab.item(weak).core.last_scraper_id, "tmdb");
    assert_eq!(lab.item(weak).core.title, "Dune: Part Two");
    // The forced items left the inbox; the one with nothing to accept is still there.
    assert_eq!(pending(&lab), 1);
    assert!(
        lab.db
            .read(|c| review::pending_for_item(c, nothing))
            .unwrap()
            .is_some()
    );
    let closed = lab.db.read(|c| review::pending_for_item(c, tie)).unwrap();
    assert!(closed.is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_id_the_item_already_has_is_looked_up_with_no_text_search() {
    let lab = Lab::new();
    let provider = arc(solaris_catalogue());
    let ctx = lab.context(vec![dynamic(&provider)]);
    let mut from_nfo = cv_domain::Item::movie("Wrong Folder Name");
    from_nfo.core.ids.set("tmdb", "693134");
    let from_nfo = lab.add(from_nfo);
    // A folder named after an IMDb ID is the same shortcut.
    let by_name = lab.movie("tt0000593", 0);

    let stats = submit_batch(&lab.manager, &ctx, BatchMode::MatchUnmatched, None, 4)
        .unwrap()
        .wait()
        .await;

    assert_eq!((stats.accepted, stats.by_id, stats.queued), (2, 2, 0));
    assert_eq!(Scripted::count(&provider.searches), 0);
    assert_eq!(lab.item(from_nfo).core.title, "Dune: Part Two");
    assert_eq!(lab.item(by_name).core.year, 1972);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runtime_evidence_settles_identical_titles() {
    let lab = Lab::new();
    let provider = arc(solaris_catalogue());
    let ctx = lab.context(vec![dynamic(&provider)]);
    let russian = lab.movie_with_video("Solaris", 0, 167, "ru");
    let no_evidence = lab.movie("Solaris", 0);

    let stats = submit_batch(&lab.manager, &ctx, BatchMode::MatchUnmatched, None, 1)
        .unwrap()
        .wait()
        .await;

    assert_eq!((stats.accepted, stats.queued), (1, 1));
    assert_eq!(lab.item(russian).core.year, 1972);
    assert_eq!(lab.item(russian).core.ids.get("tmdb"), Some("593"));
    let row = lab
        .db
        .read(|c| review::pending_for_item(c, no_evidence))
        .unwrap()
        .unwrap();
    // Neither a year nor a runtime tells the two films apart.
    assert_eq!(row.reason, "No year in the file name (2 strong candidates)");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_clean_match_costs_one_search_and_one_details_request() {
    let lab = Lab::new();
    let provider = arc(solaris_catalogue());
    let ctx = lab.context(vec![dynamic(&provider)]);
    lab.movie_with_video("Dune: Part Two", 2024, 166, "en");

    submit_batch(&lab.manager, &ctx, BatchMode::MatchUnmatched, None, 1)
        .unwrap()
        .wait()
        .await;

    assert_eq!(Scripted::count(&provider.searches), 1);
    assert_eq!(Scripted::count(&provider.details), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_locked_item_is_not_touched_or_searched() {
    let lab = Lab::new();
    let provider = arc(solaris_catalogue());
    let ctx = lab.context(vec![dynamic(&provider)]);
    let mut item = cv_domain::Item::movie("Dune: Part Two");
    item.core.locked = true;
    let locked = lab.add(item);

    let stats = submit_batch(&lab.manager, &ctx, BatchMode::MatchUnmatched, None, 1)
        .unwrap()
        .wait()
        .await;

    assert_eq!(stats.locked, 1);
    assert_eq!(Scripted::count(&provider.searches), 0);
    assert_eq!(lab.item(locked).core.last_scraper_id, "");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn each_provider_gets_its_own_job_on_its_own_lane() {
    let lab = Lab::new();
    let movies =
        arc(Scripted::new("tmdb", vec![Entry::new("593", "Solaris", 1972)]).kinds(true, false));
    let shows = arc(
        Scripted::new("tvdb", vec![Entry::new("81189", "Breaking Bad", 2008)]).kinds(false, true),
    );
    let ctx = lab.context(vec![dynamic(&movies), dynamic(&shows)]);
    let movie = lab.movie("Solaris", 1972);
    let show = lab.show("Breaking Bad", 2008);

    let batch = submit_batch(&lab.manager, &ctx, BatchMode::MatchUnmatched, None, 4).unwrap();
    assert_eq!(batch.jobs.len(), 2);
    let lanes: Vec<LaneId> = batch.jobs.iter().map(|j| j.job.snapshot().lane).collect();
    assert!(lanes.contains(&LaneId::provider("tmdb")));
    assert!(lanes.contains(&LaneId::provider("tvdb")));
    let stats = batch.wait().await;

    assert_eq!(stats.accepted, 2);
    assert_eq!(lab.item(movie).core.last_scraper_id, "tmdb");
    assert_eq!(lab.item(show).core.last_scraper_id, "tvdb");
    assert_eq!(lab.kind_of(show), ItemKind::Show);
}

#[test]
fn an_item_no_provider_serves_is_reported_not_queued() {
    let lab = Lab::new();
    let movies = arc(Scripted::new("tmdb", vec![]).kinds(true, false));
    let ctx = lab.context(vec![dynamic(&movies)]);
    let movie = lab.movie("Solaris", 1972);
    let show = lab.show("Breaking Bad", 2008);

    let plan = plan_batch(&lab.db, &ctx.providers, BatchMode::MatchUnmatched, None).unwrap();
    assert_eq!(plan.targets.len(), 1);
    assert_eq!(plan.targets[0].item, movie.0);
    assert_eq!(plan.targets[0].provider, "tmdb");
    assert_eq!(plan.no_provider, vec![show]);
}

#[test]
fn the_selection_modes_need_a_selection_and_a_provider() {
    let lab = Lab::new();
    let provider = arc(solaris_catalogue());
    let ctx = lab.context(vec![dynamic(&provider)]);
    for mode in [BatchMode::RescrapeByIds, BatchMode::ForceBestMatch] {
        assert!(matches!(
            plan_batch(&lab.db, &ctx.providers, mode, None),
            Err(BatchError::SelectionRequired(m)) if m == mode
        ));
    }
    let empty = lab.context(vec![]);
    assert!(matches!(
        plan_batch(&lab.db, &empty.providers, BatchMode::MatchNew, None),
        Err(BatchError::NoProviders)
    ));
}

#[test]
fn a_selection_narrows_the_automatic_modes() {
    let lab = Lab::new();
    let provider = arc(solaris_catalogue());
    let ctx = lab.context(vec![dynamic(&provider)]);
    let a = lab.movie("Solaris", 1972);
    let _b = lab.movie("Crash", 2004);
    let plan = plan_batch(
        &lab.db,
        &ctx.providers,
        BatchMode::MatchUnmatched,
        Some(&[a]),
    )
    .unwrap();
    assert_eq!(plan.targets.len(), 1);
    assert_eq!(plan.targets[0].label, "Solaris (1972)");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_tie_without_a_year_in_the_name_says_the_year_is_missing() {
    let lab = Lab::new();
    let provider = arc(solaris_catalogue());
    let ctx = lab.context(vec![dynamic(&provider)]);
    // No year in the name or the item: the two Solaris films tie.
    let solaris = lab.movie("Solaris", 0);

    let stats = submit_batch(&lab.manager, &ctx, BatchMode::MatchUnmatched, None, 2)
        .unwrap()
        .wait()
        .await;

    assert_eq!(stats.queued, 1);
    let row = lab
        .db
        .read(|c| review::pending_for_item(c, solaris))
        .unwrap()
        .unwrap();
    assert!(
        row.reason.starts_with("No year in the file name"),
        "{}",
        row.reason
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_title_whose_provider_has_no_key_waits_in_needs_review_saying_so() {
    let lab = Lab::new();
    let provider = arc(solaris_catalogue());
    provider.need_key_for("crash");
    let ctx = lab.context(vec![dynamic(&provider)]);
    let crash = lab.movie("Crash", 2004);

    let stats = submit_batch(&lab.manager, &ctx, BatchMode::MatchUnmatched, None, 2)
        .unwrap()
        .wait()
        .await;

    assert_eq!(stats.queued, 1, "{stats:?}");
    let row = lab
        .db
        .read(|c| review::pending_for_item(c, crash))
        .unwrap()
        .unwrap();
    assert_eq!(row.reason, "No TMDB key");
    assert_eq!(
        lab.db.read(|c| get_flags(c, crash)).unwrap().review_state,
        ReviewState::NeedsReview
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_provider_failure_fails_only_that_item_and_can_be_retried() {
    let lab = Lab::new();
    let provider = arc(solaris_catalogue());
    provider.fail_search("crash");
    let ctx = lab.context(vec![dynamic(&provider)]);
    let crash = lab.movie("Crash", 2004);
    let dune = lab.movie("Dune: Part Two", 2024);

    let batch = submit_batch(&lab.manager, &ctx, BatchMode::MatchUnmatched, None, 2).unwrap();
    let report = batch.jobs[0].job.wait().await;

    assert_eq!(report.snapshot.state, JobState::Completed);
    assert_eq!((report.snapshot.done, report.snapshot.failed), (2, 1));
    assert_eq!(report.failure_groups()[0].reason, "provider unreachable");
    assert_eq!(lab.item(dune).core.last_scraper_id, "tmdb");
    assert_eq!(lab.item(crash).core.last_scraper_id, "");
    assert_eq!(
        pending(&lab),
        0,
        "a failed item is not queued as if it were uncertain"
    );
    // Tried and failed, so it is not "not scraped yet"; the match is stored as done.
    assert_eq!(
        scrape_of(&lab, crash),
        Some((
            StepStatus::Failed,
            "The scrape failed: provider unreachable".to_owned()
        ))
    );
    assert_eq!(scrape_of(&lab, dune).map(|o| o.0), Some(StepStatus::Done));

    // The provider recovers; only the failed item runs again.
    provider.failing.lock().unwrap().clear();
    let retry = batch.jobs[0].job.retry_failed().expect("a retry job");
    let report = retry.wait().await;
    assert_eq!(report.snapshot.total, 1);
    assert_eq!(report.snapshot.state, JobState::Completed);
    assert_eq!(pending(&lab), 1, "the retried tie now waits for review");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelling_a_batch_stops_it_and_leaves_no_half_written_item() {
    let lab = Lab::new();
    let mut entries = Vec::new();
    let mut ids = Vec::new();
    for n in 0..80 {
        let title = format!("Film {n:02}");
        entries.push(Entry::new(&format!("{}", 500 + n), &title, 1990));
        ids.push(lab.movie(&title, 1990));
    }
    let provider = arc(Scripted::new("tmdb", entries).latency(Duration::from_millis(60)));
    let ctx = lab.context(vec![dynamic(&provider)]);

    let batch = submit_batch(&lab.manager, &ctx, BatchMode::MatchUnmatched, None, 4).unwrap();
    // The gate: cancel once the batch is running (the provider was asked), not after a fixed
    // delay. Four at a time at 60 ms each, the 80 items need over a second, so most are left.
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    while Scripted::count(&provider.searches) == 0 {
        assert!(
            std::time::Instant::now() < deadline,
            "the batch never started"
        );
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    assert!(batch.jobs[0].job.cancel());
    let report = tokio::time::timeout(Duration::from_secs(60), batch.jobs[0].job.wait())
        .await
        .expect("the cancel is prompt");

    assert_eq!(report.snapshot.state, JobState::Cancelled);
    assert!(report.snapshot.done < 80);
    for id in ids {
        let item = lab.item(id);
        // An item is either fully scraped or untouched.
        assert_eq!(
            item.core.last_scraper_id == "tmdb",
            item.core.plot.starts_with("Plot of"),
            "{}",
            item.core.title
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_interrupted_batch_resumes_with_its_unfinished_items() {
    use cv_db::repo::jobs::{NewJob, NewJobItem, insert_job};

    let lab = Lab::new();
    let provider = arc(solaris_catalogue());
    let ctx = lab.context(vec![dynamic(&provider)]);
    let dune = lab.movie("Dune: Part Two", 2024);
    let crash = lab.movie("Crash", 2004);

    // The state a quit leaves: a stored job whose items never ran.
    let plan = plan_batch(&lab.db, &ctx.providers, BatchMode::MatchUnmatched, None).unwrap();
    assert_eq!(plan.targets.len(), 2);
    let items: Vec<NewJobItem> = plan
        .targets
        .iter()
        .map(|t: &MatchTarget| NewJobItem {
            label: t.to_string(),
            payload: serde_json::to_string(t).unwrap(),
        })
        .collect();
    let payload = json!({ "concurrency": 2 }).to_string();
    lab.db
        .write(move |tx| {
            let job = NewJob {
                kind: MATCH_JOB_KIND,
                name: "Scraping 3 titles",
                lane: "provider:tmdb",
                priority: 0,
                payload: &payload,
            };
            insert_job(tx, &job, &items)
        })
        .unwrap();

    let resumed = resume_match_jobs(&lab.manager, &ctx).unwrap();
    assert_eq!(resumed.len(), 1);
    assert_eq!(resumed[0].provider, "tmdb");
    let report = resumed[0].job.wait().await;
    assert_eq!(report.snapshot.state, JobState::Completed);
    assert_eq!(report.snapshot.total, 2);
    assert_eq!(resumed[0].stats().accepted, 1);
    assert_eq!(resumed[0].stats().queued, 1);
    assert_eq!(lab.item(dune).core.last_scraper_id, "tmdb");
    assert_eq!(
        lab.db
            .read(|c| review::pending_for_item(c, crash))
            .unwrap()
            .unwrap()
            .state,
        ReviewQueueState::Pending
    );
    assert!(resume_match_jobs(&lab.manager, &ctx).unwrap().is_empty());
}

#[test]
fn the_settings_map_sets_the_threshold_and_the_languages() {
    use serde_json::Value;
    use std::collections::BTreeMap;

    let defaults = MatchSettings::from_values(&BTreeMap::new());
    assert_eq!(defaults.rules.threshold(), 0.75);
    assert_eq!(defaults.request.language, "en");
    assert_eq!(defaults.request.country, "US");

    let mut values: BTreeMap<String, Value> = BTreeMap::new();
    values.insert("matching.auto-accept-threshold".to_owned(), json!(0.9));
    values.insert("metadata.language".to_owned(), json!("tr"));
    values.insert("metadata.fallback-language".to_owned(), json!("de"));
    let s = MatchSettings::from_values(&values);
    assert_eq!(s.rules.threshold(), 0.9);
    values.insert("metadata.country".to_owned(), json!(" de "));
    assert_eq!(MatchSettings::from_values(&values).request.country, "DE");
    values.insert("metadata.country".to_owned(), json!(""));
    assert_eq!(MatchSettings::from_values(&values).request.country, "US");
    assert!(
        !defaults.request.local_release_date,
        "the original release by default"
    );
    values.insert("metadata.local-release-date".to_owned(), json!(true));
    assert!(
        MatchSettings::from_values(&values)
            .request
            .local_release_date
    );
    assert_eq!(
        (
            s.request.language.as_str(),
            s.request.fallback_language.as_str()
        ),
        ("tr", "de")
    );

    // A percentage works too, and junk falls back to the default.
    values.insert("matching.auto-accept-threshold".to_owned(), json!(85));
    assert_eq!(MatchSettings::from_values(&values).rules.threshold(), 0.85);
    values.insert("matching.auto-accept-threshold".to_owned(), json!("high"));
    assert_eq!(MatchSettings::from_values(&values).rules.threshold(), 0.75);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_threshold_decides_between_accepting_and_review() {
    let lab = Lab::new();
    let provider = arc(Scripted::new(
        "tmdb",
        vec![Entry::new("693134", "Dune: Part Two", 2024).also("dune part twoo")],
    ));
    let mut ctx = lab.context(vec![dynamic(&provider)]);
    let typo = lab.movie("Dune Part Twoo", 2024);

    // A misspelt title scores under 0.99: a strict threshold queues it.
    ctx.settings.rules = cv_match::AcceptRules::new(0.99);
    let stats = submit_batch(&lab.manager, &ctx, BatchMode::MatchUnmatched, None, 1)
        .unwrap()
        .wait()
        .await;
    assert_eq!((stats.accepted, stats.queued), (0, 1));
    assert_eq!(pending(&lab), 1);

    // The default threshold accepts it, and the item leaves the inbox.
    ctx.settings.rules = cv_match::AcceptRules::default();
    let stats = submit_batch(&lab.manager, &ctx, BatchMode::MatchUnmatched, None, 1)
        .unwrap()
        .wait()
        .await;
    assert_eq!((stats.accepted, stats.queued), (1, 0));
    assert_eq!(pending(&lab), 0);
    assert_eq!(lab.item(typo).core.title, "Dune: Part Two");
}

/// The stored outcome of the item's scrape.
fn scrape_of(lab: &Lab, item: ItemId) -> Option<(StepStatus, String)> {
    lab.db
        .read(move |c| step_outcome(c, item, STEP_SCRAPE))
        .unwrap()
        .map(|o| (o.status, o.reason))
}
