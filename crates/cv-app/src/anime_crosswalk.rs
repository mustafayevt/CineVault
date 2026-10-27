//! The anime ID cross-reference, loaded when a title needs it.
//!
//! `cv_providers::AnimeOfflineDb` downloads the community anime-offline-database once and keeps a
//! compact copy in the cache folder. This service decides *when* it is needed and keeps it:
//!
//! * a title is an anime when it has an ID of an anime site (AniDB, MyAnimeList, AniList, Kitsu
//!   and the others the database knows). Only then is the cross-reference loaded, on the first
//!   anime scrape, so a library with no anime never downloads it;
//! * the copy is read from disk while it is younger than a week; after that one conditional
//!   request asks whether a newer release exists (see [`REFRESH_INTERVAL`]);
//! * once loaded it stays in memory, and is loaded again after a week without a restart;
//! * when it cannot be loaded (offline, no cache) the scrape goes on without it, and loading is
//!   not tried again for [`RETRY_AFTER`], so a batch does not repeat a failing download.
//!
//! The rows are passed to `cv_providers::complete_ids`, which adds the IDs the anime sites do not
//! give each other (MyAnimeList from AniDB and so on) without a request.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use cv_domain::ProviderIds;
use cv_providers::anime_db::{AnimeDbError, LoadOutcome};
use cv_providers::{AnimeCrossReference, AnimeOfflineDb};

/// How old the local copy may get before the server is asked for a newer release: a week.
pub const REFRESH_INTERVAL: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// How long after a failed load the next title does not try again.
pub const RETRY_AFTER: Duration = Duration::from_secs(10 * 60);

/// Folder name, under the data location's cache folder, of the anime database copy.
pub const CACHE_DIR: &str = "anime-offline-database";

/// ID keys of the anime sites the offline database covers. A title with one of them is an anime.
const ANIME_KEYS: &[&str] = &[
    "anidb",
    "myanimelist",
    "anilist",
    "kitsu",
    "anisearch",
    "animeplanet",
    "livechart",
    "notify",
    "animecountdown",
    "animenewsnetwork",
];

/// Whether `ids` belong to an anime: any ID of an anime site is present.
#[must_use]
pub fn is_anime(ids: &ProviderIds) -> bool {
    ANIME_KEYS.iter().any(|key| ids.contains(key))
}

struct State {
    loaded: Option<(Instant, Arc<AnimeCrossReference>)>,
    failed_at: Option<Instant>,
}

struct Inner {
    db: AnimeOfflineDb,
    state: Mutex<State>,
    /// One load at a time: the titles of a batch wait for the first one instead of each
    /// starting a download.
    loading: tokio::sync::Mutex<()>,
}

/// The cross-reference of the app. Cloning is cheap.
#[derive(Clone)]
pub struct AnimeCrosswalk {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for AnimeCrosswalk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AnimeCrosswalk")
    }
}

impl AnimeCrosswalk {
    /// The cross-reference over `db`, refreshed weekly.
    #[must_use]
    pub fn new(db: AnimeOfflineDb) -> Self {
        Self {
            inner: Arc::new(Inner {
                db: db.refresh_interval(REFRESH_INTERVAL),
                state: Mutex::new(State {
                    loaded: None,
                    failed_at: None,
                }),
                loading: tokio::sync::Mutex::new(()),
            }),
        }
    }

    /// The cross-reference stored in `dir`, downloaded from the default address.
    ///
    /// # Errors
    /// Fails when the HTTP client for the download cannot start.
    pub fn in_dir(dir: impl Into<PathBuf>) -> Result<Self, cv_providers::ProviderError> {
        Ok(Self::new(AnimeOfflineDb::new(dir)?))
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.inner
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// The cross-reference when it is already in memory and still young. Never loads.
    #[must_use]
    pub fn loaded(&self) -> Option<Arc<AnimeCrossReference>> {
        self.state()
            .loaded
            .as_ref()
            .filter(|(at, _)| at.elapsed() < REFRESH_INTERVAL)
            .map(|(_, crosswalk)| Arc::clone(crosswalk))
    }

    /// The cross-reference for a title with `ids`: `None` when the title is not an anime (nothing
    /// is loaded then) or the cross-reference is not available.
    pub async fn for_title(&self, ids: &ProviderIds) -> Option<Arc<AnimeCrossReference>> {
        if !is_anime(ids) {
            return None;
        }
        self.get().await
    }

    /// The cross-reference, loaded or refreshed when needed.
    pub async fn get(&self) -> Option<Arc<AnimeCrossReference>> {
        if let Some(loaded) = self.loaded() {
            return Some(loaded);
        }
        let _one_at_a_time = self.inner.loading.lock().await;
        if let Some(loaded) = self.loaded() {
            return Some(loaded);
        }
        if self
            .state()
            .failed_at
            .is_some_and(|at| at.elapsed() < RETRY_AFTER)
        {
            return None;
        }
        match self.load().await {
            Ok((crosswalk, _)) => Some(crosswalk),
            Err(error) => {
                tracing::warn!(%error, "the anime ID database is not available");
                self.state().failed_at = Some(Instant::now());
                None
            }
        }
    }

    /// Loads the copy (from disk, or after asking the server when it is a week old) and keeps it
    /// in memory. Returns how it was obtained.
    ///
    /// # Errors
    /// There is no usable local copy and the download failed.
    pub async fn load(&self) -> Result<(Arc<AnimeCrossReference>, LoadOutcome), AnimeDbError> {
        let loaded = self.inner.db.load().await?;
        let crosswalk = Arc::new(loaded.crosswalk);
        let mut state = self.state();
        state.loaded = Some((Instant::now(), Arc::clone(&crosswalk)));
        state.failed_at = None;
        Ok((crosswalk, loaded.outcome))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(pairs: &[(&str, &str)]) -> ProviderIds {
        pairs.iter().copied().collect()
    }

    #[test]
    fn a_title_with_an_anime_site_id_is_an_anime() {
        assert!(is_anime(&ids(&[("anidb", "4563")])));
        assert!(is_anime(&ids(&[("tmdb", "1"), ("myanimelist", "1535")])));
        assert!(!is_anime(&ids(&[("tmdb", "603"), ("imdb", "tt0133093")])));
        assert!(!is_anime(&ProviderIds::new()));
    }
}
