//! TheSportsDB answers to CineVault items, hits and episodes.
//!
//! TheSportsDB sends every value as text (numbers included) and `null` or `""` for a missing
//! one, so the readers here treat both as absent.

use cv_domain::{
    AiredStatus, CalendarDate, EpisodeFields, EpisodeGroup, EpisodeNumber, Genre, Item, ItemDetail,
    MediaFileType, ProviderIds, SeasonText,
};
use serde_json::Value;

use super::{ID_KEY, PROVIDER_ID};
use crate::model::{RequestOptions, SearchHit};
use crate::shared::{country_codes, parse_date, set_id, year_of, year_prefix};

/// The most hits a search returns.
const MAX_HITS: usize = 25;

/// A non-blank text value of `name`. Numbers are read as their text.
pub(super) fn text<'a>(value: &'a Value, name: &str) -> Option<&'a str> {
    let text = value.get(name)?.as_str()?.trim();
    (!text.is_empty()).then_some(text)
}

/// The first entry of the list `name` in an answer (`{"leagues": [...]}`); `null` is empty.
pub(super) fn first<'a>(answer: &'a Value, name: &str) -> Option<&'a Value> {
    answer.get(name)?.as_array()?.first()
}

fn list<'a>(answer: &'a Value, name: &str) -> &'a [Value] {
    answer
        .get(name)
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

fn url(value: &Value, name: &str) -> Option<String> {
    text(value, name)
        .filter(|u| u.starts_with("http"))
        .map(str::to_owned)
}

/// Letters and digits in lower case, words separated by one space.
fn words(text: &str) -> String {
    let mut out = String::new();
    for word in text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
    {
        if !out.is_empty() {
            out.push(' ');
        }
        out.extend(word.chars().flat_map(char::to_lowercase));
    }
    out
}

fn league_ids(league: &Value) -> Option<ProviderIds> {
    let mut ids = ProviderIds::new();
    set_id(&mut ids, ID_KEY, text(league, "idLeague")?);
    ids.contains(ID_KEY).then_some(ids)
}

/// The name of the description field for a language: `en` is `strDescriptionEN`; the service
/// names a few languages after their country (`ja` is `JP`, `zh` is `CN`, `sv` is `SE`).
fn description_suffix(language: &str) -> Option<&'static str> {
    let language = language.trim().to_ascii_lowercase();
    Some(match language.split(['-', '_']).next()? {
        "en" => "EN",
        "de" => "DE",
        "fr" => "FR",
        "it" => "IT",
        "zh" => "CN",
        "ja" => "JP",
        "ru" => "RU",
        "es" => "ES",
        "pt" => "PT",
        "sv" => "SE",
        "nl" => "NL",
        "hu" => "HU",
        "no" | "nb" | "nn" => "NO",
        "pl" => "PL",
        "he" => "IL",
        _ => return None,
    })
}

/// The league description in the preferred language, else the fallback language, else English,
/// else any language that has one.
fn description(league: &Value, options: &RequestOptions) -> String {
    let wanted = [
        description_suffix(&options.language),
        description_suffix(&options.fallback_language),
        Some("EN"),
    ];
    for suffix in wanted.into_iter().flatten() {
        if let Some(text) = text(league, &format!("strDescription{suffix}")) {
            return text.replace("\r\n", "\n");
        }
    }
    league
        .as_object()
        .into_iter()
        .flat_map(|map| map.iter())
        .filter(|(key, _)| key.starts_with("strDescription"))
        .find_map(|(_, value)| value.as_str().map(str::trim).filter(|t| !t.is_empty()))
        .unwrap_or("")
        .replace("\r\n", "\n")
}

/// The hit for a league; `None` without an ID or a name.
pub(super) fn build_hit(
    league: &Value,
    exact_id: bool,
    options: &RequestOptions,
) -> Option<SearchHit> {
    let ids = league_ids(league)?;
    let name = text(league, "strLeague")?;
    Some(SearchHit {
        provider: PROVIDER_ID.to_owned(),
        ids,
        title: name.to_owned(),
        original_title: name.to_owned(),
        year: text(league, "intFormedYear").map_or(0, year_prefix),
        overview: description(league, options),
        poster_url: url(league, "strPoster").or_else(|| url(league, "strBadge")),
        exact_id,
        popularity: 0.0,
        votes: 0,
    })
}

/// The leagues of `all_leagues.php` whose name (or alternate names) holds every word of `query`,
/// best match first: the exact name, then names that start with it, then the rest by length.
pub(super) fn search_leagues(answer: &Value, query: &str) -> Vec<SearchHit> {
    let wanted = words(query);
    if wanted.is_empty() {
        return Vec::new();
    }
    let options = RequestOptions::default();
    let mut found: Vec<(u8, usize, SearchHit)> = Vec::new();
    for league in list(answer, "leagues") {
        let Some(hit) = build_hit(league, false, &options) else {
            continue;
        };
        let name = words(&hit.title);
        let alternates = text(league, "strLeagueAlternate").map_or(String::new(), words);
        let haystack = format!("{name} {alternates}");
        // A word of the query must appear as a whole word somewhere in the names.
        let all_words = wanted
            .split(' ')
            .all(|w| haystack.split(' ').any(|h| h == w));
        if !all_words {
            continue;
        }
        let rank = if name == wanted {
            0
        } else if name.starts_with(&wanted) {
            1
        } else {
            2
        };
        found.push((rank, name.len(), hit));
    }
    found.sort_by_key(|f| (f.0, f.1));
    found
        .into_iter()
        .map(|(_, _, hit)| hit)
        .take(MAX_HITS)
        .collect()
}

/// The season names of `search_all_seasons.php`, in the order sent.
pub(super) fn season_names(answer: &Value) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for season in list(answer, "seasons") {
        if let Some(name) = text(season, "strSeason")
            && !out.iter().any(|o| o == name)
        {
            out.push(name.to_owned());
        }
    }
    out
}

/// The season number of a season name: the year it starts (`2014-2015` is 2014, `2020` is 2020).
pub(super) fn season_number(name: &str) -> Option<u32> {
    let year = year_prefix(name);
    (year != 0).then_some(u32::from(year))
}

/// The show item of a league. `seasons` are the names `search_all_seasons.php` lists.
pub(super) fn build_show(
    league: &Value,
    seasons: &[String],
    options: &RequestOptions,
) -> Option<Item> {
    let ids = league_ids(league)?;
    let name = text(league, "strLeague")?;
    let mut item = Item::show(name);
    item.core.ids = ids;
    item.core.original_title = name.to_owned();
    item.core.year = text(league, "intFormedYear").map_or(0, year_prefix);
    item.core.plot = description(league, options);
    let artwork = [
        ("strPoster", MediaFileType::Poster),
        ("strFanart1", MediaFileType::Fanart),
        ("strBanner", MediaFileType::Banner),
        ("strLogo", MediaFileType::Clearlogo),
        ("strBadge", MediaFileType::Thumb),
        ("strTrophy", MediaFileType::Clearart),
    ];
    for (field, kind) in artwork {
        if let Some(link) = url(league, field) {
            item.core.artwork_urls.insert(kind, link);
        }
    }
    let first_aired = text(league, "dateFirstEvent").and_then(parse_date);
    let mut genres: Vec<Genre> = Vec::new();
    if let Some(genre) = text(league, "strSport").and_then(Genre::parse) {
        genres.push(genre);
    }
    let season_texts: Vec<SeasonText> = seasons
        .iter()
        .filter_map(|label| {
            Some(SeasonText {
                group: EpisodeGroup::aired(),
                season: season_number(label)?,
                name: label.clone(),
                overview: String::new(),
            })
        })
        .collect();
    if let ItemDetail::Show(fields) = &mut item.detail {
        fields.first_aired = first_aired;
        if fields.first_aired.is_some() && item.core.year == 0 {
            item.core.year = year_of(first_aired);
        }
        fields.status = if text(league, "strCurrentSeason").is_some() {
            AiredStatus::Continuing
        } else {
            AiredStatus::Unknown
        };
        fields.country = text(league, "strCountry")
            .map(|c| country_codes(&[c.to_owned()]).join(""))
            .unwrap_or_default();
        fields.genres = genres;
        fields.episode_group = EpisodeGroup::aired();
        fields.episode_groups = vec![EpisodeGroup::aired()];
        fields.season_texts = season_texts;
    }
    // Further fanart images are extra fanart.
    let extra: Vec<String> = ["strFanart2", "strFanart3", "strFanart4"]
        .iter()
        .filter_map(|f| url(league, f))
        .collect();
    if let ItemDetail::Show(fields) = &mut item.detail {
        fields.extra_fanart_urls = extra;
    }
    Some(item)
}

/// The episodes of one `eventsseason.php` answer. Events are numbered in the order they were
/// played: by date, then time, then the service's own ID.
pub(super) fn season_episodes(answer: &Value, season_name: &str) -> Vec<Item> {
    let Some(season) = season_number(season_name) else {
        return Vec::new();
    };
    let mut events: Vec<(Option<CalendarDate>, String, u64, &Value)> = list(answer, "events")
        .iter()
        .filter(|e| text(e, "idEvent").is_some() && text(e, "strEvent").is_some())
        .map(|e| {
            (
                text(e, "dateEvent").and_then(parse_date),
                text(e, "strTime").unwrap_or("").to_owned(),
                text(e, "idEvent")
                    .and_then(|id| id.parse().ok())
                    .unwrap_or(0),
                e,
            )
        })
        .collect();
    // Events without a date go last.
    events.sort_by(|a, b| (a.0.is_none(), a.0, &a.1, a.2).cmp(&(b.0.is_none(), b.0, &b.1, b.2)));
    events
        .into_iter()
        .enumerate()
        .map(|(index, (date, _, _, event))| {
            let number = u32::try_from(index + 1).unwrap_or(u32::MAX);
            episode(event, date, season, number)
        })
        .collect()
}

fn episode(event: &Value, date: Option<CalendarDate>, season: u32, number: u32) -> Item {
    let mut item = Item::new(ItemDetail::Episode(EpisodeFields::default()));
    item.core.title = text(event, "strEvent").unwrap_or("").to_owned();
    set_id(
        &mut item.core.ids,
        ID_KEY,
        text(event, "idEvent").unwrap_or(""),
    );
    item.core.year = year_of(date);
    item.core.plot = text(event, "strDescriptionEN").unwrap_or("").to_owned();
    if let Some(link) = url(event, "strThumb").or_else(|| url(event, "strPoster")) {
        item.core.artwork_urls.insert(MediaFileType::Thumb, link);
    }
    if let ItemDetail::Episode(fields) = &mut item.detail {
        fields.first_aired = date;
        fields
            .numbers
            .set(EpisodeNumber::new(EpisodeGroup::aired(), season, number));
    }
    item
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(text: &str) -> Value {
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn blank_and_null_values_are_absent() {
        let v = json(r#"{"a":"","b":null,"c":" x ","d":3}"#);
        assert_eq!(text(&v, "a"), None);
        assert_eq!(text(&v, "b"), None);
        assert_eq!(text(&v, "c"), Some("x"));
        assert_eq!(text(&v, "d"), None);
        assert!(first(&json(r#"{"leagues":null}"#), "leagues").is_none());
    }

    #[test]
    fn seasons_are_numbered_by_their_first_year() {
        assert_eq!(season_number("2014-2015"), Some(2014));
        assert_eq!(season_number("2020"), Some(2020));
        assert_eq!(season_number("Playoffs"), None);
    }

    #[test]
    fn descriptions_follow_the_language_then_the_fallback() {
        let league = json(
            r#"{"idLeague":"1","strLeague":"L","strDescriptionEN":"English","strDescriptionDE":"Deutsch","strDescriptionFR":""}"#,
        );
        let with = |language: &str, fallback: &str| RequestOptions {
            language: language.to_owned(),
            fallback_language: fallback.to_owned(),
            ..RequestOptions::default()
        };
        assert_eq!(description(&league, &with("de", "en")), "Deutsch");
        assert_eq!(description(&league, &with("fr", "en")), "English");
        assert_eq!(description(&league, &with("tr", "de")), "Deutsch");
        let only_dutch =
            json(r#"{"idLeague":"1","strLeague":"L","strDescriptionNL":"Nederlands"}"#);
        assert_eq!(
            description(&only_dutch, &RequestOptions::default()),
            "Nederlands"
        );
    }

    #[test]
    fn search_needs_every_word_and_ranks_the_exact_name_first() {
        let answer = json(
            r#"{"leagues":[
                {"idLeague":"2","strLeague":"English Premier League Women","strSport":"Soccer"},
                {"idLeague":"1","strLeague":"English Premier League","strLeagueAlternate":"EPL","strSport":"Soccer"},
                {"idLeague":"3","strLeague":"Scottish Premier League","strSport":"Soccer"},
                {"idLeague":"","strLeague":"No id"}]}"#,
        );
        let ids = |q: &str| -> Vec<String> {
            search_leagues(&answer, q)
                .iter()
                .map(|h| h.ids.get(ID_KEY).unwrap().to_owned())
                .collect()
        };
        assert_eq!(ids("english premier league"), ["1", "2"]);
        assert_eq!(ids("Premier"), ["1", "3", "2"]);
        assert_eq!(ids("epl"), ["1"]);
        assert!(ids("bundesliga").is_empty());
        assert!(ids("   ").is_empty());
        assert!(search_leagues(&json(r#"{"leagues":null}"#), "x").is_empty());
    }

    #[test]
    fn events_are_numbered_in_the_order_they_were_played() {
        let answer = json(
            r#"{"events":[
                {"idEvent":"12","strEvent":"C","dateEvent":"2014-08-23","strTime":"15:00:00"},
                {"idEvent":"11","strEvent":"B","dateEvent":"2014-08-16","strTime":"17:30:00"},
                {"idEvent":"10","strEvent":"A","dateEvent":"2014-08-16","strTime":"12:45:00"},
                {"idEvent":"13","strEvent":"Undated","dateEvent":null},
                {"strEvent":"No id"}]}"#,
        );
        let items = season_episodes(&answer, "2014-2015");
        let titles: Vec<_> = items.iter().map(|i| i.core.title.as_str()).collect();
        assert_eq!(titles, ["A", "B", "C", "Undated"]);
        let ItemDetail::Episode(first) = &items[0].detail else {
            panic!("episode")
        };
        let number = first.numbers.get(&EpisodeGroup::aired()).unwrap();
        assert_eq!((number.season, number.episode), (2014, 1));
        assert_eq!(items[0].core.ids.get(ID_KEY), Some("10"));
        assert!(season_episodes(&answer, "Playoffs").is_empty());
    }
}
