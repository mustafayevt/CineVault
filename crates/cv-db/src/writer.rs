//! The single writer: one dedicated thread that owns the only write connection.
//!
//! Serialising every write on one thread means SQLite never sees two writers (no `SQLITE_BUSY`
//! between our own threads), and callers need no locking. Work is submitted as closures and the
//! caller blocks until the result comes back; async callers run it through `spawn_blocking`.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};

use rusqlite::Connection;

use crate::error::{DbError, DbResult};

type Task = Box<dyn FnOnce(&mut Connection) + Send + 'static>;

pub(crate) struct Writer {
    tx: Option<mpsc::Sender<Task>>,
    handle: Option<JoinHandle<()>>,
}

impl Writer {
    /// Moves `conn` onto a new thread named `cv-db-writer`.
    pub(crate) fn spawn(conn: Connection) -> DbResult<Self> {
        let (tx, rx) = mpsc::channel::<Task>();
        let handle = thread::Builder::new()
            .name("cv-db-writer".into())
            .spawn(move || {
                let mut conn = conn;
                // Ends when every sender is dropped, i.e. when the database is closed.
                while let Ok(task) = rx.recv() {
                    task(&mut conn);
                }
            })
            .map_err(|source| DbError::Io {
                path: "cv-db-writer".into(),
                source,
            })?;
        Ok(Self {
            tx: Some(tx),
            handle: Some(handle),
        })
    }

    /// Runs `f` on the writer thread and waits for its result.
    ///
    /// A panic inside `f` is caught: any open transaction is rolled back as the stack unwinds,
    /// the writer keeps serving later requests, and the caller gets [`DbError::WriterPanicked`].
    pub(crate) fn run<T, F>(&self, f: F) -> DbResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> DbResult<T> + Send + 'static,
    {
        let (reply_tx, reply_rx) = mpsc::channel();
        let task: Task = Box::new(move |conn| {
            // If `f` panics, `reply_tx` is dropped during unwinding and the caller sees it.
            let _ = catch_unwind(AssertUnwindSafe(|| {
                let _ = reply_tx.send(f(conn));
            }));
        });
        self.tx
            .as_ref()
            .ok_or(DbError::WriterStopped)?
            .send(task)
            .map_err(|_| DbError::WriterStopped)?;
        reply_rx.recv().map_err(|_| DbError::WriterPanicked)?
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        // Closing the channel lets the thread finish queued work, then close the connection.
        self.tx.take();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}
