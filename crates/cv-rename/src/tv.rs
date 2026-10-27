//! Planning the rename of a TV show with its seasons and episodes.
//!
//! [`TvRenamer::plan`] turns a show, the files of the show, its seasons and its episodes, and a
//! naming profile into a [`TvPlan`].
//!
//! # Folders
//!
//! The show folder is rendered under the data source root; with **keep sub-folders** on (the
//! default) a pattern of one segment is rendered inside the folder that holds the show now
//! (`TV/Anime/Frieren` becomes `TV/Anime/Frieren (2023)`). Each episode goes to a season folder
//! inside it, rendered from the season pattern:
//!
//! - season 0 is the folder `Specials` when the profile asks for it, whatever the pattern says;
//! - a pattern that renders nothing puts the episode in the show folder, if the pair of season
//!   and file pattern is *recommended* (see [`TvRenamer::is_recommended`]); otherwise the folder
//!   is `Season N`.
//!
//! # Episode files
//!
//! Episodes that share a video file (a file with S01E02 and S01E03) are one **group**. The file
//! pattern is rendered for the group: the season and episode part is repeated (`S01E02 S01E03`)
//! or made a range (`S01E02-E03`) according to the profile, and titles and aired dates are
//! joined with ` - `. Everything that belongs to the video (NFO, thumbnail, subtitles, trailer,
//! sample, media info) is named after it and put in the season folder. Show and season files
//! (artwork, NFOs) are named by the profile; any other file keeps its name and moves along.
//!
//! A file pattern without an episode token is refused, because it would give every episode of a
//! season the same name. An episode without a season and episode number (a bonus file the scan
//! found no `SxxEyy` in) cannot be named: its files keep their names and their place in the show
//! folder.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use cv_domain::{EpisodeFields, EpisodeGroup, Item, ItemDetail, ItemId, MediaFileType, ShowFields};
use cv_nfo::{
    EpisodeNfoNaming, SeasonNfoNaming, ShowNfoNaming, season_nfo_paths, show_nfo_file_names,
};
use cv_template::tokens::{ModelOptions, tv_model, tv_validator};
use cv_template::{DiagnosticKind, ValidationReport, Value};
use regex::{Regex, RegexBuilder};

use crate::clean::{Place, create_destination};
use crate::error::RenameError;
use crate::lang::language_tag;
use crate::multi::{Fragment, episode_token_count, find_fragment, keeps_original_name};
use crate::naming::{SeasonArt, ShowArt, episode_thumb_name, season_art_path, show_stem};
use crate::options::{MultiEpisodeStyle, TvSettings};
use crate::pattern::uses_original_filename;
use crate::plan::{
    Fate, FolderChange, Op, Owner, PlanFile, PlannedFile, Problem, duplicate_destinations,
    keep_subfolder, ops_of,
};
use crate::render::{Namer, with_written_certification};

/// The files of one season (artwork and NFO named by season).
#[derive(Debug, Clone)]
pub struct SeasonInput {
    /// `0` is the specials season.
    pub season: u32,
    pub files: Vec<PlanFile>,
}

/// One episode and its files. Episodes of a multi-episode file list the same video file.
#[derive(Debug, Clone, Copy)]
pub struct EpisodeInput<'a> {
    pub item: &'a Item,
    pub files: &'a [PlanFile],
}

/// What the planner needs to know about one show.
#[derive(Debug, Clone, Copy)]
pub struct TvInput<'a> {
    pub show: &'a Item,
    /// The show's current folder relative to the data source root.
    pub folder: &'a str,
    /// The files of the show itself: artwork, NFO, theme, trailers.
    pub files: &'a [PlanFile],
    pub seasons: &'a [SeasonInput],
    pub episodes: &'a [EpisodeInput<'a>],
}

/// The result of planning one show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TvPlan {
    pub old_folder: String,
    /// The show folder after the rename, relative to the data source root.
    pub new_folder: String,
    pub folder_change: FolderChange,
    pub files: Vec<PlannedFile>,
    pub ops: Vec<Op>,
    pub problems: Vec<Problem>,
}

impl TvPlan {
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

/// Renders names and plans renames for TV shows with one naming profile.
pub struct TvRenamer {
    settings: TvSettings,
    namer: Namer,
    model_options: ModelOptions,
    unwanted: Vec<Regex>,
}

/// Stands in for the fragment of a multi-episode pattern while the rest is rendered.
const FRAGMENT_MARK: char = '\u{E000}';

impl TvRenamer {
    pub fn new(settings: TvSettings) -> TvRenamer {
        let namer = Namer::tv(&settings.name);
        let unwanted = settings
            .unwanted_patterns
            .iter()
            .filter_map(|p| RegexBuilder::new(p).case_insensitive(true).build().ok())
            .collect();
        TvRenamer {
            settings,
            namer,
            model_options: ModelOptions::default(),
            unwanted,
        }
    }

    pub fn settings(&self) -> &TvSettings {
        &self.settings
    }

    /// Uses these title prefixes and rating source for the sortable titles and ratings.
    #[must_use]
    pub fn with_model_options(mut self, options: ModelOptions) -> TvRenamer {
        self.model_options = options;
        self
    }

    /// Whether the season pattern and the file pattern of the profile are a good pair: one
    /// episode token, a season token before it, at most one title. A pair that is not
    /// recommended gets `Season N` folders when the season pattern renders nothing.
    pub fn is_recommended(&self) -> bool {
        self.is_recommended_for(
            Some(&self.settings.season_pattern),
            &self.settings.file_pattern,
        )
    }

    /// [`TvRenamer::is_recommended`] for other patterns.
    ///
    /// The title count is advice for the pattern editor and does not count here: a pattern with
    /// two title tokens still names each episode, so it keeps the empty season folder.
    pub fn is_recommended_for(&self, season_pattern: Option<&str>, file_pattern: &str) -> bool {
        let report =
            tv_validator(self.namer.engine()).check_tv_patterns(season_pattern, file_pattern);
        let blocks = |report: &ValidationReport| {
            report
                .diagnostics
                .iter()
                .any(|d| d.kind != DiagnosticKind::TvTitleToken)
        };
        !blocks(&report.file) && report.season.as_ref().is_none_or(|season| !blocks(season))
    }

    // ---- rendering ---------------------------------------------------------------------------

    /// Renders `template` for one episode of `show` and returns the raw text.
    pub fn render_raw(
        &self,
        template: &str,
        show: &Item,
        episode: &Item,
    ) -> Result<String, RenameError> {
        let model = self.model(show, episode);
        Ok(self.namer.render(template, &model)?)
    }

    fn model(&self, show: &Item, episode: &Item) -> Value {
        let model = if title_for_name(episode).is_empty() && !episode.core.title.is_empty() {
            let mut untitled = episode.clone();
            untitled.core.title.clear();
            tv_model(show, &untitled, &self.model_options)
        } else {
            tv_model(show, episode, &self.model_options)
        };
        with_written_certification(model, "tvShow")
    }

    /// Renders a show folder pattern and cleans the result.
    pub fn show_folder_name_for(&self, template: &str, show: &Item) -> Result<String, RenameError> {
        let episode = placeholder_episode(show);
        let raw = self.render_raw(template, show, &episode)?;
        Ok(create_destination(
            &raw,
            Place::Folder,
            &self.settings.name,
            &self.settings.show_space,
        ))
    }

    /// The show folder the profile gives.
    pub fn show_folder_name(&self, show: &Item) -> Result<String, RenameError> {
        self.show_folder_name_for(&self.settings.show_pattern, show)
    }

    /// Renders a season folder pattern for the season of `episode` and cleans the result. Season 0
    /// is `Specials` when the profile asks for it, whatever the pattern says. An empty result is
    /// not replaced here; see [`TvRenamer::plan`] for the `Season N` fallback.
    pub fn season_folder_name_for(
        &self,
        template: &str,
        show: &Item,
        episode: &Item,
    ) -> Result<String, RenameError> {
        if self.settings.special_season && season_number(show, episode) == Some(0) {
            return Ok("Specials".to_owned());
        }
        let raw = self.render_raw(template, show, episode)?;
        Ok(create_destination(
            &raw,
            Place::Folder,
            &self.settings.name,
            &self.settings.season_space,
        ))
    }

    /// The season folder the profile gives for the season of `episode`.
    pub fn season_folder_name(&self, show: &Item, episode: &Item) -> Result<String, RenameError> {
        self.season_folder_name_for(&self.settings.season_pattern, show, episode)
    }

    /// Renders a file pattern for the episodes of one file and cleans the result. `episodes` are
    /// the episodes of the file in order; the first one gives the season and the media info.
    pub fn episode_file_name_for(
        &self,
        template: &str,
        show: &Item,
        episodes: &[&Item],
    ) -> Result<String, RenameError> {
        let raw = self.render_episodes(template, show, episodes)?;
        Ok(create_destination(
            &raw,
            Place::File,
            &self.settings.name,
            &self.settings.file_space,
        ))
    }

    /// The file name (without extension) the profile gives for the episodes of one file.
    pub fn episode_file_name(
        &self,
        show: &Item,
        episodes: &[&Item],
    ) -> Result<String, RenameError> {
        self.episode_file_name_for(&self.settings.file_pattern, show, episodes)
    }

    /// Renders a pattern for one or more episodes, raw. See the module documentation.
    pub fn render_episodes(
        &self,
        template: &str,
        show: &Item,
        episodes: &[&Item],
    ) -> Result<String, RenameError> {
        let Some(first) = episodes.first() else {
            return Err(RenameError::NoTitle);
        };
        if episodes.len() == 1 {
            return self.render_raw(template, show, first);
        }
        let mut morphed = self.namer.morph(template);
        let base = self.model(show, first);
        let model = joined_model(&base, episodes);
        morphed = self.join_aired_dates(&morphed, show, episodes)?;

        let fragment = find_fragment(&morphed);
        let Some(fragment) = fragment else {
            return Ok(self.namer.render(&morphed, &model)?);
        };
        let marked = format!(
            "{}{FRAGMENT_MARK}{}",
            &morphed[..fragment.range.start],
            &morphed[fragment.range.end..]
        );
        let outer = self.namer.render(&marked, &model)?;
        let joined = self.render_fragment(&fragment, &morphed, show, episodes)?;
        Ok(outer.replacen(FRAGMENT_MARK, &joined, 1))
    }

    fn render_fragment(
        &self,
        fragment: &Fragment,
        morphed: &str,
        show: &Item,
        episodes: &[&Item],
    ) -> Result<String, RenameError> {
        let text = fragment.text(morphed);
        let render_one = |text: &str, episode: &Item| -> Result<String, RenameError> {
            let model = self.model(show, episode);
            Ok(self.namer.render(text, &model)?)
        };
        let repeat = || -> Result<String, RenameError> {
            let parts = episodes
                .iter()
                .map(|episode| render_one(text, episode))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(parts.join(" "))
        };
        if self.settings.multi_episode_style == MultiEpisodeStyle::Range
            && is_consecutive(show, episodes)
            && let (Some(first), Some(last)) = (episodes.first(), episodes.last())
        {
            let head = render_one(text, first)?;
            let tail = render_one(&fragment.range_tail(), last)?;
            return Ok(format!("{head}-{tail}"));
        }
        repeat()
    }

    /// Replaces every aired-date token by the dates of all the episodes, joined with ` - `.
    fn join_aired_dates(
        &self,
        morphed: &str,
        show: &Item,
        episodes: &[&Item],
    ) -> Result<String, RenameError> {
        static AIRED: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"\$\{\s*episode\.firstAired\b[^}]*\}").expect("valid regex")
        });
        let mut out = String::with_capacity(morphed.len());
        let mut last = 0;
        for found in AIRED.find_iter(morphed) {
            out.push_str(&morphed[last..found.start()]);
            let mut dates: Vec<String> = Vec::new();
            for episode in episodes {
                let model = self.model(show, episode);
                let date = self.namer.render(found.as_str(), &model)?;
                if !date.is_empty() && !dates.contains(&date) {
                    dates.push(date);
                }
            }
            out.push_str(&dates.join(" - "));
            last = found.end();
        }
        out.push_str(&morphed[last..]);
        Ok(out)
    }

    // ---- planning ----------------------------------------------------------------------------

    /// Plans the rename of one show.
    pub fn plan(&self, input: &TvInput<'_>) -> Result<TvPlan, RenameError> {
        let show = input.show;
        let ItemDetail::Show(show_fields) = &show.detail else {
            return Err(RenameError::NotAShow);
        };
        if show.core.title.trim().is_empty() {
            return Err(RenameError::NoTitle);
        }
        let s = &self.settings;
        if s.rename_files {
            let morphed = self.namer.morph(&s.file_pattern);
            if episode_token_count(&morphed) == 0 && !keeps_original_name(&morphed) {
                return Err(RenameError::EpisodePatternWithoutEpisode);
            }
        }
        let old_folder = input.folder.trim_matches('/').to_owned();
        let new_folder = if s.rename_show_folder {
            let name = self.show_folder_name(show)?;
            let name = name.trim_end_matches('/');
            if name.is_empty() {
                old_folder.clone()
            } else if s.keep_subfolders {
                keep_subfolder(name, &old_folder, false)
            } else {
                name.to_owned()
            }
        } else {
            old_folder.clone()
        };
        let recommended = self.is_recommended();

        let ctx = TvContext {
            renamer: self,
            show,
            show_fields,
            old_folder: &old_folder,
            new_folder: &new_folder,
            recommended,
        };

        let mut files: Vec<PlannedFile> = Vec::new();
        let groups = ctx.groups(input.episodes)?;
        // Season folders as the episodes use them, for the season artwork.
        let mut season_dirs: HashMap<u32, String> = HashMap::new();
        for group in &groups {
            season_dirs
                .entry(group.season)
                .or_insert_with(|| group.season_dir.clone());
        }
        files.extend(ctx.plan_show_files(input.files));
        for season in input.seasons {
            let dir = match season_dirs.get(&season.season) {
                Some(dir) => dir.clone(),
                None => ctx.season_dir_for(season.season)?,
            };
            files.extend(ctx.plan_season_files(season, &dir));
        }
        for group in &groups {
            files.extend(ctx.plan_group_files(group, input.episodes));
        }

        let left_behind = files.iter().any(|f| f.fate == Fate::LeftBehind);
        let folder_change = if new_folder == old_folder {
            FolderChange::Unchanged
        } else if old_folder.is_empty() || left_behind {
            FolderChange::MoveFiles
        } else {
            FolderChange::Rename
        };
        let mut problems: Vec<Problem> = Vec::new();
        for planned in &files {
            if planned.kind == MediaFileType::Nfo && matches!(planned.fate, Fate::LeftBehind) {
                problems.push(Problem::UnreadableNfo {
                    path: planned.from.clone(),
                });
            }
        }
        problems.extend(
            duplicate_destinations(&files)
                .into_iter()
                .map(|path| Problem::DuplicateDestination { path }),
        );
        Ok(TvPlan {
            ops: ops_of(&files),
            old_folder,
            new_folder,
            folder_change,
            files,
            problems,
        })
    }
}

/// An episode with no data, to render show-level patterns.
fn placeholder_episode(show: &Item) -> Item {
    let mut episode = Item::episode(show.id.unwrap_or(ItemId(0)), "");
    if let ItemDetail::Episode(fields) = &mut episode.detail {
        *fields = EpisodeFields::default();
    }
    episode
}

/// The displayed season number of an episode.
fn season_number(show: &Item, episode: &Item) -> Option<u32> {
    let ItemDetail::Episode(fields) = &episode.detail else {
        return None;
    };
    let group = match &show.detail {
        ItemDetail::Show(show) => show.episode_group.clone(),
        _ => EpisodeGroup::aired(),
    };
    fields.numbers.display(&group).map(|n| n.season)
}

/// The displayed (season, episode) of an episode.
fn display_number(show: &Item, episode: &Item) -> Option<(u32, u32)> {
    let ItemDetail::Episode(fields) = &episode.detail else {
        return None;
    };
    let group = match &show.detail {
        ItemDetail::Show(show) => show.episode_group.clone(),
        _ => EpisodeGroup::aired(),
    };
    fields
        .numbers
        .display(&group)
        .map(|n| (n.season, n.episode))
}

/// Whether the episodes are one season in unbroken order (`2, 3, 4`).
fn is_consecutive(show: &Item, episodes: &[&Item]) -> bool {
    let numbers: Option<Vec<(u32, u32)>> = episodes
        .iter()
        .map(|episode| display_number(show, episode))
        .collect();
    let Some(numbers) = numbers else {
        return false;
    };
    numbers
        .windows(2)
        .all(|pair| pair[0].0 == pair[1].0 && pair[0].1 + 1 == pair[1].1)
}

/// A title that only restates an episode number (`S01E01`, `1x01`, `S01 E01`), as the scan
/// gives an episode whose file name has no title in it.
static NUMBER_ONLY_TITLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^\s*(?:s\d{1,4}[\s._-]?[ex]\d{1,4}(?:[\s._-]?[ex]?\d{1,4})*|\d{1,4}x\d{1,4}(?:[\s._-]?x?\d{1,4})*)\s*$",
    )
    .expect("valid regex")
});

/// The title an episode file name carries: the episode's title, or nothing when the title only
/// repeats the episode number the name shows anyway (`Dark - S01E01.mkv`, not
/// `Dark - S01E01 - S01E01.mkv`).
fn title_for_name(episode: &Item) -> &str {
    let title = episode.core.title.as_str();
    if NUMBER_ONLY_TITLE.is_match(title) {
        ""
    } else {
        title
    }
}

/// The model of the first episode with the titles of all episodes joined with ` - `, duplicates
/// skipped.
fn joined_model(base: &Value, episodes: &[&Item]) -> Value {
    let Some(Value::Map(episode)) = base.get("episode") else {
        return base.clone();
    };
    let join = |pick: &dyn Fn(&Item) -> String| -> Value {
        let mut parts: Vec<String> = Vec::new();
        for item in episodes {
            let text = pick(item);
            if !text.is_empty() && !parts.contains(&text) {
                parts.push(text);
            }
        }
        Value::text(parts.join(" - "))
    };
    let mut fields = (*episode).clone();
    fields.insert("title".to_owned(), join(&|i| title_for_name(i).to_owned()));
    fields.insert(
        "originalTitle".to_owned(),
        join(&|i| i.core.original_title.clone()),
    );
    fields.insert(
        "englishTitle".to_owned(),
        join(&|i| match &i.detail {
            ItemDetail::Episode(f) => f.english_title.clone(),
            _ => String::new(),
        }),
    );
    let mut root: Vec<(String, Value)> = Vec::new();
    for key in ["tvShow", "season"] {
        if let Some(value) = base.get(key) {
            root.push((key.to_owned(), value));
        }
    }
    root.push(("episode".to_owned(), Value::map(fields)));
    Value::map(root)
}

// ---- planning internals --------------------------------------------------------------------

/// The episodes that share one video file.
struct Group {
    /// Indices into the input episodes, in episode order.
    members: Vec<usize>,
    season: u32,
    /// The season folder inside the show folder (empty: the show folder itself).
    season_dir: String,
    /// The base name of the files, without extension.
    base: String,
    /// The base name of the video as it is now.
    old_base: String,
    video_ext: String,
    video: Option<PlanFile>,
    /// The episodes have no season and episode number (a bonus file without `SxxEyy`): a name
    /// cannot be built for them, so their files keep their names and places in the show folder.
    unnumbered: bool,
}

struct TvContext<'a> {
    renamer: &'a TvRenamer,
    show: &'a Item,
    show_fields: &'a ShowFields,
    old_folder: &'a str,
    new_folder: &'a str,
    recommended: bool,
}

fn join(folder: &str, rel: &str) -> String {
    match (folder.is_empty(), rel.is_empty()) {
        (_, true) => folder.to_owned(),
        (true, false) => rel.to_owned(),
        (false, false) => format!("{folder}/{rel}"),
    }
}

fn extension(name: &str) -> Option<&str> {
    name.rsplit_once('.')
        .map(|(_, ext)| ext)
        .filter(|ext| !ext.is_empty())
}

fn stem(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(stem, _)| stem)
}

impl TvContext<'_> {
    fn settings(&self) -> &TvSettings {
        &self.renamer.settings
    }

    /// The path relative to the old show folder.
    fn rel<'f>(&self, file: &'f PlanFile) -> Option<&'f str> {
        if self.old_folder.is_empty() {
            Some(file.path.as_str())
        } else {
            file.path
                .strip_prefix(self.old_folder)
                .and_then(|rest| rest.strip_prefix('/'))
        }
    }

    /// The file in the new show folder, at the same place relative to the show folder.
    fn path_only(&self, file: &PlanFile) -> String {
        match self.rel(file) {
            Some(rel) => join(self.new_folder, rel),
            None => file.path.clone(),
        }
    }

    fn keep(&self, file: &PlanFile) -> Fate {
        Fate::Carried {
            to: vec![self.path_only(file)],
        }
    }

    fn unwanted_or(&self, file: &PlanFile, otherwise: Fate) -> Fate {
        if self.settings().cleanup_unwanted
            && self
                .renamer
                .unwanted
                .iter()
                .any(|re| re.is_match(file.name()))
        {
            Fate::Trash
        } else {
            otherwise
        }
    }

    fn carried_to(&self, file: &PlanFile, mut names: Vec<String>) -> Fate {
        let mut seen = HashSet::new();
        names.retain(|n| seen.insert(n.clone()));
        let current = self.path_only(file);
        if let Some(at) = names.iter().position(|n| *n == current) {
            names.swap(0, at);
        }
        Fate::Carried { to: names }
    }

    /// The index of the newest file among those that `counts`.
    fn newest(files: &[PlanFile], counts: impl Fn(&PlanFile) -> bool) -> Option<usize> {
        let mut best: Option<usize> = None;
        for (index, file) in files.iter().enumerate() {
            if !counts(file) {
                continue;
            }
            if best.is_none_or(|b| files[b].modified_ms < file.modified_ms) {
                best = Some(index);
            }
        }
        best
    }

    /// The season folder for a season without episodes in the input.
    fn season_dir_for(&self, season: u32) -> Result<String, RenameError> {
        let mut episode = Item::episode(self.show.id.unwrap_or(ItemId(0)), "");
        if let ItemDetail::Episode(fields) = &mut episode.detail {
            fields.numbers.set(cv_domain::EpisodeNumber::new(
                self.show_fields.episode_group.clone(),
                season,
                0,
            ));
            fields
                .numbers
                .set(cv_domain::EpisodeNumber::aired(season, 0));
        }
        self.season_dir(&episode, season, None)
    }

    /// The season folder of an episode: the profile's pattern with its fallbacks. `current` is
    /// the folder the video is in now (relative to the show folder), kept when the profile does
    /// not rename season folders.
    fn season_dir(
        &self,
        episode: &Item,
        season: u32,
        current: Option<&str>,
    ) -> Result<String, RenameError> {
        let s = self.settings();
        if !s.rename_season_folder {
            return Ok(current.unwrap_or_default().to_owned());
        }
        // An empty pattern means "no season folder"; it does not make season 0 `Specials`.
        let name = if s.season_pattern.trim().is_empty() {
            String::new()
        } else {
            self.renamer.season_folder_name(self.show, episode)?
        };
        let name = name.trim_matches('/').to_owned();
        if name.is_empty() && !self.recommended {
            return Ok(if s.special_season && season == 0 {
                "Specials".to_owned()
            } else {
                format!("Season {season}")
            });
        }
        Ok(name)
    }

    fn groups(&self, episodes: &[EpisodeInput<'_>]) -> Result<Vec<Group>, RenameError> {
        let s = self.settings();
        let mut order: Vec<(Option<String>, Vec<usize>)> = Vec::new();
        for (index, episode) in episodes.iter().enumerate() {
            let video = episode
                .files
                .iter()
                .find(|f| f.kind == MediaFileType::Video)
                .map(|f| f.path.clone());
            match (
                &video,
                order.iter_mut().find(|(v, _)| v.is_some() && *v == video),
            ) {
                (Some(_), Some((_, members))) => members.push(index),
                _ => order.push((video, vec![index])),
            }
        }
        let mut groups = Vec::new();
        for (_, mut members) in order {
            members.sort_by_key(|&i| display_number(self.show, episodes[i].item).unwrap_or((0, 0)));
            let first = episodes[members[0]];
            let unnumbered = members
                .iter()
                .any(|&i| display_number(self.show, episodes[i].item).is_none());
            let season = season_number(self.show, first.item).unwrap_or(0);
            let video = first
                .files
                .iter()
                .find(|f| f.kind == MediaFileType::Video)
                .cloned();
            let video_dir = video
                .as_ref()
                .and_then(|v| {
                    self.rel(v)
                        .map(|rel| rel.rsplit_once('/').map_or("", |(d, _)| d))
                })
                .unwrap_or_default();
            let season_dir = if unnumbered {
                video_dir.to_owned()
            } else {
                self.season_dir(first.item, season, Some(video_dir))?
            };
            let old_base = video
                .as_ref()
                .map(crate::movie::unstacked_stem)
                .unwrap_or_default();
            let base = if s.rename_files && !unnumbered {
                let items: Vec<&Item> = members.iter().map(|&i| episodes[i].item).collect();
                let mut name = self.renamer.episode_file_name(self.show, &items)?;
                if uses_original_filename(&s.file_pattern)
                    && let Some(ext) = video.as_ref().and_then(|v| extension(v.name()))
                {
                    let suffix = format!(".{}", ext.to_lowercase());
                    if name.to_lowercase().ends_with(&suffix) {
                        name.truncate(name.len() - suffix.len());
                    }
                }
                if name.is_empty() {
                    old_base.clone()
                } else {
                    name
                }
            } else {
                old_base.clone()
            };
            let video_ext = video
                .as_ref()
                .and_then(|v| extension(v.name()))
                .unwrap_or_default()
                .to_owned();
            groups.push(Group {
                members,
                season,
                season_dir,
                base,
                old_base,
                video_ext,
                video,
                unnumbered,
            });
        }
        Ok(groups)
    }

    // ---- show files --------------------------------------------------------------------------

    fn plan_show_files(&self, files: &[PlanFile]) -> Vec<PlannedFile> {
        let s = self.settings();
        let nfo_winner = Self::newest(files, |f| f.kind == MediaFileType::Nfo && f.nfo_valid);
        let kinds = [
            (MediaFileType::Poster, ShowArt::Poster, &s.poster),
            (MediaFileType::Fanart, ShowArt::Fanart, &s.fanart),
            (MediaFileType::Banner, ShowArt::Banner, &s.banner),
            (MediaFileType::Clearart, ShowArt::Clearart, &s.clearart),
            (MediaFileType::Clearlogo, ShowArt::Clearlogo, &s.clearlogo),
            (MediaFileType::Disc, ShowArt::Discart, &s.discart),
            (
                MediaFileType::Characterart,
                ShowArt::Characterart,
                &s.characterart,
            ),
            (MediaFileType::Keyart, ShowArt::Keyart, &s.keyart),
            (MediaFileType::Thumb, ShowArt::Thumb, &s.thumb),
            (MediaFileType::Squareart, ShowArt::Squareart, &s.squareart),
        ];
        let winners: HashMap<MediaFileType, usize> = kinds
            .iter()
            .filter_map(|(kind, _, _)| Self::newest(files, |f| f.kind == *kind).map(|i| (*kind, i)))
            .collect();
        files
            .iter()
            .enumerate()
            .map(|(index, file)| {
                let fate = match file.kind {
                    MediaFileType::Nfo => {
                        if !file.nfo_valid {
                            Fate::LeftBehind
                        } else if nfo_winner != Some(index) {
                            Fate::Trash
                        } else {
                            let namings: Vec<ShowNfoNaming> = s
                                .show_nfo
                                .iter()
                                .filter_map(|n| ShowNfoNaming::parse(n))
                                .collect();
                            let names = show_nfo_file_names(&namings);
                            if names.is_empty() {
                                Fate::Trash
                            } else {
                                self.carried_to(
                                    file,
                                    names.iter().map(|n| join(self.new_folder, n)).collect(),
                                )
                            }
                        }
                    }
                    kind => match kinds.iter().find(|(k, _, _)| *k == kind) {
                        Some((_, art, choices)) => {
                            let ext = extension(file.name()).unwrap_or_default();
                            let names: Vec<String> = choices
                                .iter()
                                .filter_map(|c| show_stem(*art, c))
                                .map(|stem| join(self.new_folder, &format!("{stem}.{ext}")))
                                .collect();
                            if names.is_empty() {
                                self.keep(file)
                            } else if winners.get(&kind) != Some(&index) {
                                Fate::Trash
                            } else {
                                self.carried_to(file, names)
                            }
                        }
                        None => self.unwanted_or(file, self.keep(file)),
                    },
                };
                PlannedFile {
                    from: file.path.clone(),
                    kind: file.kind,
                    owner: Owner::Show,
                    fate,
                }
            })
            .collect()
    }

    // ---- season files ------------------------------------------------------------------------

    fn plan_season_files(&self, season: &SeasonInput, season_dir: &str) -> Vec<PlannedFile> {
        let s = self.settings();
        let files = &season.files;
        let arts = [
            (
                MediaFileType::SeasonPoster,
                SeasonArt::Poster,
                &s.season_poster,
            ),
            (
                MediaFileType::SeasonFanart,
                SeasonArt::Fanart,
                &s.season_fanart,
            ),
            (
                MediaFileType::SeasonBanner,
                SeasonArt::Banner,
                &s.season_banner,
            ),
            (
                MediaFileType::SeasonThumb,
                SeasonArt::Thumb,
                &s.season_thumb,
            ),
        ];
        let nfo_winner = Self::newest(files, |f| f.kind == MediaFileType::Nfo && f.nfo_valid);
        files
            .iter()
            .enumerate()
            .map(|(index, file)| {
                let fate = match file.kind {
                    MediaFileType::Nfo => {
                        if !file.nfo_valid {
                            Fate::LeftBehind
                        } else if nfo_winner != Some(index) {
                            Fate::Trash
                        } else {
                            let namings: Vec<SeasonNfoNaming> = s
                                .season_nfo
                                .iter()
                                .filter_map(|n| SeasonNfoNaming::parse(n))
                                .collect();
                            let paths = season_nfo_paths(
                                &namings,
                                season.season,
                                Some(season_dir),
                                s.special_season,
                            );
                            if paths.is_empty() {
                                Fate::Trash
                            } else {
                                self.carried_to(
                                    file,
                                    paths.iter().map(|p| join(self.new_folder, p)).collect(),
                                )
                            }
                        }
                    }
                    kind => match arts.iter().find(|(k, _, _)| *k == kind) {
                        Some((_, art, choices)) => {
                            let ext = extension(file.name()).unwrap_or_default();
                            let names: Vec<String> = choices
                                .iter()
                                .filter_map(|c| {
                                    season_art_path(
                                        *art,
                                        c,
                                        i64::from(season.season),
                                        season_dir,
                                        ext,
                                    )
                                })
                                .map(|p| join(self.new_folder, &p))
                                .collect();
                            let winner = Self::newest(files, |f| f.kind == kind);
                            if names.is_empty() {
                                self.keep(file)
                            } else if winner != Some(index) {
                                Fate::Trash
                            } else {
                                self.carried_to(file, names)
                            }
                        }
                        None => self.unwanted_or(file, self.keep(file)),
                    },
                };
                PlannedFile {
                    from: file.path.clone(),
                    kind: file.kind,
                    owner: Owner::Season(season.season),
                    fate,
                }
            })
            .collect()
    }

    // ---- episode files -----------------------------------------------------------------------

    fn stack_string(&self, file: &PlanFile) -> String {
        if !file.stacking_marker.is_empty() {
            format!(".{}", file.stacking_marker)
        } else if file.stacking > 0 {
            format!(".CD{}", file.stacking)
        } else {
            String::new()
        }
    }

    fn subtitle_name(&self, group: &Group, file: &PlanFile) -> String {
        let mut name = format!("{}{}", group.base, self.stack_string(file));
        if let Some(tag) = &file.subtitle {
            let language = language_tag(&tag.language, self.settings().subtitle_language_style);
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

    /// The files of all episodes of a group, without repeats.
    fn group_files(group: &Group, episodes: &[EpisodeInput<'_>]) -> Vec<PlanFile> {
        let mut seen: HashSet<&str> = HashSet::new();
        let mut files = Vec::new();
        for &member in &group.members {
            for file in episodes[member].files {
                if seen.insert(file.path.as_str()) {
                    files.push(file.clone());
                }
            }
        }
        files
    }

    fn plan_group_files(&self, group: &Group, episodes: &[EpisodeInput<'_>]) -> Vec<PlannedFile> {
        static BIF_SIZE: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"(?i)(-\d+-\d+)\.bif$").expect("valid regex"));
        let s = self.settings();
        let files = Self::group_files(group, episodes);
        let owner = Owner::Episodes(group.members[0]);
        if group.unnumbered {
            // Nothing to name them by: they only move along with the show folder.
            return files
                .iter()
                .map(|file| PlannedFile {
                    from: file.path.clone(),
                    kind: file.kind,
                    owner,
                    fate: self.keep(file),
                })
                .collect();
        }
        let at = |name: String| Fate::Carried {
            to: vec![join(&join(self.new_folder, &group.season_dir), &name)],
        };
        let dir = join(self.new_folder, &group.season_dir);
        let nfo_winner = Self::newest(&files, |f| f.kind == MediaFileType::Nfo && f.nfo_valid);
        let thumb_winner = Self::newest(&files, |f| f.kind == MediaFileType::Thumb);
        // `.idx` files that go with a `.sub` subtitle.
        let mut companions: HashMap<usize, usize> = HashMap::new();
        for (sub_index, sub) in files.iter().enumerate() {
            if sub.kind == MediaFileType::Subtitle
                && extension(sub.name()).is_some_and(|e| e.eq_ignore_ascii_case("sub"))
            {
                let wanted = format!("{}.idx", stem(sub.name())).to_lowercase();
                if let Some(idx) = files
                    .iter()
                    .position(|f| f.parent() == sub.parent() && f.name().to_lowercase() == wanted)
                {
                    companions.insert(idx, sub_index);
                }
            }
        }

        files
            .iter()
            .enumerate()
            .map(|(index, file)| {
                let ext = extension(file.name()).unwrap_or_default();
                let fate = if let Some(&sub) = companions.get(&index) {
                    let name = self.subtitle_name(group, &files[sub]);
                    at(format!("{}.idx", stem(&name)))
                } else {
                    match file.kind {
                        MediaFileType::Video => {
                            at(format!("{}{}.{ext}", group.base, self.stack_string(file)))
                        }
                        MediaFileType::Nfo => {
                            if !file.nfo_valid {
                                Fate::LeftBehind
                            } else if nfo_winner != Some(index) {
                                Fate::Trash
                            } else if s
                                .episode_nfo
                                .iter()
                                .any(|n| EpisodeNfoNaming::parse(n).is_some())
                            {
                                self.carried_to(
                                    file,
                                    vec![join(&dir, &format!("{}.nfo", group.base))],
                                )
                            } else {
                                Fate::Trash
                            }
                        }
                        MediaFileType::Thumb => {
                            let names: Vec<String> = s
                                .episode_thumb
                                .iter()
                                .filter_map(|c| episode_thumb_name(c, &group.base, ext))
                                .map(|n| join(&dir, &n))
                                .collect();
                            if names.is_empty() {
                                self.keep(file)
                            } else if thumb_winner != Some(index) {
                                Fate::Trash
                            } else {
                                self.carried_to(file, names)
                            }
                        }
                        MediaFileType::Fanart => at(format!("{}-fanart.{ext}", group.base)),
                        MediaFileType::Trailer => at(format!("{}-trailer.{ext}", group.base)),
                        MediaFileType::Sample => at(format!("{}-sample.{ext}", group.base)),
                        MediaFileType::Mediainfo => at(format!("{}-mediainfo.xml", group.base)),
                        MediaFileType::Vsmeta | MediaFileType::DoubleExt => {
                            if group.video_ext.is_empty() {
                                self.keep(file)
                            } else {
                                at(format!("{}.{}.{ext}", group.base, group.video_ext))
                            }
                        }
                        MediaFileType::Bif => {
                            let size = BIF_SIZE
                                .captures(file.name())
                                .map(|c| c[1].to_owned())
                                .unwrap_or_default();
                            at(format!("{}{size}.bif", group.base))
                        }
                        MediaFileType::Subtitle => at(self.subtitle_name(group, file)),
                        MediaFileType::Extra | MediaFileType::VideoExtra => {
                            self.keep_or_rebase(group, file)
                        }
                        MediaFileType::Audio | MediaFileType::Text | MediaFileType::Unknown => {
                            self.unwanted_or(file, self.keep_or_rebase(group, file))
                        }
                        _ => self.keep(file),
                    }
                };
                PlannedFile {
                    from: file.path.clone(),
                    kind: file.kind,
                    owner,
                    fate,
                }
            })
            .collect()
    }

    /// A file next to the video that starts with the old base name keeps what follows it:
    /// `old-extra.mkv` becomes `new-extra.mkv`. Any other file only moves along.
    fn keep_or_rebase(&self, group: &Group, file: &PlanFile) -> Fate {
        let beside_video = group
            .video
            .as_ref()
            .is_some_and(|video| video.parent() == file.parent());
        let name = file.name();
        let old = group.old_base.as_str();
        if beside_video
            && !old.is_empty()
            && name.len() > old.len()
            && name.is_char_boundary(old.len())
            && name[..old.len()].eq_ignore_ascii_case(old)
        {
            return Fate::Carried {
                to: vec![join(
                    &join(self.new_folder, &group.season_dir),
                    &format!("{}{}", group.base, &name[old.len()..]),
                )],
            };
        }
        self.keep(file)
    }
}

#[cfg(test)]
mod tests;
