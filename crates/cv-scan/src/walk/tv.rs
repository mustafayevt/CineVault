//! Groups the files of a TV data-source tree into shows, seasons and episodes.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use cv_domain::MediaFileType as T;
use regex::Regex;

use super::movies::{extension, is_sibling, normalized_base, parent_name, scanned_file_of, stem};
use super::tree::{Dir, Entry};
use super::{ProblemKind, ScanProblem, ScannedFile, WalkOptions};
use crate::classify::classify;
use crate::episode::{
    AiredDate, file_episode_title, is_season_folder_name, parse_absolute_episode, parse_episode,
    season_from_word,
};
use crate::parse::{ParsedIds, detect_clean_title_and_year_of_folder, detect_ids};
use crate::stacking::parse_file_name;

/// `season01.nfo`, `season-specials.nfo`; `season.nfo` has no number and is not a season NFO.
static SEASON_NFO: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^season(\d{2,}|-specials)?\.nfo$").expect("static season NFO pattern")
});

static SEASON_FOLDER_SHORT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^s[\s._-]?(\d{1,4})$").expect("static season folder pattern")
});

static FIRST_NUMBER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\d{1,4}").expect("static number pattern"));

static BIF_SUFFIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)-\d+-\d+\.bif$").expect("static BIF pattern"));

/// The files grouped to one season of a show: season artwork and the season NFO.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedSeason {
    /// The season number. 0 is specials, -1 is the artwork for all seasons (`season-all-*`).
    pub number: i32,
    pub files: Vec<ScannedFile>,
}

/// One video (all parts of a stack, or one disc folder) with the files that belong to it.
///
/// A file that holds several episodes (`S02E05-E07`) is one `ScannedEpisode` with
/// [`numbers`](Self::numbers) listing all three; the library layer creates one episode per number
/// and lets them share these files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedEpisode {
    /// The season. `None` when neither the name nor the folders say. Season 0 is specials; for a
    /// date-based file without a season the year.
    pub season: Option<u32>,
    /// The episode numbers; empty when none was found (the season may still be known).
    pub numbers: Vec<u32>,
    /// The aired date of a date-based file.
    pub date: Option<AiredDate>,
    /// The episode title that is left in the file name once the release group, the show name and
    /// the episode number are gone. Empty when the name carries no title.
    pub title: String,
    /// The episode is a disc structure (`VIDEO_TS`, `BDMV`, `HVDVD_TS`).
    pub disc: bool,
    /// Videos first (in part order), then everything else by path.
    pub files: Vec<ScannedFile>,
}

impl ScannedEpisode {
    /// The file holds more than one episode.
    pub fn is_multi_episode(&self) -> bool {
        self.numbers.len() > 1
    }

    /// Both a season and at least one episode number were found.
    pub fn is_numbered(&self) -> bool {
        self.season.is_some() && !self.numbers.is_empty()
    }

    /// The files of one type, in order.
    pub fn files_of(&self, kind: T) -> impl Iterator<Item = &ScannedFile> {
        self.files.iter().filter(move |f| f.kind == kind)
    }
}

/// A TV show found on disk with its seasons and episodes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedShow {
    /// The data source's id when it came from the database.
    pub source_id: Option<i64>,
    pub source_root: PathBuf,
    /// The show folder.
    pub path: PathBuf,
    /// The show folder relative to the data source, `/` separated.
    pub relative_path: String,
    /// The A to Z folder the show sits in, when it does.
    pub bucket: Option<String>,
    pub title: String,
    pub year: Option<u16>,
    /// IDs found in the folder name.
    pub ids: ParsedIds,
    /// Files of the show itself: `tvshow.nfo`, artwork, theme, trailers, extra fanart and
    /// anything that is not tied to a season or an episode.
    pub files: Vec<ScannedFile>,
    /// Seasons that have artwork or an NFO, by season number.
    pub seasons: Vec<ScannedSeason>,
    /// Episodes by season, then number, then path; unnumbered ones last.
    pub episodes: Vec<ScannedEpisode>,
}

impl ScannedShow {
    /// The files of the show itself, of one type.
    pub fn files_of(&self, kind: T) -> impl Iterator<Item = &ScannedFile> {
        self.files.iter().filter(move |f| f.kind == kind)
    }

    /// The season with this number, when it has files.
    pub fn season(&self, number: i32) -> Option<&ScannedSeason> {
        self.seasons.iter().find(|s| s.number == number)
    }

    /// Every season number that has episodes or files, ascending.
    pub fn season_numbers(&self) -> Vec<i32> {
        let mut numbers: Vec<i32> = self.seasons.iter().map(|s| s.number).collect();
        numbers.extend(
            self.episodes
                .iter()
                .filter_map(|e| e.season)
                .filter_map(|s| i32::try_from(s).ok()),
        );
        numbers.sort_unstable();
        numbers.dedup();
        numbers
    }

    /// The `.vsmeta` files artwork may be extracted from. Empty unless
    /// [`WalkOptions::extract_vsmeta_artwork`] is on. Movies and shows follow the same rule.
    pub fn vsmeta_artwork_sources(&self, options: &WalkOptions) -> Vec<&ScannedFile> {
        if !options.extract_vsmeta_artwork {
            return Vec::new();
        }
        self.files
            .iter()
            .chain(self.seasons.iter().flat_map(|s| s.files.iter()))
            .chain(self.episodes.iter().flat_map(|e| e.files.iter()))
            .filter(|f| f.kind == T::Vsmeta)
            .collect()
    }
}

/// An entry of a show with its classification and, once assigned, the episode it belongs to.
struct Item<'a> {
    entry: &'a Entry,
    /// Relative to the show folder, `/` separated.
    show_rel: String,
    /// Lower-case folder part of `show_rel`; empty for the show folder.
    dir_key: String,
    kind: T,
    disc_file: bool,
    owner: Option<usize>,
}

impl<'a> Item<'a> {
    fn new(entry: &'a Entry, show_name: &str, prefix: &str) -> Self {
        let show_rel = entry
            .rel
            .strip_prefix(prefix)
            .unwrap_or(&entry.rel)
            .to_owned();
        // The show folder is the first segment, as the classifier expects.
        let c = classify(&format!("{show_name}/{show_rel}"));
        let dir_key = show_rel
            .rsplit_once('/')
            .map_or(String::new(), |(dir, _)| dir.to_lowercase());
        Self {
            entry,
            show_rel,
            dir_key,
            kind: c.file_type,
            disc_file: c.disc_file,
            owner: None,
        }
    }

    fn is_plain_video(&self) -> bool {
        self.kind == T::Video && !self.entry.is_dir && !self.disc_file
    }

    fn is_disc_video(&self) -> bool {
        self.kind == T::Video && self.entry.is_dir
    }

    fn parent_folder(&self) -> Option<&str> {
        parent_name(&self.show_rel)
    }

    fn stem_lower(&self) -> String {
        stem(&self.entry.name).to_lowercase()
    }
}

/// One episode while the files are being assigned.
struct Group {
    videos: Vec<usize>,
    members: Vec<usize>,
    disc: bool,
    /// Lower-case names a companion's stem may continue.
    stems: Vec<String>,
}

pub(super) struct TvGrouper<'a> {
    options: &'a WalkOptions,
    root: &'a Path,
    source_id: Option<i64>,
    pub shows: Vec<ScannedShow>,
    pub problems: Vec<ScanProblem>,
}

impl<'a> TvGrouper<'a> {
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
            shows: Vec::new(),
            problems,
        }
    }

    /// Every folder below the root is a show, except a one-character folder (an A to Z bucket),
    /// whose folders are the shows. Videos directly in the root belong to no show.
    ///
    /// A library folder that is one show itself (it holds a season folder such as `Season 1`, or
    /// a `tvshow.nfo`) is read as that show, so its season folders do not become shows.
    pub fn walk_root(&mut self, root: &Dir) {
        if is_show_folder(root) {
            self.show(root, None);
            return;
        }
        for file in root
            .files
            .iter()
            .filter(|f| classify(&f.rel).file_type == T::Video)
        {
            self.problems.push(ScanProblem {
                kind: ProblemKind::EpisodeInRoot,
                path: file.path.clone(),
                message: "An episode sits directly in the library folder. Put it inside a show \
                          folder."
                    .into(),
            });
        }
        if root.discs.iter().any(|d| d.has_video) {
            self.problems.push(ScanProblem {
                kind: ProblemKind::DiscFolderInRoot,
                path: root.path.clone(),
                message: "A disc folder sits directly in the library folder. Put it inside a \
                          show folder."
                    .into(),
            });
        }
        for child in &root.dirs {
            if is_bucket(child) {
                for show in &child.dirs {
                    self.show(show, Some(&child.name));
                }
            } else {
                self.show(child, None);
            }
        }
    }

    fn show(&mut self, dir: &Dir, bucket: Option<&str>) {
        let mut entries = Vec::new();
        collect(dir, &mut entries);
        let prefix = if dir.rel.is_empty() {
            String::new()
        } else {
            format!("{}/", dir.rel)
        };
        let items: Vec<Item> = entries
            .into_iter()
            .map(|e| Item::new(e, &dir.name, &prefix))
            .collect();
        if !items
            .iter()
            .any(|i| i.is_plain_video() || i.is_disc_video())
        {
            // A folder without a video is not a show.
            return;
        }
        let bad = &self.options.bad_words;
        let clean = detect_clean_title_and_year_of_folder(&dir.name, bad);
        let title = if clean.title.is_empty() {
            dir.name.clone()
        } else {
            clean.title.clone()
        };
        let assembled = assemble(items, &title, bad);
        self.shows.push(ScannedShow {
            source_id: self.source_id,
            source_root: self.root.to_path_buf(),
            path: dir.path.clone(),
            relative_path: dir.rel.clone(),
            bucket: bucket.map(str::to_owned),
            title,
            year: clean.year,
            ids: detect_ids(&dir.name),
            files: assembled.files,
            seasons: assembled.seasons,
            episodes: assembled.episodes,
        });
    }
}

/// A folder that is one show rather than a folder of shows: a season folder sits directly in
/// it, or it holds the show NFO.
fn is_show_folder(dir: &Dir) -> bool {
    dir.dirs.iter().any(|d| is_season_folder_name(&d.name))
        || dir
            .files
            .iter()
            .any(|f| f.name.eq_ignore_ascii_case("tvshow.nfo"))
}

/// A one-character folder with no video, disc or show NFO of its own.
fn is_bucket(dir: &Dir) -> bool {
    dir.name.chars().count() == 1
        && !dir.discs.iter().any(|d| d.has_video)
        && !dir.files.iter().any(|f| {
            classify(&f.rel).file_type == T::Video || f.name.eq_ignore_ascii_case("tvshow.nfo")
        })
}

/// Every file below the show folder. A disc folder is added whole, with its `.nfo` files only.
fn collect<'d>(dir: &'d Dir, out: &mut Vec<&'d Entry>) {
    out.extend(dir.files.iter());
    for disc in dir.discs.iter().filter(|d| d.has_video) {
        out.push(&disc.dir);
        out.extend(disc.files.iter().filter(|f| extension(&f.name) == "nfo"));
    }
    for child in &dir.dirs {
        collect(child, out);
    }
}

struct Assembled {
    files: Vec<ScannedFile>,
    seasons: Vec<ScannedSeason>,
    episodes: Vec<ScannedEpisode>,
}

fn assemble(mut items: Vec<Item>, show_title: &str, bad_words: &[String]) -> Assembled {
    let mut groups = make_groups(&mut items);
    attach_companions(&mut items, &mut groups);

    // Season, episodes and date come from the path of the first part.
    let mut parsed: Vec<EpisodeInfo> = groups
        .iter()
        .map(|g| {
            let rep = &items[g.videos[0]];
            let found = parse_episode(&rep.show_rel, show_title, bad_words);
            let mut season = found.season;
            // A folder named Specials holds season 0, whatever number the default gave.
            if matches!(season, None | Some(1)) && in_specials_folder(&rep.show_rel) {
                season = Some(0);
            }
            EpisodeInfo {
                season,
                numbers: found.episodes,
                date: found.date,
                title: found.cleaned_title,
            }
        })
        .collect();
    let absolute = absolute_numbers(&items, &groups, &parsed, show_title, bad_words);
    if let Some(numbers) = &absolute {
        for (info, number) in parsed.iter_mut().zip(numbers) {
            if let Some(number) = number {
                info.season = Some(1);
                info.numbers = vec![*number];
            }
        }
    }
    let absolute = absolute.is_some();
    // With the numbers settled, the title drops the release group, the show name and the number.
    for (info, group) in parsed.iter_mut().zip(&groups) {
        let rep = &items[group.videos[0]];
        info.title = file_episode_title(
            &info.title,
            &rep.show_rel,
            show_title,
            info.season,
            &info.numbers,
        );
    }

    let mut season_files: BTreeMap<i32, Vec<usize>> = BTreeMap::new();
    let mut show_files: Vec<usize> = Vec::new();
    for (i, item) in items.iter_mut().enumerate() {
        if item.owner.is_some() {
            continue;
        }
        match place_leftover(item, &groups, &parsed, show_title, bad_words, absolute) {
            Place::Season(number, kind) => {
                item.kind = kind;
                season_files.entry(number).or_default().push(i);
            }
            Place::Episode(group) => {
                item.owner = Some(group);
                groups[group].members.push(i);
            }
            Place::Show => show_files.push(i),
        }
    }

    let mut episodes: Vec<ScannedEpisode> = groups
        .iter()
        .zip(parsed)
        .map(|(group, info)| build_episode(&items, group, info))
        .collect();
    episodes.sort_by_cached_key(|e| {
        (
            e.season.unwrap_or(u32::MAX),
            e.numbers.first().copied().unwrap_or(u32::MAX),
            e.files
                .first()
                .map(|f| f.relative.to_lowercase())
                .unwrap_or_default(),
        )
    });
    Assembled {
        files: scanned_list(&items, &show_files),
        seasons: season_files
            .into_iter()
            .map(|(number, indexes)| ScannedSeason {
                number,
                files: scanned_list(&items, &indexes),
            })
            .collect(),
        episodes,
    }
}

struct EpisodeInfo {
    season: Option<u32>,
    numbers: Vec<u32>,
    date: Option<AiredDate>,
    title: String,
}

fn scanned_list(items: &[Item], indexes: &[usize]) -> Vec<ScannedFile> {
    let mut files: Vec<ScannedFile> = indexes
        .iter()
        .map(|&i| scanned_file_of(items[i].entry, items[i].kind, items[i].disc_file))
        .collect();
    files.sort_by_cached_key(|f| f.relative.to_lowercase());
    files
}

fn build_episode(items: &[Item], group: &Group, info: EpisodeInfo) -> ScannedEpisode {
    let mut files: Vec<ScannedFile> = group
        .videos
        .iter()
        .chain(&group.members)
        .map(|&i| {
            let item = &items[i];
            // A poster or generic image next to an episode is its thumbnail.
            let kind = match item.kind {
                T::Poster | T::Graphic => T::Thumb,
                other => other,
            };
            scanned_file_of(item.entry, kind, item.disc_file)
        })
        .collect();
    files.sort_by_cached_key(|f| (f.kind != T::Video, f.stacking, f.relative.to_lowercase()));
    ScannedEpisode {
        season: info.season,
        numbers: info.numbers,
        date: info.date,
        title: info.title,
        disc: group.disc,
        files,
    }
}

/// One group per video file, per disc folder, and per stack of parts (same folder, same name
/// without the part marker).
fn make_groups(items: &mut [Item]) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    let mut stacks: HashMap<(String, String), usize> = HashMap::new();
    for (i, item) in items.iter_mut().enumerate() {
        let g = if item.is_disc_video() {
            groups.push(Group {
                videos: vec![i],
                members: Vec::new(),
                disc: true,
                stems: Vec::new(),
            });
            groups.len() - 1
        } else if item.is_plain_video() {
            let mut stems = vec![item.stem_lower()];
            let mut key = None;
            if parse_file_name(&item.entry.name).is_stacked() {
                let base = normalized_base(&item.entry.name).to_lowercase();
                stems.push(base.clone());
                key = Some((item.dir_key.clone(), base));
            }
            if let Some(&g) = key.as_ref().and_then(|k| stacks.get(k)) {
                groups[g].videos.push(i);
                groups[g].stems.extend(stems);
                g
            } else {
                if let Some(k) = key {
                    stacks.insert(k, groups.len());
                }
                groups.push(Group {
                    videos: vec![i],
                    members: Vec::new(),
                    disc: false,
                    stems,
                });
                groups.len() - 1
            }
        } else {
            continue;
        };
        item.owner = Some(g);
    }
    for group in &mut groups {
        // Parts in order.
        group.videos.sort_by_cached_key(|&i| {
            let file = scanned_file_of(items[i].entry, items[i].kind, items[i].disc_file);
            (file.stacking, items[i].show_rel.to_lowercase())
        });
    }
    groups
}

/// Files in the same folder whose name continues a video's name belong to its episode, the
/// longest matching video name first (`Show - 01 - Pilot.srt` beats `Show - 01`). Everything
/// collected inside a disc episode's folder belongs to it.
fn attach_companions(items: &mut [Item], groups: &mut [Group]) {
    let mut by_dir: HashMap<String, Vec<(String, usize)>> = HashMap::new();
    for (g, group) in groups.iter().enumerate() {
        if group.disc {
            continue;
        }
        let dir = items[group.videos[0]].dir_key.clone();
        for stem in &group.stems {
            by_dir
                .entry(dir.clone())
                .or_default()
                .push((stem.clone(), g));
        }
    }
    let disc_roots: Vec<(String, usize)> = groups
        .iter()
        .enumerate()
        .filter(|(_, g)| g.disc)
        .map(|(g, group)| (format!("{}/", items[group.videos[0]].entry.rel), g))
        .collect();

    for (i, item) in items.iter_mut().enumerate() {
        if item.owner.is_some() {
            continue;
        }
        let disc = disc_roots
            .iter()
            .find(|(root, _)| item.entry.rel.starts_with(root.as_str()))
            .map(|&(_, g)| g);
        let found = disc.or_else(|| {
            let stem = item.stem_lower();
            by_dir
                .get(&item.dir_key)?
                .iter()
                .filter(|(candidate, _)| is_sibling(&stem, candidate))
                .max_by_key(|(candidate, _)| candidate.chars().count())
                .map(|&(_, g)| g)
        });
        if let Some(g) = found {
            item.owner = Some(g);
            groups[g].members.push(i);
        }
    }
}

enum Place {
    Show,
    Season(i32, T),
    Episode(usize),
}

/// Where a file that matched no video by name goes.
fn place_leftover(
    item: &Item,
    groups: &[Group],
    parsed: &[EpisodeInfo],
    show_title: &str,
    bad_words: &[String],
    absolute: bool,
) -> Place {
    let name = item.entry.name.as_str();
    let lower = name.to_lowercase();
    if lower == "tvshow.nfo" {
        return Place::Show;
    }
    if item.kind == T::Nfo
        && let Some(c) = SEASON_NFO.captures(name)
    {
        return match c.get(1).map(|m| m.as_str().to_lowercase()) {
            Some(n) if n == "-specials" => Place::Season(0, T::Nfo),
            Some(n) => n
                .parse()
                .map_or(Place::Show, |number| Place::Season(number, T::Nfo)),
            None => Place::Show,
        };
    }
    let outside_root = item.show_rel.contains('/');
    let season_kind = match item.kind {
        T::SeasonPoster | T::SeasonFanart | T::SeasonBanner | T::SeasonThumb => Some(item.kind),
        // Show artwork inside a sub-folder is season artwork.
        T::Poster if outside_root => Some(T::SeasonPoster),
        T::Fanart if outside_root => Some(T::SeasonFanart),
        T::Banner if outside_root => Some(T::SeasonBanner),
        T::Thumb if outside_root && matches!(stem(&lower), "thumb" | "landscape") => {
            Some(T::SeasonThumb)
        }
        _ => None,
    };
    if let Some(kind) = season_kind {
        return match detect_season(name, item.parent_folder()) {
            Some(number) => Place::Season(number, kind),
            None => Place::Show,
        };
    }
    let attachable = matches!(
        item.kind,
        T::Subtitle
            | T::Audio
            | T::Bif
            | T::Vsmeta
            | T::Mediainfo
            | T::Nfo
            | T::Text
            | T::DoubleExt
            | T::Unknown
    );
    if !attachable {
        return Place::Show;
    }
    let path = if item.kind == T::Bif {
        BIF_SUFFIX.replace(&item.show_rel, ".bif").into_owned()
    } else {
        item.show_rel.clone()
    };
    let mut found = parse_episode(&path, show_title, bad_words);
    if absolute
        && !in_specials_folder(&path)
        && let Some(number) = parse_absolute_episode(&path, show_title, bad_words)
    {
        found.season = Some(1);
        found.episodes = vec![number];
    }
    let Some(season) = found.season else {
        return Place::Show;
    };
    if found.episodes.is_empty() {
        return Place::Show;
    }
    let candidates: Vec<usize> = (0..groups.len())
        .filter(|&g| parsed[g].season == Some(season) && parsed[g].numbers == found.episodes)
        .collect();
    match candidates.as_slice() {
        [] => Place::Show,
        [only] => Place::Episode(*only),
        several => {
            // Several versions of one episode: the file must continue a video's name, or sit in
            // a folder named after it.
            let file_stem = stem(&lower).to_owned();
            let folder = item.parent_folder().map(str::to_lowercase);
            several
                .iter()
                .copied()
                .find(|&g| {
                    groups[g].stems.iter().any(|s| {
                        file_stem.starts_with(s.as_str()) || folder.as_deref() == Some(s.as_str())
                    })
                })
                .map_or(Place::Show, Place::Episode)
        }
    }
}

/// The season of an artwork or NFO file from its name and folder: `season-specials` or a folder named `specials` is 0, `season-all` is -1, then a
/// season word and number in the file name, then in the folder name, then a short `S01` folder
/// name, then the first number in the folder name.
fn detect_season(file_name: &str, folder: Option<&str>) -> Option<i32> {
    let file = file_name.to_lowercase();
    let folder_lower = folder.map(str::to_lowercase);
    if file.starts_with("season-specials")
        || matches!(folder_lower.as_deref(), Some("specials" | "special"))
    {
        return Some(0);
    }
    if file.starts_with("season-all") {
        return Some(-1);
    }
    let number = |n: u32| i32::try_from(n).ok();
    if let Some(n) = season_from_word(&file).and_then(number) {
        return Some(n);
    }
    let folder = folder?;
    if let Some(n) = season_from_word(folder).and_then(number) {
        return Some(n);
    }
    if let Some(c) = SEASON_FOLDER_SHORT.captures(folder) {
        return c[1].parse().ok().and_then(number);
    }
    FIRST_NUMBER
        .find(folder)
        .and_then(|m| m.as_str().parse().ok())
        .and_then(number)
}

/// A folder between the show and the file is named Specials.
/// How many numbered files of one shape it takes to read a show in absolute order.
const ABSOLUTE_MIN_FILES: usize = 3;

/// The absolute episode numbers of a show whose videos are numbered from the first episode to
/// the last, as anime releases are (`[Group] Show - 001 [1080p].mkv`, `... - 100 ...`), or
/// `None` when the show is not named that way. One entry per group: the number, or `None` for a
/// video that keeps its own reading (a disc, a special, a name with a season marker).
///
/// The bare-number rule reads three digits as a season and an episode (`102` is S01E02), which
/// is right for most TV releases and wrong for these: `001` would be a special and `100` episode
/// 0 of season 1. A show is read in absolute order when at least [`ABSOLUTE_MIN_FILES`] of its
/// videos carry only an absolute number, those names are the same apart from their digits, the
/// numbers differ, and the season reading gives at least one of them an impossible number
/// (season 0 without a special marker, episode 0, or no episode at all).
fn absolute_numbers(
    items: &[Item],
    groups: &[Group],
    parsed: &[EpisodeInfo],
    show_title: &str,
    bad_words: &[String],
) -> Option<Vec<Option<u32>>> {
    static DIGITS: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"[0-9]+").expect("static digits pattern"));
    let mut numbers: Vec<Option<u32>> = vec![None; groups.len()];
    let mut shape: Option<String> = None;
    let mut seen: Vec<u32> = Vec::new();
    let mut implausible = false;
    for (index, group) in groups.iter().enumerate() {
        let rep = &items[group.videos[0]];
        if group.disc || in_specials_folder(&rep.show_rel) {
            continue;
        }
        let Some(number) = parse_absolute_episode(&rep.show_rel, show_title, bad_words) else {
            continue;
        };
        let name_shape = DIGITS.replace_all(&rep.stem_lower(), "#").into_owned();
        if *shape.get_or_insert_with(|| name_shape.clone()) != name_shape || seen.contains(&number)
        {
            return None;
        }
        seen.push(number);
        let info = &parsed[index];
        implausible |=
            info.season == Some(0) || info.numbers.is_empty() || info.numbers.contains(&0);
        numbers[index] = Some(number);
    }
    (seen.len() >= ABSOLUTE_MIN_FILES && implausible).then_some(numbers)
}

fn in_specials_folder(show_rel: &str) -> bool {
    let mut parts: Vec<&str> = show_rel.split('/').collect();
    parts.pop();
    parts
        .iter()
        .any(|p| p.eq_ignore_ascii_case("specials") || p.eq_ignore_ascii_case("special"))
}
