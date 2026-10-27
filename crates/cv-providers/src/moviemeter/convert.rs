//! MovieMeter answers to CineVault items and hits.
//!
//! MovieMeter's JSON is read leniently: a value may be text or a number, a list may hold names
//! or `{"name": ...}` objects, and a missing value is simply empty. A title needs an ID and a
//! title; everything else is optional.

use cv_domain::{
    Credit, CreditKind, Genre, Item, ItemDetail, MediaFileType, ProviderIds, Rating, id_key,
};
use serde_json::Value;

use super::{ID_KEY, PROVIDER_ID};
use crate::model::SearchHit;
use crate::shared::{country_codes, imdb_title_id, set_id};

/// The rating source key MovieMeter ratings are stored under.
pub const RATING_ID: &str = "moviemeter";

/// MovieMeter rates from one to five stars.
const RATING_SCALE: u32 = 5;

/// Dutch genre names to the names CineVault's genres use. Others pass through unchanged.
const GENRES: &[(&str, &str)] = &[
    ("actie", "Action"),
    ("avontuur", "Adventure"),
    ("animatie", "Animation"),
    ("biografie", "Biography"),
    ("komedie", "Comedy"),
    ("misdaad", "Crime"),
    ("documentaire", "Documentary"),
    ("familie", "Family"),
    ("geschiedenis", "History"),
    ("historisch", "History"),
    ("muziek", "Music"),
    ("mysterie", "Mystery"),
    ("romantiek", "Romance"),
    ("romantisch", "Romance"),
    ("sciencefiction", "Science Fiction"),
    ("science fiction", "Science Fiction"),
    ("oorlog", "War"),
];

/// Dutch country names to the two-letter codes CineVault stores.
const COUNTRIES: &[(&str, &str)] = &[
    ("verenigde staten", "US"),
    ("verenigd koninkrijk", "GB"),
    ("groot-brittannië", "GB"),
    ("duitsland", "DE"),
    ("frankrijk", "FR"),
    ("nederland", "NL"),
    ("belgië", "BE"),
    ("italië", "IT"),
    ("spanje", "ES"),
    ("zweden", "SE"),
    ("denemarken", "DK"),
    ("noorwegen", "NO"),
    ("zwitserland", "CH"),
    ("oostenrijk", "AT"),
    ("rusland", "RU"),
    ("zuid-korea", "KR"),
    ("china", "CN"),
    ("japan", "JP"),
    ("australië", "AU"),
    ("canada", "CA"),
    ("ierland", "IE"),
    ("polen", "PL"),
    ("turkije", "TR"),
];

/// A MovieMeter film: a search result or a detail answer.
pub(super) struct Film {
    value: Value,
    id: String,
    title: String,
}

impl Film {
    /// `None` when the entry has no ID or no title.
    pub(super) fn from_value(value: &Value) -> Option<Self> {
        let id = match value.get("id")? {
            Value::Number(n) => n.to_string(),
            Value::String(s) => s.trim().to_owned(),
            _ => return None,
        };
        if id.is_empty() || id == "0" {
            return None;
        }
        let title = text(value, "title")
            .or_else(|| text(value, "display_title"))?
            .to_owned();
        Some(Self {
            value: value.clone(),
            id,
            title,
        })
    }

    fn imdb(&self) -> Option<String> {
        let raw = match self.value.get("imdb")? {
            Value::String(s) => s.trim().to_owned(),
            Value::Number(n) => n.to_string(),
            _ => return None,
        };
        // Bare digits are an ID without its `tt` prefix.
        if raw.bytes().all(|b| b.is_ascii_digit()) && !raw.is_empty() {
            return imdb_title_id(&format!("tt{raw:0>7}"));
        }
        imdb_title_id(&raw)
    }

    fn ids(&self) -> ProviderIds {
        let mut ids = ProviderIds::new();
        set_id(&mut ids, ID_KEY, &self.id);
        if let Some(imdb) = self.imdb() {
            set_id(&mut ids, id_key::IMDB, imdb);
        }
        ids
    }

    fn year(&self) -> u16 {
        number(&self.value, "year")
            .and_then(|y| u16::try_from(y).ok())
            .unwrap_or(0)
    }

    /// The other title, such as the original one: `alternative_title` or the first of
    /// `alternative_titles`.
    fn original_title(&self) -> String {
        if let Some(title) = text(&self.value, "alternative_title") {
            return title.to_owned();
        }
        names(&self.value, "alternative_titles")
            .into_iter()
            .next()
            .unwrap_or_default()
    }

    fn poster(&self) -> Option<String> {
        if let Some(url) = text(&self.value, "poster").filter(|u| u.starts_with("http")) {
            return Some(url.to_owned());
        }
        let posters = self.value.get("posters")?;
        if let Some(url) = posters.as_str().filter(|u| u.starts_with("http")) {
            return Some(url.to_owned());
        }
        // Largest first.
        ["large", "original", "regular", "medium", "small"]
            .iter()
            .find_map(|size| text(posters, size).filter(|u| u.starts_with("http")))
            .map(str::to_owned)
    }

    pub(super) fn build_hit(&self, exact_id: bool) -> SearchHit {
        SearchHit {
            provider: PROVIDER_ID.to_owned(),
            ids: self.ids(),
            title: self.title.clone(),
            original_title: self.original_title(),
            year: self.year(),
            overview: text(&self.value, "plot").unwrap_or("").to_owned(),
            poster_url: self.poster(),
            exact_id,
            popularity: 0.0,
            votes: 0,
        }
    }

    fn genres(&self) -> Vec<Genre> {
        let mut out: Vec<Genre> = Vec::new();
        for name in names(&self.value, "genres") {
            let english = GENRES
                .iter()
                .find(|(dutch, _)| dutch.eq_ignore_ascii_case(&name))
                .map_or(name.as_str(), |(_, english)| english);
            if let Some(genre) = Genre::parse(english)
                && !out.contains(&genre)
            {
                out.push(genre);
            }
        }
        out
    }

    fn countries(&self) -> Vec<String> {
        let mapped: Vec<String> = names(&self.value, "countries")
            .into_iter()
            .map(|name| {
                let lower = name.to_lowercase();
                COUNTRIES
                    .iter()
                    .find(|(dutch, _)| *dutch == lower)
                    .map_or(name, |(_, code)| (*code).to_owned())
            })
            .collect();
        country_codes(&mapped)
    }

    fn credits(&self) -> Vec<Credit> {
        let mut out: Vec<Credit> = Vec::new();
        for (field, kind) in [
            ("directors", CreditKind::Director),
            ("actors", CreditKind::Actor),
        ] {
            for name in names(&self.value, field) {
                let duplicate = out.iter().any(|c| c.kind == kind && c.person.name == name);
                if !duplicate {
                    out.push(Credit::new(kind, name, ""));
                }
            }
        }
        out
    }

    pub(super) fn build_item(&self) -> Item {
        let mut item = Item::movie(&self.title);
        let core = &mut item.core;
        core.ids = self.ids();
        core.original_title = self.original_title();
        core.year = self.year();
        core.plot = text(&self.value, "plot").unwrap_or("").to_owned();
        core.credits = self.credits();
        if let Some(average) = float(&self.value, "average").filter(|v| *v > 0.0) {
            let votes = number(&self.value, "votes_count").unwrap_or(0);
            core.ratings.insert(Rating::new(
                RATING_ID,
                average as f32,
                u32::try_from(votes).unwrap_or(u32::MAX),
                RATING_SCALE,
            ));
        }
        if let Some(poster) = self.poster() {
            core.artwork_urls.insert(MediaFileType::Poster, poster);
        }
        if let ItemDetail::Movie(movie) = &mut item.detail {
            movie.runtime_minutes = number(&self.value, "duration")
                .and_then(|d| u32::try_from(d).ok())
                .unwrap_or(0);
            movie.genres = self.genres();
            movie.countries = self.countries();
        }
        item
    }
}

fn text<'a>(value: &'a Value, name: &str) -> Option<&'a str> {
    let text = value.get(name)?.as_str()?.trim();
    (!text.is_empty()).then_some(text)
}

fn number(value: &Value, name: &str) -> Option<u64> {
    match value.get(name)? {
        Value::Number(n) => n.as_u64().or_else(|| n.as_f64().map(|f| f.max(0.0) as u64)),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn float(value: &Value, name: &str) -> Option<f64> {
    match value.get(name)? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().replace(',', ".").parse().ok(),
        _ => None,
    }
}

/// A list of names: strings, or objects with a `name` or `title`.
fn names(value: &Value, field: &str) -> Vec<String> {
    let Some(list) = value.get(field).and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    for entry in list {
        let name = match entry {
            Value::String(s) => Some(s.trim()),
            other => text(other, "name").or_else(|| text(other, "title")),
        };
        if let Some(name) = name.filter(|n| !n.is_empty())
            && !out.iter().any(|o| o == name)
        {
            out.push(name.to_owned());
        }
    }
    out
}

/// The films of a search answer: a list, or an object holding one under `films` or `results`.
pub(super) fn search_films(value: &Value) -> Vec<Film> {
    let list = value
        .as_array()
        .or_else(|| value.get("films").and_then(Value::as_array))
        .or_else(|| value.get("results").and_then(Value::as_array));
    list.map(|l| l.iter().filter_map(Film::from_value).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn film(json: &str) -> Film {
        Film::from_value(&serde_json::from_str(json).unwrap()).unwrap()
    }

    #[test]
    fn a_film_needs_an_id_and_a_title() {
        let v = |s: &str| serde_json::from_str::<Value>(s).unwrap();
        assert!(Film::from_value(&v(r#"{"title":"Heat"}"#)).is_none());
        assert!(Film::from_value(&v(r#"{"id":0,"title":"Heat"}"#)).is_none());
        assert!(Film::from_value(&v(r#"{"id":1}"#)).is_none());
        assert!(Film::from_value(&v(r#"{"id":"7","title":"Heat"}"#)).is_some());
    }

    #[test]
    fn imdb_ids_get_their_prefix() {
        assert_eq!(
            film(r#"{"id":1,"title":"X","imdb":"113277"}"#)
                .imdb()
                .as_deref(),
            Some("tt0113277")
        );
        assert_eq!(
            film(r#"{"id":1,"title":"X","imdb":"tt0113277"}"#)
                .imdb()
                .as_deref(),
            Some("tt0113277")
        );
        assert_eq!(film(r#"{"id":1,"title":"X","imdb":""}"#).imdb(), None);
    }

    #[test]
    fn lists_hold_names_or_objects() {
        let f = film(
            r#"{"id":1,"title":"X","actors":[{"name":"A B"},"C D",{"name":"A B"}],"directors":["E F"]}"#,
        );
        let names: Vec<_> = f.credits().iter().map(|c| c.person.name.clone()).collect();
        assert_eq!(names, ["E F", "A B", "C D"]);
    }

    #[test]
    fn dutch_names_are_mapped() {
        let f = film(
            r#"{"id":1,"title":"X","genres":["Actie","Misdaad","Thriller"],"countries":["Verenigde Staten","Italië","Mars"]}"#,
        );
        assert_eq!(f.genres()[0], Genre::parse("Action").unwrap());
        assert_eq!(f.genres().len(), 3);
        assert_eq!(f.countries(), ["US", "IT", "Mars"]);
    }

    #[test]
    fn ratings_are_on_five_stars() {
        let item = film(r#"{"id":1,"title":"X","average":4.2,"votes_count":1234}"#).build_item();
        let rating = item.core.ratings.get(RATING_ID).unwrap();
        assert_eq!(
            (rating.value, rating.votes, rating.max_value),
            (4.2, 1234, 5)
        );
    }

    #[test]
    fn posters_prefer_the_largest() {
        let f = film(
            r#"{"id":1,"title":"X","posters":{"small":"https://img.example.test/s.jpg","large":"https://img.example.test/l.jpg"}}"#,
        );
        assert_eq!(
            f.poster().as_deref(),
            Some("https://img.example.test/l.jpg")
        );
        assert_eq!(film(r#"{"id":1,"title":"X"}"#).poster(), None);
    }

    #[test]
    fn search_answers_come_as_lists_or_objects() {
        let list: Value = serde_json::from_str(r#"[{"id":1,"title":"A"},{"id":2}]"#).unwrap();
        assert_eq!(search_films(&list).len(), 1);
        let object: Value = serde_json::from_str(r#"{"films":[{"id":1,"title":"A"}]}"#).unwrap();
        assert_eq!(search_films(&object).len(), 1);
        assert!(search_films(&Value::Null).is_empty());
    }
}
