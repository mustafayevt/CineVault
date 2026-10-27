//! What a move of a title folder to another data source contains and needs: every file and
//! subfolder with its size, whether the move is a rename or a copy, and the space the copy takes
//! on the destination volume.
//!
//! Everything here only reads the disk.

use std::fs;
use std::path::{Path, PathBuf};

/// The free space a copy keeps in reserve on the destination: 1% of what is copied, plus this.
pub const SPACE_MARGIN_FLOOR: u64 = 100 * 1024 * 1024;

/// What one entry of the title folder is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryKind {
    File,
    /// A folder (empty folders are listed too, so they arrive).
    Dir,
    /// A symbolic link or junction. It is recorded and never followed.
    Link {
        target: String,
    },
}

/// One file or folder of the title folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveEntry {
    /// The path below the title folder, with the separator of the system (`Season 01\ep.mkv`).
    pub path: String,
    pub kind: EntryKind,
    /// Size in bytes; 0 for a folder or a link.
    pub size: u64,
}

impl MoveEntry {
    #[must_use]
    pub fn is_file(&self) -> bool {
        self.kind == EntryKind::File
    }
}

/// Every entry below `folder`, folders before what they hold, in name order. Hidden files are
/// included. A link is listed and not followed.
///
/// # Errors
/// The text says which folder could not be read.
pub fn enumerate(folder: &Path) -> Result<Vec<MoveEntry>, String> {
    let mut out = Vec::new();
    walk(folder, Path::new(""), &mut out)?;
    Ok(out)
}

fn walk(root: &Path, rel: &Path, out: &mut Vec<MoveEntry>) -> Result<(), String> {
    let dir = root.join(rel);
    let read =
        |error: std::io::Error| format!("The folder {} could not be read: {error}", dir.display());
    let mut names: Vec<_> = fs::read_dir(&dir)
        .map_err(read)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(read)?;
    names.sort_by_key(fs::DirEntry::file_name);
    for entry in names {
        let path = rel.join(entry.file_name());
        let meta = fs::symlink_metadata(entry.path())
            .map_err(|error| format!("{} could not be read: {error}", entry.path().display()))?;
        let text = path.to_string_lossy().into_owned();
        if meta.file_type().is_symlink() {
            let target = fs::read_link(entry.path())
                .map(|t| t.to_string_lossy().into_owned())
                .unwrap_or_default();
            out.push(MoveEntry {
                path: text,
                kind: EntryKind::Link { target },
                size: 0,
            });
        } else if meta.is_dir() {
            out.push(MoveEntry {
                path: text,
                kind: EntryKind::Dir,
                size: 0,
            });
            walk(root, &path, out)?;
        } else {
            out.push(MoveEntry {
                path: text,
                kind: EntryKind::File,
                size: meta.len(),
            });
        }
    }
    Ok(())
}

/// The bytes a copy of `total` bytes keeps free on top of itself: 1% and at least 100 MB.
#[must_use]
pub fn margin_for(total: u64) -> u64 {
    (total / 100).max(SPACE_MARGIN_FLOOR)
}

/// The free space of the volume holding `path`, or `None` when the system will not say. A path
/// that does not exist yet is judged by its nearest existing folder.
#[must_use]
pub fn free_space(path: &Path) -> Option<u64> {
    let mut probe: PathBuf = path.to_path_buf();
    loop {
        if probe.exists() {
            return fs4::available_space(&probe).ok();
        }
        if !probe.pop() {
            return None;
        }
    }
}

/// The destination paths of `entries` below `to` that already exist (the folder itself first).
#[must_use]
pub fn existing_destinations(to: &Path, entries: &[MoveEntry]) -> Vec<String> {
    let mut found = Vec::new();
    if fs::symlink_metadata(to).is_err() {
        return found;
    }
    found.push(to.to_string_lossy().into_owned());
    for entry in entries {
        let path = to.join(&entry.path);
        if fs::symlink_metadata(&path).is_ok() {
            found.push(path.to_string_lossy().into_owned());
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_file_folder_and_hidden_file_is_listed_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("extras/deleted")).unwrap();
        fs::create_dir_all(root.join("empty")).unwrap();
        fs::write(root.join("Heat.mkv"), vec![0_u8; 10]).unwrap();
        fs::write(root.join(".hidden"), b"x").unwrap();
        fs::write(root.join("extras/making-of.mkv"), vec![0_u8; 7]).unwrap();
        fs::write(root.join("extras/deleted/scene.mkv"), vec![0_u8; 3]).unwrap();

        let entries = enumerate(root).unwrap();

        let sep = std::path::MAIN_SEPARATOR;
        let names: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(
            names,
            [
                ".hidden".to_owned(),
                "Heat.mkv".to_owned(),
                "empty".to_owned(),
                "extras".to_owned(),
                format!("extras{sep}deleted"),
                format!("extras{sep}deleted{sep}scene.mkv"),
                format!("extras{sep}making-of.mkv"),
            ]
        );
        let bytes: u64 = entries.iter().map(|e| e.size).sum();
        assert_eq!(bytes, 21);
        assert!(entries.iter().filter(|e| e.is_file()).count() == 4);
    }

    #[test]
    fn the_margin_is_one_percent_and_at_least_100_mb() {
        assert_eq!(margin_for(1024), SPACE_MARGIN_FLOOR);
        assert_eq!(margin_for(100 * 1024 * 1024 * 1024), 1024 * 1024 * 1024);
    }

    #[test]
    fn free_space_of_a_new_path_is_read_from_its_nearest_folder() {
        let dir = tempfile::tempdir().unwrap();
        assert!(free_space(&dir.path().join("not/yet/here")).is_some());
    }

    #[test]
    fn existing_destinations_name_the_folder_and_each_taken_file() {
        let dir = tempfile::tempdir().unwrap();
        let to = dir.path().join("Heat (1995)");
        assert!(existing_destinations(&to, &[]).is_empty());
        fs::create_dir_all(&to).unwrap();
        fs::write(to.join("poster.jpg"), b"x").unwrap();
        let entries = vec![
            MoveEntry {
                path: "poster.jpg".into(),
                kind: EntryKind::File,
                size: 1,
            },
            MoveEntry {
                path: "fanart.jpg".into(),
                kind: EntryKind::File,
                size: 1,
            },
        ];
        let found = existing_destinations(&to, &entries);
        assert_eq!(found.len(), 2);
        assert!(found[1].ends_with("poster.jpg"));
    }

    #[cfg(unix)]
    #[test]
    fn a_link_is_recorded_and_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("big.mkv"), vec![0_u8; 1000]).unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("elsewhere")).unwrap();
        let entries = enumerate(dir.path()).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(matches!(entries[0].kind, EntryKind::Link { .. }));
        assert_eq!(entries[0].size, 0);
    }
}
