//! The movie-set (collection) NFO writers for Kodi and Emby, and the NFO locations of a set.
//!
//! Element order, from `tests/compat/expected/nfo/movie-set-*`:
//!
//! ```text
//! Kodi: title sorttitle overview [thumb] [fanart] uniqueid* tag* [dateadded] [lockdata]
//!       <comment> user_note english_title
//! Emby: title sorttitle plot [thumb] [fanart] [tmdbid] uniqueid* tag* [dateadded] [lockdata]
//!       <comment> user_note english_title
//! ```
//!
//! `thumb` and `fanart` are the set's poster and fanart URLs and follow the "write artwork URLs"
//! option; `dateadded` and `lockdata` follow their options. A set has no files and no release
//! date, so the date-added choice "release date" writes an empty `<dateadded/>` and the file
//! dates fall back to the date the set was added.
//!
//! A set NFO lives next to the set's artwork. With a data folder `<root>`, the choices are:
//!
//! | Choice | Setting | Path under `<root>` |
//! |---|---|---|
//! | Kodi | `KODI_NFO` | `<folder>/<set>.nfo` |
//! | Kodi, short | `KODI2_NFO` | `<folder>/set.nfo` |
//! | Artwork Automator | `AUTOMATOR_NFO` | `<folder>.nfo` |
//! | Emby | `EMBY_NFO` | `<folder>/collection.nfo` |
//!
//! `<folder>` is the set name with characters a file name cannot hold replaced, plus the suffix
//! `-tmdb-<id>` when the option "append the TMDB ID" is on and the set has an ID.

use cv_domain::ProviderIds;

use crate::format;
use crate::kodi_movie::{finish, product_name};
use crate::movie::{DateAddedField, MovieNfoOptions};
use crate::xml::{Document, Element};

/// The format a movie-set NFO is written in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum MovieSetConnector {
    #[default]
    Kodi,
    Emby,
}

impl MovieSetConnector {
    /// Parses the setting name (`KODI` or `EMBY`), ignoring case.
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_uppercase().as_str() {
            "KODI" => Some(Self::Kodi),
            "EMBY" => Some(Self::Emby),
            _ => None,
        }
    }
}

/// Everything a movie-set NFO is written from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MovieSetNfo {
    pub title: String,
    pub sort_title: String,
    pub overview: String,
    /// Provider IDs; the TMDB collection ID is `tmdb`.
    pub ids: ProviderIds,
    pub tags: Vec<String>,
    /// When the set was added, local `yyyy-MM-dd HH:mm:ss`.
    pub date_added: Option<String>,
    /// The user's own note.
    pub note: String,
    pub english_title: String,
    /// Remote poster URL.
    pub poster: String,
    /// Remote fanart URL.
    pub fanart: String,
}

/// Builds the NFO document of a movie set.
pub fn movie_set_document(
    connector: MovieSetConnector,
    set: &MovieSetNfo,
    options: &MovieNfoOptions,
) -> Document {
    let text = |name: &str, value: &str| Element::text(name, value);
    let emby = connector == MovieSetConnector::Emby;
    let mut root = Element::new("collection");

    root.push(text("title", &set.title));
    root.push(text("sorttitle", &set.sort_title));
    root.push(text(if emby { "plot" } else { "overview" }, &set.overview));
    if options.artwork_urls {
        for (name, url) in [("thumb", &set.poster), ("fanart", &set.fanart)] {
            if !url.trim().is_empty() {
                root.push(text(name, url.trim()));
            }
        }
    }
    let tmdb = set.ids.get("tmdb").filter(|id| !id.is_empty());
    if emby && let Some(id) = tmdb {
        root.push(text("tmdbid", id));
    }
    unique_ids(set, &mut root);
    for tag in set.tags.iter().map(|t| t.trim()).filter(|t| !t.is_empty()) {
        root.push(text("tag", tag));
    }
    if options.write_date_added {
        root.push(text("dateadded", &date_added(set, options)));
    }
    if options.lockdata {
        root.push(text("lockdata", "true"));
    }
    root.push_comment(format!("{} meta data", product_name(&options.generator)));
    root.push(text("user_note", &set.note));
    root.push(text("english_title", &set.english_title));
    finish(root, options, if emby { "Emby" } else { "Kodi" })
}

/// Renders the NFO of a movie set: UTF-8 text, two-space indentation, CRLF line ends.
pub fn render_movie_set_nfo(
    connector: MovieSetConnector,
    set: &MovieSetNfo,
    options: &MovieNfoOptions,
) -> String {
    movie_set_document(connector, set, options).render()
}

fn date_added(set: &MovieSetNfo, options: &MovieNfoOptions) -> String {
    match options.date_added_field {
        DateAddedField::ReleaseDate => String::new(),
        _ => set.date_added.clone().unwrap_or_default(),
    }
}

/// `<uniqueid type= default=>` for every ID; the default is imdb, then tvdb, then tmdb, else the
/// first.
fn unique_ids(set: &MovieSetNfo, root: &mut Element) {
    let mut ids: Vec<(&str, &str)> = set.ids.iter().filter(|(_, v)| !v.is_empty()).collect();
    if ids.is_empty() {
        return;
    }
    format::stable_map_order(&mut ids, |pair| pair.0.to_owned());
    let default = ["imdb", "tvdb", "tmdb"]
        .into_iter()
        .find(|key| ids.iter().any(|(k, _)| k == key))
        .or_else(|| ids.first().map(|(k, _)| *k));
    for (key, value) in ids {
        root.push(
            Element::text("uniqueid", value)
                .attr(
                    "default",
                    if Some(key) == default {
                        "true"
                    } else {
                        "false"
                    },
                )
                .attr("type", key),
        );
    }
}

// ---- locations -------------------------------------------------------------------------------

/// A movie-set NFO naming choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MovieSetNfoNaming {
    /// `<folder>/<set>.nfo`.
    Kodi,
    /// `<folder>/set.nfo`.
    Kodi2,
    /// `<folder>.nfo`, next to the set folder.
    Automator,
    /// `<folder>/collection.nfo`.
    Emby,
}

impl MovieSetNfoNaming {
    /// Parses the setting name (`KODI_NFO`, `KODI2_NFO`, `AUTOMATOR_NFO`, `EMBY_NFO`), ignoring
    /// case.
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name.trim().to_ascii_uppercase().as_str() {
            "KODI_NFO" => Self::Kodi,
            "KODI2_NFO" => Self::Kodi2,
            "AUTOMATOR_NFO" => Self::Automator,
            "EMBY_NFO" => Self::Emby,
            _ => return None,
        })
    }
}

/// The character that replaces the ones a file name cannot hold, unless a setting says otherwise.
pub const DEFAULT_NAME_REPLACEMENT: &str = "_";

/// The set name as a file name: characters Windows file names cannot hold become `_`, and
/// trailing dots and spaces are dropped.
pub fn movie_set_file_name(title: &str) -> String {
    movie_set_file_name_with(title, DEFAULT_NAME_REPLACEMENT)
}

/// As [`movie_set_file_name`], with `replacement` (`-`, a space, nothing, ...) for each character
/// a file name cannot hold. A replacement that holds such a character itself is cleaned the same
/// way, so it can never add a folder level.
pub fn movie_set_file_name_with(title: &str, replacement: &str) -> String {
    let replacement: String = replacement.chars().filter(|c| !is_illegal(*c)).collect();
    let replaced: String = title
        .trim()
        .chars()
        .map(|c| {
            if is_illegal(c) {
                replacement.clone()
            } else {
                c.to_string()
            }
        })
        .collect();
    replaced.trim_end_matches(['.', ' ']).to_owned()
}

fn is_illegal(c: char) -> bool {
    matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control()
}

/// The name of the set's folder: the file name of the title, plus `-tmdb-<id>` when asked for and
/// the set has a TMDB ID.
pub fn movie_set_folder_name(title: &str, tmdb_id: Option<&str>, append_tmdb_id: bool) -> String {
    movie_set_folder_name_with(title, tmdb_id, append_tmdb_id, DEFAULT_NAME_REPLACEMENT)
}

/// As [`movie_set_folder_name`], with the replacement of [`movie_set_file_name_with`].
pub fn movie_set_folder_name_with(
    title: &str,
    tmdb_id: Option<&str>,
    append_tmdb_id: bool,
    replacement: &str,
) -> String {
    let name = movie_set_file_name_with(title, replacement);
    match tmdb_id.filter(|id| append_tmdb_id && !id.is_empty()) {
        Some(id) => format!("{name}-tmdb-{id}"),
        None => name,
    }
}

/// The NFO paths of a set under the collection data folder, with `/` separators, without
/// duplicates, in the order of the choices.
pub fn movie_set_nfo_paths(
    namings: &[MovieSetNfoNaming],
    title: &str,
    folder_name: &str,
) -> Vec<String> {
    movie_set_nfo_paths_for(namings, &movie_set_file_name(title), folder_name)
}

/// As [`movie_set_nfo_paths`], from the set's file name (see [`movie_set_file_name_with`]).
pub fn movie_set_nfo_paths_for(
    namings: &[MovieSetNfoNaming],
    file: &str,
    folder_name: &str,
) -> Vec<String> {
    let mut paths: Vec<String> = Vec::new();
    for naming in namings {
        let path = match naming {
            MovieSetNfoNaming::Kodi => format!("{folder_name}/{file}.nfo"),
            MovieSetNfoNaming::Kodi2 => format!("{folder_name}/set.nfo"),
            MovieSetNfoNaming::Automator => format!("{folder_name}.nfo"),
            MovieSetNfoNaming::Emby => format!("{folder_name}/collection.nfo"),
        };
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    paths
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;
    use MovieSetNfoNaming::{Automator, Emby, Kodi, Kodi2};

    fn set() -> MovieSetNfo {
        let mut set = MovieSetNfo {
            title: "Dune Collection".to_owned(),
            sort_title: "Dune 1".to_owned(),
            overview: "Sand & spice".to_owned(),
            poster: "https://img.example.test/s/poster.jpg".to_owned(),
            fanart: "https://img.example.test/s/fanart.jpg".to_owned(),
            tags: vec!["epic".to_owned()],
            date_added: Some("2022-02-03 04:05:06".to_owned()),
            ..MovieSetNfo::default()
        };
        set.ids.set("tmdb", "726871");
        set
    }

    fn names(doc: &Document) -> Vec<&str> {
        doc.roots[0]
            .children
            .iter()
            .filter_map(|n| match n {
                crate::xml::Node::Element(e) => Some(e.name.as_str()),
                crate::xml::Node::Comment(_) => None,
            })
            .collect()
    }

    #[test]
    fn kodi_writes_an_overview_and_no_tmdbid_element() {
        let doc = movie_set_document(MovieSetConnector::Kodi, &set(), &MovieNfoOptions::default());
        assert_eq!(
            names(&doc),
            [
                "title",
                "sorttitle",
                "overview",
                "thumb",
                "fanart",
                "uniqueid",
                "tag",
                "dateadded",
                "user_note",
                "english_title"
            ]
        );
        let text = doc.render();
        assert!(text.contains("<overview>Sand &amp; spice</overview>"));
        assert!(text.contains(r#"<uniqueid default="true" type="tmdb">726871</uniqueid>"#));
    }

    #[test]
    fn emby_writes_a_plot_and_the_tmdb_id() {
        let doc = movie_set_document(MovieSetConnector::Emby, &set(), &MovieNfoOptions::default());
        assert_eq!(
            names(&doc)[..6],
            ["title", "sorttitle", "plot", "thumb", "fanart", "tmdbid"]
        );
        assert!(doc.render().contains("<tmdbid>726871</tmdbid>"));
    }

    #[test]
    fn options_switch_elements_off_or_on() {
        let mut options = MovieNfoOptions::default();
        options.artwork_urls = false;
        options.write_date_added = false;
        options.lockdata = true;
        let doc = movie_set_document(MovieSetConnector::Kodi, &set(), &options);
        assert_eq!(
            names(&doc),
            [
                "title",
                "sorttitle",
                "overview",
                "uniqueid",
                "tag",
                "lockdata",
                "user_note",
                "english_title"
            ]
        );
    }

    #[test]
    fn a_set_has_no_release_date() {
        let mut options = MovieNfoOptions::default();
        options.date_added_field = DateAddedField::ReleaseDate;
        let text = render_movie_set_nfo(MovieSetConnector::Emby, &set(), &options);
        assert!(text.contains("<dateadded/>"));
        options.date_added_field = DateAddedField::FileCreationDate;
        let text = render_movie_set_nfo(MovieSetConnector::Emby, &set(), &options);
        assert!(text.contains("<dateadded>2022-02-03 04:05:06</dateadded>"));
    }

    #[test]
    fn a_bare_set_writes_only_the_always_present_elements() {
        let doc = movie_set_document(
            MovieSetConnector::Kodi,
            &MovieSetNfo::default(),
            &MovieNfoOptions::default(),
        );
        assert_eq!(
            names(&doc),
            [
                "title",
                "sorttitle",
                "overview",
                "dateadded",
                "user_note",
                "english_title"
            ]
        );
        let text = doc.render();
        assert!(text.contains("\r\n"));
        assert!(text.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>"));
    }

    #[test]
    fn the_header_comment_names_the_format() {
        let options = MovieNfoOptions::default();
        let comment = |connector| {
            movie_set_document(connector, &set(), &options)
                .comment
                .unwrap()
        };
        assert!(comment(MovieSetConnector::Emby).ends_with("for Emby"));
        assert!(comment(MovieSetConnector::Kodi).starts_with("created by CineVault"));
    }

    #[test]
    fn folder_names_replace_characters_and_append_the_tmdb_id() {
        assert_eq!(
            movie_set_folder_name("Dune Collection", Some("726871"), false),
            "Dune Collection"
        );
        assert_eq!(
            movie_set_folder_name("Dune Collection", Some("726871"), true),
            "Dune Collection-tmdb-726871"
        );
        assert_eq!(
            movie_set_folder_name("Dune Collection", None, true),
            "Dune Collection"
        );
        assert_eq!(
            movie_set_folder_name("Who? What: A/B ...", None, false),
            "Who_ What_ A_B"
        );
    }

    #[test]
    fn the_replacement_character_is_configurable() {
        assert_eq!(
            movie_set_file_name_with("Who? What: A/B", "-"),
            "Who- What- A-B"
        );
        assert_eq!(
            movie_set_file_name_with("Who? What: A/B", ""),
            "Who What AB"
        );
        assert_eq!(
            movie_set_file_name_with("Alien: Saga", " -"),
            "Alien - Saga"
        );
        // A replacement cannot bring back a character the name cannot hold.
        assert_eq!(movie_set_file_name_with("A:B", "/"), "AB");
        assert_eq!(
            movie_set_file_name("A:B"),
            movie_set_file_name_with("A:B", "_")
        );
        assert_eq!(
            movie_set_folder_name_with("Alien: Saga", Some("8091"), true, "-"),
            "Alien- Saga-tmdb-8091"
        );
        assert_eq!(movie_set_file_name_with("???", ""), "");
    }

    #[test]
    fn nfo_paths_follow_the_choices() {
        let all = movie_set_nfo_paths(
            &[Kodi, Kodi2, Automator, Emby],
            "Dune Collection",
            "Dune Collection",
        );
        assert_eq!(
            all,
            [
                "Dune Collection/Dune Collection.nfo",
                "Dune Collection/set.nfo",
                "Dune Collection.nfo",
                "Dune Collection/collection.nfo"
            ]
        );
        let suffixed = movie_set_nfo_paths(
            &[Kodi, Kodi, Automator],
            "Dune Collection",
            "Dune Collection-tmdb-1",
        );
        assert_eq!(
            suffixed,
            [
                "Dune Collection-tmdb-1/Dune Collection.nfo",
                "Dune Collection-tmdb-1.nfo"
            ]
        );
    }

    #[test]
    fn setting_names_parse() {
        assert_eq!(MovieSetNfoNaming::parse("kodi2_nfo"), Some(Kodi2));
        assert_eq!(MovieSetNfoNaming::parse("EMBY_NFO"), Some(Emby));
        assert_eq!(MovieSetNfoNaming::parse("x"), None);
        assert_eq!(
            MovieSetConnector::parse("emby"),
            Some(MovieSetConnector::Emby)
        );
        assert_eq!(MovieSetConnector::parse("jellyfin"), None);
    }
}
