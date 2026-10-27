//! The start-up hook: settling what a crash left half done.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use cv_db::Database;
use cv_rename::exec::{DbJournal, ExecError, RecoveryReport, StdFs, recover_with};

use crate::library_ops::ExceptMoves;

/// How often rows left for a missing drive are tried again.
pub const DEFERRED_RECOVERY_INTERVAL: Duration = Duration::from_secs(30);

/// Settles every journal row a crash left `planned`, by looking at the disk (see
/// [`cv_rename::exec::recover`] for the table). **The composition root calls this once at
/// start-up, before the first scan or any other work on the library folders**, so that every file
/// is at its old or its new place and no scan sees a half-finished rename. It may block on slow
/// drives, so it runs off the window's thread.
///
/// A row on a drive that is not there now (unplugged, a share that does not answer) is left
/// `planned` and listed in [`RecoveryReport::deferred`];
/// [`retry_deferred_recovery`] settles it once the drive is back.
///
/// The report lists what was settled; [`RecoveryReport::needs_attention`] has the rows a person
/// must look at (the disk showed a state recovery will not guess at). Nothing is deleted and
/// nothing is moved except to finish a step whose copy was proven.
///
/// # Errors
/// The journal cannot be read or written.
pub fn recover_interrupted_batches(db: &Arc<Database>) -> Result<RecoveryReport, ExecError> {
    // The rows of a move between data sources are not settled here: a half-copied move is not
    // "never started". They are listed by `library_ops::RecoveryService::find` and resumed or
    // rolled back by the user.
    let mut journal = ExceptMoves(DbJournal::new(Arc::clone(db)));
    recover_with(&mut journal, &StdFs, &volume_reachable)
}

/// Tries the rows a recovery left for a missing drive again every
/// [`DEFERRED_RECOVERY_INTERVAL`], on a thread of its own, until none is left. Returns at once
/// when `report` has none.
pub fn retry_deferred_recovery(db: Arc<Database>, report: &RecoveryReport) {
    if report.deferred.is_empty() {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("rename-recovery-retry".into())
        .spawn(move || {
            loop {
                std::thread::sleep(DEFERRED_RECOVERY_INTERVAL);
                match recover_interrupted_batches(&db) {
                    Ok(report) if report.deferred.is_empty() => {
                        tracing::info!(
                            settled = report.rows.len(),
                            "rename recovery settled the rows of a drive that came back"
                        );
                        return;
                    }
                    Ok(report) if !report.rows.is_empty() => tracing::info!(
                        settled = report.rows.len(),
                        waiting = report.deferred.len(),
                        "rename recovery settled the rows of a drive that came back"
                    ),
                    Ok(_) => {}
                    Err(error) => {
                        tracing::warn!(%error, "rename recovery could not run");
                    }
                }
            }
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "the rename recovery retry could not start");
    }
}

/// The root of the volume `path` is on: `D:\`, `\\server\share\`, `/Volumes/<name>` on macOS,
/// otherwise `/`.
fn volume_root(path: &Path) -> PathBuf {
    let mut components = path.components();
    match components.next() {
        Some(Component::Prefix(prefix)) => {
            let mut root = PathBuf::from(prefix.as_os_str());
            root.push(std::path::MAIN_SEPARATOR_STR);
            root
        }
        Some(Component::RootDir) => {
            let mut root = PathBuf::from("/");
            if let (Some(Component::Normal(first)), Some(Component::Normal(name))) =
                (components.next(), components.next())
                && first == "Volumes"
            {
                root.push(first);
                root.push(name);
            }
            root
        }
        _ => PathBuf::new(),
    }
}

/// Whether the drive of `path` is there. A relative path counts as reachable.
fn volume_reachable(path: &Path) -> bool {
    let root = volume_root(path);
    root.as_os_str().is_empty() || std::fs::metadata(&root).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn the_volume_root_is_the_drive_or_the_share() {
        assert_eq!(
            volume_root(Path::new(r"D:\Movies\Heat (1995)\Heat.mkv")),
            PathBuf::from(r"D:\")
        );
        assert_eq!(
            volume_root(Path::new(r"\\nas\media\Movies\Heat.mkv")),
            PathBuf::from(r"\\nas\media\")
        );
    }

    #[test]
    fn a_macos_volume_is_its_own_root() {
        assert_eq!(
            volume_root(Path::new("/Volumes/Media/Movies/Heat.mkv")),
            PathBuf::from("/Volumes/Media")
        );
        assert_eq!(volume_root(Path::new("/home/me/x.mkv")), PathBuf::from("/"));
    }

    #[test]
    fn the_folder_of_a_temporary_directory_is_reachable_and_a_made_up_volume_is_not() {
        let dir = tempfile::tempdir().unwrap();
        assert!(volume_reachable(&dir.path().join("Heat.mkv")));
        #[cfg(windows)]
        {
            // A drive letter that is not mapped on a test machine.
            let unmapped = ('G'..='Z')
                .rev()
                .map(|letter| format!("{letter}:\\"))
                .find(|root| std::fs::metadata(root).is_err());
            if let Some(root) = unmapped {
                assert!(!volume_reachable(
                    &Path::new(&root).join("Movies").join("x.mkv")
                ));
            }
        }
        #[cfg(not(windows))]
        assert!(!volume_reachable(Path::new(
            "/Volumes/cinevault-test-volume-that-is-not-there/x.mkv"
        )));
    }
}
