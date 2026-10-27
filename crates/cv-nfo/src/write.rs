//! Writing NFO files: only when the content changed.
//!
//! A rewrite that would produce the same file (ignoring XML comments, because the header comment
//! carries a timestamp) leaves the file alone, so modification times and media-centre rescans stay
//! stable. A changed file is written to a temporary file next to it, flushed to the disk and
//! renamed over it (`cv_fs::write_atomic`), so a crash never leaves a half-written NFO, and a file
//! another program holds for a moment is retried instead of failing the write.

use std::fs;
use std::io;
use std::path::Path;

use crate::xml::strip_comments;

/// True when `existing` and `new` differ once XML comments are ignored.
pub fn content_differs(existing: &str, new: &str) -> bool {
    strip_comments(existing) != strip_comments(new)
}

/// Writes `content` to `path` unless the file already holds the same content. Returns whether the
/// file was written. Missing parent folders are created.
pub fn write_nfo_if_changed(path: &Path, content: &str) -> io::Result<bool> {
    match cv_fs::retry("reading an NFO", path, || fs::read(path)) {
        Ok(bytes) => {
            let existing = String::from_utf8_lossy(&bytes);
            let existing = existing.strip_prefix('\u{feff}').unwrap_or(&existing);
            if !content_differs(existing, content) {
                return Ok(false);
            }
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    cv_fs::write_atomic(path, content.as_bytes())?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comments_do_not_count_as_changes() {
        let old = "<?xml?>\r\n<!--created on 1-->\r\n<movie>\r\n  <a/>\r\n</movie>\r\n";
        let new = "<?xml?>\r\n<!--created on 2-->\r\n<movie>\r\n  <a/>\r\n</movie>\r\n";
        assert!(!content_differs(old, new));
        assert!(content_differs(old, &new.replace("<a/>", "<b/>")));
    }

    #[test]
    fn an_unchanged_file_is_not_touched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("Heat.nfo");
        let first = "<?xml?>\r\n<!--created on 1-->\r\n<movie/>\r\n";
        assert!(write_nfo_if_changed(&path, first).unwrap());
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(30));
        let same = "<?xml?>\r\n<!--created on 2-->\r\n<movie/>\r\n";
        assert!(!write_nfo_if_changed(&path, same).unwrap());
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
        assert_eq!(fs::read_to_string(&path).unwrap(), first);
        let changed = "<?xml?>\r\n<!--created on 3-->\r\n<movie><a/></movie>\r\n";
        assert!(write_nfo_if_changed(&path, changed).unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), changed);
        let leftovers: Vec<_> = fs::read_dir(path.parent().unwrap()).unwrap().collect();
        assert_eq!(leftovers.len(), 1, "no temporary file stays behind");
    }

    #[test]
    fn a_leftover_from_a_crash_does_not_block_the_next_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Heat.nfo");
        fs::write(&path, "<movie><title>Old</title></movie>").unwrap();
        // A temporary name an earlier crash left behind (here one that cannot be written over).
        fs::create_dir(dir.path().join("Heat.nfo.cvtmp")).unwrap();
        assert!(write_nfo_if_changed(&path, "<movie><title>New</title></movie>").unwrap());
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "<movie><title>New</title></movie>"
        );
    }

    /// An antivirus scanner or the indexer holding the NFO for a moment does not fail the write.
    #[cfg(windows)]
    #[test]
    fn an_nfo_held_open_for_a_moment_is_still_written() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Heat.nfo");
        fs::write(&path, "<movie><title>Old</title></movie>").unwrap();
        let held = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&path)
            .unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(150));
            drop(held);
        });
        let written = write_nfo_if_changed(&path, "<movie><title>New</title></movie>");
        release.join().unwrap();
        assert!(written.unwrap());
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "<movie><title>New</title></movie>"
        );
        let names: Vec<_> = fs::read_dir(dir.path()).unwrap().collect();
        assert_eq!(names.len(), 1, "no temporary file stays behind");
    }
}
