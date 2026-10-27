//! Data-source guards: path normalisation, the nested-folder rule and the system-folder rule.
//!
//! A data source is a folder the library is built from. Before one is added or remapped, its path
//! is normalised ([`PathStyle::normalize`]) and checked by [`SourceGuard::check`], which refuses:
//!
//! - a folder that is not a full path, or that has characters the operating system cannot use;
//! - an operating-system or system-critical folder (the system drive, `C:\Windows`, program and
//!   profile folders, `/System`, `/usr`, a home folder itself, the app's own data folder, trash
//!   and recycle folders ...);
//! - a folder that is already a data source, that lies inside one of the same library, or that
//!   contains one (which would make the existing source nested inside the new one).
//!
//! Everything here works on text, so both path syntaxes are covered by the same tests on any
//! operating system. Only [`SourceGuard::check_with_links`] touches the file system: it also
//! follows symbolic links, junctions and short names, so a link cannot smuggle a system folder in.
//! Comparison ignores case, because Windows and macOS volumes normally do; refusing two folders
//! that differ only by case is the safe side.

use std::path::Path;

/// Which operating system's path syntax is used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathStyle {
    /// Drive-letter (`D:\Movies`) and UNC (`\\server\share`) paths.
    Windows,
    /// Paths rooted at `/` (macOS).
    Posix,
}

/// Why a folder cannot be a data source.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SourceError {
    #[error("Choose a folder first.")]
    Empty,
    #[error(
        "\"{0}\" is not a full folder path. Use a path such as D:\\Movies, \\\\server\\share or /Volumes/Movies."
    )]
    NotAbsolute(String),
    #[error("\"{0}\" contains characters that a folder name cannot have.")]
    InvalidCharacters(String),
    #[error("{path} is {what}, so it cannot be a library folder.")]
    SystemFolder { path: String, what: String },
    /// The folder lies below a protected folder; `folder` is that folder, so the user can see
    /// which one it is.
    #[error("{path} is inside {what} ({folder}), so it cannot be a library folder.")]
    InsideSystemFolder {
        path: String,
        what: String,
        folder: String,
    },
    #[error("{path} is already a library folder.")]
    AlreadyAdded { path: String },
    #[error("{path} is already covered by the library folder {existing}.")]
    InsideExisting { path: String, existing: String },
    #[error(
        "{path} contains the library folder {existing}. Remove that folder first, or choose a folder outside it."
    )]
    ContainsExisting { path: String, existing: String },
    #[error("{path} is already the path of this library folder.")]
    Unchanged { path: String },
    #[error("That library folder no longer exists.")]
    UnknownSource,
    #[error("The folder {path} does not exist or cannot be reached.")]
    NotReachable { path: String },
    #[error("The order must list every {kind} library folder exactly once.")]
    BadOrder { kind: String },
}

impl SourceError {
    /// A stable code for the UI to branch on.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::NotAbsolute(_) => "not_absolute",
            Self::InvalidCharacters(_) => "invalid_characters",
            Self::SystemFolder { .. } | Self::InsideSystemFolder { .. } => "system_folder",
            Self::AlreadyAdded { .. } => "already_added",
            Self::InsideExisting { .. } => "inside_existing",
            Self::ContainsExisting { .. } => "contains_existing",
            Self::Unchanged { .. } => "unchanged",
            Self::UnknownSource => "unknown_source",
            Self::NotReachable { .. } => "not_reachable",
            Self::BadOrder { .. } => "bad_order",
        }
    }
}

/// A path split into its root and folder names.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Parsed {
    /// `C:\`, `\\server\share` or `/`.
    root: String,
    comps: Vec<String>,
}

impl Parsed {
    fn folded(&self) -> Self {
        Self {
            root: self.root.to_lowercase(),
            comps: self.comps.iter().map(|c| c.to_lowercase()).collect(),
        }
    }

    fn join(&self, style: PathStyle) -> String {
        let sep = style.separator();
        let mut out = self.root.clone();
        for comp in &self.comps {
            if !out.ends_with(sep) {
                out.push(sep);
            }
            out.push_str(comp);
        }
        out
    }

    /// True when `self` is `other` or lies inside it (both folded).
    fn is_within(&self, other: &Self) -> bool {
        self.root == other.root && self.comps.starts_with(&other.comps)
    }
}

impl PathStyle {
    /// The style of the operating system this build runs on.
    pub const fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else {
            Self::Posix
        }
    }

    const fn separator(self) -> char {
        match self {
            Self::Windows => '\\',
            Self::Posix => '/',
        }
    }

    /// Cleans `raw` into its canonical text form: separators of this style, no trailing or
    /// repeated separators, `.` and `..` resolved, an upper-case drive letter, and (on Windows)
    /// no trailing dots or spaces in folder names, which Windows ignores.
    ///
    /// # Errors
    /// Fails for an empty or relative path, and for characters the system cannot store.
    pub fn normalize(self, raw: &str) -> Result<String, SourceError> {
        Ok(self.parse(raw)?.join(self))
    }

    fn parse(self, raw: &str) -> Result<Parsed, SourceError> {
        let text = raw.trim();
        if text.is_empty() {
            return Err(SourceError::Empty);
        }
        let not_absolute = || SourceError::NotAbsolute(text.to_owned());
        let invalid = || SourceError::InvalidCharacters(text.to_owned());
        if text.chars().any(char::is_control) {
            return Err(invalid());
        }
        match self {
            Self::Posix => {
                if !text.starts_with('/') {
                    return Err(not_absolute());
                }
                Ok(Parsed {
                    root: "/".to_owned(),
                    comps: clean_components(text.split('/'), false),
                })
            }
            Self::Windows => {
                let mut text = text.replace('/', "\\");
                if let Some(rest) = text.strip_prefix("\\\\?\\UNC\\") {
                    text = format!("\\\\{rest}");
                } else if let Some(rest) = text.strip_prefix("\\\\?\\") {
                    text = rest.to_owned();
                }
                if text.contains(['<', '>', '"', '|', '?', '*']) {
                    return Err(invalid());
                }
                let has_drive = {
                    let b = text.as_bytes();
                    b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\'
                };
                if has_drive {
                    let root = format!("{}:\\", text[..1].to_ascii_uppercase());
                    let comps = clean_components(text[3..].split('\\'), true);
                    // A colon in a folder name is an NTFS alternate data stream.
                    if comps.iter().any(|c| c.contains(':')) {
                        return Err(invalid());
                    }
                    Ok(Parsed { root, comps })
                } else if let Some(rest) = text.strip_prefix("\\\\") {
                    let mut comps = clean_components(rest.split('\\'), true);
                    if comps.iter().any(|c| c.contains(':')) {
                        return Err(invalid());
                    }
                    if comps.len() < 2 {
                        // `\\server` alone is not a folder.
                        return Err(not_absolute());
                    }
                    let share = comps.remove(1);
                    let server = comps.remove(0);
                    Ok(Parsed {
                        root: format!("\\\\{server}\\{share}"),
                        comps,
                    })
                } else {
                    Err(not_absolute())
                }
            }
        }
    }
}

/// Splits into folder names: drops empty and `.` parts, applies `..`, and on Windows drops
/// trailing dots and spaces.
fn clean_components<'a>(parts: impl Iterator<Item = &'a str>, windows: bool) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for part in parts {
        match part {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            _ => {
                let name = if windows {
                    part.trim_end_matches(['.', ' '])
                } else {
                    part
                };
                if !name.is_empty() {
                    out.push(name.to_owned());
                }
            }
        }
    }
    out
}

/// The folders no library may be built on.
#[derive(Debug, Clone)]
pub struct SystemFolders {
    style: PathStyle,
    /// Folders refused themselves, but whose subfolders are fine (a drive root, `/Users`).
    exact: Vec<(Parsed, String)>,
    /// Folders refused together with everything below them, with their spelling for messages.
    tree: Vec<(Parsed, String, String)>,
    /// Folders whose direct children are refused (the home folders in `/Users`).
    children_of: Vec<(Parsed, String)>,
    /// Folder names refused wherever they appear (recycle bins, volume metadata), folded.
    names: Vec<(String, String)>,
}

/// Why a path is a system folder: what the protected folder is, and the protected folder itself
/// when the path only lies inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SystemHit {
    what: String,
    inside: Option<String>,
}

impl SystemHit {
    fn into_error(self, path: String) -> SourceError {
        match self.inside {
            Some(folder) => SourceError::InsideSystemFolder {
                path,
                what: self.what,
                folder,
            },
            None => SourceError::SystemFolder {
                path,
                what: self.what,
            },
        }
    }
}

impl SystemFolders {
    /// The built-in list for `style`, assuming the system drive is `C:` on Windows.
    pub fn new(style: PathStyle) -> Self {
        let mut folders = Self {
            style,
            exact: Vec::new(),
            tree: Vec::new(),
            children_of: Vec::new(),
            names: Vec::new(),
        };
        match style {
            PathStyle::Windows => folders.add_windows_defaults("C:"),
            PathStyle::Posix => folders.add_posix_defaults(),
        }
        folders
    }

    /// The built-in list for this operating system, plus the current user's home and
    /// application-data folders, and `extra` folders (the app's own data folder).
    pub fn from_environment(extra: &[String]) -> Self {
        let style = PathStyle::current();
        let mut folders = Self::new(style);
        match style {
            PathStyle::Windows => {
                let var = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
                if let Some(drive) = var("SystemDrive").filter(|d| d.len() == 2) {
                    folders.add_windows_defaults(&drive);
                }
                for (name, what) in [
                    ("SystemRoot", "the Windows folder"),
                    ("ProgramFiles", "the Program Files folder"),
                    ("ProgramFiles(x86)", "the Program Files folder"),
                    ("ProgramData", "the ProgramData folder"),
                    ("APPDATA", "your application data folder"),
                    ("LOCALAPPDATA", "your application data folder"),
                ] {
                    if let Some(path) = var(name) {
                        folders.protect_tree(&path, what);
                    }
                }
                if let Some(home) = var("USERPROFILE") {
                    folders.protect_exact(&home, "your user profile folder");
                }
            }
            PathStyle::Posix => {
                if let Some(home) = std::env::var("HOME").ok().filter(|h| !h.is_empty()) {
                    folders.protect_exact(&home, "your home folder");
                    folders.protect_tree(&format!("{home}/Library"), "your Library folder");
                }
            }
        }
        for path in extra {
            folders.protect_tree(path, "CineVault's own data folder");
        }
        folders
    }

    /// Refuses `path` itself (not its subfolders). An unusable path is ignored.
    pub fn protect_exact(&mut self, path: &str, what: &str) -> &mut Self {
        if let Ok(parsed) = self.style.parse(path) {
            self.exact.push((parsed.folded(), what.to_owned()));
        }
        self
    }

    /// Refuses `path` and everything below it. An unusable path is ignored.
    pub fn protect_tree(&mut self, path: &str, what: &str) -> &mut Self {
        if let Ok(parsed) = self.style.parse(path) {
            let shown = parsed.join(self.style);
            self.tree.push((parsed.folded(), what.to_owned(), shown));
        }
        self
    }

    fn protect_children(&mut self, path: &str, what: &str) {
        if let Ok(parsed) = self.style.parse(path) {
            self.children_of.push((parsed.folded(), what.to_owned()));
        }
    }

    fn add_windows_defaults(&mut self, drive: &str) {
        let root = format!("{drive}\\");
        self.protect_exact(&root, "the system drive");
        self.protect_exact(&format!("{root}Users"), "the Users folder");
        self.protect_exact(&format!("{root}Documents and Settings"), "a system folder");
        self.protect_children(&format!("{root}Users"), "a user profile folder");
        for (dir, what) in [
            ("Windows", "the Windows folder"),
            ("Program Files", "the Program Files folder"),
            ("Program Files (x86)", "the Program Files folder"),
            ("ProgramData", "the ProgramData folder"),
            ("Recovery", "a system folder"),
            ("Boot", "a system folder"),
            ("Config.Msi", "a system folder"),
        ] {
            self.protect_tree(&format!("{root}{dir}"), what);
        }
        for name in ["$recycle.bin", "system volume information", "$winreagent"] {
            self.names
                .push((name.to_owned(), "a Windows system folder".to_owned()));
        }
    }

    fn add_posix_defaults(&mut self) {
        self.protect_exact("/", "the root of the disk");
        for (dir, what) in [
            ("/Users", "the Users folder"),
            ("/home", "the home folders"),
            ("/Volumes", "the list of mounted volumes"),
        ] {
            self.protect_exact(dir, what);
        }
        self.protect_children("/Users", "a user's home folder");
        self.protect_children("/home", "a user's home folder");
        for dir in [
            "/System",
            "/Library",
            "/Applications",
            "/bin",
            "/sbin",
            "/usr",
            "/etc",
            "/var",
            "/private",
            "/dev",
            "/proc",
            "/sys",
            "/boot",
            "/cores",
            "/Network",
            "/root",
            "/lost+found",
        ] {
            self.protect_tree(dir, "an operating-system folder");
        }
        for name in [
            ".trashes",
            ".spotlight-v100",
            ".fseventsd",
            ".documentrevisions-v100",
            ".temporaryitems",
            ".mobilebackups",
        ] {
            self.names
                .push((name.to_owned(), "a macOS system folder".to_owned()));
        }
    }

    /// What `path` is, if it is a system folder.
    fn describe(&self, path: &Parsed) -> Option<SystemHit> {
        let folded = path.folded();
        let hit = |what: &str, inside: Option<String>| SystemHit {
            what: what.to_owned(),
            inside,
        };
        if let Some((_, what)) = self.exact.iter().find(|(p, _)| *p == folded) {
            return Some(hit(what, None));
        }
        if let Some((p, what, shown)) = self.tree.iter().find(|(p, ..)| folded.is_within(p)) {
            let inside = (folded.comps.len() > p.comps.len()).then(|| shown.clone());
            return Some(hit(what, inside));
        }
        if let Some((_, what)) = self.children_of.iter().find(|(p, _)| {
            folded.root == p.root
                && folded.comps.len() == p.comps.len() + 1
                && folded.comps.starts_with(&p.comps)
        }) {
            return Some(hit(what, None));
        }
        self.names.iter().find_map(|(name, what)| {
            let at = folded.comps.iter().position(|c| c == name)?;
            let inside = (at + 1 < folded.comps.len()).then(|| {
                Parsed {
                    root: path.root.clone(),
                    comps: path.comps[..=at].to_vec(),
                }
                .join(self.style)
            });
            Some(hit(what, inside))
        })
    }
}

/// A data source that is already stored, as the guard sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistingSource {
    pub id: i64,
    /// Any spelling of its path; it is normalised for the comparison.
    pub path: String,
}

/// The rules every data-source path must pass.
#[derive(Debug, Clone)]
pub struct SourceGuard {
    style: PathStyle,
    system: SystemFolders,
}

impl SourceGuard {
    pub fn new(style: PathStyle, system: SystemFolders) -> Self {
        Self { style, system }
    }

    /// The guard for this operating system. `extra_protected` lists folders to refuse in
    /// addition to the system ones, such as the app's data folder.
    pub fn for_current_os(extra_protected: &[String]) -> Self {
        Self::new(
            PathStyle::current(),
            SystemFolders::from_environment(extra_protected),
        )
    }

    /// The path syntax this guard checks.
    pub const fn style(&self) -> PathStyle {
        self.style
    }

    /// Checks `raw` against the rules and returns its normalised form.
    ///
    /// `existing` are the data sources of the same library (movies or TV). `ignore_id` leaves
    /// one of them out, for remapping a source onto a path near its own.
    ///
    /// # Errors
    /// The first rule that fails, in the order: not a path, system folder, already added, inside
    /// an existing source, containing an existing source.
    pub fn check(
        &self,
        raw: &str,
        existing: &[ExistingSource],
        ignore_id: Option<i64>,
    ) -> Result<String, SourceError> {
        let parsed = self.style.parse(raw)?;
        let path = parsed.join(self.style);
        if let Some(hit) = self.system.describe(&parsed) {
            return Err(hit.into_error(path));
        }
        let folded = parsed.folded();
        for other in existing.iter().filter(|e| Some(e.id) != ignore_id) {
            // A stored path that no longer parses cannot be compared; it never blocks.
            let Ok(other_parsed) = self.style.parse(&other.path) else {
                continue;
            };
            let other_path = other_parsed.join(self.style);
            let other_folded = other_parsed.folded();
            if folded == other_folded {
                return Err(SourceError::AlreadyAdded { path });
            }
            if folded.is_within(&other_folded) {
                return Err(SourceError::InsideExisting {
                    path,
                    existing: other_path,
                });
            }
            if other_folded.is_within(&folded) {
                return Err(SourceError::ContainsExisting {
                    path,
                    existing: other_path,
                });
            }
        }
        Ok(path)
    }

    /// [`Self::check`], and also refuses a folder that only reaches a system folder through a
    /// symbolic link, junction, mapped drive or short (8.3) name. Only meaningful when this
    /// guard's style is the style of the running system; folders that do not exist are checked
    /// by their text only.
    ///
    /// # Errors
    /// As [`Self::check`].
    pub fn check_with_links(
        &self,
        raw: &str,
        existing: &[ExistingSource],
        ignore_id: Option<i64>,
    ) -> Result<String, SourceError> {
        let path = self.check(raw, existing, ignore_id)?;
        if self.style == PathStyle::current()
            && let Some(real) = resolve_links(&path)
            && let Ok(parsed) = self.style.parse(&real)
            && let Some(hit) = self.system.describe(&parsed)
        {
            return Err(hit.into_error(path));
        }
        Ok(path)
    }
}

/// The real location of `path` with links resolved, or `None` when it does not exist.
fn resolve_links(path: &str) -> Option<String> {
    let real = std::fs::canonicalize(Path::new(path)).ok()?;
    Some(real.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests;
