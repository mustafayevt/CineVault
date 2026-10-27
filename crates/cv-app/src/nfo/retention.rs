//! How long NFO undo records are kept.
//!
//! Every Read NFO and changing Rewrite NFO leaves a record under `<undo folder>/<token>/`, with
//! a full copy of each NFO it replaced. Undo is offered right after an action (a toast) and for
//! a while from Activity, so old records only cost disk space. They are pruned when the app
//! starts and after an action, by these rules:
//!
//! * a record younger than [`RetentionPolicy::protect_for`] or among the newest
//!   [`RetentionPolicy::protect_newest`] is never removed, so an Undo a toast still offers always
//!   works;
//! * otherwise a record older than [`RetentionPolicy::max_age`] goes, and so does every record
//!   beyond the newest [`RetentionPolicy::max_records`];
//! * when what is left is still larger than [`RetentionPolicy::max_bytes`], the oldest records
//!   that are not protected go until it fits.
//!
//! Only folders named like a token (`nfo-<milliseconds>-<n>`) are considered; anything else in
//! the folder is left alone.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The limits of the undo folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionPolicy {
    /// Records younger than this are always kept.
    pub protect_for: Duration,
    /// The newest records that are always kept.
    pub protect_newest: usize,
    /// Older records are removed.
    pub max_age: Duration,
    /// Records beyond this count (newest first) are removed.
    pub max_records: usize,
    /// The folder is kept below this size by removing the oldest records that are not protected.
    pub max_bytes: u64,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            protect_for: Duration::from_secs(60 * 60),
            protect_newest: 20,
            max_age: Duration::from_secs(30 * 24 * 60 * 60),
            max_records: 200,
            max_bytes: 512 * 1024 * 1024,
        }
    }
}

/// What a prune did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PruneReport {
    /// Records removed.
    pub removed: usize,
    /// Bytes they held.
    pub freed_bytes: u64,
    /// Records kept.
    pub kept: usize,
}

/// The creation time (milliseconds since 1970) of a token folder name, or `None` for a name that
/// is not a token.
fn token_millis(name: &str) -> Option<u128> {
    let rest = name.strip_prefix("nfo-")?;
    let (ms, n) = rest.split_once('-')?;
    if ms.is_empty() || n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    ms.parse().ok()
}

fn tree_size(path: &Path) -> u64 {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return 0;
    };
    if !meta.is_dir() {
        return meta.len();
    }
    fs::read_dir(path).map_or(0, |entries| {
        entries
            .flatten()
            .map(|entry| tree_size(&entry.path()))
            .sum()
    })
}

struct Record {
    path: PathBuf,
    millis: u128,
    bytes: u64,
}

/// Prunes the undo records in `dir` by `policy`, as of `now_millis`.
pub fn prune(dir: &Path, now_millis: u128, policy: &RetentionPolicy) -> PruneReport {
    let Ok(entries) = fs::read_dir(dir) else {
        return PruneReport::default();
    };
    let mut records: Vec<Record> = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|entry| {
            let millis = token_millis(&entry.file_name().to_string_lossy())?;
            Some(Record {
                path: entry.path(),
                millis,
                bytes: 0,
            })
        })
        .collect();
    // Newest first.
    records.sort_by(|a, b| b.millis.cmp(&a.millis).then_with(|| b.path.cmp(&a.path)));
    for record in &mut records {
        record.bytes = tree_size(&record.path);
    }

    let age = |record: &Record| now_millis.saturating_sub(record.millis);
    let protected = |rank: usize, record: &Record| {
        rank < policy.protect_newest || age(record) < policy.protect_for.as_millis()
    };
    let mut remove = vec![false; records.len()];
    for (rank, record) in records.iter().enumerate() {
        if protected(rank, record) {
            continue;
        }
        if age(record) > policy.max_age.as_millis() || rank >= policy.max_records {
            remove[rank] = true;
        }
    }
    let mut total: u64 = records
        .iter()
        .zip(&remove)
        .filter(|(_, gone)| !**gone)
        .map(|(record, _)| record.bytes)
        .sum();
    for rank in (0..records.len()).rev() {
        if total <= policy.max_bytes {
            break;
        }
        if remove[rank] || protected(rank, &records[rank]) {
            continue;
        }
        remove[rank] = true;
        total = total.saturating_sub(records[rank].bytes);
    }

    let mut report = PruneReport::default();
    for (record, gone) in records.iter().zip(&remove) {
        if !*gone {
            report.kept += 1;
            continue;
        }
        match fs::remove_dir_all(&record.path) {
            Ok(()) => {
                report.removed += 1;
                report.freed_bytes += record.bytes;
            }
            Err(error) => {
                report.kept += 1;
                tracing::warn!(%error, path = %record.path.display(), "an old NFO undo record could not be removed");
            }
        }
    }
    if report.removed > 0 {
        tracing::info!(
            removed = report.removed,
            freed_bytes = report.freed_bytes,
            kept = report.kept,
            "pruned old NFO undo records"
        );
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u128 = 24 * 60 * 60 * 1000;

    fn record(dir: &Path, millis: u128, bytes: usize) -> PathBuf {
        let path = dir.join(format!("nfo-{millis}-0"));
        fs::create_dir_all(path.join("files")).unwrap();
        fs::write(path.join("action.json"), "{}").unwrap();
        fs::write(path.join("files").join("movie.nfo"), vec![b'x'; bytes]).unwrap();
        path
    }

    fn policy() -> RetentionPolicy {
        RetentionPolicy {
            protect_for: Duration::from_secs(60 * 60),
            protect_newest: 2,
            max_age: Duration::from_secs(30 * 24 * 60 * 60),
            max_records: 5,
            max_bytes: u64::MAX,
        }
    }

    #[test]
    fn old_records_go_and_recent_ones_stay() {
        let dir = tempfile::tempdir().unwrap();
        let now = 100 * DAY;
        let old = record(dir.path(), now - 40 * DAY, 10);
        let month = record(dir.path(), now - 29 * DAY, 10);
        let newest = record(dir.path(), now - DAY, 10);
        let other = dir.path().join("not-a-token");
        fs::create_dir(&other).unwrap();
        let report = prune(dir.path(), now, &policy());
        assert_eq!(report.removed, 1);
        assert!(!old.exists());
        assert!(month.exists() && newest.exists());
        assert!(
            other.exists(),
            "folders that are not records are left alone"
        );
    }

    #[test]
    fn the_newest_records_survive_however_old() {
        let dir = tempfile::tempdir().unwrap();
        let now = 400 * DAY;
        let a = record(dir.path(), now - 300 * DAY, 10);
        let b = record(dir.path(), now - 200 * DAY, 10);
        let c = record(dir.path(), now - 100 * DAY, 10);
        prune(dir.path(), now, &policy());
        assert!(!a.exists());
        assert!(
            b.exists() && c.exists(),
            "the newest two are what an Undo may still offer"
        );
    }

    #[test]
    fn records_beyond_the_count_go_oldest_first() {
        let dir = tempfile::tempdir().unwrap();
        let now = 100 * DAY;
        let paths: Vec<PathBuf> = (0..8u128)
            .map(|n| record(dir.path(), now - (10 - n) * DAY, 10))
            .collect();
        let report = prune(dir.path(), now, &policy());
        assert_eq!(report.removed, 3);
        assert!(paths[..3].iter().all(|p| !p.exists()));
        assert!(paths[3..].iter().all(|p| p.exists()));
    }

    #[test]
    fn the_size_cap_removes_old_records_but_never_protected_ones() {
        let dir = tempfile::tempdir().unwrap();
        let now = 100 * DAY;
        let old = record(dir.path(), now - 5 * DAY, 1000);
        let older = record(dir.path(), now - 6 * DAY, 1000);
        // Within the protection hour, even though the cap would want it gone too.
        let fresh_a = record(dir.path(), now - 10_000, 1000);
        let fresh_b = record(dir.path(), now - 20_000, 1000);
        let fresh_c = record(dir.path(), now - 30_000, 1000);
        let policy = RetentionPolicy {
            max_bytes: 1500,
            ..policy()
        };
        prune(dir.path(), now, &policy);
        assert!(!old.exists() && !older.exists());
        assert!(fresh_a.exists() && fresh_b.exists() && fresh_c.exists());
    }

    #[test]
    fn a_missing_folder_is_nothing_to_do() {
        let dir = tempfile::tempdir().unwrap();
        let report = prune(&dir.path().join("none"), 0, &policy());
        assert_eq!(report, PruneReport::default());
    }
}
