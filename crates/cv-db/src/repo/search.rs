//! Forgiving title search: the folded search keys and the SQL that reads them.
//!
//! Every item keeps two folded keys, `item.search_title` (its title and English title) and
//! `item.search_alt` (its original and sort title), written by [`write_keys`] whenever the item
//! is saved and filled for existing libraries by migration 101. Folding ([`fold`]) lowercases,
//! removes diacritics and apostrophes, turns `&` into `and`, turns every other punctuation mark
//! into a space and writes Roman numerals from II to XX as numbers, so `Spider-Man`,
//! `spider man` and `Spiderman`, `Amélie` and `amelie`, `Rocky II` and `rocky 2` all meet.
//!
//! A key is `|segment|segment|`. A title contributes its spaced form (`wall e`), its compact
//! form without spaces (`walle`) and, when it starts with an article, the form without it, so one
//! `instr` finds a segment that starts with the typed words (`'|' || text`), is exactly them
//! (`'|' || text || '|'`) or holds a word that starts with them (`' ' || text`).
//!
//! The search condition ([`write_condition`]) needs every typed word to start a word of a title,
//! or to be found by the full-text index (plot, people, tags, file names), or to be the year. When
//! it finds nothing in the library, a small typo is tolerated (`interstelar`). The order
//! ([`rank_sql`]) is: exact title, title prefix, word of a title, original or sort title, people,
//! other text, typo.

use rusqlite::functions::FunctionFlags;
use rusqlite::{Connection, params};
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

use super::filter::Sql;
use super::fts;
use crate::error::DbResult;

/// Folds text for searching: lower case, no diacritics, no apostrophes, `&` as `and`, other
/// punctuation as a space, Roman numerals II to XX as numbers. Words are separated by one space.
pub fn fold(text: &str) -> String {
    let mut spaced = String::with_capacity(text.len());
    for c in text.nfkd() {
        if is_combining_mark(c) {
            continue;
        }
        match c {
            '\'' | '\u{2019}' | '\u{2018}' | '`' | '\u{02BC}' | '\u{00B4}' => {}
            '&' => spaced.push_str(" and "),
            'ø' | 'Ø' => spaced.push('o'),
            'đ' | 'Đ' | 'ð' | 'Ð' => spaced.push('d'),
            'ł' | 'Ł' => spaced.push('l'),
            'ı' => spaced.push('i'),
            'ß' => spaced.push_str("ss"),
            'æ' | 'Æ' => spaced.push_str("ae"),
            'œ' | 'Œ' => spaced.push_str("oe"),
            'þ' | 'Þ' => spaced.push_str("th"),
            c if c.is_alphanumeric() => spaced.extend(c.to_lowercase()),
            _ => spaced.push(' '),
        }
    }
    let mut out = String::with_capacity(spaced.len());
    for word in spaced.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        match roman(word) {
            Some(n) => out.push_str(n),
            None => out.push_str(word),
        }
    }
    out
}

/// A Roman numeral from II to XX as the number. The single letters (I, V, X) stay as they are:
/// they are words or initials far more often than numbers.
fn roman(word: &str) -> Option<&'static str> {
    Some(match word {
        "ii" => "2",
        "iii" => "3",
        "iv" => "4",
        "vi" => "6",
        "vii" => "7",
        "viii" => "8",
        "ix" => "9",
        "xi" => "11",
        "xii" => "12",
        "xiii" => "13",
        "xiv" => "14",
        "xv" => "15",
        "xvi" => "16",
        "xvii" => "17",
        "xviii" => "18",
        "xix" => "19",
        "xx" => "20",
        _ => return None,
    })
}

/// The folded text without its spaces.
fn compact(folded: &str) -> String {
    folded.chars().filter(|c| *c != ' ').collect()
}

/// The key of a set of titles: `|spider man|spiderman|`, or an empty string for no title.
pub fn key_of(titles: &[&str]) -> String {
    let mut segments: Vec<String> = Vec::new();
    let mut add = |segment: String| {
        if !segment.is_empty() && !segments.contains(&segment) {
            segments.push(segment);
        }
    };
    for title in titles {
        let folded = fold(title);
        if folded.is_empty() {
            continue;
        }
        for article in ["the ", "a ", "an "] {
            if let Some(rest) = folded.strip_prefix(article) {
                add(rest.to_owned());
                add(compact(rest));
            }
        }
        add(compact(&folded));
        add(folded);
    }
    if segments.is_empty() {
        String::new()
    } else {
        format!("|{}|", segments.join("|"))
    }
}

/// Recomputes both keys of one item from its current titles.
pub(crate) fn write_keys(conn: &Connection, id: i64) -> DbResult<()> {
    let titles: Option<[String; 4]> = conn
        .prepare_cached(
            "SELECT title, english_title, original_title, sort_title FROM item WHERE id = ?1",
        )?
        .query_row([id], |r| Ok([r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?]))
        .ok();
    let Some([title, english, original, sort]) = titles else {
        return Ok(());
    };
    conn.prepare_cached("UPDATE item SET search_title = ?2, search_alt = ?3 WHERE id = ?1")?
        .execute(params![
            id,
            key_of(&[&title, &english]),
            key_of(&[&original, &sort])
        ])?;
    Ok(())
}

/// Recomputes the keys of every item. Returns the number of items.
pub(crate) fn rewrite_all_keys(conn: &Connection) -> DbResult<usize> {
    let ids: Vec<i64> = conn
        .prepare("SELECT id FROM item")?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    for id in &ids {
        write_keys(conn, *id)?;
    }
    Ok(ids.len())
}

/// Registers the SQL functions the search and migration 101 use on a connection.
pub(crate) fn register_functions(conn: &Connection) -> rusqlite::Result<()> {
    let flags = FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC;
    // cv_search_key(a, b): the key of two titles, for the migration.
    conn.create_scalar_function("cv_search_key", 2, flags, |ctx| {
        let a: String = ctx.get(0)?;
        let b: String = ctx.get(1)?;
        Ok(key_of(&[&a, &b]))
    })?;
    // cv_search_fuzzy(title_key, alt_key, words): the words match with a small typo.
    conn.create_scalar_function("cv_search_fuzzy", 3, flags, |ctx| {
        let text = |i: usize| ctx.get_raw(i).as_str().map_err(rusqlite::Error::from);
        Ok(fuzzy_match(text(0)?, text(1)?, text(2)?))
    })?;
    Ok(())
}

/// Every word of `words` starts a word of the keys, and at least one of them only after allowing
/// for a typo.
fn fuzzy_match(title_key: &str, alt_key: &str, words: &str) -> bool {
    let known = || {
        title_key
            .split(['|', ' '])
            .chain(alt_key.split(['|', ' ']))
            .filter(|w| !w.is_empty())
    };
    let mut any_fuzzy = false;
    for word in words.split(' ').filter(|w| !w.is_empty()) {
        if known().any(|k| k.starts_with(word)) {
            continue;
        }
        let typed: Vec<char> = word.chars().collect();
        if typed.len() < 4 {
            return false;
        }
        let allowed = if typed.len() >= 8 { 2 } else { 1 };
        if known().any(|k| near(k, &typed, allowed)) {
            any_fuzzy = true;
        } else {
            return false;
        }
    }
    any_fuzzy
}

/// The typed word is within `allowed` edits of the whole known word or of a start of it as long as
/// the typed word (one character either way), so a typo in a half-typed word still matches. The
/// first letter must be right, or swapped with the second, which rules most words out before any
/// distance is worked out.
fn near(known: &str, typed: &[char], allowed: usize) -> bool {
    let mut chars = known.chars();
    let (Some(first), second) = (chars.next(), chars.next()) else {
        return false;
    };
    if first != typed[0] && Some(first) != typed.get(1).copied() {
        return false;
    }
    if second.is_some_and(|s| first != typed[0] && s != typed[0]) {
        return false;
    }
    let known: Vec<char> = known.chars().collect();
    if known.len() + allowed + 1 < typed.len() {
        return false;
    }
    for take in typed.len().saturating_sub(1)..=typed.len() + 1 {
        if take > known.len() {
            break;
        }
        if distance(&known[..take], typed) <= allowed {
            return true;
        }
    }
    false
}

/// Edit distance with insertions, deletions, substitutions and swapped neighbours.
fn distance(a: &[char], b: &[char]) -> usize {
    let m = b.len();
    let mut before: Vec<usize> = Vec::new();
    let mut prev: Vec<usize> = (0..=m).collect();
    let mut row = vec![0usize; m + 1];
    for i in 1..=a.len() {
        row[0] = i;
        for j in 1..=m {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut best = (prev[j] + 1).min(row[j - 1] + 1).min(prev[j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                best = best.min(before[j - 2] + 1);
            }
            row[j] = best;
        }
        before = std::mem::replace(&mut prev, row.clone());
    }
    prev[m]
}

/// What was typed, folded: the words and the year it may name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Words {
    /// Folded words, in order.
    pub tokens: Vec<String>,
}

impl Words {
    pub(crate) fn parse(text: &str) -> Self {
        Self {
            tokens: fold(text)
                .split(' ')
                .filter(|w| !w.is_empty())
                .map(str::to_owned)
                .collect(),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }

    /// The year a word names, when it looks like one.
    fn year(token: &str) -> Option<i64> {
        if token.len() == 4 && token.bytes().all(|b| b.is_ascii_digit()) {
            token.parse().ok().filter(|y| (1880..=2100).contains(y))
        } else {
            None
        }
    }

    /// The words without the years, unless nothing else is left.
    fn without_years(&self) -> Vec<&str> {
        let rest: Vec<&str> = self
            .tokens
            .iter()
            .map(String::as_str)
            .filter(|t| Self::year(t).is_none())
            .collect();
        if rest.is_empty() {
            self.tokens.iter().map(String::as_str).collect()
        } else {
            rest
        }
    }

    fn compact(&self) -> String {
        self.tokens.concat()
    }
}

/// The condition for one word: it starts a word of a title, is found by the full-text index or
/// is the year.
fn write_word(out: &mut Sql, alias: &str, token: &str) {
    out.push("(");
    for (i, key) in ["search_title", "search_alt"].into_iter().enumerate() {
        if i > 0 {
            out.push(" OR ");
        }
        out.push(&format!("instr({alias}.{key}, "));
        out.bind(format!(" {token}"));
        out.push(&format!(") > 0 OR instr({alias}.{key}, "));
        out.bind(format!("|{token}"));
        out.push(") > 0");
    }
    if let Some(expr) = fts::match_expression(token) {
        out.push(&format!(
            " OR {alias}.id IN (SELECT rowid FROM item_fts WHERE item_fts MATCH "
        ));
        out.bind(expr);
        out.push(")");
    }
    if let Some(year) = Words::year(token) {
        out.push(&format!(" OR {alias}.year = {year}"));
    }
    out.push(")");
}

/// The words are found, without the typo tolerance.
fn write_exact(out: &mut Sql, alias: &str, words: &Words) {
    out.push("(");
    for (i, token) in words.tokens.iter().enumerate() {
        if i > 0 {
            out.push(" AND ");
        }
        write_word(out, alias, token);
    }
    if words.tokens.len() > 1 {
        // "spider man" finds "Spiderman", "wall e" finds "WALL-E" written as one word.
        let compact = words.compact();
        out.push(&format!(" OR instr({alias}.search_title, "));
        out.bind(format!("|{compact}"));
        out.push(&format!(") > 0 OR instr({alias}.search_alt, "));
        out.bind(format!("|{compact}"));
        out.push(") > 0");
    }
    out.push(")");
}

/// The condition of the typed words without the typo tolerance.
pub(crate) fn write_exact_condition(out: &mut Sql, words: &Words) {
    write_exact(out, "item", words);
}

/// Writes the condition of the typed words (nothing is spliced into the SQL). The condition is
/// true for an item whose titles, people, tags, plot or file names hold the words, and, only when
/// no item of the library does, for an item with a title that is a typo away.
pub(crate) fn write_condition(out: &mut Sql, words: &Words) {
    let kind = out.kind;
    out.push("(");
    write_exact(out, "item", words);
    // The typo tolerance applies only when the exact search finds nothing in the library: the
    // uncorrelated NOT EXISTS runs once per statement.
    out.push(" OR (NOT EXISTS (SELECT 1 FROM item e WHERE e.trashed_entry IS NULL");
    if let Some(kind) = kind {
        out.push(" AND e.kind = ");
        out.bind(super::codec::enum_text(&kind).unwrap_or_default());
    }
    out.push(" AND ");
    write_exact(out, "e", words);
    out.push(") AND cv_search_fuzzy(item.search_title, item.search_alt, ");
    out.bind(words.tokens.join(" "));
    out.push("))");
    out.push(")");
}

/// The SQL of the rank of an item for the typed text, 0 (best) to 6: exact title, title prefix,
/// a word of a title, original or sort title, people, other text, typo. Every piece is a folded
/// word, which holds no quote, so it is written into the SQL as a literal.
pub(crate) fn rank_sql(text: &str) -> String {
    let words = Words::parse(text);
    let tokens = words.without_years();
    if tokens.is_empty() {
        return "(item.id * 0)".to_owned();
    }
    let phrase = tokens.join(" ");
    let compact = tokens.concat();
    let title = "item.search_title";
    let alt = "item.search_alt";
    let has = |key: &str, pattern: &str| format!("instr({key}, '{pattern}') > 0");
    let all_words = |keys: &[&str]| -> String {
        tokens
            .iter()
            .map(|t| {
                let parts: Vec<String> = keys
                    .iter()
                    .flat_map(|k| [has(k, &format!(" {t}")), has(k, &format!("|{t}"))])
                    .collect();
                format!("({})", parts.join(" OR "))
            })
            .collect::<Vec<_>>()
            .join(" AND ")
    };
    let fts_all = tokens
        .iter()
        .map(|t| format!("\"{t}\"*"))
        .collect::<Vec<_>>()
        .join(" ");
    let by = |scope: &str| {
        format!("item.id IN (SELECT rowid FROM item_fts WHERE item_fts MATCH '{scope}')")
    };
    format!(
        "(CASE \
         WHEN {e1} OR {e2} THEN 0 \
         WHEN {p1} OR {p2} THEN 1 \
         WHEN {w_title} OR {w_phrase} THEN 2 \
         WHEN {a1} OR {a2} OR {w_any} THEN 3 \
         WHEN {people} THEN 4 \
         WHEN {other} THEN 5 \
         ELSE 6 END)",
        e1 = has(title, &format!("|{phrase}|")),
        e2 = has(title, &format!("|{compact}|")),
        p1 = has(title, &format!("|{phrase}")),
        p2 = has(title, &format!("|{compact}")),
        w_title = all_words(&[title]),
        w_phrase = has(title, &format!(" {phrase}")),
        a1 = has(alt, &format!("|{phrase}")),
        a2 = has(alt, &format!("|{compact}")),
        w_any = all_words(&[title, alt]),
        people = by(&format!("people : ({fts_all})")),
        other = by(&fts_all),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folding_unifies_punctuation_spacing_and_diacritics() {
        assert_eq!(fold("Spider-Man"), "spider man");
        assert_eq!(fold("  SPIDER   MAN "), "spider man");
        assert_eq!(fold("WALL\u{b7}E"), "wall e");
        assert_eq!(fold("Mission: Impossible"), "mission impossible");
        assert_eq!(fold("Schindler's List"), "schindlers list");
        assert_eq!(fold("Schindler\u{2019}s List"), "schindlers list");
        assert_eq!(fold("Amélie"), "amelie");
        assert_eq!(fold("Léon"), "leon");
        assert_eq!(fold("Se7en"), "se7en");
        assert_eq!(fold("Pride & Prejudice"), "pride and prejudice");
        assert_eq!(fold("Rocky II"), "rocky 2");
        assert_eq!(fold("Rocky IV"), "rocky 4");
        assert_eq!(fold("Mix"), "mix");
        assert_eq!(fold("Cœur"), "coeur");
        assert_eq!(fold("..."), "");
    }

    #[test]
    fn a_key_holds_the_spaced_the_compact_and_the_article_free_forms() {
        assert_eq!(key_of(&["Spider-Man", ""]), "|spiderman|spider man|");
        assert_eq!(key_of(&[""]), "");
        let key = key_of(&["The Matrix"]);
        assert!(key.contains("|matrix|"), "{key}");
        assert!(key.contains("|thematrix|"), "{key}");
        assert!(key.contains("|the matrix|"), "{key}");
    }

    #[test]
    fn a_typo_is_one_or_two_edits_by_length() {
        let key = key_of(&["Interstellar"]);
        assert!(fuzzy_match(&key, "", "interstelar"));
        assert!(fuzzy_match(&key, "", "intersteller"));
        assert!(fuzzy_match(&key_of(&["Gladiator"]), "", "gladiatro"));
        assert!(!fuzzy_match(&key_of(&["Dune"]), "", "dnue x"));
        assert!(
            !fuzzy_match(&key_of(&["Alien"]), "", "ali"),
            "short words do not drift"
        );
        // an exact word alone is not a typo match
        assert!(!fuzzy_match(&key, "", "inter"));
    }
}
