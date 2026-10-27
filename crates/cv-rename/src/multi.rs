//! Finding the season and episode part of a TV file pattern, for files that hold several
//! episodes.
//!
//! A file with episodes 2 and 3 of season 1 is named from a pattern such as
//! `${showTitle} - S${seasonNr2}E${episodeNr2} - ${title}`. The **fragment** `S01E02` is repeated
//! for each episode (`S01E02 S01E03`) or shortened to a range (`S01E02-E03`), while the rest of
//! the pattern is rendered once.
//!
//! The fragment is the episode token with the text that introduces it (`E`, `x`, `Episode `,
//! `Folge `), and the season token in front of it with its own introduction (`S`, `Season `,
//! `Staffel `) when the two sit next to each other. A pattern that has a season token somewhere
//! else (`S${seasonNr2}.E${episodeNr2}`) has no fragment: it is rendered once, for the first
//! episode. The functions work on the *morphed* pattern, in which aliases
//! are written as model paths (`${episode.episode;number(%02d)}`).

use std::ops::Range;
use std::sync::LazyLock;

use regex::Regex;

static EPISODE_TOKEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\$\{\s*episode\.(?:episode|airedEpisode|dvdEpisode)\b[^}]*\}")
        .expect("valid regex")
});
static SEASON_TOKEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\$\{\s*episode\.(?:season|airedSeason|dvdSeason)\b[^}]*\}")
        .expect("valid regex")
});
/// The text that introduces the episode token, at the end of the text before it.
static EPISODE_DELIMITER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(?:\s?(?:folge|episode|[epx]+)\s?)?$").expect("valid regex"));
/// A season token with its introduction, at the end of the text before the episode delimiter.
static SEASON_PART: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:(?:staffel|season|s)\s?)?\$\{\s*episode\.(?:season|airedSeason|dvdSeason)\b[^}]*\}$")
        .expect("valid regex")
});
static ORIGINAL_NAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\$\{[^}]*episode\.original(?:Filename|Basename)\b[^}]*\}").expect("valid regex")
});

/// The part of a pattern that is repeated for each episode of a multi-episode file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fragment {
    /// The fragment in the morphed pattern.
    pub range: Range<usize>,
    /// The text that introduces the episode token (`E`, `x`, ` Episode `), possibly empty.
    pub delimiter: String,
    /// The episode token (`${episode.episode;number(%02d)}`).
    pub token: String,
}

impl Fragment {
    /// The fragment's pattern text.
    pub fn text<'a>(&self, pattern: &'a str) -> &'a str {
        &pattern[self.range.clone()]
    }

    /// What follows the first episode in a range: `-E03` for `S01E02`, `-03` for `1x02`, `-03`
    /// for a bare `${episodeNr2}` and `-Episode 3` for `Season 1 Episode 2`.
    pub fn range_tail(&self) -> String {
        let delimiter = self.delimiter.trim_start();
        let delimiter = if delimiter.trim().eq_ignore_ascii_case("x") {
            ""
        } else {
            delimiter
        };
        format!("{delimiter}{}", self.token)
    }
}

/// Finds the fragment of a morphed pattern, or `None` when there is none to repeat.
pub fn find_fragment(morphed: &str) -> Option<Fragment> {
    let episode = EPISODE_TOKEN.find(morphed)?;
    let before = &morphed[..episode.start()];
    let delimiter = EPISODE_DELIMITER.find(before)?;
    let before_delimiter = &morphed[..delimiter.start()];
    let start = match SEASON_PART.find(before_delimiter) {
        Some(season) => season.start(),
        // A season token that is not next to the episode: the pattern is not repeated.
        None if SEASON_TOKEN.is_match(morphed) => return None,
        None => delimiter.start(),
    };
    Some(Fragment {
        range: start..episode.end(),
        delimiter: delimiter.as_str().to_owned(),
        token: episode.as_str().to_owned(),
    })
}

/// The number of episode tokens in a morphed pattern.
pub fn episode_token_count(morphed: &str) -> usize {
    EPISODE_TOKEN.find_iter(morphed).count()
}

/// Whether the pattern keeps the file's own name (`${originalFilename}`, `${originalBasename}`).
pub fn keeps_original_name(morphed: &str) -> bool {
    ORIGINAL_NAME.is_match(morphed)
}

#[cfg(test)]
mod tests {
    use cv_template::tokens::tv_aliases;

    use super::*;

    fn fragment(pattern: &str) -> Option<(String, String)> {
        let morphed = tv_aliases().morph(pattern);
        find_fragment(&morphed).map(|f| (f.text(&morphed).to_owned(), f.range_tail()))
    }

    #[test]
    fn season_and_episode_next_to_each_other() {
        let (text, tail) =
            fragment("${showTitle} - S${seasonNr2}E${episodeNr2} - ${title}").unwrap();
        assert_eq!(
            text,
            "S${episode.season;number(%02d)}E${episode.episode;number(%02d)}"
        );
        assert_eq!(tail, "E${episode.episode;number(%02d)}");
    }

    #[test]
    fn x_delimiter_is_dropped_in_a_range() {
        let (text, tail) = fragment("${showTitle} ${seasonNr2}x${episodeNr2}").unwrap();
        assert_eq!(
            text,
            "${episode.season;number(%02d)}x${episode.episode;number(%02d)}"
        );
        assert_eq!(tail, "${episode.episode;number(%02d)}");
    }

    #[test]
    fn words_stay_in_a_range() {
        let (text, tail) =
            fragment("${showTitle} - Season ${seasonNr} Episode ${episodeNr}").unwrap();
        assert_eq!(
            text,
            "Season ${episode.season;number(%d)} Episode ${episode.episode}"
        );
        assert_eq!(tail, "Episode ${episode.episode}");
    }

    #[test]
    fn a_bare_episode_token_is_a_fragment() {
        let (text, tail) = fragment("${showTitle} - ${episodeNr2} - ${title}").unwrap();
        assert_eq!(text, "${episode.episode;number(%02d)}");
        assert_eq!(tail, text);
    }

    #[test]
    fn a_season_token_elsewhere_means_no_fragment() {
        assert_eq!(
            fragment("${showTitle} - S${seasonNr2}.E${episodeNr2}"),
            None
        );
        assert_eq!(
            fragment("${showTitle} - S${seasonNr2}-E${episodeNr2}"),
            None
        );
        assert_eq!(fragment("E${episodeNr2}S${seasonNr2}"), None);
    }

    #[test]
    fn no_episode_token_means_no_fragment() {
        assert_eq!(fragment("${showTitle} ${title}"), None);
    }

    #[test]
    fn counts_and_original_names() {
        let aliases = tv_aliases();
        assert_eq!(
            episode_token_count(&aliases.morph("S${seasonNr2}E${episodeNr2}E${episodeNr}")),
            2
        );
        assert!(keeps_original_name(&aliases.morph("${originalFilename}")));
        assert!(keeps_original_name(&aliases.morph("${originalBasename}")));
        assert!(!keeps_original_name(&aliases.morph("${title}")));
    }
}
