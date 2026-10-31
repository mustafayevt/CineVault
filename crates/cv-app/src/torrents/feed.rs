//! What the "Latest releases" feed needs from the app: the sources that can list their newest
//! releases, and the library matched by title and year (no provider is asked) for the "In your
//! library" and "Better than your copy" marks.

use std::collections::HashMap;
use std::sync::Arc;

use cv_db::repo::torrent_landing::{LibraryTitleRow, library_titles};
use cv_domain::{ItemKind, MediaSource};
use cv_torrent::latest::{FeedKind, FeedSupport};
use cv_torrent::model::{LibraryCopy, MediaKind, Resolution};
use cv_torrent::query::match_key;
use cv_torrent::release::ParsedRelease;
use cv_torrent::source::TorrentSource;

use super::library::{LibraryLink, hdr_of, resolution_of};
use super::{TorrentService, TorrentServiceError};

/// A library title a release was matched with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryMatch {
    pub link: LibraryLink,
    pub kind: MediaKind,
    /// A movie's copy, when its video's frame size is known; `None` for a show.
    pub copy: Option<LibraryCopy>,
}

#[derive(Debug, Clone)]
struct Entry {
    found: LibraryMatch,
    year: Option<u16>,
}

/// The library's movies and shows by the match key of their titles.
#[derive(Debug, Clone, Default)]
pub struct LibraryIndex {
    by_key: HashMap<String, Vec<Entry>>,
}

fn name_of(row: &LibraryTitleRow) -> String {
    if row.year > 0 {
        format!("{} ({})", row.title, row.year)
    } else {
        row.title.clone()
    }
}

impl LibraryIndex {
    /// The index of these library rows.
    #[must_use]
    pub fn of(rows: Vec<LibraryTitleRow>) -> Self {
        let mut by_key: HashMap<String, Vec<Entry>> = HashMap::new();
        for row in rows {
            let kind = if row.kind == ItemKind::Show {
                MediaKind::Show
            } else {
                MediaKind::Movie
            };
            let resolution = resolution_of(row.width, row.height);
            let copy = (kind == MediaKind::Movie && resolution != Resolution::Unknown).then(|| {
                LibraryCopy {
                    resolution,
                    source: MediaSource::parse(&row.media_source).unwrap_or_default(),
                    hdr: hdr_of(&row.hdr_formats),
                }
            });
            let entry = Entry {
                found: LibraryMatch {
                    link: LibraryLink {
                        item: row.id,
                        name: name_of(&row),
                    },
                    kind,
                    copy,
                },
                year: (row.year > 0).then_some(row.year),
            };
            let mut keys = vec![match_key(&row.title)];
            let original = match_key(&row.original_title);
            if !original.is_empty() && !keys.contains(&original) {
                keys.push(original);
            }
            for key in keys.into_iter().filter(|key| !key.is_empty()) {
                by_key.entry(key).or_default().push(entry.clone());
            }
        }
        Self { by_key }
    }

    /// How many titles the index holds (a title with an original title counts twice).
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_key.values().map(Vec::len).sum()
    }

    /// Whether the index is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_key.is_empty()
    }

    /// The library title a release is of: the same kind and title, and for a movie the same
    /// year (one either way) when both have one. Of several, the one whose year fits best.
    #[must_use]
    pub fn find(&self, kind: MediaKind, parsed: &ParsedRelease) -> Option<&LibraryMatch> {
        let key = match_key(&parsed.title);
        if key.is_empty() {
            return None;
        }
        self.by_key
            .get(&key)?
            .iter()
            .filter(|entry| entry.found.kind == kind)
            .filter(|entry| match (kind, entry.year, parsed.year) {
                (MediaKind::Movie, Some(held), Some(year)) => held.abs_diff(year) <= 1,
                _ => true,
            })
            .min_by_key(|entry| match (entry.year, parsed.year) {
                (Some(held), Some(year)) => held.abs_diff(year),
                _ => u16::MAX,
            })
            .map(|entry| &entry.found)
    }
}

impl TorrentService {
    /// The sources that can list their newest releases: the switched-on built-in ones (all but
    /// Torrentio, which searches one title at a time), the managed service's indexers and the
    /// user's own Torznab instance. Each still says per feed kind whether it takes part.
    ///
    /// # Errors
    /// `NoticeNotAccepted` before the notice is accepted.
    pub fn feed_sources(&self) -> Result<Vec<Arc<dyn TorrentSource>>, TorrentServiceError> {
        Ok(self
            .enabled_sources()?
            .into_iter()
            .filter(|source| {
                [FeedKind::Movies, FeedKind::Tv]
                    .into_iter()
                    .any(|kind| source.latest_support(kind) == FeedSupport::Yes)
            })
            .collect())
    }

    /// The library's movies and shows for matching feed rows. Only the library is read.
    ///
    /// # Errors
    /// `NoticeNotAccepted` before the notice is accepted; [`TorrentServiceError::Library`] when the
    /// library cannot be read.
    pub fn library_index(&self) -> Result<LibraryIndex, TorrentServiceError> {
        self.ensure_accepted()?;
        let rows = self
            .db
            .read(library_titles)
            .map_err(|e| TorrentServiceError::Library(e.to_string()))?;
        Ok(LibraryIndex::of(rows))
    }
}

#[cfg(test)]
mod tests {
    use cv_domain::ItemId;
    use cv_torrent::release::parse_release;

    use super::*;

    fn row(id: i64, kind: ItemKind, title: &str, year: u16, height: u32) -> LibraryTitleRow {
        LibraryTitleRow {
            id: ItemId(id),
            kind,
            title: title.to_owned(),
            original_title: String::new(),
            year,
            media_source: "WEB-DL".to_owned(),
            width: 0,
            height,
            hdr_formats: Vec::new(),
        }
    }

    fn index() -> LibraryIndex {
        let mut original = row(3, ItemKind::Movie, "The Quiet Harbour", 2026, 0);
        original.original_title = "Le Port Tranquille".to_owned();
        LibraryIndex::of(vec![
            row(1, ItemKind::Movie, "Paper Lanterns", 2025, 1080),
            row(2, ItemKind::Movie, "Paper Lanterns", 1990, 480),
            original,
            row(4, ItemKind::Show, "Night Bakers", 2024, 0),
        ])
    }

    #[test]
    fn a_movie_release_finds_the_library_movie_of_its_title_and_year() {
        let index = index();
        let found = index
            .find(
                MediaKind::Movie,
                &parse_release("Paper.Lanterns.2025.2160p.WEB-DL", &[]),
            )
            .expect("in the library");
        assert_eq!(found.link.item, ItemId(1));
        assert_eq!(found.link.name, "Paper Lanterns (2025)");
        let copy = found.copy.as_ref().expect("a copy");
        assert_eq!(copy.resolution, Resolution::R1080);
        assert_eq!(copy.source, MediaSource::WebDl);
        assert_eq!(
            index
                .find(
                    MediaKind::Movie,
                    &parse_release("Paper.Lanterns.1990.DVDRip", &[])
                )
                .map(|m| m.link.item),
            Some(ItemId(2))
        );
        assert!(
            index
                .find(
                    MediaKind::Movie,
                    &parse_release("Paper.Lanterns.2010.1080p", &[])
                )
                .is_none(),
            "another year is another movie"
        );
    }

    #[test]
    fn the_original_title_matches_and_an_unread_video_has_no_copy() {
        let index = index();
        let found = index
            .find(
                MediaKind::Movie,
                &parse_release("Le.Port.Tranquille.2026.1080p", &[]),
            )
            .expect("by the original title");
        assert_eq!(found.link.item, ItemId(3));
        assert_eq!(found.copy, None);
    }

    #[test]
    fn a_show_matches_any_episode_and_never_a_movie_of_that_name() {
        let index = index();
        let found = index
            .find(
                MediaKind::Show,
                &parse_release("Night.Bakers.S02E01.720p.WEB", &[]),
            )
            .expect("the show");
        assert_eq!(found.link.item, ItemId(4));
        assert_eq!(found.copy, None);
        assert!(
            index
                .find(
                    MediaKind::Movie,
                    &parse_release("Night.Bakers.2024.1080p", &[])
                )
                .is_none()
        );
        assert!(
            index
                .find(MediaKind::Show, &parse_release("Unknown.Show.S01E01", &[]))
                .is_none()
        );
    }
}
