//! A read-only view of a disc's files: a folder on disk or an ISO image, behind one trait.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::mediainfo::ByteSource;

/// One entry of a directory listing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscEntry {
    pub name: String,
    pub is_dir: bool,
    /// Size in bytes (0 for a directory).
    pub size: u64,
}

/// The files of a disc, addressed by `/`-separated paths from its root. Names match
/// case-insensitively (disc images store upper-case names, folders copied to disk often do
/// not).
pub trait DiscFs {
    /// The entries of the directory at `dir` (`""` is the root), in no particular order.
    ///
    /// # Errors
    /// The directory does not exist, or the storage fails.
    fn list(&self, dir: &str) -> io::Result<Vec<DiscEntry>>;

    /// The size of the file at `path`.
    ///
    /// # Errors
    /// The file does not exist, or the storage fails.
    fn size(&self, path: &str) -> io::Result<u64>;

    /// Reads from the file at `path` starting at `offset`; 0 bytes means the end.
    ///
    /// # Errors
    /// The file does not exist, or the storage fails.
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> io::Result<usize>;
}

/// Reads the whole file at `path`, up to `limit` bytes. The small structure files of a disc
/// (`.IFO`, `.mpls`) go through this.
///
/// # Errors
/// As [`DiscFs::read_at`]; a file over the limit is `InvalidData`.
pub fn read_file(fs: &dyn DiscFs, path: &str, limit: u64) -> io::Result<Vec<u8>> {
    let size = fs.size(path)?;
    if size > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{path} is larger than the {limit} bytes expected"),
        ));
    }
    let mut data = vec![0_u8; usize::try_from(size).unwrap_or(0)];
    let mut done = 0;
    while done < data.len() {
        let read = fs.read_at(path, done as u64, &mut data[done..])?;
        if read == 0 {
            data.truncate(done);
            break;
        }
        done += read;
    }
    Ok(data)
}

/// The entry called `name` (any case) in `dir`.
///
/// # Errors
/// As [`DiscFs::list`].
pub fn find(fs: &dyn DiscFs, dir: &str, name: &str) -> io::Result<Option<DiscEntry>> {
    Ok(fs
        .list(dir)?
        .into_iter()
        .find(|entry| entry.name.eq_ignore_ascii_case(name)))
}

/// Joins a directory and a name into a disc path.
#[must_use]
pub fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_owned()
    } else {
        format!("{dir}/{name}")
    }
}

/// A [`ByteSource`] over one file of a [`DiscFs`].
pub struct FsFile<'a> {
    fs: &'a dyn DiscFs,
    path: String,
    len: u64,
}

impl<'a> FsFile<'a> {
    /// # Errors
    /// The file does not exist, or the storage fails.
    pub fn open(fs: &'a dyn DiscFs, path: &str) -> io::Result<Self> {
        Ok(Self {
            fs,
            path: path.to_owned(),
            len: fs.size(path)?,
        })
    }
}

impl ByteSource for FsFile<'_> {
    fn len(&self) -> u64 {
        self.len
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        self.fs.read_at(&self.path, offset, buf)
    }
}

/// A folder on disk. Its root is the folder that holds `VIDEO_TS`, `BDMV` or `HVDVD_TS`.
#[derive(Clone, Debug)]
pub struct FolderFs {
    root: PathBuf,
}

impl FolderFs {
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The real path of a disc path, matching each name without regard to case.
    fn resolve(&self, path: &str) -> io::Result<PathBuf> {
        let mut real = self.root.clone();
        for part in path.split('/').filter(|part| !part.is_empty()) {
            let direct = real.join(part);
            if direct.exists() {
                real = direct;
                continue;
            }
            let found = std::fs::read_dir(&real)?
                .filter_map(Result::ok)
                .find(|entry| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .eq_ignore_ascii_case(part)
                });
            match found {
                Some(entry) => real = entry.path(),
                None => {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        format!("{path} is not on the disc"),
                    ));
                }
            }
        }
        Ok(real)
    }
}

impl DiscFs for FolderFs {
    fn list(&self, dir: &str) -> io::Result<Vec<DiscEntry>> {
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(self.resolve(dir)?)? {
            let entry = entry?;
            let meta = entry.metadata()?;
            entries.push(DiscEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                is_dir: meta.is_dir(),
                size: if meta.is_dir() { 0 } else { meta.len() },
            });
        }
        Ok(entries)
    }

    fn size(&self, path: &str) -> io::Result<u64> {
        Ok(std::fs::metadata(self.resolve(path)?)?.len())
    }

    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        let mut file = File::open(self.resolve(path)?)?;
        file.seek(SeekFrom::Start(offset))?;
        file.read(buf)
    }
}

/// Whether `path` names a disc image the app can open by extension.
#[must_use]
pub fn is_image_path(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("iso"))
}
