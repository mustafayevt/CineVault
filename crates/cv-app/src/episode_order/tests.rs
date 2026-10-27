//! Unit tests for the option list, the season list and the provider choice.

use cv_domain::{
    EpisodeFields, EpisodeGroup, EpisodeNumber, EpisodeOrdering, MissingEpisode, ShowFields,
};

use super::{ordering_name, ordering_options, seasons_in};

fn alternate(name: &str) -> EpisodeGroup {
    EpisodeGroup::new(EpisodeOrdering::Alternate, name)
}

#[test]
fn a_show_without_provider_data_offers_the_aired_order_only() {
    let fields = ShowFields::default();
    assert_eq!(ordering_options(&fields, &[]), vec![EpisodeGroup::aired()]);
}

#[test]
fn the_provider_decides_which_standard_orderings_are_offered() {
    let fields = ShowFields::default();
    let offered = [
        EpisodeOrdering::Aired,
        EpisodeOrdering::Dvd,
        EpisodeOrdering::Absolute,
    ];
    assert_eq!(
        ordering_options(&fields, &offered),
        vec![
            EpisodeGroup::aired(),
            EpisodeGroup::dvd(),
            EpisodeGroup::absolute()
        ]
    );
}

#[test]
fn named_groups_follow_and_the_current_choice_is_always_listed() {
    let fields = ShowFields {
        episode_group: alternate("Production"),
        episode_groups: vec![
            alternate("Story"),
            EpisodeGroup::new(EpisodeOrdering::Dvd, "Box set"),
        ],
        ..ShowFields::default()
    };
    assert_eq!(
        ordering_options(&fields, &[]),
        vec![
            EpisodeGroup::aired(),
            EpisodeGroup::dvd(),
            alternate("Story"),
            alternate("Production"),
        ]
    );
}

#[test]
fn ordering_names_use_the_provider_name_for_alternate_orders() {
    assert_eq!(ordering_name(&EpisodeGroup::dvd()), "DVD order");
    assert_eq!(ordering_name(&alternate("Story")), "Story");
    assert_eq!(ordering_name(&alternate("")), "Alternate order");
}

fn episode(numbers: &[EpisodeNumber]) -> EpisodeFields {
    let mut fields = EpisodeFields::default();
    for n in numbers {
        fields.numbers.set(n.clone());
    }
    fields
}

#[test]
fn the_season_list_follows_the_chosen_ordering() {
    let episodes = [
        episode(&[
            EpisodeNumber::aired(1, 1),
            EpisodeNumber::new(EpisodeGroup::dvd(), 1, 1),
        ]),
        episode(&[
            EpisodeNumber::aired(2, 1),
            EpisodeNumber::new(EpisodeGroup::dvd(), 1, 2),
        ]),
        // No DVD number: shown by its aired one.
        episode(&[EpisodeNumber::aired(0, 1)]),
    ];
    let aired = ShowFields::default();
    assert_eq!(seasons_in(&aired, &episodes), vec![0, 1, 2]);

    let dvd = ShowFields {
        episode_group: EpisodeGroup::dvd(),
        ..ShowFields::default()
    };
    assert_eq!(seasons_in(&dvd, &episodes), vec![0, 1]);
}

#[test]
fn missing_episodes_add_their_seasons() {
    let mut missing = MissingEpisode::default();
    missing.numbers.set(EpisodeNumber::aired(4, 1));
    let fields = ShowFields {
        missing_episodes: vec![missing],
        ..ShowFields::default()
    };
    assert_eq!(seasons_in(&fields, &[]), vec![4]);
}

// ---- Which provider gives the episode list ---------------------------------------------------

mod provider_choice {
    use std::sync::Arc;

    use async_trait::async_trait;
    use cv_domain::{EpisodeGroup, EpisodeOrdering, Item, ProviderIds};
    use cv_providers::model::{Capabilities, MediaKind, RequestOptions, SearchHit, SearchQuery};
    use cv_providers::{MetadataProvider, Provider, ProviderError};

    use crate::episode_order::{EpisodeOrderError, NoEpisodeList, provider_for};
    use crate::episode_scrape::EpisodeScrapeError;
    use crate::matching::batch::MatchProviders;

    /// A show provider that looks shows up by one ID.
    struct Lists {
        id: &'static str,
        key: &'static str,
        orderings: Vec<EpisodeOrdering>,
    }

    #[async_trait]
    impl Provider for Lists {
        fn id(&self) -> &'static str {
            self.id
        }
        fn name(&self) -> &'static str {
            self.id
        }
    }

    #[async_trait]
    impl MetadataProvider for Lists {
        fn capabilities(&self) -> Capabilities {
            Capabilities {
                movies: false,
                shows: true,
                orderings: self.orderings.clone(),
                lookup_ids: vec![self.key],
                best_effort: false,
            }
        }
        async fn search(
            &self,
            _query: &SearchQuery,
            _options: &RequestOptions,
        ) -> Result<Vec<SearchHit>, ProviderError> {
            Ok(Vec::new())
        }
        async fn lookup(
            &self,
            _kind: MediaKind,
            _ids: &ProviderIds,
            _options: &RequestOptions,
        ) -> Result<Option<SearchHit>, ProviderError> {
            Ok(None)
        }
        async fn details(
            &self,
            _kind: MediaKind,
            _ids: &ProviderIds,
            _options: &RequestOptions,
        ) -> Result<Item, ProviderError> {
            Err(ProviderError::NotFound(self.id.to_owned()))
        }
    }

    fn keyless() -> MatchProviders {
        MatchProviders::new(vec![Arc::new(Lists {
            id: "tvmaze",
            key: "tvmaze",
            orderings: vec![EpisodeOrdering::Aired, EpisodeOrdering::Dvd],
        })])
    }

    #[test]
    fn a_provider_is_only_asked_for_a_show_it_can_find() {
        let mut show = Item::show("Breaking Bad");
        let providers = keyless();
        assert_eq!(
            provider_for(&providers, &show, &EpisodeGroup::aired()).err(),
            Some(NoEpisodeList::NotIdentified),
            "a show without a TVmaze ID cannot be read from TVmaze"
        );
        assert_eq!(
            provider_for(&providers, &show, &EpisodeGroup::absolute()).err(),
            Some(NoEpisodeList::NoProvider),
            "nobody offers the absolute order"
        );
        show.core.ids.set("tvmaze", "169");
        let found = provider_for(&providers, &show, &EpisodeGroup::dvd()).expect("a provider");
        assert_eq!(found.id(), "tvmaze");
    }

    #[test]
    fn a_missing_key_is_named_as_the_cause() {
        let paused = EpisodeScrapeError::no_list(NoEpisodeList::NotIdentified, true);
        assert_eq!(paused.code(), "key_needed");
        assert!(paused.to_string().contains("provider key"), "{paused}");
        assert!(!paused.to_string().contains("ordering"), "{paused}");
        let unidentified = EpisodeScrapeError::no_list(NoEpisodeList::NotIdentified, false);
        assert_eq!(unidentified.code(), "not_identified");
        assert_eq!(
            EpisodeScrapeError::no_list(NoEpisodeList::NoProvider, false).code(),
            "no_provider"
        );

        let order = EpisodeOrderError::no_list(NoEpisodeList::NotIdentified, true);
        assert_eq!(order.code(), "key_needed");
        assert!(order.to_string().contains("provider key"), "{order}");
        assert_eq!(
            EpisodeOrderError::no_list(NoEpisodeList::NotIdentified, false).code(),
            "not_identified"
        );
    }
}
