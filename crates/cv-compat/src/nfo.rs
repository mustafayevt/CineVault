//! NFO handler: writes the NFO files of a fixture entity with `cv-nfo`.
//!
//! The handler is test glue. It reads an entity from `fixtures/nfo/entities`, turns it into the
//! plain writer model, applies the case settings and returns every file written, keyed by its path
//! relative to the case folder. Movie cases cover all movie formats and movie sets; TV cases are in
//! `nfo_tv`. A case with an `existing_nfo` and "write clean NFO" off carries the old file's values and
//! unmanaged elements over.

use std::collections::BTreeMap;
use std::fs;

use cv_domain::{
    AudioStream, CalendarDate, Certification, Credit, CreditKind, Edition, Genre, MediaFileType,
    MediaSource, Person, Rating, SubtitleStream, Trailer,
};
use cv_nfo::{
    CertificationStyle, DateAddedField, MovieArtwork, MovieConnector, MovieNameContext, MovieNfo,
    MovieNfoNaming, MovieNfoOptions, MovieSetConnector, MovieSetNfo, MovieSetNfoNaming,
    MovieSetRef, VideoInfo, apply_old_movie_nfo, movie_nfo_file_names, movie_set_folder_name,
    movie_set_nfo_paths, render_movie_nfo_for, render_movie_set_nfo,
};
use serde_json::Value as Json;

use crate::family::Family;
use crate::fixture::Case;
use crate::run::{Actual, Handler, HandlerError};

/// The date added of entities that do not set one, pinned so the output is stable.
pub(crate) const DEFAULT_DATE_ADDED: &str = "2024-01-01 00:00:00";

/// NFO cases: `{id, kind, entity, profile, settings?, existing_nfo?}` to the written files.
pub(crate) struct NfoHandler;

impl Handler for NfoHandler {
    fn family(&self) -> Family {
        Family::Nfo
    }

    fn run(&self, case: &Case) -> Result<Actual, HandlerError> {
        match str_of(&case.input, "kind") {
            "movie" => run_movie(case),
            "movie-set" => run_movie_set(case),
            "tv" => crate::nfo_tv::run_tv(case),
            kind => Err(HandlerError::Failed(format!(
                "unknown NFO case kind `{kind}`"
            ))),
        }
    }
}

fn run_movie(case: &Case) -> Result<Actual, HandlerError> {
    let settings = MovieSettings::load(case)?;
    let connector = MovieConnector::parse(&settings.connector).ok_or_else(|| {
        HandlerError::Failed(format!("unknown movie connector `{}`", settings.connector))
    })?;
    let entity = read_entity(case)?;
    let mut movie = movie_model(&entity);
    if let Some(old) = existing_nfo(case) {
        apply_old_movie_nfo(&mut movie, old, &settings.options);
    }
    let folder = str_of(&entity, "folder");
    let names = movie_nfo_file_names(&settings.namings, &name_context(&entity, &settings));
    let text = render_movie_nfo_for(connector, &movie, &settings.options);
    let files: BTreeMap<String, Vec<u8>> = names
        .into_iter()
        .map(|name| (format!("{folder}/{name}"), text.clone().into_bytes()))
        .collect();
    Ok(Actual::Files(files))
}

/// The folder under the case folder that holds the movie-set data.
const SETS_FOLDER: &str = "sets";

fn run_movie_set(case: &Case) -> Result<Actual, HandlerError> {
    let settings = MovieSettings::load(case)?;
    let connector = MovieSetConnector::parse(&settings.set_connector).ok_or_else(|| {
        HandlerError::Failed(format!(
            "unknown movie-set connector `{}`",
            settings.set_connector
        ))
    })?;
    let entity = read_entity(case)?;
    let set = set_model(&entity);
    let folder =
        movie_set_folder_name(&set.title, set.ids.get("tmdb"), settings.set_append_tmdb_id);
    let text = render_movie_set_nfo(connector, &set, &settings.options);
    let files: BTreeMap<String, Vec<u8>> =
        movie_set_nfo_paths(&settings.set_namings, &set.title, &folder)
            .into_iter()
            .map(|path| (format!("{SETS_FOLDER}/{path}"), text.clone().into_bytes()))
            .collect();
    Ok(Actual::Files(files))
}

/// The text of the case's `existing_nfo`: the file that is on disk before the write.
pub(crate) fn existing_nfo(case: &Case) -> Option<&str> {
    case.input
        .get("existing_nfo")?
        .get("content")
        .and_then(Json::as_str)
}

fn set_model(entity: &Json) -> MovieSetNfo {
    let json = entity.get("set").unwrap_or(&Json::Null);
    let mut set = MovieSetNfo {
        title: str_of(json, "title").to_owned(),
        sort_title: str_of(json, "sortTitle").to_owned(),
        overview: str_of(json, "plot").to_owned(),
        tags: strings_of(json, "tags"),
        date_added: timestamp(str_of(json, "dateAdded")),
        note: str_of(json, "note").to_owned(),
        english_title: str_of(json, "englishTitle").to_owned(),
        ..MovieSetNfo::default()
    };
    if let Some(id) = json.get("tmdbId").and_then(Json::as_u64) {
        set.ids.set("tmdb", id.to_string());
    }
    let artwork = json.get("artwork");
    set.poster = artwork.map_or("", |a| str_of(a, "POSTER")).to_owned();
    set.fanart = artwork.map_or("", |a| str_of(a, "FANART")).to_owned();
    set
}

// ---- settings ------------------------------------------------------------------------------

struct MovieSettings {
    connector: String,
    set_connector: String,
    set_namings: Vec<MovieSetNfoNaming>,
    set_append_tmdb_id: bool,
    namings: Vec<MovieNfoNaming>,
    nfo_in_disc_folder: bool,
    options: MovieNfoOptions,
}

impl MovieSettings {
    /// The profile's `movie` object with the case's own `settings.movie` laid over it.
    fn load(case: &Case) -> Result<MovieSettings, HandlerError> {
        let profile = str_of(&case.input, "profile");
        let mut merged = case
            .shared
            .get("profiles")
            .and_then(|p| p.get(profile))
            .and_then(|p| p.get("movie"))
            .and_then(Json::as_object)
            .cloned()
            .ok_or_else(|| HandlerError::Failed(format!("profile `{profile}` has no `movie`")))?;
        if let Some(over) = case
            .input
            .get("settings")
            .and_then(|s| s.get("movie"))
            .and_then(Json::as_object)
        {
            merged.extend(over.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
        let settings = Json::Object(merged);
        let flag = |key: &str| settings.get(key).and_then(Json::as_bool);
        let mut options = common_options(&settings);
        options.clean_nfo = flag("writeCleanNfo").unwrap_or(true);
        let namings = settings
            .get("nfoFilenames")
            .and_then(Json::as_array)
            .map(|names| {
                names
                    .iter()
                    .filter_map(Json::as_str)
                    .filter_map(MovieNfoNaming::parse)
                    .collect()
            })
            .unwrap_or_else(|| vec![MovieNfoNaming::VideoBase]);
        let set_namings = settings
            .get("movieSetNfoFilenames")
            .and_then(Json::as_array)
            .map(|names| {
                names
                    .iter()
                    .filter_map(Json::as_str)
                    .filter_map(MovieSetNfoNaming::parse)
                    .collect()
            })
            .unwrap_or_else(|| vec![MovieSetNfoNaming::Kodi]);
        Ok(MovieSettings {
            set_connector: settings
                .get("movieSetConnector")
                .and_then(Json::as_str)
                .unwrap_or("KODI")
                .to_owned(),
            set_namings,
            set_append_tmdb_id: flag("movieSetAppendTmdbId").unwrap_or(false),
            connector: settings
                .get("movieConnector")
                .and_then(Json::as_str)
                .unwrap_or("KODI")
                .to_owned(),
            namings,
            nfo_in_disc_folder: flag("nfoDiscFolderInside").unwrap_or(false),
            options,
        })
    }
}

/// The options movies and TV shows share, read from a settings object. Settings that are not set
/// keep the writer default.
pub(crate) fn common_options(settings: &Json) -> MovieNfoOptions {
    let flag = |key: &str| settings.get(key).and_then(Json::as_bool);
    let mut options = MovieNfoOptions {
        // Without a version, so the golden files do not change with every release.
        generator: "CineVault".to_owned(),
        ..MovieNfoOptions::default()
    };
    let set = |target: &mut bool, key: &str| {
        if let Some(value) = flag(key) {
            *target = value;
        }
    };
    set(&mut options.write_date_added, "nfoWriteDateAdded");
    set(&mut options.single_studio, "nfoWriteSingleStudio");
    set(&mut options.lockdata, "nfoWriteLockdata");
    set(&mut options.trailer, "nfoWriteTrailer");
    set(&mut options.fileinfo, "nfoWriteFileinfo");
    set(&mut options.artwork_urls, "nfoWriteArtworkUrls");
    set(&mut options.create_outline, "createOutline");
    set(&mut options.outline_first_sentence, "outlineFirstSentence");
    set(
        &mut options.include_external_audio,
        "includeExternalAudioStreams",
    );
    if let Some(field) = settings
        .get("nfoDateAddedField")
        .and_then(Json::as_str)
        .and_then(DateAddedField::parse)
    {
        options.date_added_field = field;
    }
    if let Some(language) = settings.get("nfoLanguage").and_then(Json::as_str) {
        options.language = language.to_owned();
    }
    if let Some(style) = settings
        .get("certificationStyle")
        .and_then(Json::as_str)
        .and_then(CertificationStyle::parse)
    {
        options.certification_style = style;
    }
    if let Some(sources) = settings.get("ratingSources").and_then(Json::as_array) {
        options.rating_sources = sources
            .iter()
            .filter_map(Json::as_str)
            .map(str::to_owned)
            .collect();
    }
    options.created_on = DEFAULT_DATE_ADDED.to_owned();
    options
}

// ---- entity to model --------------------------------------------------------------------

pub(crate) fn read_entity(case: &Case) -> Result<Json, HandlerError> {
    let name = str_of(&case.input, "entity");
    let path = case
        .fixtures_dir
        .join("nfo/entities")
        .join(format!("{name}.json"));
    let text = fs::read_to_string(&path)
        .map_err(|e| HandlerError::Failed(format!("{}: {e}", path.display())))?;
    serde_json::from_str(&text)
        .map_err(|e| HandlerError::Failed(format!("{}: {e}", path.display())))
}

pub(crate) fn str_of<'a>(json: &'a Json, key: &str) -> &'a str {
    json.get(key).and_then(Json::as_str).unwrap_or_default()
}

pub(crate) fn uint_of(json: &Json, key: &str) -> u32 {
    json.get(key)
        .and_then(Json::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .unwrap_or(0)
}

pub(crate) fn strings_of(json: &Json, key: &str) -> Vec<String> {
    json.get(key)
        .and_then(Json::as_array)
        .map(|list| {
            list.iter()
                .filter_map(Json::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// Splits a list written as one text (`"US, GB"`, `"A; B / C"`) at `;`, `,`, `/` and `|`.
pub(crate) fn split_list(text: &str) -> Vec<String> {
    text.split([';', ',', '/', '|'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

/// A date-time text with a missing time of day completed to midnight.
pub(crate) fn timestamp(text: &str) -> Option<String> {
    let text = text.trim();
    match text.len() {
        0 => None,
        10 => Some(format!("{text} 00:00:00")),
        _ => Some(text.to_owned()),
    }
}

pub(crate) fn person(json: &Json) -> Person {
    let mut person = Person::new(str_of(json, "name"));
    person.thumb_url = str_of(json, "thumb").to_owned();
    person.profile_url = str_of(json, "profile").to_owned();
    if let Some(ids) = json.get("ids").and_then(Json::as_object) {
        for (key, value) in ids {
            match value {
                Json::String(s) => person.ids.set(key, s),
                Json::Number(n) => person.ids.set(key, n.to_string()),
                _ => false,
            };
        }
    }
    person
}

pub(crate) fn credits(entity: &Json, key: &str, kind: CreditKind, out: &mut Vec<Credit>) {
    for item in entity
        .get(key)
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
    {
        out.push(Credit {
            kind,
            person: person(item),
            role: str_of(item, "role").to_owned(),
        });
    }
}

pub(crate) fn video_info(file: &Json) -> VideoInfo {
    VideoInfo {
        codec: str_of(file, "videoCodec").to_owned(),
        width: uint_of(file, "videoWidth"),
        height: uint_of(file, "videoHeight"),
        aspect_ratio: file
            .get("aspectRatio")
            .and_then(Json::as_f64)
            .unwrap_or_default() as f32,
        duration_secs: uint_of(file, "duration"),
        hdr_formats: split_list_commas(str_of(file, "hdrFormat")),
        stereo_format: str_of(file, "video3DFormat").to_owned(),
        audio: audio_streams(file),
        subtitles: subtitle_streams(file),
        created: timestamp(str_of(file, "dateCreated")),
        modified: timestamp(str_of(file, "dateLastModified")),
        ..VideoInfo::default()
    }
}

fn split_list_commas(text: &str) -> Vec<String> {
    text.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

fn audio_streams(file: &Json) -> Vec<AudioStream> {
    file.get("audio")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .map(|a| AudioStream {
            codec: str_of(a, "codec").to_owned(),
            language: str_of(a, "language").to_owned(),
            channels: uint_of(a, "audioChannels"),
            ..AudioStream::default()
        })
        .collect()
}

pub(crate) fn subtitle_streams(file: &Json) -> Vec<SubtitleStream> {
    file.get("subtitles")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .map(|s| SubtitleStream {
            language: str_of(s, "language").to_owned(),
            ..SubtitleStream::default()
        })
        .collect()
}

pub(crate) fn files_of<'a>(entity: &'a Json, kind: &'a str) -> impl Iterator<Item = &'a Json> {
    entity
        .get("files")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .filter(move |f| str_of(f, "type") == kind)
}

fn movie_model(entity: &Json) -> MovieNfo {
    let mut movie = MovieNfo {
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
        certification: Certification(str_of(entity, "certification").to_owned()),
        genres: strings_of(entity, "genres")
            .iter()
            .filter_map(|g| Genre::parse(g))
            .collect(),
        countries: split_list(str_of(entity, "country")),
        studios: split_list(str_of(entity, "productionCompany")),
        country_text: str_of(entity, "country").to_owned(),
        studio_text: str_of(entity, "productionCompany").to_owned(),
        spoken_languages: split_list(str_of(entity, "spokenLanguages")),
        tags: strings_of(entity, "tags"),
        show_links: strings_of(entity, "showlinks"),
        release_date: str_of(entity, "releaseDate").parse::<CalendarDate>().ok(),
        date_added: Some(
            timestamp(str_of(entity, "dateAdded")).unwrap_or_else(|| DEFAULT_DATE_ADDED.to_owned()),
        ),
        watched: entity
            .get("watched")
            .and_then(Json::as_bool)
            .unwrap_or(false),
        play_count: uint_of(entity, "playcount"),
        last_played: timestamp(str_of(entity, "lastWatched")),
        media_source: MediaSource::parse(str_of(entity, "mediaSource"))
            .unwrap_or(MediaSource::Unknown),
        edition: Edition::parse(str_of(entity, "edition")).unwrap_or(Edition::None),
        original_filename: original_filename(entity),
        note: str_of(entity, "note").to_owned(),
        is_disc: entity.get("disc").and_then(Json::as_bool).unwrap_or(false),
        ..MovieNfo::default()
    };
    for key in ["imdbId", "tmdbId"] {
        let id = match entity.get(key) {
            Some(Json::String(s)) => s.clone(),
            Some(Json::Number(n)) => n.to_string(),
            _ => continue,
        };
        movie
            .ids
            .set(if key == "imdbId" { "imdb" } else { "tmdb" }, id);
    }
    if let Some(ids) = entity.get("ids").and_then(Json::as_object) {
        for (key, value) in ids {
            match value {
                Json::String(s) => movie.ids.set(key, s),
                Json::Number(n) => movie.ids.set(key, n.to_string()),
                _ => false,
            };
        }
    }
    for rating in entity
        .get("ratings")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
    {
        movie.ratings.insert(Rating::new(
            str_of(rating, "id"),
            rating.get("rating").and_then(Json::as_f64).unwrap_or(-1.0) as f32,
            uint_of(rating, "votes"),
            uint_of(rating, "max"),
        ));
    }
    credits(entity, "actors", CreditKind::Actor, &mut movie.credits);
    credits(
        entity,
        "directors",
        CreditKind::Director,
        &mut movie.credits,
    );
    credits(entity, "writers", CreditKind::Writer, &mut movie.credits);
    credits(
        entity,
        "producers",
        CreditKind::Producer,
        &mut movie.credits,
    );
    movie.trailers = entity
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
    if let Some(set) = entity.get("movieSet") {
        movie.movie_set = Some(MovieSetRef {
            title: str_of(set, "title").to_owned(),
            overview: str_of(set, "plot").to_owned(),
            tmdb_id: set.get("tmdbId").and_then(Json::as_u64),
        });
    }
    movie.artwork = artwork(entity);
    if let Some(video) = files_of(entity, "VIDEO").next() {
        movie.video = video_info(video);
    }
    for file in files_of(entity, "AUDIO") {
        movie.external_audio.extend(audio_streams(file));
    }
    for file in files_of(entity, "SUBTITLE") {
        movie.external_subtitles.extend(subtitle_streams(file));
    }
    movie
}

/// The original file name: the entity's own, else the first video file's name (as CineVault records
/// it when the movie is first scanned).
pub(crate) fn original_filename(entity: &Json) -> String {
    let own = str_of(entity, "originalFilename");
    if !own.is_empty() {
        return own.to_owned();
    }
    files_of(entity, "VIDEO")
        .next()
        .map(|f| str_of(f, "filename").to_owned())
        .unwrap_or_default()
}

fn artwork(entity: &Json) -> MovieArtwork {
    let mut urls = BTreeMap::new();
    let mut logo = String::new();
    for (key, value) in entity
        .get("artwork")
        .and_then(Json::as_object)
        .into_iter()
        .flatten()
    {
        let url = value.as_str().unwrap_or_default().to_owned();
        let kind = match key.as_str() {
            "POSTER" => MediaFileType::Poster,
            "FANART" => MediaFileType::Fanart,
            "BANNER" => MediaFileType::Banner,
            "CLEARART" => MediaFileType::Clearart,
            "CLEARLOGO" => MediaFileType::Clearlogo,
            "DISC" => MediaFileType::Disc,
            "THUMB" => MediaFileType::Thumb,
            "KEYART" => MediaFileType::Keyart,
            "SQUAREART" => MediaFileType::Squareart,
            "LOGO" => {
                logo = url;
                continue;
            }
            _ => continue,
        };
        urls.insert(kind, url);
    }
    let mut art = MovieArtwork::from_urls(&urls, &strings_of(entity, "extraFanarts"));
    art.logo = logo;
    art
}

fn name_context(entity: &Json, settings: &MovieSettings) -> MovieNameContext {
    let video = files_of(entity, "VIDEO")
        .next()
        .map(|f| str_of(f, "filename"))
        .unwrap_or_default();
    let base = video.rsplit_once('.').map_or(video, |(base, _)| base);
    MovieNameContext {
        video_base_name: base.to_owned(),
        is_disc: entity.get("disc").and_then(Json::as_bool).unwrap_or(false),
        disc_base_name: base.to_owned(),
        multi_movie_dir: entity
            .get("multiMovieDir")
            .and_then(Json::as_bool)
            .unwrap_or(false),
        nfo_in_disc_folder: settings.nfo_in_disc_folder,
    }
}
