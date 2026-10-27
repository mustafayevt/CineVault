//! Automation: running batches without the UI.
//!
//! The command-line program and the app's selection actions run the same batch steps (update,
//! scrape, media info, rename, NFO ...) over the same services. This module holds what they
//! share:
//!
//! * [`model`]: the request ([`BatchRequest`]) and the answer ([`BatchReport`]), both plain JSON;
//! * [`query`]: the quick-search grammar, so `--query` selects what the search box would show;
//! * [`selection`]: titles by data source, folder or query;
//! * [`engine`]: the steps themselves, run from a [`Plan`] in the order the plan lists them, with
//!   dry run;
//! * [`backend`]: the queue that runs plans one after another, each as a persistent job;
//! * [`item_action`]: the actions the app runs on a selection of titles, each one step of the
//!   engine, queued as a job by that backend;
//! * [`ipc`]: the local channel by which a running app executes a command-line batch, so the
//!   library keeps a single writer.

pub mod backend;
pub mod engine;
pub mod ipc;
pub mod item_action;
pub mod model;
pub mod query;
pub mod selection;

pub use backend::{BatchRun, CommandBackend, SubmitError, Submitted};
pub use engine::{BatchEngine, EngineOptions, Progress, ProgressSink, credential_summaries};
pub use item_action::{ItemAction, ItemActionError, ItemActionOutcome, run_item_action};
pub use model::{
    BatchReport, BatchRequest, CredentialSummary, EntityKind, ItemResult, Plan, PlannedStep, Scope,
    SelectionSpec, SelectionSummary, Step, StepOptions, StepReport, StepStatus, Steps,
};
