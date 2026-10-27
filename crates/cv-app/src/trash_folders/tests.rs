//! Trash folders, the bin items CineVault recorded, and History after an empty. The system bin
//! is a folder (`FakeBin`); nothing here touches the real one.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use cv_db::Database;
use cv_db::repo::history::{UndoState, get_entry};
use cv_domain::ItemId;
use serde_json::json;

use super::*;
use crate::history::{NewEntry, NewTitle, record};
use crate::library_ops::send_or_fallback;
use crate::library_ops::trash_bin::fake::FakeBin;

fn db(dir: &Path) -> Arc<Database> {
    Arc::new(Database::open(dir.join("library.db")).unwrap())
}

fn file(path: &Path, bytes: usize) -> PathBuf {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, vec![7_u8; bytes]).unwrap();
    path.to_path_buf()
}

fn entry(kind: HistoryKind, extra: serde_json::Value, batch: Option<&str>) -> NewEntry {
    NewEntry {
        kind,
        summary: "x".to_owned(),
        line: String::new(),
        bytes: None,
        extra,
        batch_ref: batch.map(str::to_owned),
        titles: vec![NewTitle {
            item_id: ItemId(1),
            title: "Heat".to_owned(),
            before: String::new(),
            after: String::new(),
        }],
    }
}

#[test]
fn batches_are_measured_without_following_links() {
    let dir = tempfile::tempdir().unwrap();
    let trash = dir.path().join("Movies").join(TRASH_DIR);
    file(&trash.join("b1/Heat/movie.nfo"), 10);
    file(&trash.join("b2/a.jpg"), 30);
    let outside = file(&dir.path().join("outside/keep.mkv"), 50);
    // A link inside the trash folder to a folder outside it (skipped where links need rights).
    let link = trash.join("b2/link");
    #[cfg(windows)]
    let linked = std::os::windows::fs::symlink_dir(outside.parent().unwrap(), &link).is_ok();
    #[cfg(unix)]
    let linked = std::os::unix::fs::symlink(outside.parent().unwrap(), &link).is_ok();
    let found = batches(&trash);
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].name, "b2");
    assert_eq!(found[0].bytes, 30);
    assert_eq!(folder_bytes(&trash), 40);
    let erased = erase_batch(&trash, "b2", None).unwrap();
    assert_eq!(erased, 30);
    assert!(!trash.join("b2").exists());
    assert!(
        outside.is_file(),
        "a link is removed, never what it points to"
    );
    let _ = linked;
}

#[test]
fn only_an_entry_of_a_trash_folder_is_ever_erased() {
    let dir = tempfile::tempdir().unwrap();
    let movies = dir.path().join("Movies");
    let keep = file(&movies.join("Heat/movie.mkv"), 5);
    assert!(erase_batch(&movies, "Heat", None).is_err());
    let trash = movies.join(TRASH_DIR);
    fs::create_dir_all(&trash).unwrap();
    assert!(erase_batch(&trash, "..", None).is_err());
    assert!(erase_batch(&trash, r"..\Heat", None).is_err());
    assert!(erase_batch(&trash, "../Heat", None).is_err());
    assert!(keep.is_file());
    // The custom folder itself is never a trash folder (it may hold the person's own files);
    // only its own `.deletedByCineVault` is.
    let custom = dir.path().join("MyTrash");
    let mine = file(&custom.join("b1/D/x.nfo"), 3);
    assert!(erase_batch(&custom, "b1", None).is_err());
    assert!(erase_batch(&custom, "b1", Some(&custom)).is_err());
    assert!(mine.is_file());
    let own = cv_fs::volume::custom_trash_root(&custom);
    file(&own.join("b1/D/x.nfo"), 3);
    assert_eq!(erase_batch(&own, "b1", Some(&custom)).unwrap(), 3);
}

#[test]
fn a_delete_whose_files_were_emptied_loses_its_undo_with_the_reason() {
    let dir = tempfile::tempdir().unwrap();
    let db = db(dir.path());
    let root = dir.path().join("Movies");
    let path = file(&root.join("Heat (1995)/movie.mkv"), 4);
    let bin = crate::library_ops::FolderOnly;
    let binned =
        send_or_fallback(&bin, &[path.parent().unwrap().to_path_buf()], &root, "b1").unwrap();
    let id = record(
        &db,
        &entry(
            HistoryKind::Delete,
            json!({ "binned": [{ "item_id": 1, "items": binned.items }] }),
            None,
        ),
    )
    .unwrap();
    let other = record(&db, &entry(HistoryKind::Delete, json!({}), None)).unwrap();
    let trash = root.join(TRASH_DIR);
    erase_batch(&trash, "b1", None).unwrap();
    let off = follow_in_history(&db, &[trash.join("b1")], &[]).unwrap();
    assert_eq!(off, vec![id]);
    let row = db.read(move |c| get_entry(c, id)).unwrap().unwrap();
    assert_eq!(row.undo_state, UndoState::None);
    assert!(row.detail_json.contains(EMPTIED_FOLDER));
    let row = db.read(move |c| get_entry(c, other)).unwrap().unwrap();
    assert_eq!(row.undo_state, UndoState::Available);
    let entry = crate::history::HistoryService::new(Arc::clone(&db))
        .entry(id)
        .unwrap();
    assert_eq!(entry.undo_note.as_deref(), Some(EMPTIED_FOLDER));
}

#[test]
fn bin_items_are_the_recorded_ones_still_in_the_bin_and_purge_takes_only_those() {
    let dir = tempfile::tempdir().unwrap();
    let db = db(dir.path());
    let bin = FakeBin::new(dir.path());
    let root = dir.path().join("Movies");
    let a = file(&root.join("A/a.mkv"), 6);
    let b = file(&root.join("B/b.mkv"), 8);
    let sent = send_or_fallback(bin.as_ref(), &[a, b], &root, "b1").unwrap();
    // Something the person put in the bin themselves.
    let theirs = file(&bin.dir.join("99-theirs.txt"), 1);
    let id = record(
        &db,
        &entry(
            HistoryKind::Delete,
            json!({ "binned": [{ "item_id": 1, "items": sent.items }] }),
            None,
        ),
    )
    .unwrap();
    let found = present_bin_items(bin.as_ref(), recorded_bin_items(&db).unwrap());
    assert_eq!(found.len(), 2);
    assert_eq!(found.iter().map(|e| e.item.size).sum::<u64>(), 14);
    let items: Vec<BinnedItem> = found.iter().map(|e| e.item.clone()).collect();
    bin.purge(&items).unwrap();
    assert!(theirs.is_file(), "nothing else in the bin is touched");
    let ids: Vec<String> = items.iter().map(|i| i.bin_id.clone()).collect();
    assert_eq!(follow_in_history(&db, &[], &ids).unwrap(), vec![id]);
    let row = db.read(move |c| get_entry(c, id)).unwrap().unwrap();
    assert!(row.detail_json.contains(EMPTIED_BIN));
    assert!(present_bin_items(bin.as_ref(), recorded_bin_items(&db).unwrap()).is_empty());
}

#[test]
fn the_automatic_empty_takes_only_batches_older_than_the_limit() {
    let dir = tempfile::tempdir().unwrap();
    let db = db(dir.path());
    let trash = dir.path().join("Movies").join(TRASH_DIR);
    let old = file(&trash.join("old/a.nfo"), 2);
    let new = file(&trash.join("new/b.nfo"), 3);
    let _ = (old, new);
    // Seen from 40 days ahead, both are older than 30 days; seen from now, neither is.
    let report = empty_older_than(
        &db,
        std::slice::from_ref(&trash),
        None,
        30,
        SystemTime::now(),
    );
    assert_eq!(report.batches, 0);
    let later = SystemTime::now() + Duration::from_secs(40 * 86_400);
    let report = empty_older_than(&db, std::slice::from_ref(&trash), None, 30, later);
    assert_eq!(report.batches, 2);
    assert_eq!(report.bytes, 5);
    assert!(batches(&trash).is_empty());
}

#[test]
fn erased_rename_leftovers_leave_the_rename_undoable() {
    let dir = tempfile::tempdir().unwrap();
    let db = db(dir.path());
    let root = dir.path().join("Movies");
    let leftover = file(&root.join(TRASH_DIR).join("1-0/Heat/old.nfo"), 2);
    let original = root.join("Heat/old.nfo");
    let (from, to) = (
        original.to_string_lossy().into_owned(),
        leftover.to_string_lossy().into_owned(),
    );
    db.write(move |tx| {
        tx.execute(
            "INSERT INTO rename_journal (batch_id, seq, op, from_path, to_path, state, created_ms)
             VALUES ('1-0', 1, 'trash', ?1, ?2, 'done', 0)",
            [&from, &to],
        )?;
        Ok(())
    })
    .unwrap();
    assert_eq!(erase_rename_leftovers(&db, "1-0"), 1);
    assert!(!leftover.exists());
    assert!(!root.join(TRASH_DIR).exists());
    let rows = db
        .read(|c| cv_db::repo::rename_journal::batch_rows(c, "1-0"))
        .unwrap();
    assert_eq!(rows[0].state, "failed");
    assert!(rows[0].error.starts_with("erased"));
}

#[cfg(windows)]
#[test]
fn a_batch_erased_only_in_part_still_switches_off_the_undo_of_its_delete() {
    let dir = tempfile::tempdir().unwrap();
    let db = db(dir.path());
    let root = dir.path().join("Movies");
    let first = file(&root.join("Heat (1995)/a.nfo"), 4);
    let kept = file(&root.join("Heat (1995)/z.mkv"), 6);
    let bin = crate::library_ops::FolderOnly;
    let binned =
        send_or_fallback(&bin, &[first.parent().unwrap().to_path_buf()], &root, "b1").unwrap();
    let id = record(
        &db,
        &entry(
            HistoryKind::Delete,
            json!({ "binned": [{ "item_id": 1, "items": binned.items }] }),
            None,
        ),
    )
    .unwrap();
    let trash = root.join(TRASH_DIR);
    // A file another program holds without sharing cannot be removed: the erase stops after
    // `a.nfo`.
    let stuck = trash.join("b1/Heat (1995)").join(kept.file_name().unwrap());
    let held = {
        use std::os::windows::fs::OpenOptionsExt;
        fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&stuck)
            .unwrap()
    };
    let later = SystemTime::now() + Duration::from_secs(40 * 86_400);
    let report = empty_older_than(&db, std::slice::from_ref(&trash), None, 30, later);
    assert_eq!(report.batches, 0);
    assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
    assert!(stuck.is_file());
    assert!(!trash.join("b1/Heat (1995)/a.nfo").exists());
    // Undo would bring back half a title: it is off, with the reason.
    assert_eq!(report.entries, vec![id]);
    let row = db.read(move |c| get_entry(c, id)).unwrap().unwrap();
    assert_eq!(row.undo_state, UndoState::None);
    drop(held);
}
