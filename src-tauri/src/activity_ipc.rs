//! The command that pauses or resumes one job.
//!
//! Live job progress comes over the channel in [`crate::jobs_ipc`].

use std::sync::Arc;

use cv_app::activity::ActivityService;
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

    /// The activity service, for the command in `cv-ipc`.
    pub fn activity(&self) -> Option<&Arc<ActivityService>> {
        self.activity.as_ref()
    }
}

/// Pauses or resumes one job. Returns false when the job is unknown or already over.
#[tauri::command]
#[specta::specta]
pub fn set_job_paused(state: State<'_, ActivityState>, id: u32, paused: bool) -> bool {
    cv_ipc::jobs::set_job_paused(state.activity(), id, paused)
}
