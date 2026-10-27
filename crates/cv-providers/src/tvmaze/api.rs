//! The JSON shapes the TVmaze API answers with. Every field is optional on the wire, so
//! everything defaults: a missing or `null` value never fails a whole response.

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

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub(super) struct Image {
    pub medium: Option<String>,
    pub original: Option<String>,
}

impl Image {
    /// The full-size URL, else the medium one.
    pub fn best(&self) -> Option<&str> {
        [self.original.as_deref(), self.medium.as_deref()]
            .into_iter()
            .flatten()
            .map(str::trim)
            .find(|u| !u.is_empty())
    }
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub(super) struct Rating {
    pub average: Option<f64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct Country {
    pub code: Option<String>,
}

/// A network or web channel.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct Network {
    pub name: Option<String>,
    pub country: Option<Country>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct Externals {
    pub imdb: Option<String>,
    pub thetvdb: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct Person {
    pub id: Option<u64>,
    pub name: Option<String>,
    pub url: Option<String>,
    pub image: Option<Image>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct Character {
    pub name: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct CastEntry {
    pub person: Person,
    pub character: Character,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct CrewEntry {
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub person: Person,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct Resolution {
    pub url: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct Resolutions {
    pub original: Option<Resolution>,
    pub medium: Option<Resolution>,
}

/// One entry of the `images` list: a poster, background, banner or typography image.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct ImageEntry {
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub main: bool,
    pub resolutions: Option<Resolutions>,
}

impl ImageEntry {
    pub fn url(&self) -> Option<&str> {
        let resolutions = self.resolutions.as_ref()?;
        [&resolutions.original, &resolutions.medium]
            .into_iter()
            .flatten()
            .filter_map(|r| r.url.as_deref())
            .map(str::trim)
            .find(|u| !u.is_empty())
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct SeasonEntry {
    pub number: Option<u32>,
    pub name: Option<String>,
    pub summary: Option<String>,
}

/// What `embed[]` adds to a show.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct Embedded {
    #[serde(deserialize_with = "null_default")]
    pub cast: Vec<CastEntry>,
    #[serde(deserialize_with = "null_default")]
    pub crew: Vec<CrewEntry>,
    #[serde(deserialize_with = "null_default")]
    pub images: Vec<ImageEntry>,
    #[serde(deserialize_with = "null_default")]
    pub seasons: Vec<SeasonEntry>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct Show {
    pub id: Option<u64>,
    pub name: Option<String>,
    pub language: Option<String>,
    #[serde(deserialize_with = "null_default")]
    pub genres: Vec<String>,
    pub status: Option<String>,
    pub runtime: Option<u32>,
    #[serde(rename = "averageRuntime")]
    pub average_runtime: Option<u32>,
    pub premiered: Option<String>,
    pub ended: Option<String>,
    pub rating: Option<Rating>,
    pub network: Option<Network>,
    #[serde(rename = "webChannel")]
    pub web_channel: Option<Network>,
    pub externals: Option<Externals>,
    pub image: Option<Image>,
    pub summary: Option<String>,
    #[serde(rename = "_embedded")]
    pub embedded: Option<Embedded>,
}

/// One entry of `/search/shows`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct SearchEntry {
    pub show: Show,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub(super) struct Episode {
    pub id: Option<u64>,
    pub name: Option<String>,
    pub season: Option<u32>,
    pub number: Option<u32>,
    pub airdate: Option<String>,
    pub runtime: Option<u32>,
    pub rating: Option<Rating>,
    pub image: Option<Image>,
    pub summary: Option<String>,
}

/// One entry of `/shows/{id}/alternatelists`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct AlternateList {
    pub id: Option<u64>,
    pub name: Option<String>,
    pub dvd_release: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct AlternateEmbedded {
    #[serde(deserialize_with = "null_default")]
    pub episodes: Vec<Episode>,
}

/// One entry of `/alternatelists/{id}/alternateepisodes?embed=episodes`: the number in the
/// alternate order and the episode it stands for.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct AlternateEpisode {
    pub season: Option<u32>,
    pub number: Option<u32>,
    #[serde(rename = "_embedded")]
    pub embedded: Option<AlternateEmbedded>,
}
