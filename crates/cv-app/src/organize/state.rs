//! The organize-state cache: which titles already have the names of the active naming profile.
//!
//! [`OrganizeStateService`] keeps the `item_organize_state` table (see
//! `cv_db::repo::organize_state`) current. A job on the CPU lane runs the renamer planner as a dry
//! run for a list of titles under the active profile and stores each title as tidy, pending,
//! conflict (with the reason), locked or unplanned. Nothing touches the disk.
//!
//! The cache is recomputed
//!
//! * after a scan ([`OrganizeStateService::refresh_missing`]: titles with no state for the
//!   active profile),
//! * when the profile changes (the key of the profile changes, so every row is stale and
//!   [`OrganizeStateService::refresh_missing`] picks them all up),
//! * after each scrape and after every organize apply or undo
//!   ([`OrganizeStateService::note_changed`], which collects the titles of a burst and queues one
//!   job for them).
//!
//! A title waiting for its state reads as "checking" (no current row). Titles are planned in
//! chunks, so a collision between two titles that land in different chunks of a very large
//! library is only found when the preview plans them together.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use cv_db::Database;
use cv_db::repo::organize_state::{
    clear_organize_states, organize_states_for, set_organize_states, stale_items,
};
use cv_domain::ItemId;
use cv_jobs::{ItemControl, ItemError, ItemWork, JobHandle, JobManager, JobSpec, LaneId};
use cv_jobs::{Priority, StageError};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{OrganizeError, OrganizeService, PreviewRequest, Scope};
use crate::app_settings::SettingsService;
use crate::changes::{ChangePart, Changes};
use crate::rename_settings::active_profile_from_values;
use cv_rename::RenameProfile;

/// Titles planned together in one step of the job.
const CHUNK: usize = 200;

/// How long titles that changed are collected before the job for them is queued, so a batch
/// scrape of hundreds of titles queues a handful of jobs, not hundreds.
const COLLECT: Duration = Duration::from_millis(750);

/// The name of the job in Activity ("Checking names of 12 titles").
pub const JOB_NAME: &str = crate::wording::job::CHECKING_NAMES;

/// A key that changes whenever the naming profile does. Rows computed for another key are stale.
#[must_use]
pub fn profile_key(profile: &RenameProfile) -> String {
    let digest = Sha256::digest(format!("{profile:?}").as_bytes());
    digest.iter().take(8).fold(String::new(), |mut key, byte| {
        use std::fmt::Write;
        let _ = write!(key, "{byte:02x}");
        key
    })
}

/// The profile Organize plans with, from the settings values.
#[must_use]
pub fn current_profile(values: &BTreeMap<String, Value>) -> RenameProfile {
    active_profile_from_values(values)
}

/// A list of titles one step of the job plans together.
#[derive(Debug, Clone)]
pub struct StateChunk {
    ids: Vec<ItemId>,
    label: String,
}

impl fmt::Display for StateChunk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

fn chunks_of(ids: &[ItemId]) -> Vec<StateChunk> {
    ids.chunks(CHUNK)
        .enumerate()
        .map(|(index, ids)| {
            let first = index * CHUNK + 1;
            StateChunk {
                ids: ids.to_vec(),
                label: format!("Titles {first}-{}", first + ids.len() - 1),
            }
        })
        .collect()
}

/// Keeps the organize-state cache current.
pub struct OrganizeStateService {
    db: Arc<Database>,
    settings: Arc<SettingsService>,
    jobs: JobManager,
    organize: OrganizeService,
    /// Titles that changed and wait for the next job.
    waiting: Mutex<BTreeSet<ItemId>>,
    /// Whether a task is already waiting to queue the job for `waiting`.
    collecting: AtomicBool,
    /// The key of the profile the last settings change left, to see a profile change.
    last_key: Mutex<String>,
    /// Announces the titles whose folders were renamed or whose check came in.
    changes: Changes,
}

impl OrganizeStateService {
    #[must_use]
    pub fn new(db: Arc<Database>, settings: Arc<SettingsService>, jobs: JobManager) -> Arc<Self> {
        let key = profile_key(&current_profile(&settings.snapshot().values));
        Arc::new(Self {
            organize: OrganizeService::new(Arc::clone(&db)),
            db,
            settings,
            jobs,
            waiting: Mutex::new(BTreeSet::new()),
            collecting: AtomicBool::new(false),
            last_key: Mutex::new(key),
            changes: Changes::default(),
        })
    }

    /// Connects the service to the hub that announces what changed to open pages.
    pub fn set_changes(&self, hub: Arc<crate::changes::ChangeHub>) {
        self.changes.set(hub);
    }

    /// The key of the profile that is active now.
    #[must_use]
    pub fn current_key(&self) -> String {
        profile_key(&current_profile(&self.settings.snapshot().values))
    }

    /// Computes and stores the states of `ids` for the active profile, on the calling thread.
    /// The job calls this; so can a test.
    ///
    /// # Errors
    /// The library cannot be read or written.
    pub fn recompute_blocking(&self, ids: &[ItemId]) -> Result<usize, OrganizeError> {
        if ids.is_empty() {
            return Ok(0);
        }
        let profile = current_profile(&self.settings.snapshot().values);
        let key = profile_key(&profile);
        let request = PreviewRequest {
            scope: Scope::Items(ids.to_vec()),
            profile,
            choices: BTreeMap::new(),
        };
        let states = self.organize.item_states(&request)?;
        let count = states.len();
        let items: Vec<ItemId> = states.iter().map(|s| s.item).collect();
        let at = crate::step_outcomes::now_ms();
        self.db
            .write(move |tx| set_organize_states(tx, &states, &key, at))?;
        // "Checking" becomes tidy, pending, conflict or locked.
        self.changes.publish(&items, &[ChangePart::Stage]);
        Ok(count)
    }

    /// The Organize preview of `request`. When it plans with the active profile and no conflict
    /// choices, the states it found replace the cached ones that differ: the preview plans the
    /// titles together and so sees collisions a recomputation of one changed title cannot, and the
    /// sidebar badge and the pipeline counts then agree with the Organize tabs.
    ///
    /// # Errors
    /// The library cannot be read, or an edited name is empty once cleaned.
    pub fn preview(&self, request: &PreviewRequest) -> Result<super::Preview, OrganizeError> {
        if !request.choices.is_empty() {
            return self.organize.preview(request);
        }
        let key = profile_key(&request.profile);
        if key != self.current_key() {
            return self.organize.preview(request);
        }
        let (preview, states) = self.organize.preview_with_states(request)?;
        let ids: Vec<ItemId> = states.iter().map(|s| s.item).collect();
        let stored = self.db.read(move |c| organize_states_for(c, &ids))?;
        let changed: Vec<_> = states
            .into_iter()
            .filter(|entry| {
                stored.get(&entry.item).is_none_or(|row| {
                    row.profile_key != key || row.state != entry.state || row.reason != entry.reason
                })
            })
            .collect();
        if !changed.is_empty() {
            let items: Vec<ItemId> = changed.iter().map(|s| s.item).collect();
            let at = crate::step_outcomes::now_ms();
            self.db
                .write(move |tx| set_organize_states(tx, &changed, &key, at))?;
            self.changes.publish(&items, &[ChangePart::Stage]);
        }
        Ok(preview)
    }

    /// Queues the job that computes the titles with no state for the active profile: after a
    /// scan, at start-up and when the profile changed. Returns `None` when there are none. Must
    /// be called inside a tokio runtime.
    pub fn refresh_missing(self: &Arc<Self>) -> Option<JobHandle> {
        let key = self.current_key();
        let stale = match self.db.read(move |c| stale_items(c, &key)) {
            Ok(stale) => stale,
            Err(error) => {
                tracing::warn!(%error, "the titles without a name check could not be listed");
                return None;
            }
        };
        self.submit(&stale)
    }

    /// Notes that these titles changed (scraped, renamed, undone): their state is forgotten, so
    /// it reads as "checking", and recomputed by a job queued after a short pause that collects
    /// the titles of a burst. Must be called inside a tokio runtime.
    pub fn note_changed(self: &Arc<Self>, ids: &[ItemId]) {
        if ids.is_empty() {
            return;
        }
        let forget = ids.to_vec();
        if let Err(error) = self.db.write(move |tx| clear_organize_states(tx, &forget)) {
            tracing::debug!(%error, "the name check of a changed title was not cleared");
        }
        self.waiting
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend(ids.iter().copied());
        // Without a runtime (a plain thread) the titles wait for the next refresh.
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        if self.collecting.swap(true, Ordering::SeqCst) {
            return;
        }
        let service = Arc::clone(self);
        runtime.spawn(async move {
            tokio::time::sleep(COLLECT).await;
            service.collecting.store(false, Ordering::SeqCst);
            let ids: Vec<ItemId> = std::mem::take(
                &mut *service
                    .waiting
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner),
            )
            .into_iter()
            .collect();
            service.submit(&ids);
        });
    }

    /// A hook for [`OrganizeService::with_changed_hook`]: every apply and undo tells this service
    /// which titles it touched.
    #[must_use]
    pub fn hook(self: &Arc<Self>) -> super::ChangedHook {
        let service = Arc::clone(self);
        Arc::new(move |ids| {
            // The folders and files moved or came back, and the check starts again.
            service
                .changes
                .publish(ids, &[ChangePart::Files, ChangePart::Stage]);
            service.note_changed(ids);
        })
    }

    /// Looks at the settings after a change: when the naming profile is not the one the cache was
    /// computed for, every title is queued for a new computation. Must be called inside a tokio
    /// runtime.
    pub fn settings_changed(self: &Arc<Self>) {
        let key = self.current_key();
        {
            let mut last = self.last_key.lock().unwrap_or_else(PoisonError::into_inner);
            if *last == key {
                return;
            }
            *last = key;
        }
        self.refresh_missing();
    }

    fn submit(self: &Arc<Self>, ids: &[ItemId]) -> Option<JobHandle> {
        if ids.is_empty() {
            return None;
        }
        let name = crate::wording::job::checking_names(ids.len());
        let spec = JobSpec::new(name, LaneId::Cpu).priority(Priority::Low);
        Some(self.jobs.submit(
            spec,
            StateWork {
                service: Arc::clone(self),
            },
            chunks_of(ids),
        ))
    }
}

/// The worker of the job.
struct StateWork {
    service: Arc<OrganizeStateService>,
}

impl ItemWork for StateWork {
    type Item = StateChunk;

    async fn process(&self, chunk: &StateChunk, control: &ItemControl) -> Result<(), ItemError> {
        if control.is_cancelled() {
            return Err(ItemError::Cancelled);
        }
        let service = Arc::clone(&self.service);
        let ids = chunk.ids.clone();
        match tokio::task::spawn_blocking(move || service.recompute_blocking(&ids)).await {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(error)) => {
                tracing::warn!(%error, "the folder names could not be checked");
                Err(StageError::new("The folder names could not be checked").into())
            }
            Err(error) => {
                tracing::error!(%error, "the name check worker stopped");
                Err(StageError::new("The name check stopped unexpectedly").into())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_are_split_into_labelled_chunks() {
        let ids: Vec<ItemId> = (1..=450).map(ItemId).collect();
        let chunks = chunks_of(&ids);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].label, "Titles 1-200");
        assert_eq!(chunks[2].label, "Titles 401-450");
        assert!(chunks_of(&[]).is_empty());
    }

    #[test]
    fn the_key_follows_the_profile() {
        let mut values = BTreeMap::new();
        let clean = profile_key(&current_profile(&values));
        assert_eq!(clean, profile_key(&current_profile(&values)));
        values.insert("renamer.active-profile".to_owned(), Value::from("detailed"));
        assert_ne!(clean, profile_key(&current_profile(&values)));
    }
}
