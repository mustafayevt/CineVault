//! Language and flags of external audio and subtitle files, checked against a table of file
//! names (`tests/fixtures/external/streams.json`): each case lists the video's base name, the
//! file name and the expected language, title and flags.

use std::collections::BTreeSet;
use std::path::PathBuf;

use cv_domain::StreamFlag;
use cv_media::external::detect_external_stream;
use serde::Deserialize;

#[derive(Deserialize)]
struct Fixture {
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    id: String,
    video: String,
    file: String,
    language: String,
    title: String,
    flags: Vec<StreamFlag>,
}

fn load() -> Fixture {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/external/streams.json");
    let text = std::fs::read_to_string(&path).expect("fixture is readable");
    serde_json::from_str(&text).expect("fixture parses")
}

#[test]
fn every_filename_fixture_gives_the_expected_language_title_and_flags() {
    let fixture = load();
    assert!(fixture.cases.len() >= 50, "the table should stay broad");
    let mut failures = Vec::new();
    for case in &fixture.cases {
        let got = detect_external_stream(&case.video, &case.file);
        let want_flags: BTreeSet<StreamFlag> = case.flags.iter().copied().collect();
        if got.language != case.language || got.title != case.title || got.flags != want_flags {
            failures.push(format!(
                "{}: {:?} / {:?}\n   expected language {:?}, title {:?}, flags {:?}\n   got      language {:?}, title {:?}, flags {:?}",
                case.id, case.video, case.file, case.language, case.title, want_flags,
                got.language, got.title, got.flags
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} cases differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn fixture_ids_are_unique() {
    let fixture = load();
    let ids: BTreeSet<&str> = fixture.cases.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids.len(), fixture.cases.len());
}
