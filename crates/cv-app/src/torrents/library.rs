//! What the library holds of a title: the link shown on the Torrent search page and the copy a
//! release is compared with for the "Better than your copy" marker.

use cv_db::Database;
use cv_db::repo::{children_of, get_item, items_by_provider_id};
use std::cmp::Reverse;

use cv_domain::{CalendarDate, Item, ItemDetail, ItemId, MediaFile, MediaFileType, MediaSource};
use cv_template::tokens::{audio_channels_dot, edition_label};
use cv_torrent::model::{Hdr, LibraryCopy, MediaKind, Resolution, Scope};
use cv_torrent::rank::copy_rank;

use crate::collections::today;
use crate::hero_audio::hero_audio;

/// A library title, for the "In library" badge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryLink {
    pub item: ItemId,
    /// `Title (Year)`.
    pub name: String,
}

fn name_of(item: &Item) -> String {
    if item.core.year > 0 {
        format!("{} ({})", item.core.title, item.core.year)
    } else {
        item.core.title.clone()
    }
}

/// The library movie or show with this TMDB ID, if there is one. A movie and a show can share a
/// TMDB number, so the kind is checked.
pub(super) fn linked_title(
    db: &Database,
    kind: MediaKind,
    tmdb_id: u64,
) -> Result<Option<LibraryLink>, cv_db::DbError> {
    let candidates = db.read(|c| items_by_provider_id(c, "tmdb", &tmdb_id.to_string()))?;
    for id in candidates {
        let Some(item) = db.read(|c| get_item(c, id))? else {
            continue;
        };
        let matches_kind = matches!(
            (&item.detail, kind),
            (ItemDetail::Movie(_), MediaKind::Movie) | (ItemDetail::Show(_), MediaKind::Show)
        );
        if matches_kind {
            return Ok(Some(LibraryLink {
                item: id,
                name: name_of(&item),
            }));
        }
    }
    Ok(None)
}

/// The resolution class of a video from its frame size. A wide picture with black bars is as
/// wide as its resolution class, so the width counts too.
#[must_use]
pub fn resolution_of(width: u32, height: u32) -> Resolution {
    if width >= 3200 || height >= 2000 {
        Resolution::R2160
    } else if width >= 1800 || height >= 1000 {
        Resolution::R1080
    } else if width >= 1200 || height >= 700 {
        Resolution::R720
    } else if width > 0 || height > 0 {
        Resolution::R480
    } else {
        Resolution::Unknown
    }
}

/// The best HDR format named in a file's HDR list.
#[must_use]
pub fn hdr_of(formats: &[String]) -> Hdr {
    let has = |needle: &str| formats.iter().any(|f| f.to_lowercase().contains(needle));
    if has("dolby") || has("dovi") {
        Hdr::DolbyVision
    } else if has("hdr10+") || has("hdr10plus") {
        Hdr::Hdr10Plus
    } else if has("hdr") {
        Hdr::Hdr10
    } else if has("hlg") {
        Hdr::Hlg
    } else {
        Hdr::None
    }
}

fn source_of(item: &Item) -> MediaSource {
    match &item.detail {
        ItemDetail::Movie(movie) => movie.media_source.clone(),
        ItemDetail::Episode(episode) => episode.media_source.clone(),
        _ => MediaSource::Unknown,
    }
}

/// The copy a movie or episode item holds: its largest video with media info.
#[must_use]
pub fn copy_of(item: &Item) -> Option<LibraryCopy> {
    let video: &MediaFile = item
        .core
        .media_files
        .iter()
        .filter(|f| f.kind == MediaFileType::Video && f.has_media_info())
        .max_by_key(|f| f.size_bytes)?;
    Some(LibraryCopy {
        resolution: resolution_of(video.width, video.height),
        source: source_of(item),
        hdr: hdr_of(&video.hdr_formats),
    })
}

/// What the library holds of a title, for the Torrent search header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryHolding {
    /// Every copy of a movie (each library movie with its TMDB ID that has a video), best first:
    /// the first is the one releases are compared with.
    Movie(Vec<MovieCopy>),
    Show(ShowHolding),
}

/// The episodes of a show the library holds and misses, as `(season, episode)` in aired order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShowHolding {
    /// The episodes that have a video.
    pub held: Vec<(u16, u16)>,
    /// The aired episodes the provider lists that have no video in the library.
    pub missing: Vec<(u16, u16)>,
}

/// A movie's copy as the header names it: "2160p · DV · HEVC · TrueHD Atmos 7.1 · Remux · 58 GB".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovieCopy {
    /// The library movie this copy is.
    pub item: ItemId,
    pub resolution: Resolution,
    pub source: MediaSource,
    pub hdr: Hdr,
    /// The video codec as the media info names it (`HEVC`, `AVC`, `AV1`); empty when not read.
    pub codec: String,
    /// The lead audio track: `TrueHD Atmos 7.1`; empty when not read.
    pub audio: String,
    /// `Extended Edition`; empty when the movie has none.
    pub edition: String,
    /// The languages of the audio tracks (ISO 639 codes as stored), in file order, once each.
    pub languages: Vec<String>,
    pub size_bytes: u64,
    pub file_name: String,
}

impl MovieCopy {
    /// The copy as the release comparison sees it.
    #[must_use]
    pub fn compared(&self) -> LibraryCopy {
        LibraryCopy {
            resolution: self.resolution,
            source: self.source.clone(),
            hdr: self.hdr,
        }
    }
}

/// "TrueHD Atmos 7.1": the codec and channels of the track a viewer would hear first.
fn audio_of(video: &MediaFile, original_language: &str) -> String {
    let Some(stream) = hero_audio(&video.audio_streams, original_language) else {
        return String::new();
    };
    let text = if stream.channels == 0 {
        stream.codec.clone()
    } else {
        format!("{} {}", stream.codec, audio_channels_dot(stream.channels))
    };
    text.trim().to_owned()
}

/// The copy of a movie: its largest video, with the resolution when its media info was read.
#[must_use]
pub fn movie_copy_of(item: &Item) -> Option<MovieCopy> {
    let video: &MediaFile = item
        .core
        .media_files
        .iter()
        .filter(|f| f.kind == MediaFileType::Video)
        .max_by_key(|f| f.size_bytes)?;
    let mut languages: Vec<String> = Vec::new();
    for stream in &video.audio_streams {
        let language = stream.language.trim();
        if !language.is_empty() && !languages.iter().any(|known| known == language) {
            languages.push(language.to_owned());
        }
    }
    let edition = match &item.detail {
        ItemDetail::Movie(movie) => edition_label(&movie.edition),
        _ => String::new(),
    };
    Some(MovieCopy {
        item: item.id?,
        resolution: resolution_of(video.width, video.height),
        source: source_of(item),
        hdr: hdr_of(&video.hdr_formats),
        codec: video.video_codec.trim().to_owned(),
        audio: audio_of(video, &item.core.original_language),
        edition,
        languages,
        size_bytes: video.size_bytes,
        file_name: video.filename.clone(),
    })
}

/// Best first: the order [`copy_rank`] gives, then the larger file.
fn sort_best_first(copies: &mut [MovieCopy]) {
    copies.sort_by_key(|copy| Reverse((copy_rank(&copy.compared()), copy.size_bytes)));
}

/// Every library movie with this TMDB ID.
fn movies_with(db: &Database, tmdb_id: u64) -> Result<Vec<Item>, cv_db::DbError> {
    let mut movies = Vec::new();
    for id in db.read(|c| items_by_provider_id(c, "tmdb", &tmdb_id.to_string()))? {
        if let Some(item) = db.read(|c| get_item(c, id))?
            && matches!(item.detail, ItemDetail::Movie(_))
        {
            movies.push(item);
        }
    }
    Ok(movies)
}

/// The copies of a movie, best first.
fn movie_copies(db: &Database, tmdb_id: u64) -> Result<Vec<MovieCopy>, cv_db::DbError> {
    let mut copies: Vec<MovieCopy> = movies_with(db, tmdb_id)?
        .iter()
        .filter_map(movie_copy_of)
        .collect();
    sort_best_first(&mut copies);
    Ok(copies)
}

/// The episode items of a show that have a video, with their aired numbers, in aired order.
fn held_episodes(db: &Database, show: ItemId) -> Result<Vec<(u16, u16, Item)>, cv_db::DbError> {
    let mut held = Vec::new();
    for season_id in db.read(|c| children_of(c, show))? {
        for episode_id in db.read(|c| children_of(c, season_id))? {
            let Some(episode) = db.read(|c| get_item(c, episode_id))? else {
                continue;
            };
            let ItemDetail::Episode(fields) = &episode.detail else {
                continue;
            };
            let has_video = episode
                .core
                .media_files
                .iter()
                .any(|f| f.kind == MediaFileType::Video);
            let number = fields.numbers.display(&cv_domain::EpisodeGroup::aired());
            if let (true, Some(number)) = (has_video, number)
                && let (Ok(season), Ok(number)) =
                    (u16::try_from(number.season), u16::try_from(number.episode))
            {
                held.push((season, number, episode));
            }
        }
    }
    held.sort_by_key(|(season, episode, _)| (*season, *episode));
    Ok(held)
}

/// The aired episodes of a show's provider list that the library holds no video of.
fn missing_of(show: &Item, held: &[(u16, u16)], today: CalendarDate) -> Vec<(u16, u16)> {
    let ItemDetail::Show(fields) = &show.detail else {
        return Vec::new();
    };
    let mut missing: Vec<(u16, u16)> = fields
        .missing_episodes
        .iter()
        .filter(|episode| episode.first_aired.is_some_and(|aired| aired <= today))
        .filter_map(|episode| {
            let number = episode.numbers.display(&cv_domain::EpisodeGroup::aired())?;
            Some((
                u16::try_from(number.season).ok()?,
                u16::try_from(number.episode).ok()?,
            ))
        })
        .filter(|number| held.binary_search(number).is_err())
        .collect();
    missing.sort_unstable();
    missing.dedup();
    missing
}

/// What the library holds of a linked title: every copy of the movie with its TMDB ID, or the
/// episodes of a show that have a video and the aired ones it misses. `None` when a movie has
/// no video.
pub(super) fn holding_of(
    db: &Database,
    link: &LibraryLink,
    tmdb_id: u64,
) -> Result<Option<LibraryHolding>, cv_db::DbError> {
    let Some(item) = db.read(|c| get_item(c, link.item))? else {
        return Ok(None);
    };
    match &item.detail {
        ItemDetail::Movie(_) => {
            let copies = movie_copies(db, tmdb_id)?;
            Ok((!copies.is_empty()).then_some(LibraryHolding::Movie(copies)))
        }
        ItemDetail::Show(_) => {
            let mut held: Vec<(u16, u16)> = held_episodes(db, link.item)?
                .into_iter()
                .map(|(season, episode, _)| (season, episode))
                .collect();
            held.dedup();
            let missing = missing_of(&item, &held, today());
            Ok(Some(LibraryHolding::Show(ShowHolding { held, missing })))
        }
        _ => Ok(None),
    }
}

/// The quality most of these copies share; of equally many, the lower one.
fn shared_quality(copies: impl IntoIterator<Item = LibraryCopy>) -> Option<LibraryCopy> {
    let mut counts: Vec<(LibraryCopy, usize)> = Vec::new();
    for copy in copies {
        match counts.iter_mut().find(|(known, _)| *known == copy) {
            Some((_, count)) => *count += 1,
            None => counts.push((copy, 1)),
        }
    }
    counts
        .into_iter()
        .max_by_key(|(copy, count)| (*count, Reverse(copy_rank(copy))))
        .map(|(copy, _)| copy)
}

/// The copy the library holds for the scope searched, the one releases are compared with: the
/// best copy of a movie, the copy of an episode, or for a whole show or a season the quality
/// most of its held episodes share.
pub(super) fn library_copy(
    db: &Database,
    link: Option<&LibraryLink>,
    tmdb_id: Option<u64>,
    scope: Scope,
) -> Result<Option<LibraryCopy>, cv_db::DbError> {
    let Some(link) = link else {
        return Ok(None);
    };
    if scope == Scope::Movie {
        let mut copies: Vec<LibraryCopy> = match tmdb_id {
            Some(tmdb_id) => movies_with(db, tmdb_id)?
                .iter()
                .filter_map(copy_of)
                .collect(),
            None => Vec::new(),
        };
        if copies.is_empty() {
            // The linked movie itself, when no TMDB ID is known.
            copies.extend(
                db.read(|c| get_item(c, link.item))?
                    .and_then(|i| copy_of(&i)),
            );
        }
        return Ok(copies.into_iter().max_by_key(copy_rank));
    }
    let in_scope = held_episodes(db, link.item)?
        .into_iter()
        .filter(|(season, episode, _)| match scope {
            Scope::Episode(s, e) => *season == s && *episode == e,
            Scope::Season(s) => *season == s,
            Scope::Show | Scope::Movie => true,
        })
        .filter_map(|(_, _, item)| copy_of(&item));
    Ok(shared_quality(in_scope))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_sizes_become_resolution_classes() {
        assert_eq!(resolution_of(3840, 2160), Resolution::R2160);
        assert_eq!(resolution_of(3840, 1600), Resolution::R2160, "scope 4K");
        assert_eq!(resolution_of(1920, 1080), Resolution::R1080);
        assert_eq!(
            resolution_of(1920, 800),
            Resolution::R1080,
            "letterboxed 1080p"
        );
        assert_eq!(resolution_of(1280, 720), Resolution::R720);
        assert_eq!(resolution_of(1280, 536), Resolution::R720);
        assert_eq!(resolution_of(720, 480), Resolution::R480);
        assert_eq!(resolution_of(0, 0), Resolution::Unknown);
    }

    #[test]
    fn hdr_lists_give_the_best_format() {
        let list = |items: &[&str]| items.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(hdr_of(&list(&["Dolby Vision", "HDR10"])), Hdr::DolbyVision);
        assert_eq!(hdr_of(&list(&["HDR10+"])), Hdr::Hdr10Plus);
        assert_eq!(hdr_of(&list(&["HDR10"])), Hdr::Hdr10);
        assert_eq!(hdr_of(&list(&["HLG"])), Hdr::Hlg);
        assert_eq!(hdr_of(&[]), Hdr::None);
    }

    #[test]
    fn a_title_without_a_read_video_has_no_copy() {
        let item = Item::movie("Heat");
        assert_eq!(copy_of(&item), None);
    }

    #[test]
    fn the_largest_read_video_is_the_copy() {
        let mut item = Item::movie("Heat");
        if let ItemDetail::Movie(movie) = &mut item.detail {
            movie.media_source = MediaSource::Bluray;
        }
        let mut small = MediaFile::new(MediaFileType::Video, "d", "small.mkv");
        small.container_format = "Matroska".into();
        small.width = 1280;
        small.height = 720;
        small.size_bytes = 1;
        let mut big = MediaFile::new(MediaFileType::Video, "d", "big.mkv");
        big.container_format = "Matroska".into();
        big.width = 3840;
        big.height = 2160;
        big.size_bytes = 5;
        big.hdr_formats = vec!["HDR10".into()];
        let mut unread = MediaFile::new(MediaFileType::Video, "d", "unread.mkv");
        unread.size_bytes = 99;
        item.core.media_files = vec![small, big, unread];
        let copy = copy_of(&item).expect("a copy");
        assert_eq!(copy.resolution, Resolution::R2160);
        assert_eq!(copy.hdr, Hdr::Hdr10);
        assert_eq!(copy.source, MediaSource::Bluray);
    }

    fn missing(
        season: u32,
        episode: u32,
        aired: Option<CalendarDate>,
    ) -> cv_domain::MissingEpisode {
        let mut numbers = cv_domain::EpisodeNumbers::new();
        numbers.set(cv_domain::EpisodeNumber::aired(season, episode));
        cv_domain::MissingEpisode {
            title: String::new(),
            numbers,
            first_aired: aired,
            plot: String::new(),
        }
    }

    #[test]
    fn missing_episodes_are_the_aired_ones_without_a_video() {
        let day = |y, m, d| CalendarDate::new(y, m, d);
        let mut show = Item::show("Example Show");
        if let ItemDetail::Show(fields) = &mut show.detail {
            fields.missing_episodes = vec![
                missing(4, 6, day(2026, 3, 1)),
                missing(4, 5, day(2026, 2, 22)),
                missing(4, 9, day(2027, 1, 1)),
                missing(4, 10, None),
                missing(1, 2, day(2020, 1, 1)),
            ];
        }
        let held = [(1, 1), (1, 2)];
        let today = day(2026, 11, 3).expect("a date");
        assert_eq!(
            missing_of(&show, &held, today),
            [(4, 5), (4, 6)],
            "unaired, undated and held episodes are not missing"
        );
    }

    #[test]
    fn a_show_is_compared_with_the_quality_most_episodes_share() {
        let copy = |resolution| LibraryCopy {
            resolution,
            source: MediaSource::WebDl,
            hdr: Hdr::None,
        };
        let shared = shared_quality([
            copy(Resolution::R1080),
            copy(Resolution::R720),
            copy(Resolution::R1080),
        ]);
        assert_eq!(shared, Some(copy(Resolution::R1080)));
        let tie = shared_quality([copy(Resolution::R2160), copy(Resolution::R720)]);
        assert_eq!(tie, Some(copy(Resolution::R720)), "a tie goes to the lower");
        assert_eq!(shared_quality([]), None);
    }
}
