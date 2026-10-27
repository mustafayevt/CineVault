//! Moving a whole folder across volumes: copy the tree beside the destination, verify
//! every file, rename it into place, and only then take the original files away.
//!
//! The same rules as a single file (see [`super::run`]): a crash at any point leaves the original
//! whole until the copy is proven, and recovery can finish the last step. The copy is written to
//! `<name>.cvpart`, a folder that never counts as the destination.

use std::io;
use std::path::{Path, PathBuf};

use super::fs::Fs;
use super::run::part_path;

/// What a folder move left behind at the source.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Remnants {
    /// Files of the original that could not be removed after the copy was verified.
    pub files: Vec<PathBuf>,
}

/// Moves `from` to `to` when a plain rename is not possible (another volume). `to` must not
/// exist. The destination appears all at once; the original is removed only after every file of
/// the copy matched byte for byte.
pub(crate) fn move_tree(fs: &dyn Fs, from: &Path, to: &Path) -> io::Result<Remnants> {
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
    if let Err(e) = fs.rename(&part, to) {
        let _ = remove_tree(fs, &part);
        return Err(e);
    }
    Ok(remove_original(fs, from))
}

/// Copies the folder `from` to `to`, folders included, and checks each file against its copy.
fn copy_tree(fs: &dyn Fs, from: &Path, to: &Path) -> io::Result<()> {
    fs.create_dir_all(to)?;
    for name in fs.list_dir(from)? {
        let (src, dst) = (from.join(&name), to.join(&name));
        let Some(meta) = fs.meta(&src)? else {
            continue;
        };
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
    Ok(())
}

/// Removes the files of a verified original and the folders they leave empty. What cannot be
/// removed stays and is reported.
fn remove_original(fs: &dyn Fs, root: &Path) -> Remnants {
    let mut left = Remnants::default();
    remove_original_in(fs, root, &mut left);
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

/// Removes a folder CineVault created itself (a `.cvpart` copy) with everything in it.
pub(crate) fn remove_tree(fs: &dyn Fs, path: &Path) -> io::Result<()> {
    match fs.meta(path)? {
        None => Ok(()),
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
