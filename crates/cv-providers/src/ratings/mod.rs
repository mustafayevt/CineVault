//! Fetching a title's ratings from the sources the user selected.
//!
//! Four kinds of source feed the rating list of a title:
//!
//! | [`RatingSource`] | What it is | Needs |
//! |---|---|---|
//! | [`Scraper`](RatingSource::Scraper) | the scraping provider's own [`RatingProvider`] (TMDB, Trakt, ...) | whatever that provider needs |
//! | [`Omdb`](RatingSource::Omdb) | IMDb, Rotten Tomatoes and Metacritic through OMDb | an OMDb key |
//! | [`Mdblist`](RatingSource::Mdblist) | Letterboxd, Rotten Tomatoes, Metacritic, MyAnimeList, Roger Ebert (and more) through MDBList | an MDBList key |
//! | [`ImdbDataset`](RatingSource::ImdbDataset) | the free IMDb ratings dataset, downloaded on demand ([`ImdbRatingsDataset`]) | nothing |
//!
//! [`RatingFetcher::fetch`] asks each selected source in the order of [`RatingSource::ALL`] (the dataset, the scraper, OMDb, MDBList) and merges the answers.
//! A source whose credentials are missing (the user has not entered a key) is
//! **skipped with a notice** and never sends a request; the other sources still run. A source
//! that has nothing for the title, or fails, is reported in the outcome and does not stop the
//! others. When two sources give the same rating key, the one with more votes wins (the dataset
//! usually has the freshest IMDb count); on a tie the earlier source wins.
//!
//! The fetch never touches the user's own rating, and it only returns ratings: storing them
//! (honouring the item lock and the rating field lock) is the caller's job.
//! [`RatingSettings`] says which sources each kind uses and whether a scrape is followed by a
//! fetch ([`RatingFetcher::after_scrape`]).

mod imdb_dataset;
mod mdblist;

use std::collections::BTreeMap;
use std::sync::Arc;

use cv_domain::{Item, ItemKind, ProviderIds, Rating, Ratings, rating_key};
use serde::{Deserialize, Serialize};

use crate::credentials::CredentialResolver;
use crate::error::ProviderError;
use crate::model::{MediaKind, RequestOptions};
use crate::traits::{Provider, RatingProvider};

pub use imdb_dataset::{
    DEFAULT_REFRESH as IMDB_DATASET_REFRESH, DEFAULT_URL as IMDB_DATASET_URL, DatasetMeta,
    DatasetOutcome, ImdbRatingsDataset, PROVIDER_ID as IMDB_DATASET_ID,
};
pub use mdblist::{
    DEFAULT_BASE_URL as MDBLIST_BASE_URL, METACRITIC_USER_RATING_ID, MdbListProvider, rating_of,
    ratings_of,
};

/// A place ratings come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RatingSource {
    /// The free IMDb ratings dataset.
    ImdbDataset,
    /// The scraping provider's own ratings.
    Scraper,
    /// OMDb: IMDb, Rotten Tomatoes, Metacritic.
    Omdb,
    /// MDBList: Letterboxd, Rotten Tomatoes, Metacritic, MyAnimeList, Roger Ebert and more.
    Mdblist,
}

impl RatingSource {
    /// Every source, in the order they are asked.
    pub const ALL: [Self; 4] = [Self::ImdbDataset, Self::Scraper, Self::Omdb, Self::Mdblist];

    /// The key used in settings.
    pub const fn key(self) -> &'static str {
        match self {
            Self::ImdbDataset => "imdb-dataset",
            Self::Scraper => "scraper",
            Self::Omdb => "omdb",
            Self::Mdblist => "mdblist",
        }
    }

    /// The name shown in messages.
    pub const fn label(self) -> &'static str {
        match self {
            Self::ImdbDataset => "IMDb ratings dataset",
            Self::Scraper => "Scraper",
            Self::Omdb => "OMDb",
            Self::Mdblist => "MDBList",
        }
    }

    /// The source for a settings key; unknown keys are `None`.
    pub fn from_key(key: &str) -> Option<Self> {
        let key = key.trim().to_ascii_lowercase();
        Self::ALL.into_iter().find(|s| s.key() == key)
    }

    /// Parses a comma-separated list of settings keys; unknown keys are dropped and repeats
    /// collapse.
    pub fn parse_list(text: &str) -> Vec<Self> {
        let mut out = Vec::new();
        for source in text.split(',').filter_map(Self::from_key) {
            if !out.contains(&source) {
                out.push(source);
            }
        }
        out
    }

    /// The settings form of a list: [`Self::parse_list`] reads it back.
    pub fn list_key(sources: &[Self]) -> String {
        sources
            .iter()
            .map(|s| s.key())
            .collect::<Vec<_>>()
            .join(",")
    }
}

/// The rating fetch settings of one kind of title.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RatingFetchSettings {
    /// The sources "Fetch ratings" and the automatic fetch ask.
    pub sources: Vec<RatingSource>,
    /// Fetch ratings automatically after a scrape.
    pub after_scrape: bool,
}

impl RatingFetchSettings {
    /// The sources asked by default: the IMDb dataset, OMDb and MDBList. A source whose key is not
    /// set is skipped without a request (and the strip says it needs a key), so a key entered on
    /// the Providers page is used at the next fetch with nothing else to switch on. The scraper's
    /// own rating comes with the scrape itself.
    pub const DEFAULT_SOURCES: [RatingSource; 3] = [
        RatingSource::ImdbDataset,
        RatingSource::Omdb,
        RatingSource::Mdblist,
    ];

    /// The defaults of a movie: [`Self::DEFAULT_SOURCES`], fetched after every scrape.
    pub fn movie() -> Self {
        Self {
            sources: Self::DEFAULT_SOURCES.to_vec(),
            after_scrape: true,
        }
    }

    /// The defaults of a show: the same sources, after every scrape like a movie.
    pub fn show() -> Self {
        Self {
            sources: Self::DEFAULT_SOURCES.to_vec(),
            after_scrape: true,
        }
    }
}

impl Default for RatingFetchSettings {
    fn default() -> Self {
        Self::movie()
    }
}

/// The rating fetch settings of movies and shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RatingSettings {
    pub movie: RatingFetchSettings,
    pub show: RatingFetchSettings,
}

impl Default for RatingSettings {
    fn default() -> Self {
        Self {
            movie: RatingFetchSettings::movie(),
            show: RatingFetchSettings::show(),
        }
    }
}

impl RatingSettings {
    /// The settings that apply to `kind`. Only movies and shows have ratings fetched.
    pub fn for_kind(&self, kind: ItemKind) -> Option<&RatingFetchSettings> {
        match kind {
            ItemKind::Movie => Some(&self.movie),
            ItemKind::Show => Some(&self.show),
            ItemKind::Set | ItemKind::Season | ItemKind::Episode => None,
        }
    }
}

/// Why a source was not asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipReason {
    /// The provider needs credentials that have no value.
    MissingCredentials {
        provider: String,
        registration_url: String,
    },
    /// The scraper source was selected but no scraping provider offers ratings.
    NoScraper,
    /// The source was selected but this build has no provider for it.
    NotConfigured,
}

/// What one source did for one title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceStatus {
    /// The source answered with this many ratings.
    Fetched(usize),
    /// The source does not know the title (or has no ratings for it).
    NotFound,
    /// The source was not asked.
    Skipped(SkipReason),
    /// The request failed.
    Failed(ProviderError),
}

/// One source's part of a fetch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceOutcome {
    pub source: RatingSource,
    pub status: SourceStatus,
}

/// A message for the user about a source that was skipped or failed, shown once per run.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RatingNotice {
    pub source: RatingSource,
    pub message: String,
}

/// The ratings fetched for one title and how each source went.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FetchedRatings {
    /// The merged ratings of all sources. Never contains the user's rating.
    pub ratings: Ratings,
    /// One entry per selected source, in the order they were asked.
    pub outcomes: Vec<SourceOutcome>,
}

impl FetchedRatings {
    /// The notices for the skipped sources and failures of this title.
    pub fn notices(&self) -> Vec<RatingNotice> {
        self.outcomes
            .iter()
            .filter_map(|o| notice_of(o.source, &o.status))
            .collect()
    }

    /// Whether any source contributed a rating.
    pub fn found_any(&self) -> bool {
        !self.ratings.is_empty()
    }
}

fn notice_of(source: RatingSource, status: &SourceStatus) -> Option<RatingNotice> {
    let message = match status {
        SourceStatus::Skipped(SkipReason::MissingCredentials {
            provider,
            registration_url,
        }) => format!(
            "{provider} was skipped: no API key is set. Add one in Settings, Scraping and providers (get one at {registration_url})."
        ),
        SourceStatus::Skipped(SkipReason::NoScraper) => {
            "The scraper was skipped: the title's scraping provider offers no ratings.".to_owned()
        }
        SourceStatus::Failed(error) => format!("{} failed: {error}", source.label()),
        SourceStatus::Skipped(SkipReason::NotConfigured)
        | SourceStatus::Fetched(_)
        | SourceStatus::NotFound => return None,
    };
    Some(RatingNotice { source, message })
}

/// The kind of title a rating fetch applies to, or `None` for sets, seasons and episodes.
pub fn media_kind_of(kind: ItemKind) -> Option<MediaKind> {
    match kind {
        ItemKind::Movie => Some(MediaKind::Movie),
        ItemKind::Show => Some(MediaKind::Show),
        ItemKind::Set | ItemKind::Season | ItemKind::Episode => None,
    }
}

/// Fetches ratings from the selected sources. Cloning is cheap.
#[derive(Clone)]
pub struct RatingFetcher {
    resolver: CredentialResolver,
    omdb: Option<Arc<dyn RatingProvider>>,
    mdblist: Option<Arc<dyn RatingProvider>>,
    dataset: Option<Arc<dyn RatingProvider>>,
}

impl RatingFetcher {
    /// A fetcher with no sources yet. `resolver` decides which providers have credentials.
    pub fn new(resolver: CredentialResolver) -> Self {
        Self {
            resolver,
            omdb: None,
            mdblist: None,
            dataset: None,
        }
    }

    /// Uses `provider` for the OMDb source.
    #[must_use]
    pub fn with_omdb(mut self, provider: Arc<dyn RatingProvider>) -> Self {
        self.omdb = Some(provider);
        self
    }

    /// Uses `provider` for the MDBList source.
    #[must_use]
    pub fn with_mdblist(mut self, provider: Arc<dyn RatingProvider>) -> Self {
        self.mdblist = Some(provider);
        self
    }

    /// Uses `provider` for the IMDb dataset source.
    #[must_use]
    pub fn with_dataset(mut self, provider: Arc<dyn RatingProvider>) -> Self {
        self.dataset = Some(provider);
        self
    }

    /// Why `provider` cannot be asked right now, or `None` when its credentials are in place
    /// (or it needs none).
    pub fn missing_credentials(&self, provider: &dyn Provider) -> Option<SkipReason> {
        let schema = provider.credentials()?;
        // A provider the resolver does not know cannot be judged here; it decides on its own.
        let resolved = self.resolver.resolve(provider.id()).ok()?;
        if resolved.missing(schema).is_empty() {
            None
        } else {
            Some(SkipReason::MissingCredentials {
                provider: schema.display_name.to_owned(),
                registration_url: schema.registration_url.to_owned(),
            })
        }
    }

    /// Asks one provider and turns its answer into a status and ratings.
    async fn ask(
        &self,
        provider: &dyn RatingProvider,
        kind: MediaKind,
        ids: &ProviderIds,
        options: &RequestOptions,
    ) -> (SourceStatus, Ratings) {
        if let Some(reason) = self.missing_credentials(provider) {
            return (SourceStatus::Skipped(reason), Ratings::new());
        }
        match provider.ratings(kind, ids, options).await {
            Ok(ratings) if ratings.is_empty() => (SourceStatus::NotFound, ratings),
            Ok(ratings) => (SourceStatus::Fetched(ratings.len()), ratings),
            Err(error) => match error.root() {
                ProviderError::NotFound(_) => (SourceStatus::NotFound, Ratings::new()),
                ProviderError::MissingCredentials {
                    provider,
                    registration_url,
                    ..
                } => (
                    SourceStatus::Skipped(SkipReason::MissingCredentials {
                        provider: provider.clone(),
                        registration_url: registration_url.clone(),
                    }),
                    Ratings::new(),
                ),
                _ => (SourceStatus::Failed(error), Ratings::new()),
            },
        }
    }

    /// Fetches the ratings of the title with `ids` from `sources`. `scraper` is the scraping
    /// provider's rating side, used for [`RatingSource::Scraper`].
    pub async fn fetch(
        &self,
        kind: MediaKind,
        ids: &ProviderIds,
        scraper: Option<&dyn RatingProvider>,
        sources: &[RatingSource],
        options: &RequestOptions,
    ) -> FetchedRatings {
        let mut result = FetchedRatings::default();
        for source in RatingSource::ALL
            .into_iter()
            .filter(|s| sources.contains(s))
        {
            let provider: Option<&dyn RatingProvider> = match source {
                RatingSource::Scraper => scraper,
                RatingSource::Omdb => self.omdb.as_deref(),
                RatingSource::Mdblist => self.mdblist.as_deref(),
                RatingSource::ImdbDataset => self.dataset.as_deref(),
            };
            let (status, ratings) = match provider {
                Some(provider) => self.ask(provider, kind, ids, options).await,
                None if source == RatingSource::Scraper => {
                    (SourceStatus::Skipped(SkipReason::NoScraper), Ratings::new())
                }
                None => (
                    SourceStatus::Skipped(SkipReason::NotConfigured),
                    Ratings::new(),
                ),
            };
            for rating in ratings.iter().filter(|r| r.id != rating_key::USER) {
                merge_source_rating(&mut result.ratings, rating);
            }
            result.outcomes.push(SourceOutcome { source, status });
        }
        result
    }

    /// Fetches the ratings of `item` and merges them into its rating list (the user's rating is
    /// untouched). Returns `None` for kinds that have no fetched ratings. Locks are not looked at
    /// here; a caller that stores the item uses the lock-aware write path.
    pub async fn fetch_item(
        &self,
        item: &mut Item,
        scraper: Option<&dyn RatingProvider>,
        settings: &RatingFetchSettings,
        options: &RequestOptions,
    ) -> Option<FetchedRatings> {
        let kind = media_kind_of(item.kind())?;
        let fetched = self
            .fetch(kind, &item.core.ids, scraper, &settings.sources, options)
            .await;
        for rating in fetched.ratings.iter() {
            item.core.ratings.insert_normalised(rating.clone());
        }
        Some(fetched)
    }

    /// The optional fetch after a scrape: does nothing (`None`) when the setting for the item's
    /// kind is off or the kind has no ratings.
    pub async fn after_scrape(
        &self,
        item: &mut Item,
        scraper: Option<&dyn RatingProvider>,
        settings: &RatingSettings,
        options: &RequestOptions,
    ) -> Option<FetchedRatings> {
        let for_kind = settings.for_kind(item.kind())?;
        if !for_kind.after_scrape || for_kind.sources.is_empty() {
            return None;
        }
        self.fetch_item(item, scraper, for_kind, options).await
    }
}

/// The notices of a whole run: each source's message once, however many titles it affected.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunNotices {
    notices: BTreeMap<RatingSource, Vec<String>>,
}

impl RunNotices {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds the notices of one title.
    pub fn add(&mut self, fetched: &FetchedRatings) {
        for notice in fetched.notices() {
            let messages = self.notices.entry(notice.source).or_default();
            if !messages.contains(&notice.message) {
                messages.push(notice.message);
            }
        }
    }

    /// The messages, grouped by source in the fixed source order.
    pub fn messages(&self) -> Vec<RatingNotice> {
        self.notices
            .iter()
            .flat_map(|(source, messages)| {
                messages.iter().map(|message| RatingNotice {
                    source: *source,
                    message: message.clone(),
                })
            })
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.notices.is_empty()
    }
}

/// Adds one source's rating to the ratings fetched so far: the one with more votes wins the
/// value, and the title's page stays known when either source named it.
fn merge_source_rating(ratings: &mut Ratings, rating: &Rating) {
    let rating = rating.clone().normalised();
    let existing = ratings.get(&rating.id).cloned();
    let Some(existing) = existing else {
        ratings.insert(rating);
        return;
    };
    let (mut kept, other) = if rating.votes > existing.votes {
        (rating, existing)
    } else {
        (existing, rating)
    };
    if kept.url.is_empty() {
        kept.url = other.url;
    }
    ratings.insert(kept);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_of_either_source_survives_the_merge() {
        let page = "https://www.rottentomatoes.com/m/blade_runner";
        // The winner by votes names no page: the loser's page is kept.
        let mut r = Ratings::new();
        merge_source_rating(
            &mut r,
            &Rating::new("tomatometerallcritics", 89.0, 10, 100).with_url(page),
        );
        merge_source_rating(
            &mut r,
            &Rating::new("tomatometerallcritics", 88.0, 200, 100),
        );
        let got = r.get("tomatometerallcritics").unwrap();
        assert_eq!((got.value, got.url.as_str()), (88.0, page));
        // The winner keeps its own page; a loser with a page does not replace it.
        let mut r = Ratings::new();
        merge_source_rating(&mut r, &Rating::new("letterboxd", 4.1, 100, 5));
        merge_source_rating(&mut r, &Rating::new("letterboxd", 4.0, 5, 5).with_url(page));
        assert_eq!(r.get("letterboxd").unwrap().url, page);
        assert_eq!(r.get("letterboxd").unwrap().value, 4.1);
    }

    #[test]
    fn source_lists_round_trip_and_drop_unknown_keys() {
        let sources = RatingSource::parse_list("omdb, mdblist,omdb,bogus,imdb-dataset");
        assert_eq!(
            sources,
            [
                RatingSource::Omdb,
                RatingSource::Mdblist,
                RatingSource::ImdbDataset
            ]
        );
        assert_eq!(
            RatingSource::parse_list(&RatingSource::list_key(&sources)),
            sources
        );
        assert!(RatingSource::parse_list("").is_empty());
    }

    #[test]
    fn defaults_follow_the_kind() {
        let settings = RatingSettings::default();
        assert!(settings.movie.after_scrape && settings.show.after_scrape);
        assert!(settings.for_kind(ItemKind::Episode).is_none());
        assert!(settings.for_kind(ItemKind::Show).is_some());
        // A key entered for OMDb or MDBList is used without choosing the source as well.
        for kind in [&settings.movie, &settings.show] {
            assert_eq!(
                kind.sources,
                [
                    RatingSource::ImdbDataset,
                    RatingSource::Omdb,
                    RatingSource::Mdblist
                ]
            );
        }
    }

    #[test]
    fn settings_serialise_with_camel_case_names() {
        let json = serde_json::to_value(RatingFetchSettings::movie()).unwrap();
        assert_eq!(json["afterScrape"], true);
        assert_eq!(json["sources"][0], "imdb-dataset");
        let back: RatingFetchSettings = serde_json::from_value(json).unwrap();
        assert_eq!(back, RatingFetchSettings::movie());
    }
}
