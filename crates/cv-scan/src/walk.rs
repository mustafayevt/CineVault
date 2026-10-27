//! The movie folder walker and the TV show walker.
//!
//! [`scan_movie_sources`] reads the movie data sources in position order and turns every folder
//! tree into [`ScannedMovie`] groups: one movie with the files that belong to it. It only reads
//! the file system; nothing is written, moved or deleted, and matching, NFO reading and media
//! info are later steps. Skip rules ([`crate::skip`]), file classification
//! ([`crate::classify::classify`]) and the name parsers ([`crate::parse`], [`crate::stacking`])
//! do the per-name work; this module decides what belongs together.
//!
//! # Folder kinds
//!
//! - **Single-movie folder:** one distinct video (parts of a stack count once), for example
//!   `Heat (1995)/Heat (1995).mkv`. The title, year, edition and IDs come from the folder name,
//!   and every file below the folder (subtitles, extras, `extrafanart`, ...) joins the movie.
//! - **Multi-movie folder:** several distinct videos in one folder, any file directly in a data
//!   source root, a folder that holds sub-folders with movies of their own, and a folder named as
//!   a bucket: one letter or digit (`A`), a year (`1999`), a decade (`1990s`), a year range
//!   (`1990-1999`) or a known genre (`Horror`). Each video becomes a movie named after the file.
//!   Companion files belong to the movie whose file base name they start with, longest base name
//!   first so `Alien.Resurrection.mkv` is not mistaken for `Alien.mkv`.
//! - **Disc structure:** a folder that holds `VIDEO_TS`, `BDMV` or `HVDVD_TS`. One movie is made
//!   for the folder above it. The disc folder is added as the movie's video, with only its own
//!   `.nfo` files, trailer files and the identifier file (`VIDEO_TS.IFO`, `index.bdmv`,
//!   `HV000I01.IFO`); nothing below a disc folder is walked.
//! - **Stacked folders:** `Movie/CD1` and `Movie/CD2` (a folder named only by a part marker)
//!   belong to `Movie`, which becomes one movie with its videos in part order.
//!
//! # Companion rules
//!
//! After grouping, the files of a movie are filtered: videos,
//! trailers, samples, extras, subtitles, audio, NFO and similar files are kept. Artwork named
//! `<other base>-poster.jpg` (a prefix that is not the movie's video base name) is downgraded to
//! a generic graphic, and a generic graphic is promoted back to the first artwork type that is
//! still missing (fanart, poster, banner, clearlogo, clearart, keyart, disc). A generic graphic
//! named like the video or the title becomes the poster. Unknown files that start with a video's
//! full name (`movie.mkv.xyz`) are kept as double-extension files; other unknown files and
//! season artwork are not part of a movie.
//!
//! Generic graphics are attached in multi-movie folders too: the same companion pass runs for
//! every movie, so
//! `Alien (1979)-poster.jpg` and `Aliens (1986)-poster.jpg` each go to their own movie.
//!
//! # TV shows
//!
//! [`scan_tv_source`] and [`scan_tv_sources`] turn TV data sources into [`ScannedShow`] groups:
//! the show with its files, its seasons (artwork and NFO) and its episodes, each with the files
//! that belong to it.
//!
//! - **Shows:** every folder directly in the data source is a show, except a one-character
//!   folder (`A`, `7`), which is an A to Z bucket whose folders are the shows. A one-character
//!   folder that holds a video or a `tvshow.nfo` of its own is a show. A folder with no video
//!   anywhere below it is not a show. A video directly in the data source is reported as
//!   [`ProblemKind::EpisodeInRoot`] and belongs to no show.
//! - **Episodes:** one per video file, per disc folder, and per stack of parts (`.cd1`, `.cd2`).
//!   The season, episode numbers and date come from [`crate::episode::parse_episode`] on the path
//!   relative to the show folder, so season folders in any supported language work. A file with
//!   several numbers (`S02E05-E07`) is one [`ScannedEpisode`] listing all numbers. A folder named
//!   `Specials` makes season 0 when the name shows no other season. A file that nothing could be
//!   read from is kept with no season and no numbers, never dropped.
//! - **Episode files:** files in the same folder whose name continues the video's name (thumbnails,
//!   subtitles, NFO, `-mediainfo.xml`, `.vsmeta`, trailers, samples) join the episode, longest
//!   video name first. A poster or generic image there becomes the episode thumbnail. Other
//!   subtitles, NFOs, BIF files and similar are matched by the season and episode in their name;
//!   with several versions of one episode they must share the video's name or sit in a folder
//!   named after it.
//! - **Season files:** `season01-poster.jpg`, `season-specials-fanart.jpg`, `season-all-*.jpg`,
//!   `season01.nfo`, and poster, fanart, banner or `thumb` images inside a season folder are
//!   attached to the season found from the file name, then the folder name (`Season 2`,
//!   `Staffel 2`, `S02`, `Specials`, the first number). Season 0 is specials, -1 is all seasons.
//! - **Show files:** everything else that is not an unmatched unknown file stays with the show:
//!   `tvshow.nfo`, artwork, theme, trailers, extra fanart, extras.
//! - **Discs:** a disc folder is one episode; only its `.nfo` files are added.
//!
//! Episode NFOs take precedence over names for numbers and titles, but reading NFOs is a later
//! step: the walker only groups files. `.vsmeta` artwork is extracted under one rule for movies
//! and shows ([`WalkOptions::extract_vsmeta_artwork`]).
//!
//! # Threads
//!
//! The folder reader lists sub-folders on several threads ([`WalkOptions::threads`], one per core
//! by default) and joins them in name order, so the groups do not depend on the thread count.
//! The size and modification time of a file come with the directory listing where the platform
//! provides them (Windows), and the inode where it is free (Unix), which is what the incremental
//! rescan in [`crate::rescan`] compares.
//!
//! # Library folders inside library folders
//!
//! A movie data source may hold a TV data source (`D:/Media` for movies, `D:/Media/Shows` for
//! TV), and the other way round; the guard only refuses nesting inside one library. Each walk
//! leaves the other data sources inside its folder to their own library
//! ([`nested_sources`], [`WalkOptions::excluding`]), so an episode is never also a movie.
//!
//! # Safety
//!
//! A folder holding `.cinevaultignore` (or `.nomedia`, unless turned off) and everything below it
//! is left out. A source that is missing, empty or unreadable is reported in
//! [`ScanReport::problems`]; callers must not remove library rows because of it.

mod movies;
mod tree;
mod tv;

pub use tv::{ScannedEpisode, ScannedSeason, ScannedShow};

use std::path::{Path, PathBuf};

use cv_domain::{DataSource, Edition, MediaFileType, MediaSource, SourceKind};

use crate::parse::ParsedIds;
use crate::skip::{LibraryKind, SkipRules};

/// How a walk behaves.
#[derive(Debug, Clone)]
pub struct WalkOptions {
    /// Skip rules for movie libraries.
    pub skip: SkipRules,
    /// The user's bad words for title parsing (lower-case regular expressions).
    pub bad_words: Vec<String>,
    /// Extract artwork from `.vsmeta` files. Applies to movies and TV shows alike.
    pub extract_vsmeta_artwork: bool,
    /// Worker threads that read folders side by side. `0` picks one per core (at most 16);
    /// `1` reads on the calling thread.
    pub threads: usize,
}

impl WalkOptions {
    pub fn new(skip: SkipRules, bad_words: Vec<String>) -> Self {
        Self {
            skip,
            bad_words,
            extract_vsmeta_artwork: false,
            threads: 0,
        }
    }

    /// Sets the number of folder-reading threads (`0` = one per core, `1` = sequential).
    pub fn with_threads(mut self, threads: usize) -> Self {
        self.threads = threads;
        self
    }

    /// The thread count to use: the configured one, or one per core when it is `0`.
    pub fn worker_threads(&self) -> usize {
        if self.threads == 0 {
            crate::par::default_threads()
        } else {
            self.threads
        }
    }

    /// Default skip rules for TV libraries and no bad words.
    pub fn tv_defaults() -> Self {
        Self::new(SkipRules::with_defaults(LibraryKind::TvShows), Vec::new())
    }

    /// Turns `.vsmeta` artwork extraction on or off.
    pub fn with_vsmeta_artwork(mut self, extract: bool) -> Self {
        self.extract_vsmeta_artwork = extract;
        self
    }

    /// The same options, also leaving out these folders (full paths) and everything below
    /// them; see [`nested_sources`].
    #[must_use]
    pub fn excluding<S: AsRef<str>>(&self, folders: &[S]) -> Self {
        let mut options = self.clone();
        options.skip = options.skip.with_excluded_paths(folders);
        options
    }

    /// Default skip rules and no bad words.
    pub fn with_defaults() -> Self {
        Self::new(SkipRules::with_defaults(LibraryKind::Movies), Vec::new())
    }
}

impl Default for WalkOptions {
    fn default() -> Self {
        Self::with_defaults()
    }
}

/// One file or disc folder grouped to a movie.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedFile {
    pub path: PathBuf,
    /// Path relative to the data source, `/` separated.
    pub relative: String,
    /// The type after the companion rules ran.
    pub kind: MediaFileType,
    /// True for a disc folder (`BDMV`, `VIDEO_TS`, `HVDVD_TS`) used as the video.
    pub is_dir: bool,
    pub size_bytes: u64,
    /// Modification time, epoch milliseconds.
    pub modified_ms: i64,
    /// The file's identity (inode and device) where the platform gives it for free, else empty.
    /// A rescan uses it to tell a replaced file from an edited one.
    pub file_id: String,
    /// Part number within a stack, 0 when not stacked.
    pub stacking: u32,
    pub stacking_marker: String,
    /// The path is part of a DVD, HD-DVD or Blu-ray structure.
    pub disc_file: bool,
}

/// A movie found on disk with the files grouped to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedMovie {
    /// The data source's id when it came from the database.
    pub source_id: Option<i64>,
    pub source_root: PathBuf,
    /// The movie folder. For a movie in a multi-movie folder, the folder that holds the files.
    pub path: PathBuf,
    /// The folder relative to the data source, `/` separated; empty for the data source itself.
    pub relative_path: String,
    pub title: String,
    pub year: Option<u16>,
    /// IDs found in the folder or file names.
    pub ids: ParsedIds,
    pub edition: Edition,
    /// The source named in the path, or the disc type for a disc structure.
    pub media_source: MediaSource,
    pub three_d: bool,
    /// The folder holds other movies (or is named as a bucket).
    pub multi_movie_dir: bool,
    /// The movie is a disc structure.
    pub disc: bool,
    /// Videos first (in part order), then everything else by path.
    pub files: Vec<ScannedFile>,
}

impl ScannedMovie {
    /// The files of one type, in order.
    pub fn files_of(&self, kind: MediaFileType) -> impl Iterator<Item = &ScannedFile> {
        self.files.iter().filter(move |f| f.kind == kind)
    }
}

/// What went wrong on the way. None of these remove or change anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProblemKind {
    /// The data source does not exist or cannot be read. Its movies may be offline.
    SourceUnavailable,
    /// The data source has no entries at all; a disconnected drive looks like this.
    SourceEmpty,
    /// A disc folder sits directly in the data source root, so no movie folder can be derived.
    DiscFolderInRoot,
    /// A folder inside the source could not be read.
    Unreadable,
    /// A video sits directly in a TV data source, outside any show folder.
    EpisodeInRoot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanProblem {
    pub kind: ProblemKind,
    pub path: PathBuf,
    pub message: String,
}

/// The result of a walk.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanReport {
    pub movies: Vec<ScannedMovie>,
    pub problems: Vec<ScanProblem>,
}

impl ScanReport {
    /// True when the walk of `root` could not read the source itself, so the movies of that source
    /// must not be treated as removed.
    pub fn source_unreliable(&self, root: &Path) -> bool {
        self.problems.iter().any(|p| {
            p.path == root
                && matches!(
                    p.kind,
                    ProblemKind::SourceUnavailable | ProblemKind::SourceEmpty
                )
        })
    }
}

/// The result of a TV walk.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TvScanReport {
    pub shows: Vec<ScannedShow>,
    pub problems: Vec<ScanProblem>,
}

impl TvScanReport {
    /// True when the walk of `root` could not read the source itself, so the shows of that source
    /// must not be treated as removed.
    pub fn source_unreliable(&self, root: &Path) -> bool {
        self.problems.iter().any(|p| {
            p.path == root
                && matches!(
                    p.kind,
                    ProblemKind::SourceUnavailable | ProblemKind::SourceEmpty
                )
        })
    }
}

/// The other data sources (of either library) that lie inside `source`'s folder. A walk of
/// `source` leaves them out, so their files belong to their own library only.
pub fn nested_sources(source: &DataSource, all: &[DataSource]) -> Vec<String> {
    let fold = |path: &str| {
        let mut text = path.trim().replace('\\', "/").to_lowercase();
        while text.ends_with('/') {
            text.pop();
        }
        text
    };
    let root = format!("{}/", fold(&source.path));
    all.iter()
        .filter(|other| other.id != source.id)
        .filter(|other| fold(&other.path).starts_with(&root))
        .map(|other| other.path.clone())
        .collect()
}

/// Walks the movie data sources in position order (ties by id). TV sources are ignored, except
/// that a TV source inside a movie source is left out of that walk.
pub fn scan_movie_sources(sources: &[DataSource], options: &WalkOptions) -> ScanReport {
    let mut ordered: Vec<&DataSource> = sources
        .iter()
        .filter(|s| s.kind == SourceKind::Movie)
        .collect();
    ordered.sort_by_key(|s| (s.position, s.id));
    let mut report = ScanReport::default();
    for source in ordered {
        let options = options.excluding(&nested_sources(source, sources));
        let part = scan_movie_source(Path::new(&source.path), Some(source.id), &options);
        report.movies.extend(part.movies);
        report.problems.extend(part.problems);
    }
    report
}

/// Walks one movie data source. Movies come back sorted by folder, then title.
pub fn scan_movie_source(root: &Path, source_id: Option<i64>, options: &WalkOptions) -> ScanReport {
    let mut problems = Vec::new();
    let Some(tree) = tree::read_tree(root, &options.skip, options.worker_threads(), &mut problems)
    else {
        return ScanReport {
            movies: Vec::new(),
            problems,
        };
    };
    if tree.files.is_empty() && tree.dirs.is_empty() && tree.discs.is_empty() {
        problems.push(ScanProblem {
            kind: ProblemKind::SourceEmpty,
            path: root.to_path_buf(),
            message: "The folder is empty. If it is on a disconnected drive, reconnect it.".into(),
        });
    }
    let mut grouper = movies::Grouper::new(options, root, source_id, problems);
    grouper.walk_root(&tree);
    let mut movies = grouper.movies;
    movies.sort_by(|a, b| {
        (a.relative_path.to_lowercase(), a.title.to_lowercase())
            .cmp(&(b.relative_path.to_lowercase(), b.title.to_lowercase()))
    });
    ScanReport {
        movies,
        problems: grouper.problems,
    }
}

/// Walks the TV data sources in position order (ties by id). Movie sources are ignored, except
/// that a movie source inside a TV source is left out of that walk.
pub fn scan_tv_sources(sources: &[DataSource], options: &WalkOptions) -> TvScanReport {
    let mut ordered: Vec<&DataSource> = sources
        .iter()
        .filter(|s| s.kind == SourceKind::Tv)
        .collect();
    ordered.sort_by_key(|s| (s.position, s.id));
    let mut report = TvScanReport::default();
    for source in ordered {
        let options = options.excluding(&nested_sources(source, sources));
        let part = scan_tv_source(Path::new(&source.path), Some(source.id), &options);
        report.shows.extend(part.shows);
        report.problems.extend(part.problems);
    }
    report
}

/// Walks one TV data source. Shows come back sorted by folder, then title. Use
/// [`WalkOptions::tv_defaults`] for the TV skip rules.
pub fn scan_tv_source(root: &Path, source_id: Option<i64>, options: &WalkOptions) -> TvScanReport {
    let mut problems = Vec::new();
    let Some(tree) = tree::read_tree(root, &options.skip, options.worker_threads(), &mut problems)
    else {
        return TvScanReport {
            shows: Vec::new(),
            problems,
        };
    };
    if tree.files.is_empty() && tree.dirs.is_empty() && tree.discs.is_empty() {
        problems.push(ScanProblem {
            kind: ProblemKind::SourceEmpty,
            path: root.to_path_buf(),
            message: "The folder is empty. If it is on a disconnected drive, reconnect it.".into(),
        });
    }
    let mut grouper = tv::TvGrouper::new(options, root, source_id, problems);
    grouper.walk_root(&tree);
    let mut shows = grouper.shows;
    shows.sort_by(|a, b| {
        (a.relative_path.to_lowercase(), a.title.to_lowercase())
            .cmp(&(b.relative_path.to_lowercase(), b.title.to_lowercase()))
    });
    TvScanReport {
        shows,
        problems: grouper.problems,
    }
}

#[cfg(test)]
mod tests;
