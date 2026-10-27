//! Safe writes into media folders.
//!
//! Every file CineVault creates or replaces next to a video (NFO, artwork, renamed
//! subtitle and trailer files, MediaInfo sidecars) goes through these helpers:
//!
//! * **Atomic replace.** The new bytes go to a temporary file in the same folder, are flushed to
//!   the disk (`sync_all`) and only then renamed over the target. A crash, a full disk or a lost
//!   network share leaves either the old file or the new one, never half of either. The
//!   temporary name is unique per write (`<name>.<pid>-<n>.cvpart`), so two writers of the same
//!   file never share one temporary file, and a leftover from a crash never blocks the next write.
//!   The `.cvpart` extension is not a media, image, NFO or subtitle extension, so a scan never
//!   takes a leftover for a real file.
//! * **Short retries.** On Windows an antivirus scanner, the search indexer or Explorer's preview
//!   often holds a just-written file for a moment, and a rename or delete then fails with
//!   "access denied", "sharing violation" or "lock violation". Those errors are retried a few
//!   times with a growing pause (about 1.2 seconds in all) before the write gives up; any other
//!   error is returned at once. A write that gives up leaves the original untouched and removes
//!   its temporary file.
//! * **Moves that never lose a file.** [`move_file`] renames, and across volumes copies, flushes,
//!   checks the length and only then removes the original.
//! * **Folder renames wait longer.** A folder cannot be renamed on Windows while any file below
//!   it is open, and Explorer or a media player browsing it lets go after a few seconds, not a
//!   few hundred milliseconds. [`retry_folder`] tries for about four seconds.
//! * **CineVault's own reads.** [`reading`] registers a file CineVault has open for reading, so a
//!   rename can wait for it ([`wait_until_unread`]) instead of failing on CineVault's own handle.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

mod readers;
pub mod volume;

pub use readers::{ReadGuard, read_inside, reading, wait_until_unread};

/// The extension of a temporary file that is still being written.
pub const TEMP_EXTENSION: &str = "cvpart";

/// The pauses between the attempts of an operation that failed with a transient error.
const RETRY_PAUSES_MS: [u64; 6] = [25, 50, 100, 200, 300, 500];

/// The pauses for a folder rename: about four seconds in all, because a folder is held by every
/// handle below it (a player reading a video over the network, Explorer showing the folder).
const FOLDER_RETRY_PAUSES_MS: [u64; 9] = [25, 50, 100, 200, 300, 500, 750, 1000, 1000];

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

/// Whether `error` is one that another program holding the file for a moment causes, so the
/// operation may succeed when tried again shortly.
///
/// On Windows: `ERROR_ACCESS_DENIED` (5, also what a rename over a file open without delete
/// sharing gives), `ERROR_SHARING_VIOLATION` (32) and `ERROR_LOCK_VIOLATION` (33). Elsewhere:
/// `EBUSY` and `ETXTBSY`.
#[must_use]
pub fn is_transient(error: &io::Error) -> bool {
    let Some(code) = error.raw_os_error() else {
        return false;
    };
    if cfg!(windows) {
        matches!(code, 5 | 32 | 33)
    } else {
        matches!(code, 16 | 26)
    }
}

/// Runs `op`, trying again after a short pause while it fails with a [transient](is_transient)
/// error. The last error is returned when every attempt failed.
///
/// # Errors
/// The error of the last attempt, or the first error that is not transient.
pub fn retry<T>(what: &str, path: &Path, op: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    retry_with(&RETRY_PAUSES_MS, what, path, op)
}

/// [`retry`] for the rename of a folder: the same transient errors, tried for about four seconds
/// instead of one, since every open file below a folder holds it.
///
/// # Errors
/// The error of the last attempt, or the first error that is not transient.
pub fn retry_folder<T>(
    what: &str,
    path: &Path,
    op: impl FnMut() -> io::Result<T>,
) -> io::Result<T> {
    retry_with(&FOLDER_RETRY_PAUSES_MS, what, path, op)
}

fn retry_with<T>(
    pauses: &[u64],
    what: &str,
    path: &Path,
    mut op: impl FnMut() -> io::Result<T>,
) -> io::Result<T> {
    let mut pauses = pauses.iter();
    loop {
        match op() {
            Ok(value) => return Ok(value),
            Err(error) if is_transient(&error) => {
                let Some(pause) = pauses.next() else {
                    return Err(error);
                };
                tracing::debug!(%error, path = %path.display(), "{what} failed for a moment; trying again");
                std::thread::sleep(Duration::from_millis(*pause));
            }
            Err(error) => return Err(error),
        }
    }
}

/// A temporary name for a new version of `target`, in the same folder: `<name>.<pid>-<n>.cvpart`.
#[must_use]
pub fn temp_path(target: &Path) -> PathBuf {
    let n = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
    let mut name = target
        .file_name()
        .map_or_else(|| "file".into(), std::ffi::OsStr::to_os_string);
    name.push(format!(".{}-{n}.{TEMP_EXTENSION}", std::process::id()));
    target.with_file_name(name)
}

/// Writes `bytes` to a new temporary file beside `target` and flushes it to the disk. Returns the
/// temporary path; pass it to [`persist`]. The folder must exist. Nothing stays behind when this
/// fails.
///
/// # Errors
/// The temporary file cannot be created, written or flushed.
pub fn write_temp(target: &Path, bytes: &[u8]) -> io::Result<PathBuf> {
    let temp = temp_path(target);
    let written = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()
    })();
    match written {
        Ok(()) => Ok(temp),
        Err(error) => {
            let _ = fs::remove_file(&temp);
            Err(error)
        }
    }
}

/// Copies the file `source` to a new temporary file beside `target` and flushes it to the disk.
/// Returns the temporary path; pass it to [`persist`]. `source` is only read. Nothing stays
/// behind when this fails.
///
/// # Errors
/// `source` cannot be read, or the copy cannot be written, flushed or is short.
pub fn copy_temp(target: &Path, source: &Path) -> io::Result<PathBuf> {
    let temp = temp_path(target);
    let copied = (|| {
        let mut from = File::open(source)?;
        let expected = from.metadata()?.len();
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        let copied = io::copy(&mut from, &mut file)?;
        file.sync_all()?;
        if copied == expected {
            Ok(())
        } else {
            Err(io::Error::other("the copy is shorter than the original"))
        }
    })();
    match copied {
        Ok(()) => Ok(temp),
        Err(error) => {
            let _ = fs::remove_file(&temp);
            Err(error)
        }
    }
}

/// Renames the temporary file `temp` over `target`, retrying [transient](is_transient) errors.
/// The temporary file is removed when the rename fails, and `target` is then unchanged.
///
/// # Errors
/// The rename failed.
pub fn persist(temp: &Path, target: &Path) -> io::Result<()> {
    match rename(temp, target) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::remove_file(temp);
            Err(error)
        }
    }
}

/// Puts the finished temporary file `temp` at `target` **only if nothing is there**, and removes
/// `temp`. Unlike [`persist`] it never replaces a file: the two names are linked (one atomic step
/// that fails when `target` exists, so a file another program created a moment ago survives), and
/// the temporary name is dropped. On a volume without hard links (exFAT, FAT32, some network
/// shares) the link is not possible; then `target` is checked and `temp` is renamed, which leaves
/// only a very small window.
///
/// # Errors
/// [`io::ErrorKind::AlreadyExists`] when `target` exists; `temp` is then left for the caller to
/// remove. Another error when the file cannot be put in place; `target` is then unchanged.
pub fn persist_new(temp: &Path, target: &Path) -> io::Result<()> {
    match retry("linking a new file", target, || fs::hard_link(temp, target)) {
        Ok(()) => {
            // The name `temp` is not needed any more; the data stays under `target`.
            let _ = fs::remove_file(temp);
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Err(error),
        Err(_) => {
            if target.exists() {
                return Err(io::Error::from(io::ErrorKind::AlreadyExists));
            }
            rename(temp, target)
        }
    }
}

/// Puts `bytes` at `target` atomically: the old file stays until the new one is complete and on
/// the disk. The folder must exist.
///
/// # Errors
/// The new file cannot be written or put in place; `target` is then unchanged and no temporary
/// file stays behind.
pub fn write_atomic(target: &Path, bytes: &[u8]) -> io::Result<()> {
    let temp = write_temp(target, bytes)?;
    persist(&temp, target)
}

/// Renames `from` to `to` (replacing `to` when it is a file), retrying [transient](is_transient)
/// errors.
///
/// # Errors
/// The rename failed.
pub fn rename(from: &Path, to: &Path) -> io::Result<()> {
    retry("a rename", from, || fs::rename(from, to))
}

/// Removes a file, retrying [transient](is_transient) errors. Only for files CineVault itself
/// made (temporary files, the copy left by a move); user files go to the trash folder instead.
///
/// # Errors
/// The file cannot be removed.
pub fn remove_own_file(path: &Path) -> io::Result<()> {
    retry("removing a file", path, || fs::remove_file(path))
}

/// Flushes a file somebody else wrote (a download tool's output) to the disk before it is put
/// in place.
///
/// # Errors
/// The file cannot be opened or flushed.
pub fn sync_file(path: &Path) -> io::Result<()> {
    retry("flushing a file", path, || {
        OpenOptions::new().write(true).open(path)?.sync_all()
    })
}

/// Copies `from` to `to` (replacing `to`) and flushes the copy to the disk. For backups that must
/// exist before the original is replaced.
///
/// # Errors
/// The copy failed or is short; a partial `to` is removed.
pub fn copy_synced(from: &Path, to: &Path) -> io::Result<()> {
    let temp = copy_temp(to, from)?;
    persist(&temp, to)
}

/// Moves the file `from` to `to`. A rename is tried first (with retries); when it fails, as it
/// does across volumes, the file is copied, flushed and checked, and only then is the original
/// removed. The parent folder of `to` is created when missing. `to` must not exist.
///
/// # Errors
/// The file could not be moved. The original is then still at `from` (and a copy may also be at
/// `to` when only the removal of the original failed).
pub fn move_file(from: &Path, to: &Path) -> io::Result<()> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    let renamed = match rename(from, to) {
        Ok(()) => return Ok(()),
        Err(error) => error,
    };
    if !from.is_file() {
        return Err(renamed);
    }
    let temp = copy_temp(to, from)?;
    persist(&temp, to)?;
    retry("removing a moved file", from, || fs::remove_file(from))
}

#[cfg(test)]
mod tests;
