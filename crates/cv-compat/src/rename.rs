//! Renamer handlers: destinations and file trees of movies and TV shows.
//!
//! The handlers are test glue over `cv-rename`. A destination case renders one pattern against a
//! fixture entity. A tree case plans the rename of a whole movie or show folder and reports it
//! the way the Rename preview does: files grouped by file type (movies) or by owner (TV), each
//! group with its old and new paths, relative to the old and the new folder.

use std::collections::{BTreeMap, BTreeSet};

use cv_domain::{Item, MediaFileType};
use cv_rename::clean::replace_invalid_characters;
use cv_rename::pattern::{is_file_pattern_valid, is_folder_pattern_unique};
use cv_rename::{EpisodeInput, SeasonInput};
use cv_rename::{
    Fate, MovieInput, MovieRenamer, MovieSettings, MultiEpisodeStyle, NameOptions, Owner, PlanFile,
    PlannedFile, RenameError, SpaceSubstitution, SubtitleLanguageStyle, SubtitleTag, TvInput,
    TvRenamer, TvSettings,
};
use cv_template::tokens::{MovieSetContext, tv_aliases};
use regex::Regex;
use serde_json::{Value as Json, json};

use crate::entity;
use crate::entity::read_entity;
use crate::family::Family;
use crate::fixture::Case;
use crate::nfo::str_of;
use crate::run::{Actual, Handler, HandlerError};

fn failed(message: impl Into<String>) -> HandlerError {
    HandlerError::Failed(message.into())
}

fn text<'a>(case: &'a Case, key: &str) -> &'a str {
    str_of(&case.input, key)
}

fn flag(json: Option<&Json>, key: &str) -> Option<bool> {
    json?.get(key)?.as_bool()
}

fn string_of(json: Option<&Json>, key: &str) -> Option<String> {
    json?.get(key)?.as_str().map(str::to_owned)
}

/// A list setting, when the object has the key (an empty list is a value).
fn list_of(json: Option<&Json>, key: &str) -> Option<Vec<String>> {
    let list = json?.get(key)?.as_array()?;
    Some(
        list.iter()
            .filter_map(Json::as_str)
            .map(str::to_owned)
            .collect(),
    )
}

fn space(renamer: Option<&Json>, enabled: &str, replacement: &str, base: &mut SpaceSubstitution) {
    if let Some(on) = flag(renamer, enabled) {
        base.enabled = on;
    }
    if let Some(text) = string_of(renamer, replacement) {
        base.replacement = text;
    }
}

fn name_options(renamer: Option<&Json>, base: &mut NameOptions) {
    if let Some(text) = string_of(renamer, "renamerColonReplacement") {
        base.colon_replacement = text;
    }
    if let Some(text) = string_of(renamer, "renamerFirstCharacterNumberReplacement") {
        base.first_character_number_replacement = text;
    }
    if let Some(on) = flag(renamer, "asciiReplacement") {
        base.ascii_replacement = on;
    }
    if let Some(on) = flag(renamer, "unicodeReplacement") {
        base.unicode_replacement = on;
    }
}

fn language_style(naming: Option<&Json>) -> Option<SubtitleLanguageStyle> {
    SubtitleLanguageStyle::parse(&string_of(naming, "subtitleLanguageStyle")?)
}

/// The naming profile of a case: the profile, then the case's `settings`, for one section.
fn naming_sections<'a>(case: &'a Case, section: &str) -> Vec<&'a Json> {
    let mut sections = Vec::new();
    if let Some(profile) = case.input.get("profile").and_then(Json::as_str)
        && let Some(found) = case.shared.get("profiles").and_then(|p| p.get(profile))
        && let Some(naming) = found.get(section)
    {
        sections.push(naming);
    }
    if let Some(naming) = case.input.get("settings").and_then(|s| s.get(section)) {
        sections.push(naming);
    }
    sections
}

fn movie_settings(case: &Case) -> MovieSettings {
    // The fixtures render the folder pattern directly under the data source root, so keeping
    // the sub-folders a title sits in (CineVault's default) is switched off here.
    let mut s = MovieSettings {
        keep_subfolders: false,
        ..MovieSettings::default()
    };
    for naming in naming_sections(case, "movie") {
        let naming = Some(naming);
        let assign = |target: &mut Vec<String>, key: &str| {
            if let Some(list) = list_of(naming, key) {
                *target = list;
            }
        };
        assign(&mut s.nfo, "nfoFilenames");
        assign(&mut s.poster, "posterFilenames");
        assign(&mut s.fanart, "fanartFilenames");
        assign(&mut s.extra_fanart, "extraFanartFilenames");
        assign(&mut s.banner, "bannerFilenames");
        assign(&mut s.clearart, "clearartFilenames");
        assign(&mut s.thumb, "thumbFilenames");
        assign(&mut s.clearlogo, "clearlogoFilenames");
        assign(&mut s.discart, "discartFilenames");
        assign(&mut s.keyart, "keyartFilenames");
        assign(&mut s.squareart, "squareartFilenames");
        assign(&mut s.trailer, "trailerFilenames");
        if let Some(style) = language_style(naming) {
            s.subtitle_language_style = style;
        }
        if let Some(on) = flag(naming, "subtitleWithoutLanguageTag") {
            s.subtitle_without_language_tag = on;
        }
    }
    let renamer = case.input.get("renamer");
    if let Some(pattern) = string_of(renamer, "renamerPathname") {
        s.folder_pattern = pattern;
    }
    if let Some(pattern) = string_of(renamer, "renamerFilename") {
        s.file_pattern = pattern;
    }
    if let Some(on) = flag(renamer, "renamerPathnameEnabled") {
        s.rename_folder = on;
    }
    if let Some(on) = flag(renamer, "renamerFilenameEnabled") {
        s.rename_files = on;
    }
    space(
        renamer,
        "renamerPathnameSpaceSubstitution",
        "renamerPathnameSpaceReplacement",
        &mut s.folder_space,
    );
    space(
        renamer,
        "renamerFilenameSpaceSubstitution",
        "renamerFilenameSpaceReplacement",
        &mut s.file_space,
    );
    name_options(renamer, &mut s.name);
    if let Some(on) = flag(renamer, "renamerNfoCleanup") {
        s.nfo_cleanup = on;
    }
    if let Some(on) = flag(renamer, "renamerCleanupUnwanted") {
        s.cleanup_unwanted = on;
    }
    if let Some(on) = flag(renamer, "renamerCreateMoviesetForSingleMovie") {
        s.create_movieset_for_single_movie = on;
    }
    s
}

fn tv_settings(case: &Case) -> TvSettings {
    // See `movie_settings`: the fixtures render without keep sub-folders.
    let mut s = TvSettings {
        keep_subfolders: false,
        ..TvSettings::default()
    };
    for naming in naming_sections(case, "tvShow") {
        let naming = Some(naming);
        let assign = |target: &mut Vec<String>, key: &str| {
            if let Some(list) = list_of(naming, key) {
                *target = list;
            }
        };
        assign(&mut s.show_nfo, "nfoFilenames");
        assign(&mut s.season_nfo, "seasonNfoFilenames");
        assign(&mut s.episode_nfo, "episodeNfoFilenames");
        assign(&mut s.poster, "posterFilenames");
        assign(&mut s.fanart, "fanartFilenames");
        assign(&mut s.banner, "bannerFilenames");
        assign(&mut s.clearart, "clearartFilenames");
        assign(&mut s.clearlogo, "clearlogoFilenames");
        assign(&mut s.season_poster, "seasonPosterFilenames");
        assign(&mut s.season_fanart, "seasonFanartFilenames");
        assign(&mut s.season_banner, "seasonBannerFilenames");
        assign(&mut s.season_thumb, "seasonThumbFilenames");
        assign(&mut s.episode_thumb, "episodeThumbFilenames");
        if let Some(style) = language_style(naming) {
            s.subtitle_language_style = style;
        }
    }
    let renamer = case.input.get("renamer");
    if let Some(pattern) = string_of(renamer, "renamerTvShowFoldername") {
        s.show_pattern = pattern;
    }
    if let Some(pattern) = string_of(renamer, "renamerSeasonFoldername") {
        s.season_pattern = pattern;
    }
    if let Some(pattern) = string_of(renamer, "renamerFilename") {
        s.file_pattern = pattern;
    }
    if let Some(on) = flag(renamer, "renamerTvShowFoldernameEnabled") {
        s.rename_show_folder = on;
    }
    if let Some(on) = flag(renamer, "renamerSeasonFoldernameEnabled") {
        s.rename_season_folder = on;
    }
    if let Some(on) = flag(renamer, "renamerFilenameEnabled") {
        s.rename_files = on;
    }
    space(
        renamer,
        "renamerShowPathnameSpaceSubstitution",
        "renamerShowPathnameSpaceReplacement",
        &mut s.show_space,
    );
    space(
        renamer,
        "renamerSeasonPathnameSpaceSubstitution",
        "renamerSeasonPathnameSpaceReplacement",
        &mut s.season_space,
    );
    space(
        renamer,
        "renamerFilenameSpaceSubstitution",
        "renamerFilenameSpaceReplacement",
        &mut s.file_space,
    );
    name_options(renamer, &mut s.name);
    if let Some(style) =
        string_of(renamer, "renamerMultiEpisodeStyle").and_then(|n| MultiEpisodeStyle::parse(&n))
    {
        s.multi_episode_style = style;
    }
    if let Some(on) = flag(renamer, "specialSeason") {
        s.special_season = on;
    }
    s
}

// ---- movie destinations --------------------------------------------------------------------

/// Movie destinations: `{id, kind, entity, template, renamer?}` to `{id, result}`; pattern
/// checks give `{id, folder_unique}` or `{id, file_valid}`.
pub(crate) struct DestinationsHandler;

impl Handler for DestinationsHandler {
    fn family(&self) -> Family {
        Family::RenameDestinations
    }

    fn run(&self, case: &Case) -> Result<Actual, HandlerError> {
        let id = &case.id;
        match text(case, "kind") {
            "invalid-characters" => {
                let settings = movie_settings(case);
                let result = replace_invalid_characters(
                    text(case, "input"),
                    &settings.name.colon_replacement,
                    settings.name.unicode_replacement,
                );
                Ok(Actual::Json(json!({ "id": id, "result": result })))
            }
            "pattern-check" => {
                if let Some(folder) = case.input.get("folder").and_then(Json::as_str) {
                    Ok(Actual::Json(
                        json!({ "id": id, "folder_unique": is_folder_pattern_unique(folder) }),
                    ))
                } else if let Some(file) = case.input.get("file").and_then(Json::as_str) {
                    Ok(Actual::Json(
                        json!({ "id": id, "file_valid": is_file_pattern_valid(file) }),
                    ))
                } else {
                    Err(failed("pattern-check without `folder` or `file`"))
                }
            }
            kind @ ("movie-token" | "movie-folder" | "movie-file") => {
                let entity = read_entity(&case.fixtures_dir, text(case, "entity"))?;
                let settings = movie_settings(case);
                let mut item = entity::movie_item(
                    &entity,
                    str_of(&entity, "folder"),
                    &entity::entity_videos(&entity),
                );
                // These fixture movies were never scanned, so they have no original file name.
                item.core.original_filename.clear();
                let set = movie_set(&entity, &settings);
                let template = text(case, "template");
                let renamer = MovieRenamer::new(settings);
                let colon = renamer.settings().name.colon_replacement.clone();
                let unicode = renamer.settings().name.unicode_replacement;
                let result = match kind {
                    // The literal text of a raw render is cleaned too (a colon, `<`).
                    "movie-token" => renamer
                        .render_raw(template, &item, set.as_ref())
                        .map(|raw| replace_invalid_characters(&raw, &colon, unicode)),
                    "movie-folder" => renamer.folder_name_for(template, &item, set.as_ref()),
                    _ => renamer.file_name_for(template, &item, set.as_ref()),
                };
                // A pattern that does not parse comes back as it was written.
                let result = match result {
                    Ok(rendered) => rendered,
                    Err(RenameError::Template(_)) => template.to_owned(),
                    Err(other) => return Err(failed(other.to_string())),
                };
                Ok(Actual::Json(json!({ "id": id, "result": result })))
            }
            other => Err(failed(format!("unknown destination kind `{other}`"))),
        }
    }
}

/// The collection of an entity, for a profile that offers it for a set of one movie.
fn movie_set(entity: &Json, settings: &MovieSettings) -> Option<MovieSetContext> {
    let set = entity.get("movieSet")?;
    settings
        .create_movieset_for_single_movie
        .then(|| MovieSetContext {
            title: str_of(set, "title").to_owned(),
            sort_title: String::new(),
            movies: vec![1],
            movies_with_dummy: vec![1],
        })
}

// ---- TV destinations -----------------------------------------------------------------------

/// The episode of a show entity with this aired season and episode number.
pub(crate) fn find_episode(entity: &Json, season: u32, episode: u32) -> Option<&Json> {
    entity.get("episodes")?.as_array()?.iter().find(|e| {
        e.get("numbers")
            .and_then(Json::as_array)
            .is_some_and(|numbers| {
                numbers.iter().any(|n| {
                    n.get("season").and_then(Json::as_u64) == Some(u64::from(season))
                        && n.get("episode").and_then(Json::as_u64) == Some(u64::from(episode))
                })
            })
    })
}

pub(crate) fn build_episode(show: &Item, json: &Json) -> Item {
    let files = json
        .get("files")
        .and_then(Json::as_array)
        .cloned()
        .unwrap_or_default();
    let dir = files.first().map_or("", |f| str_of(f, "dir"));
    let mut episode = entity::episode_item(show, json, dir);
    for file in files.iter().filter(|f| str_of(f, "type") == "VIDEO") {
        let directory = format!("{}/{}", show.core.path, str_of(file, "dir"));
        entity::add_video(&mut episode, file, &directory, str_of(file, "filename"));
    }
    episode
}

fn selected(case: &Case, key: &str) -> Option<(u32, u32)> {
    let selector = case.input.get(key)?;
    Some((
        u32::try_from(selector.get("season")?.as_u64()?).ok()?,
        u32::try_from(selector.get("episode")?.as_u64()?).ok()?,
    ))
}

/// The tokens of a pattern that the pattern check does not know.
fn unknown_tv_tokens(pattern: &str) -> String {
    let token = Regex::new(r"\$\{[^}]*\}").expect("valid regex");
    let aliases = tv_aliases();
    let mut out = String::new();
    for found in token.find_iter(pattern) {
        let inner = found.as_str()[2..found.as_str().len() - 1].trim();
        let name = inner.split(';').next().unwrap_or_default().trim();
        let known = aliases.contains(name)
            || ["tvShow.", "season.", "episode."]
                .iter()
                .any(|root| name.starts_with(root));
        if !known {
            out.push_str("  ");
            out.push_str(found.as_str());
        }
    }
    out
}

/// TV destinations: `{id, kind, entity, template, episode?, episodes?, renamer?}` to
/// `{id, result}`; pattern checks give `{id, tv_recommended}` or `{id, tv_pattern_error}`.
pub(crate) struct TvDestinationsHandler;

impl Handler for TvDestinationsHandler {
    fn family(&self) -> Family {
        Family::RenameDestinationsTv
    }

    fn run(&self, case: &Case) -> Result<Actual, HandlerError> {
        let id = &case.id;
        let kind = text(case, "kind");
        if kind == "pattern-check" {
            let renamer = TvRenamer::new(tv_settings(case));
            if let Some(file) = case.input.get("episode_file").and_then(Json::as_str) {
                let season = case.input.get("season").and_then(Json::as_str);
                return Ok(Actual::Json(
                    json!({ "id": id, "tv_recommended": renamer.is_recommended_for(season, file) }),
                ));
            }
            let pattern = text(case, "tv_pattern");
            return Ok(Actual::Json(
                json!({ "id": id, "tv_pattern_error": unknown_tv_tokens(pattern) }),
            ));
        }
        let entity = read_entity(&case.fixtures_dir, text(case, "entity"))?;
        let show = entity::show_item(&entity, str_of(&entity, "folder"));
        let renamer = TvRenamer::new(tv_settings(case));
        let template = text(case, "template");
        let pick = |selector: (u32, u32)| -> Result<Item, HandlerError> {
            find_episode(&entity, selector.0, selector.1)
                .map(|json| build_episode(&show, json))
                .ok_or_else(|| failed(format!("no episode S{}E{}", selector.0, selector.1)))
        };
        let result = match kind {
            "tv-show-folder" => renamer.show_folder_name_for(template, &show),
            "tv-season-folder" => {
                let episode = pick(selected(case, "episode").ok_or_else(|| failed("no episode"))?)?;
                renamer.season_folder_name_for(template, &show, &episode)
            }
            "tv-episode-file" => {
                let mut episodes = Vec::new();
                if let Some(list) = case.input.get("episodes").and_then(Json::as_array) {
                    for selector in list {
                        let season = selector.get("season").and_then(Json::as_u64).unwrap_or(0);
                        let number = selector.get("episode").and_then(Json::as_u64).unwrap_or(0);
                        episodes.push(pick((
                            u32::try_from(season).unwrap_or(0),
                            u32::try_from(number).unwrap_or(0),
                        ))?);
                    }
                } else {
                    episodes.push(pick(
                        selected(case, "episode").ok_or_else(|| failed("no episode"))?,
                    )?);
                }
                let refs: Vec<&Item> = episodes.iter().collect();
                renamer.episode_file_name_for(template, &show, &refs)
            }
            "tv-token" => {
                let episode = pick(selected(case, "episode").ok_or_else(|| failed("no episode"))?)?;
                renamer.render_raw(template, &show, &episode)
            }
            other => return Err(failed(format!("unknown destination kind `{other}`"))),
        };
        let result = match result {
            Ok(rendered) => rendered,
            Err(RenameError::Template(_)) => template.to_owned(),
            Err(other) => return Err(failed(other.to_string())),
        };
        Ok(Actual::Json(json!({ "id": id, "result": result })))
    }
}

// ---- file trees ----------------------------------------------------------------------------

/// A file of a tree case: the path below the folder, and its JSON description.
struct TreeFile {
    rel: String,
    json: Json,
}

fn tree_file(spec: &Json, dir: &str) -> TreeFile {
    match spec {
        Json::String(path) => TreeFile {
            rel: join(dir, path),
            json: Json::Null,
        },
        other => {
            let name = other
                .get("path")
                .or_else(|| other.get("filename"))
                .and_then(Json::as_str)
                .unwrap_or_default();
            let own_dir = str_of(other, "dir");
            let dir = if own_dir.is_empty() {
                dir.to_owned()
            } else {
                join(dir, own_dir)
            };
            TreeFile {
                rel: join(&dir, name),
                json: other.clone(),
            }
        }
    }
}

fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_owned()
    } else {
        format!("{dir}/{name}")
    }
}

/// Builds the planner's view of a file: its kind from the name, stacking, subtitle tag and the
/// readability of an NFO.
fn plan_file(folder: &str, file: &TreeFile) -> PlanFile {
    let path = join(folder, &file.rel);
    let is_video = str_of(&file.json, "type") == "VIDEO" || file.json.get("videoCodec").is_some();
    let kind = if is_video {
        MediaFileType::Video
    } else {
        cv_scan::classify::classify(&path).file_type
    };
    let mut planned = PlanFile::new(path, kind);
    if matches!(kind, MediaFileType::Video | MediaFileType::Subtitle) {
        let name = planned.name().to_owned();
        let stacking = cv_scan::stacking::parse_file_name(&name);
        if stacking.is_stacked() {
            planned = planned.stacked(stacking.marker, stacking.number);
        }
    }
    if kind == MediaFileType::Subtitle
        && let Some(first) = entity::subtitle_streams(&file.json).into_iter().next()
    {
        planned = planned.subtitle(SubtitleTag {
            language: first.language,
            forced: first.flags.contains(&cv_domain::StreamFlag::Forced),
            sdh: first
                .flags
                .contains(&cv_domain::StreamFlag::HearingImpaired),
            title: first.title,
        });
    }
    if kind == MediaFileType::Nfo
        && let Some(content) = file.json.get("content").and_then(Json::as_str)
        && !cv_nfo::read_movie_nfo(content).is_some_and(|nfo| nfo.is_valid())
    {
        planned = planned.invalid_nfo();
    }
    planned
}

/// `path` relative to `folder`, or `../` steps and the path when it is outside.
fn relative_to(folder: &str, path: &str) -> String {
    if folder.is_empty() {
        return path.to_owned();
    }
    match path
        .strip_prefix(folder)
        .and_then(|rest| rest.strip_prefix('/'))
    {
        Some(rest) => rest.to_owned(),
        None => {
            let depth = folder.split('/').count();
            format!("{}{path}", "../".repeat(depth))
        }
    }
}

/// One preview group: sorted old and new paths.
struct Group {
    old: BTreeSet<String>,
    new: BTreeSet<String>,
}

impl Group {
    fn new() -> Group {
        Group {
            old: BTreeSet::new(),
            new: BTreeSet::new(),
        }
    }

    fn add(&mut self, file: &PlannedFile, old_folder: &str, new_folder: &str) {
        self.old.insert(relative_to(old_folder, &file.from));
        match &file.fate {
            Fate::Carried { to } => {
                for path in to {
                    self.new.insert(relative_to(new_folder, path));
                }
            }
            Fate::LeftBehind => {
                self.new.insert(relative_to(new_folder, &file.from));
            }
            Fate::Trash => {}
        }
    }
}

/// The JSON of a tree result: groups, with the names that appear in more than one group marked
/// as duplicates.
fn tree_result(id: &str, old_folder: &str, new_folder: &str, groups: Vec<Group>) -> Json {
    let mut seen: BTreeMap<&String, usize> = BTreeMap::new();
    for group in &groups {
        for name in &group.new {
            *seen.entry(name).or_default() += 1;
        }
    }
    let mut problems = false;
    let mut needs_rename = old_folder != new_folder;
    let mut out = Vec::new();
    for group in &groups {
        let duped = group.new.iter().any(|name| seen[name] > 1);
        problems |= duped;
        needs_rename |= group.old != group.new;
        out.push(json!({
            "old": group.old.iter().collect::<Vec<_>>(),
            "new": group.new.iter().collect::<Vec<_>>(),
            "duped": duped,
        }));
    }
    json!({
        "id": id,
        "new_path": new_folder,
        "needs_rename": needs_rename,
        "problems": problems,
        "groups": out,
    })
}

/// Movie trees: `{id, kind, entity, profile, files?, folder?, renamer?, settings?}` to
/// `{id, new_path, needs_rename, problems, groups}`, with the files grouped by file type.
pub(crate) struct TreesHandler;

impl Handler for TreesHandler {
    fn family(&self) -> Family {
        Family::RenameTrees
    }

    fn run(&self, case: &Case) -> Result<Actual, HandlerError> {
        let entity = read_entity(&case.fixtures_dir, text(case, "entity"))?;
        let folder = case
            .input
            .get("folder")
            .and_then(Json::as_str)
            .unwrap_or_else(|| str_of(&entity, "folder"))
            .to_owned();
        let specs: Vec<Json> = match case.input.get("files").and_then(Json::as_array) {
            Some(files) => files.clone(),
            None => entity
                .get("files")
                .and_then(Json::as_array)
                .cloned()
                .unwrap_or_default(),
        };
        let files: Vec<TreeFile> = specs.iter().map(|s| tree_file(s, "")).collect();
        let videos: Vec<(String, Json)> = files
            .iter()
            .filter(|f| str_of(&f.json, "type") == "VIDEO" || f.json.get("videoCodec").is_some())
            .map(|f| {
                (
                    f.rel.rsplit('/').next().unwrap_or_default().to_owned(),
                    f.json.clone(),
                )
            })
            .collect();
        let mut item = entity::movie_item(&entity, &folder, &videos);
        if let cv_domain::ItemDetail::Movie(movie) = &mut item.detail {
            movie.is_disc = flag(Some(&case.input), "disc").unwrap_or(false);
            movie.multi_movie_dir = flag(Some(&case.input), "multiMovieDir").unwrap_or(false);
        }
        let plan_files: Vec<PlanFile> = files.iter().map(|f| plan_file(&folder, f)).collect();
        let settings = movie_settings(case);
        let set = movie_set(&entity, &settings);
        let renamer = MovieRenamer::new(settings);
        let plan = renamer
            .plan(&MovieInput {
                item: &item,
                set: set.as_ref(),
                folder: &folder,
                files: &plan_files,
            })
            .map_err(|e| failed(e.to_string()))?;

        let mut groups: BTreeMap<MediaFileType, Group> = BTreeMap::new();
        for file in &plan.files {
            groups.entry(file.kind).or_insert_with(Group::new).add(
                file,
                &plan.old_folder,
                &plan.new_folder,
            );
        }
        Ok(Actual::Json(tree_result(
            &case.id,
            &plan.old_folder,
            &plan.new_folder,
            groups.into_values().collect(),
        )))
    }
}

/// TV trees: `{id, kind, entity, profile, renamer?, settings?}` to
/// `{id, new_path, needs_rename, problems, groups}`, with the files grouped by owner: the show
/// and each season by file type, each episode (file) as one group.
pub(crate) struct TvTreesHandler;

impl Handler for TvTreesHandler {
    fn family(&self) -> Family {
        Family::RenameTreesTv
    }

    fn run(&self, case: &Case) -> Result<Actual, HandlerError> {
        let entity = read_entity(&case.fixtures_dir, text(case, "entity"))?;
        let folder = str_of(&entity, "folder").to_owned();
        let show = entity::show_item(&entity, &folder);

        // The files of the show and of each season sit in the folder itself in these trees.
        let show_files: Vec<PlanFile> = entity
            .get("files")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
            .map(|spec| {
                let mut file = tree_file(spec, "");
                file.rel = str_of(spec, "filename").to_owned();
                plan_file(&folder, &file)
            })
            .collect();
        let seasons: Vec<SeasonInput> = entity
            .get("seasons")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
            .map(|season| SeasonInput {
                season: u32::try_from(season.get("season").and_then(Json::as_u64).unwrap_or(0))
                    .unwrap_or(0),
                files: season
                    .get("files")
                    .and_then(Json::as_array)
                    .into_iter()
                    .flatten()
                    .map(|spec| {
                        let mut file = tree_file(spec, "");
                        file.rel = str_of(spec, "filename").to_owned();
                        plan_file(&folder, &file)
                    })
                    .collect(),
            })
            .collect();

        let mut episode_items: Vec<Item> = Vec::new();
        let mut episode_files: Vec<Vec<PlanFile>> = Vec::new();
        for json in entity
            .get("episodes")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
        {
            episode_items.push(build_episode(&show, json));
            episode_files.push(
                json.get("files")
                    .and_then(Json::as_array)
                    .into_iter()
                    .flatten()
                    .map(|spec| plan_file(&folder, &tree_file(spec, "")))
                    .collect(),
            );
        }
        let episodes: Vec<EpisodeInput<'_>> = episode_items
            .iter()
            .zip(&episode_files)
            .map(|(item, files)| EpisodeInput { item, files })
            .collect();

        let renamer = TvRenamer::new(tv_settings(case));
        let plan = renamer
            .plan(&TvInput {
                show: &show,
                folder: &folder,
                files: &show_files,
                seasons: &seasons,
                episodes: &episodes,
            })
            .map_err(|e| failed(e.to_string()))?;

        let mut groups: BTreeMap<(Owner, Option<MediaFileType>), Group> = BTreeMap::new();
        for file in &plan.files {
            let key = match file.owner {
                Owner::Episodes(_) => (file.owner, None),
                owner => (owner, Some(file.kind)),
            };
            groups.entry(key).or_insert_with(Group::new).add(
                file,
                &plan.old_folder,
                &plan.new_folder,
            );
        }
        Ok(Actual::Json(tree_result(
            &case.id,
            &plan.old_folder,
            &plan.new_folder,
            groups.into_values().collect(),
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths_climb_out_of_the_new_folder() {
        assert_eq!(
            relative_to("The Matrix (1999)", "The Matrix (1999)/a.mkv"),
            "a.mkv"
        );
        assert_eq!(
            relative_to("The Matrix (1999)", "nfos/m.nfo"),
            "../nfos/m.nfo"
        );
        assert_eq!(relative_to("M/Matrix", "nfos/m.nfo"), "../../nfos/m.nfo");
        assert_eq!(relative_to("", "a/b.mkv"), "a/b.mkv");
    }

    #[test]
    fn unknown_tokens_are_listed_with_two_spaces() {
        assert_eq!(unknown_tv_tokens("${showTitle}"), "");
        assert_eq!(unknown_tv_tokens("${showTitel}"), "  ${showTitel}");
        assert_eq!(
            unknown_tv_tokens("${if showTitle}x${end}"),
            "  ${if showTitle}  ${end}"
        );
        assert_eq!(unknown_tv_tokens("${title;upper} no tokens"), "");
    }
}
