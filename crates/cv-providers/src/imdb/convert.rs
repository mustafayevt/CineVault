//! IMDb page data to CineVault items and search hits.
//!
//! Parsing is two steps so a failure can never leave half a result: [`TitlePage::parse`] checks
//! everything the provider requires and fails with a [`ParseError`] otherwise; only then do the
//! infallible conversions build an item or a hit. Optional values that are absent stay empty.

use cv_domain::{
    AiredStatus, CalendarDate, Certification, Credit, CreditKind, Genre, Item, ItemDetail,
    MediaFileType, Person, ProviderIds, Rating, id_key,
};
use serde_json::Value;

use super::PROVIDER_ID;
use super::next_data::{self, ParseError, float, list, number, text};
use crate::model::{MediaKind, RequestOptions, SearchHit};
use crate::tmdb::certification_token;

/// The rating source key IMDb ratings are stored under.
pub const RATING_ID: &str = "imdb";

/// What a title is, as far as the provider cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PageKind {
    /// A movie-like title (feature, TV movie, short, video, special).
    Movie,
    /// A series or mini-series.
    Show,
    /// Something the library has no item for (an episode, a game, a person's page...).
    Other,
    /// The page does not say.
    Unknown,
}

impl PageKind {
    fn from_type_id(id: &str) -> Self {
        match id.to_ascii_lowercase().as_str() {
            "movie" | "tvmovie" | "short" | "tvshort" | "video" | "tvspecial" | "musicvideo" => {
                Self::Movie
            }
            "tvseries" | "tvminiseries" => Self::Show,
            _ => Self::Other,
        }
    }

    /// Whether a title of this kind can be returned for a request of `kind`.
    pub(super) fn fits(self, kind: MediaKind) -> bool {
        match self {
            Self::Movie => kind == MediaKind::Movie,
            Self::Show => kind == MediaKind::Show,
            Self::Other => false,
            Self::Unknown => true,
        }
    }
}

/// Whether `id` looks like an IMDb title ID (`tt` and at least six digits).
pub(super) fn is_title_id(id: &str) -> bool {
    id.strip_prefix("tt")
        .is_some_and(|d| d.len() >= 6 && d.bytes().all(|b| b.is_ascii_digit()))
}

// --- the title page -------------------------------------------------------------------------

/// A title page that passed the required-value checks.
pub(super) struct TitlePage {
    props: Value,
    pub id: String,
    pub title: String,
    pub kind: PageKind,
}

impl TitlePage {
    /// Reads a title page. Requires the `aboveTheFoldData` block with a title ID and a title.
    pub(super) fn parse(html: &str) -> Result<Self, ParseError> {
        let mut data = next_data::extract(html)?;
        next_data::page_props(&data)?;
        let props = data
            .pointer_mut("/props/pageProps")
            .map(Value::take)
            .ok_or(ParseError::Missing("pageProps"))?;
        let above = props
            .get("aboveTheFoldData")
            .filter(|v| v.is_object())
            .ok_or(ParseError::Missing("aboveTheFoldData"))?;
        let id = text(above, "/id")
            .filter(|id| is_title_id(id))
            .ok_or(ParseError::Missing("title id"))?
            .to_owned();
        let title = text(above, "/titleText/text")
            .ok_or(ParseError::Missing("title"))?
            .to_owned();
        let kind = text(above, "/titleType/id").map_or(PageKind::Unknown, PageKind::from_type_id);
        Ok(Self {
            props,
            id,
            title,
            kind,
        })
    }

    pub(super) fn above(&self) -> &Value {
        &self.props["aboveTheFoldData"]
    }

    pub(super) fn main(&self) -> &Value {
        &self.props["mainColumnData"]
    }

    fn release_date(&self) -> Option<CalendarDate> {
        let date = self.above().get("releaseDate")?;
        CalendarDate::new(
            i32::try_from(number(date, "/year")?).ok()?,
            u8::try_from(number(date, "/month")?).ok()?,
            u8::try_from(number(date, "/day")?).ok()?,
        )
    }

    fn year(&self) -> u16 {
        number(self.above(), "/releaseYear/year")
            .or_else(|| number(self.above(), "/releaseDate/year"))
            .and_then(|y| u16::try_from(y).ok())
            .unwrap_or(0)
    }

    fn poster(&self) -> Option<&str> {
        text(self.above(), "/primaryImage/url")
    }

    /// The title's main poster with its size, for the artwork list.
    pub(super) fn primary_image(&self) -> Option<super::images::Image> {
        super::images::Image::read(self.above().get("primaryImage")?)
    }

    fn plot(&self) -> &str {
        text(self.above(), "/plot/plotText/plainText").unwrap_or("")
    }

    fn original_title(&self) -> &str {
        text(self.above(), "/originalTitleText/text").unwrap_or("")
    }

    fn runtime_minutes(&self) -> u32 {
        number(self.above(), "/runtime/seconds")
            .map(|s| u32::try_from((s + 30) / 60).unwrap_or(u32::MAX))
            .unwrap_or(0)
    }

    fn genres(&self) -> Vec<Genre> {
        let mut out: Vec<Genre> = Vec::new();
        for entry in list(self.above(), "/genres/genres") {
            if let Some(genre) = text(entry, "/text").and_then(Genre::parse)
                && !out.contains(&genre)
            {
                out.push(genre);
            }
        }
        out
    }

    fn certification(&self, options: &RequestOptions) -> Certification {
        text(self.above(), "/certificate/rating")
            .map(|rating| certification_token(&options.country, rating))
            .unwrap_or_default()
    }

    fn companies(&self) -> Vec<String> {
        distinct(
            list(self.main(), "/production/edges")
                .iter()
                .filter_map(|edge| text(edge, "/node/company/companyText/text"))
                .map(str::to_owned),
        )
    }

    fn countries(&self) -> Vec<String> {
        distinct(
            list(self.main(), "/countriesOfOrigin/countries")
                .iter()
                .filter_map(|c| text(c, "/id"))
                .map(str::to_ascii_uppercase),
        )
    }

    fn languages(&self) -> Vec<String> {
        distinct(
            list(self.main(), "/spokenLanguages/spokenLanguages")
                .iter()
                .filter_map(|l| text(l, "/id"))
                .map(str::to_ascii_lowercase),
        )
    }

    fn tagline(&self) -> &str {
        text(self.main(), "/taglines/edges/0/node/text").unwrap_or("")
    }

    fn rating(&self) -> Option<Rating> {
        let value = float(self.above(), "/ratingsSummary/aggregateRating")?;
        let votes = number(self.above(), "/ratingsSummary/voteCount").unwrap_or(0);
        (value > 0.0).then(|| {
            Rating::new(
                RATING_ID,
                value as f32,
                u32::try_from(votes).unwrap_or(u32::MAX),
                10,
            )
        })
    }

    fn credits(&self) -> Vec<Credit> {
        let mut out: Vec<Credit> = Vec::new();
        let mut add = |credit: Credit| {
            let same = |c: &Credit| {
                c.kind == credit.kind
                    && match (
                        c.person.ids.get(id_key::IMDB),
                        credit.person.ids.get(id_key::IMDB),
                    ) {
                        (Some(a), Some(b)) => a == b,
                        _ => c.person.name == credit.person.name,
                    }
            };
            if !out.iter().any(same) {
                out.push(credit);
            }
        };
        for (kind, path) in [
            (CreditKind::Director, "/directorsPageTitle/0/credits"),
            (CreditKind::Writer, "/writersPageTitle/0/credits"),
        ] {
            for entry in list(self.above(), path) {
                if let Some(credit) = credit(kind, entry, "") {
                    add(credit);
                }
            }
        }
        let mut cast = list(self.above(), "/castPageTitle/edges");
        if cast.is_empty() {
            cast = list(self.main(), "/cast/edges");
        }
        for edge in cast {
            let Some(node) = edge.get("node") else {
                continue;
            };
            let role = list(node, "/characters")
                .iter()
                .filter_map(|c| text(c, "/name"))
                .collect::<Vec<_>>()
                .join(", ");
            if let Some(credit) = credit(CreditKind::Actor, node, &role) {
                add(credit);
            }
        }
        out
    }

    /// The item for this page as a `kind` (the caller has checked [`PageKind::fits`]).
    pub(super) fn build_item(&self, kind: MediaKind, options: &RequestOptions) -> Item {
        let mut item = match kind {
            MediaKind::Movie => Item::movie(&self.title),
            MediaKind::Show => Item::show(&self.title),
        };
        let core = &mut item.core;
        core.ids.set(id_key::IMDB, &self.id);
        core.original_title = self.original_title().to_owned();
        core.year = self.year();
        core.plot = self.plot().to_owned();
        core.production_companies = self.companies();
        core.credits = self.credits();
        if let Some(rating) = self.rating() {
            core.ratings.insert(rating);
        }
        if let Some(poster) = self.poster() {
            core.artwork_urls
                .insert(MediaFileType::Poster, poster.to_owned());
        }

        let english_title = if options.language.to_ascii_lowercase().starts_with("en") {
            self.title.clone()
        } else {
            String::new()
        };
        let release = self.release_date();
        let countries = self.countries();
        match &mut item.detail {
            ItemDetail::Movie(movie) => {
                movie.english_title = english_title;
                movie.tagline = self.tagline().to_owned();
                movie.runtime_minutes = self.runtime_minutes();
                movie.release_date = release;
                movie.certification = self.certification(options);
                movie.genres = self.genres();
                movie.spoken_languages = self.languages();
                movie.countries = countries;
            }
            ItemDetail::Show(show) => {
                show.english_title = english_title;
                show.tagline = self.tagline().to_owned();
                show.runtime_minutes = self.runtime_minutes();
                show.first_aired = release;
                show.certification = self.certification(options);
                show.genres = self.genres();
                show.country = countries.into_iter().next().unwrap_or_default();
                match number(self.above(), "/releaseYear/endYear") {
                    Some(end) => {
                        show.year_end = u16::try_from(end).unwrap_or(0);
                        show.status = AiredStatus::Ended;
                    }
                    None => show.status = AiredStatus::Continuing,
                }
            }
            _ => {}
        }
        item
    }

    /// The hit for an ID lookup.
    pub(super) fn build_hit(&self) -> SearchHit {
        let mut ids = ProviderIds::new();
        ids.set(id_key::IMDB, &self.id);
        SearchHit {
            provider: PROVIDER_ID.to_owned(),
            ids,
            title: self.title.clone(),
            original_title: self.original_title().to_owned(),
            year: self.year(),
            overview: self.plot().to_owned(),
            poster_url: self.poster().map(str::to_owned),
            exact_id: true,
            popularity: 0.0,
            votes: 0,
        }
    }
}

fn distinct(values: impl Iterator<Item = String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for value in values {
        if !out.contains(&value) {
            out.push(value);
        }
    }
    out
}

/// A credit from an object with a `name` (`id` and `nameText.text`).
fn credit(kind: CreditKind, entry: &Value, role: &str) -> Option<Credit> {
    let name = text(entry, "/name/nameText/text")?;
    let mut ids = ProviderIds::new();
    if let Some(id) = text(entry, "/name/id") {
        ids.set(id_key::IMDB, id);
    }
    let mut credit = Credit::new(kind, name, role);
    credit.person = Person {
        ids,
        ..credit.person
    };
    Some(credit)
}

// --- the find page --------------------------------------------------------------------------

/// The title hits of a find page for `kind`.
///
/// Requires `titleResults.results` to exist. An empty list is a search without results. If
/// there are results but none can be read, the page changed.
pub(super) fn parse_find(html: &str, kind: MediaKind) -> Result<Vec<SearchHit>, ParseError> {
    let data = next_data::extract(html)?;
    let props = next_data::page_props(&data)?;
    let results = props
        .pointer("/titleResults/results")
        .and_then(Value::as_array)
        .ok_or(ParseError::Missing("titleResults"))?;
    let mut hits = Vec::new();
    let mut readable = 0usize;
    for entry in results {
        let (Some(id), Some(title)) = (
            text(entry, "/id").filter(|id| is_title_id(id)),
            text(entry, "/titleNameText"),
        ) else {
            continue;
        };
        readable += 1;
        if !result_kind(entry).fits(kind) {
            continue;
        }
        let mut ids = ProviderIds::new();
        ids.set(id_key::IMDB, id);
        hits.push(SearchHit {
            provider: PROVIDER_ID.to_owned(),
            ids,
            title: title.to_owned(),
            original_title: String::new(),
            year: text(entry, "/titleReleaseText").map_or(0, year_of),
            overview: String::new(),
            poster_url: text(entry, "/titlePosterImageModel/url").map(str::to_owned),
            exact_id: false,
            popularity: 0.0,
            votes: 0,
        });
    }
    if !results.is_empty() && readable == 0 {
        return Err(ParseError::Missing("title results"));
    }
    Ok(hits)
}

fn result_kind(entry: &Value) -> PageKind {
    if let Some(id) = text(entry, "/imageType") {
        return PageKind::from_type_id(id);
    }
    match text(entry, "/titleTypeText").map(str::to_ascii_lowercase) {
        None => PageKind::Unknown,
        Some(t) if t.contains("series") => PageKind::Show,
        Some(t) if t.contains("episode") || t.contains("game") => PageKind::Other,
        Some(_) => PageKind::Movie,
    }
}

/// The first four digits of `1995` or `2008–2013`.
fn year_of(text: &str) -> u16 {
    text.get(..4)
        .and_then(|y| y.parse::<u16>().ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(props: &str) -> String {
        format!(
            r#"<html><script id="__NEXT_DATA__" type="application/json">{{"props":{{"pageProps":{props}}}}}</script></html>"#
        )
    }

    #[test]
    fn title_ids_need_tt_and_six_digits() {
        assert!(is_title_id("tt0113277"));
        assert!(is_title_id("tt123456"));
        assert!(!is_title_id("tt12345"));
        assert!(!is_title_id("nm0000093"));
        assert!(!is_title_id("tt01x3277"));
    }

    #[test]
    fn a_title_page_needs_an_id_and_a_title() {
        let no_above = TitlePage::parse(&page(r#"{"other":1}"#));
        assert_eq!(
            no_above.err(),
            Some(ParseError::Missing("aboveTheFoldData"))
        );
        let no_id = TitlePage::parse(&page(r#"{"aboveTheFoldData":{"titleText":{"text":"X"}}}"#));
        assert_eq!(no_id.err(), Some(ParseError::Missing("title id")));
        let no_title = TitlePage::parse(&page(r#"{"aboveTheFoldData":{"id":"tt0113277"}}"#));
        assert_eq!(no_title.err(), Some(ParseError::Missing("title")));
    }

    #[test]
    fn a_bare_title_converts_with_empty_optional_fields() {
        let html = page(r#"{"aboveTheFoldData":{"id":"tt0113277","titleText":{"text":"Heat"}}}"#);
        let page = TitlePage::parse(&html).unwrap();
        let item = page.build_item(MediaKind::Movie, &RequestOptions::default());
        assert_eq!(item.core.title, "Heat");
        assert_eq!(item.core.ids.get(id_key::IMDB), Some("tt0113277"));
        assert_eq!(item.core.year, 0);
        assert!(item.core.ratings.is_empty());
        assert!(item.core.credits.is_empty());
    }

    #[test]
    fn page_kinds_gate_the_requested_kind() {
        assert!(PageKind::from_type_id("tvSeries").fits(MediaKind::Show));
        assert!(!PageKind::from_type_id("tvSeries").fits(MediaKind::Movie));
        assert!(PageKind::from_type_id("tvMovie").fits(MediaKind::Movie));
        assert!(!PageKind::from_type_id("tvEpisode").fits(MediaKind::Show));
        assert!(PageKind::Unknown.fits(MediaKind::Movie));
    }

    #[test]
    fn a_find_page_with_unreadable_results_is_a_format_change() {
        let html = page(r#"{"titleResults":{"results":[{"unexpected":true}]}}"#);
        assert!(parse_find(&html, MediaKind::Movie).is_err());
        let empty = page(r#"{"titleResults":{"results":[]}}"#);
        assert_eq!(parse_find(&empty, MediaKind::Movie), Ok(Vec::new()));
        let missing = page(r#"{"titleResults":{}}"#);
        assert!(parse_find(&missing, MediaKind::Movie).is_err());
    }

    #[test]
    fn years_come_from_the_first_four_digits() {
        assert_eq!(year_of("2008–2013"), 2008);
        assert_eq!(year_of("1995"), 1995);
        assert_eq!(year_of("n/a"), 0);
    }
}
