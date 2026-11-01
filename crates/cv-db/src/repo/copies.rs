//! Copies of a movie: two or more movies of the library that are the same film.
//!
//! Movies are copies when they share a TMDB or IMDb ID or, when one of them has neither, the same
//! title and known year; always only within one edition, and only while their files are present
//! (online, not deleted, with a video). The groups are computed on every read from one query, so
//! a Scan, Scrape, edit, Delete or Undo needs no step of its own: the next read sees it.
//!
//! "Keep both" is the only thing stored (`copies_kept`): one set per decision with every member
//! it covered. A group is kept while all its current members belong to one set, so a new copy
//! opens it again and a copy that leaves does not.

use std::collections::HashMap;

use cv_domain::{ItemId, id_key};
use rusqlite::{Connection, params};

use crate::error::DbResult;

/// A group of two or more copies of one film.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyGroup {
    /// The copies, in id order.
    pub members: Vec<ItemId>,
}

impl CopyGroup {
    /// The group's id: its oldest member.
    pub fn id(&self) -> ItemId {
        self.members[0]
    }
}

/// One movie as the grouping reads it.
struct Candidate {
    id: ItemId,
    tmdb: Option<String>,
    imdb: Option<String>,
    title: String,
    year: i64,
    edition: String,
}

/// Every group of copies in the library, ordered by their oldest member.
pub fn copy_groups(conn: &Connection) -> DbResult<Vec<CopyGroup>> {
    let mut stmt = conn.prepare_cached(
        "SELECT i.id, t.value, m.value, i.title, i.year, i.edition FROM item i \
         LEFT JOIN ext_id t ON t.item_id = i.id AND t.provider = ?1 \
         LEFT JOIN ext_id m ON m.item_id = i.id AND m.provider = ?2 \
         WHERE i.kind = 'movie' AND i.availability = 'online' AND i.trashed_entry IS NULL \
           AND EXISTS (SELECT 1 FROM media_file f WHERE f.item_id = i.id AND f.kind = 'video') \
         ORDER BY i.id",
    )?;
    let candidates = stmt
        .query_map(params![id_key::TMDB, id_key::IMDB], |r| {
            Ok(Candidate {
                id: ItemId(r.get(0)?),
                tmdb: r.get(1)?,
                imdb: r.get(2)?,
                title: fold(&r.get::<_, String>(3)?),
                year: r.get(4)?,
                edition: fold(&r.get::<_, String>(5)?),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(group(&candidates))
}

/// Text compared without regard to case and surrounding spaces.
fn fold(text: &str) -> String {
    text.trim().to_lowercase()
}

/// Links the candidates by the copy rules and returns the groups of two or more.
fn group(candidates: &[Candidate]) -> Vec<CopyGroup> {
    let mut sets = UnionFind::new(candidates.len());
    let mut first_with: HashMap<(&str, &str, &str), usize> = HashMap::new();
    let mut same_title: HashMap<(&str, &str, i64), Vec<usize>> = HashMap::new();
    for (index, c) in candidates.iter().enumerate() {
        for (provider, value) in [("tmdb", &c.tmdb), ("imdb", &c.imdb)] {
            if let Some(value) = value {
                let first = *first_with
                    .entry((c.edition.as_str(), provider, value.as_str()))
                    .or_insert(index);
                sets.union(first, index);
            }
        }
        if c.year > 0 && !c.title.is_empty() {
            same_title
                .entry((c.edition.as_str(), c.title.as_str(), c.year))
                .or_default()
                .push(index);
        }
    }
    // A movie with neither ID joins every movie of its title and year; two movies that both have
    // IDs are linked by those IDs only.
    for members in same_title.values() {
        for &index in members {
            let c = &candidates[index];
            if c.tmdb.is_none() && c.imdb.is_none() {
                for &other in members {
                    sets.union(index, other);
                }
            }
        }
    }

    let mut by_root: HashMap<usize, Vec<ItemId>> = HashMap::new();
    for (index, c) in candidates.iter().enumerate() {
        by_root.entry(sets.find(index)).or_default().push(c.id);
    }
    let mut groups: Vec<CopyGroup> = by_root
        .into_values()
        .filter(|members| members.len() > 1)
        .map(|mut members| {
            members.sort();
            CopyGroup { members }
        })
        .collect();
    groups.sort_by_key(CopyGroup::id);
    groups
}

/// A disjoint-set forest over candidate positions.
struct UnionFind(Vec<usize>);

impl UnionFind {
    fn new(len: usize) -> Self {
        Self((0..len).collect())
    }

    fn find(&mut self, mut index: usize) -> usize {
        while self.0[index] != index {
            self.0[index] = self.0[self.0[index]];
            index = self.0[index];
        }
        index
    }

    fn union(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            self.0[a.max(b)] = a.min(b);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    fn memory_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        crate::schema::migrate(&mut conn).unwrap();
        conn
    }

    /// A movie with one video and the given IDs; returns its id.
    fn movie(
        conn: &Connection,
        title: &str,
        year: i64,
        edition: &str,
        tmdb: Option<&str>,
        imdb: Option<&str>,
    ) -> ItemId {
        conn.execute(
            "INSERT INTO item (kind, title, year, edition) VALUES ('movie', ?1, ?2, ?3)",
            params![title, year, edition],
        )
        .unwrap();
        let id = conn.last_insert_rowid();
        for (provider, value) in [("tmdb", tmdb), ("imdb", imdb)] {
            if let Some(value) = value {
                conn.execute(
                    "INSERT INTO ext_id (item_id, provider, value) VALUES (?1, ?2, ?3)",
                    params![id, provider, value],
                )
                .unwrap();
            }
        }
        conn.execute(
            "INSERT INTO media_file (item_id, kind, directory, filename) \
             VALUES (?1, 'video', 'D:/Movies', ?2)",
            params![id, format!("{title}-{id}.mkv")],
        )
        .unwrap();
        ItemId(id)
    }

    fn groups(conn: &Connection) -> Vec<Vec<ItemId>> {
        copy_groups(conn)
            .unwrap()
            .into_iter()
            .map(|g| g.members)
            .collect()
    }

    #[test]
    fn movies_sharing_an_id_are_copies() {
        let conn = memory_db();
        let a = movie(&conn, "Example Movie", 2020, "", Some("1"), None);
        let b = movie(&conn, "Example.Movie", 2020, "", Some("1"), Some("tt9"));
        let c = movie(&conn, "Other", 2019, "", Some("2"), Some("tt5"));
        let d = movie(&conn, "Other (copy)", 0, "", None, Some("tt5"));

        assert_eq!(groups(&conn), vec![vec![a, b], vec![c, d]]);
    }

    #[test]
    fn an_unscraped_movie_joins_by_title_and_year() {
        let conn = memory_db();
        let scraped = movie(&conn, "Example Movie", 2020, "", Some("1"), Some("tt1"));
        let new = movie(&conn, "  example movie ", 2020, "", None, None);

        assert_eq!(groups(&conn), vec![vec![scraped, new]]);
    }

    #[test]
    fn different_ids_are_not_copies_by_title() {
        let conn = memory_db();
        movie(&conn, "Example Movie", 2020, "", Some("1"), Some("tt1"));
        movie(&conn, "Example Movie", 2020, "", Some("2"), Some("tt2"));

        assert!(groups(&conn).is_empty());
    }

    #[test]
    fn an_unknown_year_never_matches_by_title() {
        let conn = memory_db();
        movie(&conn, "Example Movie", 0, "", None, None);
        movie(&conn, "Example Movie", 0, "", None, None);

        assert!(groups(&conn).is_empty());
    }

    #[test]
    fn editions_must_be_the_same() {
        let conn = memory_db();
        movie(&conn, "Example Movie", 2020, "Extended", Some("1"), None);
        movie(&conn, "Example Movie", 2020, "Theatrical", Some("1"), None);
        movie(&conn, "Example Movie", 2020, "", Some("1"), None);
        let a = movie(&conn, "Example Movie", 2020, "extended", None, Some("tt1"));
        let b = movie(
            &conn,
            "Example Movie",
            2020,
            " Extended ",
            None,
            Some("tt1"),
        );

        let found = groups(&conn);

        // The three TMDB rows differ in edition; the first Extended row shares no ID with the
        // "extended" pair, and both have IDs, so the title does not link them.
        assert_eq!(found, vec![vec![a, b]]);
    }

    #[test]
    fn only_movies_with_their_files_count() {
        let conn = memory_db();
        let a = movie(&conn, "Example Movie", 2020, "", Some("1"), None);
        let offline = movie(&conn, "Example Movie", 2020, "", Some("1"), None);
        let missing = movie(&conn, "Example Movie", 2020, "", Some("1"), None);
        let deleted = movie(&conn, "Example Movie", 2020, "", Some("1"), None);
        conn.execute(
            "UPDATE item SET availability = 'offline' WHERE id = ?1",
            [offline.0],
        )
        .unwrap();
        conn.execute(
            "UPDATE item SET availability = 'missing' WHERE id = ?1",
            [missing.0],
        )
        .unwrap();
        conn.execute(
            "UPDATE item SET trashed_entry = 7 WHERE id = ?1",
            [deleted.0],
        )
        .unwrap();
        assert!(groups(&conn).is_empty());

        // Undo of the delete makes them copies again.
        conn.execute(
            "UPDATE item SET trashed_entry = NULL WHERE id = ?1",
            [deleted.0],
        )
        .unwrap();
        assert_eq!(groups(&conn), vec![vec![a, deleted]]);
    }

    #[test]
    fn a_movie_without_a_video_is_not_a_copy() {
        let conn = memory_db();
        movie(&conn, "Example Movie", 2020, "", Some("1"), None);
        let empty = movie(&conn, "Example Movie", 2020, "", Some("1"), None);
        conn.execute("DELETE FROM media_file WHERE item_id = ?1", [empty.0])
            .unwrap();

        assert!(groups(&conn).is_empty());
    }

    #[test]
    fn shows_are_never_copies() {
        let conn = memory_db();
        for _ in 0..2 {
            conn.execute(
                "INSERT INTO item (kind, title, year) VALUES ('show', 'Example Show', 2020)",
                [],
            )
            .unwrap();
            let id = conn.last_insert_rowid();
            conn.execute(
                "INSERT INTO ext_id (item_id, provider, value) VALUES (?1, 'tmdb', '5')",
                [id],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO media_file (item_id, kind, directory, filename) \
                 VALUES (?1, 'video', 'D:/Shows', 'e1.mkv')",
                [id],
            )
            .unwrap();
        }

        assert!(groups(&conn).is_empty());
    }

    #[test]
    fn copies_are_linked_transitively() {
        let conn = memory_db();
        let a = movie(&conn, "Example Movie", 2020, "", Some("1"), None);
        let b = movie(&conn, "Example Movie", 2020, "", Some("1"), Some("tt1"));
        let c = movie(&conn, "Example Film", 2021, "", None, Some("tt1"));
        let d = movie(&conn, "example film", 2021, "", None, None);

        assert_eq!(groups(&conn), vec![vec![a, b, c, d]]);
    }

    #[test]
    fn five_thousand_movies_are_grouped_by_one_quick_read() {
        let mut conn = memory_db();
        let tx = conn.transaction().unwrap();
        for n in 0..5_000_i64 {
            // Every hundredth film is in the library twice.
            let film = if n % 100 == 1 { n - 1 } else { n };
            movie(
                &tx,
                &format!("Film {film}"),
                2000 + film % 20,
                "",
                Some(&film.to_string()),
                None,
            );
        }
        tx.commit().unwrap();

        let started = Instant::now();
        let found = copy_groups(&conn).unwrap();
        let took = started.elapsed();

        assert_eq!(found.len(), 50);
        assert!(found.iter().all(|g| g.members.len() == 2));
        let budget = Duration::from_millis(if std::env::var_os("CI").is_some() {
            300
        } else {
            100
        });
        assert!(took < budget, "copy groups took {took:?}");
    }
}
