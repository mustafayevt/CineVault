//! The JSON shapes Trakt's public API answers with. Every field is optional on the wire, so
//! everything defaults: a missing or `null` value never fails a whole response.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer};

/// Reads a possibly-`null` value as its default.
fn null_default<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

/// Text accessor for the nullable string fields.
pub(super) trait Text {
    fn text(&self) -> &str;
}

impl Text for Option<String> {
    fn text(&self) -> &str {
        self.as_deref().unwrap_or("").trim()
    }
}

/// The IDs of a movie, show, season, episode or person.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub(super) struct Ids {
    pub trakt: Option<u64>,
    pub slug: Option<String>,
    pub imdb: Option<String>,
    pub tmdb: Option<u64>,
    pub tvdb: Option<u64>,
}

/// A movie or a show summary (`extended=full`). One shape serves both; the fields a kind lacks
/// stay empty.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct Summary {
    pub title: Option<String>,
    pub year: Option<u32>,
    #[serde(deserialize_with = "null_default")]
    pub ids: Ids,
    pub tagline: Option<String>,
    pub overview: Option<String>,
    pub released: Option<String>,
    pub first_aired: Option<String>,
    pub runtime: Option<u32>,
    pub country: Option<String>,
    pub trailer: Option<String>,
    pub status: Option<String>,
    pub rating: Option<f64>,
    pub votes: Option<u64>,
    pub language: Option<String>,
    pub network: Option<String>,
    #[serde(deserialize_with = "null_default")]
    pub genres: Vec<String>,
    pub certification: Option<String>,
}

/// One entry of a search or ID lookup answer.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct SearchResult {
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub movie: Option<Summary>,
    pub show: Option<Summary>,
}

impl SearchResult {
    /// The movie or show of the entry when it is the wanted kind.
    pub fn into_summary(self, show: bool) -> Option<Summary> {
        if show { self.show } else { self.movie }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct PersonRef {
    pub name: Option<String>,
    #[serde(deserialize_with = "null_default")]
    pub ids: Ids,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct CastMember {
    #[serde(deserialize_with = "null_default")]
    pub characters: Vec<String>,
    #[serde(deserialize_with = "null_default")]
    pub person: PersonRef,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct CrewMember {
    #[serde(deserialize_with = "null_default")]
    pub jobs: Vec<String>,
    #[serde(deserialize_with = "null_default")]
    pub person: PersonRef,
}

/// `/movies/{id}/people` and `/shows/{id}/people`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct People {
    #[serde(deserialize_with = "null_default")]
    pub cast: Vec<CastMember>,
    /// Crew by department: `directing`, `writing`, `production`, `sound`, `camera`, ...
    #[serde(deserialize_with = "null_default")]
    pub crew: BTreeMap<String, Vec<CrewMember>>,
}

/// One entry of a `translations` answer.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub(super) struct Translation {
    pub title: Option<String>,
    pub overview: Option<String>,
    pub tagline: Option<String>,
    pub country: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct Season {
    pub number: Option<u32>,
    #[serde(deserialize_with = "null_default")]
    pub episodes: Vec<Episode>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct Episode {
    pub season: Option<u32>,
    pub number: Option<u32>,
    pub number_abs: Option<u32>,
    pub title: Option<String>,
    pub overview: Option<String>,
    #[serde(deserialize_with = "null_default")]
    pub ids: Ids,
    pub rating: Option<f64>,
    pub votes: Option<u64>,
    pub first_aired: Option<String>,
    pub runtime: Option<u32>,
}

/// The body of an error answer, when there is one.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct ApiError {
    pub error: Option<String>,
    pub error_description: Option<String>,
}
