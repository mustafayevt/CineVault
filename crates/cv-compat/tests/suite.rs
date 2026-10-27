//! The golden-file suite over the real `tests/compat` folder.

mod common;

use common::{Mode, Scripted, all_families};
use cv_compat::{
    Family, Outcome, default_root, load, registered, run_suite, update_expected, update_requested,
};

#[test]
fn the_real_fixture_tree_loads_and_every_family_has_cases() {
    let suite = load(&default_root()).expect("tests/compat loads");
    for family in Family::ALL {
        assert!(
            suite.cases_of(family).count() > 0,
            "no fixtures for {family}"
        );
    }
}

/// The official run. It prints the per-fixture report (visible with `--nocapture`) and fails when
/// any fixture fails or is pending.
///
/// With `CINEVAULT_UPDATE_COMPAT=1` it first writes CineVault's current output over the expected
/// files, so an intended change in behaviour updates the golden files in one step. Review the
/// diff of `tests/compat/expected` before committing it.
#[test]
fn compatibility_suite() {
    let root = default_root();
    if update_requested() {
        let suite = load(&root).expect("tests/compat loads");
        let changed = update_expected(&suite, &registered()).expect("golden files update");
        println!("updated {changed} expected file(s)");
    }
    let suite = load(&root).expect("tests/compat loads");
    let report = run_suite(&suite, &registered());
    let text = report.render(true);
    println!("{text}");
    assert!(report.is_complete(), "golden-file fixtures failed:\n{text}");
}

#[test]
fn every_case_is_pending_when_no_handler_is_registered() {
    let suite = load(&default_root()).expect("tests/compat loads");
    let report = run_suite(&suite, &[]);
    assert_eq!(report.counts().pending, suite.cases.len());
    assert!(report.is_green());
    assert!(!report.is_complete());
}

/// A handler that returns the expected output passes every real fixture.
#[test]
fn a_faithful_handler_passes_every_real_fixture() {
    let suite = load(&default_root()).expect("tests/compat loads");
    let report = run_suite(&suite, &all_families(Mode::Faithful));
    assert_eq!(
        report.counts().pass,
        suite.cases.len(),
        "{}",
        report.render(false)
    );
    assert!(report.is_complete());
}

#[test]
fn a_broken_handler_fails_each_fixture_individually() {
    let suite = load(&default_root()).expect("tests/compat loads");
    let report = run_suite(&suite, &all_families(Mode::Broken));
    assert_eq!(report.counts().fail, suite.cases.len());
    assert!(!report.is_green());
    let text = report.render(false);
    assert!(
        text.contains("FAIL     classification/classify-001"),
        "{text}"
    );
    assert!(text.contains("FAIL     nfo/movie-kodi-full"), "{text}");
}

#[test]
fn one_failing_family_does_not_hide_the_others() {
    let suite = load(&default_root()).expect("tests/compat loads");
    let mut handlers = all_families(Mode::Faithful);
    handlers.retain(|h| h.family() != Family::Stacking && h.family() != Family::Episodes);
    handlers.push(Box::new(Scripted {
        family: Family::Stacking,
        mode: Mode::Broken,
    }));
    handlers.push(Box::new(Scripted {
        family: Family::Episodes,
        mode: Mode::Panic,
    }));
    let report = run_suite(&suite, &handlers);
    assert!(matches!(
        report.outcome(Family::Stacking, "stack-001"),
        Some(Outcome::Fail(_))
    ));
    assert!(matches!(
        report.outcome(Family::Episodes, "episode-001"),
        Some(Outcome::Fail(d)) if d.contains("panicked")
    ));
    assert_eq!(
        report.outcome(Family::Classification, "classify-001"),
        Some(&Outcome::Pass)
    );
    assert_eq!(
        report.counts_for(Family::Stacking).fail,
        suite.cases_of(Family::Stacking).count()
    );
}
