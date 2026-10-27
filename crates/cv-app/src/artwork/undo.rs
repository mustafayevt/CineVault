//! Undo for a chosen image or a set of extras.
//!
//! Putting an image in place moves the file it displaces to the trash folder of the data
//! source. Each placement writes a History entry (kind `artwork`) that holds what was there
//! before (the files that moved, the item's file list and the remembered source URLs), so the
//! Undo toast and the History page can give it all back, also after a restart. The new files go
//! to the trash folder too, never into thin air. The entry is the one record: the token of an
//! outcome is its id.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use cv_artwork::download::files::Trash;
use cv_artwork::download::{ArtworkTarget, DownloadReport};
use cv_db::repo::artwork_refs::{
    ArtworkRef, delete_artwork_ref, replace_extra_refs, set_artwork_ref,
};
use cv_db::repo::history::get_entry;
use cv_db::repo::{get_item, update_item};
use cv_domain::{Item, ItemId, MediaFile, MediaFileType};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::changes::ChangePart;
use crate::history::snapshot::StoredRef;
use crate::history::{
    HistoryError, HistoryKind, HistoryService, NewEntry, NewTitle, TitleUndo, UndoReport,
    UndoState, last_part, record,
};

use super::{ArtworkError, ArtworkOutcome, ArtworkService, Loaded};

/// What was there before one placement (the `place` part of the entry's detail).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct PlaceUndo {
    item: ItemId,
    kind: MediaFileType,
    /// The item's files of this type before the placement.
    files_before: Vec<MediaFile>,
    /// The remembered source URL of the main image before the placement.
    url_before: Option<String>,
    /// The remembered sources of this type before the placement.
    refs_before: Vec<StoredRef>,
    /// The files the placement wrote.
    written: Vec<PathBuf>,
    /// `(original, trashed)` for each file the placement displaced.
    moves: Vec<(PathBuf, PathBuf)>,
    /// The folder the artwork files sat in and the video's base name at the placement: an undo
    /// after a rename or a move would put files back where the title no longer is.
    dir: PathBuf,
    base: String,
}

/// The name of an artwork type in a summary ("Changed poster").
fn label_of(kind: MediaFileType) -> String {
    use MediaFileType::{
        Banner, Characterart, Clearart, Clearlogo, Disc, Extrafanart, Extrathumb, Fanart, Keyart,
        Poster, SeasonBanner, SeasonFanart, SeasonPoster, SeasonThumb, Squareart, Thumb,
    };
    match kind {
        Poster => "poster",
        Fanart => "fanart",
        Banner => "banner",
        Clearart => "clearart",
        Clearlogo => "logo",
        Disc => "disc art",
        Thumb => "thumbnail",
        Keyart => "key art",
        Squareart => "square art",
        Characterart => "character art",
        SeasonPoster => "season poster",
        SeasonFanart => "season fanart",
        SeasonBanner => "season banner",
        SeasonThumb => "season thumbnail",
        Extrafanart => "extra fanart",
        Extrathumb => "extra thumbnails",
        other => return format!("{other:?}").to_lowercase(),
    }
    .to_owned()
}

/// Writes the History entry of a placement that wrote something and returns its id as the
/// outcome's token. `None` when the placement wrote nothing or the entry could not be stored (the
/// images stay in place; they cannot be taken back then).
pub(super) fn remember(
    db: &cv_db::Database,
    item: ItemId,
    loaded: &Loaded,
    kind: MediaFileType,
    report: &DownloadReport,
    choice: &crate::deleted_files::DeletedFiles,
) -> Option<String> {
    if report.images.is_empty() {
        return None;
    }
    // Settings › Deleted files: the replaced images go to the custom trash folder, or are erased.
    let mut report = report.clone();
    report.trash_moves = crate::trash_folders::settle_displaced(&report.trash_moves, choice);
    let erased = choice.erases() && !report.trash_moves.is_empty();
    let token = remember_settled(db, item, loaded, kind, &report)?;
    if erased && let Ok(id) = token.parse::<i64>() {
        if let Err(error) =
            crate::trash_folders::switch_off_undo(db, id, crate::trash_folders::ERASED_AT_ONCE)
        {
            tracing::warn!(%error, "the artwork entry keeps its Undo");
        }
        return None;
    }
    Some(token)
}

fn remember_settled(
    db: &cv_db::Database,
    item: ItemId,
    loaded: &Loaded,
    kind: MediaFileType,
    report: &DownloadReport,
) -> Option<String> {
    let written: Vec<PathBuf> = report
        .images
        .iter()
        .flat_map(|image| image.files.iter().cloned())
        .collect();
    let place = PlaceUndo {
        item,
        kind,
        files_before: loaded
            .item
            .core
            .media_files
            .iter()
            .filter(|f| f.kind == kind)
            .cloned()
            .collect(),
        url_before: loaded.item.core.artwork_urls.get(&kind).cloned(),
        refs_before: loaded
            .recorded
            .iter()
            .filter(|r| r.kind == kind)
            .map(StoredRef::from)
            .collect(),
        written: written.clone(),
        moves: report.trash_moves.clone(),
        dir: loaded.target.dir.clone(),
        base: loaded.target.base.clone(),
    };
    let before = place
        .moves
        .first()
        .map(|(original, _)| last_part(&original.to_string_lossy()))
        .unwrap_or_default();
    let place = match serde_json::to_value(&place) {
        Ok(value) => value,
        Err(error) => {
            tracing::warn!(%error, "artwork placement not recorded in History");
            return None;
        }
    };
    let name = loaded.item.core.title.trim();
    let name = if name.is_empty() {
        last_part(&loaded.item.core.path)
    } else {
        name.to_owned()
    };
    let after = written
        .first()
        .map(|p| last_part(&p.to_string_lossy()))
        .unwrap_or_default();
    let entry = NewEntry {
        kind: HistoryKind::Artwork,
        summary: format!("Changed {} · {name}", label_of(kind)),
        line: format!("{after} · {}", loaded.target.dir.display()),
        bytes: None,
        extra: json!({ "place": place }),
        batch_ref: None,
        titles: vec![NewTitle {
            item_id: item,
            title: name,
            before,
            after,
        }],
    };
    match record(db, &entry) {
        Ok(id) => Some(id.to_string()),
        Err(error) => {
            tracing::warn!(%error, "artwork placement not recorded in History");
            None
        }
    }
}

/// The stored placement of an entry that can still be undone.
fn load_record(db: &cv_db::Database, entry_id: i64) -> Result<PlaceUndo, ArtworkError> {
    let row = db
        .read(move |conn| get_entry(conn, entry_id))?
        .ok_or(ArtworkError::UndoGone)?;
    if row.kind != HistoryKind::Artwork
        || matches!(row.undo_state, UndoState::Undone | UndoState::None)
    {
        return Err(ArtworkError::UndoGone);
    }
    let detail: Value =
        serde_json::from_str(&row.detail_json).map_err(|_| ArtworkError::UndoGone)?;
    detail
        .get("place")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .ok_or(ArtworkError::UndoGone)
}

fn entry_of(token: &str) -> Result<i64, ArtworkError> {
    token.trim().parse().map_err(|_| ArtworkError::UndoGone)
}

impl ArtworkService {
    /// Puts back what a placement displaced: the new files go to the trash, the old ones come
    /// back from it, and the item's file list and remembered sources are what they were. The
    /// token (the History entry's id) works once.
    ///
    /// # Errors
    /// The token is unknown or already used ([`ArtworkError::UndoGone`]), the item is gone, or
    /// the database fails. A file that cannot be moved is listed in the outcome's problems.
    ///
    /// The undo holds the title like the placement (no move or rename runs meanwhile). Once the
    /// title was renamed or moved since the placement, nothing is put back
    /// ([`ArtworkError::UndoStale`]): the files would land in the folder the title left. The
    /// earlier image then stays in the trash folder.
    pub async fn undo_place(&self, token: &str) -> Result<ArtworkOutcome, ArtworkError> {
        let entry_id = entry_of(token)?;
        let item = load_record(&self.db, entry_id)?.item;
        let (outcome, _) = self.undo_held(item, entry_id, Vec::new()).await?;
        // The NFO names the earlier image's address again; the NFO writer holds the title.
        self.refresh_nfo(item).await;
        Ok(outcome)
    }

    /// Takes the History entry of a placement back (History's Undo, Undo all, and the toast):
    /// the same as [`Self::undo_place`], with the answer History gives for every kind. A title a
    /// later entry changed again, or whose folder moved, is left and reported.
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
        let mut left = plan.left.clone();
        if let Some((item, title)) = plan.titles().into_iter().next() {
            match self.undo_held(item, entry_id, left.clone()).await {
                Ok((_, report)) => {
                    self.refresh_nfo(item).await;
                    return Ok(report);
                }
                Err(error) => left.push(TitleUndo {
                    item_id: item,
                    title,
                    reason: error.to_string(),
                }),
            }
        }
        history.finish_undo(entry_id, Vec::new(), left)
    }

    /// The undo while the title is held; the entry is marked undone under the same hold, so a
    /// second undo of it finds nothing to do.
    async fn undo_held(
        &self,
        item: ItemId,
        entry_id: i64,
        left: Vec<TitleUndo>,
    ) -> Result<(ArtworkOutcome, UndoReport), ArtworkError> {
        let _title = self.hold(item).await?;
        // Read again under the hold: an undo that ran meanwhile has used the entry up.
        let record = load_record(&self.db, entry_id)?;
        let _permit = self
            .gate
            .acquire()
            .await
            .map_err(|_| ArtworkError::Worker)?;
        let loaded = self.load_any(record.item).await?;
        if loaded.target.dir != record.dir || loaded.target.base != record.base {
            return Err(ArtworkError::UndoStale(
                "The title was renamed or moved since, so its artwork was not changed back.                  The earlier image is in the trash folder."
                    .to_owned(),
            ));
        }
        let db = Arc::clone(&self.db);
        let target = loaded.target.clone();
        let item = record.item;
        let outcome = tokio::task::spawn_blocking(move || apply(&db, &target, &record))
            .await
            .map_err(|_| ArtworkError::Worker)??;
        let report = HistoryService::new(Arc::clone(&self.db))
            .finish_undo(entry_id, vec![item], left)
            .map_err(|error| match error {
                HistoryError::Db(db) => ArtworkError::Db(db),
                _ => ArtworkError::Worker,
            })?;
        self.changes
            .publish_one(item, &[ChangePart::Artwork, ChangePart::Files]);
        Ok((outcome, report))
    }
}

fn apply(
    db: &cv_db::Database,
    target: &ArtworkTarget,
    record: &PlaceUndo,
) -> Result<ArtworkOutcome, ArtworkError> {
    let trash = Trash::new(target.data_source.as_deref(), &target.root);
    let mut problems = Vec::new();
    let mut trashed = 0;

    // 1. The new files leave first, so an old file can come back to the same name.
    for file in record.written.iter().filter(|f| f.is_file()) {
        match trash.put(file) {
            Ok(_) => trashed += 1,
            Err(error) => problems.push(format!("{}: {error}", file.display())),
        }
    }
    // 2. The displaced files return, last moved first.
    let mut restored = 0;
    for (original, in_trash) in record.moves.iter().rev() {
        if !in_trash.is_file() {
            problems.push(format!(
                "{}: the copy in the trash is gone",
                original.display()
            ));
            continue;
        }
        if original.exists() {
            // Something else took the name meanwhile: it is moved aside, not overwritten.
            match trash.put(original) {
                Ok(_) => trashed += 1,
                Err(error) => {
                    problems.push(format!("{}: {error}", original.display()));
                    continue;
                }
            }
        }
        match move_back(in_trash, original) {
            Ok(()) => restored += 1,
            Err(error) => problems.push(format!("{}: {error}", original.display())),
        }
    }

    // 3. The library goes back to the earlier file list and sources. A file that did not come
    //    back is not listed.
    let item_id = record.item;
    let kind = record.kind;
    let record = record.clone();
    db.write(move |tx| {
        let Some(mut item) = get_item(tx, record.item)? else {
            return Err(cv_db::DbError::ItemNotFound(record.item.0));
        };
        restore_item(&mut item, &record);
        update_item(tx, &item)?;
        let refs: Vec<ArtworkRef> = record.refs_before.iter().map(ArtworkRef::from).collect();
        if matches!(
            record.kind,
            MediaFileType::Extrafanart | MediaFileType::Extrathumb
        ) {
            replace_extra_refs(tx, record.item, record.kind, &refs)?;
        } else {
            match refs.iter().find(|r| r.position == 0) {
                Some(before) => set_artwork_ref(tx, record.item, before)?,
                None => delete_artwork_ref(tx, record.item, record.kind, 0)?,
            }
        }
        Ok(())
    })?;
    tracing::info!(
        item = item_id.0,
        ?kind,
        restored,
        "artwork placement undone"
    );

    let mut outcome = ArtworkOutcome::nothing(item_id);
    outcome.images = restored;
    outcome.trashed = trashed;
    outcome.problems = problems;
    Ok(outcome)
}

fn restore_item(item: &mut Item, record: &PlaceUndo) {
    item.core.media_files.retain(|f| f.kind != record.kind);
    item.core.media_files.extend(
        record
            .files_before
            .iter()
            .filter(|f| f.path().exists())
            .cloned(),
    );
    match &record.url_before {
        Some(url) => {
            item.core.artwork_urls.insert(record.kind, url.clone());
        }
        None => {
            item.core.artwork_urls.remove(&record.kind);
        }
    }
}

fn move_back(from: &Path, to: &Path) -> std::io::Result<()> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    // Retried while another program holds a file for a moment; across volumes a flushed copy
    // comes first and the trash copy goes only once the original is back.
    cv_fs::move_file(from, to)
}
