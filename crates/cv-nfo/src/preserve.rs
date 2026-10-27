//! What a rewrite keeps from the NFO that is already on disk.
//!
//! When an NFO is written again, the old file is read first (unless "write clean NFO" is on) and
//! two kinds of content are copied into the writer model:
//!
//! * **Carried-over values** that the app does not own: play position (`epbookmark`), `status`,
//!   `code`, the play count and last played time, the `outline`, the old `episodeguide` of a show
//!   and the other `<set>` entries of a movie. The writers decide how they combine with the stored
//!   values (the higher play count and the later last-played time win; the old outline is the
//!   last fallback).
//! * **Unmanaged elements**: every direct child of the root that no writer or reader of CineVault
//!   knows by name, with its attributes and nested elements. The writers put them back unchanged,
//!   just before the CineVault private tags, so a custom `<customfield>` survives any number of
//!   rewrites.
//!
//! The `apply_old_*` functions fill the model from the old file's text and do nothing for text
//! that holds no element of the right kind (a foreign root, an empty file) or when
//! [`MovieNfoOptions::clean_nfo`] is on. [`read_existing_nfo`] picks the old text from the paths
//! an NFO is written to. With "write clean NFO" on the caller can skip the read entirely.
//!
//! Limits: an unmanaged element keeps its child elements, attributes and (for a leaf) its text;
//! text mixed with child elements is not kept (see [`crate::tree`]). A movie-set NFO is never
//! read, which matches the compatibility fixtures.

use std::fs;
use std::path::Path;

use crate::movie::{MovieNfo, MovieNfoOptions, MovieSetRef};
use crate::tree::parse_roots;
use crate::tv::{EpisodeNfo, SeasonNfo, ShowNfo};
use crate::xml::{Element, Node};

/// Element names of a movie NFO that CineVault writes or reads (including the spellings other
/// formats use); anything else is kept as an unmanaged element.
const MOVIE_ELEMENTS: &[&str] = &[
    "title",
    "originaltitle",
    "sorttitle",
    "epbookmark",
    "year",
    "ratings",
    "rating",
    "votes",
    "userrating",
    "criticrating",
    "top250",
    "set",
    "sets",
    "plot",
    "outline",
    "tagline",
    "runtime",
    "thumb",
    "fanart",
    "mpaa",
    "certification",
    "id",
    "imdb",
    "imdbid",
    "tmdbid",
    "tmdbId",
    "uniqueid",
    "ids",
    "country",
    "status",
    "code",
    "premiered",
    "releasedate",
    "aired",
    "watched",
    "playcount",
    "lastplayed",
    "genre",
    "genres",
    "studio",
    "credits",
    "director",
    "tag",
    "actor",
    "producer",
    "trailer",
    "languages",
    "language",
    "showlink",
    "dateadded",
    "lockdata",
    "fileinfo",
    "videoassettitle",
    "hasvideoversions",
    "collectionnumber",
    "source",
    "edition",
    "original_filename",
    "user_note",
    "english_title",
    "crc32",
    "original_language",
    "video3d",
    "extrathumbs",
    "crew",
    "cv_locked",
];

const SHOW_ELEMENTS: &[&str] = &[
    "title",
    "originaltitle",
    "showtitle",
    "sorttitle",
    "year",
    "top250",
    "rating",
    "ratings",
    "userrating",
    "criticrating",
    "votes",
    "outline",
    "plot",
    "tagline",
    "runtime",
    "thumb",
    "namedseason",
    "seasonplot",
    "fanart",
    "mpaa",
    "certification",
    "episodeguide",
    "id",
    "imdbid",
    "tmdbid",
    "tvdbid",
    "uniqueid",
    "premiered",
    "status",
    "watched",
    "playcount",
    "genre",
    "studio",
    "country",
    "tag",
    "credits",
    "director",
    "actor",
    "trailer",
    "dateadded",
    "lockdata",
    "enddate",
    "user_note",
    "episode_groups",
    "english_title",
    "original_language",
    "year_end",
    "crew",
    "cv_locked",
];

const SEASON_ELEMENTS: &[&str] = &[
    "seasonnumber",
    "season",
    "title",
    "showtitle",
    "sorttitle",
    "year",
    "plot",
    "thumb",
    "fanart",
    "tvdbid",
    "imdbid",
    "tmdbid",
    "uniqueid",
    "premiered",
    "lockdata",
    "user_note",
];

const EPISODE_ELEMENTS: &[&str] = &[
    "title",
    "originaltitle",
    "showtitle",
    "season",
    "episode",
    "displayseason",
    "displayepisode",
    "id",
    "uniqueid",
    "ratings",
    "rating",
    "votes",
    "userrating",
    "plot",
    "outline",
    "runtime",
    "thumb",
    "mpaa",
    "certification",
    "premiered",
    "aired",
    "watched",
    "playcount",
    "lastplayed",
    "studio",
    "tag",
    "credits",
    "director",
    "actor",
    "trailer",
    "dateadded",
    "lockdata",
    "fileinfo",
    "epbookmark",
    "code",
    "videoassettitle",
    "hasvideoversions",
    "episodenumberend",
    "source",
    "edition",
    "original_filename",
    "user_note",
    "episode_groups",
    "english_title",
    "crc32",
    "crew",
    "cv_locked",
];

/// Reads the first of `paths` that exists as a file, as text (a byte-order mark is removed, bad
/// UTF-8 is replaced). `None` when there is none or none can be read.
pub fn read_existing_nfo<P: AsRef<Path>>(paths: &[P]) -> Option<String> {
    paths.iter().find_map(|path| {
        let bytes = fs::read(path.as_ref()).ok()?;
        let text = String::from_utf8_lossy(&bytes);
        Some(text.strip_prefix('\u{feff}').unwrap_or(&text).to_owned())
    })
}

/// Copies the carried-over values and unmanaged elements of an old movie NFO into `movie`.
/// Returns whether an old `<movie>` was applied (false for clean NFOs, a foreign root, no text).
pub fn apply_old_movie_nfo(movie: &mut MovieNfo, old: &str, options: &MovieNfoOptions) -> bool {
    let Some(root) = old_root(old, "movie", options) else {
        return false;
    };
    movie.carried.epbookmark = text_of(&root, "epbookmark");
    movie.carried.status = text_of(&root, "status");
    movie.carried.code = text_of(&root, "code");
    movie.carried.outline = text_of(&root, "outline");
    movie.carried.play_count = count_of(&root, "playcount");
    movie.carried.last_played = Some(text_of(&root, "lastplayed")).filter(|t| !t.is_empty());
    movie.other_sets = old_sets(&root);
    movie.unknown_elements = unmanaged(&root, MOVIE_ELEMENTS);
    true
}

/// Copies the carried-over values and unmanaged elements of an old `tvshow.nfo` into `show`.
pub fn apply_old_show_nfo(show: &mut ShowNfo, old: &str, options: &MovieNfoOptions) -> bool {
    let Some(root) = old_root(old, "tvshow", options) else {
        return false;
    };
    show.carried.outline = text_of(&root, "outline");
    show.carried.episodeguide = root.find("episodeguide").cloned();
    show.carried.play_count = count_of(&root, "playcount");
    show.unknown_elements = unmanaged(&root, SHOW_ELEMENTS);
    true
}

/// Copies the unmanaged elements of an old season NFO into `season`.
pub fn apply_old_season_nfo(season: &mut SeasonNfo, old: &str, options: &MovieNfoOptions) -> bool {
    let Some(root) = old_root(old, "season", options) else {
        return false;
    };
    season.unknown_elements = unmanaged(&root, SEASON_ELEMENTS);
    true
}

/// Copies the carried-over values and unmanaged elements of an old episode NFO into the episodes
/// written to the same file. Old blocks are matched to episodes by season and episode number;
/// blocks and episodes left over are paired in order when there are as many of each (a renumbered
/// episode keeps its block). Returns the number of episodes that received an old block.
pub fn apply_old_episode_nfo(
    episodes: &mut [EpisodeNfo],
    old: &str,
    options: &MovieNfoOptions,
) -> usize {
    if options.clean_nfo {
        return 0;
    }
    let blocks: Vec<Element> = parse_roots(old)
        .into_iter()
        .filter(|e| e.name == "episodedetails")
        .collect();
    let mut taken = vec![false; blocks.len()];
    let mut assigned: Vec<Option<usize>> = vec![None; episodes.len()];
    for (i, episode) in episodes.iter().enumerate() {
        let found = blocks.iter().enumerate().position(|(b, block)| {
            !taken[b]
                && number_of(block, "season") == Some(episode.season)
                && number_of(block, "episode") == Some(episode.episode)
        });
        if let Some(b) = found {
            taken[b] = true;
            assigned[i] = Some(b);
        }
    }
    let free_blocks: Vec<usize> = (0..blocks.len()).filter(|b| !taken[*b]).collect();
    let free_episodes: Vec<usize> = (0..episodes.len())
        .filter(|i| assigned[*i].is_none())
        .collect();
    if free_blocks.len() == free_episodes.len() {
        for (i, b) in free_episodes.into_iter().zip(free_blocks) {
            assigned[i] = Some(b);
        }
    }
    let mut applied = 0;
    for (episode, block) in episodes.iter_mut().zip(assigned) {
        let Some(block) = block.map(|b| &blocks[b]) else {
            continue;
        };
        episode.carried.epbookmark = text_of(block, "epbookmark");
        episode.carried.code = text_of(block, "code");
        episode.carried.outline = text_of(block, "outline");
        episode.carried.trailer = text_of(block, "trailer");
        episode.carried.play_count = count_of(block, "playcount");
        episode.carried.last_played = Some(text_of(block, "lastplayed")).filter(|t| !t.is_empty());
        episode.unknown_elements = unmanaged(block, EPISODE_ELEMENTS);
        applied += 1;
    }
    applied
}

/// The first root element of `old` called `name`, unless the NFO is written clean.
fn old_root(old: &str, name: &str, options: &MovieNfoOptions) -> Option<Element> {
    if options.clean_nfo {
        return None;
    }
    parse_roots(old).into_iter().find(|e| e.name == name)
}

/// The trimmed text of the first child called `name`; empty when there is none or it holds
/// elements.
fn text_of(root: &Element, name: &str) -> String {
    root.find(name)
        .filter(|e| e.children.is_empty())
        .map(|e| e.text.trim().to_owned())
        .unwrap_or_default()
}

fn count_of(root: &Element, name: &str) -> u32 {
    text_of(root, name).parse().unwrap_or(0)
}

fn number_of(root: &Element, name: &str) -> Option<u32> {
    text_of(root, name).parse().ok()
}

/// The children of `root` whose names are not in `known`, unchanged. Top-level comments are not
/// content (the writers add their own), so only elements are returned.
fn unmanaged(root: &Element, known: &[&str]) -> Vec<Element> {
    root.children
        .iter()
        .filter_map(|node| match node {
            Node::Element(e) if !known.contains(&e.name.as_str()) => Some(e.clone()),
            _ => None,
        })
        .collect()
}

/// Every `<set>` of the old movie NFO: the text form (`<set>Name</set>`) and the object form
/// (`<set><name/><overview/></set>`), also inside `<sets>`; `tmdbcolid` gives the collection ID.
fn old_sets(root: &Element) -> Vec<MovieSetRef> {
    let mut sets = Vec::new();
    for node in &root.children {
        let Node::Element(child) = node else { continue };
        match child.name.as_str() {
            "set" => sets.extend(set_ref(child)),
            "sets" => sets.extend(child.children.iter().filter_map(|n| match n {
                Node::Element(e) if e.name == "set" => set_ref(e),
                _ => None,
            })),
            _ => {}
        }
    }
    sets
}

fn set_ref(element: &Element) -> Option<MovieSetRef> {
    let (title, overview) = if element.find("name").is_some() {
        (text_of(element, "name"), text_of(element, "overview"))
    } else if element.children.is_empty() {
        (element.text.trim().to_owned(), String::new())
    } else {
        return None;
    };
    Some(MovieSetRef {
        title,
        overview,
        tmdb_id: element
            .attribute("tmdbcolid")
            .and_then(|v| v.trim().parse().ok()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const OLD_MOVIE: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<movie>\r\n  <title>Old</title>\r\n  <epbookmark> 12.5 </epbookmark>\r\n  <outline>Old outline</outline>\r\n  <playcount>4</playcount>\r\n  <lastplayed>2025-01-02 03:04:05</lastplayed>\r\n  <status>Released</status>\r\n  <code>X-1</code>\r\n  <set tmdbcolid=\"9\"><name>Saga</name><overview>All of it</overview></set>\r\n  <set>Plain</set>\r\n  <customfield>abc</customfield>\r\n  <nested><inner a=\"1\">value</inner></nested>\r\n</movie>\r\n";

    #[test]
    fn a_movie_takes_carried_values_sets_and_unmanaged_elements() {
        let mut movie = MovieNfo::default();
        assert!(apply_old_movie_nfo(
            &mut movie,
            OLD_MOVIE,
            &MovieNfoOptions::default()
        ));
        assert_eq!(movie.carried.epbookmark, "12.5");
        assert_eq!(movie.carried.outline, "Old outline");
        assert_eq!(movie.carried.status, "Released");
        assert_eq!(movie.carried.code, "X-1");
        assert_eq!(movie.carried.play_count, 4);
        assert_eq!(
            movie.carried.last_played.as_deref(),
            Some("2025-01-02 03:04:05")
        );
        assert_eq!(movie.other_sets.len(), 2);
        assert_eq!(movie.other_sets[0].title, "Saga");
        assert_eq!(movie.other_sets[0].overview, "All of it");
        assert_eq!(movie.other_sets[0].tmdb_id, Some(9));
        assert_eq!(movie.other_sets[1].title, "Plain");
        let names: Vec<&str> = movie
            .unknown_elements
            .iter()
            .map(|e| e.name.as_str())
            .collect();
        assert_eq!(names, ["customfield", "nested"]);
    }

    #[test]
    fn clean_nfo_reads_nothing() {
        let options = MovieNfoOptions {
            clean_nfo: true,
            ..MovieNfoOptions::default()
        };
        let mut movie = MovieNfo::default();
        assert!(!apply_old_movie_nfo(&mut movie, OLD_MOVIE, &options));
        assert_eq!(movie, MovieNfo::default());
    }

    #[test]
    fn a_foreign_root_or_garbage_changes_nothing() {
        let mut movie = MovieNfo::default();
        let options = MovieNfoOptions::default();
        assert!(!apply_old_movie_nfo(
            &mut movie,
            "<tvshow><title>x</title><odd/></tvshow>",
            &options
        ));
        assert!(!apply_old_movie_nfo(&mut movie, "", &options));
        assert!(!apply_old_movie_nfo(&mut movie, "just text", &options));
        assert_eq!(movie, MovieNfo::default());
    }

    #[test]
    fn a_show_keeps_the_episodeguide_playcount_outline_and_unmanaged_elements() {
        let old = "<tvshow><title>Old</title><outline>o</outline><playcount>3</playcount><episodeguide><url>https://example.test/g</url></episodeguide><customfield>kept</customfield></tvshow>";
        let mut show = ShowNfo::default();
        assert!(apply_old_show_nfo(
            &mut show,
            old,
            &MovieNfoOptions::default()
        ));
        assert_eq!(show.carried.outline, "o");
        assert_eq!(show.carried.play_count, 3);
        assert_eq!(
            show.carried
                .episodeguide
                .as_ref()
                .unwrap()
                .find("url")
                .unwrap()
                .text,
            "https://example.test/g"
        );
        assert_eq!(show.unknown_elements.len(), 1);
        assert_eq!(show.unknown_elements[0].name, "customfield");
    }

    #[test]
    fn a_season_keeps_unmanaged_elements() {
        let old =
            "<season><seasonnumber>1</seasonnumber><title>S1</title><custom>x</custom></season>";
        let mut season = SeasonNfo::default();
        assert!(apply_old_season_nfo(
            &mut season,
            old,
            &MovieNfoOptions::default()
        ));
        assert_eq!(season.unknown_elements.len(), 1);
    }

    fn episode(season: u32, episode: u32) -> EpisodeNfo {
        EpisodeNfo {
            season,
            episode,
            ..EpisodeNfo::default()
        }
    }

    const OLD_EPISODES: &str = "<episodedetails><season>1</season><episode>1</episode><code>A</code><playcount>2</playcount><lastplayed>2024-01-01 00:00:00</lastplayed><custom>one</custom></episodedetails>\r\n<episodedetails><season>1</season><episode>2</episode><epbookmark>5</epbookmark><custom>two</custom></episodedetails>";

    #[test]
    fn episodes_are_matched_by_number() {
        let mut episodes = vec![episode(1, 2), episode(1, 1)];
        let applied =
            apply_old_episode_nfo(&mut episodes, OLD_EPISODES, &MovieNfoOptions::default());
        assert_eq!(applied, 2);
        assert_eq!(episodes[0].carried.epbookmark, "5");
        assert_eq!(episodes[0].unknown_elements[0].text, "two");
        assert_eq!(episodes[1].carried.code, "A");
        assert_eq!(episodes[1].carried.play_count, 2);
        assert_eq!(
            episodes[1].carried.last_played.as_deref(),
            Some("2024-01-01 00:00:00")
        );
    }

    #[test]
    fn a_renumbered_episode_keeps_its_only_block() {
        let old = "<episodedetails><season>1</season><episode>5</episode><custom>x</custom></episodedetails>";
        let mut episodes = vec![episode(1, 6)];
        assert_eq!(
            apply_old_episode_nfo(&mut episodes, old, &MovieNfoOptions::default()),
            1
        );
        assert_eq!(episodes[0].unknown_elements.len(), 1);
        // Two blocks for one episode with neither matching: no guess is made.
        let mut one = vec![episode(1, 9)];
        assert_eq!(
            apply_old_episode_nfo(&mut one, OLD_EPISODES, &MovieNfoOptions::default()),
            0
        );
    }

    #[test]
    fn clean_nfo_leaves_episodes_alone() {
        let options = MovieNfoOptions {
            clean_nfo: true,
            ..MovieNfoOptions::default()
        };
        let mut episodes = vec![episode(1, 1)];
        assert_eq!(
            apply_old_episode_nfo(&mut episodes, OLD_EPISODES, &options),
            0
        );
        assert!(episodes[0].unknown_elements.is_empty());
    }

    #[test]
    fn the_first_readable_path_is_used() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing.nfo");
        let present = dir.path().join("movie.nfo");
        fs::write(&present, "\u{feff}<movie/>").unwrap();
        assert_eq!(
            read_existing_nfo(&[missing.clone(), present]).as_deref(),
            Some("<movie/>")
        );
        assert_eq!(read_existing_nfo(&[missing]), None);
    }
}
