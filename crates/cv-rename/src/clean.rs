//! Name clean-up: what a rendered template becomes before it is used as a folder or file name.
//!
//! The template engine already made every token safe (see
//! `cv_template::tokens::NameTokenAppender`). [`create_destination`] then cleans the whole
//! result, literal text included, in this order:
//!
//! 1. empty `()`, `[]` and `{}` go (also when they only hold spaces), repeatedly;
//! 2. line breaks become spaces;
//! 3. a **file** name loses its path separators (`/` and `\` become spaces); a **folder** is
//!    split at them, every part is cleaned on its own, empty parts disappear, and a trailing
//!    separator stays;
//! 4. per part: `...` becomes `…` with the Unicode option, leading ` `, `.` and `-` and trailing
//!    ` `, `.`, `-` and `_` are removed, then spaces are replaced when space substitution is on;
//! 5. the ASCII option folds the text (`Amélie` is `Amelie`, `Ü` is `Ue`);
//! 6. invalid characters are replaced ([`replace_invalid_characters`]);
//! 7. runs of spaces collapse and the ends are trimmed.

use std::sync::LazyLock;

use cv_template::renamer_renderers::fold_to_ascii;
use cv_template::tokens::unicode_lookalike;
use regex::Regex;

use crate::options::{NameOptions, SpaceSubstitution};

/// Whether the name is a folder path or a file name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// A path below the data source: `/` and `\` separate its parts.
    Folder,
    /// One file name without extension.
    File,
}

static EMPTY_BRACKETS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\(\s*\)|\[\s*\]|\{\s*\}").expect("valid regex"));

/// Removes empty `()`, `[]` and `{}` until none is left (`[()]` goes completely).
pub fn remove_empty_brackets(text: &str) -> String {
    let mut text = text.to_owned();
    loop {
        let next = EMPTY_BRACKETS.replace_all(&text, "");
        if next == text {
            return text;
        }
        text = next.into_owned();
    }
}

/// Cleans a rendered name. See the module documentation for the steps.
pub fn create_destination(
    rendered: &str,
    place: Place,
    options: &NameOptions,
    space: &SpaceSubstitution,
) -> String {
    let text = remove_empty_brackets(rendered).replace(['\r', '\n'], " ");
    match place {
        Place::File => clean_part(&text.replace(['/', '\\'], " "), options, space),
        Place::Folder => {
            let text = text.replace('\\', "/");
            let parts: Vec<&str> = text.split('/').collect();
            let last = parts.len() - 1;
            let mut kept: Vec<String> = Vec::new();
            for (index, part) in parts.iter().enumerate() {
                let cleaned = clean_part(part, options, space);
                if !cleaned.is_empty() {
                    kept.push(cleaned);
                } else if index == last && !kept.is_empty() {
                    // The trailing separator stays.
                    kept.push(String::new());
                }
            }
            kept.join("/")
        }
    }
}

fn clean_part(part: &str, options: &NameOptions, space: &SpaceSubstitution) -> String {
    let mut text = part.to_owned();
    if options.unicode_replacement {
        text = text.replace("...", "\u{2026}");
    }
    let mut text = strip_ends(&text).to_owned();
    if space.enabled {
        text = substitute_spaces(&text, &space.replacement);
    }
    if options.ascii_replacement {
        let folded = ascii_fold(&text);
        // A name with nothing ASCII in it (a title in another script) keeps its letters rather
        // than becoming empty.
        if !folded.trim().is_empty() || text.trim().is_empty() {
            text = folded;
        }
    }
    let text = replace_invalid_characters(
        &text,
        &options.colon_replacement,
        options.unicode_replacement,
    );
    collapse_spaces(&text)
}

/// Leading ` `, `.` and `-` and trailing ` `, `.`, `-` and `_` go.
fn strip_ends(text: &str) -> &str {
    text.trim_start_matches([' ', '.', '-'])
        .trim_end_matches([' ', '.', '-', '_'])
}

/// Replaces each run of spaces with `replacement`, and repeats of the replacement collapse to
/// one (`Dune - Part Two` with `-` is `Dune-Part-Two`, not `Dune---Part-Two`).
fn substitute_spaces(text: &str, replacement: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_run = false;
    for c in text.chars() {
        if c == ' ' {
            if !in_run {
                out.push_str(replacement);
            }
            in_run = true;
        } else {
            in_run = false;
            out.push(c);
        }
    }
    if replacement.is_empty() {
        return out;
    }
    let twice = format!("{replacement}{replacement}");
    while out.contains(&twice) {
        out = out.replace(&twice, replacement);
    }
    out
}

/// One space for each run of spaces, no space at the ends.
fn collapse_spaces(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_run = false;
    for c in text.chars() {
        if c == ' ' {
            if !in_run {
                out.push(' ');
            }
            in_run = true;
        } else {
            in_run = false;
            out.push(c);
        }
    }
    out.trim_matches(' ').to_owned()
}

/// Replaces the characters a file name cannot hold.
///
/// With `-` as the colon replacement a colon followed by a space becomes ` - ` (`Dune: Part Two`
/// is `Dune - Part Two`); any other colon becomes the replacement alone. `"`, `<`, `>`, `|`, `?`
/// and `*` are removed. With `unicode` the colon and those characters (except `|`) become
/// look-alikes instead. `/` and `\` are not touched: they are separators or already replaced.
pub fn replace_invalid_characters(text: &str, colon_replacement: &str, unicode: bool) -> String {
    if unicode {
        return text
            .chars()
            .filter_map(|c| match c {
                '|' => None,
                ':' | '"' | '<' | '>' | '?' | '*' => unicode_lookalike(c),
                other => Some(other),
            })
            .collect();
    }
    let mut text = text.to_owned();
    if colon_replacement.trim() == "-" {
        text = text.replace(": ", " - ");
    }
    text.replace(':', colon_replacement)
        .replace(['"', '<', '>', '|', '?', '*'], "")
}

/// Folds text to ASCII: letters with marks lose them, German umlauts become `ae`, `oe` and `ue`,
/// `ß` becomes `ss`, `…` becomes `...`, typographic quotes and dashes become plain ones, and what
/// has no ASCII form is dropped.
pub fn ascii_fold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_ascii() {
            out.push(c);
            continue;
        }
        let special = match c {
            '\u{c4}' => "Ae",
            '\u{e4}' => "ae",
            '\u{d6}' => "Oe",
            '\u{f6}' => "oe",
            '\u{dc}' => "Ue",
            '\u{fc}' => "ue",
            '\u{2026}' => "...",
            '\u{2033}' | '\u{201c}' | '\u{201d}' | '\u{201e}' => "''",
            '\u{2018}' | '\u{2019}' | '\u{201a}' | '\u{2032}' => "'",
            '\u{2013}' | '\u{2014}' | '\u{2212}' => "-",
            _ => "",
        };
        if !special.is_empty() {
            out.push_str(special);
            continue;
        }
        let folded = fold_to_ascii(c);
        if folded.is_ascii() {
            out.push_str(&folded);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(raw: &str) -> String {
        create_destination(
            raw,
            Place::Folder,
            &NameOptions::default(),
            &SpaceSubstitution::default(),
        )
    }

    fn file(raw: &str) -> String {
        create_destination(
            raw,
            Place::File,
            &NameOptions::default(),
            &SpaceSubstitution::default(),
        )
    }

    #[test]
    fn empty_brackets_go_even_when_nested() {
        assert_eq!(file("Title ( ) [ ] { }"), "Title");
        assert_eq!(file("A [( )] B"), "A B");
        assert_eq!(file("Title [1080p] [] ()"), "Title [1080p]");
        assert_eq!(file("Untitled Project {tmdb-}"), "Untitled Project {tmdb-}");
    }

    #[test]
    fn ends_are_stripped() {
        assert_eq!(file("...Title..."), "Title");
        assert_eq!(file(" - Title - "), "Title");
        assert_eq!(file("_Title_"), "_Title");
        assert_eq!(file("Title ... "), "Title");
        assert_eq!(file("Title - - 2020"), "Title - - 2020");
    }

    #[test]
    fn line_breaks_and_double_spaces_collapse() {
        assert_eq!(file("Title\n2020"), "Title 2020");
        assert_eq!(file("Title  2020"), "Title 2020");
        assert_eq!(file("Title\r\n2020"), "Title 2020");
        assert_eq!(file("   "), "");
        assert_eq!(file(""), "");
    }

    #[test]
    fn invalid_characters_in_literal_text_are_removed() {
        assert_eq!(file("\"Title\" <2020> | ? *"), "Title 2020");
        assert_eq!(file("Title: 2020"), "Title - 2020");
        assert_eq!(file("Title:2020"), "Title-2020");
        assert_eq!(file("Title : 2020"), "Title - 2020");
    }

    #[test]
    fn a_file_name_loses_its_separators() {
        assert_eq!(file("A/Title/B"), "A Title B");
        assert_eq!(file("A\\Title\\B"), "A Title B");
    }

    #[test]
    fn a_folder_keeps_its_parts() {
        assert_eq!(folder("A//B///C/Title//"), "A/B/C/Title/");
        assert_eq!(folder("/Title/"), "Title/");
        assert_eq!(folder("A / Title / B"), "A/Title/B");
        assert_eq!(folder("A . / Title"), "A/Title");
        assert_eq!(folder("A\\Title"), "A/Title");
        assert_eq!(folder(""), "");
        assert_eq!(folder("   "), "");
        assert_eq!(folder("//"), "");
        assert_eq!(folder("T/The Matrix (1999)"), "T/The Matrix (1999)");
    }

    #[test]
    fn space_substitution_collapses_runs() {
        let space = SpaceSubstitution::with("_");
        let name = NameOptions::default();
        let run = |raw: &str, place| create_destination(raw, place, &name, &space);
        assert_eq!(
            run("Dune - Part  Two (2024)", Place::File),
            "Dune_-_Part_Two_(2024)"
        );
        assert_eq!(run("D/Dune Part Two", Place::Folder), "D/Dune_Part_Two");
        // Repeats of the replacement collapse, the literal dash included.
        let dash = SpaceSubstitution::with("-");
        assert_eq!(
            create_destination("Dune - Part Two", Place::File, &name, &dash),
            "Dune-Part-Two"
        );
        let dots = SpaceSubstitution::with(".");
        assert_eq!(create_destination("A B", Place::File, &name, &dots), "A.B");
        let none = SpaceSubstitution::with("");
        assert_eq!(create_destination("A B", Place::File, &name, &none), "AB");
    }

    #[test]
    fn colon_replacements() {
        let space = SpaceSubstitution::default();
        let with = |colon: &str, raw: &str| {
            let name = NameOptions {
                colon_replacement: colon.to_owned(),
                ..NameOptions::default()
            };
            create_destination(raw, Place::File, &name, &space)
        };
        assert_eq!(with("-", "Dune: Part"), "Dune - Part");
        assert_eq!(with("", "Dune: Part"), "Dune Part");
        assert_eq!(with("_", "Dune: Part"), "Dune_ Part");
        assert_eq!(with(".", "Dune: Part"), "Dune. Part");
    }

    #[test]
    fn invalid_characters_function_follows_the_fixtures() {
        let r = |text: &str, colon: &str| replace_invalid_characters(text, colon, false);
        assert_eq!(r("jb: the bla", "-"), "jb - the bla");
        assert_eq!(r("jb: the bla", "_"), "jb_ the bla");
        assert_eq!(r("jb : the bla", "-"), "jb  - the bla");
        assert_eq!(r("2 :22", "_"), "2 _22");
        assert_eq!(r("a::b", "-"), "a--b");
        assert_eq!(r("trailing:", "-"), "trailing-");
        assert_eq!(r(":leading", "_"), "_leading");
        assert_eq!(r("tab\tchar", "-"), "tab\tchar");
        assert_eq!(r("weird \"\\\\:<>|/?* movie", "-"), "weird \\\\-/ movie");
    }

    #[test]
    fn unicode_mode_writes_look_alikes_and_keeps_ellipsis() {
        let name = NameOptions {
            unicode_replacement: true,
            ..NameOptions::default()
        };
        let space = SpaceSubstitution::default();
        assert_eq!(
            create_destination("Dune: Part Two... \"A\"?", Place::File, &name, &space),
            "Dune\u{2236} Part Two\u{2026} \u{2033}A\u{2033}\u{2753}"
        );
    }

    #[test]
    fn ascii_mode_folds_and_drops() {
        assert_eq!(
            ascii_fold("Am\u{e9}lie... \u{dc}n\u{ef}c\u{f6}d\u{e9} \u{d1}and\u{fa}"),
            "Amelie... Uenicoede Nandu"
        );
        assert_eq!(ascii_fold("Stra\u{df}e"), "Strasse");
        assert_eq!(ascii_fold("a\u{2033}b\u{2026}\u{2753}"), "a''b...");
        let name = NameOptions {
            ascii_replacement: true,
            ..NameOptions::default()
        };
        let space = SpaceSubstitution::default();
        assert_eq!(
            create_destination("Am\u{e9}lie (2001)", Place::File, &name, &space),
            "Amelie (2001)"
        );
        // A name with no ASCII letters keeps them.
        assert_eq!(
            create_destination("\u{6620}\u{753b}", Place::File, &name, &space),
            "\u{6620}\u{753b}"
        );
    }
}
