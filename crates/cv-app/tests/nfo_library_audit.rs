//! The NFO audit of a real library (opt-in): `CINEVAULT_LIVE_NFO_AUDIT=1`.
//!
//! It copies the library database and settings of the data folder (`CINEVAULT_AUDIT_DATA`, by
//! default `%APPDATA%\CineVault` or `~/Library/Application Support/CineVault`) to a temporary
//! folder, renders every title's NFO from the copy as Rewrite NFO would, and compares it with the
//! NFO file next to the title, element by element. Nothing is written: the NFO files are only
//! read, and the database copy is thrown away. The report (one line per title that differs, then
//! totals per element) is printed and saved to `CINEVAULT_AUDIT_OUT` when set.
//!
//! Per title: **missing** elements (the library has a value the file lacks), **stale** ones (the
//! file holds another value) and **extra** ones (the file has an element the library would not
//! write). Titles CineVault did not scrape are listed apart: their NFO is the user's own.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::Arc;

use cv_app::nfo::{NfoPreview, NfoService, NfoSettings, NfoSync};
use cv_db::Database;
use cv_domain::ItemId;
use cv_nfo::{Element, Node, parse_roots};
use serde_json::Value;

fn data_folder() -> PathBuf {
    if let Ok(path) = std::env::var("CINEVAULT_AUDIT_DATA") {
        return PathBuf::from(path);
    }
    if let Ok(appdata) = std::env::var("APPDATA") {
        return PathBuf::from(appdata).join("CineVault");
    }
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join("Library/Application Support/CineVault")
}

/// The element as one line of text: name, attributes, text and children.
fn flat(element: &Element) -> String {
    let mut out = element.name.clone();
    for (name, value) in &element.attrs {
        let _ = write!(out, " {name}={value:?}");
    }
    if !element.text.trim().is_empty() {
        let _ = write!(out, " = {:?}", element.text.trim());
    }
    let children: Vec<String> = element
        .children
        .iter()
        .filter_map(|n| match n {
            Node::Element(e) => Some(flat(e)),
            Node::Comment(_) => None,
        })
        .collect();
    if !children.is_empty() {
        let _ = write!(out, " {{{}}}", children.join("; "));
    }
    out
}

/// The top-level elements of an NFO by name, each with its flattened values in order. An
/// episode file with several episodes is read as its first one.
fn elements(text: &str) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    if let Some(root) = parse_roots(text).into_iter().next() {
        for node in &root.children {
            if let Node::Element(e) = node {
                out.entry(e.name.clone()).or_default().push(flat(e));
            }
        }
    }
    out
}

#[derive(Default)]
struct Totals {
    titles: usize,
    in_sync: usize,
    differs: usize,
    missing_file: usize,
    not_managed: usize,
    no_target: usize,
    unscraped_differs: usize,
    missing: BTreeMap<String, usize>,
    stale: BTreeMap<String, usize>,
    extra: BTreeMap<String, usize>,
}

#[test]
fn the_nfo_files_of_the_library_match_the_library() {
    if std::env::var("CINEVAULT_LIVE_NFO_AUDIT").as_deref() != Ok("1") {
        eprintln!("skipped: set CINEVAULT_LIVE_NFO_AUDIT=1 to audit a real library");
        return;
    }
    let data = data_folder();
    let work = tempfile::tempdir().unwrap();
    for name in [
        "library.db",
        "library.db-wal",
        "library.db-shm",
        "settings.json",
    ] {
        let from = data.join(name);
        if from.exists() {
            std::fs::copy(&from, work.path().join(name)).unwrap();
        }
    }
    let values: BTreeMap<String, Value> =
        std::fs::read_to_string(work.path().join("settings.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|v| v.pointer("/settings/values").cloned())
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
    let settings = NfoSettings::from_values(&values);
    let db = Arc::new(Database::open(work.path().join("library.db")).unwrap());
    let service = NfoService::new(Arc::clone(&db), work.path().join("undo"));
    let titles: Vec<(i64, String, String, bool)> = db
        .read(|c| {
            let mut stmt = c.prepare(
                "SELECT i.id, i.kind, i.title, \
                        CASE WHEN i.kind = 'episode' THEN COALESCE(p.last_scraper_id, '') \
                             ELSE i.last_scraper_id END <> '' \
                 FROM item i LEFT JOIN item p ON p.id = i.parent_id \
                 WHERE i.kind IN ('movie', 'show', 'episode') ORDER BY i.kind, i.title",
            )?;
            let rows = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .unwrap();

    let mut totals = Totals::default();
    let mut report = String::new();
    for (id, kind, title, scraped) in titles {
        let id = ItemId(id);
        let preview: Option<NfoPreview> = match kind.as_str() {
            "movie" => service.preview_movie(id, &settings).unwrap(),
            "show" => service.preview_show(id, &settings).unwrap(),
            _ => service.preview_episode(id, &settings).unwrap(),
        };
        let Some(preview) = preview else { continue };
        totals.titles += 1;
        match preview.sync {
            NfoSync::InSync => totals.in_sync += 1,
            NfoSync::Missing => {
                totals.missing_file += 1;
                let _ = writeln!(report, "{kind} {title}: no NFO file");
            }
            NfoSync::NotManaged => totals.not_managed += 1,
            NfoSync::NoTarget => totals.no_target += 1,
            NfoSync::Differs => {
                if !scraped {
                    totals.unscraped_differs += 1;
                    continue;
                }
                totals.differs += 1;
                let want = elements(&preview.text);
                let have = elements(preview.file_text.as_deref().unwrap_or_default());
                let (mut missing, mut stale, mut extra) = (Vec::new(), Vec::new(), Vec::new());
                for (name, values) in &want {
                    match have.get(name) {
                        None => missing.push(name.clone()),
                        Some(found) if found != values => stale.push(name.clone()),
                        Some(_) => {}
                    }
                }
                for name in have.keys().filter(|n| !want.contains_key(*n)) {
                    extra.push(name.clone());
                }
                for name in &missing {
                    *totals.missing.entry(name.clone()).or_default() += 1;
                }
                for name in &stale {
                    *totals.stale.entry(name.clone()).or_default() += 1;
                }
                for name in &extra {
                    *totals.extra.entry(name.clone()).or_default() += 1;
                }
                let _ = writeln!(
                    report,
                    "{kind} {title}: missing [{}] stale [{}] extra [{}]",
                    missing.join(", "),
                    stale.join(", "),
                    extra.join(", ")
                );
            }
        }
    }

    let count = |map: &BTreeMap<String, usize>| {
        let mut list: Vec<(&String, &usize)> = map.iter().collect();
        list.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        list.iter()
            .map(|(name, n)| format!("{name} {n}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let summary = format!(
        "titles {} | current {} | differ {} (scraped) | no NFO {} | not CineVault's {} | \
         no place {} | unscraped with their own NFO {}\n\
         missing elements: {}\nstale elements: {}\nextra elements: {}\n",
        totals.titles,
        totals.in_sync,
        totals.differs,
        totals.missing_file,
        totals.not_managed,
        totals.no_target,
        totals.unscraped_differs,
        count(&totals.missing),
        count(&totals.stale),
        count(&totals.extra),
    );
    println!("{summary}\n{report}");
    if let Ok(out) = std::env::var("CINEVAULT_AUDIT_OUT") {
        std::fs::write(out, format!("{summary}\n{report}")).unwrap();
    }
}
