//! Writing artwork files safely: atomic writes, the trash and the variants of a name.
//!
//! * Every file is written next to its final name under a unique temporary name
//!   (`<name>.<pid>-<n>.cvpart`), flushed to the disk and renamed into place (`cv_fs`), so a
//!   crash or a full disk never leaves half a poster that the scanner would take for the real one.
//! * A file that is replaced, and every leftover, moves to
//!   `<data source>/.deletedByCineVault/<batch>/<path below the data source>`. Nothing is deleted.
//!   A replaced file moves only once the new one is written in full beside it, so a failed write
//!   always leaves the old file in its place.
//! * A file another program holds for a moment (a virus scanner, the indexer) is retried briefly
//!   instead of failing the write.
//! * An image is found by its name without the extension, because a poster the user saved as
//!   `poster.png` counts as the poster whatever the new download would be called.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use cv_rename::exec::{TRASH_DIR, new_batch_id};

/// The extensions an artwork file may have, lower case.
pub const IMAGE_EXTS: [&str; 6] = ["jpg", "jpeg", "png", "tbn", "webp", "gif"];

/// Whether `path` names an image file by its extension.
pub fn is_image_name(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| IMAGE_EXTS.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

/// The image files that already exist under the name of `path` with any image extension, `path`
/// itself included. The name is compared without regard to letter case.
pub fn variants(path: &Path) -> Vec<PathBuf> {
    let (Some(parent), Some(stem)) = (path.parent(), path.file_stem().and_then(|s| s.to_str()))
    else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(parent) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            is_image_name(p)
                && p.is_file()
                && p.file_stem()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| s.eq_ignore_ascii_case(stem))
        })
        .collect();
    found.sort();
    found
}

/// Whether an image exists under the name of `path`, with any image extension.
pub fn any_variant_exists(path: &Path) -> bool {
    !variants(path).is_empty()
}

/// Writes `bytes` to `path`, creating the folder. The file appears whole or not at all.
///
/// # Errors
/// The folder or the file cannot be written.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    cv_fs::write_atomic(path, bytes)
}

/// Where leftovers and replaced files go.
#[derive(Debug, Clone)]
pub struct Trash {
    /// The data source root, when the item is below one.
    data_source: Option<PathBuf>,
    /// Used for files outside the data source (or when there is none).
    fallback: PathBuf,
    label: String,
    /// Every `(original, trashed)` pair this trash moved, shared by the clones of one run.
    moves: std::sync::Arc<std::sync::Mutex<Vec<(PathBuf, PathBuf)>>>,
}

impl Trash {
    /// A trash for one run: files below `data_source` go to its trash folder, others to the trash
    /// folder inside `fallback`. The run gets a batch name of its own.
    pub fn new(data_source: Option<&Path>, fallback: &Path) -> Self {
        Self {
            data_source: data_source.map(Path::to_path_buf),
            fallback: fallback.to_path_buf(),
            label: format!("artwork-{}", new_batch_id()),
            moves: std::sync::Arc::default(),
        }
    }

    /// Every `(original, trashed)` pair moved so far by this run, in order. An Undo moves each
    /// trashed file back to its original place.
    pub fn moves(&self) -> Vec<(PathBuf, PathBuf)> {
        self.moves.lock().map(|m| m.clone()).unwrap_or_default()
    }

    /// The batch folder name of this run.
    pub fn label(&self) -> &str {
        &self.label
    }

    fn destination(&self, path: &Path) -> PathBuf {
        let (root, relative) = match &self.data_source {
            Some(root) if path.starts_with(root) => {
                (root.as_path(), path.strip_prefix(root).unwrap_or(path))
            }
            _ => (
                self.fallback.as_path(),
                path.strip_prefix(&self.fallback)
                    .unwrap_or_else(|_| Path::new(path.file_name().unwrap_or_default())),
            ),
        };
        let mut destination = root.join(TRASH_DIR).join(&self.label).join(relative);
        let mut n = 1;
        while destination.exists() {
            let stem = relative
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let ext = relative
                .extension()
                .map(|e| format!(".{}", e.to_string_lossy()))
                .unwrap_or_default();
            destination = root
                .join(TRASH_DIR)
                .join(&self.label)
                .join(relative.with_file_name(format!("{stem}-{n}{ext}")));
            n += 1;
        }
        destination
    }

    /// Moves `path` into the trash and returns where it went.
    ///
    /// # Errors
    /// The file cannot be moved (and not copied either).
    pub fn put(&self, path: &Path) -> io::Result<PathBuf> {
        let destination = self.destination(path);
        // Another volume: the file is copied, flushed and checked before the original goes.
        cv_fs::move_file(path, &destination)?;
        if let Ok(mut moves) = self.moves.lock() {
            moves.push((path.to_path_buf(), destination.clone()));
        }
        Ok(destination)
    }

    /// Moves a file [`Trash::put`] just set aside back to `original`, and forgets the move. For a
    /// write that failed after its old file was set aside.
    ///
    /// # Errors
    /// The file cannot be moved back; it then stays in the trash, and the move stays listed.
    pub fn take_back(&self, original: &Path, trashed: &Path) -> io::Result<()> {
        cv_fs::move_file(trashed, original)?;
        if let Ok(mut moves) = self.moves.lock()
            && let Some(at) = moves
                .iter()
                .rposition(|(o, t)| o == original && t == trashed)
        {
            moves.remove(at);
        }
        Ok(())
    }
}

/// What [`store`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stored {
    /// The file is new or replaced the old one.
    Written,
    /// The file already holds exactly these bytes.
    Unchanged,
    /// A different file is there and the caller did not ask to replace it.
    Kept,
}

/// Writes `bytes` to `path`.
///
/// An existing file with the same bytes is left alone. A different one is kept unless `replace`
/// is set, in which case it moves to the trash (and is listed in `trashed`), but only once the new
/// file is complete beside it: when anything fails, the old file is where it was.
///
/// # Errors
/// The file cannot be written or the old one cannot be moved to the trash.
pub fn store(
    path: &Path,
    bytes: &[u8],
    replace: bool,
    trash: &Trash,
    trashed: &mut Vec<PathBuf>,
) -> io::Result<Stored> {
    if !path.is_file() {
        write_atomic(path, bytes)?;
        return Ok(Stored::Written);
    }
    if fs::read(path).is_ok_and(|old| old == bytes) {
        return Ok(Stored::Unchanged);
    }
    if !replace {
        return Ok(Stored::Kept);
    }
    let temp = cv_fs::write_temp(path, bytes)?;
    let set_aside = match trash.put(path) {
        Ok(set_aside) => set_aside,
        Err(error) => {
            let _ = cv_fs::remove_own_file(&temp);
            return Err(error);
        }
    };
    if let Err(error) = cv_fs::persist(&temp, path) {
        if let Err(back) = trash.take_back(path, &set_aside) {
            tracing::warn!(%back, path = %path.display(), trash = %set_aside.display(),
                "a replaced image could not be put back; it is in the trash folder");
            trashed.push(set_aside);
        }
        return Err(error);
    }
    trashed.push(set_aside);
    Ok(Stored::Written)
}

/// Moves every image that exists under the name of `path` except `path` itself to the trash: the
/// `poster.png` that a new `poster.jpg` replaces. Call it after `path` is written, so a failed
/// write never leaves the type without an image.
///
/// # Errors
/// A file cannot be moved.
pub fn trash_other_variants(
    path: &Path,
    trash: &Trash,
    trashed: &mut Vec<PathBuf>,
) -> io::Result<()> {
    for other in variants(path) {
        // On a volume that ignores letter case `Poster.JPG` may be the file just written.
        if other != path && !is_same_file(&other, path) {
            trashed.push(trash.put(&other)?);
        }
    }
    Ok(())
}

/// Whether two paths name the same file on disk.
fn is_same_file(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variants_ignore_the_extension_and_the_letter_case() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["poster.PNG", "Poster.jpg", "poster.txt", "poster2.jpg"] {
            fs::write(dir.path().join(name), b"x").unwrap();
        }
        let found: Vec<String> = variants(&dir.path().join("poster.jpg"))
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(found, ["Poster.jpg", "poster.PNG"]);
        assert!(!any_variant_exists(&dir.path().join("fanart.jpg")));
    }

    #[test]
    fn an_atomic_write_leaves_no_part_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("extrafanart").join("fanart1.jpg");
        write_atomic(&target, b"abc").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"abc");
        let names: Vec<_> = fs::read_dir(target.parent().unwrap())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["fanart1.jpg"]);
    }

    #[test]
    fn replaced_files_go_to_the_trash_of_the_data_source() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let movie = root.join("Heat (1995)");
        fs::create_dir_all(&movie).unwrap();
        let poster = movie.join("poster.jpg");
        fs::write(&poster, b"old").unwrap();

        let trash = Trash::new(Some(root), &movie);
        let mut trashed = Vec::new();
        assert_eq!(
            store(&poster, b"old", true, &trash, &mut trashed).unwrap(),
            Stored::Unchanged
        );
        assert_eq!(
            store(&poster, b"new", false, &trash, &mut trashed).unwrap(),
            Stored::Kept
        );
        assert_eq!(fs::read(&poster).unwrap(), b"old");
        assert!(trashed.is_empty());

        assert_eq!(
            store(&poster, b"new", true, &trash, &mut trashed).unwrap(),
            Stored::Written
        );
        assert_eq!(fs::read(&poster).unwrap(), b"new");
        assert_eq!(trashed.len(), 1);
        let moved = &trashed[0];
        assert!(moved.starts_with(root.join(TRASH_DIR).join(trash.label())));
        assert!(
            moved.ends_with("Heat (1995)/poster.jpg") || moved.ends_with("Heat (1995)\\poster.jpg")
        );
        assert_eq!(fs::read(moved).unwrap(), b"old");
    }

    #[test]
    fn a_name_taken_in_the_trash_gets_a_number() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("m");
        fs::create_dir_all(&folder).unwrap();
        let trash = Trash::new(Some(dir.path()), &folder);
        let file = folder.join("a.jpg");
        fs::write(&file, b"1").unwrap();
        let first = trash.put(&file).unwrap();
        fs::write(&file, b"2").unwrap();
        let second = trash.put(&file).unwrap();
        assert_ne!(first, second);
        assert_eq!(fs::read(first).unwrap(), b"1");
        assert_eq!(fs::read(second).unwrap(), b"2");
    }

    #[test]
    fn a_replacement_that_cannot_be_written_never_leaves_the_slot_empty() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let movie = root.join("Heat (1995)");
        fs::create_dir_all(&movie).unwrap();
        let poster = movie.join("poster.jpg");
        fs::write(&poster, b"old").unwrap();
        // Something an earlier crash left at the temporary name that older versions used.
        fs::create_dir(movie.join("poster.jpg.cvpart")).unwrap();

        let trash = Trash::new(Some(root), &movie);
        let mut trashed = Vec::new();
        let stored = store(&poster, b"new", true, &trash, &mut trashed);
        let now = fs::read(&poster).ok();
        assert!(
            now.as_deref() == Some(b"new".as_slice()) && stored.is_ok()
                || now.as_deref() == Some(b"old".as_slice()) && trashed.is_empty(),
            "the poster is either the new one or still the old one: {now:?} {stored:?}"
        );
    }

    /// A poster the virus scanner or the indexer holds for a moment is still replaced, and the
    /// old one still reaches the trash.
    #[cfg(windows)]
    #[test]
    fn a_poster_held_open_for_a_moment_is_still_replaced() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let movie = dir.path().join("Heat (1995)");
        fs::create_dir_all(&movie).unwrap();
        let poster = movie.join("poster.jpg");
        fs::write(&poster, b"old").unwrap();
        let held = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&poster)
            .unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(150));
            drop(held);
        });
        let trash = Trash::new(Some(dir.path()), &movie);
        let mut trashed = Vec::new();
        let stored = store(&poster, b"new", true, &trash, &mut trashed);
        release.join().unwrap();
        assert_eq!(stored.unwrap(), Stored::Written);
        assert_eq!(fs::read(&poster).unwrap(), b"new");
        assert_eq!(fs::read(&trashed[0]).unwrap(), b"old");
    }
}
