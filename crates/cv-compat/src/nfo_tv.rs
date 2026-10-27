//! TV NFO handler: writes the show, season and episode NFOs of a fixture show.
//!
//! The handler is test glue. It turns a fixture show (`fixtures/nfo/entities/show-*.json`) into
//! the plain writer models of `cv-nfo`, applies the case settings and returns every file written,
//! keyed by its path relative to the case folder:
//!
//! * `<show folder>/<tvshow naming>` for the show;
//! * `<show folder>/<season path>` for every season that has episodes;
//! * `<show folder>/<episode dir>/<video base>.nfo` for every video file; episodes that share a
//!   video file share one NFO with one block per episode.
//!
//! The case's `write` list (`show`, `seasons`, `episodes`) limits what is written. A case with an
//! `existing_nfo` and "write clean NFO" off hands the old file to the model that is written to the
//! same path, which carries its values and unmanaged elements over.
//!
//! The season names and overviews of the entity (`seasonNames`, `seasonOverviews`) are not used:
//! their plain keys name no episode group, so they do not reach the NFO.

use std::collections::BTreeMap;

use cv_domain::{
    AiredStatus, CalendarDate, Certification, CreditKind, Edition, EpisodeGroup, EpisodeNumber,
    EpisodeOrdering, Genre, MediaSource, ProviderIds, Rating, Trailer,
};
use cv_nfo::{
    AirDate, EpisodeNfo, EpisodeNfoNaming, EpisodeguideKeys, SeasonArtwork, SeasonNfo,
    SeasonNfoNaming, ShowArtwork, ShowNfo, ShowNfoNaming, TvConnector, TvNfoOptions,
    apply_old_episode_nfo, apply_old_season_nfo, apply_old_show_nfo, episode_nfo_file_name,
    render_episode_nfo, render_season_nfo, render_show_nfo, season_nfo_paths, show_nfo_file_names,
    split_episode_list, split_show_list,
};
use serde_json::Value as Json;

use crate::API_KEY_PLACEHOLDER;
use crate::fixture::Case;
use crate::nfo::{
    DEFAULT_DATE_ADDED, common_options, credits, existing_nfo, files_of, original_filename,
    read_entity, str_of, strings_of, subtitle_streams, timestamp, uint_of, video_info,
};
use crate::run::{Actual, HandlerError};

/// The renamer's "special season" option: season 0 is named `season-specials`.
const SPECIAL_SEASON_NAME: bool = true;

pub(crate) fn run_tv(case: &Case) -> Result<Actual, HandlerError> {
    let settings = TvSettings::load(case)?;
    let entity = read_entity(case)?;
    let folder = str_of(&entity, "folder").to_owned();
    let mut show = show_model(&entity);
    let old = existing_nfo(case);
    let old_file = case
        .input
        .get("existing_nfo")
        .map(|existing| str_of(existing, "file"));
    if let Some(old) = old {
        apply_old_show_nfo(&mut show, old, &settings.options.common);
    }
    let episodes = episode_models(&entity, &show, str_of(&entity, "productionCompany"));
    show.seasons = season_models(&entity, &episodes);
    show.watched = !episodes.is_empty() && episodes.iter().all(|e| e.nfo.watched);
    show.episode_air_dates = episodes
        .iter()
        .filter_map(|e| {
            e.nfo.first_aired.map(|date| AirDate {
                season: e.aired_season,
                date,
            })
        })
        .collect();
    show.file_created = episodes
        .iter()
        .filter_map(|e| e.nfo.video.created.clone())
        .min();
    show.file_modified = episodes
        .iter()
        .filter_map(|e| e.nfo.video.modified.clone())
        .min();
    show.guest_actors = episodes
        .iter()
        .flat_map(|e| e.nfo.credits.iter())
        .filter(|c| matches!(c.kind, CreditKind::Actor | CreditKind::Guest))
        .cloned()
        .collect();

    let wanted = |part: &str| {
        case.input
            .get("write")
            .and_then(Json::as_array)
            .is_none_or(|list| list.iter().any(|v| v.as_str() == Some(part)))
    };
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut put = |path: String, text: String| {
        files.insert(format!("{folder}/{path}"), text.into_bytes());
    };

    if wanted("show") {
        let text = render_show_nfo(settings.connector, &show, &settings.options);
        for name in show_nfo_file_names(&settings.show_namings) {
            put(name, text.clone());
        }
    }
    if wanted("seasons") {
        for season in &show.seasons {
            let season_folder = episodes
                .iter()
                .find(|e| e.aired_season == season.number && !e.dir.is_empty())
                .map(|e| e.dir.as_str());
            let paths = season_nfo_paths(
                &settings.season_namings,
                season.number,
                season_folder,
                SPECIAL_SEASON_NAME,
            );
            let mut season = season.clone();
            if let (Some(old), Some(file)) = (old, old_file)
                && paths.iter().any(|p| p == file)
            {
                apply_old_season_nfo(&mut season, old, &settings.options.common);
            }
            let text = render_season_nfo(&show, &season, &settings.options);
            for path in paths {
                put(path, text.clone());
            }
        }
    }
    if wanted("episodes") && !settings.episode_namings.is_empty() {
        for (file, group) in files_of_episodes(&episodes) {
            let mut blocks: Vec<EpisodeNfo> = group.iter().map(|e| e.nfo.clone()).collect();
            let Some(name) = episode_nfo_file_name(file.base()) else {
                continue;
            };
            let path = file.path(&name);
            if let (Some(old), Some(file)) = (old, old_file)
                && path == file
            {
                apply_old_episode_nfo(&mut blocks, old, &settings.options.common);
            }
            let text = render_episode_nfo(settings.connector, &blocks, &settings.options);
            put(path, text);
        }
    }
    Ok(Actual::Files(files))
}

// ---- settings ------------------------------------------------------------------------------

struct TvSettings {
    connector: TvConnector,
    show_namings: Vec<ShowNfoNaming>,
    season_namings: Vec<SeasonNfoNaming>,
    episode_namings: Vec<EpisodeNfoNaming>,
    options: TvNfoOptions,
}

impl TvSettings {
    /// The profile's `tvShow` object with the case's own `settings.tvShow` laid over it.
    fn load(case: &Case) -> Result<TvSettings, HandlerError> {
        let profile = str_of(&case.input, "profile");
        let mut merged = case
            .shared
            .get("profiles")
            .and_then(|p| p.get(profile))
            .and_then(|p| p.get("tvShow"))
            .and_then(Json::as_object)
            .cloned()
            .ok_or_else(|| HandlerError::Failed(format!("profile `{profile}` has no `tvShow`")))?;
        if let Some(over) = case
            .input
            .get("settings")
            .and_then(|s| s.get("tvShow"))
            .and_then(Json::as_object)
        {
            merged.extend(over.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
        let settings = Json::Object(merged);
        let flag =
            |key: &str, default: bool| settings.get(key).and_then(Json::as_bool).unwrap_or(default);
        let names = |key: &str| -> Option<Vec<&str>> {
            settings
                .get(key)
                .and_then(Json::as_array)
                .map(|list| list.iter().filter_map(Json::as_str).collect())
        };

        let connector_name = settings
            .get("tvShowConnector")
            .and_then(Json::as_str)
            .unwrap_or("KODI");
        let connector = TvConnector::parse(connector_name).ok_or_else(|| {
            HandlerError::Failed(format!("unknown TV show connector `{connector_name}`"))
        })?;

        let mut options = TvNfoOptions {
            common: common_options(&settings),
            ..TvNfoOptions::default()
        };
        options.write_episodeguide = flag("nfoWriteEpisodeguide", options.write_episodeguide);
        options.new_episodeguide_style = flag(
            "nfoWriteNewEpisodeguideStyle",
            options.new_episodeguide_style,
        );
        options.write_date_ended = flag("nfoWriteDateEnded", options.write_date_ended);
        options.write_all_actors = flag("nfoWriteAllActors", options.write_all_actors);
        options.episodeguide_keys = EpisodeguideKeys {
            tvdb: API_KEY_PLACEHOLDER.to_owned(),
            tmdb: API_KEY_PLACEHOLDER.to_owned(),
        };

        options.common.clean_nfo = flag("writeCleanNfo", true);
        Ok(TvSettings {
            connector,
            show_namings: names("nfoFilenames").map_or_else(
                || vec![ShowNfoNaming::TvShow],
                |list| list.into_iter().filter_map(ShowNfoNaming::parse).collect(),
            ),
            season_namings: names("seasonNfoFilenames").map_or_else(
                || vec![SeasonNfoNaming::Season],
                |list| {
                    list.into_iter()
                        .filter_map(SeasonNfoNaming::parse)
                        .collect()
                },
            ),
            episode_namings: names("episodeNfoFilenames").map_or_else(
                || vec![EpisodeNfoNaming::Filename],
                |list| {
                    list.into_iter()
                        .filter_map(EpisodeNfoNaming::parse)
                        .collect()
                },
            ),
            options,
        })
    }
}

// ---- entity to model -----------------------------------------------------------------------

fn provider_ids(json: &Json) -> ProviderIds {
    let mut ids = ProviderIds::new();
    if let Some(map) = json.get("ids").and_then(Json::as_object) {
        for (key, value) in map {
            match value {
                Json::String(s) => ids.set(key, s),
                Json::Number(n) => ids.set(key, n.to_string()),
                _ => false,
            };
        }
    }
    ids
}

fn ratings(json: &Json) -> cv_domain::Ratings {
    let mut out = cv_domain::Ratings::new();
    for rating in json
        .get("ratings")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
    {
        out.insert(Rating::new(
            str_of(rating, "id"),
            rating.get("rating").and_then(Json::as_f64).unwrap_or(-1.0) as f32,
            uint_of(rating, "votes"),
            uint_of(rating, "max"),
        ));
    }
    out
}

fn date_of(json: &Json, key: &str) -> Option<CalendarDate> {
    str_of(json, key).parse::<CalendarDate>().ok()
}

fn status_of(text: &str) -> AiredStatus {
    match text.trim().to_ascii_uppercase().as_str() {
        "CONTINUING" => AiredStatus::Continuing,
        "ENDED" => AiredStatus::Ended,
        "CANCELED" | "CANCELLED" => AiredStatus::Canceled,
        _ => AiredStatus::Unknown,
    }
}

fn ordering_of(text: &str) -> EpisodeOrdering {
    match text.trim().to_ascii_uppercase().as_str() {
        "DVD" => EpisodeOrdering::Dvd,
        "ABSOLUTE" => EpisodeOrdering::Absolute,
        "ALTERNATE" => EpisodeOrdering::Alternate,
        "DISPLAY" => EpisodeOrdering::Display,
        _ => EpisodeOrdering::Aired,
    }
}

fn group_of(json: &Json) -> EpisodeGroup {
    EpisodeGroup::new(ordering_of(str_of(json, "type")), str_of(json, "name"))
}

fn show_model(entity: &Json) -> ShowNfo {
    let mut show = ShowNfo {
        title: str_of(entity, "title").to_owned(),
        original_title: str_of(entity, "originalTitle").to_owned(),
        sort_title: str_of(entity, "sortTitle").to_owned(),
        english_title: str_of(entity, "englishTitle").to_owned(),
        year: uint_of(entity, "year"),
        plot: str_of(entity, "plot").to_owned(),
        outline: str_of(entity, "outline").to_owned(),
        tagline: str_of(entity, "tagline").to_owned(),
        runtime_minutes: uint_of(entity, "runtime"),
        top250: uint_of(entity, "top250"),
        ids: provider_ids(entity),
        ratings: ratings(entity),
        certification: Certification(str_of(entity, "certification").to_owned()),
        genres: strings_of(entity, "genres")
            .iter()
            .filter_map(|g| Genre::parse(g))
            .collect(),
        countries: split_show_list(str_of(entity, "country")),
        studios: split_show_list(str_of(entity, "productionCompany")),
        tags: strings_of(entity, "tags"),
        first_aired: date_of(entity, "firstAired"),
        status: status_of(str_of(entity, "status")),
        date_added: Some(
            timestamp(str_of(entity, "dateAdded")).unwrap_or_else(|| DEFAULT_DATE_ADDED.to_owned()),
        ),
        note: str_of(entity, "note").to_owned(),
        last_scraper_id: str_of(entity, "lastScraperId").to_owned(),
        active_group: entity.get("episodeGroup").map(group_of).unwrap_or_default(),
        episode_groups: entity
            .get("episodeGroups")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
            .map(group_of)
            .collect(),
        ..ShowNfo::default()
    };
    credits(entity, "actors", CreditKind::Actor, &mut show.credits);
    credits(entity, "directors", CreditKind::Director, &mut show.credits);
    credits(entity, "writers", CreditKind::Writer, &mut show.credits);
    credits(entity, "producers", CreditKind::Producer, &mut show.credits);
    show.trailers = entity
        .get("trailers")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .map(|t| Trailer {
            name: str_of(t, "name").to_owned(),
            url: str_of(t, "url").to_owned(),
            provider: str_of(t, "provider").to_owned(),
            in_nfo: t.get("inNfo").and_then(Json::as_bool).unwrap_or(false),
            ..Trailer::default()
        })
        .collect();
    show.artwork = show_artwork(entity);
    show
}

fn show_artwork(entity: &Json) -> ShowArtwork {
    let art = entity.get("artwork").unwrap_or(&Json::Null);
    let url = |key: &str| str_of(art, key).to_owned();
    ShowArtwork {
        poster: url("POSTER"),
        banner: url("BANNER"),
        clearart: url("CLEARART"),
        clearlogo: url("CLEARLOGO"),
        thumb: url("THUMB"),
        keyart: url("KEYART"),
        squareart: url("SQUAREART"),
        logo: url("LOGO"),
        characterart: url("CHARACTERART"),
        disc: url("DISC"),
        fanart: url("FANART"),
        extra_fanarts: strings_of(entity, "extraFanartUrls"),
    }
}

/// An episode model with what the file layout needs.
struct EpisodeEntry {
    nfo: EpisodeNfo,
    /// The aired season, which decides the season it belongs to.
    aired_season: u32,
    /// The folder of the video file under the show folder.
    dir: String,
    /// The video file name.
    filename: String,
}

fn episode_models(entity: &Json, show: &ShowNfo, show_studios: &str) -> Vec<EpisodeEntry> {
    entity
        .get("episodes")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .map(|json| episode_model(json, show, show_studios))
        .collect()
}

fn episode_model(json: &Json, show: &ShowNfo, show_studios: &str) -> EpisodeEntry {
    let numbers: Vec<EpisodeNumber> = json
        .get("numbers")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .map(|n| {
            EpisodeNumber::new(
                EpisodeGroup::new(ordering_of(str_of(n, "group")), ""),
                uint_of(n, "season"),
                uint_of(n, "episode"),
            )
        })
        .collect();
    let number_in = |ordering: EpisodeOrdering| {
        numbers
            .iter()
            .find(|n| n.group.ordering == ordering)
            .map(|n| (n.season, n.episode))
    };
    let aired = number_in(EpisodeOrdering::Aired).unwrap_or((0, 0));
    let shown = number_in(show.active_group.ordering).unwrap_or(aired);

    let video = files_of(json, "VIDEO").next();
    let filename = video
        .map(|f| str_of(f, "filename"))
        .unwrap_or_default()
        .to_owned();
    let dir = video
        .map(|f| str_of(f, "dir"))
        .unwrap_or_default()
        .to_owned();

    let production = {
        let own = str_of(json, "productionCompany");
        if own.trim().is_empty() {
            // an episode without a studio shows the studios of its show
            show_studios
        } else {
            own
        }
    };
    let mut nfo = EpisodeNfo {
        title: str_of(json, "title").to_owned(),
        original_title: str_of(json, "originalTitle").to_owned(),
        english_title: str_of(json, "englishTitle").to_owned(),
        show_title: show.title.clone(),
        season: shown.0,
        episode: shown.1,
        display: number_in(EpisodeOrdering::Display),
        numbers,
        ids: provider_ids(json),
        ratings: ratings(json),
        plot: str_of(json, "plot").to_owned(),
        outline: str_of(json, "outline").to_owned(),
        runtime_minutes: uint_of(json, "runtime"),
        thumb: json
            .get("artwork")
            .map(|a| str_of(a, "THUMB"))
            .unwrap_or_default()
            .to_owned(),
        certification: Certification(str_of(json, "certification").to_owned()),
        first_aired: date_of(json, "firstAired"),
        watched: json.get("watched").and_then(Json::as_bool).unwrap_or(false),
        play_count: uint_of(json, "playcount"),
        last_played: timestamp(str_of(json, "lastWatched")),
        studios: Vec::new(),
        tags: strings_of(json, "tags"),
        date_added: Some(
            timestamp(str_of(json, "dateAdded")).unwrap_or_else(|| DEFAULT_DATE_ADDED.to_owned()),
        ),
        media_source: MediaSource::parse(str_of(json, "mediaSource"))
            .unwrap_or(MediaSource::Unknown),
        edition: Edition::parse(str_of(json, "edition")).unwrap_or(Edition::None),
        original_filename: original_filename(json),
        note: str_of(json, "note").to_owned(),
        ..EpisodeNfo::default()
    };
    nfo.studios = split_episode_list(production);
    credits(json, "actors", CreditKind::Actor, &mut nfo.credits);
    credits(json, "directors", CreditKind::Director, &mut nfo.credits);
    credits(json, "writers", CreditKind::Writer, &mut nfo.credits);
    if let Some(file) = video {
        nfo.video = video_info(file);
    }
    for file in files_of(json, "SUBTITLE") {
        nfo.external_subtitles.extend(subtitle_streams(file));
    }
    EpisodeEntry {
        nfo,
        aired_season: aired.0,
        dir,
        filename,
    }
}

/// The seasons that have episodes, in order, with the text and artwork of the entity's own
/// season records and the first air date of their episodes.
fn season_models(entity: &Json, episodes: &[EpisodeEntry]) -> Vec<SeasonNfo> {
    let mut numbers: Vec<u32> = episodes.iter().map(|e| e.aired_season).collect();
    numbers.sort_unstable();
    numbers.dedup();
    let records = entity
        .get("seasons")
        .and_then(Json::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    numbers
        .into_iter()
        .map(|number| {
            let record = records.iter().find(|r| uint_of(r, "season") == number);
            let art = record.and_then(|r| r.get("artwork")).unwrap_or(&Json::Null);
            let url = |key: &str| str_of(art, key).to_owned();
            SeasonNfo {
                number,
                title: record
                    .map(|r| str_of(r, "title"))
                    .unwrap_or_default()
                    .to_owned(),
                plot: record
                    .map(|r| str_of(r, "plot"))
                    .unwrap_or_default()
                    .to_owned(),
                note: record
                    .map(|r| str_of(r, "note"))
                    .unwrap_or_default()
                    .to_owned(),
                first_aired: episodes
                    .iter()
                    .filter(|e| e.aired_season == number)
                    .filter_map(|e| e.nfo.first_aired)
                    .min(),
                artwork: SeasonArtwork {
                    poster: url("SEASON_POSTER"),
                    banner: url("SEASON_BANNER"),
                    thumb: url("SEASON_THUMB"),
                    fanart: url("SEASON_FANART"),
                },
                unknown_elements: Vec::new(),
            }
        })
        .collect()
}

/// A video file that holds episodes.
struct VideoKey {
    dir: String,
    filename: String,
}

impl VideoKey {
    /// The file name without its extension.
    fn base(&self) -> &str {
        self.filename
            .rsplit_once('.')
            .map_or(self.filename.as_str(), |(base, _)| base)
    }

    /// `<dir>/<name>` under the show folder, or just the name when the file sits in it.
    fn path(&self, name: &str) -> String {
        if self.dir.is_empty() {
            name.to_owned()
        } else {
            format!("{}/{name}", self.dir)
        }
    }
}

/// The episodes grouped by video file, in the order of their first episode.
fn files_of_episodes(episodes: &[EpisodeEntry]) -> Vec<(VideoKey, Vec<&EpisodeEntry>)> {
    let mut groups: Vec<(VideoKey, Vec<&EpisodeEntry>)> = Vec::new();
    for episode in episodes.iter().filter(|e| !e.filename.is_empty()) {
        match groups
            .iter_mut()
            .find(|(key, _)| key.dir == episode.dir && key.filename == episode.filename)
        {
            Some((_, list)) => list.push(episode),
            None => groups.push((
                VideoKey {
                    dir: episode.dir.clone(),
                    filename: episode.filename.clone(),
                },
                vec![episode],
            )),
        }
    }
    groups
}
