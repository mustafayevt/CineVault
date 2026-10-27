//! The file-system operations the executor needs, behind a trait so a test can crash in the
//! middle, and the real implementation with Windows long-path handling.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// What exists at a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Meta {
    pub is_dir: bool,
    pub len: u64,
    /// Modification time, epoch milliseconds.
    pub modified_ms: i64,
}

/// The operations of the executor. Every path is absolute. `rename` must be atomic and fail with
/// [`io::ErrorKind::CrossesDevices`] when the paths are on different volumes.
pub trait Fs {
    /// `None` when nothing is at `path`.
    fn meta(&self, path: &Path) -> io::Result<Option<Meta>>;
    fn create_dir_all(&self, path: &Path) -> io::Result<()>;
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
    /// Copies the content and the modification time of a file. An existing `to` is replaced.
    fn copy_file(&self, from: &Path, to: &Path) -> io::Result<()>;
    /// True when both files hold the same bytes.
    fn same_content(&self, a: &Path, b: &Path) -> io::Result<bool>;
    fn remove_file(&self, path: &Path) -> io::Result<()>;
    /// Removes a folder that is empty; fails otherwise.
    fn remove_dir(&self, path: &Path) -> io::Result<()>;
    /// The names in a folder.
    fn list_dir(&self, path: &Path) -> io::Result<Vec<String>>;
}

/// Paths from this many UTF-16 units on are written in the extended-length form on Windows.
/// A folder name is limited to 248 units without it, a file path to 259.
const LONG_PATH_THRESHOLD: usize = 240;

/// The extended-length form (`\\?\C:\...`, `\\?\UNC\server\share\...`) of an absolute Windows
/// path, which lifts the 259 character limit.
///
/// The form turns off the path normalisation Windows normally does, so this function does it:
/// `/` becomes `\`, repeated separators collapse, `.` goes and `..` removes the folder before it.
/// Returns `None` when the path is not an absolute drive or UNC path, or is already in an
/// extended-length or device form.
pub fn extended_length_path(path: &str) -> Option<String> {
    if path.starts_with(r"\\?\") || path.starts_with(r"\\.\") || path.starts_with("//?/") {
        return None;
    }
    let unified = path.replace('/', "\\");
    let (prefix, rest) = if let Some(unc) = unified.strip_prefix(r"\\") {
        let mut parts = unc.splitn(3, '\\');
        let server = parts.next().filter(|s| !s.is_empty())?;
        let share = parts.next().filter(|s| !s.is_empty())?;
        (
            format!(r"\\?\UNC\{server}\{share}"),
            parts.next().unwrap_or("").to_owned(),
        )
    } else {
        let bytes = unified.as_bytes();
        let drive = bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && bytes[2] == b'\\';
        if !drive {
            return None;
        }
        (format!(r"\\?\{}:", &unified[..1]), unified[3..].to_owned())
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in rest.split('\\') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    Some(format!("{prefix}\\{}", parts.join("\\")))
}

/// The real file system.
#[derive(Debug, Clone, Copy, Default)]
pub struct StdFs;

impl StdFs {
    /// The path as the operating system should get it: extended-length on Windows when it is
    /// long, unchanged elsewhere.
    pub fn native(path: &Path) -> PathBuf {
        if cfg!(windows) {
            let text = path.to_string_lossy();
            if text.encode_utf16().count() >= LONG_PATH_THRESHOLD
                && let Some(long) = extended_length_path(&text)
            {
                return PathBuf::from(long);
            }
        }
        path.to_path_buf()
    }
}

fn millis(time: SystemTime) -> i64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_millis()).unwrap_or(i64::MAX),
        Err(e) => -i64::try_from(e.duration().as_millis()).unwrap_or(i64::MAX),
    }
}

impl Fs for StdFs {
    fn meta(&self, path: &Path) -> io::Result<Option<Meta>> {
        match fs::metadata(Self::native(path)) {
            Ok(m) => Ok(Some(Meta {
                is_dir: m.is_dir(),
                len: m.len(),
                modified_ms: m.modified().map(millis).unwrap_or(0),
            })),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            // A path through a file (`a.mkv\b`) is not there either.
            Err(e) if e.kind() == io::ErrorKind::NotADirectory => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        fs::create_dir_all(Self::native(path))
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(Self::native(from), Self::native(to))
    }

    fn copy_file(&self, from: &Path, to: &Path) -> io::Result<()> {
        let (src, dst) = (Self::native(from), Self::native(to));
        let modified = fs::metadata(&src)?.modified()?;
        fs::copy(&src, &dst)?;
        // The copy keeps the original's time, so a later undo check (size + mtime) still holds.
        // `fs::copy` also gives it the original's read-only flag, which forbids the handle the
        // time is set through: the flag is lifted for that and put back.
        let perms = fs::metadata(&dst)?.permissions();
        if perms.readonly() {
            let mut writable = perms.clone();
            #[allow(clippy::permissions_set_readonly_false)]
            writable.set_readonly(false);
            fs::set_permissions(&dst, writable)?;
        }
        let timed = File::options()
            .write(true)
            .open(&dst)
            .and_then(|file| file.set_modified(modified));
        if perms.readonly() {
            fs::set_permissions(&dst, perms)?;
        }
        timed
    }

    fn same_content(&self, a: &Path, b: &Path) -> io::Result<bool> {
        let (a, b) = (Self::native(a), Self::native(b));
        if fs::metadata(&a)?.len() != fs::metadata(&b)?.len() {
            return Ok(false);
        }
        let (mut fa, mut fb) = (File::open(a)?, File::open(b)?);
        let (mut buf_a, mut buf_b) = (vec![0u8; 256 * 1024], vec![0u8; 256 * 1024]);
        loop {
            let n = read_full(&mut fa, &mut buf_a)?;
            let m = read_full(&mut fb, &mut buf_b)?;
            if n != m || buf_a[..n] != buf_b[..m] {
                return Ok(false);
            }
            if n == 0 {
                return Ok(true);
            }
        }
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        let native = Self::native(path);
        match fs::remove_file(&native) {
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
                // A read-only file cannot be removed on Windows until the flag is cleared.
                let mut perms = fs::metadata(&native)?.permissions();
                if !perms.readonly() {
                    return Err(e);
                }
                #[allow(clippy::permissions_set_readonly_false)]
                perms.set_readonly(false);
                fs::set_permissions(&native, perms)?;
                fs::remove_file(&native)
            }
            other => other,
        }
    }

    fn remove_dir(&self, path: &Path) -> io::Result<()> {
        fs::remove_dir(Self::native(path))
    }

    fn list_dir(&self, path: &Path) -> io::Result<Vec<String>> {
        let mut names = Vec::new();
        for entry in fs::read_dir(Self::native(path))? {
            names.push(entry?.file_name().to_string_lossy().into_owned());
        }
        names.sort();
        Ok(names)
    }
}

/// How long a rename waits for CineVault's own reads inside what it moves (media information,
/// a checksum, the frames of the aspect-ratio check) to end.
pub const OWN_READ_WAIT: Duration = Duration::from_secs(20);

/// A rename did not run because CineVault itself still has a file open inside what it would
/// move (see [`cv_fs::reading`]). On Windows the rename would fail with "access denied".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadByCineVault {
    /// The file (or disc folder) that is still read.
    pub path: PathBuf,
}

impl std::fmt::Display for ReadByCineVault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CineVault is still reading {}", self.path.display())
    }
}

impl std::error::Error for ReadByCineVault {}

/// A file system whose every operation is tried again ([`cv_fs::retry`]) while it fails for a
/// moment because another program holds the file or folder (on Windows an antivirus scanner,
/// the search indexer or Explorer often keeps a short-lived handle on what was just written or
/// renamed). The executor, the undo and the recovery wrap the file system they are given in it,
/// so such a handle does not fail a rename, an undo or a resume.
///
/// A rename first waits (up to [`OWN_READ_WAIT`]) for CineVault's own reads inside the path to
/// end, and fails with a [`ReadByCineVault`] error when they do not. A folder rename is tried for
/// a few seconds ([`cv_fs::retry_folder`]): every open file below a folder holds it, and a player
/// or Explorer lets go of a folder more slowly than a scanner lets go of a file.
#[derive(Clone, Copy)]
pub struct Patient<'a> {
    inner: &'a dyn Fs,
    read_wait: Duration,
}

impl<'a> Patient<'a> {
    pub fn new(inner: &'a dyn Fs) -> Self {
        Patient {
            inner,
            read_wait: OWN_READ_WAIT,
        }
    }

    /// The same with another wait for CineVault's own reads.
    #[must_use]
    pub fn with_read_wait(mut self, wait: Duration) -> Self {
        self.read_wait = wait;
        self
    }
}

impl Fs for Patient<'_> {
    fn meta(&self, path: &Path) -> io::Result<Option<Meta>> {
        cv_fs::retry("meta", path, || self.inner.meta(path))
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        cv_fs::retry("create dir all", path, || self.inner.create_dir_all(path))
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        if let Some(path) = cv_fs::wait_until_unread(from, self.read_wait) {
            return Err(io::Error::other(ReadByCineVault { path }));
        }
        let is_dir = matches!(self.inner.meta(from), Ok(Some(meta)) if meta.is_dir);
        if is_dir {
            cv_fs::retry_folder("rename folder", from, || self.inner.rename(from, to))
        } else {
            cv_fs::retry("rename", from, || self.inner.rename(from, to))
        }
    }

    fn copy_file(&self, from: &Path, to: &Path) -> io::Result<()> {
        cv_fs::retry("copy file", from, || self.inner.copy_file(from, to))
    }

    fn same_content(&self, a: &Path, b: &Path) -> io::Result<bool> {
        cv_fs::retry("same content", a, || self.inner.same_content(a, b))
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        cv_fs::retry("remove file", path, || self.inner.remove_file(path))
    }

    fn remove_dir(&self, path: &Path) -> io::Result<()> {
        cv_fs::retry("remove dir", path, || self.inner.remove_dir(path))
    }

    fn list_dir(&self, path: &Path) -> io::Result<Vec<String>> {
        cv_fs::retry("list dir", path, || self.inner.list_dir(path))
    }
}

/// Reads until the buffer is full or the file ends.
fn read_full(file: &mut File, buf: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        let n = file.read(&mut buf[filled..])?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    Ok(filled)
}

/// True when `path` is the same as `root` or lies below it. Compared by components, ignoring
/// ASCII case (the volumes CineVault runs on do).
pub(crate) fn is_within(root: &Path, path: &Path) -> bool {
    let a = comparable(root);
    let b = comparable(path);
    b.len() >= a.len() && b[..a.len()] == a[..]
}

pub(crate) fn same_path(a: &Path, b: &Path) -> bool {
    comparable(a) == comparable(b)
}

pub(crate) fn comparable(path: &Path) -> Vec<String> {
    path.components()
        .filter(|c| !matches!(c, Component::CurDir))
        .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drive_paths_get_the_extended_prefix_and_are_normalised() {
        assert_eq!(
            extended_length_path(r"C:\Movies\Heat").as_deref(),
            Some(r"\\?\C:\Movies\Heat")
        );
        assert_eq!(
            extended_length_path("D:/Movies//A/./B/../Heat/x.mkv").as_deref(),
            Some(r"\\?\D:\Movies\A\Heat\x.mkv")
        );
    }

    #[test]
    fn unc_paths_use_the_unc_form() {
        assert_eq!(
            extended_length_path(r"\\nas\media\Movies\Heat").as_deref(),
            Some(r"\\?\UNC\nas\media\Movies\Heat")
        );
        assert_eq!(extended_length_path(r"\\nas"), None);
    }

    #[test]
    fn relative_and_already_extended_paths_are_left_alone() {
        assert_eq!(extended_length_path(r"Movies\Heat"), None);
        assert_eq!(extended_length_path("/home/user/Movies"), None);
        assert_eq!(extended_length_path(r"\\?\C:\Movies"), None);
        assert_eq!(extended_length_path(r"\\.\COM1"), None);
    }

    #[test]
    fn containment_is_by_component_and_ignores_case() {
        let root = Path::new("/d/Movies");
        assert!(is_within(root, Path::new("/d/movies")));
        assert!(is_within(root, Path::new("/d/MOVIES/Heat/a.mkv")));
        assert!(!is_within(root, Path::new("/d/Movies2/Heat")));
        assert!(!is_within(root, Path::new("/d")));
    }
}
