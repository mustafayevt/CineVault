//! Reading fernsehserien.de's pages.
//!
//! The site is German and has no API. A show page, a film page and an episode guide are read
//! from the structured data the pages embed for search engines: schema.org JSON-LD
//! (`TVSeries`, `Movie`, `TVSeason`, `TVEpisode`), with the page heading and the Open Graph
//! `<meta>` tags as a fallback for the name, plot and image. Episode guides also fall back to
//! schema.org microdata (`itemtype="…/TVEpisode"` with `itemprop` children).
//!
//! The site is behind a bot check that answers programs with an HTTP 202 challenge, so these
//! readers are checked against hand-built pages only (see the opt-in live test). They are
//! strict about what a page must have to be usable: a title for a show or film page, at least one
//! episode for an episode guide. A page without that is "page format changed", and nothing is
//! returned.

use serde_json::Value;

use crate::markup::{self, Element, has_type};
use crate::shared::{html_to_text, imdb_title_id, parse_date, year_prefix};
use crate::webpage::PageError;

/// The first path segments of the site that are sections, not shows.
const SECTIONS: &[&str] = &[
    "suche",
    "serien",
    "sendungen",
    "filme",
    "programm",
    "fernsehprogramm",
    "mediathek",
    "news",
    "impressum",
    "datenschutz",
    "kontakt",
    "login",
    "registrieren",
    "hilfe",
    "werbung",
    "nutzungsbedingungen",
    "rss",
    "app",
    "sender",
    "neu",
    "tipps",
    "tv-tipps",
];

/// A result of the search page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SearchRow {
    pub slug: String,
    pub title: String,
    pub year: u16,
}

/// Whether the text is the path of a show (`tatort`, `the-big-bang-theory`).
pub(super) fn is_slug(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 120
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// The slug of a link to a show page: `/tatort`, `https://www.fernsehserien.de/tatort/`.
fn slug_of(href: &str) -> Option<String> {
    let path = match href.split_once("://") {
        Some((_, rest)) => &rest[rest.find('/')?..],
        None => href,
    };
    let path = path.split(['?', '#']).next()?.trim_matches('/');
    (is_slug(path) && !SECTIONS.contains(&path.to_ascii_lowercase().as_str()))
        .then(|| path.to_owned())
}

/// The title of a search page with `- fernsehserien.de` and the like removed.
fn clean_title(title: &str) -> String {
    let mut title = title.trim();
    for separator in [" – ", " - ", " | ", " — "] {
        if let Some((head, tail)) = title.rsplit_once(separator)
            && tail.to_ascii_lowercase().contains("fernsehserien")
        {
            title = head.trim();
        }
    }
    title.to_owned()
}

/// What a show or film page is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PageKind {
    Series,
    Movie,
}

/// The main series or film node of a page's JSON-LD.
fn main_node(nodes: &[Value]) -> Option<(&Value, PageKind)> {
    for (types, kind) in [
        (&["TVSeries", "CreativeWorkSeries"][..], PageKind::Series),
        (&["Movie", "TVMovie"][..], PageKind::Movie),
    ] {
        if let Some(node) = nodes.iter().find(|n| types.iter().any(|t| has_type(n, t))) {
            return Some((node, kind));
        }
    }
    None
}

/// The rows of a search page. A search that lands straight on a show (the site redirects) is
/// recognised by its show data and returned as the one row.
pub(super) fn parse_search(html: &str) -> Result<Vec<SearchRow>, PageError> {
    let nodes = markup::json_ld(html);
    if let Some((node, _)) = main_node(&nodes) {
        let url = text(node, "url").or_else(|| markup::meta(html, "og:url"));
        let slug = url
            .as_deref()
            .and_then(slug_of)
            .ok_or_else(|| "the show page names no address".to_owned())?;
        let title = text(node, "name").ok_or_else(|| "the show page has no name".to_owned())?;
        return Ok(vec![SearchRow {
            slug,
            title,
            year: year_of(node),
        }]);
    }

    // A results page says so in its title or heading.
    let is_results = markup::elements(html, "title")
        .first()
        .map(Element::text)
        .into_iter()
        .chain(markup::elements(html, "h1").first().map(Element::text))
        .any(|t| t.to_lowercase().contains("suche"));
    if !is_results {
        return Err("the page is not a search result page".to_owned());
    }
    // Links to shows inside the main content (the navigation links to sections, not shows).
    let content = markup::elements(html, "main")
        .first()
        .map_or(html, |m| m.inner);
    let mut rows: Vec<SearchRow> = Vec::new();
    for link in markup::elements(content, "a") {
        let Some(slug) = link.attr("href").as_deref().and_then(slug_of) else {
            continue;
        };
        let title = link.text();
        if title.is_empty() || rows.iter().any(|r| r.slug == slug) {
            continue;
        }
        // The year is shown in brackets after the title: `Tatort (1970)`.
        let year = title
            .rfind('(')
            .map_or(0, |open| year_prefix(&title[open + 1..]));
        let title = title
            .rfind('(')
            .filter(|_| year != 0)
            .map_or(title.as_str(), |open| title[..open].trim_end())
            .to_owned();
        rows.push(SearchRow { slug, title, year });
    }
    Ok(rows)
}

/// A non-blank string of `name`.
fn text(node: &Value, name: &str) -> Option<String> {
    node.get(name)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
}

/// The names in a JSON-LD value: a string, an object with a `name`, or a list of either.
fn names(value: Option<&Value>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut add = |name: &str| {
        let name = name.trim();
        if !name.is_empty() && !out.iter().any(|o| o == name) {
            out.push(name.to_owned());
        }
    };
    fn walk(value: &Value, add: &mut impl FnMut(&str)) {
        match value {
            Value::String(s) => add(s),
            Value::Array(list) => list.iter().for_each(|v| walk(v, add)),
            Value::Object(_) => {
                if let Some(name) = value.get("name").and_then(Value::as_str) {
                    add(name);
                }
            }
            _ => {}
        }
    }
    if let Some(value) = value {
        walk(value, &mut add);
    }
    out
}

/// An image address from a string, an `ImageObject` or a list of either.
fn image_of(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(s) => Some(s.trim().to_owned()).filter(|s| s.starts_with("http")),
        Value::Array(list) => list.iter().find_map(|v| image_of(Some(v))),
        object @ Value::Object(_) => text(object, "url")
            .or_else(|| text(object, "contentUrl"))
            .filter(|s| s.starts_with("http")),
        _ => None,
    }
}

fn year_of(node: &Value) -> u16 {
    ["datePublished", "startDate", "dateCreated", "copyrightYear"]
        .iter()
        .find_map(|key| {
            node.get(*key).and_then(|v| match v {
                Value::String(s) => Some(year_prefix(s)),
                Value::Number(n) => n.as_u64().map(|y| u16::try_from(y).unwrap_or(0)),
                _ => None,
            })
        })
        .filter(|y| *y != 0)
        .unwrap_or(0)
}

fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().replace(',', ".").parse().ok(),
        _ => None,
    }
}

/// What a show or film page says.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Page {
    pub kind: PageKind,
    pub title: String,
    pub year: u16,
    pub year_end: u16,
    pub plot: String,
    pub genres: Vec<String>,
    pub countries: Vec<String>,
    pub directors: Vec<String>,
    pub actors: Vec<String>,
    pub image: Option<String>,
    pub imdb: Option<String>,
    /// `(value, votes, best value)`.
    pub rating: Option<(f32, u32, u32)>,
    pub first_aired: Option<cv_domain::CalendarDate>,
}

/// Reads a show or film page. It needs a title.
pub(super) fn parse_page(html: &str) -> Result<Page, PageError> {
    let nodes = markup::json_ld(html);
    let (node, kind) = match main_node(&nodes) {
        Some((node, kind)) => (Some(node), kind),
        None => (None, PageKind::Series),
    };
    let heading = markup::elements(html, "h1")
        .first()
        .map(Element::text)
        .filter(|t| !t.is_empty());
    let title = node
        .and_then(|n| text(n, "name"))
        .or(heading)
        .or_else(|| markup::meta(html, "og:title").map(|t| clean_title(&t)))
        .ok_or_else(|| "the page has no title".to_owned())?;

    let blank = Value::Null;
    let node_ref = node.unwrap_or(&blank);
    let plot = text(node_ref, "description")
        .or_else(|| markup::meta(html, "og:description"))
        .or_else(|| markup::meta(html, "description"))
        .map(|d| html_to_text(&d))
        .unwrap_or_default();
    let first_aired = ["startDate", "datePublished"]
        .iter()
        .find_map(|k| text(node_ref, k))
        .and_then(|d| parse_date(&d));
    let rating = node_ref.get("aggregateRating").and_then(|r| {
        let value = r.get("ratingValue").and_then(number)? as f32;
        let votes = r
            .get("ratingCount")
            .or_else(|| r.get("reviewCount"))
            .and_then(number)
            .map_or(0, |v| v as u32);
        let best = r
            .get("bestRating")
            .and_then(number)
            .filter(|b| *b > 0.0)
            .map_or(10, |b| b as u32);
        (value > 0.0).then_some((value, votes, best))
    });
    let imdb = names(node_ref.get("sameAs")).iter().find_map(|link| {
        let at = link.find("imdb.com/title/")? + "imdb.com/title/".len();
        let id: String = link[at..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        imdb_title_id(&id)
    });
    Ok(Page {
        kind,
        title,
        year: year_of(node_ref),
        year_end: node_ref
            .get("endDate")
            .and_then(Value::as_str)
            .map_or(0, year_prefix),
        plot,
        genres: names(node_ref.get("genre")),
        countries: names(node_ref.get("countryOfOrigin")),
        directors: names(node_ref.get("director")),
        actors: names(node_ref.get("actor")),
        image: image_of(node_ref.get("image")).or_else(|| markup::meta(html, "og:image")),
        imdb,
        rating,
        first_aired,
    })
}

/// The cast and crew on the cast page: schema.org `actor` and `director` entries, as JSON-LD or
/// microdata.
pub(super) fn parse_people(html: &str) -> (Vec<String>, Vec<String>) {
    let nodes = markup::json_ld(html);
    let mut actors: Vec<String> = Vec::new();
    let mut directors: Vec<String> = Vec::new();
    let add = |into: &mut Vec<String>, names: Vec<String>| {
        for name in names {
            if !into.contains(&name) {
                into.push(name);
            }
        }
    };
    for node in &nodes {
        add(&mut actors, names(node.get("actor")));
        add(&mut directors, names(node.get("director")));
    }
    let micro = |property: &str| -> Vec<String> {
        markup::with_token(html, "itemprop", property)
            .iter()
            .map(|e| {
                markup::with_token(e.inner, "itemprop", "name")
                    .first()
                    .map_or_else(|| e.text(), Element::text)
            })
            .filter(|n| !n.is_empty())
            .collect()
    };
    add(&mut actors, micro("actor"));
    add(&mut directors, micro("director"));
    (actors, directors)
}

/// One episode of an episode guide.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct EpisodeRow {
    pub season: u32,
    pub number: u32,
    pub title: String,
    pub aired: Option<cv_domain::CalendarDate>,
    pub plot: String,
}

fn whole(value: &Value) -> Option<u32> {
    number(value).filter(|n| *n >= 0.0).map(|n| n as u32)
}

fn episode_node(node: &Value, season: Option<u32>) -> Option<EpisodeRow> {
    let number = node.get("episodeNumber").and_then(whole)?;
    let season = node
        .get("partOfSeason")
        .and_then(|s| s.get("seasonNumber"))
        .and_then(whole)
        .or_else(|| node.get("seasonNumber").and_then(whole))
        .or(season)?;
    let title = text(node, "name")?;
    let aired = ["datePublished", "dateCreated", "uploadDate"]
        .iter()
        .find_map(|k| text(node, k))
        .and_then(|d| parse_date(&d));
    Some(EpisodeRow {
        season,
        number,
        title,
        aired,
        plot: text(node, "description")
            .map(|d| html_to_text(&d))
            .unwrap_or_default(),
    })
}

/// Collects the episodes below a JSON-LD node: its own `episode` list, the episodes of the
/// seasons it contains, or the node itself when it is an episode.
fn collect(node: &Value, season: Option<u32>, out: &mut Vec<EpisodeRow>) {
    if has_type(node, "TVEpisode") {
        out.extend(episode_node(node, season));
    }
    let own_season = node
        .get("seasonNumber")
        .and_then(whole)
        .filter(|_| has_type(node, "TVSeason"))
        .or(season);
    for key in ["episode", "episodes", "containsSeason", "season", "hasPart"] {
        match node.get(key) {
            Some(Value::Array(list)) => list.iter().for_each(|n| collect(n, own_season, out)),
            Some(child @ Value::Object(_)) => collect(child, own_season, out),
            _ => {}
        }
    }
}

/// The episodes of an episode guide page. `default_season` numbers episodes whose data names no
/// season (the page of one season).
pub(super) fn parse_episodes(
    html: &str,
    default_season: Option<u32>,
) -> Result<Vec<EpisodeRow>, PageError> {
    let mut rows: Vec<EpisodeRow> = Vec::new();
    for node in markup::json_ld(html) {
        collect(&node, default_season, &mut rows);
    }
    if rows.is_empty() {
        // Microdata: `<… itemtype="https://schema.org/TVEpisode">` with `itemprop` children.
        for element in markup::with_token(html, "itemprop", "episode")
            .into_iter()
            .chain(micro_episodes(html))
        {
            let prop = |name: &str| {
                markup::with_token(element.inner, "itemprop", name)
                    .first()
                    .map(|e| {
                        e.attr("content")
                            .or_else(|| e.attr("datetime"))
                            .unwrap_or_else(|| e.text())
                    })
                    .map(|t| t.trim().to_owned())
                    .filter(|t| !t.is_empty())
            };
            let (Some(number), Some(title)) = (
                prop("episodeNumber").and_then(|n| n.parse::<u32>().ok()),
                prop("name"),
            ) else {
                continue;
            };
            let Some(season) = prop("seasonNumber")
                .and_then(|n| n.parse::<u32>().ok())
                .or(default_season)
            else {
                continue;
            };
            rows.push(EpisodeRow {
                season,
                number,
                title,
                aired: prop("datePublished").and_then(|d| parse_date(&d)),
                plot: prop("description").unwrap_or_default(),
            });
        }
    }
    if rows.is_empty() {
        return Err("the page lists no episodes".to_owned());
    }
    // The same episode can be named twice (a season node and a flat list); keep the first.
    let mut seen: Vec<(u32, u32)> = Vec::new();
    rows.retain(|row| {
        let key = (row.season, row.number);
        let new = !seen.contains(&key);
        seen.push(key);
        new
    });
    rows.sort_by_key(|r| (r.season, r.number));
    Ok(rows)
}

fn micro_episodes(html: &str) -> Vec<Element<'_>> {
    markup::with_token(html, "itemtype", "https://schema.org/TVEpisode")
        .into_iter()
        .chain(markup::with_token(
            html,
            "itemtype",
            "http://schema.org/TVEpisode",
        ))
        .collect()
}

/// The seasons an episode guide links to, as `(season number, path)`:
/// `/tatort/episodenguide/staffel-3/12345`.
pub(super) fn season_links(html: &str) -> Vec<(u32, String)> {
    let mut out: Vec<(u32, String)> = Vec::new();
    for link in markup::elements(html, "a") {
        let Some(href) = link.attr("href") else {
            continue;
        };
        let path = match href.split_once("://") {
            Some((_, rest)) => rest.find('/').map(|p| rest[p..].to_owned()),
            None => Some(href.clone()),
        };
        let Some(path) = path else { continue };
        let path = path.split(['?', '#']).next().unwrap_or("").to_owned();
        let Some(after) = path.split("/episodenguide/staffel-").nth(1) else {
            continue;
        };
        let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
        let Ok(season) = digits.parse::<u32>() else {
            continue;
        };
        if !out.iter().any(|(s, _)| *s == season) {
            out.push((season, path));
        }
    }
    out.sort_by_key(|(season, _)| *season);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_to_shows_are_told_from_sections() {
        assert_eq!(slug_of("/tatort").as_deref(), Some("tatort"));
        assert_eq!(
            slug_of("https://www.fernsehserien.de/the-big-bang-theory/").as_deref(),
            Some("the-big-bang-theory")
        );
        assert_eq!(slug_of("/suche"), None);
        assert_eq!(slug_of("/tatort/episodenguide"), None);
        assert_eq!(slug_of("/"), None);
        assert_eq!(slug_of("#top"), None);
    }

    #[test]
    fn titles_lose_the_site_name() {
        assert_eq!(clean_title("Tatort – fernsehserien.de"), "Tatort");
        assert_eq!(clean_title("Tatort | Fernsehserien.de"), "Tatort");
        assert_eq!(clean_title("Tatort - Teil 2"), "Tatort - Teil 2");
    }

    #[test]
    fn a_page_without_a_title_is_not_a_show_page() {
        assert!(parse_page("<html><body><p>nothing</p></body></html>").is_err());
        assert!(parse_page("<h1>Ein Titel</h1>").is_ok());
    }

    #[test]
    fn a_page_that_is_not_a_search_page_is_not_understood() {
        assert!(
            parse_search("<html><title>Startseite</title><a href='/tatort'>Tatort</a></html>")
                .is_err()
        );
        let empty = "<html><title>Suche nach 'zzz'</title><main><p>Keine Treffer</p></main></html>";
        assert_eq!(parse_search(empty), Ok(Vec::new()));
    }

    #[test]
    fn episodes_may_sit_in_seasons_or_in_a_flat_list() {
        let html = r#"<script type="application/ld+json">{"@type":"TVSeries","containsSeason":[
            {"@type":"TVSeason","seasonNumber":1,"episode":[
              {"@type":"TVEpisode","episodeNumber":2,"name":"Zweite"},
              {"@type":"TVEpisode","episodeNumber":"1","name":"Erste","datePublished":"1970-11-29"}]},
            {"@type":"TVSeason","seasonNumber":2,"episode":{"@type":"TVEpisode","episodeNumber":1,"name":"Neue"}}]}</script>
            <script type="application/ld+json">{"@type":"TVEpisode","episodeNumber":1,"name":"Doppelt","partOfSeason":{"seasonNumber":1}}</script>"#;
        let rows = parse_episodes(html, None).unwrap();
        let shown: Vec<_> = rows
            .iter()
            .map(|r| (r.season, r.number, r.title.as_str()))
            .collect();
        assert_eq!(shown, [(1, 1, "Erste"), (1, 2, "Zweite"), (2, 1, "Neue")]);
        assert!(rows[0].aired.is_some());
        assert!(parse_episodes("<html></html>", None).is_err());
    }

    #[test]
    fn season_links_are_found_once_each() {
        let html = r#"<a href="/tatort/episodenguide/staffel-2/777">2</a><a href="https://www.fernsehserien.de/tatort/episodenguide/staffel-1/555?x=1">1</a><a href="/tatort/episodenguide/staffel-1/555">1</a><a href="/tatort/cast-crew">c</a>"#;
        assert_eq!(
            season_links(html),
            [
                (1, "/tatort/episodenguide/staffel-1/555".to_owned()),
                (2, "/tatort/episodenguide/staffel-2/777".to_owned())
            ]
        );
    }
}
