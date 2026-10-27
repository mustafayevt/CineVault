//! The naming-preset fixtures (`tests/compat/rename/presets.json`).

use cv_compat::default_root;
use cv_compat::presets::run_preset_fixtures;

#[test]
fn every_preset_fixture_renders_the_expected_names() {
    let outcomes = run_preset_fixtures(&default_root()).expect("fixtures load");
    assert!(outcomes.len() >= 20, "{} cases", outcomes.len());
    let failures: Vec<String> = outcomes
        .iter()
        .filter(|o| !o.mismatches.is_empty())
        .map(|o| format!("{}: {}", o.id, o.mismatches.join("; ")))
        .collect();
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

/// The Detailed pattern is the template the destination golden files pin
/// (`dest-267`/`dest-268`), so its tags are also checked with the space
/// options those cases set.
#[test]
fn the_detailed_pattern_is_the_one_the_destination_fixtures_pin() {
    let path = default_root().join("fixtures/rename/destinations.json");
    let source = std::fs::read_to_string(path).expect("destinations fixture");
    let fixture: serde_json::Value = serde_json::from_str(&source).expect("json");
    let pinned = |id: &str| {
        fixture["cases"]
            .as_array()
            .and_then(|cases| cases.iter().find(|c| c["id"] == id))
            .and_then(|c| c["template"].as_str().map(str::to_owned))
            .unwrap_or_else(|| panic!("no case {id}"))
    };
    let (folder, file) = cv_rename::Preset::Detailed.movie_patterns();
    assert_eq!(pinned("dest-267"), file);
    assert_eq!(pinned("dest-268"), folder);
}
