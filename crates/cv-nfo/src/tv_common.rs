//! Pieces the show, season and episode writers share: ratings, IDs, people and the outline.

use cv_domain::{Credit, CreditKind, ProviderIds, Rating, Ratings};

use crate::format;
use crate::kodi_movie::{person_ids, picture_children, rating_element, rating_name};
use crate::movie::MovieNfoOptions;
use crate::xml::Element;

/// `(key, value)` pairs in the order the fixtures list a map: by the hash slot of the key.
pub(crate) fn ordered_ids(ids: &ProviderIds) -> Vec<(&str, &str)> {
    let mut list: Vec<(&str, &str)> = ids.iter().collect();
    format::stable_map_order(&mut list, |pair| pair.0.to_owned());
    list
}

/// `<uniqueid type=".." default="..">` for every ID. `default_order` names the preferred ID types;
/// when none is present the first ID is the default. `None` leaves the `default` attribute out.
pub(crate) fn unique_ids(ids: &ProviderIds, default_order: Option<&[&str]>) -> Vec<Element> {
    let list = ordered_ids(ids);
    let default = default_order.and_then(|order| {
        order
            .iter()
            .find(|key| list.iter().any(|(k, _)| k == *key))
            .copied()
            .or_else(|| list.first().map(|(k, _)| *k))
    });
    list.into_iter()
        .map(|(key, value)| {
            let element = Element::text("uniqueid", value).attr("type", key);
            match default_order {
                Some(_) => element.attr(
                    "default",
                    if Some(key) == default {
                        "true"
                    } else {
                        "false"
                    },
                ),
                None => element,
            }
        })
        .collect()
}

/// The IDs as one JSON object of strings, in listing order: `{"tmdb":"1396","tvdb":"81189"}`.
pub(crate) fn ids_json(ids: &ProviderIds) -> String {
    let mut out = String::from("{");
    for (i, (key, value)) in ordered_ids(ids).into_iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        json_string(&mut out, key);
        out.push(':');
        json_string(&mut out, value);
    }
    out.push('}');
    out
}

fn json_string(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn external(ratings: &Ratings) -> impl Iterator<Item = &Rating> {
    ratings
        .iter()
        .filter(|r| r.id != cv_domain::rating_key::USER)
}

/// The rating the "rating sources" option puts first: the first source the entity has a rating
/// from.
pub(crate) fn preferred_rating<'a>(
    ratings: &'a Ratings,
    options: &MovieNfoOptions,
) -> Option<&'a Rating> {
    options.rating_sources.iter().find_map(|source| {
        let source = source.trim();
        if source.is_empty() {
            return None;
        }
        let wanted = rating_name(source);
        external(ratings).find(|r| rating_name(&r.id) == wanted)
    })
}

/// The default rating of a show: the preferred source, else TVDB, TMDB or IMDb in that order.
pub(crate) fn show_main_rating<'a>(
    ratings: &'a Ratings,
    options: &MovieNfoOptions,
) -> Option<&'a Rating> {
    preferred_rating(ratings, options).or_else(|| {
        ["tvdb", "tmdb", "imdb"]
            .into_iter()
            .find_map(|key| external(ratings).find(|r| r.id == key))
    })
}

/// `<ratings>` with the default rating first and the others in listing order.
pub(crate) fn ratings_element(main: Option<&Rating>, ratings: &Ratings) -> Element {
    let mut element = Element::new("ratings");
    if let Some(main) = main {
        element.push(rating_element(main, true));
    }
    let mut others: Vec<&Rating> = external(ratings)
        .filter(|r| main.is_none_or(|m| m.id != r.id))
        .collect();
    format::stable_map_order(&mut others, |r| rating_name(&r.id).to_owned());
    for other in others {
        element.push(rating_element(other, false));
    }
    element
}

/// The flat rating of the XBMC layout: the preferred source's value on a ten scale, else the last
/// other rating; `0.0` when there is none. The second value is the vote count of the preferred
/// source (0 when the entity has none).
pub(crate) fn flat_rating(ratings: &Ratings, options: &MovieNfoOptions) -> (String, String) {
    let preferred = preferred_rating(ratings, options);
    let shown = preferred.or_else(|| {
        let mut others: Vec<&Rating> = external(ratings).collect();
        format::stable_map_order(&mut others, |r| rating_name(&r.id).to_owned());
        others.last().copied()
    });
    let value = shown.map_or(0.0, |r| {
        if r.max_value > 0 {
            r.on_ten_scale()
        } else {
            r.value
        }
    });
    (
        format::fixed(value, 1),
        preferred.map_or(0, |r| r.votes).to_string(),
    )
}

/// `<userrating>`: the user rating on a ten scale with at most one decimal.
pub(crate) fn user_rating(ratings: &Ratings) -> String {
    ratings
        .get(cv_domain::rating_key::USER)
        .map_or_else(|| "0".to_owned(), |r| format::user_rating(r.on_ten_scale()))
}

/// The outline: the entity's own, else (option) one created from the plot, else the one of the
/// old NFO. `None` when there is none.
pub(crate) fn outline(
    own: &str,
    plot: &str,
    carried: &str,
    options: &MovieNfoOptions,
) -> Option<String> {
    let text = if !own.trim().is_empty() {
        own.to_owned()
    } else if options.create_outline {
        if options.outline_first_sentence {
            first_sentences(plot)
        } else {
            plot.to_owned()
        }
    } else {
        carried.to_owned()
    };
    Some(text).filter(|t| !t.trim().is_empty())
}

/// Minimum length of a created outline in first-sentence mode.
const MIN_OUTLINE_CHARS: usize = 20;

/// The plot's sentences, joined again with a period, up to the first point where at least 20
/// characters are collected. The last end mark is left off.
fn first_sentences(plot: &str) -> String {
    let mut parts: Vec<&str> = plot.split('.').collect();
    while parts.len() > 1 && parts.last().is_some_and(|p| p.is_empty()) {
        parts.pop();
    }
    let mut text = String::new();
    for part in parts {
        if !text.is_empty() {
            text.push('.');
        }
        text.push_str(part);
        if text.chars().count() >= MIN_OUTLINE_CHARS {
            break;
        }
    }
    text
}

/// A cast member: name, role (when there is one), picture, profile, the guest marker and the IDs
/// as child elements.
pub(crate) fn tv_actor(credit: &Credit, options: &MovieNfoOptions) -> Element {
    let mut e = Element::new("actor").child(Element::text("name", &credit.person.name));
    if !credit.role.trim().is_empty() {
        e.push(Element::text("role", &credit.role));
    }
    picture_children(&mut e, &credit.person, options);
    if credit.kind == CreditKind::Guest {
        e.push(Element::text("type", "GuestStar"));
    }
    for (name, value) in person_ids(&credit.person) {
        if !value.is_empty() {
            e.push(Element::text(name, value));
        }
    }
    e
}

/// The crew of a show or an episode: everyone who is not cast.
pub(crate) fn crew_of(credits: &[Credit]) -> impl Iterator<Item = &Credit> {
    credits
        .iter()
        .filter(|c| !matches!(c.kind, CreditKind::Actor | CreditKind::Guest))
}

pub(crate) fn of_kind(credits: &[Credit], kind: CreditKind) -> impl Iterator<Item = &Credit> {
    credits.iter().filter(move |c| c.kind == kind)
}

/// The id attribute of an episode ordering (`AIRED`, `DVD`, ...).
pub(crate) fn ordering_id(ordering: cv_domain::EpisodeOrdering) -> &'static str {
    use cv_domain::EpisodeOrdering as O;
    match ordering {
        O::Aired => "AIRED",
        O::Dvd => "DVD",
        O::Absolute => "ABSOLUTE",
        O::Alternate => "ALTERNATE",
        O::Display => "DISPLAY",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn created_outline_collects_sentences_up_to_twenty_characters() {
        assert_eq!(
            first_sentences("Two keepers, one rock, endless weather.\nSecond."),
            "Two keepers, one rock, endless weather"
        );
        assert_eq!(
            first_sentences("Short one. And more text here. Last."),
            "Short one. And more text here"
        );
        assert_eq!(first_sentences("Tiny. Bit."), "Tiny. Bit");
        assert_eq!(first_sentences("No end mark"), "No end mark");
        assert_eq!(first_sentences(""), "");
    }

    #[test]
    fn json_strings_are_escaped() {
        let mut ids = ProviderIds::new();
        ids.set("tvdb", "81189");
        ids.set("custom", "a\"b\\c");
        assert_eq!(
            ids_json(&ids),
            r#"{"custom":"a\"b\\c","tvdb":"81189"}"#.to_owned()
        );
        assert_eq!(ids_json(&ProviderIds::new()), "{}");
    }
}
