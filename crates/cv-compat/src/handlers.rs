use std::collections::BTreeMap;

use cv_domain::{Item, MediaFileType};
use cv_rename::conflict::copies::{CopyAttributes, CopyMember, label_copies};
use cv_rename::conflict::{BatchItem, ConflictOptions, MemoryDisk, PathLimits, detect};
use cv_rename::{Fate, MovieInput, MovieRenamer, MovieSettings, PlanFile};
use serde_json::{Value, json};

use crate::family::Family;
use crate::fixture::Case;
use crate::run::{Actual, Handler, HandlerError};

/// The handlers for the families whose features exist.
///
/// Each handler is built on the feature crate it checks. A family without a handler reports its
/// fixtures as pending.
pub fn registered() -> Vec<Box<dyn Handler>> {
    vec![
        Box::new(ClassificationHandler),
        Box::new(TitleParsingHandler),
        Box::new(IdParsingHandler),
        Box::new(StackingHandler),
        Box::new(EpisodeHandler),
        Box::new(crate::rename::DestinationsHandler),
        Box::new(crate::rename::TreesHandler),
        Box::new(crate::rename::TvDestinationsHandler),
        Box::new(crate::rename::TvTreesHandler),
        Box::new(crate::nfo::NfoHandler),
        Box::new(CopiesHandler),
    ]
}

/// File-type classification: `{id, path}` to `{id, type, disc_file, main_disc_identifier}`.
struct ClassificationHandler;

impl Handler for ClassificationHandler {
    fn family(&self) -> Family {
        Family::Classification
    }

    fn run(&self, case: &Case) -> Result<Actual, HandlerError> {
        let path = case
            .input
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| HandlerError::Failed(format!("case `{}` has no `path`", case.id)))?;
        let found = cv_scan::classify::classify(path);
        let type_name = serde_json::to_value(found.file_type)
            .ok()
            .and_then(|v| v.as_str().map(str::to_uppercase))
            .ok_or_else(|| HandlerError::Failed("file type has no name".to_owned()))?;
        Ok(Actual::Json(json!({
            "id": case.id,
            "type": type_name,
            "disc_file": found.disc_file,
            "main_disc_identifier": found.main_disc_identifier,
        })))
    }
}

/// Title, year and edition-free name parsing: `{id, input, bad_words?}` to
/// `{id, title, year}`. An empty `year` means none.
struct TitleParsingHandler;

impl Handler for TitleParsingHandler {
    fn family(&self) -> Family {
        Family::TitleParsing
    }

    fn run(&self, case: &Case) -> Result<Actual, HandlerError> {
        let input = case
            .input
            .get("input")
            .and_then(Value::as_str)
            .ok_or_else(|| HandlerError::Failed(format!("case `{}` has no `input`", case.id)))?;
        let bad_words: Vec<&str> = case
            .input
            .get("bad_words")
            .and_then(Value::as_array)
            .map(|words| words.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let found = cv_scan::parse::detect_clean_title_and_year(input, &bad_words);
        Ok(Actual::Json(json!({
            "id": case.id,
            "title": found.title,
            "year": found.year.map(|y| y.to_string()).unwrap_or_default(),
        })))
    }
}

/// Provider ID detection: `{id, input}` to `{id, imdb, tmdb, tvdb}`. A missing id is
/// `""` (IMDb, TVDB) or `0` (TMDB).
struct IdParsingHandler;

impl Handler for IdParsingHandler {
    fn family(&self) -> Family {
        Family::IdParsing
    }

    fn run(&self, case: &Case) -> Result<Actual, HandlerError> {
        let input = case
            .input
            .get("input")
            .and_then(Value::as_str)
            .ok_or_else(|| HandlerError::Failed(format!("case `{}` has no `input`", case.id)))?;
        let ids = cv_scan::parse::detect_ids(input);
        Ok(Actual::Json(json!({
            "id": case.id,
            "imdb": ids.imdb.unwrap_or_default(),
            "tmdb": ids.tmdb.unwrap_or(0),
            "tvdb": ids.tvdb.unwrap_or_default(),
        })))
    }
}

/// Stacking markers: `{id, input}` to
/// `{id, marker, number, prefix, cleaned, folder_marker, folder_cleaned}`. The input is read both
/// as a file name and as a folder name.
struct StackingHandler;

impl Handler for StackingHandler {
    fn family(&self) -> Family {
        Family::Stacking
    }

    fn run(&self, case: &Case) -> Result<Actual, HandlerError> {
        let input = case
            .input
            .get("input")
            .and_then(Value::as_str)
            .ok_or_else(|| HandlerError::Failed(format!("case `{}` has no `input`", case.id)))?;
        let file = cv_scan::stacking::parse_file_name(input);
        let folder = cv_scan::stacking::parse_folder_name(input);
        Ok(Actual::Json(json!({
            "id": case.id,
            "marker": file.marker,
            "number": file.number,
            "prefix": file.prefix,
            "cleaned": file.cleaned,
            "folder_marker": folder.marker,
            "folder_cleaned": folder.cleaned,
        })))
    }
}

/// Episode and season parsing: `{id, input, show}` to
/// `{id, season, episodes, date, stacking_marker_found, cleaned_title}`. The input is a path
/// relative to the show folder. `season` is `-1` when none was found and `date` is `yyyy-MM-dd`
/// or `""`.
struct EpisodeHandler;

impl Handler for EpisodeHandler {
    fn family(&self) -> Family {
        Family::Episodes
    }

    fn run(&self, case: &Case) -> Result<Actual, HandlerError> {
        let input = case
            .input
            .get("input")
            .and_then(Value::as_str)
            .ok_or_else(|| HandlerError::Failed(format!("case `{}` has no `input`", case.id)))?;
        let show = case
            .input
            .get("show")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let found = cv_scan::episode::detect_episode_from_filename(input, show);
        Ok(Actual::Json(json!({
            "id": case.id,
            "season": found.season.map_or(-1, i64::from),
            "episodes": found.episodes,
            "date": found.date.map(|d| d.to_string()).unwrap_or_default(),
            "stacking_marker_found": found.stacking_marker_found,
            "cleaned_title": found.cleaned_title,
        })))
    }
}

/// Copy labels: copies of one movie renamed together under the default profile, `{id, title,
/// year, copies}` with each copy `{id, folder, video, copy?, resolution?, hdr?, source?, audio?}`
/// (`copy: false` for a movie that has no copy), to `{id, conflicts, copies}` with each copy's
/// `{id, label, folder, files}`: its label (empty for none), new folder and sorted new paths.
struct CopiesHandler;

impl Handler for CopiesHandler {
    fn family(&self) -> Family {
        Family::RenameCopies
    }

    fn run(&self, case: &Case) -> Result<Actual, HandlerError> {
        let field = |json: &Value, key: &str| -> String {
            json.get(key)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let mut movie = Item::movie(field(&case.input, "title"));
        movie.core.year = case
            .input
            .get("year")
            .and_then(Value::as_u64)
            .and_then(|y| u16::try_from(y).ok())
            .unwrap_or(0);
        let copies = case
            .input
            .get("copies")
            .and_then(Value::as_array)
            .ok_or_else(|| HandlerError::Failed(format!("case `{}` has no `copies`", case.id)))?;

        let renamer = MovieRenamer::new(MovieSettings::default());
        let mut items = Vec::new();
        let mut members = BTreeMap::new();
        for copy in copies {
            let id = copy.get("id").and_then(Value::as_i64).unwrap_or_default();
            let folder = field(copy, "folder");
            let video = field(copy, "video");
            let stem = video
                .rsplit_once('.')
                .map_or(video.as_str(), |(stem, _)| stem);
            let files = vec![
                PlanFile::new(format!("{folder}/{video}"), MediaFileType::Video),
                PlanFile::new(format!("{folder}/{stem}.nfo"), MediaFileType::Nfo),
                PlanFile::new(format!("{folder}/{stem}.srt"), MediaFileType::Subtitle),
            ];
            let plan = renamer
                .plan(&MovieInput {
                    item: &movie,
                    set: None,
                    folder: &folder,
                    files: &files,
                })
                .map_err(|e| HandlerError::Failed(e.to_string()))?;
            items.push(BatchItem::from_movie(id.to_string(), "D:/Movies", &plan));
            if copy.get("copy").and_then(Value::as_bool) != Some(false) {
                members.insert(
                    id.to_string(),
                    CopyMember {
                        group: 1,
                        library_id: id,
                        attributes: CopyAttributes {
                            resolution: field(copy, "resolution"),
                            hdr: field(copy, "hdr"),
                            source: field(copy, "source"),
                            audio: field(copy, "audio"),
                        },
                    },
                );
            }
        }

        let options = ConflictOptions {
            case_insensitive: true,
            limits: PathLimits::windows_long_paths(),
            ..ConflictOptions::native()
        };
        let labels = label_copies(&mut items, &members, &options);
        let report = detect(&items, &MemoryDisk::new(true), &options);
        let results: Vec<Value> = items
            .iter()
            .map(|item| {
                let mut files: Vec<&str> = item
                    .files
                    .iter()
                    .filter_map(|f| match &f.fate {
                        Fate::Carried { to } => to.first().map(String::as_str),
                        _ => None,
                    })
                    .collect();
                files.sort_unstable();
                json!({
                    "id": item.id,
                    "label": labels.get(&item.id).cloned().unwrap_or_default(),
                    "folder": item.new_folder,
                    "files": files,
                })
            })
            .collect();
        Ok(Actual::Json(json!({
            "id": case.id,
            "conflicts": report.conflicts.len(),
            "copies": results,
        })))
    }
}
