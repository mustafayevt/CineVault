//! The write-ahead journal the executor writes, as a trait, with the SQLite store and an
//! in-memory one.

use std::sync::{Arc, Mutex, MutexGuard};

use cv_db::Database;
use cv_db::repo::rename_journal as table;
pub use table::PathObserver;

pub use table::BatchSummary;

/// The kind of a journaled operation. Matches the `op` column of `rename_journal`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalOp {
    /// A file or a folder moved or renamed.
    Move,
    /// A file copied.
    Copy,
    /// A fresh NFO written.
    WriteNfo,
    /// A file moved into the trash folder.
    Trash,
}

impl JournalOp {
    pub fn as_str(self) -> &'static str {
        match self {
            JournalOp::Move => "move",
            JournalOp::Copy => "copy",
            JournalOp::WriteNfo => "write_nfo",
            JournalOp::Trash => "trash",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "move" => JournalOp::Move,
            "copy" => JournalOp::Copy,
            "write_nfo" => JournalOp::WriteNfo,
            "trash" => JournalOp::Trash,
            _ => return None,
        })
    }
}

/// Where a journaled operation stands. Matches the `state` column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalState {
    /// Written before the operation; the operation may or may not have happened.
    Planned,
    Done,
    Failed,
    /// Taken back by an undo or a rollback.
    Undone,
}

impl JournalState {
    pub fn as_str(self) -> &'static str {
        match self {
            JournalState::Planned => "planned",
            JournalState::Done => "done",
            JournalState::Failed => "failed",
            JournalState::Undone => "undone",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "planned" => JournalState::Planned,
            "done" => JournalState::Done,
            "failed" => JournalState::Failed,
            "undone" => JournalState::Undone,
            _ => return None,
        })
    }
}

/// The operation to write ahead. Paths are absolute with the native separators.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalEntry {
    pub batch_id: String,
    /// Unique in the batch; counts up in execution order.
    pub seq: u32,
    /// The library id of the item the operation belongs to.
    pub item_id: Option<String>,
    pub op: JournalOp,
    pub from: String,
    pub to: String,
    /// The operation moves a folder, not a file.
    pub is_dir: bool,
    /// How many folders above `to` the operation creates, counted from the parent of `to`
    /// upwards (the folders that did not exist when it was planned). Undo removes only those
    /// folders. `None` when not known (rows written before this was recorded).
    pub created_parents: Option<u32>,
}

/// A stored entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalRow {
    pub entry: JournalEntry,
    pub state: JournalState,
    /// Size and modification time of the destination right after the operation.
    pub size_bytes: Option<u64>,
    pub modified_ms: Option<i64>,
    pub error: String,
}

/// Where an item of a batch stands. Matches the `state` column of `rename_journal_item`.
///
/// An item is several operations; its mark is written before the first and settled after the
/// last, so a stop between two operations (when every row is settled) is still found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemState {
    /// Being renamed, or the process stopped while it was.
    Running,
    /// Every operation ran.
    Done,
    /// An operation failed and the ones before it were taken back: the item is as it was.
    RolledBack,
    /// Half renamed: the process stopped inside the item (recovery turns `Running` into this),
    /// or a roll-back could not take everything back. Resume or roll back.
    Unfinished,
    /// Rolled back or undone by the user instead of resumed.
    Abandoned,
}

impl ItemState {
    pub fn as_str(self) -> &'static str {
        match self {
            ItemState::Running => "running",
            ItemState::Done => "done",
            ItemState::RolledBack => "rolled_back",
            ItemState::Unfinished => "unfinished",
            ItemState::Abandoned => "abandoned",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "running" => ItemState::Running,
            "done" => ItemState::Done,
            "rolled_back" => ItemState::RolledBack,
            "unfinished" => ItemState::Unfinished,
            "abandoned" => ItemState::Abandoned,
            _ => return None,
        })
    }
}

/// The mark of one item of a batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemMark {
    pub batch_id: String,
    /// The id the executor was given for the item.
    pub item_id: String,
    pub state: ItemState,
    /// What the executor was given for the item, as JSON (see `ItemPlan`); empty when not known.
    pub plan: String,
}

/// How an operation ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Completion {
    Done {
        size_bytes: Option<u64>,
        modified_ms: Option<i64>,
    },
    Failed(String),
    /// The operation was taken back.
    Undone,
}

/// Why the journal could not be written.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the rename journal could not be written: {0}")]
pub struct JournalError(pub String);

impl From<cv_db::DbError> for JournalError {
    fn from(e: cv_db::DbError) -> Self {
        JournalError(e.to_string())
    }
}

/// Where the executor records what it is about to do and what it did.
///
/// [`begin`](JournalStore::begin) must be durable before it returns: the executor touches the
/// disk right after. A store that also keeps the library (the SQLite one) updates the library
/// paths in [`complete`](JournalStore::complete), in the same transaction as the row.
pub trait JournalStore {
    /// Writes an entry as `planned`.
    fn begin(&mut self, entry: &JournalEntry) -> Result<(), JournalError>;
    /// Settles an entry.
    fn complete(
        &mut self,
        entry: &JournalEntry,
        completion: &Completion,
    ) -> Result<(), JournalError>;
    /// The entries of one batch, in order.
    fn batch(&self, batch_id: &str) -> Result<Vec<JournalRow>, JournalError>;
    /// Every entry still `planned`, in the order written, in all batches.
    fn pending(&self) -> Result<Vec<JournalRow>, JournalError>;
    /// Every batch with its counts, newest first: the list undo is offered from.
    fn batches(&self) -> Result<Vec<BatchSummary>, JournalError>;
    /// Marks an item `running` before its first operation, with its plan. Must be durable
    /// before it returns, like [`begin`](JournalStore::begin).
    fn open_item(&mut self, batch_id: &str, item_id: &str, plan: &str) -> Result<(), JournalError>;
    /// Settles the mark of an item.
    fn close_item(
        &mut self,
        batch_id: &str,
        item_id: &str,
        state: ItemState,
    ) -> Result<(), JournalError>;
    /// The item marks of one batch.
    fn item_marks(&self, batch_id: &str) -> Result<Vec<ItemMark>, JournalError>;
    /// Every item mark in `state`, in all batches.
    fn item_marks_in(&self, state: ItemState) -> Result<Vec<ItemMark>, JournalError>;
    /// Writes several entries as `planned`, all or none: a crash while they are written leaves
    /// either every one of them or none. The default writes them one by one; a store that can
    /// do better (the SQLite one) writes them in one transaction.
    fn begin_all(&mut self, entries: &[JournalEntry]) -> Result<(), JournalError> {
        for entry in entries {
            self.begin(entry)?;
        }
        Ok(())
    }
    /// Marks several items `running` with their plans (`(item id, plan)`), all or none. The
    /// executor marks every item of a batch before the first operation, so a stop also finds
    /// the items that never started. The default writes them one by one; the SQLite store
    /// writes them in one transaction.
    fn open_items(
        &mut self,
        batch_id: &str,
        items: &[(String, String)],
    ) -> Result<(), JournalError> {
        for (item_id, plan) in items {
            self.open_item(batch_id, item_id, plan)?;
        }
        Ok(())
    }
}

/// A journal in memory. Clones share the rows, so a test can keep one clone while the executor
/// "crashes" with the other.
#[derive(Debug, Clone, Default)]
pub struct MemoryJournal {
    rows: Arc<Mutex<Vec<JournalRow>>>,
    marks: Arc<Mutex<Vec<ItemMark>>>,
}

impl MemoryJournal {
    pub fn new() -> Self {
        Self::default()
    }

    fn rows(&self) -> MutexGuard<'_, Vec<JournalRow>> {
        self.rows.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// All rows, in the order written.
    pub fn all(&self) -> Vec<JournalRow> {
        self.rows().clone()
    }

    fn marks(&self) -> MutexGuard<'_, Vec<ItemMark>> {
        self.marks.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// All item marks, in the order the items started.
    pub fn all_marks(&self) -> Vec<ItemMark> {
        self.marks().clone()
    }
}

impl JournalStore for MemoryJournal {
    fn begin(&mut self, entry: &JournalEntry) -> Result<(), JournalError> {
        let mut rows = self.rows();
        if rows
            .iter()
            .any(|r| r.entry.batch_id == entry.batch_id && r.entry.seq == entry.seq)
        {
            return Err(JournalError(format!(
                "sequence {} of batch {} is already used",
                entry.seq, entry.batch_id
            )));
        }
        rows.push(JournalRow {
            entry: entry.clone(),
            state: JournalState::Planned,
            size_bytes: None,
            modified_ms: None,
            error: String::new(),
        });
        Ok(())
    }

    fn complete(
        &mut self,
        entry: &JournalEntry,
        completion: &Completion,
    ) -> Result<(), JournalError> {
        let mut rows = self.rows();
        let row = rows
            .iter_mut()
            .find(|r| r.entry.batch_id == entry.batch_id && r.entry.seq == entry.seq)
            .ok_or_else(|| JournalError("the entry was never written".into()))?;
        match completion {
            Completion::Done {
                size_bytes,
                modified_ms,
            } => {
                row.state = JournalState::Done;
                row.size_bytes = *size_bytes;
                row.modified_ms = *modified_ms;
                row.error.clear();
            }
            Completion::Failed(error) => {
                row.state = JournalState::Failed;
                row.error.clone_from(error);
            }
            Completion::Undone => row.state = JournalState::Undone,
        }
        Ok(())
    }

    fn batch(&self, batch_id: &str) -> Result<Vec<JournalRow>, JournalError> {
        let mut rows: Vec<JournalRow> = self
            .rows()
            .iter()
            .filter(|r| r.entry.batch_id == batch_id)
            .cloned()
            .collect();
        rows.sort_by_key(|r| r.entry.seq);
        Ok(rows)
    }

    fn pending(&self) -> Result<Vec<JournalRow>, JournalError> {
        Ok(self
            .rows()
            .iter()
            .filter(|r| r.state == JournalState::Planned)
            .cloned()
            .collect())
    }

    /// Newest first by position in the journal. A memory journal keeps no times: `created_ms`
    /// is 0.
    fn batches(&self) -> Result<Vec<BatchSummary>, JournalError> {
        let rows = self.rows();
        let mut ids: Vec<&str> = Vec::new();
        for row in rows.iter() {
            let id = row.entry.batch_id.as_str();
            if let Some(at) = ids.iter().position(|b| *b == id) {
                ids.remove(at);
            }
            ids.push(id);
        }
        let summaries = ids
            .iter()
            .rev()
            .map(|id| {
                let of: Vec<&JournalRow> =
                    rows.iter().filter(|r| r.entry.batch_id == *id).collect();
                let count =
                    |state: JournalState| of.iter().filter(|r| r.state == state).count() as u64;
                let items: std::collections::BTreeSet<&str> = of
                    .iter()
                    .filter(|r| matches!(r.state, JournalState::Done | JournalState::Undone))
                    .filter_map(|r| r.entry.item_id.as_deref())
                    .collect();
                BatchSummary {
                    batch_id: (*id).to_owned(),
                    created_ms: 0,
                    operations: of.len() as u64,
                    done: count(JournalState::Done),
                    undone: count(JournalState::Undone),
                    failed: count(JournalState::Failed),
                    planned: count(JournalState::Planned),
                    items: items.len() as u64,
                }
            })
            .collect();
        Ok(summaries)
    }

    fn open_item(&mut self, batch_id: &str, item_id: &str, plan: &str) -> Result<(), JournalError> {
        let mut marks = self.marks();
        marks.retain(|m| !(m.batch_id == batch_id && m.item_id == item_id));
        marks.push(ItemMark {
            batch_id: batch_id.to_owned(),
            item_id: item_id.to_owned(),
            state: ItemState::Running,
            plan: plan.to_owned(),
        });
        Ok(())
    }

    fn close_item(
        &mut self,
        batch_id: &str,
        item_id: &str,
        state: ItemState,
    ) -> Result<(), JournalError> {
        if let Some(mark) = self
            .marks()
            .iter_mut()
            .find(|m| m.batch_id == batch_id && m.item_id == item_id)
        {
            mark.state = state;
        }
        Ok(())
    }

    fn item_marks(&self, batch_id: &str) -> Result<Vec<ItemMark>, JournalError> {
        Ok(self
            .marks()
            .iter()
            .filter(|m| m.batch_id == batch_id)
            .cloned()
            .collect())
    }

    fn item_marks_in(&self, state: ItemState) -> Result<Vec<ItemMark>, JournalError> {
        Ok(self
            .marks()
            .iter()
            .filter(|m| m.state == state)
            .cloned()
            .collect())
    }
}

/// The journal in the library database (`rename_journal`). Each call is its own committed
/// transaction, so a row is on disk before the executor acts on it.
///
/// A `Done` move also points the library at the new location in the same transaction: the
/// `media_file` rows of a moved file, or `item.path` and `media_file.directory` below a moved
/// folder. A `Done` trash entry points nothing (the file leaves the item). An `Undone` entry
/// points the library back.
#[derive(Clone)]
pub struct DbJournal {
    db: Arc<Database>,
    observer: Option<Arc<dyn PathObserver>>,
}

impl DbJournal {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db, observer: None }
    }

    /// Also tells `observer` about every move the library followed, once its transaction is
    /// committed (a failed write reports nothing). Undo and crash recovery report too.
    #[must_use]
    pub fn with_observer(mut self, observer: Arc<dyn PathObserver>) -> Self {
        self.observer = Some(observer);
        self
    }
}

fn from_db_mark(row: table::ItemMarkRow) -> Option<ItemMark> {
    Some(ItemMark {
        batch_id: row.batch_id,
        item_id: row.item_key,
        state: ItemState::parse(&row.state)?,
        plan: row.plan,
    })
}

fn from_db_row(row: table::JournalRow) -> Option<JournalRow> {
    let op = JournalOp::parse(&row.op)?;
    Some(JournalRow {
        entry: JournalEntry {
            batch_id: row.batch_id,
            seq: u32::try_from(row.seq).ok()?,
            item_id: row.item_id.map(|id| id.to_string()),
            op,
            from: row.from_path,
            to: row.to_path,
            // The table has no folder flag; a folder is told from a file by the disk.
            is_dir: false,
            created_parents: row.created_parents.and_then(|n| u32::try_from(n).ok()),
        },
        state: JournalState::parse(&row.state)?,
        size_bytes: row.size_bytes.and_then(|v| u64::try_from(v).ok()),
        modified_ms: row.modified_ms,
        error: row.error,
    })
}

impl JournalStore for DbJournal {
    fn begin(&mut self, entry: &JournalEntry) -> Result<(), JournalError> {
        let item_id = entry
            .item_id
            .as_deref()
            .and_then(|id| id.parse::<i64>().ok());
        let entry = entry.clone();
        self.db.write(move |tx| {
            table::insert_planned(
                tx,
                &table::NewJournalRow {
                    batch_id: &entry.batch_id,
                    seq: i64::from(entry.seq),
                    item_id,
                    op: entry.op.as_str(),
                    from_path: &entry.from,
                    to_path: &entry.to,
                    created_parents: entry.created_parents.map(i64::from),
                },
            )
        })?;
        Ok(())
    }

    fn begin_all(&mut self, entries: &[JournalEntry]) -> Result<(), JournalError> {
        let entries = entries.to_vec();
        self.db.write(move |tx| {
            for entry in &entries {
                let item_id = entry
                    .item_id
                    .as_deref()
                    .and_then(|id| id.parse::<i64>().ok());
                table::insert_planned(
                    tx,
                    &table::NewJournalRow {
                        batch_id: &entry.batch_id,
                        seq: i64::from(entry.seq),
                        item_id,
                        op: entry.op.as_str(),
                        from_path: &entry.from,
                        to_path: &entry.to,
                        created_parents: entry.created_parents.map(i64::from),
                    },
                )?;
            }
            Ok(())
        })?;
        Ok(())
    }

    fn complete(
        &mut self,
        entry: &JournalEntry,
        completion: &Completion,
    ) -> Result<(), JournalError> {
        let seq = i64::from(entry.seq);
        let (entry, completion) = (entry.clone(), completion.clone());
        let moved = match (&completion, entry.op) {
            (Completion::Done { .. }, JournalOp::Move) => {
                Some((entry.from.clone(), entry.to.clone()))
            }
            (Completion::Undone, JournalOp::Move) => Some((entry.to.clone(), entry.from.clone())),
            _ => None,
        };
        let is_dir = entry.is_dir;
        self.db.write(move |tx| {
            match &completion {
                Completion::Done {
                    size_bytes,
                    modified_ms,
                } => {
                    table::mark_done(
                        tx,
                        &entry.batch_id,
                        seq,
                        size_bytes.and_then(|v| i64::try_from(v).ok()),
                        *modified_ms,
                    )?;
                    if entry.op == JournalOp::Move {
                        repoint(tx, &entry.from, &entry.to, entry.is_dir)?;
                    }
                }
                Completion::Failed(error) => {
                    table::mark_failed(tx, &entry.batch_id, seq, error)?;
                }
                Completion::Undone => {
                    table::mark_undone(tx, &entry.batch_id, seq)?;
                    if entry.op == JournalOp::Move {
                        repoint(tx, &entry.to, &entry.from, entry.is_dir)?;
                    }
                }
            }
            Ok(())
        })?;
        if let (Some(observer), Some((from, to))) = (&self.observer, moved) {
            observer.path_moved(&from, &to, is_dir);
        }
        Ok(())
    }

    fn batch(&self, batch_id: &str) -> Result<Vec<JournalRow>, JournalError> {
        let rows = self.db.read(|conn| table::batch_rows(conn, batch_id))?;
        Ok(rows.into_iter().filter_map(from_db_row).collect())
    }

    fn pending(&self) -> Result<Vec<JournalRow>, JournalError> {
        let rows = self.db.read(table::pending_rows)?;
        Ok(rows.into_iter().filter_map(from_db_row).collect())
    }

    fn batches(&self) -> Result<Vec<BatchSummary>, JournalError> {
        Ok(self.db.read(table::batch_summaries)?)
    }

    fn open_item(&mut self, batch_id: &str, item_id: &str, plan: &str) -> Result<(), JournalError> {
        let (batch_id, item_id, plan) = (batch_id.to_owned(), item_id.to_owned(), plan.to_owned());
        self.db
            .write(move |tx| table::open_item(tx, &batch_id, &item_id, &plan))?;
        Ok(())
    }

    fn open_items(
        &mut self,
        batch_id: &str,
        items: &[(String, String)],
    ) -> Result<(), JournalError> {
        let (batch_id, items) = (batch_id.to_owned(), items.to_vec());
        self.db.write(move |tx| {
            for (item_id, plan) in &items {
                table::open_item(tx, &batch_id, item_id, plan)?;
            }
            Ok(())
        })?;
        Ok(())
    }

    fn close_item(
        &mut self,
        batch_id: &str,
        item_id: &str,
        state: ItemState,
    ) -> Result<(), JournalError> {
        let (batch_id, item_id) = (batch_id.to_owned(), item_id.to_owned());
        self.db
            .write(move |tx| table::close_item(tx, &batch_id, &item_id, state.as_str()))?;
        Ok(())
    }

    fn item_marks(&self, batch_id: &str) -> Result<Vec<ItemMark>, JournalError> {
        let rows = self.db.read(|conn| table::item_marks(conn, batch_id))?;
        Ok(rows.into_iter().filter_map(from_db_mark).collect())
    }

    fn item_marks_in(&self, state: ItemState) -> Result<Vec<ItemMark>, JournalError> {
        let rows = self
            .db
            .read(|conn| table::item_marks_in(conn, state.as_str()))?;
        Ok(rows.into_iter().filter_map(from_db_mark).collect())
    }
}

fn repoint(conn: &rusqlite::Connection, from: &str, to: &str, is_dir: bool) -> cv_db::DbResult<()> {
    if is_dir {
        table::repoint_folder(conn, from, to)?;
    } else {
        table::repoint_file(conn, from, to)?;
    }
    Ok(())
}
