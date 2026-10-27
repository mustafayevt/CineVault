//! What the Activity drawer shows besides the live job progress.
//!
//! Job progress itself streams over its own channel (`cv_jobs::ProgressFeed`). This service adds
//! the rest of the drawer, read as one snapshot:
//!
//! * **lanes**: the disk and CPU lanes and one lane per provider, with their workers, what is
//!   queued, and, for a provider that answered HTTP 429, how much longer the lane is paused (the
//!   HTTP client's throttle state, see `HttpClient::lane_status`);
//! * **failures**: the failed items of finished jobs grouped by reason, each group with the
//!   retry from `JobHandle::retry_failed`. A job whose failures were retried is not listed again.
//!
//! Every key is the user's own, so a throttled provider links to its card on the Providers page
//! (the drawer builds the link from the provider id of the lane). No key value is part of the
//! snapshot.

use std::collections::BTreeSet;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use cv_jobs::{JobId, JobManager, JobSnapshot, LaneId};
use cv_providers::{CredentialResolver, HttpClient};

/// How many failed item labels a group lists as examples.
pub const SAMPLE_LABELS: usize = 3;

/// One lane of the drawer.
#[derive(Debug, Clone, PartialEq)]
pub struct LaneActivity {
    /// `disk`, `cpu` or `provider:<id>`.
    pub id: String,
    /// The provider id for a provider lane.
    pub provider: Option<String>,
    /// The provider's display name for a provider lane (`TMDB`).
    pub display_name: Option<String>,
    /// Configured workers.
    pub width: usize,
    /// Tasks holding a worker.
    pub running: usize,
    /// Tasks waiting for a worker.
    pub queued: usize,
    /// How much longer the provider's requests are paused after a 429.
    pub throttled_for: Option<Duration>,
}

/// The failed items of one finished job that share a reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailureActivity {
    pub job_id: u64,
    pub job_name: String,
    /// The job's lane.
    pub lane: String,
    /// The shared reason, for example `rate limit`.
    pub reason: String,
    pub count: usize,
    /// A few labels of the failed items.
    pub samples: Vec<String>,
}

/// Everything the drawer reads in one go.
#[derive(Debug, Clone, PartialEq)]
pub struct ActivitySnapshot {
    pub lanes: Vec<LaneActivity>,
    pub failures: Vec<FailureActivity>,
}

/// Reads the drawer's data from the job manager, and the HTTP client.
pub struct ActivityService {
    jobs: JobManager,
    http: Option<HttpClient>,
    resolver: CredentialResolver,
    /// Jobs whose failures were queued again.
    retried: Mutex<BTreeSet<u64>>,
}

impl ActivityService {
    #[must_use]
    pub fn new(jobs: JobManager, http: Option<HttpClient>, resolver: CredentialResolver) -> Self {
        Self {
            jobs,
            http,
            resolver,
            retried: Mutex::new(BTreeSet::new()),
        }
    }

    fn lane(&self, snapshot: &cv_jobs::LaneSnapshot) -> LaneActivity {
        let provider = match &snapshot.lane {
            LaneId::Provider(id) => Some(id.clone()),
            LaneId::Disk | LaneId::Cpu => None,
        };
        let throttled_for = provider.as_deref().and_then(|id| {
            self.http
                .as_ref()
                .and_then(|http| http.lane_status(id).throttled_for)
        });
        LaneActivity {
            id: snapshot.lane.to_string(),
            display_name: provider.as_deref().map(|id| {
                self.resolver
                    .schema(id)
                    .map_or_else(|_| id.to_owned(), |schema| schema.display_name.to_owned())
            }),
            provider,
            width: snapshot.width,
            running: snapshot.running,
            queued: snapshot.queued,
            throttled_for,
        }
    }

    fn failures_of(&self, job: &JobSnapshot) -> Vec<FailureActivity> {
        if !job.state.is_terminal() || job.failed == 0 {
            return Vec::new();
        }
        if self
            .retried
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(&job.id.0)
        {
            return Vec::new();
        }
        let Some(handle) = self.jobs.get(job.id) else {
            return Vec::new();
        };
        handle
            .failure_groups()
            .into_iter()
            .map(|group| FailureActivity {
                job_id: job.id.0,
                job_name: job.name.clone(),
                lane: job.lane.to_string(),
                count: group.count(),
                samples: group
                    .items
                    .iter()
                    .take(SAMPLE_LABELS)
                    .map(|item| item.label.clone())
                    .collect(),
                reason: group.reason,
            })
            .collect()
    }

    /// The lanes and failures as they are now.
    #[must_use]
    pub fn snapshot(&self) -> ActivitySnapshot {
        let lanes = self
            .jobs
            .lanes()
            .snapshot()
            .iter()
            .map(|lane| self.lane(lane))
            .collect();
        let failures = self
            .jobs
            .list()
            .iter()
            .flat_map(|job| self.failures_of(job))
            .collect();
        ActivitySnapshot { lanes, failures }
    }

    /// Queues the failed items of a finished job again, as the job's retry. Returns the id of
    /// the new job, or `None` when the job is unknown, still going, or nothing failed.
    pub fn retry_failed(&self, job: JobId) -> Option<JobId> {
        let retry = self.jobs.get(job)?.retry_failed()?;
        self.retried
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(job.0);
        Some(retry.id())
    }

    /// Pauses or resumes one job. Returns false when the job is unknown or already over.
    pub fn set_job_paused(&self, job: JobId, paused: bool) -> bool {
        if paused {
            self.jobs.pause(job)
        } else {
            self.jobs.resume(job)
        }
    }
}
