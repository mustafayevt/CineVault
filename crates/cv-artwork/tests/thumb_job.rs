//! The thumbnail job behind a rescan: which images it builds, how it reuses and
//! reports, and that it survives a restart.

mod support;

use std::sync::Arc;

use cv_artwork::thumb::{ThumbCache, ThumbKey, ThumbSize};
use cv_artwork::thumb_job::{
    BUILD_SIZES, THUMB_JOB_KIND, ThumbTarget, build_after, resume_thumb_jobs, submit_thumb_job,
    thumb_targets,
};
use cv_jobs::JobState;
use support::{Lib, poster_jpeg, variant};

fn built(cache: &ThumbCache, bytes: &[u8]) -> bool {
    let key = ThumbKey::of_bytes(bytes);
    BUILD_SIZES.iter().all(|s| cache.has(&key, *s)) && cache.meta(&key).is_some()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_rescan_is_followed_by_a_job_that_caches_the_new_posters() {
    let lib = Lib::movies();
    let base = poster_jpeg(240, 360, 1);
    let alien = variant(&base, 1);
    let heat = variant(&base, 2);
    let fanart = variant(&base, 3);
    lib.put_bytes("Alien (1979)/Alien (1979).mkv", b"a");
    lib.put_bytes("Alien (1979)/Alien (1979)-poster.jpg", &alien);
    lib.put_bytes("Alien (1979)/Alien (1979)-fanart.jpg", &fanart);
    lib.put_bytes("Heat (1995)/Heat (1995).mkv", b"h");
    lib.put_bytes("Heat (1995)/Heat (1995)-poster.jpg", &heat);

    let rescan = lib.rescan();
    let follow = {
        let changed = rescan.clone();
        build_after(
            &lib.manager,
            &lib.db,
            &lib.cache,
            rescan.job.clone(),
            move || changed.changed_items(),
            2,
        )
    };
    let thumbs = follow.await.unwrap().expect("a thumbnail job was queued");
    let report = thumbs.job.wait().await;

    assert_eq!(report.snapshot.state, JobState::Completed);
    assert_eq!((report.snapshot.total, report.snapshot.failed), (2, 0));
    assert!(built(&lib.cache, &alien));
    assert!(built(&lib.cache, &heat));
    assert!(
        !lib.cache.has(&ThumbKey::of_bytes(&fanart), ThumbSize::Grid),
        "fan art is generated on demand, not by the job"
    );
    assert!(
        !lib.cache.has(&ThumbKey::of_bytes(&alien), ThumbSize::Hero),
        "hero is generated on demand"
    );
    assert_eq!(thumbs.stats().generated, 2);

    // A rescan that finds nothing new queues no job.
    let again = lib.rescan();
    let follow = {
        let changed = again.clone();
        build_after(
            &lib.manager,
            &lib.db,
            &lib.cache,
            again.job.clone(),
            move || changed.changed_items(),
            2,
        )
    };
    assert!(follow.await.unwrap().is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_new_shows_season_images_are_included() {
    let lib = Lib::tv();
    let base = poster_jpeg(200, 300, 2);
    let show = variant(&base, 10);
    let season = variant(&base, 11);
    lib.put_bytes("Andor (2022)/poster.jpg", &show);
    lib.put_bytes("Andor (2022)/season01-poster.jpg", &season);
    lib.put_bytes("Andor (2022)/Season 01/Andor.S01E01.mkv", b"e");

    let rescan = lib.rescan();
    let report = rescan.job.wait().await;
    assert_eq!(report.snapshot.state, JobState::Completed);

    let targets = thumb_targets(&lib.db, &rescan.changed_items()).unwrap();
    assert_eq!(
        targets.len(),
        2,
        "show poster and season poster: {targets:?}"
    );

    let job = submit_thumb_job(&lib.manager, Arc::clone(&lib.cache), targets, 2)
        .unwrap()
        .unwrap();
    assert_eq!(job.job.wait().await.snapshot.state, JobState::Completed);
    assert!(built(&lib.cache, &show));
    assert!(built(&lib.cache, &season));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn identical_images_and_a_second_run_decode_nothing_again() {
    let lib = Lib::movies();
    let same = variant(&poster_jpeg(200, 300, 3), 1);
    lib.put_bytes("A (2001)/A (2001).mkv", b"a");
    lib.put_bytes("A (2001)/A (2001)-poster.jpg", &same);
    lib.put_bytes("B (2002)/B (2002).mkv", b"b");
    lib.put_bytes("B (2002)/B (2002)-poster.jpg", &same);
    lib.rescan().job.wait().await;

    let targets = thumb_targets(&lib.db, &lib.item_ids()).unwrap();
    assert_eq!(targets.len(), 2, "two files, one content");

    // One at a time, so the second copy always finds the first one's entry.
    let first = submit_thumb_job(&lib.manager, Arc::clone(&lib.cache), targets.clone(), 1)
        .unwrap()
        .unwrap();
    first.job.wait().await;
    assert_eq!(first.stats().generated, 1);
    assert_eq!(first.stats().reused, 1);
    assert_eq!(lib.cache.generations(), 1);

    let second = submit_thumb_job(&lib.manager, Arc::clone(&lib.cache), targets, 2)
        .unwrap()
        .unwrap();
    second.job.wait().await;
    assert_eq!(second.stats().generated, 0);
    assert_eq!(second.stats().reused, 2);
    assert_eq!(lib.cache.generations(), 1, "nothing was decoded again");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_missing_or_damaged_image_fails_only_its_own_item() {
    let lib = Lib::movies();
    let good = variant(&poster_jpeg(200, 300, 4), 1);
    lib.put_bytes("Good (2001)/Good (2001).mkv", b"g");
    lib.put_bytes("Good (2001)/Good (2001)-poster.jpg", &good);
    lib.put_bytes("Broken (2002)/Broken (2002).mkv", b"b");
    lib.put_bytes("Broken (2002)/Broken (2002)-poster.jpg", b"not an image");
    lib.put_bytes("Gone (2003)/Gone (2003).mkv", b"x");
    let gone = lib.put_bytes("Gone (2003)/Gone (2003)-poster.jpg", b"will vanish");
    lib.rescan().job.wait().await;
    std::fs::remove_file(gone).unwrap();

    let targets = thumb_targets(&lib.db, &lib.item_ids()).unwrap();
    assert_eq!(targets.len(), 3);
    let job = submit_thumb_job(&lib.manager, Arc::clone(&lib.cache), targets, 2)
        .unwrap()
        .unwrap();
    let report = job.job.wait().await;

    assert_eq!(report.snapshot.state, JobState::Completed);
    assert_eq!((report.snapshot.done, report.snapshot.failed), (3, 2));
    assert!(built(&lib.cache, &good));
    let mut reasons: Vec<String> = report
        .failure_groups()
        .into_iter()
        .map(|g| g.reason)
        .collect();
    reasons.sort();
    assert_eq!(
        reasons,
        ["image file missing", "unsupported or damaged image"]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_interrupted_build_resumes_and_reuses_what_is_cached() {
    use cv_db::repo::jobs::{NewJob, NewJobItem, insert_job};

    let lib = Lib::movies();
    let base = poster_jpeg(200, 300, 5);
    let mut posters = Vec::new();
    for n in 0..6 {
        let bytes = variant(&base, n);
        let name = format!("Film {n} (200{n})");
        lib.put_bytes(&format!("{name}/{name}.mkv"), b"v");
        lib.put_bytes(&format!("{name}/{name}-poster.jpg"), &bytes);
        posters.push(bytes);
    }
    lib.rescan().job.wait().await;
    let targets = thumb_targets(&lib.db, &lib.item_ids()).unwrap();
    assert_eq!(targets.len(), 6);

    // The state a quit leaves: a stored job whose items never ran, three of them already cached.
    for bytes in &posters[..3] {
        lib.cache.ensure_bytes(bytes, &BUILD_SIZES).unwrap();
    }
    let items: Vec<NewJobItem> = targets
        .iter()
        .map(|t: &ThumbTarget| NewJobItem {
            label: t.to_string(),
            payload: serde_json::to_string(t).unwrap(),
        })
        .collect();
    let payload = serde_json::json!({ "concurrency": 2 }).to_string();
    lib.db
        .write(move |tx| {
            let job = NewJob {
                kind: THUMB_JOB_KIND,
                name: "Preparing 3 thumbnails",
                lane: "cpu",
                priority: -1,
                payload: &payload,
            };
            insert_job(tx, &job, &items)
        })
        .unwrap();
    let before = lib.cache.generations();

    let resumed = resume_thumb_jobs(&lib.manager, &lib.cache).unwrap();
    assert_eq!(resumed.len(), 1);
    let report = resumed[0].job.wait().await;
    assert_eq!(report.snapshot.state, JobState::Completed);
    assert_eq!(report.snapshot.total, 6);
    assert_eq!(resumed[0].stats().generated, 3);
    assert_eq!(resumed[0].stats().reused, 3);
    assert_eq!(lib.cache.generations() - before, 3);
    assert!(posters.iter().all(|b| built(&lib.cache, b)));

    // Nothing is left to resume.
    assert!(
        resume_thumb_jobs(&lib.manager, &lib.cache)
            .unwrap()
            .is_empty()
    );
}
