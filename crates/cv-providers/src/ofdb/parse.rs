//! Reading OFDb's pages.
//!
//! The site is German, and its film page is marked up with schema.org microdata (`itemprop`
//! attributes), which is what the reader relies on: the title heading, `description`,
//! `ratingValue` and `ratingCount`, `genre`, `countryOfOrigin`, `director`, `actor` and
//! `image`. The detail list (`dt`/`dd`) gives the original title and the alternative titles.
//!
//! A page is accepted only when it has the one thing every such page has: the heading with the
//! title for a film page, the result table for a search page. Everything else is optional and
//! simply left empty when missing.

use crate::markup::{self, Element};
use crate::shared::{html_to_text, imdb_title_id};
use crate::webpage::PageError;

/// One row of the search result table.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct SearchRow {
    pub id: u64,
    pub title: String,
    /// The title after `[OT: ...]`, when the row has one.
    pub original_title: Option<String>,
    pub year: u16,
    /// The row is a series (`[Serie]`).
    pub is_series: bool,
    pub poster: Option<String>,
}

/// Resolves a link the page gives relative to its `<base>` (`./images/x.jpg`) against `base`.
pub(super) fn resolve(base: &str, link: &str) -> String {
    let link = link.trim();
    if link.starts_with("http://") || link.starts_with("https://") {
        return link.to_owned();
    }
    let path = link.trim_start_matches("./").trim_start_matches('/');
    format!("{}/{path}", base.trim_end_matches('/'))
}

/// The film ID in a link such as `https://www.ofdb.de/film/2706,Heat/`.
fn film_id(href: &str) -> Option<u64> {
    let rest = href.split("/film/").nth(1)?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    // `film/2706,Heat/` and `film/2706/`; a link like `film/2706,275266,Heat/plot/` is a
    // sub-page of the film, not another film.
    let after = &rest[digits.len()..];
    (!digits.is_empty() && (after.is_empty() || after.starts_with([',', '/'])))
        .then(|| digits.parse().ok())
        .flatten()
}

/// The image in a tooltip such as `<img ... src='./images/film.185px/2/2706.jpg?v=1'>`.
fn tooltip_image(base: &str, title_attr: &str) -> Option<String> {
    let at = title_attr.find("src=")?;
    let rest = &title_attr[at + 4..];
    let quote = rest.chars().next().filter(|c| matches!(c, '\'' | '"'))?;
    let end = rest[1..].find(quote)?;
    let src = &rest[1..1 + end];
    // The placeholder for a film without a poster.
    if src.contains("noposter") || src.is_empty() {
        return None;
    }
    Some(resolve(base, src))
}

/// The rows of a search answer. An answer is a search page when it has the result table body
/// (`id="TabelleBody"`); a page without one is not understood.
pub(super) fn parse_search(html: &str, base: &str) -> Result<Vec<SearchRow>, PageError> {
    if markup::by_id(html, "TabelleBody").is_none() {
        return Err("the page has no result table".to_owned());
    }
    let mut rows = Vec::new();
    for row in markup::elements(html, "tr") {
        let Some(link) = markup::elements(row.inner, "a")
            .into_iter()
            .find(|a| a.attr("href").as_deref().and_then(film_id).is_some())
        else {
            continue;
        };
        let id = link
            .attr("href")
            .as_deref()
            .and_then(film_id)
            .unwrap_or_default();
        let named = markup::with_token(link.inner, "class", "tooltipster");
        let Some(title) = named.first().map(Element::text).filter(|t| !t.is_empty()) else {
            continue;
        };
        let poster = named
            .first()
            .and_then(|n| n.attr("title"))
            .and_then(|t| tooltip_image(base, &t));
        let notes: Vec<String> = markup::with_token(row.inner, "class", "smalltext")
            .iter()
            .map(Element::text)
            .collect();
        let original_title = notes.iter().find_map(|n| {
            n.strip_prefix("[OT:")
                .and_then(|t| t.strip_suffix(']'))
                .map(|t| t.trim().to_owned())
                .filter(|t| !t.is_empty())
        });
        let is_series = notes.iter().any(|n| n.contains("[Serie]"));
        let year = markup::elements(row.inner, "td")
            .last()
            .map_or(0, |cell| crate::shared::year_prefix(&cell.text()));
        rows.push(SearchRow {
            id,
            title,
            original_title,
            year,
            is_series,
            poster,
        });
    }
    Ok(rows)
}

/// What a film page says.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct Film {
    pub title: String,
    pub year: u16,
    pub original_title: String,
    pub plot: String,
    /// The plot is cut off with an ellipsis and the full text is on this page.
    pub plot_link: Option<String>,
    /// OFDb's rating on a ten point scale and the number of votes.
    pub rating: Option<(f32, u32)>,
    pub genres: Vec<String>,
    pub countries: Vec<String>,
    pub directors: Vec<String>,
    pub actors: Vec<String>,
    pub imdb: Option<String>,
    pub poster: Option<String>,
    pub backdrop: Option<String>,
}

fn first_text(html: &str, property: &str) -> Option<String> {
    markup::with_token(html, "itemprop", property)
        .first()
        .map(Element::text)
        .filter(|t| !t.is_empty())
}

fn texts(html: &str, property: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for text in markup::with_token(html, "itemprop", property)
        .iter()
        .map(Element::text)
        .filter(|t| !t.is_empty())
    {
        if !out.contains(&text) {
            out.push(text);
        }
    }
    out
}

/// The names inside the elements marked `itemprop="director"` or `"actor"`: each holds a
/// nested `itemprop="name"`.
fn people(html: &str, property: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for person in markup::with_token(html, "itemprop", property) {
        let name = first_text(person.inner, "name").unwrap_or_else(|| person.text());
        if !name.is_empty() && !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

/// The `dt`/`dd` list: each label (without its colon) with the HTML of its value. `dd` elements
/// are often not closed, so a value runs to the next label or the end of the list.
fn definitions(html: &str) -> Vec<(String, &str)> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(at) = rest.find("<dt") {
        let after = &rest[at..];
        let Some(open_end) = after.find('>') else {
            break;
        };
        let body = &after[open_end + 1..];
        let Some(close) = body.find("</dt>") else {
            break;
        };
        let label = markup::plain_text(&body[..close]);
        let value_from = &body[close + 5..];
        let value_end = [value_from.find("<dt"), value_from.find("</dl>")]
            .into_iter()
            .flatten()
            .min()
            .unwrap_or(value_from.len());
        out.push((
            label.trim_end_matches(':').trim().to_owned(),
            &value_from[..value_end],
        ));
        rest = &value_from[value_end..];
    }
    out
}

/// A title with its year, `Heat (1995)`, split into the two.
fn split_year(heading: &str) -> (String, u16) {
    let heading = heading.trim();
    if let Some(open) = heading.rfind('(')
        && heading.ends_with(')')
    {
        let year = crate::shared::year_prefix(&heading[open + 1..heading.len() - 1]);
        let inner = &heading[open + 1..heading.len() - 1];
        if year != 0 && inner.len() == 4 {
            return (heading[..open].trim().to_owned(), year);
        }
    }
    (heading.to_owned(), 0)
}

fn background_image(base: &str, html: &str) -> Option<String> {
    let header = markup::by_id(html, "HeaderFilmBild")?;
    let style = header.attr("style")?;
    let start = style.find("url(")? + 4;
    let end = style[start..].find(')')?;
    let link = style[start..start + end].trim().trim_matches(['\'', '"']);
    (!link.is_empty()).then(|| resolve(base, link))
}

fn imdb_of(html: &str) -> Option<String> {
    markup::elements(html, "a").into_iter().find_map(|a| {
        let href = a.attr("href")?;
        let at = href.find("imdb.com/title/")? + "imdb.com/title/".len();
        let id: String = href[at..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        imdb_title_id(&id)
    })
}

/// Reads a film page. It needs the title heading.
pub(super) fn parse_film(html: &str, base: &str) -> Result<Film, PageError> {
    let heading = markup::elements(html, "h1")
        .into_iter()
        .find(|h| {
            h.attr("itemprop")
                .is_some_and(|p| p.split_whitespace().any(|t| t == "name"))
        })
        .map(|h| h.text())
        .filter(|t| !t.is_empty())
        .ok_or_else(|| "the page has no title heading".to_owned())?;
    let (title, year_in_title) = split_year(&heading);

    let mut film = Film {
        title,
        year: year_in_title,
        ..Film::default()
    };
    film.plot = first_text(html, "description").unwrap_or_default();
    film.plot_link = markup::elements(html, "a")
        .into_iter()
        .filter_map(|a| a.attr("href"))
        .find(|href| href.contains("/plot/"));
    let value = first_text(html, "ratingValue")
        .and_then(|v| v.replace(',', ".").parse::<f32>().ok())
        .filter(|v| *v > 0.0);
    let votes = markup::with_token(html, "itemprop", "ratingCount")
        .first()
        .and_then(|e| e.attr("content").or_else(|| Some(e.text())))
        .map(|v| v.chars().filter(char::is_ascii_digit).collect::<String>())
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(0);
    film.rating = value.map(|v| (v, votes));
    film.genres = texts(html, "genre");
    film.countries = texts(html, "countryOfOrigin");
    film.directors = people(html, "director");
    film.actors = people(html, "actor");
    film.imdb = imdb_of(html);
    film.poster = markup::with_token(html, "itemprop", "image")
        .first()
        .and_then(|i| i.attr("src"))
        .map(|src| resolve(base, &src));
    film.backdrop = background_image(base, html);

    for (label, value) in definitions(html) {
        match label.as_str() {
            "Originaltitel" => {
                film.original_title = html_to_text(value)
                    .lines()
                    .map(str::trim)
                    .find(|l| !l.is_empty())
                    .unwrap_or("")
                    .to_owned();
            }
            "Erscheinungsjahr" if film.year == 0 => {
                film.year = crate::shared::year_prefix(&markup::plain_text(value));
            }
            _ => {}
        }
    }
    Ok(film)
}

/// The full plot on the plot page: a headline cut off with `...`, then the text that continues
/// with `...`. The page is only a plot page when it has that text.
pub(super) fn parse_plot_page(html: &str) -> Result<String, PageError> {
    let body = markup::with_token(html, "class", "bodytext")
        .into_iter()
        .next()
        .ok_or_else(|| "the page has no plot text".to_owned())?;
    let headline = markup::elements(body.inner, "h4")
        .first()
        .map(Element::text)
        .unwrap_or_default();
    let paragraphs: Vec<Element<'_>> = markup::elements(body.inner, "p")
        .into_iter()
        .filter(|p| {
            !p.attr("class")
                .is_some_and(|c| c.contains("flagline_author"))
        })
        .collect();
    let text = paragraphs
        .first()
        .map(|p| html_to_text(p.inner))
        .filter(|t| !t.trim().is_empty())
        .ok_or_else(|| "the plot page has no text".to_owned())?;
    let text = text.trim();
    let headline = headline.trim_end_matches(['.', '…', ' ']);
    let text = text.trim_start_matches(['.', '…']).trim_start();
    Ok(if headline.is_empty() {
        text.to_owned()
    } else {
        format!("{headline} {text}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "https://www.ofdb.de";

    #[test]
    fn film_links_are_told_from_their_sub_pages() {
        assert_eq!(film_id("https://www.ofdb.de/film/2706,Heat/"), Some(2706));
        assert_eq!(film_id("https://www.ofdb.de/film/2706/"), Some(2706));
        assert_eq!(film_id("/film/2706"), Some(2706));
        assert_eq!(film_id("https://www.ofdb.de/person/170,Name/"), None);
        assert_eq!(film_id("https://www.ofdb.de/film/abc/"), None);
    }

    #[test]
    fn relative_links_resolve_against_the_base() {
        assert_eq!(
            resolve(BASE, "./images/film.370px/2/2706.jpg?v=1"),
            "https://www.ofdb.de/images/film.370px/2/2706.jpg?v=1"
        );
        assert_eq!(
            resolve(BASE, "https://x.test/a.jpg"),
            "https://x.test/a.jpg"
        );
    }

    #[test]
    fn headings_split_into_title_and_year() {
        assert_eq!(split_year("Heat (1995)"), ("Heat".to_owned(), 1995));
        assert_eq!(split_year("Heat"), ("Heat".to_owned(), 0));
        assert_eq!(
            split_year("Es (Remake) (2017)"),
            ("Es (Remake)".to_owned(), 2017)
        );
        assert_eq!(split_year("Zahl (12)"), ("Zahl (12)".to_owned(), 0));
    }

    #[test]
    fn definition_values_run_to_the_next_label() {
        let html = "<dl><dt><b>Originaltitel:</b></dt><dd>Heat<dt><b>Regie:</b></dt><dd>A<br><dd></dl><p>x</p>";
        let defs = definitions(html);
        assert_eq!(defs.len(), 2);
        assert_eq!(defs[0].0, "Originaltitel");
        assert_eq!(markup::plain_text(defs[0].1), "Heat");
        assert_eq!(defs[1].0, "Regie");
    }

    #[test]
    fn a_search_page_without_the_table_is_not_understood() {
        assert!(parse_search("<html><body><ul><li>x</li></ul></body></html>", BASE).is_err());
        let empty = r#"<table><tbody id="TabelleBody"></tbody></table>"#;
        assert_eq!(parse_search(empty, BASE), Ok(Vec::new()));
    }

    #[test]
    fn a_page_without_a_title_heading_is_not_a_film_page() {
        assert!(parse_film("<html><h1>Nope</h1></html>", BASE).is_err());
        assert!(parse_film(r#"<h1 itemprop="name">Titel (2001)</h1>"#, BASE).is_ok());
    }

    #[test]
    fn plot_pages_join_the_headline_and_the_text() {
        let html = r#"<div class="bodytext"><h4>Ein Satz, der abbricht ...</h4><p>... und hier weitergeht.</p></div>"#;
        assert_eq!(
            parse_plot_page(html).unwrap(),
            "Ein Satz, der abbricht und hier weitergeht."
        );
        assert!(parse_plot_page("<div>nothing</div>").is_err());
    }
}
