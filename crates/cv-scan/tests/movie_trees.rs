//! File-tree fixtures for the movie folder walker: `tests/compat/scan/movie-trees.json`.
//!
//! Each case lists the files of a data source. The test creates them (empty) in a temporary
//! folder, walks it and compares the movies with the expected groupings.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use cv_scan::walk::{ScanReport, ScannedMovie, WalkOptions, scan_movie_source};
use serde_json::Value;

fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/compat/scan/movie-trees.json")
}

fn materialize(root: &Path, files: &[Value]) {
    for file in files {
        let rel = file.as_str().expect("file path");
        let path = root.join(rel);
        fs::create_dir_all(path.parent().expect("parent")).expect("create folder");
        fs::write(&path, b"").expect("create file");
    }
}

fn kind_name(movie_file: &cv_scan::walk::ScannedFile) -> String {
    serde_json::to_value(movie_file.kind)
        .expect("serialise type")
        .as_str()
        .expect("type name")
        .to_owned()
}

fn describe(movie: &ScannedMovie) -> String {
    let files: BTreeMap<String, String> = movie
        .files
        .iter()
        .map(|f| (f.relative.clone(), kind_name(f)))
        .collect();
    format!(
        "{} | {} | {:?} | multi={} disc={} | {files:?}",
        movie.relative_path, movie.title, movie.year, movie.multi_movie_dir, movie.disc
    )
}

fn check(case_id: &str, actual: &ScannedMovie, expected: &Value) {
    let what = |field: &str| format!("case {case_id}, movie {}: {field}", actual.title);
    assert_eq!(
        actual.relative_path,
        expected["path"].as_str().expect("path"),
        "{}",
        what("path")
    );
    assert_eq!(
        actual.title,
        expected["title"].as_str().expect("title"),
        "{}",
        what("title")
    );
    assert_eq!(
        actual.year.map(i64::from),
        expected["year"].as_i64(),
        "{}",
        what("year")
    );
    for (field, value) in [("multi", actual.multi_movie_dir), ("disc", actual.disc)] {
        assert_eq!(
            value,
            expected[field].as_bool().unwrap_or(false),
            "{}",
            what(field)
        );
    }
    if let Some(want) = expected["three_d"].as_bool() {
        assert_eq!(actual.three_d, want, "{}", what("three_d"));
    }
    if let Some(want) = expected["edition"].as_str() {
        assert_eq!(actual.edition.name(), want, "{}", what("edition"));
    }
    if let Some(want) = expected["media_source"].as_str() {
        assert_eq!(actual.media_source.name(), want, "{}", what("media_source"));
    }
    if let Some(want) = expected["imdb"].as_str() {
        assert_eq!(actual.ids.imdb.as_deref(), Some(want), "{}", what("imdb"));
    }
    if let Some(want) = expected["tmdb"].as_u64() {
        assert_eq!(
            actual.ids.tmdb.map(u64::from),
            Some(want),
            "{}",
            what("tmdb")
        );
    }

    let actual_files: BTreeMap<String, String> = actual
        .files
        .iter()
        .map(|f| (f.relative.clone(), kind_name(f)))
        .collect();
    let expected_files: BTreeMap<String, String> = expected["files"]
        .as_object()
        .expect("files")
        .iter()
        .map(|(k, v)| (k.clone(), v.as_str().expect("type").to_owned()))
        .collect();
    assert_eq!(actual_files, expected_files, "{}", what("files"));

    if let Some(parts) = expected["parts"].as_object() {
        for (rel, number) in parts {
            let file = actual
                .files
                .iter()
                .find(|f| &f.relative == rel)
                .unwrap_or_else(|| panic!("{}", what("part file")));
            assert_eq!(
                u64::from(file.stacking),
                number.as_u64().expect("part number"),
                "{}",
                what(&format!("part number of {rel}"))
            );
        }
        // Parts come first in part order.
        let order: Vec<u32> = actual
            .files
            .iter()
            .filter(|f| parts.contains_key(&f.relative))
            .map(|f| f.stacking)
            .collect();
        assert!(order.is_sorted(), "{}", what("parts in order"));
    }
}

#[test]
fn file_tree_fixtures_produce_the_expected_movies() {
    let text = fs::read_to_string(fixture_path()).expect("fixture file");
    let fixture: Value = serde_json::from_str(&text).expect("fixture json");
    let cases = fixture["cases"].as_array().expect("cases");
    assert!(cases.len() >= 20, "fixture lost cases");
    let mut ids = std::collections::BTreeSet::new();

    for case in cases {
        let id = case["id"].as_str().expect("case id");
        assert!(ids.insert(id.to_owned()), "duplicate case id {id}");
        // The default temp name starts with a dot, which the skip rules treat as hidden.
        let dir = tempfile::Builder::new()
            .prefix("cvscan")
            .tempdir()
            .expect("temp dir");
        materialize(dir.path(), case["files"].as_array().expect("files"));

        let report: ScanReport = scan_movie_source(dir.path(), None, &WalkOptions::default());
        let mut expected: Vec<&Value> = case["movies"].as_array().expect("movies").iter().collect();
        expected.sort_by_key(|m| {
            (
                m["path"].as_str().unwrap_or_default().to_lowercase(),
                m["title"].as_str().unwrap_or_default().to_lowercase(),
            )
        });
        let actual_summary: Vec<String> = report.movies.iter().map(describe).collect();
        assert_eq!(
            report.movies.len(),
            expected.len(),
            "case {id}: movie count differs; got {actual_summary:#?}"
        );
        for (actual, want) in report.movies.iter().zip(expected) {
            check(id, actual, want);
        }
        assert!(
            report.problems.is_empty(),
            "case {id}: unexpected problems {:?}",
            report.problems
        );
    }
}

/// A folder renamed without the edition whose video still names it keeps the edition; the
/// folder's own edition wins when both name one.
#[test]
fn the_edition_comes_from_the_video_when_the_folder_names_none() {
    let dir = tempfile::Builder::new()
        .prefix("cvscan")
        .tempdir()
        .expect("temp dir");
    let files = [
        "13 Assassins (2010) [1080p]/13 Assassins - Extended Edition (2010) [1080p] [DTSHD-MA 5.1].mkv",
        "Mimic (1997)/Mimic - Director's Cut (1997) [1080p].mkv",
        "The Thing (1982)/The Thing - Uncut (1982).mkv",
        "Alien - Theatrical Edition (1979)/Alien - Directors Cut (1979).mkv",
        "Heat (1995)/Heat (1995).mkv",
    ]
    .map(Value::from);
    materialize(dir.path(), &files);
    let report = scan_movie_source(dir.path(), None, &WalkOptions::default());
    let editions: BTreeMap<String, String> = report
        .movies
        .iter()
        .map(|m| (m.title.clone(), m.edition.name().to_owned()))
        .collect();
    assert_eq!(editions["13 Assassins"], "EXTENDED_EDITION");
    assert_eq!(editions["Mimic"], "DIRECTORS_CUT");
    assert_eq!(editions["The Thing"], "UNCUT");
    assert_eq!(editions["Alien"], "THEATRICAL_EDITION");
    assert_eq!(editions["Heat"], "NONE");
}
