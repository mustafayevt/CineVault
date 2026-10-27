//! At most a few folder listings at a time per drive.
//!
//! A walk reads sub-folders side by side on up to 16 threads, and the start-up rescan walks every
//! data source. On a spinning disk or a network share more than a few listings at once only make
//! the heads jump between them, so every listing takes a permit of its volume first:
//! [`READS_PER_VOLUME`] per drive letter or network share on Windows, per device elsewhere,
//! shared by every walk in the process. Folders on different drives are still read side by side.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex, OnceLock, PoisonError};

/// Folder listings that may run at the same time on one volume.
pub const READS_PER_VOLUME: usize = 4;

/// The permits of one volume.
#[derive(Debug)]
pub(crate) struct VolumeGate {
    busy: Mutex<usize>,
    freed: Condvar,
    max: usize,
}

/// Held while a folder is listed; gives the permit back when dropped.
pub(crate) struct Permit<'a> {
    gate: &'a VolumeGate,
}

impl VolumeGate {
    pub(crate) fn new(max: usize) -> Self {
        Self {
            busy: Mutex::new(0),
            freed: Condvar::new(),
            max: max.max(1),
        }
    }

    /// Waits for a permit.
    pub(crate) fn enter(&self) -> Permit<'_> {
        let mut busy = self.busy.lock().unwrap_or_else(PoisonError::into_inner);
        while *busy >= self.max {
            busy = self
                .freed
                .wait(busy)
                .unwrap_or_else(PoisonError::into_inner);
        }
        *busy += 1;
        Permit { gate: self }
    }
}

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        let mut busy = self
            .gate
            .busy
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        *busy = busy.saturating_sub(1);
        self.gate.freed.notify_one();
    }
}

/// The volume `path` is on: the drive letter or `\\server\share` on Windows, the device number
/// elsewhere. Paths whose volume cannot be told share one key.
fn volume_key(path: &Path) -> String {
    #[cfg(windows)]
    {
        use std::path::Component;
        match path.components().next() {
            Some(Component::Prefix(prefix)) => {
                prefix.as_os_str().to_string_lossy().to_ascii_lowercase()
            }
            _ => String::new(),
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(path).map_or_else(|_| String::new(), |m| m.dev().to_string())
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = path;
        String::new()
    }
}

/// The shared gate of the volume `root` is on.
pub(crate) fn gate_for(root: &Path) -> Arc<VolumeGate> {
    static GATES: OnceLock<Mutex<HashMap<String, Arc<VolumeGate>>>> = OnceLock::new();
    let key = volume_key(root);
    let mut gates = GATES
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    Arc::clone(
        gates
            .entry(key)
            .or_insert_with(|| Arc::new(VolumeGate::new(READS_PER_VOLUME))),
    )
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use super::*;

    #[test]
    fn no_more_than_the_permits_run_at_once() {
        let gate = VolumeGate::new(3);
        let now = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for _ in 0..16 {
                scope.spawn(|| {
                    let _permit = gate.enter();
                    let running = now.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(running, Ordering::SeqCst);
                    std::thread::sleep(Duration::from_millis(20));
                    now.fetch_sub(1, Ordering::SeqCst);
                });
            }
        });
        assert_eq!(peak.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn folders_on_one_volume_share_a_gate() {
        let dir = std::env::temp_dir();
        let a = gate_for(&dir.join("a"));
        let b = gate_for(&dir.join("b"));
        assert!(Arc::ptr_eq(&a, &b));
    }
}
