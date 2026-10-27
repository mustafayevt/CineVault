//! The files CineVault itself has open for reading right now.
//!
//! On Windows a folder cannot be renamed while any file below it is open, whatever sharing the
//! open allowed: the rename fails with "access denied". CineVault reads the videos of a title for
//! minutes at a time (MediaInfo on a large Blu-ray stream, a checksum over the whole file, the
//! frames of the aspect-ratio check), and none of that holds the title
//! the way a write does. So every such read registers its path here for as long as the file is
//! open ([`reading`]), and a rename first waits for the reads inside the folder it is about to
//! move to end ([`wait_until_unread`]) instead of failing on its own handle.
//!
//! Paths are compared by their components, ignoring case on Windows and macOS, whose volumes do.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{Condvar, LazyLock, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

struct Registry {
    open: Mutex<Open>,
    closed: Condvar,
}

#[derive(Default)]
struct Open {
    next: u64,
    paths: HashMap<u64, PathBuf>,
}

static REGISTRY: LazyLock<Registry> = LazyLock::new(|| Registry {
    open: Mutex::new(Open::default()),
    closed: Condvar::new(),
});

fn open() -> MutexGuard<'static, Open> {
    REGISTRY.open.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Marks a path as being read by CineVault until it is dropped. See [`reading`].
#[derive(Debug)]
#[must_use = "the path counts as read only while the guard is alive"]
pub struct ReadGuard {
    id: u64,
}

impl Drop for ReadGuard {
    fn drop(&mut self) {
        open().paths.remove(&self.id);
        REGISTRY.closed.notify_all();
    }
}

/// Registers `path` (a file, or the folder of a disc structure) as read by CineVault until the
/// guard is dropped. Keep the guard for as long as the file, or any file below the folder, is
/// open, including in a program CineVault started.
pub fn reading(path: &Path) -> ReadGuard {
    let mut open = open();
    open.next += 1;
    let id = open.next;
    open.paths.insert(id, path.to_path_buf());
    ReadGuard { id }
}

/// A path CineVault is reading at or below `folder`, if any.
#[must_use]
pub fn read_inside(folder: &Path) -> Option<PathBuf> {
    first_inside(&open(), folder)
}

/// Waits up to `wait` for CineVault's own reads at or below `folder` to end. Returns `None` once
/// nothing there is read, or the path that is still read when the time is up.
#[must_use]
pub fn wait_until_unread(folder: &Path, wait: Duration) -> Option<PathBuf> {
    let deadline = Instant::now() + wait;
    let mut open = open();
    loop {
        let busy = first_inside(&open, folder)?;
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Some(busy);
        }
        tracing::debug!(
            folder = %folder.display(),
            file = %busy.display(),
            "waiting for a read of CineVault's own to end before a rename"
        );
        open = REGISTRY
            .closed
            .wait_timeout(open, left)
            .unwrap_or_else(PoisonError::into_inner)
            .0;
    }
}

fn first_inside(open: &Open, folder: &Path) -> Option<PathBuf> {
    let root = comparable(folder);
    open.paths
        .values()
        .find(|path| {
            let parts = comparable(path);
            parts.len() >= root.len() && parts[..root.len()] == root[..]
        })
        .cloned()
}

fn comparable(path: &Path) -> Vec<String> {
    path.components()
        .filter(|c| !matches!(c, Component::CurDir))
        .map(|c| {
            let text = c.as_os_str().to_string_lossy();
            if cfg!(any(windows, target_os = "macos")) {
                text.to_lowercase()
            } else {
                text.into_owned()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Paths of their own, so tests running at the same time never see each other's reads.
    fn unique(name: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!("cv-fs-readers-{}", std::process::id()))
            .join(name)
    }

    #[test]
    fn a_read_counts_for_its_folders_until_the_guard_is_dropped() {
        let folder = unique("Bridge (2015)");
        let stream = folder.join("BDMV").join("STREAM").join("00800.m2ts");
        assert_eq!(read_inside(&folder), None);
        let guard = reading(&stream);
        assert_eq!(read_inside(&folder), Some(stream.clone()));
        assert_eq!(read_inside(&folder.join("BDMV")), Some(stream.clone()));
        assert_eq!(
            read_inside(&stream),
            Some(stream.clone()),
            "a file is inside itself"
        );
        // A sibling whose name only starts the same is another folder.
        assert_eq!(read_inside(&unique("Bridge")), None);
        assert_eq!(read_inside(&folder.join("Other")), None);
        drop(guard);
        assert_eq!(read_inside(&folder), None);
    }

    #[test]
    fn folders_compare_without_case_where_the_volumes_do() {
        let folder = unique("Heat (1995)");
        let _guard = reading(&folder.join("Heat.mkv"));
        let upper = PathBuf::from(folder.to_string_lossy().to_uppercase());
        assert_eq!(
            read_inside(&upper).is_some(),
            cfg!(any(windows, target_os = "macos"))
        );
    }

    #[test]
    fn a_wait_ends_as_soon_as_the_read_does() {
        let folder = unique("Alien (1979)");
        let guard = reading(&folder.join("Alien.mkv"));
        let started = Instant::now();
        let reader = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            drop(guard);
        });
        assert_eq!(wait_until_unread(&folder, Duration::from_secs(10)), None);
        assert!(started.elapsed() < Duration::from_secs(5));
        reader.join().unwrap();
    }

    #[test]
    fn a_wait_that_runs_out_names_the_file_still_read() {
        let folder = unique("Ran (1985)");
        let file = folder.join("Ran.mkv");
        let _guard = reading(&file);
        assert_eq!(
            wait_until_unread(&folder, Duration::from_millis(30)),
            Some(file)
        );
        assert_eq!(
            wait_until_unread(&unique("Elsewhere"), Duration::ZERO),
            None
        );
    }
}
