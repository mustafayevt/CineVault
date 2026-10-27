//! The ratings strip: one chip per rating source with a value or the reason there is none
//! (spec `ratings`, "Primary rating and display").
//!
//! A title's detail page and the Inspector show the same chips. A chip is one of:
//!
//! | State | Meaning |
//! |---|---|
//! | `value` | the title has this rating |
//! | `fetching` | the ratings of the title are being fetched now |
//! | `needs_key` | the source that provides it needs an API key that is not set (Set up) |
//! | `connect_service` | the source is a service the user has not connected (Trakt) |
//! | `not_found` | a source was asked and has no rating for the title |
//! | `failed` | a source was asked and failed (Retry); `provider` is that source's key |
//! | `not_fetched` | a chosen source that can give it has not been asked yet (with its key) |
//! | `not_selected` | a source that could give it has its key but is not one of the chosen rating sources (Choose); `provider` is that source's key |
//!
//! The states come from the per-source step outcomes (`ratings:<source>`, written by the ratings
//! action), the keys the user has set now, the rating sources the settings choose, and whether
//! the title's ratings are being fetched. A key that was missing when a source was asked but is
//! set now is no longer "needs a key": the source is simply not fetched yet. Nothing here
//! fetches or writes.

use std::sync::Arc;

use cv_db::repo::rating_fetch_candidates;
use cv_db::repo::step_outcome::{step_outcomes, step_outcomes_for};
use cv_db::{Database, DbError};
use cv_domain::{ItemId, ItemKind, Ratings, rating_key};
use cv_providers::{CredentialResolver, RatingSource, catalogue};

use crate::app_settings::SettingsService;
use crate::lifecycle::Working;
use crate::ratings::rating_settings_from_values;
use crate::step_outcomes::{StepOutcome, StepStatus, source_step};

/// How a rating chip stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RatingChipState {
    Value,
    Fetching,
    NeedsKey,
    ConnectService,
    NotFound,
    Failed,
    NotFetched,
    NotSelected,
}

impl RatingChipState {
    /// The stable word sent to the UI.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Value => "value",
            Self::Fetching => "fetching",
            Self::NeedsKey => "needs_key",
            Self::ConnectService => "connect_service",
            Self::NotFound => "not_found",
            Self::Failed => "failed",
            Self::NotFetched => "not_fetched",
            Self::NotSelected => "not_selected",
        }
    }
}

/// One chip of the strip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RatingChip {
    /// The rating key (`imdb`, `tomatometerallcritics`, `metacritic`, `letterboxd`, `tmdb`, ...),
    /// the same as the key of a stored rating.
    pub key: String,
    pub state: RatingChipState,
    /// One plain sentence saying why there is no value; empty for a value.
    pub reason: String,
    /// The provider to set up or connect for `needs_key` and `connect_service`; the rating source
    /// key (`imdb-dataset`, `omdb`, `mdblist`, `scraper`) for `failed` and `not_selected`.
    pub provider: Option<String>,
}

/// What the user has set up, as far as the strip needs it.
pub trait RatingAccess {
    /// Whether the provider's required credentials all have a value.
    fn has_credentials(&self, provider: &str) -> bool;
}

impl RatingAccess for CredentialResolver {
    fn has_credentials(&self, provider: &str) -> bool {
        // A provider without credentials of its own needs nothing.
        self.is_active(provider).unwrap_or(true)
    }
}

/// A place a rating can come from, with the provider whose key it needs.
#[derive(Clone, Copy)]
struct Via {
    source: RatingSource,
    /// The provider whose credentials the source needs, when it needs any.
    credentials: Option<&'static str>,
}

const DATASET: Via = Via {
    source: RatingSource::ImdbDataset,
    credentials: None,
};
const OMDB: Via = Via {
    source: RatingSource::Omdb,
    credentials: Some(catalogue::key::OMDB),
};
const MDBLIST: Via = Via {
    source: RatingSource::Mdblist,
    credentials: Some(catalogue::key::MDBLIST),
};
const TMDB_SCRAPER: Via = Via {
    source: RatingSource::Scraper,
    credentials: Some(catalogue::key::TMDB),
};
const TRAKT_SCRAPER: Via = Via {
    source: RatingSource::Scraper,
    credentials: Some(catalogue::key::TRAKT),
};

struct Def {
    key: &'static str,
    movies: bool,
    shows: bool,
    via: &'static [Via],
}

/// The chips every title has room for, in display order. A rating a title has that is not here
/// (MyAnimeList, Roger Ebert, the audience scores) gets a chip when it has a value.
const DEFS: &[Def] = &[
    Def {
        key: rating_key::IMDB,
        movies: true,
        shows: true,
        via: &[DATASET, OMDB],
    },
    Def {
        key: rating_key::TOMATOMETER_ALL_CRITICS,
        movies: true,
        shows: true,
        via: &[OMDB, MDBLIST],
    },
    Def {
        key: rating_key::METACRITIC,
        movies: true,
        shows: true,
        via: &[OMDB, MDBLIST],
    },
    Def {
        key: rating_key::LETTERBOXD,
        movies: true,
        shows: false,
        via: &[MDBLIST],
    },
    Def {
        key: rating_key::TMDB,
        movies: true,
        shows: true,
        via: &[TMDB_SCRAPER],
    },
    Def {
        key: rating_key::TRAKT,
        movies: true,
        shows: true,
        via: &[TRAKT_SCRAPER],
    },
];

/// The display order of the keys that have no definition of their own.
const EXTRA_ORDER: &[&str] = &[
    rating_key::TOMATOMETER_ALL_AUDIENCE,
    rating_key::MYANIMELIST,
    rating_key::ROGEREBERT,
    rating_key::TVDB,
];

fn is_external(key: &str) -> bool {
    key != rating_key::USER && key != rating_key::DEFAULT
}

/// The chips of a movie or a show: `ratings` are its stored ratings, `outcomes` its step
/// outcomes, `fetching` whether its ratings are being fetched now, `chosen` the rating sources
/// the settings choose for its kind. A title that is neither a movie nor a show has none.
#[must_use]
pub fn rating_chips(
    kind: ItemKind,
    ratings: &Ratings,
    outcomes: &[StepOutcome],
    fetching: bool,
    chosen: &[RatingSource],
    access: &dyn RatingAccess,
) -> Vec<RatingChip> {
    let movie = match kind {
        ItemKind::Movie => true,
        ItemKind::Show => false,
        ItemKind::Set | ItemKind::Season | ItemKind::Episode => return Vec::new(),
    };
    let has = |key: &str| ratings.get(key).is_some();
    let mut chips: Vec<RatingChip> = Vec::new();
    for def in DEFS {
        let applies = if movie { def.movies } else { def.shows };
        if !applies && !has(def.key) {
            continue;
        }
        chips.push(if has(def.key) {
            value_chip(def.key)
        } else {
            missing_chip(def, outcomes, fetching, chosen, access)
        });
    }
    for key in EXTRA_ORDER {
        if has(key) {
            chips.push(value_chip(key));
        }
    }
    // Anything else the title has (a rating read from an NFO, MovieMeter ...), alphabetically.
    let mut others: Vec<&str> = ratings
        .iter()
        .map(|r| r.id.as_str())
        .filter(|key| {
            is_external(key) && !DEFS.iter().any(|d| d.key == *key) && !EXTRA_ORDER.contains(key)
        })
        .collect();
    others.sort_unstable();
    chips.extend(others.into_iter().map(value_chip));
    chips
}

fn value_chip(key: &str) -> RatingChip {
    RatingChip {
        key: key.to_owned(),
        state: RatingChipState::Value,
        reason: String::new(),
        provider: None,
    }
}

/// What the sources that could give a rating say about why it is missing.
fn missing_chip(
    def: &Def,
    outcomes: &[StepOutcome],
    fetching: bool,
    chosen: &[RatingSource],
    access: &dyn RatingAccess,
) -> RatingChip {
    let chip = |state, reason: String, provider: Option<&str>| RatingChip {
        key: def.key.to_owned(),
        state,
        reason,
        provider: provider.map(str::to_owned),
    };
    if fetching {
        return chip(
            RatingChipState::Fetching,
            "Fetching ratings\u{2026}".to_owned(),
            None,
        );
    }
    let outcome_of = |via: &Via| {
        let step = source_step(via.source.key());
        outcomes.iter().find(|o| o.step == step)
    };
    // A source that answered without this rating: the title simply has none there.
    if let Some(answered) = def.via.iter().find_map(|via| {
        outcome_of(via).filter(|o| {
            matches!(
                o.status,
                StepStatus::Done | StepStatus::Unchanged | StepStatus::NotFound
            )
        })
    }) {
        let reason = if answered.reason.is_empty() || answered.status == StepStatus::Done {
            format!("{} has no rating for this title.", via_label(def, answered))
        } else {
            answered.reason.clone()
        };
        return chip(RatingChipState::NotFound, reason, None);
    }
    if let Some((via, failed)) = def.via.iter().find_map(|via| {
        outcome_of(via)
            .filter(|o| o.status == StepStatus::Failed)
            .map(|o| (via, o))
    }) {
        return chip(
            RatingChipState::Failed,
            failed.reason.clone(),
            Some(via.source.key()),
        );
    }
    // Whether the source can be asked now: it needs no key, or its key is set.
    let usable = |via: &Via| {
        via.credentials
            .is_none_or(|provider| access.has_credentials(provider))
    };
    // The scraper's own rating comes with the scrape, whatever the chosen sources are.
    let is_chosen = |via: &Via| via.source == RatingSource::Scraper || chosen.contains(&via.source);
    // A chosen source that can be asked: the next fetch asks it.
    if def.via.iter().any(|via| usable(via) && is_chosen(via)) {
        return chip(
            RatingChipState::NotFetched,
            "Not fetched yet.".to_owned(),
            None,
        );
    }
    // A chosen source whose key is not set: the key is what is missing.
    let chosen_without_key = def
        .via
        .iter()
        .filter(|via| is_chosen(via))
        .find_map(|via| via.credentials);
    // A source that has its key but is not chosen: choosing it is all that is missing.
    if chosen_without_key.is_none()
        && let Some(via) = def.via.iter().find(|via| usable(via) && !is_chosen(via))
    {
        return chip(
            RatingChipState::NotSelected,
            format!(
                "{} is not one of the chosen rating sources. Choose it in Settings, Scraping and providers.",
                via.source.label()
            ),
            Some(via.source.key()),
        );
    }
    // Every source that could give it needs a key, or a connection, that is not set.
    let Some(provider) =
        chosen_without_key.or_else(|| def.via.iter().find_map(|via| via.credentials))
    else {
        return chip(
            RatingChipState::NotFetched,
            "Not fetched yet.".to_owned(),
            None,
        );
    };
    let name = provider_name(provider);
    if provider == catalogue::key::TRAKT {
        chip(
            RatingChipState::ConnectService,
            format!("Connect {name} to get its rating."),
            Some(provider),
        )
    } else {
        chip(
            RatingChipState::NeedsKey,
            format!("{name} needs an API key."),
            Some(provider),
        )
    }
}

fn via_label(def: &Def, outcome: &StepOutcome) -> &'static str {
    def.via
        .iter()
        .map(|via| via.source)
        .find(|source| source_step(source.key()) == outcome.step)
        .map_or("The source", RatingSource::label)
}

fn provider_name(provider: &str) -> &'static str {
    match provider {
        catalogue::key::OMDB => "OMDb",
        catalogue::key::MDBLIST => "MDBList",
        catalogue::key::TRAKT => "Trakt",
        catalogue::key::TMDB => "TMDB",
        _ => "The provider",
    }
}

/// Which titles a ratings sweep looks at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SweepScope {
    /// Scraped titles that a chosen, usable source has not answered for yet.
    Missing,
    /// Every scraped title: all its ratings are fetched again.
    All,
}

/// The provider whose key a rating source needs, if any.
fn key_of(source: RatingSource) -> Option<&'static str> {
    match source {
        RatingSource::Omdb => Some(catalogue::key::OMDB),
        RatingSource::Mdblist => Some(catalogue::key::MDBLIST),
        RatingSource::ImdbDataset | RatingSource::Scraper => None,
    }
}

/// The rating source a provider key unlocks (`omdb`, `mdblist`), if any.
#[must_use]
pub fn source_for_key(provider: &str) -> Option<RatingSource> {
    match provider {
        catalogue::key::OMDB => Some(RatingSource::Omdb),
        catalogue::key::MDBLIST => Some(RatingSource::Mdblist),
        _ => None,
    }
}

/// Whether a scraped title lacks the ratings of a source that can be asked now: a chosen source
/// (the IMDb dataset, OMDb or MDBList) that needs no key or has its key, and that has not
/// answered for the title (it was never asked, it needed a key that was missing then, or it
/// failed). A source that answered, with ratings or with none, is not asked again. `only` limits
/// the look to one source (the one a new key unlocks). The scraper's own rating comes with the
/// scrape and is not a reason.
#[must_use]
pub fn lacks_ratings(
    outcomes: &[StepOutcome],
    chosen: &[RatingSource],
    access: &dyn RatingAccess,
    only: Option<RatingSource>,
) -> bool {
    chosen
        .iter()
        .filter(|source| **source != RatingSource::Scraper)
        .filter(|source| only.is_none_or(|wanted| wanted == **source))
        .filter(|source| key_of(**source).is_none_or(|provider| access.has_credentials(provider)))
        .any(|source| {
            let step = source_step(source.key());
            !outcomes.iter().any(|o| {
                o.step == step
                    && matches!(
                        o.status,
                        StepStatus::Done | StepStatus::Unchanged | StepStatus::NotFound
                    )
            })
        })
}

/// Reads the chips of titles for the detail pages and the Inspector.
pub struct RatingStatusService {
    db: Arc<Database>,
    settings: Arc<SettingsService>,
    access: Arc<dyn RatingAccess + Send + Sync>,
    working: Arc<Working>,
}

impl RatingStatusService {
    #[must_use]
    pub fn new(
        db: Arc<Database>,
        settings: Arc<SettingsService>,
        access: Arc<dyn RatingAccess + Send + Sync>,
        working: Arc<Working>,
    ) -> Arc<Self> {
        Arc::new(Self {
            db,
            settings,
            access,
            working,
        })
    }

    /// The chips of the title `id` given its stored `ratings`. Blocking.
    ///
    /// # Errors
    /// The library cannot be read.
    pub fn chips(
        &self,
        id: ItemId,
        kind: ItemKind,
        ratings: &Ratings,
    ) -> Result<Vec<RatingChip>, DbError> {
        if !matches!(kind, ItemKind::Movie | ItemKind::Show) {
            return Ok(Vec::new());
        }
        let outcomes = self.db.read(move |conn| step_outcomes(conn, id))?;
        let settings = rating_settings_from_values(&self.settings.values());
        let chosen = if kind == ItemKind::Show {
            settings.show.sources
        } else {
            settings.movie.sources
        };
        Ok(rating_chips(
            kind,
            ratings,
            &outcomes,
            self.working.is_fetching_ratings(id),
            &chosen,
            self.access.as_ref(),
        ))
    }

    /// The titles a ratings sweep would fetch, in id order: every scraped movie and show for
    /// [`SweepScope::All`]; for [`SweepScope::Missing`] those that [lack](lacks_ratings) the
    /// ratings of a source that can be asked now (of `only`, when given). Blocking.
    ///
    /// # Errors
    /// The library cannot be read.
    pub fn sweep_titles(
        &self,
        scope: SweepScope,
        only: Option<RatingSource>,
    ) -> Result<Vec<ItemId>, DbError> {
        let candidates = self.db.read(rating_fetch_candidates)?;
        if scope == SweepScope::All {
            return Ok(candidates.into_iter().map(|(id, _)| id).collect());
        }
        let settings = rating_settings_from_values(&self.settings.values());
        let ids: Vec<ItemId> = candidates.iter().map(|(id, _)| *id).collect();
        let outcomes = self.db.read(move |conn| step_outcomes_for(conn, &ids))?;
        Ok(candidates
            .into_iter()
            .filter(|(id, kind)| {
                let chosen = if *kind == ItemKind::Show {
                    &settings.show.sources
                } else {
                    &settings.movie.sources
                };
                let none = Vec::new();
                let rows = outcomes.get(id).unwrap_or(&none);
                lacks_ratings(rows, chosen, self.access.as_ref(), only)
            })
            .map(|(id, _)| id)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use cv_domain::Rating;

    use super::*;

    /// Credentials of the providers in the set are in place.
    struct Keys(BTreeSet<&'static str>);

    impl RatingAccess for Keys {
        fn has_credentials(&self, provider: &str) -> bool {
            self.0.contains(provider)
        }
    }

    fn keys(set: &[&'static str]) -> Keys {
        Keys(set.iter().copied().collect())
    }

    /// The default rating sources.
    const DEFAULT: [RatingSource; 3] = cv_providers::RatingFetchSettings::DEFAULT_SOURCES;

    fn outcome(step: &str, status: StepStatus, reason: &str) -> StepOutcome {
        StepOutcome {
            item: ItemId(1),
            step: step.to_owned(),
            status,
            reason: reason.to_owned(),
            at_ms: 1,
        }
    }

    fn ratings(entries: &[(&str, f32, u32)]) -> Ratings {
        let mut out = Ratings::default();
        for (key, value, max) in entries {
            out.insert(Rating::new(*key, *value, 100, *max));
        }
        out
    }

    fn state_of(chips: &[RatingChip], key: &str) -> RatingChipState {
        chips
            .iter()
            .find(|c| c.key == key)
            .unwrap_or_else(|| panic!("no chip for {key}"))
            .state
    }

    fn chip<'a>(chips: &'a [RatingChip], key: &str) -> &'a RatingChip {
        chips.iter().find(|c| c.key == key).expect("a chip")
    }

    #[test]
    fn only_a_tmdb_key_explains_every_missing_source() {
        // The spec's scenario: TMDB and IMDb have values; Rotten Tomatoes and Metacritic need a
        // key, Trakt needs connecting.
        let chips = rating_chips(
            ItemKind::Movie,
            &ratings(&[("tmdb", 7.9, 10), ("imdb", 8.3, 10)]),
            &[outcome(
                "ratings:imdb-dataset",
                StepStatus::Done,
                "1 rating",
            )],
            false,
            &DEFAULT,
            &keys(&["tmdb"]),
        );
        assert_eq!(state_of(&chips, "tmdb"), RatingChipState::Value);
        assert_eq!(state_of(&chips, "imdb"), RatingChipState::Value);
        let rt = chip(&chips, "tomatometerallcritics");
        assert_eq!(rt.state, RatingChipState::NeedsKey);
        assert_eq!(rt.provider.as_deref(), Some("omdb"));
        assert_eq!(rt.reason, "OMDb needs an API key.");
        assert_eq!(state_of(&chips, "metacritic"), RatingChipState::NeedsKey);
        assert_eq!(state_of(&chips, "letterboxd"), RatingChipState::NeedsKey);
        let trakt = chip(&chips, "trakt");
        assert_eq!(trakt.state, RatingChipState::ConnectService);
        assert_eq!(trakt.provider.as_deref(), Some("trakt"));
    }

    #[test]
    fn a_key_set_after_the_fetch_is_not_needs_a_key_any_more() {
        let asked_without_key = [outcome(
            "ratings:mdblist",
            StepStatus::NeedsKey,
            "MDBList needs an API key.",
        )];
        // Still no key: Letterboxd needs one.
        let before = rating_chips(
            ItemKind::Movie,
            &ratings(&[]),
            &asked_without_key,
            false,
            &DEFAULT,
            &keys(&[]),
        );
        let letterboxd = chip(&before, "letterboxd");
        assert_eq!(letterboxd.state, RatingChipState::NeedsKey);
        assert_eq!(letterboxd.provider.as_deref(), Some("mdblist"));
        // The key is set now: the next fetch asks MDBList, so the chip only waits for it.
        let after = rating_chips(
            ItemKind::Movie,
            &ratings(&[]),
            &asked_without_key,
            false,
            &DEFAULT,
            &keys(&["mdblist"]),
        );
        assert_eq!(state_of(&after, "letterboxd"), RatingChipState::NotFetched);
    }

    #[test]
    fn a_keyed_source_that_is_not_chosen_says_so_and_does_not_blame_another() {
        // An OMDb key, no MDBList key, and only the dataset chosen (the old default).
        let chips = rating_chips(
            ItemKind::Movie,
            &ratings(&[("imdb", 8.3, 10)]),
            &[outcome(
                "ratings:imdb-dataset",
                StepStatus::Done,
                "1 rating",
            )],
            false,
            &[RatingSource::ImdbDataset],
            &keys(&["omdb", "tmdb"]),
        );
        let rt = chip(&chips, "tomatometerallcritics");
        assert_eq!(rt.state, RatingChipState::NotSelected);
        assert_eq!(rt.provider.as_deref(), Some("omdb"));
        assert!(
            rt.reason
                .starts_with("OMDb is not one of the chosen rating sources")
        );
        assert_eq!(state_of(&chips, "metacritic"), RatingChipState::NotSelected);
        // Letterboxd only comes from MDBList, which has no key.
        assert_eq!(state_of(&chips, "letterboxd"), RatingChipState::NeedsKey);

        // With the default sources the same keys are simply waiting for the fetch.
        let chips = rating_chips(
            ItemKind::Movie,
            &ratings(&[("imdb", 8.3, 10)]),
            &[],
            false,
            &DEFAULT,
            &keys(&["omdb", "tmdb"]),
        );
        assert_eq!(
            state_of(&chips, "tomatometerallcritics"),
            RatingChipState::NotFetched
        );
    }

    #[test]
    fn a_failed_source_shows_failed_and_a_source_without_the_title_shows_not_found() {
        let outcomes = [
            outcome(
                "ratings:imdb-dataset",
                StepStatus::Failed,
                "IMDb ratings dataset failed: the download failed",
            ),
            outcome("ratings:mdblist", StepStatus::NotFound, "MDBList has none."),
        ];
        let chips = rating_chips(
            ItemKind::Movie,
            &ratings(&[]),
            &outcomes,
            false,
            &DEFAULT,
            &keys(&["omdb", "mdblist", "trakt", "tmdb"]),
        );
        let imdb = chip(&chips, "imdb");
        assert_eq!(imdb.state, RatingChipState::Failed);
        assert!(imdb.reason.contains("download failed"));
        // The Retry knows which source failed.
        assert_eq!(imdb.provider.as_deref(), Some("imdb-dataset"));
        assert_eq!(state_of(&chips, "letterboxd"), RatingChipState::NotFound);
        assert_eq!(state_of(&chips, "metacritic"), RatingChipState::NotFound);
        // Nothing asked the scraper: the keys are set, so it is simply not fetched yet.
        assert_eq!(state_of(&chips, "tmdb"), RatingChipState::NotFetched);
        assert_eq!(state_of(&chips, "trakt"), RatingChipState::NotFetched);
    }

    #[test]
    fn while_the_fetch_runs_every_missing_chip_is_fetching_and_values_stay() {
        let chips = rating_chips(
            ItemKind::Show,
            &ratings(&[("tmdb", 8.0, 10)]),
            &[outcome("ratings:omdb", StepStatus::Failed, "x")],
            true,
            &DEFAULT,
            &keys(&[]),
        );
        assert_eq!(state_of(&chips, "tmdb"), RatingChipState::Value);
        assert_eq!(state_of(&chips, "imdb"), RatingChipState::Fetching);
        assert_eq!(state_of(&chips, "trakt"), RatingChipState::Fetching);
    }

    #[test]
    fn shows_have_no_letterboxd_chip_unless_they_have_the_rating_and_extras_come_after() {
        let none = rating_chips(
            ItemKind::Show,
            &ratings(&[]),
            &[],
            false,
            &DEFAULT,
            &keys(&["omdb", "tmdb", "trakt"]),
        );
        assert!(none.iter().all(|c| c.key != "letterboxd"));

        let some = rating_chips(
            ItemKind::Show,
            &ratings(&[
                ("letterboxd", 4.0, 5),
                ("myanimelist", 8.1, 10),
                ("moviemeter", 3.0, 5),
                ("user", 9.0, 10),
            ]),
            &[],
            false,
            &DEFAULT,
            &keys(&["omdb", "tmdb", "trakt"]),
        );
        let order: Vec<&str> = some.iter().map(|c| c.key.as_str()).collect();
        assert_eq!(
            order,
            [
                "imdb",
                "tomatometerallcritics",
                "metacritic",
                "letterboxd",
                "tmdb",
                "trakt",
                "myanimelist",
                "moviemeter"
            ]
        );
        assert!(some.iter().all(|c| c.key != "user"));
    }

    #[test]
    fn collections_seasons_and_episodes_have_no_strip() {
        for kind in [ItemKind::Set, ItemKind::Season, ItemKind::Episode] {
            assert!(rating_chips(kind, &ratings(&[]), &[], false, &DEFAULT, &keys(&[])).is_empty());
        }
    }

    #[test]
    fn a_title_lacks_ratings_while_a_usable_chosen_source_has_not_answered() {
        let none: [StepOutcome; 0] = [];
        let omdb_step = source_step(RatingSource::Omdb.key());
        // Scraped before the OMDb key was set: OMDb said it needed one. With the key it is due.
        let before_key = [outcome(
            &omdb_step,
            StepStatus::NeedsKey,
            "OMDb needs an API key.",
        )];
        let chosen = [RatingSource::Omdb];
        assert!(lacks_ratings(&before_key, &chosen, &keys(&["omdb"]), None));
        // Without the key there is nothing to fetch yet.
        assert!(!lacks_ratings(&before_key, &chosen, &keys(&[]), None));
        // A source that answered, with ratings or without, is not asked again; a failure is.
        for (status, due) in [
            (StepStatus::Done, false),
            (StepStatus::Unchanged, false),
            (StepStatus::NotFound, false),
            (StepStatus::Failed, true),
            (StepStatus::Skipped, true),
        ] {
            let rows = [outcome(&omdb_step, status, "")];
            assert_eq!(
                lacks_ratings(&rows, &chosen, &keys(&["omdb"]), None),
                due,
                "{status:?}"
            );
        }
        // Never asked at all: due. The scraper's own rating is not a reason.
        assert!(lacks_ratings(&none, &DEFAULT, &keys(&[]), None));
        assert!(!lacks_ratings(
            &none,
            &[RatingSource::Scraper],
            &keys(&["tmdb"]),
            None
        ));
        // Limited to the source a new key unlocks.
        let dataset_done = [outcome(
            &source_step(RatingSource::ImdbDataset.key()),
            StepStatus::Done,
            "",
        )];
        assert!(!lacks_ratings(
            &dataset_done,
            &DEFAULT,
            &keys(&[]),
            Some(RatingSource::ImdbDataset)
        ));
        assert!(lacks_ratings(
            &dataset_done,
            &DEFAULT,
            &keys(&["mdblist"]),
            Some(RatingSource::Mdblist)
        ));
        assert_eq!(source_for_key("mdblist"), Some(RatingSource::Mdblist));
        assert_eq!(source_for_key("tmdb"), None);
    }
}
