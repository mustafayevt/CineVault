//! The naming-preset fixtures.
//!
//! `tests/compat/rename/presets.json` is written by hand for the three presets (Clean, Detailed,
//! Media-server safe). Each case renders one preset against a fixture entity (`rename/entities` next to the case
//! file, else the shared `fixtures/rename/entities`) and
//! lists the names it must produce, character for character. The token values underneath
//! (`Web-DL`, `Blu-ray`, `DTS-HD MA`, `7.1`) are the ones the destination golden files pin, so a
//! change in a preset or in a token shows up here.
//!
//! A case is `{id, preset, kind: "movie", entity, expect: {folder, file}}` or
//! `{id, preset, kind: "tv", entity, episode: {season, episode}, expect: {show_folder,
//! season_folder, file}}`.

use std::fs;
use std::path::Path;

use cv_domain::Item;
use cv_rename::{MovieRenamer, Preset, TvRenamer};
use serde_json::Value as Json;

use crate::entity;
use crate::entity::read_entity;
use crate::rename::{build_episode, find_episode};

/// An entity of `<root>/rename/entities` (written for the presets), else one of the shared
/// fixture entities.
fn load_entity(root: &Path, name: &str) -> Result<Json, String> {
    let own = root.join("rename/entities").join(format!("{name}.json"));
    if let Ok(source) = fs::read_to_string(&own) {
        return serde_json::from_str(&source).map_err(|e| format!("{}: {e}", own.display()));
    }
    read_entity(&root.join("fixtures"), name).map_err(|e| format!("{e:?}"))
}

/// The outcome of one preset case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresetOutcome {
    pub id: String,
    /// One line per name that differs: `<what>: expected `..`, got `..``. Empty means it passed.
    pub mismatches: Vec<String>,
}

fn text<'a>(json: &'a Json, key: &str) -> &'a str {
    json.get(key).and_then(Json::as_str).unwrap_or_default()
}

fn compare(expect: &Json, key: &str, actual: Result<String, String>, out: &mut Vec<String>) {
    let want = text(expect, key);
    match actual {
        Ok(got) if got == want => {}
        Ok(got) => out.push(format!("{key}: expected `{want}`, got `{got}`")),
        Err(e) => out.push(format!("{key}: expected `{want}`, failed: {e}")),
    }
}

fn movie_case(case: &Json, preset: Preset, root: &Path) -> Result<Vec<String>, String> {
    let entity = load_entity(root, text(case, "entity"))?;
    let mut item = entity::movie_item(
        &entity,
        text(&entity, "folder"),
        &entity::entity_videos(&entity),
    );
    // The fixture movies are built without an import, so they have no original file name.
    item.core.original_filename.clear();
    let renamer = MovieRenamer::new(preset.movie_settings());
    let expect = case.get("expect").cloned().unwrap_or(Json::Null);
    let mut out = Vec::new();
    compare(
        &expect,
        "folder",
        renamer.folder_name(&item, None).map_err(|e| e.to_string()),
        &mut out,
    );
    compare(
        &expect,
        "file",
        renamer.file_name(&item, None).map_err(|e| e.to_string()),
        &mut out,
    );
    Ok(out)
}

fn tv_case(case: &Json, preset: Preset, root: &Path) -> Result<Vec<String>, String> {
    let entity = load_entity(root, text(case, "entity"))?;
    let show: Item = entity::show_item(&entity, text(&entity, "folder"));
    let selector = case.get("episode").ok_or("no `episode`")?;
    let number = |key: &str| {
        selector
            .get(key)
            .and_then(Json::as_u64)
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| format!("episode has no `{key}`"))
    };
    let (season, episode_nr) = (number("season")?, number("episode")?);
    let json = find_episode(&entity, season, episode_nr)
        .ok_or_else(|| format!("no episode S{season}E{episode_nr}"))?;
    let episode = build_episode(&show, json);
    let renamer = TvRenamer::new(preset.tv_settings());
    let expect = case.get("expect").cloned().unwrap_or(Json::Null);
    let mut out = Vec::new();
    compare(
        &expect,
        "show_folder",
        renamer.show_folder_name(&show).map_err(|e| e.to_string()),
        &mut out,
    );
    compare(
        &expect,
        "season_folder",
        renamer
            .season_folder_name(&show, &episode)
            .map_err(|e| e.to_string()),
        &mut out,
    );
    compare(
        &expect,
        "file",
        renamer
            .episode_file_name(&show, &[&episode])
            .map_err(|e| e.to_string()),
        &mut out,
    );
    Ok(out)
}

/// Runs every case of `<root>/rename/presets.json`.
///
/// # Errors
/// The file cannot be read or is malformed, or a case names an unknown preset, kind or entity.
pub fn run_preset_fixtures(root: &Path) -> Result<Vec<PresetOutcome>, String> {
    let path = root.join("rename/presets.json");
    let source = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let file: Json =
        serde_json::from_str(&source).map_err(|e| format!("{}: {e}", path.display()))?;
    let cases = file
        .get("cases")
        .and_then(Json::as_array)
        .ok_or("presets.json has no `cases`")?;
    let mut seen = std::collections::BTreeSet::new();
    let mut outcomes = Vec::new();
    for case in cases {
        let id = text(case, "id").to_owned();
        if id.is_empty() || !seen.insert(id.clone()) {
            return Err(format!("case id `{id}` is empty or repeated"));
        }
        let preset = Preset::parse(text(case, "preset"))
            .ok_or_else(|| format!("{id}: unknown preset `{}`", text(case, "preset")))?;
        let mismatches = match text(case, "kind") {
            "movie" => movie_case(case, preset, root),
            "tv" => tv_case(case, preset, root),
            other => Err(format!("unknown kind `{other}`")),
        }
        .map_err(|e| format!("{id}: {e}"))?;
        outcomes.push(PresetOutcome { id, mismatches });
    }
    Ok(outcomes)
}
