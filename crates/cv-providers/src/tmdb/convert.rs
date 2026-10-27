//! TMDB responses to CineVault items.

use cv_domain::{
    AiredStatus, CalendarDate, Certification, Credit, CreditKind, EpisodeFields, EpisodeGroup,
    EpisodeNumber, EpisodeOrdering, Genre, Item, ItemDetail, MediaFileType, MissingMovie, Person,
    ProviderIds, Rating, SeasonFields, SeasonText, id_key,
};

use super::api::{
    CastMember, Credits, CrewMember, Episode, EpisodeGroupRef, ExternalIds, Genre as TmdbGenre,
    MovieDetails, SearchResult, SeasonDetails, Text, TvDetails, Videos,
};
use super::language::{MainTexts, resolve_texts};
use crate::model::RequestOptions;
use crate::shared::original_language_code;

/// The rating source key TMDB ratings are stored under.
pub const RATING_ID: &str = "tmdb";

/// Everything a conversion needs besides the response.
pub(super) struct Context<'a> {
    /// Image base URL ending in `/`.
    pub image_base: &'a str,
    pub options: &'a RequestOptions,
}

impl Context<'_> {
    /// A full image URL at `size` (`original`, `w500`, ...), or `None` for a missing path.
    pub fn image(&self, size: &str, path: &Option<String>) -> Option<String> {
        image_url(self.image_base, size, path.as_deref()?)
    }
}

/// `base + size + path`, or `None` for a blank path.
pub(super) fn image_url(base: &str, size: &str, path: &str) -> Option<String> {
    let path = path.trim();
    if path.is_empty() {
        return None;
    }
    let base = base.trim_end_matches('/');
    let path = path.trim_start_matches('/');
    Some(format!("{base}/{size}/{path}"))
}

/// Parses `yyyy-MM-dd`, ignoring a time part. Blank and invalid dates are `None`.
pub(super) fn parse_date(text: &str) -> Option<CalendarDate> {
    text.trim().get(..10)?.parse().ok()
}

fn year_of(date: Option<CalendarDate>) -> u16 {
    date.and_then(|d| u16::try_from(d.year()).ok()).unwrap_or(0)
}

/// Sets `key` to `value` when TMDB sent one.
fn set_id(ids: &mut ProviderIds, key: &str, value: impl AsRef<str>) {
    if !value.as_ref().trim().is_empty() {
        ids.set(key, value);
    }
}

fn external_ids(ids: &mut ProviderIds, external: &ExternalIds, own_imdb: &Option<String>) {
    set_id(ids, id_key::IMDB, own_imdb.text());
    set_id(ids, id_key::IMDB, external.imdb_id.text());
    set_id(ids, id_key::TVDB, external.tvdb_id.to_text());
    set_id(ids, id_key::WIKIDATA, external.wikidata_id.text());
}

// --- genres ---------------------------------------------------------------------------------

/// Genres by TMDB's numeric id, so the localised names the API returns do not matter.
fn genres_for_id(id: u32) -> Option<&'static [&'static str]> {
    Some(match id {
        28 => &["Action"],
        12 => &["Adventure"],
        16 => &["Animation"],
        35 => &["Comedy"],
        80 => &["Crime"],
        99 => &["Documentary"],
        18 => &["Drama"],
        10751 => &["Family"],
        14 => &["Fantasy"],
        36 => &["History"],
        27 => &["Horror"],
        10402 => &["Music"],
        9648 => &["Mystery"],
        10749 => &["Romance"],
        878 => &["Science Fiction"],
        10770 => &["TV Movie"],
        53 => &["Thriller"],
        10752 => &["War"],
        37 => &["Western"],
        10759 => &["Action", "Adventure"],
        10763 => &["News"],
        10764 => &["Reality TV"],
        10765 => &["Science Fiction", "Fantasy"],
        10766 => &["Soap"],
        10767 => &["Talk Show"],
        10768 => &["War"],
        _ => return None,
    })
}

fn convert_genres(list: &[TmdbGenre], adult: bool) -> Vec<Genre> {
    let mut out: Vec<Genre> = Vec::new();
    let mut push = |genre: Genre| {
        if !out.contains(&genre) {
            out.push(genre);
        }
    };
    for genre in list {
        match genres_for_id(genre.id) {
            Some(names) => names
                .iter()
                .filter_map(|n| Genre::parse(n))
                .for_each(&mut push),
            None => {
                if let Some(parsed) = Genre::parse(genre.name.text()) {
                    push(parsed);
                }
            }
        }
    }
    if adult {
        push(Genre::Erotic);
    }
    out
}

// --- certifications -------------------------------------------------------------------------

/// The canonical token for a rating in a country: `US` + `PG-13` becomes `US_PG13`, `DE` + `12`
/// becomes `DE_FSK12`. Blank ratings give an empty certification. The rule covers the common
/// schemes; a rating outside them still gets a stable `<COUNTRY>_<RATING>` token.
pub(crate) fn certification_token(country: &str, rating: &str) -> Certification {
    let rating = rating.trim();
    let country = country.trim().to_ascii_uppercase();
    if rating.is_empty() || country.is_empty() {
        return Certification::default();
    }
    let upper = rating.to_uppercase();
    if matches!(upper.as_str(), "NR" | "NOT RATED" | "NOT_RATED") {
        return Certification("NOT_RATED".to_owned());
    }
    if upper == "UNRATED" {
        return Certification("UNRATED".to_owned());
    }
    let compact: String = upper.chars().filter(|c| c.is_alphanumeric()).collect();
    if compact.is_empty() {
        return Certification::default();
    }
    let token = if let Some(rest) = upper.strip_prefix("TV-") {
        let rest: String = rest.chars().filter(|c| c.is_alphanumeric()).collect();
        format!("TV_{rest}")
    } else if country == "DE" && compact.chars().all(|c| c.is_ascii_digit()) {
        format!("FSK{compact}")
    } else {
        compact
    };
    Certification(format!("{country}_{token}"))
}

// --- people ---------------------------------------------------------------------------------

fn person(ctx: &Context<'_>, id: u64, name: &Option<String>, profile: &Option<String>) -> Person {
    let mut person = Person::new(name.text().trim());
    if id != 0 {
        person.ids.set(id_key::TMDB, id.to_string());
        person.profile_url = format!("https://www.themoviedb.org/person/{id}");
    }
    person.thumb_url = ctx.image("h632", profile).unwrap_or_default();
    person
}

fn crew_kind(member: &CrewMember) -> Option<CreditKind> {
    match member.job.text().trim() {
        "Director" => Some(CreditKind::Director),
        "Screenplay" | "Writer" | "Story" => Some(CreditKind::Writer),
        "Producer" => Some(CreditKind::Producer),
        "Editor" => Some(CreditKind::Editor),
        "Original Music Composer" => Some(CreditKind::Composer),
        "Director of Photography" => Some(CreditKind::Camera),
        _ => None,
    }
}

/// Cast in billing order, then the crew roles that are kept: director, writers, producers,
/// editor, composer and director of photography. A person listed twice for the same job is
/// kept once, and one credited for several jobs of a kind (Story, Screenplay) once with the jobs
/// joined.
pub(super) fn convert_credits(
    ctx: &Context<'_>,
    credits: &Credits,
    guests: &[CastMember],
) -> Vec<Credit> {
    let mut out = Vec::new();
    let mut cast: Vec<&CastMember> = credits.cast.iter().collect();
    cast.sort_by_key(|c| c.order.unwrap_or(u32::MAX));
    for member in cast {
        if member.name.text().trim().is_empty() {
            continue;
        }
        out.push(Credit {
            kind: CreditKind::Actor,
            person: person(ctx, member.id, &member.name, &member.profile_path),
            role: member.character.text().trim().to_owned(),
        });
    }
    let mut guests: Vec<&CastMember> = guests.iter().collect();
    guests.sort_by_key(|c| c.order.unwrap_or(u32::MAX));
    for member in guests {
        if member.name.text().trim().is_empty() {
            continue;
        }
        out.push(Credit {
            kind: CreditKind::Guest,
            person: person(ctx, member.id, &member.name, &member.profile_path),
            role: member.character.text().trim().to_owned(),
        });
    }
    push_crew(ctx, &credits.crew, &mut out);
    // A person credited for the story and the screenplay is one writer, not two.
    cv_domain::merge_crew(&out)
}

fn push_crew(ctx: &Context<'_>, crew: &[CrewMember], out: &mut Vec<Credit>) {
    for member in crew {
        let Some(kind) = crew_kind(member) else {
            continue;
        };
        if member.name.text().trim().is_empty() {
            continue;
        }
        let role = member.job.text().trim().to_owned();
        let duplicate = out.iter().any(|c| {
            c.kind == kind
                && c.role == role
                && (if member.id != 0 {
                    c.person.ids.get(id_key::TMDB) == Some(member.id.to_string().as_str())
                } else {
                    c.person.name == member.name.text().trim()
                })
        });
        if duplicate {
            continue;
        }
        out.push(Credit {
            kind,
            person: person(ctx, member.id, &member.name, &member.profile_path),
            role,
        });
    }
}

// --- movies ---------------------------------------------------------------------------------

/// The release date: the film's original (worldwide) release date, or, when `local` is on, the
/// earliest theatrical or later release in `country` (a type 1 premiere is ignored) with the
/// worldwide date as the fallback. The year always comes from the worldwide date (see
/// [`movie_item`]).
fn movie_release_date(details: &MovieDetails, country: &str, local: bool) -> Option<CalendarDate> {
    let worldwide = parse_date(details.release_date.text());
    if !local && worldwide.is_some() {
        return worldwide;
    }
    let local = details
        .release_dates
        .results
        .iter()
        .filter(|r| r.iso_3166_1.text().eq_ignore_ascii_case(country))
        .flat_map(|r| r.release_dates.iter())
        .filter(|r| r.kind >= 2)
        .filter_map(|r| parse_date(r.release_date.text()))
        .min();
    local.or(worldwide)
}

/// The certification for `country`, preferring a theatrical release's rating.
fn movie_certification(details: &MovieDetails, country: &str) -> Certification {
    let mut entries: Vec<_> = details
        .release_dates
        .results
        .iter()
        .filter(|r| r.iso_3166_1.text().eq_ignore_ascii_case(country))
        .flat_map(|r| r.release_dates.iter())
        .filter(|r| !r.certification.text().trim().is_empty())
        .collect();
    entries.sort_by_key(|r| match r.kind {
        3 => 0,
        2 => 1,
        _ => 2,
    });
    entries.first().map_or_else(Certification::default, |r| {
        certification_token(country, r.certification.text())
    })
}

/// The trailers of a details response, the preferred one (the request language, else the first)
/// marked as the NFO trailer and listed first.
fn scraped_trailers(videos: &Videos, options: &RequestOptions) -> Vec<cv_domain::Trailer> {
    let mut trailers = super::media::trailers(&[videos]);
    let prefs = cv_domain::TrailerPreferences {
        language: options.language.clone(),
        ..cv_domain::TrailerPreferences::default()
    };
    cv_domain::mark_preferred(&mut trailers, &prefs);
    trailers
}

fn tmdb_rating(average: Option<f32>, votes: Option<u32>) -> Option<Rating> {
    let average = average?;
    (average > 0.0).then(|| Rating::new(RATING_ID, average, votes.unwrap_or(0), 10))
}

pub(super) fn movie_item(ctx: &Context<'_>, details: &MovieDetails) -> Item {
    let options = ctx.options;
    let texts = resolve_texts(
        &MainTexts {
            title: details.title.text(),
            original_title: details.original_title.text(),
            overview: details.overview.text(),
            tagline: details.tagline.text(),
            original_language: details.original_language.text(),
        },
        &details.translations.translations,
        &options.language,
        &options.fallback_language,
    );
    let release = movie_release_date(details, &options.country, options.local_release_date);

    let mut item = Item::movie(texts.title);
    let core = &mut item.core;
    set_id(&mut core.ids, id_key::TMDB, details.id.to_string());
    external_ids(&mut core.ids, &details.external_ids, &details.imdb_id);
    if let Some(collection) = &details.belongs_to_collection {
        set_id(&mut core.ids, id_key::TMDB_SET, collection.id.to_string());
    }
    core.original_title = details.original_title.text().trim().to_owned();
    core.original_language = original_language_code(details.original_language.text());
    // The year is the film's own (its primary release), not the year it reached the configured
    // country: Oldboy is 2003 everywhere, though it opened in the US in 2005. The country's date
    // stays the release date.
    core.year = year_of(parse_date(details.release_date.text()).or(release));
    core.plot = texts.overview;
    core.production_companies = details
        .production_companies
        .iter()
        .map(|c| c.name.text().trim().to_owned())
        .filter(|n| !n.is_empty())
        .collect();
    if let Some(rating) = tmdb_rating(details.vote_average, details.vote_count) {
        core.ratings.insert(rating);
    }
    for keyword in &details.keywords.keywords {
        core.tags.add(keyword.name.text());
    }
    if let Some(url) = ctx.image("original", &details.poster_path) {
        core.artwork_urls.insert(MediaFileType::Poster, url);
    }
    if let Some(url) = ctx.image("original", &details.backdrop_path) {
        core.artwork_urls.insert(MediaFileType::Fanart, url);
    }
    core.credits = convert_credits(ctx, &details.credits, &[]);

    if let ItemDetail::Movie(movie) = &mut item.detail {
        movie.english_title = texts.english_title;
        movie.tagline = texts.tagline;
        movie.runtime_minutes = details.runtime.unwrap_or(0);
        movie.release_date = release;
        movie.certification = movie_certification(details, &options.country);
        movie.genres = convert_genres(&details.genres, details.adult);
        movie.spoken_languages = details
            .spoken_languages
            .iter()
            .map(|l| l.iso_639_1.text().trim().to_owned())
            .filter(|l| !l.is_empty())
            .collect();
        movie.trailers = scraped_trailers(&details.videos, options);
        movie.countries = details
            .production_countries
            .iter()
            .map(|c| c.iso_3166_1.text().trim().to_owned())
            .filter(|c| !c.is_empty())
            .collect();
    }
    item
}

// --- shows ----------------------------------------------------------------------------------

fn tv_status(status: &str) -> AiredStatus {
    match status.trim().to_ascii_lowercase().as_str() {
        "returning series" | "in production" | "planned" | "pilot" => AiredStatus::Continuing,
        "ended" => AiredStatus::Ended,
        "canceled" | "cancelled" => AiredStatus::Canceled,
        _ => AiredStatus::Unknown,
    }
}

/// Maps a show's TMDB episode groups to orderings. The first DVD group is the show's DVD
/// order and the first absolute group its absolute order; every other group is an alternate
/// ordering known by its name. Each ordering is paired with the group's TMDB id.
pub(super) fn map_episode_groups(refs: &[EpisodeGroupRef]) -> Vec<(EpisodeGroup, String)> {
    let mut dvd_taken = false;
    let mut absolute_taken = false;
    let mut out = Vec::new();
    for group in refs {
        let id = group.id.text().trim();
        if id.is_empty() {
            continue;
        }
        let ordering = match group.kind {
            3 if !dvd_taken => {
                dvd_taken = true;
                EpisodeGroup::dvd()
            }
            2 if !absolute_taken => {
                absolute_taken = true;
                EpisodeGroup::absolute()
            }
            _ => EpisodeGroup::new(EpisodeOrdering::Alternate, group.name.text().trim()),
        };
        out.push((ordering, id.to_owned()));
    }
    out
}

pub(super) fn show_item(ctx: &Context<'_>, details: &TvDetails) -> Item {
    let options = ctx.options;
    let texts = resolve_texts(
        &MainTexts {
            title: details.name.text(),
            original_title: details.original_name.text(),
            overview: details.overview.text(),
            tagline: details.tagline.text(),
            original_language: details.original_language.text(),
        },
        &details.translations.translations,
        &options.language,
        &options.fallback_language,
    );
    let first_aired = parse_date(details.first_air_date.text());
    let status = tv_status(details.status.text());

    let mut item = Item::show(texts.title);
    let core = &mut item.core;
    set_id(&mut core.ids, id_key::TMDB, details.id.to_string());
    external_ids(&mut core.ids, &details.external_ids, &None);
    core.original_title = details.original_name.text().trim().to_owned();
    core.original_language = original_language_code(details.original_language.text());
    core.year = year_of(first_aired);
    core.plot = texts.overview;
    let studios = if details.production_companies.is_empty() {
        &details.networks
    } else {
        &details.production_companies
    };
    core.production_companies = studios
        .iter()
        .map(|c| c.name.text().trim().to_owned())
        .filter(|n| !n.is_empty())
        .collect();
    if let Some(rating) = tmdb_rating(details.vote_average, details.vote_count) {
        core.ratings.insert(rating);
    }
    for keyword in &details.keywords.results {
        core.tags.add(keyword.name.text());
    }
    if let Some(url) = ctx.image("original", &details.poster_path) {
        core.artwork_urls.insert(MediaFileType::Poster, url);
    }
    if let Some(url) = ctx.image("original", &details.backdrop_path) {
        core.artwork_urls.insert(MediaFileType::Fanart, url);
    }
    core.credits = convert_credits(ctx, &details.credits, &[]);

    if let ItemDetail::Show(show) = &mut item.detail {
        show.english_title = texts.english_title;
        show.tagline = texts.tagline;
        show.runtime_minutes = details
            .episode_run_time
            .iter()
            .copied()
            .find(|m| *m > 0)
            .unwrap_or(0);
        show.first_aired = first_aired;
        show.status = status;
        if matches!(status, AiredStatus::Ended | AiredStatus::Canceled) {
            show.year_end = year_of(parse_date(details.last_air_date.text()));
        }
        show.country = details.origin_country.first().cloned().unwrap_or_default();
        show.trailers = scraped_trailers(&details.videos, options);
        show.certification = details
            .content_ratings
            .results
            .iter()
            .find(|r| r.iso_3166_1.text().eq_ignore_ascii_case(&options.country))
            .map_or_else(Certification::default, |r| {
                certification_token(&options.country, r.rating.text())
            });
        show.episode_group = EpisodeGroup::aired();
        show.episode_groups = std::iter::once(EpisodeGroup::aired())
            .chain(
                map_episode_groups(&details.episode_groups.results)
                    .into_iter()
                    .map(|(group, _)| group),
            )
            .collect();
        show.genres = convert_genres(&details.genres, false);
        show.season_texts = details
            .seasons
            .iter()
            .map(|s| SeasonText {
                group: EpisodeGroup::aired(),
                season: s.season_number,
                name: s.name.text().trim().to_owned(),
                overview: s.overview.text().trim().to_owned(),
            })
            .collect();
    }
    item
}

// --- seasons and episodes -------------------------------------------------------------------

pub(super) fn season_item(ctx: &Context<'_>, details: &SeasonDetails) -> Item {
    let mut item = Item::new(ItemDetail::Season(SeasonFields {
        season: details.season_number,
    }));
    let core = &mut item.core;
    core.title = details.name.text().trim().to_owned();
    core.plot = details.overview.text().trim().to_owned();
    set_id(&mut core.ids, id_key::TMDB, details.id.to_string());
    set_id(
        &mut core.ids,
        id_key::TVDB,
        details.external_ids.tvdb_id.to_text(),
    );
    core.year = year_of(parse_date(details.air_date.text()));
    if let Some(url) = ctx.image("original", &details.poster_path) {
        core.artwork_urls.insert(MediaFileType::Poster, url);
    }
    item
}

/// An episode. `group` adds the episode's number in a non-aired ordering as
/// `(group, season, episode)`.
pub(super) fn episode_item(
    ctx: &Context<'_>,
    episode: &Episode,
    group: Option<(&EpisodeGroup, u32, u32)>,
) -> Item {
    let mut item = Item::new(ItemDetail::Episode(EpisodeFields::default()));
    let core = &mut item.core;
    core.title = episode.name.text().trim().to_owned();
    core.plot = episode.overview.text().trim().to_owned();
    set_id(&mut core.ids, id_key::TMDB, episode.id.to_string());
    external_ids(&mut core.ids, &episode.external_ids, &None);
    let aired = parse_date(episode.air_date.text());
    core.year = year_of(aired);
    if let Some(rating) = tmdb_rating(episode.vote_average, episode.vote_count) {
        core.ratings.insert(rating);
    }
    if let Some(url) = ctx.image("original", &episode.still_path) {
        core.artwork_urls.insert(MediaFileType::Thumb, url);
    }
    core.credits = convert_credits(
        ctx,
        &Credits {
            cast: Vec::new(),
            crew: Vec::new(),
        },
        &episode.guest_stars,
    );
    push_crew(ctx, &episode.crew, &mut core.credits);

    if let ItemDetail::Episode(fields) = &mut item.detail {
        fields.first_aired = aired;
        fields.runtime_minutes = episode.runtime.unwrap_or(0);
        if let (Some(season), Some(number)) = (episode.season_number, episode.episode_number) {
            fields.numbers.set(EpisodeNumber::aired(season, number));
        }
        if let Some((group, season, number)) = group {
            fields
                .numbers
                .set(EpisodeNumber::new(group.clone(), season, number));
        }
    }
    item
}

// --- search results -------------------------------------------------------------------------

pub(super) fn hit_year(result: &SearchResult) -> u16 {
    let date = if result.release_date.text().trim().is_empty() {
        result.first_air_date.text()
    } else {
        result.release_date.text()
    };
    year_of(parse_date(date))
}

pub(super) fn missing_movie(result: &SearchResult, ctx: &Context<'_>) -> MissingMovie {
    let mut ids = ProviderIds::new();
    set_id(&mut ids, id_key::TMDB, result.id.to_string());
    MissingMovie {
        title: result.title.text().trim().to_owned(),
        original_title: result.original_title.text().trim().to_owned(),
        year: hit_year(result),
        release_date: parse_date(result.release_date.text()),
        plot: result.overview.text().trim().to_owned(),
        ids,
        poster_url: ctx.image("w342", &result.poster_path).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn certification_tokens() {
        let token = |c: &str, r: &str| certification_token(c, r).0;
        assert_eq!(token("US", "PG-13"), "US_PG13");
        assert_eq!(token("US", "R"), "US_R");
        assert_eq!(token("us", "NC-17"), "US_NC17");
        assert_eq!(token("US", "TV-MA"), "US_TV_MA");
        assert_eq!(token("DE", "12"), "DE_FSK12");
        assert_eq!(token("DE", "FSK 16"), "DE_FSK16");
        assert_eq!(token("GB", "12A"), "GB_12A");
        assert_eq!(token("US", "NR"), "NOT_RATED");
        assert_eq!(token("US", "Unrated"), "UNRATED");
        assert_eq!(token("US", "  "), "");
        assert_eq!(token("", "PG"), "");
    }

    #[test]
    fn dates_ignore_the_time_part() {
        assert_eq!(
            parse_date("1995-12-15T00:00:00.000Z"),
            CalendarDate::new(1995, 12, 15)
        );
        assert_eq!(parse_date("1995-12-15"), CalendarDate::new(1995, 12, 15));
        assert_eq!(parse_date(""), None);
        assert_eq!(parse_date("1995"), None);
        assert_eq!(parse_date("1995-13-45"), None);
    }

    #[test]
    fn image_urls_join_cleanly() {
        let base = "https://image.example.test/t/p/";
        assert_eq!(
            image_url(base, "w500", "/abc.jpg").as_deref(),
            Some("https://image.example.test/t/p/w500/abc.jpg")
        );
        assert_eq!(
            image_url("https://image.example.test/t/p", "original", "abc.jpg").as_deref(),
            Some("https://image.example.test/t/p/original/abc.jpg")
        );
        assert_eq!(image_url(base, "w500", " "), None);
    }

    #[test]
    fn genres_use_ids_not_localised_names() {
        let list = vec![
            TmdbGenre {
                id: 878,
                name: Some("Bilim Kurgu".into()),
            },
            TmdbGenre {
                id: 10759,
                name: Some("Aksiyon ve Macera".into()),
            },
            TmdbGenre {
                id: 999_999,
                name: Some("Cosmic Horror".into()),
            },
        ];
        let genres = convert_genres(&list, false);
        assert_eq!(
            genres,
            vec![
                Genre::ScienceFiction,
                Genre::Action,
                Genre::Adventure,
                Genre::Other("Cosmic Horror".into())
            ]
        );
        assert!(convert_genres(&[], true).contains(&Genre::Erotic));
    }

    #[test]
    fn episode_groups_map_to_orderings() {
        let refs = vec![
            EpisodeGroupRef {
                id: Some("g1".into()),
                name: Some("DVD Order".into()),
                kind: 3,
            },
            EpisodeGroupRef {
                id: Some("g2".into()),
                name: Some("Absolute".into()),
                kind: 2,
            },
            EpisodeGroupRef {
                id: Some("g3".into()),
                name: Some("Second DVD".into()),
                kind: 3,
            },
            EpisodeGroupRef {
                id: Some("g4".into()),
                name: Some("Story Arcs".into()),
                kind: 5,
            },
        ];
        let mapped = map_episode_groups(&refs);
        assert_eq!(mapped[0].0, EpisodeGroup::dvd());
        assert_eq!(mapped[1].0, EpisodeGroup::absolute());
        assert_eq!(
            mapped[2].0,
            EpisodeGroup::new(EpisodeOrdering::Alternate, "Second DVD")
        );
        assert_eq!(
            mapped[3].0,
            EpisodeGroup::new(EpisodeOrdering::Alternate, "Story Arcs")
        );
        assert_eq!(mapped[3].1, "g4");
    }

    #[test]
    fn show_status_values() {
        assert_eq!(tv_status("Returning Series"), AiredStatus::Continuing);
        assert_eq!(tv_status("Ended"), AiredStatus::Ended);
        assert_eq!(tv_status("Canceled"), AiredStatus::Canceled);
        assert_eq!(tv_status("something"), AiredStatus::Unknown);
    }
}
