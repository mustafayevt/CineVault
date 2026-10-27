//! Planning the rename of one movie.
//!
//! [`MovieRenamer::plan`] turns a movie, its files and a naming profile into a [`MoviePlan`]: the
//! new folder, and what happens to every file. Notable choices are
//! noted below. The plan is data only; nothing is read from or written to disk.
//!
//! # Folder and base name
//!
//! The folder pattern is rendered under the data source root (`M/Matrix, The (1999)` is a folder
//! in a letter folder). With **keep sub-folders** on (the default), a pattern of one segment is
//! rendered inside the folder that holds the movie now instead, so `Movies/Amadeus` becomes
//! `Movies/Amadeus (1984)`; a movie in a folder shared with other movies gets its own folder inside
//! that folder. A pattern with `/` is always rendered under the root. A pattern that is not unique per movie (see
//! [`crate::pattern::is_folder_pattern_unique`]) makes the destination a **shared folder**: files
//! are named after the video only, because several movies live there. The file pattern gives
//! the **base name** of the video; a pattern that cannot name a file (see
//! [`crate::pattern::is_file_pattern_valid`]) keeps the current base name. Everything else is
//! named from the base name.
//!
//! # What happens to each kind of file
//!
//! | Kind | Destination |
//! |---|---|
//! | video | `<base>[ <stacking>].<ext>`; a disc structure keeps its names |
//! | NFO | the newest readable NFO goes to every NFO name of the profile; older ones are trashed; one that cannot be read stays (or is trashed with NFO clean-up) |
//! | poster, fanart, banner, clearart, thumb, clearlogo, disc art, key art, square art | the newest file goes to every name of the profile, older ones are trashed |
//! | extra fanart | the first name of the profile, the index of the old name kept |
//! | trailer | in a `trailers` folder: unchanged; otherwise every trailer name of the profile |
//! | sample, media-info, BIF, `.vsmeta` | `<base>-sample`, `<base>-mediainfo.xml`, `<base>[-W-H].bif`, `<base>.<video ext>.vsmeta` |
//! | extra | in a sub-folder: unchanged; otherwise `<base>` plus what followed the old base |
//! | subtitle | `<base>[ <stacking>].<language>[.forced][.sdh][.<title>].<ext>`; a `.sub` takes its `.idx` along |
//! | anything else | keeps its name and moves with the folder |
//!
//! Files that stay in the movie folder under another movie's name, such as a `poster.jpg` shared
//! by a shared folder, are never planned: a shared folder only uses names built on the video.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::LazyLock;

use cv_domain::{Item, ItemDetail, MediaFileType};
use cv_nfo::{MovieNameContext, MovieNfoNaming, movie_nfo_file_names};
use cv_template::tokens::{ModelOptions, MovieSetContext, movie_model};
use regex::{Regex, RegexBuilder};

use crate::clean::{Place, create_destination};
use crate::error::RenameError;
use crate::lang::language_tag;
use crate::naming::{ArtKind, NameRule, disc_rules, movie_rules, shared_folder_rules};
use crate::options::MovieSettings;
use crate::pattern::{is_file_pattern_valid, is_folder_pattern_unique, uses_original_filename};
use crate::plan::{
    Fate, FolderChange, Op, Owner, PlanFile, PlannedFile, Problem, duplicate_destinations,
    keep_subfolder, ops_of,
};
use crate::render::{Namer, with_written_certification};

/// What the planner needs to know about one movie.
#[derive(Debug, Clone, Copy)]
pub struct MovieInput<'a> {
    pub item: &'a Item,
    /// The collection the movie belongs to, for `${movieSet.*}` tokens.
    pub set: Option<&'a MovieSetContext>,
    /// The movie's current folder relative to the data source root; empty when the movie sits
    /// directly in the root.
    pub folder: &'a str,
    /// Every file of the movie, with its path relative to the data source root.
    pub files: &'a [PlanFile],
}

/// The result of planning one movie.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoviePlan {
    pub old_folder: String,
    /// The folder the movie ends up in, relative to the data source root, without a trailing
    /// `/`.
    pub new_folder: String,
    pub folder_change: FolderChange,
    /// The destination folder is shared with other movies.
    pub shared_folder: bool,
    /// The base name of the video files: no stacking marker and no extension.
    pub base_name: String,
    pub files: Vec<PlannedFile>,
    /// The operations to perform, in order. Files that stay where they are have none.
    pub ops: Vec<Op>,
    pub problems: Vec<Problem>,
}

impl MoviePlan {
    /// True when anything changes: the folder, or a file's path or presence.
    pub fn needs_rename(&self) -> bool {
        self.folder_change != FolderChange::Unchanged
            || self.files.iter().any(|f| !f.is_unchanged())
    }

    /// The destinations that more than one file would end at.
    pub fn duplicate_destinations(&self) -> Vec<String> {
        duplicate_destinations(&self.files)
    }
}

/// Renders names and plans renames for movies with one naming profile.
pub struct MovieRenamer {
    settings: MovieSettings,
    namer: Namer,
    model_options: ModelOptions,
    unwanted: Vec<Regex>,
}

impl MovieRenamer {
    pub fn new(settings: MovieSettings) -> MovieRenamer {
        let namer = Namer::movie(&settings.name);
        let unwanted = compile_patterns(&settings.unwanted_patterns);
        MovieRenamer {
            settings,
            namer,
            model_options: ModelOptions::default(),
            unwanted,
        }
    }

    pub fn settings(&self) -> &MovieSettings {
        &self.settings
    }

    /// Uses these title prefixes and rating source for the sortable title and `${rating}`.
    #[must_use]
    pub fn with_model_options(mut self, options: ModelOptions) -> MovieRenamer {
        self.model_options = options;
        self
    }

    /// Renders `template` and returns the raw text, before any clean-up.
    pub fn render_raw(
        &self,
        template: &str,
        item: &Item,
        set: Option<&MovieSetContext>,
    ) -> Result<String, RenameError> {
        let model =
            with_written_certification(movie_model(item, set, &self.model_options), "movie");
        Ok(self.namer.render(template, &model)?)
    }

    /// Renders a folder pattern and cleans the result.
    pub fn folder_name_for(
        &self,
        template: &str,
        item: &Item,
        set: Option<&MovieSetContext>,
    ) -> Result<String, RenameError> {
        let raw = self.render_raw(template, item, set)?;
        Ok(create_destination(
            &raw,
            Place::Folder,
            &self.settings.name,
            &self.settings.folder_space,
        ))
    }

    /// Renders a file pattern and cleans the result.
    pub fn file_name_for(
        &self,
        template: &str,
        item: &Item,
        set: Option<&MovieSetContext>,
    ) -> Result<String, RenameError> {
        let raw = self.render_raw(template, item, set)?;
        Ok(create_destination(
            &raw,
            Place::File,
            &self.settings.name,
            &self.settings.file_space,
        ))
    }

    /// The movie folder the profile gives, relative to the data source root.
    pub fn folder_name(
        &self,
        item: &Item,
        set: Option<&MovieSetContext>,
    ) -> Result<String, RenameError> {
        self.folder_name_for(&self.settings.folder_pattern, item, set)
    }

    /// The file base name the profile gives.
    pub fn file_name(
        &self,
        item: &Item,
        set: Option<&MovieSetContext>,
    ) -> Result<String, RenameError> {
        self.file_name_for(&self.settings.file_pattern, item, set)
    }

    /// Plans the rename of one movie.
    pub fn plan(&self, input: &MovieInput<'_>) -> Result<MoviePlan, RenameError> {
        let item = input.item;
        let ItemDetail::Movie(fields) = &item.detail else {
            return Err(RenameError::NotAMovie);
        };
        if item.core.title.trim().is_empty() {
            return Err(RenameError::NoTitle);
        }
        let s = &self.settings;
        let set = input
            .set
            .filter(|set| set.movies.len() > 1 || s.create_movieset_for_single_movie);
        let old_folder = input.folder.trim_matches('/').to_owned();
        let is_disc = fields.is_disc;

        // The folder.
        let rendered = if s.rename_folder {
            self.folder_name(item, set)?
        } else {
            String::new()
        };
        let rendered = rendered.trim_end_matches('/');
        let new_folder = if rendered.is_empty() {
            old_folder.clone()
        } else if s.keep_subfolders {
            keep_subfolder(rendered, &old_folder, fields.multi_movie_dir && !is_disc)
        } else {
            rendered.to_owned()
        };
        let shared_destination = !is_disc
            && if s.rename_folder && !rendered.is_empty() {
                !is_folder_pattern_unique(&s.folder_pattern)
            } else {
                fields.multi_movie_dir
            };

        // The base name.
        let mut videos: Vec<&PlanFile> = input
            .files
            .iter()
            .filter(|f| f.kind == MediaFileType::Video)
            .collect();
        videos.sort_by(|a, b| (a.stacking, &a.path).cmp(&(b.stacking, &b.path)));
        let old_base = videos
            .first()
            .map(|v| unstacked_stem(v))
            .unwrap_or_default();
        let video_ext = videos
            .first()
            .and_then(|v| extension(v.name()))
            .unwrap_or_default()
            .to_owned();
        let base = if is_disc || !s.rename_files || !is_file_pattern_valid(&s.file_pattern) {
            old_base.clone()
        } else {
            let mut name = self.file_name(item, set)?;
            if uses_original_filename(&s.file_pattern) {
                name = strip_extension(&name, &video_ext);
            }
            if name.is_empty() {
                old_base.clone()
            } else {
                name
            }
        };

        let ctx = Context {
            settings: s,
            files: input.files,
            old_folder: &old_folder,
            new_folder: &new_folder,
            base: &base,
            old_base: &old_base,
            video_ext: &video_ext,
            is_disc,
            shared: shared_destination,
            art_shared: shared_destination || (fields.multi_movie_dir && !is_disc),
            unwanted: &self.unwanted,
        };
        let files = ctx.plan_files();

        let left_behind = files.iter().any(|f| f.fate == Fate::LeftBehind);
        let folder_change = if new_folder == old_folder {
            FolderChange::Unchanged
        } else if old_folder.is_empty()
            || fields.multi_movie_dir
            || shared_destination
            || left_behind
        {
            FolderChange::MoveFiles
        } else {
            FolderChange::Rename
        };

        let mut problems: Vec<Problem> = files
            .iter()
            .zip(input.files)
            .filter(|(planned, file)| {
                file.kind == MediaFileType::Nfo
                    && !file.nfo_valid
                    && matches!(planned.fate, Fate::LeftBehind | Fate::Trash)
            })
            .map(|(_, file)| Problem::UnreadableNfo {
                path: file.path.clone(),
            })
            .collect();
        problems.extend(
            duplicate_destinations(&files)
                .into_iter()
                .map(|path| Problem::DuplicateDestination { path }),
        );

        Ok(MoviePlan {
            ops: ops_of(&files),
            old_folder,
            new_folder,
            folder_change,
            shared_folder: shared_destination,
            base_name: base,
            files,
            problems,
        })
    }
}

fn compile_patterns(patterns: &[String]) -> Vec<Regex> {
    patterns
        .iter()
        .filter_map(|p| RegexBuilder::new(p).case_insensitive(true).build().ok())
        .collect()
}

/// The extension of a file name without the dot.
fn extension(name: &str) -> Option<&str> {
    name.rsplit_once('.')
        .map(|(_, ext)| ext)
        .filter(|ext| !ext.is_empty())
}

/// The file name without its extension.
fn stem(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(stem, _)| stem)
}

fn strip_extension(name: &str, ext: &str) -> String {
    if ext.is_empty() {
        return name.to_owned();
    }
    let suffix = format!(".{}", ext.to_lowercase());
    if name.to_lowercase().ends_with(&suffix) {
        name[..name.len() - suffix.len()].to_owned()
    } else {
        name.to_owned()
    }
}

/// The stem of a stacked file without its stacking marker and the separator before it.
pub(crate) fn unstacked_stem(file: &PlanFile) -> String {
    let stem = stem(file.name());
    if file.stacking_marker.is_empty() {
        return stem.to_owned();
    }
    let lower = stem.to_lowercase();
    let marker = file.stacking_marker.to_lowercase();
    match lower.rfind(&marker) {
        Some(at) if stem.is_char_boundary(at) && stem.is_char_boundary(at + marker.len()) => {
            let before = stem[..at].trim_end_matches([' ', '_', '.', '-']);
            let after = &stem[at + marker.len()..];
            format!("{before}{after}")
        }
        _ => stem.to_owned(),
    }
}

fn join(folder: &str, rel: &str) -> String {
    if folder.is_empty() {
        rel.to_owned()
    } else {
        format!("{folder}/{rel}")
    }
}

fn art_kind(kind: MediaFileType) -> Option<ArtKind> {
    Some(match kind {
        MediaFileType::Poster => ArtKind::Poster,
        MediaFileType::Fanart => ArtKind::Fanart,
        MediaFileType::Banner => ArtKind::Banner,
        MediaFileType::Clearart => ArtKind::Clearart,
        MediaFileType::Thumb => ArtKind::Thumb,
        MediaFileType::Clearlogo => ArtKind::Clearlogo,
        MediaFileType::Disc => ArtKind::Discart,
        MediaFileType::Keyart => ArtKind::Keyart,
        MediaFileType::Squareart => ArtKind::Squareart,
        _ => return None,
    })
}

static BIF_SIZE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(-\d+-\d+)\.bif$").expect("valid regex"));
static TRAILING_NUMBER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\d+)$").expect("valid regex"));
static NUMBERED_TRAILER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)trailer[\[\]()_.-]?(\d+)$").expect("valid regex"));

/// Everything the per-file decisions share.
struct Context<'a> {
    settings: &'a MovieSettings,
    files: &'a [PlanFile],
    old_folder: &'a str,
    new_folder: &'a str,
    base: &'a str,
    old_base: &'a str,
    video_ext: &'a str,
    is_disc: bool,
    /// The destination folder is shared with other movies. Decides the NFO names.
    shared: bool,
    /// The movie is in a shared folder now or will be: artwork and trailers then only use the
    /// names built on the video name. A shared source folder counts even when the destination
    /// is not.
    art_shared: bool,
    unwanted: &'a [Regex],
}

impl Context<'_> {
    /// The path of a file in the new folder, keeping its place relative to the old folder.
    /// `None` when the file is not inside the old folder.
    fn path_only(&self, file: &PlanFile) -> Option<String> {
        let rel = if self.old_folder.is_empty() {
            Some(file.path.as_str())
        } else {
            file.path
                .strip_prefix(self.old_folder)
                .and_then(|rest| rest.strip_prefix('/'))
        }?;
        Some(join(self.new_folder, rel))
    }

    /// The folder of a file relative to the old movie folder; `""` for a file directly in it.
    /// `None` when the file is outside the old folder.
    fn rel_dir<'f>(&self, file: &'f PlanFile) -> Option<&'f str> {
        let parent = file.parent();
        if self.old_folder.is_empty() {
            Some(parent)
        } else if parent == self.old_folder {
            Some("")
        } else {
            parent
                .strip_prefix(self.old_folder)
                .and_then(|rest| rest.strip_prefix('/'))
        }
    }

    fn stack_string(&self, file: &PlanFile) -> String {
        let marker = if !file.stacking_marker.is_empty() {
            file.stacking_marker.clone()
        } else if file.stacking > 0 {
            format!("CD{}", file.stacking)
        } else {
            return String::new();
        };
        let space = &self.settings.file_space;
        let separator = if space.enabled {
            space.replacement.as_str()
        } else {
            " "
        };
        format!("{separator}{marker}")
    }

    fn rules(&self, kind: ArtKind) -> Vec<NameRule> {
        let s = self.settings;
        let choices = match kind {
            ArtKind::Poster => &s.poster,
            ArtKind::Fanart => &s.fanart,
            ArtKind::ExtraFanart => &s.extra_fanart,
            ArtKind::Banner => &s.banner,
            ArtKind::Clearart => &s.clearart,
            ArtKind::Thumb => &s.thumb,
            ArtKind::Clearlogo => &s.clearlogo,
            ArtKind::Discart => &s.discart,
            ArtKind::Keyart => &s.keyart,
            ArtKind::Squareart => &s.squareart,
            ArtKind::Trailer => &s.trailer,
        };
        let mut rules = movie_rules(kind, choices);
        if self.art_shared {
            rules = shared_folder_rules(kind, &rules);
        } else if self.is_disc {
            rules = disc_rules(&rules);
        }
        rules
    }

    fn nfo_names(&self) -> Vec<String> {
        let namings: Vec<MovieNfoNaming> = self
            .settings
            .nfo
            .iter()
            .filter_map(|name| MovieNfoNaming::parse(name))
            .collect();
        let disc_base = self.disc_base();
        let context = MovieNameContext {
            video_base_name: self.base.to_owned(),
            is_disc: self.is_disc,
            disc_base_name: disc_base,
            multi_movie_dir: self.shared,
            nfo_in_disc_folder: self.settings.disc_nfo_in_folder,
        };
        movie_nfo_file_names(&namings, &context)
    }

    /// The main file of a disc structure without extension (`VIDEO_TS`, `index`).
    fn disc_base(&self) -> String {
        const MAIN: [&str; 3] = ["video_ts.ifo", "index.bdmv", "hv000i01.ifo"];
        self.files
            .iter()
            .find(|f| {
                f.kind == MediaFileType::Video && MAIN.contains(&f.name().to_lowercase().as_str())
            })
            .map(|f| stem(f.name()).to_owned())
            .unwrap_or_else(|| self.base.to_owned())
    }

    fn plan_files(&self) -> Vec<PlannedFile> {
        // The newest file of each artwork kind and the newest readable NFO win.
        let mut winners: HashMap<MediaFileType, usize> = HashMap::new();
        for (index, file) in self.files.iter().enumerate() {
            let counts = art_kind(file.kind).is_some()
                || (file.kind == MediaFileType::Nfo && file.nfo_valid);
            if !counts {
                continue;
            }
            match winners.get(&file.kind) {
                Some(&current) if self.files[current].modified_ms >= file.modified_ms => {}
                _ => {
                    winners.insert(file.kind, index);
                }
            }
        }
        let trailers = self.loose_trailers();
        let extra_fanart = self.extra_fanart_targets();
        let companions = self.subtitle_companions();

        self.files
            .iter()
            .enumerate()
            .map(|(index, file)| {
                let fate = if let Some(sub) = companions.get(&index) {
                    self.idx_fate(&self.files[*sub])
                } else {
                    self.fate(file, index, &winners, &trailers, &extra_fanart)
                };
                PlannedFile {
                    from: file.path.clone(),
                    kind: file.kind,
                    owner: Owner::Movie,
                    fate,
                }
            })
            .collect()
    }

    /// `.idx` files that belong to a `.sub` subtitle, by index, with the subtitle's index.
    fn subtitle_companions(&self) -> HashMap<usize, usize> {
        let mut found = HashMap::new();
        for (sub_index, sub) in self.files.iter().enumerate() {
            if sub.kind != MediaFileType::Subtitle
                || !extension(sub.name()).is_some_and(|e| e.eq_ignore_ascii_case("sub"))
            {
                continue;
            }
            let wanted = format!("{}.idx", stem(sub.name())).to_lowercase();
            if let Some(idx) = self
                .files
                .iter()
                .position(|f| f.parent() == sub.parent() && f.name().to_lowercase() == wanted)
            {
                found.insert(idx, sub_index);
            }
        }
        found
    }

    fn idx_fate(&self, sub: &PlanFile) -> Fate {
        let name = self.subtitle_name(sub);
        let new_stem = stem(&name);
        Fate::Carried {
            to: vec![join(self.new_folder, &format!("{new_stem}.idx"))],
        }
    }

    fn subtitle_name(&self, file: &PlanFile) -> String {
        let mut name = format!("{}{}", self.base, self.stack_string(file));
        if let Some(tag) = &file.subtitle {
            let language = language_tag(&tag.language, self.settings.subtitle_language_style);
            if !language.is_empty() {
                name.push('.');
                name.push_str(&language);
            }
            if tag.forced {
                name.push_str(".forced");
            }
            if tag.sdh {
                name.push_str(".sdh");
            }
            if !tag.title.is_empty() {
                name.push('.');
                name.push_str(&tag.title);
            }
        }
        match extension(file.name()) {
            Some(ext) => format!("{name}.{ext}"),
            None => name,
        }
    }

    /// For trailers outside a `trailers` folder: the index of the file that gets the names of
    /// the profile, per trailer number. Other loose trailers with the same number are trashed.
    fn loose_trailers(&self) -> HashMap<usize, Option<u32>> {
        let mut best: BTreeMap<Option<u32>, usize> = BTreeMap::new();
        for (index, file) in self.files.iter().enumerate() {
            if file.kind != MediaFileType::Trailer || self.in_trailer_folder(file) {
                continue;
            }
            let number = trailer_number(file);
            match best.get(&number) {
                Some(&current) if self.files[current].modified_ms >= file.modified_ms => {}
                _ => {
                    best.insert(number, index);
                }
            }
        }
        best.into_iter()
            .map(|(number, index)| (index, number))
            .collect()
    }

    fn in_trailer_folder(&self, file: &PlanFile) -> bool {
        self.rel_dir(file).is_some_and(|dir| {
            dir.rsplit('/')
                .next()
                .is_some_and(|last| matches!(last.to_lowercase().as_str(), "trailer" | "trailers"))
        })
    }

    /// The index each extra fanart file keeps: the number in its name, or its place in the
    /// order of the names.
    fn extra_fanart_targets(&self) -> HashMap<usize, u32> {
        let mut indexed: Vec<usize> = (0..self.files.len())
            .filter(|&i| self.files[i].kind == MediaFileType::Extrafanart)
            .collect();
        indexed.sort_by(|&a, &b| self.files[a].path.cmp(&self.files[b].path));
        let mut used: HashSet<u32> = indexed
            .iter()
            .filter_map(|&i| trailing_number(self.files[i].name()))
            .collect();
        let mut next = 1_u32;
        let mut targets = HashMap::new();
        for index in indexed {
            let number = trailing_number(self.files[index].name()).unwrap_or_else(|| {
                while used.contains(&next) {
                    next += 1;
                }
                used.insert(next);
                next
            });
            targets.insert(index, number);
        }
        targets
    }

    fn fate(
        &self,
        file: &PlanFile,
        index: usize,
        winners: &HashMap<MediaFileType, usize>,
        trailers: &HashMap<usize, Option<u32>>,
        extra_fanart: &HashMap<usize, u32>,
    ) -> Fate {
        let keep = || match self.path_only(file) {
            Some(path) => Fate::Carried { to: vec![path] },
            None => Fate::Carried {
                to: vec![file.path.clone()],
            },
        };
        let ext = extension(file.name()).unwrap_or_default();
        let at = |name: String| Fate::Carried {
            to: vec![join(self.new_folder, &name)],
        };
        match file.kind {
            MediaFileType::Video => {
                if self.is_disc {
                    return keep();
                }
                at(match extension(file.name()) {
                    Some(ext) => format!("{}{}.{ext}", self.base, self.stack_string(file)),
                    None => format!("{}{}", self.base, self.stack_string(file)),
                })
            }
            MediaFileType::Nfo => self.nfo_fate(file, index, winners),
            MediaFileType::Poster
            | MediaFileType::Fanart
            | MediaFileType::Banner
            | MediaFileType::Clearart
            | MediaFileType::Thumb
            | MediaFileType::Clearlogo
            | MediaFileType::Disc
            | MediaFileType::Keyart
            | MediaFileType::Squareart => {
                let Some(kind) = art_kind(file.kind) else {
                    return keep();
                };
                let rules = self.rules(kind);
                if rules.is_empty() {
                    return self.unwanted_or(file, keep());
                }
                if winners.get(&file.kind) != Some(&index) {
                    return Fate::Trash;
                }
                self.carried_to(
                    file,
                    rules
                        .iter()
                        .map(|rule| join(self.new_folder, &rule.file_name(self.base, ext, None)))
                        .collect(),
                )
            }
            MediaFileType::Extrafanart => {
                let rules = self.rules(ArtKind::ExtraFanart);
                let (Some(rule), Some(number)) = (rules.first(), extra_fanart.get(&index)) else {
                    return keep();
                };
                at(rule.file_name(self.base, ext, Some(*number)))
            }
            MediaFileType::Trailer => {
                if self.in_trailer_folder(file) {
                    return keep();
                }
                let rules = self.rules(ArtKind::Trailer);
                if rules.is_empty() {
                    return keep();
                }
                let Some(number) = trailers.get(&index) else {
                    return Fate::Trash;
                };
                let names = rules
                    .iter()
                    .map(|rule| {
                        let name = match number {
                            Some(n) => numbered(&rule.file_name(self.base, "", None), *n, ext),
                            None => rule.file_name(self.base, ext, None),
                        };
                        join(self.new_folder, &name)
                    })
                    .collect();
                self.carried_to(file, names)
            }
            MediaFileType::Sample => at(format!("{}-sample.{ext}", self.base)),
            MediaFileType::Mediainfo => at(format!("{}-mediainfo.xml", self.base)),
            MediaFileType::Bif => {
                let size = BIF_SIZE
                    .captures(file.name())
                    .map(|c| c[1].to_owned())
                    .unwrap_or_default();
                at(format!("{}{size}.bif", self.base))
            }
            MediaFileType::Vsmeta | MediaFileType::DoubleExt => {
                if self.video_ext.is_empty() {
                    keep()
                } else {
                    at(format!("{}.{}.{ext}", self.base, self.video_ext))
                }
            }
            MediaFileType::Extra | MediaFileType::VideoExtra => {
                if self.rel_dir(file).is_none_or(|dir| !dir.is_empty()) {
                    return keep();
                }
                let name = file.name();
                let old = self.old_base;
                if !old.is_empty()
                    && name.len() > old.len()
                    && name.is_char_boundary(old.len())
                    && name[..old.len()].eq_ignore_ascii_case(old)
                {
                    at(format!("{}{}", self.base, &name[old.len()..]))
                } else {
                    keep()
                }
            }
            MediaFileType::Subtitle => at(self.subtitle_name(file)),
            _ => self.unwanted_or(file, keep()),
        }
    }

    /// The destinations of a file that goes to several names: the one the file already has
    /// first, so it is moved there and copied to the others.
    fn carried_to(&self, file: &PlanFile, mut names: Vec<String>) -> Fate {
        let mut seen = HashSet::new();
        names.retain(|n| seen.insert(n.clone()));
        if let Some(current) = self.path_only(file)
            && let Some(at) = names.iter().position(|n| *n == current)
        {
            names.swap(0, at);
        }
        Fate::Carried { to: names }
    }

    fn nfo_fate(
        &self,
        file: &PlanFile,
        index: usize,
        winners: &HashMap<MediaFileType, usize>,
    ) -> Fate {
        if !file.nfo_valid {
            return if self.settings.nfo_cleanup {
                Fate::Trash
            } else {
                Fate::LeftBehind
            };
        }
        if winners.get(&MediaFileType::Nfo) != Some(&index) {
            return Fate::Trash;
        }
        let names = self.nfo_names();
        if names.is_empty() {
            return Fate::Trash;
        }
        self.carried_to(
            file,
            names.iter().map(|n| join(self.new_folder, n)).collect(),
        )
    }

    /// Unwanted files (by pattern) are trashed when the profile asks for it.
    fn unwanted_or(&self, file: &PlanFile, otherwise: Fate) -> Fate {
        if self.settings.cleanup_unwanted && self.unwanted.iter().any(|re| re.is_match(file.name()))
        {
            Fate::Trash
        } else {
            otherwise
        }
    }
}

fn trailer_number(file: &PlanFile) -> Option<u32> {
    NUMBERED_TRAILER
        .captures(stem(file.name()))
        .and_then(|c| c[1].parse().ok())
        .filter(|n| *n > 0)
}

fn trailing_number(name: &str) -> Option<u32> {
    TRAILING_NUMBER
        .captures(stem(name))
        .and_then(|c| c[1].parse().ok())
}

/// `Heat-trailer` becomes `Heat-trailer.2.mp4`.
fn numbered(name: &str, number: u32, ext: &str) -> String {
    if ext.is_empty() {
        format!("{name}.{number}")
    } else {
        format!("{name}.{number}.{ext}")
    }
}

#[cfg(test)]
mod tests;
