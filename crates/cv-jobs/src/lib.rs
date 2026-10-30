//! Background job lanes, item pipelines, progress and persistence.
//!
//! Work is scheduled by the resource it uses rather than through one global queue:
//! a **disk** lane, a **CPU** lane, and one lane **per network provider**. Each lane is a fair
//! (FIFO) semaphore with a configurable width, so a slow or blocked provider can never delay
//! scanning, media info or another provider.
//!
//! Each item moves through the ordered [`Pipeline`] (scan → mediainfo → match → assets → nfo →
//! rename) while different items proceed independently.
//!
//! A [`JobManager`] wraps a list of items into a *job* with states, progress and ETA, cancel
//! (bounded, with per-item completion or rollback), pause/resume and priority.
//!
//! Per-item failures never stop a job. They are grouped by reason ([`FailureGroup`]) and
//! [`JobHandle::retry_failed`] re-queues only the failed items as a new job.
//!
//! [`JobManager::progress_feed`] turns job changes into reports of at most 10 Hz per job plus one
//! combined figure for the taskbar and dock.
//!
//! With a [`JobStore`], jobs are persistent: they and their items are saved in the library
//! database, and [`JobManager::resume_persisted`] finishes the unfinished ones after a restart.

pub mod client;
mod feed;
mod lane;
mod manager;
mod pipeline;
mod progress;
mod runtime;
mod store;

pub use feed::{
    Overall, OverallKind, OverallTracker, PROGRESS_INTERVAL, ProgressFeed, ProgressTick,
};
pub use lane::{LaneConfig, LaneId, LaneSnapshot, Lanes};
pub use manager::{
    ItemControl, ItemError, ItemWork, JobHandle, JobManager, JobSpec, KEEP_FINISHED_JOBS,
    ManagerConfig,
};
pub use pipeline::{
    AssetKind, ItemOutcome, Pipeline, PipelineConfig, Stage, StageError, StageFailure, StageLanes,
    StageRunner, Step,
};
pub use progress::{
    FailedItem, FailureGroup, ItemRecord, ItemStatus, JobId, JobReport, JobSnapshot, JobState,
    Priority, group_failures,
};
pub use runtime::{runtime_handle, set_fallback_runtime, spawn};
pub use store::{JobItemCounts, JobStore, StoreError, StoredJob, StoredJobItem};
