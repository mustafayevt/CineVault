//! Stacking parser: finds the part marker of multi-part movies and episodes
//!.
//!
//! Parts of one movie are stored as several files (`Movie.cd1.avi`, `Movie.cd2.avi`) or as
//! several folders (`Movie/CD1`, `Movie/CD2`). Two functions cover the two cases:
//!
//! - [`parse_file_name`] reads a full file name, extension included. It finds markers such as
//!   `cd1`, `dvd2`, `part3`, `pt4`, `disc1`, `disk2`, the letter forms `cda` and `-a` to `-d`, and
//!   `1of2` (also `1 of 2` and `1.of.2`). The result holds the marker, its part number and prefix,
//!   and the name with the marker removed.
//! - [`parse_folder_name`] reads a folder name (or a file name without extension) and finds a
//!   marker at its very end, such as `Movie CD1`.
//!
//! Both work on text only, so they behave the same on every operating system. The byte-exact
//! fixtures are the `stacking` family in `tests/compat`, run through `cv-compat`.

use std::sync::LazyLock;

use regex::Regex;

/// Markers found in a file name. All text fields are empty and the number is zero when the name
/// has no marker; `cleaned` is then the input unchanged.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileStacking {
    /// The marker as written, for example `cd1`, `b` or `1 of 2`. Empty when there is none.
    pub marker: String,
    /// The part number. `cd2` is 2, `-c` is 3, `2of3` is 2. Zero when the marker has no number
    /// (for example `cda`) or there is no marker.
    pub number: u32,
    /// The marker without its digits, for example `cd` for `cd1`. Empty for a single letter and
    /// for the `XofY` form.
    pub prefix: String,
    /// The name with the marker and one delimiter run before it removed, for example `Movie.avi`.
    pub cleaned: String,
}

impl FileStacking {
    /// True when a marker was found.
    pub fn is_stacked(&self) -> bool {
        !self.marker.is_empty()
    }
}

/// A marker at the end of a folder name. `marker` is empty when there is none, and `cleaned` is
/// then the input unchanged.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FolderStacking {
    /// The marker as written, for example `CD1`. Empty when there is none.
    pub marker: String,
    /// The name without the marker and the delimiters before it, for example `Movie`.
    pub cleaned: String,
}

impl FolderStacking {
    /// True when a marker was found.
    pub fn is_stacked(&self) -> bool {
        !self.marker.is_empty()
    }
}

struct Patterns {
    /// `name.cd1.avi`
    named: Regex,
    /// `cd1.iso` (marker at the start of the name)
    leading: Regex,
    /// `name.cda.avi`
    lettered: Regex,
    /// `name-a.avi`
    single_letter: Regex,
    /// `name 1of2.avi`
    x_of_y: Regex,
    /// The `XofY` marker on its own, to tell it from the other forms.
    x_of_y_marker: Regex,
    /// `Movie CD1` (folder name)
    folder: Regex,
}

static PATTERNS: LazyLock<Patterns> = LazyLock::new(|| {
    let re = |p: &str| Regex::new(p).expect("static stacking pattern");
    Patterns {
        named: re(r"(?i)^(.*)[ _.-]+((?:cd|dvd|p(?:ar)?t|dis[ck])[1-9][0-9]?)([ _.-].+)$"),
        leading: re(r"(?i)^((?:cd|dvd|p(?:ar)?t|dis[ck])[1-9][0-9]?)([ _.-].+)$"),
        lettered: re(r"(?i)^(.*)[ _.-]+((?:cd|dvd|p(?:ar)?t|dis[ck])[a-d])([ _.-].+)$"),
        single_letter: re(r"(?i)^(.*?)[_.-]+([a-d])(\.[^.]+)$"),
        x_of_y: re(
            r"(?i)^(.*?)[ (_.-]+([1-9][0-9]?[ .]?(?:of|⧸|∕|/)[ .]?[1-9][0-9]?)[ )_-]?([ _.-].+)$",
        ),
        x_of_y_marker: re(r"(?i)^([1-9][0-9]?)[ .]?(?:of|⧸|∕|/)[ .]?[1-9][0-9]?$"),
        folder: re(r"(?i)^(.*?)[ _.-]*((?:cd|dvd|p(?:ar)?t|dis[ck])[1-9][0-9]?)$"),
    }
});

/// Finds the stacking marker in a file name (extension included).
///
/// The forms are tried in this order, and the first that matches wins:
///
/// 1. `<name>[ _.-]<cd|dvd|part|pt|disc|disk><1-99><delimiter><rest>`, also with the marker at
///    the very start of the name;
/// 2. the same words followed by one letter `a` to `d`;
/// 3. `<name>[_.-]<a-d>.<ext>`, one letter before a single extension;
/// 4. `<name>[ (_.-]<X>of<Y>[ )_-]<delimiter><rest>`, with `of`, `⧸`, `∕` or `/`.
///
/// Matching ignores case. The marker is removed together with the one delimiter run before it:
/// `Movie (2010) - cd1.avi` becomes `Movie (2010) -.avi`.
pub fn parse_file_name(name: &str) -> FileStacking {
    let p = &*PATTERNS;
    let found = if let Some(c) = p.named.captures(name) {
        Some((c[1].to_owned(), c[2].to_owned(), c[3].to_owned()))
    } else if let Some(c) = p.leading.captures(name) {
        Some((String::new(), c[1].to_owned(), c[2].to_owned()))
    } else if let Some(c) = p.lettered.captures(name) {
        Some((c[1].to_owned(), c[2].to_owned(), c[3].to_owned()))
    } else if let Some(c) = p.single_letter.captures(name) {
        Some((c[1].to_owned(), c[2].to_owned(), c[3].to_owned()))
    } else {
        p.x_of_y
            .captures(name)
            .map(|c| (c[1].to_owned(), c[2].to_owned(), c[3].to_owned()))
    };
    match found {
        Some((before, marker, after)) => FileStacking {
            number: stacking_number(&marker),
            prefix: stacking_prefix(&marker),
            cleaned: format!("{before}{after}"),
            marker,
        },
        None => FileStacking {
            cleaned: name.to_owned(),
            ..FileStacking::default()
        },
    }
}

/// Finds the stacking marker at the end of a folder name, or of a file name without extension.
///
/// Only `cd`, `dvd`, `part`, `pt`, `disc` and `disk` followed by 1 to 99 count. The marker and the
/// delimiters before it are removed: `Movie.2010.part2` gives the marker `part2` and the name
/// `Movie.2010`.
pub fn parse_folder_name(name: &str) -> FolderStacking {
    match PATTERNS.folder.captures(name) {
        Some(c) => FolderStacking {
            marker: c[2].to_owned(),
            cleaned: c[1].to_owned(),
        },
        None => FolderStacking {
            marker: String::new(),
            cleaned: name.to_owned(),
        },
    }
}

/// The part number of a marker: `a` to `d` give 1 to 4, `XofY` gives X, and any other marker gives
/// its digits (`cd10` is 10). A marker without digits, such as `cda`, gives 0.
pub fn stacking_number(marker: &str) -> u32 {
    let mut letters = marker.chars();
    if let (Some(letter), None) = (letters.next(), letters.next()) {
        return match letter.to_ascii_lowercase() {
            'a' => 1,
            'b' => 2,
            'c' => 3,
            'd' => 4,
            _ => 0,
        };
    }
    let digits: String = if PATTERNS.x_of_y_marker.is_match(marker) {
        marker.chars().take_while(char::is_ascii_digit).collect()
    } else {
        marker.chars().filter(char::is_ascii_digit).collect()
    };
    digits.parse().unwrap_or(0)
}

/// The marker without its digits (`cd1` gives `cd`). Empty for a single letter and for `XofY`.
pub fn stacking_prefix(marker: &str) -> String {
    if marker.chars().count() <= 1 || PATTERNS.x_of_y_marker.is_match(marker) {
        return String::new();
    }
    marker.chars().filter(|c| !c.is_ascii_digit()).collect()
}

#[cfg(test)]
mod tests;
