//! First-run helpers: what a folder holds, and which media centre is installed.
//!
//! # Folder analysis
//!
//! [`analyze_folder`] walks a folder (bounded in depth and size, honouring the scanner's skip
//! rules) and counts its videos with the scanner's file classifier ([`cv_scan::classify`]). A video
//! counts as a TV episode when its name carries an episode marker (`S01E02`, `1x02`), when it sits
//! in a season folder (`Season 2`, `Specials`), or when the scanner's episode parser finds an
//! episode in a name that also has such a marker nearby. Everything else counts as a movie. The
//! folder is then labelled:
//!
//! | Videos | Label |
//! |---|---|
//! | none | [`FolderGuess::Empty`] (the name decides the suggested type) |
//! | no episodes | [`FolderGuess::Movies`] |
//! | at least [`TV_SHARE`] of them episodes | [`FolderGuess::Tv`] |
//! | fewer than [`MOVIE_SHARE`] episodes | [`FolderGuess::Movies`] |
//! | anything between | [`FolderGuess::Mixed`] |
//!
//! The first level of sub-folders is reported too ([`FolderAnalysis::children`]), each with its own
//! label, so a Mixed folder can be split into folders that each hold one type. Videos that sit
//! loose in the analysed folder itself are counted too ([`FolderAnalysis::loose_videos`]): a split
//! keeps only the sub-folders, so the screen tells the user what it leaves out.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use cv_domain::MediaFileType;
use cv_scan::classify::{classify, is_video_content};
use cv_scan::episode::{detect_episode_from_filename, parse_absolute_episode};
use cv_scan::skip::{LibraryKind, SkipRules};
use regex::Regex;

/// A folder is labelled TV shows when at least this share of its videos are episodes.
pub const TV_SHARE: f64 = 0.85;
/// A folder is labelled Movies when less than this share of its videos are episodes.
pub const MOVIE_SHARE: f64 = 0.15;
/// How deep the walk goes below the analysed folder.
pub const MAX_DEPTH: usize = 6;
/// How many loose video names are kept for the notice.
pub const LOOSE_EXAMPLES: usize = 5;
/// How many directory entries the walk visits before it stops and reports a partial count.
pub const MAX_ENTRIES: usize = 30_000;

/// What the contents of a folder look like.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderGuess {
    /// Movies.
    Movies,
    /// TV shows.
    Tv,
    /// Both movies and episodes.
    Mixed,
    /// No videos yet; the folder's name decides what to suggest.
    Empty,
}

/// The analysis of one folder, or of one of its sub-folders.
#[derive(Debug, Clone, PartialEq)]
pub struct FolderAnalysis {
    pub path: String,
    pub name: String,
    pub guess: FolderGuess,
    /// What to preselect: the guess, or for an empty folder the type its name suggests.
    pub suggested: SuggestedKind,
    /// Videos found (main videos only: no trailers, samples or extras).
    pub videos: usize,
    /// Of those, how many look like episodes.
    pub episodes: usize,
    /// The walk stopped at [`MAX_ENTRIES`]; the counts are lower bounds.
    pub truncated: bool,
    /// The sub-folders that hold videos, each with its own label. Filled for the analysed folder
    /// only.
    pub children: Vec<FolderAnalysis>,
    /// Videos that sit directly in the analysed folder, outside every sub-folder. A split into
    /// sub-folders leaves them out, so the first-run screen says so. Filled for the analysed
    /// folder only.
    pub loose_videos: usize,
    /// The names of the first few of those, for the notice ([`LOOSE_EXAMPLES`] at most).
    pub loose_examples: Vec<String>,
    /// The folder is one TV show (season folders or episodes sit directly in it), not a folder
    /// of shows. Such a folder is not a TV library folder by itself: its parent is. Filled for
    /// the sub-folders only.
    pub show: bool,
    /// The analysed folder is itself one TV show (season folders or episodes sit directly in
    /// it), so its parent is the TV library folder. Filled for the analysed folder only; the
    /// first-run screen offers "Use parent".
    pub single_show: bool,
    /// The full path of the video the Renaming step renders its live example on: of the main
    /// videos that are not episodes, the one whose name says the most (resolution, source, HDR,
    /// sound), first in name order among equals, so the naming styles show what they differ in.
    /// Filled for the analysed folder only.
    pub first_video: Option<String>,
}

/// The library a folder is added to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestedKind {
    Movies,
    Tv,
    /// The user has to choose: the folder is mixed.
    Undecided,
}

/// Why a folder cannot be analysed.
#[derive(Debug, thiserror::Error)]
pub enum AnalysisError {
    #[error("\"{0}\" is not a folder CineVault can read. Check the path and your access to it.")]
    NotAFolder(String),
}

static EPISODE_MARK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:^|[^a-z0-9])(?:s\d{1,3}[ ._-]?e\d{1,4}|\d{1,2}x\d{2,3})(?:[^a-z0-9]|$)")
        .expect("episode marker pattern is valid")
});

static SEASON_FOLDER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?:season|series|saison|staffel|temporada|stagione)[ ._-]*\d{1,3}$|^specials?$|^s\d{1,2}$")
        .expect("season folder pattern is valid")
});

static TV_NAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:tv|shows?|series|serien|anime|episodes?)\b|tvshows?")
        .expect("tv name pattern is valid")
});

/// Whether a video, given by its path below the analysed folder, looks like an episode.
pub fn looks_like_episode(relative_path: &str) -> bool {
    let segments: Vec<&str> = relative_path
        .split(['/', '\\'])
        .filter(|s| !s.is_empty())
        .collect();
    let Some((file, folders)) = segments.split_last() else {
        return false;
    };
    if folders.iter().any(|folder| SEASON_FOLDER.is_match(folder)) {
        return true;
    }
    let stem = file.rsplit_once('.').map_or(*file, |(stem, _)| stem);
    if EPISODE_MARK.is_match(stem) {
        // The scanner's own parser confirms that the marker is an episode and not part of a title.
        let show = folders.first().copied().unwrap_or("");
        return !detect_episode_from_filename(file, show).episodes.is_empty();
    }
    false
}

fn label(videos: usize, episodes: usize) -> FolderGuess {
    if videos == 0 {
        return FolderGuess::Empty;
    }
    if episodes == 0 {
        return FolderGuess::Movies;
    }
    #[allow(clippy::cast_precision_loss)]
    let share = episodes as f64 / videos as f64;
    if share >= TV_SHARE {
        FolderGuess::Tv
    } else if share < MOVIE_SHARE {
        FolderGuess::Movies
    } else {
        FolderGuess::Mixed
    }
}

fn suggestion(guess: FolderGuess, name: &str) -> SuggestedKind {
    match guess {
        FolderGuess::Movies => SuggestedKind::Movies,
        FolderGuess::Tv => SuggestedKind::Tv,
        FolderGuess::Mixed => SuggestedKind::Undecided,
        FolderGuess::Empty if TV_NAME.is_match(name) => SuggestedKind::Tv,
        FolderGuess::Empty => SuggestedKind::Movies,
    }
}

fn folder_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.to_string_lossy().into_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}

fn analysis(path: &Path, videos: usize, episodes: usize, truncated: bool) -> FolderAnalysis {
    let name = folder_name(path);
    let guess = label(videos, episodes);
    FolderAnalysis {
        path: path.to_string_lossy().into_owned(),
        suggested: suggestion(guess, &name),
        name,
        guess,
        videos,
        episodes,
        truncated,
        children: Vec::new(),
        loose_videos: 0,
        loose_examples: Vec::new(),
        show: false,
        single_show: false,
        first_video: None,
    }
}

#[derive(Default, Clone, Copy)]
struct Counts {
    videos: usize,
    episodes: usize,
}

/// A main video found by the walk.
struct Video {
    /// The file name.
    name: String,
    /// The path relative to the analysed folder, `/` separated.
    relative: String,
    /// The folder it sits in, relative to the analysed folder (empty for the folder itself).
    folder: String,
    /// The top-level sub-folder it is below, when it is below one.
    child: Option<usize>,
    episode: bool,
}

/// A top-level sub-folder while the walk runs.
struct Child {
    path: PathBuf,
    counts: Counts,
    /// A season folder sits directly in it.
    has_season_folder: bool,
    /// Its own name is a season folder name (`Season 1`, `Specials`).
    is_season: bool,
}

static DIGITS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[0-9]+").expect("digits pattern is valid"));

static YEAR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:^|[^0-9])(?:19|20)[0-9]{2}(?:[^0-9]|$)").expect("year pattern is valid")
});

/// How many numbered files of one name it takes to call them a show's episodes.
pub const SERIES_MIN_FILES: usize = 3;

/// Whether the videos of one folder are the numbered episodes of one show, as anime and other
/// shows in absolute order are named: `[Group] Show - 001 [1080p].mkv`, `Show - 002 ...`. The
/// names must be the same apart from their numbers, each must carry an episode number (the
/// scanner's absolute-order parser, with `show_name` taken out), the numbers must differ, and
/// no name may carry a year (`Movie 2 (1986)` is a film series, not a show). It takes at least
/// [`SERIES_MIN_FILES`] files.
pub fn numbered_series(file_names: &[&str], show_name: &str) -> bool {
    if file_names.len() < SERIES_MIN_FILES {
        return false;
    }
    let mut template: Option<String> = None;
    let mut numbers = HashSet::new();
    for name in file_names {
        let stem = name.rsplit_once('.').map_or(*name, |(stem, _)| stem);
        if YEAR.is_match(stem) {
            return false;
        }
        let shape = DIGITS.replace_all(stem, "#").into_owned();
        if *template.get_or_insert_with(|| shape.clone()) != shape {
            return false;
        }
        let Some(number) = parse_absolute_episode::<&str>(name, show_name, &[]) else {
            return false;
        };
        if !numbers.insert(number) {
            return false;
        }
    }
    true
}

/// Marks as episodes the videos of every folder that holds a numbered series.
fn mark_numbered_series(videos: &mut [Video]) {
    let mut by_folder: HashMap<String, Vec<usize>> = HashMap::new();
    for (index, video) in videos.iter().enumerate() {
        if !video.episode {
            by_folder
                .entry(video.folder.to_lowercase())
                .or_default()
                .push(index);
        }
    }
    for indexes in by_folder.into_values() {
        let Some(&first) = indexes.first() else {
            continue;
        };
        let show = videos[first]
            .folder
            .rsplit('/')
            .next()
            .unwrap_or("")
            .to_owned();
        let names: Vec<&str> = indexes.iter().map(|&i| videos[i].name.as_str()).collect();
        if numbered_series(&names, &show) {
            for i in indexes {
                videos[i].episode = true;
            }
        }
    }
}

/// Analyses a folder: what it holds and how it could be split.
///
/// # Errors
/// [`AnalysisError::NotAFolder`] when the path is not a readable folder.
pub fn analyze_folder(path: &Path) -> Result<FolderAnalysis, AnalysisError> {
    if !path.is_dir() || fs::read_dir(path).is_err() {
        return Err(AnalysisError::NotAFolder(
            path.to_string_lossy().into_owned(),
        ));
    }
    let rules = SkipRules::with_defaults(LibraryKind::Movies);
    let mut visited = 0usize;
    let mut truncated = false;
    let mut children: Vec<Child> = Vec::new();
    let mut videos: Vec<Video> = Vec::new();

    // Stack of (directory, path relative to the analysed folder, top-level child, depth).
    let mut stack: Vec<(PathBuf, String, Option<usize>, usize)> =
        vec![(path.to_path_buf(), String::new(), None, 0)];
    while let Some((dir, relative, child, depth)) = stack.pop() {
        let Ok(read) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.flatten() {
            visited += 1;
            if visited > MAX_ENTRIES {
                truncated = true;
                break;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let relative_path = if relative.is_empty() {
                name.clone()
            } else {
                format!("{relative}/{name}")
            };
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                let full = entry.path();
                if depth >= MAX_DEPTH
                    || rules.is_skipped_folder(&name, &full.to_string_lossy())
                    || rules.dir_has_marker(&full)
                {
                    continue;
                }
                let own = if depth == 0 {
                    children.push(Child {
                        path: full.clone(),
                        counts: Counts::default(),
                        has_season_folder: false,
                        is_season: SEASON_FOLDER.is_match(&name),
                    });
                    Some(children.len() - 1)
                } else {
                    if depth == 1
                        && let Some(index) = child
                        && SEASON_FOLDER.is_match(&name)
                    {
                        children[index].has_season_folder = true;
                    }
                    child
                };
                stack.push((full, relative_path, own, depth + 1));
            } else if kind.is_file() {
                let class = classify(&relative_path);
                let main_video = class.file_type == MediaFileType::Video
                    && (!class.disc_file || class.main_disc_identifier)
                    && is_video_content(&entry.path());
                if !main_video {
                    continue;
                }
                videos.push(Video {
                    episode: looks_like_episode(&relative_path),
                    name,
                    relative: relative_path,
                    folder: relative.clone(),
                    child,
                });
            }
        }
        if truncated {
            break;
        }
    }
    mark_numbered_series(&mut videos);

    let mut total = Counts::default();
    let mut loose: Vec<String> = Vec::new();
    // The example video: not an episode, the most release tags, then first by name.
    let first_video = videos
        .iter()
        .filter(|video| !video.episode)
        .min_by_key(|video| {
            (
                std::cmp::Reverse(crate::organize::tag_count(&video.name)),
                video.relative.to_lowercase(),
            )
        })
        .map(|video| path.join(&video.relative).to_string_lossy().into_owned());
    // An episode directly in the analysed folder, or a season folder sitting in it.
    let episode_at_root = videos
        .iter()
        .any(|video| video.episode && !video.relative.contains('/'));
    // Sub-folders with an episode directly inside them: each is one show.
    let mut episodes_inside = vec![false; children.len()];
    for video in &videos {
        total.videos += 1;
        total.episodes += usize::from(video.episode);
        if let Some(index) = video.child {
            children[index].counts.videos += 1;
            children[index].counts.episodes += usize::from(video.episode);
            if video.episode && video.relative.matches('/').count() == 1 {
                episodes_inside[index] = true;
            }
        } else {
            loose.push(video.name.clone());
        }
    }

    let season_at_root = children
        .iter()
        .any(|child| child.counts.videos > 0 && child.is_season);
    let mut result = analysis(path, total.videos, total.episodes, truncated);
    result.first_video = first_video;
    result.single_show = result.guess == FolderGuess::Tv && (episode_at_root || season_at_root);
    loose.sort_by_key(|name| name.to_lowercase());
    result.loose_videos = loose.len();
    loose.truncate(LOOSE_EXAMPLES);
    result.loose_examples = loose;
    result.children = children
        .into_iter()
        .zip(episodes_inside)
        .filter(|(child, _)| child.counts.videos > 0)
        .map(|(child, episodes_inside)| {
            let mut found = analysis(
                &child.path,
                child.counts.videos,
                child.counts.episodes,
                false,
            );
            found.show = child.counts.episodes > 0 && (episodes_inside || child.has_season_folder);
            found
        })
        .collect();
    result
        .children
        .sort_by_key(|child| child.name.to_lowercase());
    Ok(result)
}

#[cfg(test)]
mod tests;
