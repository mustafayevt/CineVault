//! Which NFO files are CineVault's: every NFO the writers produce is managed, and the foreign
//! files in `tests/nfo-read` (a release note, an HTML page, an untitled file) are not, so they are
//! never overwritten or removed. Also drives the journal over real files in a folder with a
//! release note next to the movie.

use std::fs;
use std::path::{Path, PathBuf};

use cv_nfo::{Journal, NfoOwnership, WriteOutcome, classify_nfo_file, stale_nfos};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn nfo_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            nfo_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "nfo") {
            out.push(path);
        }
    }
}

#[test]
fn every_nfo_the_writers_produce_is_managed() {
    let mut files = Vec::new();
    nfo_files(&repo_root().join("tests/compat/expected/nfo"), &mut files);
    assert!(files.len() > 50, "found only {} NFOs", files.len());
    for path in files {
        assert_eq!(
            classify_nfo_file(&path).unwrap(),
            NfoOwnership::Managed,
            "{}",
            path.display()
        );
    }
}

#[test]
fn foreign_and_pointer_files_are_not_managed() {
    let dir = repo_root().join("tests/nfo-read");
    let expect = |name: &str, ownership: NfoOwnership| {
        assert_eq!(
            classify_nfo_file(&dir.join(name)).unwrap(),
            ownership,
            "{name}"
        );
    };
    for name in [
        "release-info.nfo",
        "foreign-no-title.nfo",
        "foreign-root.nfo",
    ] {
        expect(name, NfoOwnership::Foreign);
    }
    for name in [
        "id-only.nfo",
        "url-imdb.nfo",
        "url-tmdb.nfo",
        "show-url-tvdb.nfo",
    ] {
        expect(name, NfoOwnership::Pointer);
    }
    for name in [
        "movie-kodi.nfo",
        "movie-utf16.nfo",
        "movie-broken-unclosed.nfo",
    ] {
        expect(name, NfoOwnership::Managed);
    }
    assert_eq!(
        classify_nfo_file(&dir.join("does-not-exist.nfo")).unwrap(),
        NfoOwnership::Missing
    );
}

#[test]
fn a_release_note_beside_the_movie_survives_writing_and_cleanup() {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("Heat (1995)");
    fs::create_dir_all(&folder).unwrap();
    let note = fs::read(repo_root().join("tests/nfo-read/release-info.nfo")).unwrap();
    fs::write(folder.join("info.nfo"), &note).unwrap();
    let stamp = fs::metadata(folder.join("info.nfo"))
        .unwrap()
        .modified()
        .unwrap();
    // The movie's NFO under its old name.
    let old = folder.join("Heat (1995).nfo");
    fs::write(&old, "<movie><title>Heat</title></movie>").unwrap();

    let mut journal = Journal::new(root.path().join("journal"));
    let wanted = vec![folder.join("movie.nfo")];
    let candidates = vec![folder.join("movie.nfo"), old.clone()];
    let content = "<movie><title>Heat</title><year>1995</year></movie>";
    assert_eq!(
        journal.write(&wanted[0], content).unwrap(),
        WriteOutcome::Created
    );
    for stale in stale_nfos(&candidates, &wanted) {
        assert!(journal.trash(&stale).unwrap());
    }
    assert!(!old.exists() && wanted[0].exists());

    // Even when the note is named like a target, it is neither written over nor trashed.
    let at_target = folder.join("movie.nfo");
    fs::write(&at_target, &note).unwrap();
    assert_eq!(
        journal.write(&at_target, content).unwrap(),
        WriteOutcome::SkippedForeign
    );
    assert!(!journal.trash(&at_target).unwrap());
    assert_eq!(fs::read(&at_target).unwrap(), note);

    assert_eq!(fs::read(folder.join("info.nfo")).unwrap(), note);
    assert_eq!(
        fs::metadata(folder.join("info.nfo"))
            .unwrap()
            .modified()
            .unwrap(),
        stamp
    );
}
