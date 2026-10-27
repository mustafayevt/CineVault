//! Golden skip cases in `tests/golden/skip-rules.json`: the "Skip rules" requirement of the library-scanning spec.

use std::path::PathBuf;

use cv_scan::skip::{LibraryKind, SkipRules};
use serde_json::Value;

fn fixture() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden/skip-rules.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).expect("skip-rules.json is valid JSON")
}

fn kind(case: &Value) -> LibraryKind {
    match case["kind"].as_str().expect("kind") {
        "movies" => LibraryKind::Movies,
        "tv" => LibraryKind::TvShows,
        other => panic!("unknown kind {other}"),
    }
}

fn rules(case: &Value) -> SkipRules {
    let patterns: Vec<String> = case["user_patterns"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|p| p.as_str().expect("pattern").to_owned())
                .collect()
        })
        .unwrap_or_default();
    let nomedia = case["skip_nomedia"].as_bool().unwrap_or(true);
    SkipRules::new(kind(case), &patterns, nomedia)
}

#[test]
fn folder_cases_match_the_golden_results() {
    let fixture = fixture();
    let cases = fixture["folders"].as_array().expect("folders");
    assert!(cases.len() >= 40, "the golden set must stay broad");
    let mut failures = Vec::new();
    for case in cases {
        let got = rules(case).is_skipped_folder(
            case["name"].as_str().expect("name"),
            case["path"].as_str().expect("path"),
        );
        if got != case["skipped"].as_bool().expect("skipped") {
            failures.push(format!("{}: got skipped={got}", case["id"]));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn marker_cases_match_the_golden_results() {
    let fixture = fixture();
    let cases = fixture["markers"].as_array().expect("markers");
    let mut failures = Vec::new();
    for case in cases {
        let files: Vec<&str> = case["files"]
            .as_array()
            .expect("files")
            .iter()
            .map(|f| f.as_str().expect("file"))
            .collect();
        let got = rules(case).folder_has_marker(files);
        if got != case["hidden"].as_bool().expect("hidden") {
            failures.push(format!("{}: got hidden={got}", case["id"]));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn fixture_ids_are_unique() {
    let fixture = fixture();
    let mut ids: Vec<&str> = ["folders", "markers"]
        .iter()
        .flat_map(|k| fixture[k].as_array().expect("array"))
        .map(|c| c["id"].as_str().expect("id"))
        .collect();
    let total = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), total);
}

#[test]
fn new_installation_defaults_skip_makemkv_and_honour_nomedia() {
    let rules = SkipRules::with_defaults(LibraryKind::Movies);
    assert!(rules.is_skipped_folder("MAKEMKV", "/m/MAKEMKV"));
    assert!(!rules.is_skipped_folder("Heat (1995)", "/m/Heat (1995)"));
    assert!(rules.skips_nomedia());
    assert!(rules.literal_patterns().is_empty());
}

#[test]
fn invalid_patterns_are_reported_as_literal() {
    let rules = SkipRules::new(
        LibraryKind::Movies,
        &["[Samples", "Feature.*", "a.b("],
        true,
    );
    assert_eq!(rules.literal_patterns(), vec!["[Samples", "a.b("]);
}

#[test]
fn dir_has_marker_reads_the_folder_but_not_its_children() {
    let rules = SkipRules::with_defaults(LibraryKind::Movies);
    let dir = tempfile::tempdir().expect("tempdir");
    let movie = dir.path().join("Heat (1995)");
    let child = movie.join("Extras");
    std::fs::create_dir_all(&child).expect("create");
    std::fs::write(movie.join("Heat (1995).mkv"), b"").expect("write");
    assert!(!rules.dir_has_marker(&movie));
    std::fs::write(child.join(".cinevaultignore"), b"").expect("write");
    assert!(rules.dir_has_marker(&child));
    assert!(
        !rules.dir_has_marker(&movie),
        "a marker only hides its own folder and below"
    );
    std::fs::write(movie.join(".nomedia"), b"").expect("write");
    assert!(rules.dir_has_marker(&movie));
    let off = SkipRules::new(LibraryKind::Movies, &[] as &[&str], false);
    assert!(!off.dir_has_marker(&movie));
    assert!(!rules.dir_has_marker(&dir.path().join("missing")));
}

#[test]
fn a_folder_named_like_a_marker_is_not_a_marker() {
    let rules = SkipRules::with_defaults(LibraryKind::Movies);
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir(dir.path().join(".nomedia")).expect("create");
    assert!(!rules.dir_has_marker(dir.path()));
}
