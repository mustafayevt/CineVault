//! Crash recovery: settles the journal rows a crash left `planned`.
//!
//! The executor writes a row before every step and settles it after. A process that dies between
//! the two leaves a `planned` row, and the disk shows either the old state, the new state or (for
//! a cross-volume copy) both. [`recover`] looks at the disk and decides, so that afterwards no row
//! is `planned` and every file is either at its old or at its new place, never lost:
//!
//! | Row | Disk | Result |
//! |---|---|---|
//! | move, trash | source gone, destination there | `done` (the move had happened) |
//! | move, trash | source there, destination not | `failed`: it never started, nothing to undo |
//! | move, trash | source and destination, same bytes | the original is removed, `done` (the copy was verified, only the delete was missing) |
//! | move of a folder | both, every file of the original is also in the destination | the original files are removed, `done` (a folder copied to another volume) |
//! | move, trash | both, different bytes | `failed`, reported as needing attention; nothing is touched |
//! | move, trash | neither | `failed`, reported as needing attention |
//! | move from a temporary case-rename name | the temporary file there, destination not | the rename is finished, `done` |
//! | copy | destination is a verified copy of the source | `done` |
//! | copy | no destination | `failed`; a half-written `.cvpart` file is removed |
//!
//! A row for a step that was written ahead and never reached (the second half of a case-only
//! rename) is found `failed` the same way. Recovery never deletes a file it did not create.
//!
//! A row whose source or destination is on a drive that is not there (unplugged, a share that
//! does not answer) is not settled at all: "the source is not there" would read as a step that
//! never started. [`recover_with`] leaves such a row `planned` and lists it in
//! [`RecoveryReport::deferred`], so recovery settles it once the drive is back.

use std::path::{Path, PathBuf};

use super::fs::{Fs, Patient};
use super::journal::JournalEntry;
use super::journal::{Completion, ItemState, JournalOp, JournalRow, JournalState, JournalStore};
use super::run::{Executor, ItemPlan, ItemReport, move_path, part_path};
use super::tree;
use super::{CASE_TMP_PREFIX, ExecError};

/// The `error` of a row recovery settled as "never started". A batch with such rows was cut
/// short: it can be resumed ([`resume_interrupted`]) or taken back with an undo.
pub const INTERRUPTED: &str = "interrupted before the step started";

/// What a row says after a resume ran its step, or after the batch was rolled back.
pub const RESUMED: &str = "interrupted before the step started; carried on later";
pub const ABANDONED: &str = "interrupted before the step started; rolled back";

/// How recovery settled one row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recovery {
    /// The step had happened; the row is `done`.
    Completed,
    /// The step had not started; the row is `failed`.
    NotStarted,
    /// The step had happened except for removing the original; recovery finished it.
    Finished,
    /// The disk shows a state recovery will not guess at. The row is `failed` and files were
    /// left as they are. The text says what was found.
    NeedsAttention(String),
}

/// One settled row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recovered {
    pub batch_id: String,
    pub seq: u32,
    pub from: String,
    pub to: String,
    pub outcome: Recovery,
}

/// What [`recover`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecoveryReport {
    pub rows: Vec<Recovered>,
    /// The items the process stopped inside, as `(batch, item)`: their marks are `unfinished`
    /// now. Each is at its old or its new place file by file, but may be half renamed; Resume
    /// ([`resume_unfinished`]) finishes it, a roll-back takes it back.
    pub unfinished_items: Vec<(String, String)>,
    /// Rows left `planned` because a drive they touch is not reachable now, as
    /// `(batch, seq, from, to)`. Their items keep their marks.
    pub deferred: Vec<(String, u32, String, String)>,
}

impl RecoveryReport {
    /// The rows that need a person to look at them.
    pub fn needs_attention(&self) -> impl Iterator<Item = &Recovered> {
        self.rows
            .iter()
            .filter(|r| matches!(r.outcome, Recovery::NeedsAttention(_)))
    }
}

/// Settles every `planned` row of the journal. Run it once when the app starts, before anything
/// else touches the library folders.
///
/// # Errors
/// Only a journal that cannot be read or written.
pub fn recover(journal: &mut dyn JournalStore, fs: &dyn Fs) -> Result<RecoveryReport, ExecError> {
    recover_with(journal, fs, &|_| true)
}

/// Like [`recover`], but a row whose source or destination fails `reachable` (its drive is not
/// there) is left `planned` and listed in [`RecoveryReport::deferred`]; the items it belongs to
/// keep their marks. Run it again when the drive is back.
///
/// # Errors
/// Only a journal that cannot be read or written.
pub fn recover_with(
    journal: &mut dyn JournalStore,
    fs: &dyn Fs,
    reachable: &dyn Fn(&Path) -> bool,
) -> Result<RecoveryReport, ExecError> {
    // Every operation is tried again while another program holds a handle for a moment.
    let patient = Patient::new(fs);
    let fs: &dyn Fs = &patient;
    let mut report = RecoveryReport::default();
    let mut waiting_items: Vec<(String, String)> = Vec::new();
    for row in journal.pending()? {
        if !reachable(Path::new(&row.entry.from)) || !reachable(Path::new(&row.entry.to)) {
            if let Some(item) = &row.entry.item_id {
                waiting_items.push((row.entry.batch_id.clone(), item.clone()));
            }
            report.deferred.push((
                row.entry.batch_id.clone(),
                row.entry.seq,
                row.entry.from.clone(),
                row.entry.to.clone(),
            ));
            continue;
        }
        let first_half_done = is_case_tmp(Path::new(&row.entry.from))
            && journal
                .batch(&row.entry.batch_id)?
                .iter()
                .any(|r| r.entry.to == row.entry.from && r.state == JournalState::Done);
        let (completion, outcome) = settle(&row, fs, first_half_done);
        let mut entry = row.entry.clone();
        if let Completion::Done { .. } = completion {
            // The table does not know folders from files; the disk does.
            entry.is_dir = fs
                .meta(Path::new(&entry.to))
                .ok()
                .flatten()
                .is_some_and(|m| m.is_dir);
        }
        journal.complete(&entry, &completion)?;
        report.rows.push(Recovered {
            batch_id: entry.batch_id,
            seq: entry.seq,
            from: entry.from,
            to: entry.to,
            outcome,
        });
    }
    // An item still `running` was cut short somewhere between its first and its last step.
    for mark in journal.item_marks_in(ItemState::Running)? {
        if waiting_items
            .iter()
            .any(|(batch, item)| *batch == mark.batch_id && *item == mark.item_id)
        {
            continue;
        }
        journal.close_item(&mark.batch_id, &mark.item_id, ItemState::Unfinished)?;
        report.unfinished_items.push((mark.batch_id, mark.item_id));
    }
    Ok(report)
}

fn failed(outcome: Recovery) -> (Completion, Recovery) {
    let message = match &outcome {
        Recovery::NotStarted => INTERRUPTED.to_owned(),
        Recovery::NeedsAttention(why) => why.clone(),
        _ => String::new(),
    };
    (Completion::Failed(message), outcome)
}

fn is_case_tmp(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|n| n.to_string_lossy().starts_with(CASE_TMP_PREFIX))
}

/// `first_half_done`: for the second half of a case-only rename, the first half is `done`.
fn settle(row: &JournalRow, fs: &dyn Fs, first_half_done: bool) -> (Completion, Recovery) {
    let from = PathBuf::from(&row.entry.from);
    let to = PathBuf::from(&row.entry.to);
    // A half-written copy of ours never counts as the destination.
    let part = part_path(&to);
    if fs.meta(&part).ok().flatten().is_some() {
        // A copy of a file or of a whole folder that was not finished.
        let _ = tree::remove_tree(fs, &part);
    }
    let src = fs.meta(&from).ok().flatten();
    let dst = fs.meta(&to).ok().flatten();
    let done = |recovery: Recovery| {
        let meta = fs.meta(&to).ok().flatten();
        (
            Completion::Done {
                size_bytes: meta.filter(|m| !m.is_dir).map(|m| m.len),
                modified_ms: meta.map(|m| m.modified_ms),
            },
            recovery,
        )
    };
    let attention = |why: String| failed(Recovery::NeedsAttention(why));

    // The second half of a case-only rename whose first half never happened has nothing to do.
    if is_case_tmp(&from) && src.is_none() && !first_half_done {
        return failed(Recovery::NotStarted);
    }

    match row.entry.op {
        JournalOp::WriteNfo => {
            if dst.is_some() {
                done(Recovery::Completed)
            } else {
                failed(Recovery::NotStarted)
            }
        }
        JournalOp::Copy => match (src, dst) {
            (Some(_), Some(_)) if fs.same_content(&from, &to).unwrap_or(false) => {
                done(Recovery::Completed)
            }
            (_, None) => failed(Recovery::NotStarted),
            (None, Some(_)) => attention(format!(
                "{} is missing and {} exists",
                from.display(),
                to.display()
            )),
            (Some(_), Some(_)) => attention(format!(
                "{} exists but is not a copy of {}",
                to.display(),
                from.display()
            )),
        },
        JournalOp::Move | JournalOp::Trash => match (src, dst) {
            (None, Some(_)) => done(Recovery::Completed),
            (Some(_), None) => {
                if is_case_tmp(&from) {
                    // The first half of a case-only rename is done; finish the second.
                    match fs.rename(&from, &to) {
                        Ok(()) => done(Recovery::Finished),
                        Err(e) => attention(format!(
                            "{} is under a temporary name and cannot be renamed: {e}",
                            from.display()
                        )),
                    }
                } else {
                    failed(Recovery::NotStarted)
                }
            }
            (Some(s), Some(_)) if !s.is_dir && fs.same_content(&from, &to).unwrap_or(false) => {
                // Copied and verified; only the delete of the original is missing.
                match fs.remove_file(&from) {
                    Ok(()) => done(Recovery::Finished),
                    Err(e) => attention(format!(
                        "{} could not be removed after its copy was verified: {e}",
                        from.display()
                    )),
                }
            }
            (Some(s), Some(d)) if s.is_dir && d.is_dir && tree::is_fully_copied(fs, &from, &to) => {
                // A folder copied to another volume and put in place; only the original is left.
                let left = tree::finish_move(fs, &from);
                match left.files.first() {
                    None => done(Recovery::Finished),
                    Some(path) => attention(format!(
                        "{} could not be removed after the copy of the folder was verified",
                        path.display()
                    )),
                }
            }
            (Some(_), Some(_)) => attention(format!(
                "both {} and {} exist and differ",
                from.display(),
                to.display()
            )),
            (None, None) => attention(format!(
                "neither {} nor {} exists",
                from.display(),
                to.display()
            )),
        },
    }
}

/// What [`resume_interrupted`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResumeReport {
    /// Steps that were run now.
    pub resumed: usize,
    /// Steps that could not be run, with the path and why. They are left as they are.
    pub skipped: Vec<(String, String)>,
}

/// Runs again the steps of a batch that recovery found never started ([`INTERRUPTED`]): the
/// rename or move is done now if the file is still where it was and the destination is free.
/// Each step gets a new journal row (so an undo takes it back like the others) and the old row
/// is marked [`RESUMED`]. Nothing is replaced and nothing is deleted.
///
/// # Errors
/// Only a journal that cannot be read or written.
pub fn resume_interrupted(
    journal: &mut dyn JournalStore,
    fs: &dyn Fs,
    batch_id: &str,
) -> Result<ResumeReport, ExecError> {
    let patient = Patient::new(fs);
    let fs: &dyn Fs = &patient;
    let rows = journal.batch(batch_id)?;
    // An item with a mark is resumed whole, from its plan ([`resume_unfinished`]): running one
    // of its steps alone would leave it half renamed.
    let marked: Vec<String> = journal
        .item_marks(batch_id)?
        .into_iter()
        .map(|m| m.item_id)
        .collect();
    let mut next = rows.iter().map(|r| r.entry.seq).max().unwrap_or(0);
    let mut report = ResumeReport::default();
    for row in rows.iter().filter(|r| {
        r.state == JournalState::Failed
            && r.error == INTERRUPTED
            && !r
                .entry
                .item_id
                .as_ref()
                .is_some_and(|id| marked.contains(id))
    }) {
        let (from, to) = (PathBuf::from(&row.entry.from), PathBuf::from(&row.entry.to));
        let skip = |report: &mut ResumeReport, why: &str| {
            report
                .skipped
                .push((row.entry.from.clone(), why.to_owned()));
        };
        if is_case_tmp(&from) || !matches!(row.entry.op, JournalOp::Move | JournalOp::Trash) {
            skip(&mut report, "this step is left for Organize to plan again");
            continue;
        }
        let src = fs.meta(&from).ok().flatten();
        let dst = fs.meta(&to).ok().flatten();
        match (src, dst) {
            (Some(src), None) => {
                next += 1;
                let entry = JournalEntry {
                    batch_id: batch_id.to_owned(),
                    seq: next,
                    item_id: row.entry.item_id.clone(),
                    op: row.entry.op,
                    from: row.entry.from.clone(),
                    to: row.entry.to.clone(),
                    is_dir: src.is_dir,
                    created_parents: None,
                };
                journal.begin(&entry)?;
                match move_path(fs, &from, &to, src.is_dir) {
                    Ok(()) => {
                        let meta = fs.meta(&to).ok().flatten();
                        journal.complete(
                            &entry,
                            &Completion::Done {
                                size_bytes: meta.filter(|m| !m.is_dir).map(|m| m.len),
                                modified_ms: meta.map(|m| m.modified_ms),
                            },
                        )?;
                        journal.complete(&row.entry, &Completion::Failed(RESUMED.to_owned()))?;
                        report.resumed += 1;
                    }
                    Err(e) => {
                        journal.complete(&entry, &Completion::Failed(e.to_string()))?;
                        skip(&mut report, &e.to_string());
                    }
                }
            }
            (None, Some(_)) => {
                // Somebody finished it already.
                journal.complete(&row.entry, &Completion::Failed(RESUMED.to_owned()))?;
            }
            (Some(_), Some(_)) => skip(&mut report, "the new name is taken"),
            (None, None) => skip(&mut report, "the file is gone"),
        }
    }
    Ok(report)
}

/// What [`resume_unfinished`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UnfinishedResume {
    /// One report per item that was run again.
    pub items: Vec<ItemReport>,
    /// Unfinished items that have no plan to run again (journaled before plans were kept), with
    /// why. They are left as they are.
    pub skipped: Vec<(String, String)>,
}

/// Runs the unfinished items of a batch again, each exactly as the executor was first given it
/// (the plan kept with its mark), in the same batch: steps whose result is on the disk already
/// count as done, the rest run, journaled like the first time, so one undo still takes back the
/// whole batch. `protected` adds folders that are never removed or renamed as a whole (the data
/// source roots of today's library). Run [`recover`] first.
///
/// An item done now gets its never-started rows marked [`RESUMED`]. An item that fails again is
/// rolled back to where the stop left it and stays unfinished.
///
/// # Errors
/// Only a journal that cannot be read or written, or a pattern that is not valid.
pub fn resume_unfinished(
    journal: &mut dyn JournalStore,
    fs: &dyn Fs,
    batch_id: &str,
    protected: &[PathBuf],
) -> Result<UnfinishedResume, ExecError> {
    let mut out = UnfinishedResume::default();
    let marks: Vec<_> = journal
        .item_marks(batch_id)?
        .into_iter()
        .filter(|m| m.state == ItemState::Unfinished)
        .collect();
    // The titles that share their options run in one executor, so the order between them (a
    // title moving into a name another one leaves) is kept as in the first run.
    let mut groups: Vec<(ItemPlan, Vec<crate::conflict::BatchItem>)> = Vec::new();
    for mark in marks {
        let Some(plan) = ItemPlan::parse(&mark.plan) else {
            out.skipped.push((
                mark.item_id.clone(),
                "there is no plan to run again; apply it from Organize".to_owned(),
            ));
            continue;
        };
        let same = |(first, _): &&mut (ItemPlan, Vec<_>)| {
            first.trash_dir == plan.trash_dir
                && first.unwanted_patterns == plan.unwanted_patterns
                && first.remove_empty_folders == plan.remove_empty_folders
                && first.protected_dirs == plan.protected_dirs
        };
        match groups.iter_mut().find(|g| same(g)) {
            Some((_, items)) => items.push(plan.item),
            None => {
                let item = plan.item.clone();
                groups.push((plan, vec![item]));
            }
        }
    }
    for (plan, items) in groups {
        let options = plan
            .options(batch_id)
            .with_protected_dirs(protected.iter().cloned());
        let report = Executor::new(fs, journal, options)?.run(&items)?;
        for item in report.items {
            if item.status != super::run::ItemStatus::Done {
                // The roll-back of this run took back only what this run did; what ran before
                // the stop is still there, so the item is as half done as it was.
                journal.close_item(batch_id, &item.id, ItemState::Unfinished)?;
            } else {
                for row in journal.batch(batch_id)? {
                    if row.state == JournalState::Failed
                        && row.error == INTERRUPTED
                        && row.entry.item_id.as_deref() == Some(item.id.as_str())
                    {
                        journal.complete(&row.entry, &Completion::Failed(RESUMED.to_owned()))?;
                    }
                }
            }
            out.items.push(item);
        }
    }
    Ok(out)
}

/// Marks the never-started rows and the unfinished items of a batch as rolled back, so the
/// batch is no longer offered as unfinished (after a roll-back or an undo of it).
///
/// # Errors
/// Only a journal that cannot be read or written.
pub fn settle_interrupted(journal: &mut dyn JournalStore, batch_id: &str) -> Result<(), ExecError> {
    for row in journal.batch(batch_id)? {
        if row.state == JournalState::Failed && row.error == INTERRUPTED {
            journal.complete(&row.entry, &Completion::Failed(ABANDONED.to_owned()))?;
        }
    }
    for mark in journal.item_marks(batch_id)? {
        if mark.state == ItemState::Unfinished {
            journal.close_item(batch_id, &mark.item_id, ItemState::Abandoned)?;
        }
    }
    Ok(())
}
