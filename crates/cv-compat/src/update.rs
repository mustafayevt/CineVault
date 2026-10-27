//! Update mode: writes CineVault's current output over the expected files.
//!
//! Use it after an intended change in behaviour, then review the diff of `tests/compat/expected`
//! before committing: every changed line is a change users will see in their NFO files, names or
//! scan results. Nothing is written unless every case of the suite produced output.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use serde_json::Value;

use crate::family::{Family, Layout};
use crate::fixture::Suite;
use crate::run::{Actual, Handler, HandlerError};

/// Why the expected files could not be updated.
#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("{family}/{id}: {message}")]
    Case {
        family: Family,
        id: String,
        message: String,
    },
    #[error("cannot write {path}: {source}")]
    Io {
        path: std::path::PathBuf,
        source: io::Error,
    },
}

/// The environment variable that switches `cargo test -p cv-compat` into update mode.
pub const UPDATE_ENV: &str = "CINEVAULT_UPDATE_COMPAT";

/// `true` when [`UPDATE_ENV`] is set to anything but `0` or an empty value.
pub fn update_requested() -> bool {
    std::env::var(UPDATE_ENV).is_ok_and(|v| !v.is_empty() && v != "0")
}

/// Runs every case and writes its output as the new expected output. Returns the number of
/// expected files (JSON documents and NFO files) whose content changed.
pub fn update_expected(suite: &Suite, handlers: &[Box<dyn Handler>]) -> Result<usize, UpdateError> {
    // Produce everything first, so a failing case leaves the tree untouched.
    let mut outputs: BTreeMap<Family, Vec<(String, Actual)>> = BTreeMap::new();
    for case in &suite.cases {
        let fail = |message: String| UpdateError::Case {
            family: case.family,
            id: case.id.clone(),
            message,
        };
        let handler = handlers
            .iter()
            .find(|h| h.family() == case.family)
            .ok_or_else(|| fail("no handler is registered for this family".to_owned()))?;
        let actual = catch_unwind(AssertUnwindSafe(|| handler.run(case)))
            .map_err(|_| fail("the handler panicked".to_owned()))?
            .map_err(|e| match e {
                HandlerError::Pending(m) | HandlerError::Failed(m) => fail(m),
            })?;
        outputs
            .entry(case.family)
            .or_default()
            .push((case.id.clone(), actual));
    }

    let expected_root = suite.root.join("expected");
    let mut changed = 0;
    for (family, cases) in outputs {
        changed += match family.layout() {
            Layout::Json { expected, .. } => {
                write_json_family(family, &expected_root.join(expected), cases)?
            }
            Layout::Files { expected_dir, .. } => {
                write_files_family(family, &expected_root.join(expected_dir), cases)?
            }
        };
    }
    Ok(changed)
}

fn io_error(path: &Path) -> impl FnOnce(io::Error) -> UpdateError + '_ {
    move |source| UpdateError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Writes `bytes` to `path` when they differ from what is there. Returns whether it wrote.
fn write_if_changed(path: &Path, bytes: &[u8]) -> Result<bool, UpdateError> {
    if fs::read(path).is_ok_and(|old| old == bytes) {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(io_error(parent))?;
    }
    fs::write(path, bytes).map_err(io_error(path))?;
    Ok(true)
}

/// One case per line, `id` first, the other keys in sorted order.
fn case_line(id: &str, value: &Value) -> String {
    let mut rest = value.clone();
    if let Value::Object(map) = &mut rest {
        map.remove("id");
    }
    let rest = serde_json::to_string(&rest).unwrap_or_else(|_| "{}".to_owned());
    let id = serde_json::to_string(id).unwrap_or_default();
    match rest.strip_prefix('{').and_then(|r| r.strip_suffix('}')) {
        Some("") | None => format!("{{\"id\":{id}}}"),
        Some(inner) => format!("{{\"id\":{id},{inner}}}"),
    }
}

fn document(lines: &[String]) -> String {
    format!(
        "{{\n  \"version\": 1,\n  \"cases\": [\n{}\n  ]\n}}\n",
        lines
            .iter()
            .map(|l| format!("    {l}"))
            .collect::<Vec<_>>()
            .join(",\n")
    )
}

fn write_json_family(
    family: Family,
    path: &Path,
    cases: Vec<(String, Actual)>,
) -> Result<usize, UpdateError> {
    let mut lines = Vec::with_capacity(cases.len());
    for (id, actual) in cases {
        let Actual::Json(mut value) = actual else {
            return Err(UpdateError::Case {
                family,
                id,
                message: "the handler returned files for a JSON family".to_owned(),
            });
        };
        if let Value::Object(map) = &mut value {
            map.insert("id".to_owned(), Value::String(id.clone()));
        }
        lines.push(case_line(&id, &value));
    }
    Ok(usize::from(write_if_changed(
        path,
        document(&lines).as_bytes(),
    )?))
}

fn write_files_family(
    family: Family,
    dir: &Path,
    cases: Vec<(String, Actual)>,
) -> Result<usize, UpdateError> {
    let mut changed = 0;
    let mut lines = Vec::with_capacity(cases.len());
    for (id, actual) in cases {
        let Actual::Files(files) = actual else {
            return Err(UpdateError::Case {
                family,
                id,
                message: "the handler returned JSON for a files family".to_owned(),
            });
        };
        let case_dir = dir.join(&id);
        // Files the case no longer produces go.
        let mut on_disk = Vec::new();
        if case_dir.is_dir() {
            collect_files(&case_dir, &case_dir, &mut on_disk).map_err(io_error(&case_dir))?;
        }
        for stale in on_disk.iter().filter(|f| !files.contains_key(*f)) {
            let path = case_dir.join(stale);
            fs::remove_file(&path).map_err(io_error(&path))?;
            changed += 1;
        }
        for (name, bytes) in &files {
            if write_if_changed(&case_dir.join(name), bytes)? {
                changed += 1;
            }
        }
        let names: Vec<Value> = files.keys().cloned().map(Value::String).collect();
        lines.push(case_line(
            &id,
            &serde_json::json!({ "files": Value::Array(names) }),
        ));
    }
    let index = dir.join("index.json");
    changed += usize::from(write_if_changed(&index, document(&lines).as_bytes())?);
    Ok(changed)
}

fn collect_files(base: &Path, dir: &Path, out: &mut Vec<String>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_files(base, &path, out)?;
        } else if let Ok(rel) = path.strip_prefix(base) {
            let parts: Vec<_> = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect();
            out.push(parts.join("/"));
        }
    }
    Ok(())
}
