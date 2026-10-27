//! What exists on disk, as detection asks for it.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use super::normalise_root;

/// Something that exists at a path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Occupant {
    pub is_dir: bool,
    /// For a folder: it holds nothing.
    pub empty: bool,
    /// The library item the file or folder belongs to, when it belongs to one.
    pub owner: Option<String>,
}

/// The file system as conflict detection sees it. Paths are relative to the root of a data
/// source and use `/`. A real implementation looks the path up on disk (with the volume's own
/// case rules) and the library for the owner.
pub trait Disk {
    /// What is at `path` below `root`, if anything.
    fn occupant(&self, root: &str, path: &str) -> Option<Occupant>;
}

/// An in-memory [`Disk`] for tests and previews.
#[derive(Debug, Clone, Default)]
pub struct MemoryDisk {
    case_insensitive: bool,
    entries: HashMap<String, Occupant>,
}

impl MemoryDisk {
    pub fn new(case_insensitive: bool) -> Self {
        MemoryDisk {
            case_insensitive,
            entries: HashMap::new(),
        }
    }

    fn key(&self, root: &str, path: &str) -> String {
        let key = format!("{}\0{path}", normalise_root(root));
        if self.case_insensitive {
            key.to_lowercase()
        } else {
            key
        }
    }

    /// Adds a file. `owner` is the id of the library item it belongs to.
    #[must_use]
    pub fn with_file(mut self, root: &str, path: &str, owner: Option<&str>) -> Self {
        let key = self.key(root, path);
        self.entries.insert(
            key,
            Occupant {
                is_dir: false,
                empty: false,
                owner: owner.map(str::to_owned),
            },
        );
        self
    }

    /// Adds a folder.
    #[must_use]
    pub fn with_dir(mut self, root: &str, path: &str, empty: bool, owner: Option<&str>) -> Self {
        let key = self.key(root, path);
        self.entries.insert(
            key,
            Occupant {
                is_dir: true,
                empty,
                owner: owner.map(str::to_owned),
            },
        );
        self
    }
}

impl Disk for MemoryDisk {
    fn occupant(&self, root: &str, path: &str) -> Option<Occupant> {
        self.entries.get(&self.key(root, path)).cloned()
    }
}

/// Looks up the library item that owns a path: `(root, path)` to its id.
pub type OwnerLookup<'a> = &'a dyn Fn(&str, &str) -> Option<String>;

/// The real file system as conflict detection sees it. The library owner of a path comes from the
/// [`OwnerLookup`], which the caller backs with the library.
pub struct FsDisk<'a> {
    owner: Option<OwnerLookup<'a>>,
}

impl<'a> FsDisk<'a> {
    pub fn new() -> Self {
        FsDisk { owner: None }
    }

    /// Looks the owner of an existing path up in the library.
    #[must_use]
    pub fn with_owner(mut self, owner: OwnerLookup<'a>) -> Self {
        self.owner = Some(owner);
        self
    }
}

impl Default for FsDisk<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl Disk for FsDisk<'_> {
    fn occupant(&self, root: &str, path: &str) -> Option<Occupant> {
        let full = Path::new(root).join(path);
        let meta = fs::metadata(&full).ok()?;
        let empty = meta.is_dir()
            && fs::read_dir(&full)
                .map(|mut entries| entries.next().is_none())
                .unwrap_or(false);
        Some(Occupant {
            is_dir: meta.is_dir(),
            empty,
            owner: self.owner.and_then(|lookup| lookup(root, path)),
        })
    }
}
