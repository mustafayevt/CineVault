//! Turns scanned groups into library items. Only what the file names say is filled in; titles
//! from the providers, NFO data and media info arrive in later steps.

use std::path::Path;

use cv_domain::{
    CalendarDate, EpisodeNumber, Item, ItemDetail, ItemId, MediaFile, MediaFileType, ProviderIds,
    id_key,
};

use crate::parse::ParsedIds;
use crate::walk::{ScannedEpisode, ScannedFile, ScannedMovie, ScannedShow};

/// A lower-cased path with `/` separators and no trailing separator: the key two spellings of
/// the same file share.
pub(crate) fn path_key(path: &str) -> String {
    let mut key = path.replace('\\', "/").to_lowercase();
    while key.len() > 1 && key.ends_with('/') {
        key.pop();
    }
    key
}

/// The key of the file `name` in folder `directory`.
pub(crate) fn file_key(directory: &str, name: &str) -> String {
    format!("{}/{}", path_key(directory), name.to_lowercase())
}

/// The key of a scanned file.
pub(crate) fn scanned_key(file: &ScannedFile) -> String {
    path_key(&file.path.to_string_lossy())
}

/// The folder of a scanned file, as stored in `media_file.directory`.
pub(crate) fn directory_of(file: &ScannedFile) -> String {
    file.path
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

pub(crate) fn name_of(file: &ScannedFile) -> String {
    file.path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// A fresh media-file row for a scanned file: no media info yet.
pub(crate) fn media_file_of(file: &ScannedFile) -> MediaFile {
    let mut row = MediaFile::new(file.kind, directory_of(file), name_of(file));
    refresh_media_file(&mut row, file);
    row
}

/// Updates the parts of a stored row that the scan owns, leaving media info alone.
pub(crate) fn refresh_media_file(row: &mut MediaFile, file: &ScannedFile) {
    row.kind = file.kind;
    row.size_bytes = file.size_bytes;
    row.modified_ms = file.modified_ms;
    row.stacking = file.stacking;
    row.stacking_marker.clone_from(&file.stacking_marker);
    row.is_iso = Path::new(&row.filename)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("iso"));
}

fn provider_ids(ids: &ParsedIds) -> ProviderIds {
    let mut out = ProviderIds::new();
    if let Some(imdb) = &ids.imdb {
        out.set(id_key::IMDB, imdb);
    }
    if let Some(tmdb) = ids.tmdb {
        out.set(id_key::TMDB, tmdb.to_string());
    }
    if let Some(tvdb) = &ids.tvdb {
        out.set(id_key::TVDB, tvdb);
    }
    out
}

fn first_video_name(files: &[ScannedFile]) -> String {
    files
        .iter()
        .find(|f| f.kind == MediaFileType::Video)
        .map(name_of)
        .unwrap_or_default()
}

/// True when the videos are the parts of one stacked movie or episode.
pub(crate) fn is_stacked(files: &[ScannedFile]) -> bool {
    files
        .iter()
        .filter(|f| f.kind == MediaFileType::Video && f.stacking > 0)
        .count()
        > 1
}

/// The new movie for a scanned group, flagged by the caller.
pub(crate) fn new_movie(movie: &ScannedMovie, data_source: &str, now_ms: i64) -> Item {
    let mut item = Item::movie(&movie.title);
    item.core.data_source = data_source.to_owned();
    item.core.path = movie.path.to_string_lossy().into_owned();
    item.core.year = movie.year.unwrap_or(0);
    item.core.ids = provider_ids(&movie.ids);
    item.core.date_added_ms = now_ms;
    item.core.original_filename = first_video_name(&movie.files);
    item.core.media_files = movie.files.iter().map(media_file_of).collect();
    if let ItemDetail::Movie(m) = &mut item.detail {
        m.edition = movie.edition.clone();
        m.media_source = movie.media_source.clone();
        m.video_in_3d = movie.three_d;
        m.multi_movie_dir = movie.multi_movie_dir;
        m.is_disc = movie.disc;
        m.stacked = is_stacked(&movie.files);
    }
    item
}

/// The new show for a scanned group.
pub(crate) fn new_show(show: &ScannedShow, data_source: &str, now_ms: i64) -> Item {
    let mut item = Item::show(&show.title);
    item.core.data_source = data_source.to_owned();
    item.core.path = show.path.to_string_lossy().into_owned();
    item.core.year = show.year.unwrap_or(0);
    item.core.ids = provider_ids(&show.ids);
    item.core.date_added_ms = now_ms;
    item.core.media_files = show.files.iter().map(media_file_of).collect();
    item
}

/// A season of a show. `files` are its artwork and NFO.
pub(crate) fn new_season(
    number: u32,
    files: &[ScannedFile],
    show_path: &str,
    data_source: &str,
    now_ms: i64,
) -> Item {
    // The parent is set when the show's id is known.
    let mut item = Item::season(ItemId(0), number);
    item.parent_id = None;
    item.core.data_source = data_source.to_owned();
    item.core.path = show_path.to_owned();
    item.core.date_added_ms = now_ms;
    item.core.media_files = files.iter().map(media_file_of).collect();
    item
}

/// The season an episode is filed under: the one found, else season 1, else the year of its
/// date.
pub(crate) fn season_of(episode: &ScannedEpisode) -> u32 {
    episode
        .season
        .or_else(|| episode.date.as_ref().map(|d| d.year))
        .unwrap_or(1)
}

/// The episode items for one scanned episode: one per episode number, all sharing its files; a
/// single item when no number was found.
pub(crate) fn new_episodes(episode: &ScannedEpisode, data_source: &str, now_ms: i64) -> Vec<Item> {
    let files: Vec<MediaFile> = episode.files.iter().map(media_file_of).collect();
    let folder = episode
        .files
        .iter()
        .find(|f| f.kind == MediaFileType::Video)
        .map(directory_of)
        .unwrap_or_default();
    // A numbered episode without a title in its name has no title; the number names it. Only
    // an episode with neither number nor date is known by its file name.
    let unplaced = episode.numbers.is_empty() && episode.date.is_none();
    let title = if episode.title.trim().is_empty() && unplaced {
        let stem = first_video_name(&episode.files);
        Path::new(&stem)
            .file_stem()
            .map_or(stem.clone(), |s| s.to_string_lossy().into_owned())
    } else {
        episode.title.clone()
    };
    let season = season_of(episode);
    let numbers: Vec<Option<u32>> = if episode.numbers.is_empty() {
        vec![None]
    } else {
        episode.numbers.iter().copied().map(Some).collect()
    };
    numbers
        .into_iter()
        .map(|number| {
            let mut item = Item::episode(ItemId(0), title.clone());
            item.parent_id = None;
            item.core.data_source = data_source.to_owned();
            item.core.path.clone_from(&folder);
            item.core.date_added_ms = now_ms;
            item.core.original_filename = first_video_name(&episode.files);
            item.core.media_files = files.clone();
            if let ItemDetail::Episode(e) = &mut item.detail {
                if let Some(n) = number {
                    e.numbers.set(EpisodeNumber::aired(season, n));
                }
                e.first_aired = episode
                    .date
                    .as_ref()
                    .and_then(|d| calendar_date(d.year, d.month, d.day));
                e.is_disc = episode.disc;
                e.multi_episode = episode.numbers.len() > 1;
                e.stacked = is_stacked(&episode.files);
            }
            item
        })
        .collect()
}

fn calendar_date(year: u32, month: u32, day: u32) -> Option<CalendarDate> {
    CalendarDate::new(
        i32::try_from(year).ok()?,
        u8::try_from(month).ok()?,
        u8::try_from(day).ok()?,
    )
}
