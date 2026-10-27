//! Checks on naming patterns that decide how the planner treats them.
//!
//! Movie folders and files:
//!
//! - a folder pattern is **unique** per movie when it holds the title and the year, or an id; a
//!   pattern that is not unique puts the movie into a shared folder (see
//!   [`crate::movie::MoviePlan::shared_folder`]);
//! - a file pattern is **valid** when it holds the title, the original file name, or an id; with
//!   any other pattern the files keep their names.
//!
//! The checks read the text of the pattern, so they work before it is rendered.

use std::sync::LazyLock;

use regex::Regex;

static TITLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\$\{.*?(title|originalTitle|englishTitle).*?\}").expect("valid regex")
});
static YEAR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\$\{.*?(year|imdb|tmdb).*?\}").expect("valid regex"));
static ID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\$\{.*?(imdb|tmdb).*?\}").expect("valid regex"));
static ORIGINAL_FILENAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\$\{.*?originalFilename.*?\}").expect("valid regex"));

/// Whether a movie folder pattern names every movie differently: title and year, or an id.
pub fn is_folder_pattern_unique(pattern: &str) -> bool {
    (TITLE.is_match(pattern) && YEAR.is_match(pattern)) || ID.is_match(pattern)
}

/// Whether a movie file pattern can name a file: it has the title, the original file name or an
/// id.
pub fn is_file_pattern_valid(pattern: &str) -> bool {
    TITLE.is_match(pattern) || ORIGINAL_FILENAME.is_match(pattern) || ID.is_match(pattern)
}

/// Whether the pattern uses the original file name, which already carries its extension.
pub fn uses_original_filename(pattern: &str) -> bool {
    ORIGINAL_FILENAME.is_match(pattern)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_patterns() {
        let cases = [
            ("${title} (${year})", true),
            ("${title}", false),
            ("${originalTitle} ${year}", true),
            ("${title} {tmdb-${tmdb}}", true),
            ("${imdb}", true),
            ("${year}", false),
            ("${edition}", false),
            ("${title} ${imdb}", true),
            ("${englishTitle} (${year})", true),
            ("${title;first}/${title}", false),
            ("${titleSortable} (${year})", true),
            ("Movies", false),
        ];
        for (pattern, want) in cases {
            assert_eq!(is_folder_pattern_unique(pattern), want, "{pattern}");
        }
    }

    #[test]
    fn file_patterns() {
        let cases = [
            ("${title}", true),
            ("${originalFilename}", true),
            ("${imdb}", true),
            ("${videoFormat}", false),
            ("${year}", false),
            ("static", false),
            ("${originalBasename}", false),
            ("${title} ${videoFormat}", true),
            ("${tmdb}", true),
            ("${englishTitle}", true),
            ("${originalTitle}", true),
        ];
        for (pattern, want) in cases {
            assert_eq!(is_file_pattern_valid(pattern), want, "{pattern}");
        }
    }
}
