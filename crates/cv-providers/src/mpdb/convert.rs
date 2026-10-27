//! MPDb.tv answers to CineVault items, hits and artwork.
//!
//! MPDb.tv publishes no schema and its answers are French-language data in JSON. The reader is
//! lenient: a value may be text or a number, a list may hold names or `{"name": ...}` objects, a
//! field may go by a few names (`title`/`titre`, `plot`/`synopsis`/`overview`, ...), and a missing
//! value is simply empty. A movie needs an ID and a title; everything else is optional. The
//! opt-in live test confirms the names against the real service.

use cv_domain::{
    Credit, CreditKind, Genre, Item, ItemDetail, MediaFileType, ProviderIds, Rating, id_key,
};
use serde_json::Value;

use super::{ID_KEY, PROVIDER_ID};
use crate::model::{ArtworkCandidate, SearchHit};
use crate::shared::{country_codes, imdb_title_id, parse_date, set_id, year_of, year_prefix};

/// The rating source key MPDb.tv ratings are stored under.
pub const RATING_ID: &str = "mpdb";

/// French genre names to the names CineVault's genres use. Others pass through unchanged.
const GENRES: &[(&str, &str)] = &[
    ("action", "Action"),
    ("aventure", "Adventure"),
    ("animation", "Animation"),
    ("comédie", "Comedy"),
    ("comedie", "Comedy"),
    ("crime", "Crime"),
    ("policier", "Crime"),
    ("documentaire", "Documentary"),
    ("drame", "Drama"),
    ("famille", "Family"),
    ("fantastique", "Fantasy"),
    ("histoire", "History"),
    ("historique", "History"),
    ("horreur", "Horror"),
    ("musique", "Music"),
    ("musical", "Musical"),
    ("mystère", "Mystery"),
    ("romance", "Romance"),
    ("science-fiction", "Science Fiction"),
    ("thriller", "Thriller"),
    ("guerre", "War"),
    ("biopic", "Biography"),
    ("biographie", "Biography"),
];

/// French country names to the two-letter codes CineVault stores.
const COUNTRIES: &[(&str, &str)] = &[
    ("états-unis", "US"),
    ("etats-unis", "US"),
    ("france", "FR"),
    ("royaume-uni", "GB"),
    ("allemagne", "DE"),
    ("italie", "IT"),
    ("espagne", "ES"),
    ("canada", "CA"),
    ("japon", "JP"),
    ("belgique", "BE"),
    ("suisse", "CH"),
    ("luxembourg", "LU"),
    ("chine", "CN"),
    ("corée du sud", "KR"),
    ("inde", "IN"),
    ("australie", "AU"),
    ("irlande", "IE"),
    ("russie", "RU"),
];

fn text<'a>(value: &'a Value, names: &[&str]) -> Option<&'a str> {
    names.iter().find_map(|name| {
        let text = value.get(*name)?.as_str()?.trim();
        (!text.is_empty()).then_some(text)
    })
}

fn number(value: &Value, names: &[&str]) -> Option<f64> {
    names.iter().find_map(|name| match value.get(*name)? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().replace(',', ".").parse().ok(),
        _ => None,
    })
}

fn id_text(value: &Value, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| match value.get(*name)? {
        Value::Number(n) => Some(n.to_string()),
        Value::String(s) => Some(s.trim().to_owned()).filter(|s| !s.is_empty()),
        _ => None,
    })
}

/// A list of names: strings, or objects with a `name`, `nom` or `title`.
fn names(value: &Value, fields: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for field in fields {
        let Some(list) = value.get(*field).and_then(Value::as_array) else {
            continue;
        };
        for entry in list {
            let name = match entry {
                Value::String(s) => Some(s.trim()),
                other => text(other, &["name", "nom", "title"]),
            };
            if let Some(name) = name.filter(|n| !n.is_empty())
                && !out.iter().any(|o| o == name)
            {
                out.push(name.to_owned());
            }
        }
        if !out.is_empty() {
            break;
        }
    }
    out
}

/// An MPDb.tv movie: a search result or a detail answer.
pub(super) struct Movie {
    value: Value,
    id: String,
    title: String,
}

impl Movie {
    /// `None` when the entry has no ID or no title.
    pub(super) fn from_value(value: &Value) -> Option<Self> {
        let id = id_text(value, &["id", "mpdb_id"])?;
        if id == "0" {
            return None;
        }
        let title = text(value, &["title", "titre", "name"])?.to_owned();
        Some(Self {
            value: value.clone(),
            id,
            title,
        })
    }

    fn imdb(&self) -> Option<String> {
        let raw = id_text(&self.value, &["imdb_id", "imdb", "imdbId"])?;
        if raw.bytes().all(|b| b.is_ascii_digit()) {
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
        if let Some(tmdb) = id_text(&self.value, &["tmdb_id", "tmdb", "tmdbId"]) {
            set_id(&mut ids, id_key::TMDB, tmdb);
        }
        ids
    }

    fn release_date(&self) -> Option<cv_domain::CalendarDate> {
        text(&self.value, &["release_date", "date_sortie", "sortie"]).and_then(parse_date)
    }

    fn year(&self) -> u16 {
        number(&self.value, &["year", "annee", "année"])
            .map(|y| y as u16)
            .filter(|y| *y != 0)
            .or_else(|| {
                text(&self.value, &["year", "annee"])
                    .map(year_prefix)
                    .filter(|y| *y != 0)
            })
            .unwrap_or_else(|| year_of(self.release_date()))
    }

    fn original_title(&self) -> String {
        text(
            &self.value,
            &["original_title", "titre_original", "originalTitle"],
        )
        .unwrap_or(&self.title)
        .to_owned()
    }

    fn plot(&self) -> String {
        text(&self.value, &["plot", "synopsis", "overview", "resume"])
            .unwrap_or("")
            .to_owned()
    }

    fn poster(&self) -> Option<String> {
        images(&self.value, &["posters", "poster"])
            .into_iter()
            .next()
            .map(|i| i.url)
            .or_else(|| text(&self.value, &["poster_url", "poster"]).map(str::to_owned))
            .filter(|u| u.starts_with("http"))
    }

    pub(super) fn build_hit(&self, exact_id: bool) -> SearchHit {
        SearchHit {
            provider: PROVIDER_ID.to_owned(),
            ids: self.ids(),
            title: self.title.clone(),
            original_title: self.original_title(),
            year: self.year(),
            overview: self.plot(),
            poster_url: self.poster(),
            exact_id,
            popularity: 0.0,
            votes: 0,
        }
    }

    fn genres(&self) -> Vec<Genre> {
        let mut out: Vec<Genre> = Vec::new();
        for name in names(&self.value, &["genres", "genre"]) {
            let lower = name.to_lowercase();
            let english = GENRES
                .iter()
                .find(|(french, _)| *french == lower)
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
        let mapped: Vec<String> = names(&self.value, &["countries", "pays"])
            .into_iter()
            .map(|name| {
                let lower = name.to_lowercase();
                COUNTRIES
                    .iter()
                    .find(|(french, _)| *french == lower)
                    .map_or(name, |(_, code)| (*code).to_owned())
            })
            .collect();
        country_codes(&mapped)
    }

    fn credits(&self) -> Vec<Credit> {
        let mut out: Vec<Credit> = Vec::new();
        for (fields, kind) in [
            (
                &["directors", "realisateurs", "director"][..],
                CreditKind::Director,
            ),
            (&["writers", "scenaristes"][..], CreditKind::Writer),
            (&["actors", "acteurs", "cast"][..], CreditKind::Actor),
        ] {
            for field in fields {
                let Some(list) = self.value.get(*field).and_then(Value::as_array) else {
                    continue;
                };
                for entry in list {
                    let (name, role) = match entry {
                        Value::String(s) => (s.trim(), ""),
                        other => (
                            text(other, &["name", "nom"]).unwrap_or(""),
                            text(other, &["role", "character", "personnage"]).unwrap_or(""),
                        ),
                    };
                    if !name.is_empty()
                        && !out.iter().any(|c| c.kind == kind && c.person.name == name)
                    {
                        out.push(Credit::new(kind, name, role));
                    }
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
        core.plot = self.plot();
        core.credits = self.credits();
        if let Some(value) =
            number(&self.value, &["rating", "note", "average"]).filter(|v| *v > 0.0)
        {
            let votes = number(&self.value, &["votes", "votes_count", "nb_votes"]).unwrap_or(0.0);
            let max = number(&self.value, &["rating_max", "max_rating", "note_max"])
                .filter(|m| *m > 0.0)
                .unwrap_or(10.0);
            core.ratings.insert(Rating::new(
                RATING_ID,
                value as f32,
                votes as u32,
                max as u32,
            ));
        }
        for (names, kind) in ARTWORK_FIELDS {
            if let Some(first) = images(&self.value, names).into_iter().next() {
                core.artwork_urls.insert(*kind, first.url);
            }
        }
        let extra_fanarts: Vec<String> = images(&self.value, &["fanarts", "fanart", "backgrounds"])
            .into_iter()
            .skip(1)
            .map(|i| i.url)
            .collect();
        if !core.artwork_urls.contains_key(&MediaFileType::Poster)
            && let Some(poster) = self.poster()
        {
            core.artwork_urls.insert(MediaFileType::Poster, poster);
        }
        let release_date = self.release_date();
        if let ItemDetail::Movie(movie) = &mut item.detail {
            movie.tagline = text(&self.value, &["tagline", "accroche"])
                .unwrap_or("")
                .to_owned();
            movie.runtime_minutes = number(&self.value, &["runtime", "duration", "duree", "durée"])
                .map_or(0, |m| m as u32);
            movie.genres = self.genres();
            movie.countries = self.countries();
            movie.release_date = release_date;
            movie.extra_fanarts = extra_fanarts;
        }
        item
    }

    /// Every image the movie offers, best first within each type.
    pub(super) fn artwork(&self) -> Vec<ArtworkCandidate> {
        let mut out = Vec::new();
        for (names, kind) in ARTWORK_FIELDS {
            for image in images(&self.value, names) {
                out.push(ArtworkCandidate {
                    kind: *kind,
                    url: image.url,
                    language: image.language,
                    width: image.width,
                    height: image.height,
                    score: image.score,
                    popularity: crate::model::Popularity::Unknown,
                });
            }
        }
        out
    }
}

/// The artwork types MPDb.tv offers and the names of the lists that hold them.
const ARTWORK_FIELDS: &[(&[&str], MediaFileType)] = &[
    (&["posters", "poster"], MediaFileType::Poster),
    (&["fanarts", "fanart", "backgrounds"], MediaFileType::Fanart),
    (&["discs", "disc", "cdart"], MediaFileType::Disc),
    (
        &["clearart", "cleararts", "hdclearart"],
        MediaFileType::Clearart,
    ),
    (
        &["clearlogo", "clearlogos", "hdclearlogo", "logos"],
        MediaFileType::Clearlogo,
    ),
    (&["banners", "banner"], MediaFileType::Banner),
];

struct Image {
    url: String,
    language: Option<String>,
    width: u32,
    height: u32,
    score: f32,
}

/// The images in the first of `fields` that holds a list: each entry is an address or an object
/// with `url` (or `file`, `src`), `lang`, `width`, `height` and a vote count.
fn images(value: &Value, fields: &[&str]) -> Vec<Image> {
    for field in fields {
        let Some(list) = value.get(*field).and_then(Value::as_array) else {
            continue;
        };
        let found: Vec<Image> = list
            .iter()
            .filter_map(|entry| match entry {
                Value::String(s) => Some(Image {
                    url: s.trim().to_owned(),
                    language: None,
                    width: 0,
                    height: 0,
                    score: 0.0,
                }),
                other => Some(Image {
                    url: text(other, &["url", "file", "src"])?.to_owned(),
                    language: text(other, &["lang", "language", "langue"])
                        .filter(|l| !l.eq_ignore_ascii_case("xx") && *l != "00")
                        .map(str::to_owned),
                    width: number(other, &["width", "largeur"]).map_or(0, |w| w as u32),
                    height: number(other, &["height", "hauteur"]).map_or(0, |h| h as u32),
                    score: number(other, &["score", "likes", "votes"]).map_or(0.0, |s| s as f32),
                }),
            })
            .filter(|i| i.url.starts_with("http"))
            .collect();
        if !found.is_empty() {
            return found;
        }
    }
    Vec::new()
}

/// The movies of a search answer: a list, or an object holding one under `results`, `movies`,
/// `films` or `data`.
pub(super) fn search_movies(value: &Value) -> Vec<Movie> {
    let list = value.as_array().or_else(|| {
        ["results", "movies", "films", "data", "items"]
            .iter()
            .find_map(|key| value.get(*key).and_then(Value::as_array))
    });
    list.map(|l| l.iter().filter_map(Movie::from_value).collect())
        .unwrap_or_default()
}

/// The movie of a detail answer: the object itself, or one under `movie`, `film` or `data`.
pub(super) fn detail_movie(value: &Value) -> Option<Movie> {
    Movie::from_value(value).or_else(|| {
        ["movie", "film", "data"]
            .iter()
            .find_map(|key| value.get(*key).and_then(Movie::from_value))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn movie(json: &str) -> Movie {
        Movie::from_value(&serde_json::from_str(json).unwrap()).unwrap()
    }

    #[test]
    fn a_movie_needs_an_id_and_a_title() {
        let v = |s: &str| serde_json::from_str::<Value>(s).unwrap();
        assert!(Movie::from_value(&v(r#"{"title":"Heat"}"#)).is_none());
        assert!(Movie::from_value(&v(r#"{"id":0,"title":"Heat"}"#)).is_none());
        assert!(Movie::from_value(&v(r#"{"id":1}"#)).is_none());
        assert!(Movie::from_value(&v(r#"{"id":"7","titre":"Heat"}"#)).is_some());
    }

    #[test]
    fn imdb_ids_get_their_prefix() {
        let imdb = |json: &str| movie(json).imdb();
        assert_eq!(
            imdb(r#"{"id":1,"title":"X","imdb_id":"113277"}"#).as_deref(),
            Some("tt0113277")
        );
        assert_eq!(
            imdb(r#"{"id":1,"title":"X","imdb":"tt0113277"}"#).as_deref(),
            Some("tt0113277")
        );
        assert_eq!(imdb(r#"{"id":1,"title":"X","imdb_id":""}"#), None);
    }

    #[test]
    fn french_names_are_mapped() {
        let m = movie(
            r#"{"id":1,"title":"X","genres":["Policier","Drame","Thriller"],"pays":["États-Unis","France","Mars"]}"#,
        );
        assert_eq!(m.genres()[0], Genre::parse("Crime").unwrap());
        assert_eq!(m.genres().len(), 3);
        assert_eq!(m.countries(), ["US", "FR", "Mars"]);
    }

    #[test]
    fn images_come_as_addresses_or_objects() {
        let m = movie(
            r#"{"id":1,"title":"X","posters":["https://i.example.test/a.jpg",{"url":"https://i.example.test/b.jpg","lang":"fr","width":1000,"height":1500,"likes":7},{"url":"relative.jpg"}],
                "fanarts":[{"file":"https://i.example.test/f.jpg","lang":"xx"}]}"#,
        );
        let art = m.artwork();
        assert_eq!(art.len(), 3);
        assert_eq!(art[1].language.as_deref(), Some("fr"));
        assert_eq!((art[1].width, art[1].height), (1000, 1500));
        assert!((art[1].score - 7.0).abs() < 1e-6);
        assert_eq!(art[2].kind, MediaFileType::Fanart);
        assert_eq!(art[2].language, None);
    }

    #[test]
    fn answers_come_as_lists_or_objects() {
        let list: Value = serde_json::from_str(r#"[{"id":1,"title":"A"},{"id":2}]"#).unwrap();
        assert_eq!(search_movies(&list).len(), 1);
        let object: Value = serde_json::from_str(r#"{"results":[{"id":1,"title":"A"}]}"#).unwrap();
        assert_eq!(search_movies(&object).len(), 1);
        assert!(search_movies(&Value::Null).is_empty());
        let wrapped: Value = serde_json::from_str(r#"{"data":{"id":3,"title":"C"}}"#).unwrap();
        assert!(detail_movie(&wrapped).is_some());
    }
}
