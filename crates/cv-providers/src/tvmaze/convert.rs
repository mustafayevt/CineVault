//! TVmaze answers to CineVault items, hits and episodes.

use cv_domain::{
    AiredStatus, Credit, CreditKind, EpisodeFields, EpisodeGroup, EpisodeNumber, EpisodeOrdering,
    Genre, Item, ItemDetail, MediaFileType, ProviderIds, Rating, SeasonText, id_key,
};

use super::PROVIDER_ID;
use super::api::{
    AlternateEpisode, AlternateList, CastEntry, CrewEntry, Embedded, Episode, Person, Show, Text,
};
use crate::model::SearchHit;
use crate::shared::{html_to_text, parse_date, set_id, year_of};

/// The rating source key TVmaze ratings are stored under.
pub const RATING_ID: &str = "tvmaze";

pub(super) fn show_ids(show: &Show) -> ProviderIds {
    let mut ids = ProviderIds::new();
    if let Some(id) = show.id {
        set_id(&mut ids, id_key::TVMAZE, id.to_string());
    }
    if let Some(externals) = &show.externals {
        if let Some(imdb) = crate::shared::imdb_title_id(externals.imdb.text()) {
            set_id(&mut ids, id_key::IMDB, imdb);
        }
        if let Some(tvdb) = externals.thetvdb {
            set_id(&mut ids, id_key::TVDB, tvdb.to_string());
        }
    }
    ids
}

fn status_of(status: &str) -> AiredStatus {
    match status.trim().to_ascii_lowercase().as_str() {
        "running" => AiredStatus::Continuing,
        "ended" => AiredStatus::Ended,
        _ => AiredStatus::Unknown,
    }
}

fn person_credit(kind: CreditKind, person: &Person, role: &str) -> Option<Credit> {
    let name = person.name.text();
    if name.is_empty() {
        return None;
    }
    let mut credit = Credit::new(kind, name, role);
    if let Some(id) = person.id {
        credit.person.ids.set(id_key::TVMAZE, id.to_string());
    }
    credit.person.thumb_url = person
        .image
        .as_ref()
        .and_then(|i| i.best())
        .unwrap_or("")
        .to_owned();
    credit.person.profile_url = person.url.text().to_owned();
    Some(credit)
}

fn crew_kind(kind: &str) -> CreditKind {
    let kind = kind.trim().to_ascii_lowercase();
    if kind.contains("producer") {
        CreditKind::Producer
    } else if kind.contains("director") {
        CreditKind::Director
    } else if kind.contains("writer") {
        CreditKind::Writer
    } else if kind.contains("compos") || kind.contains("music") {
        CreditKind::Composer
    } else if kind.contains("editor") {
        CreditKind::Editor
    } else if kind.contains("cinemat") || kind.contains("camera") {
        CreditKind::Camera
    } else {
        CreditKind::Other
    }
}

/// Cast in TVmaze's order, then the crew, each person once per kind and role.
fn credits(cast: &[CastEntry], crew: &[CrewEntry]) -> Vec<Credit> {
    let mut out: Vec<Credit> = Vec::new();
    let mut add = |credit: Credit| {
        let same = |c: &Credit| {
            c.kind == credit.kind
                && c.role == credit.role
                && match (
                    c.person.ids.get(id_key::TVMAZE),
                    credit.person.ids.get(id_key::TVMAZE),
                ) {
                    (Some(a), Some(b)) => a == b,
                    _ => c.person.name == credit.person.name,
                }
        };
        if !out.iter().any(same) {
            out.push(credit);
        }
    };
    for entry in cast {
        if let Some(credit) = person_credit(
            CreditKind::Actor,
            &entry.person,
            entry.character.name.text(),
        ) {
            add(credit);
        }
    }
    for entry in crew {
        let role = entry.kind.text();
        if let Some(credit) = person_credit(crew_kind(role), &entry.person, role) {
            add(credit);
        }
    }
    out
}

/// Artwork by type: the entry marked `main` wins, else the first.
fn artwork(embedded: &Embedded, show: &Show, item: &mut Item) {
    let pick = |kind: &str| -> Option<String> {
        let mut matching = embedded
            .images
            .iter()
            .filter(|i| i.kind.text().eq_ignore_ascii_case(kind) && i.url().is_some());
        let all: Vec<_> = matching.by_ref().collect();
        all.iter()
            .find(|i| i.main)
            .or_else(|| all.first())
            .and_then(|i| i.url())
            .map(str::to_owned)
    };
    for (kind, file) in [
        ("poster", MediaFileType::Poster),
        ("background", MediaFileType::Fanart),
        ("banner", MediaFileType::Banner),
        ("typography", MediaFileType::Clearlogo),
    ] {
        if let Some(url) = pick(kind) {
            item.core.artwork_urls.insert(file, url);
        }
    }
    if !item.core.artwork_urls.contains_key(&MediaFileType::Poster)
        && let Some(url) = show.image.as_ref().and_then(|i| i.best())
    {
        item.core
            .artwork_urls
            .insert(MediaFileType::Poster, url.to_owned());
    }
    // Backgrounds beyond the chosen one are extra fanart.
    let chosen = item.core.artwork_urls.get(&MediaFileType::Fanart).cloned();
    if let ItemDetail::Show(fields) = &mut item.detail {
        fields.extra_fanart_urls = embedded
            .images
            .iter()
            .filter(|i| i.kind.text().eq_ignore_ascii_case("background"))
            .filter_map(|i| i.url())
            .filter(|u| Some(*u) != chosen.as_deref())
            .map(str::to_owned)
            .collect();
    }
}

fn rating_of(average: Option<f64>) -> Option<Rating> {
    average
        .filter(|v| *v > 0.0)
        .map(|v| Rating::new(RATING_ID, v as f32, 0, 10))
}

/// The show item. `groups` are the episode orderings the show offers besides the aired one.
pub(super) fn build_show(show: &Show, groups: &[EpisodeGroup]) -> Item {
    let name = show.name.text();
    let mut item = Item::show(name);
    let premiered = parse_date(show.premiered.text());
    let ended = parse_date(show.ended.text());
    {
        let core = &mut item.core;
        core.ids = show_ids(show);
        core.original_title = name.to_owned();
        core.year = year_of(premiered);
        core.plot = html_to_text(show.summary.text());
        if let Some(rating) = rating_of(show.rating.as_ref().and_then(|r| r.average)) {
            core.ratings.insert(rating);
        }
    }
    let network = show
        .network
        .as_ref()
        .filter(|n| !n.name.text().is_empty())
        .or(show.web_channel.as_ref());
    if let Some(network) = network
        && !network.name.text().is_empty()
    {
        item.core.production_companies = vec![network.name.text().to_owned()];
    }
    let country = network
        .and_then(|n| n.country.as_ref())
        .map(|c| c.code.text().to_ascii_uppercase())
        .unwrap_or_default();
    let mut genres: Vec<Genre> = Vec::new();
    for name in &show.genres {
        if let Some(genre) = Genre::parse(name)
            && !genres.contains(&genre)
        {
            genres.push(genre);
        }
    }
    let seasons: Vec<SeasonText> = show
        .embedded
        .as_ref()
        .map(|e| {
            e.seasons
                .iter()
                .filter_map(|s| {
                    let season = s.number?;
                    let name = s.name.text().to_owned();
                    let overview = html_to_text(s.summary.text());
                    (!name.is_empty() || !overview.is_empty()).then_some(SeasonText {
                        group: EpisodeGroup::aired(),
                        season,
                        name,
                        overview,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    if let ItemDetail::Show(fields) = &mut item.detail {
        fields.english_title = if show.language.text().eq_ignore_ascii_case("english") {
            name.to_owned()
        } else {
            String::new()
        };
        fields.runtime_minutes = show.runtime.or(show.average_runtime).unwrap_or(0);
        fields.first_aired = premiered;
        fields.year_end = year_of(ended);
        fields.status = status_of(show.status.text());
        fields.country = country;
        fields.genres = genres;
        fields.episode_group = EpisodeGroup::aired();
        fields.episode_groups = std::iter::once(EpisodeGroup::aired())
            .chain(groups.iter().cloned())
            .collect();
        fields.season_texts = seasons;
    }
    if let Some(embedded) = &show.embedded {
        item.core.credits = credits(&embedded.cast, &embedded.crew);
        artwork(embedded, show, &mut item);
    } else if let Some(url) = show.image.as_ref().and_then(|i| i.best()) {
        item.core
            .artwork_urls
            .insert(MediaFileType::Poster, url.to_owned());
    }
    item
}

/// The hit for a search or lookup result; `None` without a name or an ID.
pub(super) fn build_hit(show: &Show, exact_id: bool) -> Option<SearchHit> {
    let name = show.name.text();
    let ids = show_ids(show);
    if name.is_empty() || !ids.contains(id_key::TVMAZE) {
        return None;
    }
    Some(SearchHit {
        provider: PROVIDER_ID.to_owned(),
        ids,
        title: name.to_owned(),
        original_title: name.to_owned(),
        year: year_of(parse_date(show.premiered.text())),
        overview: html_to_text(show.summary.text()),
        poster_url: show
            .image
            .as_ref()
            .and_then(|i| i.best())
            .map(str::to_owned),
        exact_id,
        popularity: 0.0,
        votes: 0,
    })
}

// --- orderings -------------------------------------------------------------------------------

/// The alternate lists as episode groups. The first DVD release is the DVD ordering; every
/// other list is an alternate ordering named as TVmaze names it.
pub(super) fn groups_of(lists: &[AlternateList]) -> Vec<(EpisodeGroup, u64)> {
    let mut out: Vec<(EpisodeGroup, u64)> = Vec::new();
    let mut has_dvd = false;
    for list in lists {
        let Some(id) = list.id else {
            continue;
        };
        let name = match list.name.text() {
            "" => format!("Alternate {id}"),
            name => name.to_owned(),
        };
        let group = if list.dvd_release && !has_dvd {
            has_dvd = true;
            EpisodeGroup::new(EpisodeOrdering::Dvd, name)
        } else {
            EpisodeGroup::new(EpisodeOrdering::Alternate, name)
        };
        // A DVD group is requested without a name (`EpisodeGroup::dvd()`), so it is stored
        // that way.
        let group = if group.ordering == EpisodeOrdering::Dvd {
            EpisodeGroup::dvd()
        } else {
            group
        };
        if !out.iter().any(|(g, _)| *g == group) {
            out.push((group, id));
        }
    }
    out
}

fn episode_base(episode: &Episode) -> Item {
    let mut item = Item::new(ItemDetail::Episode(EpisodeFields::default()));
    item.core.title = episode.name.text().to_owned();
    item.core.plot = html_to_text(episode.summary.text());
    if let Some(id) = episode.id {
        set_id(&mut item.core.ids, id_key::TVMAZE, id.to_string());
    }
    let aired = parse_date(episode.airdate.text());
    item.core.year = year_of(aired);
    if let Some(url) = episode.image.as_ref().and_then(|i| i.best()) {
        item.core
            .artwork_urls
            .insert(MediaFileType::Thumb, url.to_owned());
    }
    if let Some(rating) = rating_of(episode.rating.as_ref().and_then(|r| r.average)) {
        item.core.ratings.insert(rating);
    }
    if let ItemDetail::Episode(fields) = &mut item.detail {
        fields.first_aired = aired;
        fields.runtime_minutes = episode.runtime.unwrap_or(0);
    }
    item
}

fn set_number(item: &mut Item, group: EpisodeGroup, season: u32, episode: u32) {
    if let ItemDetail::Episode(fields) = &mut item.detail {
        fields
            .numbers
            .set(EpisodeNumber::new(group, season, episode));
    }
}

/// The episodes of `/shows/{id}/episodes?specials=1` in the aired order. Specials have no number
/// on TVmaze; they go to season 0 and are numbered in the order they come.
pub(super) fn aired_episodes(episodes: &[Episode]) -> Vec<Item> {
    let mut special = 0u32;
    let mut rows: Vec<(u32, u32, Item)> = Vec::new();
    for episode in episodes {
        let mut item = episode_base(episode);
        let (season, number) = match (episode.season, episode.number) {
            (Some(season), Some(number)) => (season, number),
            (Some(_) | None, None) => {
                special += 1;
                (0, special)
            }
            (None, Some(_)) => continue,
        };
        set_number(&mut item, EpisodeGroup::aired(), season, number);
        rows.push((season, number, item));
    }
    rows.sort_by_key(|(season, number, _)| (*season, *number));
    rows.into_iter().map(|(_, _, item)| item).collect()
}

/// The episodes of an alternate list, numbered in `group`, with their aired numbers when the
/// episode has them.
pub(super) fn alternate_episodes(entries: &[AlternateEpisode], group: &EpisodeGroup) -> Vec<Item> {
    let mut rows: Vec<(u32, u32, Item)> = Vec::new();
    for entry in entries {
        let (Some(season), Some(number)) = (entry.season, entry.number) else {
            continue;
        };
        let Some(episode) = entry.embedded.as_ref().and_then(|e| e.episodes.first()) else {
            continue;
        };
        let mut item = episode_base(episode);
        if let (Some(s), Some(n)) = (episode.season, episode.number) {
            set_number(&mut item, EpisodeGroup::aired(), s, n);
        }
        set_number(&mut item, group.clone(), season, number);
        rows.push((season, number, item));
    }
    rows.sort_by_key(|(season, number, _)| (*season, *number));
    rows.into_iter().map(|(_, _, item)| item).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn show(json: &str) -> Show {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn a_bare_show_converts_with_empty_fields() {
        let item = build_show(&show(r#"{"id":169,"name":"Breaking Bad"}"#), &[]);
        assert_eq!(item.core.title, "Breaking Bad");
        assert_eq!(item.core.ids.get(id_key::TVMAZE), Some("169"));
        assert_eq!(item.core.year, 0);
        assert!(item.core.credits.is_empty());
        assert!(item.core.artwork_urls.is_empty());
    }

    #[test]
    fn nulls_do_not_fail_a_show() {
        let s = show(
            r#"{"id":1,"name":"X","genres":null,"externals":null,"network":null,"image":null,"_embedded":null}"#,
        );
        assert!(build_hit(&s, false).is_some());
        assert!(build_hit(&show(r#"{"name":"No id"}"#), false).is_none());
    }

    #[test]
    fn statuses() {
        assert_eq!(status_of("Running"), AiredStatus::Continuing);
        assert_eq!(status_of("Ended"), AiredStatus::Ended);
        assert_eq!(status_of("To Be Determined"), AiredStatus::Unknown);
    }

    #[test]
    fn crew_types_map_to_kinds() {
        assert_eq!(crew_kind("Executive Producer"), CreditKind::Producer);
        assert_eq!(crew_kind("Creator"), CreditKind::Other);
        assert_eq!(crew_kind("Writer"), CreditKind::Writer);
    }

    #[test]
    fn the_first_dvd_release_is_the_dvd_ordering() {
        let lists: Vec<AlternateList> = serde_json::from_str(
            r#"[{"id":1,"name":"DVD order","dvd_release":true},
                {"id":2,"name":"Streaming order","dvd_release":false},
                {"id":3,"name":"Second DVD","dvd_release":true},
                {"id":4,"name":"Streaming order","dvd_release":false},
                {"name":"no id"}]"#,
        )
        .unwrap();
        let groups = groups_of(&lists);
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0], (EpisodeGroup::dvd(), 1));
        assert_eq!(
            groups[1],
            (
                EpisodeGroup::new(EpisodeOrdering::Alternate, "Streaming order"),
                2
            )
        );
        assert_eq!(
            groups[2],
            (
                EpisodeGroup::new(EpisodeOrdering::Alternate, "Second DVD"),
                3
            )
        );
    }

    #[test]
    fn specials_are_numbered_in_season_zero() {
        let episodes: Vec<Episode> = serde_json::from_str(
            r#"[{"id":1,"name":"Pilot","season":1,"number":1},
                {"id":2,"name":"Recap","season":1,"number":null},
                {"id":3,"name":"Behind the scenes","season":2,"number":null},
                {"id":4,"name":"Second","season":1,"number":2}]"#,
        )
        .unwrap();
        let items = aired_episodes(&episodes);
        let numbers: Vec<(u32, u32)> = items
            .iter()
            .map(|i| match &i.detail {
                ItemDetail::Episode(f) => {
                    let n = f.numbers.get(&EpisodeGroup::aired()).unwrap();
                    (n.season, n.episode)
                }
                _ => panic!("episode"),
            })
            .collect();
        assert_eq!(numbers, [(0, 1), (0, 2), (1, 1), (1, 2)]);
    }
}
