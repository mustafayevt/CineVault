//! The ratings action: which sources the app asks, and the fetch for a list of titles.

use std::path::Path;
use std::sync::Arc;

use std::sync::PoisonError;

use cv_db::repo::step_outcome::items_with_status;
use cv_domain::{ItemId, ItemKind};
use cv_providers::{
    CredentialResolver, FetchedRatings, HttpClient, ImdbRatingsDataset, MdbListProvider,
    OmdbProvider, RatingFetcher, RatingProvider, RatingSource, SkipReason, SourceStatus,
    TmdbProvider, TraktProvider,
};

use super::{ActionRun, ActionsService, ItemOutcome, OutcomeKind};
use crate::changes::ChangePart;
use crate::matching::MatchSettings;
use crate::notices::{AppNotice, NoticeAction, NoticeLevel};
use crate::ratings::{RatingFetchError, fetch_and_store_ratings, rating_settings_from_values};
use crate::step_outcomes::{self, STEP_RATINGS, StepRecord, StepStatus, source_step};
use crate::wording::counted;

/// Folder name, under the data location's cache folder, of the IMDb ratings dataset.
pub const DATASET_DIR: &str = "imdb-ratings";

/// The key of the "dataset download failed" notice among those already announced.
const DATASET_FAILED: &str = "imdb-dataset-failed";

/// The key, among the notices already announced, of the "requests failed" notice of a rating
/// source other than the dataset.
const fn source_failed_key(source: RatingSource) -> &'static str {
    match source {
        RatingSource::ImdbDataset => DATASET_FAILED,
        RatingSource::Scraper => "ratings-failed-scraper",
        RatingSource::Omdb => "ratings-failed-omdb",
        RatingSource::Mdblist => "ratings-failed-mdblist",
    }
}

/// What a retry of the dataset download did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DatasetRetry {
    /// How many titles had failed for want of the dataset.
    pub asked: usize,
    /// How many of them have their ratings now.
    pub refetched: usize,
}

/// The rating sources of the app: the fetcher (OMDb, MDBList and the IMDb dataset) and the rating
/// side of the scraping providers, used for the `scraper` source of a title scraped by them.
#[derive(Clone)]
pub struct RatingSources {
    pub fetcher: RatingFetcher,
    /// The IMDb ratings dataset, kept to retry a failed download.
    pub dataset: Option<ImdbRatingsDataset>,
    /// The scraping providers that also offer ratings, by their provider key.
    pub scrapers: Vec<Arc<dyn RatingProvider>>,
}

impl RatingSources {
    /// The real sources, over the shared HTTP client. A build without a client (the tests, which
    /// register their own providers) has no network source at all.
    #[must_use]
    pub fn for_app(
        http: Option<&HttpClient>,
        resolver: &CredentialResolver,
        cache_dir: &Path,
    ) -> Self {
        let mut fetcher = RatingFetcher::new(resolver.clone());
        let mut scrapers: Vec<Arc<dyn RatingProvider>> = Vec::new();
        let mut kept_dataset = None;
        if let Some(http) = http {
            match ImdbRatingsDataset::new(cache_dir.join(DATASET_DIR)) {
                Ok(dataset) => {
                    kept_dataset = Some(dataset.clone());
                    fetcher = fetcher.with_dataset(Arc::new(dataset));
                }
                Err(error) => tracing::warn!(%error, "the IMDb ratings dataset is not available"),
            }
            fetcher = fetcher
                .with_omdb(Arc::new(OmdbProvider::new(http.clone(), resolver.clone())))
                .with_mdblist(Arc::new(MdbListProvider::new(
                    http.clone(),
                    resolver.clone(),
                )));
            scrapers.push(Arc::new(TmdbProvider::new(http.clone(), resolver.clone())));
            scrapers.push(Arc::new(TraktProvider::new(http.clone(), resolver.clone())));
        }
        Self {
            fetcher,
            dataset: kept_dataset,
            scrapers,
        }
    }
}

impl ActionsService {
    /// Fetches the ratings of the titles from the sources the settings choose (per kind) and
    /// stores them. The user's own rating, locked titles and locked rating fields are left alone.
    pub async fn fetch_ratings(&self, ids: &[ItemId]) -> ActionRun {
        let values = self.values();
        let chosen = rating_settings_from_values(&values);
        let options = MatchSettings::from_values(&values).request;
        let sources = self.rating_sources();
        let mut run = ActionRun::default();
        for &id in ids {
            let mut sources_seen: Option<FetchedRatings> = None;
            let item = match self.load(id).await {
                Ok(item) => item,
                Err(outcome) => {
                    run.items.push(outcome);
                    continue;
                }
            };
            let fetch = match item.kind() {
                ItemKind::Movie => &chosen.movie,
                ItemKind::Show => &chosen.show,
                _ => {
                    run.items.push(ItemOutcome::of(
                        &item,
                        OutcomeKind::Skipped,
                        "Ratings are fetched for movies and shows only.",
                    ));
                    continue;
                }
            };
            if fetch.sources.is_empty() {
                let outcome = ItemOutcome::of(
                    &item,
                    OutcomeKind::Skipped,
                    "No rating source is chosen. Pick one in Settings, Scraping and providers.",
                );
                self.record_ratings(&outcome, None);
                run.items.push(outcome);
                continue;
            }
            // The sources of the title show "fetching..." until the ratings are in.
            let working = self.hold_working(id);
            let fetching = self.hold_ratings(id);
            self.changes.publish_one(id, &[ChangePart::Stage]);
            let scraper = sources
                .scrapers
                .iter()
                .find(|p| p.id() == item.core.last_scraper_id)
                .map(Arc::clone);
            let stored = fetch_and_store_ratings(
                &self.db,
                &sources.fetcher,
                scraper.as_deref(),
                id,
                fetch,
                &options,
            )
            .await;
            let outcome = match stored {
                Err(RatingFetchError::NotFound) => {
                    ItemOutcome::unreadable(id, "The title is no longer in the library.")
                }
                Err(RatingFetchError::UnsupportedKind(_)) => ItemOutcome::of(
                    &item,
                    OutcomeKind::Skipped,
                    "Ratings are fetched for movies and shows only.",
                ),
                Err(RatingFetchError::Storage(error)) => ItemOutcome::of(
                    &item,
                    OutcomeKind::Failed,
                    format!("The ratings could not be stored: {error}"),
                ),
                Ok(stored) => {
                    self.note_dataset(&stored.fetched);
                    self.note_source_failures(&stored.fetched);
                    sources_seen = Some(stored.fetched.clone());
                    let notes: Vec<String> = stored
                        .fetched
                        .notices()
                        .into_iter()
                        .map(|n| n.message)
                        .collect();
                    match stored.outcome {
                        None => ItemOutcome::of(
                            &item,
                            OutcomeKind::Skipped,
                            if notes.is_empty() {
                                "No source has a rating for this title.".to_owned()
                            } else {
                                notes.join(" ")
                            },
                        )
                        .with_status(nothing_found_status(&stored.fetched)),
                        Some(outcome) if outcome.blocked.is_some() => ItemOutcome::of(
                            &item,
                            OutcomeKind::Skipped,
                            "The title is locked, so its ratings were left alone.",
                        ),
                        // Fetched again with the same values is still a fetch: the ratings
                        // carry the new date.
                        Some(outcome) if outcome.touched() => ItemOutcome::of(
                            &item,
                            OutcomeKind::Done,
                            format!(
                                "Stored {}.{}",
                                counted(stored.fetched.ratings.len(), "rating", "ratings"),
                                if notes.is_empty() {
                                    String::new()
                                } else {
                                    format!(" {}", notes.join(" "))
                                }
                            ),
                        ),
                        Some(_) => ItemOutcome::of(
                            &item,
                            OutcomeKind::Unchanged,
                            "The ratings are up to date.",
                        ),
                    }
                }
            };
            self.record_ratings(&outcome, sources_seen.as_ref());
            // The NFO carries the ratings too: it is written again while the title is still
            // working, so no rename moves the folder meanwhile.
            if outcome.kind == OutcomeKind::Done {
                self.rewrite_nfo(id).await;
            }
            // The page reads the title again when the event arrives: it must no longer say
            // "fetching".
            drop(fetching);
            drop(working);
            self.changes
                .publish_one(id, &[ChangePart::Ratings, ChangePart::Stage]);
            run.items.push(outcome);
        }
        run
    }

    /// Tells Activity once when the IMDb dataset cannot be downloaded (with a Retry), and forgets
    /// that it did when the dataset answers again.
    fn note_dataset(&self, fetched: &FetchedRatings) {
        let status = fetched
            .outcomes
            .iter()
            .find(|o| o.source == RatingSource::ImdbDataset)
            .map(|o| &o.status);
        let mut announced = self
            .announced
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        match status {
            Some(SourceStatus::Failed(error)) => {
                if announced.insert(DATASET_FAILED) {
                    self.notices.publish(
                        AppNotice::new(
                            NoticeLevel::Warning,
                            "IMDb ratings: dataset download failed",
                        )
                        .detail(error.to_string())
                        .source("IMDb ratings dataset")
                        .action(NoticeAction::RetryImdbDataset),
                    );
                }
            }
            Some(SourceStatus::Fetched(_) | SourceStatus::NotFound) => {
                announced.remove(DATASET_FAILED);
            }
            _ => {}
        }
    }

    /// Tells Activity once when OMDb, MDBList or the scraper fails to give ratings (a partial
    /// failure still stores what the other sources gave), and forgets that it did when the source
    /// answers again. The titles show the source as failed, with a Retry on the chip.
    fn note_source_failures(&self, fetched: &FetchedRatings) {
        let mut announced = self
            .announced
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        for asked in &fetched.outcomes {
            if asked.source == RatingSource::ImdbDataset {
                continue;
            }
            let key = source_failed_key(asked.source);
            match &asked.status {
                SourceStatus::Failed(error) => {
                    if announced.insert(key) {
                        let label = asked.source.label();
                        let mut notice = AppNotice::new(
                            NoticeLevel::Warning,
                            format!("{label} ratings: the request failed"),
                        )
                        .detail(format!(
                            "{error}. The titles it failed for show it as failed; Retry on the rating fetches it again."
                        ))
                        .source(format!("{label} ratings"));
                        if let Some(provider) = provider_of(asked.source) {
                            notice = notice.provider_key(provider);
                        }
                        self.notices.publish(notice);
                    }
                }
                SourceStatus::Fetched(_) | SourceStatus::NotFound => {
                    announced.remove(key);
                }
                SourceStatus::Skipped(_) => {}
            }
        }
    }

    /// Downloads the IMDb ratings dataset again, at once, and fetches the ratings of the titles
    /// that failed for want of it. This is the Retry of the "dataset download failed" notice.
    ///
    /// # Errors
    /// The dataset is not available in this build, or the download fails again (the notice
    /// stays).
    pub async fn retry_imdb_dataset(&self) -> Result<DatasetRetry, String> {
        let dataset = self
            .rating_sources()
            .dataset
            .ok_or_else(|| "The IMDb ratings dataset is not available.".to_owned())?;
        dataset.clear_failure();
        dataset
            .ensure()
            .await
            .map_err(|error| format!("The download failed again: {error}"))?;
        self.announced
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(DATASET_FAILED);
        let failed = self
            .db
            .read(|conn| {
                items_with_status(
                    conn,
                    &source_step(RatingSource::ImdbDataset.key()),
                    StepStatus::Failed,
                )
            })
            .map_err(|error| error.to_string())?;
        let ids: Vec<ItemId> = failed.into_iter().map(|o| o.item).collect();
        let run = self.fetch_ratings(&ids).await;
        let refetched = run
            .items
            .iter()
            .filter(|o| o.kind == OutcomeKind::Done || o.kind == OutcomeKind::Unchanged)
            .count();
        Ok(DatasetRetry {
            asked: ids.len(),
            refetched,
        })
    }

    /// Stores the step's outcome and one outcome per rating source that was asked.
    fn record_ratings(&self, outcome: &ItemOutcome, fetched: Option<&FetchedRatings>) {
        let mut records = vec![StepRecord::new(
            STEP_RATINGS,
            outcome.step_status(),
            outcome.message.clone(),
        )];
        if let Some(fetched) = fetched {
            for asked in &fetched.outcomes {
                let (status, reason) = source_status(asked.source.label(), &asked.status);
                records.push(StepRecord::new(
                    source_step(asked.source.key()),
                    status,
                    reason,
                ));
            }
        }
        step_outcomes::record_all(&self.db, outcome.id, &records);
    }
}

/// The provider whose card on the Providers page holds the key of a rating source.
fn provider_of(source: RatingSource) -> Option<&'static str> {
    match source {
        RatingSource::Omdb => Some(cv_providers::catalogue::key::OMDB),
        RatingSource::Mdblist => Some(cv_providers::catalogue::key::MDBLIST),
        RatingSource::ImdbDataset | RatingSource::Scraper => None,
    }
}

/// How one rating source went, as the status and reason stored for it.
fn source_status(label: &str, status: &SourceStatus) -> (StepStatus, String) {
    match status {
        SourceStatus::Fetched(count) => (StepStatus::Done, counted(*count, "rating", "ratings")),
        SourceStatus::NotFound => (
            StepStatus::NotFound,
            format!("{label} has no rating for this title."),
        ),
        SourceStatus::Skipped(SkipReason::MissingCredentials { provider, .. }) => (
            StepStatus::NeedsKey,
            format!("{provider} needs an API key."),
        ),
        SourceStatus::Skipped(SkipReason::NoScraper) => (
            StepStatus::Skipped,
            "The title's scraping provider offers no ratings.".to_owned(),
        ),
        SourceStatus::Skipped(SkipReason::NotConfigured) => {
            (StepStatus::Skipped, format!("{label} is not available."))
        }
        SourceStatus::Failed(error) => (StepStatus::Failed, format!("{label} failed: {error}")),
    }
}

/// The status of a ratings step that stored nothing: a failure outranks a missing key, which
/// outranks "not found", which outranks a plain skip.
fn nothing_found_status(fetched: &FetchedRatings) -> StepStatus {
    let statuses: Vec<StepStatus> = fetched
        .outcomes
        .iter()
        .map(|o| source_status(o.source.label(), &o.status).0)
        .collect();
    [
        StepStatus::Failed,
        StepStatus::NeedsKey,
        StepStatus::NotFound,
    ]
    .into_iter()
    .find(|wanted| statuses.contains(wanted))
    .unwrap_or(StepStatus::Skipped)
}
