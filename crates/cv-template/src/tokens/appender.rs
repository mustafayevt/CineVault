//! The renamer engine: zero and empty values print nothing, tokens are safe for file names.

use std::borrow::Cow;
use std::path::PathBuf;

use super::aliases::AliasMap;
use crate::eval::{Engine, Error, Limits, OutputAppender};
use crate::parse::{ParseOptions, Template};
use crate::renamer_renderers::{RenamerRenderers, register_renamer};
use crate::value::Value;

/// Settings of the renamer engine.
#[derive(Debug, Clone)]
pub struct RenamerOptions {
    /// What a colon in a token becomes. With `-` or ` -` a colon followed by a space becomes
    /// ` - `, so `Dune: Part Two` is `Dune - Part Two`; any other replacement (`_`, `.`, or
    /// nothing) replaces the colon alone.
    pub colon_replacement: String,
    /// What `first` prints for a title that starts with a digit (`#` by default).
    pub number_replacement: String,
    /// The app's data folder, where `replace(file.csv)` finds its tables.
    pub data_dir: Option<PathBuf>,
    /// Write the characters a file name cannot hold as look-alikes instead of dropping them
    /// (`/` becomes `⁄`, `:` becomes `∶`, `?` becomes `❓`); see [`unicode_lookalike`].
    pub unicode_replacement: bool,
}

impl Default for RenamerOptions {
    fn default() -> Self {
        RenamerOptions {
            colon_replacement: "-".to_owned(),
            number_replacement: "#".to_owned(),
            data_dir: None,
            unicode_replacement: false,
        }
    }
}

/// The Unicode look-alike of a character a file name cannot hold, for the "Unicode replacement"
/// option. `|` has no look-alike; it is dropped as in the plain mode.
pub fn unicode_lookalike(c: char) -> Option<char> {
    Some(match c {
        '/' => '\u{2044}',
        '\\' => '\u{2216}',
        ':' => '\u{2236}',
        '"' => '\u{2033}',
        '<' => '\u{2039}',
        '>' => '\u{203A}',
        '?' => '\u{2753}',
        '*' => '\u{26B9}',
        _ => return None,
    })
}

/// Makes every token safe to use in a file name: `/` and `\` become spaces (except in the
/// `parent` token, which is a folder path), the colon is replaced, and `"`, `<`, `>`, `|`, `?`
/// and `*` are removed. Literal text between tokens is not touched.
///
/// With the Unicode option the characters are written as look-alikes instead (separators and
/// colon included), see [`unicode_lookalike`].
#[derive(Debug, Clone)]
pub struct NameTokenAppender {
    colon_replacement: String,
    unicode: bool,
}

impl NameTokenAppender {
    pub fn new(colon_replacement: impl Into<String>) -> Self {
        NameTokenAppender {
            colon_replacement: colon_replacement.into(),
            unicode: false,
        }
    }

    /// Writes look-alikes instead of dropping the characters a file name cannot hold.
    #[must_use]
    pub fn with_unicode(mut self, on: bool) -> Self {
        self.unicode = on;
        self
    }

    fn colon_pairs_with_space(&self) -> bool {
        self.colon_replacement.trim() == "-"
    }
}

impl OutputAppender for NameTokenAppender {
    fn append<'a>(&self, value: &'a str, token: &str) -> Cow<'a, str> {
        const INVALID: [char; 6] = ['"', '<', '>', '|', '?', '*'];
        let is_path = token.contains("parent");
        let needs_work = value.contains([':', '/', '\\', '"', '<', '>', '|', '?', '*']);
        if !needs_work {
            return Cow::Borrowed(value);
        }
        if self.unicode {
            return Cow::Owned(
                value
                    .chars()
                    .filter_map(|c| match c {
                        '/' | '\\' if is_path => Some(c),
                        '|' => None,
                        _ => Some(unicode_lookalike(c).unwrap_or(c)),
                    })
                    .collect(),
            );
        }
        let mut text = if is_path {
            value.to_owned()
        } else {
            value.replace(['/', '\\'], " ")
        };
        if self.colon_pairs_with_space() {
            text = text.replace(": ", " - ");
        }
        Cow::Owned(
            text.replace(':', &self.colon_replacement)
                .replace(INVALID, ""),
        )
    }
}

/// An engine with the renamer's rules: numeric zero and empty values print nothing, tokens go
/// through [`NameTokenAppender`], and the renamer renderers (`first` with the number
/// replacement, `replace` with CSV tables, the movie-set index) are registered on top of the
/// standard ones.
pub fn renamer_engine(options: &RenamerOptions) -> Engine {
    let mut engine = Engine::standard()
        .zero_is_empty(true)
        .with_limits(Limits::NAMING)
        .with_appender(
            NameTokenAppender::new(options.colon_replacement.clone())
                .with_unicode(options.unicode_replacement),
        );
    register_renamer(
        &mut engine,
        &RenamerRenderers {
            number_replacement: options.number_replacement.clone(),
            data_dir: options.data_dir.clone(),
        },
    );
    engine
}

/// Renders a naming template: rewrites the aliases to model paths, parses it the way renamer
/// templates are parsed (a typed backslash stays a backslash) and renders it against `model`.
///
/// The result is the raw name. Clean-up (empty brackets, doubled separators, invalid
/// characters in literal text) belongs to the renamer.
pub fn render_name(
    template: &str,
    aliases: &AliasMap,
    model: &Value,
    options: &RenamerOptions,
) -> Result<String, Error> {
    render_name_with(&renamer_engine(options), template, aliases, model)
}

/// [`render_name`] with an engine that is built once and reused.
pub fn render_name_with(
    engine: &Engine,
    template: &str,
    aliases: &AliasMap,
    model: &Value,
) -> Result<String, Error> {
    let morphed = aliases.morph(template);
    let parsed = Template::parse_with(
        &morphed,
        ParseOptions {
            backslash_escapes: false,
        },
    )?;
    Ok(engine.render(&parsed, model)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn append(colon: &str, value: &str, token: &str) -> String {
        NameTokenAppender::new(colon)
            .append(value, token)
            .into_owned()
    }

    #[test]
    fn colon_replacements() {
        let cases = [
            ("", "Dune Part Two"),
            ("-", "Dune - Part Two"),
            (" -", "Dune - Part Two"),
            ("_", "Dune_ Part Two"),
            (".", "Dune. Part Two"),
        ];
        for (replacement, expected) in cases {
            assert_eq!(
                append(replacement, "Dune: Part Two", "movie.title"),
                expected
            );
        }
        assert_eq!(append("-", "a:b", "movie.title"), "a-b");
        assert_eq!(append(" -", "a:b", "movie.title"), "a -b");
    }

    #[test]
    fn invalid_characters_and_separators() {
        assert_eq!(append("-", "\"A\" <B> | C? *", "movie.title"), "A B  C ");
        assert_eq!(append("-", "AC/DC\\Live", "movie.title"), "AC DC Live");
        // The parent token is a folder path and keeps its separators.
        assert_eq!(append("-", "A/B", "movie.parent"), "A/B");
        assert_eq!(append("-", "A/B: C", "movie.parent"), "A/B - C");
        assert!(matches!(
            NameTokenAppender::new("-").append("plain", "movie.title"),
            Cow::Borrowed(_)
        ));
    }

    #[test]
    fn unicode_mode_writes_look_alikes() {
        let unicode = NameTokenAppender::new("-").with_unicode(true);
        let text = |v: &str, t: &str| unicode.append(v, t).into_owned();
        assert_eq!(
            text("Dune: Part \"Two\"? <A> | *", "movie.title"),
            "Dune\u{2236} Part \u{2033}Two\u{2033}\u{2753} \u{2039}A\u{203A}  \u{26B9}"
        );
        assert_eq!(
            text("AC/DC\\Live", "movie.title"),
            "AC\u{2044}DC\u{2216}Live"
        );
        // The parent token keeps its separators.
        assert_eq!(text("A/B: C", "movie.parent"), "A/B\u{2236} C");
        assert!(matches!(
            unicode.append("plain", "movie.title"),
            Cow::Borrowed(_)
        ));
    }

    #[test]
    fn zero_and_empty_values_print_nothing() {
        use cv_domain::Item;

        use super::super::{ModelOptions, movie_aliases, movie_model};

        let options = RenamerOptions::default();
        let model = movie_model(&Item::movie("Title"), None, &ModelOptions::default());
        let aliases = movie_aliases();
        let render = |t: &str| render_name(t, &aliases, &model, &options).unwrap();
        // Year 0 prints nothing, so the renamer's clean-up can remove the empty brackets.
        assert_eq!(render("${title} (${year})"), "Title ()");
        assert_eq!(render("${title}${ (,year,)}"), "Title");
        assert_eq!(render("${title}${ - ,edition,}"), "Title");
        assert_eq!(
            render("[${tmdb}] [${hdrformat}] [${videoFormat}]"),
            "[] [] []"
        );
        // Empty lists and missing media info print nothing either.
        assert_eq!(
            render("${genres}${actors}${audioCodecsAsString}${videoBitRate}"),
            ""
        );
        assert_eq!(render("${rating}"), "");
        // Zero is still true in a condition.
        assert_eq!(render("${if year}yes${else}no${end}"), "yes");
        assert_eq!(render("${if edition}yes${else}no${end}"), "no");
    }

    #[test]
    fn typed_backslashes_stay() {
        use cv_domain::Item;

        use super::super::{ModelOptions, movie_aliases, movie_model};

        let model = movie_model(&Item::movie("Heat"), None, &ModelOptions::default());
        let name = render_name(
            r"${title}\${title}",
            &movie_aliases(),
            &model,
            &RenamerOptions::default(),
        )
        .unwrap();
        assert_eq!(name, r"Heat\Heat");
    }
}
