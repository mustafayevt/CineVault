//! The command that pauses or resumes one job.
//!
//! Live job progress comes over the channel in [`crate::jobs_ipc`].

use std::sync::Arc;

use cv_app::activity::ActivityService;
use cv_jobs::JobId;
use tauri::State;

use crate::boot::Boot;

/// Held in Tauri state. Without app services there are no jobs to pause.
pub struct ActivityState {
    activity: Option<Arc<ActivityService>>,
}

impl ActivityState {
    pub fn from_boot(boot: &Boot) -> Self {
        Self {
            activity: boot.services().map(|s| Arc::clone(s.activity())),
        }
    }
}

/// Pauses or resumes one job. Returns false when the job is unknown or already over.
#[tauri::command]
#[specta::specta]
pub fn set_job_paused(state: State<'_, ActivityState>, id: u32, paused: bool) -> bool {
    state
        .activity
        .as_ref()
        .is_some_and(|activity| activity.set_job_paused(JobId(u64::from(id)), paused))
}
