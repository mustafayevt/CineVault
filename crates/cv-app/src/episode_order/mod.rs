//! Episode orderings: the per-show choice between aired, DVD, absolute and the
//! provider's alternate orders, and matching and display that follow it.
//!
//! ```text
//! user picks an ordering ─► provider.episodes(show, ordering) ─► matcher ─► library
//!                              NotFound / Unsupported            │           ├─ numbers of every ordering kept
//!                              = "ordering not available"        │           ├─ show.episode_group = the choice
//!                                                                │           └─ missing episodes = the rest of the list
//!                                                                └─ ID, then stored number, then file name
//! ```
//!
//! * [`matcher`] pairs the local episodes with the provider's list, for every ordering. In the
//!   absolute ordering the single number of a file name (`Show - 137.mkv`) is looked up among the
//!   absolute numbers of the list, and the provider's aired number gives the episode its season
//!   and episode (see `cv_scan::episode::parse_absolute_episode`).
//! * [`EpisodeOrderer`] reads the show and its episodes, fetches the list, matches, and writes the
//!   numbers in one transaction. The choice is stored only when the provider has the ordering,
//!   so a show is never left in an ordering without numbers.
//! * [`EpisodeOrderService`] adds the choice of provider and the request settings of the app.
//! * Display follows the choice through `ShowFields::episode_group`: the season list, the episode
//!   rows and the NFO all read the number in that ordering, falling back to the aired one (see
//!   `cv_domain::EpisodeNumbers::display`). [`seasons_in`] is the season list of an ordering.
//!
//! Locked episodes keep their numbers. Episodes without a counterpart in the list keep theirs
//! too, and are reported so the UI can offer a search by title.

pub mod matcher;
pub(crate) mod service;

pub use service::{
    EpisodeOrderService, EpisodeOrderer, NoEpisodeList, OrderingReport, pick_provider, provider_for,
};

use cv_db::DbError;
use cv_domain::{EpisodeGroup, EpisodeOrdering, ShowFields};
use cv_providers::ProviderError;

#[cfg(test)]
mod tests;

/// Why an ordering could not be applied.
#[derive(Debug, thiserror::Error)]
pub enum EpisodeOrderError {
    #[error("The library database could not be read: {0}")]
    Db(#[from] DbError),
    #[error("That show is not in the library any more.")]
    ShowNotFound,
    #[error("Only shows have an episode ordering.")]
    NotAShow,
    /// The provider answered that the show has no episodes in this ordering.
    #[error("{} is not available for this show.", ordering_name(.0))]
    NotAvailable(EpisodeGroup),
    #[error("No metadata provider is ready. Add a key in Settings, Scraping and providers.")]
    NoProvider,
    /// The show could be read only with a provider key that is missing (identification is
    /// paused without it).
    #[error("Episode orders need a provider key. Add one in Settings, Scraping and providers.")]
    KeyNeeded,
    /// The show has no ID of a provider that offers the ordering.
    #[error(
        "This show is not matched with a provider yet, so it has no episode list. Use Scrape… first."
    )]
    NotIdentified,
    #[error("{0}")]
    Provider(ProviderError),
}

impl EpisodeOrderError {
    /// A stable code for the UI: `storage`, `not_found`, `unsupported`, `ordering_not_available`,
    /// `no_provider`, `key_needed`, `not_identified` or `provider`.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Db(_) => "storage",
            Self::ShowNotFound => "not_found",
            Self::NotAShow => "unsupported",
            Self::NotAvailable(_) => "ordering_not_available",
            Self::NoProvider => "no_provider",
            Self::KeyNeeded => "key_needed",
            Self::NotIdentified => "not_identified",
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

/// The ordering in words: `Aired order`, `DVD order`, `Absolute order`, or the provider's name
/// for an alternate order.
#[must_use]
pub fn ordering_name(group: &EpisodeGroup) -> String {
    match group.ordering {
        EpisodeOrdering::Aired => "Aired order".to_owned(),
        EpisodeOrdering::Dvd => "DVD order".to_owned(),
        EpisodeOrdering::Absolute => "Absolute order".to_owned(),
        EpisodeOrdering::Alternate | EpisodeOrdering::Display if !group.name.is_empty() => {
            group.name.clone()
        }
        EpisodeOrdering::Alternate => "Alternate order".to_owned(),
        EpisodeOrdering::Display => "Display order".to_owned(),
    }
}

/// The orderings a show can be switched to, in the order the picker lists them: aired, DVD,
/// absolute, then the provider's named groups. `offered` are the orderings the provider can
/// return (`Capabilities::orderings`). The show's current choice is always listed.
#[must_use]
pub fn ordering_options(fields: &ShowFields, offered: &[EpisodeOrdering]) -> Vec<EpisodeGroup> {
    let mut options = vec![EpisodeGroup::aired()];
    let mut add = |group: EpisodeGroup| {
        if !options.contains(&group) {
            options.push(group);
        }
    };
    for (kind, group) in [
        (EpisodeOrdering::Dvd, EpisodeGroup::dvd()),
        (EpisodeOrdering::Absolute, EpisodeGroup::absolute()),
    ] {
        let known = fields.episode_groups.iter().any(|g| g.ordering == kind);
        if offered.contains(&kind) || known {
            add(group);
        }
    }
    for group in &fields.episode_groups {
        if matches!(
            group.ordering,
            EpisodeOrdering::Alternate | EpisodeOrdering::Display
        ) {
            add(group.clone());
        }
    }
    add(fields.episode_group.clone());
    options
}

/// The season numbers of a show in its chosen ordering, ascending: every season that has a local
/// or a missing episode, read through `ShowFields::episode_group`. Season 0 is the specials.
#[must_use]
pub fn seasons_in<'a>(
    fields: &ShowFields,
    episodes: impl IntoIterator<Item = &'a cv_domain::EpisodeFields>,
) -> Vec<u32> {
    let active = &fields.episode_group;
    let mut seasons: Vec<u32> = episodes
        .into_iter()
        .filter_map(|e| e.numbers.display(active).map(|n| n.season))
        .chain(
            fields
                .missing_episodes
                .iter()
                .filter_map(|m| m.numbers.display(active).map(|n| n.season)),
        )
        .collect();
    seasons.sort_unstable();
    seasons.dedup();
    seasons
}
