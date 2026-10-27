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
