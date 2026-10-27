//! The indexers of the managed indexer service and how the last provisioning left them.
//!
//! Provisioning enables the public indexers that pass a connection test and deletes the rest from
//! the service; a row here remembers each outcome so the Torrent sources page can show it and a
//! search asks only the indexers in state `working`. A person's "off" (`user_off`) survives every
//! provisioning. Nothing here touches library data.

use rusqlite::{Connection, params};

use crate::error::{DbError, DbResult};

/// The outcome of checking one indexer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexerState {
    /// Passed the test; searches use it.
    Working,
    /// The site shows a browser challenge and no challenge solver is set.
    NeedsSolver,
    /// The site did not answer.
    Unreachable,
    /// Anything else; the row's reason says what.
    Error,
    /// Switched off by the person.
    Off,
}

impl IndexerState {
    /// The text stored in the table.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Working => "working",
            Self::NeedsSolver => "needs_solver",
            Self::Unreachable => "unreachable",
            Self::Error => "error",
            Self::Off => "off",
        }
    }

    /// The state stored as `text`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        [
            Self::Working,
            Self::NeedsSolver,
            Self::Unreachable,
            Self::Error,
            Self::Off,
        ]
        .into_iter()
        .find(|state| state.as_str() == text)
    }
}

/// One indexer of the service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TorrentIndexer {
    /// The service's id for it (`1337x`, `yts`).
    pub id: String,
    pub name: String,
    pub state: IndexerState,
    /// Plain words for a state that is not `working`.
    pub reason: Option<String>,
    /// When it was last checked, epoch milliseconds.
    pub checked_at: i64,
    /// Switched off by the person; kept across every provisioning.
    pub user_off: bool,
}

fn read(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<(String, String, String, Option<String>, i64, i64)> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
    ))
}

/// Every indexer, by name.
pub fn list(conn: &Connection) -> DbResult<Vec<TorrentIndexer>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, state, reason, checked_at, user_off FROM torrent_indexers \
         ORDER BY name COLLATE NOCASE, id",
    )?;
    let rows = stmt.query_map([], read)?.collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(id, name, state, reason, checked_at, user_off)| {
            let state = IndexerState::parse(&state)
                .ok_or_else(|| DbError::InvalidData(format!("unknown indexer state `{state}`")))?;
            Ok(TorrentIndexer {
                id,
                name,
                state,
                reason,
                checked_at,
                user_off: user_off != 0,
            })
        })
        .collect()
}

/// Writes one indexer, replacing the row with the same id.
pub fn upsert(conn: &Connection, row: &TorrentIndexer) -> DbResult<()> {
    conn.execute(
        "INSERT INTO torrent_indexers (id, name, state, reason, checked_at, user_off) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
         ON CONFLICT (id) DO UPDATE SET name = excluded.name, state = excluded.state, \
             reason = excluded.reason, checked_at = excluded.checked_at, \
             user_off = excluded.user_off",
        params![
            row.id,
            row.name,
            row.state.as_str(),
            row.reason,
            row.checked_at,
            i64::from(row.user_off),
        ],
    )?;
    Ok(())
}

/// Sets or clears a person's "off" for one indexer, keeping everything else about it. An indexer
/// that has no row yet gets one in state `off` (when switched off) or `error` with the reason "Not
/// checked yet" (when switched back on), which the next provisioning replaces.
pub fn set_user_off(conn: &Connection, id: &str, off: bool) -> DbResult<()> {
    let changed = conn.execute(
        "UPDATE torrent_indexers SET user_off = ?2, \
             state = CASE WHEN ?2 = 1 THEN 'off' WHEN state = 'off' THEN 'error' ELSE state END, \
             reason = CASE WHEN ?2 = 1 THEN NULL \
                           WHEN state = 'off' THEN 'Not checked yet' ELSE reason END \
         WHERE id = ?1",
        params![id, i64::from(off)],
    )?;
    if changed == 0 {
        conn.execute(
            "INSERT INTO torrent_indexers (id, name, state, reason, checked_at, user_off) \
             VALUES (?1, ?1, ?2, ?3, 0, ?4)",
            params![
                id,
                if off { "off" } else { "error" },
                if off { None } else { Some("Not checked yet") },
                i64::from(off),
            ],
        )?;
    }
    Ok(())
}

/// Removes one indexer's row.
pub fn remove(conn: &Connection, id: &str) -> DbResult<()> {
    conn.execute("DELETE FROM torrent_indexers WHERE id = ?1", [id])?;
    Ok(())
}

/// Removes every row except the ones a person switched off, which are kept so the choice survives.
pub fn clear_checked(conn: &Connection) -> DbResult<()> {
    conn.execute("DELETE FROM torrent_indexers WHERE user_off = 0", [])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema;

    fn database() -> Connection {
        let mut conn = Connection::open_in_memory().expect("memory database");
        schema::migrate(&mut conn).expect("migrate");
        conn
    }

    fn row(id: &str, state: IndexerState) -> TorrentIndexer {
        TorrentIndexer {
            id: id.to_owned(),
            name: id.to_uppercase(),
            state,
            reason: None,
            checked_at: 1_700_000_000_000,
            user_off: false,
        }
    }

    #[test]
    fn rows_round_trip_and_list_by_name() {
        let conn = database();
        assert!(list(&conn).expect("empty").is_empty());
        let mut broken = row("zeta", IndexerState::NeedsSolver);
        broken.reason = Some("Challenge detected".to_owned());
        upsert(&conn, &row("alpha", IndexerState::Working)).expect("one");
        upsert(&conn, &broken).expect("two");
        let rows = list(&conn).expect("list");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, "alpha");
        assert_eq!(rows[1], broken);
    }

    #[test]
    fn upsert_replaces_the_row_with_the_same_id() {
        let conn = database();
        upsert(&conn, &row("a", IndexerState::Working)).expect("first");
        let mut later = row("a", IndexerState::Unreachable);
        later.reason = Some("Timed out".to_owned());
        later.checked_at += 1000;
        upsert(&conn, &later).expect("second");
        assert_eq!(list(&conn).expect("list"), [later]);
    }

    #[test]
    fn a_persons_off_is_kept_and_undone_without_losing_the_row() {
        let conn = database();
        upsert(&conn, &row("a", IndexerState::Working)).expect("row");
        set_user_off(&conn, "a", true).expect("off");
        let off = &list(&conn).expect("list")[0];
        assert!(off.user_off);
        assert_eq!(off.state, IndexerState::Off);
        set_user_off(&conn, "a", false).expect("on");
        let on = &list(&conn).expect("list")[0];
        assert!(!on.user_off);
        assert_eq!(on.state, IndexerState::Error);
        assert_eq!(on.reason.as_deref(), Some("Not checked yet"));
        // Switching on something that never was off leaves its state alone.
        upsert(&conn, &row("b", IndexerState::Working)).expect("row");
        set_user_off(&conn, "b", false).expect("on");
        assert_eq!(
            list(&conn)
                .expect("list")
                .iter()
                .find(|r| r.id == "b")
                .map(|r| r.state),
            Some(IndexerState::Working)
        );
    }

    #[test]
    fn switching_off_an_unknown_indexer_adds_it() {
        let conn = database();
        set_user_off(&conn, "new", true).expect("off");
        let rows = list(&conn).expect("list");
        assert_eq!(rows.len(), 1);
        assert!(rows[0].user_off);
        assert_eq!(rows[0].state, IndexerState::Off);
        set_user_off(&conn, "other", false).expect("on");
        assert_eq!(list(&conn).expect("list").len(), 2);
    }

    #[test]
    fn clearing_keeps_what_a_person_switched_off() {
        let conn = database();
        upsert(&conn, &row("a", IndexerState::Working)).expect("a");
        upsert(&conn, &row("b", IndexerState::Working)).expect("b");
        set_user_off(&conn, "b", true).expect("off");
        clear_checked(&conn).expect("clear");
        let rows = list(&conn).expect("list");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "b");
        remove(&conn, "b").expect("remove");
        assert!(list(&conn).expect("empty").is_empty());
    }

    #[test]
    fn the_table_refuses_a_state_it_does_not_know() {
        let conn = database();
        let bad = conn.execute(
            "INSERT INTO torrent_indexers (id, name, state) VALUES ('x', 'X', 'sleeping')",
            [],
        );
        assert!(bad.is_err());
    }

    #[test]
    fn states_parse_from_their_stored_text() {
        for state in [
            IndexerState::Working,
            IndexerState::NeedsSolver,
            IndexerState::Unreachable,
            IndexerState::Error,
            IndexerState::Off,
        ] {
            assert_eq!(IndexerState::parse(state.as_str()), Some(state));
        }
        assert_eq!(IndexerState::parse("nope"), None);
    }
}
