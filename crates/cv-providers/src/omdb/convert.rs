//! OMDb answers to CineVault items, hits, episodes and ratings.
//!
//! OMDb sends every value as text and writes `N/A` for what it lacks, so each read goes through
//! [`Title::field`], which turns blank and `N/A` into nothing.

use cv_domain::{
    AiredStatus, Credit, CreditKind, EpisodeFields, EpisodeNumber, Genre, Item, ItemDetail,
    MediaFileType, ProviderIds, Rating, Ratings, id_key,
};
use serde_json::Value;

use super::PROVIDER_ID;
use crate::model::{MediaKind, SearchHit};
use crate::shared::{
    country_codes, imdb_title_id, language_codes, parse_date, parse_day_month_year, set_id,
    split_list, year_of, year_prefix,
};
use crate::tmdb::certification_token;

/// Rating source keys, as the ratings design names them (the IMDb and Rotten Tomatoes ones are
/// shared with the other providers that fill them).
pub const IMDB_RATING_ID: &str = "imdb";
pub const METACRITIC_RATING_ID: &str = "metacritic";
/// Rotten Tomatoes: critics' Tomatometer, a percentage.
pub const RT_CRITICS_RATING_ID: &str = "tomatometerallcritics";
/// Rotten Tomatoes: audience score, a percentage.
pub const RT_AUDIENCE_RATING_ID: &str = "tomatometerallaudience";
/// Rotten Tomatoes: critics' average, out of 10.
pub const RT_CRITICS_AVERAGE_RATING_ID: &str = "tomatometeravgcritics";
/// Rotten Tomatoes: audience average, out of 5.
pub const RT_AUDIENCE_AVERAGE_RATING_ID: &str = "tomatometeravgaudience";

/// A title answer with an IMDb ID and a title.
pub(super) struct Title {
    value: Value,
    imdb: String,
    title: String,
}

impl Title {
    /// `None` when the answer is not a title (no IMDb ID or no title).
    pub(super) fn from_value(value: &Value) -> Option<Self> {
        let mut title = Self {
            value: value.clone(),
            imdb: String::new(),
            title: String::new(),
        };
        title.imdb = imdb_title_id(title.field("imdbID")?)?;
        title.title = title.field("Title")?.to_owned();
        Some(title)
    }

    /// A text value; blank and `N/A` are nothing.
    fn field(&self, name: &str) -> Option<&str> {
        text(&self.value, name)
    }

    /// Whether this title can be returned for a request of `kind`. Episodes and games fit
    /// neither; a title without a type fits both.
    pub(super) fn fits(&self, kind: MediaKind) -> bool {
        match self.field("Type").map(str::to_ascii_lowercase).as_deref() {
            None => true,
            Some("movie") => kind == MediaKind::Movie,
            Some("series") => kind == MediaKind::Show,
            Some(_) => false,
        }
    }

    pub(super) fn total_seasons(&self) -> u32 {
        self.field("totalSeasons")
            .and_then(|n| n.parse().ok())
            .unwrap_or(0)
    }

    fn year(&self) -> u16 {
        self.field("Year").map_or(0, year_prefix)
    }

    fn poster(&self) -> Option<String> {
        self.field("Poster")
            .filter(|p| p.starts_with("http"))
            .map(str::to_owned)
    }

    fn ids(&self) -> ProviderIds {
        let mut ids = ProviderIds::new();
        set_id(&mut ids, id_key::IMDB, &self.imdb);
        ids
    }

    pub(super) fn build_hit(&self) -> SearchHit {
        SearchHit {
            provider: PROVIDER_ID.to_owned(),
            ids: self.ids(),
            title: self.title.clone(),
            original_title: String::new(),
            year: self.year(),
            overview: self.field("Plot").unwrap_or("").to_owned(),
            poster_url: self.poster(),
            exact_id: true,
            popularity: 0.0,
            votes: 0,
        }
    }

    /// Minutes from `170 min` (or `2 h 5 min`).
    fn runtime_minutes(&self) -> u32 {
        let Some(text) = self.field("Runtime") else {
            return 0;
        };
        let words: Vec<&str> = text.split_whitespace().collect();
        let mut minutes = 0u32;
        for (index, word) in words.iter().enumerate() {
            let digits: String = word.chars().take_while(char::is_ascii_digit).collect();
            let Ok(value) = digits.parse::<u32>() else {
                continue;
            };
            // The unit is the rest of the word (`2h`) or the next word (`2 h`).
            let attached = &word[digits.len()..];
            let unit = if attached.is_empty() {
                words.get(index + 1).copied().unwrap_or("")
            } else {
                attached
            };
            let hours = unit.to_ascii_lowercase().starts_with('h');
            minutes = minutes.saturating_add(if hours { value * 60 } else { value });
        }
        minutes
    }

    fn genres(&self) -> Vec<Genre> {
        let mut out: Vec<Genre> = Vec::new();
        for name in split_list(self.field("Genre").unwrap_or("")) {
            if let Some(genre) = Genre::parse(&name)
                && !out.contains(&genre)
            {
                out.push(genre);
            }
        }
        out
    }

    fn credits(&self) -> Vec<Credit> {
        let mut out: Vec<Credit> = Vec::new();
        for (field, kind) in [
            ("Director", CreditKind::Director),
            ("Writer", CreditKind::Writer),
            ("Actors", CreditKind::Actor),
        ] {
            for name in split_list(self.field(field).unwrap_or("")) {
                // Writers come as `Name (screenplay)`.
                let name = name.split('(').next().unwrap_or("").trim().to_owned();
                if name.is_empty() {
                    continue;
                }
                let duplicate = out.iter().any(|c| c.kind == kind && c.person.name == name);
                if !duplicate {
                    out.push(Credit::new(kind, name, ""));
                }
            }
        }
        out
    }

    /// The rating on an entry of the `Ratings` list, such as `Rotten Tomatoes`.
    fn listed(&self, source: &str) -> Option<&str> {
        self.value
            .get("Ratings")?
            .as_array()?
            .iter()
            .find(|r| text(r, "Source").is_some_and(|s| s.eq_ignore_ascii_case(source)))
            .and_then(|r| text(r, "Value"))
    }

    pub(super) fn ratings(&self) -> Ratings {
        let mut out = Ratings::new();
        if let Some(value) = self.number("imdbRating").filter(|v| *v > 0.0) {
            let votes = self.votes("imdbVotes");
            out.insert(Rating::new(IMDB_RATING_ID, value, votes, 10));
        }
        let metacritic = self.number("Metascore").or_else(|| {
            self.listed("Metacritic")
                .and_then(|v| v.split('/').next())
                .and_then(|v| v.trim().parse().ok())
        });
        if let Some(value) = metacritic {
            out.insert(Rating::new(METACRITIC_RATING_ID, value, 0, 100));
        }
        let critics = self.number("tomatoMeter").or_else(|| {
            self.listed("Rotten Tomatoes")
                .and_then(|v| v.trim_end_matches('%').trim().parse().ok())
        });
        // The Rotten Tomatoes page of the title (`tomatoURL`), for both of its meters.
        let tomato_page = self.field("tomatoURL").unwrap_or_default();
        if let Some(value) = critics {
            out.insert(
                Rating::new(
                    RT_CRITICS_RATING_ID,
                    value,
                    self.votes("tomatoReviews"),
                    100,
                )
                .with_url(tomato_page),
            );
        }
        for (field, id, max) in [
            ("tomatoUserMeter", RT_AUDIENCE_RATING_ID, 100),
            ("tomatoRating", RT_CRITICS_AVERAGE_RATING_ID, 10),
            ("tomatoUserRating", RT_AUDIENCE_AVERAGE_RATING_ID, 5),
        ] {
            if let Some(value) = self.number(field) {
                let votes = if field == "tomatoUserMeter" {
                    self.votes("tomatoUserReviews")
                } else {
                    0
                };
                let page = if field == "tomatoUserMeter" {
                    tomato_page
                } else {
                    ""
                };
                out.insert(Rating::new(id, value, votes, max).with_url(page));
            }
        }
        out
    }

    fn number(&self, name: &str) -> Option<f32> {
        self.field(name)?
            .replace(',', "")
            .parse::<f32>()
            .ok()
            .filter(|v| *v >= 0.0)
    }

    fn votes(&self, name: &str) -> u32 {
        self.field(name)
            .map(|v| v.replace(',', ""))
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    }

    pub(super) fn build_item(&self, kind: MediaKind) -> Item {
        let mut item = match kind {
            MediaKind::Movie => Item::movie(&self.title),
            MediaKind::Show => Item::show(&self.title),
        };
        let core = &mut item.core;
        core.ids = self.ids();
        core.year = self.year();
        core.plot = self.field("Plot").unwrap_or("").to_owned();
        core.production_companies = split_list(self.field("Production").unwrap_or(""));
        core.credits = self.credits();
        core.ratings = self.ratings();
        if let Some(poster) = self.poster() {
            core.artwork_urls.insert(MediaFileType::Poster, poster);
        }
        let release = self.field("Released").and_then(parse_day_month_year);
        let certification = self
            .field("Rated")
            .map(|rated| certification_token("US", rated))
            .unwrap_or_default();
        let countries = country_codes(&split_list(self.field("Country").unwrap_or("")));
        match &mut item.detail {
            ItemDetail::Movie(movie) => {
                // OMDb answers in English whatever the request language is.
                movie.english_title = self.title.clone();
                movie.runtime_minutes = self.runtime_minutes();
                movie.release_date = release;
                movie.certification = certification;
                movie.genres = self.genres();
                movie.spoken_languages =
                    language_codes(&split_list(self.field("Language").unwrap_or("")));
                movie.countries = countries;
            }
            ItemDetail::Show(show) => {
                show.english_title = self.title.clone();
                show.runtime_minutes = self.runtime_minutes();
                show.first_aired = release;
                show.certification = certification;
                show.genres = self.genres();
                show.country = countries.into_iter().next().unwrap_or_default();
                self.set_run(show);
            }
            _ => {}
        }
        item
    }

    /// Year range `2008–2013` (ended) or `2008–` (still running).
    fn set_run(&self, show: &mut cv_domain::ShowFields) {
        let Some(range) = self.field("Year") else {
            return;
        };
        let mut parts = range.splitn(2, ['–', '-', '—']);
        let _start = parts.next();
        match parts.next().map(str::trim) {
            Some("") => show.status = AiredStatus::Continuing,
            Some(end) => {
                show.year_end = year_prefix(end);
                show.status = AiredStatus::Ended;
            }
            None => {}
        }
    }
}

fn text<'a>(value: &'a Value, name: &str) -> Option<&'a str> {
    let text = value.get(name)?.as_str()?.trim();
    (!text.is_empty() && !text.eq_ignore_ascii_case("n/a")).then_some(text)
}

/// The hits of a `s=` answer for `kind`. Entries without an IMDb ID or title are skipped.
pub(super) fn hit_from_search(value: &Value, kind: MediaKind) -> Vec<SearchHit> {
    let Some(entries) = value.get("Search").and_then(Value::as_array) else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| {
            let imdb = imdb_title_id(text(entry, "imdbID")?)?;
            let title = text(entry, "Title")?;
            let fits = match text(entry, "Type").map(str::to_ascii_lowercase).as_deref() {
                None => true,
                Some("movie") => kind == MediaKind::Movie,
                Some("series") => kind == MediaKind::Show,
                Some(_) => false,
            };
            if !fits {
                return None;
            }
            let mut ids = ProviderIds::new();
            ids.set(id_key::IMDB, &imdb);
            Some(SearchHit {
                provider: PROVIDER_ID.to_owned(),
                ids,
                title: title.to_owned(),
                original_title: String::new(),
                year: text(entry, "Year").map_or(0, year_prefix),
                overview: String::new(),
                poster_url: text(entry, "Poster")
                    .filter(|p| p.starts_with("http"))
                    .map(str::to_owned),
                exact_id: false,
                popularity: 0.0,
                votes: 0,
            })
        })
        .collect()
}

/// The episodes of a `Season=` answer, numbered in the aired order. `season` is the number
/// that was asked for.
pub(super) fn parse_episodes(value: &Value, season: u32) -> Vec<Item> {
    let Some(entries) = value.get("Episodes").and_then(Value::as_array) else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| {
            let number: u32 = text(entry, "Episode")?.parse().ok()?;
            let mut item = Item::new(ItemDetail::Episode(EpisodeFields::default()));
            item.core.title = text(entry, "Title").unwrap_or("").to_owned();
            if let Some(imdb) = text(entry, "imdbID").and_then(imdb_title_id) {
                item.core.ids.set(id_key::IMDB, imdb);
            }
            let aired = text(entry, "Released")
                .and_then(|d| parse_date(d).or_else(|| parse_day_month_year(d)));
            item.core.year = year_of(aired);
            if let Some(value) = text(entry, "imdbRating").and_then(|v| v.parse::<f32>().ok())
                && value > 0.0
            {
                item.core
                    .ratings
                    .insert(Rating::new(IMDB_RATING_ID, value, 0, 10));
            }
            if let ItemDetail::Episode(fields) = &mut item.detail {
                fields.first_aired = aired;
                fields.numbers.set(EpisodeNumber::aired(season, number));
            }
            Some(item)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn title(json: &str) -> Title {
        Title::from_value(&serde_json::from_str(json).unwrap()).unwrap()
    }

    #[test]
    fn a_title_needs_an_imdb_id_and_a_name() {
        let v = |s: &str| serde_json::from_str::<Value>(s).unwrap();
        assert!(Title::from_value(&v(r#"{"Title":"Heat"}"#)).is_none());
        assert!(Title::from_value(&v(r#"{"imdbID":"tt0113277"}"#)).is_none());
        assert!(Title::from_value(&v(r#"{"imdbID":"tt0113277","Title":"N/A"}"#)).is_none());
        assert!(Title::from_value(&v(r#"{"imdbID":"tt0113277","Title":"Heat"}"#)).is_some());
    }

    #[test]
    fn runtimes_in_minutes_and_hours() {
        let run = |text: &str| {
            title(&format!(
                r#"{{"imdbID":"tt0113277","Title":"X","Runtime":"{text}"}}"#
            ))
            .runtime_minutes()
        };
        assert_eq!(run("170 min"), 170);
        assert_eq!(run("2 h 5 min"), 125);
        assert_eq!(run("N/A"), 0);
    }

    #[test]
    fn missing_values_leave_fields_empty() {
        let item = title(r#"{"imdbID":"tt0113277","Title":"Heat","Year":"N/A","Plot":"N/A","Poster":"N/A","Genre":"N/A"}"#)
            .build_item(MediaKind::Movie);
        assert_eq!(item.core.title, "Heat");
        assert_eq!(item.core.year, 0);
        assert!(item.core.plot.is_empty());
        assert!(item.core.artwork_urls.is_empty());
        assert!(item.core.ratings.is_empty());
    }

    #[test]
    fn ratings_read_both_the_list_and_the_tomato_fields() {
        let from_list = title(
            r#"{"imdbID":"tt0113277","Title":"Heat","imdbRating":"8.3","imdbVotes":"700,123",
                "Ratings":[{"Source":"Internet Movie Database","Value":"8.3/10"},
                           {"Source":"Rotten Tomatoes","Value":"87%"},
                           {"Source":"Metacritic","Value":"76/100"}]}"#,
        )
        .ratings();
        assert_eq!(from_list.get("imdb").unwrap().votes, 700_123);
        assert_eq!(from_list.get(RT_CRITICS_RATING_ID).unwrap().value, 87.0);
        assert_eq!(from_list.get(METACRITIC_RATING_ID).unwrap().max_value, 100);

        let tomatoes = title(
            r#"{"imdbID":"tt0113277","Title":"Heat","tomatoMeter":"88","tomatoRating":"8.1",
                "tomatoUserMeter":"92","tomatoUserRating":"4.2","tomatoReviews":"70",
                "tomatoUserReviews":"1000000"}"#,
        )
        .ratings();
        assert_eq!(tomatoes.get(RT_CRITICS_RATING_ID).unwrap().votes, 70);
        assert_eq!(
            tomatoes.get(RT_AUDIENCE_RATING_ID).unwrap().votes,
            1_000_000
        );
        assert_eq!(
            tomatoes
                .get(RT_AUDIENCE_AVERAGE_RATING_ID)
                .unwrap()
                .max_value,
            5
        );
        assert_eq!(
            tomatoes.get(RT_CRITICS_AVERAGE_RATING_ID).unwrap().value,
            8.1
        );
    }

    #[test]
    fn the_tomato_page_goes_with_both_rotten_tomatoes_meters() {
        let r = title(
            r#"{"imdbID":"tt0083658","Title":"Blade Runner","tomatoMeter":"89",
                "tomatoUserMeter":"91","tomatoRating":"8.4",
                "tomatoURL":"https://www.rottentomatoes.com/m/blade_runner/"}"#,
        )
        .ratings();
        let page = "https://www.rottentomatoes.com/m/blade_runner/";
        assert_eq!(r.get(RT_CRITICS_RATING_ID).unwrap().url, page);
        assert_eq!(r.get(RT_AUDIENCE_RATING_ID).unwrap().url, page);
        assert_eq!(r.get(RT_CRITICS_AVERAGE_RATING_ID).unwrap().url, "");
        let none =
            title(r#"{"imdbID":"tt0083658","Title":"X","tomatoMeter":"89","tomatoURL":"N/A"}"#)
                .ratings();
        assert_eq!(none.get(RT_CRITICS_RATING_ID).unwrap().url, "");
    }

    #[test]
    fn show_year_ranges_set_the_run() {
        let ended =
            title(r#"{"imdbID":"tt0903747","Title":"BB","Type":"series","Year":"2008–2013"}"#)
                .build_item(MediaKind::Show);
        let ItemDetail::Show(show) = &ended.detail else {
            panic!("show");
        };
        assert_eq!(show.status, AiredStatus::Ended);
        assert_eq!(show.year_end, 2013);
        let running =
            title(r#"{"imdbID":"tt0903747","Title":"BB","Type":"series","Year":"2008–"}"#)
                .build_item(MediaKind::Show);
        let ItemDetail::Show(show) = &running.detail else {
            panic!("show");
        };
        assert_eq!(show.status, AiredStatus::Continuing);
        assert_eq!(running.core.year, 2008);
    }

    #[test]
    fn kinds_gate_titles() {
        let movie = title(r#"{"imdbID":"tt0113277","Title":"Heat","Type":"movie"}"#);
        assert!(movie.fits(MediaKind::Movie));
        assert!(!movie.fits(MediaKind::Show));
        let episode = title(r#"{"imdbID":"tt0113277","Title":"Pilot","Type":"episode"}"#);
        assert!(!episode.fits(MediaKind::Movie) && !episode.fits(MediaKind::Show));
    }

    #[test]
    fn writers_lose_their_credit_notes() {
        let credits =
            title(r#"{"imdbID":"tt0113277","Title":"X","Writer":"A B (screenplay), C D (story)","Director":"E F","Actors":"G H, I J"}"#)
                .credits();
        let names: Vec<_> = credits.iter().map(|c| c.person.name.as_str()).collect();
        assert_eq!(names, ["E F", "A B", "C D", "G H", "I J"]);
    }
}
