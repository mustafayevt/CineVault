//! The daily backup of the library, while the app or the server holds the data folder.
//!
//! [`LibraryBackups::run`] waits a minute after start-up, so the cold start and the start-up scan
//! come first, then checks every hour. A check is due when no daily backup exists or the newest
//! is more than a day old; it then writes one with [`cv_db::backup::write`] on a blocking thread
//! and keeps the newest seven. A failed backup is logged and tried again at the next check; it
//! never stops the app. The command line never starts this.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Local};
use cv_db::Database;
use cv_db::backup;

/// A backup is due when the newest one is older than this.
pub const DUE_AFTER: Duration = Duration::from_secs(24 * 60 * 60);

/// When the checks run.
#[derive(Debug, Clone, Copy)]
pub struct BackupSchedule {
    /// The wait before the first check.
    pub first_check: Duration,
    /// The wait between checks.
    pub every: Duration,
}

impl Default for BackupSchedule {
    fn default() -> Self {
        Self {
            first_check: Duration::from_secs(60),
            every: Duration::from_secs(60 * 60),
        }
    }
}

/// What one check did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackupCheck {
    /// The newest backup is recent enough.
    NotDue,
    /// A backup was written here.
    Written(PathBuf),
    /// The backup failed for this reason; the next check tries again.
    Failed(String),
    /// Another check is writing a backup right now.
    Busy,
}

/// The current time; tests move it forward.
pub type Clock = Arc<dyn Fn() -> SystemTime + Send + Sync>;

/// The daily backups of one library.
pub struct LibraryBackups {
    db: Arc<Database>,
    dir: PathBuf,
    clock: Clock,
    running: AtomicBool,
    /// When the last backup this session knows of was written.
    last_good: Mutex<Option<SystemTime>>,
}

impl LibraryBackups {
    /// The backups of `db`, in the `backups` folder next to it.
    pub fn new(db: Arc<Database>) -> Self {
        let dir = db.path().parent().map_or_else(
            || PathBuf::from(backup::BACKUPS_DIR),
            |p| p.join(backup::BACKUPS_DIR),
        );
        Self {
            db,
            dir,
            clock: Arc::new(SystemTime::now),
            running: AtomicBool::new(false),
            last_good: Mutex::new(None),
        }
    }

    /// Reads the time from `clock` instead of the system.
    #[must_use]
    pub fn with_clock(mut self, clock: Clock) -> Self {
        self.clock = clock;
        self
    }

    /// The folder the backups go to.
    pub fn dir(&self) -> &std::path::Path {
        &self.dir
    }

    /// Checks once and writes a backup when one is due. Blocks for as long as the copy takes, so
    /// call it on a blocking thread.
    pub fn check(&self) -> BackupCheck {
        if self.running.swap(true, Ordering::AcqRel) {
            return BackupCheck::Busy;
        }
        let checked = self.check_now();
        self.running.store(false, Ordering::Release);
        checked
    }

    fn check_now(&self) -> BackupCheck {
        // What an interrupted backup left is removed at every check, due or not.
        backup::remove_partials(&self.dir);
        let now = (self.clock)();
        // An unreadable folder counts as no backup: the write then says why it fails.
        let newest = backup::newest(&self.dir).ok().flatten();
        let last_good = {
            let mut last_good = self
                .last_good
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            *last_good = (*last_good).max(newest);
            *last_good
        };
        if last_good.is_some_and(|at| now.duration_since(at).unwrap_or_default() <= DUE_AFTER) {
            return BackupCheck::NotDue;
        }
        let date = DateTime::<Local>::from(now).date_naive();
        match backup::write(&self.db, &self.dir, date) {
            Ok(path) => {
                *self
                    .last_good
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) = Some(now);
                if let Err(error) = backup::prune(&self.dir, backup::KEEP) {
                    tracing::warn!(%error, "cannot remove old library backups");
                }
                BackupCheck::Written(path)
            }
            Err(error) => {
                tracing::warn!(
                    %error,
                    dir = %self.dir.display(),
                    "the library backup failed; the next check tries again"
                );
                BackupCheck::Failed(error.to_string())
            }
        }
    }

    /// Checks on `schedule` until the task is aborted ([`crate::services::AppServices::shutdown`]).
    /// A copy in flight on its blocking thread finishes or dies with the process; its partial
    /// file is removed at the next check.
    pub async fn run(self: Arc<Self>, schedule: BackupSchedule) {
        tokio::time::sleep(schedule.first_check).await;
        loop {
            let backups = Arc::clone(&self);
            if let Err(error) = tokio::task::spawn_blocking(move || backups.check()).await {
                tracing::warn!(%error, "the library backup check stopped");
            }
            tokio::time::sleep(schedule.every).await;
        }
    }
}

#[cfg(test)]
mod tests;
