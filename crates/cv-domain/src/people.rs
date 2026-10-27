//! People and credits.

use serde::{Deserialize, Serialize};

use crate::ProviderIds;

/// A person, independent of the titles they appear in.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Person {
    pub name: String,
    pub ids: ProviderIds,
    pub thumb_url: String,
    pub profile_url: String,
}

impl Person {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Self::default()
        }
    }
}

/// The part a person plays in a credit.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum CreditKind {
    Actor,
    Director,
    Writer,
    Producer,
    /// Guest star (episodes).
    Guest,
    Composer,
    Editor,
    Camera,
    #[default]
    Other,
}

/// One person's credit on one item. Cast order is the order of the credits in the item.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Credit {
    pub kind: CreditKind,
    pub person: Person,
    /// The character for actors, or the job title for crew ("Director of Photography").
    pub role: String,
}

impl Credit {
    pub fn new(kind: CreditKind, name: impl Into<String>, role: impl Into<String>) -> Self {
        Self {
            kind,
            person: Person::new(name),
            role: role.into(),
        }
    }
}

/// Merges the crew credits of one person into one: the same person (by a shared provider ID, else
/// by name, ignoring case) in the same part (writer, director, ...) is listed once, with the roles
/// joined, so a writer credited for the story and the screenplay reads "Story, Screenplay". Cast
/// (actors and guests) is left as it is, since one person can play two characters. The first
/// credit of a person keeps its place and its picture.
pub fn merge_crew(credits: &[Credit]) -> Vec<Credit> {
    let mut out: Vec<Credit> = Vec::with_capacity(credits.len());
    for credit in credits {
        let crew = !matches!(credit.kind, CreditKind::Actor | CreditKind::Guest);
        let same = |other: &Credit| {
            other.kind == credit.kind
                && (other
                    .person
                    .name
                    .trim()
                    .eq_ignore_ascii_case(credit.person.name.trim())
                    || credit.person.ids.iter().any(|(key, value)| {
                        !value.is_empty() && other.person.ids.get(key) == Some(value)
                    }))
        };
        match crew
            .then(|| out.iter_mut().find(|other| same(other)))
            .flatten()
        {
            Some(first) => {
                let role = credit.role.trim();
                let have = first
                    .role
                    .split(',')
                    .any(|part| part.trim().eq_ignore_ascii_case(role));
                if !role.is_empty() && !have {
                    if first.role.trim().is_empty() {
                        role.clone_into(&mut first.role);
                    } else {
                        first.role = format!("{}, {role}", first.role.trim());
                    }
                }
            }
            None => out.push(credit.clone()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_person_credited_twice_is_listed_once_with_both_roles() {
        let credits = [
            Credit::new(CreditKind::Writer, "Dan O'Bannon", "Story"),
            Credit::new(CreditKind::Writer, "Ronald Shusett", "Story"),
            Credit::new(CreditKind::Writer, "dan o'bannon", "Screenplay"),
            Credit::new(CreditKind::Director, "Dan O'Bannon", ""),
            Credit::new(CreditKind::Actor, "Ian Holm", "Ash"),
            Credit::new(CreditKind::Actor, "Ian Holm", "Ash"),
        ];
        let merged = merge_crew(&credits);
        let writers: Vec<_> = merged
            .iter()
            .filter(|c| c.kind == CreditKind::Writer)
            .map(|c| (c.person.name.as_str(), c.role.as_str()))
            .collect();
        assert_eq!(
            writers,
            [
                ("Dan O'Bannon", "Story, Screenplay"),
                ("Ronald Shusett", "Story")
            ]
        );
        // The same name in another part is another credit, and cast is left alone.
        assert_eq!(
            merged
                .iter()
                .filter(|c| c.kind == CreditKind::Director)
                .count(),
            1
        );
        assert_eq!(
            merged
                .iter()
                .filter(|c| c.kind == CreditKind::Actor)
                .count(),
            2
        );
    }

    #[test]
    fn people_with_the_same_provider_id_are_one_person() {
        let mut a = Credit::new(CreditKind::Writer, "D. O'Bannon", "Story");
        a.person.ids.set("tmdb", "1");
        let mut b = Credit::new(CreditKind::Writer, "Dan O'Bannon", "Screenplay");
        b.person.ids.set("tmdb", "1");
        assert_eq!(merge_crew(&[a, b]).len(), 1);
    }

    #[test]
    fn credit_round_trip_with_person_ids() {
        let mut credit = Credit::new(CreditKind::Actor, "Sigourney Weaver", "Ripley");
        credit.person.ids.set("tmdb", "10205");
        credit.person.thumb_url = "https://example.test/p.jpg".into();
        let json = serde_json::to_string(&credit).unwrap();
        assert_eq!(serde_json::from_str::<Credit>(&json).unwrap(), credit);
    }

    #[test]
    fn missing_kind_defaults_to_other() {
        let credit: Credit = serde_json::from_str(r#"{"person":{"name":"X"}}"#).unwrap();
        assert_eq!(credit.kind, CreditKind::Other);
    }
}
