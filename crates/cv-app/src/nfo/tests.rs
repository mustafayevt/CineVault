//! End-to-end tests of Read NFO, Rewrite NFO and undo over a real database and real folders.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use cv_db::Database;
use cv_db::repo::{get_item, insert_item, set_field_lock, set_item_locked, update_item};
use cv_domain::{
    AiredStatus, CalendarDate, Credit, CreditKind, EpisodeNumber, EpisodeNumbers, Genre, Item,
    ItemDetail, ItemId, MediaFile, MediaFileType, MetaField, MissingEpisode,
};
use serde_json::{Value, json};

use super::report::{FileOutcome, SkipReason, UnitStatus};
use super::service::{NfoError, NfoScope, NfoService};
use super::settings::NfoSettings;

const RELEASE_NOTE: &str = "\
 ____  _____ _     _____    _    ____  _____
|  _ \\| ____| |   | ____|  / \\  / ___|| ____|
Release.Group.Presents
Movie.Name.1995.1080p.BluRay.x264-GROUP
IMDb: https://www.imdb.com/title/tt0113277/
";

struct Lab {
    root: tempfile::TempDir,
    db: Arc<Database>,
    service: NfoService,
}

impl Lab {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let db = Arc::new(Database::open(root.path().join("library.db")).unwrap());
        let service = NfoService::new(Arc::clone(&db), root.path().join("undo"));
        Self { root, db, service }
    }

    fn dir(&self, relative: &str) -> PathBuf {
        let path = self.root.path().join("media").join(relative);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn insert(&self, item: Item) -> ItemId {
        self.db.write(move |tx| insert_item(tx, &item)).unwrap()
    }

    fn item(&self, id: ItemId) -> Item {
        self.db.read(|c| get_item(c, id)).unwrap().unwrap()
    }

    fn save(&self, item: Item) {
        self.db.write(move |tx| update_item(tx, &item)).unwrap();
    }

    /// A movie "Heat (1995)" in its own folder with a video file.
    fn movie(&self, title: &str, year: u16) -> (ItemId, PathBuf) {
        let folder = self.dir(&format!("Movies/{title} ({year})"));
        let file = format!("{title} ({year}).mkv");
        fs::write(folder.join(&file), b"video").unwrap();
        let mut item = Item::movie(title);
        item.core.year = year;
        item.core.plot = "A crew of thieves and the cop who hunts them.".into();
        item.core.path = folder.to_string_lossy().into_owned();
        item.core.date_added_ms = 1_700_000_000_000;
        item.core.ids.set("imdb", "tt0113277");
        item.core.ids.set("tmdb", "949");
        item.core
            .credits
            .push(Credit::new(CreditKind::Actor, "Al Pacino", "Hanna"));
        let mut video = MediaFile::new(MediaFileType::Video, folder.to_string_lossy(), file);
        video.duration_secs = 170 * 60;
        item.core.media_files.push(video);
        if let ItemDetail::Movie(m) = &mut item.detail {
            m.runtime_minutes = 170;
            m.genres = vec![Genre::Drama];
        }
        (self.insert(item), folder)
    }
}

fn settings(pairs: &[(&str, Value)]) -> NfoSettings {
    NfoSettings::from_values(
        &pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect(),
    )
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap()
}

fn modified(path: &Path) -> std::time::SystemTime {
    fs::metadata(path).unwrap().modified().unwrap()
}

fn no_comments(text: &str) -> String {
    cv_nfo::xml::strip_comments(text)
}

fn all() -> NfoScope {
    NfoScope::All
}

fn only(id: ItemId) -> NfoScope {
    NfoScope::Items(vec![id])
}

// ---- Rewrite NFO ------------------------------------------------------------------------------

#[test]
fn a_rewrite_waits_while_the_title_is_moved_and_writes_after() {
    use crate::title_guard::{self, Work};
    use std::time::{Duration, Instant};
    let lab = Lab::new();
    let (id, folder) = lab.movie("Heat", 1995);
    let nfo = folder.join("Heat (1995).nfo");
    // A move holds the title: no NFO is written into the folder while it moves.
    let moving = title_guard::hold(&lab.db, [id], Work::Move, Duration::ZERO).unwrap();
    let started = Instant::now();
    let report = std::thread::scope(|scope| {
        let writer = scope.spawn(|| lab.service.rewrite(&only(id), &NfoSettings::default()));
        std::thread::sleep(Duration::from_millis(300));
        assert!(!nfo.exists(), "nothing was written while the title moved");
        drop(moving);
        writer.join().unwrap()
    })
    .unwrap();
    assert!(started.elapsed() >= Duration::from_millis(300));
    assert_eq!(report.totals.changed, 1, "{report:?}");
    assert!(nfo.is_file());
}

#[test]
fn an_undo_waits_while_the_title_is_moved_and_undoes_after() {
    // Review 14.7: the undo of an NFO action held no title, so it could put files back into a
    // folder in the middle of a move or a rename.
    use crate::title_guard::{self, Work};
    use std::time::Duration;
    let lab = Lab::new();
    let (id, folder) = lab.movie("Heat", 1995);
    let nfo = folder.join("Heat (1995).nfo");
    let report = lab
        .service
        .rewrite(&only(id), &NfoSettings::default())
        .unwrap();
    let token = report.token.expect("an undo record");
    assert!(nfo.is_file());

    let moving = title_guard::hold(&lab.db, [id], Work::Move, Duration::ZERO).unwrap();
    let summary = std::thread::scope(|scope| {
        let undo = scope.spawn(|| lab.service.undo(&token));
        std::thread::sleep(Duration::from_millis(300));
        assert!(nfo.is_file(), "nothing was undone while the title moved");
        drop(moving);
        undo.join().unwrap()
    })
    .unwrap();
    assert_eq!(summary.restored, 1, "{summary:?}");
    assert!(!nfo.exists());
}

#[test]
fn a_written_nfo_is_listed_with_its_title_while_the_title_is_still_held() {
    // Review 14.7: a rewrite listed the NFOs it wrote among their titles' files only after its
    // last title, when it held none of them. A title renamed in between (here while the rewrite
    // waits for the second title) got its NFO listed under the old folder, a file that is not
    // there.
    use crate::title_guard::{self, Work};
    use std::time::Duration;
    let lab = Lab::new();
    let (heat, heat_folder) = lab.movie("Heat", 1995);
    let (alien, _) = lab.movie("Alien", 1979);
    let subtitle = title_guard::hold(&lab.db, [alien], Work::Write, Duration::ZERO).unwrap();
    let report = std::thread::scope(|scope| {
        let writer = scope.spawn(|| {
            lab.service
                .rewrite(&NfoScope::Items(vec![heat, alien]), &NfoSettings::default())
        });
        // Heat's NFO is written; the rewrite now waits for Alien.
        let nfo = heat_folder.join("Heat (1995).nfo");
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !nfo.is_file() {
            assert!(
                std::time::Instant::now() < deadline,
                "Heat's NFO was not written"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        // A rename of Heat runs meanwhile: the folder and the library move on.
        let renamed = {
            let _rename =
                title_guard::hold(&lab.db, [heat], Work::Rename, Duration::from_secs(5)).unwrap();
            let renamed = heat_folder.with_file_name("Heat (1995) renamed");
            fs::rename(&heat_folder, &renamed).unwrap();
            let mut item = lab.item(heat);
            let old = item.core.path.clone();
            let new = renamed.to_string_lossy().into_owned();
            item.core.path.clone_from(&new);
            for file in &mut item.core.media_files {
                file.directory = file.directory.replace(&old, &new);
            }
            lab.save(item);
            renamed
        };
        drop(subtitle);
        let report = writer.join().unwrap().unwrap();
        assert!(renamed.join("Heat (1995).nfo").is_file());
        report
    });
    assert_eq!(report.totals.changed, 2, "{report:?}");
    for file in &lab.item(heat).core.media_files {
        assert!(
            file.path().is_file(),
            "{} is listed but not there",
            file.path().display()
        );
    }
    assert!(
        lab.item(heat)
            .core
            .media_files
            .iter()
            .any(|f| f.kind == MediaFileType::Nfo),
        "the NFO is listed"
    );
}

#[test]
fn rewrite_writes_the_movie_nfo_and_leaves_an_unchanged_one_alone() {
    let lab = Lab::new();
    let (id, folder) = lab.movie("Heat", 1995);
    let nfo = folder.join("Heat (1995).nfo");

    let first = lab
        .service
        .rewrite(&only(id), &NfoSettings::default())
        .unwrap();
    assert_eq!(first.totals.changed, 1);
    assert_eq!(first.units[0].files[0].outcome, FileOutcome::Created);
    let text = read(&nfo);
    assert!(text.contains("<title>Heat</title>") && text.contains("tt0113277"));
    assert!(text.contains("\r\n"), "CRLF line ends");
    let stamp = modified(&nfo);

    std::thread::sleep(std::time::Duration::from_millis(30));
    let second = lab
        .service
        .rewrite(&only(id), &NfoSettings::default())
        .unwrap();
    assert_eq!(second.units[0].status, UnitStatus::Unchanged);
    assert_eq!(second.token, None, "nothing changed, nothing to undo");
    assert_eq!(modified(&nfo), stamp);
}

#[test]
fn a_foreign_info_nfo_is_never_touched() {
    let lab = Lab::new();
    let (id, folder) = lab.movie("Heat", 1995);
    let info = folder.join("info.nfo");
    fs::write(&info, RELEASE_NOTE).unwrap();
    let stamp = modified(&info);
    // An NFO of the item under its old naming, which cleanup may move.
    let old = folder.join("Heat (1995).nfo");
    fs::write(&old, "<movie><title>Heat</title></movie>").unwrap();

    std::thread::sleep(std::time::Duration::from_millis(30));
    // Write under a new name, which makes the old one stale, once for one movie and once for all.
    let movie_only = settings(&[("metadata.nfo-movie-names", json!("movie"))]);
    for scope in [only(id), all()] {
        let report = lab.service.rewrite(&scope, &movie_only).unwrap();
        assert_eq!(report.totals.failed, 0);
        assert_eq!(read(&info), RELEASE_NOTE, "content");
        assert_eq!(modified(&info), stamp, "modification time");
        assert!(
            report
                .units
                .iter()
                .flat_map(|u| &u.files)
                .all(|f| f.path != info)
        );
    }
    assert!(folder.join("movie.nfo").exists());
    assert!(!old.exists(), "the stale managed NFO was moved away");

    // Reading never touches it either.
    lab.service.read(&all(), &movie_only).unwrap();
    assert_eq!(read(&info), RELEASE_NOTE);
    assert_eq!(modified(&info), stamp);
}

#[test]
fn a_foreign_file_at_a_target_name_is_not_overwritten_and_blocks_cleanup() {
    let lab = Lab::new();
    let (id, folder) = lab.movie("Heat", 1995);
    let note = folder.join("movie.nfo");
    fs::write(&note, RELEASE_NOTE).unwrap();
    let managed = folder.join("Heat (1995).nfo");
    fs::write(
        &managed,
        "<movie><title>Heat</title><year>1995</year></movie>",
    )
    .unwrap();
    let before = read(&managed);

    // Only `movie.nfo` is wanted, and a release note sits there.
    let movie_only = settings(&[("metadata.nfo-movie-names", json!("movie"))]);
    let report = lab.service.rewrite(&only(id), &movie_only).unwrap();
    assert_eq!(report.units[0].status, UnitStatus::KeptForeign);
    assert_eq!(read(&note), RELEASE_NOTE);
    assert_eq!(
        read(&managed),
        before,
        "the working NFO stays when nothing replaced it"
    );
    assert_eq!(report.token, None);

    // Both names: the free name is written, the foreign one is reported and kept.
    let both = settings(&[("metadata.nfo-movie-names", json!("both"))]);
    let report = lab.service.rewrite(&only(id), &both).unwrap();
    assert_eq!(report.units[0].status, UnitStatus::Changed);
    assert_eq!(read(&note), RELEASE_NOTE);
    let outcomes: Vec<_> = report.units[0]
        .files
        .iter()
        .map(|f| f.outcome.clone())
        .collect();
    assert!(outcomes.contains(&FileOutcome::KeptForeign));
    assert!(read(&managed).contains("<tmdbid>949</tmdbid>"));
}

#[test]
fn several_names_get_identical_content() {
    let lab = Lab::new();
    let (id, folder) = lab.movie("Heat", 1995);
    let both = settings(&[("metadata.nfo-movie-names", json!("both"))]);
    lab.service.rewrite(&only(id), &both).unwrap();
    assert_eq!(
        no_comments(&read(&folder.join("Heat (1995).nfo"))),
        no_comments(&read(&folder.join("movie.nfo")))
    );
}

#[test]
fn unknown_elements_and_carried_values_survive_a_rewrite() {
    let lab = Lab::new();
    let (id, folder) = lab.movie("Heat", 1995);
    let nfo = folder.join("Heat (1995).nfo");
    fs::write(
        &nfo,
        "<movie><title>Heat</title><customfield>abc</customfield><epbookmark>12.5</epbookmark></movie>",
    )
    .unwrap();
    lab.service
        .rewrite(&only(id), &NfoSettings::default())
        .unwrap();
    let text = read(&nfo);
    assert!(text.contains("<customfield>abc</customfield>"));
    assert!(text.contains("<epbookmark>12.5</epbookmark>"));

    // "Write clean NFO" drops both.
    let clean = settings(&[("metadata.nfo-clean", json!(true))]);
    lab.service.rewrite(&only(id), &clean).unwrap();
    let text = read(&nfo);
    assert!(!text.contains("customfield") && !text.contains("12.5"));
}

#[test]
fn date_added_can_come_from_the_file_creation_time() {
    let lab = Lab::new();
    let (id, folder) = lab.movie("Heat", 1995);
    let video = folder.join("Heat (1995).mkv");
    let (created, _) = super::time::file_times(&video);
    let created = created.unwrap();

    let from_file = settings(&[("metadata.nfo-date-added-source", json!("file-created"))]);
    lab.service.rewrite(&only(id), &from_file).unwrap();
    let text = read(&folder.join("Heat (1995).nfo"));
    assert!(
        text.contains(&format!("<dateadded>{created}</dateadded>")),
        "{text}"
    );

    // The library's own date is the default source.
    lab.service
        .rewrite(&only(id), &NfoSettings::default())
        .unwrap();
    let text = read(&folder.join("Heat (1995).nfo"));
    let added = super::time::local_from_millis(1_700_000_000_000).unwrap();
    assert!(text.contains(&format!("<dateadded>{added}</dateadded>")));
}

#[test]
fn the_nfo_options_change_what_is_written() {
    let lab = Lab::new();
    let (id, folder) = lab.movie("Heat", 1995);
    let nfo = folder.join("Heat (1995).nfo");
    let off = settings(&[
        ("metadata.nfo-date-added", json!(false)),
        ("metadata.nfo-fileinfo", json!(false)),
        ("metadata.nfo-lockdata", json!(true)),
        ("metadata.nfo-create-outline", json!(true)),
    ]);
    lab.service.rewrite(&only(id), &off).unwrap();
    let text = read(&nfo);
    assert!(!text.contains("<dateadded>") && !text.contains("<fileinfo>"));
    assert!(text.contains("<lockdata>true</lockdata>"));
    assert!(text.contains("<outline>"));
}

#[test]
fn a_missing_folder_is_reported_and_not_recreated() {
    let lab = Lab::new();
    let (id, folder) = lab.movie("Heat", 1995);
    fs::remove_dir_all(&folder).unwrap();
    let report = lab
        .service
        .rewrite(&only(id), &NfoSettings::default())
        .unwrap();
    assert_eq!(
        report.units[0].status,
        UnitStatus::Skipped {
            reason: SkipReason::FolderMissing
        }
    );
    assert!(!folder.exists());
    assert_eq!(report.totals.skipped, 1);
}

#[test]
fn unknown_ids_are_reported() {
    let lab = Lab::new();
    let report = lab
        .service
        .rewrite(
            &NfoScope::Items(vec![ItemId(9999)]),
            &NfoSettings::default(),
        )
        .unwrap();
    assert_eq!(
        report.units[0].status,
        UnitStatus::Skipped {
            reason: SkipReason::NotFound
        }
    );
}

#[test]
fn sets_are_written_under_the_collection_folder_only() {
    let lab = Lab::new();
    let mut set = Item::set("Heat Collection");
    set.core.plot = "All of them.".into();
    let id = lab.insert(set);

    let none = lab
        .service
        .rewrite(&only(id), &NfoSettings::default())
        .unwrap();
    assert_eq!(
        none.units[0].status,
        UnitStatus::Skipped {
            reason: SkipReason::NoCollectionFolder
        }
    );

    let folder = lab.root.path().join("collections");
    let with = settings(&[(
        "metadata.nfo-collection-folder",
        json!(folder.to_string_lossy()),
    )]);
    let report = lab.service.rewrite(&only(id), &with).unwrap();
    assert_eq!(report.units[0].status, UnitStatus::Changed);
    let nfo = folder.join("Heat Collection").join("Heat Collection.nfo");
    assert!(read(&nfo).contains("<title>Heat Collection</title>"));
}

// ---- undo of a rewrite -------------------------------------------------------------------------

#[test]
fn undo_puts_back_replaced_created_and_moved_files() {
    let lab = Lab::new();
    let (id, folder) = lab.movie("Heat", 1995);
    let old = folder.join("Heat (1995).nfo");
    let old_text = "<movie><title>Heat</title><year>1995</year></movie>";
    fs::write(&old, old_text).unwrap();

    // Rename the NFO to `movie.nfo`: one file created, the old one moved to the trash.
    let movie_only = settings(&[("metadata.nfo-movie-names", json!("movie"))]);
    let report = lab.service.rewrite(&only(id), &movie_only).unwrap();
    let token = report.token.clone().expect("something changed");
    assert!(folder.join("movie.nfo").exists() && !old.exists());

    let summary = lab.service.undo(&token).unwrap();
    assert!(summary.skipped.is_empty(), "{:?}", summary.skipped);
    assert_eq!(summary.restored, 2);
    assert_eq!(read(&old), old_text);
    assert!(!folder.join("movie.nfo").exists());

    assert!(matches!(
        lab.service.undo(&token),
        Err(NfoError::AlreadyUndone)
    ));

    // Replace in place, then undo.
    let report = lab
        .service
        .rewrite(&only(id), &NfoSettings::default())
        .unwrap();
    assert!(read(&old).contains("<tmdbid>949</tmdbid>"));
    lab.service.undo(report.token.as_deref().unwrap()).unwrap();
    assert_eq!(read(&old), old_text);
}

/// Every file below `dir`, sorted.
fn files_below(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                out.extend(files_below(&path));
            } else {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// A hand-written NFO (or one another tool wrote) that a rewrite replaces, or that a change of
/// the NFO names moves away, is kept in the trash folder of the data source, next to the media
/// where the user can find it, besides the undo record in the app's own folder. Once a file is
/// CineVault's, later rewrites add nothing there.
#[test]
fn an_nfo_cinevault_did_not_write_is_kept_in_the_trash_folder_when_replaced() {
    let lab = Lab::new();
    let (id, folder) = lab.movie("Heat", 1995);
    let source = lab.root.path().join("media").join("Movies");
    let mut item = lab.item(id);
    item.core.data_source = source.to_string_lossy().into_owned();
    lab.save(item);
    let nfo = folder.join("Heat (1995).nfo");
    let hand = "<?xml version=\"1.0\"?>\n<movie>\n  <title>Heat</title>\n  \
                <plot>My own words about it.</plot>\n</movie>\n";
    fs::write(&nfo, hand).unwrap();
    let trash = source.join(cv_rename::exec::TRASH_DIR);

    let report = lab
        .service
        .rewrite(&only(id), &NfoSettings::default())
        .unwrap();
    assert_eq!(report.units[0].files[0].outcome, FileOutcome::Updated);
    let kept = files_below(&trash);
    assert_eq!(kept.len(), 1, "{kept:?}");
    let relative = kept[0].to_string_lossy().replace('\\', "/");
    assert!(
        relative.ends_with("/Heat (1995)/Heat (1995).nfo"),
        "{relative}"
    );
    assert_eq!(read(&kept[0]), hand, "the original bytes are kept");

    // The file is CineVault's now: a later rewrite keeps no further copy.
    let mut item = lab.item(id);
    item.core.plot = "Changed in the app.".into();
    lab.save(item);
    let again = lab
        .service
        .rewrite(&only(id), &NfoSettings::default())
        .unwrap();
    assert_eq!(again.units[0].files[0].outcome, FileOutcome::Updated);
    assert_eq!(files_below(&trash).len(), 1);

    // A hand-written file under a name the naming set no longer writes is kept there too.
    let other = folder.join("movie.nfo");
    fs::write(&other, hand.replace("My own", "Other")).unwrap();
    fs::remove_file(&nfo).unwrap();
    lab.service
        .rewrite(&only(id), &NfoSettings::default())
        .unwrap();
    assert!(!other.exists(), "the stale movie.nfo is cleaned up");
    let kept = files_below(&trash);
    assert_eq!(kept.len(), 2, "{kept:?}");
    assert!(
        kept.iter()
            .any(|p| read(p) == hand.replace("My own", "Other"))
    );
}

#[test]
fn undo_leaves_a_file_alone_that_was_edited_afterwards() {
    let lab = Lab::new();
    let (id, folder) = lab.movie("Heat", 1995);
    let nfo = folder.join("Heat (1995).nfo");
    let report = lab
        .service
        .rewrite(&only(id), &NfoSettings::default())
        .unwrap();
    fs::write(&nfo, "<movie><title>My own edit</title></movie>").unwrap();
    let summary = lab.service.undo(report.token.as_deref().unwrap()).unwrap();
    assert_eq!(summary.restored, 0);
    assert_eq!(summary.skipped.len(), 1);
    assert!(read(&nfo).contains("My own edit"));
}

#[test]
fn an_unknown_or_malformed_token_is_refused() {
    let lab = Lab::new();
    for token in ["nfo-0-0", "../etc", "", "NFO-1"] {
        assert!(matches!(
            lab.service.undo(token),
            Err(NfoError::UnknownToken(_))
        ));
    }
}

// ---- Read NFO ----------------------------------------------------------------------------------

#[test]
fn read_nfo_takes_edits_from_the_file_and_undo_restores_the_library() {
    let lab = Lab::new();
    let (id, folder) = lab.movie("Heat", 1995);
    let nfo = folder.join("Heat (1995).nfo");
    lab.service
        .rewrite(&only(id), &NfoSettings::default())
        .unwrap();

    // Edit the file in a "text editor".
    let edited = read(&nfo)
        .replace(
            "<title>Heat</title>",
            "<title>Heat (Director's Cut)</title>",
        )
        .replace(
            "A crew of thieves and the cop who hunts them.",
            "Edited plot.",
        );
    fs::write(&nfo, edited).unwrap();

    let report = lab
        .service
        .read(&only(id), &NfoSettings::default())
        .unwrap();
    assert_eq!(report.units[0].status, UnitStatus::Changed);
    let item = lab.item(id);
    assert_eq!(item.core.title, "Heat (Director's Cut)");
    assert_eq!(item.core.plot, "Edited plot.");
    assert_eq!(item.core.date_added_ms, 1_700_000_000_000);
    assert_eq!(item.core.path, folder.to_string_lossy());

    let summary = lab.service.undo(report.token.as_deref().unwrap()).unwrap();
    assert_eq!(summary.restored, 1);
    let item = lab.item(id);
    assert_eq!(item.core.title, "Heat");
    assert_eq!(
        item.core.plot,
        "A crew of thieves and the cop who hunts them."
    );
}

#[test]
fn reading_an_unchanged_file_changes_nothing() {
    let lab = Lab::new();
    let (id, _) = lab.movie("Heat", 1995);
    lab.service
        .rewrite(&only(id), &NfoSettings::default())
        .unwrap();
    let before = lab.item(id);
    let report = lab
        .service
        .read(&only(id), &NfoSettings::default())
        .unwrap();
    assert_eq!(report.units[0].status, UnitStatus::Unchanged);
    assert_eq!(report.token, None);
    assert_eq!(lab.item(id), before);
}

#[test]
fn read_nfo_never_takes_values_from_a_foreign_file() {
    let lab = Lab::new();
    let (id, folder) = lab.movie("Heat", 1995);
    // The only NFO at a name of the movie is a release note.
    fs::write(folder.join("Heat (1995).nfo"), RELEASE_NOTE).unwrap();
    let before = lab.item(id);
    let report = lab
        .service
        .read(&only(id), &NfoSettings::default())
        .unwrap();
    assert_eq!(
        report.units[0].status,
        UnitStatus::Skipped {
            reason: SkipReason::NotManaged
        }
    );
    assert_eq!(lab.item(id), before);

    // No NFO at all.
    let (other, _) = lab.movie("Ronin", 1998);
    let report = lab
        .service
        .read(&only(other), &NfoSettings::default())
        .unwrap();
    assert_eq!(
        report.units[0].status,
        UnitStatus::Skipped {
            reason: SkipReason::NoNfo
        }
    );
}

#[test]
fn undo_of_a_read_skips_an_item_edited_since() {
    let lab = Lab::new();
    let (id, folder) = lab.movie("Heat", 1995);
    fs::write(
        folder.join("Heat (1995).nfo"),
        "<movie><title>From file</title></movie>",
    )
    .unwrap();
    let report = lab
        .service
        .read(&only(id), &NfoSettings::default())
        .unwrap();
    assert_eq!(lab.item(id).core.title, "From file");

    let mut edited = lab.item(id);
    edited.core.note = "edited in the app".into();
    lab.save(edited);

    let summary = lab.service.undo(report.token.as_deref().unwrap()).unwrap();
    assert_eq!(summary.restored, 0);
    assert_eq!(summary.skipped.len(), 1);
    assert_eq!(lab.item(id).core.title, "From file");
    assert_eq!(lab.item(id).core.note, "edited in the app");
}

// ---- TV ----------------------------------------------------------------------------------------

struct Show {
    id: ItemId,
    folder: PathBuf,
    season_id: ItemId,
    episodes: Vec<ItemId>,
}

fn tv(lab: &Lab) -> Show {
    let folder = lab.dir("TV/Severance");
    let season_folder = folder.join("Season 1");
    fs::create_dir_all(&season_folder).unwrap();
    let mut show = Item::show("Severance");
    show.core.path = folder.to_string_lossy().into_owned();
    show.core.ids.set("tvdb", "371980");
    show.core.date_added_ms = 1_700_000_000_000;
    let id = lab.insert(show);

    let mut season = Item::season(id, 1);
    season.core.path = season_folder.to_string_lossy().into_owned();
    let season_id = lab.insert(season);

    let mut episodes = Vec::new();
    // Episodes 1 and 2 share one file; episode 3 has its own.
    for (number, file, title) in [
        (1, "Severance - S01E01-E02.mkv", "Good News About Hell"),
        (2, "Severance - S01E01-E02.mkv", "Half Loop"),
        (3, "Severance - S01E03.mkv", "In Perpetuity"),
    ] {
        fs::write(season_folder.join(file), b"video").unwrap();
        let mut episode = Item::episode(id, title);
        episode.core.plot = format!("Plot of {title}.");
        episode.core.media_files.push(MediaFile::new(
            MediaFileType::Video,
            season_folder.to_string_lossy(),
            file,
        ));
        if let ItemDetail::Episode(e) = &mut episode.detail {
            e.numbers.set(EpisodeNumber::aired(1, number));
            e.first_aired = CalendarDate::new(2022, 2, 18);
            e.multi_episode = number < 3;
        }
        episodes.push(lab.insert(episode));
    }
    Show {
        id,
        folder,
        season_id,
        episodes,
    }
}

#[test]
fn a_show_writes_tvshow_season_and_episode_files() {
    let lab = Lab::new();
    let show = tv(&lab);
    let report = lab
        .service
        .rewrite(&only(show.id), &NfoSettings::default())
        .unwrap();
    assert_eq!(report.totals.failed, 0, "{:?}", report.units);

    assert!(read(&show.folder.join("tvshow.nfo")).contains("<title>Severance</title>"));
    assert!(read(&show.folder.join("season01.nfo")).contains("<seasonnumber>1</seasonnumber>"));
    let multi = read(&show.folder.join("Season 1/Severance - S01E01-E02.nfo"));
    assert_eq!(multi.matches("<episodedetails>").count(), 2);
    assert_eq!(multi.matches("<?xml").count(), 1);
    let single = read(&show.folder.join("Season 1/Severance - S01E03.nfo"));
    assert!(single.contains("<title>In Perpetuity</title>"));

    // Season NFOs inside the season folder instead.
    let in_folder = settings(&[("metadata.nfo-season-names", json!("season-folder"))]);
    lab.service.rewrite(&only(show.id), &in_folder).unwrap();
    assert!(show.folder.join("Season 1/season.nfo").exists());
    assert!(
        !show.folder.join("season01.nfo").exists(),
        "the managed season file of the old name moved away"
    );
}

/// Marks the show of `tv` as ended and gives it a missing last episode and a special, so the
/// end date has to come from the whole known episode list.
fn end_show(lab: &Lab, show: &Show) {
    let mut item = lab.item(show.id);
    let ItemDetail::Show(fields) = &mut item.detail else {
        panic!("a show");
    };
    fields.status = AiredStatus::Ended;
    let mut missing = |season: u32, episode: u32, aired: Option<CalendarDate>| {
        let mut numbers = EpisodeNumbers::default();
        numbers.set(EpisodeNumber::aired(season, episode));
        fields.missing_episodes.push(MissingEpisode {
            title: format!("S{season}E{episode}"),
            numbers,
            first_aired: aired,
            plot: String::new(),
        });
    };
    missing(1, 9, CalendarDate::new(2022, 4, 15));
    missing(0, 1, CalendarDate::new(2023, 1, 1));
    lab.save(item);
}

#[test]
fn an_ended_show_writes_the_end_date_for_emby_and_jellyfin() {
    let lab = Lab::new();
    let show = tv(&lab);
    end_show(&lab, &show);
    let nfo = || read(&show.folder.join("tvshow.nfo"));

    // Emby counts the specials, Jellyfin does not.
    lab.service
        .rewrite(
            &only(show.id),
            &settings(&[("metadata.nfo-format", json!("emby"))]),
        )
        .unwrap();
    assert!(nfo().contains("<status>Ended</status>"));
    assert!(nfo().contains("<enddate>2023-01-01</enddate>"), "{}", nfo());

    lab.service
        .rewrite(
            &only(show.id),
            &settings(&[("metadata.nfo-format", json!("jellyfin"))]),
        )
        .unwrap();
    assert!(nfo().contains("<enddate>2022-04-15</enddate>"), "{}", nfo());

    // Kodi has no end date, and the setting can switch it off.
    lab.service
        .rewrite(
            &only(show.id),
            &settings(&[("metadata.nfo-format", json!("kodi"))]),
        )
        .unwrap();
    assert!(!nfo().contains("enddate"));
    lab.service
        .rewrite(
            &only(show.id),
            &settings(&[
                ("metadata.nfo-format", json!("emby")),
                ("metadata.nfo-end-date", json!(false)),
            ]),
        )
        .unwrap();
    assert!(!nfo().contains("enddate"));
}

#[test]
fn selecting_one_episode_writes_its_file_only() {
    let lab = Lab::new();
    let show = tv(&lab);
    lab.service
        .rewrite(&only(show.episodes[2]), &NfoSettings::default())
        .unwrap();
    assert!(show.folder.join("Season 1/Severance - S01E03.nfo").exists());
    assert!(!show.folder.join("tvshow.nfo").exists());
    assert!(
        !show
            .folder
            .join("Season 1/Severance - S01E01-E02.nfo")
            .exists()
    );

    // An episode in a shared file brings the whole file.
    lab.service
        .rewrite(&only(show.episodes[0]), &NfoSettings::default())
        .unwrap();
    let multi = read(&show.folder.join("Season 1/Severance - S01E01-E02.nfo"));
    assert_eq!(multi.matches("<episodedetails>").count(), 2);

    // A season brings its NFO and its episodes.
    lab.service
        .rewrite(&only(show.season_id), &NfoSettings::default())
        .unwrap();
    assert!(show.folder.join("season01.nfo").exists());
    assert!(!show.folder.join("tvshow.nfo").exists());
}

#[test]
fn read_nfo_updates_shows_seasons_and_episodes() {
    let lab = Lab::new();
    let show = tv(&lab);
    lab.service
        .rewrite(&only(show.id), &NfoSettings::default())
        .unwrap();

    let multi_path = show.folder.join("Season 1/Severance - S01E01-E02.nfo");
    let edited = read(&multi_path).replace("Half Loop", "Half Loop (edited)");
    fs::write(&multi_path, edited).unwrap();
    let tvshow = show.folder.join("tvshow.nfo");
    fs::write(
        &tvshow,
        read(&tvshow).replace("<title>Severance</title>", "<title>Severance!</title>"),
    )
    .unwrap();

    let report = lab
        .service
        .read(&only(show.id), &NfoSettings::default())
        .unwrap();
    assert_eq!(report.totals.failed, 0, "{:?}", report.units);
    assert_eq!(lab.item(show.id).core.title, "Severance!");
    assert_eq!(lab.item(show.episodes[1]).core.title, "Half Loop (edited)");
    assert_eq!(
        lab.item(show.episodes[0]).core.title,
        "Good News About Hell"
    );

    let summary = lab.service.undo(report.token.as_deref().unwrap()).unwrap();
    assert_eq!(summary.restored, 2);
    assert_eq!(lab.item(show.id).core.title, "Severance");
    assert_eq!(lab.item(show.episodes[1]).core.title, "Half Loop");
}

#[test]
fn a_foreign_file_in_a_show_folder_is_left_alone() {
    let lab = Lab::new();
    let show = tv(&lab);
    let info = show.folder.join("info.nfo");
    fs::write(&info, RELEASE_NOTE).unwrap();
    // A foreign tvshow.nfo is not overwritten either.
    let tvshow = show.folder.join("tvshow.nfo");
    fs::write(&tvshow, "Some notes about the show, not XML.").unwrap();
    let report = lab
        .service
        .rewrite(&only(show.id), &NfoSettings::default())
        .unwrap();
    assert_eq!(read(&info), RELEASE_NOTE);
    assert_eq!(read(&tvshow), "Some notes about the show, not XML.");
    assert_eq!(report.totals.kept_foreign, 1);
    assert!(show.folder.join("season01.nfo").exists());
}

#[test]
fn a_movie_in_a_set_carries_the_set_into_its_nfo() {
    let lab = Lab::new();
    let mut set = Item::set("Heat Collection");
    set.core.ids.set("tmdb", "8091");
    let set_id = lab.insert(set);
    let (id, folder) = lab.movie("Heat", 1995);
    let mut movie = lab.item(id);
    movie.parent_id = Some(set_id);
    lab.save(movie);

    let nfo = folder.join("Heat (1995).nfo");
    lab.service
        .rewrite(&only(id), &NfoSettings::default())
        .unwrap();
    assert!(
        read(&nfo).contains("<name>Heat Collection</name>"),
        "{}",
        read(&nfo)
    );

    let jellyfin = settings(&[("metadata.nfo-format", json!("jellyfin"))]);
    lab.service.rewrite(&only(id), &jellyfin).unwrap();
    assert!(read(&nfo).contains("<collectionnumber>8091</collectionnumber>"));
}

#[test]
fn importing_local_nfos_creates_and_joins_the_collections_they_name() {
    let lab = Lab::new();
    let nfo = |title: &str, year: u16, set: &str| {
        format!(
            "<movie><title>{title}</title><year>{year}</year>             <set><name>{set}</name><overview>The films.</overview></set>             <uniqueid type=\"tmdbSet\">8091</uniqueid></movie>"
        )
    };
    let (alien, alien_folder) = lab.movie("Alien", 1979);
    fs::write(
        alien_folder.join("Alien (1979).nfo"),
        nfo("Alien", 1979, "Alien Collection"),
    )
    .unwrap();
    let (aliens, aliens_folder) = lab.movie("Aliens", 1986);
    fs::write(
        aliens_folder.join("Aliens (1986).nfo"),
        nfo("Aliens", 1986, "Alien Collection"),
    )
    .unwrap();
    let (psycho, psycho_folder) = lab.movie("Psycho", 1960);
    fs::write(
        psycho_folder.join("Psycho (1960).nfo"),
        "<movie><title>Psycho</title><set>Psycho Collection</set></movie>",
    )
    .unwrap();
    let (locked, locked_folder) = lab.movie("Alien 3", 1992);
    fs::write(
        locked_folder.join("Alien 3 (1992).nfo"),
        nfo("Alien 3", 1992, "Alien Collection"),
    )
    .unwrap();
    lab.lock_field(locked, MetaField::Collection);

    lab.service
        .import_local(&[alien, aliens, psycho, locked], &NfoSettings::default())
        .unwrap();

    let set = lab
        .item(alien)
        .parent_id
        .expect("Alien joined a collection");
    assert_eq!(lab.item(aliens).parent_id, Some(set));
    let collection = lab.item(set);
    assert_eq!(collection.core.title, "Alien Collection");
    assert_eq!(collection.core.plot, "The films.");
    assert_eq!(collection.core.ids.get("tmdb"), Some("8091"));
    let psycho_set = lab
        .item(psycho)
        .parent_id
        .expect("Psycho joined a collection");
    assert_eq!(lab.item(psycho_set).core.title, "Psycho Collection");
    assert_eq!(lab.item(locked).parent_id, None);

    // The NFO the app writes keeps the set.
    lab.service
        .rewrite(&only(alien), &NfoSettings::default())
        .unwrap();
    let written = read(&alien_folder.join("Alien (1979).nfo"));
    assert!(
        written.contains("<name>Alien Collection</name>"),
        "{written}"
    );
}

// ---- Locks -------------------------------------------------------------------------------------

impl Lab {
    fn lock_field(&self, id: ItemId, field: MetaField) {
        self.db
            .write(move |tx| set_field_lock(tx, id, field, true))
            .unwrap();
    }

    fn lock_item(&self, id: ItemId) {
        self.db
            .write(move |tx| set_item_locked(tx, &[id], true))
            .unwrap();
    }
}

#[test]
fn locked_fields_are_written_to_the_movie_and_show_nfo() {
    let lab = Lab::new();
    let (id, folder) = lab.movie("Heat", 1995);
    lab.lock_field(id, MetaField::Genres);
    lab.lock_field(id, MetaField::Plot);
    lab.service
        .rewrite(&only(id), &NfoSettings::default())
        .unwrap();
    let text = read(&folder.join("Heat (1995).nfo"));
    assert!(
        text.contains("<cv_locked>plot,genres</cv_locked>"),
        "{text}"
    );

    // The element is read back by the tolerant reader.
    let back = cv_nfo::read_movie_nfo(&text).unwrap();
    assert_eq!(back.locked_fields, ["plot", "genres"]);

    let show = tv(&lab);
    lab.lock_field(show.id, MetaField::Title);
    lab.service
        .rewrite(&only(show.id), &NfoSettings::default())
        .unwrap();
    assert!(read(&show.folder.join("tvshow.nfo")).contains("<cv_locked>title</cv_locked>"));
}

#[test]
fn read_nfo_does_not_change_a_locked_field() {
    let lab = Lab::new();
    let (id, folder) = lab.movie("Heat", 1995);
    let nfo = folder.join("Heat (1995).nfo");
    lab.service
        .rewrite(&only(id), &NfoSettings::default())
        .unwrap();
    fs::write(
        &nfo,
        read(&nfo)
            .replace(
                "A crew of thieves and the cop who hunts them.",
                "Edited plot.",
            )
            .replace("<title>Heat</title>", "<title>Heat 2</title>"),
    )
    .unwrap();

    lab.lock_field(id, MetaField::Plot);
    let report = lab
        .service
        .read(&only(id), &NfoSettings::default())
        .unwrap();
    assert_eq!(report.units[0].status, UnitStatus::Changed);
    let item = lab.item(id);
    assert_eq!(
        item.core.title, "Heat 2",
        "unlocked fields still follow the file"
    );
    assert_eq!(
        item.core.plot, "A crew of thieves and the cop who hunts them.",
        "the locked plot stays"
    );

    // With the title locked too, nothing changes at all.
    lab.lock_field(id, MetaField::Title);
    lab.save({
        let mut item = lab.item(id);
        item.core.title = "Heat".into();
        item
    });
    let report = lab
        .service
        .read(&only(id), &NfoSettings::default())
        .unwrap();
    assert_eq!(report.units[0].status, UnitStatus::Unchanged);
    assert_eq!(lab.item(id).core.title, "Heat");
}

#[test]
fn read_nfo_leaves_a_locked_item_alone() {
    let lab = Lab::new();
    let (id, folder) = lab.movie("Heat", 1995);
    let nfo = folder.join("Heat (1995).nfo");
    lab.service
        .rewrite(&only(id), &NfoSettings::default())
        .unwrap();
    fs::write(
        &nfo,
        read(&nfo).replace("<title>Heat</title>", "<title>Elsewhere</title>"),
    )
    .unwrap();
    lab.lock_item(id);
    let before = lab.item(id);
    let report = lab
        .service
        .read(&only(id), &NfoSettings::default())
        .unwrap();
    assert_eq!(report.units[0].status, UnitStatus::Unchanged);
    assert_eq!(lab.item(id), before);
}

#[test]
fn read_nfo_honours_the_locks_of_shows_and_episodes() {
    let lab = Lab::new();
    let show = tv(&lab);
    lab.service
        .rewrite(&only(show.id), &NfoSettings::default())
        .unwrap();
    let tvshow = show.folder.join("tvshow.nfo");
    fs::write(
        &tvshow,
        read(&tvshow).replace("<title>Severance</title>", "<title>Severance!</title>"),
    )
    .unwrap();
    let episode_path = show.folder.join("Season 1/Severance - S01E03.nfo");
    fs::write(
        &episode_path,
        read(&episode_path).replace("In Perpetuity", "In Perpetuity!"),
    )
    .unwrap();

    lab.lock_field(show.id, MetaField::Title);
    lab.lock_item(show.episodes[2]);
    lab.service
        .read(&only(show.id), &NfoSettings::default())
        .unwrap();
    assert_eq!(lab.item(show.id).core.title, "Severance");
    assert_eq!(lab.item(show.episodes[2]).core.title, "In Perpetuity");
}
#[test]
fn a_preview_renders_what_a_rewrite_writes_and_tracks_the_file_on_disk() {
    use super::preview::NfoSync;

    let lab = Lab::new();
    let (id, folder) = lab.movie("Heat", 1995);
    let nfo = folder.join("Heat (1995).nfo");
    let defaults = NfoSettings::default();

    // Nothing is written by a preview.
    let preview = lab.service.preview_movie(id, &defaults).unwrap().unwrap();
    assert_eq!(preview.sync, NfoSync::Missing);
    assert_eq!(preview.format, "Kodi");
    assert_eq!(preview.path.as_deref(), Some(nfo.as_path()));
    assert!(
        preview.text.contains("<title>Heat</title>"),
        "{}",
        preview.text
    );
    assert!(!preview.text.contains('\r'));
    assert!(!nfo.exists());
    // There is no file yet, so there is nothing to compare with.
    assert_eq!(preview.file_text, None);

    // After a rewrite the file and the preview say the same.
    lab.service.rewrite(&only(id), &defaults).unwrap();
    let written = lab.service.preview_movie(id, &defaults).unwrap().unwrap();
    assert_eq!(written.sync, NfoSync::InSync);
    assert_eq!(
        no_comments(&read(&nfo)).replace("\r\n", "\n"),
        no_comments(&written.text)
    );
    // The preview carries what the file says, with LF line ends, for the page's diff.
    let on_disk = written.file_text.as_deref().unwrap();
    assert!(!on_disk.contains('\r'));
    assert_eq!(no_comments(on_disk), no_comments(&written.text));

    // An edit in the library makes the file differ until the next rewrite.
    let mut item = lab.item(id);
    item.core.plot = "Changed in the library.".into();
    lab.save(item);
    let stale = lab.service.preview_movie(id, &defaults).unwrap().unwrap();
    assert_eq!(stale.sync, NfoSync::Differs);
    assert!(stale.text.contains("Changed in the library."));
    // The file still holds the older plot: that is the difference the page shows.
    let stale_file = stale.file_text.as_deref().unwrap();
    assert!(!stale_file.contains("Changed in the library."));
    assert_eq!(no_comments(stale_file), no_comments(on_disk));

    // The settings choose the format.
    let jellyfin = settings(&[("metadata.nfo-format", json!("jellyfin"))]);
    let other = lab.service.preview_movie(id, &jellyfin).unwrap().unwrap();
    assert_eq!(other.format, "Jellyfin");
}

#[test]
fn a_preview_carries_the_field_locks_so_a_locked_movie_stays_in_sync() {
    use super::preview::NfoSync;

    let lab = Lab::new();
    let (id, _) = lab.movie("Heat", 1995);
    lab.lock_field(id, MetaField::Genres);
    let defaults = NfoSettings::default();
    lab.service.rewrite(&only(id), &defaults).unwrap();

    let preview = lab.service.preview_movie(id, &defaults).unwrap().unwrap();
    assert!(
        preview.text.contains("<cv_locked>genres</cv_locked>"),
        "{}",
        preview.text
    );
    assert_eq!(preview.sync, NfoSync::InSync);
}

#[test]
fn a_preview_leaves_a_foreign_file_alone_and_says_so() {
    use super::preview::NfoSync;

    let lab = Lab::new();
    let (id, folder) = lab.movie("Heat", 1995);
    let nfo = folder.join("Heat (1995).nfo");
    fs::write(&nfo, "Some notes, not XML.").unwrap();

    let preview = lab
        .service
        .preview_movie(id, &NfoSettings::default())
        .unwrap()
        .unwrap();
    assert_eq!(preview.sync, NfoSync::NotManaged);
    assert_eq!(read(&nfo), "Some notes, not XML.");

    // Only movies have a preview; an unknown id has none.
    let set = lab.insert(Item::set("Collection"));
    assert!(
        lab.service
            .preview_movie(set, &NfoSettings::default())
            .unwrap()
            .is_none()
    );
    assert!(
        lab.service
            .preview_movie(ItemId(9999), &NfoSettings::default())
            .unwrap()
            .is_none()
    );
}

#[test]
fn a_show_preview_renders_the_tvshow_file_and_tracks_it_on_disk() {
    use super::preview::NfoSync;

    let lab = Lab::new();
    let show = tv(&lab);
    let nfo = show.folder.join("tvshow.nfo");
    let defaults = NfoSettings::default();

    let preview = lab
        .service
        .preview_show(show.id, &defaults)
        .unwrap()
        .unwrap();
    assert_eq!(preview.sync, NfoSync::Missing);
    assert_eq!(preview.format, "Kodi");
    assert_eq!(preview.path.as_deref(), Some(nfo.as_path()));
    assert!(preview.text.contains("<tvshow>"), "{}", preview.text);
    assert!(preview.text.contains("<title>Severance</title>"));
    assert!(!preview.text.contains('\r'));
    assert!(!nfo.exists(), "a preview writes nothing");

    lab.service.rewrite(&only(show.id), &defaults).unwrap();
    let written = lab
        .service
        .preview_show(show.id, &defaults)
        .unwrap()
        .unwrap();
    assert_eq!(written.sync, NfoSync::InSync);

    let jellyfin = settings(&[("metadata.nfo-format", json!("jellyfin"))]);
    let other = lab
        .service
        .preview_show(show.id, &jellyfin)
        .unwrap()
        .unwrap();
    assert_eq!(other.format, "Jellyfin");

    // Only shows have a show preview.
    assert!(
        lab.service
            .preview_show(show.episodes[0], &defaults)
            .unwrap()
            .is_none()
    );
    assert!(
        lab.service
            .preview_show(ItemId(9999), &defaults)
            .unwrap()
            .is_none()
    );
}

#[test]
fn an_episode_preview_holds_every_episode_of_its_video_file() {
    use super::preview::NfoSync;

    let lab = Lab::new();
    let show = tv(&lab);
    let defaults = NfoSettings::default();

    // Episodes 1 and 2 share one file, so one document holds both.
    let double = lab
        .service
        .preview_episode(show.episodes[1], &defaults)
        .unwrap()
        .unwrap();
    assert_eq!(double.text.matches("<episodedetails>").count(), 2);
    assert!(double.text.contains("<title>Good News About Hell</title>"));
    assert!(double.text.contains("<title>Half Loop</title>"));
    assert_eq!(double.sync, NfoSync::Missing);
    assert!(
        double
            .path
            .as_deref()
            .is_some_and(|p| p.ends_with("Severance - S01E01-E02.nfo"))
    );

    let single = lab
        .service
        .preview_episode(show.episodes[2], &defaults)
        .unwrap()
        .unwrap();
    assert_eq!(single.text.matches("<episodedetails>").count(), 1);
    assert!(single.text.contains("<title>In Perpetuity</title>"));

    lab.service
        .rewrite(&only(show.episodes[2]), &defaults)
        .unwrap();
    let written = lab
        .service
        .preview_episode(show.episodes[2], &defaults)
        .unwrap()
        .unwrap();
    assert_eq!(written.sync, NfoSync::InSync);

    // A show or an unknown id has no episode preview.
    for id in [show.id, ItemId(9999)] {
        assert!(
            lab.service
                .preview_episode(id, &defaults)
                .unwrap()
                .is_none()
        );
    }
}
