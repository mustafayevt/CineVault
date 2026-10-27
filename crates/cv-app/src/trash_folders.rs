//! CineVault's own trash folders and what it sent to the Recycle Bin (spec `drives` "Trash on
//! each drive", `operation-history` "Undo after the trash was emptied").
//!
//! * A **trash folder** is `<library folder>/.deletedByCineVault` or the custom trash folder's
//!   own `<custom>/.deletedByCineVault` (Settings › Deleted files; never the chosen folder's
//!   top). Its top entries are batches (`<batch>/…`). Sizes are read by a walk that never
//!   follows a link or junction.
//! * The **bin items** of a drive are the paths CineVault itself sent to the Recycle Bin / Trash,
//!   as History recorded them (`binned` in an entry's detail, with the bin's own id), that are
//!   still there. Nothing else in the bin is ever listed or touched.
//! * **Emptying** erases, after the typed confirmation `DELETE`, the batches of a drive's trash
//!   folders and its bin items. A path is erased only when it is a top entry of a trash folder
//!   (its parent is the trash folder, its name a plain name) and the trash folder is no link; the
//!   walk removes links themselves, never what they point to. Bin items are erased by their
//!   recorded id only.
//! * Afterwards History follows: an entry of a delete, a match or an artwork change whose files
//!   were emptied loses its Undo, with the reason; the rows of a rename's leftovers are marked
//!   erased, so the rename itself stays undoable; a move's Undo copies the title back from its
//!   new place, as it does when its originals left the bin.
//! * **Emptying automatically** (Settings › Deleted files, off by default) erases batches whose
//!   newest file is older than the chosen number of days, in CineVault's own trash folders only,
//!   never in the system bin.

use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cv_db::repo::history::{
    HistoryKind, UndoState, get_entry, open_details, set_detail, set_undo_state,
};
use cv_db::repo::rename_journal::{batch_rows, mark_failed};
use cv_db::{Database, DbError};
use cv_rename::exec::TRASH_DIR;
use serde_json::Value;

use crate::library_ops::{BinnedItem, FOLDER_PREFIX, SystemBin, is_erased};

/// What the person types to empty a trash folder.
pub const EMPTY_CONFIRMATION: &str = "DELETE";

/// The Undo reason of an entry whose files were emptied from a trash folder.
pub const EMPTIED_FOLDER: &str = "Its files were emptied from the trash folder";
/// The Undo reason of an entry whose files were emptied from the Recycle Bin.
pub const EMPTIED_BIN: &str = "Its files were emptied from the Recycle Bin";
/// The Undo reason of an entry whose files were erased at once.
pub const ERASED_AT_ONCE: &str =
    "Its files were erased: Deleted files are set to Delete permanently";

/// One top entry (a batch) of a trash folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashBatch {
    /// The trash folder that holds it.
    pub trash: PathBuf,
    /// Its name in the trash folder.
    pub name: String,
    pub bytes: u64,
    /// The newest modification time inside it, epoch milliseconds.
    pub newest_ms: i64,
}

impl TrashBatch {
    #[must_use]
    pub fn path(&self) -> PathBuf {
        self.trash.join(&self.name)
    }
}

/// A path CineVault sent to the Recycle Bin / Trash that is still there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinEntry {
    /// The History entry that records it.
    pub entry_id: i64,
    pub item: BinnedItem,
}

/// What an empty did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EmptyReport {
    pub bytes: u64,
    /// Trash-folder batches erased.
    pub batches: usize,
    /// Bin items erased.
    pub bin_items: usize,
    /// What could not be erased, with the reason.
    pub problems: Vec<(String, String)>,
    /// History entries whose Undo was switched off.
    pub entries: Vec<i64>,
}

/// The trash folder of each library folder, existing ones only, once each.
#[must_use]
pub fn trash_dirs(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut seen = BTreeSet::new();
    roots
        .iter()
        .map(|root| root.join(TRASH_DIR))
        .filter(|dir| is_plain_dir(dir))
        .filter(|dir| seen.insert(dir.to_string_lossy().to_lowercase()))
        .collect()
}

/// A folder that is not a link or junction.
fn is_plain_dir(path: &Path) -> bool {
    path.symlink_metadata()
        .is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
}

fn ms_of(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// The bytes below `path` and its newest modification time; links count as themselves.
fn measure(path: &Path) -> (u64, i64) {
    let Ok(meta) = path.symlink_metadata() else {
        return (0, 0);
    };
    let own = meta.modified().map(ms_of).unwrap_or(0);
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return (if meta.is_file() { meta.len() } else { 0 }, own);
    }
    let mut bytes = 0;
    let mut newest = own;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let (b, n) = measure(&entry.path());
            bytes += b;
            newest = newest.max(n);
        }
    }
    (bytes, newest)
}

/// The batches of a trash folder, biggest first.
#[must_use]
pub fn batches(trash: &Path) -> Vec<TrashBatch> {
    if !is_plain_dir(trash) {
        return Vec::new();
    }
    let mut out: Vec<TrashBatch> = std::fs::read_dir(trash)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| {
                    let (bytes, newest_ms) = measure(&entry.path());
                    TrashBatch {
                        trash: trash.to_path_buf(),
                        name: entry.file_name().to_string_lossy().into_owned(),
                        bytes,
                        newest_ms,
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.name.cmp(&b.name)));
    out
}

/// The bytes in a trash folder.
#[must_use]
pub fn folder_bytes(trash: &Path) -> u64 {
    if is_plain_dir(trash) {
        measure(trash).0
    } else {
        0
    }
}

/// Erases `path` and everything below it, removing links themselves and never what they point
/// to. Returns the bytes erased.
fn erase_tree(path: &Path) -> io::Result<u64> {
    let meta = path.symlink_metadata()?;
    if meta.file_type().is_symlink() {
        // A link to a folder is removed as a folder on Windows.
        return cv_fs::retry("erase", path, || {
            std::fs::remove_file(path).or_else(|e| std::fs::remove_dir(path).or(Err(e)))
        })
        .map(|()| 0);
    }
    if !meta.is_dir() {
        let len = meta.len();
        return cv_fs::retry("erase", path, || std::fs::remove_file(path)).map(|()| len);
    }
    let mut bytes = 0;
    for entry in std::fs::read_dir(path)? {
        bytes += erase_tree(&entry?.path())?;
    }
    cv_fs::retry_folder("erase", path, || std::fs::remove_dir(path))?;
    Ok(bytes)
}

/// Erases one batch of a trash folder after checking that it is one: its parent is `trash`, its
/// name a plain name, and `trash` is a `.deletedByCineVault` (a library folder's, or the custom
/// trash folder's own sub-folder) and no link. `custom` is not used to widen that.
///
/// # Errors
/// The batch is not inside a trash folder (nothing is touched), or the disk refused.
pub fn erase_batch(trash: &Path, name: &str, _custom: Option<&Path>) -> io::Result<u64> {
    let refuse = |why: &str| io::Error::new(io::ErrorKind::PermissionDenied, why.to_owned());
    // Only a `.deletedByCineVault` (the custom trash folder's batches live in its own one,
    // `cv_fs::volume::custom_trash_root`): never the chosen folder itself, whose top entries may
    // be the person's own files.
    let is_trash = trash
        .file_name()
        .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(TRASH_DIR));
    if !is_trash {
        return Err(refuse(
            "This is not a CineVault trash folder; nothing was erased",
        ));
    }
    if !is_plain_dir(trash) {
        return Err(refuse("The trash folder is a link; nothing was erased"));
    }
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', ':']) {
        return Err(refuse(
            "This is not an entry of the trash folder; nothing was erased",
        ));
    }
    let path = trash.join(name);
    if path.parent() != Some(trash) {
        return Err(refuse(
            "This is not an entry of the trash folder; nothing was erased",
        ));
    }
    erase_tree(&path)
}

/// Whether an erase of `batch` that stopped with an error had already removed some of it (the
/// batch is gone or holds fewer bytes than were listed). History then follows it like an erased
/// batch, so no Undo brings back a half-emptied title.
#[must_use]
pub fn partly_erased(batch: &TrashBatch) -> bool {
    let path = batch.path();
    path.symlink_metadata().is_err() || measure(&path).0 < batch.bytes
}

/// Every path CineVault sent to the system bin that History still records (not undone), with its
/// entry; trash-folder and erased items are left out.
///
/// # Errors
/// The library cannot be read.
pub fn recorded_bin_items(db: &Database) -> Result<Vec<BinEntry>, DbError> {
    let mut out = Vec::new();
    for kind in [HistoryKind::Delete, HistoryKind::Move, HistoryKind::Match] {
        let details = db.read(move |conn| {
            let mut found = Vec::new();
            for (id, detail) in open_details(conn, kind)? {
                let Some(row) = get_entry(conn, id)? else {
                    continue;
                };
                if row.undo_state == UndoState::None {
                    continue;
                }
                let undone: Vec<i64> = cv_db::repo::history::entry_titles(conn, id)?
                    .into_iter()
                    .filter(|t| t.state == cv_db::repo::history::TitleState::Undone)
                    .map(|t| t.item_id)
                    .collect();
                found.push((id, detail, undone));
            }
            Ok(found)
        })?;
        for (id, detail, undone) in details {
            for (item_id, items) in crate::history::binned_items(&detail) {
                if undone.contains(&item_id) {
                    continue;
                }
                for item in items {
                    if item.bin_id.is_empty()
                        || item.bin_id.starts_with(FOLDER_PREFIX)
                        || is_erased(&item)
                    {
                        continue;
                    }
                    out.push(BinEntry { entry_id: id, item });
                }
            }
        }
    }
    Ok(out)
}

/// The recorded bin items that the bin still holds.
#[must_use]
pub fn present_bin_items(bin: &dyn SystemBin, all: Vec<BinEntry>) -> Vec<BinEntry> {
    if all.is_empty() {
        return all;
    }
    let items: Vec<BinnedItem> = all.iter().map(|e| e.item.clone()).collect();
    let here = bin.present(&items);
    let mut seen = BTreeSet::new();
    all.into_iter()
        .zip(here)
        .filter(|(entry, here)| *here && seen.insert(entry.item.bin_id.clone()))
        .map(|(entry, _)| entry)
        .collect()
}

/// Every string of a JSON value.
fn strings(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(s) => out.push(s.clone()),
        Value::Array(list) => list.iter().for_each(|v| strings(v, out)),
        Value::Object(map) => map.values().for_each(|v| strings(v, out)),
        _ => {}
    }
}

/// Switches off the Undo of entry `id` with `reason`, keeping the rest of its detail.
///
/// # Errors
/// The library cannot be written.
pub fn switch_off_undo(db: &Database, id: i64, reason: &str) -> Result<(), DbError> {
    let reason = reason.to_owned();
    db.write(move |tx| {
        let Some(row) = get_entry(tx, id)? else {
            return Ok(());
        };
        if matches!(row.undo_state, UndoState::Undone | UndoState::None) {
            return Ok(());
        }
        let mut detail: Value = serde_json::from_str(&row.detail_json)
            .unwrap_or_else(|_| Value::Object(Default::default()));
        if !detail.is_object() {
            detail = Value::Object(Default::default());
        }
        detail["undo_off"] = Value::String(reason);
        set_detail(tx, id, &detail.to_string())?;
        set_undo_state(tx, id, UndoState::None, None)
    })
}

/// Makes History follow an empty: `emptied` are the trash-folder paths erased, `purged` the bin
/// ids erased. Returns the entries whose Undo was switched off.
///
/// # Errors
/// The library cannot be read or written.
pub fn follow_in_history(
    db: &Database,
    emptied: &[PathBuf],
    purged: &[String],
) -> Result<Vec<i64>, DbError> {
    if emptied.is_empty() && purged.is_empty() {
        return Ok(Vec::new());
    }
    let mut off = Vec::new();
    let names: Vec<String> = emptied
        .iter()
        .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect();
    for kind in HistoryKind::ALL {
        let rows = db.read(move |conn| open_details(conn, kind))?;
        for (id, detail) in rows {
            let value: Value = serde_json::from_str(&detail).unwrap_or(Value::Null);
            let mut texts = Vec::new();
            strings(&value, &mut texts);
            let in_folder = texts.iter().any(|text| {
                let path = Path::new(text.strip_prefix(FOLDER_PREFIX).unwrap_or(text));
                emptied.iter().any(|dir| cv_fs::volume::inside(path, dir))
            });
            let in_bin = texts.iter().any(|text| purged.contains(text));
            match kind {
                HistoryKind::Rename => {
                    // The leftovers are gone; the rename itself stays undoable.
                    let batch =
                        db.read(move |conn| Ok(get_entry(conn, id)?.and_then(|r| r.batch_ref)))?;
                    if let Some(batch) = batch
                        && (in_folder || names.iter().any(|n| n == &batch))
                    {
                        mark_rows_erased(db, &batch, emptied, "emptied from the trash folder")?;
                    }
                }
                // A move's Undo copies the title back from its new place when its originals
                // are gone.
                HistoryKind::Move => {}
                HistoryKind::DeletePermanent => {}
                _ => {
                    if in_folder || in_bin {
                        switch_off_undo(db, id, if in_bin { EMPTIED_BIN } else { EMPTIED_FOLDER })?;
                        off.push(id);
                    }
                }
            }
        }
    }
    Ok(off)
}

/// Marks the `trash` rows of rename batch `batch` whose file lies in one of `gone` (or every one
/// when `gone` is empty) as erased, so the rename's Undo leaves them out. Returns how many.
///
/// # Errors
/// The library cannot be written.
pub fn mark_rows_erased(
    db: &Database,
    batch: &str,
    gone: &[PathBuf],
    why: &str,
) -> Result<usize, DbError> {
    let batch = batch.to_owned();
    let gone = gone.to_vec();
    let why = format!("erased: {why}");
    db.write(move |tx| {
        let mut count = 0;
        for row in batch_rows(tx, &batch)? {
            let to = Path::new(&row.to_path);
            if row.op != "trash" || row.state != "done" {
                continue;
            }
            if !gone.is_empty() && !gone.iter().any(|dir| cv_fs::volume::inside(to, dir)) {
                continue;
            }
            if to.symlink_metadata().is_ok() {
                continue;
            }
            mark_failed(tx, &batch, row.seq, &why)?;
            count += 1;
        }
        Ok(count)
    })
}

/// Erases the leftovers rename batch `batch` put in the trash (Settings › Deleted files is
/// Delete permanently) and marks their rows erased, so the rename stays undoable. Only files
/// inside a trash folder are erased.
pub fn erase_rename_leftovers(db: &Database, batch: &str) -> usize {
    let rows = match db.read({
        let batch = batch.to_owned();
        move |conn| batch_rows(conn, &batch)
    }) {
        Ok(rows) => rows,
        Err(error) => {
            tracing::warn!(%error, batch, "the leftovers of the rename were not erased");
            return 0;
        }
    };
    let mut dirs: BTreeSet<PathBuf> = BTreeSet::new();
    for row in rows.iter().filter(|r| r.op == "trash" && r.state == "done") {
        let to = Path::new(&row.to_path);
        // `<root>/.deletedByCineVault/<batch>/…` only.
        let Some(batch_dir) = to.ancestors().find(|a| {
            a.file_name()
                .is_some_and(|n| n == std::ffi::OsStr::new(batch))
                && a.parent()
                    .and_then(Path::file_name)
                    .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(TRASH_DIR))
        }) else {
            continue;
        };
        if let Err(error) = crate::library_ops::erase_path(to) {
            tracing::warn!(%error, path = %to.display(), "a leftover was not erased");
            continue;
        }
        dirs.insert(batch_dir.to_path_buf());
    }
    let marked = mark_rows_erased(
        db,
        batch,
        &dirs.iter().cloned().collect::<Vec<_>>(),
        "Delete permanently",
    )
    .unwrap_or_else(|error| {
        tracing::warn!(%error, batch, "the erased leftovers stay in the journal");
        0
    });
    for dir in dirs {
        remove_empty(&dir);
        if let Some(trash) = dir.parent() {
            let _ = std::fs::remove_dir(trash);
        }
    }
    marked
}

/// The batch folder (`<…>/.deletedByCineVault/<batch>`) a trashed path lies in.
fn batch_dir_of(path: &Path) -> Option<&Path> {
    path.ancestors().skip(1).find(|a| {
        a.parent()
            .and_then(Path::file_name)
            .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(TRASH_DIR))
    })
}

/// Applies Settings › Deleted files to files an artwork or NFO change set aside in a drive's
/// trash folder (`(original, trashed)` pairs): a custom trash folder takes the ones on its volume
/// (by a rename), Delete permanently erases them. Returns the pairs as they are now; the caller
/// switches the History entry's Undo off when they were erased.
#[must_use]
pub fn settle_displaced(
    moves: &[(PathBuf, PathBuf)],
    choice: &crate::deleted_files::DeletedFiles,
) -> Vec<(PathBuf, PathBuf)> {
    let mut out = Vec::with_capacity(moves.len());
    let mut batches: BTreeSet<PathBuf> = BTreeSet::new();
    for (original, trashed) in moves {
        let Some(batch) = batch_dir_of(trashed) else {
            out.push((original.clone(), trashed.clone()));
            continue;
        };
        if choice.erases() {
            if let Err(error) = crate::library_ops::erase_path(trashed) {
                tracing::warn!(%error, path = %trashed.display(), "a replaced file was not erased");
            }
            batches.insert(batch.to_path_buf());
            out.push((original.clone(), trashed.clone()));
            continue;
        }
        let label = batch
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let target = choice
            .custom_folder()
            .and_then(|custom| cv_fs::volume::custom_trash_path(custom, &label, original))
            .filter(|target| target.symlink_metadata().is_err());
        let Some(target) = target else {
            out.push((original.clone(), trashed.clone()));
            continue;
        };
        let moved = target
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| cv_fs::retry("trash", trashed, || std::fs::rename(trashed, &target)));
        match moved {
            Ok(()) => {
                batches.insert(batch.to_path_buf());
                out.push((original.clone(), target));
            }
            Err(error) => {
                tracing::warn!(%error, path = %trashed.display(), "kept in the drive's trash folder");
                out.push((original.clone(), trashed.clone()));
            }
        }
    }
    for batch in batches {
        remove_empty(&batch);
        if let Some(trash) = batch.parent() {
            let _ = std::fs::remove_dir(trash);
        }
    }
    out
}

/// Removes the empty folders below `dir`, then `dir` when it is empty.
fn remove_empty(dir: &Path) {
    if !is_plain_dir(dir) {
        return;
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            remove_empty(&entry.path());
        }
    }
    let _ = std::fs::remove_dir(dir);
}

/// Erases the batches of `trashes` whose newest file is older than `days`, and makes History
/// follow. Never the system bin.
pub fn empty_older_than(
    db: &Database,
    trashes: &[PathBuf],
    custom: Option<&Path>,
    days: u32,
    now: SystemTime,
) -> EmptyReport {
    let limit = ms_of(now)
        - i64::try_from(Duration::from_secs(u64::from(days) * 86_400).as_millis())
            .unwrap_or(i64::MAX);
    let mut report = EmptyReport::default();
    let mut emptied = Vec::new();
    for trash in trashes {
        for batch in batches(trash) {
            if batch.newest_ms == 0 || batch.newest_ms >= limit {
                continue;
            }
            match erase_batch(trash, &batch.name, custom) {
                Ok(bytes) => {
                    report.bytes += bytes;
                    report.batches += 1;
                    emptied.push(batch.path());
                }
                Err(error) => {
                    if partly_erased(&batch) {
                        emptied.push(batch.path());
                    }
                    report.problems.push((
                        batch.path().to_string_lossy().into_owned(),
                        error.to_string(),
                    ));
                }
            }
        }
    }
    match follow_in_history(db, &emptied, &[]) {
        Ok(entries) => report.entries = entries,
        Err(error) => tracing::warn!(%error, "History did not follow the automatic empty"),
    }
    report
}

#[cfg(test)]
mod tests;
