//! History for match changes (spec `title-matching` "A match change is undoable").
//!
//! Scraping a title with a different match than it has, and "Accept all confident", write a
//! History entry (kind `match`). Before the new match is written the title is copied (fields,
//! IDs, ratings, people, collection, step outcomes, remembered artwork sources) and its NFO and
//! artwork files go to the Recycle Bin / Trash, or the library folder's trash folder; Undo
//! brings the copy and the files back and sends the files the new match wrote to the bin.

use std::path::PathBuf;
use std::sync::{Arc, PoisonError};

use cv_db::Database;
use cv_db::repo::review;
use cv_domain::{Item, ItemId, ProviderIds};
use serde_json::{Value, json};

use super::MatchService;
use crate::history::snapshot::{TitleSnapshot, capture, is_match_file};
use crate::history::{
    HistoryError, HistoryKind, HistoryService, NewEntry, NewTitle, TitleUndo, UndoReport, record,
};
use crate::library_ops::{BinnedItem, SystemBin, present_all, restore_all, send_or_fallback};
use crate::title_guard::{WRITE_WAIT, Work};

/// "Heat (1995)".
pub(super) fn label_of(title: &str, year: u16) -> String {
    let title = title.trim();
    if year > 0 {
        format!("{title} ({year})")
    } else {
        title.to_owned()
    }
}

pub(super) fn item_label(item: &Item) -> String {
    label_of(&item.core.title, item.core.year)
}

/// Whether `new` names another title than `old`: some ID both have differs. The collection ID
/// names the collection, not the title, and does not count.
pub(super) fn ids_differ(old: &ProviderIds, new: &ProviderIds) -> bool {
    new.iter().any(|(key, value)| {
        key != cv_domain::id_key::TMDB_SET
            && old
                .get(key)
                .is_some_and(|before| !before.trim().eq_ignore_ascii_case(value.trim()))
    })
}

fn batch_label(kind: &str, id: i64) -> String {
    format!("{kind}-{id}-{}", crate::library_ops::now_millis())
}

/// Sends the NFO and artwork files of a title to the bin (or the trash folder) and remembers
/// where each went in the snapshot.
fn bin_match_files(bin: &dyn SystemBin, snapshot: &mut TitleSnapshot) -> Result<(), String> {
    let paths = snapshot.match_files();
    if paths.is_empty() {
        return Ok(());
    }
    let root = PathBuf::from(snapshot.item.core.data_source.trim());
    match send_or_fallback(bin, &paths, &root, &batch_label("match", snapshot.item_id)) {
        Ok(binned) => {
            snapshot.binned = binned.items;
            Ok(())
        }
        Err(error) => {
            // What went already comes back; the title stays as it was.
            if !error.sent.is_empty() {
                let _ = restore_all(bin, &error.sent);
            }
            Err(error.reason)
        }
    }
}

/// Puts the files a snapshot binned back (the scrape did not happen after all).
fn unbin(bin: &dyn SystemBin, snapshot: &TitleSnapshot) {
    if snapshot.binned.is_empty() {
        return;
    }
    if let Err(problems) = restore_all(bin, &snapshot.binned) {
        tracing::warn!(
            ?problems,
            "files of a match that was not written stay in the bin"
        );
    }
}

/// Takes one title back to its snapshot. Nothing changes for the title when it cannot be done
/// whole.
fn restore_title(
    db: &Database,
    bin: &dyn SystemBin,
    snapshot: &TitleSnapshot,
) -> Result<(), String> {
    let id = ItemId(snapshot.item_id);
    let _held = crate::title_guard::hold(db, [id], Work::Write, WRITE_WAIT)
        .map_err(|busy| busy.to_string())?;
    let Some(current) = db
        .read(move |conn| cv_db::repo::get_item(conn, id))
        .map_err(|e| e.to_string())?
    else {
        return Err("This title is no longer in the library.".to_owned());
    };
    let bin_word = crate::library_ops::bin_name();
    if present_all(bin, &snapshot.binned).iter().any(|here| !here) {
        return Err(format!(
            "The earlier files are no longer in the {bin_word}; nothing was restored."
        ));
    }
    // The files the new match wrote sit at the old names: they go to the bin first, never over.
    let root = PathBuf::from(snapshot.item.core.data_source.trim());
    let mut newer: Vec<BinnedItem> = Vec::new();
    let mut taken: Vec<PathBuf> = snapshot
        .binned
        .iter()
        .map(|b| PathBuf::from(&b.original))
        .filter(|p| p.symlink_metadata().is_ok())
        .collect();
    // The NFO and artwork the new match added at names the title did not use (every file of a
    // title never scraped before, an artwork type the earlier match lacked) go to the bin too:
    // they describe a match the library no longer holds. The title's own earlier files stay.
    {
        let before: Vec<PathBuf> = snapshot
            .item
            .core
            .media_files
            .iter()
            .filter(|f| is_match_file(f))
            .map(cv_domain::MediaFile::path)
            .collect();
        for path in current
            .core
            .media_files
            .iter()
            .filter(|f| is_match_file(f))
            .map(cv_domain::MediaFile::path)
        {
            if path.symlink_metadata().is_ok() && !before.contains(&path) && !taken.contains(&path)
            {
                taken.push(path);
            }
        }
    }
    if !taken.is_empty() {
        match send_or_fallback(
            bin,
            &taken,
            &root,
            &batch_label("match-undo", snapshot.item_id),
        ) {
            Ok(binned) => newer = binned.items,
            Err(error) => {
                let _ = restore_all(bin, &error.sent);
                return Err(error.reason);
            }
        }
    }
    if let Err(problems) = restore_all(bin, &snapshot.binned) {
        let _ = restore_all(bin, &newer);
        let first = problems
            .first()
            .map_or_else(String::new, |(path, why)| format!("{path}: {why}"));
        return Err(format!(
            "The earlier files could not be put back ({first})."
        ));
    }
    let snap = snapshot.clone();
    db.write(move |tx| snap.restore_in(tx))
        .map_err(|e| e.to_string())?;
    if let Some(row) = snapshot.review_id {
        // The title waited in the queue before it was accepted: it waits again.
        if let Err(error) = db.write(move |tx| review::reopen(tx, row)) {
            tracing::info!(%error, "the review row of an undone match was not reopened");
        }
    }
    Ok(())
}

impl MatchService {
    /// Replaces the bin the replaced NFO and artwork files go to (the app gives the system bin;
    /// without one the library folder's trash folder is used).
    pub fn set_bin(&self, bin: Arc<dyn SystemBin>) {
        *self.bin.lock().unwrap_or_else(PoisonError::into_inner) = bin;
    }

    pub(super) fn bin(&self) -> Arc<dyn SystemBin> {
        Arc::clone(&self.bin.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// A copy of the title for a match entry, with its NFO and artwork files already in the bin
    /// when `new_ids` name another title. `None` when the title cannot be read.
    ///
    /// # Errors
    /// The files could not be sent to the bin; the title is as it was.
    pub(super) fn snapshot_for_match(
        &self,
        id: ItemId,
        new_ids: Option<&ProviderIds>,
        review_id: Option<i64>,
    ) -> Result<Option<TitleSnapshot>, String> {
        let Ok(Some(mut snapshot)) = capture(&self.db, id) else {
            return Ok(None);
        };
        snapshot.review_id = review_id;
        let changes_title = new_ids.is_some_and(|ids| ids_differ(&snapshot.item.core.ids, ids));
        if changes_title && !snapshot.item.core.locked {
            bin_match_files(self.bin().as_ref(), &mut snapshot)?;
        }
        Ok(Some(snapshot))
    }

    /// Puts the files of a snapshot back after the match was not written.
    pub(super) fn unbin_snapshot(&self, snapshot: &TitleSnapshot) {
        unbin(self.bin().as_ref(), snapshot);
    }

    /// Writes the History entry of matches: one title for a single scrape, many for Accept all
    /// confident. `news` holds the new name of each title. `None` when the entry could not be
    /// stored.
    pub(super) fn record_match(
        &self,
        summary: String,
        line: String,
        done: Vec<(TitleSnapshot, String)>,
    ) -> Option<i64> {
        if done.is_empty() {
            return None;
        }
        let titles: Vec<NewTitle> = done
            .iter()
            .map(|(snapshot, new)| NewTitle {
                item_id: ItemId(snapshot.item_id),
                title: item_label(&snapshot.item),
                before: item_label(&snapshot.item),
                after: new.clone(),
            })
            .collect();
        let snapshots = match serde_json::to_value(done.iter().map(|(s, _)| s).collect::<Vec<_>>())
        {
            Ok(value) => value,
            Err(error) => {
                tracing::warn!(%error, "match change not recorded in History");
                return None;
            }
        };
        let entry = NewEntry {
            kind: HistoryKind::Match,
            summary,
            line,
            bytes: None,
            extra: json!({ "snapshots": snapshots }),
            batch_ref: None,
            titles,
        };
        match record(&self.db, &entry) {
            Ok(id) => Some(id),
            Err(error) => {
                tracing::warn!(%error, "match change not recorded in History");
                None
            }
        }
    }

    /// Takes a match entry back (History's Undo, Undo all, and the toast): each title is put
    /// back as it was with its NFO and artwork files, the files the new match wrote go to the
    /// bin. A title a later entry changed again, or whose earlier files are gone from the bin,
    /// is left and reported.
    ///
    /// # Errors
    /// The entry is unknown or cannot be undone, or the library cannot be read or written.
    pub async fn undo_entry(
        &self,
        entry_id: i64,
        only: Option<ItemId>,
    ) -> Result<UndoReport, HistoryError> {
        let history = HistoryService::new(Arc::clone(&self.db));
        let plan = history.plan_undo(entry_id, only)?;
        let snapshots: Vec<TitleSnapshot> = serde_json::from_str::<Value>(plan.detail_json())
            .ok()
            .and_then(|detail| detail.get("snapshots").cloned())
            .and_then(|value| serde_json::from_value(value).ok())
            .unwrap_or_default();
        let todo = plan.titles();
        // A title the Undo makes New again keeps no NFO (its scrape's NFO went to the bin), so
        // only the titles that had a scraper get theirs written again.
        let had_scraper: Vec<i64> = snapshots
            .iter()
            .filter(|s| !s.item.core.last_scraper_id.trim().is_empty())
            .map(|s| s.item_id)
            .collect();
        let db = Arc::clone(&self.db);
        let bin = self.bin();
        let results = tokio::task::spawn_blocking(move || {
            todo.into_iter()
                .map(|(id, title)| {
                    let outcome = match snapshots.iter().find(|s| s.item_id == id.0) {
                        Some(snapshot) => restore_title(&db, bin.as_ref(), snapshot),
                        None => Err("What is needed to take this back was not kept.".to_owned()),
                    };
                    (id, title, outcome)
                })
                .collect::<Vec<_>>()
        })
        .await
        .map_err(|e| HistoryError::Refused(e.to_string()))?;
        let mut undone = Vec::new();
        let mut left = plan.left.clone();
        for (item_id, title, outcome) in results {
            match outcome {
                Ok(()) => undone.push(item_id),
                Err(reason) => left.push(TitleUndo {
                    item_id,
                    title,
                    reason,
                }),
            }
        }
        let hook = self
            .after_undo
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Some(hook) = hook {
            for id in undone.iter().filter(|id| had_scraper.contains(&id.0)) {
                hook(*id);
            }
        }
        history.finish_undo(entry_id, undone, left)
    }
}
