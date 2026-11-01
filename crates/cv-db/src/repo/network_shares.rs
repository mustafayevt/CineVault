//! Network shares: the SMB shares CineVault connected with a login of their own.
//!
//! A row names the server, the share, the user and the folder the share appears in on this
//! computer (`local_root`). It never holds a password: a remembered one is in the system credential
//! store under `network-share.<id>`. Library folders are not linked to a row; a folder belongs to
//! the share whose `local_root` is that folder or holds it ([`share_for_path`]), so a folder added
//! with "Add folder…" on a connected share belongs to it too.
//!
//! Nothing here touches library rows or files on a share.

use cv_fs::paths;
use rusqlite::{Connection, OptionalExtension, params};

use crate::error::{DbError, DbResult};

const SELECT: &str = "SELECT id, server, share, user, local_root, remember, created_ms, \
                      last_connected_ms, last_error, unused_since_ms, connected_by_us \
                      FROM network_share";

/// One connected network share.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkShare {
    pub id: i64,
    /// The server's name or address (`nas`, `192.168.1.20`).
    pub server: String,
    /// The share's name on the server (`movies`).
    pub share: String,
    /// The user of the login, with an optional `DOMAIN\` part.
    pub user: String,
    /// Where the share's files appear on this computer (`\\nas\movies`, a mount folder).
    pub local_root: String,
    /// Whether the password is kept in the credential store.
    pub remember: bool,
    pub created_ms: i64,
    /// When the share last connected; `None` until it first did.
    pub last_connected_ms: Option<i64>,
    /// Why the last connect failed, as a code (`wrong_login`); `None` after a success.
    pub last_error: Option<String>,
    /// When the last library folder on the share was removed; `None` while one uses it.
    pub unused_since_ms: Option<i64>,
    /// Whether CineVault made the system's connection (rather than finding the user's own). Only
    /// such a connection is ever disconnected. Written by [`set_connected_by_us`] only.
    pub connected_by_us: bool,
}

/// The facts of a share that is about to be added.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NewNetworkShare<'a> {
    pub server: &'a str,
    pub share: &'a str,
    pub user: &'a str,
    pub local_root: &'a str,
    pub remember: bool,
}

fn read(row: &rusqlite::Row<'_>) -> rusqlite::Result<NetworkShare> {
    Ok(NetworkShare {
        id: row.get(0)?,
        server: row.get(1)?,
        share: row.get(2)?,
        user: row.get(3)?,
        local_root: row.get(4)?,
        remember: row.get(5)?,
        created_ms: row.get(6)?,
        last_connected_ms: row.get(7)?,
        last_error: row.get(8)?,
        unused_since_ms: row.get(9)?,
        connected_by_us: row.get(10)?,
    })
}

/// Adds a share.
///
/// # Errors
/// Fails on the unique index when another share already appears at the same `local_root`
/// (ignoring case).
pub fn insert_share(conn: &Connection, new: &NewNetworkShare<'_>) -> DbResult<NetworkShare> {
    conn.execute(
        "INSERT INTO network_share (server, share, user, local_root, remember) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            new.server,
            new.share,
            new.user,
            new.local_root,
            new.remember
        ],
    )?;
    let id = conn.last_insert_rowid();
    get_share(conn, id)?
        .ok_or_else(|| DbError::InvalidData("the new network share vanished".into()))
}

/// One share by id.
pub fn get_share(conn: &Connection, id: i64) -> DbResult<Option<NetworkShare>> {
    Ok(conn
        .query_row(&format!("{SELECT} WHERE id = ?1"), [id], read)
        .optional()?)
}

/// Every share, oldest first.
pub fn list_shares(conn: &Connection) -> DbResult<Vec<NetworkShare>> {
    let mut stmt = conn.prepare(&format!("{SELECT} ORDER BY id"))?;
    let rows = stmt.query_map([], read)?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Writes the changeable facts of `share` (user, local root, remember, the connect results and the
/// unused mark) to its row. The server and share names of a row never change.
///
/// # Errors
/// [`DbError::NetworkShareNotFound`] when the row does not exist.
pub fn update_share(conn: &Connection, share: &NetworkShare) -> DbResult<()> {
    let changed = conn.execute(
        "UPDATE network_share SET user = ?2, local_root = ?3, remember = ?4, \
         last_connected_ms = ?5, last_error = ?6, unused_since_ms = ?7 WHERE id = ?1",
        params![
            share.id,
            share.user,
            share.local_root,
            share.remember,
            share.last_connected_ms,
            share.last_error,
            share.unused_since_ms
        ],
    )?;
    if changed == 0 {
        return Err(DbError::NetworkShareNotFound(share.id));
    }
    Ok(())
}

/// Records whether CineVault made the system's connection to share `id`. Kept apart from
/// [`update_share`], so a copy of a row read earlier never overwrites it.
///
/// # Errors
/// [`DbError::NetworkShareNotFound`] when the row does not exist.
pub fn set_connected_by_us(conn: &Connection, id: i64, by_us: bool) -> DbResult<()> {
    let changed = conn.execute(
        "UPDATE network_share SET connected_by_us = ?2 WHERE id = ?1",
        params![id, by_us],
    )?;
    if changed == 0 {
        return Err(DbError::NetworkShareNotFound(id));
    }
    Ok(())
}

/// Removes a share's row. Returns whether there was one. Library folders and titles are untouched.
pub fn delete_share(conn: &Connection, id: i64) -> DbResult<bool> {
    Ok(conn.execute("DELETE FROM network_share WHERE id = ?1", [id])? > 0)
}

/// The share that holds `path`: the one whose `local_root` is `path` or a folder above it,
/// compared by whole path components with this platform's case rule.
pub fn share_for_path(conn: &Connection, path: &str) -> DbResult<Option<NetworkShare>> {
    let shares = list_shares(conn)?;
    Ok(share_holding(&shares, path, paths::CASE_SENSITIVE).cloned())
}

/// The share of `shares` that holds `path`, comparing names case-sensitively when `case_sensitive`
/// is true. When two roots hold it, the deeper one wins.
#[must_use]
pub fn share_holding<'a>(
    shares: &'a [NetworkShare],
    path: &str,
    case_sensitive: bool,
) -> Option<&'a NetworkShare> {
    shares
        .iter()
        .filter(|share| paths::is_within_with(path, &share.local_root, case_sensitive))
        .max_by_key(|share| share.local_root.trim_end_matches(['\\', '/']).len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema;

    fn library() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        schema::migrate(&mut conn).unwrap();
        conn
    }

    fn movies(conn: &Connection) -> NetworkShare {
        insert_share(
            conn,
            &NewNetworkShare {
                server: "nas",
                share: "movies",
                user: r"HOME\alex",
                local_root: r"\\nas\movies",
                remember: true,
            },
        )
        .unwrap()
    }

    #[test]
    fn a_share_is_inserted_listed_updated_and_deleted() {
        let conn = library();
        let share = movies(&conn);
        assert_eq!(
            (share.server.as_str(), share.share.as_str()),
            ("nas", "movies")
        );
        assert_eq!(share.user, r"HOME\alex");
        assert!(share.remember);
        assert!(share.created_ms > 0);
        assert_eq!(
            (
                share.last_connected_ms,
                share.last_error.as_deref(),
                share.unused_since_ms
            ),
            (None, None, None)
        );

        let mut changed = share.clone();
        changed.user = "alex".into();
        changed.remember = false;
        changed.last_connected_ms = Some(1_000);
        changed.last_error = Some("wrong_login".into());
        changed.unused_since_ms = Some(2_000);
        update_share(&conn, &changed).unwrap();
        assert_eq!(get_share(&conn, share.id).unwrap(), Some(changed.clone()));
        assert_eq!(list_shares(&conn).unwrap(), vec![changed]);

        assert!(delete_share(&conn, share.id).unwrap());
        assert!(!delete_share(&conn, share.id).unwrap());
        assert!(list_shares(&conn).unwrap().is_empty());
    }

    #[test]
    fn who_made_the_connection_is_kept_apart_from_the_other_facts() {
        let conn = library();
        let share = movies(&conn);
        assert!(
            !share.connected_by_us,
            "found, not made, until told otherwise"
        );

        set_connected_by_us(&conn, share.id, true).unwrap();
        assert!(get_share(&conn, share.id).unwrap().unwrap().connected_by_us);
        // A copy read before the change does not undo it.
        update_share(&conn, &share).unwrap();
        assert!(get_share(&conn, share.id).unwrap().unwrap().connected_by_us);

        set_connected_by_us(&conn, share.id, false).unwrap();
        assert!(!get_share(&conn, share.id).unwrap().unwrap().connected_by_us);
        delete_share(&conn, share.id).unwrap();
        assert!(matches!(
            set_connected_by_us(&conn, share.id, true),
            Err(DbError::NetworkShareNotFound(_))
        ));
    }

    #[test]
    fn updating_a_missing_share_fails() {
        let conn = library();
        let mut gone = movies(&conn);
        delete_share(&conn, gone.id).unwrap();
        gone.user = "bob".into();
        assert!(matches!(
            update_share(&conn, &gone),
            Err(DbError::NetworkShareNotFound(id)) if id == gone.id
        ));
    }

    #[test]
    fn a_local_root_is_unique_whatever_its_case() {
        let conn = library();
        movies(&conn);
        let again = insert_share(
            &conn,
            &NewNetworkShare {
                server: "NAS",
                share: "Movies",
                user: "alex",
                local_root: r"\\NAS\Movies",
                remember: false,
            },
        );
        assert!(again.is_err());
    }

    #[test]
    fn the_share_of_a_path_is_found_by_whole_components() {
        let conn = library();
        let share = movies(&conn);

        let found = |path: &str| share_for_path(&conn, path).unwrap().map(|s| s.id);
        assert_eq!(found(r"\\nas\movies"), Some(share.id), "the root itself");
        assert_eq!(found(r"\\nas\movies\"), Some(share.id), "with a separator");
        assert_eq!(
            found(r"\\nas\movies\Films\Heat (1995)"),
            Some(share.id),
            "a folder below it"
        );
        assert_eq!(
            found(r"\\nas\movies2"),
            None,
            "a sibling with a common prefix"
        );
        assert_eq!(found(r"\\nas\movies2\Films"), None);
        assert_eq!(found(r"\\nas"), None, "the server alone");
        assert_eq!(found(r"D:\Movies"), None);
    }

    #[test]
    fn the_share_of_a_path_ignores_case_on_windows_only() {
        let conn = library();
        let share = movies(&conn);
        let shares = list_shares(&conn).unwrap();

        let windows = share_holding(&shares, r"\\NAS\Movies\Films", false).map(|s| s.id);
        let linux = share_holding(&shares, r"\\NAS\Movies\Films", true).map(|s| s.id);
        assert_eq!(windows, Some(share.id));
        assert_eq!(linux, None);
        // The stored-library lookup follows this platform's rule.
        assert_eq!(
            share_for_path(&conn, r"\\NAS\Movies\Films")
                .unwrap()
                .map(|s| s.id),
            (!paths::CASE_SENSITIVE).then_some(share.id)
        );
    }

    #[test]
    fn mount_folders_hold_their_library_folders() {
        let conn = library();
        let mac = insert_share(
            &conn,
            &NewNetworkShare {
                server: "nas",
                share: "tv",
                user: "alex",
                local_root: "/Users/alex/CineVault Shares/nas/tv",
                remember: true,
            },
        )
        .unwrap();
        let gvfs = insert_share(
            &conn,
            &NewNetworkShare {
                server: "nas",
                share: "films",
                user: "alex",
                local_root: "/run/user/1000/gvfs/smb-share:server=nas,share=films",
                remember: true,
            },
        )
        .unwrap();

        let found = |path: &str| share_for_path(&conn, path).unwrap().map(|s| s.id);
        assert_eq!(
            found("/Users/alex/CineVault Shares/nas/tv/Shows"),
            Some(mac.id)
        );
        assert_eq!(found("/Users/alex/CineVault Shares/nas/tv2"), None);
        assert_eq!(
            found("/run/user/1000/gvfs/smb-share:server=nas,share=films/4K"),
            Some(gvfs.id)
        );
    }

    #[test]
    fn the_deeper_root_wins() {
        let outer = NetworkShare {
            id: 1,
            server: "nas".into(),
            share: "a".into(),
            user: String::new(),
            local_root: "/shares/nas".into(),
            remember: true,
            created_ms: 0,
            last_connected_ms: None,
            last_error: None,
            unused_since_ms: None,
            connected_by_us: false,
        };
        let inner = NetworkShare {
            id: 2,
            local_root: "/shares/nas/a".into(),
            ..outer.clone()
        };
        let shares = [outer, inner];
        assert_eq!(
            share_holding(&shares, "/shares/nas/a/x", true).map(|s| s.id),
            Some(2)
        );
        assert_eq!(
            share_holding(&shares, "/shares/nas/b", true).map(|s| s.id),
            Some(1)
        );
    }
}
