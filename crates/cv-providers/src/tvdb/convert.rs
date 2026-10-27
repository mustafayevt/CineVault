//! TheTVDB responses to CineVault items, artwork candidates and trailers.

use cv_domain::{
    AiredStatus, CalendarDate, Certification, Credit, CreditKind, EpisodeFields, EpisodeGroup,
    EpisodeNumber, EpisodeOrdering, Genre, Item, ItemDetail, MediaFileType, Person, ProviderIds,
    SeasonFields, Trailer, id_key,
};

use super::api::{
    Artwork, ArtworkType, Character, EpisodeRecord, Extended, RemoteId, SearchItem, SeasonExtended,
    SeasonRef, Text, Translations,
};
use super::language::{app_language, country_alpha2, same_country};
use crate::model::{ArtworkCandidate, MediaKind, RequestOptions, SearchHit};
use crate::shared::original_language_code;
use crate::tmdb::certification_token;

/// Where relative image paths are served from.
const ARTWORK_HOST: &str = "https://artworks.thetvdb.com";

/// The provider key trailers are attributed to.
const TRAILER_PROVIDER: &str = "tvdb";

/// Everything a conversion needs besides the response.
pub(super) struct Context<'a> {
    pub options: &'a RequestOptions,
    /// TheTVDB language codes in the order they are tried.
    pub chain: &'a [String],
}

// --- small helpers --------------------------------------------------------------------------

/// Parses `yyyy-MM-dd`, ignoring a time part. Blank and invalid dates are `None`.
pub(super) fn parse_date(text: &str) -> Option<CalendarDate> {
    text.trim().get(..10)?.parse().ok()
}

fn year_of(date: Option<CalendarDate>) -> u16 {
    date.and_then(|d| u16::try_from(d.year()).ok()).unwrap_or(0)
}

/// A four-digit year from `2008` or a date.
fn year_from(text: &str) -> u16 {
    text.trim()
        .get(..4)
        .and_then(|y| y.parse::<u16>().ok())
        .filter(|y| *y > 0)
        .unwrap_or(0)
}

/// An absolute image URL, or `None` for a blank path.
pub(super) fn image_url(path: &str) -> Option<String> {
    let path = path.trim();
    if path.is_empty() {
        return None;
    }
    if path.starts_with("http://") || path.starts_with("https://") {
        return Some(path.to_owned());
    }
    Some(format!("{ARTWORK_HOST}/{}", path.trim_start_matches('/')))
}

fn set_id(ids: &mut ProviderIds, key: &str, value: impl AsRef<str>) {
    let value = value.as_ref().trim();
    if !value.is_empty() && value != "0" {
        ids.set(key, value);
    }
}

/// IMDb, TMDB, Wikidata and TVmaze IDs from the remote IDs (types 2, 12, 18 and 19).
fn remote_ids(ids: &mut ProviderIds, remote: &[RemoteId]) {
    for entry in remote {
        let key = match entry.kind {
            2 => id_key::IMDB,
            12 => id_key::TMDB,
            18 => id_key::WIKIDATA,
            19 => id_key::TVMAZE,
            _ => continue,
        };
        // An IMDb remote ID that is not a title (a person's `nm...`) is not ours.
        if key == id_key::IMDB && !entry.id.as_str().starts_with("tt") {
            continue;
        }
        if !ids.contains(key) {
            set_id(ids, key, entry.id.as_str());
        }
    }
}

// --- translations ---------------------------------------------------------------------------

fn name_in(translations: &Translations, language: &str) -> Option<String> {
    translations
        .name_translations
        .iter()
        .find(|t| {
            t.language.text().trim().eq_ignore_ascii_case(language)
                && !t.name.text().trim().is_empty()
        })
        .map(|t| t.name.text().trim().to_owned())
}

fn overview_in(translations: &Translations, language: &str) -> Option<String> {
    translations
        .overview_translations
        .iter()
        .find(|t| {
            t.language.text().trim().eq_ignore_ascii_case(language)
                && !t.overview.text().trim().is_empty()
        })
        .map(|t| t.overview.text().trim().to_owned())
}

/// A title's texts after the language fallback.
pub(super) struct Texts {
    pub title: String,
    pub english_title: String,
    pub overview: String,
}

/// The title and plot in the first language of the chain that has them, else the record's own
/// (original-language) name and plot.
pub(super) fn resolve_texts(ext: &Extended, chain: &[String]) -> Texts {
    let empty = Translations::default();
    let translations = ext.translations.as_ref().unwrap_or(&empty);
    let title = chain
        .iter()
        .find_map(|l| name_in(translations, l))
        .unwrap_or_else(|| ext.name.text().trim().to_owned());
    let overview = chain
        .iter()
        .find_map(|l| overview_in(translations, l))
        .unwrap_or_else(|| ext.overview.text().trim().to_owned());
    Texts {
        english_title: name_in(translations, "eng").unwrap_or_default(),
        title,
        overview,
    }
}

// --- people, genres, companies, certifications ---------------------------------------------

fn person(character: &Character) -> Person {
    let mut person = Person::new(character.person_name.text().trim());
    if let Some(id) = character.people_id.as_u64() {
        person.ids.set(id_key::TVDB, id.to_string());
        person.profile_url = format!("https://thetvdb.com/dereferrer/people/{id}");
    }
    let image = if character.person_img_url.text().trim().is_empty() {
        character.image.text()
    } else {
        character.person_img_url.text()
    };
    person.thumb_url = image_url(image).unwrap_or_default();
    person
}

fn credit_kind(character: &Character) -> Option<CreditKind> {
    let by_name = character.people_type.text().trim().to_ascii_lowercase();
    match by_name.as_str() {
        "actor" => return Some(CreditKind::Actor),
        "director" => return Some(CreditKind::Director),
        "writer" => return Some(CreditKind::Writer),
        "producer" | "executive producer" => return Some(CreditKind::Producer),
        "" => {}
        _ => return None,
    }
    match character.kind {
        Some(1) => Some(CreditKind::Director),
        Some(2) => Some(CreditKind::Writer),
        Some(3) => Some(CreditKind::Actor),
        _ => None,
    }
}

/// Cast in billing order, then directors, writers and producers. A person listed twice for the
/// same part is kept once.
pub(super) fn convert_credits(characters: &[Character]) -> Vec<Credit> {
    let mut entries: Vec<(CreditKind, &Character)> = characters
        .iter()
        .filter(|c| !c.person_name.text().trim().is_empty())
        .filter_map(|c| credit_kind(c).map(|k| (k, c)))
        .collect();
    // Cast first (by billing), then crew in the order TheTVDB lists it.
    entries.sort_by_key(|(kind, c)| {
        (
            u8::from(*kind != CreditKind::Actor),
            if *kind == CreditKind::Actor {
                c.sort.unwrap_or(u32::MAX)
            } else {
                0
            },
        )
    });
    let mut out: Vec<Credit> = Vec::new();
    for (kind, character) in entries {
        let role = if kind == CreditKind::Actor {
            character.name.text().trim().to_owned()
        } else {
            character.people_type.text().trim().to_owned()
        };
        let person = person(character);
        let duplicate = out
            .iter()
            .any(|c| c.kind == kind && c.role == role && c.person.name == person.name);
        if !duplicate {
            out.push(Credit { kind, person, role });
        }
    }
    out
}

fn convert_genres(list: &[super::api::Named]) -> Vec<Genre> {
    let mut out: Vec<Genre> = Vec::new();
    for genre in list {
        if let Some(parsed) = Genre::parse(genre.name.text())
            && !out.contains(&parsed)
        {
            out.push(parsed);
        }
    }
    out
}

/// Studio, production and network names. Series list their companies in one array tagged with a
/// type; movies group them by role.
fn company_names(value: &serde_json::Value) -> Vec<String> {
    const WANTED: &[&str] = &["network", "studio", "production company", "production"];
    let mut out: Vec<String> = Vec::new();
    let mut push = |name: Option<&str>| {
        if let Some(name) = name.map(str::trim).filter(|n| !n.is_empty())
            && !out.iter().any(|o| o == name)
        {
            out.push(name.to_owned());
        }
    };
    match value {
        serde_json::Value::Array(list) => {
            for company in list {
                let kind = company
                    .pointer("/companyType/companyTypeName")
                    .and_then(|v| v.as_str())
                    .map(str::to_ascii_lowercase);
                if kind.as_deref().is_none_or(|k| WANTED.contains(&k)) {
                    push(company.get("name").and_then(|v| v.as_str()));
                }
            }
        }
        serde_json::Value::Object(groups) => {
            for role in ["studio", "production", "network"] {
                if let Some(serde_json::Value::Array(list)) = groups.get(role) {
                    for company in list {
                        push(company.get("name").and_then(|v| v.as_str()));
                    }
                }
            }
        }
        _ => {}
    }
    out
}

fn certification(ext: &Extended, country: &str) -> Certification {
    ext.content_ratings
        .iter()
        .find(|r| same_country(r.country.text(), country) && !r.name.text().trim().is_empty())
        .map_or_else(Certification::default, |r| {
            certification_token(country, r.name.text())
        })
}

// --- orderings ------------------------------------------------------------------------------

/// The names of the alternate orderings, indexed like [`ALTERNATE_SLUGS`].
const ALTERNATE_NAMES: &[&str] = &[
    "Alternate Order",
    "Regional Order",
    "Alternate DVD Order",
    "Alternate Two Order",
];
const ALTERNATE_SLUGS: &[&str] = &["alternate", "regional", "altdvd", "alttwo"];

/// The ordering a TheTVDB season type slug stands for. `official` and `default` are both the
/// aired order; the other named types are alternate orderings known by their name.
pub(super) fn group_for_slug(slug: &str) -> Option<EpisodeGroup> {
    let slug = slug.trim().to_ascii_lowercase();
    Some(match slug.as_str() {
        "official" | "default" => EpisodeGroup::aired(),
        "dvd" => EpisodeGroup::dvd(),
        "absolute" => EpisodeGroup::absolute(),
        other => {
            let at = ALTERNATE_SLUGS.iter().position(|s| *s == other)?;
            EpisodeGroup::new(EpisodeOrdering::Alternate, ALTERNATE_NAMES[at])
        }
    })
}

/// The season type slug to request for an ordering, or `None` when TheTVDB has no such type.
pub(super) fn slug_for_group(group: &EpisodeGroup) -> Option<&'static str> {
    match group.ordering {
        EpisodeOrdering::Aired | EpisodeOrdering::Display => Some("official"),
        EpisodeOrdering::Dvd => Some("dvd"),
        EpisodeOrdering::Absolute => Some("absolute"),
        EpisodeOrdering::Alternate => {
            let name = group.name.trim();
            if name.is_empty() {
                return Some("alternate");
            }
            ALTERNATE_NAMES
                .iter()
                .zip(ALTERNATE_SLUGS)
                .find(|(n, s)| n.eq_ignore_ascii_case(name) || s.eq_ignore_ascii_case(name))
                .map(|(_, s)| *s)
        }
    }
}

/// The orderings a series offers: aired first, then each other season type it has.
pub(super) fn groups_of(seasons: &[SeasonRef]) -> Vec<EpisodeGroup> {
    let mut groups = vec![EpisodeGroup::aired()];
    for season in seasons {
        let Some(group) = season
            .kind
            .as_ref()
            .and_then(|k| group_for_slug(k.slug.text()))
        else {
            continue;
        };
        if !groups.contains(&group) {
            groups.push(group);
        }
    }
    groups
}

/// The season of the aired order with this number, as `seasons/{id}` knows it.
pub(super) fn aired_season(seasons: &[SeasonRef], number: u32) -> Option<&SeasonRef> {
    seasons.iter().find(|s| {
        s.number == number
            && s.kind
                .as_ref()
                .and_then(|k| group_for_slug(k.slug.text()))
                .is_some_and(|g| g == EpisodeGroup::aired())
    })
}

// --- artwork --------------------------------------------------------------------------------

/// Which artwork type a TheTVDB type name is, for a record type (`series`, `season`, `movie`,
/// `episode`). Types CineVault has no use for (cinemagraphs, cast photos) are `None`.
fn media_type(record: &str, name: &str) -> Option<MediaFileType> {
    let mut key: String = name
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>()
        .to_ascii_lowercase();
    let mut season = record.eq_ignore_ascii_case("season");
    if let Some(rest) = key.strip_prefix("season") {
        season = true;
        key = rest.to_owned();
    }
    Some(match (key.as_str(), season) {
        ("poster", false) => MediaFileType::Poster,
        ("poster", true) => MediaFileType::SeasonPoster,
        ("background" | "fanart", false) => MediaFileType::Fanart,
        ("background" | "fanart", true) => MediaFileType::SeasonFanart,
        ("banner", false) => MediaFileType::Banner,
        ("banner", true) => MediaFileType::SeasonBanner,
        ("icon", false) => MediaFileType::Squareart,
        ("clearart", _) => MediaFileType::Clearart,
        ("clearlogo", _) => MediaFileType::Clearlogo,
        ("screencap", _) => MediaFileType::Thumb,
        _ => return None,
    })
}

/// How well an image's language fits: its position in the chain, then images without text,
/// then other languages.
fn language_rank(language: &Option<String>, chain: &[String]) -> usize {
    match language.as_deref().map(str::trim).filter(|l| !l.is_empty()) {
        None => chain.len(),
        Some(l) => chain
            .iter()
            .position(|c| c.eq_ignore_ascii_case(l))
            .unwrap_or(chain.len() + 1),
    }
}

/// Candidates for `artworks`, best first within each artwork type: the preferred language, then
/// the fallback and English, then images without text, then the rest; each group by score.
pub(super) fn artwork_candidates(
    artworks: &[Artwork],
    types: &[ArtworkType],
    chain: &[String],
) -> Vec<ArtworkCandidate> {
    let mut ranked: Vec<(usize, ArtworkCandidate)> = Vec::new();
    for artwork in artworks {
        let Some(info) = types.iter().find(|t| t.id == artwork.kind) else {
            continue;
        };
        let Some(kind) = media_type(info.record_type.text(), info.name.text()) else {
            continue;
        };
        let Some(url) = image_url(artwork.image.text()) else {
            continue;
        };
        let (width, height) = if artwork.width > 0 && artwork.height > 0 {
            (artwork.width, artwork.height)
        } else {
            (info.width, info.height)
        };
        let language = artwork
            .language
            .as_deref()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(app_language);
        ranked.push((
            language_rank(&artwork.language, chain),
            ArtworkCandidate {
                kind,
                url,
                language,
                width,
                height,
                score: artwork.score.unwrap_or(0.0) as f32,
                popularity: crate::model::Popularity::Unknown,
            },
        ));
    }
    ranked.sort_by(|(rank_a, a), (rank_b, b)| {
        a.kind
            .cmp(&b.kind)
            .then(rank_a.cmp(rank_b))
            .then_with(|| b.score.total_cmp(&a.score))
    });
    ranked.into_iter().map(|(_, candidate)| candidate).collect()
}

/// The first (best) candidate of a kind.
fn best(candidates: &[ArtworkCandidate], kind: MediaFileType) -> Option<String> {
    candidates
        .iter()
        .find(|c| c.kind == kind)
        .map(|c| c.url.clone())
}

/// Fills `artwork_urls` from the main image and the best candidate of each artwork type.
fn fill_artwork_urls(item: &mut Item, main_image: &str, candidates: &[ArtworkCandidate]) {
    let urls = &mut item.core.artwork_urls;
    if let Some(url) = best(candidates, MediaFileType::Poster).or_else(|| image_url(main_image)) {
        urls.insert(MediaFileType::Poster, url);
    }
    for kind in [
        MediaFileType::Fanart,
        MediaFileType::Banner,
        MediaFileType::Clearlogo,
        MediaFileType::Clearart,
        MediaFileType::Squareart,
    ] {
        if let Some(url) = best(candidates, kind) {
            urls.insert(kind, url);
        }
    }
}

// --- trailers -------------------------------------------------------------------------------

/// Trailers with a web address, each once, the preferred language first.
pub(super) fn convert_trailers(
    records: &[super::api::TrailerRecord],
    chain: &[String],
) -> Vec<Trailer> {
    let mut sorted: Vec<&super::api::TrailerRecord> = records.iter().collect();
    sorted.sort_by_key(|t| language_rank(&t.language, chain));
    let mut out: Vec<Trailer> = Vec::new();
    for record in sorted {
        let url = record.url.text().trim();
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            continue;
        }
        if out.iter().any(|t| t.url == url) {
            continue;
        }
        let id = if record.id.is_blank() {
            url.to_owned()
        } else {
            record.id.as_str().to_owned()
        };
        out.push(Trailer {
            id,
            name: record.name.text().trim().to_owned(),
            url: url.to_owned(),
            quality: String::new(),
            language: record
                .language
                .as_deref()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(app_language)
                .unwrap_or_default(),
            provider: TRAILER_PROVIDER.to_owned(),
            in_nfo: false,
            date: None,
            scraped_by: TRAILER_PROVIDER.to_owned(),
            length_secs: 0,
        });
    }
    out
}

// --- movies and shows -----------------------------------------------------------------------

fn tv_status(status: &str) -> AiredStatus {
    match status.trim().to_ascii_lowercase().as_str() {
        "continuing" | "upcoming" => AiredStatus::Continuing,
        "ended" => AiredStatus::Ended,
        "canceled" | "cancelled" => AiredStatus::Canceled,
        _ => AiredStatus::Unknown,
    }
}

/// The release date for `country`: the earliest release there, else the worldwide first release.
fn movie_release_date(ext: &Extended, country: &str) -> Option<CalendarDate> {
    let local = ext
        .releases
        .iter()
        .filter(|r| same_country(r.country.text(), country))
        .filter_map(|r| parse_date(r.date.text()))
        .min();
    local
        .or_else(|| {
            ext.releases
                .iter()
                .filter_map(|r| parse_date(r.date.text()))
                .min()
        })
        .or_else(|| parse_date(ext.first_aired.text()))
}

pub(super) fn movie_item(
    ctx: &Context<'_>,
    ext: &Extended,
    candidates: &[ArtworkCandidate],
) -> Item {
    let texts = resolve_texts(ext, ctx.chain);
    let release = movie_release_date(ext, &ctx.options.country);

    let mut item = Item::movie(texts.title);
    let core = &mut item.core;
    set_id(&mut core.ids, id_key::TVDB, ext.id.as_str());
    remote_ids(&mut core.ids, &ext.remote_ids);
    core.original_title = ext.name.text().trim().to_owned();
    core.original_language = original_language_code(ext.original_language.text());
    core.year = if release.is_some() {
        year_of(release)
    } else {
        year_from(ext.year.as_str())
    };
    core.plot = texts.overview;
    core.production_companies = company_names(&ext.companies);
    core.credits = convert_credits(&ext.characters);
    fill_artwork_urls(&mut item, ext.image.text(), candidates);

    if let ItemDetail::Movie(movie) = &mut item.detail {
        movie.english_title = texts.english_title;
        movie.runtime_minutes = ext.runtime.or(ext.average_runtime).unwrap_or(0);
        movie.release_date = release;
        movie.certification = certification(ext, &ctx.options.country);
        movie.genres = convert_genres(&ext.genres);
        movie.spoken_languages = ext
            .audio_languages
            .iter()
            .map(|l| app_language(l))
            .filter(|l| !l.is_empty())
            .fold(Vec::new(), |mut all, l| {
                if !all.contains(&l) {
                    all.push(l);
                }
                all
            });
        let country = ext.original_country.text().trim();
        if !country.is_empty() {
            movie.countries = vec![country_alpha2(country)];
        }
        movie.trailers = convert_trailers(&ext.trailers, ctx.chain);
    }
    item
}

pub(super) fn show_item(
    ctx: &Context<'_>,
    ext: &Extended,
    candidates: &[ArtworkCandidate],
) -> Item {
    let texts = resolve_texts(ext, ctx.chain);
    let first_aired = parse_date(ext.first_aired.text());
    let status = tv_status(ext.status.as_ref().map_or("", |s| s.name.text()));

    let mut item = Item::show(texts.title);
    let core = &mut item.core;
    set_id(&mut core.ids, id_key::TVDB, ext.id.as_str());
    remote_ids(&mut core.ids, &ext.remote_ids);
    core.original_title = ext.name.text().trim().to_owned();
    core.original_language = original_language_code(ext.original_language.text());
    core.year = if first_aired.is_some() {
        year_of(first_aired)
    } else {
        year_from(ext.year.as_str())
    };
    core.plot = texts.overview;
    core.production_companies = company_names(&ext.companies);
    core.credits = convert_credits(&ext.characters);
    fill_artwork_urls(&mut item, ext.image.text(), candidates);

    if let ItemDetail::Show(show) = &mut item.detail {
        show.english_title = texts.english_title;
        show.runtime_minutes = ext.average_runtime.or(ext.runtime).unwrap_or(0);
        show.first_aired = first_aired;
        show.status = status;
        if matches!(status, AiredStatus::Ended | AiredStatus::Canceled) {
            show.year_end = year_of(parse_date(ext.last_aired.text()));
        }
        let country = ext.original_country.text().trim();
        if !country.is_empty() {
            show.country = country_alpha2(country);
        }
        show.certification = certification(ext, &ctx.options.country);
        show.episode_group = EpisodeGroup::aired();
        show.episode_groups = groups_of(&ext.seasons);
        show.genres = convert_genres(&ext.genres);
        show.trailers = convert_trailers(&ext.trailers, ctx.chain);
        show.extra_fanart_urls = candidates
            .iter()
            .filter(|c| c.kind == MediaFileType::Fanart)
            .skip(1)
            .map(|c| c.url.clone())
            .collect();
    }
    item
}

// --- seasons and episodes -------------------------------------------------------------------

pub(super) fn season_item(
    ctx: &Context<'_>,
    season: &SeasonExtended,
    candidates: &[ArtworkCandidate],
) -> Item {
    let mut item = Item::new(ItemDetail::Season(SeasonFields {
        season: season.number,
    }));
    let core = &mut item.core;
    let empty = Translations::default();
    let translations = season.translations.as_ref().unwrap_or(&empty);
    core.title = ctx
        .chain
        .iter()
        .find_map(|l| name_in(translations, l))
        .unwrap_or_else(|| season.name.text().trim().to_owned());
    core.plot = ctx
        .chain
        .iter()
        .find_map(|l| overview_in(translations, l))
        .unwrap_or_default();
    set_id(&mut core.ids, id_key::TVDB, season.id.as_str());
    core.year = year_from(season.year.as_str());
    if let Some(url) =
        best(candidates, MediaFileType::SeasonPoster).or_else(|| image_url(season.image.text()))
    {
        core.artwork_urls.insert(MediaFileType::SeasonPoster, url);
    }
    if let Some(url) = best(candidates, MediaFileType::SeasonFanart) {
        core.artwork_urls.insert(MediaFileType::SeasonFanart, url);
    }
    if let Some(url) = best(candidates, MediaFileType::SeasonBanner) {
        core.artwork_urls.insert(MediaFileType::SeasonBanner, url);
    }
    item
}

/// An episode without numbers; the caller adds the numbers of the orderings it knows.
pub(super) fn episode_item(record: &EpisodeRecord) -> Item {
    let mut item = Item::new(ItemDetail::Episode(EpisodeFields::default()));
    let core = &mut item.core;
    core.title = record.name.text().trim().to_owned();
    core.plot = record.overview.text().trim().to_owned();
    set_id(&mut core.ids, id_key::TVDB, record.id.as_str());
    let aired = parse_date(record.aired.text());
    core.year = year_of(aired);
    if let Some(url) = image_url(record.image.text()) {
        core.artwork_urls.insert(MediaFileType::Thumb, url);
    }
    if let ItemDetail::Episode(fields) = &mut item.detail {
        fields.first_aired = aired;
        fields.runtime_minutes = record.runtime.unwrap_or(0);
    }
    item
}

/// Records the episode's season and number in `group`.
pub(super) fn set_number(item: &mut Item, group: &EpisodeGroup, season: u32, episode: u32) {
    if let ItemDetail::Episode(fields) = &mut item.detail {
        fields
            .numbers
            .set(EpisodeNumber::new(group.clone(), season, episode));
    }
}

// --- search results -------------------------------------------------------------------------

fn translated(
    map: &std::collections::BTreeMap<String, String>,
    chain: &[String],
) -> Option<String> {
    chain.iter().find_map(|language| {
        map.iter()
            .find(|(k, v)| k.eq_ignore_ascii_case(language) && !v.trim().is_empty())
            .map(|(_, v)| v.trim().to_owned())
    })
}

/// A search result as a hit; `None` for one without an ID or a name.
pub(super) fn hit_from_search(item: &SearchItem, chain: &[String]) -> Option<SearchHit> {
    let tvdb = item.tvdb_id.as_u64()?;
    let original = item.name.text().trim().to_owned();
    let title = translated(&item.translations, chain).unwrap_or_else(|| original.clone());
    if title.is_empty() {
        return None;
    }
    let mut ids = ProviderIds::new();
    ids.set(id_key::TVDB, tvdb.to_string());
    remote_ids(&mut ids, &item.remote_ids);
    let year = match year_from(item.year.as_str()) {
        0 => year_from(item.first_air_time.text()),
        y => y,
    };
    Some(SearchHit {
        provider: super::PROVIDER_ID.to_owned(),
        ids,
        title,
        original_title: original,
        year,
        overview: translated(&item.overviews, chain)
            .unwrap_or_else(|| item.overview.text().trim().to_owned()),
        poster_url: image_url(item.image_url.text()),
        exact_id: false,
        popularity: 0.0,
        votes: 0,
    })
}

/// A looked-up record as a hit that settles the search.
pub(super) fn hit_from_extended(ctx: &Context<'_>, ext: &Extended, kind: MediaKind) -> SearchHit {
    let texts = resolve_texts(ext, ctx.chain);
    let mut ids = ProviderIds::new();
    set_id(&mut ids, id_key::TVDB, ext.id.as_str());
    remote_ids(&mut ids, &ext.remote_ids);
    let year = match kind {
        MediaKind::Movie => {
            let release = movie_release_date(ext, &ctx.options.country);
            if release.is_some() {
                year_of(release)
            } else {
                year_from(ext.year.as_str())
            }
        }
        MediaKind::Show => match year_of(parse_date(ext.first_aired.text())) {
            0 => year_from(ext.year.as_str()),
            y => y,
        },
    };
    SearchHit {
        provider: super::PROVIDER_ID.to_owned(),
        ids,
        title: texts.title,
        original_title: ext.name.text().trim().to_owned(),
        year,
        overview: texts.overview,
        poster_url: image_url(ext.image.text()),
        exact_id: true,
        popularity: 0.0,
        votes: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::super::api::Flex;
    use super::*;

    fn flex(text: &str) -> Flex {
        Flex(text.to_owned())
    }

    #[test]
    fn season_type_slugs_map_to_orderings_and_back() {
        assert_eq!(group_for_slug("official"), Some(EpisodeGroup::aired()));
        assert_eq!(group_for_slug("default"), Some(EpisodeGroup::aired()));
        assert_eq!(group_for_slug("dvd"), Some(EpisodeGroup::dvd()));
        assert_eq!(group_for_slug("absolute"), Some(EpisodeGroup::absolute()));
        let regional = group_for_slug("regional").unwrap();
        assert_eq!(regional.ordering, EpisodeOrdering::Alternate);
        assert_eq!(regional.name, "Regional Order");
        assert_eq!(group_for_slug("something-else"), None);

        for slug in [
            "official",
            "dvd",
            "absolute",
            "alternate",
            "regional",
            "altdvd",
            "alttwo",
        ] {
            let group = group_for_slug(slug).unwrap();
            assert_eq!(slug_for_group(&group), Some(slug), "{slug}");
        }
        // "default" is the aired order too, requested as `official`.
        assert_eq!(slug_for_group(&EpisodeGroup::aired()), Some("official"));
        // Names are matched ignoring case and accept the slug.
        let by_slug = EpisodeGroup::new(EpisodeOrdering::Alternate, "ALTDVD");
        assert_eq!(slug_for_group(&by_slug), Some("altdvd"));
        let unknown = EpisodeGroup::new(EpisodeOrdering::Alternate, "Story Arcs");
        assert_eq!(slug_for_group(&unknown), None);
        let bare = EpisodeGroup::new(EpisodeOrdering::Alternate, "");
        assert_eq!(slug_for_group(&bare), Some("alternate"));
    }

    #[test]
    fn artwork_type_names_map_per_record_type() {
        assert_eq!(media_type("series", "Poster"), Some(MediaFileType::Poster));
        assert_eq!(
            media_type("movie", "Background"),
            Some(MediaFileType::Fanart)
        );
        assert_eq!(
            media_type("season", "Poster"),
            Some(MediaFileType::SeasonPoster)
        );
        assert_eq!(
            media_type("season", "Banner"),
            Some(MediaFileType::SeasonBanner)
        );
        assert_eq!(
            media_type("series", "ClearLogo"),
            Some(MediaFileType::Clearlogo)
        );
        assert_eq!(media_type("series", "Icon"), Some(MediaFileType::Squareart));
        assert_eq!(
            media_type("episode", "Screencap"),
            Some(MediaFileType::Thumb)
        );
        assert_eq!(media_type("series", "Cinemagraph"), None);
        assert_eq!(media_type("person", "Actor"), None);
    }

    #[test]
    fn image_paths_become_absolute_urls() {
        assert_eq!(
            image_url("https://artworks.thetvdb.com/banners/a.jpg").as_deref(),
            Some("https://artworks.thetvdb.com/banners/a.jpg")
        );
        assert_eq!(
            image_url("/banners/a.jpg").as_deref(),
            Some("https://artworks.thetvdb.com/banners/a.jpg")
        );
        assert_eq!(image_url("  "), None);
    }

    #[test]
    fn show_status_values() {
        assert_eq!(tv_status("Continuing"), AiredStatus::Continuing);
        assert_eq!(tv_status("Upcoming"), AiredStatus::Continuing);
        assert_eq!(tv_status("Ended"), AiredStatus::Ended);
        assert_eq!(tv_status("odd"), AiredStatus::Unknown);
    }

    #[test]
    fn dates_ignore_the_time_part() {
        assert_eq!(parse_date("2008-01-20"), CalendarDate::new(2008, 1, 20));
        assert_eq!(
            parse_date("2008-01-20T01:00:00Z"),
            CalendarDate::new(2008, 1, 20)
        );
        assert_eq!(parse_date(""), None);
        assert_eq!(year_from("2008"), 2008);
        assert_eq!(year_from(""), 0);
    }

    #[test]
    fn an_imdb_remote_id_must_be_a_title() {
        let mut ids = ProviderIds::new();
        remote_ids(
            &mut ids,
            &[
                RemoteId {
                    id: flex("nm0000123"),
                    kind: 2,
                },
                RemoteId {
                    id: flex("tt0903747"),
                    kind: 2,
                },
                RemoteId {
                    id: flex("1396"),
                    kind: 12,
                },
                RemoteId {
                    id: flex("whatever"),
                    kind: 3,
                },
            ],
        );
        assert_eq!(ids.get(id_key::IMDB), Some("tt0903747"));
        assert_eq!(ids.get(id_key::TMDB), Some("1396"));
        assert_eq!(ids.len(), 2);
    }
}
