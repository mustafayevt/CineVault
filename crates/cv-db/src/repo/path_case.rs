//! SQL for comparing stored paths with the platform's case rule.
//!
//! Windows and macOS find files without regard to case, so the queries that match a stored path
//! ignore ASCII case there. On Linux `Movie.mkv` and `movie.mkv` are two files, so the same
//! queries compare exactly; folding would repoint, restore or act on the wrong file. The SQL a
//! Windows or macOS build runs is the text it always had.
//!
//! Columns that hold paths are declared `COLLATE NOCASE`, so an exact comparison has to say
//! `COLLATE BINARY` outright; a bare `=` would keep folding.

use cv_fs::paths::CASE_SENSITIVE;

/// The collation to put after a path comparison: `NOCASE`, or `BINARY` on Linux.
pub(crate) const COLLATE: &str = if CASE_SENSITIVE { "BINARY" } else { "NOCASE" };

/// `expr` (a path or part of one) as the platform compares it: `lower(expr)`, or exactly as stored
/// on Linux. Compare two folded expressions with `=` or `IN`.
pub(crate) fn fold(expr: &str) -> String {
    if CASE_SENSITIVE {
        format!("({expr} COLLATE BINARY)")
    } else {
        format!("lower({expr})")
    }
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::*;

    /// A table shaped like the library's: the path column folds case by its declaration.
    fn paths() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE t (path TEXT COLLATE NOCASE);
             INSERT INTO t VALUES ('/Movies/Heat'), ('/movies/heat');",
        )
        .unwrap();
        conn
    }

    #[test]
    fn a_folded_comparison_follows_the_platform_rule() {
        let conn = paths();
        let sql = format!(
            "SELECT COUNT(*) FROM t WHERE {} = {}",
            fold("path"),
            fold("?1")
        );
        let same: i64 = conn
            .query_row(&sql, ["/Movies/Heat"], |r| r.get(0))
            .unwrap();
        assert_eq!(same, if CASE_SENSITIVE { 1 } else { 2 });
    }

    #[test]
    fn the_collation_follows_the_platform_rule() {
        let conn = paths();
        let sql = format!("SELECT COUNT(*) FROM t WHERE path = ?1 COLLATE {COLLATE}");
        let same: i64 = conn
            .query_row(&sql, ["/Movies/Heat"], |r| r.get(0))
            .unwrap();
        assert_eq!(same, if CASE_SENSITIVE { 1 } else { 2 });
    }
}
