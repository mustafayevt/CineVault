//! Matching local episodes to the provider's episode list in the show's ordering.
//!
//! Everything here is pure: it takes the numbers and IDs of the local episodes and the episode
//! items a provider returned, and says which belongs to which. [`super::service`] reads and
//! writes the library around it.
//!
//! A local episode is found in the list, in this order, by:
//!
//! 1. **ID**: it already carries an ID the provider episode has (an earlier scrape linked them),
//!    so a change of ordering never loses the link;
//! 2. **its number in the chosen ordering**: stored by an earlier scrape in that ordering;
//! 3. **the number its file name carries, read in the chosen ordering**: for the absolute
//!    ordering the single number of `Show - 137.mkv`; for every other ordering the season and
//!    episode the scan read from the name (`S01E03` is DVD season 1, episode 3 when the show is
//!    in DVD order).
//!
//! The absolute ordering has no seasons, so its numbers are compared by episode alone.

use std::collections::HashMap;

use cv_domain::{
    EpisodeGroup, EpisodeNumber, EpisodeNumbers, EpisodeOrdering, Item, ItemDetail, ItemId,
    ProviderIds,
};

/// What the matcher needs to know about one local episode.
#[derive(Debug, Clone)]
pub struct LocalEpisode {
    pub id: ItemId,
    pub numbers: EpisodeNumbers,
    pub ids: ProviderIds,
    /// The absolute number the file name carries, when it carries one.
    pub file_absolute: Option<u32>,
}

/// How a local episode was found in the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchedBy {
    /// A shared provider ID.
    Id,
    /// The number stored for the chosen ordering.
    Number,
    /// The number the file name carries, read in the chosen ordering. The aired number stored
    /// for the episode is the scan's reading of the name and is replaced by the provider's.
    FileName,
}

/// One local episode paired with a provider episode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pair {
    pub local: usize,
    pub provider: usize,
    pub by: MatchedBy,
}

/// The outcome of matching.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Matching {
    pub pairs: Vec<Pair>,
    /// Indexes of local episodes the list has no counterpart for.
    pub unmatched_local: Vec<usize>,
    /// Indexes of provider episodes no local episode matched: the missing episodes.
    pub unmatched_provider: Vec<usize>,
}

/// The key two numbers are compared by: the absolute ordering ignores the season.
fn key(group: &EpisodeGroup, number: &EpisodeNumber) -> (u32, u32) {
    if group.ordering == EpisodeOrdering::Absolute {
        (0, number.episode)
    } else {
        (number.season, number.episode)
    }
}

/// The number an episode has in `group`. A stored DVD or absolute ordering may carry a group
/// name the provider's list does not use; a show has one of each, so the kind alone decides.
pub fn number_in<'a>(
    numbers: &'a EpisodeNumbers,
    group: &EpisodeGroup,
) -> Option<&'a EpisodeNumber> {
    numbers.get(group).or_else(|| {
        matches!(
            group.ordering,
            EpisodeOrdering::Dvd | EpisodeOrdering::Absolute
        )
        .then(|| numbers.iter().find(|n| n.group.ordering == group.ordering))
        .flatten()
    })
}

fn episode_numbers(item: &Item) -> Option<&EpisodeNumbers> {
    match &item.detail {
        ItemDetail::Episode(fields) => Some(&fields.numbers),
        _ => None,
    }
}

/// Pairs the local episodes with the provider's episodes (`list`, in the order `group`).
pub fn match_episodes(local: &[LocalEpisode], list: &[Item], group: &EpisodeGroup) -> Matching {
    let mut by_id: HashMap<(&str, &str), usize> = HashMap::new();
    let mut by_number: HashMap<(u32, u32), usize> = HashMap::new();
    for (index, item) in list.iter().enumerate() {
        for pair in item.core.ids.iter() {
            by_id.entry(pair).or_insert(index);
        }
        if let Some(number) = episode_numbers(item).and_then(|n| number_in(n, group)) {
            by_number.entry(key(group, number)).or_insert(index);
        }
    }

    let aired = EpisodeGroup::aired();
    let mut matching = Matching::default();
    let mut used = vec![false; list.len()];
    for (position, episode) in local.iter().enumerate() {
        let found = episode
            .ids
            .iter()
            .find_map(|pair| by_id.get(&pair).map(|&p| (p, MatchedBy::Id)))
            .or_else(|| {
                number_in(&episode.numbers, group)
                    .and_then(|n| by_number.get(&key(group, n)))
                    .map(|&p| (p, MatchedBy::Number))
            })
            .or_else(|| {
                let from_name = if group.ordering == EpisodeOrdering::Absolute {
                    episode.file_absolute.map(|n| (0, n))
                } else if group.ordering == EpisodeOrdering::Aired {
                    None
                } else {
                    episode.numbers.get(&aired).map(|n| key(group, n))
                };
                from_name
                    .and_then(|k| by_number.get(&k))
                    .map(|&p| (p, MatchedBy::FileName))
            });
        match found {
            Some((provider, by)) => {
                used[provider] = true;
                matching.pairs.push(Pair {
                    local: position,
                    provider,
                    by,
                });
            }
            None => matching.unmatched_local.push(position),
        }
    }
    matching.unmatched_provider = used
        .iter()
        .enumerate()
        .filter(|(_, used)| !**used)
        .map(|(index, _)| index)
        .collect();
    matching
}

/// Records the provider's numbers and IDs on a local episode that was matched with it.
///
/// Every number the provider gives is stored, so the aired, DVD and absolute numbers all end up
/// on the episode; the numbers of orderings the provider did not give are kept. When the match
/// was made from the file name and the provider has no aired number, the aired number stored by
/// the scan was only a reading of the name, so it is dropped.
pub fn apply_pair(
    numbers: &mut EpisodeNumbers,
    ids: &mut ProviderIds,
    provider: &Item,
    by: MatchedBy,
) {
    let Some(given) = episode_numbers(provider) else {
        return;
    };
    for number in given.iter() {
        numbers.set(number.clone());
    }
    if by == MatchedBy::FileName && given.get(&EpisodeGroup::aired()).is_none() {
        numbers.remove(&EpisodeGroup::aired());
    }
    for (id_key, value) in provider.core.ids.iter() {
        if !ids.contains(id_key) {
            ids.set(id_key, value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cv_domain::EpisodeFields;

    fn provider_episode(id: &str, numbers: &[EpisodeNumber]) -> Item {
        let mut fields = EpisodeFields::default();
        for n in numbers {
            fields.numbers.set(n.clone());
        }
        let mut item = Item::new(ItemDetail::Episode(fields));
        item.core.ids.set("tvdb", id);
        item
    }

    fn local(id: i64, aired: Option<(u32, u32)>, file_absolute: Option<u32>) -> LocalEpisode {
        let mut numbers = EpisodeNumbers::new();
        if let Some((s, e)) = aired {
            numbers.set(EpisodeNumber::aired(s, e));
        }
        LocalEpisode {
            id: ItemId(id),
            numbers,
            ids: ProviderIds::new(),
            file_absolute,
        }
    }

    fn abs(episode: u32) -> EpisodeNumber {
        EpisodeNumber::new(EpisodeGroup::absolute(), 1, episode)
    }

    fn dvd(season: u32, episode: u32) -> EpisodeNumber {
        EpisodeNumber::new(EpisodeGroup::dvd(), season, episode)
    }

    #[test]
    fn absolute_numbers_ignore_the_season() {
        let list = vec![
            provider_episode("1", &[EpisodeNumber::aired(1, 1), abs(1)]),
            provider_episode("2", &[EpisodeNumber::aired(2, 1), abs(2)]),
        ];
        let files = vec![local(1, Some((1, 2)), Some(2))];
        let m = match_episodes(&files, &list, &EpisodeGroup::absolute());
        assert_eq!(
            m.pairs,
            vec![Pair {
                local: 0,
                provider: 1,
                by: MatchedBy::FileName
            }]
        );
        assert_eq!(m.unmatched_provider, vec![0]);
    }

    #[test]
    fn dvd_files_are_read_as_dvd_numbers() {
        // The file is called S01E03 and means DVD season 1, episode 3, which aired as S02E01.
        let list = vec![provider_episode(
            "9",
            &[EpisodeNumber::aired(2, 1), dvd(1, 3)],
        )];
        let m = match_episodes(&[local(1, Some((1, 3)), None)], &list, &EpisodeGroup::dvd());
        assert_eq!(m.pairs.len(), 1);
        assert_eq!(m.pairs[0].by, MatchedBy::FileName);
    }

    #[test]
    fn stored_ids_and_numbers_win_over_the_file_name() {
        let list = vec![
            provider_episode("1", &[EpisodeNumber::aired(1, 1), dvd(1, 2)]),
            provider_episode("2", &[EpisodeNumber::aired(1, 2), dvd(1, 1)]),
        ];
        let mut linked = local(1, Some((1, 1)), None);
        linked.ids.set("tvdb", "1");
        let mut numbered = local(2, Some((1, 2)), None);
        numbered.numbers.set(dvd(1, 1));
        let m = match_episodes(&[linked, numbered], &list, &EpisodeGroup::dvd());
        assert_eq!(
            (m.pairs[0].provider, m.pairs[0].by),
            (0, MatchedBy::Id),
            "an ID link survives a change of ordering"
        );
        assert_eq!((m.pairs[1].provider, m.pairs[1].by), (1, MatchedBy::Number));
    }

    #[test]
    fn aired_order_does_not_read_file_names_twice() {
        let list = vec![provider_episode("1", &[EpisodeNumber::aired(1, 1)])];
        let m = match_episodes(
            &[local(1, Some((1, 1)), None)],
            &list,
            &EpisodeGroup::aired(),
        );
        assert_eq!(m.pairs[0].by, MatchedBy::Number);
    }

    #[test]
    fn unmatched_episodes_are_reported_on_both_sides() {
        let list = vec![provider_episode("1", &[EpisodeNumber::aired(1, 1)])];
        let m = match_episodes(
            &[local(1, Some((1, 1)), None), local(2, Some((4, 4)), None)],
            &list,
            &EpisodeGroup::aired(),
        );
        assert_eq!(m.unmatched_local, vec![1]);
        assert!(m.unmatched_provider.is_empty());
    }

    #[test]
    fn applying_a_pair_keeps_the_other_orderings() {
        let provider = provider_episode("5", &[EpisodeNumber::aired(2, 1), dvd(1, 3)]);
        let mut numbers = EpisodeNumbers::new();
        numbers.set(EpisodeNumber::aired(1, 3));
        numbers.set(abs(40));
        let mut ids = ProviderIds::new();
        apply_pair(&mut numbers, &mut ids, &provider, MatchedBy::FileName);
        assert_eq!(numbers.get(&EpisodeGroup::aired()).unwrap().season, 2);
        assert_eq!(numbers.get(&EpisodeGroup::dvd()).unwrap().episode, 3);
        assert_eq!(numbers.get(&EpisodeGroup::absolute()).unwrap().episode, 40);
        assert_eq!(ids.get("tvdb"), Some("5"));
    }

    #[test]
    fn a_file_name_reading_is_dropped_when_the_provider_has_no_aired_number() {
        let provider = provider_episode("5", &[abs(137)]);
        let mut numbers = EpisodeNumbers::new();
        numbers.set(EpisodeNumber::aired(1, 37));
        let mut ids = ProviderIds::new();
        apply_pair(&mut numbers, &mut ids, &provider, MatchedBy::FileName);
        assert!(numbers.get(&EpisodeGroup::aired()).is_none());
        assert_eq!(numbers.get(&EpisodeGroup::absolute()).unwrap().episode, 137);
    }
}
