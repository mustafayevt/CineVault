//! The Linux backstop for the one-copy rule.
//!
//! `tauri-plugin-single-instance` hands a second launch to the running copy over D-Bus. Without a
//! session bus (a bare window manager, some containers) it cannot reach the running copy, so a
//! second copy would open the same library. An exclusive lock on `instance.lock` in the data
//! folder stops it instead. The lock belongs to an open file handle, so the operating system
//! releases it when the process ends, even after a crash.

use std::fs::{File, OpenOptions};
use std::path::Path;

use tauri::{AppHandle, Runtime};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

/// The lock file, in the data folder.
pub const LOCK_FILE_NAME: &str = "instance.lock";

/// What a second copy says before it leaves.
pub const ALREADY_RUNNING_MESSAGE: &str = "CineVault is already running.";

/// Takes the exclusive lock on `<data_root>/instance.lock`. The lock lasts as long as the returned
/// file stays open, so the caller keeps it for the life of the process. `Ok(None)` means another
/// handle, usually in another process, holds the lock.
///
/// The data folder is created when it is missing, because the library needs it anyway.
pub fn acquire(data_root: &Path) -> Result<Option<File>, String> {
    std::fs::create_dir_all(data_root)
        .map_err(|error| format!("{}: {error}", data_root.display()))?;
    let path = data_root.join(LOCK_FILE_NAME);
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    // Called through the trait on purpose: std's `File::try_lock` has the same name and would
    // otherwise win the method lookup with its own error type.
    match fs4::FileExt::try_lock(&file) {
        Ok(()) => Ok(Some(file)),
        Err(fs4::TryLockError::WouldBlock) => Ok(None),
        Err(fs4::TryLockError::Error(error)) => Err(format!("{}: {error}", path.display())),
    }
}

/// Tells the user that another copy is running, then ends this copy when the message is closed.
/// The dialog is shown without waiting: setup runs on the main thread, and a blocking dialog must
/// not run there.
pub fn report_already_running<R: Runtime>(app: &AppHandle<R>) {
    tracing::warn!("another CineVault copy holds the data folder lock; this copy exits");
    let handle = app.clone();
    app.dialog()
        .message(ALREADY_RUNNING_MESSAGE)
        .kind(MessageDialogKind::Info)
        .show(move |_| handle.exit(0));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_lock_creates_and_holds_the_lock_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let held = acquire(dir.path()).expect("the lock file opens");
        assert!(held.is_some());
        assert!(dir.path().join(LOCK_FILE_NAME).is_file());
    }

    #[test]
    fn instance_lock_refuses_a_second_handle_until_the_first_is_dropped() {
        let dir = tempfile::tempdir().expect("temp dir");
        let first = acquire(dir.path()).expect("first acquire");
        assert!(first.is_some());
        let second = acquire(dir.path()).expect("second acquire");
        assert!(second.is_none(), "a second handle must not get the lock");
        drop(first);
        let third = acquire(dir.path()).expect("third acquire");
        assert!(
            third.is_some(),
            "the lock is free once the first handle closes"
        );
    }

    #[test]
    fn instance_lock_creates_a_missing_data_folder() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().join("CineVault");
        let held = acquire(&root).expect("acquire in a new folder");
        assert!(held.is_some());
        assert!(root.join(LOCK_FILE_NAME).is_file());
    }
}
