//! History Undo: one entry, or one title's part of it, taken back (spec `operation-history`
//! "Undo a batch", "Undo checks each title first", "Undo of an older entry respects newer
//! ones").

use cv_db::repo::history::{
    HistoryEntryRow, HistoryTitleRow, entry_titles, get_entry, newer_entries_for, set_title_state,
    set_undo_state,
};
use cv_db::repo::library_ops::{hidden_by, tree_ids, unhide_items};
use cv_domain::ItemId;

use super::service::binned_of;
use super::{HistoryError, HistoryKind, HistoryService, TitleState, UndoState};
use crate::library_ops::{BinnedItem, NOT_IN_BIN, PATH_TAKEN, restore_all};

/// A title an Undo left as it is, with the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TitleUndo {
    pub item_id: ItemId,
    pub title: String,
    pub reason: String,
}

/// What an Undo did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoReport {
    pub entry_id: i64,
    /// The titles taken back.
    pub undone: Vec<ItemId>,
    /// The titles left as they are, each with its reason.
    pub left: Vec<TitleUndo>,
    /// The entry's state afterwards.
    pub state: UndoState,
    /// When the entry was wholly undone.
    pub undone_at: Option<i64>,
}

/// What an Undo is about to do: the entry, the titles to take back, and the ones already left.
pub struct UndoPlan {
    pub(super) row: HistoryEntryRow,
    pub(super) todo: Vec<HistoryTitleRow>,
    /// Titles a later entry changed again.
    pub left: Vec<TitleUndo>,
}

impl UndoPlan {
    /// The kind of the entry.
    #[must_use]
    pub fn kind(&self) -> HistoryKind {
        self.row.kind
    }

    /// The entry's stored detail (JSON).
    #[must_use]
    pub fn detail_json(&self) -> &str {
        &self.row.detail_json
    }

    /// The titles to take back, with their names.
    #[must_use]
    pub fn titles(&self) -> Vec<(ItemId, String)> {
        self.todo
            .iter()
            .map(|t| (ItemId(t.item_id), t.title.clone()))
            .collect()
    }
}

/// The titles a kind's Undo took back and the ones it left.
pub(super) type Dispatched = (Vec<ItemId>, Vec<TitleUndo>);

impl HistoryService {
    /// Takes a whole entry back (spec "Undo a batch"). Every title is checked first: one that a
    /// later entry changed again, or whose files cannot be put back whole, is left as it is and
    /// reported, while the others are undone. The entry then reads "Undone" (every title back)
    /// or partly undone. The Undo of a toast calls this with the same entry.
    ///
    /// # Errors
    /// The entry is unknown or cannot be undone (a permanent delete, undone already), or the
    /// library cannot be read or written.
    pub fn undo(&self, entry_id: i64) -> Result<UndoReport, HistoryError> {
        self.undo_some(entry_id, None)
    }

    /// Takes back one title's part of an entry (the "Renamed from … · Undo" of a title page);
    /// the entry then says how many of its titles are undone.
    ///
    /// # Errors
    /// See [`Self::undo`]; also when the title's part is not in the entry or undone already.
    pub fn undo_title(&self, entry_id: i64, item: ItemId) -> Result<UndoReport, HistoryError> {
        self.undo_some(entry_id, Some(item))
    }

    fn undo_some(&self, entry_id: i64, only: Option<ItemId>) -> Result<UndoReport, HistoryError> {
        let plan = self.plan_undo(entry_id, only)?;
        let mut undone = Vec::new();
        let mut left = plan.left.clone();
        if !plan.todo.is_empty() {
            let (back, refused) = self.dispatch(&plan.row, &plan.todo)?;
            undone = back;
            left.extend(refused);
        }
        self.finish_undo(entry_id, undone, left)
    }

    /// Checks what an Undo of `entry_id` may do: refuses an entry without Undo or undone
    /// already, and sorts its titles into the ones to take back and the ones a later entry
    /// changed again (those are left, "Undo <later entry> first"). Artwork and match entries are
    /// taken back by their own services, which use this and [`Self::finish_undo`].
    ///
    /// # Errors
    /// The entry is unknown or cannot be undone, or the library cannot be read.
    pub fn plan_undo(&self, entry_id: i64, only: Option<ItemId>) -> Result<UndoPlan, HistoryError> {
        let (row, titles) = self.db.read(move |conn| {
            let row = get_entry(conn, entry_id)?;
            let titles = entry_titles(conn, entry_id)?;
            Ok((row, titles))
        })?;
        let row = row.ok_or(HistoryError::UnknownEntry(entry_id))?;
        match row.undo_state {
            UndoState::None => {
                let note = serde_json::from_str::<serde_json::Value>(&row.detail_json)
                    .ok()
                    .and_then(|d| {
                        d.get("undo_off")
                            .and_then(|v| v.as_str())
                            .map(str::to_owned)
                    });
                return Err(HistoryError::Refused(note.map_or_else(
                    || "A permanent delete cannot be undone.".to_owned(),
                    |note| format!("This change cannot be undone: {}.", note.to_lowercase()),
                )));
            }
            UndoState::Undone => {
                return Err(HistoryError::Refused(
                    "This change is undone already.".to_owned(),
                ));
            }
            UndoState::Available | UndoState::Partial => {}
        }
        let mut todo: Vec<HistoryTitleRow> = titles
            .into_iter()
            .filter(|t| t.state == TitleState::Done)
            .filter(|t| only.is_none_or(|id| id.0 == t.item_id))
            .collect();
        if todo.is_empty() {
            return Err(HistoryError::Refused(if only.is_some() {
                "This title's part of the change is not there to undo.".to_owned()
            } else {
                "There is nothing left to undo here.".to_owned()
            }));
        }
        // A later entry that changed a title again is undone first; this one leaves it alone.
        let mut left = Vec::new();
        let ids: Vec<i64> = todo.iter().map(|t| t.item_id).collect();
        let newer = self
            .db
            .read(move |conn| newer_entries_for(conn, entry_id, &ids))?;
        todo.retain(|t| match newer.iter().find(|n| n.item_id == t.item_id) {
            Some(n) => {
                left.push(TitleUndo {
                    item_id: ItemId(t.item_id),
                    title: t.title.clone(),
                    reason: format!("Undo \"{}\" first.", n.summary),
                });
                false
            }
            None => true,
        });
        Ok(UndoPlan { row, todo, left })
    }

    /// Records an Undo: marks the titles taken back, sets the entry's state (undone when no
    /// title is left done, partly undone when some are, unchanged when none went back) and tells
    /// the pages that show the titles.
    ///
    /// # Errors
    /// The library cannot be written.
    pub fn finish_undo(
        &self,
        entry_id: i64,
        undone: Vec<ItemId>,
        left: Vec<TitleUndo>,
    ) -> Result<UndoReport, HistoryError> {
        let (state, undone_at) = self.settle(entry_id, &undone)?;
        if let Some(hook) = &self.changed
            && !undone.is_empty()
        {
            hook(&undone);
        }
        Ok(UndoReport {
            entry_id,
            undone,
            left,
            state,
            undone_at,
        })
    }

    /// Runs the Undo of the entry's kind for `titles`.
    fn dispatch(
        &self,
        row: &HistoryEntryRow,
        titles: &[HistoryTitleRow],
    ) -> Result<Dispatched, HistoryError> {
        match row.kind {
            HistoryKind::Delete => self.restore_deleted(row, titles),
            HistoryKind::DeletePermanent => Err(HistoryError::Refused(
                "A permanent delete cannot be undone.".to_owned(),
            )),
            HistoryKind::Rename | HistoryKind::Move => self.undo_batch(row, titles),
            HistoryKind::Artwork | HistoryKind::Match => Err(HistoryError::Refused(
                "This change is taken back by its own service, not by the History service."
                    .to_owned(),
            )),
        }
    }

    /// Takes the titles of a rename or move batch back through the journal (the organize undo,
    /// which checks every file first and leaves a title whose files changed since).
    fn undo_batch(
        &self,
        row: &HistoryEntryRow,
        titles: &[HistoryTitleRow],
    ) -> Result<Dispatched, HistoryError> {
        let Some(batch) = row.batch_ref.as_deref() else {
            return Err(HistoryError::Refused(
                "This change has no journal to undo it from.".to_owned(),
            ));
        };
        let organize = self.organize.clone().unwrap_or_else(|| {
            crate::organize::OrganizeService::new(std::sync::Arc::clone(&self.db))
                .with_bin(std::sync::Arc::clone(&self.bin))
        });
        let ids: Vec<ItemId> = titles.iter().map(|t| ItemId(t.item_id)).collect();
        // The whole batch at once when every title of it is asked for, so a batch a stop left
        // half done is settled too.
        let whole = row.undo_state == UndoState::Available
            && usize::try_from(row.titles).is_ok_and(|n| n == ids.len());
        let outcome = organize
            .undo_items(batch, (!whole).then_some(ids.as_slice()))
            .map_err(|e| HistoryError::Refused(e.to_string()))?;
        let mut left: Vec<TitleUndo> = Vec::new();
        for problem in outcome.not_restorable.iter().chain(&outcome.failed) {
            let Some(id) = problem.id.as_deref().and_then(|id| id.parse::<i64>().ok()) else {
                continue;
            };
            if left.iter().any(|l| l.item_id.0 == id) {
                continue;
            }
            let title = titles
                .iter()
                .find(|t| t.item_id == id)
                .map_or_else(|| problem.title.clone(), |t| t.title.clone());
            let reason = if problem.path.is_empty() {
                problem.reason.clone()
            } else {
                format!("{}: {}", problem.path, problem.reason)
            };
            left.push(TitleUndo {
                item_id: ItemId(id),
                title,
                reason,
            });
        }
        let undone = ids
            .into_iter()
            .filter(|id| !left.iter().any(|l| l.item_id == *id))
            .collect();
        Ok((undone, left))
    }

    /// Marks the undone titles and sets the entry's state: undone when no title is left done,
    /// partly undone when some are, unchanged when none went back.
    fn settle(
        &self,
        entry_id: i64,
        undone: &[ItemId],
    ) -> Result<(UndoState, Option<i64>), HistoryError> {
        let undone: Vec<i64> = undone.iter().map(|id| id.0).collect();
        let now = crate::library_ops::now_millis();
        Ok(self.db.write(move |tx| {
            for &item in &undone {
                set_title_state(tx, entry_id, item, TitleState::Undone)?;
            }
            let titles = entry_titles(tx, entry_id)?;
            let current = get_entry(tx, entry_id)?.map_or(UndoState::Available, |r| r.undo_state);
            let still_done = titles.iter().any(|t| t.state == TitleState::Done);
            let any_undone = titles.iter().any(|t| t.state == TitleState::Undone);
            let (state, at) = if !still_done && any_undone {
                (UndoState::Undone, Some(now))
            } else if any_undone {
                (UndoState::Partial, None)
            } else {
                (current, None)
            };
            if state != current || at.is_some() {
                set_undo_state(tx, entry_id, state, at)?;
            }
            Ok((state, at))
        })?)
    }

    /// Restores deleted titles from the bin and brings their hidden rows back (spec
    /// `title-deletion` "Delete is undoable"): details, artwork rows, stage and history as they
    /// were, without a rescan. A title whose files cannot all be put back (gone from the bin,
    /// the old path taken, the drive offline) is left as it is and nothing of it is restored.
    fn restore_deleted(
        &self,
        row: &HistoryEntryRow,
        titles: &[HistoryTitleRow],
    ) -> Result<Dispatched, HistoryError> {
        let binned = binned_of(&row.detail_json);
        let entry_id = row.id;
        let mut undone = Vec::new();
        let mut left = Vec::new();
        for title in titles {
            let id = ItemId(title.item_id);
            let leave = |reason: String| TitleUndo {
                item_id: id,
                title: title.title.clone(),
                reason,
            };
            let Some(item) = self.db.read(move |conn| cv_db::repo::get_item(conn, id))? else {
                left.push(leave("This title is no longer in the library.".to_owned()));
                continue;
            };
            let source = &item.core.data_source;
            if source.is_empty() || !std::path::Path::new(source).is_dir() {
                left.push(leave(format!(
                    "The drive of {source} is offline; nothing was restored."
                )));
                continue;
            }
            let items: Vec<BinnedItem> = binned
                .iter()
                .filter(|(item_id, _)| *item_id == title.item_id)
                .flat_map(|(_, list)| list.iter().cloned())
                .collect();
            if let Err(problems) = restore_all(self.bin.as_ref(), &items) {
                left.push(leave(restore_reason(&problems)));
                continue;
            }
            self.db.write(move |tx| {
                let hidden = hidden_by(tx, entry_id)?;
                let tree: Vec<ItemId> = tree_ids(tx, id)?
                    .into_iter()
                    .filter(|i| hidden.contains(i))
                    .collect();
                unhide_items(tx, &tree)?;
                Ok(())
            })?;
            undone.push(id);
        }
        Ok((undone, left))
    }
}

/// One sentence for what kept a title's files from coming back.
fn restore_reason(problems: &[(String, String)]) -> String {
    let bin = crate::library_ops::bin_name();
    if let Some((path, _)) = problems.iter().find(|(_, why)| why == PATH_TAKEN) {
        return format!(
            "Something is at {path} now. Nothing was replaced and the files stay in the {bin}."
        );
    }
    if problems.iter().any(|(_, why)| why == NOT_IN_BIN) {
        return format!("The files are no longer in the {bin}; nothing was restored.");
    }
    problems.first().map_or_else(
        || "The files could not be restored.".to_owned(),
        |(path, why)| format!("{path}: {why}"),
    )
}
