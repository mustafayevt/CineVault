//! What a match change keeps so History can take it back (spec `title-matching` "A match change
//! is undoable"): the title as it was (fields, IDs, ratings, people, collection), its
//! step outcomes and remembered artwork sources, and the NFO and artwork files that went to the
//! bin or a trash folder before the new match was written.

use cv_db::repo::artwork_refs::{
    ArtworkRef, artwork_refs, delete_artwork_ref, replace_extra_refs, set_artwork_ref,
};
use cv_db::repo::step_outcome::{
    StepStatus, clear_step_outcome, record_step_outcome, step_outcomes,
};
use cv_db::repo::{get_item, update_item};
use cv_db::{Database, DbError};
use cv_domain::{Item, ItemId, MediaFile, MediaFileType};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::library_ops::BinnedItem;

/// One remembered artwork source, as stored in an entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct StoredRef {
    kind: MediaFileType,
    position: u32,
    url: String,
    cache_hash: Option<String>,
    width: u32,
    height: u32,
}

impl From<&ArtworkRef> for StoredRef {
    fn from(r: &ArtworkRef) -> Self {
        Self {
            kind: r.kind,
            position: r.position,
            url: r.url.clone(),
            cache_hash: r.cache_hash.clone(),
            width: r.width,
            height: r.height,
        }
    }
}

impl From<&StoredRef> for ArtworkRef {
    fn from(r: &StoredRef) -> Self {
        Self {
            kind: r.kind,
            position: r.position,
            url: r.url.clone(),
            cache_hash: r.cache_hash.clone(),
            width: r.width,
            height: r.height,
        }
    }
}

/// One step outcome, as stored in an entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredStep {
    step: String,
    status: String,
    reason: String,
    at_ms: i64,
}

/// One title before a scrape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TitleSnapshot {
    pub item_id: i64,
    pub item: Item,
    steps: Vec<StoredStep>,
    refs: Vec<StoredRef>,
    /// The NFO and artwork files the new match replaced, with where each went.
    pub binned: Vec<BinnedItem>,
    /// The review row the title was accepted from; Undo puts it back in the queue.
    pub review_id: Option<i64>,
}

/// Reads a title as it is now. `None` when the title is gone.
///
/// # Errors
/// The library cannot be read.
pub(crate) fn capture(db: &Database, id: ItemId) -> Result<Option<TitleSnapshot>, DbError> {
    db.read(|conn| {
        let Some(item) = get_item(conn, id)? else {
            return Ok(None);
        };
        let steps = step_outcomes(conn, id)?
            .into_iter()
            .map(|o| StoredStep {
                step: o.step,
                status: o.status.as_str().to_owned(),
                reason: o.reason,
                at_ms: o.at_ms,
            })
            .collect();
        let refs = artwork_refs(conn, id)?
            .iter()
            .map(StoredRef::from)
            .collect();
        Ok(Some(TitleSnapshot {
            item_id: id.0,
            item,
            steps,
            refs,
            binned: Vec::new(),
            review_id: None,
        }))
    })
}

/// Whether a file is part of a title's match: its NFO and its artwork (not the video, subtitles
/// or extras).
#[must_use]
pub(crate) fn is_match_file(file: &MediaFile) -> bool {
    file.kind == MediaFileType::Nfo || file.kind.is_graphic()
}

impl TitleSnapshot {
    /// The NFO and artwork files of the title that are on disk, as paths.
    #[must_use]
    pub(crate) fn match_files(&self) -> Vec<std::path::PathBuf> {
        self.item
            .core
            .media_files
            .iter()
            .filter(|f| is_match_file(f))
            .map(MediaFile::path)
            .filter(|p| p.symlink_metadata().is_ok())
            .collect()
    }

    /// Puts the title back in the library: its fields and IDs, step outcomes and remembered
    /// artwork sources; its NFO and artwork files are the snapshot's that are on disk again,
    /// every other file stays as the library has it now.
    pub(crate) fn restore_in(&self, tx: &Connection) -> Result<(), DbError> {
        let id = ItemId(self.item_id);
        let Some(current) = get_item(tx, id)? else {
            return Err(DbError::ItemNotFound(self.item_id));
        };
        let mut restored = self.item.clone();
        let mut files: Vec<MediaFile> = current
            .core
            .media_files
            .into_iter()
            .filter(|f| !is_match_file(f))
            .collect();
        files.extend(
            self.item
                .core
                .media_files
                .iter()
                .filter(|f| is_match_file(f) && f.path().symlink_metadata().is_ok())
                .cloned(),
        );
        restored.core.media_files = files;
        update_item(tx, &restored)?;

        for current in step_outcomes(tx, id)? {
            if !self.steps.iter().any(|s| s.step == current.step) {
                clear_step_outcome(tx, id, &current.step)?;
            }
        }
        for step in &self.steps {
            let Some(status) = StepStatus::parse(&step.status) else {
                continue;
            };
            record_step_outcome(tx, id, &step.step, status, &step.reason, step.at_ms)?;
        }

        let before: Vec<ArtworkRef> = self.refs.iter().map(ArtworkRef::from).collect();
        for now in artwork_refs(tx, id)? {
            if !before
                .iter()
                .any(|r| r.kind == now.kind && r.position == now.position)
            {
                delete_artwork_ref(tx, id, now.kind, now.position)?;
            }
        }
        for kind in [MediaFileType::Extrafanart, MediaFileType::Extrathumb] {
            let extras: Vec<ArtworkRef> =
                before.iter().filter(|r| r.kind == kind).cloned().collect();
            replace_extra_refs(tx, id, kind, &extras)?;
        }
        for entry in before.iter().filter(|r| {
            !matches!(
                r.kind,
                MediaFileType::Extrafanart | MediaFileType::Extrathumb
            )
        }) {
            set_artwork_ref(tx, id, entry)?;
        }
        Ok(())
    }
}
