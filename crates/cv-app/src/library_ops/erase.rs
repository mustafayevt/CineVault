//! Permanent delete: Shift+Delete with the typed confirmation (spec `title-deletion` "Shift+Delete
//! asks for a typed confirmation", "Permanent delete erases only what it listed").
//!
//! The one place CineVault erases user files for good, so every guard is here again, whatever
//! the caller checked:
//!
//! 1. the caller passes the literal confirmation [`ERASE_CONFIRMATION`] the person typed;
//! 2. a title on a drive that is offline, a path outside its library folder, a path that is a
//!    library folder's root or holds one, and a folder other titles share (only the title's own
//!    files go, with the files named after its videos) are refused;
//! 3. the History entry is written first, with every path it is about to erase;
//! 4. titles are erased one by one, file by file, with the short retries of `cv-fs`; a title stops
//!    at its first file that cannot be erased and keeps the rest, the other titles go on;
//! 5. the entry then lists what was erased and what was left, and only titles erased whole leave
//!    the library (the one user-confirmed exception to "never delete library rows").

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use cv_db::repo::data_sources::list_sources;
use cv_db::repo::history::{TitleState, set_detail, set_title_state};
use cv_domain::ItemId;

use super::move_plan::{EntryKind, enumerate};
use super::removal::{Loaded, label, paths_to_trash, row_of};
use super::{FolderUse, LibraryOpsError, Problem, RemovalService, parts_below};
use crate::history::{HistoryKind, NewEntry, NewTitle, record_in, titles_word};

/// What the person types to confirm a permanent delete.
pub const ERASE_CONFIRMATION: &str = "DELETE";

/// Erases files and empty folders. A trait so tests can make a file "in use".
pub trait EraseFs: Send + Sync {
    /// Removes one file or link.
    ///
    /// # Errors
    /// The system's reason.
    fn remove_file(&self, path: &Path) -> io::Result<()>;

    /// Removes one empty folder.
    ///
    /// # Errors
    /// The system's reason.
    fn remove_dir(&self, path: &Path) -> io::Result<()>;
}

/// The real disk, with the short retries of `cv-fs` for a file another program holds a moment.
#[derive(Debug, Clone, Copy, Default)]
pub struct StdErase;

impl EraseFs for StdErase {
    fn remove_file(&self, path: &Path) -> io::Result<()> {
        cv_fs::retry("erase", path, || match std::fs::remove_file(path) {
            // A link to a folder is removed as a folder on Windows.
            Err(e) if cfg!(windows) && path.is_symlink() => std::fs::remove_dir(path).or(Err(e)),
            other => other,
        })
    }

    fn remove_dir(&self, path: &Path) -> io::Result<()> {
        cv_fs::retry_folder("erase", path, || std::fs::remove_dir(path))
    }
}

/// One title of a permanent delete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErasedTitle {
    pub id: ItemId,
    pub title: String,
    /// Files, links and folders erased.
    pub erased: Vec<String>,
    /// What was not erased because the title stopped at `error`.
    pub left: Vec<String>,
    /// The file the title stopped at, with the reason.
    pub error: Option<String>,
}

/// The answer of a permanent delete.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EraseReport {
    /// Titles erased whole; they left the library.
    pub erased: Vec<ErasedTitle>,
    /// Titles that stopped at a file; they stay in the library with what is left.
    pub stopped: Vec<ErasedTitle>,
    /// Titles refused before anything was erased, with the reason.
    pub problems: Vec<Problem>,
    /// The History entry (without Undo).
    pub history_id: Option<i64>,
}

/// A title with the paths it is about to lose.
struct Planned<'a> {
    entry: &'a Loaded,
    label: String,
    folder: String,
    paths: Vec<PathBuf>,
    bytes: u64,
}

impl RemovalService {
    /// Replaces the disk a permanent delete erases on (tests make a file "in use").
    #[must_use]
    pub fn with_eraser(mut self, eraser: Arc<dyn EraseFs>) -> Self {
        self.eraser = eraser;
        self
    }

    /// Erases the titles' files for good (see the module documentation). `confirm` must be the
    /// literal [`ERASE_CONFIRMATION`] the person typed.
    ///
    /// # Errors
    /// The confirmation is not exactly `DELETE` (nothing is touched), or the library cannot be
    /// read or written.
    pub fn erase(&self, ids: &[ItemId], confirm: &str) -> Result<EraseReport, LibraryOpsError> {
        if confirm != ERASE_CONFIRMATION {
            return Err(LibraryOpsError::rejected(
                "confirmation",
                "Type DELETE to delete permanently. Nothing was deleted.",
            ));
        }
        // Held for the whole erase, so no NFO or artwork write lands in a folder that is going
        // (it would be left behind, or stop the title at a folder that is no longer empty).
        let (_held, ids, mut busy) = self.hold_titles(ids)?;
        let (loaded, mut skipped) = self.load(&ids)?;
        skipped.append(&mut busy);
        let roots: Vec<String> = self
            .db
            .read(|conn| list_sources(conn, None))?
            .into_iter()
            .map(|s| s.path)
            .collect();
        let mut report = EraseReport {
            problems: skipped,
            ..EraseReport::default()
        };
        let mut planned = Vec::new();
        for entry in &loaded {
            match plan_title(entry, &roots) {
                Ok(plan) => planned.push(plan),
                Err(reason) => report.problems.push(Problem {
                    id: entry.item.id.unwrap_or(ItemId(0)),
                    title: entry.item.core.title.clone(),
                    reason,
                }),
            }
        }
        if planned.is_empty() {
            return Ok(report);
        }

        // The record comes first, with every path about to go.
        let history = erase_entry(&planned);
        let entry_id = self
            .db
            .write(move |tx| record_in(tx, &history, super::now_millis()))?;
        report.history_id = Some(entry_id);

        let mut outcomes = Vec::new();
        for plan in &planned {
            let id = plan.entry.item.id.unwrap_or(ItemId(0));
            let (erased, left, error) = self.erase_title(plan);
            let title = ErasedTitle {
                id,
                title: plan.label.clone(),
                erased,
                left,
                error,
            };
            if title.error.is_none() {
                // Erased whole: the title leaves the library (the person confirmed it).
                self.forget(plan.entry)?;
                report.erased.push(title.clone());
            } else {
                self.db.write(move |tx| {
                    set_title_state(tx, entry_id, id.0, TitleState::Left)?;
                    Ok(())
                })?;
                report.stopped.push(title.clone());
            }
            outcomes.push(title);
        }
        let detail = outcome_detail(&planned, &outcomes);
        self.db.write(move |tx| set_detail(tx, entry_id, &detail))?;
        Ok(report)
    }

    /// Erases one title file by file, deepest folders last; stops at the first failure.
    fn erase_title(&self, plan: &Planned<'_>) -> (Vec<String>, Vec<String>, Option<String>) {
        let mut files: Vec<PathBuf> = Vec::new();
        let mut dirs: Vec<PathBuf> = Vec::new();
        for path in &plan.paths {
            let is_dir = path
                .symlink_metadata()
                .is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink());
            if is_dir {
                for entry in enumerate(path).unwrap_or_default() {
                    let full = path.join(&entry.path);
                    match entry.kind {
                        EntryKind::Dir => dirs.push(full),
                        EntryKind::File | EntryKind::Link { .. } => files.push(full),
                    }
                }
                dirs.push(path.clone());
            } else {
                files.push(path.clone());
            }
        }
        // Deepest first, so each folder is empty when its turn comes.
        dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
        let text = |p: &Path| p.to_string_lossy().into_owned();
        let mut erased = Vec::new();
        let steps: Vec<(PathBuf, bool)> = files
            .into_iter()
            .map(|f| (f, false))
            .chain(dirs.into_iter().map(|d| (d, true)))
            .collect();
        for (index, (path, is_dir)) in steps.iter().enumerate() {
            let result = if *is_dir {
                self.eraser.remove_dir(path)
            } else {
                self.eraser.remove_file(path)
            };
            match result {
                Ok(()) => erased.push(text(path)),
                Err(error) if error.kind() == io::ErrorKind::NotFound => erased.push(text(path)),
                Err(error) => {
                    let left = steps[index..].iter().map(|(p, _)| text(p)).collect();
                    let in_use = if cfg!(windows) {
                        matches!(error.raw_os_error(), Some(32 | 33))
                    } else {
                        matches!(error.raw_os_error(), Some(16 | 26))
                    };
                    let why = if in_use {
                        format!("{} is in use by another program", path.display())
                    } else {
                        format!("{} could not be erased: {error}", path.display())
                    };
                    return (erased, left, Some(why));
                }
            }
        }
        (erased, Vec::new(), None)
    }
}

/// What a title would lose, or why it is refused.
fn plan_title<'a>(entry: &'a Loaded, roots: &[String]) -> Result<Planned<'a>, String> {
    let row = row_of(entry);
    let source = &row.source;
    if source.is_empty() || !Path::new(source).is_dir() {
        return Err(format!(
            "The drive of {source} is offline, so nothing was deleted."
        ));
    }
    let paths = paths_to_trash(entry, &row)?;
    for path in &paths {
        let text = path.to_string_lossy();
        if !parts_below(source, &text).is_some_and(|parts| !parts.is_empty()) {
            return Err(format!(
                "{text} is not inside its library folder, so nothing was deleted."
            ));
        }
        // Never a library folder's root, nor a folder that holds one.
        if roots.iter().any(|root| parts_below(&text, root).is_some()) {
            return Err(format!(
                "{text} is or holds a library folder, so nothing was deleted."
            ));
        }
    }
    let bytes = paths.iter().map(|p| super::trash_bin::size_of(p)).sum();
    let folder = match row.folder_use {
        FolderUse::Own => row.folder.clone(),
        _ => paths
            .first()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
    };
    Ok(Planned {
        entry,
        label: label(&row),
        folder,
        paths,
        bytes,
    })
}

/// The History entry written before anything is erased.
fn erase_entry(planned: &[Planned<'_>]) -> NewEntry {
    let what = match planned {
        [one] => one.label.clone(),
        many => titles_word(many.len()),
    };
    NewEntry {
        kind: HistoryKind::DeletePermanent,
        summary: format!("Permanently deleted {what}"),
        line: match planned {
            [one] => one.folder.clone(),
            many => titles_word(many.len()),
        },
        bytes: Some(planned.iter().map(|p| p.bytes).sum()),
        extra: serde_json::json!({
            "planned": planned
                .iter()
                .map(|p| serde_json::json!({
                    "item_id": p.entry.item.id.map_or(0, |id| id.0),
                    "paths": p.paths.iter().map(|x| x.to_string_lossy()).collect::<Vec<_>>(),
                }))
                .collect::<Vec<_>>(),
        }),
        batch_ref: None,
        titles: planned
            .iter()
            .map(|p| NewTitle {
                item_id: p.entry.item.id.unwrap_or(ItemId(0)),
                title: p.label.clone(),
                before: p.folder.clone(),
                after: String::new(),
            })
            .collect(),
    }
}

/// The entry's detail after the erase: the plan plus what each title lost and kept.
fn outcome_detail(planned: &[Planned<'_>], outcomes: &[ErasedTitle]) -> String {
    let mut entry = erase_entry(planned);
    entry.extra["outcome"] = serde_json::json!(
        outcomes
            .iter()
            .map(|o| serde_json::json!({
                "item_id": o.id.0,
                "erased": o.erased,
                "left": o.left,
                "error": o.error,
            }))
            .collect::<Vec<_>>()
    );
    if outcomes.iter().any(|o| o.error.is_some()) {
        let stopped = outcomes.iter().filter(|o| o.error.is_some()).count();
        entry.line = format!("{} · {} not erased", entry.line, titles_word(stopped));
    }
    entry.detail_json()
}
