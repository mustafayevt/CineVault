//! Library items built from the fixture entities of `rename/entities`.
//!
//! The renamer fixtures describe a movie or a show as plain JSON (`title`, `year`, `genres`,
//! `files` with media info, `episodes` with `numbers`). The helpers here turn that into the
//! domain items the renamer reads, so the fixtures run through the real token models.
//!
//! Two choices are documented where they are made: ratings are not fed to the renamer (the
//! fixtures pin names without a rating), and the fixture's `videoBitRate` is stored as the bit
//! rate unchanged.

use cv_domain::{
    AiredStatus, AudioStream, CalendarDate, Certification, Credit, CreditKind, Edition,
    EpisodeGroup, EpisodeNumber, EpisodeOrdering, Genre, Item, ItemDetail, ItemId, MediaFile,
    MediaFileType, MediaSource, StreamFlag, SubtitleStream, Video3dFormat, id_key,
};
use serde_json::Value as Json;

use crate::nfo::{split_list, str_of, strings_of, uint_of};
use crate::run::HandlerError;

/// Reads the fixture entity `name` from the rename or NFO entity folder.
pub(crate) fn read_entity(fixtures: &std::path::Path, name: &str) -> Result<Json, HandlerError> {
    for dir in ["rename/entities", "nfo/entities"] {
        let path = fixtures.join(dir).join(format!("{name}.json"));
        if let Ok(text) = std::fs::read_to_string(&path) {
            return serde_json::from_str(&text)
                .map_err(|e| HandlerError::Failed(format!("{}: {e}", path.display())));
        }
    }
    Err(HandlerError::Failed(format!("entity `{name}` not found")))
}

/// The data source root the items live under in these tests.
pub(crate) const DATA_SOURCE: &str = "/ds";

fn number(json: &Json, key: &str) -> u32 {
    uint_of(json, key)
}

fn float(json: &Json, key: &str) -> f64 {
    json.get(key).and_then(Json::as_f64).unwrap_or(0.0)
}

/// Codec spellings found in release names and some fixture entities, mapped to the names
/// CineVault stores. The tokens print the stored name.
const STORED_CODEC_NAMES: &[(&str, &str)] = &[
    ("TrueHD/Atmos", "TrueHD Atmos"),
    ("DTSHD-MA", "DTS-HD MA"),
    ("EAC3", "E-AC-3"),
];

fn stored_codec(fixture: &str) -> String {
    STORED_CODEC_NAMES
        .iter()
        .find(|(name, _)| *name == fixture)
        .map_or(fixture, |(_, stored)| *stored)
        .to_owned()
}

fn video_3d(name: &str) -> Video3dFormat {
    match name {
        "3D" => Video3dFormat::ThreeD,
        "3D SBS" => Video3dFormat::SideBySide,
        "3D TAB" => Video3dFormat::TopAndBottom,
        "3D HSBS" => Video3dFormat::HalfSideBySide,
        "3D HTAB" => Video3dFormat::HalfTopAndBottom,
        "3D MVC" => Video3dFormat::Mvc,
        _ => Video3dFormat::None,
    }
}

/// A video file with the media info of a fixture file object.
pub(crate) fn video_file(json: &Json, directory: &str, filename: &str) -> MediaFile {
    let mut video = MediaFile::new(MediaFileType::Video, directory, filename);
    video.video_codec = str_of(json, "videoCodec").to_owned();
    video.width = number(json, "videoWidth");
    video.height = number(json, "videoHeight");
    video.aspect_ratio = float(json, "aspectRatio") as f32;
    video.duration_secs = number(json, "duration");
    video.bit_depth = number(json, "bitDepth");
    video.frame_rate = float(json, "frameRate");
    // The fixtures count the bit rate in the unit the `bitrate` renderer expects.
    video.video_bitrate_kbps = number(json, "videoBitRate");
    video.size_bytes = json.get("filesize").and_then(Json::as_u64).unwrap_or(0);
    video.video_3d = video_3d(str_of(json, "video3DFormat"));
    video.hdr_formats = str_of(json, "hdrFormat")
        .split(", ")
        .filter(|f| !f.is_empty())
        .map(str::to_owned)
        .collect();
    for track in json
        .get("audio")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
    {
        video.audio_streams.push(AudioStream {
            codec: stored_codec(str_of(track, "codec")),
            language: str_of(track, "language").to_owned(),
            channels: number(track, "audioChannels"),
            ..AudioStream::default()
        });
    }
    video.subtitles = subtitle_streams(json);
    video
}

/// The subtitle streams of a fixture file object.
pub(crate) fn subtitle_streams(json: &Json) -> Vec<SubtitleStream> {
    json.get("subtitles")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .map(|s| {
            let mut flags = std::collections::BTreeSet::new();
            if s.get("forced").and_then(Json::as_bool).unwrap_or(false) {
                flags.insert(StreamFlag::Forced);
            }
            if s.get("sdh").and_then(Json::as_bool).unwrap_or(false) {
                flags.insert(StreamFlag::HearingImpaired);
            }
            SubtitleStream {
                language: str_of(s, "language").to_owned(),
                title: str_of(s, "title").to_owned(),
                flags,
                ..SubtitleStream::default()
            }
        })
        .collect()
}

fn credits(entity: &Json, key: &str, kind: CreditKind, out: &mut Vec<Credit>) {
    for person in entity
        .get(key)
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
    {
        out.push(Credit::new(
            kind,
            str_of(person, "name"),
            str_of(person, "role"),
        ));
    }
}

fn date(json: &Json, key: &str) -> Option<CalendarDate> {
    str_of(json, key).parse().ok()
}

fn genres(entity: &Json) -> Vec<Genre> {
    strings_of(entity, "genres")
        .iter()
        .filter_map(|g| Genre::parse(g))
        .collect()
}

/// A movie from an entity. `videos` are the video files (directory, filename, media info); the
/// movie's folder is `<data source>/<folder>` unless `folder` says otherwise.
pub(crate) fn movie_item(entity: &Json, folder: &str, videos: &[(String, Json)]) -> Item {
    let mut item = Item::movie(str_of(entity, "title"));
    item.id = Some(ItemId(1));
    let core = &mut item.core;
    core.original_title = str_of(entity, "originalTitle").to_owned();
    core.year = u16::try_from(number(entity, "year")).unwrap_or(0);
    core.data_source = DATA_SOURCE.to_owned();
    core.path = format!("{DATA_SOURCE}/{folder}");
    core.ids.set(id_key::IMDB, str_of(entity, "imdbId"));
    if let Some(tmdb) = entity.get("tmdbId").and_then(Json::as_u64) {
        core.ids.set(id_key::TMDB, tmdb.to_string());
    }
    core.production_companies = split_list_semicolons(str_of(entity, "productionCompany"));
    core.note = str_of(entity, "note").to_owned();
    for tag in strings_of(entity, "tags") {
        core.tags.add(&tag);
    }
    credits(entity, "actors", CreditKind::Actor, &mut core.credits);
    credits(entity, "directors", CreditKind::Director, &mut core.credits);
    credits(entity, "writers", CreditKind::Writer, &mut core.credits);
    credits(entity, "producers", CreditKind::Producer, &mut core.credits);
    for (filename, json) in videos {
        let directory = format!("{DATA_SOURCE}/{folder}");
        if core.original_filename.is_empty() {
            core.original_filename.clone_from(filename);
        }
        core.media_files
            .push(video_file(json, &directory, filename));
    }
    if let ItemDetail::Movie(movie) = &mut item.detail {
        movie.english_title = str_of(entity, "englishTitle").to_owned();
        movie.sort_title = str_of(entity, "sortTitle").to_owned();
        movie.certification = Certification(str_of(entity, "certification").to_owned());
        movie.media_source =
            MediaSource::parse(str_of(entity, "mediaSource")).unwrap_or(MediaSource::Unknown);
        movie.edition = Edition::parse(str_of(entity, "edition")).unwrap_or(Edition::None);
        movie.spoken_languages = split_list(str_of(entity, "spokenLanguages"));
        movie.release_date = date(entity, "releaseDate");
        movie.genres = genres(entity);
    }
    item
}

fn split_list_semicolons(text: &str) -> Vec<String> {
    text.split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The video files of a movie entity as `(filename, json)`.
pub(crate) fn entity_videos(entity: &Json) -> Vec<(String, Json)> {
    entity
        .get("files")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .filter(|f| str_of(f, "type") == "VIDEO")
        .map(|f| (str_of(f, "filename").to_owned(), f.clone()))
        .collect()
}

fn ordering(name: &str) -> EpisodeOrdering {
    match name.trim().to_ascii_uppercase().as_str() {
        "DVD" => EpisodeOrdering::Dvd,
        "ABSOLUTE" => EpisodeOrdering::Absolute,
        "ALTERNATE" => EpisodeOrdering::Alternate,
        "DISPLAY" => EpisodeOrdering::Display,
        _ => EpisodeOrdering::Aired,
    }
}

fn status(text: &str) -> AiredStatus {
    match text.trim().to_ascii_uppercase().as_str() {
        "CONTINUING" => AiredStatus::Continuing,
        "ENDED" => AiredStatus::Ended,
        "CANCELED" | "CANCELLED" => AiredStatus::Canceled,
        _ => AiredStatus::Unknown,
    }
}

/// A show from an entity. `folder` is the show folder under the data source.
pub(crate) fn show_item(entity: &Json, folder: &str) -> Item {
    let mut item = Item::show(str_of(entity, "title"));
    item.id = Some(ItemId(100));
    let core = &mut item.core;
    core.original_title = str_of(entity, "originalTitle").to_owned();
    core.year = u16::try_from(number(entity, "year")).unwrap_or(0);
    core.data_source = DATA_SOURCE.to_owned();
    core.path = format!("{DATA_SOURCE}/{folder}");
    core.note = str_of(entity, "note").to_owned();
    if let Some(ids) = entity.get("ids").and_then(Json::as_object) {
        for (key, value) in ids {
            match value {
                Json::String(s) => core.ids.set(key, s),
                Json::Number(n) => core.ids.set(key, n.to_string()),
                _ => false,
            };
        }
    }
    core.production_companies = split_list_semicolons(str_of(entity, "productionCompany"));
    for tag in strings_of(entity, "tags") {
        core.tags.add(&tag);
    }
    if let ItemDetail::Show(show) = &mut item.detail {
        show.english_title = str_of(entity, "englishTitle").to_owned();
        show.first_aired = date(entity, "firstAired");
        show.status = status(str_of(entity, "status"));
        show.certification = Certification(str_of(entity, "certification").to_owned());
        show.genres = genres(entity);
    }
    item
}

/// An episode of a show from an entity episode object. `dir` is the folder of its files under
/// the show folder.
pub(crate) fn episode_item(show: &Item, json: &Json, dir: &str) -> Item {
    let mut item = Item::episode(show.id.unwrap_or(ItemId(100)), str_of(json, "title"));
    item.core.original_title = str_of(json, "originalTitle").to_owned();
    item.core.data_source = DATA_SOURCE.to_owned();
    item.core.path = format!("{}/{dir}", show.core.path);
    if let ItemDetail::Episode(episode) = &mut item.detail {
        for n in json
            .get("numbers")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
        {
            episode.numbers.set(EpisodeNumber::new(
                EpisodeGroup::new(ordering(str_of(n, "group")), ""),
                number(n, "season"),
                number(n, "episode"),
            ));
        }
        episode.first_aired = date(json, "firstAired");
        episode.media_source =
            MediaSource::parse(str_of(json, "mediaSource")).unwrap_or(MediaSource::Unknown);
        episode.multi_episode = json
            .get("multiEpisode")
            .and_then(Json::as_bool)
            .unwrap_or(false);
    }
    item
}

/// Adds a video file to an item's media files.
pub(crate) fn add_video(item: &mut Item, json: &Json, directory: &str, filename: &str) {
    if item.core.original_filename.is_empty() {
        item.core.original_filename = filename.to_owned();
    }
    item.core
        .media_files
        .push(video_file(json, directory, filename));
}
