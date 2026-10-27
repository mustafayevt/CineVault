//! Pipeline behaviour: per-item ordering across many items, independence between items,
//! the NFO-after-scrape scenario and failure handling.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cv_jobs::{
    AssetKind, LaneConfig, Lanes, Pipeline, PipelineConfig, Stage, StageError, StageRunner, Step,
};
use tokio::time::{sleep, timeout};

const LIMIT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Edge {
    Start,
    End,
}

type Event = (usize, u32, Stage, Edge);

/// Records a global, totally ordered event log.
struct Recorder {
    seq: AtomicUsize,
    log: Mutex<Vec<Event>>,
    fail: Mutex<HashMap<(u32, Stage), &'static str>>,
}

impl Recorder {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            seq: AtomicUsize::new(0),
            log: Mutex::new(Vec::new()),
            fail: Mutex::new(HashMap::new()),
        })
    }

    fn push(&self, item: u32, stage: Stage, edge: Edge) {
        let n = self.seq.fetch_add(1, Ordering::SeqCst);
        self.log.lock().unwrap().push((n, item, stage, edge));
    }

    fn find(&self, item: u32, stage: Stage, edge: Edge) -> Option<usize> {
        self.log
            .lock()
            .unwrap()
            .iter()
            .find(|e| e.1 == item && e.2 == stage && e.3 == edge)
            .map(|e| e.0)
    }

    fn events_for(&self, item: u32) -> Vec<(Stage, Edge)> {
        let mut events: Vec<Event> = self
            .log
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.1 == item)
            .cloned()
            .collect();
        events.sort_by_key(|e| e.0);
        events.into_iter().map(|e| (e.2, e.3)).collect()
    }
}

struct Runner {
    rec: Arc<Recorder>,
    /// Delay in ms for (item, stage); 0 only yields.
    delay: fn(u32, Stage) -> u64,
}

impl StageRunner for Runner {
    type Item = u32;

    async fn run(&self, item: &u32, stage: Stage) -> Result<(), StageError> {
        self.rec.push(*item, stage, Edge::Start);
        let ms = (self.delay)(*item, stage);
        if ms > 0 {
            sleep(Duration::from_millis(ms)).await;
        } else {
            tokio::task::yield_now().await;
        }
        let failure = self.rec.fail.lock().unwrap().get(&(*item, stage)).copied();
        self.rec.push(*item, stage, Edge::End);
        match failure {
            Some(reason) => Err(StageError::new(reason)),
            None => Ok(()),
        }
    }
}

/// Deterministic pseudo-random jitter so items overtake each other.
fn jitter(item: u32, stage: Stage) -> u64 {
    let salt: u32 = match stage {
        Stage::Scan => 1,
        Stage::MediaInfo => 2,
        Stage::Match => 3,
        Stage::Asset(AssetKind::Artwork) => 4,
        Stage::Nfo => 5,
        Stage::Rename => 6,
    };
    u64::from((item.wrapping_mul(2_654_435_761).wrapping_add(salt * 40_503) >> 7) % 4)
}

fn lanes() -> Lanes {
    Lanes::new(LaneConfig {
        disk: 3,
        cpu: 3,
        provider_default: 5,
        ..LaneConfig::default()
    })
}

fn step_index(stage: Stage) -> usize {
    Step::ORDER
        .iter()
        .position(|s| *s == stage.step())
        .expect("step in order")
}

#[tokio::test]
async fn hundred_items_keep_their_step_order_and_interleave() {
    let rec = Recorder::new();
    let pipeline = Pipeline::new(
        lanes(),
        Runner {
            rec: rec.clone(),
            delay: jitter,
        },
        PipelineConfig::default(),
    );
    let items: Vec<u32> = (0..100).collect();
    let outcomes = timeout(LIMIT, pipeline.run_all(items.clone()))
        .await
        .expect("pipeline finished");

    assert_eq!(outcomes.len(), 100);
    for (outcome, item) in outcomes.iter().zip(&items) {
        assert_eq!(outcome.item, *item, "outcomes keep submission order");
        assert!(outcome.finished, "item {item} finished");
        assert!(outcome.failures.is_empty());
    }

    for item in &items {
        let events = rec.events_for(*item);
        // 6 stages, each with a start and an end.
        assert_eq!(events.len(), 12, "item {item} ran every stage once");

        let mut last_end_of_step = [None::<usize>; 6];
        let mut first_start_of_step = [None::<usize>; 6];
        for (pos, (stage, edge)) in events.iter().enumerate() {
            let idx = step_index(*stage);
            match edge {
                Edge::Start => {
                    first_start_of_step[idx].get_or_insert(pos);
                }
                Edge::End => last_end_of_step[idx] = Some(pos),
            }
        }
        // No stage of a later step starts before every stage of the earlier step ended.
        for idx in 1..6 {
            let prev_end = last_end_of_step[idx - 1].expect("previous step ended");
            let start = first_start_of_step[idx].expect("step started");
            assert!(
                prev_end < start,
                "item {item}: {:?} started before {:?} finished",
                Step::ORDER[idx],
                Step::ORDER[idx - 1]
            );
        }
        // A stage ends only after it started.
        for (pos, (stage, edge)) in events.iter().enumerate() {
            if *edge == Edge::End {
                assert!(
                    events[..pos].contains(&(*stage, Edge::Start)),
                    "item {item}: {stage} ended before it started"
                );
            }
        }
    }

    // Items are independent: they finish out of submission order, and item 99 starts before
    // item 0 is done.
    let finish_order: Vec<u32> = {
        let log = rec.log.lock().unwrap();
        let mut done: Vec<(usize, u32)> = log
            .iter()
            .filter(|e| e.2 == Stage::Rename && e.3 == Edge::End)
            .map(|e| (e.0, e.1))
            .collect();
        done.sort();
        done.into_iter().map(|d| d.1).collect()
    };
    assert_eq!(finish_order.len(), 100);
    assert_ne!(
        finish_order, items,
        "items overtake each other instead of running one by one"
    );
    let scan_of_last = rec.find(99, Stage::Scan, Edge::Start).unwrap();
    let last_of_first = rec.find(0, Stage::Rename, Edge::End).unwrap();
    assert!(
        scan_of_last < last_of_first,
        "item 99 starts before item 0 is done"
    );
}

/// The spec scenario: artwork is still downloading when the scrape is saved; the NFO waits for
/// the scrape and the assets, and the rename waits for the NFO.
#[tokio::test]
async fn nfo_is_written_after_scrape_and_before_rename_while_artwork_downloads() {
    fn slow_artwork(_: u32, stage: Stage) -> u64 {
        match stage {
            Stage::Asset(AssetKind::Artwork) => 80,
            _ => 0,
        }
    }
    let rec = Recorder::new();
    let pipeline = Pipeline::new(
        lanes(),
        Runner {
            rec: rec.clone(),
            delay: slow_artwork,
        },
        PipelineConfig::default(),
    );
    let outcome = pipeline.run_item(7).await;
    assert!(outcome.finished);

    let events = rec.events_for(7);
    let pos = |stage: Stage, edge: Edge| events.iter().position(|e| *e == (stage, edge)).unwrap();
    let artwork = Stage::Asset(AssetKind::Artwork);
    assert!(pos(Stage::Match, Edge::End) < pos(Stage::Nfo, Edge::Start));
    assert!(pos(artwork, Edge::End) < pos(Stage::Nfo, Edge::Start));
    assert!(pos(Stage::Nfo, Edge::End) < pos(Stage::Rename, Edge::Start));
}

#[tokio::test]
async fn a_failing_item_stops_alone_and_an_asset_failure_does_not_block_the_nfo() {
    let rec = Recorder::new();
    {
        let mut fail = rec.fail.lock().unwrap();
        fail.insert((3, Stage::Match), "no match");
        fail.insert((5, Stage::Asset(AssetKind::Artwork)), "rate limit");
        fail.insert((8, Stage::Nfo), "disk full");
    }
    let pipeline = Pipeline::new(
        lanes(),
        Runner {
            rec: rec.clone(),
            delay: jitter,
        },
        PipelineConfig::default(),
    );
    let outcomes = pipeline.run_all((0..10).collect()).await;

    let finished: Vec<u32> = outcomes
        .iter()
        .filter(|o| o.finished)
        .map(|o| o.item)
        .collect();
    assert_eq!(finished, vec![0, 1, 2, 4, 5, 6, 7, 9]);

    let three = &outcomes[3];
    assert_eq!(three.failures.len(), 1);
    assert_eq!(three.failures[0].stage, Stage::Match);
    assert_eq!(three.failures[0].error.reason, "no match");
    assert!(
        rec.events_for(3)
            .iter()
            .all(|(s, _)| step_index(*s) <= step_index(Stage::Match)),
        "nothing runs after a failed match"
    );

    let five = &outcomes[5];
    assert_eq!(five.failures[0].stage, Stage::Asset(AssetKind::Artwork));
    assert!(five.completed.contains(&Stage::Nfo));
    assert!(five.completed.contains(&Stage::Rename));

    let eight = &outcomes[8];
    assert!(!eight.finished);
    assert!(
        rec.events_for(8).iter().all(|(s, _)| *s != Stage::Rename),
        "rename never runs after a failed NFO"
    );
}

#[tokio::test]
async fn empty_asset_list_skips_the_assets_step() {
    let rec = Recorder::new();
    let pipeline = Pipeline::new(
        lanes(),
        Runner {
            rec: rec.clone(),
            delay: |_, _| 0,
        },
        PipelineConfig {
            assets: Vec::new(),
            ..PipelineConfig::default()
        },
    );
    let outcome = pipeline.run_item(1).await;
    assert!(outcome.finished);
    assert_eq!(
        outcome.completed,
        vec![
            Stage::Scan,
            Stage::MediaInfo,
            Stage::Match,
            Stage::Nfo,
            Stage::Rename
        ]
    );
}
