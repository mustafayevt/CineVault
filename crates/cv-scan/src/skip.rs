//! Skip rules: which folders a scan must not enter and which marker files hide a folder.
//!
//! A folder is skipped when any of these hold ([`SkipRules::is_skipped_folder`]):
//!
//! 1. its upper-cased name is on the built-in list of system and junk folders, which includes the
//!    `.deletedByCineVault` trash of Organize ([`HARD_SKIP_MOVIE`]
//!    or [`HARD_SKIP_TV`]);
//! 2. it is a hidden dot-folder: for movies a name that starts with `.` or `@` followed by a word
//!    character or `@` (the names `.45` and `.buelos...` are real titles and are kept), for TV
//!    shows the same with `.` only;
//! 3. a user pattern matches the whole folder name. Patterns are regular expressions; one that is
//!    not a valid expression is matched literally instead (see [`SkipRules::literal_patterns`]);
//! 4. a user pattern, with any `\Q` and `\E` quoting removed, is the folder's full path;
//! 5. the folder is another library folder ([`SkipRules::with_excluded_paths`]): a movie folder
//!    that holds a TV folder (or the other way round) leaves that folder to its own library.
//!
//! A folder that contains a marker file ([`MARKER_IGNORE`], and [`MARKER_NOMEDIA`] unless turned
//! off) is skipped together with everything below it ([`SkipRules::folder_has_marker`]). Markers
//! are matched without regard to case, because the supported volumes are case-insensitive.
//!
//! Everything but [`SkipRules::dir_has_marker`] works on text, so the rules are tested identically
//! on every operating system. User patterns use the Rust `regex` syntax, which has no look-around
//! or back-references; a pattern that needs them is treated as a literal name. User patterns are
//! case-sensitive unless they carry `(?i)`; the path comparison ignores case and separator style.

use std::path::Path;

use regex::Regex;

/// Marker file that hides its folder and everything below it.
pub const MARKER_IGNORE: &str = ".cinevaultignore";
/// Marker file used by many media tools; honoured unless the setting is off (it is on by default).
pub const MARKER_NOMEDIA: &str = ".nomedia";

/// The user skip list a new installation starts with.
pub const DEFAULT_USER_PATTERNS: [&str; 1] = ["MAKEMKV"];

/// System and junk folders no movie scan enters (compared in upper case).
pub const HARD_SKIP_MOVIE: &[&str] = &[
    ".",
    "..",
    "CERTIFICATE",
    "$RECYCLE.BIN",
    "RECYCLER",
    "SYSTEM VOLUME INFORMATION",
    "@EADIR",
    "ADV_OBJ",
    "PLEX VERSIONS",
    "LOST.DIR",
    ".DELETEDBYCINEVAULT",
];

/// System and junk folders no TV scan enters (compared in upper case).
pub const HARD_SKIP_TV: &[&str] = &[
    ".",
    "..",
    "CERTIFICATE",
    "$RECYCLE.BIN",
    "RECYCLER",
    "SYSTEM VOLUME INFORMATION",
    "@EADIR",
    "ADV_OBJ",
    "PLEX VERSIONS",
    "EXTRATHUMB",
    ".DELETEDBYCINEVAULT",
];

/// Which library a scan builds. The two differ slightly in the built-in rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryKind {
    Movies,
    TvShows,
}

#[derive(Debug, Clone)]
struct UserPattern {
    /// The text as the user entered it.
    text: String,
    /// Full-name matcher: the pattern as a regular expression, or the quoted text as a fallback.
    matcher: Regex,
    literal: bool,
}

/// The skip rules of one library kind with the user's settings applied.
#[derive(Debug, Clone)]
pub struct SkipRules {
    kind: LibraryKind,
    patterns: Vec<UserPattern>,
    skip_nomedia: bool,
    /// Full paths (in [`canonical_path`] form) of folders that belong to another data source.
    excluded: Vec<String>,
}

impl SkipRules {
    /// Builds the rules. Empty patterns are ignored. `skip_nomedia` turns the `.nomedia` marker on
    /// or off; `.cinevaultignore` is always honoured.
    pub fn new<S: AsRef<str>>(kind: LibraryKind, user_patterns: &[S], skip_nomedia: bool) -> Self {
        let patterns = user_patterns
            .iter()
            .map(|p| p.as_ref())
            .filter(|p| !p.trim().is_empty())
            .map(compile_pattern)
            .collect();
        Self {
            kind,
            patterns,
            skip_nomedia,
            excluded: Vec::new(),
        }
    }

    /// The same rules, also skipping these folders (given by their full path) with everything
    /// below them. A scan passes the other data sources that lie inside the one it walks, so
    /// their files are read by their own library only.
    #[must_use]
    pub fn with_excluded_paths<S: AsRef<str>>(mut self, paths: &[S]) -> Self {
        self.excluded = paths
            .iter()
            .map(|p| canonical_path(p.as_ref()))
            .filter(|p| !p.is_empty())
            .collect();
        self
    }

    /// The rules of a new installation: the default user list and `.nomedia` honoured.
    pub fn with_defaults(kind: LibraryKind) -> Self {
        Self::new(kind, &DEFAULT_USER_PATTERNS, true)
    }

    pub fn kind(&self) -> LibraryKind {
        self.kind
    }

    pub fn skips_nomedia(&self) -> bool {
        self.skip_nomedia
    }

    /// The user patterns that are not valid regular expressions and are matched literally, so the
    /// settings page can tell the user.
    pub fn literal_patterns(&self) -> Vec<&str> {
        self.patterns
            .iter()
            .filter(|p| p.literal)
            .map(|p| p.text.as_str())
            .collect()
    }

    /// Whether a scan must not enter the folder. `name` is the folder's own name and `path` its
    /// full path (the pattern-equals-path rule needs it); pass the data source's own name and
    /// path to test the data source itself.
    pub fn is_skipped_folder(&self, name: &str, path: &str) -> bool {
        self.is_builtin_skipped(name) || self.is_user_skipped(name, path) || self.is_excluded(path)
    }

    fn is_excluded(&self, path: &str) -> bool {
        !self.excluded.is_empty() && {
            let path = canonical_path(path);
            self.excluded.contains(&path)
        }
    }

    /// The built-in rules only: the hard list and hidden dot-folders.
    pub fn is_builtin_skipped(&self, name: &str) -> bool {
        let upper = name.to_uppercase();
        self.hard_list().contains(&upper.as_str()) || is_hidden_folder(self.kind, name)
    }

    fn is_user_skipped(&self, name: &str, path: &str) -> bool {
        let path = canonical_path(path);
        self.patterns.iter().any(|p| {
            p.matcher.is_match(name)
                || (!path.is_empty() && canonical_path(&strip_quotes(&p.text)) == path)
        })
    }

    fn hard_list(&self) -> &'static [&'static str] {
        match self.kind {
            LibraryKind::Movies => HARD_SKIP_MOVIE,
            LibraryKind::TvShows => HARD_SKIP_TV,
        }
    }

    /// Whether `file_name` is a marker that hides the folder it sits in.
    pub fn is_marker_file(&self, file_name: &str) -> bool {
        file_name.eq_ignore_ascii_case(MARKER_IGNORE)
            || (self.skip_nomedia && file_name.eq_ignore_ascii_case(MARKER_NOMEDIA))
    }

    /// Whether a folder with these file names is hidden by a marker. The folder and everything
    /// below it is then left out of the scan.
    pub fn folder_has_marker<I, S>(&self, file_names: I) -> bool
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        file_names
            .into_iter()
            .any(|n| self.is_marker_file(n.as_ref()))
    }

    /// [`folder_has_marker`](Self::folder_has_marker) for a folder on disk. An unreadable folder
    /// has no marker; the scan reports it separately.
    pub fn dir_has_marker(&self, dir: &Path) -> bool {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return false;
        };
        entries
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| !t.is_dir()))
            .any(|e| {
                e.file_name()
                    .to_str()
                    .is_some_and(|n| self.is_marker_file(n))
            })
    }
}

/// Hidden folders: a leading `.` (movies also `@`) then a word character or `@`, except the
/// movie titles `.45` and `.buelos...`. Word characters are ASCII letters, digits and `_`.
fn is_hidden_folder(kind: LibraryKind, name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    let rest = chars.as_str();
    match kind {
        LibraryKind::Movies => {
            if first != '.' && first != '@' {
                return false;
            }
            let lower = rest.to_lowercase();
            if lower.starts_with("45") || lower.starts_with("buelos") {
                return false;
            }
        }
        LibraryKind::TvShows => {
            if first != '.' {
                return false;
            }
        }
    }
    rest.chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '@')
}

fn compile_pattern(text: &str) -> UserPattern {
    let anchored = |body: &str| format!("^(?:{body})$");
    match Regex::new(&anchored(text)) {
        Ok(matcher) => UserPattern {
            text: text.to_owned(),
            matcher,
            literal: false,
        },
        Err(_) => UserPattern {
            text: text.to_owned(),
            // Escaped text is always a valid expression.
            matcher: Regex::new(&anchored(&regex::escape(text)))
                .expect("an escaped literal is a valid regular expression"),
            literal: true,
        },
    }
}

fn strip_quotes(text: &str) -> String {
    text.replace("\\Q", "").replace("\\E", "")
}

/// Path text for comparison: one separator, no trailing separator, case folded (the supported
/// volumes are case-insensitive).
fn canonical_path(path: &str) -> String {
    path.trim()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_lowercase()
}
