//! Artwork downloads for the app: Download missing artwork, Re-download artwork, a chosen image,
//! and the step that runs after an item was matched and scraped.
//!
//! The rules and the file writing live in `cv_artwork::download`; this service finds the files of
//! an item, asks the artwork providers for candidates, and records the outcome in the library.
//!
//! # Settings
//!
//! The `artwork.*` keys of [`cv_artwork::download::config`], and `metadata.language` for the
//! provider requests. A scrape always fills the empty artwork types afterwards; there is no
//! switch for it (tests turn it off with [`ArtworkService::set_after_scrape_runs`]).
//!
//! # What is touched
//!
//! Movies, shows and collections. A show's seasons and episodes are worked on with it
//! (`seasons.rs`: season posters, fanart, banners and thumbs, and episode thumbs). The folder of a
//! movie or show must exist; nothing is created
//! for an item whose folder is gone. Collections (`collection.rs`) are written in the places of
//! `artwork.collection-locations`. An image the item already has, under any name, is never replaced unless the
//! caller asks for replacement, and what a replacement displaces goes to the trash folder of the
//! data source.
//!
//! A selection is worked on one item after the other. The automatic runs after scrapes overlap
//! at most [`PARALLEL_ITEMS`] at a time, on top of the rate limit the HTTP client keeps for image
//! downloads, so a big batch never floods a provider.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use cv_artwork::download::config::ArtworkConfig;
use cv_artwork::download::fetch::{FetchError, HttpImageFetcher, ImageFetcher};
use cv_artwork::download::files::Trash;
use cv_artwork::download::record::record;
use cv_artwork::download::{
    ArtworkDownloader, ArtworkTarget, DownloadReport, DownloadRequest, Subject, candidate_kind,
    picker_slots, present_artwork,
};
use cv_artwork::frame_source::{file_url, path_from_file_url};
use cv_artwork::naming::{ArtworkNames, FolderKind, MovieArt};
use cv_artwork::quality::{is_low_quality, quality};
use cv_artwork::select::{SelectionSettings, best_score, matching_score, rank};
use cv_artwork::thumb::{ThumbCache, ThumbKey, ThumbSize};
use cv_db::Database;
use cv_db::repo::artwork_refs::artwork_refs;
use cv_db::repo::{get_item, update_item};
use cv_domain::{Item, ItemDetail, ItemId, ItemKind, MediaFileType};
use cv_providers::artwork::{gather_artwork, text_language};
use cv_providers::model::{MediaKind, Popularity, RequestOptions};
use cv_providers::{ArtworkProvider, SourcedArtwork};
use cv_rename::naming::ShowArt;
use tokio::sync::Semaphore;

mod collection;
pub mod frames;
mod seasons;
pub mod stream;
mod undo;

use crate::app_settings::SettingsService;
use crate::changes::{ChangeHub, ChangePart, Changes};
use crate::matching::MatchSettings;
use crate::nfo::model::main_video;
use crate::nfo::paths::video_base_name;
use crate::provider_credentials::ProviderCredentialsService;
use crate::step_outcomes::{self, STEP_ARTWORK, StepStatus};
use crate::wording::counted;

/// Items worked on at the same time.
pub const PARALLEL_ITEMS: usize = 2;

/// Why an artwork run could not start for an item.
#[derive(Debug, thiserror::Error)]
pub enum ArtworkError {
    #[error("the item no longer exists")]
    ItemNotFound,
    #[error("artwork is only downloaded for movies, shows and collections")]
    Unsupported,
    #[error("the item has no folder to put artwork in")]
    NoFolder,
    #[error("this item cannot have that kind of artwork")]
    UnknownSlot,
    #[error("{0}")]
    BadSource(String),
    #[error(transparent)]
    Db(#[from] cv_db::DbError),
    #[error("that placement cannot be undone any more")]
    UndoGone,
    /// The title was renamed or moved since the placement; nothing was put back.
    #[error("{0}")]
    UndoStale(String),
    #[error("the artwork worker stopped")]
    Worker,
    /// The title is being moved or renamed; nothing was written.
    #[error("{0}")]
    Busy(String),
}

/// What a run did for one item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtworkOutcome {
    pub item: ItemId,
    /// Images written (main images and extras).
    pub images: usize,
    /// Actor portraits written.
    pub portraits: usize,
    /// Files moved to the trash.
    pub trashed: usize,
    /// What could not be done, as `what: reason`.
    pub problems: Vec<String>,
    /// For a chosen image or set of extras that wrote something: its History entry's id, the
    /// token that [`ArtworkService::undo_place`] takes to put the old files back.
    pub undo: Option<String>,
}

impl ArtworkOutcome {
    fn nothing(item: ItemId) -> Self {
        Self {
            item,
            images: 0,
            portraits: 0,
            trashed: 0,
            problems: Vec::new(),
            undo: None,
        }
    }

    fn of(item: ItemId, report: &DownloadReport) -> Self {
        Self {
            item,
            images: report.images.len(),
            portraits: report.actors.len(),
            trashed: report.trashed.len(),
            problems: report
                .failures
                .iter()
                .map(|f| format!("{}: {}", f.what, f.reason))
                .collect(),
            undo: None,
        }
    }
}

/// What a selection run did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArtworkSummary {
    pub outcomes: Vec<ArtworkOutcome>,
    /// Items that could not be worked on at all, with why.
    pub skipped: Vec<(ItemId, String)>,
}

impl ArtworkSummary {
    pub fn images(&self) -> usize {
        self.outcomes.iter().map(|o| o.images).sum()
    }

    pub fn portraits(&self) -> usize {
        self.outcomes.iter().map(|o| o.portraits).sum()
    }
}

/// What a clean-up found for one item.
#[derive(Debug)]
pub struct CleanupResult {
    pub item: ItemId,
    /// The files moved to the trash (with a dry run: the files that would be), or why the item
    /// could not be tidied.
    pub outcome: Result<Vec<PathBuf>, ArtworkError>,
}

/// Which run a request is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Run {
    /// Fill the empty types from the providers.
    Missing,
    /// Fetch again what is missing on disk from the remembered URLs.
    Redownload,
}

/// The most candidates the picker lists for one artwork type.
pub const MAX_PICKER_CANDIDATES: usize = 60;

/// Previews built at the same time when the picker opens.
const PREVIEW_PARALLEL: usize = 6;

/// One image a provider offers, as the picker shows it: already downloaded into the thumbnail
/// cache, so the interface draws it from `cv://img` like any other image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickerCandidate {
    /// Key of the provider that offers it (`tmdb`, `fanart`, ...).
    pub provider: String,
    /// Where it comes from. Passed back to [`ArtworkService::place`] or
    /// [`ArtworkService::place_extras`] to use it.
    pub url: String,
    /// The language of the text on the image, `None` for an image without text.
    pub language: Option<String>,
    /// Pixels. Read from the image itself when the provider does not say.
    pub width: u32,
    pub height: u32,
    /// How well it fits the artwork settings: 90 for language and size, 50 for language only, 40
    /// for size only, 0 otherwise.
    pub score: u32,
    /// The picker's "Best" order, higher first: [`cv_artwork::select::best_score`] times 100.
    pub best: u32,
    /// How many people voted on it at the provider (TMDB), `0` when unknown.
    pub votes: u32,
    /// The average of those votes times ten (`56` for 5.6), `0` without votes.
    pub rating: u32,
    /// How many people liked it at the provider (Fanart.tv), `0` when unknown.
    pub likes: u32,
    /// Too small or the wrong shape for its type: the picker lists it behind "Show N more".
    pub low_quality: bool,
    /// The content key of its thumbnail.
    pub key: ThumbKey,
}

impl PickerCandidate {
    /// A provider's image as the picker shows it, with its fit, its place in the "Best" order and
    /// its popularity. `width` and `height` are the image's own when the provider does not say.
    #[must_use]
    pub fn of(
        item: &SourcedArtwork,
        kind: MediaFileType,
        selection: &SelectionSettings,
        key: ThumbKey,
        (width, height): (u32, u32),
    ) -> Self {
        let candidate = &item.candidate;
        let mut sized = item.clone();
        if sized.candidate.width == 0 {
            sized.candidate.width = width;
        }
        if sized.candidate.height == 0 {
            sized.candidate.height = height;
        }
        let (votes, rating, likes) = match candidate.popularity {
            Popularity::Unknown => (0, 0, 0),
            Popularity::Rated { average, votes } => {
                let tenths = if votes == 0 || !average.is_finite() {
                    0
                } else {
                    (average.clamp(0.0, 10.0) * 10.0).round() as u32
                };
                (votes, tenths, 0)
            }
            Popularity::Liked { likes } => (0, 0, likes),
        };
        Self {
            provider: item.provider.to_owned(),
            url: candidate.url.clone(),
            language: text_language(candidate.language.as_deref()),
            width: sized.candidate.width,
            height: sized.candidate.height,
            score: matching_score(&sized, kind, selection),
            best: (best_score(&sized, kind, selection).max(0.0) * 100.0).round() as u32,
            votes,
            rating,
            likes,
            low_quality: is_low_quality(&sized, selection),
            key,
        }
    }
}

/// A provider that was asked for candidates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickerProvider {
    pub key: String,
    pub name: String,
}

/// What the artwork picker shows for an item and an artwork type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickerOptions {
    pub title: String,
    /// `0` when unknown.
    pub year: u16,
    /// The types the item can have, in listing order.
    pub slots: Vec<MediaFileType>,
    /// The providers that were asked.
    pub providers: Vec<PickerProvider>,
    /// Names of the providers that failed, so the list may be incomplete.
    pub unreachable: Vec<String>,
    /// Best fit first: the images the automatic selection would pick, in its order, then the rest
    /// by the provider's own score.
    pub candidates: Vec<PickerCandidate>,
}

/// A candidate that was downloaded into the thumbnail cache.
#[derive(Clone)]
struct Preview {
    key: ThumbKey,
    width: u32,
    height: u32,
}

/// Candidate previews remembered at most, so a long session of picking stays small. Past it the
/// map starts again; the thumbnails stay in the image cache.
const MAX_PREVIEWS: usize = 10_000;

/// Remembers the preview of `url`, starting the map again when it is full.
fn remember_preview(previews: &mut HashMap<String, Preview>, url: String, preview: Preview) {
    if previews.len() >= MAX_PREVIEWS && !previews.contains_key(&url) {
        previews.clear();
    }
    previews.insert(url, preview);
}

/// What [`ArtworkService::gather`] found.
struct Gathered {
    candidates: Vec<SourcedArtwork>,
    providers: Vec<PickerProvider>,
    /// Names of the providers that failed.
    failed: Vec<String>,
}

/// The artwork service of the app.
pub struct ArtworkService {
    db: Arc<Database>,
    providers: Arc<ProviderCredentialsService>,
    settings: Arc<SettingsService>,
    downloader: ArtworkDownloader,
    fetcher: Arc<dyn ImageFetcher>,
    thumbs: Arc<ThumbCache>,
    /// Candidates downloaded so far, by URL, so reopening the picker fetches nothing again.
    previews: Mutex<HashMap<String, Preview>>,
    gate: Semaphore,
    /// Announces the artwork that was written, so open pages refresh.
    changes: Changes,
    /// Where the picker gets frame grabs from, once the app has set it up.
    frames: Mutex<Option<frames::FrameSetup>>,
    /// Writes the NFO again after a chosen image changed a remembered address.
    nfo: Mutex<Option<Arc<crate::nfo::NfoService>>>,
    /// Whether a scrape fills the empty artwork types by itself. Always, except in tests.
    after_scrape_runs: AtomicBool,
}

impl ArtworkService {
    /// Builds the service. Images are fetched through the providers' shared HTTP client, and new
    /// posters get their thumbnails in `thumbs` as they are written.
    #[must_use]
    pub fn new(
        db: Arc<Database>,
        providers: Arc<ProviderCredentialsService>,
        settings: Arc<SettingsService>,
        thumbs: Arc<ThumbCache>,
    ) -> Arc<Self> {
        let fetcher: Arc<dyn ImageFetcher> = match providers.http() {
            Some(http) => Arc::new(HttpImageFetcher::new(http.clone())),
            None => {
                tracing::warn!("no HTTP client: only local artwork files can be used");
                Arc::new(LocalFilesOnly)
            }
        };
        Self::with_fetcher(db, providers, settings, thumbs, fetcher)
    }

    /// As [`Self::new`] with another way to fetch images. For tests.
    #[must_use]
    pub fn with_fetcher(
        db: Arc<Database>,
        providers: Arc<ProviderCredentialsService>,
        settings: Arc<SettingsService>,
        thumbs: Arc<ThumbCache>,
        fetcher: Arc<dyn ImageFetcher>,
    ) -> Arc<Self> {
        Arc::new(Self {
            db,
            providers,
            settings,
            downloader: ArtworkDownloader::new(Arc::clone(&fetcher))
                .with_thumbs(Arc::clone(&thumbs)),
            fetcher,
            thumbs,
            previews: Mutex::new(HashMap::new()),
            gate: Semaphore::new(PARALLEL_ITEMS),
            changes: Changes::default(),
            frames: Mutex::new(None),
            nfo: Mutex::new(None),
            after_scrape_runs: AtomicBool::new(true),
        })
    }

    /// Whether any artwork provider that is ready (its credentials in place) serves `media`.
    #[must_use]
    pub fn has_ready_provider(&self, media: MediaKind) -> bool {
        let inactive: Vec<&str> = self
            .providers
            .list()
            .map(|statuses| {
                statuses
                    .into_iter()
                    .filter(|status| !status.active)
                    .map(|status| status.provider)
                    .collect()
            })
            .unwrap_or_default();
        self.providers
            .registry()
            .artwork_providers_for(media)
            .iter()
            .any(|p| !inactive.contains(&p.id()))
    }

    /// Settings › Deleted files as it is now.
    pub(crate) fn deleted_files(&self) -> crate::deleted_files::DeletedFiles {
        crate::deleted_files::DeletedFiles::from_values(|key| self.settings.get(key))
    }

    /// The artwork settings as they are now.
    #[must_use]
    pub fn config(&self) -> ArtworkConfig {
        ArtworkConfig::from_values(&self.settings.snapshot().values)
    }

    // ---- the actions -------------------------------------------------------------------------

    /// Download missing artwork for a selection: fills only the empty types (and the extras and
    /// portraits the settings ask for). `replace` chooses every image again and moves the old
    /// files to the trash.
    pub async fn download_missing(&self, items: &[ItemId], replace: bool) -> ArtworkSummary {
        self.run_all(items, Run::Missing, replace, &[]).await
    }

    /// As [`Self::download_missing`], asking only the artwork providers named in `only` (their
    /// keys, such as `tmdb`; any case). An empty list asks every ready provider.
    pub async fn download_missing_from(
        &self,
        items: &[ItemId],
        replace: bool,
        only: &[String],
    ) -> ArtworkSummary {
        self.run_all(items, Run::Missing, replace, only).await
    }

    /// Re-download artwork for a selection: fetches again, from the remembered source URLs, the
    /// images that are missing on disk. `replace` fetches all of them again.
    pub async fn redownload(&self, items: &[ItemId], replace: bool) -> ArtworkSummary {
        self.run_all(items, Run::Redownload, replace, &[]).await
    }

    /// Tidies the artwork files of a selection: an image of a type that sits under a name the
    /// artwork naming settings no longer use, while the image under a name they do use is there
    /// too, is moved to the trash folder of the data source. For a collection these are its own
    /// files in the collection artwork locations (see `collection.rs`). An image that is the only
    /// one of its type is never touched, whatever it is called, and nothing is deleted. With
    /// `dry_run` the files that would move are listed and nothing changes.
    pub async fn cleanup(&self, items: &[ItemId], dry_run: bool) -> Vec<CleanupResult> {
        let mut results = Vec::with_capacity(items.len());
        for &id in items {
            results.push(CleanupResult {
                item: id,
                outcome: self.cleanup_item(id, dry_run).await,
            });
        }
        results
    }

    async fn cleanup_item(&self, id: ItemId, dry_run: bool) -> Result<Vec<PathBuf>, ArtworkError> {
        if self.is_set(id).await {
            return self.cleanup_set(id, dry_run).await;
        }
        let _title = self.hold(id).await?;
        let loaded = self.load(id).await?;
        let names = ArtworkNames::from_values(&self.settings.snapshot().values);
        let stale = stale_artwork(&loaded.item, &loaded.target, &names);
        if dry_run || stale.is_empty() {
            return Ok(stale);
        }
        let db = Arc::clone(&self.db);
        tokio::task::spawn_blocking(move || {
            let trash = Trash::new(loaded.target.data_source.as_deref(), &loaded.target.root);
            let mut moved = Vec::new();
            for path in stale {
                match trash.put(&path) {
                    Ok(_) => moved.push(path),
                    Err(error) => {
                        tracing::warn!(path = %path.display(), %error, "stale artwork not moved")
                    }
                }
            }
            if !moved.is_empty() {
                let gone = moved.clone();
                db.write(move |tx| {
                    if let Some(mut item) = get_item(tx, id)? {
                        item.core.media_files.retain(|f| !gone.contains(&f.path()));
                        update_item(tx, &item)?;
                    }
                    Ok(())
                })?;
            }
            Ok(moved)
        })
        .await
        .map_err(|_| ArtworkError::Worker)?
    }

    /// Sets one image of an item from `url` (a candidate, a pasted address, or a dropped file as
    /// a `file:` URL), written with the configured file names. An image that is there is replaced
    /// when `replace` is set and kept otherwise.
    ///
    /// # Errors
    /// The item cannot be used for artwork.
    pub async fn place(
        &self,
        item: ItemId,
        kind: MediaFileType,
        url: &str,
        replace: bool,
    ) -> Result<ArtworkOutcome, ArtworkError> {
        // A collection has no NFO of its own to refresh: its image goes to its artwork locations.
        if self.is_set(item).await {
            return self.place_set(item, kind, url).await;
        }
        let outcome = self.place_held(item, kind, url, replace).await?;
        // The title is no longer held here: the NFO writer holds it itself.
        if outcome.images > 0 {
            self.refresh_nfo(item).await;
        }
        Ok(outcome)
    }

    /// [`Self::place`] while the title is held, before the NFO is written again.
    async fn place_held(
        &self,
        item: ItemId,
        kind: MediaFileType,
        url: &str,
        replace: bool,
    ) -> Result<ArtworkOutcome, ArtworkError> {
        let _permit = self
            .gate
            .acquire()
            .await
            .map_err(|_| ArtworkError::Worker)?;
        let _title = self.hold(item).await?;
        if let Some(season) = self.season_context(item).await? {
            return self.place_season(season, kind, url, replace).await;
        }
        let loaded = self.load(item).await?;
        let config = self.config();
        // The item's present images of this type in its artwork folder: with `replace`, the new
        // image takes the place of all of them, whatever names they have.
        let current: Vec<PathBuf> = loaded
            .item
            .core
            .media_files
            .iter()
            .filter(|f| f.kind == kind && Path::new(&f.directory) == loaded.target.dir)
            .map(cv_domain::MediaFile::path)
            .collect();
        let report = self
            .downloader
            .place_over(&loaded.target, &config, kind, url, replace, &current)
            .await;
        self.record(item, &report).await?;
        let mut outcome = ArtworkOutcome::of(item, &report);
        outcome.undo = undo::remember(
            &self.db,
            item,
            &loaded,
            kind,
            &report,
            &self.deleted_files(),
        );
        Ok(outcome)
    }

    /// Replaces the extra fanart or the extra thumbs (`kind` is `Extrafanart` or `Extrathumb`)
    /// with the images at `urls`: candidates, pasted addresses and dropped files as `file:` URLs.
    /// The whole set is replaced; the old files go to the trash.
    ///
    /// # Errors
    /// The item cannot be used for artwork.
    pub async fn place_extras(
        &self,
        item: ItemId,
        kind: MediaFileType,
        urls: &[String],
    ) -> Result<ArtworkOutcome, ArtworkError> {
        let _permit = self
            .gate
            .acquire()
            .await
            .map_err(|_| ArtworkError::Worker)?;
        let _title = self.hold(item).await?;
        let loaded = self.load(item).await?;
        let config = self.config();
        let report = self
            .downloader
            .place_extras(&loaded.target, &config, kind, urls)
            .await;
        self.record(item, &report).await?;
        let mut outcome = ArtworkOutcome::of(item, &report);
        outcome.undo = undo::remember(
            &self.db,
            item,
            &loaded,
            kind,
            &report,
            &self.deleted_files(),
        );
        Ok(outcome)
    }

    /// The images the artwork picker offers for `slot` of an item: the candidates of every
    /// enabled provider, downloaded into the thumbnail cache so the interface can draw them. A
    /// provider that fails costs only its own candidates and is named in
    /// [`PickerOptions::unreachable`]; an image that cannot be fetched is left out.
    ///
    /// # Errors
    /// The item cannot be used for artwork, or cannot have that type.
    pub async fn picker_options(
        &self,
        item: ItemId,
        slot: MediaFileType,
    ) -> Result<PickerOptions, ArtworkError> {
        let loaded = self.load(item).await?;
        let slots = picker_slots(&loaded.target);
        if !slots.contains(&slot) {
            return Err(ArtworkError::UnknownSlot);
        }
        let kind = candidate_kind(slot);
        let config = self.config();
        let gathered = self.gather(&loaded.item, loaded.media, &[]).await;

        // The images the automatic selection would take come first, in its order; whatever its
        // settings leave out follows, best provider score first.
        let ranked = rank(&gathered.candidates, kind, &config.selection);
        let listed: HashSet<&str> = ranked.iter().map(|c| c.candidate.url.as_str()).collect();
        let mut rest: Vec<&SourcedArtwork> = gathered
            .candidates
            .iter()
            .filter(|c| c.candidate.kind == kind && !listed.contains(c.candidate.url.as_str()))
            .collect();
        rest.sort_by(|a, b| quality(&b.candidate).total_cmp(&quality(&a.candidate)));
        let mut seen = HashSet::new();
        let ordered: Vec<&SourcedArtwork> = ranked
            .iter()
            .copied()
            .chain(rest)
            .filter(|c| seen.insert(c.candidate.url.as_str()))
            .take(MAX_PICKER_CANDIDATES)
            .collect();

        let urls: Vec<String> = ordered.iter().map(|c| c.candidate.url.clone()).collect();
        let previews = self.previews(urls).await;
        let candidates = ordered
            .into_iter()
            .filter_map(|c| {
                let preview = previews.get(&c.candidate.url)?;
                Some(PickerCandidate::of(
                    c,
                    kind,
                    &config.selection,
                    preview.key.clone(),
                    (preview.width, preview.height),
                ))
            })
            .collect();
        Ok(PickerOptions {
            title: loaded.item.core.title.clone(),
            year: loaded.item.core.year,
            slots,
            providers: gathered.providers,
            unreachable: gathered.failed,
            candidates,
        })
    }

    /// The step after a scrape: when the setting is on, fills the item's empty artwork types.
    /// Returns at once; the work runs in the background. Locked items are left alone, as their
    /// scrape was.
    pub fn after_match(self: &Arc<Self>, item: ItemId) {
        drop(self.after_match_task(item));
    }

    /// Switches the artwork a scrape fills by itself on (the default) or off. Only tests turn it
    /// off, to drive the artwork step by hand.
    pub fn set_after_scrape_runs(&self, on: bool) {
        self.after_scrape_runs.store(on, Ordering::SeqCst);
    }

    /// Whether a scrape fills the empty artwork types by itself (always, except in tests).
    #[must_use]
    pub fn after_scrape_enabled(&self) -> bool {
        self.after_scrape_runs.load(Ordering::SeqCst)
    }

    /// [`Self::after_match`] that hands back the background task, so a later step that moves the
    /// item's folder (rename after scrape) can wait for the downloads to finish. `None` when the
    /// switch is off (tests only) and nothing runs.
    pub fn after_match_task(self: &Arc<Self>, item: ItemId) -> Option<tokio::task::JoinHandle<()>> {
        if !self.after_scrape_runs.load(Ordering::SeqCst) {
            return None;
        }
        let service = Arc::clone(self);
        Some(cv_jobs::spawn(async move {
            service.fill_after_scrape(item).await;
        }))
    }

    /// The artwork step of a scrape, run to its end: fills the item's empty artwork types (and the
    /// collection's, for a movie in one) and leaves the step outcome behind. Locked items are left
    /// alone. It always runs after a scrape; only tests switch it off.
    /// Returns whether it saved any image for the title.
    pub async fn fill_after_scrape(self: &Arc<Self>, item: ItemId) -> bool {
        if !self.after_scrape_runs.load(Ordering::SeqCst) {
            return false;
        }
        self.after_match_set(item);
        match self.run_item(item, Run::Missing, false, true, &[]).await {
            Ok(outcome) => {
                let saved = outcome.images + outcome.portraits > 0;
                if saved {
                    tracing::info!(
                        item = item.0,
                        images = outcome.images,
                        portraits = outcome.portraits,
                        "artwork downloaded after a scrape"
                    );
                }
                for problem in &outcome.problems {
                    tracing::debug!(item = item.0, %problem, "artwork problem");
                }
                saved
            }
            Err(error) => {
                tracing::debug!(item = item.0, %error, "no artwork after a scrape");
                false
            }
        }
    }

    // ---- the work ----------------------------------------------------------------------------

    async fn run_all(
        &self,
        items: &[ItemId],
        run: Run,
        replace: bool,
        only: &[String],
    ) -> ArtworkSummary {
        let mut summary = ArtworkSummary::default();
        for id in items {
            match self.run_item(*id, run, replace, false, only).await {
                Ok(outcome) => summary.outcomes.push(outcome),
                Err(error) => summary.skipped.push((*id, error.to_string())),
            }
        }
        summary
    }

    async fn run_item(
        &self,
        id: ItemId,
        run: Run,
        replace: bool,
        skip_locked: bool,
        only: &[String],
    ) -> Result<ArtworkOutcome, ArtworkError> {
        if self.is_set(id).await {
            // A collection keeps the URLs of its artwork; both runs write what is missing.
            return self.run_set(id, replace).await;
        }
        let _permit = self
            .gate
            .acquire()
            .await
            .map_err(|_| ArtworkError::Worker)?;
        let _title = self.hold(id).await?;
        let loaded = self.load(id).await?;
        // Filling what is missing is the after-scrape step; it leaves its outcome behind.
        let records_outcome = run == Run::Missing && !replace && only.is_empty();
        if skip_locked && loaded.item.core.locked {
            if records_outcome {
                step_outcomes::record(
                    &self.db,
                    id,
                    STEP_ARTWORK,
                    StepStatus::Skipped,
                    "The title is locked, so its artwork was left alone.",
                );
            }
            return Ok(ArtworkOutcome::nothing(id));
        }
        let config = self.config();

        let gathered = if run == Run::Missing && config.download.auto_select {
            Some(self.gather(&loaded.item, loaded.media, only).await)
        } else {
            None
        };
        let candidates: &[SourcedArtwork] =
            gathered.as_ref().map_or(&[], |g| g.candidates.as_slice());
        let present = present_artwork(&loaded.item);
        let request = DownloadRequest {
            target: &loaded.target,
            config: &config,
            candidates,
            recorded: &loaded.recorded,
            present: &present,
            cast: &loaded.item.core.credits,
            replace,
        };
        let report = match run {
            Run::Missing => self.downloader.download_missing(&request).await,
            Run::Redownload => self.downloader.redownload(&request).await,
        };
        self.record(id, &report).await?;
        let mut outcome = ArtworkOutcome::of(id, &report);
        if loaded.media == MediaKind::Show {
            // The seasons and episodes of a show have images of their own.
            let parts = self
                .run_tv_parts(
                    id,
                    &loaded,
                    run,
                    replace,
                    skip_locked,
                    seasons::TvParts { only, season: None },
                )
                .await?;
            outcome.images += parts.images;
            outcome.trashed += parts.trashed;
            outcome.problems.extend(parts.problems);
        }
        if records_outcome {
            let (status, reason) = self.step_status(&loaded, gathered.as_ref(), &outcome);
            step_outcomes::record(&self.db, id, STEP_ARTWORK, status, &reason);
        }
        Ok(outcome)
    }

    /// The step outcome of a run that filled the missing artwork of `loaded`: what it did, or
    /// why it could not.
    fn step_status(
        &self,
        loaded: &Loaded,
        gathered: Option<&Gathered>,
        outcome: &ArtworkOutcome,
    ) -> (StepStatus, String) {
        if outcome.images + outcome.portraits > 0 {
            return (
                StepStatus::Done,
                format!(
                    "Saved {}.",
                    counted(outcome.images + outcome.portraits, "image", "images")
                ),
            );
        }
        if let Some(problem) = outcome.problems.first() {
            return (StepStatus::Failed, problem.clone());
        }
        let Some(gathered) = gathered else {
            return (
                StepStatus::Skipped,
                "Automatic artwork selection is off.".to_owned(),
            );
        };
        if loaded.item.core.ids.is_empty() {
            return (
                StepStatus::Skipped,
                "The title has no provider ID yet.".to_owned(),
            );
        }
        if gathered.providers.is_empty() {
            let keyless = self.providers_needing_a_key(loaded.media);
            return match keyless.first() {
                Some(name) => (StepStatus::NeedsKey, format!("{name} needs an API key.")),
                None => (
                    StepStatus::Skipped,
                    "No artwork provider is switched on.".to_owned(),
                ),
            };
        }
        if gathered.candidates.is_empty() {
            return if gathered.failed.len() == gathered.providers.len() {
                (
                    StepStatus::Failed,
                    format!("{} did not answer.", gathered.failed.join(", ")),
                )
            } else {
                (
                    StepStatus::NotFound,
                    "No artwork provider knows this title.".to_owned(),
                )
            };
        }
        (
            StepStatus::Unchanged,
            "It has its artwork already.".to_owned(),
        )
    }

    /// The names of the artwork providers for `media` that wait for an API key.
    fn providers_needing_a_key(&self, media: MediaKind) -> Vec<String> {
        let waiting: Vec<&str> = self
            .providers
            .list()
            .map(|statuses| {
                statuses
                    .into_iter()
                    .filter(|status| !status.active && !status.missing.is_empty())
                    .map(|status| status.provider)
                    .collect()
            })
            .unwrap_or_default();
        self.providers
            .registry()
            .artwork_providers_for(media)
            .into_iter()
            .filter(|p| waiting.contains(&p.id()))
            .map(|p| p.name().to_owned())
            .collect()
    }

    /// Holds the item's title while its images are written, so no move or rename of the title
    /// runs meanwhile (see [`crate::title_guard`]). Taken before the item is loaded, so the
    /// folder it writes to is the current one.
    async fn hold(&self, id: ItemId) -> Result<crate::title_guard::TitleGuard, ArtworkError> {
        use crate::title_guard::{GuardError, WRITE_WAIT, Work, hold_items_async};
        hold_items_async(&self.db, &[id], Work::Write, WRITE_WAIT)
            .await
            .map_err(|error| match error {
                GuardError::Busy(busy) => ArtworkError::Busy(busy.to_string()),
                GuardError::Db(db) => ArtworkError::Db(db),
            })
    }

    async fn record(&self, id: ItemId, report: &DownloadReport) -> Result<(), ArtworkError> {
        let changed =
            !report.images.is_empty() || !report.actors.is_empty() || !report.trashed.is_empty();
        let (db, report) = (Arc::clone(&self.db), report.clone());
        tokio::task::spawn_blocking(move || record(&db, id, &report))
            .await
            .map_err(|_| ArtworkError::Worker)?
            .map_err(ArtworkError::from)?;
        if changed {
            self.changes
                .publish_one(id, &[ChangePart::Artwork, ChangePart::Files]);
        }
        Ok(())
    }

    /// Connects the service to the hub that announces what changed to open pages.
    pub fn set_changes(&self, hub: Arc<ChangeHub>) {
        self.changes.set(hub);
    }

    /// Sets the NFO service: after a chosen image (or its Undo) the title's NFO names the
    /// address of the image that is now on disk.
    pub fn set_nfo(&self, nfo: Arc<crate::nfo::NfoService>) {
        *self
            .nfo
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(nfo);
    }

    /// Writes the NFO of `id` again, so its artwork addresses (`<thumb aspect="poster">`,
    /// `<fanart><thumb>`) name the images now on disk. Only when the NFO settings write NFOs
    /// with artwork addresses and the title has an NFO already: a title without one does not get
    /// one from choosing a picture. Call it without holding the title; the NFO writer holds it.
    /// A problem is logged: the image is in place either way.
    pub(crate) async fn refresh_nfo(&self, id: ItemId) {
        let Some(nfo) = self
            .nfo
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
        else {
            return;
        };
        let settings = crate::nfo::NfoSettings::from_values(&self.settings.snapshot().values);
        if !settings.movie.artwork_urls {
            return;
        }
        let db = Arc::clone(&self.db);
        let written = tokio::task::spawn_blocking(move || {
            let has_nfo = db
                .read(|c| get_item(c, id))
                .ok()
                .flatten()
                .is_some_and(|item| {
                    item.core
                        .media_files
                        .iter()
                        .any(|f| f.kind == MediaFileType::Nfo)
                });
            if !has_nfo {
                return Ok(None);
            }
            nfo.rewrite(&crate::nfo::NfoScope::Items(vec![id]), &settings)
                .map(Some)
        })
        .await;
        match written {
            Ok(Ok(Some(report))) => {
                if report.totals.changed > 0 {
                    self.changes.publish_one(id, &[ChangePart::Files]);
                }
            }
            Ok(Ok(None)) => {}
            Ok(Err(error)) => {
                tracing::warn!(item = id.0, %error, "the NFO was not written after an artwork change");
            }
            Err(error) => tracing::warn!(item = id.0, %error, "the NFO task stopped"),
        }
    }

    /// The artwork providers for `media` whose credentials are in place, in the order of their
    /// default ranking, limited to the ones named in `only` (any case) when it is not empty.
    fn ready_providers(&self, media: MediaKind, only: &[String]) -> Vec<Arc<dyn ArtworkProvider>> {
        let inactive: Vec<&str> = self
            .providers
            .list()
            .map(|statuses| {
                statuses
                    .into_iter()
                    .filter(|status| !status.active)
                    .map(|status| status.provider)
                    .collect()
            })
            .unwrap_or_default();
        self.providers
            .registry()
            .artwork_providers_for(media)
            .into_iter()
            .filter(|p| !inactive.contains(&p.id()))
            .filter(|p| {
                only.is_empty() || only.iter().any(|name| name.eq_ignore_ascii_case(p.id()))
            })
            .collect()
    }

    /// As [`Self::candidates`], with the providers that were asked and the ones that failed.
    async fn gather(&self, item: &Item, media: MediaKind, only: &[String]) -> Gathered {
        let nothing = || Gathered {
            candidates: Vec::new(),
            providers: Vec::new(),
            failed: Vec::new(),
        };
        if item.core.ids.is_empty() {
            return nothing();
        }
        let ready = self.ready_providers(media, only);
        if ready.is_empty() {
            return nothing();
        }
        let options: RequestOptions =
            MatchSettings::from_values(&self.settings.snapshot().values).request;
        let gathered = gather_artwork(&ready, media, &item.core.ids, &options).await;
        for (provider, error) in &gathered.failures {
            tracing::debug!(provider, %error, "artwork provider failed");
        }
        let name_of = |id: &str| {
            ready
                .iter()
                .find(|p| p.id() == id)
                .map_or_else(|| id.to_owned(), |p| p.name().to_owned())
        };
        Gathered {
            providers: ready
                .iter()
                .map(|p| PickerProvider {
                    key: p.id().to_owned(),
                    name: p.name().to_owned(),
                })
                .collect(),
            failed: gathered
                .failures
                .iter()
                .map(|(id, _)| name_of(id))
                .collect(),
            candidates: gathered.candidates,
        }
    }

    /// The thumbnail keys of remote images (the posters of match candidates), by URL, for the
    /// ones that are ready within `wait`. The rest keep downloading in the background, so asking
    /// again later finds them; images fetched earlier (by the picker too) are not fetched again.
    pub async fn remote_thumbs(
        self: &Arc<Self>,
        urls: Vec<String>,
        wait: std::time::Duration,
    ) -> HashMap<String, ThumbKey> {
        let mut unique = HashSet::new();
        let urls: Vec<String> = urls
            .into_iter()
            .filter(|url| !url.is_empty() && unique.insert(url.clone()))
            .collect();
        if urls.is_empty() {
            return HashMap::new();
        }
        let service = Arc::clone(self);
        let asked = urls.clone();
        let task = tokio::spawn(async move { service.previews(urls).await });
        // On a timeout the task goes on; what it has stored so far is read from the cache.
        let _ = tokio::time::timeout(wait, task).await;
        let known = self
            .previews
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        asked
            .into_iter()
            .filter_map(|url| {
                let key = known.get(&url)?.key.clone();
                Some((url, key))
            })
            .collect()
    }

    /// Downloads the candidates into the thumbnail cache, a few at a time, and returns what could
    /// be fetched, by URL. What an earlier call fetched is not fetched again.
    async fn previews(&self, urls: Vec<String>) -> HashMap<String, Preview> {
        let mut found: HashMap<String, Preview> = HashMap::new();
        let mut missing = Vec::new();
        {
            let known = self
                .previews
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for url in urls {
                match known.get(&url) {
                    Some(preview) => {
                        found.insert(url, preview.clone());
                    }
                    None => missing.push(url),
                }
            }
        }
        let gate = Arc::new(Semaphore::new(PREVIEW_PARALLEL));
        let mut tasks = tokio::task::JoinSet::new();
        for url in missing {
            let (gate, fetcher, thumbs) = (
                Arc::clone(&gate),
                Arc::clone(&self.fetcher),
                Arc::clone(&self.thumbs),
            );
            tasks.spawn(async move {
                let _slot = gate.acquire().await.ok()?;
                let preview = preview_of(fetcher.as_ref(), thumbs, &url).await;
                preview.map(|preview| (url, preview))
            });
        }
        while let Some(done) = tasks.join_next().await {
            if let Ok(Some((url, preview))) = done {
                remember_preview(
                    &mut self
                        .previews
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner),
                    url.clone(),
                    preview.clone(),
                );
                found.insert(url, preview);
            }
        }
        found
    }

    /// [`Self::load`], or for a season the season with its show's folder.
    async fn load_any(&self, id: ItemId) -> Result<Loaded, ArtworkError> {
        match self.season_context(id).await? {
            Some(season) => Ok(season.as_loaded()),
            None => self.load(id).await,
        }
    }

    async fn load(&self, id: ItemId) -> Result<Loaded, ArtworkError> {
        let db = Arc::clone(&self.db);
        tokio::task::spawn_blocking(move || {
            let item = db
                .read(|c| get_item(c, id))?
                .ok_or(ArtworkError::ItemNotFound)?;
            let media = match item.kind() {
                ItemKind::Movie => MediaKind::Movie,
                ItemKind::Show => MediaKind::Show,
                _ => return Err(ArtworkError::Unsupported),
            };
            let target = target_of(&item).ok_or(ArtworkError::NoFolder)?;
            let recorded = db.read(|c| artwork_refs(c, id))?;
            Ok(Loaded {
                item,
                media,
                target,
                recorded,
            })
        })
        .await
        .map_err(|_| ArtworkError::Worker)?
    }
}

/// Downloads one candidate and builds its grid thumbnail. `None` when it cannot be fetched or is
/// not a picture.
async fn preview_of(
    fetcher: &dyn ImageFetcher,
    thumbs: Arc<ThumbCache>,
    url: &str,
) -> Option<Preview> {
    let bytes = match fetcher.fetch(url).await {
        Ok(bytes) => bytes,
        Err(error) => {
            tracing::debug!(url, %error, "artwork candidate not fetched");
            return None;
        }
    };
    tokio::task::spawn_blocking(move || {
        thumbs
            .ensure_bytes(&bytes, &[ThumbSize::Grid])
            .map_err(|error| tracing::debug!(%error, "artwork candidate is not a picture"))
            .ok()
            .map(|ensured| Preview {
                key: ensured.key,
                width: ensured.meta.source_width,
                height: ensured.meta.source_height,
            })
    })
    .await
    .ok()
    .flatten()
}

/// The address an artwork source is fetched from: a web address as it is, a `file:` address as it
/// is, and a path on disk (pasted by the user or dropped from the file manager) as a `file:`
/// address. Surrounding quotes and spaces are ignored.
///
/// # Errors
/// The text is none of these.
pub fn source_url(text: &str) -> Result<String, ArtworkError> {
    let text = text.trim().trim_matches(['"', '\'']).trim();
    let lower = text.to_ascii_lowercase();
    if lower.starts_with("https://") || lower.starts_with("http://") || lower.starts_with("file:") {
        return Ok(text.to_owned());
    }
    let path = Path::new(text);
    if !text.is_empty() && path.is_absolute() {
        return Ok(file_url(path));
    }
    Err(ArtworkError::BadSource(
        "Use a web address (https://...) or the full path of a picture on this computer."
            .to_owned(),
    ))
}

struct Loaded {
    item: Item,
    media: MediaKind,
    target: ArtworkTarget,
    recorded: Vec<cv_db::repo::artwork_refs::ArtworkRef>,
}

/// Where the artwork of a movie or show goes, or `None` when it has no folder.
#[must_use]
pub fn target_of(item: &Item) -> Option<ArtworkTarget> {
    let own = item.core.path.trim();
    let data_source = Some(item.core.data_source.trim())
        .filter(|d| !d.is_empty())
        .map(PathBuf::from);
    match &item.detail {
        ItemDetail::Movie(movie) => {
            let video = main_video(item);
            let root = if own.is_empty() {
                PathBuf::from(&video?.directory)
            } else {
                PathBuf::from(own)
            };
            let video_dir = video.map_or_else(|| root.clone(), |v| PathBuf::from(&v.directory));
            let base = video.map_or_else(String::new, |v| video_base_name(&v.filename));
            if base.is_empty() && !movie.is_disc {
                return None;
            }
            let (folder, dir) = if movie.is_disc {
                (FolderKind::Disc, root.clone())
            } else if movie.multi_movie_dir {
                (FolderKind::MultiMovie, video_dir)
            } else {
                (FolderKind::Single, video_dir)
            };
            Some(ArtworkTarget {
                subject: Subject::Movie,
                dir,
                root,
                base,
                folder,
                data_source,
            })
        }
        ItemDetail::Show(_) => {
            if own.is_empty() {
                return None;
            }
            let root = PathBuf::from(own);
            Some(ArtworkTarget {
                subject: Subject::Show,
                dir: root.clone(),
                root,
                base: String::new(),
                folder: FolderKind::Single,
                data_source,
            })
        }
        _ => None,
    }
}

/// The artwork file type of a show's image, or `None` for anything else.
fn show_art_of(kind: MediaFileType) -> Option<ShowArt> {
    Some(match kind {
        MediaFileType::Poster => ShowArt::Poster,
        MediaFileType::Fanart => ShowArt::Fanart,
        MediaFileType::Banner => ShowArt::Banner,
        MediaFileType::Clearart => ShowArt::Clearart,
        MediaFileType::Clearlogo => ShowArt::Clearlogo,
        MediaFileType::Disc => ShowArt::Discart,
        MediaFileType::Characterart => ShowArt::Characterart,
        MediaFileType::Keyart => ShowArt::Keyart,
        MediaFileType::Thumb => ShowArt::Thumb,
        MediaFileType::Squareart => ShowArt::Squareart,
        _ => return None,
    })
}

/// The lower-case file name without its extension.
fn stem_of(name: &str) -> String {
    Path::new(name)
        .file_stem()
        .map_or_else(String::new, |s| s.to_string_lossy().to_lowercase())
}

/// The artwork files of the item that the naming settings do not use although the image under a
/// name they do use exists. Only files in the artwork folder, listed in the library and on disk.
fn stale_artwork(item: &Item, target: &ArtworkTarget, names: &ArtworkNames) -> Vec<PathBuf> {
    let here: Vec<&cv_domain::MediaFile> = item
        .core
        .media_files
        .iter()
        .filter(|f| {
            f.kind.is_graphic() && Path::new(&f.directory) == target.dir && f.path().is_file()
        })
        .collect();
    // The lower-case stems the settings write for one image type, per file type.
    let wanted = |kind: MediaFileType| -> Option<Vec<String>> {
        let list = match target.subject {
            Subject::Movie => {
                let art = MovieArt::from_file_type(kind)?;
                names.movie.targets(art, &target.base, "jpg", target.folder)
            }
            Subject::Show => names.tv.show_targets(show_art_of(kind)?, "jpg"),
        };
        Some(list.iter().map(|n| stem_of(n)).collect())
    };
    let mut stale = Vec::new();
    for file in &here {
        let Some(used) = wanted(file.kind) else {
            continue;
        };
        if used.is_empty() || used.contains(&stem_of(&file.filename)) {
            continue;
        }
        let has_used = here
            .iter()
            .any(|other| other.kind == file.kind && used.contains(&stem_of(&other.filename)));
        if has_used {
            stale.push(file.path());
        }
    }
    stale
}

/// The fetcher of a build without an HTTP client: `file:` addresses are read from disk, a web
/// address cannot be fetched.
struct LocalFilesOnly;

#[async_trait::async_trait]
impl ImageFetcher for LocalFilesOnly {
    async fn fetch(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        match path_from_file_url(url) {
            Some(path) => tokio::fs::read(&path)
                .await
                .map_err(|e| FetchError::File(e.to_string())),
            None => Err(FetchError::Network(
                "there is no network client to download with".to_owned(),
            )),
        }
    }
}

#[cfg(test)]
mod preview_tests {
    use super::*;

    fn preview() -> Preview {
        Preview {
            key: ThumbKey::of_bytes(b"x"),
            width: 1,
            height: 1,
        }
    }

    #[test]
    fn the_remembered_previews_stay_below_their_cap() {
        let mut previews = HashMap::new();
        for n in 0..MAX_PREVIEWS {
            remember_preview(&mut previews, format!("https://i.test/{n}.jpg"), preview());
        }
        assert_eq!(previews.len(), MAX_PREVIEWS);
        // A known URL does not start the map again.
        remember_preview(&mut previews, "https://i.test/0.jpg".into(), preview());
        assert_eq!(previews.len(), MAX_PREVIEWS);
        remember_preview(&mut previews, "https://i.test/new.jpg".into(), preview());
        assert_eq!(previews.len(), 1);
    }
}
