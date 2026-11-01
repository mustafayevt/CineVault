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

use std::collections::{BTreeSet, HashMap};

use cv_domain::{ItemId, id_key};
use rusqlite::{Connection, params};

use crate::error::{DbError, DbResult};

/// A group of two or more copies of one film.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyGroup {
    /// The copies, in id order.
    pub members: Vec<ItemId>,
    /// The "keep both" decision that covers every current member, when there is one.
    pub kept: Option<KeptSetId>,
}

/// One "keep both" decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeptSetId(pub i64);

impl CopyGroup {
    /// The group's id: its oldest member.
    pub fn id(&self) -> ItemId {
        self.members[0]
    }

    /// Whether the user has not kept the group: it counts and shows its badges.
    pub fn is_open(&self) -> bool {
        self.kept.is_none()
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

/// Every group of copies in the library, ordered by their oldest member, with its kept state.
pub fn copy_groups(conn: &Connection) -> DbResult<Vec<CopyGroup>> {
    // Only movies that share a key with another one can be copies: SQLite narrows the library to
    // them in one pass (every list page reads the groups), and the links are made below. The
    // keys fold case the way the links do, so no pair is lost.
    let mut stmt = conn.prepare_cached(
        "WITH e AS MATERIALIZED ( \
           SELECT i.id, t.value AS tmdb, m.value AS imdb, fold_copy_key(i.title) AS title, \
                  i.year, fold_copy_key(i.edition) AS edition \
           FROM item i \
           LEFT JOIN ext_id t ON t.item_id = i.id AND t.provider = ?1 \
           LEFT JOIN ext_id m ON m.item_id = i.id AND m.provider = ?2 \
           WHERE i.kind = 'movie' AND i.availability = 'online' AND i.trashed_entry IS NULL \
             AND EXISTS (SELECT 1 FROM media_file f WHERE f.item_id = i.id AND f.kind = 'video')) \
         SELECT id, tmdb, imdb, title, year, edition FROM e \
         WHERE (edition, tmdb) IN (SELECT edition, tmdb FROM e WHERE tmdb IS NOT NULL \
                                   GROUP BY edition, tmdb HAVING count(*) > 1) \
            OR (edition, imdb) IN (SELECT edition, imdb FROM e WHERE imdb IS NOT NULL \
                                   GROUP BY edition, imdb HAVING count(*) > 1) \
            OR (edition, title, year) IN ( \
                 SELECT edition, title, year FROM e WHERE year > 0 AND title <> '' \
                 GROUP BY edition, title, year \
                 HAVING count(*) > 1 AND sum(tmdb IS NULL AND imdb IS NULL) > 0) \
         ORDER BY id",
    )?;
    let candidates = stmt
        .query_map(params![id_key::TMDB, id_key::IMDB], |r| {
            Ok(Candidate {
                id: ItemId(r.get(0)?),
                tmdb: r.get(1)?,
                imdb: r.get(2)?,
                title: r.get(3)?,
                year: r.get(4)?,
                edition: r.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut groups = group(&candidates);
    if !groups.is_empty() {
        let kept = kept_sets(conn)?;
        for g in &mut groups {
            g.kept = covering_set(&kept, &g.members);
        }
    }
    Ok(groups)
}

/// The kept sets each movie belongs to.
fn kept_sets(conn: &Connection) -> DbResult<HashMap<ItemId, BTreeSet<KeptSetId>>> {
    let mut stmt = conn.prepare_cached("SELECT item_id, set_id FROM copies_kept")?;
    let mut rows = stmt.query([])?;
    let mut sets: HashMap<ItemId, BTreeSet<KeptSetId>> = HashMap::new();
    while let Some(r) = rows.next()? {
        sets.entry(ItemId(r.get(0)?))
            .or_default()
            .insert(KeptSetId(r.get(1)?));
    }
    Ok(sets)
}

/// The newest kept set that holds every one of `members`.
fn covering_set(
    kept: &HashMap<ItemId, BTreeSet<KeptSetId>>,
    members: &[ItemId],
) -> Option<KeptSetId> {
    let (first, rest) = members.split_first()?;
    let mut common = kept.get(first)?.clone();
    for member in rest {
        let sets = kept.get(member)?;
        common.retain(|set| sets.contains(set));
    }
    common.last().copied()
}

/// The size of each of `ids` under the library's one size rule (distinct files of online rows).
pub fn copy_sizes(conn: &Connection, ids: &[ItemId]) -> DbResult<HashMap<ItemId, u64>> {
    let sql = format!(
        "SELECT i.id, {} FROM item i WHERE i.id IN (SELECT value FROM json_each(?1))",
        super::size_totals::counted_size_sql("i.id")
    );
    let ids = serde_json::to_string(&ids.iter().map(|id| id.0).collect::<Vec<_>>())?;
    let mut stmt = conn.prepare(&sql)?;
    let sizes = stmt
        .query_map([ids], |r| {
            Ok((
                ItemId(r.get(0)?),
                u64::try_from(r.get::<_, i64>(1)?).unwrap_or(0),
            ))
        })?
        .collect::<Result<HashMap<_, _>, _>>()?;
    Ok(sizes)
}

/// Records that the user keeps all of `members` as copies ("Keep both" / "Keep all"), in one new
/// set. Returns the set, which [`remove_kept_set`] takes back.
///
/// # Errors
/// Fewer than two movies, or one of them does not exist.
pub fn keep_set(conn: &Connection, members: &[ItemId], now_ms: i64) -> DbResult<KeptSetId> {
    let members: BTreeSet<ItemId> = members.iter().copied().collect();
    if members.len() < 2 {
        return Err(DbError::InvalidData(
            "keeping copies needs at least two movies".into(),
        ));
    }
    for &id in &members {
        if !super::items::item_exists(conn, id)? {
            return Err(DbError::ItemNotFound(id.0));
        }
    }
    let set: i64 = conn.query_row(
        "SELECT COALESCE(MAX(set_id), 0) + 1 FROM copies_kept",
        [],
        |r| r.get(0),
    )?;
    let mut insert = conn
        .prepare_cached("INSERT INTO copies_kept (set_id, item_id, kept_ms) VALUES (?1, ?2, ?3)")?;
    for id in members {
        insert.execute(params![set, id.0, now_ms])?;
    }
    Ok(KeptSetId(set))
}

/// Takes back a "keep both" decision ("Ask again", or Undo of the keep): the group counts again.
pub fn remove_kept_set(conn: &Connection, set: KeptSetId) -> DbResult<()> {
    conn.execute("DELETE FROM copies_kept WHERE set_id = ?1", [set.0])?;
    Ok(())
}

/// Text compared without regard to case and surrounding spaces.
fn fold(text: &str) -> String {
    text.trim().to_lowercase()
}

/// Registers `fold_copy_key(text)`, the SQL form of [`fold`], on a connection.
pub(crate) fn register_functions(conn: &Connection) -> rusqlite::Result<()> {
    use rusqlite::functions::FunctionFlags;
    conn.create_scalar_function(
        "fold_copy_key",
        1,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| Ok(fold(&ctx.get::<String>(0)?)),
    )
}

/// Links the candidates by the copy rules and returns the groups of two or more.
///
/// Two movies with different TMDB IDs are never put in one group, whatever else links them: an
/// IMDb ID shared across two TMDB IDs is a wrong ID on one of them, not one film.
fn group(candidates: &[Candidate]) -> Vec<CopyGroup> {
    let mut sets = UnionFind::new(candidates);
    // TMDB first, so the films are known before an IMDb link could join two of them.
    for provider in ["tmdb", "imdb"] {
        let mut first_with: HashMap<(&str, &str), usize> = HashMap::new();
        for (index, c) in candidates.iter().enumerate() {
            let value = if provider == "tmdb" { &c.tmdb } else { &c.imdb };
            if let Some(value) = value {
                let first = *first_with
                    .entry((c.edition.as_str(), value.as_str()))
                    .or_insert(index);
                sets.union(first, index);
            }
        }
    }
    let mut same_title: HashMap<(&str, &str, i64), Vec<usize>> = HashMap::new();
    for (index, c) in candidates.iter().enumerate() {
        if c.year > 0 && !c.title.is_empty() {
            same_title
                .entry((c.edition.as_str(), c.title.as_str(), c.year))
                .or_default()
                .push(index);
        }
    }
    // A movie with neither ID joins the one film its title and year name: the group of every
    // movie with IDs of that title and year, or the other movies without IDs when none has
    // one. When the movies with IDs are two films, it is left out: it cannot be told which.
    for members in same_title.values() {
        let (without, with): (Vec<usize>, Vec<usize>) = members.iter().partition(|&&index| {
            candidates[index].tmdb.is_none() && candidates[index].imdb.is_none()
        });
        let films: BTreeSet<usize> = with.iter().map(|&index| sets.find(index)).collect();
        let anchor = match films.len() {
            0 => without.first().copied(),
            1 => films.first().copied(),
            _ => None,
        };
        if let Some(anchor) = anchor {
            for index in without {
                sets.union(anchor, index);
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
            CopyGroup {
                members,
                kept: None,
            }
        })
        .collect();
    groups.sort_by_key(CopyGroup::id);
    groups
}

/// A disjoint-set forest over candidate positions that never joins two TMDB IDs.
struct UnionFind<'a> {
    parent: Vec<usize>,
    /// The TMDB ID of each set, at its root.
    tmdb: Vec<Option<&'a str>>,
}

impl<'a> UnionFind<'a> {
    fn new(candidates: &'a [Candidate]) -> Self {
        Self {
            parent: (0..candidates.len()).collect(),
            tmdb: candidates.iter().map(|c| c.tmdb.as_deref()).collect(),
        }
    }

    fn find(&mut self, mut index: usize) -> usize {
        while self.parent[index] != index {
            self.parent[index] = self.parent[self.parent[index]];
            index = self.parent[index];
        }
        index
    }

    /// Joins the sets of `a` and `b`, unless they hold different TMDB IDs.
    fn union(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if a == b {
            return;
        }
        if let (Some(x), Some(y)) = (self.tmdb[a], self.tmdb[b])
            && x != y
        {
            return;
        }
        let (root, child) = (a.min(b), a.max(b));
        self.tmdb[root] = self.tmdb[root].or(self.tmdb[child]);
        self.parent[child] = root;
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
    fn an_unscraped_movie_does_not_bridge_two_films_of_one_title() {
        let conn = memory_db();
        movie(&conn, "Example Movie", 2020, "", Some("1"), Some("tt1"));
        movie(&conn, "Example Movie", 2020, "", Some("2"), Some("tt2"));
        movie(&conn, "Example Movie", 2020, "", None, None);
        assert!(
            groups(&conn).is_empty(),
            "two films: the new one cannot be placed"
        );

        // One film of that title in the library twice: the new movie joins it.
        let conn = memory_db();
        let a = movie(&conn, "Example Movie", 2020, "", Some("1"), None);
        let b = movie(&conn, "Example Movie", 2020, "", Some("1"), Some("tt1"));
        let new = movie(&conn, "Example Movie", 2020, "", None, None);
        assert_eq!(groups(&conn), vec![vec![a, b, new]]);
    }

    #[test]
    fn two_unscraped_movies_of_one_title_and_year_are_copies() {
        let conn = memory_db();
        let a = movie(&conn, "Example Movie", 2020, "", None, None);
        let b = movie(&conn, "example movie", 2020, "", None, None);

        assert_eq!(groups(&conn), vec![vec![a, b]]);
    }

    #[test]
    fn an_imdb_id_never_joins_two_tmdb_ids() {
        let conn = memory_db();
        movie(&conn, "Example Movie", 2020, "", Some("1"), Some("tt1"));
        movie(&conn, "Example Movie", 2020, "", Some("2"), Some("tt1"));
        assert!(groups(&conn).is_empty());

        // Through a third movie with the IMDb ID only, the first film keeps it and the second
        // stays apart.
        let conn = memory_db();
        let a = movie(&conn, "Example Movie", 2020, "", Some("1"), Some("tt1"));
        let b = movie(&conn, "Example Movie", 2020, "", None, Some("tt1"));
        movie(&conn, "Example Movie", 2020, "", Some("2"), Some("tt1"));
        assert_eq!(groups(&conn), vec![vec![a, b]]);
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
    fn a_kept_group_is_closed_until_a_new_copy_joins() {
        let conn = memory_db();
        let a = movie(&conn, "Example Movie", 2020, "", Some("1"), None);
        let b = movie(&conn, "Example Movie", 2020, "", Some("1"), None);
        assert!(copy_groups(&conn).unwrap()[0].is_open());

        let set = keep_set(&conn, &[b, a], 10).unwrap();
        let kept = copy_groups(&conn).unwrap();
        assert_eq!(kept[0].kept, Some(set));
        assert!(!kept[0].is_open());

        // A third copy found later opens the group again, with all three.
        let c = movie(&conn, "Example Movie", 2020, "", Some("1"), None);
        let reopened = copy_groups(&conn).unwrap();
        assert_eq!(reopened[0].members, vec![a, b, c]);
        assert!(reopened[0].is_open());
    }

    #[test]
    fn a_kept_group_that_loses_a_copy_stays_kept() {
        let conn = memory_db();
        let a = movie(&conn, "Example Movie", 2020, "", Some("1"), None);
        let b = movie(&conn, "Example Movie", 2020, "", Some("1"), None);
        let c = movie(&conn, "Example Movie", 2020, "", Some("1"), None);
        let set = keep_set(&conn, &[a, b, c], 10).unwrap();

        conn.execute("UPDATE item SET trashed_entry = 3 WHERE id = ?1", [c.0])
            .unwrap();

        let groups = copy_groups(&conn).unwrap();
        assert_eq!(groups[0].members, vec![a, b]);
        assert_eq!(groups[0].kept, Some(set));
    }

    #[test]
    fn ask_again_opens_a_kept_group() {
        let conn = memory_db();
        let a = movie(&conn, "Example Movie", 2020, "", Some("1"), None);
        let b = movie(&conn, "Example Movie", 2020, "", Some("1"), None);
        let first = keep_set(&conn, &[a, b], 10).unwrap();
        let second = keep_set(&conn, &[a, b], 20).unwrap();
        assert_ne!(first, second);
        assert_eq!(copy_groups(&conn).unwrap()[0].kept, Some(second));

        remove_kept_set(&conn, second).unwrap();
        assert_eq!(copy_groups(&conn).unwrap()[0].kept, Some(first));
        remove_kept_set(&conn, first).unwrap();
        assert!(copy_groups(&conn).unwrap()[0].is_open());
    }

    #[test]
    fn keeping_needs_two_existing_movies() {
        let conn = memory_db();
        let a = movie(&conn, "Example Movie", 2020, "", Some("1"), None);

        assert!(keep_set(&conn, &[a, a], 10).is_err());
        assert!(matches!(
            keep_set(&conn, &[a, ItemId(999)], 10),
            Err(DbError::ItemNotFound(999))
        ));
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
