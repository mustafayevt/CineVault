//! Which volume a path is on, and where a file goes in a custom trash folder.
//!
//! A custom trash folder (Settings › Deleted files) takes files only from its own volume: there a
//! file arrives by a rename, which is instant and keeps the file whole. A file on another volume
//! would have to be copied and checked like a move, so it goes to the trash folder of its own
//! drive instead (the callers decide that from [`custom_trash_path`] returning `None`).
//!
//! A file goes to `<custom>/.deletedByCineVault/<batch>/<volume name>/<path below the volume
//! root>`, the volume name being the drive letter (`D`), the share (`nas_media` for
//! `\\nas\media`) or the mount point's name (`Media` for `/Volumes/Media`, `root` for `/`).
//! CineVault's batches live in their own `.deletedByCineVault` sub-folder of the chosen folder
//! ([`custom_trash_root`]), never at its top: the chosen folder may hold the person's own files
//! (or be a library folder or a drive root), and emptying the trash erases only the entries of a
//! `.deletedByCineVault`. Scans skip that name too.

use std::path::{Component, Path, PathBuf, Prefix};

/// The volume of `path` and its parts below the volume root.
fn split(path: &Path) -> Option<(String, Vec<String>)> {
    let mut components = path.components().peekable();
    let name = match components.peek() {
        Some(Component::Prefix(prefix)) => {
            let name = match prefix.kind() {
                Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
                    char::from(letter).to_ascii_uppercase().to_string()
                }
                Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => {
                    format!("{}_{}", server.to_string_lossy(), share.to_string_lossy())
                }
                _ => return None,
            };
            components.next();
            name
        }
        Some(Component::RootDir) => unix_volume(path)?,
        _ => return None,
    };
    let mut parts = Vec::new();
    for component in components {
        match component {
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            Component::RootDir | Component::CurDir => {}
            // A path that climbs out is never placed anywhere.
            Component::ParentDir | Component::Prefix(_) => return None,
        }
    }
    #[cfg(unix)]
    {
        // Below the mount point, not below `/`.
        let skip = mount_depth(path);
        parts.drain(..skip.min(parts.len()));
    }
    Some((name, parts))
}

#[cfg(unix)]
fn unix_volume(path: &Path) -> Option<String> {
    let mount = mount_point(path)?;
    Some(
        mount
            .file_name()
            .map_or_else(|| "root".to_owned(), |n| n.to_string_lossy().into_owned()),
    )
}

#[cfg(not(unix))]
fn unix_volume(_path: &Path) -> Option<String> {
    None
}

/// The nearest folder of `path` that exists (the path itself when it does).
#[cfg(unix)]
fn existing(path: &Path) -> Option<&Path> {
    path.ancestors().find(|p| p.symlink_metadata().is_ok())
}

/// The mount point that holds `path`: the highest parent on the same device.
#[cfg(unix)]
fn mount_point(path: &Path) -> Option<PathBuf> {
    use std::os::unix::fs::MetadataExt;
    let start = existing(path)?;
    let dev = std::fs::metadata(start).ok()?.dev();
    let mut mount = start.to_path_buf();
    while let Some(parent) = mount.parent() {
        match std::fs::metadata(parent) {
            Ok(meta) if meta.dev() == dev => mount = parent.to_path_buf(),
            _ => break,
        }
    }
    Some(mount)
}

#[cfg(unix)]
fn mount_depth(path: &Path) -> usize {
    mount_point(path).map_or(0, |m| {
        m.components()
            .filter(|c| matches!(c, Component::Normal(_)))
            .count()
    })
}

/// A key that is the same for two paths on one volume.
#[cfg(unix)]
fn volume_key(path: &Path) -> Option<String> {
    use std::os::unix::fs::MetadataExt;
    let start = existing(path)?;
    std::fs::metadata(start)
        .ok()
        .map(|meta| meta.dev().to_string())
}

/// A key that is the same for two paths on one volume.
#[cfg(not(unix))]
fn volume_key(path: &Path) -> Option<String> {
    split(path).map(|(name, _)| name.to_lowercase())
}

/// Whether two paths are on the same volume. Unknown counts as not.
#[must_use]
pub fn same_volume(a: &Path, b: &Path) -> bool {
    match (volume_key(a), volume_key(b)) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

/// The name of the sub-folder of a custom trash folder that holds CineVault's batches (the same
/// name as a library folder's own trash folder).
pub const CUSTOM_TRASH_SUBDIR: &str = ".deletedByCineVault";

/// Where CineVault keeps its batches in the custom trash folder `custom`:
/// `<custom>/.deletedByCineVault`. Only this folder's entries are ever emptied.
#[must_use]
pub fn custom_trash_root(custom: &Path) -> PathBuf {
    custom.join(CUSTOM_TRASH_SUBDIR)
}

/// The volume name of `path` as a custom trash folder names its sub-folder: `D`, `nas_media`,
/// `Media`; `None` when the path has none.
#[must_use]
pub fn volume_name(path: &Path) -> Option<String> {
    split(path).map(|(name, _)| name)
}

/// Where `path` goes in the custom trash folder `custom` for batch `batch`:
/// `<custom>/.deletedByCineVault/<batch>/<volume name>/<path below the volume root>`. `None`
/// when `path` is on another volume (it then goes to its own drive's trash folder), lies inside
/// `custom` already, or has no volume.
#[must_use]
pub fn custom_trash_path(custom: &Path, batch: &str, path: &Path) -> Option<PathBuf> {
    if custom.as_os_str().is_empty()
        || !custom.is_absolute()
        || batch.is_empty()
        || batch.contains(['/', '\\'])
        || inside(path, custom)
        || !same_volume(custom, path)
    {
        return None;
    }
    let (name, parts) = split(path)?;
    if parts.is_empty() {
        return None;
    }
    Some(parts.iter().fold(
        custom_trash_root(custom).join(batch).join(name),
        |acc, part| acc.join(part),
    ))
}

/// Whether `path` is `folder` or below it, compared without regard to separators, and to case
/// where the platform's file names ignore it (Windows and macOS; Linux keeps it).
#[must_use]
pub fn inside(path: &Path, folder: &Path) -> bool {
    crate::paths::is_within(path, folder)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn a_drive_path_goes_below_its_letter() {
        let custom = Path::new(r"D:\Trash");
        assert_eq!(
            custom_trash_path(custom, "b1", Path::new(r"D:\Movies\Heat (1995)\movie.nfo")),
            Some(PathBuf::from(
                r"D:\Trash\.deletedByCineVault\b1\D\Movies\Heat (1995)\movie.nfo"
            ))
        );
        assert_eq!(
            volume_name(Path::new(r"\\nas\media\x")).as_deref(),
            Some("nas_media")
        );
    }

    #[cfg(windows)]
    #[test]
    fn another_drive_or_the_folder_itself_is_refused() {
        let custom = Path::new(r"D:\Trash");
        assert_eq!(
            custom_trash_path(custom, "b1", Path::new(r"E:\Movies\a.mkv")),
            None
        );
        assert_eq!(
            custom_trash_path(custom, "b1", Path::new(r"d:\trash\x\a.mkv")),
            None
        );
        assert_eq!(custom_trash_path(custom, "b1", Path::new(r"D:\")), None);
        assert_eq!(
            custom_trash_path(custom, "../b", Path::new(r"D:\a.mkv")),
            None
        );
        assert!(same_volume(Path::new(r"d:\a"), Path::new(r"\\?\D:\b")));
    }

    #[test]
    fn a_path_inside_follows_the_platforms_case_rules() {
        let (upper, lower) = (Path::new("/a/Trash/x"), Path::new("/a/trash"));
        assert_eq!(inside(upper, lower), !crate::paths::CASE_SENSITIVE);
        assert!(inside(Path::new("/a/trash/x"), lower));
    }

    #[test]
    fn a_path_inside_is_told_apart_from_a_sibling_with_the_same_start() {
        assert!(inside(Path::new("/a/trash/x"), Path::new("/a/trash")));
        assert!(!inside(Path::new("/a/trash2/x"), Path::new("/a/trash")));
    }

    #[cfg(unix)]
    #[test]
    fn a_path_on_the_same_mount_is_placed() {
        let dir = std::env::temp_dir();
        let custom = dir.join("cv-custom-trash");
        let file = dir.join("cv-some-file.nfo");
        let placed = custom_trash_path(&custom, "b1", &file).unwrap();
        assert!(placed.starts_with(custom.join(CUSTOM_TRASH_SUBDIR).join("b1")));
        assert!(placed.ends_with("cv-some-file.nfo"));
    }
}
