//! Moving a whole folder across volumes: copy the tree beside the destination, flush and verify
//! every file, rename it into place, and only then take the original files away.
//!
//! The same rules as a single file (see [`super::run`]): a crash at any point leaves the original
//! whole until the copy is proven, and recovery can finish the last step. The copy is written to
//! `<name>.cvpart`, a folder that never counts as the destination.
//!
//! Links (symbolic links and junctions) are never followed: what they point at is not part of
//! the title. As a move to another drive does, a folder that is or holds a link is refused before
//! anything is copied, and a link found later is neither copied nor removed nor entered.

use std::io;
use std::path::{Path, PathBuf};

use super::fs::Fs;
use super::run::{part_path, put_in_place};

/// What a folder move left behind at the source.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Remnants {
    /// Files of the original that could not be removed after the copy was verified.
    pub files: Vec<PathBuf>,
}

/// Moves `from` to `to` when a plain rename is not possible (another volume). `to` must not
/// exist. The destination appears all at once; the original is removed only after every file of
/// the copy matched byte for byte. A folder that is or holds a link is refused.
pub(crate) fn move_tree(fs: &dyn Fs, from: &Path, to: &Path) -> io::Result<Remnants> {
    if let Some(link) = first_link(fs, from)? {
        return Err(holds_link(&link));
    }
    if fs.meta(to)?.is_some() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} already exists", to.display()),
        ));
    }
    let part = part_path(to);
    if fs.meta(&part)?.is_some() {
        remove_tree(fs, &part)?;
    }
    if let Err(e) = copy_tree(fs, from, &part) {
        let _ = remove_tree(fs, &part);
        return Err(e);
    }
    if let Err(e) = put_in_place(fs, &part, to) {
        let _ = remove_tree(fs, &part);
        return Err(e);
    }
    Ok(remove_original(fs, from))
}

/// Copies the folder `from` to `to`, folders included, and checks each file against its copy.
/// Each folder of the copy is flushed once its files are in it.
fn copy_tree(fs: &dyn Fs, from: &Path, to: &Path) -> io::Result<()> {
    fs.create_dir_all(to)?;
    for name in fs.list_dir(from)? {
        let (src, dst) = (from.join(&name), to.join(&name));
        let Some(meta) = fs.meta(&src)? else {
            continue;
        };
        if meta.is_link {
            return Err(holds_link(&src));
        }
        if meta.is_dir {
            copy_tree(fs, &src, &dst)?;
        } else {
            fs.copy_file(&src, &dst)?;
            if !fs.same_content(&src, &dst)? {
                return Err(io::Error::other(format!(
                    "the copy of {} does not match the original",
                    src.display()
                )));
            }
        }
    }
    fs.sync_dir(to);
    Ok(())
}

/// `path` itself when it is a link, else the first link below it.
fn first_link(fs: &dyn Fs, path: &Path) -> io::Result<Option<PathBuf>> {
    match fs.meta(path)? {
        Some(meta) if meta.is_link => Ok(Some(path.to_path_buf())),
        Some(meta) if meta.is_dir => {
            for name in fs.list_dir(path)? {
                if let Some(link) = first_link(fs, &path.join(name))? {
                    return Ok(Some(link));
                }
            }
            Ok(None)
        }
        _ => Ok(None),
    }
}

fn holds_link(link: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        format!(
            "the folder contains a link ({}); links are not followed or copied to another \
             drive, so move this title by hand",
            link.display()
        ),
    )
}

/// Removes the files of a verified original and the folders they leave empty. What cannot be
/// removed stays and is reported, and so does a link, which is left as it is.
fn remove_original(fs: &dyn Fs, root: &Path) -> Remnants {
    let mut left = Remnants::default();
    match fs.meta(root) {
        Ok(Some(meta)) if meta.is_link => left.files.push(root.to_path_buf()),
        _ => remove_original_in(fs, root, &mut left),
    }
    left
}

fn remove_original_in(fs: &dyn Fs, dir: &Path, left: &mut Remnants) {
    let Ok(names) = fs.list_dir(dir) else {
        left.files.push(dir.to_path_buf());
        return;
    };
    for name in names {
        let path = dir.join(&name);
        match fs.meta(&path) {
            Ok(Some(meta)) if meta.is_link => left.files.push(path),
            Ok(Some(meta)) if meta.is_dir => remove_original_in(fs, &path, left),
            Ok(Some(_)) => {
                if fs.remove_file(&path).is_err() {
                    left.files.push(path);
                }
            }
            Ok(None) => {}
            Err(_) => left.files.push(path),
        }
    }
    if fs.remove_dir(dir).is_err() && left.files.is_empty() {
        left.files.push(dir.to_path_buf());
    }
}

/// Removes a folder CineVault created itself (a `.cvpart` copy) with everything in it. A link in
/// it is removed as a link, never entered.
pub(crate) fn remove_tree(fs: &dyn Fs, path: &Path) -> io::Result<()> {
    match fs.meta(path)? {
        None => Ok(()),
        // A link to a folder is removed as a folder on Windows.
        Some(meta) if meta.is_link => fs.remove_file(path).or_else(|_| fs.remove_dir(path)),
        Some(meta) if !meta.is_dir => fs.remove_file(path),
        Some(_) => {
            for name in fs.list_dir(path)? {
                remove_tree(fs, &path.join(name))?;
            }
            fs.remove_dir(path)
        }
    }
}

/// True when every file below `original` has a byte-identical file at the same place below
/// `copy`. A crash after the rename of a folder copy leaves the original in this state.
pub(crate) fn is_fully_copied(fs: &dyn Fs, original: &Path, copy: &Path) -> bool {
    let Ok(names) = fs.list_dir(original) else {
        return false;
    };
    names.iter().all(|name| {
        let (src, dst) = (original.join(name), copy.join(name));
        match (fs.meta(&src), fs.meta(&dst)) {
            (Ok(Some(s)), _) if s.is_link => false,
            (Ok(Some(s)), Ok(Some(d))) if s.is_dir && d.is_dir => is_fully_copied(fs, &src, &dst),
            (Ok(Some(s)), Ok(Some(d))) if !s.is_dir && !d.is_dir => {
                fs.same_content(&src, &dst).unwrap_or(false)
            }
            (Ok(None), _) => true,
            _ => false,
        }
    })
}

/// Finishes a folder move that stopped after the copy was in place: removes the original files.
pub(crate) fn finish_move(fs: &dyn Fs, original: &Path) -> Remnants {
    remove_original(fs, original)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::super::fs::{Meta, StdFs};
    use super::super::tests::folder_link;
    use super::*;

    /// The disk, where one ordinary folder passes for a link. It stands for a link that appears
    /// after the check, and its files for what such a link points at.
    struct OneLink {
        link: PathBuf,
    }

    impl Fs for OneLink {
        fn meta(&self, path: &Path) -> io::Result<Option<Meta>> {
            let meta = StdFs.meta(path)?;
            Ok(meta.map(|m| Meta {
                is_link: m.is_link || path == self.link,
                ..m
            }))
        }
        fn create_dir_all(&self, path: &Path) -> io::Result<()> {
            StdFs.create_dir_all(path)
        }
        fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
            StdFs.rename(from, to)
        }
        fn copy_file(&self, from: &Path, to: &Path) -> io::Result<()> {
            StdFs.copy_file(from, to)
        }
        fn same_content(&self, a: &Path, b: &Path) -> io::Result<bool> {
            StdFs.same_content(a, b)
        }
        fn remove_file(&self, path: &Path) -> io::Result<()> {
            StdFs.remove_file(path)
        }
        fn remove_dir(&self, path: &Path) -> io::Result<()> {
            StdFs.remove_dir(path)
        }
        fn list_dir(&self, path: &Path) -> io::Result<Vec<String>> {
            StdFs.list_dir(path)
        }
        fn sync_dir(&self, path: &Path) {
            StdFs.sync_dir(path);
        }
    }

    /// A title folder with a video and a folder `linked` holding `other.mkv`.
    fn title() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let title = dir.path().join("Heat (1995)");
        fs::create_dir_all(title.join("linked")).unwrap();
        fs::write(title.join("Heat (1995).mkv"), "video").unwrap();
        fs::write(title.join("linked/other.mkv"), "not this title").unwrap();
        (dir, title)
    }

    #[test]
    fn a_folder_holding_a_link_is_refused_before_anything_is_copied() {
        let (dir, title) = title();
        let fs = OneLink {
            link: title.join("linked"),
        };
        let to = dir.path().join("dest/Heat (1995)");
        let error = move_tree(&fs, &title, &to).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Unsupported, "{error}");
        assert!(title.join("Heat (1995).mkv").exists());
        assert!(title.join("linked/other.mkv").exists());
        assert!(!to.exists() && !part_path(&to).exists());
    }

    #[test]
    fn removing_an_original_never_enters_a_link() {
        let (_dir, title) = title();
        let fs = OneLink {
            link: title.join("linked"),
        };
        let left = finish_move(&fs, &title);
        assert!(!title.join("Heat (1995).mkv").exists());
        assert_eq!(
            fs::read_to_string(title.join("linked/other.mkv")).unwrap(),
            "not this title"
        );
        assert_eq!(left.files, vec![title.join("linked")]);
    }

    #[test]
    fn a_real_link_is_seen_as_one_and_never_followed() {
        let dir = tempfile::tempdir().unwrap();
        let elsewhere = dir.path().join("elsewhere");
        let title = dir.path().join("Heat (1995)");
        fs::create_dir_all(&elsewhere).unwrap();
        fs::create_dir_all(&title).unwrap();
        fs::write(elsewhere.join("other.mkv"), "not this title").unwrap();
        fs::write(title.join("Heat (1995).mkv"), "video").unwrap();
        let link = title.join("linked");
        if !folder_link(&elsewhere, &link) {
            eprintln!("skipped: this system will not make a folder link");
            return;
        }
        let meta = StdFs.meta(&link).unwrap().unwrap();
        assert!(meta.is_link && meta.is_dir, "{meta:?}");
        assert!(!StdFs.meta(&title).unwrap().unwrap().is_link);

        let to = dir.path().join("dest/Heat (1995)");
        let error = move_tree(&StdFs, &title, &to).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Unsupported, "{error}");
        assert!(!to.exists() && !part_path(&to).exists());

        // Recovery finishing a move removes the title's files and leaves the link and what it
        // points at alone.
        let left = finish_move(&StdFs, &title);
        assert!(!title.join("Heat (1995).mkv").exists());
        assert!(fs::symlink_metadata(&link).is_ok(), "the link stays");
        assert_eq!(
            fs::read_to_string(elsewhere.join("other.mkv")).unwrap(),
            "not this title"
        );
        assert_eq!(left.files, vec![link]);
    }
}
