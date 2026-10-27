//! Per-item pipelines: one item's work keeps a fixed logical order while different items
//! proceed independently.
//!
//! The order for one item is
//! `scan → mediainfo → match → assets → nfo → rename`, where the **assets** step
//! fetches the artwork, which must finish before the NFO is written. Every stage runs on the lane of the resource it uses ([`StageLanes`]), so
//! a stalled provider only holds back the items waiting for that provider.
//!
//! Failure rules, per item (other items are never affected):
//! - a failure in scan, mediainfo or match stops the item, since nothing downstream has data to
//!   work on;
//! - a failure in one asset kind is recorded and the item continues, because the NFO only needs
//!   the scraped data and a missing picture must not block renaming;
//! - a failure in nfo or rename stops the item, so a rename never runs after a
//!   failed NFO write.

use std::fmt;
use std::future::Future;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::lane::{LaneId, Lanes};

/// The six ordered steps of an item pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    /// Identify the item's files on disk.
    Scan,
    /// Read technical media information.
    MediaInfo,
    /// Match and scrape metadata from the providers.
    Match,
    /// Artwork.
    Assets,
    /// Write the NFO file.
    Nfo,
    /// Rename or move the item's folder and files.
    Rename,
}

impl Step {
    /// All steps in execution order.
    pub const ORDER: [Step; 6] = [
        Step::Scan,
        Step::MediaInfo,
        Step::Match,
        Step::Assets,
        Step::Nfo,
        Step::Rename,
    ];
}

/// One kind of asset fetched in the assets step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    /// Posters, fanart and other images.
    Artwork,
}

impl AssetKind {
    /// All asset kinds.
    pub const ALL: [AssetKind; 1] = [AssetKind::Artwork];
}

/// A unit of work handed to the [`StageRunner`]: a step, or one asset kind of the assets step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Stage {
    /// Scan step.
    Scan,
    /// Media info step.
    MediaInfo,
    /// Match and scrape step.
    Match,
    /// One part of the assets step.
    Asset(AssetKind),
    /// NFO step.
    Nfo,
    /// Rename step.
    Rename,
}

impl Stage {
    /// The pipeline step this stage belongs to.
    pub fn step(self) -> Step {
        match self {
            Stage::Scan => Step::Scan,
            Stage::MediaInfo => Step::MediaInfo,
            Stage::Match => Step::Match,
            Stage::Asset(_) => Step::Assets,
            Stage::Nfo => Step::Nfo,
            Stage::Rename => Step::Rename,
        }
    }
}

impl fmt::Display for Stage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Stage::Scan => f.write_str("scan"),
            Stage::MediaInfo => f.write_str("mediainfo"),
            Stage::Match => f.write_str("match"),
            Stage::Asset(AssetKind::Artwork) => f.write_str("artwork"),
            Stage::Nfo => f.write_str("nfo"),
            Stage::Rename => f.write_str("rename"),
        }
    }
}

/// Which lane each stage runs on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StageLanes {
    /// Lane for the match/scrape stage (the metadata provider).
    pub match_lane: LaneId,
    /// Lane for artwork downloads.
    pub artwork: LaneId,
}

impl Default for StageLanes {
    fn default() -> Self {
        Self {
            match_lane: LaneId::provider("tmdb"),
            artwork: LaneId::provider("fanart"),
        }
    }
}

impl StageLanes {
    /// Lane that runs `stage`: disk for scan, NFO and rename, CPU for media info,
    /// and the configured provider lanes for match and assets.
    pub fn lane_for(&self, stage: Stage) -> LaneId {
        match stage {
            Stage::Scan | Stage::Nfo | Stage::Rename => LaneId::Disk,
            Stage::MediaInfo => LaneId::Cpu,
            Stage::Match => self.match_lane.clone(),
            Stage::Asset(AssetKind::Artwork) => self.artwork.clone(),
        }
    }
}

/// Pipeline settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PipelineConfig {
    /// Lane per stage.
    pub lanes: StageLanes,
    /// Asset kinds fetched in the assets step. When empty the assets step is skipped.
    pub assets: Vec<AssetKind>,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            lanes: StageLanes::default(),
            assets: AssetKind::ALL.to_vec(),
        }
    }
}

/// Why a stage failed. The reason groups failures in the Activity drawer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StageError {
    /// Short, groupable reason such as `rate limit` or `no match`.
    pub reason: String,
}

impl StageError {
    /// Creates an error with the given reason.
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}

impl fmt::Display for StageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.reason)
    }
}

impl std::error::Error for StageError {}

/// Performs the real work of each stage for one item.
pub trait StageRunner: Send + Sync + 'static {
    /// Identifies an item.
    type Item: Clone + Send + Sync + 'static;

    /// Runs `stage` for `item`. Called while a worker of the stage's lane is held.
    fn run(
        &self,
        item: &Self::Item,
        stage: Stage,
    ) -> impl Future<Output = Result<(), StageError>> + Send;
}

/// A failed stage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StageFailure {
    /// The stage that failed.
    pub stage: Stage,
    /// Why.
    pub error: StageError,
}

/// How one item finished.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemOutcome<I> {
    /// The item.
    pub item: I,
    /// Stages that completed, in the order they finished.
    pub completed: Vec<Stage>,
    /// Stages that failed. The last one stopped the item unless it is an asset stage.
    pub failures: Vec<StageFailure>,
    /// True when the item ran through the rename step without a fatal failure.
    pub finished: bool,
}

/// Runs items through the pipeline.
pub struct Pipeline<R: StageRunner> {
    lanes: Lanes,
    runner: Arc<R>,
    config: PipelineConfig,
}

impl<R: StageRunner> Pipeline<R> {
    /// Creates a pipeline that schedules stages on `lanes`.
    pub fn new(lanes: Lanes, runner: R, config: PipelineConfig) -> Self {
        Self {
            lanes,
            runner: Arc::new(runner),
            config,
        }
    }

    /// Runs one item through every step in order and reports how it ended.
    pub async fn run_item(&self, item: R::Item) -> ItemOutcome<R::Item> {
        run_item(
            self.lanes.clone(),
            Arc::clone(&self.runner),
            self.config.clone(),
            item,
        )
        .await
    }

    /// Runs all items concurrently, each through its own ordered pipeline. Items are submitted
    /// in the given order and the outcomes are returned in that order.
    pub async fn run_all(&self, items: Vec<R::Item>) -> Vec<ItemOutcome<R::Item>> {
        let handles: Vec<_> = items
            .into_iter()
            .map(|item| {
                tokio::spawn(run_item(
                    self.lanes.clone(),
                    Arc::clone(&self.runner),
                    self.config.clone(),
                    item,
                ))
            })
            .collect();
        let mut outcomes = Vec::with_capacity(handles.len());
        for handle in handles {
            match handle.await {
                Ok(outcome) => outcomes.push(outcome),
                // A panicking runner is a bug; surface it instead of hiding the item.
                Err(err) => std::panic::resume_unwind(err.into_panic()),
            }
        }
        outcomes
    }
}

async fn run_stage<R: StageRunner>(
    lanes: &Lanes,
    runner: &Arc<R>,
    config: &PipelineConfig,
    item: &R::Item,
    stage: Stage,
) -> Result<(), StageError> {
    let lane = config.lanes.lane_for(stage);
    lanes.run(&lane, runner.run(item, stage)).await
}

async fn run_item<R: StageRunner>(
    lanes: Lanes,
    runner: Arc<R>,
    config: PipelineConfig,
    item: R::Item,
) -> ItemOutcome<R::Item> {
    let mut outcome = ItemOutcome {
        item: item.clone(),
        completed: Vec::new(),
        failures: Vec::new(),
        finished: false,
    };

    for step in Step::ORDER {
        let stage = match step {
            Step::Assets => {
                run_assets(&lanes, &runner, &config, &item, &mut outcome).await;
                continue;
            }
            Step::Scan => Stage::Scan,
            Step::MediaInfo => Stage::MediaInfo,
            Step::Match => Stage::Match,
            Step::Nfo => Stage::Nfo,
            Step::Rename => Stage::Rename,
        };
        match run_stage(&lanes, &runner, &config, &item, stage).await {
            Ok(()) => outcome.completed.push(stage),
            Err(error) => {
                outcome.failures.push(StageFailure { stage, error });
                return outcome;
            }
        }
    }
    outcome.finished = true;
    outcome
}

/// Runs the configured asset kinds side by side and waits for all of them.
async fn run_assets<R: StageRunner>(
    lanes: &Lanes,
    runner: &Arc<R>,
    config: &PipelineConfig,
    item: &R::Item,
    outcome: &mut ItemOutcome<R::Item>,
) {
    let mut kinds = config.assets.clone();
    kinds.sort();
    kinds.dedup();
    let handles: Vec<_> = kinds
        .into_iter()
        .map(|kind| {
            let stage = Stage::Asset(kind);
            let lanes = lanes.clone();
            let runner = Arc::clone(runner);
            let config = config.clone();
            let item = item.clone();
            let handle =
                tokio::spawn(
                    async move { run_stage(&lanes, &runner, &config, &item, stage).await },
                );
            (stage, handle)
        })
        .collect();
    for (stage, handle) in handles {
        match handle.await {
            Ok(Ok(())) => outcome.completed.push(stage),
            Ok(Err(error)) => outcome.failures.push(StageFailure { stage, error }),
            Err(err) => std::panic::resume_unwind(err.into_panic()),
        }
    }
}
