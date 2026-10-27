//! The `.deletedByCineVault` trash folder in the root of a data source is never scanned.
//!
//! Organize moves leftovers and replaced files into `<data source>/.deletedByCineVault/<batch>/`
//! keeping their relative paths, so the trash holds whole movie and show trees. A scan that
//! entered it would list every trashed title again. The hidden dot-folder rule already keeps
//! it out; these tests pin that for every data source of both kinds, whatever the letter case.

use std::fs;
use std::path::Path;

use cv_domain::{DataSource, SourceKind};
use cv_scan::walk::{WalkOptions, scan_movie_sources, scan_tv_sources};

const TRASH: &str = ".deletedByCineVault";

fn put(root: &Path, rel: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().expect("parent")).expect("create folder");
    fs::write(path, b"").expect("create file");
}

fn source(id: i64, kind: SourceKind, root: &Path) -> DataSource {
    DataSource {
        id,
        kind,
        path: root.to_string_lossy().into_owned(),
        position: id,
        label: String::new(),
        created_ms: 0,
    }
}

#[test]
fn the_trash_of_every_movie_source_is_skipped() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut sources = Vec::new();
    for (id, name, title) in [
        (1, "films-a", "Heat (1995)"),
        (2, "films-b", "Alien (1979)"),
    ] {
        let root = dir.path().join(name);
        put(&root, &format!("{title}/{title}.mkv"));
        // The trash keeps the relative paths of what it took, so it holds whole movie trees.
        put(&root, &format!("{TRASH}/batch-1/{title}/{title}.mkv"));
        put(
            &root,
            &format!("{TRASH}/batch-1/Trashed Movie (2001)/Trashed Movie (2001).mkv"),
        );
        put(
            &root,
            &format!("{TRASH}/batch-1/Trashed Movie (2001)/poster.jpg"),
        );
        sources.push(source(id, SourceKind::Movie, &root));
    }

    let report = scan_movie_sources(&sources, &WalkOptions::with_defaults());
    let mut titles: Vec<&str> = report.movies.iter().map(|m| m.title.as_str()).collect();
    titles.sort_unstable();
    assert_eq!(titles, ["Alien", "Heat"], "problems: {:?}", report.problems);
    for movie in &report.movies {
        assert!(
            !movie.relative_path.contains(TRASH),
            "{} came from the trash",
            movie.relative_path
        );
        assert!(movie.files.iter().all(|f| !f.relative.contains(TRASH)));
    }
}

#[test]
fn the_trash_of_every_tv_source_is_skipped() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut sources = Vec::new();
    for (id, name, show) in [
        (1, "shows-a", "Severance (2022)"),
        (2, "shows-b", "Andor (2022)"),
    ] {
        let root = dir.path().join(name);
        put(&root, &format!("{show}/Season 01/{show} - S01E01.mkv"));
        put(
            &root,
            &format!("{TRASH}/batch-9/{show}/Season 01/{show} - S01E01.mkv"),
        );
        put(
            &root,
            &format!("{TRASH}/batch-9/Old Show (2010)/Season 01/Old Show - S01E01.mkv"),
        );
        put(
            &root,
            &format!("{TRASH}/batch-9/Old Show (2010)/tvshow.nfo"),
        );
        sources.push(source(id, SourceKind::Tv, &root));
    }

    let report = scan_tv_sources(&sources, &WalkOptions::tv_defaults());
    let mut titles: Vec<&str> = report.shows.iter().map(|s| s.title.as_str()).collect();
    titles.sort_unstable();
    assert_eq!(
        titles,
        ["Andor", "Severance"],
        "problems: {:?}",
        report.problems
    );
    for show in &report.shows {
        assert!(
            !show.relative_path.contains(TRASH),
            "{} came from the trash",
            show.relative_path
        );
    }
}

#[test]
fn the_trash_is_skipped_whatever_its_letter_case() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path().join("films");
    put(&root, "Heat (1995)/Heat (1995).mkv");
    put(&root, ".DELETEDBYCINEVAULT/b/Gone (2000)/Gone (2000).mkv");
    let sources = [source(1, SourceKind::Movie, &root)];
    let report = scan_movie_sources(&sources, &WalkOptions::with_defaults());
    let titles: Vec<&str> = report.movies.iter().map(|m| m.title.as_str()).collect();
    assert_eq!(titles, ["Heat"]);
}
