//! People of the library: the actors and directors credited on movies and shows, found by name.
//!
//! The command palette searches them (`@` scope). Names that differ only in case are one person
//! here, as they are in the `actor:` and `director:` filters the palette opens.

use rusqlite::{Connection, params};

use crate::error::DbResult;

/// The most people one search returns.
pub const MAX_PEOPLE: usize = 200;

/// The role a person is found in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersonRole {
    Actor,
    Director,
}

/// A person credited in the library, with the number of movies and shows that credit them in
/// that role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonUsage {
    pub name: String,
    pub role: PersonRole,
    pub titles: u32,
}

/// Escapes the wildcards of a `LIKE` pattern (the escape character is `\`).
fn escape_like(term: &str) -> String {
    let mut out = String::with_capacity(term.len());
    for ch in term.chars() {
        if matches!(ch, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// The actors and directors of movies and shows whose name contains `term` (ignoring case),
/// most credited first, then by name. An empty term lists everyone. At most `limit` rows
/// ([`MAX_PEOPLE`] at most); one person credited as both actor and director is two rows.
pub fn search_people(conn: &Connection, term: &str, limit: usize) -> DbResult<Vec<PersonUsage>> {
    let pattern = format!("%{}%", escape_like(term.trim()));
    let limit = i64::try_from(limit.clamp(1, MAX_PEOPLE)).unwrap_or(1);
    let mut stmt = conn.prepare_cached(
        "SELECT min(p.name), c.kind, count(DISTINCT c.item_id) \
         FROM credit c \
         JOIN person p ON p.id = c.person_id \
         JOIN item i ON i.id = c.item_id \
         WHERE c.kind IN ('actor', 'director') AND i.kind IN ('movie', 'show') \
           AND i.trashed_entry IS NULL \
           AND trim(p.name) <> '' AND p.name LIKE ?1 ESCAPE '\\' \
         GROUP BY lower(p.name), c.kind \
         ORDER BY 3 DESC, 1 COLLATE NOCASE, 2 \
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![pattern, limit], |row| {
        let kind: String = row.get(1)?;
        Ok(PersonUsage {
            name: row.get(0)?,
            role: if kind == "director" {
                PersonRole::Director
            } else {
                PersonRole::Actor
            },
            titles: u32::try_from(row.get::<_, i64>(2)?).unwrap_or(0),
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

#[cfg(test)]
mod tests {
    use cv_domain::{Credit, CreditKind, Item};

    use super::*;
    use crate::Database;
    use crate::repo::insert_item;

    fn credit(name: &str, kind: CreditKind) -> Credit {
        Credit::new(kind, name, "")
    }

    fn movie(title: &str, credits: Vec<Credit>) -> Item {
        let mut item = Item::movie(title);
        item.core.credits = credits;
        item
    }

    #[test]
    fn people_are_found_by_part_of_their_name_with_their_title_counts() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(dir.path().join("library.db")).unwrap();
        db.write(|tx| {
            insert_item(
                tx,
                &movie(
                    "Heat",
                    vec![
                        credit("Al Pacino", CreditKind::Actor),
                        credit("Robert De Niro", CreditKind::Actor),
                        credit("Michael Mann", CreditKind::Director),
                    ],
                ),
            )?;
            insert_item(
                tx,
                &movie(
                    "The Insider",
                    vec![
                        credit("al pacino", CreditKind::Actor),
                        credit("Michael Mann", CreditKind::Director),
                        credit("Lisa Gerrard", CreditKind::Composer),
                    ],
                ),
            )?;
            Ok(())
        })
        .unwrap();

        let found = db.read(|conn| search_people(conn, "al", 10)).unwrap();
        assert_eq!(
            found,
            [PersonUsage {
                name: "Al Pacino".to_owned(),
                role: PersonRole::Actor,
                titles: 2,
            }]
        );
        let mann = db.read(|conn| search_people(conn, "MANN", 10)).unwrap();
        assert_eq!(mann[0].role, PersonRole::Director);
        assert_eq!(mann[0].titles, 2);
        // Composers are not searched, and a wildcard is a literal character.
        assert!(
            db.read(|conn| search_people(conn, "Gerrard", 10))
                .unwrap()
                .is_empty()
        );
        assert!(
            db.read(|conn| search_people(conn, "%", 10))
                .unwrap()
                .is_empty()
        );
        let everyone = db.read(|conn| search_people(conn, "", 2)).unwrap();
        assert_eq!(everyone.len(), 2);
        assert_eq!(everyone[0].titles, 2);
    }
}
