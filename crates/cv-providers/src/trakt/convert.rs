//! Trakt answers to CineVault items, hits and episodes.

use cv_domain::{
    AiredStatus, Credit, CreditKind, EpisodeFields, EpisodeGroup, EpisodeNumber, Genre, Item,
    ItemDetail, ProviderIds, Rating, Trailer, id_key,
};

use super::PROVIDER_ID;
use super::api::{Episode, Ids, People, PersonRef, Summary, Text, Translation};
use crate::model::{MediaKind, RequestOptions, SearchHit};
use crate::shared::{country_codes, imdb_title_id, language_codes, parse_date, set_id, year_of};
use crate::tmdb::certification_token;

/// The rating source key Trakt ratings are stored under.
pub const RATING_ID: &str = "trakt";

/// The IDs Trakt knows, as provider IDs.
pub(super) fn provider_ids(ids: &Ids) -> ProviderIds {
    let mut out = ProviderIds::new();
    if let Some(id) = ids.trakt {
        set_id(&mut out, id_key::TRAKT, id.to_string());
    }
    if let Some(imdb) = ids.imdb.as_deref().and_then(imdb_title_id) {
        set_id(&mut out, id_key::IMDB, imdb);
    }
    if let Some(id) = ids.tmdb {
        set_id(&mut out, id_key::TMDB, id.to_string());
    }
    if let Some(id) = ids.tvdb {
        set_id(&mut out, id_key::TVDB, id.to_string());
    }
    out
}

/// The IDs of a person: like [`provider_ids`], but the IMDb ID is a name ID (`nm...`).
fn person_ids(ids: &Ids) -> ProviderIds {
    let mut out = provider_ids(ids);
    if let Some(imdb) = ids.imdb.as_deref().map(str::trim)
        && imdb.to_ascii_lowercase().starts_with("nm")
    {
        set_id(&mut out, id_key::IMDB, imdb.to_ascii_lowercase());
    }
    out
}

/// The rating of a summary: a positive value out of 10 with its votes.
pub(super) fn rating_of(summary: &Summary) -> Option<Rating> {
    let value = summary.rating.filter(|v| *v > 0.0)? as f32;
    let votes = u32::try_from(summary.votes.unwrap_or(0)).unwrap_or(u32::MAX);
    Some(Rating::new(RATING_ID, value, votes, 10))
}

fn show_status(status: &str) -> AiredStatus {
    match status.trim().to_ascii_lowercase().as_str() {
        "returning series" | "in production" | "planned" | "upcoming" | "pilot" => {
            AiredStatus::Continuing
        }
        "ended" => AiredStatus::Ended,
        "canceled" | "cancelled" => AiredStatus::Canceled,
        _ => AiredStatus::Unknown,
    }
}

fn convert_genres(names: &[String]) -> Vec<Genre> {
    let mut out: Vec<Genre> = Vec::new();
    for name in names {
        if let Some(genre) = Genre::parse(&name.replace('-', " "))
            && !out.contains(&genre)
        {
            out.push(genre);
        }
    }
    out
}

/// The department of a crew list as a credit kind; `None` for departments CineVault has no
/// kind for (art, costume, visual effects...), which are left out.
fn crew_kind(department: &str, jobs: &[String]) -> Option<CreditKind> {
    let job = jobs.first().map_or("", |j| j.trim()).to_ascii_lowercase();
    match department {
        "directing" if job.contains("director") || job.is_empty() => Some(CreditKind::Director),
        "writing" => Some(CreditKind::Writer),
        "production" if job.contains("producer") => Some(CreditKind::Producer),
        "sound" if job.contains("compos") || job.contains("music") => Some(CreditKind::Composer),
        "editing" => Some(CreditKind::Editor),
        "camera" => Some(CreditKind::Camera),
        _ => None,
    }
}

fn person_credit(kind: CreditKind, person: &PersonRef, role: &str) -> Option<Credit> {
    let name = person.name.text();
    if name.is_empty() {
        return None;
    }
    let mut credit = Credit::new(kind, name, role);
    credit.person.ids = person_ids(&person.ids);
    Some(credit)
}

/// Cast first, then the crew kinds above, each person once per kind.
pub(super) fn credits(people: &People) -> Vec<Credit> {
    let mut out: Vec<Credit> = Vec::new();
    let mut add = |credit: Credit| {
        let same = |c: &Credit| {
            c.kind == credit.kind
                && match (
                    c.person.ids.get(id_key::TRAKT),
                    credit.person.ids.get(id_key::TRAKT),
                ) {
                    (Some(a), Some(b)) => a == b,
                    _ => c.person.name == credit.person.name,
                }
        };
        if !out.iter().any(same) {
            out.push(credit);
        }
    };
    for member in &people.cast {
        let role = member
            .characters
            .iter()
            .map(|c| c.trim())
            .filter(|c| !c.is_empty())
            .collect::<Vec<_>>()
            .join(", ");
        if let Some(credit) = person_credit(CreditKind::Actor, &member.person, &role) {
            add(credit);
        }
    }
    for (department, members) in &people.crew {
        for member in members {
            let Some(kind) = crew_kind(department, &member.jobs) else {
                continue;
            };
            let role = member.jobs.first().map_or("", |j| j.trim());
            if let Some(credit) = person_credit(kind, &member.person, role) {
                add(credit);
            }
        }
    }
    out
}

/// The translation to use: the one for the request country, else the first.
pub(super) fn pick_translation<'a>(
    translations: &'a [Translation],
    options: &RequestOptions,
) -> Option<&'a Translation> {
    translations
        .iter()
        .find(|t| {
            t.country
                .text()
                .eq_ignore_ascii_case(options.country.trim())
        })
        .or_else(|| translations.first())
}

fn trailer_of(url: &str) -> Option<Trailer> {
    let url = url.trim();
    if !url.starts_with("http") {
        return None;
    }
    // `https://youtube.com/watch?v=ID` carries the video ID.
    let id = url
        .split_once("v=")
        .map(|(_, rest)| rest.split('&').next().unwrap_or("").to_owned())
        .unwrap_or_default();
    Some(Trailer {
        id,
        name: "Trailer".to_owned(),
        url: url.to_owned(),
        provider: PROVIDER_ID.to_owned(),
        scraped_by: PROVIDER_ID.to_owned(),
        ..Trailer::default()
    })
}

fn non_empty<'a>(preferred: &'a str, fallback: &'a str) -> &'a str {
    if preferred.is_empty() {
        fallback
    } else {
        preferred
    }
}

/// The movie or show for a summary, with people and a translation when the caller has them.
pub(super) fn build_item(
    kind: MediaKind,
    summary: &Summary,
    people: Option<&People>,
    translation: Option<&Translation>,
) -> Item {
    let base_title = summary.title.text().to_owned();
    let title = non_empty(translation.map_or("", |t| t.title.text()), &base_title).to_owned();
    let mut item = match kind {
        MediaKind::Movie => Item::movie(&title),
        MediaKind::Show => Item::show(&title),
    };
    let plot = non_empty(
        translation.map_or("", |t| t.overview.text()),
        summary.overview.text(),
    )
    .to_owned();
    let tagline = non_empty(
        translation.map_or("", |t| t.tagline.text()),
        summary.tagline.text(),
    )
    .to_owned();
    let core = &mut item.core;
    core.ids = provider_ids(&summary.ids);
    core.original_title = base_title.clone();
    core.year = summary
        .year
        .and_then(|y| u16::try_from(y).ok())
        .unwrap_or(0);
    core.plot = plot;
    if let Some(rating) = rating_of(summary) {
        core.ratings.insert(rating);
    }
    if let Some(people) = people {
        core.credits = credits(people);
    }
    // Trakt's certifications are US ones whatever the request country is.
    let certification = certification_token("US", summary.certification.text());
    let genres = convert_genres(&summary.genres);
    let countries = country_codes(&[summary.country.text().to_owned()]);
    let trailer = summary.trailer.as_deref().and_then(trailer_of);
    match &mut item.detail {
        ItemDetail::Movie(movie) => {
            movie.english_title = base_title;
            movie.tagline = tagline;
            movie.runtime_minutes = summary.runtime.unwrap_or(0);
            movie.release_date = parse_date(summary.released.text());
            if core.year == 0 {
                core.year = year_of(movie.release_date);
            }
            movie.certification = certification;
            movie.genres = genres;
            movie.spoken_languages = language_codes(&[summary.language.text().to_owned()]);
            movie.countries = countries;
            movie.trailers = trailer.into_iter().collect();
        }
        ItemDetail::Show(show) => {
            show.english_title = base_title;
            show.tagline = tagline;
            show.runtime_minutes = summary.runtime.unwrap_or(0);
            show.first_aired = parse_date(summary.first_aired.text());
            if core.year == 0 {
                core.year = year_of(show.first_aired);
            }
            show.certification = certification;
            show.genres = genres;
            show.country = countries.into_iter().next().unwrap_or_default();
            show.status = show_status(summary.status.text());
            show.episode_group = EpisodeGroup::aired();
            show.episode_groups = vec![EpisodeGroup::aired()];
            show.trailers = trailer.into_iter().collect();
            let network = summary.network.text();
            if !network.is_empty() {
                core.production_companies = vec![network.to_owned()];
            }
        }
        _ => {}
    }
    item
}

/// The hit for a search or lookup result; `None` without a title or a Trakt ID.
pub(super) fn build_hit(summary: &Summary, exact_id: bool) -> Option<SearchHit> {
    let title = summary.title.text();
    let ids = provider_ids(&summary.ids);
    if title.is_empty() || !ids.contains(id_key::TRAKT) {
        return None;
    }
    let year = summary
        .year
        .and_then(|y| u16::try_from(y).ok())
        .unwrap_or(0);
    Some(SearchHit {
        provider: PROVIDER_ID.to_owned(),
        ids,
        title: title.to_owned(),
        original_title: title.to_owned(),
        year,
        overview: summary.overview.text().to_owned(),
        poster_url: None,
        exact_id,
        popularity: 0.0,
        votes: 0,
    })
}

/// An episode with its aired number, plus the absolute number when Trakt has one.
pub(super) fn episode_item(episode: &Episode, season: u32) -> Option<Item> {
    let season = episode.season.unwrap_or(season);
    let number = episode.number.filter(|n| *n > 0)?;
    let mut item = Item::new(ItemDetail::Episode(EpisodeFields::default()));
    item.core.title = episode.title.text().to_owned();
    item.core.plot = episode.overview.text().to_owned();
    item.core.ids = provider_ids(&episode.ids);
    let aired = parse_date(episode.first_aired.text());
    item.core.year = year_of(aired);
    if let Some(value) = episode.rating.filter(|v| *v > 0.0) {
        let votes = u32::try_from(episode.votes.unwrap_or(0)).unwrap_or(u32::MAX);
        item.core
            .ratings
            .insert(Rating::new(RATING_ID, value as f32, votes, 10));
    }
    if let ItemDetail::Episode(fields) = &mut item.detail {
        fields.first_aired = aired;
        fields.runtime_minutes = episode.runtime.unwrap_or(0);
        fields.numbers.set(EpisodeNumber::aired(season, number));
        if let Some(absolute) = episode.number_abs.filter(|n| *n > 0) {
            fields
                .numbers
                .set(EpisodeNumber::new(EpisodeGroup::absolute(), 1, absolute));
        }
    }
    Some(item)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(json: &str) -> Summary {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn a_bare_summary_converts_with_empty_fields() {
        let s = summary(r#"{"title":"Heat","ids":{"trakt":1,"imdb":"tt0113277"}}"#);
        let item = build_item(MediaKind::Movie, &s, None, None);
        assert_eq!(item.core.title, "Heat");
        assert_eq!(item.core.ids.get(id_key::TRAKT), Some("1"));
        assert_eq!(item.core.year, 0);
        assert!(item.core.ratings.is_empty());
        assert!(item.core.credits.is_empty());
    }

    #[test]
    fn nulls_do_not_fail_a_summary() {
        let s = summary(r#"{"title":"Heat","genres":null,"ids":null,"year":null,"rating":null}"#);
        assert!(s.genres.is_empty());
        assert!(build_hit(&s, false).is_none());
    }

    #[test]
    fn statuses() {
        assert_eq!(show_status("returning series"), AiredStatus::Continuing);
        assert_eq!(show_status("ended"), AiredStatus::Ended);
        assert_eq!(show_status("canceled"), AiredStatus::Canceled);
        assert_eq!(show_status("???"), AiredStatus::Unknown);
    }

    #[test]
    fn slug_genres_parse() {
        let genres = convert_genres(&["science-fiction".into(), "crime".into(), "crime".into()]);
        assert_eq!(genres.len(), 2);
        assert_eq!(genres[0], Genre::parse("Science Fiction").unwrap());
    }

    #[test]
    fn crew_departments_map_to_kinds() {
        assert_eq!(
            crew_kind("directing", &["Director".into()]),
            Some(CreditKind::Director)
        );
        assert_eq!(
            crew_kind("production", &["Executive Producer".into()]),
            Some(CreditKind::Producer)
        );
        assert_eq!(crew_kind("production", &["Casting".into()]), None);
        assert_eq!(crew_kind("costume & make-up", &[]), None);
    }

    #[test]
    fn trailer_ids_come_from_the_url() {
        let t = trailer_of("https://youtube.com/watch?v=abc123&t=1").unwrap();
        assert_eq!(t.id, "abc123");
        assert!(trailer_of("not a url").is_none());
    }

    #[test]
    fn episodes_need_a_positive_number() {
        let ok: Episode = serde_json::from_str(
            r#"{"season":1,"number":2,"number_abs":2,"title":"Cat in the Bag","ids":{"trakt":5}}"#,
        )
        .unwrap();
        let item = episode_item(&ok, 1).unwrap();
        let ItemDetail::Episode(fields) = &item.detail else {
            panic!("episode");
        };
        assert_eq!(fields.numbers.len(), 2);
        let none: Episode = serde_json::from_str(r#"{"season":1,"number":0}"#).unwrap();
        assert!(episode_item(&none, 1).is_none());
    }
}
