//! A small pool of read-only connections.
//!
//! WAL lets any number of readers run while the writer commits. Each caller checks a connection
//! out for the duration of one closure; when all are busy the caller waits for one to return.

use std::ops::Deref;
use std::sync::{Condvar, Mutex};

use rusqlite::Connection;

use crate::error::{DbError, DbResult};

pub(crate) struct ReaderPool {
    idle: Mutex<Vec<Connection>>,
    returned: Condvar,
}

impl ReaderPool {
    pub(crate) fn new(connections: Vec<Connection>) -> Self {
        Self {
            idle: Mutex::new(connections),
            returned: Condvar::new(),
        }
    }

    /// Checks out a connection, waiting while all of them are in use.
    pub(crate) fn get(&self) -> DbResult<PooledReader<'_>> {
        let mut idle = self.idle.lock().map_err(|_| DbError::PoolPoisoned)?;
        loop {
            if let Some(conn) = idle.pop() {
                return Ok(PooledReader {
                    pool: self,
                    conn: Some(conn),
                });
            }
            idle = self
                .returned
                .wait(idle)
                .map_err(|_| DbError::PoolPoisoned)?;
        }
    }

    fn put_back(&self, conn: Connection) {
        // A poisoned lock only means another reader panicked; the connection is still fine.
        let mut idle = self
            .idle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        idle.push(conn);
        self.returned.notify_one();
    }
}

/// A checked-out reader; it goes back to the pool when dropped.
pub(crate) struct PooledReader<'a> {
    pool: &'a ReaderPool,
    conn: Option<Connection>,
}

impl Deref for PooledReader<'_> {
    type Target = Connection;

    fn deref(&self) -> &Connection {
        self.conn.as_ref().expect("connection present until drop")
    }
}

impl Drop for PooledReader<'_> {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            self.pool.put_back(conn);
        }
    }
}
