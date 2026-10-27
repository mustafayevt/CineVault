//! The Torrent search landing: the library movies whose copy could be better and the
//! newest additions, each with the TMDB ID that opens it on the page.

use cv_db::repo::torrent_landing::{LandingRow, movies_with_video, recently_added};
use cv_domain::{ItemId, ItemKind};
use cv_torrent::model::{MediaKind, Resolution};

use super::library::resolution_of;
use super::{TorrentService, TorrentServiceError};

/// How many tiles a landing strip shows.
pub const STRIP_LENGTH: usize = 8;

/// A library title on a landing strip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LandingTitle {
    pub item: ItemId,
    pub kind: MediaKind,
    pub tmdb_id: u64,
    pub title: String,
    pub year: Option<u16>,
    /// The resolution class of a movie's copy; `Unknown` for a show or an unread video.
    pub resolution: Resolution,
    /// Epoch milliseconds; `None` when unknown.
    pub added_ms: Option<i64>,
}

/// The two strips of the landing.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Landing {
    /// Movies below 2160p, the lowest resolution first, at most [`STRIP_LENGTH`].
    pub upgrades: Vec<LandingTitle>,
    /// How many movies qualify in all.
    pub upgrade_total: usize,
    /// The newest movies and shows, at most [`STRIP_LENGTH`].
    pub recent: Vec<LandingTitle>,
}

fn title_of(row: LandingRow) -> LandingTitle {
    LandingTitle {
        item: row.id,
        kind: if row.kind == ItemKind::Show {
            MediaKind::Show
        } else {
            MediaKind::Movie
        },
        tmdb_id: row.tmdb_id,
        year: (row.year > 0).then_some(row.year),
        resolution: if row.kind == ItemKind::Movie {
            resolution_of(row.width, row.height)
        } else {
            Resolution::Unknown
        },
        added_ms: (row.date_added_ms > 0).then_some(row.date_added_ms),
        title: row.title,
    }
}

/// The order of the upgrade strip: the copies furthest from 2160p first.
fn rank(resolution: Resolution) -> u8 {
    match resolution {
        Resolution::R480 => 0,
        Resolution::R720 => 1,
        Resolution::R1080 => 2,
        Resolution::R2160 | Resolution::Unknown => 3,
    }
}

/// Picks the landing strips from the library rows.
#[must_use]
pub fn landing_of(movies: Vec<LandingRow>, recent: Vec<LandingRow>) -> Landing {
    let mut upgrades: Vec<LandingTitle> = movies
        .into_iter()
        .map(title_of)
        .filter(|title| !matches!(title.resolution, Resolution::R2160 | Resolution::Unknown))
        .collect();
    // Stable: the same resolution keeps the title order of the query.
    upgrades.sort_by_key(|title| rank(title.resolution));
    let upgrade_total = upgrades.len();
    upgrades.truncate(STRIP_LENGTH);
    Landing {
        upgrades,
        upgrade_total,
        recent: recent.into_iter().map(title_of).collect(),
    }
}

impl TorrentService {
    /// The landing strips: library movies below 2160p and the newest additions, each with a TMDB
    /// ID. Nothing is contacted; only the library is read.
    ///
    /// # Errors
    /// `NoticeNotAccepted` until the Torrent search notice is accepted; [`TorrentServiceError::Library`] when the library
    /// cannot be read.
    pub fn landing(&self) -> Result<Landing, TorrentServiceError> {
        self.ensure_accepted()?;
        let (movies, recent) = self
            .db
            .read(|conn| {
                Ok((
                    movies_with_video(conn)?,
                    recently_added(conn, STRIP_LENGTH)?,
                ))
            })
            .map_err(|e| TorrentServiceError::Library(e.to_string()))?;
        Ok(landing_of(movies, recent))
    }

    /// The library database, for the poster keys of the landing tiles.
    #[must_use]
    pub fn library(&self) -> &std::sync::Arc<cv_db::Database> {
        &self.db
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: i64, title: &str, kind: ItemKind, height: u32) -> LandingRow {
        LandingRow {
            id: ItemId(id),
            kind,
            tmdb_id: u64::try_from(id).unwrap(),
            title: title.to_owned(),
            year: 1995,
            date_added_ms: id * 10,
            width: 0,
            height,
        }
    }

    #[test]
    fn copies_below_2160p_are_upgrades_the_lowest_first() {
        let movies = vec![
            row(1, "Alien", ItemKind::Movie, 720),
            row(2, "Dune", ItemKind::Movie, 2160),
            row(3, "Heat", ItemKind::Movie, 720),
            row(4, "Oppenheimer", ItemKind::Movie, 1080),
            row(5, "Solaris", ItemKind::Movie, 480),
        ];
        let landing = landing_of(movies, Vec::new());
        let titles: Vec<&str> = landing.upgrades.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(titles, ["Solaris", "Alien", "Heat", "Oppenheimer"]);
        assert_eq!(landing.upgrade_total, 4);
        assert_eq!(landing.upgrades[1].resolution, Resolution::R720);
    }

    #[test]
    fn the_strip_is_capped_and_the_total_kept() {
        let movies = (1..=12)
            .map(|i| row(i, &format!("M{i:02}"), ItemKind::Movie, 1080))
            .collect();
        let landing = landing_of(movies, Vec::new());
        assert_eq!(landing.upgrades.len(), STRIP_LENGTH);
        assert_eq!(landing.upgrade_total, 12);
    }

    #[test]
    fn recent_titles_keep_their_kind_and_date() {
        let recent = vec![
            row(9, "Shogun", ItemKind::Show, 0),
            row(8, "Dune", ItemKind::Movie, 2160),
        ];
        let landing = landing_of(Vec::new(), recent);
        assert_eq!(landing.recent[0].kind, MediaKind::Show);
        assert_eq!(landing.recent[0].resolution, Resolution::Unknown);
        assert_eq!(landing.recent[1].resolution, Resolution::R2160);
        assert_eq!(landing.recent[1].added_ms, Some(80));
    }
}
