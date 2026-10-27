//! A journal of NFO file changes, so Rewrite NFO can be undone.
//!
//! Every NFO change goes through a [`Journal`]:
//!
//! * [`Journal::write`] puts new content at a path unless the path holds a foreign file (see
//!   [`crate::ownership`]) or the same content already (comments ignored). A file it replaces is
//!   first copied into the journal folder.
//! * [`Journal::trash`] moves a **managed** NFO into the journal's trash folder. Nothing is ever
//!   deleted for good: the file is only moved, and [`Journal::undo`] moves it back.
//! * [`Journal::undo`] reverses the entries in the opposite order. An entry is skipped, and
//!   reported, when the file changed since CineVault wrote it (the user edited it) or when the
//!   original place is taken again, so undo never overwrites work it did not do.
//!
//! The journal folder holds `backup/` (replaced files) and `trash/` (removed or created files an
//! undo took out of the way). The entries serialize, so the caller can keep them next to the
//! folder and undo after a restart.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::ownership::{NfoOwnership, classify_nfo_file};
use crate::write::content_differs;

/// One change recorded in a [`Journal`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JournalEntry {
    /// A file that did not exist was created.
    Created {
        path: PathBuf,
        /// Hash of the content written, to notice later edits.
        written: u64,
    },
    /// An existing file was replaced; `backup` holds the old content.
    Replaced {
        path: PathBuf,
        backup: PathBuf,
        written: u64,
    },
    /// A managed file was moved to `trash`.
    Trashed { path: PathBuf, trash: PathBuf },
}

/// What [`Journal::write`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteOutcome {
    /// A new file was created.
    Created,
    /// An existing file was replaced (its old content is in the journal).
    Replaced,
    /// The file already holds this content (ignoring comments); nothing was written.
    Unchanged,
    /// The path holds a foreign file; nothing was written.
    SkippedForeign,
}

/// What an undo did to one path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoStep {
    pub path: PathBuf,
    pub restored: bool,
    /// Why nothing was done, when `restored` is false.
    pub reason: Option<String>,
}

/// What [`Journal::undo`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UndoReport {
    pub steps: Vec<UndoStep>,
}

impl UndoReport {
    pub fn restored(&self) -> usize {
        self.steps.iter().filter(|s| s.restored).count()
    }

    pub fn skipped(&self) -> usize {
        self.steps.len() - self.restored()
    }
}

/// The changes of one action, with the folder that holds backups and trashed files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Journal {
    dir: PathBuf,
    entries: Vec<JournalEntry>,
}

/// FNV-1a over the bytes. Stable across runs and versions, which a stored value needs.
fn fingerprint(content: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2645;
    for byte in content.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

fn unique_name(dir: &Path, index: usize, path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map_or_else(|| "file".to_owned(), |n| n.to_string_lossy().into_owned());
    dir.join(format!("{index:04}-{name}"))
}

/// Moves a file; across volumes it copies, flushes and checks the copy, then removes the
/// original (`cv_fs::move_file`).
fn move_file(from: &Path, to: &Path) -> io::Result<()> {
    cv_fs::move_file(from, to)
}

fn read_text(path: &Path) -> io::Result<String> {
    let bytes = cv_fs::retry("reading an NFO", path, || fs::read(path))?;
    let text = String::from_utf8_lossy(&bytes);
    Ok(text.strip_prefix('\u{feff}').unwrap_or(&text).to_owned())
}

impl Journal {
    /// A new journal that keeps its files under `dir` (created when first needed).
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            entries: Vec::new(),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn entries(&self) -> &[JournalEntry] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Writes `content` to `path`, as the type docs describe.
    ///
    /// # Errors
    /// Fails when the existing file cannot be read, or the backup or the write fails. The journal
    /// then holds no entry for this path.
    pub fn write(&mut self, path: &Path, content: &str) -> io::Result<WriteOutcome> {
        let ownership = classify_nfo_file(path)?;
        if !ownership.may_overwrite() {
            return Ok(WriteOutcome::SkippedForeign);
        }
        let existed = ownership != NfoOwnership::Missing;
        let mut backup = None;
        if existed {
            if !content_differs(&read_text(path)?, content) {
                return Ok(WriteOutcome::Unchanged);
            }
            let target = unique_name(&self.dir.join("backup"), self.entries.len(), path);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            // The backup is complete and on the disk before the file is replaced.
            cv_fs::copy_synced(path, &target)?;
            backup = Some(target);
        }
        if !crate::write::write_nfo_if_changed(path, content)? {
            return Ok(WriteOutcome::Unchanged);
        }
        let written = fingerprint(content);
        match backup {
            Some(backup) => {
                self.entries.push(JournalEntry::Replaced {
                    path: path.to_owned(),
                    backup,
                    written,
                });
                Ok(WriteOutcome::Replaced)
            }
            None => {
                self.entries.push(JournalEntry::Created {
                    path: path.to_owned(),
                    written,
                });
                Ok(WriteOutcome::Created)
            }
        }
    }

    /// Moves the managed NFO at `path` into the journal's trash folder. Returns `false` and
    /// leaves the file alone when it is not a managed NFO (missing, empty, a pointer or foreign).
    ///
    /// # Errors
    /// Fails when the file cannot be read or moved.
    pub fn trash(&mut self, path: &Path) -> io::Result<bool> {
        if !classify_nfo_file(path)?.is_managed() {
            return Ok(false);
        }
        let target = unique_name(&self.dir.join("trash"), self.entries.len(), path);
        move_file(path, &target)?;
        self.entries.push(JournalEntry::Trashed {
            path: path.to_owned(),
            trash: target,
        });
        Ok(true)
    }

    /// Reverses every entry, newest first. See the module docs for what is skipped.
    pub fn undo(&self) -> UndoReport {
        let mut report = UndoReport::default();
        for (index, entry) in self.entries.iter().enumerate().rev() {
            let (path, outcome) = match entry {
                JournalEntry::Created { path, written } => {
                    (path, self.undo_created(index, path, *written))
                }
                JournalEntry::Replaced {
                    path,
                    backup,
                    written,
                } => (path, Self::undo_replaced(path, backup, *written)),
                JournalEntry::Trashed { path, trash } => (path, Self::undo_trashed(path, trash)),
            };
            report.steps.push(UndoStep {
                path: path.clone(),
                restored: outcome.is_ok(),
                reason: outcome.err(),
            });
        }
        report
    }

    fn unchanged_since(path: &Path, written: u64) -> Result<(), String> {
        match read_text(path) {
            Ok(text) if fingerprint(&text) == written => Ok(()),
            Ok(_) => Err("the file was changed after CineVault wrote it".to_owned()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                Err("the file is no longer there".to_owned())
            }
            Err(e) => Err(e.to_string()),
        }
    }

    fn undo_created(&self, index: usize, path: &Path, written: u64) -> Result<(), String> {
        Self::unchanged_since(path, written)?;
        let target = unique_name(&self.dir.join("trash"), index, path);
        move_file(path, &target).map_err(|e| e.to_string())
    }

    fn undo_replaced(path: &Path, backup: &Path, written: u64) -> Result<(), String> {
        Self::unchanged_since(path, written)?;
        let old = fs::read(backup).map_err(|e| format!("the backup is unreadable: {e}"))?;
        cv_fs::write_atomic(path, &old).map_err(|e| e.to_string())
    }

    fn undo_trashed(path: &Path, trash: &Path) -> Result<(), String> {
        if path.exists() {
            return Err("a file is there now".to_owned());
        }
        // A folder that is gone was renamed or moved with its title: the file is not put back
        // into a folder made again for it, it stays in the trash.
        if !path.parent().is_some_and(Path::is_dir) {
            return Err("its folder is no longer there".to_owned());
        }
        move_file(trash, path).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOVIE: &str = "<?xml version=\"1.0\"?>\r\n<!--created on 1-->\r\n<movie>\r\n  <title>Heat</title>\r\n</movie>\r\n";

    fn movie(title: &str) -> String {
        MOVIE.replace("Heat", title)
    }

    fn journal(dir: &Path) -> Journal {
        Journal::new(dir.join("journal"))
    }

    #[test]
    fn a_created_file_is_undone_by_moving_it_to_the_trash() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Heat.nfo");
        let mut j = journal(dir.path());
        assert_eq!(j.write(&path, MOVIE).unwrap(), WriteOutcome::Created);
        assert!(path.exists());

        let report = j.undo();
        assert_eq!((report.restored(), report.skipped()), (1, 0));
        assert!(!path.exists());
        let trashed: Vec<_> = fs::read_dir(dir.path().join("journal/trash"))
            .unwrap()
            .collect();
        assert_eq!(trashed.len(), 1, "the file is kept, not deleted");
    }

    #[test]
    fn a_trashed_file_whose_folder_is_gone_stays_in_the_trash() {
        // Review 14.7: after the title folder was renamed or moved, an undo put a trashed NFO
        // back under its old path and made the old folder again for it.
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("Heat (1995)");
        fs::create_dir_all(&folder).unwrap();
        let path = folder.join("movie.nfo");
        fs::write(&path, MOVIE).unwrap();
        let mut j = journal(dir.path());
        assert!(j.trash(&path).unwrap());
        fs::rename(&folder, dir.path().join("Heat (1995) renamed")).unwrap();

        let report = j.undo();

        assert_eq!((report.restored(), report.skipped()), (0, 1));
        assert!(!folder.exists(), "the old folder is not made again");
        let kept: Vec<_> = fs::read_dir(dir.path().join("journal/trash"))
            .unwrap()
            .collect();
        assert_eq!(kept.len(), 1, "the file stays in the trash");
    }

    #[test]
    fn a_replaced_file_comes_back_and_an_unchanged_one_is_not_journaled() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Heat.nfo");
        fs::write(&path, MOVIE).unwrap();
        let mut j = journal(dir.path());

        let same = MOVIE.replace("created on 1", "created on 2");
        assert_eq!(j.write(&path, &same).unwrap(), WriteOutcome::Unchanged);
        assert!(j.is_empty());

        let changed = movie("Heat 2");
        assert_eq!(j.write(&path, &changed).unwrap(), WriteOutcome::Replaced);
        assert_eq!(fs::read_to_string(&path).unwrap(), changed);

        assert_eq!(j.undo().restored(), 1);
        assert_eq!(fs::read_to_string(&path).unwrap(), MOVIE);
    }

    #[test]
    fn an_edit_made_after_the_write_blocks_the_undo() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Heat.nfo");
        fs::write(&path, MOVIE).unwrap();
        let mut j = journal(dir.path());
        j.write(&path, &movie("Heat 2")).unwrap();
        fs::write(&path, movie("Edited by hand")).unwrap();

        let report = j.undo();
        assert_eq!((report.restored(), report.skipped()), (0, 1));
        assert!(
            report.steps[0]
                .reason
                .as_deref()
                .unwrap()
                .contains("changed")
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), movie("Edited by hand"));
    }

    #[test]
    fn a_foreign_file_is_never_written_over() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("movie.nfo");
        let note = "Release.Group.Presents\r\nMovie.Name.2020-GROUP\r\n";
        fs::write(&path, note).unwrap();
        let mut j = journal(dir.path());
        assert_eq!(j.write(&path, MOVIE).unwrap(), WriteOutcome::SkippedForeign);
        assert_eq!(fs::read_to_string(&path).unwrap(), note);
        assert!(j.is_empty());
    }

    #[test]
    fn a_pointer_file_is_replaced_and_comes_back_on_undo() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Heat.nfo");
        let url = "https://www.themoviedb.org/movie/949";
        fs::write(&path, url).unwrap();
        let mut j = journal(dir.path());
        assert_eq!(j.write(&path, MOVIE).unwrap(), WriteOutcome::Replaced);
        assert_eq!(j.undo().restored(), 1);
        assert_eq!(fs::read_to_string(&path).unwrap(), url);
    }

    #[test]
    fn only_managed_files_are_trashed_and_they_can_come_back() {
        let dir = tempfile::tempdir().unwrap();
        let managed = dir.path().join("old.nfo");
        let foreign = dir.path().join("info.nfo");
        fs::write(&managed, MOVIE).unwrap();
        fs::write(&foreign, "Release.Group.Presents").unwrap();
        let mut j = journal(dir.path());

        assert!(!j.trash(&foreign).unwrap());
        assert!(!j.trash(&dir.path().join("missing.nfo")).unwrap());
        assert!(foreign.exists());

        assert!(j.trash(&managed).unwrap());
        assert!(!managed.exists());
        assert_eq!(j.undo().restored(), 1);
        assert_eq!(fs::read_to_string(&managed).unwrap(), MOVIE);
    }

    #[test]
    fn undo_does_not_overwrite_a_file_that_took_the_place() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.nfo");
        fs::write(&path, MOVIE).unwrap();
        let mut j = journal(dir.path());
        j.trash(&path).unwrap();
        fs::write(&path, "something new").unwrap();
        let report = j.undo();
        assert_eq!(report.skipped(), 1);
        assert_eq!(fs::read_to_string(&path).unwrap(), "something new");
    }

    #[test]
    fn a_journal_survives_serialisation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Heat.nfo");
        let mut j = journal(dir.path());
        j.write(&path, MOVIE).unwrap();
        let json = serde_json::to_string(&j).unwrap();
        let again: Journal = serde_json::from_str(&json).unwrap();
        assert_eq!(again, j);
        assert_eq!(again.undo().restored(), 1);
    }
}
