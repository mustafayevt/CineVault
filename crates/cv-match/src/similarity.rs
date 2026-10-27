//! String similarity: word letter-pair Sørensen–Dice and Jaro-Winkler.
//!
//! [`title_similarity`] combines them into the base title score used by [`crate::score`].

use std::collections::HashMap;

/// Characters replaced by a space before a second Dice comparison (`[ ] _ . : |`).
const NON_SEARCH_CHARS: [char; 6] = ['[', ']', '_', '.', ':', '|'];

/// Sørensen–Dice coefficient over the letter pairs of each word, case-insensitive.
///
/// Pairs never span a word boundary. A word of one character ("2", the "L" and "A" of "L.A.")
/// counts as one token of its own, so an extra short word lowers the score: "Heat" is not a
/// perfect match for "L.A. Heat 2". Equal strings (ignoring case) score 1.0. Two strings without
/// any token (empty) that are not equal score 0.0.
pub fn dice_bigram(a: &str, b: &str) -> f64 {
    if a.to_lowercase() == b.to_lowercase() {
        return 1.0;
    }
    let pairs_a = letter_pairs(a);
    let pairs_b = letter_pairs(b);
    let total = pairs_a.len() + pairs_b.len();
    if total == 0 {
        return 0.0;
    }
    let mut remaining: HashMap<(char, char), usize> = HashMap::new();
    for pair in &pairs_b {
        *remaining.entry(*pair).or_default() += 1;
    }
    let mut common = 0usize;
    for pair in &pairs_a {
        if let Some(n) = remaining.get_mut(pair)
            && *n > 0
        {
            *n -= 1;
            common += 1;
        }
    }
    (2 * common) as f64 / total as f64
}

fn letter_pairs(s: &str) -> Vec<(char, char)> {
    let mut out = Vec::new();
    for word in s.split_whitespace() {
        let chars: Vec<char> = word.chars().flat_map(char::to_lowercase).collect();
        if let [only] = chars.as_slice() {
            out.push((*only, '\0'));
        }
        out.extend(chars.windows(2).map(|w| (w[0], w[1])));
    }
    out
}

/// Jaro-Winkler similarity (prefix up to 4, scaling 0.1, boost above Jaro 0.7) on text that is
/// trimmed, whitespace-collapsed and upper-cased.
pub fn jaro_winkler(a: &str, b: &str) -> f64 {
    let a: Vec<char> = normalize(a).chars().collect();
    let b: Vec<char> = normalize(b).chars().collect();
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let jaro = jaro(&a, &b);
    if jaro <= 0.7 {
        return jaro;
    }
    let prefix = a.iter().zip(&b).take(4).take_while(|(x, y)| x == y).count();
    jaro + prefix as f64 * 0.1 * (1.0 - jaro)
}

fn normalize(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_uppercase()
}

fn jaro(a: &[char], b: &[char]) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let window = (a.len().max(b.len()) / 2).saturating_sub(1);
    let mut a_hit = vec![false; a.len()];
    let mut b_hit = vec![false; b.len()];
    let mut matches = 0usize;
    for (i, ca) in a.iter().enumerate() {
        let lo = i.saturating_sub(window);
        let hi = (i + window + 1).min(b.len());
        for j in lo..hi {
            if !b_hit[j] && b[j] == *ca {
                a_hit[i] = true;
                b_hit[j] = true;
                matches += 1;
                break;
            }
        }
    }
    if matches == 0 {
        return 0.0;
    }
    let a_matched = a.iter().zip(&a_hit).filter(|(_, h)| **h).map(|(c, _)| c);
    let b_matched = b.iter().zip(&b_hit).filter(|(_, h)| **h).map(|(c, _)| c);
    let transposed = a_matched.zip(b_matched).filter(|(x, y)| x != y).count() / 2;
    let m = matches as f64;
    (m / a.len() as f64 + m / b.len() as f64 + (m - transposed as f64) / m) / 3.0
}

/// The best similarity between a query and one candidate title.
///
/// The maximum of: Dice on the raw strings; Dice with `[ ] _ . : |` in the title replaced by
/// spaces; Dice with a trailing ` yyyy` removed from the query; and Jaro-Winkler.
pub fn title_similarity(query: &str, title: &str) -> f64 {
    let query = query.trim();
    let title = title.trim();
    if query.is_empty() || title.is_empty() {
        return 0.0;
    }
    let cleaned: String = title
        .chars()
        .map(|c| {
            if NON_SEARCH_CHARS.contains(&c) {
                ' '
            } else {
                c
            }
        })
        .collect();
    let mut best = dice_bigram(query, title).max(dice_bigram(query, &cleaned));
    if let Some(stripped) = strip_trailing_year(query) {
        best = best.max(dice_bigram(stripped, title));
    }
    best.max(jaro_winkler(query, title)).clamp(0.0, 1.0)
}

fn strip_trailing_year(s: &str) -> Option<&str> {
    let (head, tail) = s.rsplit_once(' ')?;
    (tail.len() == 4 && tail.bytes().all(|b| b.is_ascii_digit())).then(|| head.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_extra_short_word_is_not_a_perfect_title_match() {
        // A regression case: "L.A. Heat 2" scored as an exact match for "Heat".
        assert!(title_similarity("Heat", "L.A. Heat 2") < 0.8);
        assert!(title_similarity("Heat", "Heat 2") < 0.95);
        assert!(title_similarity("Heat", "Heat") > 0.99);
        // A one-letter word on both sides still matches.
        assert!(title_similarity("Alien 3", "Alien 3") > 0.99);
        assert!(title_similarity("Alien 3", "Alien³") < title_similarity("Alien 3", "Alien 3"));
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn dice_known_values() {
        assert!(close(dice_bigram("night", "nacht"), 0.25));
        assert!(close(dice_bigram("France", "French"), 0.4));
        assert_eq!(dice_bigram("Solaris", "SOLARIS"), 1.0);
        assert_eq!(dice_bigram("abc", "xyz"), 0.0);
    }

    #[test]
    fn dice_ignores_word_order_boundaries() {
        // Pairs never span words, so "ab cd" has pairs ab, cd (not bc).
        assert!(close(dice_bigram("ab cd", "cd ab"), 1.0));
        assert!(close(dice_bigram("ab cd", "abcd"), 4.0 / 5.0));
    }

    #[test]
    fn dice_degenerate_inputs() {
        assert_eq!(dice_bigram("", ""), 1.0);
        assert_eq!(dice_bigram("a", "b"), 0.0);
        assert_eq!(dice_bigram("", "abc"), 0.0);
    }

    #[test]
    fn jaro_winkler_known_values() {
        assert!(close(jaro_winkler("MARTHA", "MARHTA"), 0.961));
        assert!(close(jaro_winkler("DWAYNE", "DUANE"), 0.84));
        assert!(close(jaro_winkler("DIXON", "DICKSONX"), 0.813));
        assert_eq!(jaro_winkler("same", "  SAME "), 1.0);
        assert_eq!(jaro_winkler("abc", "xyz"), 0.0);
    }

    #[test]
    fn title_similarity_variants() {
        assert_eq!(title_similarity("Solaris", "solaris"), 1.0);
        // Separator characters in the title do not hurt.
        assert!(title_similarity("Mission Impossible", "Mission:Impossible") > 0.99);
        // A trailing year in the query is ignored for the Dice comparison.
        assert_eq!(title_similarity("Crash 2004", "Crash"), 1.0);
        assert_eq!(title_similarity("", "Crash"), 0.0);
        assert!(title_similarity("Dune", "Casablanca") < 0.6);
    }
}
