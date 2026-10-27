//! Moving whole folders between data sources ("change data source").
//!
//! [`Executor::move_folders`] takes folders whose destination is in another data source. Each
//! move is one journal row, written first, like a folder rename in a batch: the library follows
//! in the same transaction when the store is the [`DbJournal`](crate::exec::DbJournal) (the
//! folder, its files and the data source of the items), and an undo takes the folder back. On the
//! same volume the folder is renamed, so the move is instant; on another volume it is copied,
//! verified and then removed (see [`crate::exec`]'s `tree` module).

use std::path::{Path, PathBuf};

use super::{
    BatchReport, Ctx, ExecError, Executor, Fail, ItemReport, ItemStatus, Step, io_fail,
    io_fail_folder, move_path,
};
use crate::exec::fs::{Fs, is_within, same_path};
use crate::exec::tree::{self, Remnants};

/// A folder to move into another data source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderMove {
    /// The library id of the item (or any key unique in the batch).
    pub id: String,
    /// The data source root the folder is in now.
    pub from_root: PathBuf,
    /// The folder, absolute, below `from_root`.
    pub from: PathBuf,
    /// The data source root the folder goes to.
    pub to_root: PathBuf,
    /// Where the folder will be, absolute, below `to_root`. It must not exist or be empty.
    pub to: PathBuf,
}

impl Executor<'_> {
    /// Moves each folder to its destination, one item at a time. A folder that cannot be moved
    /// fails alone and stays where it was; the report holds the result of each, in order.
    ///
    /// # Errors
    /// Only a journal that cannot be written, which stops the batch at once.
    pub fn move_folders(&mut self, moves: &[FolderMove]) -> Result<BatchReport, ExecError> {
        let mut reports = Vec::with_capacity(moves.len());
        for item in moves {
            let canceled = self.cancel.is_some_and(|c| c());
            let report = if canceled {
                let mut r = ItemReport::new(&item.id);
                r.status = ItemStatus::Canceled;
                r
            } else {
                self.run_move(item)?
            };
            if let Some(progress) = self.progress.as_mut() {
                progress(&report);
            }
            reports.push(report);
        }
        Ok(BatchReport {
            batch_id: self.options.batch_id.clone(),
            items: reports,
        })
    }

    fn run_move(&mut self, item: &FolderMove) -> Result<ItemReport, ExecError> {
        let mut ctx = Ctx {
            root: item.from_root.clone(),
            item_id: Some(item.id.clone()),
            steps: Vec::new(),
            report: ItemReport::new(&item.id),
            left_half: false,
        };
        match self.execute_move(&mut ctx, item) {
            Ok(()) => {}
            Err(Fail::Journal(e)) => return Err(e.into()),
            Err(Fail::Op(message)) => {
                let notes = self.roll_back(&mut ctx)?;
                ctx.report.notes.extend(notes);
                ctx.report.status = ItemStatus::Failed(message);
            }
        }
        Ok(ctx.report)
    }

    fn execute_move(&mut self, ctx: &mut Ctx, item: &FolderMove) -> Step<()> {
        check_places(item)?;
        for root in [&item.from_root, &item.to_root] {
            let available = self
                .fs
                .meta(root)
                .ok()
                .flatten()
                .is_some_and(|meta| meta.is_dir);
            if !available {
                return Err(Fail::Op(format!(
                    "the data source {} is not available",
                    root.display()
                )));
            }
        }
        let src = self
            .fs
            .meta(&item.from)
            .map_err(|e| io_fail("cannot read", &item.from, &e))?;
        let dst = self
            .fs
            .meta(&item.to)
            .map_err(|e| io_fail("cannot read", &item.to, &e))?;
        let Some(src) = src else {
            // A run that stopped after the move: the folder is already there.
            return if dst.is_some_and(|d| d.is_dir) {
                ctx.report.already_applied += 1;
                Ok(())
            } else {
                Err(Fail::Op(format!(
                    "the folder {} is missing",
                    item.from.display()
                )))
            };
        };
        if !src.is_dir {
            return Err(Fail::Op(format!("{} is not a folder", item.from.display())));
        }
        if let Some(dst) = dst {
            if !dst.is_dir {
                return Err(Fail::Op(format!("{} is a file", item.to.display())));
            }
            let names = self
                .fs
                .list_dir(&item.to)
                .map_err(|e| io_fail("cannot list", &item.to, &e))?;
            if !names.is_empty() {
                return Err(Fail::Op(format!(
                    "{} already holds files; nothing was replaced",
                    item.to.display()
                )));
            }
            // An empty folder is in the way of nothing.
            self.fs
                .remove_dir(&item.to)
                .map_err(|e| io_fail("cannot replace", &item.to, &e))?;
        }

        let entry = self.entry(
            ctx,
            crate::exec::JournalOp::Move,
            &item.from,
            &item.to,
            true,
        );
        self.journal.begin(&entry)?;
        let remnants = match self.move_whole(&item.from, &item.to) {
            Ok(remnants) => remnants,
            Err(e) => {
                return Err(self.failed(&entry, io_fail_folder("cannot move", &item.from, &e)));
            }
        };
        self.done(ctx, entry, &item.to)?;
        ctx.report.moved += 1;
        for path in remnants.files {
            ctx.report.notes.push(format!(
                "{} could not be removed after the copy was verified; it was left in the old place",
                path.display()
            ));
        }
        if self.options.remove_empty_folders
            && let Some(parent) = item.from.parent()
        {
            self.remove_empty_chain(ctx, parent, None);
        }
        Ok(())
    }

    /// A rename on one volume, a verified copy otherwise.
    fn move_whole(&self, from: &Path, to: &Path) -> std::io::Result<Remnants> {
        if let Some(parent) = to.parent() {
            self.fs.create_dir_all(parent)?;
        }
        match self.fs.rename(from, to) {
            Err(e) if e.kind() == std::io::ErrorKind::CrossesDevices => {
                tree::move_tree(&self.fs, from, to)
            }
            other => other.map(|()| Remnants::default()),
        }
    }
}

/// Both folders must lie inside their data source, not be the root itself, and not contain each
/// other.
fn check_places(item: &FolderMove) -> Step<()> {
    let inside = |root: &Path, path: &Path| is_within(root, path) && !same_path(root, path);
    if !inside(&item.from_root, &item.from) {
        return Err(Fail::Op(format!(
            "{} is not a folder inside the data source {}",
            item.from.display(),
            item.from_root.display()
        )));
    }
    if !inside(&item.to_root, &item.to) {
        return Err(Fail::Op(format!(
            "{} is not a folder inside the data source {}",
            item.to.display(),
            item.to_root.display()
        )));
    }
    if is_within(&item.from, &item.to) || is_within(&item.to, &item.from) {
        return Err(Fail::Op(
            "the destination and the folder contain each other".to_owned(),
        ));
    }
    Ok(())
}

/// Files and folders of one item that go to the trash folder of its data source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashRequest {
    /// The library id of the item (or any key unique in the batch).
    pub id: String,
    /// The data source root the paths are in.
    pub root: PathBuf,
    /// Absolute paths below `root`: files or whole folders.
    pub paths: Vec<PathBuf>,
}

impl Executor<'_> {
    /// Moves files and folders into `<data source>/.deletedByCineVault/<batch>/`, journaled, so
    /// an undo can bring them back. This is the fallback of "delete with files" for a volume that
    /// has no system trash; nothing is deleted for good.
    ///
    /// # Errors
    /// Only a journal that cannot be written, which stops the batch at once.
    pub fn trash_paths(&mut self, requests: &[TrashRequest]) -> Result<BatchReport, ExecError> {
        let mut reports = Vec::with_capacity(requests.len());
        for request in requests {
            let mut ctx = Ctx {
                root: request.root.clone(),
                item_id: Some(request.id.clone()),
                steps: Vec::new(),
                report: ItemReport::new(&request.id),
                left_half: false,
            };
            match self.trash_item(&mut ctx, request) {
                Ok(()) => {}
                Err(Fail::Journal(e)) => return Err(e.into()),
                Err(Fail::Op(message)) => {
                    let notes = self.roll_back(&mut ctx)?;
                    ctx.report.notes.extend(notes);
                    ctx.report.status = ItemStatus::Failed(message);
                }
            }
            if let Some(progress) = self.progress.as_mut() {
                progress(&ctx.report);
            }
            reports.push(ctx.report);
        }
        Ok(BatchReport {
            batch_id: self.options.batch_id.clone(),
            items: reports,
        })
    }

    fn trash_item(&mut self, ctx: &mut Ctx, request: &TrashRequest) -> Step<()> {
        let available = self
            .fs
            .meta(&request.root)
            .ok()
            .flatten()
            .is_some_and(|meta| meta.is_dir);
        if !available {
            return Err(Fail::Op(format!(
                "the data source {} is not available",
                request.root.display()
            )));
        }
        for path in &request.paths {
            if !is_within(&request.root, path) || same_path(&request.root, path) {
                return Err(Fail::Op(format!(
                    "{} is not inside the data source {}",
                    path.display(),
                    request.root.display()
                )));
            }
            let Some(meta) = self
                .fs
                .meta(path)
                .map_err(|e| io_fail("cannot read", path, &e))?
            else {
                ctx.report.already_applied += 1;
                continue;
            };
            let rel = path
                .strip_prefix(&request.root)
                .map_err(|_| Fail::Op(format!("{} is outside the data source", path.display())))?;
            let base = super::trash_target(
                &request.root,
                &self.options.trash_dir,
                &self.options.batch_id,
                path,
                rel,
            );
            let dest = self.unused_name(&base)?;
            let entry = self.entry(ctx, crate::exec::JournalOp::Trash, path, &dest, meta.is_dir);
            self.journal.begin(&entry)?;
            match move_path(&self.fs, path, &dest, meta.is_dir) {
                Ok(()) => self.done(ctx, entry, &dest)?,
                Err(e) => return Err(self.failed(&entry, io_fail("cannot trash", path, &e))),
            }
            ctx.report.trashed += 1;
        }
        Ok(())
    }
}
