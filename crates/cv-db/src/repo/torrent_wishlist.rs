//! The torrent Wishlist: titles the user wants a release of, the kind of release they are after,
//! and the releases the checks found for them.
//!
//! The criteria are stored as the app writes them (`criteria_json`); this module does not read
//! them. One entry per title and scope: adding the same one again replaces its criteria. A
//! dismissed release stays stored so a later check never lists it again. Nothing here touches
//! library data, and only the user removes an entry.

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::error::{DbError, DbResult};

/// An entry of the Wishlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WishRow {
    pub id: i64,
    /// `movie` or `show`.
    pub kind: String,
    pub tmdb_id: i64,
    pub title: String,
    pub year: Option<i64>,
    pub poster_url: Option<String>,
    /// `movie`, `season:<n>`, `episode:<s>:<e>` or `next`.
    pub scope: String,
    pub criteria_json: String,
    /// Epoch milliseconds.
    pub added_ms: i64,
    pub checked_ms: Option<i64>,
    /// What the last check could not do: `no-source`, `airs:<date>`, `no-new-episode` or a
    /// failure's words.
    pub check_error: Option<String>,
    /// Set once the library holds a copy that meets the criteria.
    pub done_ms: Option<i64>,
    /// What a `next` entry looked for at its last check (`S03E06`).
    pub target: Option<String>,
    /// How many library copies met the criteria when the entry was added, last edited or told to
    /// keep looking; the entry is done once more copies meet them.
    pub held_copies: i64,
}

/// What a new entry holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewWish {
    pub kind: String,
    pub tmdb_id: i64,
    pub title: String,
    pub year: Option<i64>,
    pub poster_url: Option<String>,
    pub scope: String,
    pub criteria_json: String,
    pub added_ms: i64,
    /// How many library copies meet the criteria now.
    pub held_copies: i64,
}

/// Where a match stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchState {
    /// Found and not looked at yet.
    New,
    /// Shown on the Wishlist.
    Seen,
    /// Handed to the torrent app.
    Sent,
    /// Hidden by the user; never stored again for this entry.
    Dismissed,
}

impl MatchState {
    /// The text stored in the table.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Seen => "seen",
            Self::Sent => "sent",
            Self::Dismissed => "dismissed",
        }
    }

    /// The state stored as `text`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        [Self::New, Self::Seen, Self::Sent, Self::Dismissed]
            .into_iter()
            .find(|state| state.as_str() == text)
    }
}

/// A release a check kept for an entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchRow {
    pub entry_id: i64,
    /// The search's merge key of the release.
    pub key: String,
    pub name: String,
    pub size: Option<i64>,
    pub seeders: Option<i64>,
    pub magnet: Option<String>,
    pub torrent_url: Option<String>,
    /// The ids of the sources that returned it, comma separated.
    pub sources: String,
    /// Epoch milliseconds.
    pub found_ms: i64,
    pub state: MatchState,
}

/// What [`update`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateOutcome {
    Updated,
    /// No entry has that id.
    Missing,
    /// Another entry already has this title and scope.
    Duplicate,
}

const WISH_COLUMNS: &str = "id, kind, tmdb_id, title, year, poster_url, scope, criteria_json, \
     added_ms, checked_ms, check_error, done_ms, target, held_copies";

fn wish_of(row: &Row<'_>) -> rusqlite::Result<WishRow> {
    Ok(WishRow {
        id: row.get(0)?,
        kind: row.get(1)?,
        tmdb_id: row.get(2)?,
        title: row.get(3)?,
        year: row.get(4)?,
        poster_url: row.get(5)?,
        scope: row.get(6)?,
        criteria_json: row.get(7)?,
        added_ms: row.get(8)?,
        checked_ms: row.get(9)?,
        check_error: row.get(10)?,
        done_ms: row.get(11)?,
        target: row.get(12)?,
        held_copies: row.get(13)?,
    })
}

/// Adds an entry, or, when the title and scope are already on the Wishlist, replaces that entry's
/// title, poster, criteria and held copies and judges it again (done and the last check are
/// cleared). Returns the entry's id.
pub fn add(conn: &Connection, wish: &NewWish) -> DbResult<i64> {
    let id = conn.query_row(
        "INSERT INTO torrent_wishlist \
             (kind, tmdb_id, title, year, poster_url, scope, criteria_json, added_ms, \
             held_copies) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) \
         ON CONFLICT (kind, tmdb_id, scope) DO UPDATE SET title = excluded.title, \
             year = excluded.year, poster_url = excluded.poster_url, \
             criteria_json = excluded.criteria_json, checked_ms = NULL, check_error = NULL, \
             done_ms = NULL, held_copies = excluded.held_copies \
         RETURNING id",
        params![
            wish.kind,
            wish.tmdb_id,
            wish.title,
            wish.year,
            wish.poster_url,
            wish.scope,
            wish.criteria_json,
            wish.added_ms,
            wish.held_copies,
        ],
        |row| row.get(0),
    )?;
    forget_unsent(conn, id)?;
    Ok(id)
}

/// Every entry, the newest first.
pub fn list(conn: &Connection) -> DbResult<Vec<WishRow>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {WISH_COLUMNS} FROM torrent_wishlist ORDER BY added_ms DESC, id DESC"
    ))?;
    let rows = stmt
        .query_map([], wish_of)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// One entry.
pub fn get(conn: &Connection, id: i64) -> DbResult<Option<WishRow>> {
    Ok(conn
        .query_row(
            &format!("SELECT {WISH_COLUMNS} FROM torrent_wishlist WHERE id = ?1"),
            [id],
            wish_of,
        )
        .optional()?)
}

/// Drops the matches that are neither sent nor dismissed: they were kept for criteria or a scope
/// that changed. Dismissed ones stay hidden and sent ones stay as a record.
fn forget_unsent(conn: &Connection, id: i64) -> DbResult<()> {
    conn.execute(
        "DELETE FROM torrent_wishlist_match WHERE entry_id = ?1 AND state IN ('new', 'seen')",
        [id],
    )?;
    Ok(())
}

/// Changes an entry's scope, criteria and held copies. The entry is judged again: done and the
/// last check are cleared, and the matches kept for the old criteria go (dismissed and sent ones
/// stay).
pub fn update(
    conn: &Connection,
    id: i64,
    scope: &str,
    criteria_json: &str,
    held_copies: i64,
) -> DbResult<UpdateOutcome> {
    let Some(entry) = get(conn, id)? else {
        return Ok(UpdateOutcome::Missing);
    };
    let taken: Option<i64> = conn
        .query_row(
            "SELECT id FROM torrent_wishlist WHERE kind = ?1 AND tmdb_id = ?2 AND scope = ?3 \
                 AND id <> ?4",
            params![entry.kind, entry.tmdb_id, scope, id],
            |row| row.get(0),
        )
        .optional()?;
    if taken.is_some() {
        return Ok(UpdateOutcome::Duplicate);
    }
    conn.execute(
        "UPDATE torrent_wishlist SET scope = ?2, criteria_json = ?3, checked_ms = NULL, \
             check_error = NULL, done_ms = NULL, held_copies = ?4, \
             target = CASE WHEN scope = ?2 THEN target ELSE NULL END \
         WHERE id = ?1",
        params![id, scope, criteria_json, held_copies],
    )?;
    forget_unsent(conn, id)?;
    Ok(UpdateOutcome::Updated)
}

/// Makes a done entry active again, counting from now: done and the last check are cleared, the
/// held copies are set, and the matches that are new or seen go (dismissed and sent ones stay).
/// Returns whether there was such an entry.
pub fn keep_looking(conn: &Connection, id: i64, held_copies: i64) -> DbResult<bool> {
    let changed = conn.execute(
        "UPDATE torrent_wishlist SET done_ms = NULL, checked_ms = NULL, check_error = NULL, \
             held_copies = ?2 \
         WHERE id = ?1",
        params![id, held_copies],
    )?;
    if changed > 0 {
        forget_unsent(conn, id)?;
    }
    Ok(changed > 0)
}

/// Lowers an entry's held copies to `count` when fewer copies meet its criteria now (some left the
/// library), so a later arrival still counts. A higher count changes nothing.
pub fn lower_held(conn: &Connection, id: i64, count: i64) -> DbResult<()> {
    conn.execute(
        "UPDATE torrent_wishlist SET held_copies = ?2 WHERE id = ?1 AND held_copies > ?2",
        params![id, count],
    )?;
    Ok(())
}

/// Removes an entry and its matches.
pub fn remove(conn: &Connection, id: i64) -> DbResult<bool> {
    // The matches go with it through the foreign key; deleted here too for a connection that has
    // foreign keys off.
    conn.execute(
        "DELETE FROM torrent_wishlist_match WHERE entry_id = ?1",
        [id],
    )?;
    Ok(conn.execute("DELETE FROM torrent_wishlist WHERE id = ?1", [id])? > 0)
}

/// Notes a check of an entry: when, why it found nothing to go on, and (for a `next` entry) the
/// episode it looked for. When that episode differs from the one of the last check, the earlier
/// episode's matches go.
pub fn record_check(
    conn: &Connection,
    id: i64,
    checked_ms: i64,
    error: Option<&str>,
    target: Option<&str>,
) -> DbResult<()> {
    let before: Option<Option<String>> = conn
        .query_row(
            "SELECT target FROM torrent_wishlist WHERE id = ?1",
            [id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(before) = before else {
        return Ok(());
    };
    if before.is_some() && before.as_deref() != target {
        conn.execute(
            "DELETE FROM torrent_wishlist_match WHERE entry_id = ?1",
            [id],
        )?;
    }
    conn.execute(
        "UPDATE torrent_wishlist SET checked_ms = ?2, check_error = ?3, target = ?4 WHERE id = ?1",
        params![id, checked_ms, error, target],
    )?;
    Ok(())
}

/// Marks an entry done: the library gained a copy that meets its criteria.
pub fn mark_done(conn: &Connection, id: i64, done_ms: i64) -> DbResult<()> {
    conn.execute(
        "UPDATE torrent_wishlist SET done_ms = ?2 WHERE id = ?1 AND done_ms IS NULL",
        params![id, done_ms],
    )?;
    Ok(())
}

/// Stores what a check kept for an entry. A release seen before keeps its state and gets the new
/// size, seeders and links; a dismissed one stays as it is. Returns how many were new.
pub fn store_matches(conn: &Connection, rows: &[MatchRow]) -> DbResult<usize> {
    let mut added = 0;
    for row in rows {
        let changed = conn.execute(
            "UPDATE torrent_wishlist_match SET name = ?3, size = ?4, seeders = ?5, magnet = ?6, \
                 torrent_url = ?7, sources = ?8 \
             WHERE entry_id = ?1 AND key = ?2 AND state <> 'dismissed'",
            params![
                row.entry_id,
                row.key,
                row.name,
                row.size,
                row.seeders,
                row.magnet,
                row.torrent_url,
                row.sources,
            ],
        )?;
        if changed > 0 {
            continue;
        }
        added += conn.execute(
            "INSERT OR IGNORE INTO torrent_wishlist_match (entry_id, key, name, size, seeders, \
                 magnet, torrent_url, sources, found_ms, state) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                row.entry_id,
                row.key,
                row.name,
                row.size,
                row.seeders,
                row.magnet,
                row.torrent_url,
                row.sources,
                row.found_ms,
                row.state.as_str(),
            ],
        )?;
    }
    Ok(added)
}

/// Every match that is not dismissed, of every entry, most seeders first.
pub fn matches(conn: &Connection) -> DbResult<Vec<MatchRow>> {
    let mut stmt = conn.prepare(
        "SELECT entry_id, key, name, size, seeders, magnet, torrent_url, sources, found_ms, \
             state \
         FROM torrent_wishlist_match WHERE state <> 'dismissed' \
         ORDER BY entry_id, seeders IS NULL, seeders DESC, found_ms DESC, key",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                MatchRow {
                    entry_id: row.get(0)?,
                    key: row.get(1)?,
                    name: row.get(2)?,
                    size: row.get(3)?,
                    seeders: row.get(4)?,
                    magnet: row.get(5)?,
                    torrent_url: row.get(6)?,
                    sources: row.get(7)?,
                    found_ms: row.get(8)?,
                    state: MatchState::New,
                },
                row.get::<_, String>(9)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(mut row, state)| {
            row.state = MatchState::parse(&state)
                .ok_or_else(|| DbError::InvalidData(format!("unknown match state `{state}`")))?;
            Ok(row)
        })
        .collect()
}

/// Sets the state of one match. Returns whether there was such a match.
pub fn set_match_state(
    conn: &Connection,
    entry_id: i64,
    key: &str,
    state: MatchState,
) -> DbResult<bool> {
    Ok(conn.execute(
        "UPDATE torrent_wishlist_match SET state = ?3 WHERE entry_id = ?1 AND key = ?2",
        params![entry_id, key, state.as_str()],
    )? > 0)
}

/// Marks every new match seen. Returns how many there were.
pub fn mark_seen(conn: &Connection) -> DbResult<usize> {
    Ok(conn.execute(
        "UPDATE torrent_wishlist_match SET state = 'seen' WHERE state = 'new'",
        [],
    )?)
}

/// How many matches of entries that are not done have not been looked at yet.
pub fn count_new(conn: &Connection) -> DbResult<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM torrent_wishlist_match m \
         JOIN torrent_wishlist w ON w.id = m.entry_id \
         WHERE m.state = 'new' AND w.done_ms IS NULL",
        [],
        |row| row.get(0),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema;

    fn database() -> Connection {
        let mut conn = Connection::open_in_memory().expect("memory database");
        conn.pragma_update(None, "foreign_keys", true)
            .expect("foreign keys");
        schema::migrate(&mut conn).expect("migrate");
        conn
    }

    fn wish(scope: &str, criteria: &str) -> NewWish {
        NewWish {
            kind: "show".to_owned(),
            tmdb_id: 42,
            title: "Example Show".to_owned(),
            year: Some(2020),
            poster_url: None,
            scope: scope.to_owned(),
            criteria_json: criteria.to_owned(),
            added_ms: 1_000,
            held_copies: 0,
        }
    }

    fn found(entry_id: i64, key: &str, seeders: i64) -> MatchRow {
        MatchRow {
            entry_id,
            key: key.to_owned(),
            name: format!("Example.Show.{key}.1080p.WEB-DL"),
            size: Some(1_000_000_000),
            seeders: Some(seeders),
            magnet: Some(format!("magnet:?xt=urn:btih:{key}")),
            torrent_url: None,
            sources: "yts".to_owned(),
            found_ms: 2_000,
            state: MatchState::New,
        }
    }

    #[test]
    fn adding_a_title_and_scope_twice_keeps_one_entry_with_the_new_criteria() {
        let conn = database();
        let first = add(&conn, &wish("season:2", "{\"a\":1}")).expect("add");
        let other = add(&conn, &wish("season:3", "{}")).expect("another season");
        record_check(&conn, first, 5_000, None, None).expect("check");
        mark_done(&conn, first, 6_000).expect("done");
        store_matches(&conn, &[found(first, "aa", 5), found(first, "bb", 9)]).expect("matches");
        set_match_state(&conn, first, "bb", MatchState::Dismissed).expect("dismiss");

        let again = add(&conn, &wish("season:2", "{\"a\":2}")).expect("add again");

        assert_eq!(again, first);
        assert_ne!(other, first);
        let entry = get(&conn, first).expect("get").expect("entry");
        assert_eq!(entry.criteria_json, "{\"a\":2}");
        assert_eq!(entry.done_ms, None, "judged again");
        assert_eq!(entry.checked_ms, None, "due at once");
        assert_eq!(list(&conn).expect("list").len(), 2);
        // The match kept for the old criteria went; the dismissed one is still remembered.
        assert!(matches(&conn).expect("matches").is_empty());
        assert_eq!(
            store_matches(&conn, &[found(first, "bb", 9)]).expect("again"),
            0,
            "a dismissed release is never stored again"
        );
        assert!(matches(&conn).expect("matches").is_empty());
    }

    #[test]
    fn adding_again_and_keep_looking_set_the_held_copies_and_lower_held_only_lowers() {
        let conn = database();
        let mut held = wish("movie", "{}");
        held.held_copies = 2;
        let id = add(&conn, &held).expect("add");
        assert_eq!(get(&conn, id).expect("get").expect("entry").held_copies, 2);
        held.held_copies = 1;
        add(&conn, &held).expect("add again");
        assert_eq!(get(&conn, id).expect("get").expect("entry").held_copies, 1);

        lower_held(&conn, id, 3).expect("higher");
        assert_eq!(get(&conn, id).expect("get").expect("entry").held_copies, 1);
        lower_held(&conn, id, 0).expect("lower");
        assert_eq!(get(&conn, id).expect("get").expect("entry").held_copies, 0);

        record_check(&conn, id, 5_000, Some("no-source"), None).expect("check");
        store_matches(&conn, &[found(id, "aa", 5), found(id, "bb", 9)]).expect("matches");
        set_match_state(&conn, id, "bb", MatchState::Sent).expect("sent");
        mark_done(&conn, id, 6_000).expect("done");

        assert!(keep_looking(&conn, id, 1).expect("keep looking"));
        let entry = get(&conn, id).expect("get").expect("entry");
        assert_eq!(entry.done_ms, None);
        assert_eq!(entry.checked_ms, None, "due at once");
        assert_eq!(entry.check_error, None);
        assert_eq!(entry.held_copies, 1);
        let kept = matches(&conn).expect("matches");
        assert_eq!(
            kept.iter().map(|r| r.key.as_str()).collect::<Vec<_>>(),
            ["bb"],
            "the sent one stays as a record"
        );
        assert!(!keep_looking(&conn, 999, 0).expect("unknown"));
    }

    #[test]
    fn matches_keep_their_state_and_list_most_seeders_first() {
        let conn = database();
        let id = add(&conn, &wish("next", "{}")).expect("add");
        assert_eq!(
            store_matches(&conn, &[found(id, "aa", 5), found(id, "bb", 50)]).expect("store"),
            2
        );
        set_match_state(&conn, id, "aa", MatchState::Sent).expect("sent");
        let mut fresher = found(id, "aa", 70);
        fresher.name = "Renamed".to_owned();
        assert_eq!(store_matches(&conn, &[fresher]).expect("again"), 0);

        let rows = matches(&conn).expect("matches");
        assert_eq!(
            rows.iter().map(|r| r.key.as_str()).collect::<Vec<_>>(),
            ["aa", "bb"]
        );
        assert_eq!(rows[0].state, MatchState::Sent);
        assert_eq!(rows[0].seeders, Some(70));
        assert_eq!(rows[0].name, "Renamed");
        assert_eq!(count_new(&conn).expect("count"), 1);
        assert_eq!(mark_seen(&conn).expect("seen"), 1);
        assert_eq!(count_new(&conn).expect("count"), 0);
    }

    #[test]
    fn done_entries_are_not_counted_as_new() {
        let conn = database();
        let id = add(&conn, &wish("movie", "{}")).expect("add");
        store_matches(&conn, &[found(id, "aa", 5)]).expect("store");
        assert_eq!(count_new(&conn).expect("count"), 1);
        mark_done(&conn, id, 9_000).expect("done");
        assert_eq!(count_new(&conn).expect("count"), 0);
    }

    #[test]
    fn another_target_drops_the_earlier_episodes_matches() {
        let conn = database();
        let id = add(&conn, &wish("next", "{}")).expect("add");
        record_check(&conn, id, 1, None, Some("S03E06")).expect("first check");
        store_matches(&conn, &[found(id, "aa", 5)]).expect("store");
        record_check(&conn, id, 2, None, Some("S03E06")).expect("same episode");
        assert_eq!(matches(&conn).expect("kept").len(), 1);
        record_check(&conn, id, 3, Some("No source answered"), Some("S03E07"))
            .expect("next episode");
        assert!(matches(&conn).expect("dropped").is_empty());
        let entry = get(&conn, id).expect("get").expect("entry");
        assert_eq!(entry.target.as_deref(), Some("S03E07"));
        assert_eq!(entry.check_error.as_deref(), Some("No source answered"));
        assert_eq!(entry.checked_ms, Some(3));
    }

    #[test]
    fn updating_refuses_a_scope_another_entry_has_and_removing_takes_the_matches() {
        let conn = database();
        let two = add(&conn, &wish("season:2", "{}")).expect("two");
        let three = add(&conn, &wish("season:3", "{}")).expect("three");
        assert_eq!(
            update(&conn, three, "season:2", "{}", 0).expect("update"),
            UpdateOutcome::Duplicate
        );
        assert_eq!(
            update(&conn, 999, "season:4", "{}", 0).expect("update"),
            UpdateOutcome::Missing
        );
        record_check(&conn, three, 1, None, Some("x")).expect("check");
        assert_eq!(
            update(&conn, three, "season:4", "{\"b\":1}", 1).expect("update"),
            UpdateOutcome::Updated
        );
        let entry = get(&conn, three).expect("get").expect("entry");
        assert_eq!(entry.scope, "season:4");
        assert_eq!(entry.target, None, "a new scope looks afresh");
        assert_eq!(entry.held_copies, 1);

        store_matches(&conn, &[found(two, "aa", 5)]).expect("store");
        assert!(remove(&conn, two).expect("remove"));
        assert!(!remove(&conn, two).expect("gone"));
        assert!(matches(&conn).expect("matches").is_empty());
    }
}
