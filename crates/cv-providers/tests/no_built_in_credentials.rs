//! CineVault ships no provider keys: every credential is the user's own. These checks fail if a
//! built-in credential path comes back: a build-time input, an embedded default, a "Built-in key"
//! source, or "Reset to built-in".

use std::path::{Path, PathBuf};

use cv_providers::catalogue::CREDENTIAL_SCHEMAS;
use cv_providers::{CredentialResolver, CredentialState, MemoryUserCredentials};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repository root")
}

/// The names of the removed built-in path, split so this file does not contain them whole.
fn forbidden() -> Vec<String> {
    [
        ("CV_BUILT", "IN"),
        ("credentials.", "local"),
        ("Builtin", "Credentials"),
        ("default_", "builtin"),
        ("Embedded", "Credentials"),
        ("with_builtin_", "key_hint"),
        ("Builtin", "Key"),
        ("Reset to ", "built-in"),
        ("Built-in", " key"),
        ("builtin_", "available"),
        ("builtin_", "allowed"),
        ("reset_to_", "builtin"),
    ]
    .iter()
    .map(|(a, b)| format!("{a}{b}"))
    .collect()
}

fn visit(dir: &Path, hits: &mut Vec<String>, needles: &[String], this_file: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if matches!(
                name.as_str(),
                "target" | "node_modules" | ".git" | ".dev-tools" | "dist"
            ) {
                continue;
            }
            visit(&path, hits, needles, this_file);
            continue;
        }
        let text_file = matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("rs" | "ts" | "tsx" | "toml" | "yml" | "yaml" | "json" | "sql" | "md")
        );
        if !text_file || path == this_file {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for needle in needles {
            if text.contains(needle.as_str()) {
                hits.push(format!("{} contains `{needle}`", path.display()));
            }
        }
    }
}

#[test]
fn no_built_in_credential_path_remains_in_code_tests_docs_or_workflows() {
    let root = repo_root();
    let needles = forbidden();
    let this_file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/no_built_in_credentials.rs")
        .canonicalize()
        .unwrap();
    let mut hits = Vec::new();
    for part in [
        "crates",
        "cli",
        "src-tauri",
        "ui/src",
        ".github",
        "Cargo.toml",
        ".gitignore",
        "AGENTS.md",
    ] {
        let path = root.join(part);
        if path.is_dir() {
            visit(&path, &mut hits, &needles, &this_file);
        } else if let Ok(text) = std::fs::read_to_string(&path) {
            for needle in &needles {
                if text.contains(needle.as_str()) {
                    hits.push(format!("{} contains `{needle}`", path.display()));
                }
            }
        }
    }
    assert!(
        hits.is_empty(),
        "built-in credential leftovers:\n{}",
        hits.join("\n")
    );
}

#[test]
fn the_build_has_no_credential_input_or_embedded_defaults() {
    let root = repo_root();
    for gone in [
        "crates/cv-providers/build.rs",
        "crates/cv-providers/src/builtin.rs",
        "crates/cv-providers/src/builtin",
        "credentials.local.example.toml",
        "credentials.local.toml",
    ] {
        assert!(!root.join(gone).exists(), "{gone} should not exist");
    }
    let manifest = std::fs::read_to_string(root.join("crates/cv-providers/Cargo.toml")).unwrap();
    assert!(
        !manifest.contains("build-dependencies"),
        "no build script input"
    );
}

#[test]
fn with_nothing_saved_every_provider_is_missing_and_resolves_no_value() {
    let resolver = CredentialResolver::new(std::sync::Arc::new(MemoryUserCredentials::default()));
    for schema in CREDENTIAL_SCHEMAS {
        let status = resolver.status(schema.provider).unwrap();
        assert_eq!(
            status.state,
            CredentialState::Missing,
            "{}",
            schema.provider
        );
        let resolved = resolver.resolve(schema.provider).unwrap();
        for field in schema.fields {
            assert!(
                !resolved.has(field.key),
                "{}.{}",
                schema.provider,
                field.key
            );
        }
    }
}
