//! What the library holds of a title: the link shown on the Torrent search page and the copy a
//! release is compared with for the "Better than your copy" marker.

use cv_db::Database;
use cv_db::repo::{children_of, get_item, items_by_provider_id};
use cv_domain::{Item, ItemDetail, ItemId, MediaFile, MediaFileType, MediaSource};
use cv_torrent::model::{Hdr, LibraryCopy, MediaKind, Resolution, Scope};

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
    /// A movie's copy: its largest video with media info.
    Movie(MovieCopy),
    /// The episodes of a show that have a video, as `(season, episode)` in aired order.
    Show(Vec<(u16, u16)>),
}

/// A movie's copy as the header names it: "1080p WEB-DL · 5.6 GB · <file name>".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovieCopy {
    pub resolution: Resolution,
    pub source: MediaSource,
    pub hdr: Hdr,
    pub size_bytes: u64,
    pub file_name: String,
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
    Some(MovieCopy {
        resolution: resolution_of(video.width, video.height),
        source: source_of(item),
        hdr: hdr_of(&video.hdr_formats),
        size_bytes: video.size_bytes,
        file_name: video.filename.clone(),
    })
}

/// What the library holds of a linked title: a movie's copy, or the episodes of a show that have
/// a video. `None` when a movie has no video.
pub(super) fn holding_of(
    db: &Database,
    link: &LibraryLink,
) -> Result<Option<LibraryHolding>, cv_db::DbError> {
    let Some(item) = db.read(|c| get_item(c, link.item))? else {
        return Ok(None);
    };
    match &item.detail {
        ItemDetail::Movie(_) => Ok(movie_copy_of(&item).map(LibraryHolding::Movie)),
        ItemDetail::Show(_) => {
            let mut held = Vec::new();
            for season_id in db.read(|c| children_of(c, link.item))? {
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
                        && let (Ok(season), Ok(episode)) =
                            (u16::try_from(number.season), u16::try_from(number.episode))
                    {
                        held.push((season, episode));
                    }
                }
            }
            held.sort_unstable();
            held.dedup();
            Ok(Some(LibraryHolding::Show(held)))
        }
        _ => Ok(None),
    }
}

/// The copy of a movie, or of an episode of a show, that the library holds for the scope
/// searched. A whole show or a season has no single copy.
pub(super) fn library_copy(
    db: &Database,
    link: Option<&LibraryLink>,
    scope: Scope,
) -> Result<Option<LibraryCopy>, cv_db::DbError> {
    let Some(link) = link else {
        return Ok(None);
    };
    match scope {
        Scope::Movie => Ok(db
            .read(|c| get_item(c, link.item))?
            .and_then(|i| copy_of(&i))),
        Scope::Episode(season, episode) => {
            let seasons = db.read(|c| children_of(c, link.item))?;
            for season_id in seasons {
                let Some(season_item) = db.read(|c| get_item(c, season_id))? else {
                    continue;
                };
                let ItemDetail::Season(fields) = &season_item.detail else {
                    continue;
                };
                if fields.season != u32::from(season) {
                    continue;
                }
                for episode_id in db.read(|c| children_of(c, season_id))? {
                    let Some(item) = db.read(|c| get_item(c, episode_id))? else {
                        continue;
                    };
                    if let ItemDetail::Episode(fields) = &item.detail
                        && fields
                            .numbers
                            .display(&cv_domain::EpisodeGroup::aired())
                            .is_some_and(|n| n.episode == u32::from(episode))
                    {
                        return Ok(copy_of(&item));
                    }
                }
            }
            Ok(None)
        }
        Scope::Show | Scope::Season(_) => Ok(None),
    }
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
}
