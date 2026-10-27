//! Remove titles from the library, with or without their files.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use cv_db::Database;
use cv_db::repo::delete_tree;
use cv_db::repo::fingerprint::delete_fingerprints;
use cv_db::repo::get_item;
use cv_db::repo::library_ops::{
    OwnedFile, delete_fingerprints_under, files_of_items, hide_items, is_hidden,
    other_items_in_folder, tree_ids,
};
use cv_db::repo::rename_journal::owner_of_file;
use cv_domain::{Item, ItemId};
use cv_rename::exec::new_batch_id;
use cv_scan::skip::MARKER_IGNORE;

use super::trash_bin::{BinError, Binned, BinnedItem, FOLDER_PREFIX, OsTrash, SystemBin};
use super::{LibraryOpsError, TitleKind, parts_below, unique};
use crate::history::{HistoryKind, NewEntry, NewTitle, record_in, titles_word};
use crate::title_guard::{self, TitleGuard, Work};

/// What a title has in its folder, which decides what can be done with the folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderUse {
    /// The title has the folder to itself. It can be ignored by a scan and trashed as a whole.
    Own,
    /// Other titles keep files in the folder. Only the title's own files are touched.
    Shared,
    /// The files sit directly in the root of the data source. Only the files are touched.
    SourceRoot,
    /// The title has no folder of its own (a season or an episode), or it is outside its source.
    NoFolder,
}

/// One title of the preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovalRow {
    pub id: ItemId,
    pub kind: TitleKind,
    pub title: String,
    pub year: u16,
    /// The data source the title is in.
    pub source: String,
    pub folder: String,
    pub folder_use: FolderUse,
    /// Files the library knows for the title and everything below it.
    pub files: u32,
    pub bytes: u64,
    /// Items below the title (seasons and episodes of a show).
    pub children: u32,
    /// The folder exists on disk right now.
    pub on_disk: bool,
}

/// A title an operation did not handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub id: ItemId,
    pub title: String,
    pub reason: String,
}

/// What the preview of a removal says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovalPlan {
    pub rows: Vec<RemovalRow>,
    /// Titles that cannot be removed this way, with the reason.
    pub skipped: Vec<Problem>,
}

/// What happened to the `.cinevaultignore` marker of a removed title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IgnoreOutcome {
    NotRequested,
    Written,
    /// Not written because the folder is not the title's own; the text says why.
    Skipped(String),
}

/// A title taken out of the library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovedTitle {
    pub id: ItemId,
    pub title: String,
    pub ignore: IgnoreOutcome,
}

/// The answer of "remove from library".
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RemoveReport {
    pub removed: Vec<RemovedTitle>,
    pub problems: Vec<Problem>,
}

/// Where the files of a deleted title went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteMethod {
    /// The system trash (Recycle Bin, Trash).
    SystemTrash,
    /// `<data source>/.deletedByCineVault/<batch>/`, because the system trash could not take them.
    DataSourceTrash,
    /// The files were not on disk any more.
    NothingOnDisk,
    /// Erased at once (Settings › Deleted files is Delete permanently).
    Erased,
}

/// A title deleted with its files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletedTitle {
    pub id: ItemId,
    pub title: String,
    pub method: DeleteMethod,
    /// Files and folders sent to the trash.
    pub paths: u32,
    pub notes: Vec<String>,
}

/// The answer of "delete with files".
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeleteReport {
    pub deleted: Vec<DeletedTitle>,
    /// Titles left in the library with their files, each with the reason.
    pub problems: Vec<Problem>,
    /// No longer set: a delete is undone through its History entry ([`Self::history_id`]).
    pub batch_id: Option<String>,
    /// The History entry of the delete, which Undo and Restore take back.
    pub history_id: Option<i64>,
}

/// Removes titles from the library and, on request, sends their files to the bin.
#[derive(Clone)]
pub struct RemovalService {
    pub(super) db: Arc<Database>,
    pub(super) bin: Arc<dyn SystemBin>,
    pub(super) eraser: Arc<dyn super::erase::EraseFs>,
    /// Settings › Deleted files: with Delete permanently, a delete is a permanent delete.
    pub(super) choice: Arc<dyn crate::deleted_files::DeletedFilesSource>,
}

pub(super) struct Loaded {
    pub(super) item: Item,
    pub(super) kind: TitleKind,
    pub(super) tree: Vec<ItemId>,
    pub(super) files: Vec<OwnedFile>,
    pub(super) shared: bool,
}

impl RemovalService {
    /// A service that sends files to the real Recycle Bin / Trash.
    #[must_use]
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            db,
            bin: Arc::new(OsTrash),
            eraser: Arc::new(super::erase::StdErase),
            choice: crate::deleted_files::default_source(),
        }
    }

    /// Reads Settings › Deleted files from `choice` at every delete.
    #[must_use]
    pub fn with_choice(
        mut self,
        choice: Arc<dyn crate::deleted_files::DeletedFilesSource>,
    ) -> Self {
        self.choice = choice;
        self
    }

    /// Replaces the system bin (tests use a folder).
    #[must_use]
    pub fn with_bin(mut self, bin: Arc<dyn SystemBin>) -> Self {
        self.bin = bin;
        self
    }

    /// Looks at the titles and says what a removal would touch. Nothing is changed. A title
    /// that sits inside another selected title (an episode of a selected show) is left out.
    ///
    /// # Errors
    /// The library cannot be read.
    pub fn plan(&self, ids: &[ItemId]) -> Result<RemovalPlan, LibraryOpsError> {
        let (loaded, skipped) = self.load(ids)?;
        let rows = loaded.iter().map(row_of).collect();
        Ok(RemovalPlan { rows, skipped })
    }

    /// Takes the titles out of the library. Their files stay on disk, so the next scan adds them
    /// again, unless `ignore_folders` is set: then each title that has its folder to itself gets
    /// a `.cinevaultignore` marker, and scans skip that folder. A marker that cannot be written
    /// leaves its title in the library.
    ///
    /// # Errors
    /// The library cannot be read or written.
    pub fn remove_from_library(
        &self,
        ids: &[ItemId],
        ignore_folders: bool,
    ) -> Result<RemoveReport, LibraryOpsError> {
        let (loaded, skipped) = self.load(ids)?;
        let mut report = RemoveReport {
            problems: skipped,
            ..RemoveReport::default()
        };
        for entry in &loaded {
            let row = row_of(entry);
            let ignore = if !ignore_folders {
                IgnoreOutcome::NotRequested
            } else if row.folder_use == FolderUse::Own && row.on_disk {
                match write_marker(Path::new(&row.folder)) {
                    Ok(()) => IgnoreOutcome::Written,
                    Err(error) => {
                        report.problems.push(Problem {
                            id: row.id,
                            title: row.title.clone(),
                            reason: format!(
                                "The ignore marker could not be written in {}: {error}. \
                                 The title stays in the library.",
                                row.folder
                            ),
                        });
                        continue;
                    }
                }
            } else {
                IgnoreOutcome::Skipped(skip_reason(&row))
            };
            self.forget(entry)?;
            report.removed.push(RemovedTitle {
                id: row.id,
                title: row.title,
                ignore,
            });
        }
        Ok(report)
    }

    /// Sends the files of the titles to the Recycle Bin / Trash and hides the titles (spec
    /// `title-deletion`). A title with its own folder goes as that folder; a title
    /// that shares its folder with other titles, or sits in the root of its library folder,
    /// sends only its own files. A path the bin cannot take (a network or removable drive, a
    /// drive whose bin is off, an item larger than the bin) or refuses goes to
    /// the library folder's trash folder instead. Nothing is erased.
    ///
    /// The titles are hidden, not removed: their rows keep every detail, artwork row and stage,
    /// so the delete's History entry (one for the whole call) brings them back with their files.
    /// A title on a drive that is offline, or whose files could not be sent, stays as it is.
    ///
    /// # Errors
    /// The library cannot be read or written.
    ///
    /// With Settings › Deleted files set to Delete permanently (chosen after the same typed
    /// confirmation as Shift+Delete), the delete is a permanent delete instead: the files are
    /// erased, the titles leave the library and the History entry has no Undo.
    pub fn delete_with_files(&self, ids: &[ItemId]) -> Result<DeleteReport, LibraryOpsError> {
        if self.choice.deleted_files().erases() {
            let erased = self.erase(ids, super::erase::ERASE_CONFIRMATION)?;
            return Ok(DeleteReport {
                deleted: erased
                    .erased
                    .iter()
                    .map(|title| DeletedTitle {
                        id: title.id,
                        title: title.title.clone(),
                        method: DeleteMethod::Erased,
                        paths: u32::try_from(title.erased.len()).unwrap_or(u32::MAX),
                        notes: Vec::new(),
                    })
                    .collect(),
                problems: erased
                    .problems
                    .into_iter()
                    .chain(erased.stopped.into_iter().map(|title| Problem {
                        id: title.id,
                        title: title.title,
                        reason: title.error.unwrap_or_default(),
                    }))
                    .collect(),
                batch_id: None,
                history_id: erased.history_id,
            });
        }
        // The titles are held while their files go: an NFO or artwork write that runs now would
        // land in a folder that is leaving, or put a file back where Undo must restore one.
        let (_held, ids, mut busy) = self.hold_titles(ids)?;
        let (loaded, mut skipped) = self.load(&ids)?;
        skipped.append(&mut busy);
        let mut report = DeleteReport {
            problems: skipped,
            ..DeleteReport::default()
        };
        let batch = new_batch_id();
        let mut sent: Vec<SentTitle> = Vec::new();
        for entry in &loaded {
            let row = row_of(entry);
            match self.bin_files(entry, &row, &batch) {
                Ok(Binned { items, notes }) => {
                    report.deleted.push(DeletedTitle {
                        id: row.id,
                        title: row.title.clone(),
                        method: method_of(&items),
                        paths: u32::try_from(items.len()).unwrap_or(u32::MAX),
                        notes,
                    });
                    sent.push(SentTitle {
                        row,
                        tree: entry.tree.clone(),
                        items,
                        hide: true,
                    });
                }
                Err(BinError {
                    sent: items,
                    reason,
                }) => {
                    report.problems.push(Problem {
                        id: row.id,
                        title: row.title.clone(),
                        reason,
                    });
                    // What went before the failure is in the entry too, so Undo puts it back.
                    if !items.is_empty() {
                        sent.push(SentTitle {
                            row,
                            tree: entry.tree.clone(),
                            items,
                            hide: false,
                        });
                    }
                }
            }
        }
        if sent.is_empty() {
            return Ok(report);
        }
        let entry = delete_entry(&sent);
        let hide: Vec<ItemId> = sent
            .iter()
            .filter(|title| title.hide)
            .flat_map(|title| title.tree.iter().copied())
            .collect();
        let id = self.db.write(move |tx| {
            let id = record_in(tx, &entry, super::now_millis())?;
            hide_items(tx, &hide, id)?;
            Ok(id)
        })?;
        report.history_id = Some(id);
        Ok(report)
    }

    /// Sends the title's files to the bin. `Err` is the reason the title must stay, with what
    /// was sent before it.
    fn bin_files(&self, entry: &Loaded, row: &RemovalRow, batch: &str) -> Result<Binned, BinError> {
        let refuse = |reason: String| BinError {
            sent: Vec::new(),
            reason,
        };
        let source = &row.source;
        if source.is_empty() || !Path::new(source).is_dir() {
            return Err(refuse(format!(
                "The drive of {source} is offline, so nothing was deleted."
            )));
        }
        let paths = paths_to_trash(entry, row).map_err(refuse)?;
        if paths.is_empty() {
            return Ok(Binned::default());
        }
        super::trash_bin::send_or_fallback(self.bin.as_ref(), &paths, Path::new(source), batch)
    }

    /// Holds the titles of `ids` (a season or an episode stands for its show) for a delete,
    /// waiting a while for writes into them to finish. Returns the guard, the ids whose title is
    /// held, and a problem for each id whose title is still busy (nothing of it is touched).
    pub(super) fn hold_titles(
        &self,
        ids: &[ItemId],
    ) -> Result<(TitleGuard, Vec<ItemId>, Vec<Problem>), LibraryOpsError> {
        let ids = unique(ids);
        let mut titles = Vec::with_capacity(ids.len());
        for &id in &ids {
            titles.push((id, title_guard::title_of(&self.db, id)?));
        }
        let (guard, busy) = title_guard::hold_each(
            &self.db,
            titles.iter().map(|(_, title)| *title),
            Work::Delete,
            title_guard::MOVE_WAIT,
        );
        let mut held = Vec::new();
        let mut problems = Vec::new();
        for (id, title) in titles {
            match busy.iter().find(|(t, _)| *t == title) {
                None => held.push(id),
                Some((_, why)) => {
                    let name = self
                        .db
                        .read(move |conn| get_item(conn, id))?
                        .map(|item| item.core.title)
                        .unwrap_or_default();
                    problems.push(Problem {
                        id,
                        title: name,
                        reason: format!("{why} Nothing was deleted."),
                    });
                }
            }
        }
        Ok((guard, held, problems))
    }

    /// The titles to work on, without repeats, sets or titles inside another selected title.
    /// A hidden title (deleted to the bin) is no longer in the library.
    pub(super) fn load(
        &self,
        ids: &[ItemId],
    ) -> Result<(Vec<Loaded>, Vec<Problem>), LibraryOpsError> {
        let ids = unique(ids);
        Ok(self.db.read(move |conn| {
            let mut skipped = Vec::new();
            let mut loaded: Vec<Loaded> = Vec::new();
            for id in ids {
                let item = if is_hidden(conn, id)? {
                    None
                } else {
                    get_item(conn, id)?
                };
                let Some(item) = item else {
                    skipped.push(Problem {
                        id,
                        title: String::new(),
                        reason: "This title is no longer in the library.".to_owned(),
                    });
                    continue;
                };
                let Some(kind) = TitleKind::of(&item) else {
                    skipped.push(Problem {
                        id,
                        title: item.core.title.clone(),
                        reason: "A collection is managed on its own page.".to_owned(),
                    });
                    continue;
                };
                let tree = tree_ids(conn, id)?;
                let files = files_of_items(conn, &tree)?;
                let shared = kind.owns_folder()
                    && !item.core.path.is_empty()
                    && !other_items_in_folder(conn, id, &item.core.path)?.is_empty();
                loaded.push(Loaded {
                    item,
                    kind,
                    tree,
                    files,
                    shared,
                });
            }
            // An episode of a selected show goes with the show.
            let covered: BTreeSet<ItemId> = loaded
                .iter()
                .flat_map(|entry| entry.tree.iter().skip(1).copied())
                .collect();
            loaded.retain(|entry| entry.item.id.is_none_or(|id| !covered.contains(&id)));
            Ok((loaded, skipped))
        })?)
    }

    /// Takes the title and everything below it out of the library, and the scanner's memory of
    /// its files, so a scan sees them as new.
    pub(super) fn forget(&self, entry: &Loaded) -> Result<(), LibraryOpsError> {
        let Some(id) = entry.item.id else {
            return Ok(());
        };
        let files: Vec<String> = entry.files.iter().map(OwnedFile::path).collect();
        let folder = entry
            .kind
            .owns_folder()
            .then(|| entry.item.core.path.clone())
            .filter(|folder| !folder.is_empty() && !entry.shared);
        let source = entry.item.core.data_source.clone();
        self.db.write(move |tx| {
            delete_tree(tx, id)?;
            // A file another title still records (a duplicate row of the same folder) keeps its
            // fingerprint, or the next scan would take that title's files for new ones.
            let unowned: Vec<String> = files
                .into_iter()
                .filter(|path| owner_of_file(tx, path).ok().flatten().is_none())
                .collect();
            delete_fingerprints(tx, &unowned)?;
            if let Some(folder) = folder
                && parts_below(&source, &folder).is_some_and(|parts| !parts.is_empty())
            {
                delete_fingerprints_under(tx, &folder)?;
            }
            Ok(())
        })?;
        Ok(())
    }
}

/// A title a delete sent (some of) its files of to the bin.
struct SentTitle {
    row: RemovalRow,
    tree: Vec<ItemId>,
    items: Vec<BinnedItem>,
    /// Every file went: the title is hidden. Otherwise it stays, and only what went is recorded.
    hide: bool,
}

/// Where the items of a title went.
fn method_of(items: &[BinnedItem]) -> DeleteMethod {
    if items.is_empty() {
        DeleteMethod::NothingOnDisk
    } else if items.iter().all(|i| i.bin_id.starts_with(FOLDER_PREFIX)) {
        DeleteMethod::DataSourceTrash
    } else {
        DeleteMethod::SystemTrash
    }
}

/// The name of the system bin in a summary.
pub(crate) const fn bin_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "Trash"
    } else {
        "Recycle Bin"
    }
}

/// "Godzilla (1998)".
pub(super) fn label(row: &RemovalRow) -> String {
    if row.year > 0 {
        format!("{} ({})", row.title, row.year)
    } else {
        row.title.clone()
    }
}

/// The History entry of a delete: "Deleted Godzilla (1998) to the Recycle Bin", the bytes, and
/// every binned item per title (what Restore needs).
fn delete_entry(sent: &[SentTitle]) -> NewEntry {
    let all: Vec<&BinnedItem> = sent.iter().flat_map(|title| &title.items).collect();
    let place = if !all.is_empty() && all.iter().all(|i| i.bin_id.starts_with(FOLDER_PREFIX)) {
        "the trash folder".to_owned()
    } else {
        format!("the {}", bin_name())
    };
    let what = match sent {
        [one] => label(&one.row),
        many => titles_word(many.len()),
    };
    let bytes: u64 = all.iter().map(|i| i.size).sum();
    NewEntry {
        kind: HistoryKind::Delete,
        summary: format!("Deleted {what} to {place}"),
        line: match sent {
            [one] => one.row.folder.clone(),
            many => titles_word(many.len()),
        },
        bytes: Some(bytes),
        extra: serde_json::json!({
            "binned": sent
                .iter()
                .map(|t| serde_json::json!({ "item_id": t.row.id.0, "items": t.items }))
                .collect::<Vec<_>>(),
        }),
        batch_ref: None,
        titles: sent
            .iter()
            .map(|t| NewTitle {
                item_id: t.row.id,
                title: label(&t.row),
                before: t.row.folder.clone(),
                after: place.clone(),
            })
            .collect(),
    }
}

pub(super) fn row_of(entry: &Loaded) -> RemovalRow {
    let core = &entry.item.core;
    let folder_use = if !entry.kind.owns_folder() || core.path.is_empty() {
        FolderUse::NoFolder
    } else {
        match parts_below(&core.data_source, &core.path) {
            None => FolderUse::NoFolder,
            Some(parts) if parts.is_empty() => FolderUse::SourceRoot,
            Some(_) if entry.shared => FolderUse::Shared,
            Some(_) => FolderUse::Own,
        }
    };
    RemovalRow {
        id: entry.item.id.unwrap_or(ItemId(0)),
        kind: entry.kind,
        title: core.title.clone(),
        year: core.year,
        source: core.data_source.clone(),
        folder: core.path.clone(),
        folder_use,
        files: u32::try_from(entry.files.len()).unwrap_or(u32::MAX),
        bytes: entry.files.iter().map(|file| file.size_bytes).sum(),
        children: u32::try_from(entry.tree.len().saturating_sub(1)).unwrap_or(u32::MAX),
        on_disk: !core.path.is_empty() && Path::new(&core.path).is_dir(),
    }
}

fn skip_reason(row: &RemovalRow) -> String {
    match row.folder_use {
        FolderUse::Own => "The folder was not found on disk.".to_owned(),
        FolderUse::Shared => "Other titles keep files in this folder.".to_owned(),
        FolderUse::SourceRoot => "The files are in the root of the data source.".to_owned(),
        FolderUse::NoFolder => "This title has no folder of its own.".to_owned(),
    }
}

/// The paths that go to the trash: the folder of a title that owns it, else its files that exist.
/// Every path must lie strictly inside the title's data source.
pub(super) fn paths_to_trash(entry: &Loaded, row: &RemovalRow) -> Result<Vec<PathBuf>, String> {
    let inside = |path: &str| parts_below(&row.source, path).is_some_and(|parts| !parts.is_empty());
    if row.folder_use == FolderUse::Own {
        if !row.on_disk {
            return Ok(Vec::new());
        }
        if !inside(&row.folder) {
            return Err(format!(
                "{} is not inside its data source, so nothing was deleted.",
                row.folder
            ));
        }
        return Ok(vec![PathBuf::from(&row.folder)]);
    }
    let mut out: Vec<PathBuf> = Vec::new();
    for file in &entry.files {
        let path = file.path();
        if !inside(&path) {
            return Err(format!(
                "{path} is not inside its data source, so nothing was deleted."
            ));
        }
        let path = PathBuf::from(path);
        if path.symlink_metadata().is_ok() && !out.contains(&path) {
            out.push(path);
        }
    }
    Ok(out)
}

/// Writes the marker that makes scans skip a folder.
fn write_marker(folder: &Path) -> std::io::Result<()> {
    let marker = folder.join(MARKER_IGNORE);
    if marker.exists() {
        return Ok(());
    }
    std::fs::write(
        marker,
        "CineVault skips this folder when it scans. Delete this file to scan it again.\n",
    )
}
