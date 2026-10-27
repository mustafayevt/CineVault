//! Watched state and play counts of the items that can be played: movies and episodes.
//!
//! Seasons, shows and collections are not played themselves. [`playable_in`] expands them to the
//! movies and episodes below them, so "mark season 2 watched" and "mark the show watched" are the
//! same operation on a list of episodes. The state of an item is read and restored whole
//! ([`WatchState`]), which is what makes the change undoable without losing a play count of 3.

use cv_domain::{EpisodeGroup, ItemDetail, ItemId};
use rusqlite::{Connection, params};

use super::items::{children_of, get_item};
use crate::error::{DbError, DbResult};

/// The watched state of one movie or episode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WatchState {
    pub id: ItemId,
    pub watched: bool,
    pub play_count: u32,
    pub last_watched_ms: Option<i64>,
}

impl WatchState {
    /// The state after marking watched: played at least once, last watched `now_ms`. A title that
    /// is already watched is unchanged (`None`).
    #[must_use]
    pub fn marked_watched(&self, now_ms: i64) -> Option<Self> {
        if self.watched {
            return None;
        }
        Some(Self {
            id: self.id,
            watched: true,
            play_count: self.play_count.max(1),
            last_watched_ms: Some(now_ms),
        })
    }

    /// The state after marking not watched: play count 0, no last-watched date. A title that is
    /// already not watched and has no plays is unchanged (`None`).
    #[must_use]
    pub fn marked_unwatched(&self) -> Option<Self> {
        if !self.watched && self.play_count == 0 && self.last_watched_ms.is_none() {
            return None;
        }
        Some(Self {
            id: self.id,
            watched: false,
            play_count: 0,
            last_watched_ms: None,
        })
    }
}

/// The movies and episodes among `ids` and below them (a show's episodes, a collection's movies,
/// the episodes of a season), each once, in id order. Unknown ids are ignored.
///
/// A season item has no children of its own: its episodes are the show's episodes numbered with
/// that season in the ordering the show displays (the aired numbers when an episode has none).
pub fn playable_in(conn: &Connection, ids: &[ItemId]) -> DbResult<Vec<ItemId>> {
    let mut stmt = conn.prepare_cached(
        "WITH RECURSIVE tree(id) AS (
             SELECT id FROM item WHERE id = ?1
             UNION ALL
             SELECT i.id FROM item i JOIN tree t ON i.parent_id = t.id
         )
         SELECT i.id FROM item i JOIN tree ON tree.id = i.id
          WHERE i.kind IN ('movie', 'episode')",
    )?;
    let mut found: Vec<ItemId> = Vec::new();
    for id in ids {
        found.extend(season_episodes(conn, *id)?);
        let rows = stmt
            .query_map([id.0], |r| r.get::<_, i64>(0).map(ItemId))?
            .collect::<Result<Vec<_>, _>>()?;
        found.extend(rows);
    }
    found.sort_by_key(|id| id.0);
    found.dedup();
    Ok(found)
}

/// The episodes of a season item; empty for any other item.
fn season_episodes(conn: &Connection, id: ItemId) -> DbResult<Vec<ItemId>> {
    let Some(season) = get_item(conn, id)? else {
        return Ok(Vec::new());
    };
    let (ItemDetail::Season(season_fields), Some(show_id)) = (&season.detail, season.parent_id)
    else {
        return Ok(Vec::new());
    };
    let Some(show) = get_item(conn, show_id)? else {
        return Ok(Vec::new());
    };
    let group = match &show.detail {
        ItemDetail::Show(fields) => fields.episode_group.clone(),
        _ => EpisodeGroup::aired(),
    };
    let mut found = Vec::new();
    for child in children_of(conn, show_id)? {
        let Some(item) = get_item(conn, child)? else {
            continue;
        };
        let ItemDetail::Episode(episode) = &item.detail else {
            continue;
        };
        let in_season = episode
            .numbers
            .display(&group)
            .is_some_and(|n| n.season == season_fields.season);
        if in_season {
            found.push(child);
        }
    }
    Ok(found)
}

/// Reads the watched state of a movie or episode; `None` for any other item or an unknown id.
pub fn get_watch_state(conn: &Connection, id: ItemId) -> DbResult<Option<WatchState>> {
    let mut stmt = conn.prepare_cached(
        "SELECT watched, play_count, last_watched_ms FROM item
          WHERE id = ?1 AND kind IN ('movie', 'episode')",
    )?;
    let mut rows = stmt.query([id.0])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    let play_count: i64 = row.get(1)?;
    Ok(Some(WatchState {
        id,
        watched: row.get(0)?,
        play_count: u32::try_from(play_count).unwrap_or(0),
        last_watched_ms: row.get(2)?,
    }))
}

/// Writes a watched state. `updated_ms` moves on, as for any edit of the title.
///
/// # Errors
/// Fails with [`DbError::ItemNotFound`] when the item is not a movie or episode.
pub fn set_watch_state(conn: &Connection, state: &WatchState) -> DbResult<()> {
    let changed = conn
        .prepare_cached(
            "UPDATE item SET watched = ?2, play_count = ?3, last_watched_ms = ?4,
                    updated_ms = CAST(unixepoch('subsec') * 1000 AS INTEGER)
              WHERE id = ?1 AND kind IN ('movie', 'episode')",
        )?
        .execute(params![
            state.id.0,
            state.watched,
            i64::from(state.play_count),
            state.last_watched_ms
        ])?;
    if changed == 0 {
        return Err(DbError::ItemNotFound(state.id.0));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Database;
    use crate::repo::insert_item;
    use cv_domain::Item;

    #[test]
    fn a_show_and_its_seasons_expand_to_episodes() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(dir.path().join("library.db")).unwrap();
        let (show, seasons, eps, movie) = db
            .write(|tx| {
                let show = insert_item(tx, &Item::show("S"))?;
                let seasons = [
                    insert_item(tx, &Item::season(show, 1))?,
                    insert_item(tx, &Item::season(show, 2))?,
                ];
                let mut eps = Vec::new();
                for (season, number) in [(1, 1), (1, 2), (2, 1)] {
                    let mut e = Item::episode(show, format!("S{season}E{number}"));
                    if let ItemDetail::Episode(f) = &mut e.detail {
                        f.numbers
                            .set(cv_domain::EpisodeNumber::aired(season, number));
                    }
                    eps.push(insert_item(tx, &e)?);
                }
                let movie = insert_item(tx, &Item::movie("M"))?;
                Ok((show, seasons, eps, movie))
            })
            .unwrap();
        let all = db.read(|c| playable_in(c, &[show, movie, show])).unwrap();
        let mut want = eps.clone();
        want.push(movie);
        want.sort_by_key(|i| i.0);
        assert_eq!(all, want);
        let season_two = db.read(|c| playable_in(c, &[seasons[1]])).unwrap();
        assert_eq!(season_two, vec![eps[2]]);
        let season_one = db.read(|c| playable_in(c, &[seasons[0]])).unwrap();
        assert_eq!(season_one, vec![eps[0], eps[1]]);
        assert!(db.read(|c| get_watch_state(c, show)).unwrap().is_none());
    }

    #[test]
    fn states_round_trip_and_keep_the_play_count_on_restore() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(dir.path().join("library.db")).unwrap();
        let id = db.write(|tx| insert_item(tx, &Item::movie("M"))).unwrap();
        let before = db.read(|c| get_watch_state(c, id)).unwrap().unwrap();
        assert!(before.marked_unwatched().is_none());
        let watched = before.marked_watched(5_000).unwrap();
        assert_eq!(
            (watched.play_count, watched.last_watched_ms),
            (1, Some(5_000))
        );
        db.write(move |tx| set_watch_state(tx, &watched)).unwrap();
        let thrice = WatchState {
            play_count: 3,
            ..watched
        };
        db.write(move |tx| set_watch_state(tx, &thrice)).unwrap();
        assert!(thrice.marked_watched(9).is_none());
        let off = thrice.marked_unwatched().unwrap();
        assert_eq!(
            (off.watched, off.play_count, off.last_watched_ms),
            (false, 0, None)
        );
        db.write(move |tx| set_watch_state(tx, &off)).unwrap();
        db.write(move |tx| set_watch_state(tx, &thrice)).unwrap();
        assert_eq!(
            db.read(|c| get_watch_state(c, id)).unwrap().unwrap(),
            thrice
        );
    }
}
