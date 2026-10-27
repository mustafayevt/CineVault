//! Episode scraping: the episodes of a scraped show are scraped by number in the
//! show's chosen ordering, with the user's episode field selection, and an episode whose number
//! is unknown can be found by title in the show's episode list and assigned.
//!
//! ```text
//! show scrape / "Scrape episodes" ─► provider.episodes(show, ordering) ─► matcher (17.2)
//!                                                                          │ ID, number, file name
//!                                                                          ▼
//!                       repo::apply_scrape(episode, provider episode, episode field options)
//!                       (entity lock, field locks and do-not-overwrite honoured)
//!
//! unnumbered episode ─► search the list by title ─► EpisodeHit ─► assign: numbers + IDs + scrape
//! ```
//!
//! * [`title_search`] ranks a provider's episode list against typed text. It is pure.
//! * [`EpisodeScraper`] reads and writes the library around a list the provider returned: it
//!   scrapes the whole show or one episode, and assigns a picked list entry to an episode.
//!   One scrape is one transaction.
//! * [`EpisodeScrapeService`] picks the provider and the settings of the app (language, the
//!   `metadata.scrape-fields-episode` selection and the do-not-overwrite mode).
//!
//! The selectable fields are the episode fields of [`cv_domain::MetaField`]: title, original
//! title, plot, aired date (`release_date`), rating, runtime, studio, tags, guests (`actors`),
//! directors and writers (`crew`) and thumb. Numbers and provider IDs are never a field: a match
//! always records them, so the link survives a change of ordering. A locked episode keeps its
//! numbers too.
//!
//! Scraping writes the library only. The episode NFOs are written by the Rewrite NFO action
//! (`crate::nfo`), which reads what this module stored.

mod service;
pub mod title_search;

pub use service::{
    EpisodeScrapeService, EpisodeScraper, FetchedEpisodes, ScrapeOverride, apply_fetched,
    fetch_for_show, scrape_after_show, scrape_after_show_blocking,
};
pub use title_search::{EpisodeHit, EpisodePick, search_titles};

use cv_db::DbError;
use cv_domain::{EpisodeGroup, ItemId, ScrapeOutcome};
use cv_providers::ProviderError;

use crate::episode_order::NoEpisodeList;
use crate::episode_order::matcher::MatchedBy;

#[cfg(test)]
mod tests;

/// What scraping one episode did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpisodeOutcome {
    pub id: ItemId,
    /// How the episode was found in the provider's list.
    pub by: MatchedBy,
    /// What the scrape wrote, kept and could not write. `blocked` is set for a locked episode.
    pub outcome: ScrapeOutcome,
}

/// What scraping the episodes of a show did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpisodeScrapeReport {
    /// The ordering the list was read in (the show's choice).
    pub group: EpisodeGroup,
    /// Local episodes found in the provider's list. Two files of one episode (an original and
    /// its REPACK) count once, as the show page counts episodes.
    pub matched: usize,
    /// Matched episodes whose stored values changed, counted the same way.
    pub changed: usize,
    /// Local episodes the list has no counterpart for. They keep their values; the user can
    /// search the list by title.
    pub unmatched: Vec<ItemId>,
    /// Matched episodes left untouched because the episode is locked.
    pub locked: usize,
    /// Matched episodes where a field lock kept at least one selected field.
    pub field_locked: usize,
    /// Episodes the list has that the library does not. `None` for a single-episode scrape,
    /// which leaves the show's missing list alone.
    pub missing: Option<usize>,
    /// One entry per matched episode.
    pub episodes: Vec<EpisodeOutcome>,
}

/// What assigning a list entry to an episode did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssignReport {
    pub episode: ItemId,
    /// The ordering the number is in.
    pub group: EpisodeGroup,
    /// The season and episode the episode now has in `group`. `None` when it is locked and
    /// nothing changed.
    pub number: Option<(u32, u32)>,
    /// What the scrape wrote; `blocked` is set for a locked episode.
    pub outcome: ScrapeOutcome,
}

/// Why episodes could not be scraped.
#[derive(Debug, thiserror::Error)]
pub enum EpisodeScrapeError {
    #[error("The library database could not be read: {0}")]
    Db(#[from] DbError),
    #[error("That show is not in the library any more.")]
    ShowNotFound,
    #[error("That episode is not in the library any more.")]
    EpisodeNotFound,
    #[error("Only shows have an episode list to scrape.")]
    NotAShow,
    #[error("That item is not an episode.")]
    NotAnEpisode,
    /// The provider has no episodes for the show in the chosen ordering.
    #[error("The provider has no episode list for this show in that ordering.")]
    NotAvailable,
    #[error("No metadata provider is ready. Add a key in Settings, Scraping and providers.")]
    NoProvider,
    /// Episode data could be read only with a provider key that is missing (identification is
    /// paused without it).
    #[error("Episode scraping needs a provider key. Add one in Settings, Scraping and providers.")]
    KeyNeeded,
    /// The show has no ID of a provider that has episode lists.
    #[error(
        "This show is not matched with a provider yet, so it has no episode list. Use Scrape… first."
    )]
    NotIdentified,
    /// The provider's list has nothing for this episode by ID or number.
    #[error("The episode list has no match for this episode. Search it by title.")]
    NoCounterpart,
    /// The picked entry is not in the provider's list (it changed since the search).
    #[error("That episode is no longer in the provider's list. Search again.")]
    UnknownEpisode,
    #[error("{0}")]
    Provider(ProviderError),
}

impl EpisodeScrapeError {
    /// A stable code for the UI: `storage`, `not_found`, `unsupported`, `not_available`,
    /// `no_provider`, `key_needed`, `not_identified`, `no_match`, `stale_pick` or `provider`.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Db(_) => "storage",
            Self::ShowNotFound | Self::EpisodeNotFound => "not_found",
            Self::NotAShow | Self::NotAnEpisode => "unsupported",
            Self::NotAvailable => "not_available",
            Self::NoProvider => "no_provider",
            Self::KeyNeeded => "key_needed",
            Self::NotIdentified => "not_identified",
            Self::NoCounterpart => "no_match",
            Self::UnknownEpisode => "stale_pick",
            Self::Provider(_) => "provider",
        }
    }

    /// The error for a show no provider can give a list for. While identification is paused
    /// for a missing key, the key is the cause.
    #[must_use]
    pub fn no_list(why: NoEpisodeList, identification_paused: bool) -> Self {
        match why {
            _ if identification_paused => Self::KeyNeeded,
            NoEpisodeList::NoProvider => Self::NoProvider,
            NoEpisodeList::NotIdentified => Self::NotIdentified,
        }
    }
}
