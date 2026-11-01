//! The daily backup of the library, while the app or the server holds the data folder.
//!
//! [`LibraryBackups::run`] waits a minute after start-up, so the cold start and the start-up scan
//! come first, then checks every hour. A check is due when no daily backup exists or the newest
//! is more than a day old; it then writes one with [`cv_db::backup::write`] on a blocking thread
//! and keeps the newest seven. A failed backup is logged and tried again at the next check; it
//! never stops the app. When backups have kept failing for three days, one notice in the
//! notification centre says so, until a backup succeeds again. The command line never starts
//! this.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Local};
use cv_db::Database;
use cv_db::backup;

use crate::notices::{AppNotice, NoticeHub, NoticeLevel};

/// A backup is due when the newest one is older than this.
pub const DUE_AFTER: Duration = Duration::from_secs(24 * 60 * 60);

/// Backups that have failed for this long since one was due are reported.
pub const REPORT_AFTER: Duration = Duration::from_secs(3 * 24 * 60 * 60);

/// The topic of the notice about failing backups.
const NOTICE_TOPIC: &str = "library-backup";

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
    notices: Arc<NoticeHub>,
    running: AtomicBool,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    /// When the last backup this session knows of was written.
    last_good: Option<SystemTime>,
    /// When the first failure since the last success happened.
    failing_since: Option<SystemTime>,
    /// Whether the failures are in the notification centre.
    reported: bool,
}

impl LibraryBackups {
    /// The backups of `db`, in the `backups` folder next to it. Backups that keep failing are
    /// reported to `notices`.
    pub fn new(db: Arc<Database>, notices: Arc<NoticeHub>) -> Self {
        let dir = db.path().parent().map_or_else(
            || PathBuf::from(backup::BACKUPS_DIR),
            |p| p.join(backup::BACKUPS_DIR),
        );
        Self {
            db,
            dir,
            clock: Arc::new(SystemTime::now),
            notices,
            running: AtomicBool::new(false),
            state: Mutex::new(State::default()),
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
            let mut state = self.state();
            state.last_good = state.last_good.max(newest);
            state.last_good
        };
        if last_good.is_some_and(|at| now.duration_since(at).unwrap_or_default() <= DUE_AFTER) {
            return BackupCheck::NotDue;
        }
        let date = DateTime::<Local>::from(now).date_naive();
        match backup::write(&self.db, &self.dir, date) {
            Ok(path) => {
                let reported = std::mem::replace(
                    &mut *self.state(),
                    State {
                        last_good: Some(now),
                        ..State::default()
                    },
                )
                .reported;
                if reported {
                    self.notices.publish(AppNotice::resolved(NOTICE_TOPIC));
                }
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
                let reason = error.to_string();
                self.failed(last_good, now, &reason);
                BackupCheck::Failed(reason)
            }
        }
    }

    /// Reports the failures once they have gone on for [`REPORT_AFTER`] since a backup was due:
    /// a day after the last good one, or since the first failure when there never was one.
    fn failed(&self, last_good: Option<SystemTime>, now: SystemTime, reason: &str) {
        let mut state = self.state();
        let since = *state.failing_since.get_or_insert(now);
        let due_since = last_good.map_or(since, |at| at + DUE_AFTER);
        if state.reported || now.duration_since(due_since).unwrap_or_default() < REPORT_AFTER {
            return;
        }
        state.reported = true;
        drop(state);
        self.notices.publish(failing_notice(last_good, reason));
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
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

/// The notice about backups that keep failing, with the day of the last good one.
fn failing_notice(last_good: Option<SystemTime>, reason: &str) -> AppNotice {
    let title = last_good.map_or_else(
        || "The library has never been backed up.".to_owned(),
        |at| {
            let day = DateTime::<Local>::from(at).format("%-d %B %Y");
            format!("The library has not been backed up since {day}.")
        },
    );
    AppNotice::new(NoticeLevel::Warning, title)
        .detail(reason)
        .source("Backups")
        .topic(NOTICE_TOPIC)
}

#[cfg(test)]
mod tests;
