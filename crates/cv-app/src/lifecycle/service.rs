//! The lifecycle service: what the IPC layer and the screens ask for. It joins the library, the
//! settings, the cached organize state and the titles a job is touching now.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex, PoisonError};

use cv_db::{Database, DbError};
use cv_domain::{ItemId, ItemKind};

use super::{Lifecycle, LifecycleContext, LifecycleCounts, lifecycle_counts, read};
use crate::app_settings::SettingsService;
use crate::organize::OrganizeStateService;

/// The titles a job is touching now (a scrape's extras, a ratings fetch). A title can be held by
/// several jobs at once; it is working until the last one lets go.
#[derive(Debug, Default)]
pub struct Working {
    held: Mutex<HashMap<ItemId, u32>>,
    /// The titles whose ratings are being fetched now (a subset of the working ones): their
    /// rating chips say "fetching".
    ratings: Mutex<HashMap<ItemId, u32>>,
}

/// Holds a title as fetching its ratings until it is dropped.
#[derive(Debug)]
pub struct RatingsGuard {
    working: Arc<Working>,
    item: ItemId,
}

/// Holds a title as working until it is dropped.
#[derive(Debug)]
pub struct WorkingGuard {
    working: Arc<Working>,
    item: ItemId,
}

impl Working {
    /// Marks `item` as working until the returned guard is dropped.
    #[must_use]
    pub fn hold(self: &Arc<Self>, item: ItemId) -> WorkingGuard {
        *self
            .held
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(item)
            .or_default() += 1;
        WorkingGuard {
            working: Arc::clone(self),
            item,
        }
    }

    /// The titles that are working now.
    #[must_use]
    pub fn items(&self) -> HashSet<ItemId> {
        self.held
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .keys()
            .copied()
            .collect()
    }

    /// Marks `item` as fetching its ratings until the returned guard is dropped.
    #[must_use]
    pub fn hold_ratings(self: &Arc<Self>, item: ItemId) -> RatingsGuard {
        *self
            .ratings
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(item)
            .or_default() += 1;
        RatingsGuard {
            working: Arc::clone(self),
            item,
        }
    }

    /// Whether the ratings of `item` are being fetched now.
    #[must_use]
    pub fn is_fetching_ratings(&self, item: ItemId) -> bool {
        self.ratings
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains_key(&item)
    }

    /// Whether `item` is working now.
    #[must_use]
    pub fn is_working(&self, item: ItemId) -> bool {
        self.held
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains_key(&item)
    }
}

impl Drop for WorkingGuard {
    fn drop(&mut self) {
        let mut held = self
            .working
            .held
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(count) = held.get_mut(&self.item) {
            *count -= 1;
            if *count == 0 {
                held.remove(&self.item);
            }
        }
    }
}

impl Drop for RatingsGuard {
    fn drop(&mut self) {
        let mut held = self
            .working
            .ratings
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(count) = held.get_mut(&self.item) {
            *count -= 1;
            if *count == 0 {
                held.remove(&self.item);
            }
        }
    }
}

/// Reads lifecycles and counts for the screens.
pub struct LifecycleService {
    db: Arc<Database>,
    settings: Arc<SettingsService>,
    organize: Arc<OrganizeStateService>,
    working: Arc<Working>,
}

impl LifecycleService {
    #[must_use]
    pub fn new(
        db: Arc<Database>,
        settings: Arc<SettingsService>,
        organize: Arc<OrganizeStateService>,
        working: Arc<Working>,
    ) -> Arc<Self> {
        Arc::new(Self {
            db,
            settings,
            organize,
            working,
        })
    }

    /// The titles a job is touching now.
    #[must_use]
    pub fn working(&self) -> &Arc<Working> {
        &self.working
    }

    /// The key of the naming profile the organize-state cache is read against; the library's stage
    /// filter uses it, so a filtered list matches the pipeline counts.
    #[must_use]
    pub fn profile_key(&self) -> String {
        self.organize.current_key()
    }

    fn context(&self) -> LifecycleContext {
        LifecycleContext {
            values: self.settings.snapshot().values,
            profile_key: self.organize.current_key(),
            working: self.working.items(),
        }
    }

    /// The lifecycle of `ids`. A collection, a title the library does not know and anything else
    /// without a lifecycle is absent from the answer. Blocking.
    ///
    /// # Errors
    /// The library cannot be read.
    pub fn read(&self, ids: &[ItemId]) -> Result<BTreeMap<ItemId, Lifecycle>, DbError> {
        read(&self.db, &self.context(), ids)
    }

    /// The pipeline counts of the movies or the shows of the library. Blocking.
    ///
    /// # Errors
    /// The library cannot be read.
    pub fn counts(&self, kind: ItemKind) -> Result<LifecycleCounts, DbError> {
        let working = self.working_of(kind)?;
        lifecycle_counts(&self.db, kind, &self.organize.current_key(), working)
    }

    /// How many of the working titles are movies or shows (`kind`).
    fn working_of(&self, kind: ItemKind) -> Result<u32, DbError> {
        let ids: Vec<ItemId> = self.working.items().into_iter().collect();
        if ids.is_empty() {
            return Ok(0);
        }
        let rows = self
            .db
            .read(move |c| cv_db::repo::lifecycle::lifecycle_rows(c, &ids))?;
        let mut bases: HashSet<ItemId> = HashSet::new();
        for row in rows.values() {
            // A season or an episode at work stands for its show.
            let base = if matches!(row.kind, ItemKind::Season | ItemKind::Episode) {
                row.parent_id.unwrap_or(row.id)
            } else {
                row.id
            };
            if row.kind == kind || (kind == ItemKind::Show && base != row.id) {
                bases.insert(base);
            }
        }
        Ok(u32::try_from(bases.len()).unwrap_or(u32::MAX))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_title_is_working_until_the_last_hold_is_dropped() {
        let working = Arc::new(Working::default());
        let first = working.hold(ItemId(1));
        let second = working.hold(ItemId(1));
        assert!(working.is_working(ItemId(1)));
        drop(first);
        assert!(working.is_working(ItemId(1)));
        drop(second);
        assert!(!working.is_working(ItemId(1)));
        assert!(working.items().is_empty());
    }
}
