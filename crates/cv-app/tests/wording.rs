//! The sentences the Rust code shows people (errors, notices, Activity entries) use the app's
//! glossary like the UI catalogue does: the three verbs Scan, Scrape and Rename (never Identify,
//! Enrich, Rescrape or "Search & scrape") and the current names of the Settings groups.
//!
//! The check reads every string literal in the shipped crates' sources, so a new message cannot
//! bring a retired word back.

use std::fs;
use std::path::{Path, PathBuf};

/// Words of the old wording, matched case-insensitively as the start of a word.
const RETIRED_WORDS: [&str; 3] = ["identif", "enrich", "rescrap"];

/// The Settings groups and sub-pages a sentence may send people to, as the Settings page names
/// them. "Settings, Providers", "Settings, Tools" or "Settings, Integrations" are names of pages
/// that no longer exist.
const SETTINGS_PAGES: [&str; 10] = [
    "General",
    "Library and folders",
    "Automation",
    "Scraping and providers",
    "Artwork, subtitles and trailers",
    "Naming",
    "Advanced",
    "External tools",
    "Keyboard shortcuts",
    "Torrent sources",
];

/// Literals that hold one of the words for a reason other than wording: data keys, stable codes
/// and the list of retired job words in `wording.rs`'s own test.
const ALLOWED: [&str; 6] = [
    "identifier",
    "identification_paused",
    "Identify",
    "Enrich",
    "Rescrape",
    "not_identified",
];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the workspace root")
        .to_path_buf()
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// The source folders of everything that ships: the core crates (not the test runner), the
/// desktop app and the command-line program.
fn shipped_sources() -> Vec<PathBuf> {
    let root = workspace_root();
    let mut dirs = vec![
        root.join("src-tauri").join("src"),
        root.join("cli").join("src"),
    ];
    for entry in fs::read_dir(root.join("crates"))
        .expect("crates folder")
        .flatten()
    {
        let name = entry.file_name();
        if name.to_string_lossy() != "cv-compat" {
            dirs.push(entry.path().join("src"));
        }
    }
    let mut files = Vec::new();
    for dir in dirs {
        rust_files(&dir, &mut files);
    }
    assert!(files.len() > 50, "found only {} source files", files.len());
    files
}

/// The string literals of one Rust source, with the line each starts on. Comments and character
/// literals are skipped; escapes are kept as written, which is enough for a word check.
fn string_literals(source: &str) -> Vec<(usize, String)> {
    let chars: Vec<char> = source.chars().collect();
    let mut found = Vec::new();
    let mut line = 1;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\n' {
            line += 1;
            i += 1;
        } else if c == '/' && chars.get(i + 1) == Some(&'/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && chars.get(i + 1) == Some(&'*') {
            i += 2;
            while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                if chars[i] == '\n' {
                    line += 1;
                }
                i += 1;
            }
            i += 2;
        } else if c == '\'' {
            // A character literal ('"', '\'', '\u{..}') or a lifetime ('a).
            if chars.get(i + 1) == Some(&'\\') {
                i += 2;
                while i < chars.len() && chars[i] != '\'' {
                    i += 1;
                }
                i += 1;
            } else if chars.get(i + 2) == Some(&'\'') {
                i += 3;
            } else {
                i += 1;
            }
        } else if c == '"' {
            let start = line;
            let mut text = String::new();
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    text.push(chars[i]);
                    i += 1;
                }
                if chars[i] == '\n' {
                    line += 1;
                }
                text.push(chars[i]);
                i += 1;
            }
            i += 1;
            found.push((start, text));
        } else {
            i += 1;
        }
    }
    found
}

fn retired_word(text: &str) -> Option<&'static str> {
    let lower = text.to_lowercase();
    if lower.contains("search & scrape") || lower.contains("search and scrape") {
        return Some("search & scrape");
    }
    RETIRED_WORDS.into_iter().find(|word| {
        lower.match_indices(word).any(|(at, _)| {
            lower[..at]
                .chars()
                .next_back()
                .is_none_or(|before| !before.is_alphanumeric() && before != '_')
        })
    })
}

/// The text of a literal as Rust reads it across a `\` line continuation.
fn joined(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find("\\\n") {
        out.push_str(&rest[..at]);
        rest = rest[at + 2..].trim_start();
    }
    out.push_str(rest);
    out
}

/// A Settings page a sentence names that is not one of [`SETTINGS_PAGES`]: `Settings, Tools`, the
/// old Providers page, or
/// any `Settings > …` path (the app's own pages are named with commas; `Kodi Settings > …` is
/// Kodi's menu and stays).
fn stale_settings_page(text: &str) -> Option<String> {
    let text = joined(text);
    if text.contains("Providers page") {
        return Some("the Providers page".to_owned());
    }
    for (at, _) in text.match_indices("Settings, ") {
        let rest = &text[at + "Settings, ".len()..];
        if !SETTINGS_PAGES.iter().any(|page| rest.starts_with(page)) {
            return Some(format!(
                "Settings, {}",
                rest.split(['.', ':', ';']).next().unwrap_or(rest)
            ));
        }
    }
    for (at, _) in text.match_indices("Settings > ") {
        if !text[..at].ends_with("Kodi ") {
            return Some(text[at..].split(['.', ';']).next().unwrap_or("").to_owned());
        }
    }
    None
}

#[test]
fn shown_sentences_use_the_glossary() {
    let mut problems = Vec::new();
    for file in shipped_sources() {
        let source = fs::read_to_string(&file).expect("readable source");
        for (line, text) in string_literals(&source) {
            if ALLOWED.contains(&text.as_str()) {
                continue;
            }
            if let Some(word) = retired_word(&text) {
                problems.push(format!("{}:{line}: \"{word}\" in {text:?}", file.display()));
            }
            if let Some(page) = stale_settings_page(&text) {
                problems.push(format!("{}:{line}: \"{page}\" in {text:?}", file.display()));
            }
        }
    }
    assert!(
        problems.is_empty(),
        "retired wording:\n{}",
        problems.join("\n")
    );
}

#[test]
fn the_literal_reader_finds_strings_and_skips_comments() {
    let source = "// \"Identify\" in a comment\nlet q = '\"';\nlet s = \"Scrape\\\" it\";\n/* \"x\" */ let t = \"two\nlines\";\nfn f<'a>(x: &'a str) { \"after\" }";
    let found = string_literals(source);
    assert_eq!(
        found,
        vec![
            (3, "Scrape\\\" it".to_owned()),
            (4, "two\nlines".to_owned()),
            (6, "after".to_owned()),
        ]
    );
    assert_eq!(retired_word("Not identified yet"), Some("identif"));
    assert_eq!(retired_word("Use Search & scrape"), Some("search & scrape"));
    assert_eq!(retired_word("an unidentified file"), None);
    assert_eq!(retired_word("Scrape again"), None);
    assert_eq!(
        stale_settings_page("Add a key in Settings, Providers."),
        Some("Settings, Providers".to_owned())
    );
    assert_eq!(
        stale_settings_page("Set its path under Settings, \\\n     Tools."),
        Some("Settings, Tools".to_owned())
    );
    assert_eq!(
        stale_settings_page("Set its path under Settings, \\\n     External tools."),
        None
    );
    assert_eq!(
        stale_settings_page("Turn it on in Settings > Integrations."),
        Some("Settings > Integrations".to_owned())
    );
    assert_eq!(
        stale_settings_page("Allow it (Kodi Settings > Services > Control)."),
        None
    );
}
