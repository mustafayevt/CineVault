use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;

use crate::family::{Family, Layout};

/// Why the fixture tree could not be loaded. A broken tree is an error, never a silent skip.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("cannot read {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path} is not valid JSON: {source}")]
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("{path}: {message}")]
    Shape { path: PathBuf, message: String },
    #[error("{family}: case `{id}` has no expected output")]
    MissingExpected { family: Family, id: String },
    #[error("{family}: expected output `{id}` has no fixture case")]
    UnexpectedExpected { family: Family, id: String },
    #[error("{family}: duplicate case id `{id}`")]
    DuplicateId { family: Family, id: String },
    #[error("{family}: case `{id}`: expected file `{file}` is listed in the index but missing")]
    MissingFile {
        family: Family,
        id: String,
        file: String,
    },
    #[error("{family}: case `{id}`: expected file `{file}` exists but is not listed in the index")]
    UnlistedFile {
        family: Family,
        id: String,
        file: String,
    },
}

/// The golden output of a case.
#[derive(Debug, Clone, PartialEq)]
pub enum Expected {
    /// The result object of the case, including its `id`.
    Json(Value),
    /// Relative path (always `/` separated) to the file bytes.
    Files(BTreeMap<String, Vec<u8>>),
}

/// One fixture: an input case plus its expected output.
#[derive(Debug, Clone)]
pub struct Case {
    pub family: Family,
    pub id: String,
    /// The case object as written in the fixture file.
    pub input: Value,
    /// The fixture file top-level object without `cases` (profiles, notes, data source).
    pub shared: Arc<Value>,
    /// The `fixtures/` folder, for resolving entity files, template folders and similar.
    pub fixtures_dir: PathBuf,
    pub expected: Expected,
}

/// Every fixture case under one `tests/compat` folder.
#[derive(Debug)]
pub struct Suite {
    pub root: PathBuf,
    pub cases: Vec<Case>,
}

impl Suite {
    pub fn cases_of(&self, family: Family) -> impl Iterator<Item = &Case> {
        self.cases.iter().filter(move |c| c.family == family)
    }
}

/// The repository `tests/compat` folder.
pub fn default_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/compat")
}

/// Loads and cross-checks every family under `root`.
pub fn load(root: &Path) -> Result<Suite, LoadError> {
    let mut cases = Vec::new();
    for family in Family::ALL {
        load_family(root, family, &mut cases)?;
    }
    Ok(Suite {
        root: root.to_path_buf(),
        cases,
    })
}

fn read_bytes(path: &Path) -> Result<Vec<u8>, LoadError> {
    fs::read(path).map_err(|source| LoadError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn read_json(path: &Path) -> Result<Value, LoadError> {
    let bytes = read_bytes(path)?;
    serde_json::from_slice(&bytes).map_err(|source| LoadError::Json {
        path: path.to_path_buf(),
        source,
    })
}

fn shape(path: &Path, message: impl Into<String>) -> LoadError {
    LoadError::Shape {
        path: path.to_path_buf(),
        message: message.into(),
    }
}

/// Splits a `{version, ..., cases: [...]}` document into its cases (id and object, in file order)
/// and the remaining top-level fields.
fn split_cases(path: &Path, doc: Value) -> Result<(Vec<(String, Value)>, Value), LoadError> {
    let Value::Object(mut top) = doc else {
        return Err(shape(path, "top level must be an object"));
    };
    let Some(Value::Array(list)) = top.remove("cases") else {
        return Err(shape(path, "missing `cases` array"));
    };
    let mut cases = Vec::with_capacity(list.len());
    for (i, case) in list.into_iter().enumerate() {
        let Some(id) = case.get("id").and_then(Value::as_str).map(str::to_owned) else {
            return Err(shape(path, format!("case {i} has no string `id`")));
        };
        cases.push((id, case));
    }
    Ok((cases, Value::Object(top)))
}

fn load_family(root: &Path, family: Family, out: &mut Vec<Case>) -> Result<(), LoadError> {
    let fixtures_dir = root.join("fixtures");
    let expected_root = root.join("expected");
    let (fixture, mut expected) = match family.layout() {
        Layout::Json { fixture, expected } => {
            let path = expected_root.join(expected);
            let (list, _) = split_cases(&path, read_json(&path)?)?;
            let map: BTreeMap<String, Expected> = list
                .into_iter()
                .map(|(id, v)| (id, Expected::Json(v)))
                .collect();
            (fixture, map)
        }
        Layout::Files {
            fixture,
            expected_dir,
        } => (
            fixture,
            load_file_expectations(family, &expected_root.join(expected_dir))?,
        ),
    };

    let fixture_path = fixtures_dir.join(fixture);
    let (list, shared) = split_cases(&fixture_path, read_json(&fixture_path)?)?;
    let shared = Arc::new(shared);
    let mut seen = BTreeSet::new();
    for (id, input) in list {
        if !seen.insert(id.clone()) {
            return Err(LoadError::DuplicateId { family, id });
        }
        let Some(exp) = expected.remove(&id) else {
            return Err(LoadError::MissingExpected { family, id });
        };
        out.push(Case {
            family,
            id,
            input,
            shared: Arc::clone(&shared),
            fixtures_dir: fixtures_dir.clone(),
            expected: exp,
        });
    }
    if let Some(id) = expected.into_keys().next() {
        return Err(LoadError::UnexpectedExpected { family, id });
    }
    Ok(())
}

/// Reads `<dir>/index.json` and the files it lists, and checks that no other file is present.
fn load_file_expectations(
    family: Family,
    dir: &Path,
) -> Result<BTreeMap<String, Expected>, LoadError> {
    let index_path = dir.join("index.json");
    let (list, _) = split_cases(&index_path, read_json(&index_path)?)?;
    let mut map = BTreeMap::new();
    for (id, entry) in list {
        let Some(Value::Array(files)) = entry.get("files") else {
            return Err(shape(
                &index_path,
                format!("case `{id}` has no `files` array"),
            ));
        };
        let case_dir = dir.join(&id);
        let mut contents = BTreeMap::new();
        for file in files {
            let Some(name) = file.as_str() else {
                return Err(shape(
                    &index_path,
                    format!("case `{id}`: file names must be strings"),
                ));
            };
            let path = case_dir.join(name);
            if !path.is_file() {
                return Err(LoadError::MissingFile {
                    family,
                    id,
                    file: name.to_owned(),
                });
            }
            contents.insert(name.to_owned(), read_bytes(&path)?);
        }
        let mut on_disk = Vec::new();
        if case_dir.is_dir() {
            list_files(&case_dir, &case_dir, &mut on_disk)?;
        }
        if let Some(extra) = on_disk.into_iter().find(|f| !contents.contains_key(f)) {
            return Err(LoadError::UnlistedFile {
                family,
                id,
                file: extra,
            });
        }
        map.insert(id, Expected::Files(contents));
    }
    Ok(map)
}

fn list_files(base: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), LoadError> {
    let entries = fs::read_dir(dir).map_err(|source| LoadError::Io {
        path: dir.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| LoadError::Io {
            path: dir.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if path.is_dir() {
            list_files(base, &path, out)?;
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
