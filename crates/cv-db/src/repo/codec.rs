//! Conversions between domain values and their stored text/JSON form.

use cv_domain::{EpisodeGroup, EpisodeOrdering, MissingEpisode, MissingMovie, SeasonText};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::error::{DbError, DbResult};

/// The serde name of a unit-like enum, as stored in a text column.
pub(crate) fn enum_text<T: Serialize>(value: &T) -> DbResult<String> {
    match serde_json::to_value(value)? {
        serde_json::Value::String(s) => Ok(s),
        other => Err(DbError::InvalidData(format!(
            "expected a text enum value, got {other}"
        ))),
    }
}

/// The inverse of [`enum_text`].
pub(crate) fn parse_enum<T: DeserializeOwned>(text: &str, what: &str) -> DbResult<T> {
    serde_json::from_value(serde_json::Value::String(text.to_owned()))
        .map_err(|_| DbError::InvalidData(format!("unknown {what} `{text}`")))
}

/// Encodes an episode group for `item.episode_group`: `aired`, `dvd`, `dvd:Production`.
pub(crate) fn group_text(group: &EpisodeGroup) -> DbResult<String> {
    let ordering = enum_text(&group.ordering)?;
    Ok(
        if group.name.is_empty() || group.ordering == EpisodeOrdering::Aired {
            ordering
        } else {
            format!("{ordering}:{}", group.name)
        },
    )
}

/// The inverse of [`group_text`].
pub(crate) fn parse_group(text: &str) -> DbResult<EpisodeGroup> {
    let (ordering, name) = text.split_once(':').unwrap_or((text, ""));
    Ok(EpisodeGroup::new(
        parse_enum(ordering, "episode ordering")?,
        name,
    ))
}

/// Rarely queried, kind-specific data stored in the `item.ext` JSON column.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct Ext {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub spoken_languages: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub countries: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub extra_thumbs: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub extra_fanarts: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub extra_fanart_urls: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub show_links: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub missing_movies: Vec<MissingMovie>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub episode_groups: Vec<EpisodeGroup>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub season_texts: Vec<SeasonText>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub missing_episodes: Vec<MissingEpisode>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub episode_list_fetched: bool,
}

/// `u64` to SQLite's signed 64-bit integer, saturating.
pub(crate) fn to_i64(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

/// SQLite integer to `u64`, clamping negatives to zero.
pub(crate) fn to_u64(v: i64) -> u64 {
    u64::try_from(v).unwrap_or(0)
}

/// SQLite integer to `u32`, clamping into range.
pub(crate) fn to_u32(v: i64) -> u32 {
    u32::try_from(v.max(0)).unwrap_or(u32::MAX)
}

/// SQLite integer to `u16`, clamping into range.
pub(crate) fn to_u16(v: i64) -> u16 {
    u16::try_from(v.max(0)).unwrap_or(u16::MAX)
}
