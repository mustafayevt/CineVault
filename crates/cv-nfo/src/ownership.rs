//! Who owns an NFO file: which files CineVault may overwrite and which it may clean up.
//!
//! An NFO is **managed** when it parses with a non-empty title (a movie, set, show or episode
//! title, or a season title). Everything else in a folder is foreign: release-group notes such as
//! `info.nfo`, tool output, files that are not XML. The rules that protect foreign files:
//!
//! * Cleanup removes only managed NFOs, and only those the item could have written itself (its own
//!   naming choices, see [`stale_nfos`]), so another movie's `Other.nfo` in a shared folder or a
//!   release note is never a candidate.
//! * Writing refuses a target that exists and is foreign ([`NfoOwnership::may_overwrite`]).
//!
//! Two kinds of existing file are overwritten although they have no title: an empty file (there is
//! nothing to lose) and a **pointer** file, whose whole content is a URL or an ID such as
//! `https://www.themoviedb.org/movie/949` (the reader takes the ID from it, see
//! [`crate::read`]; the ID is already in the library when the file is first written over, and the
//! change is journaled, so it can be undone). A pointer file is never removed by cleanup.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use crate::read::decode_nfo_bytes;
use crate::read_movie::{read_movie_nfo, read_movie_set_nfo};
use crate::read_tv::{read_episode_nfo, read_season_nfo, read_show_nfo};

/// What an NFO path holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NfoOwnership {
    /// There is no file.
    Missing,
    /// A file with no content (or only white space).
    Empty,
    /// Parses with a non-empty title: CineVault's to rewrite and to clean up.
    Managed,
    /// Only a URL or an ID.
    Pointer,
    /// Anything else. Never overwritten, never deleted.
    Foreign,
}

impl NfoOwnership {
    /// Whether a writer may put its NFO at this path.
    pub fn may_overwrite(self) -> bool {
        !matches!(self, Self::Foreign)
    }

    /// Whether cleanup may remove the file.
    pub fn is_managed(self) -> bool {
        self == Self::Managed
    }
}

static POINTER_TOKEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?:https?://\S+|tt\d{5,}|(?:imdb|tmdb|tvdb|themoviedb|thetvdb)[:=]\S+)$")
        .expect("static pointer pattern")
});

/// Classifies the text of an existing NFO (see the module docs).
pub fn classify_nfo_text(text: &str) -> NfoOwnership {
    let text = text.trim();
    if text.is_empty() {
        return NfoOwnership::Empty;
    }
    if has_title(text) {
        return NfoOwnership::Managed;
    }
    if text.split_whitespace().all(|t| POINTER_TOKEN.is_match(t)) {
        return NfoOwnership::Pointer;
    }
    NfoOwnership::Foreign
}

/// Classifies the bytes of an existing NFO.
pub fn classify_nfo_bytes(bytes: &[u8]) -> NfoOwnership {
    classify_nfo_text(&decode_nfo_bytes(bytes))
}

/// Classifies the file at `path`; [`NfoOwnership::Missing`] when there is none.
///
/// # Errors
/// Fails when the file exists but cannot be read.
pub fn classify_nfo_file(path: &Path) -> io::Result<NfoOwnership> {
    match cv_fs::retry("reading an NFO", path, || fs::read(path)) {
        Ok(bytes) => Ok(classify_nfo_bytes(&bytes)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(NfoOwnership::Missing),
        Err(e) => Err(e),
    }
}

/// True when some reader finds a non-empty title in the text.
fn has_title(text: &str) -> bool {
    read_movie_nfo(text).is_some_and(|m| m.is_valid())
        || read_movie_set_nfo(text).is_some_and(|s| s.is_valid())
        || read_show_nfo(text).is_some_and(|s| s.is_valid())
        || read_season_nfo(text).is_some_and(|s| !s.season.title.trim().is_empty())
        || read_episode_nfo(text)
            .iter()
            .any(|e| !e.episode.title.trim().is_empty())
}

/// True when the text parses with a non-empty title.
pub fn is_managed_nfo_text(text: &str) -> bool {
    classify_nfo_text(text).is_managed()
}

fn same_path(a: &Path, b: &Path) -> bool {
    a.to_string_lossy()
        .eq_ignore_ascii_case(&b.to_string_lossy())
}

/// The managed NFOs that an item no longer wants.
///
/// `candidates` are the paths the item could have written under any naming choice (including
/// names from before a rename); `keep` are the paths of the configured naming set. A file is
/// stale when it is a candidate, is not in `keep` and is managed. Paths compare without regard to
/// case, because Windows and macOS volumes do. A candidate that does not exist, or is foreign, an
/// empty file or a pointer, is left alone, and so is every file that is not a candidate.
pub fn stale_nfos(candidates: &[PathBuf], keep: &[PathBuf]) -> Vec<PathBuf> {
    let mut stale: Vec<PathBuf> = Vec::new();
    for path in candidates {
        if keep.iter().any(|k| same_path(k, path)) || stale.iter().any(|s| same_path(s, path)) {
            continue;
        }
        if classify_nfo_file(path).is_ok_and(NfoOwnership::is_managed) {
            stale.push(path.clone());
        }
    }
    stale
}

/// Whether CineVault wrote the NFO `text`: its header comment, the first comment before the root
/// element, reads `created ... by CineVault ...`. A file another tool or a person wrote has no
/// such comment, so a rewrite that replaces it keeps the original (see the app's NFO service).
pub fn written_by_cinevault(text: &str) -> bool {
    let mut rest = text.trim_start_matches('\u{feff}').trim_start();
    if let Some(after) = rest.strip_prefix("<?")
        && let Some(end) = after.find("?>")
    {
        rest = after[end + 2..].trim_start();
    }
    let Some(comment) = rest.strip_prefix("<!--") else {
        return false;
    };
    let comment = comment.find("-->").map_or(comment, |end| &comment[..end]);
    comment.trim_start().starts_with("created") && comment.contains(" by CineVault")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_header_comment_of_cinevault_marks_a_file_as_written_by_it() {
        let ours = "\u{feff}<?xml version=\"1.0\"?>\r\n<!--created on 2026-10-23 - by CineVault \
                    0.1.0 for Kodi-->\r\n<movie><title>Heat</title></movie>";
        assert!(written_by_cinevault(ours));
        assert!(written_by_cinevault(
            "<!--created by CineVault 0.1.0 for Jellyfin--><movie/>"
        ));
        for other in [
            "<?xml version=\"1.0\"?>\n<movie><title>Heat</title></movie>",
            "<?xml version=\"1.0\"?>\n<!--created on 2020 by another tool--><movie/>",
            "<movie><!--created by CineVault--><title>Heat</title></movie>",
            "",
        ] {
            assert!(!written_by_cinevault(other), "{other}");
        }
    }

    const RELEASE_NOTE: &str = "\
 ______  _______  _       _______  _______  _______  _______
Release.Group.Presents
Movie.Name.2020.1080p.BluRay.x264-GROUP
IMDb: https://www.imdb.com/title/tt1234567/
Title.......: Movie Name
";

    #[test]
    fn a_titled_nfo_is_managed() {
        for text in [
            "<movie><title>Heat</title></movie>",
            "<?xml version=\"1.0\"?>\r\n<tvshow><title>Severance</title></tvshow>",
            "<collection><title>Alien Collection</title></collection>",
            "<season><seasonnumber>1</seasonnumber><title>Season 1</title></season>",
            "<episodedetails><title>Pilot</title><season>1</season><episode>1</episode></episodedetails>",
        ] {
            assert_eq!(classify_nfo_text(text), NfoOwnership::Managed, "{text}");
        }
    }

    #[test]
    fn notes_and_untitled_xml_are_foreign() {
        assert_eq!(classify_nfo_text(RELEASE_NOTE), NfoOwnership::Foreign);
        assert_eq!(
            classify_nfo_text("<movie><title>  </title><year>1995</year></movie>"),
            NfoOwnership::Foreign
        );
        assert_eq!(
            classify_nfo_text("<movie><imdbid>tt0113277</imdbid></movie>"),
            NfoOwnership::Foreign
        );
        assert!(!NfoOwnership::Foreign.may_overwrite());
    }

    #[test]
    fn empty_and_pointer_files_may_be_overwritten_but_are_never_cleaned_up() {
        assert_eq!(classify_nfo_text(" \r\n "), NfoOwnership::Empty);
        for text in [
            "https://www.themoviedb.org/movie/949",
            "tt0113277",
            "https://www.imdb.com/title/tt0113277/\r\nhttps://www.themoviedb.org/movie/949\r\n",
        ] {
            let ownership = classify_nfo_text(text);
            assert_eq!(ownership, NfoOwnership::Pointer, "{text}");
            assert!(ownership.may_overwrite());
            assert!(!ownership.is_managed());
        }
        assert!(NfoOwnership::Empty.may_overwrite());
        assert!(NfoOwnership::Missing.may_overwrite());
    }

    #[test]
    fn a_link_inside_prose_is_not_a_pointer() {
        assert_eq!(
            classify_nfo_text("Get it at https://www.imdb.com/title/tt0113277/"),
            NfoOwnership::Foreign
        );
    }

    #[test]
    fn bytes_with_a_byte_order_mark_and_old_encodings_classify() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice("<movie><title>Amélie</title></movie>".as_bytes());
        assert_eq!(classify_nfo_bytes(&bytes), NfoOwnership::Managed);
        let latin1 = b"<movie><title>Am\xe9lie</title></movie>";
        assert_eq!(classify_nfo_bytes(latin1), NfoOwnership::Managed);
    }

    #[test]
    fn stale_files_are_managed_candidates_outside_the_naming_set() {
        let dir = tempfile::tempdir().unwrap();
        let path = |name: &str| dir.path().join(name);
        let put = |name: &str, text: &str| fs::write(path(name), text).unwrap();
        put("Heat (1995).nfo", "<movie><title>Heat</title></movie>");
        put("movie.nfo", "<movie><title>Heat</title></movie>");
        put("info.nfo", RELEASE_NOTE);
        put("Other (2001).nfo", "<movie><title>Other</title></movie>");
        put("Old Name.nfo", "https://www.themoviedb.org/movie/949");
        let candidates: Vec<PathBuf> = ["Heat (1995).nfo", "movie.nfo", "Old Name.nfo", "Gone.nfo"]
            .iter()
            .map(|n| path(n))
            .collect();

        let keep = vec![path("heat (1995).NFO")];
        assert_eq!(stale_nfos(&candidates, &keep), [path("movie.nfo")]);
        // Not candidates: the release note and another movie's file are never listed, whatever
        // the naming set is.
        assert!(stale_nfos(&candidates, &candidates).is_empty());
        assert_eq!(
            stale_nfos(&candidates, &[]),
            [path("Heat (1995).nfo"), path("movie.nfo")]
        );
    }
}
