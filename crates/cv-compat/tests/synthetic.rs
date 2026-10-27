//! The runner against a tiny self-contained fixture tree, independent of `tests/compat`.

mod common;

use std::fs;
use std::path::Path;

use common::{Mode, Scripted, all_families};
use cv_compat::{
    Actual, Case, Family, Handler, HandlerError, Layout, LoadError, Outcome, load, run_suite,
    update_expected,
};
use tempfile::TempDir;

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    fs::write(path, text).expect("write");
}

/// One case `<family>-1` per family, with minimal inputs and expected outputs.
fn sample_tree() -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    for family in Family::ALL {
        let id = format!("{}-1", family.name());
        match family.layout() {
            Layout::Json { fixture, expected } => {
                write(
                    &root.join("fixtures").join(fixture),
                    &format!(r#"{{"version":1,"cases":[{{"id":"{id}","input":"x"}}]}}"#),
                );
                write(
                    &root.join("expected").join(expected),
                    &format!(r#"{{"version":1,"cases":[{{"id":"{id}","result":"y"}}]}}"#),
                );
            }
            Layout::Files {
                fixture,
                expected_dir,
            } => {
                write(
                    &root.join("fixtures").join(fixture),
                    &format!(r#"{{"version":1,"cases":[{{"id":"{id}"}}]}}"#),
                );
                let base = root.join("expected").join(expected_dir);
                write(
                    &base.join("index.json"),
                    &format!(r#"{{"version":1,"cases":[{{"id":"{id}","files":["a/out.nfo"]}}]}}"#),
                );
                write(
                    &base.join(&id).join("a/out.nfo"),
                    "<?xml?>\r\n<!--created on {timestamp}-->\r\n<a>{apikey}</a>\r\n",
                );
            }
        }
    }
    dir
}

#[test]
fn reports_pass_fail_and_pending_per_fixture() {
    let dir = sample_tree();
    let suite = load(dir.path()).expect("loads");
    assert_eq!(suite.cases.len(), Family::ALL.len());

    let mut handlers: Vec<Box<dyn Handler>> = Vec::new();
    for family in Family::ALL {
        let mode = match family {
            Family::Classification | Family::Nfo => Mode::Faithful,
            Family::Stacking => Mode::Broken,
            Family::IdParsing => Mode::Pending,
            Family::TitleParsing => Mode::Panic,
            _ => continue, // no handler: pending
        };
        handlers.push(Box::new(Scripted { family, mode }));
    }
    let report = run_suite(&suite, &handlers);

    assert_eq!(
        report.outcome(Family::Classification, "classification-1"),
        Some(&Outcome::Pass)
    );
    assert_eq!(report.outcome(Family::Nfo, "nfo-1"), Some(&Outcome::Pass));
    assert!(matches!(
        report.outcome(Family::Stacking, "stacking-1"),
        Some(Outcome::Fail(_))
    ));
    assert!(matches!(
        report.outcome(Family::TitleParsing, "title-parsing-1"),
        Some(Outcome::Fail(_))
    ));
    assert!(matches!(
        report.outcome(Family::IdParsing, "id-parsing-1"),
        Some(Outcome::Pending(_))
    ));
    assert!(matches!(
        report.outcome(Family::RenameTreesTv, "rename-trees-tv-1"),
        Some(Outcome::Pending(_))
    ));

    let counts = report.counts();
    assert_eq!((counts.pass, counts.fail), (2, 2));
    assert_eq!(counts.pending, Family::ALL.len() - 4);
    assert!(!report.is_green());

    let text = report.render(true);
    assert!(
        text.contains("PASS     classification/classification-1"),
        "{text}"
    );
    assert!(text.contains("FAIL     stacking/stacking-1"), "{text}");
    assert!(text.contains("PENDING  id-parsing/id-parsing-1"), "{text}");
    assert!(
        text.contains("10 fixtures: 2 passed, 2 failed, 6 pending"),
        "{text}"
    );
}

/// Writes fixed new output for every case: JSON families get `{"result": "new"}`, the NFO
/// family a different file set.
struct Rewriting(Family);

impl Handler for Rewriting {
    fn family(&self) -> Family {
        self.0
    }

    fn run(&self, case: &Case) -> Result<Actual, HandlerError> {
        Ok(match case.family.layout() {
            Layout::Json { .. } => {
                Actual::Json(serde_json::json!({"id": case.id, "result": "new"}))
            }
            Layout::Files { .. } => Actual::Files(
                [("b/new.nfo".to_owned(), b"<b/>\r\n".to_vec())]
                    .into_iter()
                    .collect(),
            ),
        })
    }
}

#[test]
fn update_mode_writes_the_current_output_as_the_golden_files() {
    let dir = sample_tree();
    let suite = load(dir.path()).expect("loads");
    let handlers: Vec<Box<dyn Handler>> = Family::ALL
        .into_iter()
        .map(|f| Box::new(Rewriting(f)) as Box<dyn Handler>)
        .collect();
    assert_eq!(run_suite(&suite, &handlers).counts().pass, 0);

    let changed = update_expected(&suite, &handlers).expect("updates");
    assert!(changed >= Family::ALL.len(), "{changed}");

    let suite = load(dir.path()).expect("reloads after the update");
    let report = run_suite(&suite, &handlers);
    assert!(report.is_complete(), "{}", report.render(true));
    // The file the case no longer writes is gone, the new one is listed in the index.
    let nfo = dir.path().join("expected/nfo/nfo-1");
    assert!(!nfo.join("a/out.nfo").exists());
    assert_eq!(
        fs::read(nfo.join("b/new.nfo")).expect("new file"),
        b"<b/>\r\n"
    );
    // A second update changes nothing.
    assert_eq!(update_expected(&suite, &handlers).expect("updates"), 0);
}

#[test]
fn update_mode_writes_nothing_when_a_case_fails() {
    let dir = sample_tree();
    let suite = load(dir.path()).expect("loads");
    let before = fs::read(dir.path().join("expected/stacking.json")).expect("read");
    let mut handlers: Vec<Box<dyn Handler>> = Family::ALL
        .into_iter()
        .filter(|f| *f != Family::Episodes)
        .map(|f| Box::new(Rewriting(f)) as Box<dyn Handler>)
        .collect();
    handlers.push(Box::new(Scripted {
        family: Family::Episodes,
        mode: Mode::Panic,
    }));
    assert!(update_expected(&suite, &handlers).is_err());
    assert_eq!(
        fs::read(dir.path().join("expected/stacking.json")).expect("read"),
        before
    );
}

#[test]
fn file_mismatch_names_the_file() {
    let dir = sample_tree();
    let suite = load(dir.path()).expect("loads");
    let report = run_suite(&suite, &all_families(Mode::Broken));
    let Some(Outcome::Fail(detail)) = report.outcome(Family::Nfo, "nfo-1") else {
        panic!("expected a failure");
    };
    assert!(detail.contains("a/out.nfo"), "{detail}");
}

#[test]
fn a_broken_tree_is_a_load_error_not_a_skip() {
    // expected output without a fixture case
    let dir = sample_tree();
    write(
        &dir.path().join("expected/stacking.json"),
        r#"{"version":1,"cases":[{"id":"stacking-1"},{"id":"stacking-2"}]}"#,
    );
    assert!(matches!(
        load(dir.path()),
        Err(LoadError::UnexpectedExpected {
            family: Family::Stacking,
            ..
        })
    ));

    // fixture case without expected output
    let dir = sample_tree();
    write(
        &dir.path().join("expected/stacking.json"),
        r#"{"version":1,"cases":[]}"#,
    );
    assert!(matches!(
        load(dir.path()),
        Err(LoadError::MissingExpected {
            family: Family::Stacking,
            ..
        })
    ));

    // file on disk that the index does not list
    let dir = sample_tree();
    write(&dir.path().join("expected/nfo/nfo-1/extra.nfo"), "<a/>");
    assert!(matches!(
        load(dir.path()),
        Err(LoadError::UnlistedFile {
            family: Family::Nfo,
            ..
        })
    ));
}
