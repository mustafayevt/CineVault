//! History over a real library file and real (temporary) folders. The system bin is a folder.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use cv_db::Database;
use cv_db::repo::data_sources::insert_source;
use cv_db::repo::{get_item, insert_item};
use cv_domain::{DataSource, Item, ItemId, MediaFile, MediaFileType, SourceKind};
use cv_rename::{Preset, RenameProfile};

use super::*;
use crate::library_ops::ChangeSourceService;
use crate::library_ops::trash_bin::fake::FakeBin;
use crate::organize::{ApplyRequest, OrganizeService, PreviewRequest, Scope};

/// A library with two movie sources, `movies` and `archive`, and a bin in a folder.
pub(crate) struct Lab {
    pub(crate) dir: tempfile::TempDir,
    pub(crate) db: Arc<Database>,
    pub(crate) movies: DataSource,
    pub(crate) archive: DataSource,
    pub(crate) bin: Arc<FakeBin>,
}

#[allow(dead_code)]
impl Lab {
    pub(crate) fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(Database::open(dir.path().join("library.db")).unwrap());
        let make = |name: &str| {
            let path = dir.path().join(name);
            fs::create_dir_all(&path).unwrap();
            let text = path.to_string_lossy().into_owned();
            let mut source = db
                .write(move |tx| insert_source(tx, SourceKind::Movie, &text))
                .unwrap();
            // The source lock is process-wide by id: ids of their own keep tests apart.
            let (old, new) = (source.id, crate::library_ops::tests::next_source_id());
            db.write(move |tx| {
                tx.execute("UPDATE data_source SET id = ?1 WHERE id = ?2", [new, old])?;
                Ok(())
            })
            .unwrap();
            source.id = new;
            source
        };
        let movies = make("movies");
        let archive = make("archive");
        let bin = FakeBin::new(dir.path());
        Self {
            dir,
            db,
            movies,
            archive,
            bin,
        }
    }

    pub(crate) fn history(&self) -> HistoryService {
        HistoryService::new(Arc::clone(&self.db)).with_bin(Arc::clone(&self.bin) as _)
    }

    /// A scraped movie `<movies>/<folder>` with a video, an NFO and a poster named `base`.
    pub(crate) fn movie(&self, folder: &str, base: &str, title: &str, year: u16) -> ItemId {
        let root = Path::new(&self.movies.path);
        let mut item = Item::movie(title);
        item.core.year = year;
        item.core
            .ids
            .set("tmdb", format!("{}", 1000 + u32::from(year)));
        item.core.last_scraper_id = "tmdb".to_owned();
        item.core.data_source.clone_from(&self.movies.path);
        item.core.path = root.join(folder).to_string_lossy().into_owned();
        for (name, kind) in [
            (format!("{base}.mkv"), MediaFileType::Video),
            (format!("{base}.nfo"), MediaFileType::Nfo),
            (format!("{base}-poster.jpg"), MediaFileType::Poster),
        ] {
            let path = root.join(folder).join(&name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, name.as_bytes()).unwrap();
            let mut file = MediaFile::new(kind, item.core.path.clone(), name);
            file.size_bytes = 10;
            item.core.media_files.push(file);
        }
        self.db.write(move |tx| insert_item(tx, &item)).unwrap()
    }

    pub(crate) fn item(&self, id: ItemId) -> Item {
        self.db.read(move |c| get_item(c, id)).unwrap().unwrap()
    }

    pub(crate) fn path(&self, rel: &str) -> PathBuf {
        Path::new(&self.movies.path).join(rel)
    }

    pub(crate) fn rename(&self, ids: &[ItemId]) -> String {
        let request = ApplyRequest {
            preview: PreviewRequest {
                scope: Scope::Everything,
                profile: RenameProfile::from_preset(Preset::Clean),
                choices: BTreeMap::new(),
            },
            include: ids.iter().map(|id| id.0.to_string()).collect(),
        };
        let applied = OrganizeService::new(Arc::clone(&self.db))
            .apply(&request)
            .unwrap();
        assert_eq!(applied.done(), ids.len(), "{applied:?}");
        applied.batch_id.unwrap()
    }

    pub(crate) fn move_to_archive(&self, ids: &[ItemId]) -> String {
        let applied = ChangeSourceService::new(Arc::clone(&self.db))
            .apply(ids, self.archive.id, &mut |_| {})
            .unwrap();
        assert_eq!(applied.moved(), ids.len(), "{applied:?}");
        applied.batch_id.unwrap()
    }
}

#[test]
fn a_rename_batch_writes_one_entry_with_its_titles() {
    let lab = Lab::new();
    let heat = lab.movie("heat.1995.1080p", "heat", "Heat", 1995);
    let ronin = lab.movie("ronin.1998", "ronin", "Ronin", 1998);
    let batch = lab.rename(&[heat, ronin]);

    let page = lab.history().list(HistoryFilter::All, 0, 50).unwrap();
    assert_eq!(page.total, 1);
    let entry = &page.entries[0];
    assert_eq!(entry.kind, HistoryKind::Rename);
    assert_eq!(entry.summary, "Renamed 2 titles");
    assert!(
        entry.detail.starts_with("2 folders, 6 files"),
        "{}",
        entry.detail
    );
    assert_eq!(entry.batch_ref.as_deref(), Some(batch.as_str()));
    assert_eq!(entry.undo_state, UndoState::Available);
    assert_eq!(entry.titles_total, 2);
    let heat_row = entry.preview.iter().find(|t| t.item_id == heat).unwrap();
    assert_eq!(heat_row.title, "Heat (1995)");
    assert_eq!(heat_row.before, "heat.1995.1080p");
    assert_eq!(heat_row.after, "Heat (1995)");
    assert_eq!(
        lab.history().entry_for_batch(&batch).unwrap(),
        Some(entry.id)
    );
}

#[test]
fn a_move_batch_writes_one_entry_with_old_and_new_folders() {
    let lab = Lab::new();
    let heat = lab.movie("Heat (1995)", "Heat (1995)", "Heat", 1995);
    let batch = lab.move_to_archive(&[heat]);

    let page = lab.history().list(HistoryFilter::Moves, 0, 50).unwrap();
    assert_eq!(page.entries.len(), 1);
    let entry = &page.entries[0];
    assert_eq!(entry.kind, HistoryKind::Move);
    assert!(
        entry.summary.starts_with("Moved Heat (1995)"),
        "{}",
        entry.summary
    );
    assert_eq!(entry.batch_ref.as_deref(), Some(batch.as_str()));
    assert!(entry.bytes.is_some_and(|b| b > 0), "{entry:?}");
    let title = &entry.preview[0];
    assert!(title.before.ends_with("Heat (1995)"));
    assert!(Path::new(&title.after).starts_with(&lab.archive.path));
}

#[test]
fn the_list_filters_by_tab_and_is_newest_first_across_a_reopen() {
    let lab = Lab::new();
    let heat = lab.movie("heat.1995", "heat", "Heat", 1995);
    lab.rename(&[heat]);
    let ronin = lab.movie("Ronin (1998)", "Ronin (1998)", "Ronin", 1998);
    lab.move_to_archive(&[ronin]);
    lab.history()
        .record(&NewEntry {
            kind: HistoryKind::Artwork,
            summary: "Changed poster · Heat (1995)".to_owned(),
            line: String::new(),
            bytes: None,
            extra: serde_json::json!({}),
            batch_ref: None,
            titles: vec![NewTitle {
                item_id: heat,
                title: "Heat (1995)".to_owned(),
                before: "poster.jpg".to_owned(),
                after: "poster.jpg".to_owned(),
            }],
        })
        .unwrap();

    // Reopen the library file: History is still there.
    let path = lab.dir.path().join("library.db");
    let reopened = HistoryService::new(Arc::new(Database::open(&path).unwrap()));
    let all = reopened.list(HistoryFilter::All, 0, 50).unwrap();
    let kinds: Vec<HistoryKind> = all.entries.iter().map(|e| e.kind).collect();
    assert_eq!(
        kinds,
        [HistoryKind::Artwork, HistoryKind::Move, HistoryKind::Rename]
    );
    for (filter, kind) in [
        (HistoryFilter::Renames, HistoryKind::Rename),
        (HistoryFilter::Moves, HistoryKind::Move),
        (HistoryFilter::ArtworkAndMatches, HistoryKind::Artwork),
    ] {
        let page = reopened.list(filter, 0, 50).unwrap();
        assert_eq!(page.total, 1, "{filter:?}");
        assert_eq!(page.entries[0].kind, kind);
    }
    assert_eq!(
        reopened.list(HistoryFilter::Deletes, 0, 50).unwrap().total,
        0
    );
    let paged = reopened.list(HistoryFilter::All, 1, 1).unwrap();
    assert_eq!(paged.entries[0].kind, HistoryKind::Move);
    assert_eq!(paged.total, 3);
    assert_eq!(reopened.for_title(heat, 10).unwrap().len(), 2);
}

#[test]
fn nothing_in_the_bin_totals_zero() {
    let lab = Lab::new();
    assert_eq!(lab.history().bin_total().unwrap(), 0);
}

// ---- delete to the bin and Restore ------------------------------------------------

impl Lab {
    pub(crate) fn removal(&self) -> crate::library_ops::RemovalService {
        crate::library_ops::RemovalService::new(Arc::clone(&self.db))
            .with_bin(Arc::clone(&self.bin) as _)
    }

    pub(crate) fn listed_movies(&self) -> u64 {
        let query = cv_db::repo::ItemQuery::new(cv_domain::ItemKind::Movie);
        self.db
            .read(move |c| cv_db::repo::count_items(c, &query))
            .unwrap()
    }

    pub(crate) fn hidden(&self, id: ItemId) -> bool {
        self.db
            .read(move |c| cv_db::repo::library_ops::is_hidden(c, id))
            .unwrap()
    }
}

#[test]
fn a_deleted_title_comes_back_from_the_bin_with_its_details_artwork_and_stage() {
    let lab = Lab::new();
    let heat = lab.movie("Heat (1995)", "Heat (1995)", "Heat", 1995);
    let ronin = lab.movie("Ronin (1998)", "Ronin (1998)", "Ronin", 1998);
    let mut detailed = lab.item(heat);
    detailed.core.plot = "A crew of thieves.".to_owned();
    lab.db
        .write(move |tx| cv_db::repo::update_item(tx, &detailed))
        .unwrap();
    let before = lab.item(heat);
    assert_eq!(lab.listed_movies(), 2);

    let report = lab.removal().delete_with_files(&[heat]).unwrap();
    let entry_id = report.history_id.unwrap();
    assert!(!lab.path("Heat (1995)").exists());
    assert!(lab.hidden(heat) && !lab.hidden(ronin));
    assert_eq!(lab.listed_movies(), 1, "a hidden title is in no list");
    let entry = lab.history().entry(entry_id).unwrap();
    assert_eq!(entry.kind, HistoryKind::Delete);
    assert_eq!(
        entry.summary,
        format!(
            "Deleted Heat (1995) to the {}",
            crate::library_ops::BIN_NAME
        )
    );
    assert_eq!(entry.bytes, lab.history().bin_total().ok());
    assert!(entry.bytes.is_some_and(|b| b > 0));

    let undone = lab.history().undo(entry_id).unwrap();
    assert_eq!(undone.undone, [heat], "{undone:?}");
    assert!(undone.left.is_empty());
    assert_eq!(undone.state, UndoState::Undone);
    assert!(undone.undone_at.is_some());
    assert!(lab.path("Heat (1995)/Heat (1995).mkv").is_file());
    assert!(lab.path("Heat (1995)/Heat (1995)-poster.jpg").is_file());
    assert!(!lab.hidden(heat));
    assert_eq!(lab.listed_movies(), 2);
    let after = lab.item(heat);
    assert_eq!(after.core.plot, before.core.plot);
    assert_eq!(after.core.media_files, before.core.media_files);
    assert_eq!(after.core.ids, before.core.ids);
    assert_eq!(after.core.last_scraper_id, before.core.last_scraper_id);
    assert_eq!(lab.history().bin_total().unwrap(), 0);
    let entry = lab.history().entry(entry_id).unwrap();
    assert_eq!(entry.undo_state, UndoState::Undone);
    assert!(lab.history().undo(entry_id).is_err(), "undone once only");
}

#[test]
fn an_emptied_bin_restores_nothing_and_the_entry_stays() {
    let lab = Lab::new();
    let heat = lab.movie("Heat (1995)", "Heat (1995)", "Heat", 1995);
    let entry_id = lab
        .removal()
        .delete_with_files(&[heat])
        .unwrap()
        .history_id
        .unwrap();
    lab.bin.empty();
    assert_eq!(lab.history().bin_total().unwrap(), 0);

    let report = lab.history().undo(entry_id).unwrap();
    assert!(report.undone.is_empty());
    assert_eq!(report.left.len(), 1);
    assert!(
        report.left[0].reason.contains(&format!(
            "no longer in the {}",
            crate::library_ops::BIN_NAME
        )),
        "{report:?}"
    );
    assert!(lab.hidden(heat));
    assert_eq!(report.state, UndoState::Available);
    assert_eq!(
        lab.history().entry(entry_id).unwrap().undo_state,
        UndoState::Available
    );
}

#[test]
fn a_restore_never_replaces_a_folder_now_at_the_old_path() {
    let lab = Lab::new();
    let heat = lab.movie("Heat (1995)", "Heat (1995)", "Heat", 1995);
    let entry_id = lab
        .removal()
        .delete_with_files(&[heat])
        .unwrap()
        .history_id
        .unwrap();
    fs::create_dir_all(lab.path("Heat (1995)")).unwrap();
    fs::write(lab.path("Heat (1995)/new.txt"), b"new").unwrap();

    let report = lab.history().undo(entry_id).unwrap();
    assert!(report.undone.is_empty());
    assert!(report.left[0].reason.contains("Nothing was replaced"));
    assert_eq!(
        fs::read(lab.path("Heat (1995)/new.txt")).unwrap(),
        b"new",
        "the new folder is untouched"
    );
    assert!(!lab.path("Heat (1995)/Heat (1995).mkv").exists());
    assert_eq!(lab.bin.names(), ["Heat (1995)"], "the bin is untouched");
    assert!(lab.hidden(heat));
}

#[test]
fn a_restore_to_an_offline_drive_is_refused() {
    let lab = Lab::new();
    let heat = lab.movie("Heat (1995)", "Heat (1995)", "Heat", 1995);
    let entry_id = lab
        .removal()
        .delete_with_files(&[heat])
        .unwrap()
        .history_id
        .unwrap();
    let unplugged = lab.dir.path().join("unplugged");
    fs::rename(&lab.movies.path, &unplugged).unwrap();

    let report = lab.history().undo(entry_id).unwrap();
    assert!(report.undone.is_empty());
    assert!(report.left[0].reason.contains("offline"), "{report:?}");
    assert_eq!(lab.bin.names(), ["Heat (1995)"]);
    fs::rename(&unplugged, &lab.movies.path).unwrap();
}

#[test]
fn deleting_two_titles_restores_each_on_its_own() {
    let lab = Lab::new();
    let heat = lab.movie("Heat (1995)", "Heat (1995)", "Heat", 1995);
    let ronin = lab.movie("Ronin (1998)", "Ronin (1998)", "Ronin", 1998);
    let entry_id = lab
        .removal()
        .delete_with_files(&[heat, ronin])
        .unwrap()
        .history_id
        .unwrap();
    assert_eq!(
        lab.history().entry(entry_id).unwrap().summary,
        format!("Deleted 2 titles to the {}", crate::library_ops::BIN_NAME)
    );
    // Ronin's old place is taken: Heat comes back, Ronin stays in the bin.
    fs::create_dir_all(lab.path("Ronin (1998)")).unwrap();
    let report = lab.history().undo(entry_id).unwrap();
    assert_eq!(report.undone, [heat]);
    assert_eq!(report.left.len(), 1);
    assert_eq!(report.state, UndoState::Partial);
    assert!(!lab.hidden(heat) && lab.hidden(ronin));
    let entry = lab.history().entry(entry_id).unwrap();
    assert_eq!((entry.titles_undone, entry.titles_total), (1, 2));
    // Once the place is free again, the rest comes back.
    fs::remove_dir(lab.path("Ronin (1998)")).unwrap();
    let report = lab.history().undo(entry_id).unwrap();
    assert_eq!(report.undone, [ronin]);
    assert_eq!(report.state, UndoState::Undone);
}

// ---- permanent delete ---------------------------------------------------------------

/// The real disk, except that one file name is "in use".
struct InUse(&'static str);

impl crate::library_ops::EraseFs for InUse {
    fn remove_file(&self, path: &Path) -> std::io::Result<()> {
        if path.file_name().is_some_and(|n| n == self.0) {
            let code = if cfg!(windows) { 32 } else { 16 };
            return Err(std::io::Error::from_raw_os_error(code));
        }
        crate::library_ops::StdErase.remove_file(path)
    }
    fn remove_dir(&self, path: &Path) -> std::io::Result<()> {
        crate::library_ops::StdErase.remove_dir(path)
    }
}

#[test]
fn a_permanent_delete_needs_the_typed_confirmation() {
    let lab = Lab::new();
    let heat = lab.movie("Heat (1995)", "Heat (1995)", "Heat", 1995);
    for typed in ["", "delete", "DELET", "DELETE ", "yes"] {
        let error = lab.removal().erase(&[heat], typed).unwrap_err();
        assert_eq!(error.code(), "confirmation");
    }
    assert!(lab.path("Heat (1995)/Heat (1995).mkv").is_file());
    assert_eq!(
        lab.history().list(HistoryFilter::All, 0, 10).unwrap().total,
        0
    );
}

#[test]
fn a_permanent_delete_erases_the_folder_and_is_listed_without_undo() {
    let lab = Lab::new();
    let heat = lab.movie("Heat (1995)", "Heat (1995)", "Heat", 1995);
    let ronin = lab.movie("Ronin (1998)", "Ronin (1998)", "Ronin", 1998);
    let report = lab.removal().erase(&[heat], "DELETE").unwrap();
    assert_eq!(report.erased.len(), 1, "{report:?}");
    assert!(!lab.path("Heat (1995)").exists());
    assert!(lab.path("Ronin (1998)/Ronin (1998).mkv").is_file());
    assert!(
        !lab.db
            .read(move |c| cv_db::repo::item_exists(c, heat))
            .unwrap(),
        "an erased title leaves the library"
    );
    assert!(
        lab.db
            .read(move |c| cv_db::repo::item_exists(c, ronin))
            .unwrap()
    );
    assert!(lab.bin.names().is_empty(), "nothing went to the bin");

    let page = lab.history().list(HistoryFilter::Deletes, 0, 10).unwrap();
    let entry = &page.entries[0];
    assert_eq!(entry.kind, HistoryKind::DeletePermanent);
    assert_eq!(entry.summary, "Permanently deleted Heat (1995)");
    assert_eq!(entry.undo_state, UndoState::None);
    assert!(entry.bytes.is_some_and(|b| b > 0));
    let refused = lab.history().undo(entry.id).unwrap_err();
    assert!(refused.to_string().contains("cannot be undone"));
}

#[test]
fn a_file_in_use_stops_only_its_own_title() {
    let lab = Lab::new();
    let heat = lab.movie("Heat (1995)", "Heat (1995)", "Heat", 1995);
    let ronin = lab.movie("Ronin (1998)", "Ronin (1998)", "Ronin", 1998);
    let report = lab
        .removal()
        .with_eraser(Arc::new(InUse("Ronin (1998).nfo")))
        .erase(&[heat, ronin], "DELETE")
        .unwrap();
    assert_eq!(report.erased.len(), 1);
    assert_eq!(report.erased[0].id, heat);
    assert_eq!(report.stopped.len(), 1);
    let stopped = &report.stopped[0];
    assert_eq!(stopped.id, ronin);
    assert!(
        stopped.error.as_deref().unwrap().contains("in use"),
        "{stopped:?}"
    );
    assert!(stopped.left.iter().any(|p| p.ends_with("Ronin (1998).nfo")));
    assert!(!lab.path("Heat (1995)").exists());
    assert!(lab.path("Ronin (1998)/Ronin (1998).nfo").is_file());
    assert!(
        lab.db
            .read(move |c| cv_db::repo::item_exists(c, ronin))
            .unwrap(),
        "the stopped title stays in the library"
    );
    let entry = lab.history().entry(report.history_id.unwrap()).unwrap();
    assert_eq!(entry.summary, "Permanently deleted 2 titles");
    assert!(entry.detail.contains("not erased"), "{}", entry.detail);
    let titles = lab.history().titles(entry.id).unwrap();
    let ronin_row = titles.iter().find(|t| t.item_id == ronin).unwrap();
    assert_eq!(ronin_row.state, TitleState::Left);
}

#[test]
fn a_title_in_a_library_folder_root_loses_only_its_files_and_never_the_root() {
    let lab = Lab::new();
    let root = PathBuf::from(&lab.movies.path);
    let mut item = Item::movie("Loose");
    item.core.data_source.clone_from(&lab.movies.path);
    item.core.path.clone_from(&lab.movies.path);
    fs::write(root.join("loose.mkv"), b"video").unwrap();
    fs::write(root.join("other.txt"), b"not the title's").unwrap();
    item.core.media_files.push(MediaFile::new(
        MediaFileType::Video,
        lab.movies.path.clone(),
        "loose.mkv",
    ));
    let loose = lab.db.write(move |tx| insert_item(tx, &item)).unwrap();

    let report = lab.removal().erase(&[loose], "DELETE").unwrap();
    assert_eq!(report.erased.len(), 1, "{report:?}");
    assert!(!root.join("loose.mkv").exists());
    assert!(root.is_dir(), "the library folder stays");
    assert!(root.join("other.txt").is_file());
}

#[test]
fn a_folder_that_holds_a_library_folder_or_lies_outside_one_is_refused() {
    let lab = Lab::new();
    let heat = lab.movie("Heat (1995)", "Heat (1995)", "Heat", 1995);
    let inner = lab.path("Heat (1995)/inner");
    fs::create_dir_all(&inner).unwrap();
    let text = inner.to_string_lossy().into_owned();
    lab.db
        .write(move |tx| insert_source(tx, SourceKind::Movie, &text))
        .unwrap();
    let report = lab.removal().erase(&[heat], "DELETE").unwrap();
    assert!(report.erased.is_empty());
    assert!(
        report.problems[0].reason.contains("library folder"),
        "{report:?}"
    );
    assert!(lab.path("Heat (1995)/Heat (1995).mkv").is_file());
    assert!(
        report.history_id.is_none(),
        "nothing planned, nothing recorded"
    );

    let outside = lab.dir.path().join("elsewhere");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("precious.mkv"), b"keep").unwrap();
    let mut stray = Item::movie("Stray");
    stray.core.data_source.clone_from(&lab.movies.path);
    stray.core.path = outside.to_string_lossy().into_owned();
    stray.core.media_files.push(MediaFile::new(
        MediaFileType::Video,
        stray.core.path.clone(),
        "precious.mkv",
    ));
    let stray = lab.db.write(move |tx| insert_item(tx, &stray)).unwrap();
    let report = lab.removal().erase(&[stray], "DELETE").unwrap();
    assert!(report.erased.is_empty());
    assert!(outside.join("precious.mkv").is_file());
}

// ---- Undo of renames and moves ----------------------------------------------------

#[test]
fn undo_of_a_rename_batch_puts_every_title_back_and_marks_the_entry() {
    let lab = Lab::new();
    let heat = lab.movie("heat.1995", "heat", "Heat", 1995);
    let ronin = lab.movie("ronin.1998", "ronin", "Ronin", 1998);
    let batch = lab.rename(&[heat, ronin]);
    let entry = lab.history().entry_for_batch(&batch).unwrap().unwrap();

    let report = lab.history().undo(entry).unwrap();
    assert_eq!(report.undone.len(), 2, "{report:?}");
    assert!(report.left.is_empty());
    assert_eq!(report.state, UndoState::Undone);
    assert!(lab.path("heat.1995/heat.mkv").is_file());
    assert!(lab.path("ronin.1998/ronin.mkv").is_file());
    assert!(lab.item(heat).core.path.ends_with("heat.1995"));
    let listed = lab.history().entry(entry).unwrap();
    assert_eq!(listed.undo_state, UndoState::Undone);
    assert!(listed.undone_at.is_some());
}

#[test]
fn undo_of_one_title_takes_back_only_its_part_of_the_batch() {
    let lab = Lab::new();
    let heat = lab.movie("heat.1995", "heat", "Heat", 1995);
    let ronin = lab.movie("ronin.1998", "ronin", "Ronin", 1998);
    let batch = lab.rename(&[heat, ronin]);
    let entry = lab.history().entry_for_batch(&batch).unwrap().unwrap();

    let report = lab.history().undo_title(entry, heat).unwrap();
    assert_eq!(report.undone, [heat]);
    assert_eq!(report.state, UndoState::Partial);
    assert!(lab.path("heat.1995/heat.mkv").is_file());
    assert!(
        lab.path("Ronin (1998)/Ronin (1998).mkv").is_file(),
        "the other title stays"
    );
    let listed = lab.history().entry(entry).unwrap();
    assert_eq!(
        (listed.titles_undone, listed.titles_total),
        (1, 2),
        "1 of 2 undone"
    );
    assert!(lab.history().undo_title(entry, heat).is_err(), "once only");
    // The page line of Heat: its newest entry, with its own titles only.
    let mine = lab.history().for_title(heat, 1).unwrap();
    assert_eq!(mine[0].preview.len(), 1);
    assert_eq!(mine[0].preview[0].state, TitleState::Undone);

    let report = lab.history().undo(entry).unwrap();
    assert_eq!(report.undone, [ronin]);
    assert_eq!(report.state, UndoState::Undone);
}

#[test]
fn undo_leaves_a_title_whose_folder_changed_since_and_takes_the_others_back() {
    let lab = Lab::new();
    let heat = lab.movie("heat.1995", "heat", "Heat", 1995);
    let ronin = lab.movie("ronin.1998", "ronin", "Ronin", 1998);
    let batch = lab.rename(&[heat, ronin]);
    let entry = lab.history().entry_for_batch(&batch).unwrap().unwrap();
    // Renamed by hand afterwards.
    fs::rename(lab.path("Ronin (1998)"), lab.path("Ronin by hand")).unwrap();

    let report = lab.history().undo(entry).unwrap();
    assert_eq!(report.undone, [heat], "{report:?}");
    assert_eq!(report.left.len(), 1);
    assert_eq!(report.left[0].item_id, ronin);
    assert!(!report.left[0].reason.is_empty());
    assert_eq!(report.state, UndoState::Partial);
    assert!(
        lab.path("Ronin by hand/Ronin (1998).mkv").is_file(),
        "left as it is"
    );
}

#[test]
fn undo_of_an_older_entry_names_the_newer_one_and_leaves_that_title() {
    let lab = Lab::new();
    let heat = lab.movie("heat.1995", "heat", "Heat", 1995);
    let move_batch = lab.move_to_archive(&[heat]);
    let moved = lab.history().entry_for_batch(&move_batch).unwrap().unwrap();
    // Renamed in its new place afterwards.
    let request = ApplyRequest {
        preview: PreviewRequest {
            scope: Scope::Everything,
            profile: RenameProfile::from_preset(Preset::Clean),
            choices: BTreeMap::new(),
        },
        include: vec![heat.0.to_string()],
    };
    let applied = OrganizeService::new(Arc::clone(&lab.db))
        .apply(&request)
        .unwrap();
    assert_eq!(applied.done(), 1, "{applied:?}");
    let renamed = lab
        .history()
        .entry_for_batch(applied.batch_id.as_deref().unwrap())
        .unwrap()
        .unwrap();

    let report = lab.history().undo(moved).unwrap();
    assert!(report.undone.is_empty());
    assert_eq!(report.left.len(), 1);
    assert_eq!(report.left[0].reason, "Undo \"Renamed Heat (1995)\" first.");
    assert_eq!(report.state, UndoState::Available);
    let archive = Path::new(&lab.archive.path);
    assert!(
        archive.join("Heat (1995)/Heat (1995).mkv").is_file(),
        "nothing moved"
    );

    // Newest first: the rename, then the move.
    assert_eq!(
        lab.history().undo(renamed).unwrap().state,
        UndoState::Undone
    );
    let report = lab.history().undo(moved).unwrap();
    assert_eq!(report.undone, [heat], "{report:?}");
    assert!(lab.path("heat.1995/heat.mkv").is_file());
    assert!(lab.item(heat).core.path.starts_with(&lab.movies.path));
}

#[test]
fn undo_of_a_move_brings_the_title_back_and_the_library_follows() {
    let lab = Lab::new();
    let heat = lab.movie("Heat (1995)", "Heat (1995)", "Heat", 1995);
    let batch = lab.move_to_archive(&[heat]);
    let entry = lab.history().entry_for_batch(&batch).unwrap().unwrap();
    let report = lab.history().undo(entry).unwrap();
    assert_eq!(report.undone, [heat], "{report:?}");
    assert!(lab.path("Heat (1995)/Heat (1995).mkv").is_file());
    assert!(lab.item(heat).core.path.starts_with(&lab.movies.path));
    assert_eq!(
        lab.history().entry(entry).unwrap().undo_state,
        UndoState::Undone
    );
}

#[test]
fn a_place_is_named_by_its_drive_or_mount_point() {
    assert_eq!(place_name(r"d:\Movies\Heat"), "D:");
    assert_eq!(place_name("/home/alex/Movies/"), "/home/alex/Movies");
    // A Linux drive is named by where it is mounted; the same text elsewhere is just a path.
    let mounted = place_name("/media/alex/Films/Movies/Heat");
    if cfg!(target_os = "linux") {
        assert_eq!(mounted, "/media/alex/Films");
        assert_eq!(place_name("/mnt/nas/Movies"), "/mnt/nas");
    } else {
        assert_eq!(mounted, "/media/alex/Films/Movies/Heat");
    }
}
