//! Episode orderings and per-ordering episode numbers.

use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};

/// The kind of episode ordering a provider offers.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum EpisodeOrdering {
    #[default]
    Aired,
    Dvd,
    Absolute,
    Alternate,
    Display,
}

/// An episode ordering: its kind plus the provider's name for it (empty for the standard ones).
///
/// Equality ignores `name` for [`EpisodeOrdering::Aired`], because there is only one aired order.
#[derive(Debug, Clone, Default, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct EpisodeGroup {
    pub ordering: EpisodeOrdering,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub name: String,
}

impl EpisodeGroup {
    pub fn new(ordering: EpisodeOrdering, name: impl Into<String>) -> Self {
        Self {
            ordering,
            name: name.into(),
        }
    }

    pub fn aired() -> Self {
        Self::new(EpisodeOrdering::Aired, "")
    }

    pub fn dvd() -> Self {
        Self::new(EpisodeOrdering::Dvd, "")
    }

    pub fn absolute() -> Self {
        Self::new(EpisodeOrdering::Absolute, "")
    }
}

impl PartialEq for EpisodeGroup {
    fn eq(&self, other: &Self) -> bool {
        self.ordering == other.ordering
            && (self.ordering == EpisodeOrdering::Aired || self.name == other.name)
    }
}

impl Hash for EpisodeGroup {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.ordering.hash(state);
        if self.ordering != EpisodeOrdering::Aired {
            self.name.hash(state);
        }
    }
}

/// An episode's season and episode number in one ordering.
///
/// Season `0` is the specials season. A missing season or episode in stored data means `0`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct EpisodeNumber {
    pub group: EpisodeGroup,
    pub season: u32,
    pub episode: u32,
}

impl EpisodeNumber {
    pub fn new(group: EpisodeGroup, season: u32, episode: u32) -> Self {
        Self {
            group,
            season,
            episode,
        }
    }

    pub fn aired(season: u32, episode: u32) -> Self {
        Self::new(EpisodeGroup::aired(), season, episode)
    }
}

/// All numbers of one episode, at most one per ordering.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EpisodeNumbers(Vec<EpisodeNumber>);

impl EpisodeNumbers {
    pub fn new() -> Self {
        Self::default()
    }

    /// Stores a number, replacing the existing one for the same ordering.
    pub fn set(&mut self, number: EpisodeNumber) {
        match self.0.iter_mut().find(|n| n.group == number.group) {
            Some(slot) => *slot = number,
            None => self.0.push(number),
        }
    }

    /// The number in exactly this ordering.
    pub fn get(&self, group: &EpisodeGroup) -> Option<&EpisodeNumber> {
        self.0.iter().find(|n| &n.group == group)
    }

    /// The number to display when the show uses `preferred`: that ordering if the episode has
    /// it, otherwise the aired number.
    pub fn display(&self, preferred: &EpisodeGroup) -> Option<&EpisodeNumber> {
        self.get(preferred)
            .or_else(|| self.get(&EpisodeGroup::aired()))
    }

    pub fn remove(&mut self, group: &EpisodeGroup) -> Option<EpisodeNumber> {
        let idx = self.0.iter().position(|n| &n.group == group)?;
        Some(self.0.remove(idx))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &EpisodeNumber> {
        self.0.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn three_orderings() -> EpisodeNumbers {
        let mut n = EpisodeNumbers::new();
        n.set(EpisodeNumber::aired(2, 5));
        n.set(EpisodeNumber::new(EpisodeGroup::dvd(), 1, 3));
        n.set(EpisodeNumber::new(EpisodeGroup::absolute(), 0, 17));
        n
    }

    #[test]
    fn aired_dvd_and_absolute_are_all_stored() {
        let n = three_orderings();
        assert_eq!(n.len(), 3);
        assert_eq!(n.get(&EpisodeGroup::aired()).unwrap().episode, 5);
        assert_eq!(n.get(&EpisodeGroup::dvd()).unwrap().season, 1);
        assert_eq!(n.get(&EpisodeGroup::absolute()).unwrap().episode, 17);
    }

    #[test]
    fn selected_ordering_decides_the_displayed_number() {
        let n = three_orderings();
        assert_eq!(
            n.display(&EpisodeGroup::dvd()).unwrap(),
            &EpisodeNumber::new(EpisodeGroup::dvd(), 1, 3)
        );
        assert_eq!(
            n.display(&EpisodeGroup::aired()).unwrap(),
            &EpisodeNumber::aired(2, 5)
        );
        // An ordering the episode lacks falls back to aired.
        let alt = EpisodeGroup::new(EpisodeOrdering::Alternate, "Production order");
        assert_eq!(n.display(&alt).unwrap(), &EpisodeNumber::aired(2, 5));
    }

    #[test]
    fn set_replaces_same_ordering() {
        let mut n = three_orderings();
        n.set(EpisodeNumber::new(
            EpisodeGroup::new(EpisodeOrdering::Aired, "ignored"),
            2,
            6,
        ));
        assert_eq!(n.len(), 3);
        assert_eq!(n.get(&EpisodeGroup::aired()).unwrap().episode, 6);
    }

    #[test]
    fn named_orderings_are_distinct() {
        let a = EpisodeGroup::new(EpisodeOrdering::Alternate, "A");
        let b = EpisodeGroup::new(EpisodeOrdering::Alternate, "B");
        assert_ne!(a, b);
        let mut n = EpisodeNumbers::new();
        n.set(EpisodeNumber::new(a.clone(), 1, 1));
        n.set(EpisodeNumber::new(b.clone(), 1, 2));
        assert_eq!(n.len(), 2);
        assert_eq!(n.get(&b).unwrap().episode, 2);
    }

    #[test]
    fn round_trip_and_missing_numbers_mean_zero() {
        let n = three_orderings();
        let json = serde_json::to_string(&n).unwrap();
        assert_eq!(serde_json::from_str::<EpisodeNumbers>(&json).unwrap(), n);

        let parsed: EpisodeNumber =
            serde_json::from_str(r#"{"group":{"ordering":"dvd"},"episode":3}"#).unwrap();
        assert_eq!(parsed.season, 0);
        assert_eq!(parsed.episode, 3);
        assert_eq!(parsed.group, EpisodeGroup::dvd());
    }

    #[test]
    fn empty_group_name_is_not_written() {
        let json = serde_json::to_string(&EpisodeNumber::aired(1, 1)).unwrap();
        assert!(!json.contains("name"));
    }
}
