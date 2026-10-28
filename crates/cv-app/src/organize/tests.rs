//! Organize against a real library file and real folders.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use cv_db::Database;
use cv_db::repo::data_sources::insert_source;
use cv_db::repo::{get_item, insert_item};
use cv_domain::{EpisodeNumber, Item, ItemDetail, ItemId, MediaFile, MediaFileType, SourceKind};
use cv_rename::{Preset, RenameProfile};
use tempfile::TempDir;

use super::*;

mod faults;

struct Lib {
    _dir: TempDir,
    root: PathBuf,
    db: Arc<Database>,
    service: OrganizeService,
}

impl Lib {
    fn new() -> Lib {
        Lib::named("library")
    }

    /// A library whose data source root folder has this name.
    fn named(name: &str) -> Lib {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join(name);
        fs::create_dir_all(&root).unwrap();
        let db = Arc::new(Database::open(dir.path().join("library.db")).unwrap());
        let source = root.to_string_lossy().into_owned();
        let made = db
            .write(move |tx| insert_source(tx, SourceKind::Movie, &source))
            .unwrap();
        // The source lock is process-wide by source id (one library per process in the app): an
        // id of its own keeps one test's applies from holding up another's.
        let (old, new) = (made.id, crate::library_ops::tests::next_source_id());
        db.write(move |tx| {
            tx.execute("UPDATE data_source SET id = ?1 WHERE id = ?2", [new, old])?;
            Ok(())
        })
        .unwrap();
        let service = OrganizeService::new(Arc::clone(&db));
        Lib {
            _dir: dir,
            root,
            db,
            service,
        }
    }

    fn put(&self, rel: &str) {
        let path = self.root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, rel).unwrap();
    }

    fn exists(&self, rel: &str) -> bool {
        self.root.join(rel).exists()
    }

    fn root_text(&self) -> String {
        self.root.to_string_lossy().into_owned()
    }

    /// A movie in `folder` with a video, an NFO and a poster named after `base`, on disk and in
    /// the library.
    fn movie(&self, folder: &str, base: &str, title: &str, year: u16) -> ItemId {
        let mut item = Item::movie(title);
        mark_scraped(&mut item);
        item.core.year = year;
        // Scraped: a rename only follows a scrape.
        item.core
            .ids
            .set("tmdb", format!("{}", 1000 + u32::from(year)));
        item.core.last_scraper_id = "tmdb".to_owned();
        item.core.data_source = self.root_text();
        item.core.path = self.root.join(folder).to_string_lossy().into_owned();
        for (name, kind) in [
            (format!("{base}.mkv"), MediaFileType::Video),
            (format!("{base}.nfo"), MediaFileType::Nfo),
            (format!("{base}-poster.jpg"), MediaFileType::Poster),
        ] {
            self.put(&format!("{folder}/{name}"));
            item.core
                .media_files
                .push(MediaFile::new(kind, item.core.path.clone(), name));
        }
        self.db.write(move |tx| insert_item(tx, &item)).unwrap()
    }

    fn request(&self, scope: Scope) -> PreviewRequest {
        PreviewRequest {
            scope,
            profile: RenameProfile::from_preset(Preset::Clean),
            choices: BTreeMap::new(),
        }
    }

    fn item(&self, id: ItemId) -> Item {
        self.db.read(|c| get_item(c, id)).unwrap().unwrap()
    }
}

fn walk(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(walk(&path));
        } else {
            out.push(path.to_string_lossy().replace('\\', "/"));
        }
    }
    out
}

fn ids(list: &[ItemId]) -> Vec<String> {
    list.iter().map(|id| id.0.to_string()).collect()
}

#[test]
fn the_preview_lists_changes_counts_tidy_and_locked_and_touches_nothing() {
    let lib = Lib::new();
    let heat = lib.movie("heat.1995.1080p", "heat", "Heat", 1995);
    lib.movie("Alien (1979)", "Alien (1979)", "Alien", 1979);
    let mut locked = Item::movie("Locked");
    mark_scraped(&mut locked);
    locked.core.year = 2001;
    locked.core.locked = true;
    locked.core.data_source = lib.root_text();
    locked.core.path = lib.root.join("locked").to_string_lossy().into_owned();
    locked.core.media_files.push(MediaFile::new(
        MediaFileType::Video,
        locked.core.path.clone(),
        "locked.mkv",
    ));
    lib.put("locked/locked.mkv");
    lib.db.write(move |tx| insert_item(tx, &locked)).unwrap();
    let before = walk(&lib.root);

    let preview = lib
        .service
        .preview(&lib.request(Scope::Everything))
        .unwrap();

    assert_eq!(preview.counts.change, 1);
    assert_eq!(preview.counts.tidy, 1);
    assert_eq!(preview.counts.locked, 1);
    assert_eq!(preview.counts.conflicts, 0);
    assert_eq!(preview.rows.len(), 1);
    let row = &preview.rows[0];
    assert_eq!(row.id, heat.0.to_string());
    assert_eq!(row.before, "heat.1995.1080p");
    assert_eq!(row.after, "Heat (1995)");
    assert_eq!(row.before_file, "heat.mkv");
    assert_eq!(row.after_file, "Heat (1995).mkv");
    assert_eq!(row.status, RowStatus::Change);
    assert!(row.root.starts_with(&lib.root_text()));
    assert_eq!(row.companions.nfo, 1);
    assert_eq!(row.companions.artwork, 1);
    assert_eq!(row.moves, 3);
    assert_eq!(preview.counts.moves, 3);
    assert_eq!(walk(&lib.root), before, "a preview never touches the disk");
}

#[test]
fn the_preview_shows_the_examples_with_the_edge_cases() {
    let lib = Lib::new();
    let preview = lib
        .service
        .preview(&lib.request(Scope::Everything))
        .unwrap();
    let kinds: Vec<ExampleKind> = preview.examples.iter().map(|e| e.kind).collect();
    assert_eq!(
        kinds,
        [
            ExampleKind::Edition,
            ExampleKind::NoHdr,
            ExampleKind::Colon,
            ExampleKind::Episode
        ]
    );
    assert!(preview.examples.iter().all(|e| e.sample));
    let episode = &preview.examples[3];
    assert_eq!(episode.folder, "Severance (2022)/Season 02");
    assert_eq!(episode.file, "Severance - S02E01 - Hello, Ms. Cobel");
    let colon = &preview.examples[2];
    assert_eq!(colon.folder, "Dune - Part Two (2024)");
    let edition = &preview.examples[0];
    assert_eq!(edition.file, "Heat - Director's Cut (1995)");
}

#[test]
fn a_library_title_with_an_edge_case_replaces_its_sample() {
    let lib = Lib::new();
    lib.movie("old", "old", "Mission: Impossible", 1996);
    let preview = lib
        .service
        .preview(&lib.request(Scope::Everything))
        .unwrap();
    let colon = &preview.examples[2];
    assert!(!colon.sample);
    assert_eq!(colon.title, "Mission: Impossible");
    assert_eq!(colon.folder, "Mission - Impossible (1996)");
}

#[test]
fn a_broken_template_blocks_the_preview_and_says_which() {
    let lib = Lib::new();
    lib.movie("old", "old", "Heat", 1995);
    let mut request = lib.request(Scope::Everything);
    request.profile.movie.folder_pattern = "${titel} (${year})".to_owned();
    let preview = lib.service.preview(&request).unwrap();
    assert!(preview.rows.is_empty());
    assert_eq!(preview.problems.len(), 1);
    assert_eq!(preview.problems[0].field, PatternField::MovieFolder);
    assert!(
        preview.problems[0].message.contains("titel"),
        "{:?}",
        preview.problems
    );
}

#[test]
fn apply_renames_only_the_checked_items_and_updates_the_library_paths() {
    let lib = Lib::new();
    let heat = lib.movie("heat.1995.1080p", "heat", "Heat", 1995);
    let arrival = lib.movie("arrival.2016", "arrival", "Arrival", 2016);
    let request = ApplyRequest {
        preview: lib.request(Scope::Everything),
        include: ids(&[heat]),
    };

    let applied = lib.service.apply(&request).unwrap();

    assert_eq!(applied.done(), 1);
    assert_eq!(applied.failed(), 0);
    assert!(applied.batch_id.is_some());
    assert!(lib.exists("Heat (1995)/Heat (1995).mkv"));
    assert!(lib.exists("Heat (1995)/Heat (1995).nfo"));
    assert!(lib.exists("Heat (1995)/Heat (1995)-poster.jpg"));
    assert!(!lib.exists("heat.1995.1080p"));
    assert!(
        lib.exists("arrival.2016/arrival.mkv"),
        "an unchecked item is not renamed"
    );
    // The library follows without a rescan.
    let moved = lib.item(heat);
    assert!(
        moved.core.path.ends_with("Heat (1995)"),
        "{}",
        moved.core.path
    );
    assert!(
        moved
            .core
            .media_files
            .iter()
            .all(|f| f.directory.ends_with("Heat (1995)"))
    );
    assert!(lib.item(arrival).core.path.ends_with("arrival.2016"));
}

#[test]
fn a_title_whose_nfo_or_artwork_is_being_written_is_left_out_of_an_apply() {
    // Safety review 14.2: a manual Organize (or any apply) must not move a folder while a
    // scrape's follow-up still writes into it, or the write lands in a recreated old folder.
    let lib = Lib::new();
    let heat = lib.movie("heat.1995.1080p", "heat", "Heat", 1995);
    let arrival = lib.movie("arrival.2016", "arrival", "Arrival", 2016);
    // The follow-up of Heat's scrape is queued or in its steps: it pins the title.
    let follow_up = crate::title_guard::pin(&lib.db, [heat]);
    let request = ApplyRequest {
        preview: lib.request(Scope::Everything),
        include: ids(&[heat, arrival]),
    };

    let started = std::time::Instant::now();
    let applied = lib.service.apply(&request).unwrap();

    assert!(
        started.elapsed() < std::time::Duration::from_secs(10),
        "a busy title is reported, not waited for"
    );
    assert_eq!(applied.done(), 1, "{applied:?}");
    assert_eq!(
        applied
            .busy
            .iter()
            .map(|b| b.id.clone())
            .collect::<Vec<_>>(),
        ids(&[heat])
    );
    let why = applied.busy_reason(&heat.0.to_string()).unwrap();
    assert!(why.contains("NFO or artwork"), "{why}");
    assert_eq!(applied.not_run, ids(&[heat]));
    assert!(
        lib.exists("heat.1995.1080p/heat.mkv"),
        "the busy title is not moved"
    );
    assert!(lib.exists("Arrival (2016)/Arrival (2016).mkv"));

    // Once the writes are over, the next apply renames it.
    drop(follow_up);
    let applied = lib
        .service
        .apply(&ApplyRequest {
            preview: lib.request(Scope::Everything),
            include: ids(&[heat]),
        })
        .unwrap();
    assert_eq!(applied.done(), 1, "{applied:?}");
    assert!(lib.exists("Heat (1995)/Heat (1995).mkv"));
}

#[test]
fn an_apply_that_waits_for_a_short_write_never_holds_up_a_scrape_follow_up() {
    // The two reviews' guards together: a scrape's follow-up pins Heat and writes into it one
    // file at a time; a subtitle write holds Arrival for a moment. An Organize of Heat, Arrival
    // and Drive waits for the subtitle, but holds nothing while it waits, so the follow-up's next
    // write into Heat (and any write into Drive) goes ahead at once: neither waits on the other.
    use crate::title_guard::{self, Work};
    use std::time::{Duration, Instant};

    let lib = Lib::new();
    let heat = lib.movie("heat.1995.1080p", "heat", "Heat", 1995);
    let arrival = lib.movie("arrival.2016", "arrival", "Arrival", 2016);
    let drive = lib.movie("drive.2011", "drive", "Drive", 2011);
    let follow_up = title_guard::pin(&lib.db, [heat]);
    let nfo = title_guard::hold(&lib.db, [heat], Work::Write, Duration::ZERO)
        .expect("the follow-up's own write is not kept out by its pin");
    let subtitle = title_guard::hold(&lib.db, [arrival], Work::Write, Duration::ZERO).unwrap();

    let service = lib.service.clone();
    let request = ApplyRequest {
        preview: lib.request(Scope::Everything),
        include: ids(&[heat, arrival, drive]),
    };
    let organize = std::thread::spawn(move || {
        let started = Instant::now();
        let applied = service.apply(&request).unwrap();
        (applied, started.elapsed())
    });
    std::thread::sleep(Duration::from_millis(400));
    // The follow-up moves on from the NFO to the artwork while the apply waits.
    drop(nfo);
    let artwork = title_guard::hold(&lib.db, [heat], Work::Write, Duration::ZERO)
        .expect("the waiting apply does not hold Heat");
    let other = title_guard::hold(&lib.db, [drive], Work::Write, Duration::ZERO)
        .expect("the waiting apply holds nothing");
    drop(other);
    drop(subtitle);
    let (applied, took) = organize.join().unwrap();
    drop(artwork);
    drop(follow_up);

    assert!(took < Duration::from_secs(10), "took {took:?}");
    assert_eq!(applied.done(), 2, "{applied:?}");
    assert_eq!(
        applied
            .busy
            .iter()
            .map(|b| b.id.clone())
            .collect::<Vec<_>>(),
        ids(&[heat])
    );
    assert_eq!(applied.not_run, ids(&[heat]));
    assert!(lib.exists("heat.1995.1080p/heat.mkv"));
    assert!(lib.exists("Arrival (2016)/Arrival (2016).mkv"));
    assert!(lib.exists("Drive (2011)/Drive (2011).mkv"));
    assert_eq!(title_guard::holder(&lib.db, heat), None, "all released");
    assert_eq!(title_guard::holder(&lib.db, arrival), None, "all released");
}

#[test]
fn an_apply_that_waits_for_a_move_plans_from_where_the_move_left_the_title() {
    // Review 14.7: the apply planned before it held its titles, so a move that ended while the
    // apply waited for it left the apply with the old paths. Here the move put Heat in its new
    // place and left an original it could not take to the trash folder in the old one (a file
    // in use): the stale plan renamed that old folder and pointed the library back at it.
    use crate::title_guard::{self, Work};
    use std::time::Duration;

    let lib = Lib::new();
    let heat = lib.movie("heat.1995.1080p", "heat", "Heat", 1995);
    let moving = title_guard::hold(&lib.db, [heat], Work::Move, Duration::ZERO).unwrap();

    let service = lib.service.clone();
    let request = ApplyRequest {
        preview: lib.request(Scope::Everything),
        include: ids(&[heat]),
    };
    let organize = std::thread::spawn(move || service.apply(&request).unwrap());
    std::thread::sleep(Duration::from_millis(400));

    // The move: the title is whole in `moved/heat.1995.1080p` and the library points there;
    // the video it could not take to the trash folder is still in the old folder.
    let new_folder = lib.root.join("moved").join("heat.1995.1080p");
    fs::create_dir_all(&new_folder).unwrap();
    for name in ["heat.mkv", "heat.nfo", "heat-poster.jpg"] {
        fs::copy(
            lib.root.join("heat.1995.1080p").join(name),
            new_folder.join(name),
        )
        .unwrap();
    }
    fs::remove_file(lib.root.join("heat.1995.1080p/heat.nfo")).unwrap();
    fs::remove_file(lib.root.join("heat.1995.1080p/heat-poster.jpg")).unwrap();
    let mut item = lib.item(heat);
    let old = item.core.path.clone();
    let new = new_folder.to_string_lossy().into_owned();
    item.core.path.clone_from(&new);
    for file in &mut item.core.media_files {
        file.directory = file.directory.replace(&old, &new);
    }
    lib.db
        .write(move |tx| cv_db::repo::update_item(tx, &item))
        .unwrap();
    drop(moving);

    let applied = organize.join().unwrap();
    assert_eq!(applied.done(), 1, "{applied:?}");
    // The old folder, with what the move left in it, is not touched.
    assert!(lib.exists("heat.1995.1080p/heat.mkv"));
    // The title is renamed from where it is now, inside the folder the move put it in (keep
    // sub-folders is on).
    assert!(lib.exists("moved/Heat (1995)/Heat (1995).mkv"));
    assert!(!lib.exists("moved/heat.1995.1080p/heat.mkv"));
    let after = lib.item(heat);
    assert_eq!(
        Path::new(&after.core.path),
        lib.root.join("moved").join("Heat (1995)").as_path()
    );
}

/// Adds subtitle files (on disk and in the library, with no stream details) to a movie.
fn add_subtitles(lib: &Lib, id: ItemId, names: &[&str]) {
    let mut item = lib.item(id);
    for name in names {
        let rel = relative(&lib.root, &item.core.path);
        lib.put(&format!("{rel}/{name}"));
        item.core.media_files.push(MediaFile::new(
            MediaFileType::Subtitle,
            item.core.path.clone(),
            *name,
        ));
    }
    lib.db
        .write(move |tx| cv_db::repo::update_item(tx, &item))
        .unwrap();
}

fn relative(root: &Path, path: &str) -> String {
    Path::new(path)
        .strip_prefix(root)
        .unwrap()
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}

#[test]
fn subtitles_keep_their_language_and_forced_marks_through_a_rename() {
    let lib = Lib::new();
    let heat = lib.movie("heat.1995", "heat", "Heat", 1995);
    add_subtitles(
        &lib,
        heat,
        &[
            "heat.eng.srt",
            "heat.jpn.srt",
            "heat.deu.forced.srt",
            "heat.fra.ass",
        ],
    );
    let request = lib.request(Scope::Items(vec![heat]));

    let preview = lib.service.preview(&request).unwrap();
    assert_eq!(preview.counts.conflicts, 0, "{:?}", preview.conflicts);
    assert_eq!(preview.counts.change, 1);
    let applied = lib
        .service
        .apply(&ApplyRequest {
            preview: request,
            include: ids(&[heat]),
        })
        .unwrap();
    assert_eq!(applied.done(), 1, "{applied:?}");
    for name in [
        "Heat (1995).eng.srt",
        "Heat (1995).jpn.srt",
        "Heat (1995).deu.forced.srt",
        "Heat (1995).fra.ass",
    ] {
        assert!(
            lib.exists(&format!("Heat (1995)/{name}")),
            "{name}: {:?}",
            walk(&lib.root)
        );
    }
}

#[test]
fn a_tidy_movie_with_a_tagged_subtitle_stays_tidy() {
    let lib = Lib::new();
    let fargo = lib.movie("Fargo (1996)", "Fargo (1996)", "Fargo", 1996);
    add_subtitles(&lib, fargo, &["Fargo (1996).eng.srt"]);
    let preview = lib
        .service
        .preview(&lib.request(Scope::Items(vec![fargo])))
        .unwrap();
    assert_eq!(preview.counts.tidy, 1, "{:?}", preview.rows);
}

#[test]
fn a_data_source_inside_another_one_is_never_removed_by_an_organize() {
    let lib = Lib::new();
    // A TV data source in a folder of the movie data source; its episodes were also taken for
    // two movies of the movie source.
    let tv_root = lib.root.join("Chernobyl").to_string_lossy().into_owned();
    lib.db
        .write(move |tx| insert_source(tx, SourceKind::Tv, &tv_root))
        .unwrap();
    let one = lib.movie(
        "Chernobyl/Season 1",
        "Chernobyl S01E01",
        "Chernobyl S01E01",
        0,
    );
    let two = lib.movie(
        "Chernobyl/Season 1",
        "Chernobyl S01E02",
        "Chernobyl S01E02",
        0,
    );
    // Two movies in one folder: the scan marks it a multi-movie folder.
    for id in [one, two] {
        let mut item = lib.item(id);
        if let ItemDetail::Movie(fields) = &mut item.detail {
            fields.multi_movie_dir = true;
        }
        lib.db
            .write(move |tx| cv_db::repo::update_item(tx, &item))
            .unwrap();
    }

    // The movies move out to the root and empty the folders of the TV source.
    let mut preview = lib.request(Scope::Items(vec![one, two]));
    preview.profile.movie.keep_subfolders = false;
    let applied = lib
        .service
        .apply(&ApplyRequest {
            preview,
            include: ids(&[one, two]),
        })
        .unwrap();

    assert_eq!(applied.done(), 2, "{applied:?}");
    assert!(lib.exists("Chernobyl S01E01/Chernobyl S01E01.mkv"));
    assert!(lib.exists("Chernobyl"), "{:?}", walk(&lib.root));
}

#[test]
fn a_loose_movie_moved_into_its_own_folder_lives_there_and_is_tidy_afterwards() {
    let lib = Lib::new();
    // The movie sits in the root of its data source: its folder is the root.
    let mut item = Item::movie("Dune");
    mark_scraped(&mut item);
    item.core.year = 2021;
    item.core.data_source = lib.root_text();
    item.core.path = lib.root_text();
    lib.put("Dune.2021.2160p.mkv");
    item.core.media_files.push(MediaFile::new(
        MediaFileType::Video,
        lib.root_text(),
        "Dune.2021.2160p.mkv",
    ));
    let dune = lib.db.write(move |tx| insert_item(tx, &item)).unwrap();
    let request = lib.request(Scope::Items(vec![dune]));

    let applied = lib
        .service
        .apply(&ApplyRequest {
            preview: request.clone(),
            include: ids(&[dune]),
        })
        .unwrap();
    assert_eq!(applied.done(), 1, "{applied:?}");
    assert!(lib.exists("Dune (2021)/Dune (2021).mkv"));
    assert!(
        lib.item(dune).core.path.ends_with("Dune (2021)"),
        "{}",
        lib.item(dune).core.path
    );

    let again = lib.service.preview(&request).unwrap();
    assert_eq!(again.counts.tidy, 1, "{again:?}");
    assert!(again.rows.is_empty());

    lib.service.undo(&applied.batch_id.unwrap()).unwrap();
    assert!(lib.exists("Dune.2021.2160p.mkv"));
    assert_eq!(lib.item(dune).core.path, lib.root_text());
}

/// A movie that sits in the root of its data source, where the root is the movie's own folder
/// (the first-run split adds `Mixed\Dune (2021)` as a movie folder of its own).
fn movie_at_its_own_root(lib: &Lib, base: &str) -> ItemId {
    let mut item = Item::movie("Dune");
    mark_scraped(&mut item);
    item.core.year = 2021;
    item.core.data_source = lib.root_text();
    item.core.path = lib.root_text();
    for (name, kind) in [
        (format!("{base}.mkv"), MediaFileType::Video),
        (format!("{base}.nfo"), MediaFileType::Nfo),
        (format!("{base}-poster.jpg"), MediaFileType::Poster),
    ] {
        lib.put(&name);
        item.core
            .media_files
            .push(MediaFile::new(kind, lib.root_text(), name));
    }
    // The scan takes the files of a data source root for a multi-movie folder.
    if let ItemDetail::Movie(fields) = &mut item.detail {
        fields.multi_movie_dir = true;
    }
    lib.db.write(move |tx| insert_item(tx, &item)).unwrap()
}

/// A data source root that is the movie's own folder is never nested into itself
/// (`Dune (2021)\Dune (2021)\`): the folder stays, the movie is tidy.
#[test]
fn a_movie_whose_data_source_root_is_its_own_folder_is_never_nested_into_itself() {
    let lib = Lib::named("Dune (2021)");
    let dune = movie_at_its_own_root(&lib, "Dune (2021)");
    let request = lib.request(Scope::Items(vec![dune]));

    let preview = lib.service.preview(&request).unwrap();
    assert_eq!(preview.counts.tidy, 1, "{:?}", preview.rows);
    assert!(preview.rows.is_empty(), "{:?}", preview.rows);

    lib.service
        .apply(&ApplyRequest {
            preview: request,
            include: ids(&[dune]),
        })
        .unwrap();
    assert!(!lib.exists("Dune (2021)"), "{:?}", walk(&lib.root));
    assert!(lib.exists("Dune (2021).mkv"));
    assert_eq!(lib.item(dune).core.path, lib.root_text());
}

/// When the root folder is the movie's folder under another name, only the files are
/// renamed; nothing moves into a sub-folder.
#[test]
fn a_movie_folder_root_under_a_release_name_renames_the_files_in_place() {
    let lib = Lib::named("Dune.2021.2160p");
    let dune = movie_at_its_own_root(&lib, "Dune.2021.2160p");
    let request = lib.request(Scope::Items(vec![dune]));

    let applied = lib
        .service
        .apply(&ApplyRequest {
            preview: request,
            include: ids(&[dune]),
        })
        .unwrap();
    assert_eq!(applied.done(), 1, "{applied:?}");
    assert!(lib.exists("Dune (2021).mkv"), "{:?}", walk(&lib.root));
    assert!(lib.exists("Dune (2021).nfo"), "{:?}", walk(&lib.root));
    assert!(!lib.exists("Dune (2021)"), "{:?}", walk(&lib.root));
    assert_eq!(lib.item(dune).core.path, lib.root_text());
}

#[test]
fn the_journal_lists_the_batch_and_undo_restores_names_and_library_paths() {
    let lib = Lib::new();
    let heat = lib.movie("heat.1995.1080p", "heat", "Heat", 1995);
    let alien = lib.movie("alien.1979", "alien", "Alien", 1979);
    let request = ApplyRequest {
        preview: lib.request(Scope::Everything),
        include: ids(&[heat, alien]),
    };
    let footer = lib.service.preview(&request.preview).unwrap().counts.moves;
    let applied = lib.service.apply(&request).unwrap();
    let batch = applied.batch_id.unwrap();

    let journal = lib.service.journal().unwrap();
    assert_eq!(journal.len(), 1);
    assert_eq!(journal[0].batch_id, batch);
    assert_eq!(journal[0].items, 2);
    // History counts the files the footer said the apply moves, not journal rows.
    assert_eq!(journal[0].files, footer, "{:?}", journal[0]);
    assert_ne!(journal[0].files, journal[0].operations);
    assert_eq!(journal[0].undone_ms, None);
    let mut titles = journal[0].titles.clone();
    titles.sort();
    assert_eq!(
        titles,
        ["Alien", "Heat"],
        "a batch says which titles it covered"
    );
    assert!(journal[0].undoable());

    let outcome = lib.service.undo(&batch).unwrap();

    assert!(outcome.is_complete(), "{outcome:?}");
    assert_eq!(outcome.restored, 2);
    assert!(lib.exists("heat.1995.1080p/heat.mkv"));
    assert!(lib.exists("alien.1979/alien.nfo"));
    assert!(!lib.exists("Heat (1995)"));
    assert!(lib.item(heat).core.path.ends_with("heat.1995.1080p"));
    let after = lib.service.journal().unwrap();
    assert!(
        !after[0].undoable(),
        "an undone batch has nothing left to undo"
    );
    assert!(after[0].undone_ms.is_some(), "the undo has its time");
    assert_eq!(
        after[0].files, footer,
        "an undone batch still says what it did"
    );
}

#[test]
fn a_folder_changed_after_the_organize_is_reported_while_the_others_are_restored() {
    let lib = Lib::new();
    let heat = lib.movie("heat.1995.1080p", "heat", "Heat", 1995);
    let alien = lib.movie("alien.1979", "alien", "Alien", 1979);
    let applied = lib
        .service
        .apply(&ApplyRequest {
            preview: lib.request(Scope::Everything),
            include: ids(&[heat, alien]),
        })
        .unwrap();
    // Somebody edits a renamed file afterwards.
    fs::write(
        lib.root.join("Heat (1995)").join("Heat (1995).nfo"),
        "edited by hand, longer than before",
    )
    .unwrap();

    let outcome = lib.service.undo(&applied.batch_id.unwrap()).unwrap();

    assert_eq!(outcome.restored, 1, "{outcome:?}");
    assert_eq!(outcome.not_restorable.len(), 1);
    assert_eq!(outcome.not_restorable[0].title, "Heat");
    assert!(!outcome.not_restorable[0].reason.is_empty());
    assert!(lib.exists("alien.1979/alien.mkv"));
    assert!(lib.exists("Heat (1995)/Heat (1995).mkv"));
}

#[test]
fn two_folders_that_collide_are_blocked_until_kept_as_versions() {
    let lib = Lib::new();
    let a = lib.movie("Les.Miserables.2012.REPACK", "a", "Les Misérables", 2012);
    let b = lib.movie("Les Miserables 2012 1080p", "b", "Les Misérables", 2012);
    let request = lib.request(Scope::Everything);

    let preview = lib.service.preview(&request).unwrap();
    assert_eq!(preview.counts.conflicts, 2);
    assert_eq!(preview.counts.change, 0);
    assert!(preview.rows.iter().all(|r| r.status == RowStatus::Conflict));
    assert_eq!(preview.conflicts.len(), 1);
    let conflict = &preview.conflicts[0];
    assert_eq!(conflict.code, ConflictCode::Duplicate);
    assert_eq!(conflict.items.len(), 2);
    assert!(conflict.options.contains(&ResolutionOption::KeepAsVersions));
    assert!(conflict.options.contains(&ResolutionOption::Skip));
    assert!(conflict.options.contains(&ResolutionOption::Edit));

    // Apply leaves blocked items out, even when they are checked.
    let blocked = lib
        .service
        .apply(&ApplyRequest {
            preview: request.clone(),
            include: ids(&[a, b]),
        })
        .unwrap();
    assert_eq!(blocked.batch_id, None);
    assert_eq!(blocked.not_run.len(), 2);
    assert!(lib.exists("Les.Miserables.2012.REPACK/a.mkv"));

    // Keep both as versions.
    let mut resolved = request;
    resolved
        .choices
        .insert(a.0.to_string(), Choice::KeepAsVersions);
    resolved
        .choices
        .insert(b.0.to_string(), Choice::KeepAsVersions);
    let preview = lib.service.preview(&resolved).unwrap();
    assert_eq!(preview.counts.conflicts, 0, "{preview:?}");
    assert_eq!(preview.counts.change, 2);
    assert!(preview.conflicts.is_empty());
    let applied = lib
        .service
        .apply(&ApplyRequest {
            preview: resolved,
            include: ids(&[a, b]),
        })
        .unwrap();
    assert_eq!(applied.done(), 2, "{applied:?}");
    let files = walk(&lib.root);
    let in_folder = files
        .iter()
        .filter(|f| f.contains("/Les Misérables (2012)/") && f.ends_with(".mkv"))
        .count();
    assert_eq!(in_folder, 2, "{files:?}");
}

#[test]
fn two_movies_whose_files_differ_but_whose_folders_are_one_are_not_merged() {
    let lib = Lib::new();
    let theatrical = lib.movie("Blade Runner (1982)", "br", "Blade Runner", 1982);
    let final_cut = lib.movie("Blade Runner FC", "br-fc", "Blade Runner", 1982);
    let mut item = lib.item(final_cut);
    if let ItemDetail::Movie(fields) = &mut item.detail {
        fields.edition = cv_domain::Edition::Other("Final Cut".to_owned());
    }
    lib.db
        .write(move |tx| cv_db::repo::update_item(tx, &item))
        .unwrap();
    // Both folders become one new folder; the file names keep the edition apart.
    let mut request = lib.request(Scope::Everything);
    request.profile.movie.folder_pattern = "${title} [${year}]".to_owned();

    let preview = lib.service.preview(&request).unwrap();
    assert_eq!(preview.counts.conflicts, 2, "{preview:?}");
    assert_eq!(preview.conflicts.len(), 1);
    assert_eq!(preview.conflicts[0].code, ConflictCode::Duplicate);
    assert_eq!(preview.conflicts[0].path, "Blade Runner [1982]");

    let applied = lib
        .service
        .apply(&ApplyRequest {
            preview: request,
            include: ids(&[theatrical, final_cut]),
        })
        .unwrap();
    assert_eq!(applied.batch_id, None);
    assert!(lib.exists("Blade Runner FC/br-fc.mkv"));
}

#[test]
fn skip_and_edit_resolve_a_conflict_too() {
    let lib = Lib::new();
    let a = lib.movie("one", "a", "Twin", 2000);
    let b = lib.movie("two", "b", "Twin", 2000);
    let mut request = lib.request(Scope::Everything);
    request.choices.insert(a.0.to_string(), Choice::Skip);

    let preview = lib.service.preview(&request).unwrap();
    assert_eq!(preview.counts.skipped, 1);
    assert_eq!(preview.counts.change, 1);
    let skipped = preview
        .rows
        .iter()
        .find(|r| r.id == a.0.to_string())
        .unwrap();
    assert_eq!(skipped.status, RowStatus::Skipped);

    let mut request = lib.request(Scope::Everything);
    request.choices.insert(
        b.0.to_string(),
        Choice::Edit {
            folder: Some("Twin (2000) second".to_owned()),
            base: None,
        },
    );
    let preview = lib.service.preview(&request).unwrap();
    assert_eq!(preview.counts.conflicts, 0, "{preview:?}");
    assert_eq!(preview.counts.change, 2);
    let edited = preview
        .rows
        .iter()
        .find(|r| r.id == b.0.to_string())
        .unwrap();
    assert_eq!(edited.after, "Twin (2000) second");
}

#[test]
fn a_destination_that_holds_another_title_is_a_conflict() {
    let lib = Lib::new();
    // Heat (1995) already exists and belongs to another library item.
    lib.movie("Heat (1995)", "Heat (1995)", "Heat Remastered", 2020);
    let heat = lib.movie("heat.1995.1080p", "heat", "Heat", 1995);

    let preview = lib
        .service
        .preview(&lib.request(Scope::Items(vec![heat])))
        .unwrap();

    assert_eq!(preview.counts.conflicts, 1, "{preview:?}");
    assert_eq!(
        preview.conflicts[0].code,
        ConflictCode::Foreign {
            owner: Some("Heat Remastered (2020)".to_owned())
        },
        "the title in the way is named, never shown by its library id"
    );
}

#[test]
fn the_preview_lists_every_file_that_changes_even_when_the_folder_stays() {
    let lib = Lib::new();
    // The folder and the video are tidy; the poster and the NFO have names the profile does not
    // write.
    let mut item = Item::movie("Alien");
    mark_scraped(&mut item);
    item.core.year = 1979;
    item.core.data_source = lib.root_text();
    item.core.path = lib.root.join("Alien (1979)").to_string_lossy().into_owned();
    for (name, kind) in [
        ("Alien (1979).mkv", MediaFileType::Video),
        ("movie.nfo", MediaFileType::Nfo),
        ("poster.jpg", MediaFileType::Poster),
    ] {
        lib.put(&format!("Alien (1979)/{name}"));
        item.core
            .media_files
            .push(MediaFile::new(kind, item.core.path.clone(), name));
    }
    let alien = lib.db.write(move |tx| insert_item(tx, &item)).unwrap();

    let preview = lib
        .service
        .preview(&lib.request(Scope::Items(vec![alien])))
        .unwrap();

    let row = &preview.rows[0];
    assert_eq!(
        (row.before.as_str(), row.after.as_str()),
        ("Alien (1979)", "Alien (1979)")
    );
    let moves: Vec<(&str, Vec<&str>)> = row
        .files
        .iter()
        .map(|f| (f.from.as_str(), f.to.iter().map(String::as_str).collect()))
        .collect();
    assert!(
        moves.contains(&(
            "Alien (1979)/poster.jpg",
            vec!["Alien (1979)/Alien (1979)-poster.jpg"]
        )),
        "{moves:?}"
    );
    assert!(
        moves.contains(&(
            "Alien (1979)/movie.nfo",
            vec!["Alien (1979)/Alien (1979).nfo"]
        )),
        "{moves:?}"
    );
    assert!(
        row.files
            .iter()
            .all(|f| f.from != "Alien (1979)/Alien (1979).mkv"),
        "a file that stays as it is is not listed"
    );
}

#[test]
fn a_selection_limits_the_preview_and_a_set_stands_for_its_movies() {
    let lib = Lib::new();
    let heat = lib.movie("heat.1995", "heat", "Heat", 1995);
    lib.movie("alien.1979", "alien", "Alien", 1979);
    let only_heat = lib
        .service
        .preview(&lib.request(Scope::Items(vec![heat])))
        .unwrap();
    assert_eq!(only_heat.rows.len(), 1);
    assert_eq!(only_heat.rows[0].title, "Heat");

    let mut set = Item::set("Collection");
    set.core.data_source = lib.root_text();
    let set_id = lib.db.write(move |tx| insert_item(tx, &set)).unwrap();
    let mut member = lib.item(heat);
    member.parent_id = Some(set_id);
    lib.db
        .write(move |tx| cv_db::repo::update_item(tx, &member))
        .unwrap();
    let by_set = lib
        .service
        .preview(&lib.request(Scope::Items(vec![set_id])))
        .unwrap();
    assert_eq!(by_set.rows.len(), 1);
    assert_eq!(by_set.rows[0].title, "Heat");
}

/// A collection with two owned movies and one released movie the library lacks.
fn dune_collection(lib: &Lib) -> (ItemId, ItemId, ItemId) {
    let first = lib.movie("dune.2021", "dune", "Dune", 2021);
    let second = lib.movie("dune2.2024", "dune2", "Dune: Part Two", 2024);
    let mut set = Item::set("Dune Collection");
    set.core.data_source = lib.root_text();
    if let ItemDetail::Set(fields) = &mut set.detail {
        fields.missing_movies.push(cv_domain::MissingMovie {
            title: "Dune".to_owned(),
            year: 1984,
            release_date: cv_domain::CalendarDate::new(1984, 12, 14),
            ..cv_domain::MissingMovie::default()
        });
        // Not out yet: it takes no place in the order.
        fields.missing_movies.push(cv_domain::MissingMovie {
            title: "Dune: Messiah".to_owned(),
            year: 2999,
            ..cv_domain::MissingMovie::default()
        });
    }
    let set_id = lib.db.write(move |tx| insert_item(tx, &set)).unwrap();
    for movie in [first, second] {
        let mut member = lib.item(movie);
        member.parent_id = Some(set_id);
        lib.db
            .write(move |tx| cv_db::repo::update_item(tx, &member))
            .unwrap();
    }
    (set_id, first, second)
}

#[test]
fn a_collection_token_puts_the_movie_in_a_folder_named_after_its_collection() {
    let lib = Lib::new();
    let (_, first, second) = dune_collection(&lib);
    let lone = lib.movie("heat.1995", "heat", "Heat", 1995);
    let mut request = lib.request(Scope::Items(vec![first, second, lone]));
    request.profile.movie.folder_pattern = "${movieSet.title}/${title} (${year})".to_owned();
    let preview = lib.service.preview(&request).unwrap();
    let after = |title: &str| {
        preview
            .rows
            .iter()
            .find(|r| r.title == title)
            .unwrap()
            .after
            .clone()
    };
    assert_eq!(after("Dune"), "Dune Collection/Dune (2021)");
    assert_eq!(
        after("Dune: Part Two"),
        "Dune Collection/Dune - Part Two (2024)"
    );
    // Not in a collection: the token is empty and the leading separator is dropped.
    assert_eq!(after("Heat"), "Heat (1995)");
}

#[test]
fn a_collection_of_one_movie_is_left_out_unless_it_is_switched_on() {
    let lib = Lib::new();
    let movie = lib.movie("dune.2021", "dune", "Dune", 2021);
    let mut set = Item::set("Dune Collection");
    set.core.data_source = lib.root_text();
    let set_id = lib.db.write(move |tx| insert_item(tx, &set)).unwrap();
    let mut member = lib.item(movie);
    member.parent_id = Some(set_id);
    lib.db
        .write(move |tx| cv_db::repo::update_item(tx, &member))
        .unwrap();

    let mut request = lib.request(Scope::Items(vec![movie]));
    request.profile.movie.folder_pattern = "${movieSet.title}/${title} (${year})".to_owned();
    let off = lib.service.preview(&request).unwrap();
    assert_eq!(off.rows[0].after, "Dune (2021)");

    request.profile.movie.create_movieset_for_single_movie = true;
    let on = lib.service.preview(&request).unwrap();
    assert_eq!(on.rows[0].after, "Dune Collection/Dune (2021)");
}

#[test]
fn the_collection_index_follows_release_order_and_counts_missing_movies_in_the_second_form() {
    let lib = Lib::new();
    let (_, first, second) = dune_collection(&lib);
    let mut request = lib.request(Scope::Items(vec![first, second]));
    request.profile.movie.folder_pattern =
        "${movieSetIndex}${movieSetIndex2} ${title} (${year})".to_owned();
    let preview = lib.service.preview(&request).unwrap();
    let after = |title: &str| {
        preview
            .rows
            .iter()
            .find(|r| r.title == title)
            .unwrap()
            .after
            .clone()
    };
    // Dune (1984, missing) comes first: Dune (2021) is the owned movie 0 and the third entry 1;
    // Part Two is the owned movie 1 and the third entry 2. The unreleased film takes no place.
    assert_eq!(after("Dune"), "01 Dune (2021)");
    assert_eq!(after("Dune: Part Two"), "12 Dune - Part Two (2024)");
}

#[test]
fn a_show_is_previewed_applied_and_undone_with_its_seasons_and_episodes() {
    let lib = Lib::new();
    let source = lib.root_text();
    let show_path = lib.root.join("Show.Name.2020");
    let mut show = Item::show("Show Name");
    mark_scraped(&mut show);
    show.core.year = 2020;
    show.core.ids.set("tvdb", "1");
    show.core.last_scraper_id = "tvdb".to_owned();
    show.core.data_source = source.clone();
    show.core.path = show_path.to_string_lossy().into_owned();
    lib.put("Show.Name.2020/tvshow.nfo");
    show.core.media_files.push(MediaFile::new(
        MediaFileType::Nfo,
        show.core.path.clone(),
        "tvshow.nfo",
    ));
    let show_id = lib.db.write(move |tx| insert_item(tx, &show)).unwrap();

    let season_dir = show_path.join("S1");
    let mut episode = Item::episode(show_id, "Pilot");
    episode.core.data_source = source;
    episode.core.path = season_dir.to_string_lossy().into_owned();
    if let ItemDetail::Episode(fields) = &mut episode.detail {
        fields.numbers.set(EpisodeNumber::aired(1, 1));
    }
    lib.put("Show.Name.2020/S1/ep1.mkv");
    episode.core.media_files.push(MediaFile::new(
        MediaFileType::Video,
        episode.core.path.clone(),
        "ep1.mkv",
    ));
    lib.db.write(move |tx| insert_item(tx, &episode)).unwrap();

    let request = lib.request(Scope::Items(vec![show_id]));
    let preview = lib.service.preview(&request).unwrap();
    assert_eq!(preview.counts.change, 1, "{preview:?}");
    let row = &preview.rows[0];
    assert_eq!(row.kind, RowKind::Show);
    assert_eq!(row.after, "Show Name (2020)");
    assert!(
        row.files
            .iter()
            .any(|f| f.from == "Show.Name.2020/S1/ep1.mkv"
                && f.to == ["Show Name (2020)/Season 01/Show Name - S01E01 - Pilot.mkv"]),
        "the episode files are listed: {:?}",
        row.files
    );

    let applied = lib
        .service
        .apply(&ApplyRequest {
            preview: request,
            include: ids(&[show_id]),
        })
        .unwrap();
    assert_eq!(applied.done(), 1, "{applied:?}");
    let files = walk(&lib.root);
    assert!(
        files
            .iter()
            .any(|f| f.ends_with("Show Name (2020)/Season 01/Show Name - S01E01 - Pilot.mkv")),
        "{files:?}"
    );
    assert!(
        files
            .iter()
            .any(|f| f.ends_with("Show Name (2020)/tvshow.nfo"))
    );

    let undone = lib.service.undo(&applied.batch_id.unwrap()).unwrap();
    assert!(undone.is_complete(), "{undone:?}");
    assert!(lib.exists("Show.Name.2020/S1/ep1.mkv"));
}

#[test]
fn undo_of_an_unknown_batch_is_refused() {
    let lib = Lib::new();
    let error = lib.service.undo("no-such-batch").unwrap_err();
    assert!(matches!(error, OrganizeError::UnknownBatch(_)));
}

#[test]
fn start_up_recovery_settles_nothing_when_nothing_was_interrupted() {
    let lib = Lib::new();
    let report = recover_interrupted_batches(&lib.db).unwrap();
    assert!(report.rows.is_empty());
}

#[test]
fn start_up_recovery_settles_a_row_a_crash_left_planned() {
    use cv_rename::exec::{DbJournal, JournalEntry, JournalOp, JournalStore};
    let lib = Lib::new();
    // A move that happened on the disk but whose row was never settled.
    lib.put("new/file.mkv");
    let mut journal = DbJournal::new(Arc::clone(&lib.db));
    journal
        .begin(&JournalEntry {
            batch_id: "crashed".to_owned(),
            seq: 0,
            item_id: None,
            op: JournalOp::Move,
            from: lib
                .root
                .join("old")
                .join("file.mkv")
                .to_string_lossy()
                .into_owned(),
            to: lib
                .root
                .join("new")
                .join("file.mkv")
                .to_string_lossy()
                .into_owned(),
            is_dir: false,
            created_parents: Some(0),
        })
        .unwrap();

    let report = recover_interrupted_batches(&lib.db).unwrap();

    assert_eq!(report.rows.len(), 1);
    assert!(journal.pending().unwrap().is_empty());
}

#[test]
fn pattern_sets_round_trip_through_a_profile() {
    let mut profile = RenameProfile::from_preset(Preset::Clean);
    let mut set = PatternSet::of(&profile);
    assert_eq!(set.movie_folder, "${title} (${year})");
    set.movie_folder = "${title}".to_owned();
    set.apply_to(&mut profile);
    assert_eq!(profile.movie.folder_pattern, "${title}");
    assert_eq!(PatternSet::of(&profile), set);
}

#[test]
fn checking_a_pattern_reports_unknown_tokens_with_suggestions_and_positions() {
    let findings = check_pattern(PatternField::MovieFolder, "${title} (${yeer})");
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0].severity, FindingSeverity::Error);
    assert!(
        findings[0].suggestions.iter().any(|s| s == "year"),
        "{findings:?}"
    );
    assert!(findings[0].offset > 0);
    assert!(check_pattern(PatternField::MovieFolder, "${title} (${year})").is_empty());
    assert_eq!(
        check_pattern(PatternField::MovieFolder, "   ")[0].severity,
        FindingSeverity::Error
    );
    let episode = check_pattern(PatternField::EpisodeFile, "${showTitle} - ${title}");
    assert!(
        !episode.is_empty(),
        "an episode pattern needs an episode token"
    );
}

#[test]
fn template_problems_read_as_sentences_for_people() {
    let findings = check_pattern(PatternField::MovieFolder, "${titel} (${year})");
    assert_eq!(
        findings[0].message,
        "There is no token \u{201C}titel\u{201D}. Did you mean \u{201C}title\u{201D}?",
        "{findings:?}"
    );
    let episode = check_pattern(PatternField::EpisodeFile, "${showTitle} - ${title}");
    let message = &episode[0].message;
    assert!(
        message.starts_with("The episode file template needs"),
        "{message}"
    );
    assert!(
        message.ends_with('.') && !message.contains('`'),
        "{message}"
    );
}

#[test]
fn the_token_catalogue_lists_the_movie_and_tv_aliases() {
    let movie = token_catalogue(false);
    assert!(
        movie
            .iter()
            .any(|t| t.name == "title" && t.target == "movie.title")
    );
    assert!(movie.iter().any(|t| t.name == "videoFormat"));
    let tv = token_catalogue(true);
    assert!(tv.iter().any(|t| t.name == "episodeNr2"));
    assert!(
        !tv.iter()
            .any(|t| t.name == "videoFormat" && t.target.starts_with("movie."))
    );
}

// ---- the thumbnail cache after a rename ---------------------------------------------

mod thumbnails {
    use super::*;
    use cv_artwork::img::{ImageRequest, ImageService, SourceResolver};
    use cv_artwork::library_source::LibrarySources;
    use cv_artwork::thumb::{ThumbCache, ThumbKey, ThumbSize};
    use image::{ImageFormat, Rgba, RgbaImage};
    use std::io::Cursor;

    fn png() -> Vec<u8> {
        let img = RgbaImage::from_fn(600, 900, |x, y| {
            Rgba([40, (x % 255) as u8, (y % 255) as u8, 255])
        });
        let mut out = Vec::new();
        img.write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
            .unwrap();
        out
    }

    fn get(service: &ImageService, key: &ThumbKey, size: &str) -> cv_artwork::img::ImageResponse {
        let path = format!("/img/{key}/{size}");
        service.handle(&ImageRequest {
            method: "GET",
            path: &path,
            if_none_match: None,
        })
    }

    struct Setup {
        lib: Lib,
        sources: Arc<LibrarySources>,
        cache: Arc<ThumbCache>,
        images: ImageService,
        heat: ItemId,
        key: ThumbKey,
    }

    /// A library with one movie whose poster is a real image, shown once in the grid.
    fn shown_poster() -> Setup {
        let mut lib = Lib::new();
        let heat = lib.movie("heat.1995.1080p", "heat", "Heat", 1995);
        fs::write(lib.root.join("heat.1995.1080p/heat-poster.jpg"), png()).unwrap();
        let sources = Arc::new(LibrarySources::new());
        let observer: Arc<dyn cv_db::repo::rename_journal::PathObserver> = sources.clone();
        lib.service = lib.service.clone().with_path_observer(observer);
        let cache = Arc::new(ThumbCache::open(&lib._dir.path().join("cache"), None).unwrap());
        let images = ImageService::new(Arc::clone(&cache), sources.clone());
        let key = sources.posters_for(&lib.db, &[heat]).unwrap()[&heat].clone();
        assert_eq!(get(&images, &key, "grid").status, 200);
        assert_eq!(cache.generations(), 1);
        Setup {
            lib,
            sources,
            cache,
            images,
            heat,
            key,
        }
    }

    fn rename_heat(s: &Setup) -> String {
        let request = ApplyRequest {
            preview: s.lib.request(Scope::Everything),
            include: ids(&[s.heat]),
        };
        let applied = s.lib.service.apply(&request).unwrap();
        assert_eq!(applied.done(), 1);
        applied.batch_id.unwrap()
    }

    #[test]
    fn posters_display_from_the_cache_right_after_a_rename_without_a_rescan() {
        let s = shown_poster();
        let before = get(&s.images, &s.key, "grid").body;

        rename_heat(&s);

        // The index already points at the renamed file.
        let new_path = s.sources.source_of(&s.key).unwrap();
        assert!(
            new_path.ends_with("Heat (1995)/Heat (1995)-poster.jpg"),
            "{new_path:?}"
        );
        assert!(new_path.exists());
        // The grid asks the library again: same key, found without reading the file.
        let keys = s.sources.posters_for(&s.lib.db, &[s.heat]).unwrap();
        assert_eq!(keys[&s.heat], s.key);
        // The cached thumbnail is served, not generated again.
        let after = get(&s.images, &s.key, "grid");
        assert_eq!(after.status, 200);
        assert_eq!(after.body, before);
        assert_eq!(s.cache.generations(), 1);
        assert!(s.cache.has(&s.key, ThumbSize::Grid));
    }

    #[test]
    fn a_size_not_cached_yet_is_generated_from_the_new_path_after_a_rename() {
        let s = shown_poster();
        assert!(!s.cache.has(&s.key, ThumbSize::Hero));

        rename_heat(&s);
        let hero = get(&s.images, &s.key, "hero");

        assert_eq!(hero.status, 200);
        assert_eq!(s.cache.generations(), 2);
        assert_eq!(s.sources.len(), 1);
    }

    #[test]
    fn undo_points_the_index_back_and_the_thumbnail_still_serves() {
        let s = shown_poster();
        let batch = rename_heat(&s);

        s.lib.service.undo(&batch).unwrap();

        let path = s.sources.source_of(&s.key).unwrap();
        assert!(
            path.ends_with("heat.1995.1080p/heat-poster.jpg"),
            "{path:?}"
        );
        assert!(path.exists());
        assert_eq!(get(&s.images, &s.key, "grid").status, 200);
        assert_eq!(get(&s.images, &s.key, "list").status, 200);
        assert_eq!(s.cache.generations(), 2);
    }
}

#[test]
fn a_title_whose_files_are_missing_or_offline_is_never_planned() {
    let lib = Lib::new();
    let missing = lib.movie("heat.1995.1080p", "heat", "Heat", 1995);
    let offline = lib.movie("alien.1979", "alien", "Alien", 1979);
    lib.db
        .write(move |tx| {
            cv_db::repo::set_availability(tx, missing, cv_domain::Availability::Missing, "gone")?;
            cv_db::repo::set_availability(tx, offline, cv_domain::Availability::Offline, "")
        })
        .unwrap();

    let preview = lib
        .service
        .preview(&lib.request(Scope::Everything))
        .unwrap();

    assert_eq!(preview.counts.change, 0);
    assert_eq!(preview.counts.unplanned, 2);
    for row in &preview.rows {
        assert!(
            matches!(&row.status, RowStatus::Unplanned(reason) if !reason.is_empty()),
            "{row:?}"
        );
        assert_eq!(row.moves, 0);
    }
}

/// Starts a rescan of `source` on another thread at the first move an organize makes, and notes
/// whether it finished while the organize was still moving files.
struct RescanMidway {
    db: Arc<Database>,
    source: cv_domain::DataSource,
    started: std::sync::Mutex<Option<std::thread::JoinHandle<()>>>,
    finished: Arc<std::sync::atomic::AtomicBool>,
    finished_midway: std::sync::atomic::AtomicBool,
}

impl cv_rename::exec::PathObserver for RescanMidway {
    fn path_moved(&self, _from: &str, _to: &str, _is_dir: bool) {
        use std::sync::atomic::Ordering;
        let mut started = self.started.lock().unwrap();
        if started.is_some() {
            return;
        }
        let (db, source, finished) = (
            Arc::clone(&self.db),
            self.source.clone(),
            Arc::clone(&self.finished),
        );
        *started = Some(std::thread::spawn(move || {
            cv_scan::rescan::rescan_source(
                &db,
                &source,
                &cv_scan::rescan::RescanOptions::default(),
            )
            .unwrap();
            finished.store(true, Ordering::SeqCst);
        }));
        drop(started);
        // Time enough for a rescan of a few files to walk and write, were it let through.
        std::thread::sleep(std::time::Duration::from_millis(400));
        if self.finished.load(Ordering::SeqCst) {
            self.finished_midway.store(true, Ordering::SeqCst);
        }
    }
}

impl RescanMidway {
    fn new(db: &Arc<Database>, source: &cv_domain::DataSource) -> Arc<Self> {
        Arc::new(RescanMidway {
            db: Arc::clone(db),
            source: source.clone(),
            started: std::sync::Mutex::new(None),
            finished: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            finished_midway: std::sync::atomic::AtomicBool::new(false),
        })
    }

    /// Waits for the rescan; true when it ran while the files were moving.
    fn ran_midway(&self) -> bool {
        if let Some(thread) = self.started.lock().unwrap().take() {
            thread.join().unwrap();
        }
        self.finished_midway
            .load(std::sync::atomic::Ordering::SeqCst)
    }
}

fn show_rows(db: &Database) -> Vec<String> {
    db.read(|c| {
        let mut stmt = c.prepare("SELECT path FROM item WHERE kind = 'show' ORDER BY id")?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
    .unwrap()
}

/// reg2-1: a rescan the folder watcher started while an organize renamed a show folder walked
/// the old folder name and read the new one from the library (or the other way round). Shows
/// pair by folder, so it added the show a second time; the next organize and undo then left two
/// rows on one folder. A rescan now waits until the files and the library agree.
#[test]
fn a_rescan_during_a_show_organize_or_its_undo_waits_and_adds_no_second_show() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("TV");
    for rel in [
        "The Office (US)/Season 1/The Office (US) S01E01.mkv",
        "The Office (US)/Season 1/The Office (US) S01E02.mkv",
        "The Office (US)/Specials/The Office (US) S00E01.mkv",
    ] {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, rel).unwrap();
    }
    fs::create_dir_all(root.join("The Office (US)/Season 01")).unwrap();
    let db = Arc::new(Database::open(dir.path().join("library.db")).unwrap());
    let text = root.to_string_lossy().into_owned();
    let source = db
        .write(move |tx| insert_source(tx, SourceKind::Tv, &text))
        .unwrap();
    let options = cv_scan::rescan::RescanOptions::default();
    cv_scan::rescan::rescan_source(&db, &source, &options).unwrap();
    let original = root.join("The Office (US)").to_string_lossy().into_owned();
    assert_eq!(show_rows(&db), std::slice::from_ref(&original));
    let show = db
        .read(|c| {
            c.query_row("SELECT id FROM item WHERE kind = 'show'", [], |r| {
                r.get(0).map(ItemId)
            })
            .map_err(Into::into)
        })
        .unwrap();
    // Scraped: Organize plans only scraped titles.
    db.write(move |tx| {
        tx.execute(
            "INSERT INTO ext_id (item_id, provider, value) VALUES (?1, 'tvdb', '73244')",
            [show.0],
        )?;
        tx.execute(
            "UPDATE item SET last_scraper_id = 'tvdb' WHERE id = ?1",
            [show.0],
        )?;
        Ok(())
    })
    .unwrap();

    for _cycle in 0..2 {
        let midway = RescanMidway::new(&db, &source);
        let service = OrganizeService::new(Arc::clone(&db))
            .with_path_observer(Arc::clone(&midway) as Arc<dyn cv_rename::exec::PathObserver>);
        let request = PreviewRequest {
            scope: Scope::Items(vec![show]),
            profile: RenameProfile::from_preset(Preset::Clean),
            choices: BTreeMap::new(),
        };
        let applied = service
            .apply(&ApplyRequest {
                preview: request,
                include: ids(&[show]),
            })
            .unwrap();
        assert_eq!(applied.done(), 1, "{applied:?}");
        assert!(!midway.ran_midway(), "the rescan waited for the organize");
        let renamed = root.join("The Office US").to_string_lossy().into_owned();
        assert_eq!(show_rows(&db), [renamed], "one show, in the renamed folder");

        let midway = RescanMidway::new(&db, &source);
        let service = OrganizeService::new(Arc::clone(&db))
            .with_path_observer(Arc::clone(&midway) as Arc<dyn cv_rename::exec::PathObserver>);
        let undone = service.undo(&applied.batch_id.unwrap()).unwrap();
        assert!(undone.is_complete(), "{undone:?}");
        assert!(!midway.ran_midway(), "the rescan waited for the undo");
        assert_eq!(
            show_rows(&db),
            std::slice::from_ref(&original),
            "one show, back in its folder"
        );
    }
    cv_scan::rescan::rescan_source(&db, &source, &options).unwrap();
    assert_eq!(show_rows(&db), [original]);
}

/// Marks a fixture title scraped: Organize plans only scraped titles.
fn mark_scraped(item: &mut Item) {
    if item.core.ids.get("tmdb").is_none() && item.core.ids.get("tvdb").is_none() {
        item.core.ids.set("tmdb", "1");
    }
    item.core.last_scraper_id = "tmdb".to_owned();
}

/// A title that is not scraped yet (new, not matched, waiting for a decision) is listed as
/// waiting for its scrape, never planned and never renamed, even when it is checked.
#[test]
fn a_title_that_is_not_scraped_is_left_out_until_it_is() {
    let lib = Lib::new();
    let heat = lib.movie("heat.1995.1080p", "heat", "Heat", 1995);
    let raw = lib.movie("arrival.2016", "arrival", "Arrival", 2016);
    // Arrival waits for a decision: no provider ID and no scrape.
    lib.db
        .write(move |tx| {
            tx.execute("DELETE FROM ext_id WHERE item_id = ?1", [raw.0])?;
            tx.execute(
                "UPDATE item SET last_scraper_id = '' WHERE id = ?1",
                [raw.0],
            )?;
            Ok(())
        })
        .unwrap();
    let request = lib.request(Scope::Everything);

    let preview = lib.service.preview(&request).unwrap();
    let status = |id: ItemId| {
        preview
            .rows
            .iter()
            .find(|row| row.id == id.0.to_string())
            .map(|row| row.status.clone())
    };
    assert_eq!(status(heat), Some(RowStatus::Change));
    assert_eq!(status(raw), Some(RowStatus::Unscraped));
    assert_eq!(preview.counts.change, 1);

    let states = lib.service.item_states(&request).unwrap();
    let state = states.iter().find(|s| s.item == raw).unwrap();
    assert_eq!(
        state.state,
        cv_db::repo::organize_state::OrganizeStateKind::Unplanned
    );

    let applied = lib
        .service
        .apply(&ApplyRequest {
            preview: request,
            include: ids(&[heat, raw]),
        })
        .unwrap();
    assert_eq!(applied.items.len(), 1, "{applied:?}");
    assert!(applied.not_run.contains(&raw.0.to_string()));
    assert!(
        lib.exists("arrival.2016/arrival.mkv"),
        "its files keep their names"
    );
    assert!(lib.exists("Heat (1995)"));
}

/// A leftover that an apply sends to the trash folder stops being one of the title's
/// files, so the title is tidy afterwards instead of planning the gone file again.
#[test]
fn a_file_the_apply_sends_to_the_trash_leaves_the_title_tidy() {
    let lib = Lib::new();
    let heat = lib.movie("heat.1995.1080p", "heat", "Heat", 1995);
    // A second poster: the profile has one name for it, so one of the two is a leftover.
    let mut item = lib.item(heat);
    lib.put("heat.1995.1080p/poster.jpg");
    item.core.media_files.push(MediaFile::new(
        MediaFileType::Poster,
        item.core.path.clone(),
        "poster.jpg",
    ));
    lib.db
        .write(move |tx| cv_db::repo::update_item(tx, &item))
        .unwrap();
    let request = lib.request(Scope::Items(vec![heat]));
    let before = lib.service.preview(&request).unwrap();
    let trashed: u32 = before.rows.iter().map(|row| row.trashed).sum();
    assert!(trashed > 0, "a poster goes to the trash: {before:?}");

    let applied = lib
        .service
        .apply(&ApplyRequest {
            preview: request.clone(),
            include: ids(&[heat]),
        })
        .unwrap();
    assert_eq!(applied.done(), 1, "{applied:?}");

    let after = lib.service.preview(&request).unwrap();
    assert!(after.rows.is_empty(), "nothing is left to do: {after:?}");
    assert_eq!(after.counts.tidy, 1);
    let files = lib.item(heat).core.media_files;
    assert!(
        files.iter().all(|f| f.path().exists()),
        "every listed file is on disk: {files:?}"
    );
}

// ---- path edge cases -------------------------------------------

/// The names in a folder, as the disk spells them.
fn names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

// On Windows and macOS the two spellings are one folder, so a case-only change must not be read
// as a conflict with itself; on Linux they are two names and it is an ordinary rename.
#[test]
fn a_case_only_rename_is_previewed_applied_and_undone() {
    let lib = Lib::new();
    let heat = lib.movie("heat (1995)", "heat (1995)", "Heat", 1995);
    let request = lib.request(Scope::Items(vec![heat]));

    let preview = lib.service.preview(&request).unwrap();
    let row = preview.rows.iter().find(|r| r.id == heat.0.to_string());
    assert_eq!(
        row.map(|r| &r.status),
        Some(&RowStatus::Change),
        "a case-only change is a change, not a conflict: {:?}",
        preview.rows
    );

    let applied = lib
        .service
        .apply(&ApplyRequest {
            preview: request,
            include: ids(&[heat]),
        })
        .unwrap();
    assert_eq!(applied.done(), 1, "{applied:?}");
    assert_eq!(names_in(&lib.root), ["Heat (1995)"]);
    assert_eq!(
        names_in(&lib.root.join("Heat (1995)")),
        [
            "Heat (1995)-poster.jpg",
            "Heat (1995).mkv",
            "Heat (1995).nfo"
        ]
    );
    assert!(
        !lib.root.join(cv_rename::exec::TRASH_DIR).exists(),
        "nothing was trashed"
    );
    assert!(lib.item(heat).core.path.ends_with("Heat (1995)"));

    let undone = lib.service.undo(&applied.batch_id.unwrap()).unwrap();
    assert!(undone.is_complete(), "{undone:?}");
    assert_eq!(names_in(&lib.root), ["heat (1995)"]);
    assert_eq!(
        names_in(&lib.root.join("heat (1995)")),
        [
            "heat (1995)-poster.jpg",
            "heat (1995).mkv",
            "heat (1995).nfo"
        ]
    );
    assert!(lib.item(heat).core.path.ends_with("heat (1995)"));
}

/// A title whose names use media-info tokens waits while its video details are not read: the
/// tokens would render empty and strip the quality tags from its name. It is held back in the
/// preview, never applied, and renamed once the details are read. A profile without media tokens
/// renames it at once.
#[test]
fn a_title_whose_video_details_are_not_read_is_held_back_when_the_names_use_media_tokens() {
    let lib = Lib::new();
    let folder = "Killers of the Flower Moon (2023) [2160p] [Dolby Vision, HDR10] [DTSHD-MA 5.1]";
    let killers = lib.movie(folder, "killers", "Killers of the Flower Moon", 2023);
    let mut request = lib.request(Scope::Everything);
    request.profile = RenameProfile::from_preset(Preset::Detailed);

    let preview = lib.service.preview(&request).unwrap();
    let row = preview
        .rows
        .iter()
        .find(|row| row.id == killers.0.to_string())
        .expect("the held title is listed");
    assert_eq!(row.status, RowStatus::MediaUnread);
    assert_eq!(row.after, folder, "no new name is offered");
    assert_eq!(preview.counts.change, 0);
    let states = lib.service.item_states(&request).unwrap();
    let state = states.iter().find(|s| s.item == killers).unwrap();
    assert_eq!(
        state.state,
        cv_db::repo::organize_state::OrganizeStateKind::Unplanned
    );
    assert_eq!(state.reason, cv_rename::MEDIA_HELD_REASON);

    // Checked or not, it is never applied.
    let apply = ApplyRequest {
        preview: request.clone(),
        include: ids(&[killers]),
    };
    let applied = lib.service.apply(&apply).unwrap();
    assert!(applied.items.is_empty(), "{applied:?}");
    assert_eq!(applied.not_run, ids(&[killers]));
    assert!(lib.exists(&format!("{folder}/killers.mkv")));

    // Once the details are read, the name keeps its tags.
    let mut item = lib.item(killers);
    let video = item
        .core
        .media_files
        .iter_mut()
        .find(|f| f.kind == MediaFileType::Video)
        .unwrap();
    video.container_format = "Matroska".into();
    video.video_codec = "HEVC".into();
    video.width = 3840;
    video.height = 2160;
    video.hdr_formats = vec!["Dolby Vision".into(), "HDR10".into()];
    video.audio_streams.push(cv_domain::AudioStream {
        codec: "DTS-HD MA".into(),
        channels: 6,
        ..cv_domain::AudioStream::default()
    });
    lib.db
        .write(move |tx| cv_db::repo::update_item(tx, &item))
        .unwrap();
    let preview = lib.service.preview(&request).unwrap();
    let row = &preview.rows[0];
    assert_eq!(row.status, RowStatus::Change, "{row:?}");
    assert!(row.after.contains("[2160p]"), "{}", row.after);
    assert!(row.after.contains("Dolby Vision"), "{}", row.after);
}

#[test]
fn a_title_whose_video_details_are_not_read_renames_when_the_names_use_no_media_token() {
    let lib = Lib::new();
    let killers = lib.movie(
        "killers.2023",
        "killers",
        "Killers of the Flower Moon",
        2023,
    );
    // Clean: title and year only.
    let request = lib.request(Scope::Everything);
    let preview = lib.service.preview(&request).unwrap();
    assert_eq!(preview.rows[0].status, RowStatus::Change);
    let applied = lib
        .service
        .apply(&ApplyRequest {
            preview: request,
            include: ids(&[killers]),
        })
        .unwrap();
    assert_eq!(applied.done(), 1, "{applied:?}");
    assert!(lib.exists("Killers of the Flower Moon (2023)/Killers of the Flower Moon (2023).mkv"));
}
