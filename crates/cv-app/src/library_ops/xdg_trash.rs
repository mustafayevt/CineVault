//! Sending a path to a freedesktop Trash by one rename (Linux, and tested on any Unix).
//!
//! The rule: an item reaches a Trash only by a single `rename` inside its own file system, into
//! `<trash>/files/`, with its `<trash>/info/<name>.trashinfo` written first. There is no copy
//! anywhere in this file. When the rename fails for any reason (another device after all, a
//! permission, a path that vanished) everything made for it is removed again and the error is
//! returned, so the item is still where it was and the caller moves it to the drive's own trash
//! folder (`.deletedByCineVault`) instead. The result is always "kept somewhere", never "erased".
//!
//! Choosing the Trash folder is [`super::xdg_dirs`]; reading, restoring and emptying what is
//! sent here is the `trash` crate's job on the info files written here.

// Only the Linux build sends; on other Unix systems the sender is compiled for its tests.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::ffi::OsStr;
use std::fs::{DirBuilder, OpenOptions};
use std::io::{ErrorKind, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};

/// Sends every path to the Trash of its own file system, one rename each. Stops at the first
/// error and returns it: the paths not sent stay in place for the caller's fallback.
#[cfg(target_os = "linux")]
pub(crate) fn send_all(paths: &[PathBuf]) -> Result<(), String> {
    for path in paths {
        let dir = super::xdg_dirs::trash_dir_for(path).map_err(|why| {
            format!(
                "{} has no Trash folder CineVault can use ({why:?})",
                path.display()
            )
        })?;
        let now = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string();
        move_into(&dir, path, &now)?;
    }
    Ok(())
}

/// Moves `path` into `trash_dir` (`files/` and `info/` are made when missing) and returns the
/// info file written for it. `now` is the `DeletionDate` text. On any error nothing is left
/// behind in the Trash and `path` is untouched.
pub(crate) fn move_into(trash_dir: &Path, path: &Path, now: &str) -> Result<PathBuf, String> {
    let fail = |what: &str, error: &dyn std::fmt::Display| {
        format!(
            "Could not move {} to the Trash: {what}: {error}",
            path.display()
        )
    };
    if !path.is_absolute() {
        return Err(fail("the path is not absolute", &""));
    }
    let name = path
        .file_name()
        .ok_or_else(|| fail("the path has no name", &""))?;
    // The item must exist now; a link is moved itself, never what it points to.
    let meta = std::fs::symlink_metadata(path).map_err(|e| fail("it cannot be read", &e))?;
    // A folder cannot go into a Trash that lies inside it.
    if trash_dir.starts_with(path) {
        return Err(fail("the Trash is inside it", &""));
    }

    prepare_dir(trash_dir).map_err(|e| fail("the Trash folder cannot be made", &e))?;
    let files = trash_dir.join("files");
    let info = trash_dir.join("info");
    std::fs::create_dir_all(&files).map_err(|e| fail("the Trash folder cannot be made", &e))?;
    std::fs::create_dir_all(&info).map_err(|e| fail("the Trash folder cannot be made", &e))?;

    let is_folder = meta.file_type().is_dir();
    let text = format!(
        "[Trash Info]\nPath={}\nDeletionDate={now}\n",
        encode_path(path)
    );
    let mut attempt = 1u32;
    loop {
        let mut stored = name.to_os_string();
        if attempt > 1 {
            stored.push(format!(".{attempt}"));
        }
        attempt += 1;
        let mut info_name = stored.clone();
        info_name.push(".trashinfo");
        let info_file = info.join(info_name);
        let target = files.join(&stored);

        // The info file is made first and atomically, so the name is ours alone.
        let mut file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&info_file)
        {
            Ok(file) => file,
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(fail("the info file cannot be made", &e)),
        };
        if let Err(e) = file
            .write_all(text.as_bytes())
            .and_then(|()| file.sync_all())
        {
            drop(file);
            let _ = std::fs::remove_file(&info_file);
            return Err(fail("the info file cannot be written", &e));
        }
        drop(file);

        // A placeholder holds the name in `files/` so nothing else can take it before the
        // rename (which would otherwise replace it).
        let reserved = if is_folder {
            DirBuilder::new().create(&target)
        } else {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target)
                .map(drop)
        };
        match reserved {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                // A leftover in `files/` without an info file: take the next name.
                let _ = std::fs::remove_file(&info_file);
                continue;
            }
            Err(e) => {
                let _ = std::fs::remove_file(&info_file);
                return Err(fail("the Trash cannot be written to", &e));
            }
        }

        // The one move: a rename on this file system. An error (including another device,
        // `EXDEV`) is returned as it is; there is no copy to fall back on.
        return match std::fs::rename(path, &target) {
            Ok(()) => Ok(info_file),
            Err(e) => {
                if is_folder {
                    let _ = std::fs::remove_dir(&target);
                } else {
                    let _ = std::fs::remove_file(&target);
                }
                let _ = std::fs::remove_file(&info_file);
                Err(fail("it cannot be renamed into the Trash", &e))
            }
        };
    }
}

/// Makes the Trash folder itself when it is missing, private to the user (mode `0700`), as the
/// freedesktop rules want; folders above it that are missing get the default mode.
fn prepare_dir(trash_dir: &Path) -> std::io::Result<()> {
    if trash_dir.exists() {
        return Ok(());
    }
    if let Some(parent) = trash_dir.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match DirBuilder::new().mode(0o700).create(trash_dir) {
        Err(e) if e.kind() == ErrorKind::AlreadyExists => Ok(()),
        other => other,
    }
}

/// The `Path=` value: every byte percent-encoded except unreserved ASCII (letters, digits and
/// `-_.~`) and `/`, which is the escaping the freedesktop Trash specification asks for.
pub(crate) fn encode_path(path: &Path) -> String {
    encode_bytes(path.as_os_str())
}

fn encode_bytes(text: &OsStr) -> String {
    let mut out = String::new();
    for &byte in text.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~' | b'/') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    /// The inverse of [`encode_path`], as a reader of the info file would do it.
    fn decode(text: &str) -> PathBuf {
        let bytes = text.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%' {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap();
                out.push(u8::from_str_radix(hex, 16).unwrap());
                i += 3;
            } else {
                out.push(bytes[i]);
                i += 1;
            }
        }
        PathBuf::from(OsString::from_vec(out))
    }

    fn path_line(info: &Path) -> String {
        let text = std::fs::read_to_string(info).unwrap();
        text.lines()
            .find_map(|l| l.strip_prefix("Path=").map(str::to_owned))
            .unwrap()
    }

    /// A temp dir with a library folder and a Trash folder next to each other.
    fn setup() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let lib = root.path().join("lib");
        let trash = root.path().join("Trash");
        std::fs::create_dir(&lib).unwrap();
        (root, lib, trash)
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut found: Vec<String> = std::fs::read_dir(dir)
            .map(|d| {
                d.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        found.sort();
        found
    }

    #[test]
    fn a_file_lands_in_files_with_its_info_file() {
        let (_root, lib, trash) = setup();
        let file = lib.join("Movie.mkv");
        std::fs::write(&file, b"x").unwrap();
        let info = move_into(&trash, &file, "2026-10-28T09:08:07").unwrap();
        assert!(!file.exists());
        assert_eq!(std::fs::read(trash.join("files/Movie.mkv")).unwrap(), b"x");
        assert_eq!(info, trash.join("info/Movie.mkv.trashinfo"));
        assert_eq!(
            std::fs::read_to_string(&info).unwrap(),
            format!(
                "[Trash Info]\nPath={}\nDeletionDate=2026-10-28T09:08:07\n",
                encode_path(&file)
            )
        );
        assert_eq!(decode(&path_line(&info)), file);
    }

    #[test]
    fn a_new_trash_folder_is_private() {
        let (_root, lib, trash) = setup();
        let file = lib.join("a.txt");
        std::fs::write(&file, b"x").unwrap();
        move_into(&trash, &file, "2026-10-28T00:00:00").unwrap();
        let mode = std::fs::metadata(&trash).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
    }

    #[test]
    fn a_folder_lands_in_files_with_its_content() {
        let (_root, lib, trash) = setup();
        let folder = lib.join("Dune (2021)");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("movie.nfo"), b"nfo").unwrap();
        let inode = std::fs::metadata(&folder).unwrap().ino();
        let info = move_into(&trash, &folder, "2026-10-28T09:08:07").unwrap();
        assert!(!folder.exists());
        let moved = trash.join("files/Dune (2021)");
        assert_eq!(std::fs::read(moved.join("movie.nfo")).unwrap(), b"nfo");
        // The same folder (one rename), not a copy.
        assert_eq!(std::fs::metadata(&moved).unwrap().ino(), inode);
        assert_eq!(decode(&path_line(&info)), folder);
    }

    #[test]
    fn the_moved_file_is_the_same_inode() {
        let (_root, lib, trash) = setup();
        let file = lib.join("Movie.mkv");
        std::fs::write(&file, b"x").unwrap();
        let before = std::fs::metadata(&file).unwrap();
        move_into(&trash, &file, "2026-10-28T09:08:07").unwrap();
        let after = std::fs::metadata(trash.join("files/Movie.mkv")).unwrap();
        assert_eq!((after.dev(), after.ino()), (before.dev(), before.ino()));
    }

    #[test]
    fn a_second_item_of_the_same_name_becomes_name_2_then_3() {
        let (_root, lib, trash) = setup();
        for (i, content) in [b"1", b"2", b"3"].into_iter().enumerate() {
            let file = lib.join("Movie.mkv");
            std::fs::write(&file, content).unwrap();
            let info = move_into(&trash, &file, "2026-10-28T09:08:07").unwrap();
            let stored = match i {
                0 => "Movie.mkv",
                1 => "Movie.mkv.2",
                _ => "Movie.mkv.3",
            };
            assert_eq!(info, trash.join(format!("info/{stored}.trashinfo")));
            assert_eq!(
                std::fs::read(trash.join("files").join(stored)).unwrap(),
                content
            );
        }
        assert_eq!(names(&trash.join("files")).len(), 3);
        assert_eq!(names(&trash.join("info")).len(), 3);
    }

    #[test]
    fn a_leftover_in_files_without_an_info_file_is_not_replaced() {
        let (_root, lib, trash) = setup();
        std::fs::create_dir_all(trash.join("files")).unwrap();
        std::fs::write(trash.join("files/Movie.mkv"), b"old").unwrap();
        let file = lib.join("Movie.mkv");
        std::fs::write(&file, b"new").unwrap();
        move_into(&trash, &file, "2026-10-28T09:08:07").unwrap();
        assert_eq!(
            std::fs::read(trash.join("files/Movie.mkv")).unwrap(),
            b"old"
        );
        assert_eq!(
            std::fs::read(trash.join("files/Movie.mkv.2")).unwrap(),
            b"new"
        );
        // The skipped name left no info file behind.
        assert_eq!(names(&trash.join("info")), ["Movie.mkv.2.trashinfo"]);
    }

    #[test]
    fn a_path_with_a_space_and_percent_round_trips() {
        let (_root, lib, trash) = setup();
        let file = lib.join("100% Real & Ünïcode #1.mkv");
        std::fs::write(&file, b"x").unwrap();
        let info = move_into(&trash, &file, "2026-10-28T09:08:07").unwrap();
        let line = path_line(&info);
        assert!(line.contains("100%25%20Real%20%26%20%C3%9Cn%C3%AFcode%20%231.mkv"));
        assert!(!line.contains(' '));
        assert_eq!(decode(&line), file);
    }

    #[test]
    fn a_name_that_is_not_utf8_round_trips() {
        let (_root, lib, trash) = setup();
        let file = lib.join(OsString::from_vec(vec![b'a', 0xFF, b'b']));
        std::fs::write(&file, b"x").unwrap();
        let info = move_into(&trash, &file, "2026-10-28T09:08:07").unwrap();
        assert!(path_line(&info).ends_with("/a%FFb"));
        assert_eq!(decode(&path_line(&info)), file);
    }

    #[test]
    fn a_link_is_moved_itself_not_what_it_points_to() {
        let (_root, lib, trash) = setup();
        let target = lib.join("real");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("keep.txt"), b"x").unwrap();
        let link = lib.join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        move_into(&trash, &link, "2026-10-28T09:08:07").unwrap();
        assert!(!link.exists() && std::fs::symlink_metadata(&link).is_err());
        assert!(target.join("keep.txt").exists());
        let stored = std::fs::symlink_metadata(trash.join("files/link")).unwrap();
        assert!(stored.file_type().is_symlink());
    }

    #[test]
    fn a_missing_source_leaves_nothing_behind() {
        let (_root, lib, trash) = setup();
        let error = move_into(&trash, &lib.join("gone.mkv"), "2026-10-28T09:08:07").unwrap_err();
        assert!(error.contains("gone.mkv"), "{error}");
        assert!(names(&trash.join("files")).is_empty());
        assert!(names(&trash.join("info")).is_empty());
    }

    #[test]
    fn a_failed_rename_removes_the_info_file_and_the_placeholder() {
        let (_root, lib, trash) = setup();
        // A folder can be renamed only when its parent is writable: lock the library folder.
        let folder = lib.join("Locked");
        std::fs::create_dir(&folder).unwrap();
        std::fs::set_permissions(&lib, std::fs::Permissions::from_mode(0o500)).unwrap();
        // Root ignores the mode, so the test then has nothing to check.
        let probe = std::fs::File::create(lib.join("probe")).is_ok();
        let result = move_into(&trash, &folder, "2026-10-28T09:08:07");
        std::fs::set_permissions(&lib, std::fs::Permissions::from_mode(0o700)).unwrap();
        if probe {
            return;
        }
        assert!(result.is_err());
        assert!(folder.exists(), "the source stays where it was");
        assert!(
            names(&trash.join("files")).is_empty(),
            "no placeholder left"
        );
        assert!(names(&trash.join("info")).is_empty(), "no info file left");
    }

    #[test]
    fn a_read_only_trash_folder_leaves_the_source_alone() {
        let (_root, lib, trash) = setup();
        std::fs::create_dir_all(trash.join("files")).unwrap();
        std::fs::create_dir_all(trash.join("info")).unwrap();
        let file = lib.join("Movie.mkv");
        std::fs::write(&file, b"x").unwrap();
        std::fs::set_permissions(trash.join("files"), std::fs::Permissions::from_mode(0o500))
            .unwrap();
        let probe = std::fs::File::create(trash.join("files/probe")).is_ok();
        let result = move_into(&trash, &file, "2026-10-28T09:08:07");
        std::fs::set_permissions(trash.join("files"), std::fs::Permissions::from_mode(0o700))
            .unwrap();
        if probe {
            return;
        }
        assert!(result.is_err());
        assert_eq!(std::fs::read(&file).unwrap(), b"x");
        assert!(names(&trash.join("info")).is_empty(), "no info file left");
    }

    #[test]
    fn a_relative_path_and_a_trash_inside_the_item_are_refused() {
        let (_root, lib, trash) = setup();
        assert!(move_into(&trash, Path::new("Movie.mkv"), "x").is_err());
        let inner = lib.join("Trash");
        assert!(move_into(&inner, &lib, "x").is_err());
        assert!(lib.exists() && !inner.exists());
    }

    #[test]
    fn encode_keeps_unreserved_bytes_and_slashes() {
        assert_eq!(encode_path(Path::new("/a-b_c.d~e/F 1")), "/a-b_c.d~e/F%201");
    }
}
