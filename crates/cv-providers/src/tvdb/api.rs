//! The JSON shapes TheTVDB v4 answers with. Every field is optional on the wire, so everything
//! defaults: a missing or `null` value never fails a whole response. IDs arrive as numbers in
//! some answers and as strings in others, so they are read as text.

use std::collections::BTreeMap;
use std::fmt;

use serde::de::{self, Visitor};
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
        self.as_deref().unwrap_or("")
    }
}

/// A value that is a string, a number or `null`, kept as trimmed text (`""` for `null`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Flex(pub String);

impl Flex {
    pub fn as_str(&self) -> &str {
        self.0.trim()
    }

    pub fn is_blank(&self) -> bool {
        self.as_str().is_empty() || self.as_str() == "0"
    }

    pub fn as_u64(&self) -> Option<u64> {
        self.as_str().parse().ok().filter(|n| *n != 0)
    }
}

impl<'de> Deserialize<'de> for Flex {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Flex;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a string, a number or null")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Flex, E> {
                Ok(Flex(v.to_owned()))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Flex, E> {
                Ok(Flex(v.to_string()))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Flex, E> {
                Ok(Flex(v.to_string()))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Flex, E> {
                Ok(Flex(if v.fract() == 0.0 {
                    format!("{v:.0}")
                } else {
                    v.to_string()
                }))
            }
            fn visit_bool<E: de::Error>(self, _: bool) -> Result<Flex, E> {
                Ok(Flex::default())
            }
            fn visit_unit<E: de::Error>(self) -> Result<Flex, E> {
                Ok(Flex::default())
            }
            fn visit_none<E: de::Error>(self) -> Result<Flex, E> {
                Ok(Flex::default())
            }
            fn visit_some<D2: Deserializer<'de>>(self, d: D2) -> Result<Flex, D2::Error> {
                d.deserialize_any(V)
            }
        }
        d.deserialize_any(V)
    }
}

/// `{"status": "...", "data": ..., "links": ...}`, the wrapper of every answer.
#[derive(Debug, Deserialize)]
#[serde(default, bound(deserialize = "T: Deserialize<'de>"))]
pub(super) struct Envelope<T> {
    pub data: Option<T>,
    pub links: Option<Links>,
}

impl<T> Default for Envelope<T> {
    fn default() -> Self {
        Self {
            data: None,
            links: None,
        }
    }
}

/// Paging information of a list answer.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct Links {
    /// The next page number, `null` on the last page.
    pub next: Flex,
}

/// The body of an error answer.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct ApiError {
    #[serde(alias = "Message")]
    pub message: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct LoginData {
    pub token: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub(super) struct RemoteId {
    pub id: Flex,
    #[serde(rename = "type")]
    pub kind: u32,
}

/// One entry of a `search` answer.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct SearchItem {
    pub tvdb_id: Flex,
    pub name: Option<String>,
    pub year: Flex,
    pub first_air_time: Option<String>,
    pub overview: Option<String>,
    pub image_url: Option<String>,
    #[serde(deserialize_with = "null_default")]
    pub translations: BTreeMap<String, String>,
    #[serde(deserialize_with = "null_default")]
    pub overviews: BTreeMap<String, String>,
    #[serde(deserialize_with = "null_default")]
    pub remote_ids: Vec<RemoteId>,
}

/// One entry of a `search/remoteid` answer.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct RemoteMatch {
    pub series: Option<IdOnly>,
    pub movie: Option<IdOnly>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct IdOnly {
    pub id: Flex,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub(super) struct Named {
    pub name: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(super) struct NameTranslation {
    pub name: Option<String>,
    pub language: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(super) struct OverviewTranslation {
    pub overview: Option<String>,
    pub language: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(super) struct Translations {
    #[serde(deserialize_with = "null_default")]
    pub name_translations: Vec<NameTranslation>,
    #[serde(deserialize_with = "null_default")]
    pub overview_translations: Vec<OverviewTranslation>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(super) struct Artwork {
    pub image: Option<String>,
    pub language: Option<String>,
    #[serde(rename = "type")]
    pub kind: u32,
    pub score: Option<f64>,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(super) struct Character {
    /// The character's name for an actor.
    pub name: Option<String>,
    pub people_id: Flex,
    pub person_name: Option<String>,
    pub image: Option<String>,
    #[serde(rename = "personImgURL")]
    pub person_img_url: Option<String>,
    pub sort: Option<u32>,
    #[serde(rename = "type")]
    pub kind: Option<u32>,
    pub people_type: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(super) struct ContentRating {
    pub name: Option<String>,
    pub country: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub(super) struct TrailerRecord {
    pub id: Flex,
    pub name: Option<String>,
    pub url: Option<String>,
    pub language: Option<String>,
    pub runtime: Option<u32>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub(super) struct SeasonKind {
    pub name: Option<String>,
    /// The season type slug: `official`, `dvd`, `absolute`, ...
    #[serde(rename = "type")]
    pub slug: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub(super) struct SeasonRef {
    pub id: Flex,
    pub number: u32,
    pub name: Option<String>,
    pub image: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<SeasonKind>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub(super) struct Release {
    pub country: Option<String>,
    pub date: Option<String>,
}

/// A series or movie from `series/{id}/extended` or `movies/{id}/extended`.
#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(super) struct Extended {
    pub id: Flex,
    pub name: Option<String>,
    pub image: Option<String>,
    pub first_aired: Option<String>,
    pub last_aired: Option<String>,
    pub year: Flex,
    pub status: Option<Named>,
    pub original_country: Option<String>,
    pub original_language: Option<String>,
    pub average_runtime: Option<u32>,
    pub runtime: Option<u32>,
    pub overview: Option<String>,
    #[serde(deserialize_with = "null_default")]
    pub genres: Vec<Named>,
    #[serde(deserialize_with = "null_default")]
    pub artworks: Vec<Artwork>,
    #[serde(deserialize_with = "null_default")]
    pub characters: Vec<Character>,
    #[serde(deserialize_with = "null_default")]
    pub remote_ids: Vec<RemoteId>,
    /// A list for series, an object of lists (`studio`, `production`, ...) for movies.
    pub companies: serde_json::Value,
    #[serde(deserialize_with = "null_default")]
    pub content_ratings: Vec<ContentRating>,
    #[serde(deserialize_with = "null_default")]
    pub trailers: Vec<TrailerRecord>,
    #[serde(deserialize_with = "null_default")]
    pub seasons: Vec<SeasonRef>,
    #[serde(deserialize_with = "null_default")]
    pub releases: Vec<Release>,
    #[serde(deserialize_with = "null_default")]
    pub audio_languages: Vec<String>,
    pub translations: Option<Translations>,
}

/// An episode in an episode list.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(super) struct EpisodeRecord {
    pub id: Flex,
    pub name: Option<String>,
    pub overview: Option<String>,
    pub aired: Option<String>,
    pub runtime: Option<u32>,
    pub image: Option<String>,
    pub number: Option<u32>,
    pub season_number: Option<u32>,
    pub absolute_number: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct EpisodePage {
    #[serde(deserialize_with = "null_default")]
    pub episodes: Vec<EpisodeRecord>,
}

/// A season from `seasons/{id}/extended`.
#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(super) struct SeasonExtended {
    pub id: Flex,
    pub number: u32,
    pub name: Option<String>,
    pub image: Option<String>,
    pub year: Flex,
    #[serde(deserialize_with = "null_default")]
    pub artwork: Vec<Artwork>,
    pub translations: Option<Translations>,
}

/// One entry of `artwork/types`.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(super) struct ArtworkType {
    pub id: u32,
    pub name: Option<String>,
    pub record_type: Option<String>,
    pub width: u32,
    pub height: u32,
}
