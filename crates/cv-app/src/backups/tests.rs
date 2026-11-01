//! The backup checks with a clock the tests move: due, not due, failing and stopped.

use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use cv_db::Database;
use tempfile::TempDir;

use super::*;

const HOUR: Duration = Duration::from_secs(60 * 60);

/// A library, its backups and a clock that starts now and moves when the test says.
struct Fixture {
    _dir: TempDir,
    backups: Arc<LibraryBackups>,
    ahead: Arc<Mutex<Duration>>,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(Database::open(dir.path().join("library.db")).unwrap());
        let ahead = Arc::new(Mutex::new(Duration::ZERO));
        let clock = {
            let ahead = Arc::clone(&ahead);
            Arc::new(move || SystemTime::now() + *ahead.lock().unwrap())
        };
        let backups = Arc::new(LibraryBackups::new(db).with_clock(clock));
        Self {
            _dir: dir,
            backups,
            ahead,
        }
    }

    fn later(&self, by: Duration) {
        *self.ahead.lock().unwrap() += by;
    }

    fn files(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(self.backups.dir())
            .map(|rd| {
                rd.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }
}

fn today_after(by: Duration) -> String {
    backup::file_name(DateTime::<Local>::from(SystemTime::now() + by).date_naive())
}

#[test]
fn a_library_without_a_backup_gets_one() {
    let f = Fixture::new();
    let BackupCheck::Written(path) = f.backups.check() else {
        panic!("no backup written");
    };
    assert_eq!(path.parent(), Some(f.backups.dir()));
    assert_eq!(f.files(), [today_after(Duration::ZERO)]);
}

#[test]
fn a_recent_backup_is_left_alone() {
    let f = Fixture::new();
    assert!(matches!(f.backups.check(), BackupCheck::Written(_)));
    f.later(5 * HOUR);
    assert_eq!(f.backups.check(), BackupCheck::NotDue);
    assert_eq!(f.files().len(), 1);
}

#[test]
fn a_day_old_backup_is_followed_by_todays() {
    let f = Fixture::new();
    assert!(matches!(f.backups.check(), BackupCheck::Written(_)));
    f.later(25 * HOUR);
    assert!(matches!(f.backups.check(), BackupCheck::Written(_)));
    let mut expected = vec![today_after(Duration::ZERO), today_after(25 * HOUR)];
    expected.sort();
    assert_eq!(f.files(), expected);
}

#[test]
fn a_failed_backup_is_tried_again_at_the_next_check() {
    let f = Fixture::new();
    // A file where the folder should be: the backup cannot be written.
    std::fs::write(f.backups.dir(), b"in the way").unwrap();
    assert!(matches!(f.backups.check(), BackupCheck::Failed(_)));
    assert!(matches!(f.backups.check(), BackupCheck::Failed(_)));
    std::fs::remove_file(f.backups.dir()).unwrap();
    assert!(matches!(f.backups.check(), BackupCheck::Written(_)));
}

#[test]
fn a_partial_copy_is_removed_by_a_check_that_is_not_due() {
    let f = Fixture::new();
    assert!(matches!(f.backups.check(), BackupCheck::Written(_)));
    let partial = f
        .backups
        .dir()
        .join(format!("{}.partial", today_after(Duration::ZERO)));
    std::fs::write(&partial, b"half").unwrap();
    assert_eq!(f.backups.check(), BackupCheck::NotDue);
    assert!(!partial.exists());
}

#[tokio::test]
async fn the_schedule_checks_after_its_delay_and_stops_when_aborted() {
    let f = Fixture::new();
    let schedule = BackupSchedule {
        first_check: Duration::from_millis(20),
        every: Duration::from_millis(20),
    };
    let task = tokio::spawn(Arc::clone(&f.backups).run(schedule));
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    let written = |f: &Fixture| {
        f.files()
            .iter()
            .filter(|name| backup::is_daily_name(name))
            .count()
    };
    while written(&f) == 0 {
        assert!(std::time::Instant::now() < deadline, "no backup written");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    // Nothing checks any more: a backup that is now due is not written.
    f.later(25 * HOUR);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(written(&f), 1);
}
