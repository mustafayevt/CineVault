//! Ratings of one item: several sources side by side, plus the user's own rating.
//!
//! The `rating` table holds one row per `(item, source)`. Source rows come from scrapes and rating
//! fetches and are stored with the scale of the source ([`Rating::normalised`]): Letterboxd out
//! of 5, Roger Ebert out of 4, percentages out of 100.
//!
//! Every source row carries the time it was fetched (`fetched_at_ms`), which the detail page shows
//! on hover. A rating that arrives without a time is stamped with the time it is stored.
//!
//! The user's own rating is the row with the source key `user`. It is stored by
//! [`set_user_rating`] and by nothing else: [`update_item`](super::update_item) (which every
//! scrape path ends in) rewrites the source rows and leaves the user row as it is, so a rescrape,
//! a locked field or a stale copy of an item can never overwrite it, and [`upsert_ratings`]
//! refuses to write it.

use std::time::{SystemTime, UNIX_EPOCH};

use cv_domain::{ItemId, ItemKind, Rating, Ratings, check_user_rating, rating_key};
use rusqlite::{Connection, params};

use super::children::load_ratings;
use super::items::item_exists;
use crate::error::{DbError, DbResult};

/// All ratings of an item (source ratings and the user rating).
///
/// # Errors
/// Fails when the item does not exist or the database fails.
pub fn get_ratings(conn: &Connection, id: ItemId) -> DbResult<Ratings> {
    ensure_item(conn, id)?;
    Ok(load_ratings(conn, id.0)?.into_iter().collect())
}

/// The user's own rating of an item on the 0-10 scale, if there is one.
///
/// # Errors
/// Fails when the item does not exist or the database fails.
pub fn get_user_rating(conn: &Connection, id: ItemId) -> DbResult<Option<f32>> {
    Ok(get_ratings(conn, id)?.user_rating())
}

/// Sets (`Some`, 0.5 to 10 in half steps) or clears (`None`) the user's own rating.
///
/// # Errors
/// Fails with [`DbError::InvalidData`] for a value that is not 0.5 to 10 in half steps (the stored
/// rating is then unchanged), when the item does not exist, or when the database fails.
pub fn set_user_rating(conn: &Connection, id: ItemId, value_on_ten: Option<f32>) -> DbResult<()> {
    ensure_item(conn, id)?;
    match value_on_ten {
        None => {
            conn.prepare_cached("DELETE FROM rating WHERE item_id = ?1 AND source = ?2")?
                .execute(params![id.0, rating_key::USER])?;
        }
        Some(value) => {
            let rating = Rating::user(value).map_err(|e| DbError::InvalidData(e.to_string()))?;
            write_rating(conn, id, &rating)?;
        }
    }
    Ok(())
}

/// Adds or replaces source ratings, one row per source, each stored in its
/// [normalised](Rating::normalised) form. Other sources of the item stay. Invalid ratings and the
/// user rating are skipped. Returns how many ratings were written.
///
/// # Errors
/// Fails when the item does not exist or the database fails.
pub fn upsert_ratings(conn: &Connection, id: ItemId, ratings: &[Rating]) -> DbResult<usize> {
    ensure_item(conn, id)?;
    let mut written = 0;
    for rating in ratings {
        let rating = rating.clone().normalised();
        if rating.is_user() || !rating.is_valid() {
            continue;
        }
        write_rating(conn, id, &rating)?;
        written += 1;
    }
    Ok(written)
}

/// Removes one source rating. The user rating is not a source rating: use
/// [`set_user_rating`] with `None`. Returns whether a row was removed.
///
/// # Errors
/// Fails when the item does not exist or the database fails.
pub fn remove_rating(conn: &Connection, id: ItemId, source: &str) -> DbResult<bool> {
    ensure_item(conn, id)?;
    let source = cv_domain::canonical_rating_id(source);
    if source == rating_key::USER {
        return Ok(false);
    }
    let removed = conn
        .prepare_cached("DELETE FROM rating WHERE item_id = ?1 AND source = ?2")?
        .execute(params![id.0, source])?;
    Ok(removed > 0)
}

/// The movies and shows a rating fetch can look up: those with an IMDb, TMDB, Trakt or TVDB ID
/// (a scraped title), with their kind, in id order. Collections, seasons and episodes are left
/// out, as is a title that was never scraped.
///
/// # Errors
/// The database fails.
pub fn rating_fetch_candidates(conn: &Connection) -> DbResult<Vec<(ItemId, ItemKind)>> {
    let mut stmt = conn.prepare_cached(
        "SELECT i.id, i.kind FROM item i
         WHERE i.kind IN ('movie', 'show') AND i.trashed_entry IS NULL
           AND EXISTS (SELECT 1 FROM ext_id e WHERE e.item_id = i.id
                       AND e.provider IN ('imdb', 'tmdb', 'trakt', 'tvdb'))
         ORDER BY i.id",
    )?;
    let rows = stmt.query_map([], |r| {
        let kind: String = r.get(1)?;
        Ok((
            ItemId(r.get(0)?),
            if kind == "show" {
                ItemKind::Show
            } else {
                ItemKind::Movie
            },
        ))
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// The current time in epoch milliseconds, the stamp of a rating that arrives without one.
pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .unwrap_or(0)
}

fn write_rating(conn: &Connection, id: ItemId, r: &Rating) -> DbResult<()> {
    if r.is_user() {
        check_user_rating(r.value).map_err(|e| DbError::InvalidData(e.to_string()))?;
    }
    let fetched = if r.fetched_at_ms > 0 {
        r.fetched_at_ms
    } else {
        now_ms()
    };
    conn.prepare_cached(
        "INSERT INTO rating (item_id, source, value, votes, max_value, fetched_at_ms, url)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT (item_id, source) DO UPDATE SET
             value = excluded.value, votes = excluded.votes, max_value = excluded.max_value,
             fetched_at_ms = excluded.fetched_at_ms,
             url = CASE WHEN excluded.url = '' THEN rating.url ELSE excluded.url END",
    )?
    .execute(params![
        id.0,
        r.id,
        r.value,
        r.votes,
        r.max_value,
        fetched,
        r.url
    ])?;
    Ok(())
}

fn ensure_item(conn: &Connection, id: ItemId) -> DbResult<()> {
    if item_exists(conn, id)? {
        Ok(())
    } else {
        Err(DbError::ItemNotFound(id.0))
    }
}

#[cfg(test)]
mod tests;
