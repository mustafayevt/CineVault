//! Library items to NFO writer models.
//!
//! The writers in `cv-nfo` print a plain model and read nothing else, so this module resolves
//! everything a model needs from an item and its relatives: timestamps in local time, the set a
//! movie belongs to, the video file and the files next to it, and for a show the values derived
//! from its episodes (watched state, season dates, guest actors). Locks and the values carried
//! over from the old file are applied by the caller afterwards.

use cv_domain::{
    CreditKind, EpisodeGroup, EpisodeNumber, EpisodeOrdering, Item, ItemDetail, MediaFile,
    MediaFileType, id_key,
};
use cv_nfo::DateAddedField;
use cv_nfo::{
    AirDate, EpisodeNfo, MovieArtwork, MovieNfo, MovieNfoOptions, MovieSetNfo, MovieSetRef,
    SeasonArtwork, SeasonNfo, ShowArtwork, ShowNfo, VideoInfo, split_episode_list, split_show_list,
};

use super::time::{file_times, local_from_millis};

/// The video file an item's NFO is about: the first part of a stack, else the first video.
pub fn main_video(item: &Item) -> Option<&MediaFile> {
    item.core
        .media_files
        .iter()
        .filter(|f| {
            f.kind == MediaFileType::Video && !f.is_covered_disc_marker(&item.core.media_files)
        })
        .min_by_key(|f| if f.stacking == 0 { 1 } else { f.stacking })
}

/// Whether the options read the file system times of the video file.
fn wants_file_times(options: &MovieNfoOptions) -> bool {
    options.write_date_added
        && matches!(
            options.date_added_field,
            DateAddedField::FileCreationDate | DateAddedField::FileLastModifiedDate
        )
}

fn video_info(file: &MediaFile, options: &MovieNfoOptions) -> VideoInfo {
    let mut info = VideoInfo::from_media_file(file);
    if wants_file_times(options) {
        let (created, modified) = file_times(&file.path());
        info.created = created;
        info.modified = modified;
    }
    info
}

fn streams_of(item: &Item, kind: MediaFileType) -> impl Iterator<Item = &MediaFile> {
    item.core.media_files.iter().filter(move |f| f.kind == kind)
}

/// The stream facts of an item's video: those of the main video, with the duration and size of
/// every part when the video is stacked (`Movie CD1`, `Movie CD2`), the parts are added up.
fn item_video_info(item: &Item, main: &MediaFile, options: &MovieNfoOptions) -> VideoInfo {
    let mut info = video_info(main, options);
    let parts: Vec<&MediaFile> = item
        .core
        .media_files
        .iter()
        .filter(|f| {
            f.kind == MediaFileType::Video
                && f.stacking > 0
                && !f.is_covered_disc_marker(&item.core.media_files)
        })
        .collect();
    if main.stacking > 0 && parts.len() > 1 {
        info.duration_secs = parts.iter().map(|f| f.duration_secs).sum();
        info.size_bytes = parts.iter().map(|f| f.size_bytes).sum();
    }
    info
}

/// The subtitle files next to an item's video, one stream each: what the media info read,
/// with the language, the forced and hearing-impaired marks and the title the file name gives
/// filling the gaps (`Heat (1995).deu.forced.srt`). A file the media info found several tracks
/// in (a VobSub pair) lists each of them.
fn sidecar_subtitles(item: &Item) -> Vec<cv_domain::SubtitleStream> {
    let video_stem = main_video(item)
        .map(|v| {
            std::path::Path::new(&v.filename)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        })
        .unwrap_or_default();
    let mut out = Vec::new();
    for file in streams_of(item, MediaFileType::Subtitle) {
        if file.subtitles.len() > 1 {
            out.extend(file.subtitles.iter().cloned());
        } else {
            out.push(cv_media::external::subtitle_file_stream(file, &video_stem));
        }
    }
    out
}

fn tags_of(item: &Item) -> Vec<String> {
    item.core.tags.iter().map(str::to_owned).collect()
}

fn crc32_of(file: &MediaFile) -> Option<String> {
    file.checksums
        .get("crc32")
        .map(|c| c.to_ascii_uppercase())
        .filter(|c| !c.is_empty())
}

// ---- movies and sets ----------------------------------------------------------------------

/// The movie model of a movie item. `set` is the movie set it belongs to, when it has one.
/// `None` when the item is not a movie.
pub fn movie_model(item: &Item, set: Option<&Item>, options: &MovieNfoOptions) -> Option<MovieNfo> {
    let ItemDetail::Movie(m) = &item.detail else {
        return None;
    };
    let c = &item.core;
    let video = main_video(item);
    let mut movie = MovieNfo {
        title: c.title.clone(),
        original_title: c.original_title.clone(),
        sort_title: m.sort_title.clone(),
        english_title: m.english_title.clone(),
        original_language: c.original_language.clone(),
        video_in_3d: m.video_in_3d,
        year: u32::from(c.year),
        plot: c.plot.clone(),
        outline: c.outline.clone(),
        tagline: m.tagline.clone(),
        runtime_minutes: m.runtime_minutes,
        top250: m.top250,
        ids: c.ids.clone(),
        ratings: c.ratings.clone(),
        certification: m.certification.clone(),
        genres: m.genres.clone(),
        countries: nfo_countries(&m.countries),
        studios: c.production_companies.clone(),
        spoken_languages: m.spoken_languages.clone(),
        tags: tags_of(item),
        credits: c.credits.clone(),
        trailers: m.trailers.clone(),
        show_links: m.show_links.clone(),
        movie_set: set.map(|s| MovieSetRef {
            title: s.core.title.clone(),
            overview: s.core.plot.clone(),
            tmdb_id: s.core.ids.get_u64("tmdb"),
        }),
        artwork: MovieArtwork::from_urls(&nfo_urls(c), &shareable_list(&m.extra_fanarts)),
        release_date: m.release_date,
        date_added: local_from_millis(c.date_added_ms),
        watched: m.watched,
        play_count: m.play_count,
        last_played: m.last_watched_ms.and_then(local_from_millis),
        media_source: m.media_source.clone(),
        edition: m.edition.clone(),
        original_filename: c.original_filename.clone(),
        note: c.note.clone(),
        is_disc: m.is_disc,
        ..MovieNfo::default()
    };
    // The Jellyfin collection number is the TMDB ID of the set the movie belongs to.
    if let Some(id) = set.and_then(|s| s.core.ids.get("tmdb"))
        && movie.ids.get(id_key::TMDB_SET).is_none()
    {
        movie.ids.set(id_key::TMDB_SET, id);
    }
    movie.artwork.extra_thumbs = shareable_list(&m.extra_thumbs);
    if let Some(file) = video {
        movie.crc32 = crc32_of(file);
        movie.video = item_video_info(item, file, options);
    }
    for file in streams_of(item, MediaFileType::Audio) {
        movie.external_audio.extend(file.audio_streams.clone());
    }
    movie.external_subtitles = sidecar_subtitles(item);
    Some(movie)
}

/// The countries as NFO files carry them: the library stores codes (`JP`), the files hold the
/// English names (`Japan`), as Kodi-format NFO files carry them. Text that is not a known code stays.
fn nfo_countries(countries: &[String]) -> Vec<String> {
    countries
        .iter()
        .map(|c| cv_domain::country_display(c))
        .filter(|c| !c.is_empty())
        .collect()
}

/// Whether an artwork address may go into an NFO. A `file:` address is an image the user chose
/// from this computer: the library remembers it (Re-download reads it again), but it is a path
/// outside the library that a media center cannot use, so the NFO leaves it out.
fn shareable(url: &str) -> bool {
    !url.trim()
        .get(..5)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("file:"))
}

/// The artwork addresses of an item an NFO may carry.
fn nfo_urls(core: &cv_domain::ItemCore) -> std::collections::BTreeMap<MediaFileType, String> {
    core.artwork_urls
        .iter()
        .filter(|(_, url)| shareable(url))
        .map(|(kind, url)| (*kind, url.clone()))
        .collect()
}

fn shareable_list(urls: &[String]) -> Vec<String> {
    urls.iter().filter(|url| shareable(url)).cloned().collect()
}

/// The set model of a set item. `None` when the item is not a set.
pub fn set_model(item: &Item) -> Option<MovieSetNfo> {
    let ItemDetail::Set(s) = &item.detail else {
        return None;
    };
    let c = &item.core;
    let urls = nfo_urls(c);
    let url = |kind| urls.get(&kind).cloned().unwrap_or_default();
    Some(MovieSetNfo {
        title: c.title.clone(),
        sort_title: s.sort_title.clone(),
        overview: c.plot.clone(),
        ids: c.ids.clone(),
        tags: tags_of(item),
        date_added: local_from_millis(c.date_added_ms),
        note: c.note.clone(),
        english_title: s.english_title.clone(),
        poster: url(MediaFileType::Poster),
        fanart: url(MediaFileType::Fanart),
    })
}

// ---- shows, seasons and episodes ----------------------------------------------------------

/// A show with its seasons and episodes, as loaded from the library.
#[derive(Debug, Clone, Copy)]
pub struct ShowTree<'a> {
    pub show: &'a Item,
    pub seasons: &'a [Item],
    pub episodes: &'a [Item],
}

fn tv_number(
    episode: &Item,
    group: &EpisodeGroup,
    ordering: EpisodeOrdering,
) -> Option<(u32, u32)> {
    let ItemDetail::Episode(e) = &episode.detail else {
        return None;
    };
    e.numbers
        .get(group)
        .or_else(|| e.numbers.iter().find(|n| n.group.ordering == ordering))
        .map(|n| (n.season, n.episode))
}

/// The season and episode number an episode is shown with in the show's ordering, falling back
/// to its aired number, then to `(0, 0)`.
pub fn shown_number(episode: &Item, active: &EpisodeGroup) -> (u32, u32) {
    tv_number(episode, active, active.ordering)
        .or_else(|| aired_number(episode))
        .unwrap_or((0, 0))
}

/// The aired season and episode number of an episode.
pub fn aired_number(episode: &Item) -> Option<(u32, u32)> {
    tv_number(episode, &EpisodeGroup::aired(), EpisodeOrdering::Aired)
}

fn show_studios(show: &Item) -> String {
    show.core.production_companies.join(", ")
}

fn show_artwork(show: &Item, extra: &[String]) -> ShowArtwork {
    let urls = nfo_urls(&show.core);
    let url = |kind| urls.get(&kind).cloned().unwrap_or_default();
    ShowArtwork {
        poster: url(MediaFileType::Poster),
        banner: url(MediaFileType::Banner),
        clearart: url(MediaFileType::Clearart),
        clearlogo: url(MediaFileType::Clearlogo),
        thumb: url(MediaFileType::Thumb),
        keyart: url(MediaFileType::Keyart),
        squareart: url(MediaFileType::Squareart),
        logo: String::new(),
        characterart: url(MediaFileType::Characterart),
        disc: url(MediaFileType::Disc),
        fanart: url(MediaFileType::Fanart),
        extra_fanarts: shareable_list(extra),
    }
}

/// The seasons that have episodes (or a season record), in order, with their text, artwork and
/// the first air date of their episodes.
fn season_models(tree: &ShowTree<'_>, active: &EpisodeGroup) -> Vec<SeasonNfo> {
    let ItemDetail::Show(show) = &tree.show.detail else {
        return Vec::new();
    };
    let mut numbers: Vec<u32> = tree
        .episodes
        .iter()
        .map(|e| aired_number(e).map_or(0, |n| n.0))
        .collect();
    for season in tree.seasons {
        if let ItemDetail::Season(s) = &season.detail {
            numbers.push(s.season);
        }
    }
    numbers.sort_unstable();
    numbers.dedup();
    numbers
        .into_iter()
        .map(|number| {
            let record = tree
                .seasons
                .iter()
                .find(|s| matches!(&s.detail, ItemDetail::Season(f) if f.season == number));
            let text = show
                .season_texts
                .iter()
                .find(|t| t.season == number && &t.group == active)
                .or_else(|| {
                    show.season_texts
                        .iter()
                        .find(|t| t.season == number && t.group == EpisodeGroup::aired())
                });
            let art = |kind, fallback| {
                record
                    .map(|s| {
                        let urls = nfo_urls(&s.core);
                        urls.get(&kind)
                            .or_else(|| urls.get(&fallback))
                            .cloned()
                            .unwrap_or_default()
                    })
                    .unwrap_or_default()
            };
            SeasonNfo {
                number,
                title: text.map(|t| t.name.clone()).unwrap_or_default(),
                plot: text.map(|t| t.overview.clone()).unwrap_or_default(),
                note: record.map(|s| s.core.note.clone()).unwrap_or_default(),
                first_aired: tree
                    .episodes
                    .iter()
                    .filter(|e| aired_number(e).map_or(0, |n| n.0) == number)
                    .filter_map(|e| match &e.detail {
                        ItemDetail::Episode(f) => f.first_aired,
                        _ => None,
                    })
                    .min(),
                artwork: SeasonArtwork {
                    poster: art(MediaFileType::SeasonPoster, MediaFileType::Poster),
                    banner: art(MediaFileType::SeasonBanner, MediaFileType::Banner),
                    thumb: art(MediaFileType::SeasonThumb, MediaFileType::Thumb),
                    fanart: art(MediaFileType::SeasonFanart, MediaFileType::Fanart),
                },
                unknown_elements: Vec::new(),
            }
        })
        .collect()
}

/// The show model (with its seasons) of a show tree. `None` when the item is not a show.
pub fn show_model(tree: &ShowTree<'_>, options: &MovieNfoOptions) -> Option<ShowNfo> {
    let ItemDetail::Show(s) = &tree.show.detail else {
        return None;
    };
    let c = &tree.show.core;
    let active = s.episode_group.clone();

    let real_episodes = || {
        tree.episodes.iter().filter_map(|e| match &e.detail {
            ItemDetail::Episode(f) => Some((e, f)),
            _ => None,
        })
    };
    let watched = real_episodes().next().is_some() && real_episodes().all(|(_, f)| f.watched);

    let mut guest_actors: Vec<cv_domain::Credit> = Vec::new();
    for (episode, _) in real_episodes() {
        for credit in &episode.core.credits {
            if credit.kind == CreditKind::Guest
                && !guest_actors
                    .iter()
                    .any(|g| g.person.name == credit.person.name)
            {
                guest_actors.push(credit.clone());
            }
        }
    }

    // File times of the episodes' videos, only when the option reads them.
    let mut created: Vec<String> = Vec::new();
    let mut modified: Vec<String> = Vec::new();
    if wants_file_times(options) {
        for (episode, _) in real_episodes() {
            if let Some(file) = main_video(episode) {
                let (c, m) = file_times(&file.path());
                created.extend(c);
                modified.extend(m);
            }
        }
    }

    let mut air_dates: Vec<AirDate> = real_episodes()
        .filter_map(|(e, f)| {
            Some(AirDate {
                season: aired_number(e).map_or(0, |n| n.0),
                date: f.first_aired?,
            })
        })
        .collect();
    air_dates.extend(s.missing_episodes.iter().filter_map(|m| {
        let season = m
            .numbers
            .get(&EpisodeGroup::aired())
            .or_else(|| m.numbers.iter().next())
            .map_or(0, |n| n.season);
        Some(AirDate {
            season,
            date: m.first_aired?,
        })
    }));

    Some(ShowNfo {
        title: c.title.clone(),
        original_title: c.original_title.clone(),
        sort_title: s.sort_title.clone(),
        english_title: s.english_title.clone(),
        original_language: c.original_language.clone(),
        year_end: u32::from(s.year_end),
        year: u32::from(c.year),
        plot: c.plot.clone(),
        outline: c.outline.clone(),
        tagline: s.tagline.clone(),
        runtime_minutes: s.runtime_minutes,
        top250: s.top250,
        ids: c.ids.clone(),
        ratings: c.ratings.clone(),
        certification: s.certification.clone(),
        genres: s.genres.clone(),
        countries: nfo_countries(&split_show_list(&s.country)),
        studios: c.production_companies.clone(),
        tags: tags_of(tree.show),
        credits: c.credits.clone(),
        guest_actors,
        trailers: s.trailers.clone(),
        artwork: show_artwork(tree.show, &s.extra_fanart_urls),
        first_aired: s.first_aired,
        status: s.status,
        watched,
        date_added: local_from_millis(c.date_added_ms),
        file_created: created.into_iter().min(),
        file_modified: modified.into_iter().min(),
        note: c.note.clone(),
        last_scraper_id: c.last_scraper_id.clone(),
        episode_groups: s.episode_groups.clone(),
        active_group: active.clone(),
        seasons: season_models(tree, &active),
        episode_air_dates: air_dates,
        ..ShowNfo::default()
    })
}

/// The episode model of an episode item of `show`. `None` when the item is not an episode.
pub fn episode_model(show: &Item, episode: &Item, options: &MovieNfoOptions) -> Option<EpisodeNfo> {
    let ItemDetail::Episode(e) = &episode.detail else {
        return None;
    };
    let ItemDetail::Show(s) = &show.detail else {
        return None;
    };
    let c = &episode.core;
    let (season, number) = shown_number(episode, &s.episode_group);
    let display = e
        .numbers
        .iter()
        .find(|n| n.group.ordering == EpisodeOrdering::Display)
        .map(|n| (n.season, n.episode));
    let own_studios = c.production_companies.join(", ");
    let studios = if own_studios.trim().is_empty() {
        show_studios(show)
    } else {
        own_studios
    };
    let mut nfo = EpisodeNfo {
        title: c.title.clone(),
        original_title: c.original_title.clone(),
        english_title: e.english_title.clone(),
        show_title: show.core.title.clone(),
        season,
        episode: number,
        display,
        numbers: e.numbers.iter().cloned().collect::<Vec<EpisodeNumber>>(),
        ids: c.ids.clone(),
        ratings: c.ratings.clone(),
        plot: c.plot.clone(),
        outline: c.outline.clone(),
        runtime_minutes: e.runtime_minutes,
        thumb: nfo_urls(c)
            .get(&MediaFileType::Thumb)
            .cloned()
            .unwrap_or_default(),
        first_aired: e.first_aired,
        watched: e.watched,
        play_count: e.play_count,
        last_played: e.last_watched_ms.and_then(local_from_millis),
        studios: split_episode_list(&studios),
        tags: tags_of(episode),
        credits: c.credits.clone(),
        date_added: local_from_millis(c.date_added_ms),
        media_source: e.media_source.clone(),
        edition: e.edition.clone(),
        original_filename: c.original_filename.clone(),
        note: c.note.clone(),
        ..EpisodeNfo::default()
    };
    if let Some(file) = main_video(episode) {
        nfo.crc32 = crc32_of(file);
        nfo.video = item_video_info(episode, file, options);
    }
    nfo.external_subtitles = sidecar_subtitles(episode);
    Some(nfo)
}

#[cfg(test)]
mod address_tests {
    use super::*;

    #[test]
    fn a_local_file_chosen_as_artwork_never_reaches_the_nfo() {
        let mut item = Item::movie("Heat");
        item.core.artwork_urls.insert(
            MediaFileType::Poster,
            "file:///C:/Users/me/art/poster.png".to_owned(),
        );
        item.core
            .artwork_urls
            .insert(MediaFileType::Fanart, "https://img.test/f.jpg".to_owned());
        let nfo = movie_model(&item, None, &MovieNfoOptions::default()).expect("a movie");
        assert_eq!(nfo.artwork.poster, "");
        assert_eq!(nfo.artwork.fanart, "https://img.test/f.jpg");
    }

    #[test]
    fn country_codes_are_written_as_english_names() {
        let mut item = Item::movie("13 Assassins");
        if let ItemDetail::Movie(m) = &mut item.detail {
            m.countries = vec!["JP".into(), "GB".into(), "Atlantis".into()];
        }
        let nfo = movie_model(&item, None, &MovieNfoOptions::default()).expect("a movie");
        assert_eq!(nfo.countries, ["Japan", "United Kingdom", "Atlantis"]);
    }

    #[test]
    fn a_stacked_movie_reports_all_its_parts() {
        let mut item = Item::movie("Long Film");
        for (part, minutes) in [(1, 50), (2, 45)] {
            let mut file = MediaFile::new(
                MediaFileType::Video,
                "/m/Long Film",
                format!("Long Film CD{part}.mkv"),
            );
            file.stacking = part;
            file.duration_secs = minutes * 60;
            file.size_bytes = 1_000;
            item.core.media_files.push(file);
        }
        let nfo = movie_model(&item, None, &MovieNfoOptions::default()).expect("a movie");
        assert_eq!(nfo.video.duration_secs, 5_700);
        assert_eq!(nfo.video.size_bytes, 2_000);
    }
}
