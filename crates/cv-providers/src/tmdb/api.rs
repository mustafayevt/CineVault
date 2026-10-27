//! The JSON shapes TMDB answers with. Every field is optional on the wire, so everything
//! defaults: a missing or `null` value never fails a whole response.

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

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct Configuration {
    pub images: ConfigurationImages,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct ConfigurationImages {
    pub secure_base_url: Option<String>,
}

/// A page of search results.
#[derive(Debug, Deserialize)]
#[serde(default, bound(deserialize = "T: Deserialize<'de>"))]
pub(super) struct Page<T> {
    pub page: u32,
    pub total_pages: u32,
    #[serde(deserialize_with = "null_default")]
    pub results: Vec<T>,
}

impl<T> Default for Page<T> {
    fn default() -> Self {
        Self {
            page: 0,
            total_pages: 0,
            results: Vec::new(),
        }
    }
}

/// A movie, show or collection in a search, `find` or collection part list.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct SearchResult {
    pub id: u64,
    pub title: Option<String>,
    pub name: Option<String>,
    pub original_title: Option<String>,
    pub original_name: Option<String>,
    pub release_date: Option<String>,
    pub first_air_date: Option<String>,
    pub overview: Option<String>,
    pub poster_path: Option<String>,
    pub popularity: Option<f32>,
    pub vote_count: Option<u32>,
}

/// `person/{id}/external_ids`: the IDs other sites know a person by.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct PersonExternalIds {
    pub imdb_id: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct FindResponse {
    #[serde(deserialize_with = "null_default")]
    pub movie_results: Vec<SearchResult>,
    #[serde(deserialize_with = "null_default")]
    pub tv_results: Vec<SearchResult>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct Genre {
    pub id: u32,
    pub name: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct Named {
    pub name: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct Country {
    pub iso_3166_1: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct Language {
    pub iso_639_1: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct Collection {
    pub id: u64,
}

/// An ID that TMDB sends as a number, a string or `null`.
#[derive(Debug, Default, Deserialize)]
#[serde(untagged)]
pub(super) enum LooseId {
    Number(u64),
    Text(String),
    #[default]
    Missing,
}

impl LooseId {
    pub fn to_text(&self) -> String {
        match self {
            Self::Number(n) => n.to_string(),
            Self::Text(s) => s.clone(),
            Self::Missing => String::new(),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct ExternalIds {
    pub imdb_id: Option<String>,
    pub tvdb_id: LooseId,
    pub wikidata_id: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct CastMember {
    pub id: u64,
    pub name: Option<String>,
    pub character: Option<String>,
    pub order: Option<u32>,
    pub profile_path: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct CrewMember {
    pub id: u64,
    pub name: Option<String>,
    pub job: Option<String>,
    pub profile_path: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct Credits {
    #[serde(deserialize_with = "null_default")]
    pub cast: Vec<CastMember>,
    #[serde(deserialize_with = "null_default")]
    pub crew: Vec<CrewMember>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct TranslationData {
    pub title: Option<String>,
    pub name: Option<String>,
    pub overview: Option<String>,
    pub tagline: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct Translation {
    pub iso_3166_1: Option<String>,
    pub iso_639_1: Option<String>,
    pub data: TranslationData,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct Translations {
    #[serde(deserialize_with = "null_default")]
    pub translations: Vec<Translation>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct Keyword {
    pub name: Option<String>,
}

/// Movies call the list `keywords`, shows call it `results`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct Keywords {
    #[serde(deserialize_with = "null_default")]
    pub keywords: Vec<Keyword>,
    #[serde(deserialize_with = "null_default")]
    pub results: Vec<Keyword>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct ReleaseDate {
    pub certification: Option<String>,
    #[serde(rename = "type")]
    pub kind: u32,
    pub release_date: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct CountryReleases {
    pub iso_3166_1: Option<String>,
    #[serde(deserialize_with = "null_default")]
    pub release_dates: Vec<ReleaseDate>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct ReleaseDates {
    #[serde(deserialize_with = "null_default")]
    pub results: Vec<CountryReleases>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct ContentRating {
    pub iso_3166_1: Option<String>,
    pub rating: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct ContentRatings {
    #[serde(deserialize_with = "null_default")]
    pub results: Vec<ContentRating>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct MovieDetails {
    pub id: u64,
    pub imdb_id: Option<String>,
    pub title: Option<String>,
    pub original_title: Option<String>,
    pub original_language: Option<String>,
    pub tagline: Option<String>,
    pub overview: Option<String>,
    pub runtime: Option<u32>,
    pub release_date: Option<String>,
    pub adult: bool,
    #[serde(deserialize_with = "null_default")]
    pub genres: Vec<Genre>,
    #[serde(deserialize_with = "null_default")]
    pub production_companies: Vec<Named>,
    #[serde(deserialize_with = "null_default")]
    pub production_countries: Vec<Country>,
    #[serde(deserialize_with = "null_default")]
    pub spoken_languages: Vec<Language>,
    pub vote_average: Option<f32>,
    pub vote_count: Option<u32>,
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    pub belongs_to_collection: Option<Collection>,
    pub credits: Credits,
    pub keywords: Keywords,
    pub release_dates: ReleaseDates,
    pub translations: Translations,
    pub external_ids: ExternalIds,
    pub videos: Videos,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct SeasonSummary {
    pub season_number: u32,
    pub name: Option<String>,
    pub overview: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct EpisodeGroupRef {
    pub id: Option<String>,
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub kind: u32,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct EpisodeGroupRefs {
    #[serde(deserialize_with = "null_default")]
    pub results: Vec<EpisodeGroupRef>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct TvDetails {
    pub id: u64,
    pub name: Option<String>,
    pub original_name: Option<String>,
    pub original_language: Option<String>,
    pub tagline: Option<String>,
    pub overview: Option<String>,
    pub status: Option<String>,
    pub first_air_date: Option<String>,
    pub last_air_date: Option<String>,
    #[serde(deserialize_with = "null_default")]
    pub episode_run_time: Vec<u32>,
    #[serde(deserialize_with = "null_default")]
    pub genres: Vec<Genre>,
    #[serde(deserialize_with = "null_default")]
    pub production_companies: Vec<Named>,
    #[serde(deserialize_with = "null_default")]
    pub networks: Vec<Named>,
    #[serde(deserialize_with = "null_default")]
    pub origin_country: Vec<String>,
    pub vote_average: Option<f32>,
    pub vote_count: Option<u32>,
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    #[serde(deserialize_with = "null_default")]
    pub seasons: Vec<SeasonSummary>,
    pub credits: Credits,
    pub keywords: Keywords,
    pub translations: Translations,
    pub external_ids: ExternalIds,
    pub content_ratings: ContentRatings,
    pub episode_groups: EpisodeGroupRefs,
    pub videos: Videos,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct Episode {
    pub id: u64,
    pub name: Option<String>,
    pub overview: Option<String>,
    pub air_date: Option<String>,
    pub season_number: Option<u32>,
    pub episode_number: Option<u32>,
    /// Position inside an episode group (zero-based).
    pub order: Option<u32>,
    pub runtime: Option<u32>,
    pub vote_average: Option<f32>,
    pub vote_count: Option<u32>,
    pub still_path: Option<String>,
    #[serde(deserialize_with = "null_default")]
    pub guest_stars: Vec<CastMember>,
    #[serde(deserialize_with = "null_default")]
    pub crew: Vec<CrewMember>,
    pub external_ids: ExternalIds,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct SeasonDetails {
    pub id: u64,
    pub season_number: u32,
    pub name: Option<String>,
    pub overview: Option<String>,
    pub air_date: Option<String>,
    pub poster_path: Option<String>,
    #[serde(deserialize_with = "null_default")]
    pub episodes: Vec<Episode>,
    pub external_ids: ExternalIds,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct EpisodeGroupSeason {
    pub order: u32,
    #[serde(deserialize_with = "null_default")]
    pub episodes: Vec<Episode>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct EpisodeGroupDetails {
    #[serde(deserialize_with = "null_default")]
    pub groups: Vec<EpisodeGroupSeason>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct CollectionDetails {
    pub id: u64,
    pub name: Option<String>,
    pub overview: Option<String>,
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    #[serde(deserialize_with = "null_default")]
    pub parts: Vec<SearchResult>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct Image {
    pub file_path: Option<String>,
    pub width: u32,
    pub height: u32,
    pub iso_639_1: Option<String>,
    pub vote_average: Option<f32>,
    pub vote_count: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct Images {
    #[serde(deserialize_with = "null_default")]
    pub posters: Vec<Image>,
    #[serde(deserialize_with = "null_default")]
    pub backdrops: Vec<Image>,
    #[serde(deserialize_with = "null_default")]
    pub logos: Vec<Image>,
    #[serde(deserialize_with = "null_default")]
    pub stills: Vec<Image>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct Video {
    pub id: Option<String>,
    pub key: Option<String>,
    pub name: Option<String>,
    pub site: Option<String>,
    pub size: u32,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub iso_639_1: Option<String>,
    pub iso_3166_1: Option<String>,
    pub published_at: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct Videos {
    #[serde(deserialize_with = "null_default")]
    pub results: Vec<Video>,
}

/// The error body, for example `{"status_code":7,"status_message":"Invalid API key"}`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct ApiError {
    pub status_message: Option<String>,
}
