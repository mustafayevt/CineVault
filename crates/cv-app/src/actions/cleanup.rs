//! The standalone clean-up of unwanted files: the same file list the clean-up after a rename
//! uses (the unwanted-file patterns of the active naming profile), run on chosen titles at any
//! time.
//!
//! * **What is looked at.** The folder of a movie, and the folder of a show with its season
//!   folders (folders whose name starts with a dot, such as the trash folder, are left alone).
//!   Only files directly in those folders are considered, never the files in a deeper folder.
//!   A title whose files sit in the root of its data source has no folder of its own and is
//!   skipped, as is a title that is locked. A video the library knows is never unwanted, whatever
//!   the pattern says.
//! * **What happens.** The matching files move to `<data source>/.deletedByCineVault/<batch>/`
//!   with the path below the data source kept, through the same journal as Organize, so the batch
//!   shows in the Organize journal and can be undone. Nothing is deleted.
//! * **Dry run.** Lists what would move and changes nothing.
//! * **Off by default.** The clean-up switch of Organize (`renamer.cleanup-unwanted`, also in
//!   Settings, Files) or a custom profile's own decides, as it does for the rename: while it is off
//!   the action says where to turn it on.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use cv_db::Database;
use cv_db::repo::library_ops::tree_ids;
use cv_domain::{Item, ItemId, ItemKind, MediaFileType};
use cv_rename::exec::{
    DbJournal, ExecOptions, Executor, Fs, ItemStatus, StdFs, TrashRequest, new_batch_id,
    unwanted_files_in,
};

use super::{ActionRun, ActionsService, Blocker, ItemOutcome, OutcomeKind};
use crate::automation::selection::is_inside;
use crate::rename_settings::{CleanupChoices, RenameSettings, apply_cleanup_choices};

/// How many file names a message lists before it says "and N more".
const NAMES_SHOWN: usize = 4;

/// What one title's clean-up found.
struct Found {
    id: ItemId,
    item: Item,
    root: PathBuf,
    paths: Vec<PathBuf>,
}

impl ActionsService {
    /// Moves the unwanted files of the titles to the trash folder of their data source, or, with
    /// `dry_run`, lists what would move.
    pub async fn cleanup_unwanted(&self, ids: &[ItemId], dry_run: bool) -> ActionRun {
        let values = self.values();
        let mut profile = RenameSettings::from_values(&values).book.active();
        apply_cleanup_choices(&mut profile, &CleanupChoices::from_values(&values));
        let batch_id = new_batch_id();
        let movie_patterns = profile.movie.exec_options(&batch_id).unwanted_patterns;
        let show_patterns = profile.tv.exec_options(&batch_id).unwanted_patterns;
        if movie_patterns.is_empty() && show_patterns.is_empty() {
            return ActionRun {
                items: Vec::new(),
                blocker: Some(Blocker::Setup(
                    "Clean-up of unwanted files is off. Turn on \"Move unwanted files to the \
                     trash\" in Organize, under Clean-up, and choose the file types there."
                        .to_owned(),
                )),
            };
        }

        let mut run = ActionRun::default();
        let mut found: Vec<Found> = Vec::new();
        let mut seen_dirs: HashSet<String> = HashSet::new();
        for &id in ids {
            let item = match self.load(id).await {
                Ok(item) => item,
                Err(outcome) => {
                    run.items.push(outcome);
                    continue;
                }
            };
            let patterns = match item.kind() {
                ItemKind::Movie => &movie_patterns,
                ItemKind::Show => &show_patterns,
                _ => {
                    run.items.push(ItemOutcome::of(
                        &item,
                        OutcomeKind::Skipped,
                        "Only movies and shows have a folder to tidy.",
                    ));
                    continue;
                }
            };
            if patterns.is_empty() {
                let what = if item.kind() == ItemKind::Show {
                    "TV shows"
                } else {
                    "movies"
                };
                run.items.push(ItemOutcome::of(
                    &item,
                    OutcomeKind::Skipped,
                    format!(
                        "Clean-up is off for {what} in the naming profile \"{}\".",
                        profile.name
                    ),
                ));
                continue;
            }
            if item.core.locked {
                run.items.push(ItemOutcome::of(
                    &item,
                    OutcomeKind::Skipped,
                    "The title is locked, so its folder is left as it is.",
                ));
                continue;
            }
            let (source, folder) = (item.core.data_source.clone(), item.core.path.clone());
            if folder.is_empty() || !strictly_inside(&folder, &source) {
                run.items.push(ItemOutcome::of(
                    &item,
                    OutcomeKind::Skipped,
                    "The files are in the root of the data source, so there is no folder to tidy.",
                ));
                continue;
            }
            let db = Arc::clone(&self.db);
            let (patterns, show) = (patterns.clone(), item.kind() == ItemKind::Show);
            let looked = tokio::task::spawn_blocking(move || {
                find_unwanted(&db, id, &folder, show, &patterns)
            })
            .await;
            let paths = match looked {
                Ok(Ok(paths)) => paths,
                Ok(Err(message)) => {
                    run.items
                        .push(ItemOutcome::of(&item, OutcomeKind::Failed, message));
                    continue;
                }
                Err(_) => {
                    run.items.push(ItemOutcome::of(
                        &item,
                        OutcomeKind::Failed,
                        "The work stopped unexpectedly.",
                    ));
                    continue;
                }
            };
            // A folder shared by several titles is tidied once.
            let paths: Vec<PathBuf> = paths
                .into_iter()
                .filter(|path| seen_dirs.insert(cv_fs::paths::key(path)))
                .collect();
            if paths.is_empty() {
                run.items.push(ItemOutcome::of(
                    &item,
                    OutcomeKind::Unchanged,
                    "No unwanted files were found.",
                ));
                continue;
            }
            found.push(Found {
                id,
                item,
                root: PathBuf::from(source),
                paths,
            });
        }

        if dry_run {
            for entry in found {
                run.items.push(ItemOutcome::of(
                    &entry.item,
                    OutcomeKind::Done,
                    format!("Would move {} to the trash folder.", describe(&entry.paths)),
                ));
            }
            return run;
        }
        if found.is_empty() {
            return run;
        }

        let requests: Vec<TrashRequest> = found
            .iter()
            .map(|entry| TrashRequest {
                id: entry.id.0.to_string(),
                root: entry.root.clone(),
                paths: entry.paths.clone(),
            })
            .collect();
        let db = Arc::clone(&self.db);
        // Settings › Deleted files: a custom trash folder, or erased at once.
        let choice =
            crate::deleted_files::DeletedFiles::from_values(|key| values.get(key).cloned());
        let moved = tokio::task::spawn_blocking(move || {
            let mut journal = DbJournal::new(Arc::clone(&db));
            let fs = StdFs;
            let mut options = ExecOptions::new(batch_id.clone());
            if let Some(custom) = choice.custom_folder() {
                options.trash_dir = custom.to_string_lossy().into_owned();
            }
            let mut executor =
                Executor::new(&fs, &mut journal, options).map_err(|error| error.to_string())?;
            let report = executor
                .trash_paths(&requests)
                .map_err(|error| error.to_string())?;
            if choice.erases() {
                crate::trash_folders::erase_rename_leftovers(&db, &batch_id);
            }
            Ok::<_, String>(report)
        })
        .await;
        let report = match moved {
            Ok(Ok(report)) => report,
            Ok(Err(message)) => {
                for entry in &found {
                    run.items.push(ItemOutcome::of(
                        &entry.item,
                        OutcomeKind::Failed,
                        format!("The files could not be moved: {message}"),
                    ));
                }
                return run;
            }
            Err(_) => {
                for entry in &found {
                    run.items.push(ItemOutcome::of(
                        &entry.item,
                        OutcomeKind::Failed,
                        "The work stopped unexpectedly.",
                    ));
                }
                return run;
            }
        };
        for entry in &found {
            let status = report
                .items
                .iter()
                .find(|done| done.id == entry.id.0.to_string());
            let outcome = match status.map(|done| &done.status) {
                Some(ItemStatus::Done) => ItemOutcome::of(
                    &entry.item,
                    OutcomeKind::Done,
                    format!(
                        "Moved {} to the trash folder. Undo it from the Organize journal.",
                        describe(&entry.paths)
                    ),
                ),
                Some(ItemStatus::Failed(why)) => ItemOutcome::of(
                    &entry.item,
                    OutcomeKind::Failed,
                    format!("The files could not be moved: {why}"),
                ),
                Some(ItemStatus::Canceled) | None => {
                    ItemOutcome::of(&entry.item, OutcomeKind::Skipped, "Nothing was moved.")
                }
            };
            run.items.push(outcome);
        }
        run
    }
}

/// `child` is below `parent`, not the same folder.
fn strictly_inside(child: &str, parent: &str) -> bool {
    !parent.is_empty() && is_inside(child, parent) && !is_inside(parent, child)
}

/// "3 files (a.txt, b.url, c.html)", with at most [`NAMES_SHOWN`] names.
fn describe(paths: &[PathBuf]) -> String {
    let names: Vec<String> = paths
        .iter()
        .filter_map(|path| path.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect();
    let shown = names
        .iter()
        .take(NAMES_SHOWN)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    let more = names.len().saturating_sub(NAMES_SHOWN);
    let count = if names.len() == 1 {
        "1 file".to_owned()
    } else {
        format!("{} files", names.len())
    };
    if more > 0 {
        format!("{count} ({shown} and {more} more)")
    } else {
        format!("{count} ({shown})")
    }
}

/// The unwanted files of a title's folder: directly in the folder and, for a show, in its season
/// folders. A video the library holds for the title (or its episodes) is never in the list.
fn find_unwanted(
    db: &Database,
    id: ItemId,
    folder: &str,
    show: bool,
    patterns: &[String],
) -> Result<Vec<PathBuf>, String> {
    let fs = StdFs;
    let root = Path::new(folder);
    let mut dirs = vec![root.to_path_buf()];
    if show && let Ok(names) = fs.list_dir(root) {
        let mut names = names;
        names.sort();
        for name in names {
            let path = root.join(&name);
            let is_dir = fs
                .meta(&path)
                .ok()
                .flatten()
                .is_some_and(|meta| meta.is_dir);
            if is_dir && !name.starts_with('.') {
                dirs.push(path);
            }
        }
    }
    let protected = known_videos(db, id).map_err(|error| error.to_string())?;
    let mut found = Vec::new();
    for dir in dirs {
        let names = unwanted_files_in(&fs, &dir, patterns).map_err(|error| error.to_string())?;
        for name in names {
            let path = dir.join(name);
            if !protected.contains(&cv_fs::paths::key(&path)) {
                found.push(path);
            }
        }
    }
    Ok(found)
}

/// The comparison keys (`cv_fs::paths::key`: case folded except on Linux) of the paths of the
/// videos the library holds for the title and everything below it.
fn known_videos(db: &Database, id: ItemId) -> Result<HashSet<String>, cv_db::DbError> {
    db.read(|conn| {
        let mut videos = HashSet::new();
        for member in tree_ids(conn, id)? {
            if let Some(item) = cv_db::repo::get_item(conn, member)? {
                videos.extend(
                    item.core
                        .media_files
                        .iter()
                        .filter(|file| file.kind == MediaFileType::Video)
                        .map(|file| cv_fs::paths::key(&file.path())),
                );
            }
        }
        Ok(videos)
    })
}
