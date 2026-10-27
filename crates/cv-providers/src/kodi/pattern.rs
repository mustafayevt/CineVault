//! Compiling the regular expressions of a scraper.
//!
//! Scrapers are written for PCRE. The `fancy-regex` engine understands look-around and
//! back-references, but a few things PCRE accepts literally mean something else or fail there.
//! [`translate`] rewrites those before compiling:
//!
//! * `\<` and `\>` are the characters `<` and `>`, not word boundaries;
//! * an escaped punctuation character that is not special is the character itself (`\/`, `\"`);
//! * a `{` that does not start a quantifier is a literal brace;
//! * a `[` inside a character class is a literal bracket, and `]` first in a class is literal;
//! * `\h` is a blank and `\R` a line break.
//!
//! Expressions match case-insensitively with `.` matching line breaks, which is how scraper
//! expressions are written (`cs="yes"` makes one case-sensitive).

use fancy_regex::{Regex, RegexBuilder};

/// How much backtracking one match may do before it is given up, so a bad expression cannot
/// hang a scrape.
const BACKTRACK_LIMIT: usize = 2_000_000;

/// Compiles a scraper expression.
pub(crate) fn compile(pattern: &str, case_sensitive: bool) -> Result<Regex, fancy_regex::Error> {
    let flags = if case_sensitive { "(?s)" } else { "(?si)" };
    RegexBuilder::new(&format!("{flags}{}", translate(pattern)))
        .backtrack_limit(BACKTRACK_LIMIT)
        .build()
}

fn is_rust_meta(c: char) -> bool {
    matches!(
        c,
        '\\' | '.'
            | '+'
            | '*'
            | '?'
            | '('
            | ')'
            | '|'
            | '['
            | ']'
            | '{'
            | '}'
            | '^'
            | '$'
            | '#'
            | '&'
            | '-'
            | '~'
    )
}

/// Whether `chars[at..]` (starting at `{`) is a quantifier: `{n}`, `{n,}` or `{n,m}`. Returns
/// the index just after the closing brace.
fn quantifier_end(chars: &[char], at: usize) -> Option<usize> {
    let mut i = at + 1;
    let digits = |i: &mut usize| {
        let start = *i;
        while *i < chars.len() && chars[*i].is_ascii_digit() {
            *i += 1;
        }
        *i > start
    };
    if !digits(&mut i) {
        return None;
    }
    if i < chars.len() && chars[i] == ',' {
        i += 1;
        digits(&mut i);
    }
    (i < chars.len() && chars[i] == '}').then_some(i + 1)
}

/// Rewrites PCRE-isms (see the module documentation).
pub(crate) fn translate(pattern: &str) -> String {
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::with_capacity(pattern.len() + 8);
    let mut in_class = false;
    // True right after `[` or `[^`, where a `]` is a literal member.
    let mut class_open = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && i + 1 < chars.len() {
            let d = chars[i + 1];
            class_open = false;
            match d {
                '<' | '>' => out.push(d),
                'h' => out.push_str(if in_class { " \\t" } else { "[ \\t]" }),
                'R' if !in_class => out.push_str("(?:\\r\\n|\\n|\\r)"),
                d if d.is_ascii_alphanumeric() || is_rust_meta(d) => {
                    out.push('\\');
                    out.push(d);
                }
                d => out.push(d),
            }
            i += 2;
            continue;
        }
        if in_class {
            match c {
                ']' if class_open => out.push_str("\\]"),
                ']' => {
                    in_class = false;
                    out.push(']');
                }
                '[' if chars.get(i + 1) == Some(&':') => out.push('['),
                '[' | '&' | '~' => {
                    out.push('\\');
                    out.push(c);
                }
                _ => out.push(c),
            }
            class_open = false;
            i += 1;
            continue;
        }
        match c {
            '[' => {
                in_class = true;
                class_open = true;
                out.push('[');
                if chars.get(i + 1) == Some(&'^') {
                    out.push('^');
                    i += 1;
                }
            }
            '{' => match quantifier_end(&chars, i) {
                Some(end) => {
                    out.extend(&chars[i..end]);
                    i = end;
                    continue;
                }
                None => out.push_str("\\{"),
            },
            _ => out.push(c),
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find(pattern: &str, text: &str) -> Option<String> {
        let re = compile(pattern, false).unwrap();
        re.captures(text)
            .unwrap()
            .map(|c| c.get(1).map_or(String::new(), |m| m.as_str().to_owned()))
    }

    #[test]
    fn matches_across_lines_and_ignoring_case() {
        assert_eq!(
            find("<DIV>(.*?)</div>", "<div>a\nb</div>").as_deref(),
            Some("a\nb")
        );
        assert!(
            compile("abc", true)
                .unwrap()
                .is_match("ABC")
                .map(|m| !m)
                .unwrap()
        );
    }

    #[test]
    fn look_around_and_back_references() {
        assert_eq!(
            find("(\\w+)(?= film)", "a good film").as_deref(),
            Some("good")
        );
        assert_eq!(find("(\\w)\\1", "hello").as_deref(), Some("l"));
    }

    #[test]
    fn pcre_literals() {
        assert_eq!(
            find("href=\\\"(.*?)\\\"", "href=\"/a/b\"").as_deref(),
            Some("/a/b")
        );
        assert_eq!(
            find("\\/title\\/(tt\\d+)", "/title/tt0113277").as_deref(),
            Some("tt0113277")
        );
        assert_eq!(
            find("var x = {(.*?)};", "var x = {a:1};").as_deref(),
            Some("a:1")
        );
        assert_eq!(find("a\\<(\\w)\\>", "a<b>").as_deref(), Some("b"));
        assert_eq!(find("([[]\\d+[]])", "id [42]").as_deref(), Some("[42]"));
        assert_eq!(find("(\\d{2,3})", "x1234").as_deref(), Some("123"));
    }

    #[test]
    fn a_broken_expression_is_an_error_not_a_panic() {
        assert!(compile("(unclosed", false).is_err());
    }
}
