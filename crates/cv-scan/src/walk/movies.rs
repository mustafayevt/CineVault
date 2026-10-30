//! Groups the files of a data-source tree into movies.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use cv_domain::{Edition, Genre, MediaFileType as T, MediaSource};
use regex::Regex;

use super::tree::{Dir, Entry};
use super::{ProblemKind, ScanProblem, ScannedFile, ScannedMovie, WalkOptions};
use crate::classify::classify;
pub(super) use crate::companion::{is_sibling, normalized_base, stem};
use crate::parse::{
    CleanTitle, ParsedIds, detect_clean_title_and_year, detect_clean_title_and_year_of_folder,
    detect_edition, detect_ids, detect_media_source,
};
use crate::stacking::{parse_file_name, parse_folder_name, stacking_number};

/// Folder names that bucket movies: one letter or digit, a year, a decade or a year range.
static BUCKET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?:[A-Za-z0-9_]|\d{4}|\d{4}s|\d{4}-\d{4})$").expect("static bucket pattern")
});

static THREE_D: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)[ .,_(\[\-]3D[ .,_)\]\-]?").expect("static 3D pattern"));

/// An entry with its classification; `kind` changes while the companion rules run.
#[derive(Clone)]
struct Cls<'a> {
    entry: &'a Entry,
    kind: T,
    disc_file: bool,
}

impl<'a> Cls<'a> {
    fn new(entry: &'a Entry) -> Self {
        let c = classify(&entry.rel);
        Self {
            entry,
            kind: c.file_type,
            disc_file: c.disc_file,
        }
    }

    fn is_plain_video(&self) -> bool {
        self.kind == T::Video && !self.entry.is_dir && !self.disc_file
    }
}

/// Everything [`Grouper::finish`] needs to build one movie.
struct Build<'a> {
    folder: &'a Dir,
    title: CleanTitle,
    edition: Edition,
    /// Names searched for provider IDs, most specific first.
    id_names: Vec<String>,
    /// Text searched for the media source.
    source_hint: String,
    three_d: bool,
    multi: bool,
    /// Lower-case names a companion's prefix may equal (video stems, with and without a part
    /// marker, and for a single folder the folder name).
    main_bases: Vec<String>,
    files: Vec<Cls<'a>>,
}

pub(super) struct Grouper<'a> {
    options: &'a WalkOptions,
    root: &'a Path,
    source_id: Option<i64>,
    pub movies: Vec<ScannedMovie>,
    pub problems: Vec<ScanProblem>,
}

impl<'a> Grouper<'a> {
    pub fn new(
        options: &'a WalkOptions,
        root: &'a Path,
        source_id: Option<i64>,
        problems: Vec<ScanProblem>,
    ) -> Self {
        Self {
            options,
            root,
            source_id,
            movies: Vec::new(),
            problems,
        }
    }

    /// The data-source root is always a multi-movie folder for the files directly in it.
    pub fn walk_root(&mut self, root: &Dir) {
        if root.discs.iter().any(|d| d.has_video) {
            self.problems.push(ScanProblem {
                kind: ProblemKind::DiscFolderInRoot,
                path: root.path.clone(),
                message: "A disc folder sits directly in the library folder. Put it inside a \
                          movie folder."
                    .into(),
            });
        }
        if !root.files.is_empty() {
            self.multi(root, root.files.iter().collect());
        }
        for child in &root.dirs {
            self.walk_dir(child);
        }
    }

    fn walk_dir(&mut self, dir: &Dir) {
        // A disc structure makes the folder above it one movie, whatever lies beside it.
        if has_video_disc(dir) {
            self.single(dir);
            return;
        }
        let parts = dir.dirs.iter().filter(|c| is_stack_part(c)).count();
        let own_video = dir
            .files
            .iter()
            .any(|f| classify(&f.rel).file_type == T::Video);
        if !own_video && parts == 0 {
            for child in &dir.dirs {
                self.walk_dir(child);
            }
            return;
        }
        let others = dir
            .dirs
            .iter()
            .filter(|c| !is_stack_part(c))
            .any(has_movie_dir);
        if others {
            // Other movies live below: this folder keeps only the files directly in it.
            self.multi(dir, dir.files.iter().collect());
            for child in dir.dirs.iter().filter(|c| !is_stack_part(c)) {
                self.walk_dir(child);
            }
            return;
        }
        let disc_file_here = dir.files.iter().any(|f| {
            let c = classify(&f.rel);
            c.file_type == T::Video && c.disc_file
        });
        if parts > 0 || disc_file_here || distinct_videos(dir) <= 1 {
            self.single(dir);
        } else {
            self.multi(dir, dir.files.iter().collect());
        }
    }

    /// A single-movie folder: every file below the folder belongs to the movie.
    fn single(&mut self, dir: &Dir) {
        let mut entries = Vec::new();
        collect(dir, &mut entries);
        // A bucket folder (A, 1999, 1990s, Horror) with one video is still a multi-movie folder.
        if is_bucket_name(&dir.name) {
            self.multi(dir, entries);
            return;
        }
        let files: Vec<Cls> = entries.into_iter().map(Cls::new).collect();
        let first_video = files.iter().find(|c| c.is_plain_video());
        let bad = &self.options.bad_words;
        let mut title = detect_clean_title_and_year_of_folder(&dir.name, bad);
        if title.title.is_empty() {
            if let Some(v) = first_video {
                title = detect_clean_title_and_year(&parse_file_name(&v.entry.name).cleaned, bad);
            }
            if title.title.is_empty() {
                title.title = dir.name.clone();
            }
        }
        let video_names: Vec<String> = files
            .iter()
            .filter(|c| c.is_plain_video())
            .map(|c| c.entry.name.clone())
            .collect();
        let mut id_names = vec![dir.name.clone()];
        id_names.extend(video_names.iter().cloned());
        let three_d =
            THREE_D.is_match(&dir.name) || video_names.first().is_some_and(|n| THREE_D.is_match(n));
        let mut main_bases = vec![dir.name.to_lowercase()];
        main_bases.extend(video_bases(files.iter()));
        let source_hint = match video_names.first() {
            Some(v) => format!("{}/{v}", dir.rel),
            None => dir.rel.clone(),
        };
        // The edition is named by the folder or, when the folder names none, by the video:
        // a library renamed with an edition in the file names only ("Mimic (1997)/Mimic -
        // Director's Cut (1997).mkv") keeps it.
        let edition = match detect_edition(&dir.name) {
            Edition::None => video_names
                .first()
                .map_or(Edition::None, |name| detect_edition(stem(name))),
            edition => edition,
        };
        let build = Build {
            folder: dir,
            edition,
            title,
            id_names,
            source_hint,
            three_d,
            multi: false,
            main_bases,
            files,
        };
        self.finish(build);
    }

    /// A multi-movie folder: one movie per distinct video, with the files named after it.
    fn multi(&mut self, dir: &Dir, entries: Vec<&Entry>) {
        let mut pool: Vec<Option<Cls>> = entries.into_iter().map(|e| Some(Cls::new(e))).collect();
        let mut order: Vec<usize> = (0..pool.len())
            .filter(|&i| pool[i].as_ref().is_some_and(Cls::is_plain_video))
            .collect();
        // Longest file name first, so overlapping names resolve to the longest match.
        order.sort_by(|&a, &b| {
            let (na, nb) = (name_of(&pool, a), name_of(&pool, b));
            nb.chars()
                .count()
                .cmp(&na.chars().count())
                .then_with(|| na.cmp(nb))
        });
        for i in order {
            let Some(first) = pool[i].as_ref() else {
                continue;
            };
            let base = normalized_base(&first.entry.name);
            let key = base.to_lowercase();
            let members: Vec<usize> = (0..pool.len())
                .filter(|&j| {
                    j == i
                        || pool[j]
                            .as_ref()
                            .is_some_and(|c| is_sibling(&stem(&c.entry.name).to_lowercase(), &key))
                })
                .collect();
            let mut files: Vec<Cls> = members.iter().filter_map(|&j| pool[j].take()).collect();
            files.sort_by_key(|c| c.entry.name.to_lowercase());
            let Some(video) = files.iter().find(|c| c.is_plain_video()) else {
                continue;
            };
            let video_name = video.entry.name.clone();
            let bad = &self.options.bad_words;
            let mut title = detect_clean_title_and_year(&parse_file_name(&video_name).cleaned, bad);
            if title.title.is_empty() {
                title.title = base.clone();
            }
            let mut main_bases = video_bases(files.iter());
            main_bases.push(key);
            let build = Build {
                folder: dir,
                edition: detect_edition(stem(&video_name)),
                title,
                id_names: files.iter().map(|c| c.entry.name.clone()).collect(),
                source_hint: join(&dir.rel, &video_name),
                three_d: THREE_D.is_match(&video_name),
                multi: true,
                main_bases,
                files,
            };
            self.finish(build);
        }
    }

    fn finish(&mut self, build: Build) {
        let Build {
            folder,
            title,
            edition,
            id_names,
            source_hint,
            three_d,
            multi,
            main_bases,
            files,
        } = build;
        let disc = files.iter().any(|c| c.kind == T::Video && c.disc_file);
        let mut files = apply_companion_rules(files, &main_bases, &title.title.to_lowercase());
        if disc {
            // Only the disc folder and its identifier files stand for the video of a disc movie.
            files.retain(|c| c.kind != T::Video || c.entry.is_dir || c.disc_file);
        }
        if !files.iter().any(|c| c.kind == T::Video) {
            return;
        }
        // File names inside a disc structure (`VIDEO_TS`, `AUDIO_TS`) would read as a source.
        let hint = if disc { &folder.rel } else { &source_hint };
        let mut media_source = detect_media_source(hint);
        if media_source == MediaSource::Unknown && disc {
            media_source = disc_source(&files);
        }
        let mut ids = ParsedIds::default();
        for name in &id_names {
            let found = detect_ids(name);
            ids.imdb = ids.imdb.or(found.imdb);
            ids.tmdb = ids.tmdb.or(found.tmdb);
            ids.tvdb = ids.tvdb.or(found.tvdb);
        }
        let mut scanned: Vec<ScannedFile> = files.iter().map(scanned_file).collect();
        scanned.sort_by_cached_key(|f| (f.kind != T::Video, f.stacking, f.relative.to_lowercase()));
        self.movies.push(ScannedMovie {
            source_id: self.source_id,
            source_root: self.root.to_path_buf(),
            path: folder.path.clone(),
            relative_path: folder.rel.clone(),
            title: title.title,
            year: title.year,
            ids,
            edition,
            media_source,
            three_d,
            multi_movie_dir: multi,
            disc,
            files: scanned,
        });
    }
}

fn name_of<'p>(pool: &'p [Option<Cls>], i: usize) -> &'p str {
    pool[i].as_ref().map_or("", |c| c.entry.name.as_str())
}

fn is_bucket_name(name: &str) -> bool {
    BUCKET.is_match(name) || Genre::parse(name).is_some_and(|g| g.is_known())
}

/// The folder holds a disc structure with a video file in it.
fn has_video_disc(dir: &Dir) -> bool {
    dir.discs.iter().any(|d| d.has_video)
}

/// The folder has a video of its own (or a disc structure).
fn is_video_folder(dir: &Dir) -> bool {
    has_video_disc(dir)
        || dir
            .files
            .iter()
            .any(|f| classify(&f.rel).file_type == T::Video)
}

/// A folder named only by a part marker (`CD1`) with a video in it: part of its parent's movie.
fn is_stack_part(dir: &Dir) -> bool {
    is_video_folder(dir) && {
        let f = parse_folder_name(&dir.name);
        f.is_stacked() && f.cleaned.trim().is_empty()
    }
}

/// The folder or something below it is a movie folder.
fn has_movie_dir(dir: &Dir) -> bool {
    is_video_folder(dir) || dir.dirs.iter().any(has_movie_dir)
}

fn distinct_videos(dir: &Dir) -> usize {
    dir.files
        .iter()
        .filter(|f| classify(&f.rel).file_type == T::Video)
        .map(|f| normalized_base(&f.name).to_lowercase())
        .collect::<HashSet<_>>()
        .len()
}

/// Every file below the folder. Disc folders are added whole, with only their `.nfo` files,
/// trailers and identifier file.
fn collect<'d>(dir: &'d Dir, out: &mut Vec<&'d Entry>) {
    out.extend(dir.files.iter());
    for disc in dir.discs.iter().filter(|d| d.has_video) {
        out.push(&disc.dir);
        out.extend(disc.files.iter().filter(|f| keep_disc_file(f)));
    }
    for child in &dir.dirs {
        collect(child, out);
    }
}

fn keep_disc_file(file: &Entry) -> bool {
    let lower = file.name.to_lowercase();
    extension(&lower) == "nfo"
        || stem(&lower).ends_with("trailer")
        || ["video_ts.ifo", "index.bdmv", "hv000i01.ifo"].contains(&lower.as_str())
}

pub(super) fn extension(name: &str) -> String {
    match name.rfind('.') {
        Some(i) if i > 0 => name[i + 1..].to_lowercase(),
        _ => String::new(),
    }
}

pub(super) fn join(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_owned()
    } else {
        format!("{parent}/{name}")
    }
}

/// Lower-case stems of the videos, raw and without a part marker.
fn video_bases<'c, 'e: 'c>(files: impl Iterator<Item = &'c Cls<'e>>) -> Vec<String> {
    let mut out = Vec::new();
    for c in files.filter(|c| c.is_plain_video()) {
        out.push(stem(&c.entry.name).to_lowercase());
        out.push(normalized_base(&c.entry.name).to_lowercase());
    }
    out
}

/// The artwork companion rules, applied to the files of one movie.
fn apply_companion_rules<'a>(
    mut files: Vec<Cls<'a>>,
    main_bases: &[String],
    title_lower: &str,
) -> Vec<Cls<'a>> {
    let video_names: Vec<String> = files
        .iter()
        .filter(|c| c.kind == T::Video && !c.entry.is_dir)
        .map(|c| c.entry.name.to_lowercase())
        .collect();
    for c in &mut files {
        let lower = c.entry.name.to_lowercase();
        match c.kind {
            T::Unknown
                if video_names
                    .iter()
                    .any(|v| lower.len() > v.len() && lower.starts_with(v.as_str())) =>
            {
                c.kind = T::DoubleExt;
            }
            T::Graphic => {
                let s = stem(&lower);
                if s == title_lower || main_bases.iter().any(|b| b == s) {
                    c.kind = T::Poster;
                }
            }
            _ => {}
        }
    }

    let mut kept: Vec<Cls<'a>> = Vec::new();
    let mut graphics: Vec<Cls<'a>> = Vec::new();
    for mut c in files {
        match c.kind {
            T::Video
            | T::Subtitle
            | T::Trailer
            | T::Extra
            | T::VideoExtra
            | T::Sample
            | T::Nfo
            | T::Text
            | T::Extrafanart
            | T::Extrathumb
            | T::Audio
            | T::Mediainfo
            | T::Vsmeta
            | T::Theme
            | T::Characterart
            | T::DoubleExt
            | T::Bif => kept.push(c),
            T::Fanart | T::Thumb if in_extra_art_folder(&c.entry.rel) => {}
            T::Poster
            | T::Fanart
            | T::Banner
            | T::Clearart
            | T::Clearlogo
            | T::Keyart
            | T::Squareart
            | T::Thumb
            | T::Disc => {
                if has_invalid_basename(&c, main_bases) {
                    c.kind = T::Graphic;
                    graphics.push(c);
                } else {
                    kept.push(c);
                }
            }
            T::Graphic => graphics.push(c),
            // Unknown files and season artwork do not belong to a movie.
            _ => {}
        }
    }

    // A downgraded graphic is promoted to the first artwork type the movie still lacks.
    for wanted in [
        T::Fanart,
        T::Poster,
        T::Banner,
        T::Clearlogo,
        T::Clearart,
        T::Keyart,
        T::Disc,
    ] {
        if kept.iter().any(|c| c.kind == wanted) {
            continue;
        }
        if let Some(at) = graphics
            .iter()
            .position(|g| classify(&g.entry.rel).file_type == wanted)
        {
            let mut g = graphics.remove(at);
            g.kind = wanted;
            kept.push(g);
        }
    }
    kept
}

fn in_extra_art_folder(rel: &str) -> bool {
    let lower = rel.to_lowercase();
    lower.contains("/extrafanart/") || lower.contains("/extrathumbs/")
}

/// Artwork named `<prefix>-<type>` whose prefix is not the name of the movie's video.
fn has_invalid_basename(c: &Cls, main_bases: &[String]) -> bool {
    let words: &[&str] = match c.kind {
        T::Poster => &["poster"],
        T::Fanart => &["fanart", "backdrop", "background"],
        T::Banner => &["banner"],
        T::Clearlogo => &["clearlogo", "logo"],
        T::Clearart => &["clearart"],
        T::Keyart => &["keyart"],
        T::Squareart => &["squareart", "square", "backgroundsquare"],
        T::Thumb => &["thumb", "landscape"],
        T::Disc => &["discart", "disc"],
        _ => return false,
    };
    let lower = stem(&c.entry.name).to_lowercase();
    let Some((prefix, word)) = lower.rsplit_once('-') else {
        return false;
    };
    words.contains(&word) && !main_bases.iter().any(|b| b == prefix)
}

/// The disc type named by the disc folders of a movie.
fn disc_source(files: &[Cls]) -> MediaSource {
    for c in files.iter().filter(|c| c.entry.is_dir) {
        let upper = c.entry.name.to_uppercase();
        if upper.ends_with("BDMV") {
            return MediaSource::Bluray;
        }
        if upper.ends_with("HVDVD_TS") {
            return MediaSource::HdDvd;
        }
        if upper.ends_with("VIDEO_TS") {
            return MediaSource::Dvd;
        }
    }
    MediaSource::Unknown
}

fn scanned_file(c: &Cls) -> ScannedFile {
    scanned_file_of(c.entry, c.kind, c.disc_file)
}

/// The scanned file for an entry with its final type. A video gets its part number from a marker
/// in its name or in the folder that holds it (`Movie/CD2`).
pub(super) fn scanned_file_of(entry: &Entry, kind: T, disc_file: bool) -> ScannedFile {
    let mut marker = String::new();
    let mut number = 0;
    if kind == T::Video && !entry.is_dir {
        let own = parse_file_name(&entry.name);
        if own.is_stacked() {
            marker = own.marker;
            number = own.number;
        } else if let Some(parent) = parent_name(&entry.rel) {
            let folder = parse_folder_name(parent);
            if folder.is_stacked() && folder.cleaned.trim().is_empty() {
                number = stacking_number(&folder.marker);
                marker = folder.marker;
            }
        }
    }
    ScannedFile {
        path: PathBuf::from(&entry.path),
        relative: entry.rel.clone(),
        kind,
        is_dir: entry.is_dir,
        size_bytes: entry.size,
        modified_ms: entry.modified_ms,
        created_ms: entry.created_ms,
        file_id: entry.file_id.clone(),
        stacking: number,
        stacking_marker: marker,
        disc_file,
    }
}

pub(super) fn parent_name(rel: &str) -> Option<&str> {
    let mut parts = rel.rsplit('/');
    parts.next()?;
    parts.next()
}
