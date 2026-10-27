//! File-tree fixtures for the TV show walker: `tests/compat/scan/tv-trees.json`.
//!
//! Each case lists the files of a data source. The test creates them (empty) in a temporary
//! folder, walks it and compares the shows, seasons and episodes with the expected groupings.
//! Shows are compared in path order and episodes in output order (season, first number, path).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use cv_scan::walk::{
    ProblemKind, ScannedEpisode, ScannedFile, ScannedShow, WalkOptions, scan_tv_source,
};
use serde_json::Value;

fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/compat/scan/tv-trees.json")
}

fn materialize(root: &Path, files: &[Value]) {
    for file in files {
        let rel = file.as_str().expect("file path");
        let path = root.join(rel);
        fs::create_dir_all(path.parent().expect("parent")).expect("create folder");
        fs::write(&path, b"").expect("create file");
    }
}

fn kind_name(file: &ScannedFile) -> String {
    serde_json::to_value(file.kind)
        .expect("serialise type")
        .as_str()
        .expect("type name")
        .to_owned()
}

fn actual_files(files: &[ScannedFile]) -> BTreeMap<String, String> {
    files
        .iter()
        .map(|f| (f.relative.clone(), kind_name(f)))
        .collect()
}

fn expected_files(value: &Value, what: &str) -> BTreeMap<String, String> {
    value
        .as_object()
        .unwrap_or_else(|| panic!("{what}: files must be an object"))
        .iter()
        .map(|(k, v)| (k.clone(), v.as_str().expect("type name").to_owned()))
        .collect()
}

fn problem_name(kind: ProblemKind) -> &'static str {
    match kind {
        ProblemKind::SourceUnavailable => "source_unavailable",
        ProblemKind::SourceEmpty => "source_empty",
        ProblemKind::DiscFolderInRoot => "disc_folder_in_root",
        ProblemKind::Unreadable => "unreadable",
        ProblemKind::EpisodeInRoot => "episode_in_root",
    }
}

fn describe(show: &ScannedShow) -> String {
    let episodes: Vec<String> = show
        .episodes
        .iter()
        .map(|e| {
            format!(
                "S{:?} E{:?} date={:?} title={:?} {:?}",
                e.season,
                e.numbers,
                e.date.map(|d| d.to_string()),
                e.title,
                actual_files(&e.files)
            )
        })
        .collect();
    format!(
        "{} | {} | {:?} | bucket={:?} | files {:?} | seasons {:?} | episodes {episodes:#?}",
        show.relative_path,
        show.title,
        show.year,
        show.bucket,
        actual_files(&show.files),
        show.seasons
            .iter()
            .map(|s| (s.number, actual_files(&s.files)))
            .collect::<Vec<_>>(),
    )
}

fn check_episode(case: &str, show: &str, index: usize, actual: &ScannedEpisode, want: &Value) {
    let what = |field: &str| format!("case {case}, show {show}, episode #{index}: {field}");
    assert_eq!(
        actual.season.map(i64::from),
        want["season"].as_i64(),
        "{}",
        what("season")
    );
    let numbers: Vec<u64> = want["numbers"]
        .as_array()
        .expect("numbers")
        .iter()
        .map(|n| n.as_u64().expect("number"))
        .collect();
    let got: Vec<u64> = actual.numbers.iter().map(|n| u64::from(*n)).collect();
    assert_eq!(got, numbers, "{}", what("numbers"));
    assert_eq!(
        actual.date.map(|d| d.to_string()),
        want["date"].as_str().map(str::to_owned),
        "{}",
        what("date")
    );
    if let Some(title) = want["title"].as_str() {
        assert_eq!(actual.title, title, "{}", what("title"));
    }
    assert_eq!(
        actual.disc,
        want["disc"].as_bool().unwrap_or(false),
        "{}",
        what("disc")
    );
    assert_eq!(
        actual_files(&actual.files),
        expected_files(&want["files"], &what("files")),
        "{}",
        what("files")
    );
    if let Some(parts) = want["parts"].as_object() {
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
        let order: Vec<u32> = actual
            .files
            .iter()
            .filter(|f| parts.contains_key(&f.relative))
            .map(|f| f.stacking)
            .collect();
        assert!(order.is_sorted(), "{}", what("parts in order"));
    }
}

fn check_show(case: &str, actual: &ScannedShow, want: &Value) {
    let what = |field: &str| format!("case {case}, show {}: {field}", actual.title);
    assert_eq!(
        actual.relative_path,
        want["path"].as_str().expect("path"),
        "{}",
        what("path")
    );
    assert_eq!(
        actual.title,
        want["title"].as_str().expect("title"),
        "{}",
        what("title")
    );
    assert_eq!(
        actual.year.map(i64::from),
        want["year"].as_i64(),
        "{}",
        what("year")
    );
    assert_eq!(
        actual.bucket.as_deref(),
        want["bucket"].as_str(),
        "{}",
        what("bucket")
    );
    if let Some(id) = want["tvdb"].as_str() {
        assert_eq!(actual.ids.tvdb.as_deref(), Some(id), "{}", what("tvdb"));
    }
    if let Some(id) = want["tmdb"].as_u64() {
        assert_eq!(actual.ids.tmdb.map(u64::from), Some(id), "{}", what("tmdb"));
    }
    assert_eq!(
        actual_files(&actual.files),
        expected_files(&want["files"], &what("files")),
        "{}",
        what("show files")
    );

    let wanted_seasons = want["seasons"].as_object().cloned().unwrap_or_default();
    let got_seasons: BTreeMap<String, BTreeMap<String, String>> = actual
        .seasons
        .iter()
        .map(|s| (s.number.to_string(), actual_files(&s.files)))
        .collect();
    let want_seasons: BTreeMap<String, BTreeMap<String, String>> = wanted_seasons
        .iter()
        .map(|(k, v)| (k.clone(), expected_files(v, &what("season files"))))
        .collect();
    assert_eq!(got_seasons, want_seasons, "{}", what("seasons"));

    let wanted: Vec<&Value> = want["episodes"]
        .as_array()
        .expect("episodes")
        .iter()
        .collect();
    assert_eq!(
        actual.episodes.len(),
        wanted.len(),
        "{}; got {:#?}",
        what("episode count"),
        describe(actual)
    );
    for (index, (episode, want)) in actual.episodes.iter().zip(wanted).enumerate() {
        check_episode(case, &actual.title, index, episode, want);
    }
}

#[test]
fn tv_file_tree_fixtures_produce_the_expected_groupings() {
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

        let report = scan_tv_source(dir.path(), None, &WalkOptions::tv_defaults());
        let wanted: Vec<&Value> = case["shows"].as_array().expect("shows").iter().collect();
        let summary: Vec<String> = report.shows.iter().map(describe).collect();
        assert_eq!(
            report.shows.len(),
            wanted.len(),
            "case {id}: show count differs; got {summary:#?}"
        );
        for (actual, want) in report.shows.iter().zip(wanted) {
            check_show(id, actual, want);
        }

        let got_problems: Vec<&str> = report
            .problems
            .iter()
            .map(|p| problem_name(p.kind))
            .collect();
        let want_problems: Vec<&str> = case["problems"]
            .as_array()
            .map(|a| a.iter().map(|p| p.as_str().expect("problem")).collect())
            .unwrap_or_default();
        assert_eq!(got_problems, want_problems, "case {id}: problems");
    }
}
