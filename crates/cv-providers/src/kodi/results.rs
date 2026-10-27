//! Reading what scraper functions return: search results, details and episode lists.
//!
//! A scraper answers with XML in Kodi's shapes:
//!
//! * `GetSearchResults`: `<results><entity><title/><year/><id/><url/>...</entity></results>`;
//! * `GetDetails`: `<details>` with `title`, `originaltitle`, `year`, `premiered`, `plot`,
//!   `runtime`, `genre`, `rating` or `ratings`, `thumb`, `fanart`, `actor`, `episodeguide`, ...;
//! * `GetEpisodeList`: `<episodeguide><episode><title/><season/><epnum/><aired/><url/></episode>`;
//! * `GetEpisodeDetails`: `<details>` of one episode.
//!
//! Fields a scraper leaves out stay empty.

use cv_domain::{
    AiredStatus, Credit, CreditKind, EpisodeFields, EpisodeGroup, EpisodeNumber, Genre, Item,
    ItemDetail, MediaFileType, ProviderIds, Rating, Trailer, id_key,
};

use super::scraper::{KodiContent, ScraperUrl};
use super::xml::{self, Element, escape};
use crate::model::SearchHit;
use crate::shared::{parse_date, year_of};
use crate::tmdb::certification_token;

// --- references ------------------------------------------------------------------------------

/// What a search result points at: the scraper's own ID and the page URLs its detail function
/// reads. A reference is stored as the title's ID for the add-on, in one of these forms:
///
/// * `https://host/page`: just the page;
/// * `id|https://host/page`: an ID and a page;
/// * `<id>..</id><url>..</url>`: the general form, used when there are several pages, the URL
///   carries options (POST, referer, function) or the ID holds a `|`;
/// * `id`: an ID with no page.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reference {
    pub id: String,
    pub urls: Vec<ScraperUrl>,
}

impl Reference {
    pub fn encode(&self) -> String {
        let plain_url = |u: &ScraperUrl| !u.post && u.spoof.is_none() && u.function.is_none();
        match self.urls.as_slice() {
            [] => self.id.clone(),
            [url] if plain_url(url) && !self.id.contains(['|', '<']) => {
                if self.id.is_empty() {
                    url.url.clone()
                } else {
                    format!("{}|{}", self.id, url.url)
                }
            }
            urls => {
                let mut out = String::new();
                if !self.id.is_empty() {
                    out.push_str(&format!("<id>{}</id>", escape(&self.id)));
                }
                for url in urls {
                    out.push_str("<url");
                    if url.post {
                        out.push_str(" post=\"yes\"");
                    }
                    if let Some(spoof) = &url.spoof {
                        out.push_str(&format!(" spoof=\"{}\"", escape(spoof)));
                    }
                    if let Some(function) = &url.function {
                        out.push_str(&format!(" function=\"{}\"", escape(function)));
                    }
                    out.push_str(&format!(">{}</url>", escape(&url.url)));
                }
                out
            }
        }
    }

    pub fn decode(text: &str) -> Self {
        let text = text.trim();
        if text.starts_with('<') {
            let document = xml::parse(text);
            return Self {
                id: document.child_text("id"),
                urls: document
                    .elements("url")
                    .filter_map(ScraperUrl::from_element)
                    .collect(),
            };
        }
        if text.starts_with("http://") || text.starts_with("https://") {
            return Self {
                id: String::new(),
                urls: vec![ScraperUrl {
                    url: text.to_owned(),
                    ..ScraperUrl::default()
                }],
            };
        }
        match text.split_once('|') {
            Some((id, url)) => Self {
                id: id.trim().to_owned(),
                urls: vec![ScraperUrl {
                    url: url.trim().to_owned(),
                    ..ScraperUrl::default()
                }],
            },
            None => Self {
                id: text.to_owned(),
                urls: Vec::new(),
            },
        }
    }
}

// --- search ----------------------------------------------------------------------------------

/// The titles in a `GetSearchResults` answer, in the scraper's order.
pub(super) fn search_hits(output: &str, provider: &str) -> Vec<SearchHit> {
    let document = xml::parse(output);
    let container = document.first("results").unwrap_or(&document);
    container
        .elements("entity")
        .filter_map(|entity| {
            let title = entity.child_text("title");
            let urls: Vec<ScraperUrl> = entity
                .elements("url")
                .filter_map(ScraperUrl::from_element)
                .collect();
            let reference = Reference {
                id: entity.child_text("id"),
                urls,
            };
            if title.is_empty() || (reference.id.is_empty() && reference.urls.is_empty()) {
                return None;
            }
            let mut ids = ProviderIds::new();
            ids.set(provider, reference.encode());
            let year_text = entity.child_text("year");
            let year = if year_text.is_empty() {
                year_of(parse_date(&entity.child_text("premiered")))
            } else {
                leading_year(&year_text)
            };
            Some(SearchHit {
                provider: provider.to_owned(),
                ids,
                title,
                original_title: entity.child_text("originaltitle"),
                year,
                overview: entity.child_text("plot"),
                poster_url: first_thumb(entity),
                exact_id: false,
                popularity: 0.0,
                votes: 0,
            })
        })
        .collect()
}

fn first_thumb(element: &Element) -> Option<String> {
    element
        .elements("thumb")
        .map(|t| t.text().trim().to_owned())
        .find(|t| t.starts_with("http"))
}

// --- details ---------------------------------------------------------------------------------

/// The root of a details answer: its `<details>` element, else the document itself.
fn details_root(document: &Element) -> &Element {
    document.first("details").unwrap_or(document)
}

/// Whether a details answer holds any field at all.
pub(super) fn is_empty_answer(output: &str) -> bool {
    let document = xml::parse(output);
    details_root(&document).child_elements().next().is_none()
}

/// The page URLs in a show's `<episodeguide>`.
pub(super) fn episode_guide_urls(output: &str) -> Vec<ScraperUrl> {
    let document = xml::parse(output);
    let Some(guide) = details_root(&document).first("episodeguide") else {
        return Vec::new();
    };
    let urls: Vec<ScraperUrl> = guide
        .elements("url")
        .filter_map(ScraperUrl::from_element)
        .collect();
    if urls.is_empty() {
        ScraperUrl::parse_all(guide.text().trim())
    } else {
        urls
    }
}

/// Builds the item for a `GetDetails` answer. `provider` is the add-on's provider key and
/// `reference` the ID value stored for it.
pub(super) fn details_item(
    output: &str,
    content: KodiContent,
    provider: &str,
    reference: &str,
    country: &str,
) -> Item {
    let document = xml::parse(output);
    let details = details_root(&document);
    let mut item = match content {
        KodiContent::Movies => Item::movie(details.child_text("title")),
        KodiContent::TvShows => Item::show(details.child_text("title")),
    };
    fill_common(&mut item, details, provider);
    item.core.ids.set(provider, reference);
    item.core.last_scraper_id = provider.to_owned();
    let genres = split_list(details, "genre")
        .iter()
        .filter_map(|g| Genre::parse(g))
        .fold(Vec::<Genre>::new(), |mut all, genre| {
            if !all.contains(&genre) {
                all.push(genre);
            }
            all
        });
    let premiered = release_date(details);
    let certification = certification_of(&details.child_text("mpaa"), country);
    let (extra_fanart, runtime) = (
        fanart_urls(details),
        runtime_minutes(&details.child_text("runtime")),
    );
    match &mut item.detail {
        ItemDetail::Movie(movie) => {
            movie.sort_title = details.child_text("sorttitle");
            movie.tagline = details.child_text("tagline");
            movie.runtime_minutes = runtime;
            movie.release_date = premiered;
            movie.genres = genres;
            movie.certification = certification;
            movie.countries = split_list(details, "country");
            movie.top250 = leading_number(&details.child_text("top250"));
            movie.trailers = trailers(details, provider);
            movie.extra_fanarts = extra_fanart.into_iter().skip(1).collect();
        }
        ItemDetail::Show(show) => {
            show.sort_title = details.child_text("sorttitle");
            show.tagline = details.child_text("tagline");
            show.runtime_minutes = runtime;
            show.first_aired = premiered;
            show.genres = genres;
            show.certification = certification;
            show.country = split_list(details, "country")
                .into_iter()
                .next()
                .unwrap_or_default();
            show.status = status_of(&details.child_text("status"));
            show.top250 = leading_number(&details.child_text("top250"));
            show.trailers = trailers(details, provider);
            show.extra_fanart_urls = extra_fanart.into_iter().skip(1).collect();
            show.episode_group = EpisodeGroup::aired();
            show.episode_groups = vec![EpisodeGroup::aired()];
        }
        _ => {}
    }
    item
}

/// Builds the item for an episode, from a `GetEpisodeList` entry or a `GetEpisodeDetails`
/// answer (`<episode>`/`<details>` element).
pub(super) fn episode_item(element: &Element, provider: &str) -> Item {
    let mut item = Item::new(ItemDetail::Episode(EpisodeFields::default()));
    item.core.title = element.child_text("title");
    fill_common(&mut item, element, provider);
    let season = leading_number(&element.child_text("season"));
    let number_text = [element.child_text("epnum"), element.child_text("episode")]
        .into_iter()
        .find(|t| !t.is_empty())
        .unwrap_or_default();
    let number = leading_number(&number_text);
    let aired = parse_date(&element.child_text("aired")).or_else(|| release_date(element));
    let reference = Reference {
        id: element.child_text("id"),
        urls: element
            .elements("url")
            .filter_map(ScraperUrl::from_element)
            .collect(),
    };
    if !reference.id.is_empty() || !reference.urls.is_empty() {
        item.core.ids.set(provider, reference.encode());
    }
    item.core.last_scraper_id = provider.to_owned();
    item.core.year = year_of(aired);
    let runtime = runtime_minutes(&element.child_text("runtime"));
    if let ItemDetail::Episode(fields) = &mut item.detail {
        fields.first_aired = aired;
        fields.runtime_minutes = runtime;
        if number > 0 || season > 0 || !number_text.is_empty() {
            fields
                .numbers
                .set(EpisodeNumber::new(EpisodeGroup::aired(), season, number));
        }
    }
    item
}

/// The episodes of a `GetEpisodeList` answer, ordered by season and number (specials first).
pub(super) fn episode_list(output: &str, provider: &str) -> Vec<Item> {
    let document = xml::parse(output);
    let guide = document.first("episodeguide").unwrap_or(&document);
    let mut episodes: Vec<Item> = guide
        .elements("episode")
        .map(|e| episode_item(e, provider))
        .filter(|item| matches!(&item.detail, ItemDetail::Episode(f) if f.numbers.get(&EpisodeGroup::aired()).is_some()))
        .collect();
    episodes.sort_by_key(|item| match &item.detail {
        ItemDetail::Episode(f) => f
            .numbers
            .get(&EpisodeGroup::aired())
            .map_or((u32::MAX, u32::MAX), |n| (n.season, n.episode)),
        _ => (u32::MAX, u32::MAX),
    });
    episodes
}

// --- shared fields ---------------------------------------------------------------------------

fn fill_common(item: &mut Item, element: &Element, provider: &str) {
    let core = &mut item.core;
    if core.title.is_empty() {
        core.title = element.child_text("title");
    }
    core.original_title = element.child_text("originaltitle");
    core.plot = element.child_text("plot");
    core.outline = element.child_text("outline");
    let year_text = element.child_text("year");
    core.year = if year_text.is_empty() {
        year_of(release_date(element))
    } else {
        leading_year(&year_text)
    };
    core.production_companies = split_list(element, "studio");
    for rating in ratings(element, provider) {
        core.ratings.insert(rating);
    }
    for (kind, tag) in [
        (CreditKind::Director, "director"),
        (CreditKind::Writer, "credits"),
    ] {
        for name in split_list(element, tag) {
            core.credits.push(Credit::new(kind, name, ""));
        }
    }
    for actor in element.elements("actor") {
        let name = actor.child_text("name");
        if name.is_empty() {
            continue;
        }
        let mut credit = Credit::new(CreditKind::Actor, name, actor.child_text("role"));
        credit.person.thumb_url = actor.child_text("thumb");
        core.credits.push(credit);
    }
    for (key, value) in unique_ids(element) {
        core.ids.set(key, value);
    }
    artwork(core, element);
}

fn artwork(core: &mut cv_domain::ItemCore, element: &Element) {
    for thumb in element.elements("thumb") {
        let url = thumb.text().trim().to_owned();
        if !url.starts_with("http") || thumb.attr("season").is_some() {
            continue;
        }
        let kind = match thumb
            .attr("aspect")
            .unwrap_or("poster")
            .to_ascii_lowercase()
            .as_str()
        {
            "poster" | "" => MediaFileType::Poster,
            "banner" => MediaFileType::Banner,
            "clearlogo" | "logo" => MediaFileType::Clearlogo,
            "clearart" => MediaFileType::Clearart,
            "landscape" | "thumb" => MediaFileType::Thumb,
            "discart" | "disc" => MediaFileType::Disc,
            "keyart" => MediaFileType::Keyart,
            "fanart" => MediaFileType::Fanart,
            _ => continue,
        };
        core.artwork_urls.entry(kind).or_insert(url);
    }
    if let Some(first) = fanart_urls(element).into_iter().next() {
        core.artwork_urls
            .entry(MediaFileType::Fanart)
            .or_insert(first);
    }
}

/// The fanart images: `<fanart url="prefix"><thumb>path</thumb></fanart>`.
fn fanart_urls(element: &Element) -> Vec<String> {
    let Some(fanart) = element.first("fanart") else {
        return Vec::new();
    };
    let prefix = fanart.attr("url").unwrap_or_default();
    fanart
        .elements("thumb")
        .map(|t| format!("{prefix}{}", t.text().trim()))
        .filter(|u| u.starts_with("http"))
        .collect()
}

fn trailers(element: &Element, provider: &str) -> Vec<Trailer> {
    element
        .elements("trailer")
        .map(|t| t.text().trim().to_owned())
        .filter(|u| u.starts_with("http"))
        .map(|url| Trailer {
            name: "Trailer".to_owned(),
            url,
            provider: provider.to_owned(),
            scraped_by: provider.to_owned(),
            ..Trailer::default()
        })
        .collect()
}

/// IDs a scraper reports besides its own: `<uniqueid type="imdb">` and an `<id>` that is an
/// IMDb ID.
fn unique_ids(element: &Element) -> Vec<(&'static str, String)> {
    let mut out: Vec<(&'static str, String)> = Vec::new();
    let mut add = |key: &'static str, value: String| {
        if !value.is_empty() && !out.iter().any(|(k, _)| *k == key) {
            out.push((key, value));
        }
    };
    for unique in element.elements("uniqueid") {
        let value = unique.text().trim().to_owned();
        let key = match unique
            .attr("type")
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str()
        {
            "imdb" => id_key::IMDB,
            "tmdb" => id_key::TMDB,
            "tvdb" | "thetvdb" => id_key::TVDB,
            "trakt" => id_key::TRAKT,
            "anidb" => id_key::ANIDB,
            "tvmaze" => id_key::TVMAZE,
            _ => continue,
        };
        add(key, value);
    }
    if let Some(imdb) = crate::shared::imdb_title_id(&element.child_text("id")) {
        add(id_key::IMDB, imdb);
    }
    out
}

/// Ratings from `<ratings><rating name max><value/><votes/></rating></ratings>` or a plain
/// `<rating max>7.5</rating>` with an optional `<votes>`.
fn ratings(element: &Element, provider: &str) -> Vec<Rating> {
    let mut out = Vec::new();
    let max_of = |e: &Element| {
        e.attr("max")
            .and_then(|m| m.trim().parse::<u32>().ok())
            .filter(|m| *m > 0)
            .unwrap_or(10)
    };
    if let Some(list) = element.first("ratings") {
        for rating in list.elements("rating") {
            let name = rating
                .attr("name")
                .unwrap_or_default()
                .trim()
                .to_ascii_lowercase();
            let source = if name.is_empty() {
                provider
            } else {
                name.as_str()
            };
            if let Some(value) = parse_decimal(&rating.child_text("value")) {
                out.push(Rating::new(
                    source,
                    value,
                    leading_number(&rating.child_text("votes").replace([',', '.', ' '], "")),
                    max_of(rating),
                ));
            }
        }
    }
    if out.is_empty()
        && let Some(rating) = element.first("rating")
        && let Some(value) = parse_decimal(&rating.text())
    {
        let name = rating
            .attr("name")
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        let source = if name.is_empty() {
            provider
        } else {
            name.as_str()
        };
        out.push(Rating::new(
            source,
            value,
            leading_number(&element.child_text("votes").replace([',', '.', ' '], "")),
            max_of(rating),
        ));
    }
    out
}

// --- value parsing ---------------------------------------------------------------------------

/// The values of every child named `tag`, each split at `/`, `,` and `|`.
fn split_list(element: &Element, tag: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for child in element.elements(tag) {
        for part in child.text().split(['/', ',', '|']) {
            let part = part.trim();
            if !part.is_empty() && !out.iter().any(|p| p == part) {
                out.push(part.to_owned());
            }
        }
    }
    out
}

fn release_date(element: &Element) -> Option<cv_domain::CalendarDate> {
    ["premiered", "releasedate", "aired"]
        .iter()
        .find_map(|tag| parse_date(&element.child_text(tag)))
}

/// The four digits a text starts with, `0` otherwise.
fn leading_year(text: &str) -> u16 {
    let year = crate::shared::year_prefix(text);
    if year == 0 {
        // "(1995)" or "Released 1995": the first four-digit run.
        let digits: Vec<char> = text.chars().collect();
        for window in digits.windows(4) {
            if window.iter().all(char::is_ascii_digit) {
                return window.iter().collect::<String>().parse().unwrap_or(0);
            }
        }
    }
    year
}

/// The first run of digits as a number, `0` when there is none.
fn leading_number(text: &str) -> u32 {
    let digits: String = text
        .trim()
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().unwrap_or(0)
}

fn parse_decimal(text: &str) -> Option<f32> {
    let text = text.trim();
    let text = if text.contains('.') {
        text.replace(',', "")
    } else {
        text.replace(',', ".")
    };
    text.parse::<f32>()
        .ok()
        .filter(|v| v.is_finite() && *v >= 0.0)
}

/// Minutes from `105`, `105 min`, `1h 45m`, `1:45` or `1:45:00`.
fn runtime_minutes(text: &str) -> u32 {
    let lower = text.trim().to_ascii_lowercase();
    if lower.is_empty() {
        return 0;
    }
    let numbers: Vec<u32> = lower
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok())
        .collect();
    if lower.contains(':') {
        return match numbers.as_slice() {
            [h, m, ..] => h * 60 + m,
            [m] => *m,
            [] => 0,
        };
    }
    if lower.contains('h') {
        return match numbers.as_slice() {
            [h, m, ..] => h * 60 + m,
            [h] => h * 60,
            [] => 0,
        };
    }
    numbers.first().copied().unwrap_or(0)
}

fn status_of(text: &str) -> AiredStatus {
    let lower = text.trim().to_ascii_lowercase();
    if lower.contains("cancel") {
        AiredStatus::Canceled
    } else if lower.contains("end") {
        AiredStatus::Ended
    } else if lower.contains("continu") || lower.contains("returning") || lower.contains("running")
    {
        AiredStatus::Continuing
    } else {
        AiredStatus::Unknown
    }
}

/// `Rated PG-13`, `PG-13`, `US:PG-13` and `DE:12` as canonical tokens. A rating with no
/// country is read in the request's country.
fn certification_of(text: &str, country: &str) -> cv_domain::Certification {
    let text = text.trim();
    let text = text.strip_prefix("Rated ").unwrap_or(text).trim();
    if let Some((prefix, rating)) = text.split_once(':')
        && prefix.trim().len() == 2
        && prefix.trim().bytes().all(|b| b.is_ascii_alphabetic())
    {
        return certification_token(prefix.trim(), rating);
    }
    certification_token(country, text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_round_trip_in_every_form() {
        let plain = Reference {
            id: String::new(),
            urls: vec![ScraperUrl {
                url: "https://x.example/m/1".into(),
                ..ScraperUrl::default()
            }],
        };
        assert_eq!(plain.encode(), "https://x.example/m/1");
        assert_eq!(Reference::decode(&plain.encode()), plain);

        let with_id = Reference {
            id: "42".into(),
            ..plain.clone()
        };
        assert_eq!(with_id.encode(), "42|https://x.example/m/1");
        assert_eq!(Reference::decode(&with_id.encode()), with_id);

        let general = Reference {
            id: "a|b".into(),
            urls: vec![
                ScraperUrl {
                    url: "https://x.example/s?a=1&b=2".into(),
                    post: true,
                    spoof: Some("https://x.example/".into()),
                    function: Some("GetMore".into()),
                },
                ScraperUrl {
                    url: "https://x.example/2".into(),
                    ..ScraperUrl::default()
                },
            ],
        };
        assert_eq!(Reference::decode(&general.encode()), general);
        assert_eq!(
            Reference::decode("77"),
            Reference {
                id: "77".into(),
                urls: vec![]
            }
        );
    }

    #[test]
    fn search_entities_become_hits() {
        let hits = search_hits(
            "<results><entity><title>Night Train</title><year>1995</year><id>7</id>\
             <url>https://x.example/m/7</url><thumb>https://img.example/7.jpg</thumb></entity>\
             <entity><title>No target</title></entity>\
             <entity><url>https://x.example/m/8</url></entity></results>",
            "kodi.sample",
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "Night Train");
        assert_eq!(hits[0].year, 1995);
        assert_eq!(
            hits[0].ids.get("kodi.sample"),
            Some("7|https://x.example/m/7")
        );
        assert_eq!(
            hits[0].poster_url.as_deref(),
            Some("https://img.example/7.jpg")
        );
    }

    #[test]
    fn runtimes() {
        assert_eq!(runtime_minutes("105"), 105);
        assert_eq!(runtime_minutes("105 min"), 105);
        assert_eq!(runtime_minutes("1h 45m"), 105);
        assert_eq!(runtime_minutes("2 h"), 120);
        assert_eq!(runtime_minutes("1:45:00"), 105);
        assert_eq!(runtime_minutes(""), 0);
    }

    #[test]
    fn numbers_and_years() {
        assert_eq!(leading_year("1995"), 1995);
        assert_eq!(leading_year("(1995)"), 1995);
        assert_eq!(leading_year("n/a"), 0);
        assert_eq!(leading_number("Top 250: #17"), 250);
        assert_eq!(parse_decimal("7,5"), Some(7.5));
        assert_eq!(parse_decimal("1,234.5"), Some(1234.5));
        assert_eq!(parse_decimal("n/a"), None);
    }

    #[test]
    fn certifications() {
        assert_eq!(certification_of("Rated PG-13", "US").0, "US_PG13");
        assert_eq!(certification_of("DE:12", "US").0, "DE_FSK12");
        assert_eq!(certification_of("", "US").0, "");
    }
}
