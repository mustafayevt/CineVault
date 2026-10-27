//! An unchanged rescan of a 5,000-movie synthetic library completes in
//! under 10 seconds and modifies nothing.
//!
//! The library has a video, a poster and an NFO per movie (15,000 files). The first rescan fills
//! the database; the timed rescan then runs against an untouched tree. "Modifies nothing" is
//! checked three ways: the report says no write transaction ran, SQLite's `data_version` (which
//! changes whenever another connection commits) is the same, and a hash over every library table
//! is identical.

mod support;

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use cv_domain::{ItemKind, SourceKind};
use support::{Lib, content_hash, data_version};

const MOVIES: usize = 5_000;
const BUDGET: Duration = Duration::from_secs(10);

fn build_tree(root: &Path) {
    let threads = 8;
    std::thread::scope(|scope| {
        for t in 0..threads {
            scope.spawn(move || {
                for n in (t..MOVIES).step_by(threads) {
                    let year = 1950 + (n % 75);
                    let name = format!("Synthetic Film {n:04} ({year})");
                    let dir = root.join(&name);
                    fs::create_dir_all(&dir).expect("movie folder");
                    fs::write(dir.join(format!("{name}.mkv")), format!("video {n}"))
                        .expect("video");
                    fs::write(dir.join(format!("{name}-poster.jpg")), "poster").expect("poster");
                    fs::write(dir.join(format!("{name}.nfo")), "<movie/>").expect("nfo");
                }
            });
        }
    });
}

#[test]
fn an_unchanged_5k_rescan_is_fast_and_modifies_nothing() {
    let lib = Lib::single_reader(SourceKind::Movie);
    let built = Instant::now();
    build_tree(&lib.root);
    eprintln!("built {MOVIES} movies in {:?}", built.elapsed());

    let first = Instant::now();
    let filled = lib.rescan();
    eprintln!("first rescan (fills the library): {:?}", first.elapsed());
    assert_eq!(filled.new_items.len(), MOVIES);
    assert_eq!(lib.items(ItemKind::Movie).len(), MOVIES);
    assert_eq!(lib.count("fingerprint"), (MOVIES * 3) as i64);

    lib.read_media_info();
    let hash_before = content_hash(&lib.db);
    let version_before = data_version(&lib.db);

    let started = Instant::now();
    let report = lib.rescan();
    let elapsed = started.elapsed();
    eprintln!(
        "unchanged rescan of {MOVIES} movies ({} files): {elapsed:?}",
        report.stats.files
    );

    assert!(
        elapsed < BUDGET,
        "an unchanged rescan of {MOVIES} movies took {elapsed:?}, the budget is {BUDGET:?}"
    );
    assert_eq!(report.stats.units, MOVIES);
    assert_eq!(report.stats.unchanged, MOVIES);
    assert_eq!(report.stats.files, MOVIES * 3);
    assert!(report.is_unchanged(), "nothing was added, changed or lost");
    assert!(!report.wrote, "no write transaction ran");
    assert!(
        report.needs_media_info.is_empty(),
        "no media info is read again"
    );
    assert_eq!(
        data_version(&lib.db),
        version_before,
        "no connection committed"
    );
    assert_eq!(
        content_hash(&lib.db),
        hash_before,
        "every table is identical"
    );
}
